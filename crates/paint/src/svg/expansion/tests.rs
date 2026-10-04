//! Tests of the render tree bounds.

use std::fmt::Write as _;

use super::*;
use crate::svg::tests::use_chain;

const LIMITS: Limits = Limits {
    depth: 1024.0,
    css: css::Limits {
        parse: 1e8,
        declarations: 1e5,
        matches: 1e6,
    },
    link_scan: 1e6,
};

fn measure_content(content: &str) -> Result<Usage, &'static str> {
    let source = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">{content}</svg>"#
    );
    let xml = Document::parse(&source).unwrap();
    measure(&xml, &LIMITS)
}

fn elements(content: &str) -> f64 {
    measure_content(content)
        .unwrap_or_else(|e| panic!("{e}"))
        .elements
}

fn segments(content: &str) -> f64 {
    measure_content(content)
        .unwrap_or_else(|e| panic!("{e}"))
        .segments
}

#[test]
fn use_copies_count() {
    assert!(elements(&use_chain(3)) < 1000.0);
    assert!(elements(&use_chain(5)) > 30_000.0);
    // Cycles do not loop.
    let cycle = "<g id='a'><use href='#b'/></g><g id='b'><use href='#a'/></g>";
    assert!(elements(cycle) < 10.0);
}

#[test]
fn ids_resolve_as_in_usvg() {
    let big = format!("<g id='big'>{}</g>", "<rect/>".repeat(100));
    // A plain href comes before xlink:href.
    let content =
        format!("<defs><g id='empty'/>{big}</defs><use xlink:href='#empty' href='#big'/>");
    assert!(elements(&content) > 200.0);
    // url() refers to the last element with an id.
    let masks = format!(
        "<mask id='m'><rect/></mask><mask id='m'>{}</mask>",
        "<rect/>".repeat(100)
    );
    let users = "<rect mask='url(#m)'/>".repeat(10);
    assert!(elements(&format!("{masks}{users}")) > 1000.0);
    // An unquoted id ends only at a space or `)`.
    let tab = format!(
        "<mask id='a'><rect/></mask><mask id='a&#9;b'>{}</mask>{}",
        "<rect/>".repeat(100),
        "<rect mask='url(#a&#9;b)'/>".repeat(10)
    );
    assert!(elements(&tab) > 1000.0);
}

#[test]
fn references_count_as_copies_unless_shared() {
    let mask = format!("<mask id='m'>{}</mask>", "<rect/>".repeat(100));
    let users = "<rect mask='url(#m)'/>".repeat(10);
    assert!(elements(&format!("{mask}{users}")) > 1000.0);
    // References from style sheets count for the elements they match.
    let css = "<style>.a { mask: url('#m') }</style>";
    let styled = "<rect class='a'/>".repeat(10);
    assert!(elements(&format!("{mask}{css}{styled}")) > 1000.0);
    let unstyled = "<rect/>".repeat(10);
    assert!(elements(&format!("{mask}{css}{unstyled}")) < 200.0);
    // A user-space clip path is converted once: its path segments count
    // once, its elements for every use (usvg walks them).
    let outline = format!("<path d='M0 0{}'/>", " l1 1".repeat(1000));
    let clip = format!("<clipPath id='c'>{outline}</clipPath>");
    let users = "<rect clip-path='url(#c)'/>".repeat(100);
    let usage = measure_content(&format!("{clip}{users}")).unwrap();
    // The outline in the document, once more as shared, and the users;
    // copies would be 100,000.
    assert!(usage.segments < 4000.0, "{usage:?}");
    assert!(usage.elements >= 300.0, "{usage:?}");
}

#[test]
fn illustrator_style_sheets_pass() {
    let mut content = String::from("<style>");
    for i in 0..6 {
        write!(content, ".st{i}{{fill:url(#SVGID_{i}_);}}").unwrap();
    }
    content.push_str("</style>");
    for i in 0..6 {
        write!(
            content,
            "<linearGradient id='SVGID_{i}_' gradientUnits='userSpaceOnUse'>\
             <stop offset='0' style='stop-color:#fff'/><stop offset='1'/></linearGradient>"
        )
        .unwrap();
    }
    for i in 0..100 {
        write!(content, "<path class='st{}' d='M0 0 L1 1'/>", i % 6).unwrap();
    }
    assert!(elements(&content) < 500.0);
}

#[test]
fn url_cycles_are_rejected() {
    for (kind, attribute) in [("mask", "mask"), ("clipPath", "clip-path")] {
        let mut content = String::new();
        for i in 0..3 {
            let next = (i + 1) % 3;
            write!(
                content,
                "<{kind} id='l{i}'><rect {attribute}='url(#l{next})'/></{kind}>"
            )
            .unwrap();
        }
        write!(content, "<rect {attribute}='url(#l0)'/>").unwrap();
        assert!(measure_content(&content).is_err(), "{kind}");
    }
    // Gradient href cycles are cut.
    let gradients = "<linearGradient id='a' href='#b'/><linearGradient id='b' href='#a'/>\
                     <rect fill='url(#a)'/>";
    assert!(measure_content(gradients).is_ok());
    // Cycles that mix url() and href recurse in usvg.
    let patterns = "<pattern id='p1'><rect fill='url(#p2)'/></pattern>\
                    <pattern id='p2' href='#p1'/><rect fill='url(#p1)'/>";
    assert!(measure_content(patterns).is_err());
    let filters = "<filter id='f1'><feImage href='#r'/></filter><filter id='f2' href='#f1'/>\
                   <rect id='r' filter='url(#f2)'/>";
    assert!(measure_content(filters).is_err());
}

#[test]
fn pattern_units_are_inherited_through_href() {
    let content = format!(
        "<pattern id='p1' patternContentUnits='objectBoundingBox'>{}</pattern>\
         <pattern id='p2' href='#p1' patternUnits='userSpaceOnUse'/>{}",
        "<rect/>".repeat(100),
        "<rect fill='url(#p2)'/>".repeat(10)
    );
    assert!(elements(&content) > 1000.0, "copied for every user");
}

#[test]
fn reference_chains_count_as_depth() {
    let mut content = String::from("<mask id='m0'><rect/></mask>");
    for i in 1..10 {
        write!(
            content,
            "<mask id='m{i}'><rect mask='url(#m{})'/></mask>",
            i - 1
        )
        .unwrap();
    }
    content.push_str("<rect mask='url(#m9)'/>");
    let usage = measure_content(&content).unwrap();
    assert!(usage.depth >= 10.0 * LINK_DEPTH, "{usage:?}");
    let mut long = String::from("<clipPath id='c0'><rect/></clipPath>");
    for i in 1..100 {
        write!(
            long,
            "<clipPath id='c{i}'><rect clip-path='url(#c{})'/></clipPath>",
            i - 1
        )
        .unwrap();
    }
    long.push_str("<rect clip-path='url(#c99)'/>");
    assert!(measure_content(&long).is_err());
}

#[test]
fn markers_count_per_vertex() {
    let marker = "<marker id='m' markerWidth='3' markerHeight='3'><rect/></marker>";
    let points = "1,1 ".repeat(1000);
    let line = format!("<polyline points='{points}' marker-mid='url(#m)'/>");
    let small = elements(&format!("{marker}{line}"));
    assert!(small > 2000.0 && small < 5000.0, "{small}");
    let large = format!("<marker id='m'>{}</marker>", "<rect/>".repeat(20));
    assert!(elements(&format!("{large}{line}")) > 20_000.0);
}

#[test]
fn nested_markers_multiply() {
    let path =
        |marker: &str| format!("<path d='M0 0 L1 1 L2 2 L3 3' marker-mid='url(#{marker})'/>");
    let mut content = String::new();
    for i in 0..6 {
        let inner = path(&format!("m{}", i + 1));
        write!(content, "<marker id='m{i}'>{inner}</marker>").unwrap();
    }
    content.push_str(&path("m0"));
    assert!(elements(&content) > 100_000.0);
    // Arrowheads (with markerWidth) do not nest.
    let mut content = String::new();
    for i in 0..6 {
        write!(
            content,
            "<marker id='m{i}' markerWidth='4' markerHeight='4'><path d='M0 0 L1 1 L2 2'/></marker>"
        )
        .unwrap();
    }
    content.push_str(&"<line x2='5' marker-end='url(#m0)'/>".repeat(20));
    assert!(elements(&content) < 1000.0);
    // A marker property in a style sheet in any namespace can nest them.
    let css = "<style xmlns='urn:x'>path { marker-mid: url(#m1) }</style>";
    let mut content = format!("{css}{content}");
    content.push_str(&path("m0"));
    assert!(elements(&content) > 100_000.0);
}

#[test]
fn arcs_and_radii_count() {
    // The move and the curves of one arc.
    assert_eq!(path_segments("M0 0 A10 10 0 1 0 1 1"), 9.0);
    assert!(path_segments("M0 0 A1e30 1e30 0 1 0 1 1") > 100_000.0);
    // The radius grows to the chord when it is too small.
    assert!(path_segments("M0 0 a1 1 0 1 0 1e30 0") > 100_000.0);
    assert!(segments("<circle r='1e38'/>") > 1e6);
    assert!(segments("<rect width='1' height='1' rx='1e38'/>") > 1e6);
    assert!(segments("<ellipse rx='1' ry='1e38'/>") > 1e6);
    assert!(segments("<circle r='10'/>") < 20.0);
    // Percentages resolve against the viewport.
    assert!(segments("<circle r='40%'/><rect rx='10%'/>") < 40.0);
    // em with a font size can be anything.
    assert!(segments("<g font-size='1e30'><circle r='1e8em'/></g>").is_infinite());
    assert!(segments("<circle r='2em'/>") < 20.0);
}

#[test]
fn path_segments_follow_relative_commands() {
    assert_eq!(path_segments("M1 1 h10 v10 z"), 4.0);
    assert_eq!(path_segments("m1 1 10 10 20 20"), 3.0);
}

#[test]
fn large_data_images_count() {
    let href = "x".repeat(100_000);
    let image = format!("<image href='data:image/png;base64,{href}'/>");
    assert!(elements(&image) > 3000.0);
    // The plain href counts, as usvg uses it.
    let decoy = format!("<image xlink:href='data:,' href='data:image/png;base64,{href}'/>");
    assert!(elements(&decoy) > 3000.0);
}

#[test]
fn quadratic_scans_are_limited() {
    // Masks that each link a mask with many children.
    let big = format!("<mask id='big'>{}</mask>", "<rect/>".repeat(1000));
    let links = "<mask><rect mask='url(#big)'/></mask>".repeat(1000);
    assert!(measure_content(&format!("{big}{links}")).is_err());
}

#[test]
fn url_ids_are_parsed_as_in_svgtypes() {
    let ids: Vec<&str> = url_ids("url(#a) url( '#b c ' ) url(c) url(\"#d\") url(#e\tf)").collect();
    assert_eq!(ids, ["a", "b c", "d", "e\tf"]);
}
