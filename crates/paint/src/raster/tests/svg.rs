//! Tests of the inline SVG items: paths, their work budget and clip
//! groups (ADR 0023, ADR 0024).

use super::*;

fn svg_path(points: &[(f32, f32)]) -> Arc<swb_layout::svg::SvgPath> {
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

fn fill_path(path: Arc<swb_layout::svg::SvgPath>, transform: Matrix, color: Rgba) -> DisplayItem {
    DisplayItem::FillPath {
        path,
        transform,
        color,
        rule: swb_style::FillRule::NonZero,
        anti_alias: true,
    }
}

fn stroke(width: f32, dashes: Option<&[f32]>) -> Arc<swb_layout::svg::StrokeStyle> {
    Arc::new(swb_layout::svg::StrokeStyle {
        width,
        cap: swb_style::StrokeLinecap::Butt,
        join: swb_style::StrokeLinejoin::Miter,
        miter_limit: 4.0,
        dashes: dashes.map(Arc::from),
        dash_offset: 0.0,
    })
}

#[test]
fn svg_paths_fill_and_stroke_with_their_transform() {
    // A 10x10 square scaled by 4 and moved to (10, 10).
    let square = svg_path(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    let m = Matrix::new(4.0, 0.0, 0.0, 4.0, 10.0, 10.0);
    let p = render(vec![fill_path(Arc::clone(&square), m, RED)]);
    assert_eq!(rgb(&p, 12, 12), (255, 0, 0));
    assert_eq!(rgb(&p, 48, 48), (255, 0, 0));
    assert_eq!(rgb(&p, 52, 30), (255, 255, 255));
    // The stroke scales with the transform: 2 units are 8 px.
    let p = render(vec![DisplayItem::StrokePath {
        path: square,
        transform: m,
        color: BLUE,
        stroke: stroke(2.0, None),
        anti_alias: true,
    }]);
    assert_eq!(rgb(&p, 7, 30), (0, 0, 255));
    assert_eq!(rgb(&p, 13, 30), (0, 0, 255));
    assert_eq!(rgb(&p, 30, 30), (255, 255, 255));
    // At scale 2 the same items cover twice the device pixels.
    let p = render_with(
        vec![fill_path(
            svg_path(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]),
            Matrix::IDENTITY,
            RED,
        )],
        2.0,
        &NoImages,
    );
    assert_eq!(rgb(&p, 18, 2), (255, 0, 0));
    assert_eq!(rgb(&p, 2, 18), (255, 255, 255));
}

#[test]
fn svg_paths_with_hostile_geometry_draw_nothing() {
    let square = svg_path(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    for m in [
        Matrix::new(1e30, 0.0, 0.0, 1e30, 0.0, 0.0),
        Matrix::new(f32::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0),
        Matrix::new(0.0, 0.0, 0.0, 0.0, 10.0, 10.0),
        Matrix::new(1e12, 0.0, 0.0, 1e12, -1e13, -1e13),
    ] {
        let p = render(vec![
            fill_path(Arc::clone(&square), m, RED),
            DisplayItem::StrokePath {
                path: Arc::clone(&square),
                transform: m,
                color: RED,
                stroke: stroke(1e30, Some(&[1e-30, 1e-30])),
                anti_alias: true,
            },
        ]);
        // Nothing or a fill of the whole target; no panic.
        let _ = rgb(&p, 50, 50);
    }
}

#[test]
fn svg_strokes_with_too_many_dashes_are_solid() {
    use swb_layout::svg::{PathSegment, SvgPath};
    let line = Arc::new(
        SvgPath::from_segments(&[
            PathSegment::MoveTo(Point::new(0.0, 50.0)),
            PathSegment::LineTo(Point::new(100.0, 50.0)),
        ])
        .expect("a path"),
    );
    let item = |dashes: &[f32]| DisplayItem::StrokePath {
        path: Arc::clone(&line),
        transform: Matrix::IDENTITY,
        color: RED,
        stroke: stroke(10.0, Some(dashes)),
        anti_alias: true,
    };
    // 10 on, 10 off: the gap at x = 15 is white.
    let p = render(vec![item(&[10.0, 10.0])]);
    assert_eq!(rgb(&p, 5, 50), (255, 0, 0));
    assert_eq!(rgb(&p, 15, 50), (255, 255, 255));
    // 100 / 0.001 * 2 dashes, more than the limit.
    let tiny = 0.0005;
    assert!(100.0 / (2.0 * tiny) * 2.0 > crate::path_cost::MAX_DASHES);
    let mut target = Pixmap::new(100, 100).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    let mut r = rasterizer(
        &mut target,
        &mut vectors,
        MAX_GROUP_LAYER_PIXELS,
        MAX_MASK_PIXELS,
    );
    let items = [item(&[tiny, tiny])];
    let mut fonts = FontContext::for_tests();
    r.run(&items, 0..1, &transform_ends(&items), &mut fonts, &NoImages);
    assert!(r.budget.skipped.dashes);
    assert_eq!(rgb(&target, 15, 50), (255, 0, 0));
}

#[test]
fn svg_paths_beyond_the_work_budget_are_not_drawn() {
    let square = svg_path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]);
    let items = [
        fill_path(Arc::clone(&square), Matrix::IDENTITY, RED),
        fill_path(square, Matrix::translate(0.0, 50.0), BLUE),
    ];
    let mut target = Pixmap::new(100, 100).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    let mut r = rasterizer(
        &mut target,
        &mut vectors,
        MAX_GROUP_LAYER_PIXELS,
        MAX_MASK_PIXELS,
    );
    // The first fill costs its fixed part, about 100 x 100 pixels at 8
    // units and its edges (two of 100 rows at 200 units): about 123,000.
    r.budget.max_path_work = 150_000.0;
    let mut fonts = FontContext::for_tests();
    r.run(&items, 0..2, &transform_ends(&items), &mut fonts, &NoImages);
    assert!(r.budget.skipped.paths);
    assert_eq!(rgb(&target, 50, 75), (255, 0, 0));
}

/// A clip shape of `points` in a clip path of the SVG content.
fn clip_shape(
    points: &[(f32, f32)],
    transform: Matrix,
    clip: Option<swb_layout::svg::ClipRegion>,
) -> swb_layout::svg::ClipShape {
    swb_layout::svg::ClipShape {
        path: svg_path(points),
        transform,
        rule: swb_style::FillRule::NonZero,
        clip,
    }
}

/// The items of `content` inside an SVG clip group at (`dx`, 0).
fn clipped(clip: swb_layout::svg::ClipPath, dx: f32, content: DisplayItem) -> Vec<DisplayItem> {
    vec![
        DisplayItem::PushSvgClip {
            clip: Arc::new(clip),
            transform: Matrix::translate(dx, 0.0),
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            rect_fallback: false,
        },
        content,
        DisplayItem::PopSvgClip,
    ]
}

fn full_square() -> DisplayItem {
    let square = svg_path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]);
    fill_path(square, Matrix::IDENTITY, RED)
}

/// A path of `curves` cubic curves that all reach across the height
/// of a 100 x 100 box and overlap, as in the repro of ADR 0023.
fn dense_path(curves: usize) -> Arc<swb_layout::svg::SvgPath> {
    use swb_layout::svg::{PathSegment, SvgPath};
    let p = Point::new;
    let mut segments = vec![PathSegment::MoveTo(p(0.0, 0.0))];
    for i in 0..curves {
        let k = (i % 50) as f32;
        segments.push(PathSegment::CubicTo(
            p(k, 0.0),
            p(100.0 - k, 100.0),
            p(((7 * i) % 98) as f32, 50.0),
        ));
    }
    Arc::new(SvgPath::from_segments(&segments).expect("a path"))
}

/// Runs `items` on a white 100 x 100 target with the default path
/// budget; returns the target and whether paths were skipped.
fn run_with_path_budget(items: &[DisplayItem]) -> (Pixmap, bool) {
    let mut target = Pixmap::new(100, 100).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    let mut r = rasterizer(
        &mut target,
        &mut vectors,
        MAX_GROUP_LAYER_PIXELS,
        MAX_MASK_PIXELS,
    );
    let mut fonts = FontContext::for_tests();
    r.run(
        items,
        0..items.len(),
        &transform_ends(items),
        &mut fonts,
        &NoImages,
    );
    let skipped = r.budget.skipped.paths;
    (target, skipped)
}

#[test]
fn dense_svg_paths_cost_by_their_overlapping_edges() {
    // 40,000 curves take seconds to fill (the pairs of edges that
    // overlap in y grow with the square of the curves); 300 are cheap.
    let anti_aliased = |curves| fill_path(dense_path(curves), Matrix::IDENTITY, RED);
    let (_, skipped) = run_with_path_budget(&[anti_aliased(300)]);
    assert!(!skipped);
    let (target, skipped) = run_with_path_budget(&[anti_aliased(40_000)]);
    assert!(skipped);
    assert_eq!(rgb(&target, 50, 50), (255, 255, 255));
    // The same without anti-aliasing (the curves are cut into lines),
    // and as a stroke.
    let crisp = DisplayItem::FillPath {
        path: dense_path(40_000),
        transform: Matrix::IDENTITY,
        color: RED,
        rule: swb_style::FillRule::NonZero,
        anti_alias: false,
    };
    assert!(run_with_path_budget(&[crisp]).1);
    let stroked = DisplayItem::StrokePath {
        path: dense_path(40_000),
        transform: Matrix::IDENTITY,
        color: RED,
        stroke: stroke(2.0, None),
        anti_alias: true,
    };
    assert!(run_with_path_budget(&[stroked]).1);
}

/// A path from `segments` of the page-size tests of dense charts.
fn path_of(segments: &[swb_layout::svg::PathSegment]) -> Arc<swb_layout::svg::SvgPath> {
    Arc::new(swb_layout::svg::SvgPath::from_segments(segments).expect("a path"))
}

/// Runs `item` on a white 1200 x 780 target with the default path
/// budget; true if the path was skipped.
fn skipped_on_page(item: DisplayItem) -> bool {
    let mut target = Pixmap::new(1200, 780).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    let mut r = rasterizer(
        &mut target,
        &mut vectors,
        MAX_GROUP_LAYER_PIXELS,
        MAX_MASK_PIXELS,
    );
    let mut fonts = FontContext::for_tests();
    let items = [item];
    r.run(&items, 0..1, &transform_ends(&items), &mut fonts, &NoImages);
    r.budget.skipped.paths
}

#[test]
fn realistic_dense_paths_are_drawn() {
    use swb_layout::svg::PathSegment as S;
    // Pairs of edges that overlap in y without crossing are cheap
    // (ADR 0023, part 3): a noisy line chart of 40,000 points with a
    // thin stroke, a histogram of 5,000 bars in one path (10,000
    // edges the height of the page), and a random walk of 60,000
    // segments all draw.
    let mut state = 3u32;
    let mut random = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1u32 << 24) as f32
    };
    let p = Point::new;
    let mut chart = vec![S::MoveTo(p(0.0, 400.0))];
    for i in 0..40_000 {
        chart.push(S::LineTo(p(i as f32 * 0.03, 50.0 + random() * 680.0)));
    }
    let stroked = |path, width| DisplayItem::StrokePath {
        path,
        transform: Matrix::IDENTITY,
        color: RED,
        stroke: stroke(width, None),
        anti_alias: true,
    };
    assert!(!skipped_on_page(stroked(path_of(&chart), 1.0)));
    // A wider stroke has an outline with edges to cross: it costs
    // five times as much.
    assert!(skipped_on_page(stroked(path_of(&chart), 10.0)));
    let mut bars = vec![];
    for i in 0..5_000 {
        let x = i as f32 * 0.24;
        bars.push(S::MoveTo(p(x, 780.0)));
        bars.push(S::LineTo(p(x, 20.0 + random() * 680.0)));
        bars.push(S::LineTo(p(x + 0.5, 100.0)));
        bars.push(S::LineTo(p(x + 0.5, 780.0)));
        bars.push(S::Close);
    }
    let fill = |path| DisplayItem::FillPath {
        path,
        transform: Matrix::IDENTITY,
        color: RED,
        rule: swb_style::FillRule::NonZero,
        anti_alias: true,
    };
    assert!(!skipped_on_page(fill(path_of(&bars))));
    let (mut x, mut y) = (600.0f32, 400.0f32);
    let mut walk = vec![S::MoveTo(p(x, y))];
    for _ in 0..60_000 {
        x = (x + random() * 6.0 - 3.0).clamp(0.0, 1200.0);
        y = (y + random() * 6.0 - 3.0).clamp(0.0, 780.0);
        walk.push(S::LineTo(p(x, y)));
    }
    assert!(!skipped_on_page(stroked(path_of(&walk), 1.0)));
}

#[test]
fn fills_of_separate_thin_spans_are_charged() {
    use swb_layout::svg::PathSegment as S;
    let p = Point::new;
    // `teeth` separate rectangles across 1,200 px, half as wide as their
    // pitch, from `top` to `bottom`.
    let comb = |teeth: usize, top: f32, bottom: f32| {
        let pitch = 1200.0 / teeth as f32;
        let mut segments = vec![];
        for i in 0..teeth {
            let x = i as f32 * pitch;
            segments.extend([
                S::MoveTo(p(x, top)),
                S::LineTo(p(x, bottom)),
                S::LineTo(p(x + pitch / 2.0, bottom)),
                S::LineTo(p(x + pitch / 2.0, top)),
                S::Close,
            ]);
        }
        path_of(&segments)
    };
    let fill = |path, anti_alias| DisplayItem::FillPath {
        path,
        transform: Matrix::IDENTITY,
        color: RED,
        rule: swb_style::FillRule::NonZero,
        anti_alias,
    };
    // 3,000 teeth 0.2 px wide take 6 s with anti-aliasing (ADR 0023,
    // part 3), 16 ms without; 300 teeth 2 px wide take 10 ms.
    assert!(skipped_on_page(fill(comb(3_000, 0.0, 780.0), true)));
    assert!(!skipped_on_page(fill(comb(3_000, 0.0, 780.0), false)));
    assert!(!skipped_on_page(fill(comb(300, 0.0, 780.0), true)));
    // One of ten stacked combs of 78 px takes 0.6 s.
    assert!(skipped_on_page(fill(comb(3_000, 78.0, 156.0), true)));
    // The area under a noisy chart: 40,000 points take 1.6 s, 2,000
    // points 20 ms.
    let mut state = 5u32;
    let mut random = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut area = |points: usize| {
        let mut segments = vec![S::MoveTo(p(0.0, 780.0))];
        for i in 0..=points {
            let x = i as f32 * 1200.0 / points as f32;
            segments.push(S::LineTo(p(x, 100.0 + random() * 600.0)));
        }
        segments.extend([S::LineTo(p(1200.0, 780.0)), S::Close]);
        path_of(&segments)
    };
    assert!(skipped_on_page(fill(area(40_000), true)));
    assert!(!skipped_on_page(fill(area(2_000), true)));
}

#[test]
fn strokes_of_thin_dashes_are_charged() {
    use swb_layout::svg::PathSegment as S;
    let line = |y: f32| {
        path_of(&[
            S::MoveTo(Point::new(0.0, y)),
            S::LineTo(Point::new(1200.0, y)),
        ])
    };
    let stroked = |path, width, dashes: Option<&[f32]>| DisplayItem::StrokePath {
        path,
        transform: Matrix::IDENTITY,
        color: RED,
        stroke: stroke(width, dashes),
        anti_alias: true,
    };
    // A line 780 px wide with 0.2 px dashes and gaps takes 6 s (ADR
    // 0023, part 3); with dashes of 2 px, 11 ms.
    assert!(skipped_on_page(stroked(
        line(390.0),
        780.0,
        Some(&[0.2, 0.2])
    )));
    assert!(!skipped_on_page(stroked(
        line(390.0),
        780.0,
        Some(&[2.0, 2.0])
    )));
    // Thin dashes of a line that is 3 px wide take 24 ms; a solid line
    // 780 px wide, 3 ms.
    assert!(!skipped_on_page(stroked(
        line(390.0),
        3.0,
        Some(&[0.2, 0.2])
    )));
    assert!(!skipped_on_page(stroked(line(390.0), 780.0, None)));
    // A hairline has no outline.
    assert!(!skipped_on_page(stroked(
        line(390.0),
        1.0,
        Some(&[0.2, 0.2])
    )));
}

#[test]
fn dense_svg_clip_paths_hide_the_group() {
    use swb_layout::svg::{ClipPath, ClipShape};
    let clip = |curves| ClipPath {
        shapes: vec![ClipShape {
            path: dense_path(curves),
            transform: Matrix::IDENTITY,
            rule: swb_style::FillRule::NonZero,
            clip: None,
        }],
        outer: None,
    };
    let (_, skipped) = run_with_path_budget(&clipped(clip(300), 0.0, full_square()));
    assert!(!skipped);
    let (target, skipped) = run_with_path_budget(&clipped(clip(40_000), 0.0, full_square()));
    assert!(skipped);
    assert_eq!(rgb(&target, 50, 50), (255, 255, 255));
}

#[test]
fn svg_clip_groups_show_the_union_of_the_clip_shapes() {
    use swb_layout::svg::{ClipPath, ClipRegion};
    let id = Matrix::IDENTITY;
    let left = clip_shape(
        &[(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)],
        id,
        None,
    );
    let right = clip_shape(
        &[(60.0, 0.0), (100.0, 0.0), (100.0, 40.0), (60.0, 40.0)],
        id,
        None,
    );
    let clip = ClipPath {
        shapes: vec![left, right],
        outer: None,
    };
    let p = render(clipped(clip, 0.0, full_square()));
    assert_eq!(rgb(&p, 20, 20), (255, 0, 0));
    assert_eq!(rgb(&p, 80, 20), (255, 0, 0));
    assert_eq!(rgb(&p, 50, 20), (255, 255, 255));
    assert_eq!(rgb(&p, 20, 60), (255, 255, 255));
    // The `transform` of the group moves the clip shapes with it.
    let shifted = ClipPath {
        shapes: vec![clip_shape(
            &[(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)],
            id,
            None,
        )],
        outer: None,
    };
    let p = render(clipped(shifted, 50.0, full_square()));
    assert_eq!(rgb(&p, 20, 20), (255, 255, 255));
    assert_eq!(rgb(&p, 70, 20), (255, 0, 0));
    // The `outer` region is intersected: here the left half of the
    // shapes only.
    let outer = ClipPath {
        shapes: vec![clip_shape(
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 40.0), (0.0, 40.0)],
            id,
            None,
        )],
        outer: Some(ClipRegion::Rect(Rect::new(0.0, 0.0, 50.0, 100.0))),
    };
    let p = render(clipped(outer, 0.0, full_square()));
    assert_eq!(rgb(&p, 20, 20), (255, 0, 0));
    assert_eq!(rgb(&p, 80, 20), (255, 255, 255));
    // A shape with its own clip: the part inside it.
    let own = ClipPath {
        shapes: vec![clip_shape(
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
            id,
            Some(ClipRegion::Rect(Rect::new(50.0, 50.0, 50.0, 50.0))),
        )],
        outer: None,
    };
    let p = render(clipped(own, 0.0, full_square()));
    assert_eq!(rgb(&p, 20, 20), (255, 255, 255));
    assert_eq!(rgb(&p, 70, 70), (255, 0, 0));
}

#[test]
fn an_empty_svg_clip_hides_the_group() {
    use swb_layout::svg::ClipPath;
    let clip = ClipPath {
        shapes: Vec::new(),
        outer: None,
    };
    let p = render(clipped(clip, 0.0, full_square()));
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
}

#[test]
fn svg_clip_groups_are_bounded_by_the_layer_budget() {
    use swb_layout::svg::ClipPath;
    let id = Matrix::IDENTITY;
    let clip = ClipPath {
        shapes: vec![clip_shape(
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
            id,
            None,
        )],
        outer: None,
    };
    let items = clipped(clip, 0.0, full_square());
    let mut target = Pixmap::new(100, 100).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    // The group needs the layer and a temporary one for the coverage.
    let mut r = rasterizer(&mut target, &mut vectors, 100 * 100, MAX_MASK_PIXELS);
    let mut fonts = FontContext::for_tests();
    r.run(&items, 0..3, &transform_ends(&items), &mut fonts, &NoImages);
    assert!(r.budget.skipped.layers);
    assert_eq!(r.budget.layer_pixels, 0);
    // The group draws nothing when its clip cannot be made.
    assert_eq!(rgb(&target, 50, 50), (255, 255, 255));
}

/// A clip group over a small triangle with the rectangle fallback of a
/// rounded overflow clip, run with the given layer budget.
fn fallback_group(max_layer_pixels: u64) -> (Pixmap, bool) {
    use swb_layout::svg::ClipPath;
    let clip = ClipPath {
        shapes: vec![clip_shape(
            &[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)],
            Matrix::IDENTITY,
            None,
        )],
        outer: None,
    };
    let mut items = clipped(clip, 0.0, full_square());
    if let Some(DisplayItem::PushSvgClip { rect_fallback, .. }) = items.first_mut() {
        *rect_fallback = true;
    }
    let mut target = Pixmap::new(100, 100).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    let mut r = rasterizer(&mut target, &mut vectors, max_layer_pixels, MAX_MASK_PIXELS);
    let mut fonts = FontContext::for_tests();
    r.run(&items, 0..3, &transform_ends(&items), &mut fonts, &NoImages);
    let skipped = r.budget.skipped.layers;
    assert_eq!(r.budget.layer_pixels, 0);
    assert_eq!(r.clips.len(), 0);
    (target, skipped)
}

#[test]
fn a_rounded_overflow_clip_without_a_layer_keeps_its_rectangle() {
    let (target, skipped) = fallback_group(100 * 100 - 1);
    assert!(skipped);
    // The content is drawn, clipped to the bounds (the whole target).
    assert_eq!(rgb(&target, 50, 50), (255, 0, 0));
}

#[test]
fn a_rounded_overflow_clip_without_room_for_coverage_keeps_its_rectangle() {
    // The layer fits, the temporary layer of the coverage does not.
    let (target, skipped) = fallback_group(100 * 100);
    assert!(skipped);
    assert_eq!(rgb(&target, 50, 50), (255, 0, 0));
    // With room for both, the clip applies.
    let (target, skipped) = fallback_group(2 * 100 * 100);
    assert!(!skipped);
    assert_eq!(rgb(&target, 50, 50), (255, 255, 255));
    assert_eq!(rgb(&target, 2, 2), (255, 0, 0));
}

#[test]
fn svg_fills_without_anti_aliasing_cover_the_pixels_with_centers_inside() {
    // A circle of radius 15.3 around (20, 20): 732 pixel centers are
    // inside (the count that Chromium draws with `crispEdges`).
    use swb_layout::svg::{PathSegment, SvgPath};
    let (cx, cy, r, k) = (20.0_f32, 20.0_f32, 15.3_f32, swb_layout::KAPPA * 15.3);
    let p = Point::new;
    let circle = Arc::new(
        SvgPath::from_segments(&[
            PathSegment::MoveTo(p(cx + r, cy)),
            PathSegment::CubicTo(p(cx + r, cy + k), p(cx + k, cy + r), p(cx, cy + r)),
            PathSegment::CubicTo(p(cx - k, cy + r), p(cx - r, cy + k), p(cx - r, cy)),
            PathSegment::CubicTo(p(cx - r, cy - k), p(cx - k, cy - r), p(cx, cy - r)),
            PathSegment::CubicTo(p(cx + k, cy - r), p(cx + r, cy - k), p(cx + r, cy)),
            PathSegment::Close,
        ])
        .expect("a path"),
    );
    let item = |anti_alias| DisplayItem::FillPath {
        path: Arc::clone(&circle),
        transform: Matrix::IDENTITY,
        color: Rgba::BLACK,
        rule: swb_style::FillRule::NonZero,
        anti_alias,
    };
    let count = |anti_alias| {
        let pixmap = render(vec![item(anti_alias)]);
        let mut exact = 0;
        let mut partial = 0;
        for y in 0..40 {
            for x in 0..40 {
                match rgb(&pixmap, x, y) {
                    (0, 0, 0) => exact += 1,
                    (255, 255, 255) => {}
                    _ => partial += 1,
                }
            }
        }
        (exact, partial)
    };
    assert_eq!(count(false), (732, 0));
    assert!(count(true).1 > 20);
}
