//! The 4x4 matrix of `DOMMatrix` (Geometry Interfaces Module Level 1 §6,
//! <https://drafts.fxtf.org/geometry/#dommatrix>): values in `f64`, column
//! major (`m[column * 4 + row]` is `m{column+1}{row+1}`: `m11, m12, m13,
//! m14, m21, ...`), and the 2D flag.
//!
//! The operations change the matrix in place as the `...Self` methods do;
//! the immutable methods apply them to a copy. Each operation is
//! post-multiplication (`this = this x operation`), written the way
//! Chromium's results show (measured, `tools/probes/web-geometry-matrix.json`):
//! a translation adds to the fourth column, a scale multiplies columns, a
//! skew adds one column to another, a rotation multiplies by the rotation
//! matrix. So an operation propagates `NaN` and the infinities as a product
//! does, and `scale(-1)` makes the zeros of the first two columns `-0`.
//! The sines and cosines of the multiples of 90 degrees are exact
//! (`rotate(90)` has `m11` exactly 0, `rotate(360)` is the identity).

#![allow(
    clippy::many_single_char_names,
    reason = "the names of the specification: x, y, z, w, a to f"
)]

use swb_style::{TransformOp, parse_transform_list};

/// A matrix with the 2D flag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Matrix {
    /// `m11 .. m44` in the order `m11, m12, m13, m14, m21, ...`.
    pub(crate) m: [f64; 16],
    /// Whether the matrix is a 2D matrix (`is2D`).
    pub(crate) is_2d: bool,
}

/// Why a string is not a matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    /// The text is not a transform list.
    Syntax,
    /// A length needs a context that the string does not have.
    Unresolvable,
}

/// The identity values.
const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// The sine and cosine of an angle in degrees; exact for the multiples of
/// 90 degrees. The angle is first reduced to less than one turn, as in
/// Chromium (measured: `rotate(1e17)` is `rotate(280)`).
fn sin_cos_degrees(degrees: f64) -> (f64, f64) {
    let degrees = degrees % 360.0;
    if degrees.is_finite() && degrees % 90.0 == 0.0 {
        let quarter = (degrees / 90.0).rem_euclid(4.0);
        return match quarter as u8 {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        };
    }
    degrees.to_radians().sin_cos()
}

impl Matrix {
    /// The identity, a 2D matrix.
    pub(crate) fn identity() -> Matrix {
        Matrix {
            m: IDENTITY,
            is_2d: true,
        }
    }

    /// A 2D matrix from `a, b, c, d, e, f`.
    pub(crate) fn from_2d(v: [f64; 6]) -> Matrix {
        let mut m = IDENTITY;
        let [a, b, c, d, e, f] = v;
        m[0] = a;
        m[1] = b;
        m[4] = c;
        m[5] = d;
        m[12] = e;
        m[13] = f;
        Matrix { m, is_2d: true }
    }

    /// A 3D matrix from 16 values.
    pub(crate) fn from_3d(m: [f64; 16]) -> Matrix {
        Matrix { m, is_2d: false }
    }

    /// A matrix from 6 or 16 values; `None` for another count.
    pub(crate) fn from_values(values: &[f64]) -> Option<Matrix> {
        if let Ok(v) = <[f64; 6]>::try_from(values) {
            Some(Matrix::from_2d(v))
        } else {
            <[f64; 16]>::try_from(values).ok().map(Matrix::from_3d)
        }
    }

    /// Whether every value is the identity's (`-0` is 0; `NaN` is not).
    pub(crate) fn is_identity(&self) -> bool {
        self.m.iter().zip(IDENTITY).all(|(a, b)| *a == b)
    }

    /// Whether every value is finite.
    pub(crate) fn is_finite(&self) -> bool {
        self.m.iter().all(|v| v.is_finite())
    }

    /// `translateSelf`: the fourth column gets the translation.
    pub(crate) fn translate(&mut self, tx: f64, ty: f64, tz: f64) {
        for r in 0..4 {
            let sum = tx * self.m[r] + ty * self.m[4 + r] + tz * self.m[8 + r];
            self.m[12 + r] += sum;
        }
        if tz != 0.0 {
            self.is_2d = false;
        }
    }

    /// The columns multiplied by the factors.
    fn scale_columns(&mut self, sx: f64, sy: f64, sz: f64) {
        for r in 0..4 {
            self.m[r] *= sx;
            self.m[4 + r] *= sy;
            self.m[8 + r] *= sz;
        }
    }

    /// `scaleSelf`: a scale around the origin point.
    pub(crate) fn scale(&mut self, sx: f64, sy: f64, sz: f64, ox: f64, oy: f64, oz: f64) {
        let about_origin = ox != 0.0 || oy != 0.0 || oz != 0.0;
        if about_origin {
            self.translate(ox, oy, oz);
        }
        self.scale_columns(sx, sy, sz);
        if about_origin {
            self.translate(-ox, -oy, -oz);
        }
        if sz != 1.0 || oz != 0.0 {
            self.is_2d = false;
        }
    }

    /// `self = self x other`.
    pub(crate) fn multiply(&mut self, other: &Matrix) {
        let a = self.m;
        let mut out = [0.0; 16];
        for c in 0..4 {
            for r in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += a[k * 4 + r] * other.m[c * 4 + k];
                }
                out[c * 4 + r] = sum;
            }
        }
        self.m = out;
        if !other.is_2d {
            self.is_2d = false;
        }
    }

    /// `self = other x self`.
    pub(crate) fn pre_multiply(&mut self, other: &Matrix) {
        let mut result = *other;
        result.multiply(self);
        // The flag of the result: 2D only if both are.
        result.is_2d = other.is_2d && self.is_2d;
        *self = result;
    }

    /// `rotateSelf`: rotations about z, then y, then x, in degrees. Missing
    /// `rot_y` and `rot_z` make `rot_x` the rotation about z.
    pub(crate) fn rotate(&mut self, rot_x: f64, rot_y: Option<f64>, rot_z: Option<f64>) {
        let (rx, ry, rz) = if rot_y.is_none() && rot_z.is_none() {
            (0.0, 0.0, rot_x)
        } else {
            (rot_x, rot_y.unwrap_or(0.0), rot_z.unwrap_or(0.0))
        };
        if rx != 0.0 || ry != 0.0 {
            self.is_2d = false;
        }
        if rz != 0.0 {
            self.multiply(&Matrix::rotation_z(rz));
        }
        if ry != 0.0 {
            self.multiply(&Matrix::rotation_y(ry));
        }
        if rx != 0.0 {
            self.multiply(&Matrix::rotation_x(rx));
        }
    }

    /// `rotateFromVectorSelf`: the rotation about z by the angle of the
    /// vector.
    pub(crate) fn rotate_from_vector(&mut self, x: f64, y: f64) {
        let degrees = y.atan2(x).to_degrees();
        if degrees != 0.0 {
            self.multiply(&Matrix::rotation_z(degrees));
        }
    }

    /// `rotateAxisAngleSelf`.
    pub(crate) fn rotate_axis_angle(&mut self, x: f64, y: f64, z: f64, degrees: f64) {
        if x != 0.0 || y != 0.0 {
            self.is_2d = false;
        }
        self.multiply(&Matrix::rotation(x, y, z, degrees));
    }

    /// The rotation about the z axis (CSS Transforms 1 `rotate()`).
    fn rotation_z(degrees: f64) -> Matrix {
        let (s, c) = sin_cos_degrees(degrees);
        let mut m = IDENTITY;
        m[0] = c;
        m[1] = s;
        m[4] = -s;
        m[5] = c;
        Matrix { m, is_2d: true }
    }

    /// The rotation about the y axis.
    fn rotation_y(degrees: f64) -> Matrix {
        let (s, c) = sin_cos_degrees(degrees);
        let mut m = IDENTITY;
        m[0] = c;
        m[2] = -s;
        m[8] = s;
        m[10] = c;
        Matrix { m, is_2d: false }
    }

    /// The rotation about the x axis.
    fn rotation_x(degrees: f64) -> Matrix {
        let (s, c) = sin_cos_degrees(degrees);
        let mut m = IDENTITY;
        m[5] = c;
        m[6] = s;
        m[9] = -s;
        m[10] = c;
        Matrix { m, is_2d: false }
    }

    /// The rotation about the axis (x, y, z) (CSS Transforms 2 `rotate3d()`,
    /// <https://drafts.csswg.org/css-transforms-2/#Rotate3dDefined>); the
    /// identity for a zero axis.
    fn rotation(x: f64, y: f64, z: f64, degrees: f64) -> Matrix {
        let length = (x * x + y * y + z * z).sqrt();
        if length == 0.0 {
            return Matrix::identity();
        }
        let (x, y, z) = (x / length, y / length, z / length);
        // An axis that is one of the coordinate axes: the exact rotation.
        if y == 0.0 && z == 0.0 {
            return Matrix::rotation_x(degrees * x.signum());
        }
        if x == 0.0 && z == 0.0 {
            return Matrix::rotation_y(degrees * y.signum());
        }
        if x == 0.0 && y == 0.0 {
            return Matrix::rotation_z(degrees * z.signum());
        }
        // Reduce to one turn before the half angle is taken (Chromium's
        // values for huge angles).
        let (half_sin, half_cos) = sin_cos_degrees((degrees % 360.0) / 2.0);
        let sc = half_sin * half_cos;
        let sq = half_sin * half_sin;
        let mut m = IDENTITY;
        // Row r, column c of the matrix of the specification is `m[c * 4 + r]`.
        m[0] = 1.0 - 2.0 * (y * y + z * z) * sq;
        m[1] = 2.0 * (x * y * sq + z * sc);
        m[2] = 2.0 * (x * z * sq - y * sc);
        m[4] = 2.0 * (x * y * sq - z * sc);
        m[5] = 1.0 - 2.0 * (x * x + z * z) * sq;
        m[6] = 2.0 * (y * z * sq + x * sc);
        m[8] = 2.0 * (x * z * sq + y * sc);
        m[9] = 2.0 * (y * z * sq - x * sc);
        m[10] = 1.0 - 2.0 * (x * x + y * y) * sq;
        Matrix { m, is_2d: false }
    }

    /// `skewXSelf`: the second column gets the first times the tangent.
    pub(crate) fn skew_x(&mut self, degrees: f64) {
        let t = degrees.to_radians().tan();
        for r in 0..4 {
            self.m[4 + r] += t * self.m[r];
        }
    }

    /// `skewYSelf`: the first column gets the second times the tangent.
    pub(crate) fn skew_y(&mut self, degrees: f64) {
        let t = degrees.to_radians().tan();
        for r in 0..4 {
            self.m[r] += t * self.m[4 + r];
        }
    }

    /// `invertSelf`: the inverse, or all `NaN` (and 3D) if the matrix is not
    /// invertible.
    pub(crate) fn invert(&mut self) {
        let inverse = if self.is_2d {
            self.inverse_2d()
        } else {
            self.inverse_3d()
        };
        if let Some(m) = inverse {
            self.m = m;
        } else {
            self.m = [f64::NAN; 16];
            self.is_2d = false;
        }
    }

    /// The inverse of a 2D matrix.
    fn inverse_2d(&self) -> Option<[f64; 16]> {
        let [a, b, c, d, e, f] = [
            self.m[0], self.m[1], self.m[4], self.m[5], self.m[12], self.m[13],
        ];
        let det = a * d - b * c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let mut m = IDENTITY;
        m[0] = d / det;
        m[1] = -b / det;
        m[4] = -c / det;
        m[5] = a / det;
        m[12] = (c * f - d * e) / det;
        m[13] = (b * e - a * f) / det;
        m.iter().all(|v| v.is_finite()).then_some(m)
    }

    /// The inverse of a 4x4 matrix by the adjugate.
    fn inverse_3d(&self) -> Option<[f64; 16]> {
        let a = &self.m;
        // Cofactors of `m[c * 4 + r]`: the usual expansion of the
        // determinant of the 4x4 matrix in 2x2 minors.
        let s0 = a[0] * a[5] - a[4] * a[1];
        let s1 = a[0] * a[9] - a[8] * a[1];
        let s2 = a[0] * a[13] - a[12] * a[1];
        let s3 = a[4] * a[9] - a[8] * a[5];
        let s4 = a[4] * a[13] - a[12] * a[5];
        let s5 = a[8] * a[13] - a[12] * a[9];
        let c5 = a[10] * a[15] - a[14] * a[11];
        let c4 = a[6] * a[15] - a[14] * a[7];
        let c3 = a[6] * a[11] - a[10] * a[7];
        let c2 = a[2] * a[15] - a[14] * a[3];
        let c1 = a[2] * a[11] - a[10] * a[3];
        let c0 = a[2] * a[7] - a[6] * a[3];
        let det = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let inv = 1.0 / det;
        let mut m = [0.0; 16];
        m[0] = (a[5] * c5 - a[9] * c4 + a[13] * c3) * inv;
        m[1] = (-a[1] * c5 + a[9] * c2 - a[13] * c1) * inv;
        m[2] = (a[1] * c4 - a[5] * c2 + a[13] * c0) * inv;
        m[3] = (-a[1] * c3 + a[5] * c1 - a[9] * c0) * inv;
        m[4] = (-a[4] * c5 + a[8] * c4 - a[12] * c3) * inv;
        m[5] = (a[0] * c5 - a[8] * c2 + a[12] * c1) * inv;
        m[6] = (-a[0] * c4 + a[4] * c2 - a[12] * c0) * inv;
        m[7] = (a[0] * c3 - a[4] * c1 + a[8] * c0) * inv;
        m[8] = (a[7] * s5 - a[11] * s4 + a[15] * s3) * inv;
        m[9] = (-a[3] * s5 + a[11] * s2 - a[15] * s1) * inv;
        m[10] = (a[3] * s4 - a[7] * s2 + a[15] * s0) * inv;
        m[11] = (-a[3] * s3 + a[7] * s1 - a[11] * s0) * inv;
        m[12] = (-a[6] * s5 + a[10] * s4 - a[14] * s3) * inv;
        m[13] = (a[2] * s5 - a[10] * s2 + a[14] * s1) * inv;
        m[14] = (-a[2] * s4 + a[6] * s2 - a[14] * s0) * inv;
        m[15] = (a[2] * s3 - a[6] * s1 + a[10] * s0) * inv;
        m.iter().all(|v| v.is_finite()).then_some(m)
    }

    /// `transformPoint`: the matrix times the point (x, y, z, w).
    pub(crate) fn transform_point(&self, p: [f64; 4]) -> [f64; 4] {
        let m = &self.m;
        if self.is_2d {
            // Chromium leaves z and w alone and does not mix them in
            // (measured: a point with x = NaN keeps z = 0 and w = 1).
            let [x, y, z, w] = p;
            return [
                m[0] * x + m[4] * y + m[12] * w,
                m[1] * x + m[5] * y + m[13] * w,
                z,
                w,
            ];
        }
        let mut out = [0.0; 4];
        for (r, slot) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for (k, v) in p.iter().enumerate() {
                sum += self.m[k * 4 + r] * v;
            }
            *slot = sum;
        }
        out
    }

    /// The matrix of a string: "parse a string into an abstract matrix"
    /// (<https://drafts.fxtf.org/geometry/#parse-a-string-into-an-abstract-matrix>).
    /// The empty string is the identity.
    pub(crate) fn from_string(text: &str) -> Result<Matrix, ParseError> {
        let mut matrix = Matrix::identity();
        if text.is_empty() {
            return Ok(matrix);
        }
        let ops = parse_transform_list(text).map_err(|e| match e {
            swb_style::TransformListError::Syntax => ParseError::Syntax,
            swb_style::TransformListError::Unresolvable => ParseError::Unresolvable,
        })?;
        for op in &ops {
            matrix.apply(op);
        }
        Ok(matrix)
    }

    /// Applies one function of a transform list after the ones before it.
    fn apply(&mut self, op: &TransformOp) {
        match *op {
            TransformOp::Matrix(v) => self.multiply(&Matrix::from_2d(v)),
            TransformOp::Matrix3d(v) => self.multiply(&Matrix::from_3d(v)),
            TransformOp::Translate { x, y, z, three_d } => {
                self.translate(x, y, z);
                if three_d {
                    self.is_2d = false;
                }
            }
            TransformOp::Scale { x, y, z, three_d } => {
                self.scale_columns(x, y, z);
                if three_d {
                    self.is_2d = false;
                }
            }
            TransformOp::Rotate {
                x,
                y,
                z,
                degrees,
                three_d,
            } => {
                self.multiply(&Matrix::rotation(x, y, z, degrees));
                if three_d {
                    self.is_2d = false;
                }
            }
            TransformOp::Skew { x, y } => {
                let mut skew = Matrix::identity();
                skew.m[1] = y.to_radians().tan();
                skew.m[4] = x.to_radians().tan();
                self.multiply(&skew);
            }
            TransformOp::Perspective(d) => {
                let mut perspective = Matrix::identity();
                perspective.m[11] = -1.0 / d;
                perspective.is_2d = false;
                self.multiply(&perspective);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64; 16], b: &[f64; 16]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn translate_and_scale_post_multiply() {
        let mut m = Matrix::from_2d([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        m.translate(10.0, 20.0, 0.0);
        assert_eq!((m.m[12], m.m[13]), (75.0, 106.0));
        assert!(m.is_2d);
        let mut s = Matrix::from_2d([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        s.scale(2.0, 2.0, 1.0, 10.0, 20.0, 0.0);
        assert_eq!((s.m[0], s.m[4], s.m[12], s.m[13]), (2.0, 6.0, -65.0, -94.0));
        let mut z = Matrix::identity();
        z.scale(1.0, 1.0, 2.0, 0.0, 0.0, 0.0);
        assert!(!z.is_2d);
    }

    #[test]
    fn translate_propagates_nan() {
        let mut m = Matrix::from_2d([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        m.translate(f64::NAN, 0.0, 0.0);
        assert!(m.m[12..].iter().all(|v| v.is_nan()));
        assert!(m.is_2d);
    }

    #[test]
    fn rotations_are_exact_at_quarter_turns() {
        let mut m = Matrix::identity();
        m.rotate(90.0, None, None);
        assert_eq!(&m.m[..2], &[0.0, 1.0]);
        assert_eq!(&m.m[4..6], &[-1.0, 0.0]);
        assert!(m.is_2d);
        let mut full = Matrix::identity();
        full.rotate(360.0, None, None);
        assert!(full.is_identity());
        let mut x = Matrix::identity();
        x.rotate(90.0, Some(0.0), Some(0.0));
        assert!(!x.is_2d);
        assert_eq!(x.m[6], 1.0);
        assert_eq!(x.m[9], -1.0);
        let mut axis = Matrix::identity();
        axis.rotate_axis_angle(1.0, 1.0, 1.0, 120.0);
        let expected = [
            0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        assert!(close(&axis.m, &expected), "{:?}", axis.m);
    }

    #[test]
    fn huge_angles_are_reduced_to_one_turn() {
        let rotated = |degrees: f64| {
            let mut m = Matrix::identity();
            m.rotate(degrees, None, None);
            m.m
        };
        // Chromium (measured): `rotate(1e17).a` is 0.17365 (`rotate(280)`).
        assert_eq!(rotated(1e17), rotated(280.0));
        assert!((rotated(1e17)[0] - 0.173_648_177_666_930_3).abs() < 1e-12);
        assert_eq!(rotated(-450.5), rotated(-90.5));
        // `rotateAxisAngle(1, 1, 0, 1e30).a` is 0.98063 in Chromium.
        let mut axis = Matrix::identity();
        axis.rotate_axis_angle(1.0, 1.0, 0.0, 1e30);
        assert!((axis.m[0] - 0.980_63).abs() < 1e-5, "{}", axis.m[0]);
    }

    #[test]
    fn inverse_cases() {
        let mut m = Matrix::from_2d([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        m.invert();
        assert_eq!(
            (m.m[0], m.m[1], m.m[4], m.m[5], m.m[12], m.m[13]),
            (-2.0, 1.0, 1.5, -0.5, 1.0, -2.0)
        );
        assert!(m.is_2d);
        let mut singular = Matrix::from_2d([1.0, 2.0, 2.0, 4.0, 0.0, 0.0]);
        singular.invert();
        assert!(singular.m.iter().all(|v| v.is_nan()));
        assert!(!singular.is_2d);
        let mut t = Matrix::identity();
        t.translate(5.0, 6.0, 7.0);
        let original = t;
        t.invert();
        let mut product = original;
        product.multiply(&t);
        assert!(product.is_identity());
        let mut p = Matrix::identity();
        p.m[11] = 0.5;
        p.is_2d = false;
        p.invert();
        assert_eq!(p.m[11], -0.5);
        let general = Matrix::from_3d([
            1.0, 2.0, 0.0, 1.0, 0.0, 1.0, 3.0, 0.0, 4.0, 0.0, 1.0, 2.0, 1.0, 1.0, 1.0, 1.0,
        ]);
        let mut inverse = general;
        inverse.invert();
        let mut product = general;
        product.multiply(&inverse);
        assert!(close(&product.m, &IDENTITY), "{:?}", product.m);
        let mut singular = Matrix::from_3d(std::array::from_fn(|i| (i + 1) as f64));
        singular.invert();
        assert!(singular.m.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn strings() {
        let m = Matrix::from_string("translate(1px) scale(2)").unwrap();
        assert_eq!((m.m[0], m.m[12]), (2.0, 1.0));
        assert!(m.is_2d);
        let m = Matrix::from_string("scale(2) translate(1px)").unwrap();
        assert_eq!(m.m[12], 2.0);
        let m = Matrix::from_string("translate3d(1px, 2px, 0px)").unwrap();
        assert!(!m.is_2d);
        assert!(Matrix::from_string("rotate(1turn)").unwrap().is_identity());
        assert!(Matrix::from_string("").unwrap().is_identity());
        assert_eq!(Matrix::from_string("foo"), Err(ParseError::Syntax));
        assert_eq!(
            Matrix::from_string("translate(1em)"),
            Err(ParseError::Unresolvable)
        );
        let p = Matrix::from_string("perspective(100px)").unwrap();
        assert_eq!(p.m[11], -0.01);
        assert_eq!(Matrix::from_string("perspective(0)").unwrap().m[11], -1.0);
    }

    #[test]
    fn point_and_skew() {
        let mut m = Matrix::from_2d([2.0, 0.0, 0.0, 2.0, 10.0, 20.0]);
        assert_eq!(
            m.transform_point([1.0, 1.0, 1.0, 1.0]),
            [12.0, 22.0, 1.0, 1.0]
        );
        m = Matrix::from_2d([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        m.skew_x(45.0);
        assert!((m.m[4] - 4.0).abs() < 1e-12 && (m.m[5] - 6.0).abs() < 1e-12);
        let mut flip = Matrix::from_2d([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        flip.scale(-1.0, 1.0, 1.0, 0.0, 0.0, 0.0);
        assert!(flip.m[2].is_sign_negative() && flip.m[2] == 0.0);
    }
}
