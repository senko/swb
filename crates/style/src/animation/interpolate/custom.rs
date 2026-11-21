//! The interpolation of registered custom properties by their syntax (CSS
//! Properties and Values API 1 §2.4,
//! <https://www.w3.org/TR/css-properties-values-api-1/#animation-behavior-of-custom-properties>;
//! ADR 0044 section 4).
//!
//! The computed values are token lists (`registered.rs` computes them):
//! each side is matched against the terms of the syntax; when both match
//! the same term of an interpolable type, the tokens are parsed as typed
//! values, interpolated with the functions of this module's parent, and
//! written back as `registered.rs` writes them (`50px`, `2.5`, `180deg`,
//! `rgb(191, 0, 64)`, `rotate(45deg)`). A `+` or `#` list interpolates item
//! by item when both lists have the same length. Everything else is
//! discrete: `*`, keywords, `<custom-ident>`, `<string>`, `<url>`,
//! `<image>`, `<transform-function>`, lists of different lengths, two
//! values of different terms, and colors with `color-mix()` (it may hold
//! `currentcolor`). `currentcolor` interpolates with the element's color.
//! A `<transform-list>` is written with its function names
//! ([`transform_list`]).

#[cfg(test)]
mod tests;
mod transform_list;

use std::sync::Arc;

use swb_css::{ComponentValue, Parser, parse_component_values, split_on_commas, trim_whitespace};

use super::{color, lerp, lp};
use crate::ComputedStyle;
use crate::cssom::format_number;
use crate::cssom::resolved::length_percentage_text;
use crate::parse::color::parse_color;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::parse::{ParseResult, parse_angle, parse_integer, parse_number};
use crate::properties::compute::ComputeContext;
use crate::properties::transform::{compute_transform, parse_transform};
use crate::registered::{Component, DataType, Multiplier, Syntax, space_items};
use crate::values::{Color, LengthContext, LengthPercentage, Rgba, TransformFunction};

/// The value between the computed values `from` and `to` of a registered
/// custom property with `syntax` at progress `p`; `None` if the pair is
/// discrete.
pub(super) fn interpolate(
    syntax: &Syntax,
    from: &[ComponentValue],
    to: &[ComponentValue],
    current: Rgba,
    p: f64,
) -> Option<Arc<[ComponentValue]>> {
    let Syntax::Terms(terms) = syntax else {
        return None;
    };
    let (from, to) = (trim_whitespace(from), trim_whitespace(to));
    let matches = |tokens: &[ComponentValue]| {
        terms
            .iter()
            .position(|t| t.compute(tokens, &LengthContext::DEFAULT).is_some())
    };
    let index = matches(from)?;
    if matches(to)? != index {
        return None;
    }
    let term = terms.get(index)?;
    let Component::Type(ty) = term.component else {
        return None;
    };
    let text = match term.multiplier {
        Multiplier::One => item(ty, from, to, current, p)?,
        Multiplier::Space => items(ty, &space_items(from), &space_items(to), current, p, " ")?,
        Multiplier::Comma => {
            let split = |t| split_on_commas(t).map(trim_whitespace).collect::<Vec<_>>();
            items(ty, &split(from), &split(to), current, p, ", ")?
        }
    };
    Some(Arc::from(parse_component_values(&text)))
}

/// Two lists of items of type `ty`, item by item, joined by `separator`.
fn items(
    ty: DataType,
    a: &[&[ComponentValue]],
    b: &[&[ComponentValue]],
    current: Rgba,
    p: f64,
    separator: &str,
) -> Option<String> {
    if a.len() != b.len() {
        return None;
    }
    let texts: Option<Vec<String>> = a
        .iter()
        .zip(b)
        .map(|(x, y)| item(ty, x, y, current, p))
        .collect();
    Some(texts?.join(separator))
}

/// Parses all of `tokens` with `f`.
fn parse<T>(
    tokens: &[ComponentValue],
    f: impl FnOnce(&mut Parser<'_>) -> ParseResult<T>,
) -> Option<T> {
    Parser::new(tokens).parse_entirely(f).ok()
}

/// A number as `registered.rs` writes it.
fn number(v: f32) -> String {
    format_number(f64::from(v))
}

/// One item of type `ty`; `None` if the type is discrete or a side does
/// not parse.
fn item(
    ty: DataType,
    a: &[ComponentValue],
    b: &[ComponentValue],
    current: Rgba,
    p: f64,
) -> Option<String> {
    let pair = |f: &dyn Fn(&[ComponentValue]) -> Option<f32>| Some((f(a)?, f(b)?));
    Some(match ty {
        DataType::Length | DataType::Percentage | DataType::LengthPercentage => {
            let opts = if ty == DataType::Length {
                LengthOptions::LENGTH
            } else {
                LengthOptions::LENGTH_PERCENTAGE
            };
            let value = |t: &[ComponentValue]| {
                parse(t, |p| parse_length_percentage(p, opts))
                    .map(|v| v.compute(&LengthContext::DEFAULT))
            };
            let v = lp(&value(a)?, &value(b)?, p, false);
            match v {
                LengthPercentage::Px(px) => format!("{}px", number(px)),
                other => length_percentage_text(&other),
            }
        }
        DataType::Number => {
            let (x, y) = pair(&|t| parse(t, parse_number))?;
            number(lerp(x, y, p))
        }
        DataType::Integer => {
            let (x, y) = (parse(a, parse_integer)?, parse(b, parse_integer)?);
            round_half_away(f64::from(x) + (f64::from(y) - f64::from(x)) * p).to_string()
        }
        DataType::Angle => {
            let (x, y) = pair(&|t| parse(t, |p| parse_angle(p, false)))?;
            format!("{}deg", number(lerp(x, y, p)))
        }
        DataType::Time => {
            let (x, y) = pair(&|t| dimension(t, "s"))?;
            format!("{}s", number(lerp(x, y, p)))
        }
        DataType::Resolution => {
            let (x, y) = pair(&|t| dimension(t, "dppx"))?;
            format!("{}dppx", number(lerp(x, y, p).max(0.0)))
        }
        DataType::Color => {
            let value = |t: &[ComponentValue]| parse(t, parse_color);
            let (x, y) = (value(a)?, value(b)?);
            if is_mix(x) || is_mix(y) {
                return None;
            }
            color::mix(x, y, current, p).computed_text()
        }
        DataType::TransformList => transform_list::interpolate(a, b, p)?,
        DataType::TransformFunction
        | DataType::CustomIdent
        | DataType::String
        | DataType::Url
        | DataType::Image => return None,
    })
}

/// An interpolated `<integer>`: rounded half away from zero (−2.5 is −3,
/// as Chromium 148 rounds a registered `<integer>`; the `order` and
/// `z-index` properties round half up, `round_integer` in the parent).
fn round_half_away(v: f64) -> i32 {
    v.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// True for a `color-mix()` value (it may hold `currentcolor`).
fn is_mix(c: Color) -> bool {
    matches!(c, Color::Special(id) if matches!(id.get(), crate::values::color_mix::Special::Mix(_)))
}

/// The number of a computed dimension in `unit` (`2s`, `1dppx`).
fn dimension(tokens: &[ComponentValue], unit: &str) -> Option<f32> {
    match tokens {
        [ComponentValue::Dimension { number, unit: u }] if u.eq_ignore_ascii_case(unit) => {
            Some(number.value)
        }
        _ => None,
    }
}

/// A computed `<transform-list>` (its lengths are px already).
fn transform_list_value(tokens: &[ComponentValue]) -> Option<Arc<[TransformFunction]>> {
    let specified = parse(tokens, parse_transform)?;
    let initial = ComputedStyle::initial();
    let cx = ComputeContext {
        parent: &initial,
        lengths: LengthContext::DEFAULT,
        element: None,
        quirks: false,
    };
    Some(compute_transform(&specified, &cx))
}
