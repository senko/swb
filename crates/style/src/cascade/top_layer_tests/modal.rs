//! The inert flag of modal dialogs (ADR 0037 section 7, sub-feature 6e): the
//! document is inert except for the topmost modal dialog and its flat tree
//! descendants. The values follow the rule of HTML "blocked by a modal
//! dialog"; the probe cases `inert-click` and `top-layer-hit-order` measure
//! what it does in Chromium.

use super::test_util::compute_element_styles;
use super::*;

/// Parses `html`, shows the popovers `open` and then the modal dialogs
/// `modals` in order, and styles the document. `then` shows more popovers
/// after the dialogs.
fn render_modal(
    html: &str,
    open: &[&str],
    modals: &[&str],
    then: &[&str],
    css: &str,
) -> (Document, StyleMap) {
    let mut doc = parse_html(&format!("<!DOCTYPE html>{html}"));
    for id in open {
        let element = node(&doc, id);
        swb_dom::show_popover(&mut Silent(&mut doc), element, true, None).expect("shows");
    }
    for id in modals {
        let element = node(&doc, id);
        swb_dom::show_modal(&mut Silent(&mut doc), element, None).expect("shows");
    }
    for id in then {
        let element = node(&doc, id);
        swb_dom::show_popover(&mut Silent(&mut doc), element, true, None).expect("shows");
    }
    let base = Url::parse("https://example.com/").expect("valid URL");
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base);
    let map = compute_styles(
        &doc,
        &stylist,
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base,
    );
    (doc, map)
}

fn inert(doc: &Document, map: &StyleMap, id: &str) -> bool {
    style(doc, map, id).modal_inert
}

#[test]
fn a_modal_dialog_makes_the_rest_of_the_document_inert() {
    let (doc, map) = render_modal(
        "<button id=out>o</button><div id=wrap><dialog id=d><button id=in>i</button>\
         <span id=s>t</span></dialog><p id=after>a</p></div>",
        &[],
        &["d"],
        &[],
        "",
    );
    let root = doc.document_element().expect("a root");
    assert!(map.get(root).is_some_and(|s| s.modal_inert));
    for id in ["out", "wrap", "after"] {
        assert!(inert(&doc, &map, id), "#{id} is inert");
    }
    for id in ["d", "in", "s"] {
        assert!(!inert(&doc, &map, id), "#{id} is not inert");
    }
    // The backdrop belongs to the dialog.
    assert!(!backdrop(&doc, &map, "d").expect("a backdrop").modal_inert);
}

#[test]
fn without_a_modal_dialog_nothing_is_inert() {
    let (doc, map) = render_modal(
        "<button id=out>o</button><dialog id=d open><b id=b>x</b></dialog>\
         <div popover=manual id=p>p</div>",
        &["p"],
        &[],
        &[],
        "",
    );
    let root = doc.document_element().expect("a root");
    assert!(map.get(root).is_some_and(|s| !s.modal_inert));
    for id in ["out", "d", "b", "p"] {
        assert!(!inert(&doc, &map, id), "#{id} is not inert");
    }
}

#[test]
fn popovers_are_inert_unless_they_are_in_the_dialog() {
    // `top-layer-hit-order` rows 5 to 7: a popover outside the dialog is
    // inert whether it was shown before or after; one inside is not; the
    // modal dialog below the topmost one is inert.
    let (doc, map) = render_modal(
        "<div popover=manual id=before>b</div><dialog id=d1>1</dialog>\
         <dialog id=d2><div popover=manual id=in>i</div></dialog>\
         <div popover=manual id=after>a</div>",
        &["before"],
        &["d1", "d2"],
        &["in", "after"],
        "",
    );
    assert!(inert(&doc, &map, "before"));
    assert!(inert(&doc, &map, "after"));
    assert!(inert(&doc, &map, "d1"));
    assert!(!inert(&doc, &map, "d2"));
    assert!(!inert(&doc, &map, "in"));
    // The backdrop of an inert top-layer element is inert.
    assert!(
        backdrop(&doc, &map, "after")
            .expect("a backdrop")
            .modal_inert
    );
    assert!(!backdrop(&doc, &map, "in").expect("a backdrop").modal_inert);
}

#[test]
fn pseudo_elements_inherit_the_flag() {
    let (doc, map) = render_modal(
        "<p id=x>x</p><dialog id=d><p id=y>y</p></dialog>",
        &[],
        &["d"],
        &[],
        "#x::before, #y::before { content: 'a' }",
    );
    let before = |id| {
        map.pseudo(node(&doc, id), PseudoKind::Before)
            .expect("a box")
    };
    assert!(before("x").modal_inert);
    assert!(!before("y").modal_inert);
}

#[test]
fn style_sharing_keeps_the_flag_apart() {
    // Equal siblings share one style; the children of the root and of the
    // dialog have different parent styles, so equal rules do not join an
    // inert element and a free one.
    let (doc, map) = render_modal(
        "<p id=a>a</p><p id=b>b</p><dialog id=d><p id=c>c</p><p id=e>e</p></dialog>",
        &[],
        &["d"],
        &[],
        "",
    );
    let get = |id| map.get(node(&doc, id)).expect("a style");
    assert!(Arc::ptr_eq(get("a"), get("b")));
    assert!(Arc::ptr_eq(get("c"), get("e")));
    assert!(!Arc::ptr_eq(get("a"), get("c")));
    assert!(get("a").modal_inert && !get("c").modal_inert);
}

#[test]
fn share_last_keeps_the_flag_apart() {
    // Two popovers with equal values, one inert and one in the dialog: the
    // top layer's `share_last` does not join them.
    let (doc, map) = render_modal(
        "<div popover=manual id=p>p</div><dialog id=d><div popover=manual id=q>q</div></dialog>",
        &["p"],
        &["d"],
        &["q"],
        "",
    );
    let get = |id| map.get(node(&doc, id)).expect("a style");
    assert!(get("p").modal_inert && !get("q").modal_inert);
    assert!(!Arc::ptr_eq(get("p"), get("q")));
}

#[test]
fn on_demand_styles_carry_the_flag() {
    // `compute_element_styles` (getComputedStyle) sets the same flag.
    let (doc, _) = render_modal(
        "<p id=x>x</p><dialog id=d><p id=y>y</p></dialog>",
        &[],
        &["d"],
        &[],
        "",
    );
    let base = Url::parse("https://example.com/").expect("valid URL");
    let stylist = Stylist::new(doc.quirks_mode);
    let of = |id: &str| {
        compute_element_styles(
            &doc,
            &stylist,
            &MediaEnvironment::default(),
            &ElementStates::default(),
            &base,
            node(&doc, id),
        )
        .last()
        .map(|(_, s)| s.modal_inert)
    };
    assert_eq!(of("x"), Some(true));
    assert_eq!(of("y"), Some(false));
}
