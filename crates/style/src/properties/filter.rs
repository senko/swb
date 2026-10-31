//! Parsing and computing `filter` (Filter Effects 1 §5,
//! <https://drafts.fxtf.org/filter-effects-1/#FilterProperty>): `none`, or
//! a list of `url()` references and the filter functions `blur()`,
//! `brightness()`, `contrast()`, `drop-shadow()`, `grayscale()`,
//! `hue-rotate()`, `invert()`, `opacity()`, `saturate()` and `sepia()`.
//!
//! The grammar follows Chromium 148 (measured, `tools/probes/filter.json`):
//! every function argument is optional (`blur()` is `blur(0px)`,
//! `invert()` is `invert(1)`), an amount is a non-negative number or
//! percentage (also in `calc()`), a `blur()` length cannot be negative or a
//! percentage, `hue-rotate()` takes an angle or a unitless zero,
//! `drop-shadow()` takes a color first or last and two or three lengths
//! (the third cannot be negative), and the functions are separated by
//! white space only. An unclosed function is closed at the end of the
//! value. `grayscale()`, `invert()`, `opacity()` and `sepia()` are
//! limited to 1 when they are parsed.
//!
//! Departure: an angle in `calc()` (`hue-rotate(calc(1deg + 2deg))`) is
//! invalid, because swb's `calc()` has no angles.

use std::sync::Arc;

use swb_css::{ParseError, Parser};

use super::compute::ComputeContext;
use crate::parse::color::parse_color;
use crate::parse::length::{LengthOptions, parse_length_percentage, parse_math_function};
use crate::parse::{ParseResult, ParserContext, parse_angle};
use crate::values::{
    Color, DropShadow, Filter, FilterFunction, MAX_FILTER_BLUR, MAX_FILTER_FUNCTIONS,
    SpecifiedLengthPercentage as Lp,
};

/// A specified `filter`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedFilter {
    /// `none`.
    None,
    /// One or more functions.
    List(Arc<[SpecifiedFunction]>),
}

/// The functions that take one amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Amount {
    Brightness,
    Contrast,
    Grayscale,
    Invert,
    Opacity,
    Saturate,
    Sepia,
}

impl Amount {
    fn from_name(name: &str) -> Option<Amount> {
        Some(match name {
            "brightness" => Amount::Brightness,
            "contrast" => Amount::Contrast,
            "grayscale" => Amount::Grayscale,
            "invert" => Amount::Invert,
            "opacity" => Amount::Opacity,
            "saturate" => Amount::Saturate,
            "sepia" => Amount::Sepia,
            _ => return None,
        })
    }

    /// True if the amount is limited to 1.
    fn is_limited(self) -> bool {
        matches!(
            self,
            Amount::Grayscale | Amount::Invert | Amount::Opacity | Amount::Sepia
        )
    }

    fn function(self, value: f32) -> FilterFunction {
        match self {
            Amount::Brightness => FilterFunction::Brightness(value),
            Amount::Contrast => FilterFunction::Contrast(value),
            Amount::Grayscale => FilterFunction::Grayscale(value),
            Amount::Invert => FilterFunction::Invert(value),
            Amount::Opacity => FilterFunction::Opacity(value),
            Amount::Saturate => FilterFunction::Saturate(value),
            Amount::Sepia => FilterFunction::Sepia(value),
        }
    }
}

/// One specified filter function.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedFunction {
    Blur(Lp),
    Amount(Amount, f32),
    HueRotate(f32),
    DropShadow {
        color: Option<Color>,
        x: Lp,
        y: Lp,
        blur: Option<Lp>,
    },
    Url(Arc<str>),
}

/// `filter`: `none | [<filter-function> | <url>]+`.
pub(crate) fn parse_filter(p: &mut Parser<'_>, cx: &ParserContext) -> ParseResult<SpecifiedFilter> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(SpecifiedFilter::None);
    }
    let mut list = Vec::new();
    while !p.is_exhausted() {
        list.push(parse_function(p, cx)?);
        if list.len() > MAX_FILTER_FUNCTIONS {
            return Err(ParseError::Invalid);
        }
    }
    if list.is_empty() {
        return Err(ParseError::EndOfInput);
    }
    Ok(SpecifiedFilter::List(Arc::from(list)))
}

/// A reference as the computed value keeps it: a fragment stays as
/// written, another URL is made absolute.
fn resolve_reference(url: &str, cx: &ParserContext) -> Arc<str> {
    let url = url.trim();
    if url.is_empty() || url.starts_with('#') {
        Arc::from(url)
    } else {
        cx.resolve_url(url)
    }
}

fn parse_function(p: &mut Parser<'_>, cx: &ParserContext) -> ParseResult<SpecifiedFunction> {
    if let Ok(url) = p.try_parse(Parser::expect_url) {
        return Ok(SpecifiedFunction::Url(resolve_reference(url, cx)));
    }
    let (name, mut args) = p.expect_function()?;
    let a = &mut args;
    let name = name.to_ascii_lowercase();
    let function = match name.as_str() {
        "blur" => SpecifiedFunction::Blur(if a.is_exhausted() {
            Lp::ZERO
        } else {
            parse_length_percentage(a, LengthOptions::NON_NEGATIVE_LENGTH)?
        }),
        "hue-rotate" => SpecifiedFunction::HueRotate(if a.is_exhausted() {
            0.0
        } else {
            parse_angle(a, true)?
        }),
        "drop-shadow" => parse_drop_shadow(a)?,
        other => {
            let kind = Amount::from_name(other).ok_or(ParseError::Unexpected)?;
            let value = if a.is_exhausted() {
                1.0
            } else {
                parse_amount(a)?
            };
            SpecifiedFunction::Amount(
                kind,
                if kind.is_limited() {
                    value.min(1.0)
                } else {
                    value
                },
            )
        }
    };
    a.expect_exhausted()?;
    Ok(function)
}

/// `<number [0,∞]> | <percentage [0,∞]>`, also as `calc()`; a percentage
/// is a fraction.
fn parse_amount(p: &mut Parser<'_>) -> ParseResult<f32> {
    p.try_parse(|p| {
        if let Ok(v) = p.expect_percentage() {
            return if v < 0.0 {
                Err(ParseError::Invalid)
            } else {
                Ok(v / 100.0)
            };
        }
        if let Ok(v) = p.expect_number() {
            return if v < 0.0 {
                Err(ParseError::Invalid)
            } else {
                Ok(v)
            };
        }
        let value = parse_math_function(p)?
            .as_number_or_percentage()
            .ok_or(ParseError::Invalid)?;
        Ok(value.max(0.0))
    })
}

/// `drop-shadow( [<color>? && <length>{2,3}] )`.
fn parse_drop_shadow(p: &mut Parser<'_>) -> ParseResult<SpecifiedFunction> {
    let mut color = parse_color(p).ok();
    let x = parse_length_percentage(p, LengthOptions::LENGTH)?;
    let y = parse_length_percentage(p, LengthOptions::LENGTH)?;
    let blur = parse_length_percentage(p, LengthOptions::NON_NEGATIVE_LENGTH).ok();
    if color.is_none() {
        color = parse_color(p).ok();
    }
    Ok(SpecifiedFunction::DropShadow { color, x, y, blur })
}

// ----- Computing -----

impl SpecifiedFilter {
    /// Computes the value.
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> Filter {
        match self {
            SpecifiedFilter::None => Filter::NONE,
            SpecifiedFilter::List(list) => {
                Filter::new(list.iter().map(|f| f.compute(cx)).collect())
            }
        }
    }
}

/// A length in px for a filter: not negative, at most [`MAX_FILTER_BLUR`].
fn length(cx: &ComputeContext<'_>, value: &Lp) -> f32 {
    let px = cx.px(value);
    if px.is_nan() {
        0.0
    } else {
        px.clamp(0.0, MAX_FILTER_BLUR)
    }
}

impl SpecifiedFunction {
    fn compute(&self, cx: &ComputeContext<'_>) -> FilterFunction {
        match self {
            SpecifiedFunction::Blur(blur) => FilterFunction::Blur(length(cx, blur)),
            SpecifiedFunction::Amount(kind, value) => kind.function(*value),
            SpecifiedFunction::HueRotate(degrees) => FilterFunction::HueRotate(*degrees),
            SpecifiedFunction::DropShadow { color, x, y, blur } => {
                FilterFunction::DropShadow(DropShadow {
                    color: color.unwrap_or(Color::CurrentColor),
                    x: cx.px(x),
                    y: cx.px(y),
                    blur: blur.as_ref().map_or(0.0, |b| length(cx, b)),
                })
            }
            SpecifiedFunction::Url(url) => FilterFunction::Url(Arc::clone(url)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComputedStyle;
    use crate::parse::test_util::parse_all;
    use crate::values::LengthContext;

    fn parse(css: &str) -> Option<SpecifiedFilter> {
        parse_all(css, |p| parse_filter(p, &ParserContext::user_agent())).ok()
    }

    fn computed(css: &str) -> Option<Filter> {
        let parent = ComputedStyle::initial();
        let cx = ComputeContext {
            parent: &parent,
            lengths: LengthContext::DEFAULT,
            element: None,
            quirks: false,
        };
        parse(css).map(|f| f.compute(&cx))
    }

    #[test]
    fn grammar_of_chromium() {
        for css in [
            "none",
            "blur()",
            "blur(0)",
            "blur(5px)",
            "blur(calc(1px + 2px))",
            "brightness()",
            "brightness(50%)",
            "brightness(calc(10% * 2))",
            "contrast(2.5)",
            "hue-rotate()",
            "hue-rotate(0)",
            "hue-rotate(0.25turn)",
            "invert(1) hue-rotate(180deg)",
            "invert(1)brightness(55%)contrast(250%)hue-rotate(180deg)",
            "drop-shadow(1px 2px)",
            "drop-shadow(red 1px 2px 3px)",
            "drop-shadow(1px 2px 3px red)",
            "url(#a)",
            "url(#a) blur(2px)",
            "BLUR(2px)",
            "blur(",
            "blur(1px",
        ] {
            assert!(parse(css).is_some(), "{css}");
        }
        for css in [
            "",
            "auto",
            "none none",
            "none blur(1px)",
            "blur(5)",
            "blur(-5px)",
            "blur(10%)",
            "blur(1px 2px)",
            "blur(2deg)",
            "blur(1px), blur(2px)",
            "brightness(-1)",
            "brightness(1px)",
            "brightness(1 2)",
            "brightness(calc(50% + 0.5))",
            "hue-rotate(90)",
            "hue-rotate(10%)",
            "hue-rotate(1px)",
            "drop-shadow()",
            "drop-shadow(1px)",
            "drop-shadow(1px 2px 3px 4px)",
            "drop-shadow(1px red 2px)",
            "drop-shadow(1px 2px -3px)",
            "drop-shadow(10% 10px)",
            "drop-shadow(inset 1px 1px)",
            "bogus(1)",
            "blur(1px) bogus(1)",
            "red",
            "3",
        ] {
            assert!(parse(css).is_none(), "{css}");
        }
    }

    #[test]
    fn computed_values_of_chromium() {
        use FilterFunction as F;
        let f = |css| computed(css).expect("valid").functions().to_vec();
        assert_eq!(f("blur()"), [F::Blur(0.0)]);
        assert_eq!(f("blur(1em)"), [F::Blur(16.0)]);
        assert_eq!(f("blur(calc(-1px))"), [F::Blur(0.0)]);
        assert_eq!(f("blur(1e400px)"), [F::Blur(MAX_FILTER_BLUR)]);
        assert_eq!(f("brightness()"), [F::Brightness(1.0)]);
        assert_eq!(f("brightness(50%)"), [F::Brightness(0.5)]);
        assert_eq!(f("brightness(200%)"), [F::Brightness(2.0)]);
        assert_eq!(f("brightness(100)"), [F::Brightness(100.0)]);
        assert_eq!(f("grayscale(2)"), [F::Grayscale(1.0)]);
        assert_eq!(f("grayscale(200%)"), [F::Grayscale(1.0)]);
        assert_eq!(f("invert()"), [F::Invert(1.0)]);
        assert_eq!(f("opacity(2)"), [F::Opacity(1.0)]);
        assert_eq!(f("sepia(0.5)"), [F::Sepia(0.5)]);
        assert_eq!(f("saturate(30)"), [F::Saturate(30.0)]);
        assert_eq!(f("hue-rotate(0.25turn)"), [F::HueRotate(90.0)]);
        assert_eq!(f("hue-rotate()"), [F::HueRotate(0.0)]);
        assert_eq!(
            f("drop-shadow(1px 2px)"),
            [F::DropShadow(DropShadow {
                color: Color::CurrentColor,
                x: 1.0,
                y: 2.0,
                blur: 0.0,
            })]
        );
        assert_eq!(
            f("drop-shadow(1px 2px 3px red)"),
            [F::DropShadow(DropShadow {
                color: Color::Rgba(crate::values::Rgba::rgb(255, 0, 0)),
                x: 1.0,
                y: 2.0,
                blur: 3.0,
            })]
        );
        assert_eq!(f("url(#a)"), [F::Url(Arc::from("#a"))]);
        assert!(computed("none").expect("valid").is_none());
    }

    #[test]
    fn the_number_of_functions_is_limited() {
        let ok = "invert(0) ".repeat(MAX_FILTER_FUNCTIONS);
        assert!(parse(&ok).is_some());
        let too_many = "invert(0) ".repeat(MAX_FILTER_FUNCTIONS + 1);
        assert!(parse(&too_many).is_none());
    }
}
