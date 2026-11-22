use std::cell::Cell;
use std::sync::Arc;

use swb_layout::{GroupTransform, Matrix, Point, Rect};
use swb_style::{FilterFunction, MixBlendMode, Rgba};
use swb_text::FontContext;
use tiny_skia::Pixmap;

use super::*;
use crate::display_list::{DisplayItem, DisplayList, ImageRef};
use crate::image::DecodedImage;
use crate::raster::{ImageSource, PixelRect, RasterParams, rasterize, rasterize_region};
use crate::svg::VectorCache;

const PARAMS: RasterParams = RasterParams {
    scroll: Point::new(0.0, 0.0),
    viewport_scroll: Point::new(0.0, 0.0),
    scale: 1.0,
};

const SIZE: (u32, u32) = (200, 150);

fn rect(x: f32, y: f32, w: f32, h: f32, color: Rgba) -> DisplayItem {
    DisplayItem::Rect {
        rect: Rect::new(x, y, w, h),
        radii: [(0.0, 0.0); 4],
        color,
    }
}

fn list(items: Vec<DisplayItem>) -> DisplayList {
    let mut items = items;
    crate::group_bounds::finish_groups(&mut items);
    DisplayList { items }
}

struct NoImages;

impl ImageSource for NoImages {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        None
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        None
    }
}

/// The damage between two lists without images.
fn damage(old: &DisplayList, new: &DisplayList) -> Damage {
    let (ok, nk) = (media_keys(old, &NoImages), media_keys(new, &NoImages));
    let found = list_damage(
        Side {
            list: old,
            keys: &ok,
        },
        Side {
            list: new,
            keys: &nk,
        },
        PARAMS,
    );
    if found.full {
        return Damage::Full;
    }
    merge_damage(&found.rects, &backdrop_boxes(new, &nk, PARAMS), SIZE)
}

fn rects(d: &Damage) -> Vec<PixelRect> {
    match d {
        Damage::Rects(r) => r.clone(),
        other => panic!("not rectangles: {other:?}"),
    }
}

fn opacity(value: f32) -> DisplayItem {
    DisplayItem::opacity_group(value, MixBlendMode::Normal)
}

fn translate(x: f32, y: f32) -> DisplayItem {
    DisplayItem::PushTransform {
        transform: GroupTransform::Matrix(Matrix::translate(x, y)),
        bounds: Rect::default(),
        hit_bounds: Rect::default(),
        fixed_bounds: None,
    }
}

#[test]
fn equal_lists_have_no_damage() {
    let a = list(vec![
        rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE),
        opacity(0.5),
        rect(10.0, 10.0, 20.0, 20.0, Rgba::BLACK),
        DisplayItem::PopOpacity,
    ]);
    assert_eq!(damage(&a, &a.clone()), Damage::None);
}

#[test]
fn a_changed_item_damages_its_bounds() {
    let a = list(vec![
        rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE),
        rect(10.0, 10.0, 20.0, 20.0, Rgba::BLACK),
        rect(100.0, 100.0, 20.0, 20.0, Rgba::BLACK),
    ]);
    let mut b = a.clone();
    b.items[2] = rect(100.0, 100.0, 20.0, 20.0, Rgba::rgb(255, 0, 0));
    // The bounds rounded out and grown by 1 px.
    assert_eq!(
        rects(&damage(&a, &b)),
        vec![PixelRect::from_edges(99, 99, 121, 121)]
    );
}

#[test]
fn a_changed_group_damages_the_whole_group() {
    let items = |o: f32| {
        list(vec![
            rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE),
            opacity(o),
            rect(10.0, 10.0, 20.0, 20.0, Rgba::BLACK),
            rect(50.0, 40.0, 20.0, 20.0, Rgba::BLACK),
            DisplayItem::PopOpacity,
            rect(150.0, 100.0, 10.0, 10.0, Rgba::BLACK),
        ])
    };
    assert_eq!(
        rects(&damage(&items(0.5), &items(0.6))),
        vec![PixelRect::from_edges(9, 9, 71, 61)]
    );
}

#[test]
fn a_moved_translation_damages_old_and_new_places() {
    let items = |x: f32| {
        list(vec![
            translate(x, 0.0),
            rect(10.0, 10.0, 10.0, 10.0, Rgba::BLACK),
            DisplayItem::PopTransform,
            rect(150.0, 100.0, 10.0, 10.0, Rgba::BLACK),
        ])
    };
    let r = rects(&damage(&items(0.0), &items(100.0)));
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(r.contains(&PixelRect::from_edges(9, 9, 21, 21)));
    assert!(r.contains(&PixelRect::from_edges(109, 9, 121, 21)));
}

#[test]
fn a_new_group_damages_only_what_it_holds() {
    // Opacity 1 to 0.5 adds a group around the same items: the items after
    // it (the common suffix) are not damage.
    let before = list(vec![
        rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE),
        rect(10.0, 10.0, 20.0, 20.0, Rgba::BLACK),
        rect(150.0, 100.0, 10.0, 10.0, Rgba::BLACK),
    ]);
    let after = list(vec![
        rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE),
        opacity(0.5),
        rect(10.0, 10.0, 20.0, 20.0, Rgba::BLACK),
        DisplayItem::PopOpacity,
        rect(150.0, 100.0, 10.0, 10.0, Rgba::BLACK),
    ]);
    assert_eq!(
        rects(&damage(&before, &after)),
        vec![PixelRect::from_edges(9, 9, 31, 31)]
    );
    assert_eq!(
        rects(&damage(&after, &before)),
        vec![PixelRect::from_edges(9, 9, 31, 31)]
    );
}

#[test]
fn damage_under_a_filter_grows_by_its_reach() {
    let items = |c: Rgba| {
        list(vec![
            DisplayItem::PushFilter {
                filter: Arc::from(vec![FilterFunction::Blur(4.0)]),
                current_color: Rgba::BLACK,
                bounds: Rect::default(),
                fixed_bounds: None,
                escapes_clips: false,
            },
            rect(20.0, 20.0, 10.0, 10.0, Rgba::BLACK),
            rect(100.0, 100.0, 10.0, 10.0, c),
            DisplayItem::PopFilter,
        ])
    };
    let r = rects(&damage(&items(Rgba::BLACK), &items(Rgba::WHITE)));
    assert_eq!(r.len(), 1);
    let reach = (3.0 * 4.0 + 8.0) as u32;
    assert!(
        r[0].x <= 100 - reach && r[0].right() >= 110 + reach,
        "{r:?}"
    );
}

#[test]
fn a_clip_limits_the_damage() {
    let items = |y: f32| {
        list(vec![
            DisplayItem::PushClip(Rect::new(10.0, 10.0, 50.0, 50.0)),
            rect(10.0, y, 50.0, 500.0, Rgba::BLACK),
            DisplayItem::PopClip,
        ])
    };
    let r = rects(&damage(&items(10.0), &items(-100.0)));
    assert_eq!(r, vec![PixelRect::from_edges(9, 8, 61, 62)]);
}

#[test]
fn a_touched_backdrop_filter_is_damaged_whole() {
    let items = |c: Rgba| {
        list(vec![
            rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE),
            rect(10.0, 10.0, 10.0, 10.0, c),
            DisplayItem::BackdropFilter {
                filter: Arc::from(vec![FilterFunction::Blur(3.0)]),
                current_color: Rgba::BLACK,
                rect: Rect::new(15.0, 15.0, 100.0, 60.0),
                radii: None,
                alpha: 1.0,
                layers: 0,
            },
        ])
    };
    let r = rects(&damage(&items(Rgba::BLACK), &items(Rgba::WHITE)));
    assert_eq!(r, vec![PixelRect::from_edges(9, 9, 116, 76)]);
}

#[test]
fn many_rectangles_merge_into_few() {
    let items = |c: Rgba| {
        let mut items = vec![rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE)];
        for i in 0..400 {
            let (x, y) = ((i % 20) as f32 * 10.0, (i / 20) as f32 * 7.0);
            items.push(rect(x, y, 2.0, 2.0, c));
        }
        list(items)
    };
    // Scattered over the whole target: a full raster.
    assert_eq!(
        damage(&items(Rgba::BLACK), &items(Rgba::WHITE)),
        Damage::Full
    );
    // Twenty small changes in a corner: at most eight rectangles.
    let corner = |c: Rgba| {
        let mut items = vec![rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE)];
        for i in 0..20 {
            items.push(rect(i as f32 * 4.0, (i % 3) as f32 * 9.0, 2.0, 2.0, c));
        }
        list(items)
    };
    let r = rects(&damage(&corner(Rgba::BLACK), &corner(Rgba::WHITE)));
    assert!(r.len() <= MAX_RECTS, "{r:?}");
    for (i, a) in r.iter().enumerate() {
        for b in r.iter().skip(i + 1) {
            assert!(!overlaps(*a, *b), "{a:?} overlaps {b:?}");
        }
    }
}

#[test]
fn groups_that_do_not_nest_need_a_full_raster() {
    let bad = list(vec![
        opacity(0.5),
        DisplayItem::PushClip(Rect::new(0.0, 0.0, 10.0, 10.0)),
        DisplayItem::PopOpacity,
        DisplayItem::PopClip,
    ]);
    assert_eq!(damage(&bad, &bad.clone()), Damage::Full);
}

/// An image source whose one image changes its pixels with its generation.
struct Video {
    frames: [DecodedImage; 2],
    generation: Cell<u64>,
}

impl ImageSource for Video {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        self.frames.get((self.generation.get() % 2) as usize)
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        None
    }

    fn generation(&self, _image: &ImageRef) -> u64 {
        self.generation.get()
    }
}

fn solid(color: Rgba) -> DecodedImage {
    let mut pixmap = Pixmap::new(8, 8).expect("a small pixmap");
    crate::fill(&mut pixmap, color);
    DecodedImage::from_pixmap(pixmap)
}

#[test]
fn a_new_generation_of_an_image_is_damage() {
    let video = Video {
        frames: [solid(Rgba::rgb(255, 0, 0)), solid(Rgba::rgb(0, 0, 255))],
        generation: Cell::new(0),
    };
    let image = DisplayItem::Image {
        image: ImageRef::Node(swb_dom::NodeId::DOCUMENT),
        rect: Rect::new(40.0, 30.0, 64.0, 48.0),
        tile: Rect::new(40.0, 30.0, 64.0, 48.0),
        clip: Rect::new(40.0, 30.0, 64.0, 48.0),
    };
    let items = list(vec![rect(0.0, 0.0, 200.0, 150.0, Rgba::WHITE), image]);
    let mut fonts = FontContext::for_tests();
    let mut kept = Pixmap::new(SIZE.0, SIZE.1).expect("a pixmap");
    rasterize(&items, &mut kept, PARAMS, &mut fonts, &video);
    let old_keys = media_keys(&items, &video);
    video.generation.set(1);
    let new_keys = media_keys(&items, &video);
    let found = list_damage(
        Side {
            list: &items,
            keys: &old_keys,
        },
        Side {
            list: &items,
            keys: &new_keys,
        },
        PARAMS,
    );
    let r = rects(&merge_damage(&found.rects, &[], SIZE));
    assert_eq!(r, vec![PixelRect::from_edges(39, 29, 105, 79)]);
    // The region raster of the damage gives the pixels of a full raster.
    rasterize_region(&items, &mut kept, PARAMS, &r, None, &mut fonts, &video);
    let mut full = Pixmap::new(SIZE.0, SIZE.1).expect("a pixmap");
    rasterize(&items, &mut full, PARAMS, &mut fonts, &video);
    assert_eq!(kept.data(), full.data());
}

#[test]
fn a_long_list_with_one_change_is_compared_in_one_pass() {
    // 100,000 items, one changes: only that one is damage.
    let items = |c: Rgba| {
        let mut items = Vec::with_capacity(100_001);
        for i in 0..100_000 {
            let color = if i == 50_000 { c } else { Rgba::BLACK };
            items.push(rect((i % 100) as f32, (i / 1000) as f32, 1.0, 1.0, color));
        }
        list(items)
    };
    let r = rects(&damage(&items(Rgba::BLACK), &items(Rgba::WHITE)));
    assert_eq!(r, vec![PixelRect::from_edges(0, 49, 2, 52)]);
}

/// 10,000 nested opacity groups (a page cannot make them: the box depth
/// is limited), with a change in the innermost one or in the outermost
/// opacity. The walk keeps its own stack and skips whole groups.
#[test]
fn ten_thousand_nested_groups() {
    let nested = |outer: f32, c: Rgba| {
        let mut items = vec![opacity(outer)];
        for _ in 0..10_000 {
            items.push(opacity(0.99));
        }
        items.push(rect(10.0, 10.0, 5.0, 5.0, c));
        for _ in 0..10_001 {
            items.push(DisplayItem::PopOpacity);
        }
        list(items)
    };
    let started = std::time::Instant::now();
    let r = rects(&damage(
        &nested(0.5, Rgba::BLACK),
        &nested(0.5, Rgba::WHITE),
    ));
    assert_eq!(r, vec![PixelRect::from_edges(9, 9, 16, 16)]);
    let r = rects(&damage(
        &nested(0.5, Rgba::BLACK),
        &nested(0.6, Rgba::BLACK),
    ));
    assert_eq!(r, vec![PixelRect::from_edges(9, 9, 16, 16)]);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

/// Groups that appear and go at every item: the kinds differ along the
/// whole list; the lookahead and the common suffix bound the work.
#[test]
fn groups_that_appear_at_every_item() {
    let items = |odd: bool| {
        let mut items = Vec::new();
        for i in 0..20_000 {
            let r = rect(
                (i % 190) as f32,
                (i / 190 % 140) as f32,
                1.0,
                1.0,
                Rgba::BLACK,
            );
            if (i % 2 == 1) == odd {
                items.extend([opacity(0.5), r, DisplayItem::PopOpacity]);
            } else {
                items.push(r);
            }
        }
        list(items)
    };
    let started = std::time::Instant::now();
    let d = damage(&items(false), &items(true));
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_ne!(d, Damage::None);
}
