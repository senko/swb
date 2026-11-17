//! Computes styles for the real target pages in `fixtures/pages/` and
//! checks a few computed values against Chromium's.
//!
//! The stylesheets are loaded the way the engine loads them: `<link
//! rel=stylesheet>` (from the fixture manifest) and `<style>` elements, in
//! document order.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use swb_css::MediaEnvironment;
use swb_dom::{Document, NodeId};
use swb_style::{
    ComputedStyle, ContainerSizes, Display, DomChanges, ElementStates, FontFamily, GenericFamily,
    LengthPercentage, Restyle, Rgba, StyleMap, Stylist, compute_styles, compute_styles_reusing,
    compute_styles_with_containers, restyle_states, restyle_targeted,
};
use url::Url;

fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pages")
        .join(name)
}

/// Reads the URL → body file pairs from a fixture manifest. The manifest
/// is JSON with one `"url"` and one `"body"` line per entry.
fn manifest(dir: &Path) -> Vec<(String, PathBuf)> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).expect("manifest exists");
    let value = |line: &str| -> Option<String> {
        let start = line.find(": \"")? + 3;
        let end = line.rfind('"')?;
        Some(line.get(start..end)?.replace("\\u0026", "&"))
    };
    let mut entries = Vec::new();
    let mut url = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("\"url\"") {
            url = value(line);
        } else if line.starts_with("\"body\"")
            && let (Some(u), Some(body)) = (url.take(), value(line))
        {
            entries.push((u, dir.join(body)));
        }
    }
    entries
}

struct Page {
    doc: Document,
    stylist: Stylist,
    url: Url,
}

fn load(name: &str) -> Page {
    let dir = fixture_dir(name);
    let entries = manifest(&dir);
    let (page_url, html_path) = entries
        .iter()
        .find(|(_, path)| path.extension().is_some_and(|e| e == "html"))
        .map(|(u, p)| (u.clone(), p.clone()))
        .expect("an HTML entry");
    // The page is the entry whose URL is in fixture.json.
    let fixture = std::fs::read_to_string(dir.join("fixture.json")).expect("fixture.json");
    let page_url = fixture
        .lines()
        .find(|l| l.trim().starts_with("\"url\""))
        .and_then(|l| l.split('"').nth(3))
        .map_or(page_url, str::to_owned);
    let html_path = entries
        .iter()
        .find(|(u, _)| *u == page_url)
        .map_or(html_path, |(_, p)| p.clone());
    let bytes = std::fs::read(&html_path).expect("HTML file");
    let (doc, _) = swb_dom::parse_html_bytes(&bytes, Some("utf-8"));
    let url = Url::parse(&page_url).expect("page URL");
    let mut stylist = Stylist::new(doc.quirks_mode);
    for node in doc.descendants(NodeId::DOCUMENT) {
        let Some(e) = doc.element(node) else {
            continue;
        };
        match &**e.local_name() {
            "link" => {
                let is_sheet = e.attr("rel").is_some_and(|r| {
                    r.split_ascii_whitespace()
                        .any(|t| t.eq_ignore_ascii_case("stylesheet"))
                });
                let Some(href) = e.attr("href").filter(|_| is_sheet) else {
                    continue;
                };
                let Ok(sheet_url) = url.join(href) else {
                    continue;
                };
                let Some((_, path)) = entries.iter().find(|(u, _)| *u == sheet_url.as_str()) else {
                    panic!("stylesheet {sheet_url} is not in the fixture");
                };
                let css = std::fs::read_to_string(path).expect("stylesheet file");
                stylist.add_author_sheet(&swb_css::parse_stylesheet(&css), &sheet_url);
            }
            "style" => {
                let css = doc.text_content(node);
                stylist.add_author_sheet(&swb_css::parse_stylesheet(&css), &url);
            }
            _ => {}
        }
    }
    Page { doc, stylist, url }
}

fn environment() -> MediaEnvironment {
    MediaEnvironment {
        viewport_width: 1280.0,
        viewport_height: 800.0,
        ..MediaEnvironment::default()
    }
}

fn styles(page: &Page) -> StyleMap {
    compute_styles(
        &page.doc,
        &page.stylist,
        &environment(),
        &ElementStates::default(),
        &page.url,
    )
}

fn style_of(styles: &StyleMap, node: NodeId) -> &ComputedStyle {
    styles.get(node).expect("element has a style")
}

fn first(page: &Page, pred: impl Fn(&swb_dom::ElementData) -> bool) -> NodeId {
    page.doc
        .find_element(NodeId::DOCUMENT, pred)
        .expect("element exists")
}

#[test]
fn hacker_news() {
    let page = load("hacker-news");
    assert_eq!(page.doc.quirks_mode, swb_dom::QuirksMode::Quirks);
    let styles = styles(&page);

    // The orange header cell: `<td bgcolor="#ff6600">`.
    let header = first(&page, |e| e.attr("bgcolor") == Some("#ff6600"));
    let s = style_of(&styles, header);
    assert_eq!(
        s.background_color.resolve(s.color),
        Rgba::rgb(0xff, 0x66, 0x00)
    );
    assert_eq!(s.display, Display::TableCell);

    // The main table: width="85%", bgcolor, cellspacing="0".
    let main = first(&page, |e| e.id() == Some("hnmain"));
    let s = style_of(&styles, main);
    assert_eq!(
        s.width,
        swb_style::Size::LengthPercentage(LengthPercentage::Percent(0.85))
    );
    assert_eq!(
        s.background_color.resolve(s.color),
        Rgba::rgb(0xf6, 0xf6, 0xef)
    );
    assert_eq!(s.border_spacing_horizontal, 0.0);
    assert_eq!(
        s.min_width,
        swb_style::Size::LengthPercentage(LengthPercentage::Px(796.0))
    );
    // Quirks mode: tables do not inherit the font size.
    assert_eq!(s.font_size, 16.0);

    // `td { font-family: Verdana, Geneva, sans-serif; font-size: 10pt;
    // color: #828282 }`, cellpadding="0".
    let cell = first(&page, |e| e.has_class("title"));
    let s = style_of(&styles, cell);
    assert!((s.font_size - 13.333_333).abs() < 1e-3);
    assert_eq!(s.color, Rgba::rgb(0x82, 0x82, 0x82));
    assert_eq!(s.font_family[0], FontFamily::Named("Verdana".into()));
    assert_eq!(s.padding_top, LengthPercentage::Px(0.0));
    assert_eq!(
        s.vertical_align,
        swb_style::VerticalAlign::Keyword(swb_style::VerticalAlignKeyword::Top)
    );
    assert_eq!(s.text_align, swb_style::TextAlign::WebkitRight);

    // Links: `a:link { color: #000000; text-decoration: none }`.
    let link = first(&page, |e| e.attr("href") == Some("news"));
    let s = style_of(&styles, link);
    assert_eq!(s.color, Rgba::BLACK);
    assert!(s.text_decoration_line.is_empty());

    // The logo image: width/height attributes and an inline style.
    let logo = first(&page, |e| e.attr("src") == Some("y18.svg"));
    let s = style_of(&styles, logo);
    assert_eq!(
        s.width,
        swb_style::Size::LengthPercentage(LengthPercentage::Px(18.0))
    );
    assert_eq!(s.display, Display::Block);
    assert_eq!(s.border_top_width, 1.0);

    // Head content is not rendered.
    let title = first(&page, |e| &**e.local_name() == "title");
    assert!(styles.get(title).is_none());

    // The vote arrow background image resolves against the stylesheet.
    let urls = styles.image_urls();
    assert!(
        urls.iter()
            .any(|u| &**u == "https://news.ycombinator.com/triangle.svg"),
        "{urls:?}"
    );
}

#[test]
fn wikipedia() {
    let page = load("wikipedia-web-browser");
    assert_eq!(page.doc.quirks_mode, swb_dom::QuirksMode::NoQuirks);
    let started = Instant::now();
    let styles = styles(&page);
    eprintln!(
        "wikipedia: compute_styles took {:?} for {} nodes",
        started.elapsed(),
        page.doc.len()
    );
    let body = page.doc.body().expect("body");
    let s = style_of(&styles, body);
    assert_eq!(
        &*s.font_family,
        &[FontFamily::Generic(GenericFamily::SansSerif)]
    );
    let html = page.doc.document_element().expect("html");
    let root = style_of(&styles, html);
    assert_eq!(root.font_size, 16.0);
    assert!(
        root.custom_properties
            .as_ref()
            .is_some_and(|c| c.len() > 50)
    );

    // The article text: `.mw-body-content { font-size: var(--font-size-medium) }`
    // is 14px (0.875rem) at the default size.
    let content = first(&page, |e| e.id() == Some("mw-content-text"));
    let s = style_of(&styles, content);
    assert!(s.font_size > 13.0 && s.font_size < 17.0, "{}", s.font_size);
    // Text color comes from `var(--color-base)`.
    assert_eq!(s.color, Rgba::rgb(0x20, 0x21, 0x22));

    // Every rendered element has a style.
    let mut count = 0;
    for node in page.doc.descendants(body) {
        if page.doc.element(node).is_some() && styles.get(node).is_some() {
            count += 1;
        }
    }
    assert!(count > 3000, "{count}");
}

#[test]
fn senko_net() {
    let page = load("senko-net");
    let styles = styles(&page);
    let body = page.doc.body().expect("body");
    assert!(styles.get(body).is_some());
}

/// The element nodes of the page, in document order.
fn elements(page: &Page) -> Vec<NodeId> {
    page.doc
        .descendants(NodeId::DOCUMENT)
        .filter(|&n| page.doc.element(n).is_some())
        .collect()
}

/// A series of hover, active and focus changes on `nodes`, ending with
/// none.
fn state_series(nodes: &[NodeId]) -> Vec<ElementStates> {
    let step = (nodes.len() / 12).max(1);
    let mut series = Vec::new();
    for (i, &node) in nodes.iter().enumerate().step_by(step) {
        series.push(ElementStates {
            hover: Some(node),
            ..ElementStates::default()
        });
        series.push(if i % 2 == 0 {
            ElementStates {
                hover: Some(node),
                active: Some(node),
                ..ElementStates::default()
            }
        } else {
            ElementStates {
                focus: Some(node),
                focus_visible: true,
                ..ElementStates::default()
            }
        });
    }
    series.push(ElementStates::default());
    series
}

/// A restyle that reuses the previous styles ([`compute_styles_reusing`])
/// gives the same styles as a fresh one, after a series of hover, active
/// and focus changes, and an element whose custom properties did not
/// change keeps the very same map (ADR 0007, "Sharing of custom-property
/// maps"). `same_styles` compares every element's and pseudo-element's
/// style deeply, so this covers the reuse on all page fixtures.
#[test]
fn restyle_reuse_matches_a_fresh_restyle() {
    let sizes = ContainerSizes::new();
    for name in [
        "ars-technica",
        "bbc",
        "github-swb",
        "hacker-news",
        "senko-net",
        "wikipedia-web-browser",
    ] {
        let page = load(name);
        let nodes = elements(&page);
        let mut previous = styles(&page);
        let mut shared = 0usize;
        for states in state_series(&nodes) {
            let env = environment();
            let fresh = compute_styles(&page.doc, &page.stylist, &env, &states, &page.url);
            let reused = compute_styles_reusing(
                &page.doc,
                &page.stylist,
                &env,
                &states,
                &page.url,
                &sizes,
                &previous,
            );
            assert!(reused.same_styles(&fresh), "{name}: {states:?}");
            assert!(fresh.same_styles(&reused), "{name}: {states:?}");
            for &node in &nodes {
                let (Some(old), Some(new)) = (previous.get(node), reused.get(node)) else {
                    continue;
                };
                match (&old.custom_properties, &new.custom_properties) {
                    (Some(a), Some(b)) if **a == **b => {
                        assert!(Arc::ptr_eq(a, b), "{name}: an unchanged map is not shared");
                        shared += 1;
                    }
                    (Some(_), Some(_)) | (None, None) => {}
                    _ => panic!("{name}: custom properties appeared or vanished"),
                }
            }
            previous = reused;
        }
        eprintln!("{name}: {shared} shared maps");
    }
}

/// A deterministic random number in `0..n` (a linear congruential
/// generator; the series must not change between runs).
fn next_random(seed: &mut u64, n: usize) -> usize {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((*seed >> 33) as usize) % n.max(1)
}

/// The next state of a random series of hover, active and focus changes
/// (focus by keyboard and by click, focus-visible alone, no focus).
fn random_states(
    states: &ElementStates,
    styled: &[NodeId],
    focusable: &[NodeId],
    seed: &mut u64,
) -> ElementStates {
    let mut next = states.clone();
    match next_random(seed, 10) {
        0..=3 => next.hover = Some(styled[next_random(seed, styled.len())]),
        4 => {
            next.active = if next.active.is_some() {
                None
            } else {
                next.hover
            }
        }
        5 | 6 => {
            let pool = if focusable.is_empty() {
                styled
            } else {
                focusable
            };
            next.focus = Some(pool[next_random(seed, pool.len())]);
            next.focus_visible = next_random(seed, 2) == 0;
        }
        7 => next.focus_visible = !next.focus_visible,
        8 => next.focus = None,
        _ => next.hover = None,
    }
    next
}

/// The first element or pseudo-element whose style differs, for messages.
fn first_difference(page: &Page, a: &StyleMap, b: &StyleMap) -> String {
    for node in page.doc.descendants(NodeId::DOCUMENT) {
        if a.get(node) != b.get(node) {
            let e = page.doc.element(node);
            return format!(
                "element {node:?} <{}> class={:?}",
                e.map_or(String::new(), |e| e.local_name().to_string()),
                e.and_then(|e| e.attr("class"))
            );
        }
        for kind in swb_style::PseudoKind::ALL {
            if a.pseudo(node, kind) != b.pseudo(node, kind) {
                return format!("pseudo {kind:?} of {node:?}");
            }
        }
        if a.list_item_ordinal(node) != b.list_item_ordinal(node) {
            return format!("ordinal of {node:?}");
        }
    }
    "the container log, the instances or the hidden contents flag".to_owned()
}

/// Runs a random series of state changes on `page` and checks that the
/// targeted restyle ([`restyle_states`]) gives the result of a full
/// restyle after each (ADR 0041 section 6). Returns the number of changes
/// per path.
fn targeted_series(page: &Page, sizes: &ContainerSizes, steps: usize) -> Vec<(String, usize)> {
    let env = environment();
    let mut states = ElementStates::default();
    let mut map =
        compute_styles_with_containers(&page.doc, &page.stylist, &env, &states, &page.url, sizes);
    let styled: Vec<NodeId> = page
        .doc
        .descendants(NodeId::DOCUMENT)
        .filter(|&n| map.get(n).is_some())
        .collect();
    let focusable: Vec<NodeId> = styled
        .iter()
        .copied()
        .filter(|&n| {
            page.doc
                .element(n)
                .is_some_and(|e| matches!(&**e.local_name(), "a" | "button" | "input" | "summary"))
        })
        .collect();
    let mut seed = 0x5eed_u64;
    let mut paths = std::collections::BTreeMap::<String, usize>::new();
    for step in 0..steps {
        let new = random_states(&states, &styled, &focusable, &mut seed);
        let full =
            compute_styles_with_containers(&page.doc, &page.stylist, &env, &new, &page.url, sizes);
        let before = map.clone();
        let outcome = restyle_states(
            &page.doc,
            &page.stylist,
            &env,
            &page.url,
            sizes,
            &states,
            &new,
            &mut map,
        );
        let path = match &outcome {
            Restyle::Done { restyled: 0, .. } => "nothing".to_owned(),
            Restyle::Done { .. } => "targeted".to_owned(),
            Restyle::Full(reason) => format!("full: {reason:?}"),
        };
        *paths.entry(path).or_default() += 1;
        match outcome {
            Restyle::Done { changed, .. } => {
                assert!(
                    map.same_result(&full),
                    "step {step}: {:?} -> {:?}: {}",
                    states.hover,
                    new.hover,
                    first_difference(page, &map, &full)
                );
                assert_eq!(
                    changed,
                    !before.same_styles(&full),
                    "step {step}: the change flag"
                );
            }
            Restyle::Full(_) => {
                assert!(before.same_result(&map), "a fallback leaves the map");
                map = full;
            }
        }
        states = new;
    }
    paths.into_iter().collect()
}

/// The targeted restyle of ADR 0041 gives the styles of a full restyle
/// after a random series of hover, active and focus changes on every page
/// fixture, and most changes take the targeted path.
#[test]
fn targeted_restyle_matches_a_full_restyle() {
    for name in [
        "ars-technica",
        "bbc",
        "craigslist-posting",
        "github-swb",
        "hacker-news",
        "senko-net",
        "wikipedia-web-browser",
    ] {
        let page = load(name);
        let paths = targeted_series(&page, &ContainerSizes::new(), 120);
        eprintln!("{name}: {paths:?}");
        let targeted: usize = paths
            .iter()
            .filter(|(p, _)| p == "targeted" || p == "nothing")
            .map(|(_, n)| n)
            .sum();
        assert!(targeted >= 100, "{name}: {paths:?}");
    }
}

/// The same with sizes for GitHub's query containers, so that the
/// container log has sizes to refresh.
#[test]
fn targeted_restyle_with_container_sizes() {
    let page = load("github-swb");
    let base = styles(&page);
    let mut sizes = ContainerSizes::new();
    for node in page.doc.descendants(NodeId::DOCUMENT) {
        if base.get(node).is_some_and(|s| s.is_query_container()) {
            sizes.insert(node, 900.0, 300.0);
        }
    }
    assert!(!sizes.is_empty());
    let paths = targeted_series(&page, &sizes, 120);
    eprintln!("github-swb with sizes: {paths:?}");
}

/// Attribute names that the random DOM changes set: those that selectors,
/// the HTML states and the table of attributes read by other elements
/// (ADR 0042 section 2) look at, and some that nothing reads.
const ATTRIBUTES: &[&str] = &[
    "hidden",
    "aria-expanded",
    "aria-hidden",
    "aria-current",
    "data-state",
    "open",
    "disabled",
    "checked",
    "selected",
    "required",
    "readonly",
    "placeholder",
    "value",
    "type",
    "href",
    "lang",
    "dir",
    "title",
    "role",
    "tabindex",
    "contenteditable",
    "link",
    "cellpadding",
    "border",
    "start",
    "reversed",
    "width",
    "height",
    "bgcolor",
    "align",
    "rel",
    "name",
];

/// Values for the attributes of [`ATTRIBUTES`] and for `style`.
const VALUES: &[&str] = &[
    "",
    "true",
    "false",
    "rtl",
    "en",
    "de",
    "3",
    "#x",
    "text",
    "checkbox",
    "red",
    "color: red",
    "display: none",
    "display: block",
    "display: list-item",
    "counter-increment: c",
    "font-size: 30px",
];

/// The connected elements below the `body` (the root, `head` and `body`
/// stay).
fn movable_elements(doc: &Document) -> Vec<NodeId> {
    let Some(body) = doc.body() else {
        return Vec::new();
    };
    doc.descendants(body)
        .skip(1)
        .filter(|&n| doc.element(n).is_some())
        .collect()
}

/// An attribute name without a namespace.
fn qual(name: &str) -> swb_dom::QualName {
    swb_dom::QualName::new(None, swb_dom::ns!(), swb_dom::LocalName::from(name))
}

/// A clone of `source` or a new element (with a class of `classes` and
/// maybe a text), not yet inserted.
fn new_subtree(
    doc: &mut Document,
    source: NodeId,
    classes: &[String],
    seed: &mut u64,
) -> Option<NodeId> {
    if next_random(seed, 2) == 0 {
        return doc
            .clone_node(source, NodeId::DOCUMENT, true)
            .ok()
            .map(|c| c.root);
    }
    let name = ["div", "li", "span", "p", "td"][next_random(seed, 5)];
    let qualified =
        swb_dom::QualName::new(None, swb_dom::ns!(html), swb_dom::LocalName::from(name));
    let created = doc.create_element(qualified, Vec::new()).ok()?;
    if !classes.is_empty() {
        let class = classes[next_random(seed, classes.len())].clone();
        let _ = doc.set_attr(created, "class", &class);
    }
    if next_random(seed, 2) == 0
        && let Ok(t) = doc.create_text("t")
    {
        let _ = doc.append_child(created, t);
    }
    Some(created)
}

/// One random DOM change of `doc` (ADR 0042 section 7): class tokens, IDs,
/// other attributes and `style` added, changed and removed; subtrees
/// inserted (new elements and clones), removed and moved; text changed,
/// emptied and filled. Errors (a move into the moved subtree) are ignored.
fn random_dom_change(doc: &mut Document, classes: &[String], seed: &mut u64) {
    let elements = movable_elements(doc);
    if elements.is_empty() {
        return;
    }
    let pick = |seed: &mut u64| elements[next_random(seed, elements.len())];
    match next_random(seed, 12) {
        0..=2 => {
            let e = pick(seed);
            let mut tokens: Vec<String> = doc
                .element(e)
                .map(|d| d.classes().map(str::to_owned).collect())
                .unwrap_or_default();
            if !tokens.is_empty() && next_random(seed, 2) == 0 {
                tokens.remove(next_random(seed, tokens.len()));
            } else if !classes.is_empty() {
                tokens.push(classes[next_random(seed, classes.len())].clone());
            }
            let _ = doc.set_attr(e, "class", &tokens.join(" "));
        }
        3 => {
            let e = pick(seed);
            if next_random(seed, 3) == 0 {
                doc.remove_attribute(e, &qual("id"));
            } else {
                let other = pick(seed);
                let id = doc
                    .element(other)
                    .and_then(|d| d.id())
                    .map_or_else(|| format!("r{}", next_random(seed, 50)), str::to_owned);
                let _ = doc.set_attr(e, "id", &id);
            }
        }
        4 | 5 => {
            let e = pick(seed);
            let name = if next_random(seed, 4) == 0 {
                "style"
            } else {
                ATTRIBUTES[next_random(seed, ATTRIBUTES.len())]
            };
            if next_random(seed, 3) == 0 {
                doc.remove_attribute(e, &qual(name));
            } else {
                let _ = doc.set_attr(e, name, VALUES[next_random(seed, VALUES.len())]);
            }
        }
        6 => {
            let source = pick(seed);
            if let Some(node) = new_subtree(doc, source, classes, seed) {
                insert_somewhere(doc, node, &elements, seed);
            }
        }
        7 => {
            let e = pick(seed);
            doc.remove(e);
        }
        8 => {
            // A move.
            let e = pick(seed);
            doc.remove(e);
            insert_somewhere(doc, e, &elements, seed);
        }
        9 => {
            let texts: Vec<NodeId> = doc
                .descendants(NodeId::DOCUMENT)
                .filter(|&n| doc.node(n).as_text().is_some())
                .collect();
            if !texts.is_empty() {
                let t = texts[next_random(seed, texts.len())];
                let data = if next_random(seed, 2) == 0 {
                    ""
                } else {
                    "changed"
                };
                let _ = doc.set_data(t, data);
            }
        }
        10 => {
            // Text into an element (maybe empty before).
            let e = pick(seed);
            if let Ok(t) = doc.create_text("x") {
                let _ = doc.append_child(e, t);
            }
        }
        _ => {
            // Empty an element.
            let e = pick(seed);
            let children: Vec<NodeId> = doc.children(e).collect();
            for c in children {
                doc.remove(c);
            }
        }
    }
}

/// Inserts `node` before a random element of `elements` or as the last
/// child of one.
fn insert_somewhere(doc: &mut Document, node: NodeId, elements: &[NodeId], seed: &mut u64) {
    let target = elements[next_random(seed, elements.len())];
    if doc.shadow_including_root(target) != NodeId::DOCUMENT {
        return;
    }
    if next_random(seed, 2) == 0 {
        if let Some(parent) = doc.parent(target) {
            let _ = doc.insert_before(parent, node, Some(target));
        }
    } else {
        let _ = doc.append_child(target, node);
    }
}

/// A journal entry, for messages.
fn describe(doc: &Document, change: &swb_dom::Change<'_>) -> String {
    let name = |n: NodeId| {
        doc.element(n).map_or_else(
            || format!("{n:?}"),
            |e| format!("{n:?}<{} class={:?}>", e.local_name(), e.attr("class")),
        )
    };
    match change {
        swb_dom::Change::Children {
            parent,
            added,
            removed,
            ..
        } => format!(
            "children of {}: +{:?} -{:?}",
            name(*parent),
            added.iter().map(|&n| name(n)).collect::<Vec<_>>(),
            removed.iter().map(|&n| name(n)).collect::<Vec<_>>()
        ),
        swb_dom::Change::Attribute {
            element,
            name: attr,
            old_value,
        } => format!(
            "{} [{}] {:?} -> {:?}",
            name(*element),
            attr.local,
            old_value,
            doc.element(*element).and_then(|e| e.attr(&attr.local))
        ),
        swb_dom::Change::Text { node, .. } => format!("text {node:?}"),
        swb_dom::Change::TopLayer { element } => format!("top layer {}", name(*element)),
    }
}

/// Records the journal of `doc` in `changes`, as one drain of the engine
/// does, and appends the descriptions of its changes to `described`.
fn drain(doc: &mut Document, changes: &mut DomChanges, described: &mut Vec<String>) {
    let journal = doc.take_journal();
    assert!(!journal.incomplete());
    described.extend(journal.iter().map(|c| describe(doc, &c)));
    for change in journal.iter() {
        let target = match &change {
            swb_dom::Change::Children { parent, .. } => *parent,
            swb_dom::Change::Attribute { element, .. } | swb_dom::Change::TopLayer { element } => {
                *element
            }
            swb_dom::Change::Text { node, .. } => *node,
        };
        let connected = doc.shadow_including_root(target) == NodeId::DOCUMENT;
        changes.record(doc, &change, connected);
    }
}

/// Runs a seeded random series of DOM changes (one to three between two
/// flushes, in one drain or in one drain each, sometimes with a hover
/// change) on `page` and checks after each flush that the targeted restyle
/// ([`restyle_targeted`]) gives the result of a full restyle, with no style
/// left for a node outside the document (ADR 0042 section 7). Returns the
/// number of flushes per path.
fn dom_series(page: &mut Page, steps: usize, seed: u64) -> Vec<(String, usize)> {
    let env = environment();
    let sizes = ContainerSizes::new();
    page.doc.set_journaling(true);
    let _ = page.doc.take_journal();
    let classes: Vec<String> = {
        let mut set = std::collections::BTreeSet::new();
        for n in page.doc.descendants(NodeId::DOCUMENT) {
            if let Some(e) = page.doc.element(n) {
                set.extend(e.classes().map(str::to_owned));
            }
        }
        set.into_iter().take(400).collect()
    };
    let mut states = ElementStates::default();
    let mut map =
        compute_styles_with_containers(&page.doc, &page.stylist, &env, &states, &page.url, &sizes);
    let mut seed = seed;
    let mut paths = std::collections::BTreeMap::<String, usize>::new();
    let mut changes = DomChanges::default();
    for step in 0..steps {
        // Several drains before one flush: the tasks of a page between two
        // rendering updates.
        let drain_each = next_random(&mut seed, 2) == 0;
        let mut described = Vec::new();
        for _ in 0..=next_random(&mut seed, 3) {
            random_dom_change(&mut page.doc, &classes, &mut seed);
            if drain_each {
                drain(&mut page.doc, &mut changes, &mut described);
            }
        }
        let mut new = states.clone();
        if next_random(&mut seed, 4) == 0 {
            let elements = movable_elements(&page.doc);
            if !elements.is_empty() {
                new.hover = Some(elements[next_random(&mut seed, elements.len())]);
            }
        }
        drain(&mut page.doc, &mut changes, &mut described);
        let full =
            compute_styles_with_containers(&page.doc, &page.stylist, &env, &new, &page.url, &sizes);
        let before = map.clone();
        let outcome = restyle_targeted(
            &page.doc,
            &page.stylist,
            &env,
            &page.url,
            &sizes,
            &states,
            &new,
            &changes,
            &mut map,
        );
        let path = match &outcome {
            Restyle::Done { restyled: 0, .. } => "nothing".to_owned(),
            Restyle::Done { .. } => "targeted".to_owned(),
            Restyle::Full(reason) => format!("full: {reason:?}"),
        };
        *paths.entry(path).or_default() += 1;
        match outcome {
            Restyle::Done { .. } => {
                assert!(
                    map.same_result(&full),
                    "step {step}: {} ({:?}); the changes: {described:#?}; hover {:?} -> {:?}",
                    first_difference(page, &map, &full),
                    map.first_difference(&full),
                    states.hover,
                    new.hover
                );
            }
            Restyle::Full(_) => {
                assert!(before.same_result(&map), "a fallback leaves the map");
                map = full;
            }
        }
        changes.clear();
        states = new;
    }
    paths.into_iter().collect()
}

/// The targeted restyle of ADR 0042 gives the styles of a full restyle after
/// a random series of DOM changes on every page fixture, and most flushes
/// take the targeted path.
#[test]
fn dom_restyle_matches_a_full_restyle() {
    for name in [
        "ars-technica",
        "bbc",
        "craigslist-posting",
        "github-swb",
        "hacker-news",
        "senko-net",
        "wikipedia-web-browser",
    ] {
        let mut page = load(name);
        let paths = dom_series(&mut page, 150, 0xd0d0);
        eprintln!("{name}: {paths:?}");
        let targeted: usize = paths
            .iter()
            .filter(|(p, _)| p == "targeted" || p == "nothing")
            .map(|(_, n)| n)
            .sum();
        assert!(targeted >= 75, "{name}: {paths:?}");
    }
}

/// A generated page with rules of each shape that ADR 0042 section 1
/// analyses (sibling combinators, the structural pseudo-classes, `:empty`,
/// `:has()` in the subject and in other compounds with both leading
/// combinators, `:lang()`, `:dir()`, attribute selectors, states), and a
/// small set of classes, so that random changes hit the rules often: a
/// wrong reach shows as a difference from the full restyle.
const SHAPES_CSS: &str = "
    .a + .b { color: rgb(1, 0, 0) }
    .a ~ .c { margin-left: 1px }
    .b + .c .d { padding-left: 1px }
    .c ~ .a > .d { padding-top: 1px }
    li:first-child { color: rgb(2, 0, 0) }
    li:last-child { color: rgb(3, 0, 0) }
    li:only-child { margin-top: 2px }
    li:nth-child(2n+1) { background-color: rgb(0, 1, 0) }
    li:nth-last-child(2) { border-left: 1px solid }
    span:first-of-type { color: rgb(4, 0, 0) }
    span:last-of-type .d { color: rgb(5, 0, 0) }
    div:empty { height: 3px }
    .a:empty + .b { width: 7px }
    .a:has(.b) { color: rgb(6, 0, 0) }
    .a:has(> .c) .d { color: rgb(7, 0, 0) }
    .b:has(+ .c) { margin-right: 3px }
    .c:has(~ .a .b) ~ .d { margin-bottom: 3px }
    .a:has(.c .d) > .b { padding-right: 2px }
    section:has(.e) li { text-indent: 2px }
    :lang(de) .d { font-style: italic }
    :dir(rtl) > .b { text-align: right }
    [data-state] .c { color: rgb(8, 0, 0) }
    [aria-expanded=true] + .d { color: rgb(9, 0, 0) }
    [hidden] { display: none }
    .e[title] ~ li { word-spacing: 1px }
    .a:hover .b { color: rgb(10, 0, 0) }
    .e:hover + .c { color: rgb(11, 0, 0) }
    :is(.a, .e) .b + .d { letter-spacing: 1px }
    :not(.c) > .e { padding-bottom: 1px }
    #x .a { color: rgb(12, 0, 0) }
    #y + .b { color: rgb(13, 0, 0) }
    ol li::marker { color: rgb(14, 0, 0) }
    .d::before { content: 'x' }
    input:disabled + .a { color: rgb(15, 0, 0) }
    input:checked ~ .b { color: rgb(16, 0, 0) }
    fieldset:disabled .e { color: rgb(17, 0, 0) }
    [contenteditable] :read-write { color: rgb(18, 0, 0) }
    details[open] > .c { color: rgb(19, 0, 0) }
    table td { padding-left: 1px }
";

/// The generated page of [`SHAPES_CSS`].
fn shapes_page() -> Page {
    use std::fmt::Write as _;
    let classes = ["a", "b", "c", "d", "e"];
    let mut html = String::from("<!DOCTYPE html><html lang=en><body>");
    let mut seed = 7_u64;
    for section in 0..6 {
        let _ = write!(html, "<section id=s{section}>");
        for item in 0..5 {
            let class = classes[next_random(&mut seed, classes.len())];
            let other = classes[next_random(&mut seed, classes.len())];
            let _ = write!(
                html,
                "<div class='{class}'><span class={other}>t</span><span class=d></span>\
                 <ul><li class={class}>1</li><li>2</li><li class={other}>3</li></ul>\
                 <div class={other}></div></div>"
            );
            if item == 2 {
                html.push_str(
                    "<ol><li>a</li><li class=a>b</li></ol>\
                     <fieldset><input type=checkbox><div class=a><span class=e>f</span></div></fieldset>\
                     <details><summary>s</summary><div class=c>c</div></details>\
                     <table><tr><td class=a>1</td><td>2</td></tr></table>",
                );
            }
        }
        html.push_str("<div id=x><p class=a>x</p></div><div id=y></div><p class=b>y</p></section>");
    }
    html.push_str("</body></html>");
    let (doc, _) = swb_dom::parse_html_bytes(html.as_bytes(), Some("utf-8"));
    let mut stylist = Stylist::new(doc.quirks_mode);
    let url = Url::parse("https://example.com/").expect("url");
    stylist.add_author_sheet(&swb_css::parse_stylesheet(SHAPES_CSS), &url);
    Page { doc, stylist, url }
}

/// The oracle on the generated page: many changes, several seeds.
#[test]
fn dom_restyle_matches_on_every_rule_shape() {
    for seed in [1_u64, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
        let mut page = shapes_page();
        let paths = dom_series(&mut page, 300, seed);
        eprintln!("shapes (seed {seed}): {paths:?}");
        let targeted: usize = paths
            .iter()
            .filter(|(p, _)| p == "targeted" || p == "nothing")
            .map(|(_, n)| n)
            .sum();
        assert!(targeted >= 200, "{paths:?}");
    }
}
