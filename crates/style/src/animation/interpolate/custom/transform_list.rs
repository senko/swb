//! The interpolation of a registered `<transform-list>` as text. Chromium
//! 148 keeps the function names in the computed value of the custom
//! property, so the result is a list of named functions, not the canonical
//! primitives of the `transform` property (ADR 0044 section 14; measured
//! in `registered-transform-list` of `tools/probes/css-animations-3a.json`):
//!
//! - `none` against a list is discrete (the pair flips at 0.5).
//! - The functions pair by position; the shorter list is padded with the
//!   identity of the function it meets (`translateY(0px)`).
//! - Two functions of one family interpolate by value: the same name keeps
//!   its name (`translateX(2.5px)`); `translate()`, `translateX()` and
//!   `translateY()` together give `translate(x[, y])`, and the three
//!   `scale` functions give `scale(x[, y])`. `skewX()` against `skewY()` or
//!   `skew()` is not a pair.
//! - Two `matrix()` functions interpolate as 2D matrices and stay
//!   `matrix(...)`.
//! - From the first position that is not a pair, the rest of both lists
//!   interpolates as one matrix, written `matrix3d(...)`. A rest with a
//!   percentage needs the reference box: the pair is discrete.
//!
//! The model has no 3D transforms: a list with a 3D function is discrete.

use swb_css::ComponentValue;

use super::super::{to_f32, transform};
use super::{number, transform_list_value as computed_list};
use crate::cssom::resolved::length_percentage_text;
use crate::values::{LengthPercentage, TransformFunction};

/// The name of a function, which decides how it pairs and how it is
/// written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Translate,
    TranslateX,
    TranslateY,
    Scale,
    ScaleX,
    ScaleY,
    Rotate,
    Skew,
    SkewX,
    SkewY,
    Matrix,
}

impl Kind {
    /// The kind of function `name`; `None` for a function that swb does not
    /// model (the 3D ones).
    fn of(name: &str) -> Option<Kind> {
        Some(match name.to_ascii_lowercase().as_str() {
            "translate" => Kind::Translate,
            "translatex" => Kind::TranslateX,
            "translatey" => Kind::TranslateY,
            "scale" => Kind::Scale,
            "scalex" => Kind::ScaleX,
            "scaley" => Kind::ScaleY,
            "rotate" => Kind::Rotate,
            "skew" => Kind::Skew,
            "skewx" => Kind::SkewX,
            "skewy" => Kind::SkewY,
            "matrix" => Kind::Matrix,
            _ => return None,
        })
    }

    /// The kind that two functions interpolate as; `None` if they do not
    /// pair.
    fn pair(self, other: Kind) -> Option<Kind> {
        use Kind::{Scale, ScaleX, ScaleY, Translate, TranslateX, TranslateY};
        if self == other {
            return Some(self);
        }
        match (self, other) {
            (Translate | TranslateX | TranslateY, Translate | TranslateX | TranslateY) => {
                Some(Translate)
            }
            (Scale | ScaleX | ScaleY, Scale | ScaleX | ScaleY) => Some(Scale),
            _ => None,
        }
    }
}

/// A function with its name and its canonical value.
type Item = (Kind, TransformFunction);

/// The functions of a computed `<transform-list>`; empty for `none`, `None`
/// if a function is not modelled.
fn items(tokens: &[ComponentValue]) -> Option<Vec<Item>> {
    let canonical = computed_list(tokens)?;
    let kinds: Option<Vec<Kind>> = tokens
        .iter()
        .filter_map(|t| match t {
            ComponentValue::Whitespace => None,
            ComponentValue::Function(f) => Some(Kind::of(&f.name)),
            _ => Some(None),
        })
        .collect();
    if canonical.is_empty() {
        return Some(Vec::new());
    }
    let kinds = kinds?;
    (kinds.len() == canonical.len())
        .then(|| kinds.into_iter().zip(canonical.iter().cloned()).collect())
}

/// Two computed lists at `p`, as the text of the value; `None` if the pair
/// is discrete.
pub(super) fn interpolate(a: &[ComponentValue], b: &[ComponentValue], p: f64) -> Option<String> {
    let (a, b) = (items(a)?, items(b)?);
    if a.is_empty() || b.is_empty() {
        return (a.is_empty() && b.is_empty()).then(|| "none".to_owned());
    }
    let mut texts = Vec::with_capacity(a.len().max(b.len()));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => (x.clone(), y.clone()),
            (Some(x), None) => (x.clone(), identity(x)),
            (None, Some(y)) => (identity(y), y.clone()),
            (None, None) => break,
        };
        let Some(kind) = x.0.pair(y.0) else {
            texts.push(rest(a.get(i..)?, b.get(i..)?, p)?);
            break;
        };
        texts.push(pair(kind, &x.1, &y.1, p)?);
    }
    Some(texts.join(" "))
}

/// The identity function of the kind of `item`.
fn identity(item: &Item) -> Item {
    (item.0, transform::identity_of(&item.1))
}

/// Two functions that pair, as text.
fn pair(kind: Kind, a: &TransformFunction, b: &TransformFunction, p: f64) -> Option<String> {
    if let (Kind::Matrix, TransformFunction::Matrix(x), TransformFunction::Matrix(y)) = (kind, a, b)
    {
        let m = transform::interpolate_matrices(&x.map(f64::from), &y.map(f64::from), p);
        let numbers: Vec<String> = m.iter().map(|v| number(to_f32(*v))).collect();
        return Some(format!("matrix({})", numbers.join(", ")));
    }
    function_text(kind, &transform::function_pair(a, b, p)?)
}

/// The rests of both lists as one `matrix3d()`; `None` if a percentage
/// needs the reference box.
fn rest(a: &[Item], b: &[Item], p: f64) -> Option<String> {
    let matrix = |items: &[Item]| {
        let list: Vec<TransformFunction> = items.iter().map(|i| i.1.clone()).collect();
        transform::list_matrix(&list, None)
    };
    let [a, b, c, d, e, f] =
        transform::interpolate_matrices(&matrix(a)?, &matrix(b)?, p).map(|v| number(to_f32(v)));
    let zero = "0".to_owned();
    let one = "1".to_owned();
    let cells = [
        a,
        b,
        zero.clone(),
        zero.clone(),
        c,
        d,
        zero.clone(),
        zero.clone(),
        zero.clone(),
        zero.clone(),
        one.clone(),
        zero.clone(),
        e,
        f,
        zero,
        one,
    ];
    Some(format!("matrix3d({})", cells.join(", ")))
}

/// A function of `kind` as Chromium writes it.
fn function_text(kind: Kind, f: &TransformFunction) -> Option<String> {
    use TransformFunction as T;
    Some(match (kind, f) {
        (Kind::Translate, T::Translate(x, y)) => {
            if is_zero(y) {
                format!("translate({})", length_percentage_text(x))
            } else {
                format!(
                    "translate({}, {})",
                    length_percentage_text(x),
                    length_percentage_text(y)
                )
            }
        }
        (Kind::TranslateX, T::Translate(x, _)) => {
            format!("translateX({})", length_percentage_text(x))
        }
        (Kind::TranslateY, T::Translate(_, y)) => {
            format!("translateY({})", length_percentage_text(y))
        }
        (Kind::Scale, T::Scale(x, y)) => {
            if x.to_bits() == y.to_bits() {
                format!("scale({})", number(*x))
            } else {
                format!("scale({}, {})", number(*x), number(*y))
            }
        }
        (Kind::ScaleX, T::Scale(x, _)) => format!("scaleX({})", number(*x)),
        (Kind::ScaleY, T::Scale(_, y)) => format!("scaleY({})", number(*y)),
        (Kind::Rotate, T::Rotate(a)) => format!("rotate({}deg)", number(*a)),
        (Kind::Skew, T::Skew(x, y)) => format!("skew({}deg, {}deg)", number(*x), number(*y)),
        (Kind::SkewX, T::Skew(x, _)) => format!("skewX({}deg)", number(*x)),
        (Kind::SkewY, T::Skew(_, y)) => format!("skewY({}deg)", number(*y)),
        _ => return None,
    })
}

/// True for the length 0 (Chromium writes `translate(10px)` for
/// `translate(10px, 0px)`).
fn is_zero(v: &LengthPercentage) -> bool {
    matches!(v, LengthPercentage::Px(px) if px.classify() == std::num::FpCategory::Zero)
}
