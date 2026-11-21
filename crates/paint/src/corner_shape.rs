//! `corner-shape` (CSS Borders 4 §3,
//! <https://drafts.csswg.org/css-borders-4/#corner-shaping>): the outline of
//! a box whose corners are superellipses. The corner of the radii `rx` and
//! `ry` is the curve `|x|^n + |y|^n = 1` with `n = 2^k` for the parameter
//! `k` of `superellipse(k)` (`round` is 1, `squircle` 2, `bevel` 0), scaled
//! to the radii and placed inside the box. A negative `k` mirrors the
//! curve around the corner point of the box: `scoop` (-1) is concave, and
//! `notch` (negative infinity) cuts a rectangle out of the corner. `square`
//! (positive infinity) is the corner of the box.
//!
//! The curves are polylines of [`STEPS`] segments per corner. The
//! background color, uniform solid borders, the clip of `overflow` and
//! outer shadows use the path; background images, gradients and borders
//! with different sides keep the rounded rectangle (ADR 0045).

use std::sync::Arc;

use swb_layout::svg::{PathSegment, SvgPath};
use swb_layout::{Matrix, Point, Rect, has_rounded_corner};
use swb_style::{ComputedStyle, FillRule, Rgba};

use crate::display_list::{DisplayItem, Radii};

/// The segments of one corner curve.
const STEPS: usize = 24;
/// The largest `|k|` that is computed as a curve; beyond it the corner
/// is a square or a notch.
const MAX_K: f32 = 16.0;

/// The parameters `k` of the four corners (top left, top right, bottom
/// right, bottom left), or `None` if all are `round`.
pub(crate) fn shapes(style: &ComputedStyle) -> Option<[f32; 4]> {
    let k = [
        style.corner_top_left_shape.0,
        style.corner_top_right_shape.0,
        style.corner_bottom_right_shape.0,
        style.corner_bottom_left_shape.0,
    ];
    k.iter().any(|&v| v != 1.0).then_some(k)
}

/// The outline of `rect` with the corner radii `radii` and the shapes `k`.
/// `None` for an empty box.
pub(crate) fn path(rect: Rect, radii: Radii, k: [f32; 4]) -> Option<SvgPath> {
    if !(rect.width > 0.0 && rect.height > 0.0) {
        return None;
    }
    let radii = scaled(rect, radii);
    // (corner x, corner y, direction x, direction y) clockwise from the
    // top left.
    let corners = [
        (rect.x, rect.y, 1.0, 1.0),
        (rect.right(), rect.y, -1.0, 1.0),
        (rect.right(), rect.bottom(), -1.0, -1.0),
        (rect.x, rect.bottom(), 1.0, -1.0),
    ];
    let mut segments = Vec::new();
    for i in 0..4 {
        let (cx, cy, sx, sy) = corners[i];
        let (rx, ry) = radii[i];
        // The start and the end of the curve, clockwise: the top left
        // corner goes from the left edge to the top edge, and so on.
        let (a, b) = if i % 2 == 0 {
            ((cx, cy + ry * sy), (cx + rx * sx, cy))
        } else {
            ((cx + rx * sx, cy), (cx, cy + ry * sy))
        };
        let first = segments.is_empty();
        let p = |(x, y): (f32, f32)| Point::new(x, y);
        segments.push(if first {
            PathSegment::MoveTo(p(a))
        } else {
            PathSegment::LineTo(p(a))
        });
        for point in curve((cx, cy), (sx, sy), (rx, ry), k[i], i % 2 == 1) {
            segments.push(PathSegment::LineTo(p(point)));
        }
        segments.push(PathSegment::LineTo(p(b)));
    }
    segments.push(PathSegment::Close);
    SvgPath::from_segments(&segments)
}

/// The radii scaled down so that they fit the box (CSS Backgrounds 3
/// §4.5).
fn scaled(rect: Rect, radii: Radii) -> Radii {
    let mut f = 1.0_f32;
    let sums = [
        (radii[0].0 + radii[1].0, rect.width),
        (radii[1].1 + radii[2].1, rect.height),
        (radii[2].0 + radii[3].0, rect.width),
        (radii[3].1 + radii[0].1, rect.height),
    ];
    for (sum, len) in sums {
        if sum > len && sum > 0.0 {
            f = f.min(len / sum);
        }
    }
    radii.map(|(h, v)| (h.max(0.0) * f, v.max(0.0) * f))
}

/// The points strictly between the two ends of a corner curve, in
/// clockwise order. `reversed` is true for the corners whose clockwise
/// direction runs from the horizontal edge to the vertical one.
fn curve(
    (cx, cy): (f32, f32),
    (sx, sy): (f32, f32),
    (rx, ry): (f32, f32),
    k: f32,
    reversed: bool,
) -> Vec<(f32, f32)> {
    if rx <= 0.0 || ry <= 0.0 || k.is_nan() {
        return vec![(cx, cy)];
    }
    if k >= MAX_K {
        return vec![(cx, cy)];
    }
    if k <= -MAX_K {
        return vec![(cx + sx * rx, cy + sy * ry)];
    }
    let power = 2.0 / 2.0_f32.powf(k.abs());
    let mut points = Vec::with_capacity(STEPS - 1);
    for step in 1..STEPS {
        let mut t = std::f32::consts::FRAC_PI_2 * step as f32 / STEPS as f32;
        if reversed {
            t = std::f32::consts::FRAC_PI_2 - t;
        }
        let (sin, cos) = t.sin_cos();
        let (u, v) = if k >= 0.0 {
            (rx * (1.0 - cos.powf(power)), ry * (1.0 - sin.powf(power)))
        } else {
            (rx * sin.powf(power), ry * cos.powf(power))
        };
        points.push((cx + sx * u, cy + sy * v));
    }
    points
}

/// A fill of the box outline with the corner shapes `k`, or `None` if the
/// box has no rounded corner or no area.
pub(crate) fn fill(rect: Rect, radii: Radii, k: [f32; 4], color: Rgba) -> Option<DisplayItem> {
    if !has_rounded_corner(&radii) {
        return None;
    }
    Some(DisplayItem::FillPath {
        path: Arc::new(path(rect, radii, k)?),
        transform: Matrix::IDENTITY,
        color,
        rule: FillRule::NonZero,
        anti_alias: true,
    })
}

/// The clip group of the overflow clip of a box with rounded corners and
/// `corner-shape`: the padding box `rect` with the shapes `k`.
pub(crate) fn clip(rect: Rect, radii: Radii, k: [f32; 4]) -> Option<DisplayItem> {
    Some(crate::display_list::path_clip_item(
        path(rect, radii, k)?,
        FillRule::NonZero,
        Matrix::IDENTITY,
        rect,
        false,
    ))
}

/// A border of the same width and color on all sides: the area between
/// the outline of the border box and the one of the padding box.
pub(crate) fn ring(
    rect: Rect,
    radii: Radii,
    k: [f32; 4],
    width: f32,
    color: Rgba,
) -> Option<DisplayItem> {
    let inner = Rect::new(
        rect.x + width,
        rect.y + width,
        rect.width - 2.0 * width,
        rect.height - 2.0 * width,
    );
    let inner_radii = radii.map(|(h, v)| ((h - width).max(0.0), (v - width).max(0.0)));
    let mut segments = path(rect, radii, k)?.segments().to_vec();
    if let Some(hole) = path(inner, inner_radii, k) {
        segments.extend_from_slice(hole.segments());
    }
    Some(DisplayItem::FillPath {
        path: Arc::new(SvgPath::from_segments(&segments)?),
        transform: Matrix::IDENTITY,
        color,
        rule: FillRule::EvenOdd,
        anti_alias: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(0.0, 0.0, 100.0, 60.0);

    fn bounds_of(k: f32) -> Rect {
        path(BOX, [(20.0, 20.0); 4], [k; 4])
            .map(|p| p.bounds())
            .unwrap_or_default()
    }

    #[test]
    fn all_shapes_stay_inside_the_box() {
        for k in [
            -16.0,
            -2.0,
            -1.0,
            0.0,
            1.0,
            2.0,
            3.5,
            16.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            let b = bounds_of(k);
            assert!(b.x >= -0.01 && b.y >= -0.01, "{k}: {b:?}");
            assert!(b.right() <= 100.01 && b.bottom() <= 60.01, "{k}: {b:?}");
        }
    }

    #[test]
    fn a_squircle_is_between_a_circle_and_a_square() {
        // The point of the curve on the diagonal of a corner: the circle
        // is at 1 - 0.7071, the squircle is nearer the corner.
        let on_diagonal = |k: f32| {
            let points = curve((0.0, 0.0), (1.0, 1.0), (20.0, 20.0), k, false);
            let mid = points[STEPS / 2 - 1];
            mid.0
        };
        let (round, squircle, bevel) = (on_diagonal(1.0), on_diagonal(2.0), on_diagonal(0.0));
        assert!((round - 20.0 * (1.0 - std::f32::consts::FRAC_1_SQRT_2)).abs() < 0.1);
        assert!(squircle < round && round < bevel);
        assert!((bevel - 10.0).abs() < 0.1);
    }

    #[test]
    fn extreme_values_make_finite_paths() {
        for k in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0e30, -1.0e30] {
            let radii = [(1.0e9, 1.0e9); 4];
            if let Some(p) = path(BOX, radii, [k; 4]) {
                assert!(p.bounds().width.is_finite());
            }
        }
        assert!(path(Rect::new(0.0, 0.0, 0.0, 10.0), [(1.0, 1.0); 4], [2.0; 4]).is_none());
    }
}
