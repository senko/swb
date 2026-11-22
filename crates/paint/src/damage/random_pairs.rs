//! Random pairs of display lists: the raster of the old list, with the new
//! list's damage drawn by the region raster, must equal a full raster of the
//! new list bit for bit. The generator mixes rectangles (plain and rounded),
//! lines, hit regions, backdrop filters, clips, opacity and blend groups,
//! transforms (translations, rotations, scales) and filters; the mutations
//! insert, remove, replace, wrap, unwrap, duplicate and swap items and
//! groups. The case of the feature 4b review (an opacity group whose bounds
//! change) has its own test.

use std::sync::Arc;

use swb_layout::{GroupTransform, Matrix, Point, Rect};
use swb_style::{FilterFunction, MixBlendMode, Rgba};
use swb_text::FontContext;
use tiny_skia::Pixmap;

use super::*;
use crate::display_list::{DisplayItem, DisplayList, ImageRef};
use crate::image::DecodedImage;
use crate::raster::{ImageSource, RasterParams, rasterize, rasterize_region};
use crate::svg::VectorCache;

struct NoImages;

impl ImageSource for NoImages {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        None
    }
    fn vector_cache(&self) -> Option<&VectorCache> {
        None
    }
}

/// A xorshift generator: the cases are the same in every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.below(1000) as f32 / 1000.0) * (hi - lo)
    }
}

/// The page size in CSS px; the target is this times the scale.
const PAGE: (f32, f32) = (160.0, 120.0);

fn color(r: &mut Rng) -> Rgba {
    let a = if r.below(3) == 0 { 128 } else { 255 };
    Rgba::new(
        r.below(256) as u8,
        r.below(256) as u8,
        r.below(256) as u8,
        a,
    )
}

fn drawing(r: &mut Rng) -> DisplayItem {
    match r.below(6) {
        0..=2 => {
            let rect = Rect::new(
                r.f(-10.0, 140.0),
                r.f(-10.0, 100.0),
                r.f(3.0, 60.0),
                r.f(3.0, 50.0),
            );
            // The builder clamps radii to half the box.
            let radius = (rect.width / 2.0).min(rect.height / 2.0).min(8.0);
            let radii = if r.below(3) == 0 { radius } else { 0.0 };
            DisplayItem::Rect {
                rect,
                radii: [(radii, radii); 4],
                color: color(r),
            }
        }
        3 => DisplayItem::Polyline {
            points: Arc::from(vec![
                Point::new(r.f(0.0, 150.0), r.f(0.0, 110.0)),
                Point::new(r.f(0.0, 150.0), r.f(0.0, 110.0)),
            ]),
            width: r.f(1.0, 12.0),
            color: color(r),
        },
        4 => DisplayItem::HitRegion {
            rect: Rect::new(r.f(0.0, 100.0), r.f(0.0, 100.0), 30.0, 30.0),
            node: swb_dom::NodeId::DOCUMENT,
        },
        _ => DisplayItem::BackdropFilter {
            filter: Arc::from(vec![FilterFunction::Blur(r.f(1.0, 4.0))]),
            current_color: Rgba::BLACK,
            rect: Rect::new(
                r.f(0.0, 100.0),
                r.f(0.0, 80.0),
                r.f(10.0, 50.0),
                r.f(10.0, 40.0),
            ),
            radii: None,
            alpha: 1.0,
            layers: 0,
        },
    }
}

/// The start and end items of a random group or clip.
fn group(r: &mut Rng) -> (DisplayItem, DisplayItem) {
    match r.below(6) {
        0 => (
            DisplayItem::PushClip(Rect::new(
                r.f(0.0, 80.0),
                r.f(0.0, 60.0),
                r.f(10.0, 80.0),
                r.f(10.0, 60.0),
            )),
            DisplayItem::PopClip,
        ),
        1 => (
            DisplayItem::opacity_group(0.5, MixBlendMode::Normal),
            DisplayItem::PopOpacity,
        ),
        2 => (
            DisplayItem::opacity_group(0.7, MixBlendMode::Multiply),
            DisplayItem::PopOpacity,
        ),
        3 => {
            let m = match r.below(3) {
                0 => Matrix::translate(r.f(-20.0, 20.0), r.f(-20.0, 20.0)),
                1 => {
                    let angle = r.f(-1.5, 1.5);
                    let (s, c) = angle.sin_cos();
                    Matrix::new(c, s, -s, c, r.f(0.0, 60.0), r.f(0.0, 40.0))
                }
                _ => Matrix::new(
                    r.f(0.3, 3.0),
                    0.0,
                    0.0,
                    r.f(0.3, 3.0),
                    r.f(0.0, 50.0),
                    r.f(0.0, 30.0),
                ),
            };
            (
                DisplayItem::PushTransform {
                    transform: GroupTransform::Matrix(m),
                    bounds: Rect::default(),
                    hit_bounds: Rect::default(),
                    fixed_bounds: None,
                },
                DisplayItem::PopTransform,
            )
        }
        _ => {
            let filter = match r.below(3) {
                0 => FilterFunction::Blur(r.f(1.0, 6.0)),
                1 => FilterFunction::Brightness(1.5),
                _ => FilterFunction::Invert(1.0),
            };
            (
                DisplayItem::PushFilter {
                    filter: Arc::from(vec![filter]),
                    current_color: Rgba::BLACK,
                    bounds: Rect::default(),
                    fixed_bounds: None,
                    escapes_clips: false,
                },
                DisplayItem::PopFilter,
            )
        }
    }
}

/// Appends `n` items or groups, nested at most three deep.
fn generate(r: &mut Rng, items: &mut Vec<DisplayItem>, depth: usize, n: usize) {
    for _ in 0..n {
        if depth < 3 && r.below(4) == 0 {
            let (start, end) = group(r);
            items.push(start);
            let inner = 1 + r.below(4);
            generate(r, items, depth + 1, inner);
            items.push(end);
        } else {
            items.push(drawing(r));
        }
    }
}

/// One random change at a random place; the groups keep their nesting.
fn mutate(r: &mut Rng, items: &mut Vec<DisplayItem>) {
    let Some(ends) = group_ends(items) else {
        return;
    };
    let n = items.len();
    if n == 0 {
        return;
    }
    let i = r.below(n);
    let opens = ends[i] > i;
    let closes = is_pop(&items[i]);
    // The last item of the item or group at `i`.
    let last = if opens { ends[i] } else { i };
    match r.below(8) {
        0 if !opens && !closes => {
            items.remove(i);
        }
        1 if opens => {
            items.drain(i..=last);
        }
        2 if opens => {
            items.remove(last);
            items.remove(i);
        }
        3 if !closes => items.insert(i, drawing(r)),
        4 if !closes => {
            let (start, end) = group(r);
            items.insert(last + 1, end);
            items.insert(i, start);
        }
        5 if !opens && !closes => items[i] = drawing(r),
        6 if !closes => {
            let copy = items[i..=last].to_vec();
            let after = last + 1;
            items.splice(after..after, copy);
        }
        7 if !closes => {
            let next = last + 1;
            if next < n && !is_pop(&items[next]) {
                let next_last = if ends[next] > next { ends[next] } else { next };
                let mut swapped = items[next..=next_last].to_vec();
                swapped.extend_from_slice(&items[i..=last]);
                items.splice(i..=next_last, swapped);
            }
        }
        _ => {}
    }
}

fn params(scale: f32) -> RasterParams {
    RasterParams {
        scroll: Point::new(0.0, 0.0),
        viewport_scroll: Point::new(0.0, 0.0),
        scale,
    }
}

fn full(list: &DisplayList, fonts: &mut FontContext, size: (u32, u32), p: RasterParams) -> Pixmap {
    let mut pixmap = Pixmap::new(size.0, size.1).expect("the test target is not empty");
    crate::fill(&mut pixmap, Rgba::WHITE);
    rasterize(list, &mut pixmap, p, fonts, &NoImages);
    pixmap
}

fn finished(items: &[DisplayItem]) -> DisplayList {
    let mut items = items.to_vec();
    crate::group_bounds::finish_groups(&mut items);
    DisplayList { items }
}

/// The raster of `old` with the damage of `new` drawn into it, compared
/// with a full raster of `new`: `None` if equal (or a full raster), else
/// the number of different pixels and the largest channel difference.
fn mismatch(
    old: &[DisplayItem],
    new: &[DisplayItem],
    size: (u32, u32),
    p: RasterParams,
    fonts: &mut FontContext,
) -> Option<(usize, i32)> {
    let (old, new) = (finished(old), finished(new));
    let mut kept = full(&old, fonts, size, p);
    let (old_keys, new_keys) = (media_keys(&old, &NoImages), media_keys(&new, &NoImages));
    let found = list_damage(
        Side {
            list: &old,
            keys: &old_keys,
        },
        Side {
            list: &new,
            keys: &new_keys,
        },
        p,
    );
    if found.full {
        return None;
    }
    let boxes = backdrop_boxes(&new, &new_keys, p);
    match merge_damage(&found.rects, &boxes, size) {
        Damage::Full => return None,
        Damage::None => {}
        Damage::Rects(parts) => {
            rasterize_region(
                &new,
                &mut kept,
                p,
                &parts,
                Some(Rgba::WHITE),
                fonts,
                &NoImages,
            );
        }
    }
    let want = full(&new, fonts, size, p);
    let different: Vec<i32> = kept
        .data()
        .chunks(4)
        .zip(want.data().chunks(4))
        .filter(|(a, b)| a != b)
        .map(|(a, b)| {
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let largest = different.iter().copied().max()?;
    Some((different.len(), largest))
}

/// Runs `count` random pairs; returns the cases that differ.
fn run(seed: u64, count: usize, max_items: usize, scale: f32) -> Vec<String> {
    let mut fonts = FontContext::for_tests();
    let mut rng = Rng(seed);
    let size = ((PAGE.0 * scale) as u32, (PAGE.1 * scale) as u32);
    let p = params(scale);
    let mut failures = Vec::new();
    for case in 0..count {
        let mut old = Vec::new();
        let n = 2 + rng.below(max_items);
        generate(&mut rng, &mut old, 0, n);
        let mut new = old.clone();
        for _ in 0..=rng.below(3) {
            mutate(&mut rng, &mut new);
        }
        if let Some((pixels, largest)) = mismatch(&old, &new, size, p, &mut fonts) {
            failures.push(format!(
                "seed {seed:#x} case {case} scale {scale}: {pixels} px differ, \
                 at most {largest} levels"
            ));
        }
    }
    failures
}

#[test]
fn random_list_pairs_render_like_a_full_raster() {
    let mut failures = Vec::new();
    for (seed, count, max_items, scale) in [
        (0x9e37_79b9_7f4a_7c15_u64, 1500, 6, 1.0),
        (0x0123_4567_u64, 1500, 14, 1.0),
        (0xdead_beef_u64, 1000, 30, 1.0),
        (0x00ab_cdef_u64, 800, 12, 1.5),
    ] {
        failures.extend(run(seed, count, max_items, scale));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An equal rounded rectangle in an opacity group whose bounds grow: the
/// group's layer changes its extent, and with it the anti-aliasing of the
/// rectangle, so the whole group is damage (found in the review of feature
/// 4b: 4 px at up to 9 levels at scale 1.5).
#[test]
fn an_opacity_group_whose_bounds_change_is_redrawn_whole() {
    let mut fonts = FontContext::for_tests();
    let mut failures = Vec::new();
    for scale in [1.0, 1.5] {
        let size = ((PAGE.0 * scale) as u32, (PAGE.1 * scale) as u32);
        for x in [50.0, 100.0, 120.0, 132.35, 140.0] {
            for added_x in [0.0, 10.0, 30.0] {
                for added_height in [10.0, 40.0] {
                    let kept = DisplayItem::Rect {
                        rect: Rect::new(x, 20.0, 28.9, 47.4),
                        radii: [(8.0, 8.0); 4],
                        color: Rgba::new(121, 199, 184, 255),
                    };
                    let added = DisplayItem::Rect {
                        rect: Rect::new(added_x, 10.0, 10.0, added_height),
                        radii: [(0.0, 0.0); 4],
                        color: Rgba::new(1, 169, 100, 255),
                    };
                    let group = |inner: Vec<DisplayItem>| {
                        let mut items = vec![DisplayItem::opacity_group(0.5, MixBlendMode::Normal)];
                        items.extend(inner);
                        items.push(DisplayItem::PopOpacity);
                        items
                    };
                    let old = group(vec![kept.clone()]);
                    let new = group(vec![added, kept]);
                    if let Some(found) = mismatch(&old, &new, size, params(scale), &mut fonts) {
                        failures.push(format!("scale {scale} x {x} added at {added_x}: {found:?}"));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
