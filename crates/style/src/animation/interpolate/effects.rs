//! The interpolation of the effect values (ADR 0044 section 4): the
//! individual transform properties (CSS Transforms 2 §5), `filter` and
//! `backdrop-filter` (Filter Effects 1 §11,
//! <https://www.w3.org/TR/filter-effects-1/#interpolation-of-filters>),
//! `box-shadow` and `text-shadow` (CSS Backgrounds 3 §7.2,
//! <https://www.w3.org/TR/css-backgrounds-3/#box-shadow>).

use std::sync::Arc;

use super::{color, lerp, lp};
use crate::values::{
    BoxShadow, Color, DropShadow, Filter, FilterFunction, LengthPercentage, Rgba, Rotate, Scale,
    TextShadow, Translate,
};

// ----- translate, rotate, scale -----

/// `translate`: per component; `none` is 0 0 0; percentages stay
/// (`translate: 5px 25%`).
pub(super) fn translate(a: Option<&Translate>, b: Option<&Translate>, p: f64) -> Translate {
    let zero = Translate {
        x: LengthPercentage::ZERO,
        y: LengthPercentage::ZERO,
        z: 0.0,
    };
    let (x, y) = (a.unwrap_or(&zero), b.unwrap_or(&zero));
    Translate {
        x: lp(&x.x, &y.x, p, false),
        y: lp(&x.y, &y.y, p, false),
        z: lerp(x.z, y.z, p),
    }
}

/// `rotate`: the angle, when both have the same axis (`none` takes the
/// other's axis); rotations about different axes are discrete (swb has no
/// 3D rotation): `None`.
pub(super) fn rotate(a: Option<Rotate>, b: Option<Rotate>, p: f64) -> Option<Rotate> {
    let axis_a = a.map(|r| normalize(r.axis));
    let axis_b = b.map(|r| normalize(r.axis));
    if let (Some(x), Some(y)) = (axis_a, axis_b)
        && !same_axis(x, y)
    {
        return None;
    }
    let angle = |r: Option<Rotate>| r.map_or(0.0, |r| r.angle);
    Some(Rotate {
        axis: a.or(b).map_or([0.0, 0.0, 1.0], |r| r.axis),
        angle: lerp(angle(a), angle(b), p),
    })
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 {
        v.map(|c| c / len)
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn same_axis(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// `scale`: per factor; `none` is 1 1 1.
pub(super) fn scale(a: Option<Scale>, b: Option<Scale>, p: f64) -> Scale {
    let one = Scale {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    let (x, y) = (a.unwrap_or(one), b.unwrap_or(one));
    Scale {
        x: lerp(x.x, y.x, p),
        y: lerp(x.y, y.y, p),
        z: lerp(x.z, y.z, p),
    }
}

// ----- filter -----

/// The initial value of the kind of `f` (for padding a shorter list).
fn initial_of(f: &FilterFunction) -> Option<FilterFunction> {
    use FilterFunction as F;
    Some(match f {
        F::Blur(_) => F::Blur(0.0),
        F::Brightness(_) => F::Brightness(1.0),
        F::Contrast(_) => F::Contrast(1.0),
        F::Grayscale(_) => F::Grayscale(0.0),
        F::HueRotate(_) => F::HueRotate(0.0),
        F::Invert(_) => F::Invert(0.0),
        F::Opacity(_) => F::Opacity(1.0),
        F::Saturate(_) => F::Saturate(1.0),
        F::Sepia(_) => F::Sepia(0.0),
        F::DropShadow(_) => F::DropShadow(DropShadow {
            color: Color::Rgba(Rgba::TRANSPARENT),
            x: 0.0,
            y: 0.0,
            blur: 0.0,
        }),
        F::Url(_) => return None,
    })
}

/// Two functions of the same kind at `p`, clamped to their ranges.
fn function_pair(
    a: &FilterFunction,
    b: &FilterFunction,
    current: Rgba,
    p: f64,
) -> Option<FilterFunction> {
    use FilterFunction as F;
    let non_negative = |x: f32, y: f32| lerp(x, y, p).max(0.0);
    let unit = |x: f32, y: f32| lerp(x, y, p).clamp(0.0, 1.0);
    Some(match (a, b) {
        (F::Blur(x), F::Blur(y)) => F::Blur(non_negative(*x, *y)),
        (F::Brightness(x), F::Brightness(y)) => F::Brightness(non_negative(*x, *y)),
        (F::Contrast(x), F::Contrast(y)) => F::Contrast(non_negative(*x, *y)),
        (F::Grayscale(x), F::Grayscale(y)) => F::Grayscale(unit(*x, *y)),
        (F::HueRotate(x), F::HueRotate(y)) => F::HueRotate(lerp(*x, *y, p)),
        (F::Invert(x), F::Invert(y)) => F::Invert(unit(*x, *y)),
        (F::Opacity(x), F::Opacity(y)) => F::Opacity(unit(*x, *y)),
        (F::Saturate(x), F::Saturate(y)) => F::Saturate(non_negative(*x, *y)),
        (F::Sepia(x), F::Sepia(y)) => F::Sepia(unit(*x, *y)),
        (F::DropShadow(x), F::DropShadow(y)) => F::DropShadow(DropShadow {
            color: color::mix(x.color, y.color, current, p),
            x: lerp(x.x, y.x, p),
            y: lerp(x.y, y.y, p),
            blur: non_negative(x.blur, y.blur),
        }),
        _ => return None,
    })
}

/// `filter` and `backdrop-filter`: function by function when the shorter
/// list is a prefix of the longer one in kind, padded with initial values
/// (`none` is the other list with initial values); other pairs are
/// discrete (`f-mismatch`).
pub(super) fn filter(a: &Filter, b: &Filter, current: Rgba, p: f64) -> Option<Filter> {
    let (fa, fb) = (a.functions(), b.functions());
    let len = fa.len().max(fb.len());
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let (x, y) = match (fa.get(i), fb.get(i)) {
            (Some(x), Some(y)) => (x.clone(), y.clone()),
            (Some(x), None) => (x.clone(), initial_of(x)?),
            (None, Some(y)) => (initial_of(y)?, y.clone()),
            (None, None) => break,
        };
        out.push(function_pair(&x, &y, current, p)?);
    }
    Some(Filter::new(out))
}

// ----- shadows -----

/// The transparent shadow that pads a shorter list.
fn padding_box_shadow(inset: bool) -> BoxShadow {
    BoxShadow {
        color: Color::Rgba(Rgba::TRANSPARENT),
        x: 0.0,
        y: 0.0,
        blur: 0.0,
        spread: 0.0,
        inset,
    }
}

/// `box-shadow`: shadow by shadow (color, offsets, blur, spread); the
/// shorter list is padded with `transparent 0 0 0 0` of the other's
/// `inset`; a pair that differs in `inset` makes the list discrete
/// (`sh-inset`).
pub(super) fn box_shadows(
    a: &Arc<[BoxShadow]>,
    b: &Arc<[BoxShadow]>,
    current: Rgba,
    p: f64,
) -> Option<Arc<[BoxShadow]>> {
    let len = a.len().max(b.len());
    (0..len)
        .map(|i| {
            let (x, y) = match (a.get(i), b.get(i)) {
                (Some(x), Some(y)) => (*x, *y),
                (Some(x), None) => (*x, padding_box_shadow(x.inset)),
                (None, Some(y)) => (padding_box_shadow(y.inset), *y),
                (None, None) => return None,
            };
            if x.inset != y.inset {
                return None;
            }
            Some(BoxShadow {
                color: color::mix(x.color, y.color, current, p),
                x: lerp(x.x, y.x, p),
                y: lerp(x.y, y.y, p),
                blur: lerp(x.blur, y.blur, p).max(0.0),
                spread: lerp(x.spread, y.spread, p),
                inset: x.inset,
            })
        })
        .collect()
}

/// `text-shadow`: as `box-shadow`, without spread and `inset`.
pub(super) fn text_shadows(
    a: &Arc<[TextShadow]>,
    b: &Arc<[TextShadow]>,
    current: Rgba,
    p: f64,
) -> Option<Arc<[TextShadow]>> {
    let padding = TextShadow {
        color: Color::Rgba(Rgba::TRANSPARENT),
        x: 0.0,
        y: 0.0,
        blur: 0.0,
    };
    let len = a.len().max(b.len());
    (0..len)
        .map(|i| {
            let x = a.get(i).copied().unwrap_or(padding);
            let y = b.get(i).copied().unwrap_or(padding);
            Some(TextShadow {
                color: color::mix(x.color, y.color, current, p),
                x: lerp(x.x, y.x, p),
                y: lerp(x.y, y.y, p),
                blur: lerp(x.blur, y.blur, p).max(0.0),
            })
        })
        .collect()
}
