//! The geometry of the basic shapes of `clip-path` (CSS Shapes 1 §3,
//! <https://drafts.csswg.org/css-shapes-1/#basic-shape-functions>): a
//! computed [`BasicShape`] placed in a reference box. Boxes
//! (`swb-paint`) and SVG elements (`svg/clip.rs`) share it.
//!
//! A percentage of a length refers to the reference box; the radius of a
//! `circle()` as a percentage refers to `sqrt((width² + height²) / 2)`.
//! `path()` is filled with the even-odd rule whatever its fill rule says,
//! and its coordinates start at the top left of the reference box
//! (measured in Chromium 148).

use swb_style::{BasicShape, FillRule, Inset, LengthPercentage, ShapePosition, ShapeRadius};

use crate::geom::{Matrix, Point, Rect};
use crate::svg::{PathSegment, SvgPath, ellipse_segments, path_from_data, rounded_rect_segments};

/// Corner radii (horizontal, vertical) in px: top-left, top-right,
/// bottom-right, bottom-left.
pub type CornerRadii = [(f32, f32); 4];

/// A shape placed in a reference box.
#[derive(Debug)]
pub enum ShapeGeometry {
    /// Nothing is visible.
    Empty,
    /// An axis-aligned rectangle.
    Rect(Rect),
    /// A path, in the coordinates that `transform` maps to the box's
    /// (the translation to the top left of the reference box for
    /// `path()`), with its extent `bounds` in the box's coordinates.
    Path {
        /// The path.
        path: SvgPath,
        /// The fill rule.
        rule: FillRule,
        /// From the path's coordinates to the box's.
        transform: Matrix,
        /// The extent of the path in the box's coordinates.
        bounds: Rect,
    },
}

impl ShapeGeometry {
    /// A path in the box's coordinates, or `Empty` if it has nothing to
    /// draw.
    fn path(segments: &[PathSegment], rule: FillRule) -> ShapeGeometry {
        match SvgPath::from_segments(segments) {
            Some(path) => {
                let bounds = path.bounds();
                ShapeGeometry::Path {
                    path,
                    rule,
                    transform: Matrix::IDENTITY,
                    bounds,
                }
            }
            None => ShapeGeometry::Empty,
        }
    }
}

/// A geometry box alone: its rectangle with its corner radii (scaled
/// down to fit).
pub fn box_geometry(reference: Rect, radii: Option<&CornerRadii>) -> ShapeGeometry {
    rounded_rect(reference, radii)
}

/// `shape` placed in the reference box `reference`.
pub fn shape_geometry(shape: &BasicShape, reference: Rect) -> ShapeGeometry {
    let b = reference;
    if !(b.width.is_finite() && b.height.is_finite() && b.x.is_finite() && b.y.is_finite()) {
        return ShapeGeometry::Empty;
    }
    match shape {
        BasicShape::Inset(i) => inset(i, b),
        BasicShape::Circle { radius, position } => {
            let (cx, cy) = center(position.as_ref(), b);
            let reach = b.width.hypot(b.height) / std::f32::consts::SQRT_2;
            let sides = [
                (cx - b.x).abs(),
                (cx - b.right()).abs(),
                (cy - b.y).abs(),
                (cy - b.bottom()).abs(),
            ];
            let r = match radius {
                ShapeRadius::ClosestSide => sides.iter().copied().fold(f32::INFINITY, f32::min),
                ShapeRadius::FarthestSide => sides.iter().copied().fold(0.0, f32::max),
                ShapeRadius::Length(l) => l.resolve(reach).max(0.0),
            };
            ellipse(cx, cy, r, r)
        }
        BasicShape::Ellipse { rx, ry, position } => {
            let (cx, cy) = center(position.as_ref(), b);
            let rx = radius_of(rx, cx, b.x, b.right(), b.width);
            let ry = radius_of(ry, cy, b.y, b.bottom(), b.height);
            ellipse(cx, cy, rx, ry)
        }
        BasicShape::Polygon { rule, points } => {
            let mut segments = Vec::with_capacity(points.len() + 1);
            for (i, (x, y)) in points.iter().enumerate() {
                let p = Point::new(b.x + x.resolve(b.width), b.y + y.resolve(b.height));
                segments.push(if i == 0 {
                    PathSegment::MoveTo(p)
                } else {
                    PathSegment::LineTo(p)
                });
            }
            segments.push(PathSegment::Close);
            ShapeGeometry::path(&segments, *rule)
        }
        BasicShape::Path { data, .. } => match path_from_data(data) {
            Some(path) => ShapeGeometry::Path {
                bounds: path.bounds().translate(Point::new(b.x, b.y)),
                path,
                rule: FillRule::EvenOdd,
                transform: Matrix::translate(b.x, b.y),
            },
            None => ShapeGeometry::Empty,
        },
    }
}

/// A rectangle with corner radii (scaled down to fit).
fn rounded_rect(rect: Rect, radii: Option<&CornerRadii>) -> ShapeGeometry {
    if !(rect.width > 0.0 && rect.height > 0.0) {
        return ShapeGeometry::Empty;
    }
    let Some(radii) = radii.filter(|r| r.iter().any(|&(h, v)| h > 0.0 && v > 0.0)) else {
        return ShapeGeometry::Rect(rect);
    };
    match rounded_path(rect, radii) {
        Some(path) => ShapeGeometry::Path {
            path,
            rule: FillRule::NonZero,
            transform: Matrix::IDENTITY,
            bounds: rect,
        },
        None => ShapeGeometry::Empty,
    }
}

fn inset(i: &Inset, b: Rect) -> ShapeGeometry {
    let left = i.left.resolve(b.width);
    let top = i.top.resolve(b.height);
    let right = i.right.resolve(b.width);
    let bottom = i.bottom.resolve(b.height);
    let rect = Rect::new(
        b.x + left,
        b.y + top,
        b.width - left - right,
        b.height - top - bottom,
    );
    let radii = i.radii.as_ref().map(|radii| {
        radii.each_ref().map(|c| {
            (
                c.horizontal.resolve(rect.width).max(0.0),
                c.vertical.resolve(rect.height).max(0.0),
            )
        })
    });
    rounded_rect(rect, radii.as_ref())
}

/// The center of a `circle()` or `ellipse()`: the middle of the box by
/// default.
fn center(position: Option<&ShapePosition>, b: Rect) -> (f32, f32) {
    let Some(p) = position else {
        return (b.x + b.width / 2.0, b.y + b.height / 2.0);
    };
    let axis = |offset: &LengthPercentage, from_end: bool, start: f32, size: f32| {
        let d = offset.resolve(size);
        if from_end {
            start + size - d
        } else {
            start + d
        }
    };
    (
        axis(&p.x.offset, p.x.from_end, b.x, b.width),
        axis(&p.y.offset, p.y.from_end, b.y, b.height),
    )
}

/// The radius of one axis of an ellipse: `closest-side` and
/// `farthest-side` are the distance from the center `c` to the nearer or
/// the farther of the two sides `lo` and `hi` on the axis; a percentage
/// refers to `basis`.
fn radius_of(radius: &ShapeRadius, c: f32, lo: f32, hi: f32, basis: f32) -> f32 {
    let (a, b) = ((c - lo).abs(), (c - hi).abs());
    match radius {
        ShapeRadius::ClosestSide => a.min(b),
        ShapeRadius::FarthestSide => a.max(b),
        ShapeRadius::Length(l) => l.resolve(basis).max(0.0),
    }
}

/// The path of an ellipse: four cubic quarters from the right end.
fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> ShapeGeometry {
    if !(rx > 0.0 && ry > 0.0 && rx.is_finite() && ry.is_finite()) {
        return ShapeGeometry::Empty;
    }
    ShapeGeometry::path(&ellipse_segments(cx, cy, rx, ry), FillRule::NonZero)
}

/// The path of the rounded rectangle, or `None` for an empty rectangle
/// or one that is not finite. The radii are scaled down to fit (CSS
/// Backgrounds 3 §4.5).
pub fn rounded_path(rect: Rect, radii: &CornerRadii) -> Option<SvgPath> {
    if !(rect.width > 0.0 && rect.height > 0.0) {
        return None;
    }
    let mut radii = *radii;
    let f = [
        (radii[0].0 + radii[1].0, rect.width),
        (radii[1].1 + radii[2].1, rect.height),
        (radii[2].0 + radii[3].0, rect.width),
        (radii[3].1 + radii[0].1, rect.height),
    ]
    .iter()
    .filter(|(sum, _)| *sum > 0.0)
    .map(|(sum, len)| len / sum)
    .fold(1.0_f32, f32::min);
    for (h, v) in &mut radii {
        *h *= f;
        *v *= f;
    }
    SvgPath::from_segments(&rounded_rect_segments(rect, radii))
}
