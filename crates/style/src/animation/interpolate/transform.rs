//! The interpolation of transform lists (CSS Transforms 1 §10,
//! <https://www.w3.org/TR/css-transforms-1/#interpolation-of-transforms>;
//! CSS Transforms 2 §13 and §14,
//! <https://drafts.csswg.org/css-transforms-2/#matrix-interpolation>), as
//! ADR 0044 section 4 measured it in Chromium 148:
//!
//! - `none` is a list of identity functions of the other list.
//! - Functions that match pair by pair (swb computes `translateX()` and
//!   `translate()` to one primitive) interpolate per function; the shorter
//!   list is padded with identity functions. Angles interpolate as
//!   numbers (`rotate(0)` to `rotate(720deg)` at 0.25 is 180°).
//! - From the first pair that does not match, the rest of both lists
//!   interpolate as matrices: decomposed in 2D (translation, rotation,
//!   skew, scale), interpolated per part, recomposed. The decomposition is
//!   the 3D one of Transforms 2 reduced to 2D, with the rule of Transforms
//!   1 for a negative determinant: negate the x scale (with its row and the
//!   skew) when m11 < m22, else the y scale. The angles go the short way
//!   when they differ by more than 180°.
//!
//! A matrix pair with a percentage in a `translate()` needs the reference
//! box, which is known only at layout: the computed value keeps both rests
//! and the progress ([`TransformFunction::Interpolated`]), and layout and
//! `getComputedStyle` make the matrix with the box
//! ([`InterpolatedTransform::matrix`]). Such a value can be the start of a
//! later interpolation (a transition from an animated value), so it nests;
//! a value with more than [`MAX_NODES`] interpolated values in it would cost
//! too much per frame, and such a pair is discrete instead.
//!
//! The model has no 3D transforms, so the decomposition is 2D only.

use std::sync::Arc;

use super::{lerp, lp, to_f32};
use crate::values::{InterpolatedTransform, LengthPercentage, TransformFunction};

/// The most interpolated values that one [`InterpolatedTransform`] may hold,
/// itself included. Making its matrix resolves each of them once, so a
/// page that chains animations cannot make one frame do more than this many
/// matrix interpolations per element. A pair that would exceed it is
/// discrete (it flips at progress 0.5).
pub const MAX_NODES: u32 = 32;

/// A 2D matrix `[a, b, c, d, e, f]` (`matrix(a, b, c, d, e, f)`: x' = a·x +
/// c·y + e, y' = b·x + d·y + f).
pub type Matrix2 = [f64; 6];

/// The identity matrix.
const IDENTITY: Matrix2 = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// The parts of a decomposed 2D matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decomposed {
    /// The translation.
    pub translate: [f64; 2],
    /// The rotation in degrees.
    pub angle: f64,
    /// The skew factor (the tangent of the x skew angle).
    pub skew: f64,
    /// The scale factors.
    pub scale: [f64; 2],
}

/// The product `m × n` (`n` applied first).
fn multiply(m: &Matrix2, n: &Matrix2) -> Matrix2 {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

/// Decomposes a 2D matrix.
pub fn decompose(m: &Matrix2) -> Decomposed {
    let [a, b, c, d, e, f] = *m;
    let mut sx = a.hypot(b);
    let (mut n0x, mut n0y) = if sx == 0.0 {
        (1.0, 0.0)
    } else {
        (a / sx, b / sx)
    };
    let mut skew = n0x * c + n0y * d;
    let (r1x, r1y) = (c - n0x * skew, d - n0y * skew);
    let mut sy = r1x.hypot(r1y);
    if a * d - b * c < 0.0 {
        if a < d {
            sx = -sx;
            n0x = -n0x;
            n0y = -n0y;
            skew = -skew;
        } else {
            sy = -sy;
        }
    }
    let skew = if sy == 0.0 { 0.0 } else { skew / sy };
    Decomposed {
        translate: [e, f],
        angle: n0y.atan2(n0x).to_degrees(),
        skew,
        scale: [sx, sy],
    }
}

/// Recomposes a decomposed matrix: translate, rotate, skew, scale.
pub fn recompose(p: &Decomposed) -> Matrix2 {
    let (sin, cos) = p.angle.to_radians().sin_cos();
    let [sx, sy] = p.scale;
    let k = p.skew;
    [
        cos * sx,
        sin * sx,
        (cos * k - sin) * sy,
        (sin * k + cos) * sy,
        p.translate[0],
        p.translate[1],
    ]
}

/// The matrix between `from` and `to` at `progress`.
pub fn interpolate_matrices(from: &Matrix2, to: &Matrix2, progress: f64) -> Matrix2 {
    let a = decompose(from);
    let b = decompose(to);
    let (mut angle_a, mut angle_b) = (a.angle, b.angle);
    if (angle_a - angle_b).abs() > 180.0 {
        if angle_a > angle_b {
            angle_a -= 360.0;
        } else {
            angle_b -= 360.0;
        }
    }
    // Each part is a finite `f32` (a huge progress saturates), so the
    // matrix has no NaN.
    let mix = |x: f64, y: f64| f64::from(to_f32(x + (y - x) * progress));
    recompose(&Decomposed {
        translate: [
            mix(a.translate[0], b.translate[0]),
            mix(a.translate[1], b.translate[1]),
        ],
        angle: mix(angle_a, angle_b),
        skew: mix(a.skew, b.skew),
        scale: [mix(a.scale[0], b.scale[0]), mix(a.scale[1], b.scale[1])],
    })
}

/// The matrix of a function in the reference box `(width, height)`;
/// without a box, `None` for a function with a percentage.
fn function_matrix(f: &TransformFunction, reference: Option<(f32, f32)>) -> Option<Matrix2> {
    Some(match f {
        TransformFunction::Matrix(m) => m.map(f64::from),
        TransformFunction::Translate(x, y) => {
            let resolve = |v: &LengthPercentage, basis: Option<f32>| match v {
                LengthPercentage::Px(px) => Some(f64::from(*px)),
                _ => basis.map(|b| f64::from(v.resolve(b))),
            };
            [
                1.0,
                0.0,
                0.0,
                1.0,
                resolve(x, reference.map(|r| r.0))?,
                resolve(y, reference.map(|r| r.1))?,
            ]
        }
        TransformFunction::Interpolated(i) => {
            let (width, height) = reference?;
            i.matrix(width, height)
        }
        TransformFunction::Scale(x, y) => [f64::from(*x), 0.0, 0.0, f64::from(*y), 0.0, 0.0],
        TransformFunction::Rotate(deg) => {
            let (sin, cos) = f64::from(*deg).to_radians().sin_cos();
            [cos, sin, -sin, cos, 0.0, 0.0]
        }
        TransformFunction::Skew(x, y) => [
            1.0,
            f64::from(*y).to_radians().tan(),
            f64::from(*x).to_radians().tan(),
            1.0,
            0.0,
            0.0,
        ],
    })
}

/// The matrix of a list of functions (without a box, `None` with a
/// percentage).
pub(super) fn list_matrix(
    list: &[TransformFunction],
    reference: Option<(f32, f32)>,
) -> Option<Matrix2> {
    list.iter().try_fold(IDENTITY, |m, f| {
        Some(multiply(&m, &function_matrix(f, reference)?))
    })
}

impl InterpolatedTransform {
    /// The matrix `[a, b, c, d, e, f]` in a reference box of `width` ×
    /// `height` px: both lists as matrices, interpolated as decomposed 2D
    /// matrices.
    pub fn matrix(&self, width: f32, height: f32) -> Matrix2 {
        let reference = Some((width, height));
        let from = list_matrix(&self.from, reference).unwrap_or(IDENTITY);
        let to = list_matrix(&self.to, reference).unwrap_or(IDENTITY);
        interpolate_matrices(&from, &to, self.progress).map(|v| f64::from(to_f32(v)))
    }
}

/// The number of interpolated values in `list`.
fn nodes(list: &[TransformFunction]) -> u32 {
    list.iter()
        .map(|f| match f {
            TransformFunction::Interpolated(i) => i.nodes,
            _ => 0,
        })
        .fold(0, u32::saturating_add)
}

/// The identity function of the kind of `f`.
pub(super) fn identity_of(f: &TransformFunction) -> TransformFunction {
    match f {
        TransformFunction::Matrix(_) | TransformFunction::Interpolated(_) => {
            TransformFunction::Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
        }
        TransformFunction::Translate(..) => {
            TransformFunction::Translate(LengthPercentage::ZERO, LengthPercentage::ZERO)
        }
        TransformFunction::Scale(..) => TransformFunction::Scale(1.0, 1.0),
        TransformFunction::Rotate(_) => TransformFunction::Rotate(0.0),
        TransformFunction::Skew(..) => TransformFunction::Skew(0.0, 0.0),
    }
}

/// Two functions of the same primitive at `p`; `None` if they differ (or
/// for two matrices, which interpolate as matrices).
pub(super) fn function_pair(
    a: &TransformFunction,
    b: &TransformFunction,
    p: f64,
) -> Option<TransformFunction> {
    use TransformFunction as T;
    Some(match (a, b) {
        (T::Translate(ax, ay), T::Translate(bx, by)) => {
            T::Translate(lp(ax, bx, p, false), lp(ay, by, p, false))
        }
        (T::Scale(ax, ay), T::Scale(bx, by)) => T::Scale(lerp(*ax, *bx, p), lerp(*ay, *by, p)),
        (T::Rotate(x), T::Rotate(y)) => T::Rotate(lerp(*x, *y, p)),
        (T::Skew(ax, ay), T::Skew(bx, by)) => T::Skew(lerp(*ax, *bx, p), lerp(*ay, *by, p)),
        _ => return None,
    })
}

/// Two computed `transform` lists at `p`; `None` if the pair is discrete
/// (a matrix pair with percentages whose value would hold more than
/// [`MAX_NODES`] interpolated values).
pub(super) fn interpolate_lists(
    a: &Arc<[TransformFunction]>,
    b: &Arc<[TransformFunction]>,
    p: f64,
) -> Option<Arc<[TransformFunction]>> {
    let len = a.len().max(b.len());
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let (fa, fb) = match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => (x.clone(), y.clone()),
            (Some(x), None) => (x.clone(), identity_of(x)),
            (None, Some(y)) => (identity_of(y), y.clone()),
            (None, None) => break,
        };
        if let Some(f) = function_pair(&fa, &fb, p) {
            out.push(f);
            continue;
        }
        // The rest as matrices; a missing side is the identity.
        let rest_a = a.get(i..).unwrap_or(&[]);
        let rest_b = b.get(i..).unwrap_or(&[]);
        out.push(rest_pair(rest_a, rest_b, p)?);
        break;
    }
    Some(Arc::from(out))
}

/// The rests of two lists from their first pair that does not match: one
/// matrix, or the interpolated value when a rest has a percentage.
fn rest_pair(
    a: &[TransformFunction],
    b: &[TransformFunction],
    p: f64,
) -> Option<TransformFunction> {
    if let (Some(ma), Some(mb)) = (list_matrix(a, None), list_matrix(b, None)) {
        let m = interpolate_matrices(&ma, &mb, p);
        return Some(TransformFunction::Matrix(m.map(to_f32)));
    }
    let count = nodes(a).saturating_add(nodes(b)).saturating_add(1);
    if count > MAX_NODES {
        return None;
    }
    Some(TransformFunction::Interpolated(Arc::new(
        InterpolatedTransform {
            from: Arc::from(a),
            to: Arc::from(b),
            progress: p,
            nodes: count,
        },
    )))
}

#[cfg(test)]
// The numbers are Chromium's values as it prints them (0.707107 is one).
#[allow(clippy::unreadable_literal, clippy::approx_constant)]
mod tests {
    use super::*;

    fn list(css: &str) -> Arc<[TransformFunction]> {
        super::super::tests::style(&format!("transform: {css}")).transform
    }

    fn matrix_of(l: &[TransformFunction]) -> Matrix2 {
        let resolved: Vec<TransformFunction> = l
            .iter()
            .map(|f| match f {
                TransformFunction::Translate(x, y) => TransformFunction::Translate(
                    LengthPercentage::Px(x.resolve(200.0)),
                    LengthPercentage::Px(y.resolve(4.0)),
                ),
                other => other.clone(),
            })
            .collect();
        list_matrix(&resolved, None).expect("no percentage")
    }

    fn close(got: &Matrix2, want: &Matrix2) -> bool {
        got.iter()
            .zip(want)
            .all(|(a, b)| (a - b).abs() <= 1e-4 || (a - b).abs() <= 1e-5 * b.abs())
    }

    /// The rows of `interp-transforms` and `interp-transform-decomposition`
    /// (Chromium's `getComputedStyle` matrices; a 200 x 4 px box).
    #[test]
    #[allow(clippy::too_many_lines)] // One row per case.
    fn chromium_matrices() {
        type Row = (&'static str, &'static str, [Matrix2; 2]);
        let rows: [Row; 17] = [
            (
                "translateX(10px) rotate(0deg)",
                "translateX(100px) rotate(90deg)",
                [
                    [0.92388, 0.382683, -0.382683, 0.92388, 32.5, 0.0],
                    [0.707107, 0.707107, -0.707107, 0.707107, 55.0, 0.0],
                ],
            ),
            (
                "rotate(45deg)",
                "scale(2)",
                [
                    [1.03934, 0.694463, -0.694463, 1.03934, 0.0, 0.0],
                    [1.38582, 0.574025, -0.574025, 1.38582, 0.0, 0.0],
                ],
            ),
            (
                "none",
                "rotate(360deg)",
                [
                    [0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
                    [-1.0, 0.0, 0.0, -1.0, 0.0, 0.0],
                ],
            ),
            (
                "rotate(0deg)",
                "rotate(720deg)",
                [
                    [-1.0, 0.0, 0.0, -1.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                ],
            ),
            (
                "translateX(10px)",
                "translateX(20px) rotate(90deg)",
                [
                    [0.92388, 0.382683, -0.382683, 0.92388, 12.5, 0.0],
                    [0.707107, 0.707107, -0.707107, 0.707107, 15.0, 0.0],
                ],
            ),
            (
                "translate(-50%)",
                "translate(0)",
                [
                    [1.0, 0.0, 0.0, 1.0, -75.0, 0.0],
                    [1.0, 0.0, 0.0, 1.0, -50.0, 0.0],
                ],
            ),
            (
                "scale(1)",
                "scale(-1, 1)",
                [
                    [0.5, 0.0, 0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                ],
            ),
            (
                "skewX(0deg)",
                "skewX(60deg)",
                [
                    [1.0, 0.0, 0.267949, 1.0, 0.0, 0.0],
                    [1.0, 0.0, 0.57735, 1.0, 0.0, 0.0],
                ],
            ),
            (
                "rotate(0deg)",
                "matrix(-1, 0, 0, -1, 0, 0)",
                [
                    [0.707107, 0.707107, -0.707107, 0.707107, 0.0, 0.0],
                    [2.22045e-16, 1.0, -1.0, 2.22045e-16, 0.0, 0.0],
                ],
            ),
            (
                "translateY(20px) scale(.5)",
                "none",
                [
                    [0.625, 0.0, 0.0, 0.625, 0.0, 15.0],
                    [0.75, 0.0, 0.0, 0.75, 0.0, 10.0],
                ],
            ),
            (
                "rotate(0deg)",
                "matrix(-1, 0, 0, 1, 0, 0)",
                [
                    [0.5, 0.0, 0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                ],
            ),
            (
                "rotate(0deg)",
                "matrix(1, 0, 1, 1, 0, 0)",
                [
                    [1.0, 0.0, 0.25, 1.0, 0.0, 0.0],
                    [1.0, 0.0, 0.5, 1.0, 0.0, 0.0],
                ],
            ),
            (
                "rotate(30deg)",
                "matrix(0, 1, 1, 0, 0, 0)",
                [
                    [0.707107, 0.707107, -0.353553, 0.353553, 0.0, 0.0],
                    [0.5, 0.866025, 0.0, 0.0, 0.0, 0.0],
                ],
            ),
            (
                "scale(1)",
                "matrix(0, 1, -1, 0, 0, 0)",
                [
                    [0.92388, 0.382683, -0.382683, 0.92388, 0.0, 0.0],
                    [0.707107, 0.707107, -0.707107, 0.707107, 0.0, 0.0],
                ],
            ),
            (
                "matrix(1, 0, 0, 1, 0, 0)",
                "matrix(-1, 0, 0, -1, 0, 0)",
                [
                    [0.707107, 0.707107, -0.707107, 0.707107, 0.0, 0.0],
                    [2.22045e-16, 1.0, -1.0, 2.22045e-16, 0.0, 0.0],
                ],
            ),
            (
                "translate(10px, 20px) scale(2) skewX(30deg)",
                "rotate(60deg) translate(5px)",
                [
                    [1.69037, 0.452933, 0.279018, 1.8865, 8.125, 16.0825],
                    [1.29904, 0.75, -0.375, 1.51554, 6.25, 12.1651],
                ],
            ),
            (
                "rotate(170deg)",
                "matrix(-1, 0, 0, 1, 0, 0)",
                [
                    [-0.304381, 0.396677, -0.793353, -0.608761, 0.0, 0.0],
                    [-4.83812e-18, -5.52999e-17, -0.996195, 0.0871557, 0.0, 0.0],
                ],
            ),
        ];
        for (a, b, wants) in rows {
            for (p, want) in [0.25, 0.5].iter().zip(wants) {
                let got = interpolate_lists(&list(a), &list(b), *p).expect("interpolable");
                let m = matrix_of(&got);
                assert!(close(&m, &want), "{a} to {b} at {p}: {m:?} != {want:?}");
            }
        }
    }

    /// A matrix pair with a percentage keeps both rests; the matrix in a
    /// 200 × 4 px box is that of the lists with the percentage resolved
    /// (`matrix-percent` of `tools/probes/css-animations-3a.json`).
    #[test]
    fn a_matrix_pair_with_percentages() {
        type Row = (&'static str, &'static str, [Matrix2; 2]);
        let rows: [Row; 4] = [
            (
                "translate(-50%)",
                "rotate(45deg)",
                [
                    [0.980785, 0.19509, -0.19509, 0.980785, -75.0, 0.0],
                    [0.92388, 0.382683, -0.382683, 0.92388, -50.0, 0.0],
                ],
            ),
            (
                "translateX(10px) translate(50%, 50%) scale(2)",
                "translateX(30px) rotate(90deg)",
                [
                    [1.61679, 0.669696, -0.669696, 1.61679, 90.0, 1.5],
                    [1.06066, 1.06066, -1.06066, 1.06066, 70.0, 1.0],
                ],
            ),
            (
                "rotate(0deg)",
                "translate(10%, 50%) rotate(180deg)",
                [
                    [0.707107, 0.707107, -0.707107, 0.707107, 5.0, 0.5],
                    [2.22045e-16, 1.0, -1.0, 2.22045e-16, 10.0, 1.0],
                ],
            ),
            (
                "translate(-50%, -50%) rotate(10deg)",
                "scale(1.5) translate(25%)",
                [
                    [1.11538, 0.146842, -0.146842, 1.11538, -56.25, -1.5],
                    [1.24524, 0.108945, -0.108945, 1.24524, -12.5, -1.0],
                ],
            ),
        ];

        for (a, b, wants) in rows {
            for (p, want) in [0.25, 0.5].iter().zip(wants) {
                let got = interpolate_lists(&list(a), &list(b), *p).expect("interpolable");
                let m = matrix_in_box(&got);
                assert!(close(&m, &want), "{a} to {b} at {p}: {m:?} != {want:?}");
            }
        }
    }

    fn matrix_in_box(l: &[TransformFunction]) -> Matrix2 {
        list_matrix(l, Some((200.0, 4.0))).expect("a box resolves every function")
    }

    /// An interpolated value is the start of another interpolation, on
    /// either side; the nesting stops at [`MAX_NODES`].
    #[test]
    fn interpolated_values_nest_up_to_the_bound() {
        let a = list("translate(-50%)");
        let b = list("rotate(90deg)");
        let mid = interpolate_lists(&a, &b, 0.5).expect("interpolable");
        assert!(matches!(&mid[..], [TransformFunction::Interpolated(i)] if i.nodes == 1));
        // From the interpolated value to `none` and to a plain list: the
        // matrix at progress 0 is the start, at 1 the end.
        let none: Arc<[TransformFunction]> = Arc::from([]);
        for to in [&none, &b] {
            let start = matrix_in_box(&mid);
            let got = interpolate_lists(&mid, to, 0.0).expect("interpolable");
            assert!(close(&matrix_in_box(&got), &start));
            let got = interpolate_lists(to, &mid, 1.0).expect("interpolable");
            assert!(close(&matrix_in_box(&got), &start));
            let end = matrix_in_box(to);
            let got = interpolate_lists(&mid, to, 1.0).expect("interpolable");
            assert!(close(&matrix_in_box(&got), &end));
        }
        // A chain of transitions, each from the last value: one more node
        // each time, then discrete.
        let mut value = mid;
        for _ in 1..MAX_NODES {
            value = interpolate_lists(&value, &b, 0.5).expect("under the bound");
        }
        assert!(matches!(&value[..], [TransformFunction::Interpolated(i)] if i.nodes == MAX_NODES));
        assert!(interpolate_lists(&value, &b, 0.5).is_none());
        // Both sides nested: the nodes add up.
        let two = interpolate_lists(&value, &value, 0.5);
        assert!(two.is_none());
    }

    /// Huge progress values give finite matrices.
    #[test]
    fn huge_progress_gives_finite_matrices() {
        for (a, b) in [
            ("rotate(45deg)", "scale(2)"),
            ("translate(-50%)", "rotate(45deg)"),
        ] {
            for p in [1e300, -1e300, f64::MAX] {
                let got = interpolate_lists(&list(a), &list(b), p).expect("interpolable");
                let m = matrix_in_box(&got);
                for v in m {
                    assert!(!v.is_nan(), "{a} to {b} at {p}: {m:?}");
                }
            }
        }
    }

    #[test]
    fn decompose_round_trips() {
        for m in [
            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [0.5, 0.2, -0.3, 2.0, 4.0, -5.0],
            [-1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 1.0, 1.0, 0.0, 0.0, 0.0],
        ] {
            assert!(close(&recompose(&decompose(&m)), &m), "{m:?}");
        }
    }
}
