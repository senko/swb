//! Tests of the backdrop filters (`backdrop-filter`, ADR 0033) in the
//! rasterizer. `tools/probes/backdrop-filter.json` has the Chromium values
//! that the engine tests compare with.

use swb_style::FilterFunction;

use super::*;

const BLACK: Rgba = Rgba::rgb(0, 0, 0);

fn black(rect: Rect) -> DisplayItem {
    DisplayItem::Rect {
        rect,
        radii: [(0.0, 0.0); 4],
        color: BLACK,
    }
}

/// A backdrop item at `rect` with the functions `list`.
fn backdrop(list: Vec<FilterFunction>, rect: Rect, layers: u8) -> DisplayItem {
    DisplayItem::BackdropFilter {
        filter: Arc::from(list),
        current_color: BLACK,
        rect,
        radii: None,
        alpha: 1.0,
        layers,
    }
}

/// Black on the left half (x below 50) of the white target.
fn left_half() -> DisplayItem {
    black(Rect::new(0.0, 0.0, 50.0, 100.0))
}

#[test]
fn the_backdrop_is_blurred_inside_the_box_only() {
    let items = vec![
        left_half(),
        backdrop(
            vec![FilterFunction::Blur(4.0)],
            Rect::new(20.0, 20.0, 60.0, 60.0),
            0,
        ),
    ];
    let p = render(items);
    // The edge at x = 50 is a gradient inside the box.
    let (edge, _, _) = rgb(&p, 50, 50);
    assert!((100..160).contains(&edge), "{edge}");
    assert!(rgb(&p, 44, 50).0 < 90 && rgb(&p, 44, 50).0 > 0);
    // Far from the edge and outside the box nothing changes.
    assert_eq!(rgb(&p, 25, 50), (0, 0, 0));
    assert_eq!(rgb(&p, 10, 50), (0, 0, 0));
    assert_eq!(rgb(&p, 55, 10), (255, 255, 255));
    assert_eq!(rgb(&p, 90, 50), (255, 255, 255));
}

#[test]
fn the_blur_does_not_read_beyond_the_box() {
    // The box starts at the edge: the black beyond its left border does not
    // reach into it, and the border does not fade (the image is mirrored).
    let items = vec![
        left_half(),
        backdrop(
            vec![FilterFunction::Blur(4.0)],
            Rect::new(50.0, 20.0, 40.0, 60.0),
            0,
        ),
    ];
    let p = render(items);
    for x in [50, 51, 60, 89] {
        assert_eq!(rgb(&p, x, 50), (255, 255, 255), "x {x}");
    }
}

#[test]
fn color_functions_and_the_opacity_of_the_drawing() {
    let invert = vec![FilterFunction::Invert(1.0)];
    let rect = Rect::new(10.0, 10.0, 60.0, 60.0);
    let p = render(vec![left_half(), backdrop(invert.clone(), rect, 0)]);
    assert_eq!(rgb(&p, 30, 50), (255, 255, 255));
    assert_eq!(rgb(&p, 60, 50), (0, 0, 0));
    // The filtered backdrop is drawn with `alpha` over the backdrop.
    let half = DisplayItem::BackdropFilter {
        filter: Arc::from(invert),
        current_color: BLACK,
        rect,
        radii: None,
        alpha: 0.5,
        layers: 0,
    };
    let p = render(vec![left_half(), half]);
    let (r, g, b) = rgb(&p, 30, 50);
    assert!(r == g && g == b && (120..136).contains(&r), "{r} {g} {b}");
}

#[test]
fn a_rounded_box_keeps_its_corners() {
    let rect = Rect::new(10.0, 10.0, 80.0, 80.0);
    let item = DisplayItem::BackdropFilter {
        filter: Arc::from(vec![FilterFunction::Invert(1.0)]),
        current_color: BLACK,
        rect,
        radii: Some([(30.0, 30.0); 4]),
        alpha: 1.0,
        layers: 0,
    };
    let p = render(vec![black(Rect::new(0.0, 0.0, 100.0, 100.0)), item]);
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    assert_eq!(rgb(&p, 12, 12), (0, 0, 0));
    assert_eq!(rgb(&p, 87, 87), (0, 0, 0));
    assert_eq!(rgb(&p, 50, 11), (255, 255, 255));
}

#[test]
fn a_layer_is_a_backdrop_root() {
    // The black rectangle is outside the group that holds the box, so the
    // box sees nothing and the page stays as it is.
    let rect = Rect::new(10.0, 10.0, 60.0, 60.0);
    let items = vec![
        left_half(),
        DisplayItem::PushOpacity {
            opacity: 1.0,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
        backdrop(vec![FilterFunction::Invert(1.0)], rect, 0),
        DisplayItem::PopOpacity,
    ];
    let p = render(items);
    assert_eq!(rgb(&p, 30, 50), (0, 0, 0));
    assert_eq!(rgb(&p, 60, 50), (255, 255, 255));
}

#[test]
fn the_layers_of_the_box_itself_are_skipped() {
    // The box has its own opacity group (`layers` 1): its backdrop is what
    // lies below the group, and the group composites the result with 0.5.
    let rect = Rect::new(10.0, 10.0, 60.0, 60.0);
    let items = vec![
        left_half(),
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
        backdrop(vec![FilterFunction::Invert(1.0)], rect, 1),
        DisplayItem::PopOpacity,
    ];
    let p = render(items);
    let (r, g, b) = rgb(&p, 30, 50);
    assert!(r == g && g == b && (120..136).contains(&r), "{r} {g} {b}");
}

#[test]
fn the_rounded_overflow_layer_is_not_a_root() {
    // A rounded overflow clip is a group, but the backdrop of a box inside
    // sees the page behind it.
    use swb_layout::svg::PathSegment::{Close, LineTo, MoveTo};
    let rect = Rect::new(10.0, 10.0, 60.0, 60.0);
    let corner = |x: f32, y: f32| Point::new(x, y);
    let clip = crate::display_list::path_clip_item(
        swb_layout::svg::SvgPath::from_segments(&[
            MoveTo(corner(10.0, 10.0)),
            LineTo(corner(70.0, 10.0)),
            LineTo(corner(70.0, 70.0)),
            LineTo(corner(10.0, 70.0)),
            Close,
        ])
        .expect("a path"),
        swb_style::FillRule::NonZero,
        Matrix::IDENTITY,
        rect,
        false,
    );
    let items = vec![
        left_half(),
        clip,
        backdrop(vec![FilterFunction::Invert(1.0)], rect, 0),
        DisplayItem::PopSvgClip,
    ];
    let p = render(items);
    assert_eq!(rgb(&p, 30, 50), (255, 255, 255));
}

/// Rasterizes `items` on a white 100x100 target with the given budgets of
/// layer pixels and filter work; returns the target and the skipped
/// filters and layers.
fn render_with_filter_budgets(
    items: &[DisplayItem],
    max_layer_pixels: u64,
    max_filter_pixels: u64,
) -> (Pixmap, bool, bool) {
    let mut target = Pixmap::new(100, 100).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    let mut r = rasterizer(&mut target, &mut vectors, max_layer_pixels, MAX_MASK_PIXELS);
    r.budget.max_filter_pixels = max_filter_pixels;
    let mut fonts = FontContext::for_tests();
    for item in items {
        r.item(item, &mut fonts, &NoImages);
    }
    assert_eq!(r.budget.layer_pixels, 0, "all layers are released");
    let (filters, layers) = (r.budget.skipped.filters, r.budget.skipped.layers);
    (target, filters, layers)
}

#[test]
fn a_backdrop_beyond_the_budgets_stays_unfiltered() {
    // 60 x 60 px: a color pass costs 1 unit per pixel, and the layer holds
    // the pixels.
    let rect = Rect::new(10.0, 10.0, 60.0, 60.0);
    let items = vec![
        left_half(),
        backdrop(vec![FilterFunction::Invert(1.0)], rect, 0),
    ];
    let (p, filters, layers) = render_with_filter_budgets(&items, 3600, 3600);
    assert_eq!(rgb(&p, 30, 50), (255, 255, 255));
    assert!(!filters && !layers);
    let (p, filters, _) = render_with_filter_budgets(&items, 3600, 3599);
    assert_eq!(rgb(&p, 30, 50), (0, 0, 0));
    assert!(filters);
    let (p, _, layers) = render_with_filter_budgets(&items, 3599, 3600);
    assert_eq!(rgb(&p, 30, 50), (0, 0, 0));
    assert!(layers);
    // A blur also needs a scratch buffer: twice the layer.
    let blur = vec![
        left_half(),
        backdrop(vec![FilterFunction::Blur(2.0)], rect, 0),
    ];
    let (p, _, layers) = render_with_filter_budgets(&blur, 7199, 1 << 20);
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    assert!(layers);
}

#[test]
fn hostile_boxes_draw_nothing() {
    // Boxes outside the target, empty, or not finite.
    for rect in [
        Rect::new(1000.0, 1000.0, 50.0, 50.0),
        Rect::new(10.0, 10.0, 0.0, 50.0),
        Rect::new(f32::NAN, 0.0, 50.0, 50.0),
        Rect::new(-1e30, -1e30, 2e30, 2e30),
        Rect::new(0.0, 0.0, f32::INFINITY, 50.0),
    ] {
        let items = vec![
            left_half(),
            backdrop(vec![FilterFunction::Blur(1e9)], rect, 3),
        ];
        let _ = render(items);
    }
    // A giant blur of the whole target is limited and finishes.
    let whole = Rect::new(0.0, 0.0, 100.0, 100.0);
    let p = render(vec![
        left_half(),
        backdrop(vec![FilterFunction::Blur(1e9)], whole, 0),
    ]);
    let (r, g, b) = rgb(&p, 50, 50);
    assert!(r == g && g == b, "{r} {g} {b}");
}
