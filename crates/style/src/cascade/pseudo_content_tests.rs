//! The `content` flag of rules: `::before` and `::after` get a cascade
//! only if a matched rule can set `content` (ADR 0007, "The `content`
//! flag"). The results must equal those of a cascade for every
//! pseudo-element.

use swb_css::MediaEnvironment;
use swb_dom::{Document, parse_html};

use super::test_util::*;
use super::*;
use crate::Rgba;
use crate::content::content_text;

fn render(html: &str, css: &str) -> (Document, StyleMap) {
    let doc = parse_html(&format!("<!DOCTYPE html>{html}"));
    let stylist = stylist(&doc, css);
    let map = compute_styles(
        &doc,
        &stylist,
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base(),
    );
    (doc, map)
}

/// The text of the `::before` of `#id`, or `None` if it has no box.
fn before_text(doc: &Document, map: &StyleMap, id: &str) -> Option<String> {
    let style = map.pseudo(node(doc, id), PseudoKind::Before)?;
    Some(content_text(style).unwrap_or_default())
}

/// The flags of the author rules of `css`, one per selector.
fn flags(css: &str) -> Vec<bool> {
    let doc = parse_html("<!DOCTYPE html>");
    let user_agent_rules = Stylist::new(doc.quirks_mode).len();
    let stylist = stylist(&doc, css);
    (user_agent_rules..stylist.len())
        .map(|i| {
            stylist
                .rule(u32::try_from(i).expect("few rules"))
                .sets_content
        })
        .collect()
}

#[test]
fn rules_that_can_set_content_have_the_flag() {
    let css = "\
        a::before { content: 'x' }\
        b::before { color: red }\
        i::before { content: 'x' !important }\
        u::before { color: red; content: none }\
        s::before { all: unset }\
        em::before { all: initial; color: red }\
        p::before { content: inherit }\
        q::before { content: var(--c) }\
        @layer base { span::before { content: 'l' } div::before { margin: 0 } }\
        .a::before, .b { content: 'one'; color: red }\
        .c::after { --content: 'x'; color: red }";
    // One flag per selector, in order.
    assert_eq!(
        flags(css),
        [
            true, false, true, true, true, true, true, true, true, false, true, true, false
        ]
    );
}

#[test]
fn the_user_agent_rule_of_q_sets_content() {
    let stylist = Stylist::new(swb_dom::QuirksMode::NoQuirks);
    assert!(stylist.has_pseudo_rules(PseudoKind::Before));
    let flagged = (0..stylist.len())
        .filter(|&i| {
            stylist
                .rule(u32::try_from(i).expect("few rules"))
                .sets_content
        })
        .count();
    assert!(
        flagged >= 2,
        "q::before and q::after of the UA sheet: {flagged}"
    );
}

#[test]
fn content_in_a_matched_rule_makes_a_box() {
    let (doc, map) = render(
        "<p id=a>a</p><p id=b>b</p><p id=c>c</p><p id=d>d</p><p id=e>e</p><q id=q>q</q>",
        "#a::before { content: 'A' } #b::before { color: red }\
         #c::before { content: 'C' !important } #c::before { content: 'x' }\
         #d::before { content: none } #e::before { content: normal; color: red }",
    );
    assert_eq!(before_text(&doc, &map, "a").as_deref(), Some("A"));
    // A rule without `content`: `content` is `normal`, no box, no cascade.
    assert_eq!(before_text(&doc, &map, "b"), None);
    assert_eq!(before_text(&doc, &map, "c").as_deref(), Some("C"));
    assert_eq!(before_text(&doc, &map, "d"), None);
    assert_eq!(before_text(&doc, &map, "e"), None);
    // `q::before` from the user-agent sheet.
    assert_eq!(before_text(&doc, &map, "q").as_deref(), Some("\u{201C}"));
}

#[test]
fn an_important_rule_that_loses_still_decides_through_the_winner() {
    // The rule with `content` is flagged, whichever declaration wins: the
    // cascade runs and finds `none` or the string.
    let (doc, map) = render(
        "<p id=a>a</p><p id=b>b</p>",
        "#a::before { content: 'x' } p::before { content: none !important }\
         #b::before { content: 'y' !important } p::before { content: none }",
    );
    assert_eq!(before_text(&doc, &map, "a"), None);
    assert_eq!(before_text(&doc, &map, "b").as_deref(), Some("y"));
}

#[test]
fn content_in_a_layer_and_all_are_found() {
    let (doc, map) = render(
        "<p id=a>a</p><p id=b>b</p><p id=c>c</p><p id=d>d</p>",
        "@layer one { #a::before { content: 'L' } }\
         #b::before { all: unset } #b::before { color: red }\
         #c::before { content: 'C' } #c::before { all: unset }\
         @layer two { #d::before { content: 'D' } } @layer one { #d::before { content: none } }",
    );
    assert_eq!(before_text(&doc, &map, "a").as_deref(), Some("L"));
    // `all: unset` sets `content: unset`, which is `normal`.
    assert_eq!(before_text(&doc, &map, "b"), None);
    assert_eq!(before_text(&doc, &map, "c"), None);
    assert_eq!(before_text(&doc, &map, "d").as_deref(), Some("D"));
}

#[test]
fn inherit_and_variables_in_content() {
    // `content` is not inherited, but `content: inherit` takes the
    // element's own value (here a rule's `content` on the element, and
    // `normal` for #b).
    let (doc, map) = render(
        "<p id=a>a</p><p id=b>b</p><p id=c>c</p>",
        "#a { content: 'parent' } #a::before { content: inherit }\
         #b::before { content: inherit }\
         #c { --t: 'V' } #c::before { content: var(--t) }",
    );
    assert_eq!(before_text(&doc, &map, "a").as_deref(), Some("parent"));
    assert_eq!(before_text(&doc, &map, "b"), None);
    assert_eq!(before_text(&doc, &map, "c").as_deref(), Some("V"));
}

#[test]
fn after_and_other_pseudo_elements_keep_their_paths() {
    let (doc, map) = render(
        "<ul><li id=li>i</li></ul><input id=in placeholder=p><p id=a>a</p>",
        "#a::after { color: red } #a::before { content: 'x' } li::marker { color: blue }\
         #in::placeholder { color: red }",
    );
    let a = node(&doc, "a");
    assert!(map.pseudo(a, PseudoKind::After).is_none());
    assert!(map.pseudo(a, PseudoKind::Before).is_some());
    // Markers and placeholders have a box whatever their rules are.
    assert!(map.pseudo(node(&doc, "li"), PseudoKind::Marker).is_some());
    assert!(
        map.pseudo(node(&doc, "in"), PseudoKind::Placeholder)
            .is_some()
    );
}

#[test]
fn get_computed_style_of_a_pseudo_element_without_content_is_unchanged() {
    // `getComputedStyle(el, '::before')` runs the cascade also without
    // `content` and for a pseudo-element without a box.
    let doc = parse_html("<!DOCTYPE html><p id=a>a</p><p id=b>b</p><p id=c>c</p>");
    let stylist = stylist(
        &doc,
        "#a::before { color: red; display: block } #b::before { content: 'x'; color: blue }",
    );
    let env = MediaEnvironment::default();
    let states = ElementStates::default();
    for (id, color, display, text) in [
        ("a", Rgba::rgb(255, 0, 0), Display::Block, None),
        ("b", Rgba::rgb(0, 0, 255), Display::Inline, Some("x")),
    ] {
        let chain = compute_element_styles(&doc, &stylist, &env, &states, &base(), node(&doc, id));
        let style = compute_pseudo_element_style(
            &doc,
            &stylist,
            &env,
            &states,
            &base(),
            &chain,
            PseudoKind::Before,
        )
        .expect("a style");
        assert_eq!(style.color, color, "#{id}");
        assert_eq!(style.display, display, "#{id}");
        assert_eq!(content_text(&style).as_deref(), text, "#{id}");
    }
    // No rule at all: the style of a pseudo-element that has no box.
    let chain = compute_element_styles(&doc, &stylist, &env, &states, &base(), node(&doc, "c"));
    let style = compute_pseudo_element_style(
        &doc,
        &stylist,
        &env,
        &states,
        &base(),
        &chain,
        PseudoKind::Before,
    );
    assert!(style.is_some());
}
