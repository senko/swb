//! `@property`, `color-mix()` with `currentcolor`, `field-sizing` and
//! `content-visibility` in the cascade (ADR 0038, section "7b"). The
//! expected values are those of Chromium 148 in the cases of
//! `tools/probes/css-small-features.json` named in each test.

use swb_css::MediaEnvironment;
use swb_dom::{Document, parse_html};

use super::test_util::*;
use super::*;
use crate::values::{ContentVisibility, FieldSizing, LengthPercentage, Rgba};

fn render(html: &str, css: &str) -> (Document, StyleMap) {
    let doc = parse_html(&format!("<!DOCTYPE html>{html}"));
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
    let map = compute_styles(
        &doc,
        &stylist,
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base(),
    );
    (doc, map)
}

fn style<'a>(doc: &Document, map: &'a StyleMap, id: &str) -> &'a ComputedStyle {
    map.get(node(doc, id))
        .unwrap_or_else(|| panic!("#{id} has no style"))
}

/// The computed value of custom property `name` of `#id`, as text.
fn custom(doc: &Document, map: &StyleMap, id: &str, name: &str) -> Option<String> {
    style(doc, map, id)
        .custom_properties
        .as_ref()
        .and_then(|props| props.get(name))
        .map(|tokens| swb_css::serialize_component_values(tokens))
}

const LENGTH: &str = "@property --w { syntax: '<length>'; inherits: false; initial-value: 5px }";

#[test]
fn a_registered_property_has_its_initial_value_and_a_computed_value() {
    // `ap-length`, `ap-var-use`.
    let (doc, map) = render(
        "<div id=a></div><div id=b></div><div id=c></div><div id=d></div>",
        &format!(
            "{LENGTH} #b {{ --w: 20px }} #c {{ --w: 10 }} #d {{ --w: 2em; font-size: 10px; \
             width: var(--w) }}"
        ),
    );
    assert_eq!(custom(&doc, &map, "a", "--w").as_deref(), Some("5px"));
    assert_eq!(custom(&doc, &map, "b", "--w").as_deref(), Some("20px"));
    // A value that does not match the syntax is the initial value.
    assert_eq!(custom(&doc, &map, "c", "--w").as_deref(), Some("5px"));
    // `em` is computed with the element's own font size.
    assert_eq!(custom(&doc, &map, "d", "--w").as_deref(), Some("20px"));
    assert_eq!(
        style(&doc, &map, "d").width,
        crate::values::Size::LengthPercentage(LengthPercentage::Px(20.0))
    );
}

#[test]
fn inheritance_follows_the_inherits_descriptor() {
    // `ap-inheritance`.
    let (doc, map) = render(
        "<div id=p><div id=k><div id=g></div></div></div>",
        "@property --a { syntax: '<length>'; inherits: false; initial-value: 1px }\
         @property --b { syntax: '<length>'; inherits: true; initial-value: 2px }\
         #p { --a: 10px; --b: 20px; --c: 30px }",
    );
    for (id, a, b, c) in [
        ("p", "10px", "20px", Some("30px")),
        ("k", "1px", "20px", Some("30px")),
        ("g", "1px", "20px", Some("30px")),
    ] {
        assert_eq!(custom(&doc, &map, id, "--a").as_deref(), Some(a), "#{id}");
        assert_eq!(custom(&doc, &map, id, "--b").as_deref(), Some(b), "#{id}");
        assert_eq!(custom(&doc, &map, id, "--c").as_deref(), c, "#{id}");
    }
    // The root's parent is the initial values.
    let (doc, map) = render(
        "<p id=p></p>",
        "@property --b { syntax: '<number>'; inherits: true; initial-value: 7 }",
    );
    assert_eq!(custom(&doc, &map, "p", "--b").as_deref(), Some("7"));
}

#[test]
fn a_registered_value_with_var_is_computed_after_substitution() {
    // `ap-var-use`: `--w: var(--u)` with `--u: 8` is not a length.
    let (doc, map) = render(
        "<div id=j></div><div id=k></div><div id=f></div>",
        &format!(
            "{LENGTH} #j {{ --w: var(--u); --u: 8px }} #k {{ --w: var(--u); --u: 8 }}\
             #f {{ --w: var(--missing) }}"
        ),
    );
    assert_eq!(custom(&doc, &map, "j", "--w").as_deref(), Some("8px"));
    assert_eq!(custom(&doc, &map, "k", "--w").as_deref(), Some("5px"));
    assert_eq!(custom(&doc, &map, "f", "--w").as_deref(), Some("5px"));
}

#[test]
fn a_universal_registration_without_initial_value_is_guaranteed_invalid() {
    // `ap-universal-guaranteed-invalid`: a failed `var()` gives no value
    // (a typed property would take its initial value).
    let (doc, map) = render(
        "<div id=a></div><div id=b></div>",
        "@property --u { syntax: '*'; inherits: false } #b { --u: var(--missing) }\
         #a { width: var(--u, 30px) }",
    );
    assert_eq!(custom(&doc, &map, "a", "--u"), None);
    assert_eq!(custom(&doc, &map, "b", "--u"), None);
    assert_eq!(
        style(&doc, &map, "a").width,
        crate::values::Size::LengthPercentage(LengthPercentage::Px(30.0))
    );
}

#[test]
fn rules_that_are_not_valid_or_do_not_apply_register_nothing() {
    // `ap-invalid-*`, `ap-in-media-false`, `ap-duplicate-later-*`.
    let (doc, map) = render(
        "<div id=a style='--w: 7px'></div>",
        "@property --w { syntax: '<length>'; inherits: false }\
         @media not all { @property --w { syntax: '<length>'; inherits: false; initial-value: 1px } }",
    );
    assert_eq!(custom(&doc, &map, "a", "--w").as_deref(), Some("7px"));
    let (doc, map) = render(
        "<div id=a style='--w: 7'></div>",
        "@property --w { syntax: '<length>'; inherits: false; initial-value: 1px }\
         @property --w { syntax: '<number>'; inherits: false }",
    );
    assert_eq!(custom(&doc, &map, "a", "--w").as_deref(), Some("1px"));
    // The later valid rule wins.
    let (doc, map) = render(
        "<div id=a></div>",
        "@property --w { syntax: '<length>'; inherits: false; initial-value: 1px }\
         @property --w { syntax: '<number>'; inherits: false; initial-value: 2 }",
    );
    assert_eq!(custom(&doc, &map, "a", "--w").as_deref(), Some("2"));
}

#[test]
fn a_registered_color_keeps_currentcolor() {
    // `ap-color-use-currentcolor`: the computed value of `--c` is the
    // keyword; the element that uses it supplies its own color.
    let (doc, map) = render(
        "<div id=p><div id=k></div></div>",
        "@property --c { syntax: '<color>'; inherits: true; initial-value: red }\
         #p { color: blue; --c: currentcolor } #k { color: green; background-color: var(--c) }",
    );
    assert_eq!(
        custom(&doc, &map, "k", "--c").as_deref(),
        Some("currentcolor")
    );
    assert_eq!(
        style(&doc, &map, "k")
            .background_color
            .resolve(style(&doc, &map, "k").color),
        Rgba::rgb(0, 128, 0)
    );
}

#[test]
fn an_inherited_color_mix_follows_the_color_of_the_element() {
    // `cm-inheritance`: `border-color` is not inherited, but `inherit`
    // passes the mix; the used color is the element's own.
    let (doc, map) = render(
        "<div id=p><div id=m></div><div id=k></div></div>",
        "#p { color: rgb(255, 0, 0); \
           border: 4px solid; border-color: color-mix(in srgb, currentcolor 50%, transparent) }\
         #m { color: rgb(0, 128, 0); border: 4px solid; border-color: inherit }\
         #k { color: rgb(0, 0, 255); border: 4px solid; border-color: color-mix(in srgb, currentcolor 50%, transparent) }",
    );
    let used = |id: &str| {
        let s = style(&doc, &map, id);
        s.border_top_color.resolve(s.color)
    };
    assert_eq!(used("p"), Rgba::new(255, 0, 0, 128));
    assert_eq!(used("m"), Rgba::new(0, 128, 0, 128));
    assert_eq!(used("k"), Rgba::new(0, 0, 255, 128));
}

#[test]
fn the_color_property_keeps_the_float_color_of_a_mix() {
    // `cm-inheritance-color-self`: `currentcolor` in the `color` property
    // is the exact color of the parent.
    let (doc, map) = render(
        "<div id=p><div id=k><div id=k2></div></div></div>",
        "#p { color: rgb(255, 0, 0) }\
         #k, #k2 { color: color-mix(in srgb, currentcolor 50%, white) }",
    );
    let text = |id: &str| {
        let s = style(&doc, &map, id);
        s.color_exact
            .map(|e| crate::values::Color::Special(e).resolved_text(s.color))
    };
    assert_eq!(text("p"), None);
    assert_eq!(text("k").as_deref(), Some("color(srgb 1 0.5 0.5)"));
    assert_eq!(text("k2").as_deref(), Some("color(srgb 1 0.75 0.75)"));
}

#[test]
fn field_sizing_and_content_visibility_are_computed_not_inherited() {
    // `fs-computed-values`, `fs-inheritance`, `cv-computed`.
    let (doc, map) = render(
        "<div id=p><div id=k></div></div><div id=h></div><div id=a></div><div id=x></div>",
        "#p { field-sizing: content; content-visibility: hidden } \
         #h { content-visibility: hidden } #a { content-visibility: AUTO } \
         #x { content-visibility: foo; field-sizing: foo }",
    );
    assert_eq!(style(&doc, &map, "p").field_sizing, FieldSizing::Content);
    assert_eq!(style(&doc, &map, "k").field_sizing, FieldSizing::Fixed);
    assert_eq!(
        style(&doc, &map, "k").content_visibility,
        ContentVisibility::Visible
    );
    assert!(style(&doc, &map, "h").skips_contents());
    assert!(style(&doc, &map, "h").has_size_containment());
    assert!(style(&doc, &map, "h").has_layout_containment());
    assert!(style(&doc, &map, "h").has_paint_containment());
    // `auto` has layout and paint containment but no size containment while
    // the box is relevant.
    let auto = style(&doc, &map, "a");
    assert_eq!(auto.content_visibility, ContentVisibility::Auto);
    assert!(!auto.skips_contents() && !auto.has_size_containment());
    assert!(auto.has_layout_containment() && auto.has_paint_containment());
    assert_eq!(
        style(&doc, &map, "x").content_visibility,
        ContentVisibility::Visible
    );
}

#[test]
fn layout_containment_does_not_apply_to_inline_boxes() {
    // `ct-containing-block` (`g`): a non-atomic inline.
    let (doc, map) = render(
        "<span id=i></span><div id=d></div>",
        "#i, #d { contain: layout } #d { display: inline-block }",
    );
    assert!(!style(&doc, &map, "i").has_layout_containment());
    assert!(style(&doc, &map, "d").has_layout_containment());
}

#[test]
fn a_failed_var_in_a_universal_registration_is_not_its_initial_value() {
    // `ap-universal`: `--p: var(--u)` with `--u` undefined is "" even though
    // the initial value is `x`; a declared value that is valid is kept.
    let (doc, map) = render(
        "<div id=a></div><div id=b></div><div id=c></div>",
        "@property --p { syntax: '*'; inherits: false; initial-value: x }\
         #b { --p: var(--u) } #c { --p: anything }",
    );
    assert_eq!(custom(&doc, &map, "a", "--p").as_deref(), Some("x"));
    assert_eq!(custom(&doc, &map, "b", "--p"), None);
    assert_eq!(custom(&doc, &map, "c", "--p").as_deref(), Some("anything"));
}
