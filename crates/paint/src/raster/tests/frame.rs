//! Tests of frames in the rasterizer (ADR 0031 section 4). The engine tests
//! (`crates/engine/tests/frames/paint.rs`) compare whole pages with Chromium.

use std::cell::Cell;
use std::collections::HashMap;

use swb_dom::NodeId;

use super::*;
use crate::group_bounds::transform_ends;

const GREEN: Rgba = Rgba::rgb(0, 255, 0);

/// A frame's document for the tests.
#[derive(Default)]
struct Doc {
    list: DisplayList,
    ends: Vec<usize>,
    scroll: Point,
    canvas: Option<Rgba>,
    children: HashMap<NodeId, Doc>,
}

impl Doc {
    fn new(items: Vec<DisplayItem>) -> Doc {
        Doc {
            ends: transform_ends(&items),
            list: DisplayList { items },
            ..Doc::default()
        }
    }

    fn with_child(mut self, node: NodeId, doc: Doc) -> Doc {
        self.children.insert(node, doc);
        self
    }
}

/// The frames of a document; counts the lookups.
struct Frames<'a> {
    children: &'a HashMap<NodeId, Doc>,
    lookups: &'a Cell<u32>,
}

impl ImageSource for Frames<'_> {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        None
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        None
    }

    fn frame(&self, node: NodeId) -> Option<FrameContent<'_>> {
        self.lookups.set(self.lookups.get() + 1);
        let doc = self.children.get(&node)?;
        Some(FrameContent {
            list: &doc.list,
            ends: &doc.ends,
            scroll: doc.scroll,
            canvas: doc.canvas,
            source: Box::new(Frames {
                children: &doc.children,
                lookups: self.lookups,
            }),
        })
    }
}

fn node(n: usize) -> NodeId {
    NodeId::from_index(n).unwrap()
}

fn rect(x: f32, y: f32, w: f32, h: f32, color: Rgba) -> DisplayItem {
    DisplayItem::Rect {
        rect: Rect::new(x, y, w, h),
        radii: [(0.0, 0.0); 4],
        color,
    }
}

fn frame(id: usize, x: f32, y: f32, w: f32, h: f32) -> DisplayItem {
    DisplayItem::Frame {
        node: node(id),
        rect: Rect::new(x, y, w, h),
        interactive: true,
    }
}

/// Rasterizes `items` with the frame documents `top` on a white 100x100
/// target at `params`, with the lookups of the source.
fn paint(items: Vec<DisplayItem>, top: &Doc, scale: f32, scroll: Point) -> (Pixmap, u32) {
    let size = (100.0 * scale) as u32;
    let mut target = Pixmap::new(size, size).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let params = RasterParams {
        scroll,
        viewport_scroll: scroll,
        scale,
    };
    let lookups = Cell::new(0);
    let source = Frames {
        children: &top.children,
        lookups: &lookups,
    };
    let mut fonts = FontContext::for_tests();
    rasterize(
        &DisplayList { items },
        &mut target,
        params,
        &mut fonts,
        &source,
    );
    (target, lookups.get())
}

fn draw(items: Vec<DisplayItem>, top: &Doc) -> Pixmap {
    paint(items, top, 1.0, Point::default()).0
}

#[test]
fn a_frame_is_clipped_to_its_box_and_moved_to_its_corner() {
    let doc = Doc::new(vec![rect(0.0, 0.0, 200.0, 20.0, RED)]);
    let top = Doc::default().with_child(node(1), doc);
    let p = draw(vec![frame(1, 10.0, 30.0, 40.0, 40.0)], &top);
    assert_eq!(rgb(&p, 15, 35), (255, 0, 0));
    assert_eq!(rgb(&p, 49, 49), (255, 0, 0));
    assert_eq!(rgb(&p, 15, 55), (255, 255, 255), "below the rectangle");
    assert_eq!(rgb(&p, 55, 35), (255, 255, 255), "right of the box");
    assert_eq!(rgb(&p, 5, 35), (255, 255, 255), "left of the box");
}

#[test]
fn a_frame_scrolls_by_its_own_offset() {
    let mut doc = Doc::new(vec![
        rect(0.0, 0.0, 40.0, 100.0, RED),
        rect(0.0, 100.0, 40.0, 100.0, GREEN),
    ]);
    doc.scroll = Point::new(0.0, 100.0);
    let top = Doc::default().with_child(node(1), doc);
    let p = draw(vec![frame(1, 10.0, 10.0, 40.0, 40.0)], &top);
    assert_eq!(rgb(&p, 20, 20), (0, 255, 0));
    // The parent's scroll offset moves the box as any box.
    let doc = Doc::new(vec![rect(0.0, 0.0, 40.0, 40.0, RED)]);
    let top = Doc::default().with_child(node(1), doc);
    let (p, _) = paint(
        vec![frame(1, 10.0, 110.0, 40.0, 40.0)],
        &top,
        1.0,
        Point::new(0.0, 100.0),
    );
    assert_eq!(rgb(&p, 20, 20), (255, 0, 0));
}

#[test]
fn the_canvas_paints_below_the_list_inside_the_box() {
    let mut doc = Doc::new(vec![rect(0.0, 0.0, 10.0, 10.0, RED)]);
    doc.canvas = Some(GREEN);
    let top = Doc::default().with_child(node(1), doc);
    let items = vec![frame(1, 20.0, 20.0, 40.0, 40.0)];
    let p = draw(items, &top);
    assert_eq!(rgb(&p, 25, 25), (255, 0, 0));
    assert_eq!(rgb(&p, 50, 50), (0, 255, 0));
    assert_eq!(rgb(&p, 65, 65), (255, 255, 255));
}

#[test]
fn a_transparent_frame_shows_its_parent() {
    let doc = Doc::new(vec![rect(0.0, 0.0, 10.0, 10.0, RED)]);
    let top = Doc::default().with_child(node(1), doc);
    let items = vec![
        rect(0.0, 0.0, 100.0, 100.0, BLUE),
        frame(1, 20.0, 20.0, 40.0, 40.0),
    ];
    let p = draw(items, &top);
    assert_eq!(rgb(&p, 25, 25), (255, 0, 0));
    assert_eq!(rgb(&p, 50, 50), (0, 0, 255));
}

#[test]
fn the_groups_of_the_parent_apply_to_the_frame() {
    let doc = Doc::new(vec![rect(0.0, 0.0, 100.0, 100.0, RED)]);
    let top = Doc::default().with_child(node(1), doc);
    let items = vec![
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: Rect::new(10.0, 10.0, 50.0, 50.0),
            fixed_bounds: None,
            escapes_clips: false,
        },
        frame(1, 10.0, 10.0, 50.0, 50.0),
        DisplayItem::PopOpacity,
    ];
    let p = draw(items, &top);
    let (r, g, _) = rgb(&p, 30, 30);
    assert_eq!(r, 255);
    assert!((126..=129).contains(&g), "{g}");
    // A clip of the parent.
    let items = vec![
        DisplayItem::PushClip(Rect::new(0.0, 0.0, 30.0, 100.0)),
        frame(1, 10.0, 10.0, 50.0, 50.0),
        DisplayItem::PopClip,
    ];
    let p = draw(items, &top);
    assert_eq!(rgb(&p, 25, 30), (255, 0, 0));
    assert_eq!(rgb(&p, 35, 30), (255, 255, 255));
}

/// A group of content fixed to the viewport with `items`.
fn fixed(items: Vec<DisplayItem>) -> Vec<DisplayItem> {
    let mut all = vec![
        DisplayItem::PushViewportClip,
        DisplayItem::PushTransform {
            transform: GroupTransform::Fixed,
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            hit_bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            fixed_bounds: None,
        },
    ];
    all.extend(items);
    all.push(DisplayItem::PopTransform);
    all.push(DisplayItem::PopClip);
    all
}

#[test]
fn fixed_content_stays_at_the_corner_of_the_frame() {
    // The frame is scrolled, and its fixed box is 10 px from its corner; a
    // box as large as the page stays inside the frame.
    let mut doc = Doc::new(fixed(vec![
        rect(-50.0, -50.0, 55.0, 55.0, GREEN),
        rect(10.0, 10.0, 10.0, 10.0, RED),
    ]));
    doc.scroll = Point::new(0.0, 500.0);
    let top = Doc::default().with_child(node(1), doc);
    let p = draw(vec![frame(1, 20.0, 20.0, 40.0, 40.0)], &top);
    assert_eq!(rgb(&p, 35, 35), (255, 0, 0));
    assert_eq!(rgb(&p, 22, 22), (0, 255, 0));
    assert_eq!(rgb(&p, 15, 15), (255, 255, 255), "the frame's clip stays");
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    // The page's own fixed content is not affected after the frame.
    let items: Vec<DisplayItem> = std::iter::once(frame(1, 20.0, 20.0, 40.0, 40.0))
        .chain(fixed(vec![rect(70.0, 70.0, 10.0, 10.0, BLUE)]))
        .collect();
    let p = draw(items, &top);
    assert_eq!(rgb(&p, 75, 75), (0, 0, 255));
}

#[test]
fn nested_frames_add_their_corners() {
    let leaf = Doc::new(vec![rect(0.0, 0.0, 100.0, 100.0, RED)]);
    let middle = Doc::new(vec![frame(2, 10.0, 10.0, 20.0, 20.0)]).with_child(node(2), leaf);
    let top = Doc::default().with_child(node(1), middle);
    let p = draw(vec![frame(1, 20.0, 20.0, 50.0, 50.0)], &top);
    assert_eq!(rgb(&p, 25, 25), (255, 255, 255));
    assert_eq!(rgb(&p, 35, 35), (255, 0, 0));
    assert_eq!(rgb(&p, 49, 49), (255, 0, 0));
    assert_eq!(rgb(&p, 52, 52), (255, 255, 255));
}

#[test]
fn a_frame_starts_on_a_device_pixel() {
    let doc = Doc::new(vec![rect(0.0, 0.0, 10.0, 10.0, RED)]);
    let top = Doc::default().with_child(node(1), doc);
    let p = draw(vec![frame(1, 10.5, 10.5, 40.0, 40.0)], &top);
    // Snapped as the clip: at 11, also for what the frame paints.
    assert_eq!(rgb(&p, 10, 15), (255, 255, 255));
    assert_eq!(rgb(&p, 11, 15), (255, 0, 0));
    assert_eq!(rgb(&p, 20, 15), (255, 0, 0));
    assert_eq!(rgb(&p, 21, 15), (255, 255, 255));
    // At scale 2 the corner is at device pixel 21.
    let (p, _) = paint(
        vec![frame(1, 10.5, 10.5, 40.0, 40.0)],
        &top,
        2.0,
        Point::default(),
    );
    assert_eq!(rgb(&p, 20, 30), (255, 255, 255));
    assert_eq!(rgb(&p, 21, 30), (255, 0, 0));
    assert_eq!(rgb(&p, 40, 30), (255, 0, 0));
    assert_eq!(rgb(&p, 41, 30), (255, 255, 255));
}

#[test]
fn a_frame_outside_the_visible_area_is_not_looked_at() {
    let doc = Doc::new(vec![rect(0.0, 0.0, 10.0, 10.0, RED)]);
    let top = Doc::default().with_child(node(1), doc);
    let (_, lookups) = paint(
        vec![frame(1, 10.0, 500.0, 40.0, 40.0)],
        &top,
        1.0,
        Point::default(),
    );
    assert_eq!(lookups, 0);
    let (_, lookups) = paint(
        vec![frame(1, 10.0, 10.0, 40.0, 40.0)],
        &top,
        1.0,
        Point::default(),
    );
    assert_eq!(lookups, 1);
}

#[test]
fn an_element_without_a_frame_draws_nothing() {
    let top = Doc::default();
    let p = draw(vec![frame(9, 10.0, 10.0, 40.0, 40.0)], &top);
    assert_eq!(rgb(&p, 20, 20), (255, 255, 255));
    // Nor does a document with an empty list.
    let top = Doc::default().with_child(node(1), Doc::new(Vec::new()));
    let p = draw(vec![frame(1, 10.0, 10.0, 40.0, 40.0)], &top);
    assert_eq!(rgb(&p, 20, 20), (255, 255, 255));
}

/// The frames of a strip share the budget of items: the frame that does not
/// fit draws nothing, also no canvas.
#[test]
fn a_frame_past_the_item_budget_draws_nothing() {
    let count = crate::raster::frame::MAX_FRAME_ITEMS as usize / 2 + 1;
    let filler = DisplayItem::HitRegion {
        rect: Rect::new(0.0, 0.0, 1.0, 1.0),
        node: node(100),
    };
    let mut first = Doc::new(vec![filler.clone(); count]);
    first.canvas = Some(RED);
    let mut second = Doc::new(vec![filler; count]);
    second.canvas = Some(GREEN);
    let mut third = Doc::new(vec![rect(0.0, 0.0, 5.0, 5.0, BLUE)]);
    third.canvas = Some(BLUE);
    let top = Doc::default()
        .with_child(node(1), first)
        .with_child(node(2), second)
        .with_child(node(3), third);
    let p = draw(
        vec![
            frame(1, 0.0, 0.0, 30.0, 30.0),
            frame(2, 40.0, 0.0, 30.0, 30.0),
            frame(3, 80.0, 0.0, 20.0, 20.0),
        ],
        &top,
    );
    assert_eq!(rgb(&p, 10, 10), (255, 0, 0));
    assert_eq!(rgb(&p, 50, 10), (255, 255, 255), "past the budget");
    // A small frame after it still fits.
    assert_eq!(rgb(&p, 90, 10), (0, 0, 255));
}

/// A chain of nested frames, each with translation groups: the recursion
/// stops at the limit of the groups, without a stack overflow, and what is
/// shallower still draws.
#[test]
fn deep_groups_in_nested_frames_stop_at_the_limit() {
    let group = || DisplayItem::PushTransform {
        transform: GroupTransform::Matrix(Matrix::translate(0.0, 0.0)),
        bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
        hit_bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
        fixed_bounds: None,
    };
    let mut doc = Doc::new(vec![rect(0.0, 0.0, 100.0, 100.0, RED)]);
    for _ in 0..30 {
        let mut items = vec![rect(0.0, 0.0, 5.0, 5.0, BLUE)];
        items.extend(std::iter::repeat_with(group).take(100));
        items.push(frame(1, 0.0, 0.0, 100.0, 100.0));
        items.extend(std::iter::repeat_n(DisplayItem::PopTransform, 100));
        doc = Doc::new(items).with_child(node(1), doc);
    }
    let top = Doc::default().with_child(node(1), doc);
    let handle = std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            let p = draw(vec![frame(1, 0.0, 0.0, 100.0, 100.0)], &top);
            rgb(&p, 2, 2)
        })
        .unwrap();
    assert_eq!(handle.join().unwrap(), (0, 0, 255));
}
