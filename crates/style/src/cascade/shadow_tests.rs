//! The flat tree in the style walk (ADR 0036 section 4) and the scope
//! test of the document's author rules (5b).

use swb_css::MediaEnvironment;
use swb_dom::{Document, ShadowRootInit, ShadowRootMode, SlotAssignment, parse_html};

use super::test_util::compute_element_styles;
use super::*;
use crate::values::*;

fn base() -> Url {
    Url::parse("https://example.com/").expect("valid URL")
}

fn node(doc: &Document, id: &str) -> NodeId {
    doc.element_by_id(id)
        .unwrap_or_else(|| panic!("no element #{id}"))
}

/// Attaches an open root to `host` with `markup` as its content and
/// returns the root.
fn shadow(doc: &mut Document, host: NodeId, markup: &str) -> NodeId {
    let init = ShadowRootInit {
        mode: ShadowRootMode::Open,
        slot_assignment: SlotAssignment::Named,
        delegates_focus: false,
        clonable: false,
        serializable: false,
    };
    let root = doc.attach_shadow(host, init, false).expect("a host");
    let fragment = swb_dom::parse_fragment(doc, host, markup).expect("markup fits");
    doc.append_child(root, fragment).expect("a fragment");
    root
}

/// The element with id `id` in the tree of `root`.
fn in_root(doc: &Document, root: NodeId, id: &str) -> NodeId {
    doc.descendants(root)
        .find(|&n| doc.element(n).and_then(ElementData::id) == Some(id))
        .unwrap_or_else(|| panic!("no #{id} in the root"))
}

fn stylist(doc: &Document, css: &str) -> Stylist {
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
    stylist
}

fn styles(doc: &Document, css: &str) -> StyleMap {
    let env = MediaEnvironment::default();
    compute_styles(
        doc,
        &stylist(doc, css),
        &env,
        &ElementStates::default(),
        &base(),
    )
}

/// The style of `node` from [`compute_element_styles`].
fn element_style(doc: &Document, css: &str, node: NodeId) -> Option<Arc<ComputedStyle>> {
    let env = MediaEnvironment::default();
    let chain = compute_element_styles(
        doc,
        &stylist(doc, css),
        &env,
        &ElementStates::default(),
        &base(),
        node,
    );
    chain
        .last()
        .filter(|(n, _)| *n == node)
        .map(|(_, s)| Arc::clone(s))
}

const CSS: &str = "body { font-size: 18px; color: blue } span { color: red; font-size: 40px }\
                   #h { font-size: 20px }";

const BLUE: Rgba = Rgba::rgb(0, 0, 255);
const RED: Rgba = Rgba::rgb(255, 0, 0);

#[test]
fn shadow_children_inherit_from_the_host_and_slotted_ones_from_the_slot() {
    let mut doc =
        parse_html("<!DOCTYPE html><div id=h><i id=x>slotted</i><b id=u slot=none>u</b></div>");
    let host = node(&doc, "h");
    let root = shadow(
        &mut doc,
        host,
        "<span id=in>in</span><p id=p style='font-size:30px'><slot id=s></slot></p>",
    );
    let map = styles(&doc, CSS);
    let span = in_root(&doc, root, "in");
    let p = in_root(&doc, root, "p");
    let slot = in_root(&doc, root, "s");
    // The document's rules do not match the shadow `span`: it inherits
    // from the host.
    let s = map.get(span).expect("a style");
    assert_eq!((s.font_size, s.color), (20.0, BLUE));
    // The `style` attribute applies in a shadow tree; the slot inherits
    // from the `p` and the slotted `i` from the slot.
    assert_eq!(map.get(p).expect("a style").font_size, 30.0);
    assert_eq!(map.get(slot).expect("a style").display, Display::Contents);
    assert_eq!(map.get(node(&doc, "x")).expect("a style").font_size, 30.0);
    // The light child that no slot takes is not in the walk.
    assert!(map.get(node(&doc, "u")).is_none());
}

#[test]
fn light_rules_still_match_slotted_elements() {
    let mut doc = parse_html("<!DOCTYPE html><div id=h><span id=x>x</span></div>");
    let host = node(&doc, "h");
    shadow(&mut doc, host, "<slot></slot>");
    let map = styles(&doc, CSS);
    let x = map.get(node(&doc, "x")).expect("a style");
    assert_eq!((x.font_size, x.color), (40.0, RED));
}

#[test]
fn a_slot_passed_on_stays_in_the_inheritance_chain() {
    // The case `nested-slot-inherit` of tools/probes/shadow-dom.json.
    let mut doc = parse_html("<!DOCTYPE html><div id=h><b id=x>pass</b></div>");
    let host = node(&doc, "h");
    let root = shadow(
        &mut doc,
        host,
        "<div id=ih><slot id=os style='font-size:24px'></slot></div>",
    );
    let inner_host = in_root(&doc, root, "ih");
    shadow(&mut doc, inner_host, "<i><slot></slot></i>");
    let map = styles(&doc, "");
    assert_eq!(map.get(node(&doc, "x")).expect("a style").font_size, 24.0);
    let x = element_style(&doc, "", node(&doc, "x")).expect("a style");
    assert_eq!(x.font_size, 24.0);
}

#[test]
fn element_styles_follow_the_flat_tree() {
    let mut doc = parse_html(
        "<!DOCTYPE html><div id=h><i id=x>x</i><b id=u slot=none><em id=e>e</em></b></div>",
    );
    let host = node(&doc, "h");
    let root = shadow(
        &mut doc,
        host,
        "<p style='font-size:30px'><slot></slot></p><span id=in>s</span>",
    );
    // Slotted: through the slot.
    let x = element_style(&doc, CSS, node(&doc, "x")).expect("a style");
    assert_eq!(x.font_size, 30.0);
    // A shadow element: the document's rules do not match it.
    let span = element_style(&doc, CSS, in_root(&doc, root, "in")).expect("a style");
    assert_eq!((span.font_size, span.color), (20.0, BLUE));
    // Outside the flat tree: the initial style is the parent (measured:
    // case `unslotted-computed-style`), the document's rules match, and
    // the display is not blockified.
    let u = element_style(&doc, "b { color: red }", node(&doc, "u")).expect("a style");
    assert_eq!((u.font_size, u.color), (16.0, RED));
    assert_eq!(u.display, Display::Inline);
    let e = element_style(&doc, "b { font-size: 22px }", node(&doc, "e")).expect("a style");
    assert_eq!(e.font_size, 22.0);
    // A detached host: no styles.
    let x = node(&doc, "x");
    doc.remove(host);
    assert!(element_style(&doc, CSS, x).is_none());
}
