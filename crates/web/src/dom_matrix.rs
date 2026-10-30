//! `DOMMatrixReadOnly`, `DOMMatrix` (with the window alias `WebKitCSSMatrix`),
//! `DOMPointReadOnly` and `DOMPoint` of Geometry Interfaces Module Level 1
//! (<https://drafts.fxtf.org/geometry/>); M8 feature 11c1, ADR 0028
//! section 13.
//!
//! The matrix math is in [`crate::matrix`]; the string form of a matrix
//! comes from `swb_style::parse_transform_list`. The values of the
//! behavior that the specification leaves open or that Chromium differs on
//! were measured in Chromium 148 (`tools/probes/web-geometry-matrix.json`):
//! the constructor takes a string, a sequence of 6 or 16 numbers or
//! nothing (a `DOMMatrix` object is converted to a string, as any
//! object); a string that is not a transform list is a `SyntaxError`
//! ("Failed to parse '...'." or "Values must be resolvable at parse time"
//! for `em` and `%`); the stringifier throws `InvalidStateError` for `NaN`
//! and the infinities and writes numbers as `Number::toString` does.
//!
//! Departures: an explicit `undefined` in the middle of the arguments of
//! `scale()` and `rotate()` is a missing argument (Chromium converts it to
//! `NaN`); `calc()` is not accepted in a matrix string; `DOMQuad` and the
//! SVG point and matrix interfaces are not in swb.

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{DOMMatrixInit, DOMPointInit, DomStringOrUnrestrictedDoubleSequence};
use crate::matrix::{Matrix, ParseError};
use crate::state::WebState;
use crate::text_codec::byte_range;

// ----- Host data -----

/// The data of a `DOMMatrix` or `DOMMatrixReadOnly`.
struct MatrixData(Matrix);

impl GenericData for MatrixData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// The data of a `DOMPoint` or `DOMPointReadOnly`: x, y, z, w.
struct PointData([f64; 4]);

impl GenericData for PointData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// A new `DOMMatrix` of `realm`.
fn new_matrix(rt: &mut Runtime, realm: u32, matrix: Matrix) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(MatrixData(matrix))?;
    rt.new_host_object(realm, g::DOM_MATRIX, data)
}

/// A new `DOMPoint` of `realm`.
fn new_point(rt: &mut Runtime, realm: u32, point: [f64; 4]) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(PointData(point))?;
    rt.new_host_object(realm, g::DOM_POINT, data)
}

fn illegal_invocation() -> Exception {
    Exception::type_error("Illegal invocation")
}

/// The matrix of `this`.
fn matrix_of(rt: &Runtime, this: Platform) -> b::Result<Matrix> {
    match this.data {
        HostData::Generic(handle) => Ok(rt
            .heap()
            .generic::<MatrixData>(handle)
            .map_err(|_| illegal_invocation())?
            .0),
        HostData::Word(_) => Err(illegal_invocation()),
    }
}

/// Changes the matrix of `this`.
fn change_matrix(rt: &mut Runtime, this: Platform, f: impl FnOnce(&mut Matrix)) -> b::Result<()> {
    match this.data {
        HostData::Generic(handle) => {
            let data = rt
                .heap_mut()
                .generic_mut::<MatrixData>(handle)
                .map_err(|_| illegal_invocation())?;
            f(&mut data.0);
            Ok(())
        }
        HostData::Word(_) => Err(illegal_invocation()),
    }
}

/// The point of `this`.
fn point_of(rt: &Runtime, this: Platform) -> b::Result<[f64; 4]> {
    match this.data {
        HostData::Generic(handle) => Ok(rt
            .heap()
            .generic::<PointData>(handle)
            .map_err(|_| illegal_invocation())?
            .0),
        HostData::Word(_) => Err(illegal_invocation()),
    }
}

/// Changes one value of the point of `this`.
fn set_point(rt: &mut Runtime, this: Platform, index: usize, value: f64) -> b::Result<()> {
    match this.data {
        HostData::Generic(handle) => {
            let data = rt
                .heap_mut()
                .generic_mut::<PointData>(handle)
                .map_err(|_| illegal_invocation())?;
            if let Some(slot) = data.0.get_mut(index) {
                *slot = value;
            }
            Ok(())
        }
        HostData::Word(_) => Err(illegal_invocation()),
    }
}

// ----- Creating matrices -----

/// "Create a `DOMMatrix` from the 2D dictionary / the dictionary"
/// (<https://drafts.fxtf.org/geometry/#dom-dommatrixreadonly-frommatrix>):
/// the values and `is2D` of a `DOMMatrixInit`.
fn matrix_from_init(init: &DOMMatrixInit) -> b::Result<Matrix> {
    let mismatch = || Exception::type_error("Property mismatch on matrix initialization.");
    // A pair of members for one value: both present must be equal
    // (SameValueZero), and the longer name wins.
    let pick = |short: Option<f64>, long: Option<f64>, default: f64| -> b::Result<f64> {
        match (short, long) {
            (Some(a), Some(m)) if a == m || (a.is_nan() && m.is_nan()) => Ok(m),
            (Some(_), Some(_)) => Err(mismatch()),
            (Some(v), None) | (None, Some(v)) => Ok(v),
            (None, None) => Ok(default),
        }
    };
    let m11 = pick(init.a, init.m11, 1.0)?;
    let m12 = pick(init.b, init.m12, 0.0)?;
    let m21 = pick(init.c, init.m21, 0.0)?;
    let m22 = pick(init.d, init.m22, 1.0)?;
    let m41 = pick(init.e, init.m41, 0.0)?;
    let m42 = pick(init.f, init.m42, 0.0)?;
    let three_d_values = [
        init.m13, init.m14, init.m23, init.m24, init.m31, init.m32, init.m34, init.m43,
    ]
    .iter()
    .any(|v| *v != 0.0)
        || init.m33 != 1.0
        || init.m44 != 1.0;
    let is_2d = match init.is2_d {
        Some(true) if three_d_values => {
            return Err(Exception::type_error(
                "The is2D member is set to true but the input matrix is a 3d matrix.",
            ));
        }
        Some(flag) => flag,
        None => !three_d_values,
    };
    Ok(Matrix {
        m: [
            m11, m12, init.m13, init.m14, m21, m22, init.m23, init.m24, init.m31, init.m32,
            init.m33, init.m34, m41, m42, init.m43, init.m44,
        ],
        is_2d,
    })
}

/// The matrix of the constructor argument.
fn matrix_from_constructor(
    init: Option<DomStringOrUnrestrictedDoubleSequence>,
) -> b::Result<Matrix> {
    match init {
        None => Ok(Matrix::identity()),
        Some(DomStringOrUnrestrictedDoubleSequence::DomString(text)) => matrix_from_text(&text),
        Some(DomStringOrUnrestrictedDoubleSequence::UnrestrictedDoubleSequence(values)) => {
            Matrix::from_values(&values).ok_or_else(|| {
                Exception::type_error(
                    "The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.",
                )
            })
        }
    }
}

/// The matrix of a transform list as text.
fn matrix_from_text(text: &str) -> b::Result<Matrix> {
    Matrix::from_string(text).map_err(|e| match e {
        ParseError::Syntax => Exception::dom("SyntaxError", format!("Failed to parse '{text}'.")),
        ParseError::Unresolvable => {
            Exception::dom("SyntaxError", "Values must be resolvable at parse time")
        }
    })
}

/// The matrix of a `Float32Array` or `Float64Array` of 6 or 16 elements.
fn matrix_from_floats(rt: &mut Runtime, array: Gc<Object>, wide: bool) -> b::Result<Matrix> {
    let (buffer, offset, length) = byte_range(rt, array)?;
    let size = if wide { 8 } else { 4 };
    let count = length / size;
    if count != 6 && count != 16 {
        return Err(Exception::type_error(
            "The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.",
        ));
    }
    let bytes = rt.buffer_range(buffer, offset, count * size)?;
    let values: Vec<f64> = bytes
        .chunks_exact(size)
        .map(|chunk| {
            if wide {
                f64::from_le_bytes(chunk.try_into().unwrap_or([0; 8]))
            } else {
                f64::from(f32::from_le_bytes(chunk.try_into().unwrap_or([0; 4])))
            }
        })
        .collect();
    Matrix::from_values(&values).ok_or_else(|| Exception::type_error("Illegal sequence length"))
}

/// A `Float32Array` or `Float64Array` with the 16 values.
fn float_array(rt: &mut Runtime, realm: u32, matrix: &Matrix, wide: bool) -> VmResult<Gc<Object>> {
    let mut bytes = Vec::with_capacity(16 * 8);
    for v in matrix.m {
        if wide {
            bytes.extend_from_slice(&v.to_le_bytes());
        } else {
            bytes.extend_from_slice(&(v as f32).to_le_bytes());
        }
    }
    let length = bytes.len();
    let buffer = rt.create_array_buffer(realm, bytes, None)?;
    rt.create_view(
        realm,
        Some(if wide { "Float64" } else { "Float32" }),
        buffer,
        0,
        Some(length),
    )
}

/// A new matrix object of `class` (`DOMMatrix` or `DOMMatrixReadOnly`).
fn new_matrix_of_class(
    rt: &mut Runtime,
    realm: u32,
    class: swb_js::ClassId,
    matrix: Matrix,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(MatrixData(matrix))?;
    rt.new_host_object(realm, class, data)
}

/// The members of `toJSON`.
fn matrix_json(rt: &mut Runtime, realm: u32, matrix: &Matrix) -> b::Result<Gc<Object>> {
    const NAMES: [&str; 16] = [
        "m11", "m12", "m13", "m14", "m21", "m22", "m23", "m24", "m31", "m32", "m33", "m34", "m41",
        "m42", "m43", "m44",
    ];
    let m = &matrix.m;
    let object = rt.new_plain_object(realm)?;
    let first = [
        ("a", m[0]),
        ("b", m[1]),
        ("c", m[4]),
        ("d", m[5]),
        ("e", m[12]),
        ("f", m[13]),
    ];
    for (key, value) in first {
        b::to_js::dictionary_member(rt, object, key, Value::number(value))?;
    }
    for (key, value) in NAMES.iter().zip(m) {
        b::to_js::dictionary_member(rt, object, key, Value::number(*value))?;
    }
    b::to_js::dictionary_member(rt, object, "is2D", Value::Bool(matrix.is_2d))?;
    b::to_js::dictionary_member(rt, object, "isIdentity", Value::Bool(matrix.is_identity()))?;
    Ok(object)
}

// ----- The read-only operations -----

/// Applies `f` to a copy of the matrix of `this`: a new `DOMMatrix`.
fn derived(cx: &mut Cx<'_>, this: Platform, f: impl FnOnce(&mut Matrix)) -> b::Result<Gc<Object>> {
    let mut matrix = matrix_of(cx.rt, this)?;
    f(&mut matrix);
    let realm = cx.realm();
    Ok(new_matrix(cx.rt, realm, matrix)?)
}

/// Applies `f` to the matrix of `this` and returns `this`.
fn in_place(cx: &mut Cx<'_>, this: Platform, f: impl FnOnce(&mut Matrix)) -> b::Result<Gc<Object>> {
    change_matrix(cx.rt, this, f)?;
    Ok(this.object)
}

/// `scale()`: `scaleY` is `scaleX` if missing.
fn scale_by(m: &mut Matrix, x: f64, y: Option<f64>, z: f64, origin: [f64; 3]) {
    m.scale(x, y.unwrap_or(x), z, origin[0], origin[1], origin[2]);
}

/// The getters of the 22 aliases of the matrix values and the 2D flags.
macro_rules! getters {
    ($($name:ident = $index:expr;)+) => {
        $(
            fn $name(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
                Ok(matrix_of(cx.rt, this)?.m[$index])
            }
        )+
    };
}

impl g::dom_matrix_read_only::DOMMatrixReadOnlyImpl for WebState {
    getters! {
        a = 0; b = 1; c = 4; d = 5; e = 12; f = 13;
        m11 = 0; m12 = 1; m13 = 2; m14 = 3;
        m21 = 4; m22 = 5; m23 = 6; m24 = 7;
        m31 = 8; m32 = 9; m33 = 10; m34 = 11;
        m41 = 12; m42 = 13; m43 = 14; m44 = 15;
    }

    fn is2_d(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(matrix_of(cx.rt, this)?.is_2d)
    }

    fn is_identity(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(matrix_of(cx.rt, this)?.is_identity())
    }

    fn constructor(
        cx: &mut Cx<'_>,
        init: Option<DomStringOrUnrestrictedDoubleSequence>,
    ) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_constructor(init)?;
        let data = cx.rt.alloc_host_data(MatrixData(matrix))?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn from_matrix(cx: &mut Cx<'_>, other: DOMMatrixInit) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_init(&other)?;
        let realm = cx.realm();
        Ok(new_matrix_of_class(
            cx.rt,
            realm,
            g::DOM_MATRIX_READ_ONLY,
            matrix,
        )?)
    }

    fn from_float32_array(cx: &mut Cx<'_>, array32: Gc<Object>) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_floats(cx.rt, array32, false)?;
        let realm = cx.realm();
        Ok(new_matrix_of_class(
            cx.rt,
            realm,
            g::DOM_MATRIX_READ_ONLY,
            matrix,
        )?)
    }

    fn from_float64_array(cx: &mut Cx<'_>, array64: Gc<Object>) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_floats(cx.rt, array64, true)?;
        let realm = cx.realm();
        Ok(new_matrix_of_class(
            cx.rt,
            realm,
            g::DOM_MATRIX_READ_ONLY,
            matrix,
        )?)
    }

    fn flip_x(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.scale(-1.0, 1.0, 1.0, 0.0, 0.0, 0.0))
    }

    fn flip_y(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.scale(1.0, -1.0, 1.0, 0.0, 0.0, 0.0))
    }

    fn inverse(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        derived(cx, this, Matrix::invert)
    }

    fn multiply(cx: &mut Cx<'_>, this: Platform, other: DOMMatrixInit) -> b::Result<Gc<Object>> {
        let other = matrix_from_init(&other)?;
        derived(cx, this, |m| m.multiply(&other))
    }

    fn rotate(
        cx: &mut Cx<'_>,
        this: Platform,
        rot_x: f64,
        rot_y: Option<f64>,
        rot_z: Option<f64>,
    ) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.rotate(rot_x, rot_y, rot_z))
    }

    fn rotate_axis_angle(
        cx: &mut Cx<'_>,
        this: Platform,
        x: f64,
        y: f64,
        z: f64,
        angle: f64,
    ) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.rotate_axis_angle(x, y, z, angle))
    }

    fn rotate_from_vector(
        cx: &mut Cx<'_>,
        this: Platform,
        x: f64,
        y: f64,
    ) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.rotate_from_vector(x, y))
    }

    fn scale(
        cx: &mut Cx<'_>,
        this: Platform,
        scale_x: f64,
        scale_y: Option<f64>,
        scale_z: f64,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
    ) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| {
            scale_by(m, scale_x, scale_y, scale_z, [origin_x, origin_y, origin_z]);
        })
    }

    fn scale3d(
        cx: &mut Cx<'_>,
        this: Platform,
        scale: f64,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
    ) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| {
            m.scale(scale, scale, scale, origin_x, origin_y, origin_z);
        })
    }

    fn scale_non_uniform(
        cx: &mut Cx<'_>,
        this: Platform,
        scale_x: f64,
        scale_y: f64,
    ) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.scale(scale_x, scale_y, 1.0, 0.0, 0.0, 0.0))
    }

    fn skew_x(cx: &mut Cx<'_>, this: Platform, sx: f64) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.skew_x(sx))
    }

    fn skew_y(cx: &mut Cx<'_>, this: Platform, sy: f64) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.skew_y(sy))
    }

    fn to_float32_array(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let matrix = matrix_of(cx.rt, this)?;
        let realm = cx.realm();
        Ok(float_array(cx.rt, realm, &matrix, false)?)
    }

    fn to_float64_array(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let matrix = matrix_of(cx.rt, this)?;
        let realm = cx.realm();
        Ok(float_array(cx.rt, realm, &matrix, true)?)
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let matrix = matrix_of(cx.rt, this)?;
        let realm = cx.realm();
        matrix_json(cx.rt, realm, &matrix)
    }

    fn transform_point(
        cx: &mut Cx<'_>,
        this: Platform,
        point: DOMPointInit,
    ) -> b::Result<Gc<Object>> {
        let matrix = matrix_of(cx.rt, this)?;
        let out = matrix.transform_point([point.x, point.y, point.z, point.w]);
        let realm = cx.realm();
        Ok(new_point(cx.rt, realm, out)?)
    }

    fn translate(
        cx: &mut Cx<'_>,
        this: Platform,
        tx: f64,
        ty: f64,
        tz: f64,
    ) -> b::Result<Gc<Object>> {
        derived(cx, this, |m| m.translate(tx, ty, tz))
    }

    /// <https://drafts.fxtf.org/geometry/#dommatrixreadonly-stringification-behavior>
    fn stringify(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let matrix = matrix_of(cx.rt, this)?;
        if !matrix.is_finite() {
            return Err(Exception::dom(
                "InvalidStateError",
                "DOMMatrix cannot be serialized with NaN or Infinity values.",
            ));
        }
        let number = swb_js_text::number::number_to_string;
        let m = &matrix.m;
        let values: Vec<String> = if matrix.is_2d {
            [m[0], m[1], m[4], m[5], m[12], m[13]]
                .iter()
                .map(|v| number(*v))
                .collect()
        } else {
            m.iter().map(|v| number(*v)).collect()
        };
        let name = if matrix.is_2d { "matrix" } else { "matrix3d" };
        Ok(format!("{name}({})", values.join(", ")))
    }
}

/// The accessors of the 22 aliases in `DOMMatrix`: the getter and the
/// setter. A setter of a 3D member makes the matrix 3D
/// (<https://drafts.fxtf.org/geometry/#dom-dommatrix-m13>).
macro_rules! accessors {
    ($($get:ident, $set:ident = $index:expr, $three_d_unless:expr;)+) => {
        $(
            fn $get(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
                Ok(matrix_of(cx.rt, this)?.m[$index])
            }

            fn $set(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
                change_matrix(cx.rt, this, |m| {
                    m.m[$index] = value;
                    if let Some(neutral) = $three_d_unless {
                        if value != neutral {
                            m.is_2d = false;
                        }
                    }
                })
            }
        )+
    };
}

impl g::dom_matrix::DOMMatrixImpl for WebState {
    accessors! {
        a, set_a = 0, None::<f64>;
        b, set_b = 1, None::<f64>;
        c, set_c = 4, None::<f64>;
        d, set_d = 5, None::<f64>;
        e, set_e = 12, None::<f64>;
        f, set_f = 13, None::<f64>;
        m11, set_m11 = 0, None::<f64>;
        m12, set_m12 = 1, None::<f64>;
        m13, set_m13 = 2, Some(0.0);
        m14, set_m14 = 3, Some(0.0);
        m21, set_m21 = 4, None::<f64>;
        m22, set_m22 = 5, None::<f64>;
        m23, set_m23 = 6, Some(0.0);
        m24, set_m24 = 7, Some(0.0);
        m31, set_m31 = 8, Some(0.0);
        m32, set_m32 = 9, Some(0.0);
        m33, set_m33 = 10, Some(1.0);
        m34, set_m34 = 11, Some(0.0);
        m41, set_m41 = 12, None::<f64>;
        m42, set_m42 = 13, None::<f64>;
        m43, set_m43 = 14, Some(0.0);
        m44, set_m44 = 15, Some(1.0);
    }

    fn constructor(
        cx: &mut Cx<'_>,
        init: Option<DomStringOrUnrestrictedDoubleSequence>,
    ) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_constructor(init)?;
        let data = cx.rt.alloc_host_data(MatrixData(matrix))?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn from_matrix(cx: &mut Cx<'_>, other: DOMMatrixInit) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_init(&other)?;
        let realm = cx.realm();
        Ok(new_matrix(cx.rt, realm, matrix)?)
    }

    fn from_float32_array(cx: &mut Cx<'_>, array32: Gc<Object>) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_floats(cx.rt, array32, false)?;
        let realm = cx.realm();
        Ok(new_matrix(cx.rt, realm, matrix)?)
    }

    fn from_float64_array(cx: &mut Cx<'_>, array64: Gc<Object>) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_floats(cx.rt, array64, true)?;
        let realm = cx.realm();
        Ok(new_matrix(cx.rt, realm, matrix)?)
    }

    fn invert_self(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        in_place(cx, this, Matrix::invert)
    }

    fn multiply_self(
        cx: &mut Cx<'_>,
        this: Platform,
        other: DOMMatrixInit,
    ) -> b::Result<Gc<Object>> {
        let other = matrix_from_init(&other)?;
        in_place(cx, this, |m| m.multiply(&other))
    }

    fn pre_multiply_self(
        cx: &mut Cx<'_>,
        this: Platform,
        other: DOMMatrixInit,
    ) -> b::Result<Gc<Object>> {
        let other = matrix_from_init(&other)?;
        in_place(cx, this, |m| m.pre_multiply(&other))
    }

    fn rotate_axis_angle_self(
        cx: &mut Cx<'_>,
        this: Platform,
        x: f64,
        y: f64,
        z: f64,
        angle: f64,
    ) -> b::Result<Gc<Object>> {
        in_place(cx, this, |m| m.rotate_axis_angle(x, y, z, angle))
    }

    fn rotate_from_vector_self(
        cx: &mut Cx<'_>,
        this: Platform,
        x: f64,
        y: f64,
    ) -> b::Result<Gc<Object>> {
        in_place(cx, this, |m| m.rotate_from_vector(x, y))
    }

    fn rotate_self(
        cx: &mut Cx<'_>,
        this: Platform,
        rot_x: f64,
        rot_y: Option<f64>,
        rot_z: Option<f64>,
    ) -> b::Result<Gc<Object>> {
        in_place(cx, this, |m| m.rotate(rot_x, rot_y, rot_z))
    }

    fn scale3d_self(
        cx: &mut Cx<'_>,
        this: Platform,
        scale: f64,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
    ) -> b::Result<Gc<Object>> {
        in_place(cx, this, |m| {
            m.scale(scale, scale, scale, origin_x, origin_y, origin_z);
        })
    }

    fn scale_self(
        cx: &mut Cx<'_>,
        this: Platform,
        scale_x: f64,
        scale_y: Option<f64>,
        scale_z: f64,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
    ) -> b::Result<Gc<Object>> {
        in_place(cx, this, |m| {
            scale_by(m, scale_x, scale_y, scale_z, [origin_x, origin_y, origin_z]);
        })
    }

    fn skew_x_self(cx: &mut Cx<'_>, this: Platform, sx: f64) -> b::Result<Gc<Object>> {
        in_place(cx, this, |m| m.skew_x(sx))
    }

    fn skew_y_self(cx: &mut Cx<'_>, this: Platform, sy: f64) -> b::Result<Gc<Object>> {
        in_place(cx, this, |m| m.skew_y(sy))
    }

    fn translate_self(
        cx: &mut Cx<'_>,
        this: Platform,
        tx: f64,
        ty: f64,
        tz: f64,
    ) -> b::Result<Gc<Object>> {
        in_place(cx, this, |m| m.translate(tx, ty, tz))
    }

    /// `setMatrixValue()`: the matrix of the string replaces the values
    /// (and the flag); a failure leaves the matrix as it was.
    fn set_matrix_value(
        cx: &mut Cx<'_>,
        this: Platform,
        transform_list: String,
    ) -> b::Result<Gc<Object>> {
        let matrix = matrix_from_text(&transform_list)?;
        in_place(cx, this, |m| *m = matrix)
    }
}

// ----- Points -----

/// The point of a `DOMPointInit`.
fn point_of_init(init: &DOMPointInit) -> [f64; 4] {
    [init.x, init.y, init.z, init.w]
}

impl g::dom_point_read_only::DOMPointReadOnlyImpl for WebState {
    fn x(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(point_of(cx.rt, this)?[0])
    }

    fn y(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(point_of(cx.rt, this)?[1])
    }

    fn z(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(point_of(cx.rt, this)?[2])
    }

    fn w(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(point_of(cx.rt, this)?[3])
    }

    fn constructor(cx: &mut Cx<'_>, x: f64, y: f64, z: f64, w: f64) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(PointData([x, y, z, w]))?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn from_point(cx: &mut Cx<'_>, other: DOMPointInit) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(PointData(point_of_init(&other)))?;
        let realm = cx.realm();
        Ok(cx.rt.new_host_object(realm, g::DOM_POINT_READ_ONLY, data)?)
    }

    /// `matrixTransform()`: the point times the matrix of the dictionary.
    fn matrix_transform(
        cx: &mut Cx<'_>,
        this: Platform,
        matrix: DOMMatrixInit,
    ) -> b::Result<Gc<Object>> {
        let point = point_of(cx.rt, this)?;
        let matrix = matrix_from_init(&matrix)?;
        let realm = cx.realm();
        Ok(new_point(cx.rt, realm, matrix.transform_point(point))?)
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let point = point_of(cx.rt, this)?;
        let realm = cx.realm();
        let object = cx.rt.new_plain_object(realm)?;
        for (key, value) in ["x", "y", "z", "w"].into_iter().zip(point) {
            b::to_js::dictionary_member(cx.rt, object, key, Value::number(value))?;
        }
        Ok(object)
    }
}

impl g::dom_point::DOMPointImpl for WebState {
    fn x(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(point_of(cx.rt, this)?[0])
    }

    fn set_x(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
        set_point(cx.rt, this, 0, value)
    }

    fn y(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(point_of(cx.rt, this)?[1])
    }

    fn set_y(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
        set_point(cx.rt, this, 1, value)
    }

    fn z(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(point_of(cx.rt, this)?[2])
    }

    fn set_z(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
        set_point(cx.rt, this, 2, value)
    }

    fn w(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(point_of(cx.rt, this)?[3])
    }

    fn set_w(cx: &mut Cx<'_>, this: Platform, value: f64) -> b::Result<()> {
        set_point(cx.rt, this, 3, value)
    }

    fn constructor(cx: &mut Cx<'_>, x: f64, y: f64, z: f64, w: f64) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(PointData([x, y, z, w]))?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn from_point(cx: &mut Cx<'_>, other: DOMPointInit) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        Ok(new_point(cx.rt, realm, point_of_init(&other))?)
    }
}
