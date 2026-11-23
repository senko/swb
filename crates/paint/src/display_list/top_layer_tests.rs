//! The paint order and the hit regions of the top layer (ADR 0037 section
//! 8, sub-feature 6b): each box of the top layer is a stacking context of
//! its own after the root's, whatever its `z-index`, outside the groups and
//! clips of its DOM ancestors. Chromium 148 measured the same order in the
//! cases `top-layer-paint-order` and `backdrop-popover` of
//! `tools/probes/top-layer.json` and `top-layer-hit-order` of
//! `tools/probes/web-top-layer.json`.

use swb_css::MediaEnvironment;
use swb_dom::{Document, NodeId, ToggleState, TopLayerHost, parse_html};
use swb_layout::{LayoutInput, NoFormControls, NoReplacedSizes, NoScroll, Point, Size, layout};
use swb_style::{ElementStates, Stylist, compute_styles};
use swb_text::FontContext;
use url::Url;

use swb_style::Rgba;

use super::*;

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

/// The display list of `html` in an 800×600 viewport after showing the
/// popovers `open` in order (an id after `-` is hidden, one after `*` is a
/// dialog that opens as a modal dialog), and the document.
fn paint(html: &str, open: &[&str]) -> (Document, DisplayList) {
    let mut doc = parse_html(html);
    for op in open {
        let (hide, id) = op.strip_prefix('-').map_or((false, *op), |id| (true, id));
        let (modal, id) = id.strip_prefix('*').map_or((false, id), |id| (true, id));
        let element = doc.element_by_id(id).expect("a popover");
        let host = &mut Silent(&mut doc);
        if modal {
            swb_dom::show_modal(host, element, None).expect("shows");
        } else if hide {
            swb_dom::hide_popover(host, element, swb_dom::HideOptions::default()).expect("hides");
        } else {
            swb_dom::show_popover(host, element, true, None).expect("shows");
        }
    }
    let base = Url::parse("https://example.com/").expect("valid URL");
    let mut stylist = Stylist::new(doc.quirks_mode);
    let css: String = doc
        .descendants(NodeId::DOCUMENT)
        .filter(|&n| doc.is_html_element(n, &swb_dom::local_name!("style")))
        .map(|n| doc.text_content(n))
        .collect();
    stylist.add_author_sheet(&swb_css::parse_stylesheet(&css), &base);
    let styles = compute_styles(
        &doc,
        &stylist,
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base,
    );
    let mut fonts = FontContext::for_tests();
    let tree = layout(
        &LayoutInput {
            document: &doc,
            styles: &styles,
            viewport: Size::new(800.0, 600.0),
            replaced: &NoReplacedSizes,
            controls: &NoFormControls,
        },
        &mut fonts,
    );
    let scrolling = Scrolling {
        offsets: &NoScroll,
        indicators: false,
        scale: 1.0,
        composited: None,
    };
    let list = build_display_list(&tree, &NoSizes, &NoHighlights, &scrolling);
    (doc, list)
}

struct NoSizes;

impl ImageSizes for NoSizes {
    fn size(&self, _: &ImageRef) -> Option<swb_layout::NaturalSize> {
        None
    }
}

/// The ids of the elements at `(x, y)`, topmost first.
fn ids_at(doc: &Document, list: &DisplayList, x: f32, y: f32) -> Vec<String> {
    list.hit_test_all(Point::new(x, y), Point::default())
        .into_iter()
        .map(|n| {
            doc.element(n).map_or_else(String::new, |e| {
                e.id()
                    .map_or_else(|| e.local_name().to_string(), str::to_owned)
            })
        })
        .collect()
}

/// The index of the hit region of `id` and the number of groups and clips
/// open there.
fn region(doc: &Document, list: &DisplayList, id: &str) -> (usize, i32) {
    let node = doc.element_by_id(id).expect("an element");
    let mut depth = 0;
    for (i, item) in list.items.iter().enumerate() {
        match item {
            DisplayItem::HitRegion { node: n, .. } if *n == node => return (i, depth),
            DisplayItem::PushClip(_)
            | DisplayItem::PushViewportClip
            | DisplayItem::PushOpacity { .. }
            | DisplayItem::PushFilter { .. }
            | DisplayItem::PushMask { .. }
            | DisplayItem::PushSvgClip { .. }
            | DisplayItem::PushTransform { .. } => depth += 1,
            DisplayItem::PopClip
            | DisplayItem::PopOpacity
            | DisplayItem::PopFilter
            | DisplayItem::PopMask
            | DisplayItem::PopSvgClip
            | DisplayItem::PopTransform => depth -= 1,
            _ => {}
        }
    }
    panic!("#{id} has no hit region");
}

const PAGE: &str = "<!DOCTYPE html><style>body { margin: 0 } \
    [popover] { position: absolute; inset: auto; margin: 0; border: 0; padding: 0; width: 100px; \
    height: 100px } \
    #a { left: 0; top: 0; z-index: 5 } #b { left: 50px; top: 50px; z-index: -5 } \
    #z { position: fixed; left: 0; top: 0; width: 400px; height: 300px; z-index: 2147483647 } \
    #w { opacity: 0.5; overflow: hidden; width: 10px; height: 10px; transform: scale(0.5) } \
    #k { position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; z-index: -1 }\
    </style>";

#[test]
fn the_last_top_layer_element_is_on_top_whatever_its_z_index() {
    let html = format!(
        "{PAGE}<div id=z></div><div id=w><div popover=manual id=a>a</div></div>\
         <div popover=manual id=b>b<div id=k></div></div>"
    );
    let (doc, list) = paint(&html, &["a", "b"]);
    // `top-layer-hit-order` #1: `b, a, z` (and the root; the popovers'
    // backdrops have `pointer-events: none`).
    assert_eq!(ids_at(&doc, &list, 95.0, 95.0)[..3], ["b", "a", "z"]);
    assert_eq!(ids_at(&doc, &list, 25.0, 25.0)[..2], ["a", "z"]);
    // The top layer comes after the root's items and outside the opacity,
    // clip and transform of #w; #b after #a.
    let (z, _) = region(&doc, &list, "z");
    let (a, a_depth) = region(&doc, &list, "a");
    let (b, b_depth) = region(&doc, &list, "b");
    assert!(z < a && a < b, "paint order z {z}, a {a}, b {b}");
    assert_eq!((a_depth, b_depth), (0, 0), "no group or clip around them");
    // #k (`z-index: -1`) is in the stacking context of #b: it paints
    // above #b's background.
    assert_eq!(ids_at(&doc, &list, 65.0, 65.0)[..2], ["k", "b"]);
    // Shown again, #a moves to the top (`top-layer-hit-order` #2).
    let (doc, list) = paint(&html, &["a", "b", "-a", "a"]);
    assert_eq!(ids_at(&doc, &list, 95.0, 95.0)[0], "a");
}

#[test]
fn a_fixed_top_layer_box_is_clipped_to_the_viewport_only() {
    // A fixed popover (the user agent's `position: fixed`): one viewport
    // clip around it, as a fixed box of the root; its backdrop likewise.
    let html =
        format!("{PAGE}<div id=w><div popover=manual id=f style='position: fixed'>f</div></div>");
    let (doc, list) = paint(&html, &["f"]);
    let (_, depth) = region(&doc, &list, "f");
    // The viewport clip and the scroll transform of a fixed box.
    assert_eq!(depth, 2);
    // The backdrop paints right before its element, without a hit region
    // (`pointer-events: none` in the user agent's sheet).
    let node = doc.element_by_id("f").expect("f");
    let regions = list
        .items
        .iter()
        .filter(|i| matches!(i, DisplayItem::HitRegion { node: n, .. } if *n == node))
        .count();
    assert_eq!(regions, 1);
}

#[test]
fn the_backdrop_paints_right_below_its_element() {
    // `backdrop-popover`: the backdrop of #b paints after #a (shown
    // earlier) and before #b; the user agent's `pointer-events: none
    // !important` keeps it out of hit testing (an author `!important`
    // cannot change it).
    let html = format!(
        "{PAGE}<style>#b::backdrop {{ pointer-events: auto !important; inset: auto; \
         left: 20px; top: 20px; width: 200px; height: 200px; background: rgb(255, 0, 0) }}</style>\
         <div popover=manual id=a>a</div><div popover=manual id=b>b</div>"
    );
    let (doc, list) = paint(&html, &["a", "b"]);
    let backdrop = list
        .items
        .iter()
        .position(
            |i| matches!(i, DisplayItem::Rect { color, .. } if *color == Rgba::rgb(255, 0, 0)),
        )
        .expect("the backdrop paints");
    let (a, _) = region(&doc, &list, "a");
    let (b, _) = region(&doc, &list, "b");
    assert!(
        a < backdrop && backdrop < b,
        "a {a}, backdrop {backdrop}, b {b}"
    );
    assert_eq!(ids_at(&doc, &list, 30.0, 30.0), ["a"]);
}

const MODAL_PAGE: &str = "<!DOCTYPE html><style>body { margin: 0 } \
    #out { position: absolute; left: 0; top: 0; width: 100px; height: 100px } \
    #fr { position: absolute; left: 0; top: 150px; width: 100px; height: 40px } \
    dialog { width: 60px; height: 60px; padding: 0; border: 0 } \
    [popover] { position: absolute; inset: auto; margin: 0; border: 0; padding: 0; width: 50px; \
    height: 50px } #p { left: 200px; top: 0 } #q { left: 200px; top: 100px }</style>\
    <div id=out>outside text</div><iframe id=fr></iframe>\
    <div id=p popover=manual>p</div>\
    <dialog id=d>d<div id=q popover=manual>q</div></dialog>";

#[test]
fn an_inert_box_has_no_hit_region_and_the_backdrop_catches_the_hit() {
    // `inert-click` and `top-layer-hit-order` rows 5 to 7: under a modal
    // dialog a hit outside it is the dialog (the backdrop's element); a
    // popover outside, shown after the dialog, is drawn but not hit; one
    // inside is hit.
    let (doc, list) = paint(MODAL_PAGE, &["*d", "p", "q"]);
    assert_eq!(ids_at(&doc, &list, 20.0, 20.0), ["d"], "the outside box");
    assert_eq!(
        ids_at(&doc, &list, 20.0, 160.0),
        ["d"],
        "the outside iframe's box"
    );
    assert_eq!(
        ids_at(&doc, &list, 220.0, 20.0),
        ["d"],
        "the popover outside"
    );
    assert_eq!(
        ids_at(&doc, &list, 220.0, 120.0)[0],
        "q",
        "the popover inside"
    );
    // The text of the outside box has no hit region either: every region
    // under the point is the dialog's.
    for node in list.hit_test_all(Point::new(5.0, 5.0), Point::default()) {
        assert!(
            doc.element(node)
                .is_some_and(|e| e.local_name() == "dialog"),
            "only the dialog is hit, not {node:?}"
        );
    }
    // Without the modal dialog the same points hit the boxes.
    let (doc, list) = paint(MODAL_PAGE, &["p"]);
    assert_eq!(ids_at(&doc, &list, 20.0, 20.0)[0], "out");
    assert_eq!(ids_at(&doc, &list, 220.0, 20.0)[0], "p");
}

#[test]
fn an_inert_svg_shape_has_no_hit_shape() {
    let html = "<!DOCTYPE html><style>body { margin: 0 } dialog { width: 60px; height: 60px }</style>\
        <svg width=50 height=50><rect width=50 height=50 fill=red /></svg><dialog id=d>d</dialog>";
    let shapes = |list: &DisplayList| {
        list.items
            .iter()
            .filter(|i| matches!(i, DisplayItem::HitShape { .. }))
            .count()
    };
    let (_, list) = paint(html, &[]);
    assert_eq!(shapes(&list), 1);
    let (_, list) = paint(html, &["*d"]);
    assert_eq!(shapes(&list), 0);
}

#[test]
fn an_inert_frame_is_not_interactive() {
    // `inert-click`: the iframe gets no input under a modal dialog.
    let frame = |list: &DisplayList| {
        list.items
            .iter()
            .find_map(|i| match i {
                DisplayItem::Frame { interactive, .. } => Some(*interactive),
                _ => None,
            })
            .expect("a frame item")
    };
    let (_, list) = paint(MODAL_PAGE, &["*d"]);
    assert!(!frame(&list));
    let (_, list) = paint(MODAL_PAGE, &[]);
    assert!(frame(&list));
}
