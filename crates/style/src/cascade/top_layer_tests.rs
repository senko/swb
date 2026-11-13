//! The style of the top layer (ADR 0037 section 8, sub-feature 6b):
//! `::backdrop` rules and inheritance, the `position` fix-up of elements
//! in the top layer, and the `overlay` property. The values are those that
//! Chromium 148 measured in the cases of `tools/probes/top-layer.json`
//! named in each test.

use swb_css::MediaEnvironment;
use swb_dom::{Document, ToggleState, TopLayerHost, parse_html};

use super::*;
use crate::values::Position;
use crate::values::*;

/// A host of the popover algorithms that fires no events.
struct Silent<'a>(&'a mut Document);

impl TopLayerHost for Silent<'_> {
    fn doc(&mut self) -> &mut Document {
        self.0
    }

    fn fire_beforetoggle(
        &mut self,
        _: NodeId,
        _: ToggleState,
        _: ToggleState,
        _: bool,
        _: Option<NodeId>,
    ) -> Option<bool> {
        Some(true)
    }

    fn queue_toggle_task(&mut self, element: NodeId, serial: u64) {
        let _ = self.0.take_toggle(element, serial);
    }
}

fn node(doc: &Document, id: &str) -> NodeId {
    doc.element_by_id(id)
        .unwrap_or_else(|| panic!("no element #{id}"))
}

/// Parses `html`, shows the popovers `open` in order and styles the
/// document with the author sheet `css`.
fn render(html: &str, css: &str, open: &[&str]) -> (Document, StyleMap) {
    let mut doc = parse_html(&format!("<!DOCTYPE html>{html}"));
    for id in open {
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

fn style<'a>(doc: &Document, map: &'a StyleMap, id: &str) -> &'a ComputedStyle {
    map.get(node(doc, id))
        .unwrap_or_else(|| panic!("#{id} has no style"))
}

fn backdrop<'a>(doc: &Document, map: &'a StyleMap, id: &str) -> Option<&'a Arc<ComputedStyle>> {
    map.pseudo(node(doc, id), PseudoKind::Backdrop)
}

#[test]
fn backdrop_rules_apply_only_in_the_top_layer() {
    // `popover-open-ua`: the user-agent backdrop of a popover.
    let (doc, map) = render(
        "<div popover id=p>p</div><div popover id=c>c</div><p id=x>x</p>",
        "",
        &["p"],
    );
    let b = backdrop(&doc, &map, "p").expect("a backdrop");
    assert_eq!(b.display, Display::Block);
    assert_eq!(b.position, Position::Fixed);
    assert_eq!(b.pointer_events, PointerEvents::None);
    assert_eq!(b.background_color, Color::Rgba(Rgba::TRANSPARENT));
    assert_eq!(b.overlay, Overlay::None);
    // A closed popover and an element outside the top layer have none.
    assert!(backdrop(&doc, &map, "c").is_none());
    assert!(backdrop(&doc, &map, "x").is_none());
}

#[test]
fn author_backdrop_rules_and_inheritance() {
    // `backdrop-author` and `backdrop-inheritance`, with a popover.
    let (doc, map) = render(
        "<div popover=manual id=p>p</div><div popover=manual id=q>q</div>",
        "#p { color: rgb(1, 1, 1); font-size: 30px; --v: 7px }\
         #p::backdrop { background: rgb(0, 255, 0); inset: auto; top: 300px;\
           border: solid currentcolor; border-width: 1em; padding: var(--v) }\
         :popover-open::backdrop { width: 40px }\
         #q::backdrop { display: none }",
        &["p", "q"],
    );
    let b = backdrop(&doc, &map, "p").expect("a backdrop");
    assert_eq!(b.background_color, Color::Rgba(Rgba::rgb(0, 255, 0)));
    assert_eq!(b.color, Rgba::rgb(1, 1, 1));
    assert_eq!(b.font_size, 30.0);
    assert_eq!(b.border_top_width, 30.0);
    assert_eq!(
        b.border_top_color.resolve(b.color),
        Rgba::rgb(1, 1, 1),
        "currentcolor is the element's color"
    );
    assert_eq!(b.padding_top, LengthPercentage::Px(7.0));
    assert_eq!(b.top, LengthPercentageOrAuto::px(300.0));
    assert_eq!(b.left, LengthPercentageOrAuto::Auto);
    assert_eq!(b.width, Size::LengthPercentage(LengthPercentage::Px(40.0)));
    // `display: none`: no box, so no style in the map.
    assert!(backdrop(&doc, &map, "q").is_none());
}

#[test]
fn top_layer_position_fix_up() {
    // `popover-author-position`: `static` and `relative` compute to
    // `absolute`; `absolute` and `fixed` stay; a span is blockified.
    let (doc, map) = render(
        "<div id=w style='position: relative'>\
           <div popover=manual id=p style='position: absolute'>p</div>\
           <div popover=manual id=q style='position: static'>q</div>\
           <div popover=manual id=r style='position: relative'>r</div>\
           <span popover=manual id=s>s</span>\
           <div popover=manual id=c style='position: static'>closed</div></div>",
        "[popover] { display: block }",
        &["p", "q", "r", "s"],
    );
    assert_eq!(style(&doc, &map, "p").position, Position::Absolute);
    assert_eq!(style(&doc, &map, "q").position, Position::Absolute);
    assert_eq!(style(&doc, &map, "r").position, Position::Absolute);
    assert_eq!(style(&doc, &map, "s").position, Position::Fixed);
    assert_eq!(style(&doc, &map, "s").display, Display::Block);
    // Outside the top layer, nothing changes (`popover-author-display`).
    assert_eq!(style(&doc, &map, "c").position, Position::Static);
}

#[test]
fn overlay_is_auto_in_the_top_layer() {
    // `popover-closed`, `popover-open-ua`, `overlay-and-states`.
    let (doc, map) = render(
        "<div popover id=p>p</div><div popover id=c>c</div>\
         <details id=x open><summary>s</summary>t</details>",
        "#p, #c { overlay: none }",
        &["p"],
    );
    assert_eq!(style(&doc, &map, "p").overlay, Overlay::Auto);
    assert_eq!(style(&doc, &map, "x").overlay, Overlay::None);
    let (doc, map) = render("<div popover id=c>c</div>", "#c { display: block }", &[]);
    assert_eq!(style(&doc, &map, "c").overlay, Overlay::None);
    // An author value computes (it has no effect).
    let (doc, map) = render("<p id=a>a</p>", "#a { overlay: auto }", &[]);
    assert_eq!(style(&doc, &map, "a").overlay, Overlay::Auto);
}

/// The style of `getComputedStyle(#id, '::backdrop')` with the author
/// sheet `css` and the popovers `open` shown.
fn computed_backdrop(html: &str, css: &str, open: &[&str], id: &str) -> ComputedStyle {
    let (doc, map) = render(html, css, open);
    let element = node(&doc, id);
    let base = Url::parse("https://example.com/").expect("valid URL");
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base);
    let mut chain: Vec<(NodeId, Arc<ComputedStyle>)> = std::iter::once(element)
        .chain(doc.ancestors(element))
        .filter_map(|n| map.get(n).map(|s| (n, Arc::clone(s))))
        .collect();
    chain.reverse();
    compute_pseudo_element_style(
        &doc,
        &stylist,
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base,
        &chain,
        PseudoKind::Backdrop,
    )
    .expect("a style")
}

#[test]
fn the_backdrop_of_an_element_outside_the_top_layer() {
    // `backdrop-popover` #3: the user-agent rules do not apply, author
    // rules do; `position` is `absolute`, `display` `block`.
    let b = computed_backdrop(
        "<div popover id=c>c</div>",
        "#c { display: block } #c::backdrop { background: rgb(255, 255, 0) }",
        &[],
        "c",
    );
    assert_eq!(b.position, Position::Absolute);
    assert_eq!(b.display, Display::Block);
    assert_eq!(b.top, LengthPercentageOrAuto::Auto);
    assert_eq!(b.pointer_events, PointerEvents::Auto);
    assert_eq!(b.background_color, Color::Rgba(Rgba::rgb(255, 255, 0)));
}

#[test]
fn getcomputedstyle_of_a_backdrop_uses_its_rules() {
    // `compute_pseudo_element_style` as `getComputedStyle(el, '::backdrop')`.
    let b = computed_backdrop(
        "<div popover id=p>p</div>",
        "#p { color: rgb(9, 9, 9) } #p::backdrop { top: 5px; position: static }",
        &["p"],
        "p",
    );
    assert_eq!(b.top, LengthPercentageOrAuto::px(5.0));
    assert_eq!(b.color, Rgba::rgb(9, 9, 9));
    // `static` computes to `absolute`; the user-agent `inset: 0` applies.
    assert_eq!(b.position, Position::Absolute);
    assert_eq!(b.left, LengthPercentageOrAuto::ZERO);
}

#[test]
fn display_contents_of_a_top_layer_element_computes_to_block() {
    // `top-layer-display-contents`: Chromium computes `block`; an element
    // outside the top layer keeps `contents`.
    let (doc, map) = render(
        "<div popover=manual id=p style='display: contents'>p</div>\
         <div id=x style='display: contents'>x</div>",
        "",
        &["p"],
    );
    assert_eq!(style(&doc, &map, "p").display, Display::Block);
    assert_eq!(style(&doc, &map, "x").display, Display::Contents);
}

#[test]
fn equal_neighbours_in_the_top_layer_share_their_styles() {
    // Nested popovers have different parent styles, so the sharing cache
    // does not join them; the styles of the elements and of the backdrops
    // that are equal to the previous ones are shared (memory of the
    // hostile case `popover-nested-auto-10000`). A different value is not.
    let (doc, map) = render(
        "<div popover=manual id=a><div popover=manual id=b><div popover=manual id=c>\
         <div popover=manual id=d></div></div></div></div>",
        "#c { color: red } #c::backdrop { background: red }",
        &["a", "b", "c", "d"],
    );
    let element = |id| map.get(node(&doc, id)).expect("a style");
    let back = |id| backdrop(&doc, &map, id).expect("a backdrop");
    assert!(Arc::ptr_eq(element("a"), element("b")));
    assert!(Arc::ptr_eq(back("a"), back("b")));
    assert!(!Arc::ptr_eq(element("b"), element("c")));
    assert!(!Arc::ptr_eq(back("b"), back("c")));
    assert_eq!(element("c").color, Rgba::rgb(255, 0, 0));
    assert_eq!(
        back("c").background_color,
        Color::Rgba(Rgba::rgb(255, 0, 0))
    );
    // `d` is equal to `b` in value but not the last one before it.
    assert_eq!(element("d").background_color, element("b").background_color);
    assert_eq!(back("d").background_color, back("b").background_color);
}
