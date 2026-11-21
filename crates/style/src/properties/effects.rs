//! Parsing and computing the properties of M13 feature 2a: `box-shadow`
//! and `text-shadow` (CSS Backgrounds 3 §7, CSS Text Decoration 3 §4),
//! `translate`, `rotate` and `scale` (CSS Transforms 2 §3,
//! <https://www.w3.org/TR/css-transforms-2/#individual-transforms>), the
//! `corner-*-shape` longhands (CSS Borders 4 §3,
//! <https://drafts.csswg.org/css-borders-4/#corner-shaping>) and
//! `-webkit-text-stroke-width` (Compatibility Standard §3,
//! <https://compat.spec.whatwg.org/#the-webkit-text-stroke-width>).
//!
//! Grammar and computed values follow Chromium 148 (measured,
//! `tools/probes/css-videojs-2a.json`).

use std::sync::Arc;

use swb_css::{ParseError, Parser};

use super::compute::ComputeContext;
use crate::parse::color::parse_color;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::parse::{ParseResult, parse_angle, parse_number};
use crate::values::{
    BoxShadow, Color, CornerShape, Rotate, Scale, SpecifiedLengthPercentage as Lp, TextShadow,
    Translate,
};

/// One specified shadow of `box-shadow` or `text-shadow`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpecifiedShadow {
    color: Option<Color>,
    x: Lp,
    y: Lp,
    blur: Option<Lp>,
    spread: Option<Lp>,
    inset: bool,
}

/// A specified shadow list; empty for `none`.
pub(crate) type SpecifiedShadows = Arc<[SpecifiedShadow]>;

/// `none | <shadow>#`, with `<shadow>` as `box-shadow` (`text`: false) or
/// `text-shadow` (`text`: true; no `inset` and no spread) has it.
pub(crate) fn parse_shadows(p: &mut Parser<'_>, text: bool) -> ParseResult<SpecifiedShadows> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::from([]));
    }
    let list = p.parse_comma_separated(|p| parse_shadow(p, text))?;
    Ok(Arc::from(list))
}

/// `<color>? && [<length>{2,4}] && inset?` (CSS Backgrounds 3): the three
/// parts in any order, the lengths in one run.
fn parse_shadow(p: &mut Parser<'_>, text: bool) -> ParseResult<SpecifiedShadow> {
    let mut color = None;
    let mut inset = false;
    let mut lengths: Option<Vec<Lp>> = None;
    while !p.is_exhausted() {
        if color.is_none()
            && let Ok(c) = parse_color(p)
        {
            color = Some(c);
        } else if !text && !inset && p.try_parse(|p| p.expect_ident_matching("inset")).is_ok() {
            inset = true;
        } else if lengths.is_none() {
            let mut run = Vec::new();
            let max = if text { 3 } else { 4 };
            while run.len() < max {
                match parse_length_percentage(p, LengthOptions::LENGTH) {
                    Ok(v) => run.push(v),
                    Err(_) => break,
                }
            }
            if run.len() < 2 {
                return Err(ParseError::Invalid);
            }
            lengths = Some(run);
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    let mut lengths = lengths.ok_or(ParseError::Invalid)?.into_iter();
    let (x, y) = (
        lengths.next().ok_or(ParseError::Invalid)?,
        lengths.next().ok_or(ParseError::Invalid)?,
    );
    let (blur, spread) = (lengths.next(), lengths.next());
    if blur.as_ref().is_some_and(is_negative) {
        return Err(ParseError::Invalid);
    }
    Ok(SpecifiedShadow {
        color,
        x,
        y,
        blur,
        spread,
        inset,
    })
}

/// True for a length that is a negative number of px or em without
/// `calc()` (a negative blur is invalid; a `calc()` clamps at compute
/// time).
fn is_negative(lp: &Lp) -> bool {
    matches!(lp, Lp::Length(l) if l.value < 0.0)
}

/// The largest blur radius and spread in px that a computed shadow keeps:
/// beyond it a shadow is a flat area to the raster anyway, and the sum of
/// the reaches cannot overflow `f32`.
pub(crate) const MAX_SHADOW_LENGTH: f32 = 1.0e6;

fn clamp_length(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(-MAX_SHADOW_LENGTH, MAX_SHADOW_LENGTH)
    }
}

impl SpecifiedShadow {
    /// The computed `box-shadow`.
    fn compute_box(&self, cx: &ComputeContext<'_>) -> BoxShadow {
        BoxShadow {
            color: self.color.unwrap_or(Color::CurrentColor),
            x: clamp_length(cx.px(&self.x)),
            y: clamp_length(cx.px(&self.y)),
            blur: clamp_length(self.blur.as_ref().map_or(0.0, |b| cx.px(b))).max(0.0),
            spread: clamp_length(self.spread.as_ref().map_or(0.0, |b| cx.px(b))),
            inset: self.inset,
        }
    }

    /// The computed `text-shadow`.
    fn compute_text(&self, cx: &ComputeContext<'_>) -> TextShadow {
        let b = self.compute_box(cx);
        TextShadow {
            color: b.color,
            x: b.x,
            y: b.y,
            blur: b.blur,
        }
    }
}

/// Computes a `box-shadow` list.
pub(crate) fn compute_box_shadows(
    v: &SpecifiedShadows,
    cx: &ComputeContext<'_>,
) -> Arc<[BoxShadow]> {
    v.iter().map(|s| s.compute_box(cx)).collect()
}

/// Computes a `text-shadow` list.
pub(crate) fn compute_text_shadows(
    v: &SpecifiedShadows,
    cx: &ComputeContext<'_>,
) -> Arc<[TextShadow]> {
    v.iter().map(|s| s.compute_text(cx)).collect()
}

// ----- translate, rotate, scale -----

/// A specified `translate`: `None` for `none`; the z offset is a length.
pub(crate) type SpecifiedTranslate = Option<(Lp, Lp, Lp)>;

/// `none | <length-percentage> [<length-percentage> <length>?]?`.
pub(crate) fn parse_translate(p: &mut Parser<'_>) -> ParseResult<SpecifiedTranslate> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    let x = parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE)?;
    let y = p
        .try_parse(|p| parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE))
        .unwrap_or(Lp::ZERO);
    let z = p
        .try_parse(|p| parse_length_percentage(p, LengthOptions::LENGTH))
        .unwrap_or(Lp::ZERO);
    Ok(Some((x, y, z)))
}

/// Computes `translate`.
pub(crate) fn compute_translate(
    v: &SpecifiedTranslate,
    cx: &ComputeContext<'_>,
) -> Option<Translate> {
    v.as_ref().map(|(x, y, z)| Translate {
        x: cx.lp(x),
        y: cx.lp(y),
        z: cx.px(z),
    })
}

/// `none | <angle> | [x | y | z | <number>{3}] && <angle>`. An axis of
/// zero length is invalid in Chromium (the declaration is dropped).
pub(crate) fn parse_rotate(p: &mut Parser<'_>) -> ParseResult<Option<Rotate>> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    let mut axis = None;
    let mut angle = None;
    while !p.is_exhausted() {
        // The axis first: `0 0 1 90deg` starts with a number that is also
        // a valid angle (`0`).
        if axis.is_none()
            && let Ok(a) = parse_axis(p)
        {
            axis = Some(a);
        } else if angle.is_none()
            && let Ok(a) = parse_angle(p, false)
        {
            angle = Some(a);
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    let angle = angle.ok_or(ParseError::Invalid)?;
    Ok(Some(Rotate {
        axis: axis.unwrap_or([0.0, 0.0, 1.0]),
        angle,
    }))
}

/// `x | y | z | <number>{3}`.
fn parse_axis(p: &mut Parser<'_>) -> ParseResult<[f32; 3]> {
    p.try_parse(|p| {
        if let Ok(ident) = p.expect_ident() {
            return match ident.to_ascii_lowercase().as_str() {
                "x" => Ok([1.0, 0.0, 0.0]),
                "y" => Ok([0.0, 1.0, 0.0]),
                "z" => Ok([0.0, 0.0, 1.0]),
                _ => Err(ParseError::Unexpected),
            };
        }
        let axis = [parse_number(p)?, parse_number(p)?, parse_number(p)?];
        Ok(axis)
    })
}

/// `none | [<number> | <percentage>]{1,3}`.
pub(crate) fn parse_scale(p: &mut Parser<'_>) -> ParseResult<Option<Scale>> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    let factor = |p: &mut Parser<'_>| -> ParseResult<f32> {
        p.try_parse(|p| {
            if let Ok(v) = p.expect_percentage() {
                return Ok(v / 100.0);
            }
            parse_number(p)
        })
    };
    let x = factor(p)?;
    let y = factor(p).unwrap_or(x);
    let z = factor(p).unwrap_or(1.0);
    Ok(Some(Scale { x, y, z }))
}

// ----- corner-shape -----

/// `<corner-shape-value>`: `round | scoop | bevel | notch | square |
/// squircle | superellipse(<number>)`.
pub(crate) fn parse_corner_shape(p: &mut Parser<'_>) -> ParseResult<CornerShape> {
    p.try_parse(|p| {
        if let Ok(ident) = p.expect_ident() {
            return CornerShape::from_ident(ident).ok_or(ParseError::Unexpected);
        }
        let (name, mut args) = p.expect_function()?;
        if !name.eq_ignore_ascii_case("superellipse") {
            return Err(ParseError::Unexpected);
        }
        let k = args.parse_entirely(|a| {
            if a.try_parse(|a| a.expect_ident_matching("infinity")).is_ok() {
                return Ok(f32::INFINITY);
            }
            if a.try_parse(|a| a.expect_ident_matching("-infinity"))
                .is_ok()
            {
                return Ok(f32::NEG_INFINITY);
            }
            parse_number(a)
        })?;
        Ok(CornerShape(k))
    })
}

/// `-webkit-text-stroke-width`: `<length> | thin | medium | thick`.
pub(crate) fn parse_stroke_width(p: &mut Parser<'_>) -> ParseResult<Lp> {
    p.try_parse(|p| {
        if let Ok(ident) = p.expect_ident() {
            return match ident.to_ascii_lowercase().as_str() {
                // Chromium scales the border widths of the keywords by a
                // third (measured: `thin` computes to 0.333333px).
                "thin" => Ok(Lp::Length(crate::values::Length::px(1.0 / 3.0))),
                "medium" => Ok(Lp::Length(crate::values::Length::px(1.0))),
                "thick" => Ok(Lp::Length(crate::values::Length::px(5.0 / 3.0))),
                _ => Err(ParseError::Unexpected),
            };
        }
        parse_length_percentage(p, LengthOptions::NON_NEGATIVE_LENGTH)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::test_util::parse_all;

    #[test]
    fn shadow_grammar() {
        let ok = |css: &str, text| parse_all(css, |p| parse_shadows(p, text)).is_ok();
        for css in [
            "none",
            "0 1px 3px 0 #0000001a, 0 1px 2px -1px #0000001a",
            "inset 0 2px 4px red",
            "1px 2px",
            "red 1px 2px 3px 4px",
            "1px 2px 3px 4px red inset",
            "0 0 #0000",
        ] {
            assert!(ok(css, false), "{css}");
        }
        for css in [
            "",
            "1px",
            "1px 2px 3px 4px 5px",
            "1px 2px -3px",
            "red",
            "none, 1px 1px",
            "1px 2px red red",
            "1px red 2px",
            "inset inset 1px 1px",
        ] {
            assert!(!ok(css, false), "{css}");
        }
        assert!(ok("1px 2px 3px red", true));
        assert!(!ok("1px 2px 3px 4px", true));
        assert!(!ok("inset 1px 2px", true));
    }

    #[test]
    fn individual_transform_grammar() {
        let t = |css: &str| parse_all(css, parse_translate).is_ok();
        assert!(t("none") && t("10px") && t("10px 20px") && t("10% 20px 30px"));
        assert!(!t("10px 20px 30%") && !t("") && !t("1"));
        let r = |css: &str| parse_all(css, parse_rotate).ok().flatten();
        assert_eq!(r("45deg").map(|r| r.axis), Some([0.0, 0.0, 1.0]));
        assert_eq!(r("x 90deg").map(|r| r.axis), Some([1.0, 0.0, 0.0]));
        assert_eq!(r("90deg y").map(|r| r.axis), Some([0.0, 1.0, 0.0]));
        assert_eq!(r("1 2 3 10deg").map(|r| r.axis), Some([1.0, 2.0, 3.0]));
        assert!(r("x").is_none() && r("x y 1deg").is_none());
        let s = |css: &str| parse_all(css, parse_scale).ok().flatten();
        assert_eq!(s("2").map(|s| (s.x, s.y, s.z)), Some((2.0, 2.0, 1.0)));
        assert_eq!(s("2 3").map(|s| (s.x, s.y, s.z)), Some((2.0, 3.0, 1.0)));
        assert_eq!(s("50% 2 3").map(|s| (s.x, s.y, s.z)), Some((0.5, 2.0, 3.0)));
        assert!(s("1px").is_none() && s("1 2 3 4").is_none());
    }

    #[test]
    fn corner_shapes() {
        let c = |css: &str| parse_all(css, parse_corner_shape).ok();
        assert_eq!(c("squircle"), Some(CornerShape(2.0)));
        assert_eq!(c("superellipse(3)"), Some(CornerShape(3.0)));
        assert_eq!(
            c("superellipse(-infinity)"),
            Some(CornerShape(f32::NEG_INFINITY))
        );
        assert!(c("circle").is_none() && c("superellipse()").is_none());
        assert_eq!(CornerShape(0.0).keyword(), Some("bevel"));
        assert_eq!(CornerShape(3.0).keyword(), None);
    }
}
