//! Tests of the raster cache (`raster/cache.rs`, ADR 0044 section 19): an
//! entry gives the pixels of a raster without it, bit for bit; the parts of
//! the key, the budgets and the region raster.

use super::*;

/// No images, with a raster cache.
#[derive(Default)]
struct Cached(RasterCache);

impl ImageSource for Cached {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        None
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        None
    }

    fn raster_cache(&self) -> Option<&RasterCache> {
        Some(&self.0)
    }
}

/// A polygon path through `points`.
fn polygon_path(points: &[(f32, f32)]) -> Arc<swb_layout::svg::SvgPath> {
    use swb_layout::svg::{PathSegment, SvgPath};
    let mut segments: Vec<PathSegment> = points
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            let p = Point::new(x, y);
            if i == 0 {
                PathSegment::MoveTo(p)
            } else {
                PathSegment::LineTo(p)
            }
        })
        .collect();
    segments.push(PathSegment::Close);
    Arc::new(SvgPath::from_segments(&segments).expect("a path"))
}

/// A rounded overflow clip (an octagon with slanted corners, so that the
/// coverage has partial values) around red and blue rectangles; the clip
/// path is a new `Arc` in each call, as in each display list.
fn clipped_items(dx: f32) -> Vec<DisplayItem> {
    use swb_layout::svg::{ClipPath, ClipShape};
    let octagon = [
        (10.3, 0.0),
        (79.7, 0.0),
        (90.0, 10.3),
        (90.0, 69.7),
        (79.7, 80.0),
        (10.3, 80.0),
        (0.0, 69.7),
        (0.0, 10.3),
    ];
    let clip = ClipPath {
        shapes: vec![ClipShape {
            path: polygon_path(&octagon),
            transform: Matrix::IDENTITY,
            rule: swb_style::FillRule::NonZero,
            clip: None,
        }],
        outer: None,
    };
    let rect = |x: f32, y: f32, color| DisplayItem::Rect {
        rect: Rect::new(x, y, 60.0, 50.0),
        radii: [(0.0, 0.0); 4],
        color,
    };
    vec![
        DisplayItem::PushSvgClip {
            clip: Arc::new(clip),
            transform: Matrix::translate(dx, 5.5),
            bounds: Rect::new(dx, 5.5, 90.0, 80.0),
            rect_fallback: true,
            backdrop_root: false,
        },
        rect(dx - 5.0, 0.0, RED),
        rect(dx + 40.0, 40.0, BLUE),
        DisplayItem::PopSvgClip,
    ]
}

/// Rasterizes `items` on a white 100 x 100 target at `scale`, in full or in
/// the parts `region`.
fn draw(
    items: Vec<DisplayItem>,
    scale: f32,
    images: &dyn ImageSource,
    region: Option<&[PixelRect]>,
) -> Pixmap {
    let size = (100.0 * scale) as u32;
    let mut target = Pixmap::new(size, size).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let params = RasterParams {
        scroll: Point::default(),
        viewport_scroll: Point::default(),
        scale,
    };
    let mut fonts = FontContext::for_tests();
    let list = DisplayList { items };
    match region {
        None => {
            rasterize(&list, &mut target, params, &mut fonts, images);
        }
        Some(parts) => {
            rasterize_region(
                &list,
                &mut target,
                params,
                parts,
                Some(Rgba::WHITE),
                &mut fonts,
                images,
            );
        }
    }
    target
}

#[test]
fn a_kept_clip_coverage_gives_the_pixels_of_a_raster_without_it() {
    for scale in [1.0, 1.5] {
        let without = draw(clipped_items(4.25), scale, &NoImages, None);
        let cached = Cached::default();
        // A coverage is kept on its second miss.
        let cold = draw(clipped_items(4.25), scale, &cached, None);
        draw(clipped_items(4.25), scale, &cached, None);
        assert_eq!(cached.0.stats().misses, 2);
        let warm = draw(clipped_items(4.25), scale, &cached, None);
        let stats = cached.0.stats();
        assert_eq!((stats.hits, stats.entries), (1, 1), "scale {scale}");
        assert_eq!(cold.data(), without.data(), "scale {scale}");
        assert_eq!(warm.data(), without.data(), "scale {scale}");
        // A region raster with the kept coverage: its parts equal a full
        // raster.
        let parts = [
            PixelRect::from_edges(0, 0, 30, 25),
            PixelRect::from_edges(40, 50, 90, 90),
        ];
        let region = draw(clipped_items(4.25), scale, &cached, Some(&parts));
        assert_eq!(cached.0.stats().hits, 2);
        for part in parts {
            let part = part.clamp_to(region.width(), region.height());
            for y in part.y..part.bottom() {
                for x in part.x..part.right() {
                    assert_eq!(region.pixel(x, y), without.pixel(x, y), "({x}, {y})");
                }
            }
        }
    }
}

#[test]
fn a_moved_clip_is_another_entry() {
    let cached = Cached::default();
    draw(clipped_items(4.25), 1.0, &cached, None);
    draw(clipped_items(4.25), 1.0, &cached, None);
    // Moved by a fraction: another matrix, another coverage.
    let moved = draw(clipped_items(4.75), 1.0, &cached, None);
    assert_eq!(cached.0.stats().hits, 0);
    assert_eq!(
        moved.data(),
        draw(clipped_items(4.75), 1.0, &NoImages, None).data()
    );
}

#[test]
fn a_kept_coverage_that_does_not_fit_the_budget_is_computed_again() {
    let items = clipped_items(4.25);
    let ends = transform_ends(&items);
    let cached = Cached::default();
    // Warm the cache with the full budgets.
    draw(items.clone(), 1.0, &cached, None);
    draw(items.clone(), 1.0, &cached, None);
    let run = |images: &dyn ImageSource, max_path_work: f64| {
        let mut target = Pixmap::new(100, 100).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let mut vectors = FrameBudget::new();
        let mut r = rasterizer(
            &mut target,
            &mut vectors,
            MAX_GROUP_LAYER_PIXELS,
            MAX_MASK_PIXELS,
        );
        r.budget.max_path_work = max_path_work;
        let mut fonts = FontContext::for_tests();
        r.run(&items, 0..items.len(), &ends, &mut fonts, images);
        let (skipped, work) = (r.budget.skipped.paths, r.budget.path_work);
        (target, skipped, work)
    };
    // With the whole budget the hit charges what the coverage charged.
    let (_, _, cold_work) = run(&NoImages, path::MAX_PATH_WORK);
    let (_, skipped, warm_work) = run(&cached, path::MAX_PATH_WORK);
    assert!(!skipped);
    assert_eq!(warm_work.to_bits(), cold_work.to_bits());
    // With too little budget the hit is not used: the raster draws what it
    // draws without the cache (the fallback rectangle).
    let (without, skipped_without, _) = run(&NoImages, 1000.0);
    let (with, skipped_with, _) = run(&cached, 1000.0);
    assert!(skipped_without && skipped_with);
    assert_eq!(with.data(), without.data());
}

/// A composited transform group (rotated and scaled) with its own opacity
/// group around the clipped content of [`clipped_items`].
fn composited_items(angle: f32, opacity: f32) -> Vec<DisplayItem> {
    let (s, c) = angle.to_radians().sin_cos();
    let k = 0.9;
    let m = Matrix::new(k * c, k * s, -k * s, k * c, 12.5, 3.25);
    let mut items = vec![
        DisplayItem::PushTransform {
            transform: GroupTransform::Matrix(m),
            bounds: Rect::new(0.0, 0.0, 100.0, 90.0),
            hit_bounds: Rect::new(0.0, 0.0, 100.0, 90.0),
            fixed_bounds: None,
            composited: true,
        },
        DisplayItem::PushOpacity {
            opacity,
            blend: MixBlendMode::Normal,
            bounds: Rect::new(0.0, 0.0, 100.0, 90.0),
            fixed_bounds: None,
            escapes_clips: false,
        },
    ];
    items.extend(clipped_items(4.25));
    items.push(DisplayItem::PopOpacity);
    items.push(DisplayItem::PopTransform);
    items
}

#[test]
fn a_composited_group_draws_its_kept_layer_with_each_transform_and_opacity() {
    for scale in [1.0, 1.5] {
        let cached = Cached::default();
        for (angle, opacity) in [(0.0, 0.0), (5.0, 0.4), (10.0, 0.8), (15.0, 1.0)] {
            let without = draw(composited_items(angle, opacity), scale, &NoImages, None);
            let with = draw(composited_items(angle, opacity), scale, &cached, None);
            assert_eq!(with.data(), without.data(), "{angle} {opacity} at {scale}");
            let parts = [PixelRect::from_edges(10, 20, 60, 70)];
            let region = draw(
                composited_items(angle, opacity),
                scale,
                &cached,
                Some(&parts),
            );
            for y in 20..70 {
                for x in 10..60 {
                    assert_eq!(region.pixel(x, y), without.pixel(x, y), "({x}, {y})");
                }
            }
        }
        // The layer was drawn once, at opacity 0 (it shows nothing then),
        // and found in the other rasters.
        let stats = cached.0.stats();
        assert!(stats.hits >= 6, "{stats:?}");
        // Opacity 0 draws nothing.
        let empty = draw(composited_items(0.0, 0.0), scale, &cached, None);
        assert!(empty.data().iter().all(|&b| b == 255));
    }
}

#[test]
fn a_switched_off_cache_finds_and_keeps_nothing() {
    let cached = Cached::default();
    cached.0.set_enabled(false);
    draw(composited_items(5.0, 1.0), 1.0, &cached, None);
    draw(composited_items(5.0, 1.0), 1.0, &cached, None);
    let stats = cached.0.stats();
    assert_eq!((stats.hits, stats.entries), (0, 0), "{stats:?}");
    cached.0.set_enabled(true);
    draw(composited_items(5.0, 1.0), 1.0, &cached, None);
    assert_eq!(cached.0.stats().entries, 1);
}

/// A layer made while an earlier item of the raster had already set a skip
/// flag, and whose own content is left out too, is not kept: a later frame
/// with free budgets draws the whole content (found in the review of
/// feature 4c).
#[test]
fn a_layer_made_after_an_earlier_skip_is_not_kept() {
    let items = composited_items(5.0, 1.0);
    let ends = transform_ends(&items);
    let cached = Cached::default();
    let run = |images: &dyn ImageSource, starved: bool| {
        let mut target = Pixmap::new(100, 100).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let mut vectors = FrameBudget::new();
        let mut r = rasterizer(
            &mut target,
            &mut vectors,
            MAX_GROUP_LAYER_PIXELS,
            MAX_MASK_PIXELS,
        );
        if starved {
            // An earlier item ran out of path work: the flag is set, and
            // the clip of the layer's content does not fit either.
            r.budget.skipped.paths = true;
            r.budget.max_path_work = 1000.0;
        }
        let mut fonts = FontContext::for_tests();
        r.run(&items, 0..items.len(), &ends, &mut fonts, images);
        target
    };
    let starved = run(&cached, true);
    let warm = run(&cached, false);
    let cold = run(&NoImages, false);
    assert!(
        starved.data() != cold.data(),
        "the starved raster leaves the clip out"
    );
    assert_eq!(cached.0.stats().hits, 0, "{:?}", cached.0.stats());
    assert!(
        warm.data() == cold.data(),
        "a later raster draws the whole content"
    );
}
