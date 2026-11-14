//! Container queries in the cascade (ADR 0038): rule selection under
//! `@container`, the query container lookup, the container units, style
//! sharing and the container log. The sizes stand for a layout; the
//! expected results are those of Chromium 148 in the cases of
//! `tools/probes/container-queries.json` named in each test.

use swb_css::MediaEnvironment;
use swb_dom::{Document, parse_html};

use super::*;
use crate::container::ContainerSizes;
use crate::values::Rgba;

const RED: Rgba = Rgba::rgb(255, 0, 0);
const GREEN: Rgba = Rgba::rgb(0, 128, 0);

fn node(doc: &Document, id: &str) -> NodeId {
    doc.element_by_id(id)
        .unwrap_or_else(|| panic!("no element #{id}"))
}

fn base() -> Url {
    Url::parse("https://example.com/").expect("valid URL")
}

fn stylist(doc: &Document, css: &str) -> Stylist {
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
    stylist
}

/// Styles `html` with `css`, the containers `sizes` (id, width, height)
/// standing for the last layout.
fn render(html: &str, css: &str, sizes: &[(&str, f32, f32)]) -> (Document, StyleMap) {
    let doc = parse_html(&format!("<!DOCTYPE html>{html}"));
    let mut boxes = ContainerSizes::new();
    for &(id, w, h) in sizes {
        boxes.insert(node(&doc, id), w, h);
    }
    let map = compute_styles_with_containers(
        &doc,
        &stylist(&doc, css),
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base(),
        &boxes,
    );
    (doc, map)
}

fn style<'a>(doc: &Document, map: &'a StyleMap, id: &str) -> &'a ComputedStyle {
    map.get(node(doc, id))
        .unwrap_or_else(|| panic!("#{id} has no style"))
}

fn color(doc: &Document, map: &StyleMap, id: &str) -> Rgba {
    style(doc, map, id).color
}

const SIGNAL: &str = ".t { color: rgb(255, 0, 0) }";

#[test]
fn rules_apply_when_the_condition_holds_for_the_container_size() {
    let css = format!(
        "{SIGNAL} #c {{ container-type: inline-size }} \
         @container (width > 200px) {{ #a {{ color: rgb(0, 128, 0) }} }} \
         @container (width > 400px) {{ #b {{ color: rgb(0, 128, 0) }} }}"
    );
    let html = "<div id=c><p class=t id=a></p><p class=t id=b></p></div>";
    let (doc, map) = render(html, &css, &[("c", 300.0, 0.0)]);
    assert_eq!(color(&doc, &map, "a"), GREEN);
    assert_eq!(color(&doc, &map, "b"), RED);
    // Before the first layout, the size is unknown: every query is false.
    let (doc, map) = render(html, &css, &[]);
    assert_eq!(color(&doc, &map, "a"), RED);
    assert!(!map.container_log().is_empty());
}

#[test]
fn the_lookup_follows_names_and_axes() {
    // `names`: an outer size container, a named inline-size one, a named
    // container of type normal.
    let css = format!(
        "{SIGNAL} #o {{ container: outer / size }} \
         #n {{ container-name: a b card; container-type: inline-size }} \
         #plain {{ container-name: plain }} #c {{ container: inner / inline-size }} \
         @container card (width > 300px) {{ #q1 {{ color: rgb(0, 128, 0) }} }} \
         @container (width > 300px) {{ #q2 {{ color: rgb(0, 128, 0) }} }} \
         @container Card (width > 300px) {{ #q3 {{ color: rgb(0, 128, 0) }} }} \
         @container plain (width > 0px) {{ #q4 {{ color: rgb(0, 128, 0) }} }} \
         @container (height > 0px) {{ #q5 {{ color: rgb(0, 128, 0) }} }} \
         @container card (height = 50px) {{ #q6 {{ color: rgb(0, 128, 0) }} }} \
         @container plain {{ #q7 {{ color: rgb(0, 128, 0) }} }} \
         @container (width > 100px) or (foo: 1) {{ #q8 {{ color: rgb(0, 128, 0) }} }} \
         @container b (width > 300px) {{ #q9 {{ color: rgb(0, 128, 0) }} }}"
    );
    let html = "<div id=o><div id=n><div id=plain><div id=c>\
                <p class=t id=q1></p><p class=t id=q2></p><p class=t id=q3></p>\
                <p class=t id=q4></p><p class=t id=q5></p><p class=t id=q6></p>\
                <p class=t id=q7></p><p class=t id=q8></p><p class=t id=q9></p>\
                </div></div></div></div>";
    let sizes = [("o", 600.0, 80.0), ("n", 400.0, 50.0), ("c", 200.0, 40.0)];
    let (doc, map) = render(html, &css, &sizes);
    let results: Vec<bool> = (1..=9)
        .map(|i| color(&doc, &map, &format!("q{i}")) == GREEN)
        .collect();
    assert_eq!(
        results,
        [true, false, false, false, true, false, true, false, true]
    );
}

#[test]
fn a_container_without_a_box_shadows_the_outer_one() {
    // `odd-container-boxes`: an inline container selected, its queries
    // unknown; an inline-block one works.
    let css = format!(
        "{SIGNAL} .c {{ container-type: inline-size }} \
         @container (width >= 0px) {{ .t {{ color: rgb(0, 128, 0) }} }}"
    );
    let html = "<div class=c id=o><span class=c id=s><span class=t id=a></span></span>\
                <span class=c id=ib style='display: inline-block'><span class=t id=b></span></span></div>";
    // Even with a size for the inline container, its box is not eligible.
    let sizes = [("o", 600.0, 0.0), ("s", 50.0, 0.0), ("ib", 50.0, 0.0)];
    let (doc, map) = render(html, &css, &sizes);
    assert_eq!(color(&doc, &map, "a"), RED);
    assert_eq!(color(&doc, &map, "b"), GREEN);
}

#[test]
fn own_styles_do_not_query_the_element_and_pseudo_elements_do() {
    // `own-size`, `pseudo-elements`.
    let css = format!(
        "{SIGNAL} #c {{ container-type: inline-size }} #c::before {{ content: 'x' }} \
         @container (width = 100px) {{ #c {{ color: rgb(0, 128, 0) }} #c::before {{ color: rgb(0, 0, 255) }} }}"
    );
    let html = "<div id=c class=t></div>";
    let (doc, map) = render(html, &css, &[("c", 100.0, 0.0)]);
    assert_eq!(color(&doc, &map, "c"), RED);
    let before = map
        .pseudo(node(&doc, "c"), PseudoKind::Before)
        .expect("a ::before box");
    assert_eq!(before.color, Rgba::rgb(0, 0, 255));
}

#[test]
fn units_use_the_nearest_container_per_axis() {
    // `units-inline-size-and-none`, `units-odd-containers`.
    let css = "#o { container-type: size } #c { container-type: inline-size } \
               #s { container-type: inline-size } \
               .w { font-size: 10cqw } .h { font-size: 10cqh } .min { font-size: 10cqmin }";
    let html = "<div id=o><div id=c><p class=w id=w></p><p class=h id=h></p><p class=min id=m></p>\
                <span id=s><span class=w id=inline></span></span></div></div>\
                <p class=w id=none></p>";
    let sizes = [("o", 500.0, 100.0), ("c", 400.0, 30.0), ("s", 50.0, 10.0)];
    let (doc, map) = render(html, css, &sizes);
    let size = |id: &str| style(&doc, &map, id).font_size;
    assert_eq!(size("w"), 40.0);
    assert_eq!(size("h"), 10.0, "cqh skips the inline-size container");
    assert_eq!(size("m"), 10.0);
    assert_eq!(size("inline"), 40.0, "an inline container is skipped");
    assert_eq!(size("none"), 80.0, "the viewport without a container");
}

#[test]
fn sharing_keeps_the_results_of_two_containers_apart() {
    // Two containers with one style and the same children: the children
    // have the same parent style and rules but other containers.
    let css = format!(
        "{SIGNAL} .c {{ container-type: inline-size }} \
         @container (width > 200px) {{ .t {{ color: rgb(0, 128, 0) }} }} \
         .u {{ font-size: 10cqw }}"
    );
    let html = "<div class=c id=a><p class=t id=ta></p><p class=u id=ua></p></div>\
                <div class=c id=b><p class=t id=tb></p><p class=u id=ub></p></div>";
    let (doc, map) = render(html, &css, &[("a", 300.0, 0.0), ("b", 100.0, 0.0)]);
    assert_eq!(color(&doc, &map, "ta"), GREEN);
    assert_eq!(color(&doc, &map, "tb"), RED);
    assert_eq!(style(&doc, &map, "ua").font_size, 30.0);
    assert_eq!(style(&doc, &map, "ub").font_size, 10.0);
}

#[test]
fn a_style_attribute_of_an_ancestor_can_bring_container_units() {
    // `units-attr-var`, `units-attr-var-script`: no rule has a container
    // unit; the custom property of the ancestor's `style` attribute does.
    // The sharing key must hold the container too (two containers, one
    // style for the children).
    let css = ".k { container-type: inline-size } .t { font-size: var(--w) } \
               #cp::after { content: 'x'; font-size: var(--w) }";
    let html = "<div style='--w: 10cqw'><div class=k id=a><p class=t id=ta></p></div>\
                <div class=k id=b><p class=t id=tb></p></div>\
                <div class=k id=cp></div></div>";
    let sizes = [("a", 300.0, 0.0), ("b", 200.0, 0.0), ("cp", 100.0, 0.0)];
    let (doc, map) = render(html, css, &sizes);
    assert_eq!(style(&doc, &map, "ta").font_size, 30.0);
    assert_eq!(style(&doc, &map, "tb").font_size, 20.0);
    // The chains of `getComputedStyle` give the same.
    let stylist = stylist(&doc, css);
    let mut boxes = ContainerSizes::new();
    for (id, w, h) in sizes {
        boxes.insert(node(&doc, id), w, h);
    }
    let env = MediaEnvironment::default();
    let states = ElementStates::default();
    let chain_of = |id: &str| {
        compute_element_styles_with_containers(
            &doc,
            &stylist,
            &env,
            &states,
            &base(),
            node(&doc, id),
            &boxes,
        )
    };
    assert_eq!(chain_of("tb").last().map(|(_, s)| s.font_size), Some(20.0));
    let chain = chain_of("cp");
    let after = compute_pseudo_element_style_with_containers(
        &doc,
        &stylist,
        &env,
        &states,
        &base(),
        &chain,
        PseudoKind::After,
        &boxes,
    )
    .expect("a style");
    assert_eq!(after.font_size, 10.0);
    // Without a unit in the attribute the viewport is used (no container
    // dependence at all).
    let (doc, map) = render(
        "<div style='--w: 10vw'><div class=k id=a><p class=t id=ta></p></div></div>",
        css,
        &[("a", 300.0, 0.0)],
    );
    assert_eq!(style(&doc, &map, "ta").font_size, 80.0);
}

#[test]
fn the_log_holds_until_a_result_changes() {
    let css = format!(
        "{SIGNAL} #c {{ container-type: inline-size }} \
         @container (width > 200px) {{ #a {{ color: rgb(0, 128, 0) }} }}"
    );
    let html = "<div id=c><p class=t id=a></p></div>";
    let (doc, map) = render(html, &css, &[("c", 300.0, 0.0)]);
    let c = node(&doc, "c");
    let log = map.container_log();
    assert_eq!(log.containers().collect::<Vec<_>>(), [c]);
    let mut sizes = ContainerSizes::new();
    sizes.insert(c, 250.0, 0.0);
    assert!(log.holds(&sizes));
    sizes.insert(c, 150.0, 0.0);
    assert!(!log.holds(&sizes));
    // A page without container rules logs nothing.
    let (_, map) = render(html, SIGNAL, &[("c", 300.0, 0.0)]);
    assert!(map.container_log().is_empty());
}

#[test]
fn element_and_pseudo_chains_use_the_sizes() {
    let css = format!(
        "{SIGNAL} #c {{ container-type: inline-size }} #c::after {{ content: 'x' }} \
         @container (width > 200px) {{ #a {{ color: rgb(0, 128, 0) }} #c::after {{ color: rgb(0, 128, 0) }} }}"
    );
    let doc = parse_html("<!DOCTYPE html><div id=c><p class=t id=a></p></div>");
    let stylist = stylist(&doc, &css);
    let mut sizes = ContainerSizes::new();
    sizes.insert(node(&doc, "c"), 300.0, 0.0);
    let env = MediaEnvironment::default();
    let states = ElementStates::default();
    let chain = compute_element_styles_with_containers(
        &doc,
        &stylist,
        &env,
        &states,
        &base(),
        node(&doc, "a"),
        &sizes,
    );
    assert_eq!(chain.last().map(|(_, s)| s.color), Some(GREEN));
    let chain = compute_element_styles_with_containers(
        &doc,
        &stylist,
        &env,
        &states,
        &base(),
        node(&doc, "c"),
        &sizes,
    );
    let after = compute_pseudo_element_style_with_containers(
        &doc,
        &stylist,
        &env,
        &states,
        &base(),
        &chain,
        PseudoKind::After,
        &sizes,
    )
    .expect("a style");
    assert_eq!(after.color, GREEN);
}

#[test]
fn deep_nesting_with_a_named_query_stays_linear() {
    // 20,000 nested containers; the query's name is on the root container.
    let depth = 20_000;
    let mut html = String::from("<div id=root style='container: top / inline-size'>");
    for _ in 0..depth {
        html.push_str("<div class=c>");
    }
    html.push_str("<p class=t id=leaf></p>");
    let css = format!(
        "{SIGNAL} .c {{ container-type: inline-size }} \
         @container top (width > 100px) {{ .t, .c {{ color: rgb(0, 128, 0) }} }}"
    );
    let (doc, map) = render(&html, &css, &[("root", 500.0, 0.0)]);
    assert_eq!(color(&doc, &map, "leaf"), GREEN);
}
