//! `background-clip: text` and the clips of background layers by rounded
//! corners in the display list (M13 feature 2c, ADR 0046): what is in the
//! mask of the text, which descendants leave it, the work bound, and the
//! rounded clip groups. The pixels against Chromium are in
//! `static_css_pixels.rs`. The pages are files in a temporary directory;
//! nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Size};
use swb_net::NetworkFetcher;
use swb_paint::{DisplayItem, DisplayList, MaskLayerImage};
use swb_style::Rgba;

mod common;
use common::Site;

/// The display list of a page with `body` and the style `css`.
fn list_of(name: &str, css: &str, body: &str) -> DisplayList {
    let site = Site::new(name);
    let html = format!(
        "<!DOCTYPE html><html><head><style>body{{margin:0;background:#fff}}\
         .t{{width:200px;height:60px;font:bold 40px/60px Arial;color:#0000;\
         -webkit-text-fill-color:transparent;background:linear-gradient(90deg,red,blue);\
         background-clip:text}}{css}</style></head><body>{body}</body></html>"
    );
    let url = site.page("page.html", &html);
    let mut page: Page =
        common::new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(400.0, 300.0));
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(20));
    common::display_list(&mut page)
}

/// The items of the content layers of the mask groups of `list`.
fn masks(list: &DisplayList) -> Vec<Vec<DisplayItem>> {
    list.items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::PushMask { layers, .. } => match &layers[..] {
                [layer] => match &layer.image {
                    MaskLayerImage::Content { items, .. } => Some(items.to_vec()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn count(list: &DisplayList, f: impl Fn(&DisplayItem) -> bool) -> usize {
    list.items.iter().filter(|i| f(i)).count()
}

fn is_gradient(item: &DisplayItem) -> bool {
    matches!(item, DisplayItem::LinearGradient { .. })
}

fn text_items(items: &[DisplayItem]) -> usize {
    items
        .iter()
        .filter(|i| matches!(i, DisplayItem::Text { .. }))
        .count()
}

/// The layer is clipped by one group whose mask holds the text of the box,
/// in white, and the text itself paints as before.
#[test]
fn the_text_of_the_box_is_the_mask() {
    let list = list_of("text-clip-basic", "", "<div class=t>Hi</div>");
    let masks = masks(&list);
    let [items] = &masks[..] else {
        panic!("{masks:?}");
    };
    assert_eq!(text_items(items), 1);
    assert!(items.iter().all(|i| match i {
        DisplayItem::Text { color, .. } => *color == Rgba::WHITE,
        _ => false,
    }));
    // One gradient, inside the group; the text paints after it.
    assert_eq!(count(&list, is_gradient), 1);
    let push = list
        .items
        .iter()
        .position(|i| matches!(i, DisplayItem::PushMask { .. }))
        .unwrap();
    let gradient = list.items.iter().position(is_gradient).unwrap();
    let pop = list
        .items
        .iter()
        .position(|i| matches!(i, DisplayItem::PopMask))
        .unwrap();
    assert!(push < gradient && gradient < pop);
    let text = list
        .items
        .iter()
        .position(|i| matches!(i, DisplayItem::Text { .. }))
        .unwrap();
    assert!(pop < text, "the mask's text is not the painted text");
}

/// The text of descendants that paint in the layer of the box is in the
/// mask; positioned boxes and boxes with an effect, and their text, are
/// not (measured in Chromium 148, `text-clip` of
/// `tools/probes/css-videojs-2c.json`).
#[test]
fn descendants_in_layers_of_their_own_leave_the_mask() {
    let body = "<div class=t>a<span>b</span><div>c</div>\
        <span style='display:inline-block;overflow:hidden;width:5px'>d</span>\
        <span style='float:right'>e</span>\
        <span style='position:absolute'>f</span>\
        <span style='position:relative'>g</span>\
        <span style='display:inline-block;opacity:.5'>h</span>\
        <span style='display:inline-block;transform:translate(0,1px)'>i</span>\
        <span style='display:inline-block;filter:blur(0)'>j</span>\
        <span style='visibility:hidden'>k</span>\
        <div style='display:flex'><i>l</i></div></div>";
    let list = list_of("text-clip-layers", "", body);
    let masks = masks(&list);
    let [items] = &masks[..] else {
        panic!("{masks:?}");
    };
    // a, b, c, d (its clip does not cut the mask), e, l.
    assert_eq!(text_items(items), 6, "{items:?}");
}

/// The stroke is in the mask whatever its color, decorations too; shadows
/// are not.
#[test]
fn strokes_and_decorations_are_in_the_mask_but_not_shadows() {
    let css = ".s{-webkit-text-stroke:3px transparent;text-decoration:underline;\
               text-shadow:5px 5px red}";
    let list = list_of("text-clip-stroke", css, "<div class='t s'>Hi</div>");
    let masks = masks(&list);
    let [items] = &masks[..] else {
        panic!("{masks:?}");
    };
    let strokes = items
        .iter()
        .filter(|i| matches!(i, DisplayItem::TextStroke { .. }))
        .count();
    let lines = items
        .iter()
        .filter(|i| matches!(i, DisplayItem::Rect { .. }))
        .count();
    assert_eq!((text_items(items), strokes, lines), (1, 1, 1), "{items:?}");
}

/// The decoration lines of all ancestors are white shapes: the mask has one
/// rectangle per line kind and fragment, however many ancestors declare
/// lines (a rectangle per ancestor used to take 31 GB in a deep tree).
#[test]
fn the_decorations_of_many_ancestors_are_one_rectangle_per_line() {
    let css = ".d{text-decoration:underline overline line-through}";
    let body = format!(
        "<div class=t>{}Hi{}</div>",
        "<div class=d>".repeat(20),
        "</div>".repeat(20)
    );
    let list = list_of("text-clip-decorations", css, &body);
    let masks = masks(&list);
    let [items] = &masks[..] else {
        panic!("{masks:?}");
    };
    let lines = items
        .iter()
        .filter(|i| matches!(i, DisplayItem::Rect { .. }))
        .count();
    assert_eq!((text_items(items), lines), (1, 3), "{items:?}");
}

/// A box without text paints no layer that says `text`; a layer that says
/// something else still paints.
#[test]
fn a_box_without_text_paints_no_text_layer() {
    let css = ".e{background-image:linear-gradient(red,blue),linear-gradient(blue,red);\
               background-clip:text,border-box}";
    let list = list_of("text-clip-empty", css, "<div class='t e'></div>");
    // The empty box paints the border-box layer only.
    assert_eq!(masks(&list), Vec::<Vec<DisplayItem>>::new());
    assert_eq!(count(&list, is_gradient), 1);
}

/// More than 100,000 glyphs in a box: its layers paint as `border-box`
/// (no mask) and the log has a warning.
#[test]
fn too_many_glyphs_paint_the_border_box() {
    let text = "x".repeat(100_100);
    let css = ".w{font:1px/1px Arial;height:auto;width:100px;overflow-wrap:anywhere}";
    let body = format!("<div class='t w'>{text}</div>");
    let list = list_of("text-clip-glyphs", css, &body);
    assert_eq!(masks(&list), Vec::<Vec<DisplayItem>>::new());
    assert_eq!(count(&list, is_gradient), 1);
}

/// A rounded box clips its image layers by the rounded border: the rows
/// with the corners paint in a group each (the top and the bottom band of
/// the layer), the part between them without a group. A box with square
/// corners has no group.
#[test]
fn rounded_boxes_clip_their_layers() {
    let css = ".r{width:100px;height:100px;background:linear-gradient(red,blue)}";
    // The number of groups, of gradient items outside groups and of those
    // inside.
    let shape = |style: &str| {
        let list = list_of(
            "rounded-layer",
            css,
            &format!("<div class=r style='{style}'></div>"),
        );
        let (mut depth, mut outside, mut inside, mut groups) = (0, 0, 0, 0);
        for item in &list.items {
            match item {
                DisplayItem::PushSvgClip { .. } => {
                    depth += 1;
                    groups += 1;
                }
                DisplayItem::PopSvgClip => depth -= 1,
                DisplayItem::LinearGradient { .. } if depth == 0 => outside += 1,
                DisplayItem::LinearGradient { .. } => inside += 1,
                _ => {}
            }
        }
        (groups, outside, inside)
    };
    assert_eq!(shape(""), (0, 1, 0));
    assert_eq!(shape("border-radius:20px"), (2, 1, 2));
    // The inner radii of a padding-box clip; a color under the layers is in
    // the same groups (one anti-aliased edge: the color is a rectangle).
    assert_eq!(
        shape("border-radius:20px;border:5px solid #0003;background-clip:padding-box"),
        (2, 1, 2)
    );
    assert_eq!(shape("border-radius:20px;background-color:#0c0"), (2, 1, 2));
    // The corner rows fill the box: one group.
    assert_eq!(shape("border-radius:50px"), (1, 0, 1));
}
