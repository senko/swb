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
//!
//! A hit test has a work budget ([`HitWork`]): a clip path with many dense
//! shapes, used by many elements, would otherwise take seconds for one
//! point. A test that runs out of work finds no hit.

use std::cell::Cell;

use swb_layout::svg::{ClipPath, ClipRegion, PathSegment, SvgPath};
use swb_layout::{Matrix, Point};
use swb_style::FillRule;

/// The lines that a curve is cut into.
const CURVE_STEPS: u32 = 16;

/// The work of one hit test, in lines of flattened paths (a segment that a
/// quick test rejects counts one): about 1.4 ns each, so about 40 ms. A
/// path of 40,000 curves costs 640,000 per test of its fill; 12,000 of
/// them (40 shapes in each of 300 references) took 20 s before the limit.
const MAX_HIT_WORK: u64 = 30_000_000;

/// The work that is left for one hit test.
#[derive(Debug)]
pub(crate) struct HitWork {
    left: Cell<u64>,
    out: Cell<bool>,
}

impl HitWork {
    /// The budget of one hit test.
    pub(crate) fn new() -> HitWork {
        HitWork {
            left: Cell::new(MAX_HIT_WORK),
            out: Cell::new(false),
        }
    }

    /// Takes `n` from the budget; false (for good) if it does not hold
    /// that much.
    fn spend(&self, n: u64) -> bool {
        let left = self.left.get();
        if self.out.get() || left < n {
            self.out.set(true);
            self.left.set(0);
            return false;
        }
        self.left.set(left - n);
        true
    }

    /// True if a test ran out of work: results after that are not valid.
    fn exhausted(&self) -> bool {
        self.out.get()
    }
}

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
    work: &HitWork,
) -> bool {
    if work.exhausted() {
        return false;
    }
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
    let hit = fill.is_some_and(|rule| inside_fill(path, p, rule, work))
        || stroke_width.is_some_and(|w| near_stroke(path, p, w / 2.0, work));
    hit && !work.exhausted()
}

/// True if `point` (in the clip path's coordinates) is in the visible
/// region of `clip`: inside the fill of one of its shapes and the shape's
/// own clip, and inside the clip path's own `outer` region. The outline is
/// the flattened path, not the anti-aliased coverage that the painting
/// uses.
pub(crate) fn clip_contains(clip: &ClipPath, point: Point, work: &HitWork) -> bool {
    clip.shapes.iter().any(|shape| {
        contains(
            &shape.path,
            &shape.transform,
            Some(shape.rule),
            None,
            point,
            work,
        ) && shape
            .clip
            .as_ref()
            .is_none_or(|region| region_contains(region, point, work))
    }) && clip
        .outer
        .as_ref()
        .is_none_or(|region| region_contains(region, point, work))
        && !work.exhausted()
}

fn region_contains(region: &ClipRegion, point: Point, work: &HitWork) -> bool {
    match region {
        ClipRegion::Rect(rect) => rect.contains(point),
        ClipRegion::Path(clip) => clip_contains(clip, point, work),
    }
}

/// Calls `edge` for each line of the flattened path. `close_all` adds the
/// line that closes every subpath (a fill closes them all; a stroke only
/// the ones with a `Close`). A curve whose control points' y range
/// `skip(min, max)` rejects is not cut into lines (it cannot matter to the
/// test). Stops when the work is used up.
fn edges(
    path: &SvgPath,
    close_all: bool,
    work: &HitWork,
    skip: &impl Fn(f32, f32) -> bool,
    edge: &mut impl FnMut(Point, Point),
) {
    let mut start = Point::default();
    let mut current = Point::default();
    let mut open = false;
    for segment in path.segments() {
        // A segment costs one, a curve that is cut into lines more.
        if !work.spend(1) {
            return;
        }
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
                let ys = [current.y, c.y, p.y];
                if !skip(min_of(&ys), max_of(&ys)) {
                    if !work.spend(u64::from(CURVE_STEPS)) {
                        return;
                    }
                    flatten(current, |t| quad_at(current, c, p, t), edge);
                }
                current = p;
                open = true;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let ys = [current.y, c1.y, c2.y, p.y];
                if !skip(min_of(&ys), max_of(&ys)) {
                    if !work.spend(u64::from(CURVE_STEPS)) {
                        return;
                    }
                    flatten(current, |t| cubic_at(current, c1, c2, p, t), edge);
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

fn min_of(values: &[f32]) -> f32 {
    values.iter().copied().fold(f32::INFINITY, f32::min)
}

fn max_of(values: &[f32]) -> f32 {
    values.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}

/// Calls `edge` for the [`CURVE_STEPS`] lines of the curve `at` from
/// `from`.
fn flatten(from: Point, at: impl Fn(f32) -> Point, edge: &mut impl FnMut(Point, Point)) {
    let mut previous = from;
    for i in 1..=CURVE_STEPS {
        let point = at(i as f32 / CURVE_STEPS as f32);
        edge(previous, point);
        previous = point;
    }
}

fn quad_at(from: Point, c: Point, p: Point, t: f32) -> Point {
    let u = 1.0 - t;
    Point::new(
        u * u * from.x + 2.0 * u * t * c.x + t * t * p.x,
        u * u * from.y + 2.0 * u * t * c.y + t * t * p.y,
    )
}

fn cubic_at(from: Point, c1: Point, c2: Point, p: Point, t: f32) -> Point {
    let u = 1.0 - t;
    Point::new(
        u * u * u * from.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p.x,
        u * u * u * from.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p.y,
    )
}

/// True if `p` is inside the filled path (SVG 2 §13.4.2).
fn inside_fill(path: &SvgPath, p: Point, rule: FillRule, work: &HitWork) -> bool {
    let mut winding = 0_i32;
    let mut crossings = 0_u32;
    // A curve that lies above or below the ray has no crossing.
    let skip = |min: f32, max: f32| min > p.y || max <= p.y;
    edges(path, true, work, &skip, &mut |a, b| {
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
fn near_stroke(path: &SvgPath, p: Point, distance: f32, work: &HitWork) -> bool {
    let limit = distance * distance;
    let mut near = false;
    let skip = |min: f32, max: f32| min - distance > p.y || max + distance < p.y;
    edges(path, false, work, &skip, &mut |a, b| {
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

    /// `super::contains` with a fresh budget.
    fn contains(
        path: &SvgPath,
        transform: &Matrix,
        fill: Option<FillRule>,
        stroke_width: Option<f32>,
        point: Point,
    ) -> bool {
        super::contains(path, transform, fill, stroke_width, point, &HitWork::new())
    }

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

    /// A path of `n` curves that each cross the whole height of the box
    /// (0, 0, 100, 100) and overlap each other.
    fn dense(n: usize) -> SvgPath {
        let p = Point::new;
        let mut segments = vec![PathSegment::MoveTo(p(0.0, 0.0))];
        for i in 0..n {
            let k = (i % 50) as f32;
            segments.push(PathSegment::CubicTo(
                p(k, 0.0),
                p(100.0 - k, 100.0),
                p(((7 * i) % 98) as f32, 50.0),
            ));
        }
        path(&segments)
    }

    #[test]
    fn a_hit_test_has_a_work_budget() {
        let dense = dense(4_000);
        let id = Matrix::IDENTITY;
        // Near many curves.
        let inside = Point::new(50.0, 50.0);
        let hit = |work: &HitWork| super::contains(&dense, &id, None, Some(20.0), inside, work);
        assert!(hit(&HitWork::new()));
        // The same test with too little work finds nothing, and the budget
        // stays used up for the next test.
        let work = HitWork {
            left: Cell::new(1_000),
            out: Cell::new(false),
        };
        assert!(!hit(&work));
        assert!(work.exhausted());
        let fill = Some(FillRule::NonZero);
        let near = Point::new(20.0, 20.0);
        assert!(!super::contains(&square(), &id, fill, None, near, &work));
    }

    #[test]
    fn curves_beyond_the_ray_are_skipped() {
        // The answer is the same with the quick rejection of curves that
        // lie above or below the point.
        let p = Point::new;
        let bump = path(&[
            PathSegment::MoveTo(p(0.0, 100.0)),
            PathSegment::CubicTo(p(0.0, 40.0), p(40.0, 40.0), p(40.0, 100.0)),
            PathSegment::Close,
            PathSegment::MoveTo(p(50.0, 0.0)),
            PathSegment::QuadTo(p(70.0, 20.0), p(90.0, 0.0)),
            PathSegment::Close,
        ]);
        let id = Matrix::IDENTITY;
        let fill = Some(FillRule::NonZero);
        assert!(contains(&bump, &id, fill, None, p(20.0, 90.0)));
        assert!(!contains(&bump, &id, fill, None, p(20.0, 30.0)));
        assert!(contains(&bump, &id, fill, None, p(70.0, 5.0)));
        assert!(contains(&bump, &id, None, Some(2.0), p(70.0, 10.0)));
        assert!(!contains(&bump, &id, None, Some(2.0), p(70.0, 60.0)));
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
