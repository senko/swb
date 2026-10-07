//! Parsing and computing `transform`, `transform-origin` (CSS Transforms 1,
//! <https://www.w3.org/TR/css-transforms-1/>) and `clip` (CSS 2.2 §11.1.2,
//! <https://www.w3.org/TR/CSS22/visufx.html#clipping>).
//!
//! Only 2D transforms are supported. The 3D functions that look the same
//! as a 2D function without `perspective` (`translate3d()`,
//! `translateZ()`, `scale3d()`, `scaleZ()`, `rotateZ()`) are accepted as
//! that function (`translateZ()` and `scaleZ()` as the identity); the
//! other 3D functions make the declaration invalid.

use std::sync::Arc;

use swb_css::{ParseError, Parser};

use super::compute::ComputeContext;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::parse::{ParseResult, parse_angle, parse_number};
use crate::values::{
    ClipRect, SpecifiedLengthPercentage as Lp, TransformFunction, TransformOrigin,
};

/// A specified 2D transform function (lengths not computed yet).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedTransformFunction {
    /// `matrix()`.
    Matrix([f32; 6]),
    /// `translate()` and its one-axis forms.
    Translate(Lp, Lp),
    /// `scale()` and its one-axis forms.
    Scale(f32, f32),
    /// `rotate()`, in degrees.
    Rotate(f32),
    /// `skew()` and its one-axis forms, in degrees.
    Skew(f32, f32),
}

/// A specified `transform`: the functions in order; empty for `none`.
pub(crate) type SpecifiedTransform = Arc<[SpecifiedTransformFunction]>;

/// A specified `transform-origin`: horizontal and vertical offsets.
pub(crate) type SpecifiedTransformOrigin = (Lp, Lp);

/// A specified `clip`: `None` for `auto`, else the four edges of
/// `rect()` (top, right, bottom, left), `None` for `auto`.
pub(crate) type SpecifiedClip = Option<[Option<Lp>; 4]>;

/// `transform`: `none | <transform-function>+`.
/// <https://www.w3.org/TR/css-transforms-1/#transform-property>
pub(crate) fn parse_transform(p: &mut Parser<'_>) -> ParseResult<SpecifiedTransform> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(Arc::from([]));
    }
    let mut functions = Vec::new();
    while !p.is_exhausted() {
        functions.push(p.try_parse(parse_transform_function)?);
    }
    if functions.is_empty() {
        return Err(ParseError::Unexpected);
    }
    Ok(Arc::from(functions))
}

/// One transform function. A function without a 2D effect
/// (`translateZ()`, `scaleZ()`) is the identity: the box still counts as
/// transformed (a stacking context and a containing block), as in
/// browsers (`translateZ(0)` is a common way to get both).
fn parse_transform_function(p: &mut Parser<'_>) -> ParseResult<SpecifiedTransformFunction> {
    use SpecifiedTransformFunction as F;
    let (name, mut args) = p.expect_function()?;
    let name = name.to_ascii_lowercase();
    args.parse_entirely(|a| {
        Ok(match name.as_str() {
            "matrix" => {
                let mut m = [0.0; 6];
                for (i, v) in m.iter_mut().enumerate() {
                    if i > 0 {
                        a.expect_comma()?;
                    }
                    *v = parse_number(a)?;
                }
                F::Matrix(m)
            }
            "translate" => {
                let x = translation(a)?;
                let y = if a.expect_comma().is_ok() {
                    translation(a)?
                } else {
                    Lp::ZERO
                };
                F::Translate(x, y)
            }
            "translatex" => F::Translate(translation(a)?, Lp::ZERO),
            "translatey" => F::Translate(Lp::ZERO, translation(a)?),
            "translate3d" => {
                let x = translation(a)?;
                a.expect_comma()?;
                let y = translation(a)?;
                a.expect_comma()?;
                parse_length_percentage(a, LengthOptions::LENGTH)?;
                F::Translate(x, y)
            }
            "translatez" | "scalez" => {
                if name == "translatez" {
                    parse_length_percentage(a, LengthOptions::LENGTH)?;
                } else {
                    scale_factor(a)?;
                }
                F::Translate(Lp::ZERO, Lp::ZERO)
            }
            "scale" => {
                let x = scale_factor(a)?;
                let y = if a.expect_comma().is_ok() {
                    scale_factor(a)?
                } else {
                    x
                };
                F::Scale(x, y)
            }
            "scalex" => F::Scale(scale_factor(a)?, 1.0),
            "scaley" => F::Scale(1.0, scale_factor(a)?),
            "scale3d" => {
                let x = scale_factor(a)?;
                a.expect_comma()?;
                let y = scale_factor(a)?;
                a.expect_comma()?;
                scale_factor(a)?;
                F::Scale(x, y)
            }
            "rotate" | "rotatez" => F::Rotate(parse_angle(a, true)?),
            "skew" => {
                let x = parse_angle(a, true)?;
                let y = if a.expect_comma().is_ok() {
                    parse_angle(a, true)?
                } else {
                    0.0
                };
                F::Skew(x, y)
            }
            "skewx" => F::Skew(parse_angle(a, true)?, 0.0),
            "skewy" => F::Skew(0.0, parse_angle(a, true)?),
            _ => return Err(ParseError::Unexpected),
        })
    })
}

/// A `<length-percentage>` argument of `translate()`.
fn translation(p: &mut Parser<'_>) -> ParseResult<Lp> {
    parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE)
}

/// A `<number> | <percentage>` argument of `scale()` (CSS Transforms 2).
fn scale_factor(p: &mut Parser<'_>) -> ParseResult<f32> {
    if let Ok(v) = p.expect_percentage() {
        return Ok(v / 100.0);
    }
    parse_number(p)
}

/// `transform-origin`: one to three values; the third (z) is a length and
/// is ignored. <https://www.w3.org/TR/css-transforms-1/#transform-origin-property>
pub(crate) fn parse_transform_origin(p: &mut Parser<'_>) -> ParseResult<SpecifiedTransformOrigin> {
    let first = origin_item(p)?;
    let second = p.try_parse(origin_item).ok();
    if second.is_some() {
        // The z offset.
        let _ = p.try_parse(|p| parse_length_percentage(p, LengthOptions::LENGTH));
    }
    let center = || Lp::Percentage(0.5);
    let Some(second) = second else {
        return Ok(match first {
            OriginItem::Vertical(y) => (center(), y),
            OriginItem::Horizontal(x) | OriginItem::Length(x) => (x, center()),
            OriginItem::Center => (center(), center()),
        });
    };
    // Two keywords may come in either order (`top left`); a length may
    // not.
    let keywords =
        !matches!(first, OriginItem::Length(_)) && !matches!(second, OriginItem::Length(_));
    let swapped = keywords
        && (matches!(first, OriginItem::Vertical(_))
            || matches!(second, OriginItem::Horizontal(_)));
    let (x, y) = if swapped {
        (second, first)
    } else {
        (first, second)
    };
    let x = match x {
        OriginItem::Horizontal(v) | OriginItem::Length(v) => v,
        OriginItem::Center => center(),
        OriginItem::Vertical(_) => return Err(ParseError::Invalid),
    };
    let y = match y {
        OriginItem::Vertical(v) | OriginItem::Length(v) => v,
        OriginItem::Center => center(),
        OriginItem::Horizontal(_) => return Err(ParseError::Invalid),
    };
    Ok((x, y))
}

/// One value of `transform-origin`.
enum OriginItem {
    /// `left` or `right`.
    Horizontal(Lp),
    /// `top` or `bottom`.
    Vertical(Lp),
    /// `center`.
    Center,
    /// A `<length-percentage>`.
    Length(Lp),
}

fn origin_item(p: &mut Parser<'_>) -> ParseResult<OriginItem> {
    if let Ok(item) = p.expect_one_of(&[
        ("left", 0),
        ("right", 1),
        ("top", 2),
        ("bottom", 3),
        ("center", 4),
    ]) {
        return Ok(match item {
            0 => OriginItem::Horizontal(Lp::Percentage(0.0)),
            1 => OriginItem::Horizontal(Lp::Percentage(1.0)),
            2 => OriginItem::Vertical(Lp::Percentage(0.0)),
            3 => OriginItem::Vertical(Lp::Percentage(1.0)),
            _ => OriginItem::Center,
        });
    }
    parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE).map(OriginItem::Length)
}

/// `clip`: `auto | rect(<top>, <right>, <bottom>, <left>)`, each edge a
/// `<length>` or `auto`. The legacy form without commas is accepted, as
/// in browsers.
pub(crate) fn parse_clip(p: &mut Parser<'_>) -> ParseResult<SpecifiedClip> {
    if p.expect_ident_matching("auto").is_ok() {
        return Ok(None);
    }
    let mut args = p.expect_function_matching("rect")?;
    args.parse_entirely(|a| {
        let edge = |a: &mut Parser<'_>| -> ParseResult<Option<Lp>> {
            if a.expect_ident_matching("auto").is_ok() {
                return Ok(None);
            }
            parse_length_percentage(a, LengthOptions::LENGTH).map(Some)
        };
        let top = edge(a)?;
        let commas = a.expect_comma().is_ok();
        let mut rest = [None, None, None];
        for (i, slot) in rest.iter_mut().enumerate() {
            if commas && i > 0 {
                a.expect_comma()?;
            }
            *slot = edge(a)?;
        }
        let [right, bottom, left] = rest;
        Ok(Some([top, right, bottom, left]))
    })
}

/// Computes `transform`.
pub(crate) fn compute_transform(
    value: &SpecifiedTransform,
    cx: &ComputeContext<'_>,
) -> Arc<[TransformFunction]> {
    use SpecifiedTransformFunction as S;
    value
        .iter()
        .map(|f| match f {
            S::Matrix(m) => TransformFunction::Matrix(*m),
            S::Translate(x, y) => TransformFunction::Translate(cx.lp(x), cx.lp(y)),
            S::Scale(x, y) => TransformFunction::Scale(*x, *y),
            S::Rotate(a) => TransformFunction::Rotate(*a),
            S::Skew(x, y) => TransformFunction::Skew(*x, *y),
        })
        .collect()
}

/// Computes `transform-origin`.
pub(crate) fn compute_transform_origin(
    value: &SpecifiedTransformOrigin,
    cx: &ComputeContext<'_>,
) -> TransformOrigin {
    TransformOrigin {
        x: cx.lp(&value.0),
        y: cx.lp(&value.1),
    }
}

/// Computes `clip`.
pub(crate) fn compute_clip(value: &SpecifiedClip, cx: &ComputeContext<'_>) -> Option<ClipRect> {
    let [top, right, bottom, left] = value.as_ref()?;
    let px = |v: &Option<Lp>| v.as_ref().map(|lp| cx.px(lp));
    Some(ClipRect {
        top: px(top),
        right: px(right),
        bottom: px(bottom),
        left: px(left),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::test_util::parse_all as parse_with;

    fn transform(css: &str) -> Option<Vec<SpecifiedTransformFunction>> {
        parse_with(css, parse_transform).ok().map(|t| t.to_vec())
    }

    #[test]
    fn transform_functions() {
        use SpecifiedTransformFunction as F;
        let px = |v: f32| Lp::Length(crate::values::Length::px(v));
        assert_eq!(transform("none"), Some(vec![]));
        assert_eq!(
            transform("translateY(-50%) scale(1.1)"),
            Some(vec![
                F::Translate(Lp::ZERO, Lp::Percentage(-0.5)),
                F::Scale(1.1, 1.1)
            ])
        );
        assert_eq!(
            transform("translate(10px) rotate(90deg) skewX(0)"),
            Some(vec![
                F::Translate(px(10.0), Lp::ZERO),
                F::Rotate(90.0),
                F::Skew(0.0, 0.0)
            ])
        );
        assert_eq!(
            transform("matrix(1, 0, 0, 1, 5, 6)"),
            Some(vec![F::Matrix([1.0, 0.0, 0.0, 1.0, 5.0, 6.0])])
        );
        assert_eq!(
            transform("scale(50%, 2) translateZ(5px) rotateZ(0.5turn)"),
            Some(vec![
                F::Scale(0.5, 2.0),
                F::Translate(Lp::ZERO, Lp::ZERO),
                F::Rotate(180.0)
            ])
        );
        assert_eq!(transform("rotateX(10deg)"), None);
        assert_eq!(transform("perspective(10px)"), None);
        assert_eq!(transform("matrix(1, 0, 0, 1, 5)"), None);
        assert_eq!(transform("translate(1px 2px)"), None);
        assert_eq!(transform("rotate(10px)"), None);
        assert_eq!(transform("none scale(2)"), None);
    }

    #[test]
    fn transform_origin_values() {
        let origin = |css: &str| parse_with(css, parse_transform_origin).ok();
        let px = |v: f32| Lp::Length(crate::values::Length::px(v));
        let pct = Lp::Percentage;
        assert_eq!(origin("left"), Some((pct(0.0), pct(0.5))));
        assert_eq!(origin("bottom"), Some((pct(0.5), pct(1.0))));
        assert_eq!(origin("top left"), Some((pct(0.0), pct(0.0))));
        assert_eq!(origin("10px 20%"), Some((px(10.0), pct(0.2))));
        assert_eq!(origin("right 5px 3px"), Some((pct(1.0), px(5.0))));
        assert_eq!(origin("center top"), Some((pct(0.5), pct(0.0))));
        assert_eq!(origin("top 10px"), None);
        assert_eq!(origin("left right"), None);
    }

    #[test]
    fn clip_rectangles() {
        let clip = |css: &str| parse_with(css, parse_clip).ok();
        let px = |v: f32| Some(Lp::Length(crate::values::Length::px(v)));
        assert_eq!(clip("auto"), Some(None));
        assert_eq!(
            clip("rect(1px, 2px, auto, 4px)"),
            Some(Some([px(1.0), px(2.0), None, px(4.0)]))
        );
        assert_eq!(
            clip("rect(0 0 0 0)"),
            Some(Some([px(0.0), px(0.0), px(0.0), px(0.0)]))
        );
        assert_eq!(clip("rect(1px, 2px 3px, 4px)"), None);
        assert_eq!(clip("rect(10%, 0, 0, 0)"), None);
    }
}
