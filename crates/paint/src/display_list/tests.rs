use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::svg::{ClipPath, SvgPath};
use swb_layout::{GroupTransform, Matrix, Point, Rect};
use swb_style::{ComputedStyle, CornerRadius, FillRule, LengthPercentage, Rgba};

use super::*;
use crate::group_bounds::finish_groups;
use crate::hit_path::HitWork;

fn corner(horizontal: LengthPercentage, vertical: LengthPercentage) -> CornerRadius {
    CornerRadius {
        horizontal,
        vertical,
    }
}

#[test]
fn overlapping_radii_are_scaled_down() {
    let mut style = (*ComputedStyle::initial()).clone();
    style.border_top_left_radius = corner(LengthPercentage::Px(80.0), LengthPercentage::Px(40.0));
    style.border_top_right_radius =
        corner(LengthPercentage::Px(40.0), LengthPercentage::Percent(0.2));
    // The top radii need 120 px of the 60 px width: all radii are
    // halved.
    let radii = resolve_radii(&style, Rect::new(0.0, 0.0, 60.0, 100.0));
    assert_eq!(radii, [(40.0, 20.0), (20.0, 10.0), (0.0, 0.0), (0.0, 0.0)]);
    // A box wide enough keeps them.
    let radii = resolve_radii(&style, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert_eq!(radii, [(80.0, 40.0), (40.0, 20.0), (0.0, 0.0), (0.0, 0.0)]);
}

/// A frame item is a hit of its own: the topmost thing at the point
/// unless a later region covers it, a clip hides it, or the element has
/// `pointer-events: none` (ADR 0031 section 8).
#[test]
fn hit_testing_reports_the_content_of_a_frame() {
    let node = |i: usize| NodeId::from_index(i).unwrap();
    let region = |rect, n| DisplayItem::HitRegion { rect, node: n };
    let content = Rect::new(12.0, 12.0, 96.0, 46.0);
    let frame = |interactive| DisplayItem::Frame {
        node: node(1),
        rect: content,
        interactive,
    };
    let border = Rect::new(10.0, 10.0, 100.0, 50.0);
    let work = HitWork::new();
    let at = Point::new(20.0, 30.0);
    let list = DisplayList {
        items: vec![region(border, node(1)), frame(true)],
    };
    assert_eq!(
        list.hit_test_with(at, Point::default(), &work),
        Some(Hit::Frame {
            node: node(1),
            point: at,
            rect: content
        })
    );
    // As a node, the frame is its `iframe` element.
    assert_eq!(list.hit_test(at, Point::default()), Some(node(1)));
    // The border of the `iframe` is the element, not the frame.
    assert_eq!(
        list.hit_test_with(Point::new(11.0, 30.0), Point::default(), &work),
        Some(Hit::Node(node(1)))
    );
    // A region painted later covers the frame.
    let covered = DisplayList {
        items: vec![
            region(border, node(1)),
            frame(true),
            region(Rect::new(0.0, 0.0, 30.0, 40.0), node(2)),
        ],
    };
    assert_eq!(
        covered.hit_test_with(at, Point::default(), &work),
        Some(Hit::Node(node(2)))
    );
    // `pointer-events: none`: the frame is not hit, the region below it
    // is not there either (the element has none).
    let none = DisplayList {
        items: vec![
            region(Rect::new(0.0, 0.0, 200.0, 200.0), node(3)),
            frame(false),
        ],
    };
    assert_eq!(
        none.hit_test_with(at, Point::default(), &work),
        Some(Hit::Node(node(3)))
    );
    // A clip around the item hides the part outside it.
    let clipped = DisplayList {
        items: vec![
            region(border, node(1)),
            DisplayItem::PushClip(Rect::new(10.0, 10.0, 5.0, 50.0)),
            frame(true),
            DisplayItem::PopClip,
        ],
    };
    assert_eq!(
        clipped.hit_test_with(at, Point::default(), &work),
        Some(Hit::Node(node(1)))
    );
    assert!(matches!(
        clipped.hit_test_with(Point::new(13.0, 30.0), Point::default(), &work),
        Some(Hit::Frame { .. })
    ));
}

#[test]
fn hit_testing_reports_a_frame_in_a_transform_group() {
    let node = NodeId::from_index(1).unwrap();
    let content = Rect::new(0.0, 0.0, 100.0, 50.0);
    let mut items = vec![
        DisplayItem::PushTransform {
            transform: GroupTransform::Matrix(Matrix::translate(200.0, 100.0)),
            bounds: Rect::default(),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        },
        DisplayItem::Frame {
            node,
            rect: content,
            interactive: true,
        },
        DisplayItem::PopTransform,
    ];
    finish_groups(&mut items);
    let list = DisplayList { items };
    // The point is in the group's coordinates: the frame's own.
    let hit = list.hit_test_with(Point::new(250.0, 120.0), Point::default(), &HitWork::new());
    assert_eq!(
        hit,
        Some(Hit::Frame {
            node,
            point: Point::new(50.0, 20.0),
            rect: content
        })
    );
    assert_eq!(
        list.hit_test_with(Point::new(10.0, 10.0), Point::default(), &HitWork::new()),
        None
    );
}

#[test]
fn hit_testing_maps_points_into_groups() {
    let node = NodeId::DOCUMENT;
    let region = |rect| DisplayItem::HitRegion { rect, node };
    // The bounds of the groups, as `build_display_list` sets them.
    let finished = |mut items: Vec<DisplayItem>| {
        finish_groups(&mut items);
        DisplayList { items }
    };
    let list = finished(vec![
        DisplayItem::PushTransform {
            transform: GroupTransform::Fixed,
            bounds: Rect::default(),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        },
        region(Rect::new(0.0, 0.0, 10.0, 10.0)),
        DisplayItem::PopTransform,
    ]);
    let scroll = Point::new(0.0, 500.0);
    assert_eq!(list.hit_test(Point::new(5.0, 505.0), scroll), Some(node));
    assert_eq!(list.hit_test(Point::new(5.0, 5.0), scroll), None);
    let flat = finished(vec![
        DisplayItem::PushTransform {
            transform: GroupTransform::Matrix(Matrix::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)),
            bounds: Rect::default(),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        },
        region(Rect::new(-10.0, -10.0, 20.0, 20.0)),
        DisplayItem::PopTransform,
    ]);
    assert_eq!(flat.hit_test(Point::default(), Point::default()), None);
    // A group that the point misses is skipped with its nested groups;
    // the regions after it count.
    let skipped = finished(vec![
        DisplayItem::PushTransform {
            transform: GroupTransform::Matrix(Matrix::translate(100.0, 0.0)),
            bounds: Rect::default(),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        },
        DisplayItem::PushTransform {
            transform: GroupTransform::Matrix(Matrix::translate(0.0, 0.0)),
            bounds: Rect::default(),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        },
        region(Rect::new(0.0, 0.0, 10.0, 10.0)),
        DisplayItem::PopTransform,
        DisplayItem::PopTransform,
        region(Rect::new(0.0, 0.0, 10.0, 10.0)),
    ]);
    assert_eq!(
        skipped.hit_test(Point::new(5.0, 5.0), Point::default()),
        Some(node)
    );
    assert_eq!(
        skipped.hit_test(Point::new(105.0, 5.0), Point::default()),
        Some(node)
    );
    assert_eq!(
        skipped.hit_test(Point::new(55.0, 5.0), Point::default()),
        None
    );
}

/// 300 groups that use one clip path of 40 shapes with 40,000 curves
/// each, and a point inside the bounds that no shape contains: the
/// work budget ends the test (unbounded, it takes 20 s in a release
/// build).
#[test]
fn hit_testing_dense_clip_paths_is_bounded() {
    use swb_layout::svg::{ClipShape, PathSegment};
    let p = Point::new;
    let mut segments = vec![PathSegment::MoveTo(p(0.0, 0.0))];
    for i in 0..40_000 {
        let k = (i % 50) as f32;
        segments.push(PathSegment::CubicTo(
            p(k, 0.0),
            p(100.0 - k, 100.0),
            p(((7 * i) % 98) as f32, 50.0),
        ));
    }
    let path = Arc::new(SvgPath::from_segments(&segments).expect("a path"));
    let shape = || ClipShape {
        path: Arc::clone(&path),
        transform: Matrix::IDENTITY,
        rule: FillRule::EvenOdd,
        clip: None,
    };
    let clip = Arc::new(ClipPath {
        shapes: (0..40).map(|_| shape()).collect(),
        outer: None,
    });
    let mut items = Vec::new();
    for _ in 0..300 {
        items.push(DisplayItem::PushSvgClip {
            clip: Arc::clone(&clip),
            transform: Matrix::IDENTITY,
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            rect_fallback: false,
            backdrop_root: false,
        });
        items.push(DisplayItem::HitRegion {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            node: NodeId::DOCUMENT,
        });
        items.push(DisplayItem::PopSvgClip);
    }
    let list = DisplayList { items };
    let start = std::time::Instant::now();
    // Whether the point is in the clip does not matter: the test ends.
    let _ = list.hit_test(Point::new(99.5, 99.5), Point::default());
    let elapsed = start.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
}

#[test]
fn polyline_bounds_include_the_line_width() {
    let line = DisplayItem::Polyline {
        points: Arc::from([Point::new(10.0, 20.0), Point::new(30.0, 5.0)]),
        width: 1.0,
        color: Rgba::BLACK,
    };
    assert_eq!(line.bounds(), Some(Rect::new(8.0, 3.0, 24.0, 19.0)));
    let empty = DisplayItem::Polyline {
        points: Arc::from([]),
        width: 1.0,
        color: Rgba::BLACK,
    };
    assert_eq!(empty.bounds(), None);
}
