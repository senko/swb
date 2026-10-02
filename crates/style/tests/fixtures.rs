//! Computes styles for the real target pages in `fixtures/pages/` and
//! checks a few computed values against Chromium's.
//!
//! The stylesheets are loaded the way the engine loads them: `<link
//! rel=stylesheet>` (from the fixture manifest) and `<style>` elements, in
//! document order.

use std::path::{Path, PathBuf};
use std::time::Instant;

use swb_css::MediaEnvironment;
use swb_dom::{Document, NodeId};
use swb_style::{
    ComputedStyle, Display, ElementStates, FontFamily, GenericFamily, LengthPercentage, Rgba,
    StyleMap, Stylist, compute_styles,
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
