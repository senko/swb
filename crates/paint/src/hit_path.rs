//! Hit testing of SVG shapes: is a point inside the fill or on the stroke
//! of a path (SVG 2 §16.4, `pointer-events`,
//! <https://svgwg.org/svg2-draft/interact.html#PointerEventsProperty>).
//!
//! The test runs in the path's user space, so a transform that scales the
//! axes differently gives an elliptical pen as in painting. Curves are
//! flattened into [`CURVE_STEPS`] lines. A stroke is the set of points
//! within half the stroke width of the path; caps and joins are
//! approximated by round ones (a hit test does not need the pixel exact
//! outline).

use swb_layout::svg::{ClipPath, ClipRegion, PathSegment, SvgPath};
use swb_layout::{Matrix, Point};
use swb_style::FillRule;

/// The lines that a curve is cut into.
const CURVE_STEPS: u32 = 16;

/// True if `point` (in the coordinates after `transform`) is in the fill
/// of `path` (with rule `fill`, if the fill takes part) or on its stroke
/// (of width `stroke_width` in the path's coordinates, if the stroke
/// takes part).
pub(crate) fn contains(
    path: &SvgPath,
    transform: &Matrix,
    fill: Option<FillRule>,
    stroke_width: Option<f32>,
    point: Point,
) -> bool {
    let Some(inverse) = transform.invert() else {
        return false;
    };
    let p = inverse.apply(point);
    if !(p.x.is_finite() && p.y.is_finite()) {
        return false;
    }
    let bounds = path.bounds();
    let half = stroke_width.map_or(0.0, |w| w / 2.0);
    if p.x < bounds.x - half
        || p.x > bounds.right() + half
        || p.y < bounds.y - half
        || p.y > bounds.bottom() + half
    {
        return false;
    }
    if let Some(rule) = fill
        && inside_fill(path, p, rule)
    {
        return true;
    }
    stroke_width.is_some_and(|w| near_stroke(path, p, w / 2.0))
}

/// True if `point` (in the clip path's coordinates) is in the visible
/// region of `clip`: inside the fill of one of its shapes and the shape's
/// own clip, and inside the clip path's own `outer` region. The outline is
/// the flattened path, not the anti-aliased coverage that the painting
/// uses.
pub(crate) fn clip_contains(clip: &ClipPath, point: Point) -> bool {
    clip.shapes.iter().any(|shape| {
        contains(&shape.path, &shape.transform, Some(shape.rule), None, point)
            && shape
                .clip
                .as_ref()
                .is_none_or(|region| region_contains(region, point))
    }) && clip
        .outer
        .as_ref()
        .is_none_or(|region| region_contains(region, point))
}

fn region_contains(region: &ClipRegion, point: Point) -> bool {
    match region {
        ClipRegion::Rect(rect) => rect.contains(point),
        ClipRegion::Path(clip) => clip_contains(clip, point),
    }
}

/// Calls `edge` for each line of the flattened path. `close_all` adds the
/// line that closes every subpath (a fill closes them all; a stroke only
/// the ones with a `Close`).
fn edges(path: &SvgPath, close_all: bool, edge: &mut impl FnMut(Point, Point)) {
    let mut start = Point::default();
    let mut current = Point::default();
    let mut open = false;
    for segment in path.segments() {
        match *segment {
            PathSegment::MoveTo(p) => {
                if close_all && open {
                    edge(current, start);
                }
                start = p;
                current = p;
                open = false;
            }
            PathSegment::LineTo(p) => {
                edge(current, p);
                current = p;
                open = true;
            }
            PathSegment::QuadTo(c, p) => {
                let from = current;
                let mut previous = from;
                for i in 1..=CURVE_STEPS {
                    let t = i as f32 / CURVE_STEPS as f32;
                    let u = 1.0 - t;
                    let at = Point::new(
                        u * u * from.x + 2.0 * u * t * c.x + t * t * p.x,
                        u * u * from.y + 2.0 * u * t * c.y + t * t * p.y,
                    );
                    edge(previous, at);
                    previous = at;
                }
                current = p;
                open = true;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let from = current;
                let mut previous = from;
                for i in 1..=CURVE_STEPS {
                    let t = i as f32 / CURVE_STEPS as f32;
                    let u = 1.0 - t;
                    let at = Point::new(
                        u * u * u * from.x
                            + 3.0 * u * u * t * c1.x
                            + 3.0 * u * t * t * c2.x
                            + t * t * t * p.x,
                        u * u * u * from.y
                            + 3.0 * u * u * t * c1.y
                            + 3.0 * u * t * t * c2.y
                            + t * t * t * p.y,
                    );
                    edge(previous, at);
                    previous = at;
                }
                current = p;
                open = true;
            }
            PathSegment::Close => {
                edge(current, start);
                current = start;
                open = false;
            }
        }
    }
    if close_all && open {
        edge(current, start);
    }
}

/// True if `p` is inside the filled path (SVG 2 §13.4.2).
fn inside_fill(path: &SvgPath, p: Point, rule: FillRule) -> bool {
    let mut winding = 0_i32;
    let mut crossings = 0_u32;
    edges(path, true, &mut |a, b| {
        // A crossing of the ray from `p` to the right with the edge.
        if (a.y <= p.y) != (b.y <= p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if x > p.x {
                crossings += 1;
                winding += if b.y > a.y { 1 } else { -1 };
            }
        }
    });
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => crossings % 2 == 1,
    }
}

/// True if `p` is within `distance` of a line of the path.
fn near_stroke(path: &SvgPath, p: Point, distance: f32) -> bool {
    let limit = distance * distance;
    let mut near = false;
    edges(path, false, &mut |a, b| {
        if near {
            return;
        }
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let length2 = dx * dx + dy * dy;
        let t = if length2 > 0.0 {
            (((p.x - a.x) * dx + (p.y - a.y) * dy) / length2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cy) = (a.x + t * dx - p.x, a.y + t * dy - p.y);
        near = cx * cx + cy * cy <= limit;
    });
    near
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(segments: &[PathSegment]) -> SvgPath {
        SvgPath::from_segments(segments).expect("a path")
    }

    fn square() -> SvgPath {
        let p = Point::new;
        path(&[
            PathSegment::MoveTo(p(10.0, 10.0)),
            PathSegment::LineTo(p(30.0, 10.0)),
            PathSegment::LineTo(p(30.0, 30.0)),
            PathSegment::LineTo(p(10.0, 30.0)),
            PathSegment::Close,
        ])
    }

    #[test]
    fn fill_is_inside_the_outline() {
        let id = Matrix::IDENTITY;
        let hit = |p: Point| contains(&square(), &id, Some(FillRule::NonZero), None, p);
        assert!(hit(Point::new(20.0, 20.0)));
        assert!(!hit(Point::new(5.0, 20.0)));
        assert!(!hit(Point::new(20.0, 35.0)));
        // Without the fill, the inside is not hit.
        assert!(!contains(
            &square(),
            &id,
            None,
            None,
            Point::new(20.0, 20.0)
        ));
    }

    #[test]
    fn the_stroke_is_near_the_lines() {
        let id = Matrix::IDENTITY;
        let hit = |p: Point| contains(&square(), &id, None, Some(4.0), p);
        assert!(hit(Point::new(11.0, 20.0)));
        assert!(hit(Point::new(8.5, 20.0)));
        assert!(!hit(Point::new(20.0, 20.0)));
        assert!(!hit(Point::new(7.0, 20.0)));
    }

    #[test]
    fn a_transform_moves_and_scales_the_shape() {
        let m = Matrix::new(2.0, 0.0, 0.0, 2.0, 100.0, 0.0);
        let fill = Some(FillRule::NonZero);
        assert!(contains(&square(), &m, fill, None, Point::new(140.0, 40.0)));
        assert!(!contains(&square(), &m, fill, None, Point::new(20.0, 20.0)));
        // A matrix that cannot be inverted hits nothing.
        let flat = Matrix::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert!(!contains(
            &square(),
            &flat,
            fill,
            None,
            Point::new(0.0, 0.0)
        ));
    }

    #[test]
    fn even_odd_leaves_holes() {
        let p = Point::new;
        let ring = path(&[
            PathSegment::MoveTo(p(0.0, 0.0)),
            PathSegment::LineTo(p(40.0, 0.0)),
            PathSegment::LineTo(p(40.0, 40.0)),
            PathSegment::LineTo(p(0.0, 40.0)),
            PathSegment::Close,
            PathSegment::MoveTo(p(10.0, 10.0)),
            PathSegment::LineTo(p(30.0, 10.0)),
            PathSegment::LineTo(p(30.0, 30.0)),
            PathSegment::LineTo(p(10.0, 30.0)),
            PathSegment::Close,
        ]);
        let id = Matrix::IDENTITY;
        let center = Point::new(20.0, 20.0);
        assert!(contains(&ring, &id, Some(FillRule::NonZero), None, center));
        assert!(!contains(&ring, &id, Some(FillRule::EvenOdd), None, center));
        assert!(contains(
            &ring,
            &id,
            Some(FillRule::EvenOdd),
            None,
            Point::new(5.0, 20.0)
        ));
    }

    #[test]
    fn curves_are_flattened() {
        let p = Point::new;
        let arch = path(&[
            PathSegment::MoveTo(p(0.0, 20.0)),
            PathSegment::CubicTo(p(0.0, -10.0), p(40.0, -10.0), p(40.0, 20.0)),
            PathSegment::Close,
        ]);
        let id = Matrix::IDENTITY;
        let fill = Some(FillRule::NonZero);
        assert!(contains(&arch, &id, fill, None, p(20.0, 10.0)));
        assert!(!contains(&arch, &id, fill, None, p(2.0, 2.0)));
    }
}
