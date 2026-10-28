//! The serialization of specified values as Chromium writes them for
//! `element.style` (CSSOM §6.7.2 "serialize a CSS value",
//! <https://drafts.csswg.org/cssom/#serialize-a-css-value>): from the
//! component values of a declaration that the property's parser accepted.
//!
//! The rules were measured in Chromium 148
//! (`tools/probes/web-cssom-style.json`):
//!
//! - Whitespace collapses to one space; `,` is followed by one space and
//!   `/` has a space on each side.
//! - Keywords are lower case; names that a page chooses (font families,
//!   counter and grid line names, `attr()` arguments) keep their case.
//! - Numbers have at most six significant digits, in the form of C's `%g`
//!   (`1.23457e+08`, `1e-07`); units are lower case.
//! - A `0` where the property takes a length is `0px`; in an angle, `0deg`.
//!   In quirks mode a unitless length of the properties that allow it gets
//!   `px`.
//! - Colors given as `#hex` or by the functions `rgb()`, `rgba()`, `hsl()`,
//!   `hsla()` and `hwb()` become `rgb(r, g, b)` or `rgba(r, g, b, a)`;
//!   names and keywords stay names. Other color functions keep their form.
//! - Strings use double quotes; `url()` contains a quoted string.
//! - `calc()` sums whose terms have one unit are computed (`calc(15px)`);
//!   other sums keep their terms, sorted (numbers, percentages, then
//!   dimensions by unit), as CSS Values 4 §10.12 "simplification".
//!   `min()` and `max()` of one unit are computed into `calc()`.
//!
//! Deliberate departures: numbers are the `f32` values of swb's tokenizer
//! (Chromium keeps doubles, so values past six digits of precision near
//! `f32` limits may print differently); `lab()` and the other color
//! functions of CSS Color 4 keep the units as written (Chromium converts
//! `50%` to `50` in `lab()`).

use std::fmt::Write as _;

use swb_css::{BlockKind, ComponentValue, Function, Number, Parser, serialize_identifier};

use crate::parse::color::parse_color;
use crate::values::{Color, Rgba};

/// What a bare `0` (and in quirks mode any unitless number) is in a
/// context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zero {
    /// A number: stays as written.
    Number,
    /// A length: `0px`.
    Length,
    /// An angle: `0deg`.
    Angle,
}

/// How to write the values of one property.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Style {
    /// What a bare number is at the top level.
    pub(crate) zero: Zero,
    /// True if identifiers keep their case (names that pages choose).
    pub(crate) keep_case: bool,
    /// True if every number is a length (`px`): at the top level of a
    /// length property, where a number other than `0` passed the parser
    /// only as a quirks mode unitless length.
    pub(crate) quirky_lengths: bool,
}

impl Style {
    /// Keywords and numbers.
    pub(crate) const PLAIN: Style = Style {
        zero: Zero::Number,
        keep_case: false,
        quirky_lengths: false,
    };
    /// Lengths and keywords. A number at the top level is a length: `0`,
    /// or in quirks mode a unitless length.
    pub(crate) const LENGTH: Style = Style {
        zero: Zero::Length,
        keep_case: false,
        quirky_lengths: true,
    };
    /// Values of several kinds kept as written (shorthands that swb keeps
    /// whole): a bare `0` is a length.
    pub(crate) const MIXED: Style = Style {
        zero: Zero::Length,
        keep_case: false,
        quirky_lengths: false,
    };
    /// Names that keep their case.
    pub(crate) const NAMES: Style = Style {
        zero: Zero::Number,
        keep_case: true,
        quirky_lengths: false,
    };
}

/// Serializes `values` in `style`.
pub(crate) fn serialize(values: &[ComponentValue], style: Style) -> String {
    let mut out = String::new();
    write_list(values, style, &mut out);
    out
}

/// Writes a list of component values: whitespace collapsed, separators
/// spaced.
pub(crate) fn write_list(values: &[ComponentValue], style: Style, out: &mut String) {
    let mut pending_space = false;
    for value in values {
        match value {
            ComponentValue::Whitespace => pending_space = true,
            ComponentValue::Comma => {
                out.push_str(", ");
                pending_space = false;
            }
            ComponentValue::Delim('/') => {
                trim_end_space(out);
                out.push_str(" / ");
                pending_space = false;
            }
            _ => {
                if pending_space && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                pending_space = false;
                write_value(value, style, out);
            }
        }
    }
    trim_end_space(out);
}

fn trim_end_space(out: &mut String) {
    while out.ends_with(' ') {
        out.pop();
    }
}

/// Writes one component value.
fn write_value(value: &ComponentValue, style: Style, out: &mut String) {
    match value {
        ComponentValue::Ident(name) => {
            if style.keep_case {
                serialize_identifier(name, out);
            } else {
                serialize_identifier(&name.to_ascii_lowercase(), out);
            }
        }
        ComponentValue::Number(n) => write_number_in(n, style, out),
        ComponentValue::Percentage(n) => {
            out.push_str(&format_number(f64::from(n.value)));
            out.push('%');
        }
        ComponentValue::Dimension { number, unit } => {
            out.push_str(&format_number(f64::from(number.value)));
            out.push_str(&unit.to_ascii_lowercase());
        }
        ComponentValue::String(s) => swb_css::serialize_string(s, out),
        ComponentValue::Url(url) => write_url(url, out),
        ComponentValue::Hash { .. } => write_color_value(value, out),
        ComponentValue::Function(f) => write_function(f, style, out),
        ComponentValue::Block(block) => {
            out.push(block.kind.open());
            let inner = if block.kind == BlockKind::Square {
                // Grid line names keep their case.
                Style {
                    keep_case: true,
                    ..style
                }
            } else {
                style
            };
            write_list(&block.contents, inner, out);
            out.push(block.kind.close());
        }
        ComponentValue::Delim(c) => out.push(*c),
        other => out.push_str(&swb_css::serialize_component_values(std::slice::from_ref(
            other,
        ))),
    }
}

/// Writes a number token in a context.
fn write_number_in(n: &Number, style: Style, out: &mut String) {
    let text = format_number(f64::from(n.value));
    out.push_str(&text);
    match style.zero {
        Zero::Length if n.value == 0.0 || style.quirky_lengths => out.push_str("px"),
        Zero::Angle if n.value == 0.0 => out.push_str("deg"),
        _ => {}
    }
}

/// `url("...")`.
fn write_url(url: &str, out: &mut String) {
    out.push_str("url(");
    swb_css::serialize_string(url, out);
    out.push(')');
}

/// The canonical names of the functions whose names have upper case
/// letters (transform functions).
const CAMEL_CASE_FUNCTIONS: &[&str] = &[
    "translateX",
    "translateY",
    "translateZ",
    "scaleX",
    "scaleY",
    "scaleZ",
    "rotateX",
    "rotateY",
    "rotateZ",
    "skewX",
    "skewY",
];

/// Writes a function.
fn write_function(f: &Function, style: Style, out: &mut String) {
    let lower = f.name.to_ascii_lowercase();
    match lower.as_str() {
        "rgb" | "rgba" | "hsl" | "hsla" | "hwb" => {
            write_color_value(&ComponentValue::Function(f.clone()), out);
            return;
        }
        "url" => {
            if let [ComponentValue::String(s)] = trimmed(&f.arguments) {
                write_url(s, out);
                return;
            }
        }
        "calc" | "min" | "max" | "clamp" | "-webkit-calc" => {
            write_math(f, &lower, style, out);
            return;
        }
        _ => {}
    }
    let name = CAMEL_CASE_FUNCTIONS
        .iter()
        .find(|c| c.eq_ignore_ascii_case(&lower))
        .copied()
        .unwrap_or(&lower);
    let inner = Style {
        zero: function_zero(&lower, style.zero),
        keep_case: style.keep_case || matches!(lower.as_str(), "attr" | "counter" | "counters"),
        quirky_lengths: false,
    };
    out.push_str(name);
    out.push('(');
    if matches!(lower.as_str(), "counter" | "counters") {
        write_counter_arguments(&f.arguments, out);
    } else if lower == "rect" {
        write_rect_arguments(&f.arguments, out);
    } else {
        write_list(&f.arguments, inner, out);
    }
    out.push(')');
}

/// What a bare `0` is among the arguments of the function `name`.
fn function_zero(name: &str, outer: Zero) -> Zero {
    match name {
        "translate"
        | "translatex"
        | "translatey"
        | "translatez"
        | "translate3d"
        | "perspective"
        | "inset"
        | "circle"
        | "ellipse"
        | "polygon"
        | "linear-gradient"
        | "radial-gradient"
        | "conic-gradient"
        | "repeating-linear-gradient"
        | "repeating-radial-gradient"
        | "repeating-conic-gradient"
        | "minmax"
        | "fit-content"
        | "repeat"
        | "rect" => Zero::Length,
        "rotate" | "rotatex" | "rotatey" | "rotatez" | "skew" | "skewx" | "skewy" => Zero::Angle,
        "matrix" | "matrix3d" | "scale" | "scalex" | "scaley" | "scalez" | "scale3d"
        | "rotate3d" | "cubic-bezier" | "steps" => Zero::Number,
        _ => outer,
    }
}

/// `counter(name, style)` and `counters(name, "sep", style)`: the name
/// keeps its case, the style is a keyword.
fn write_counter_arguments(args: &[ComponentValue], out: &mut String) {
    let parts = split_commas(args);
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let style = if i == 0 { Style::NAMES } else { Style::PLAIN };
        write_list(part, style, out);
    }
}

/// `rect(a b c d)` and `rect(a, b, c, d)`: written with commas.
fn write_rect_arguments(args: &[ComponentValue], out: &mut String) {
    let mut first = true;
    for value in args {
        if matches!(value, ComponentValue::Whitespace | ComponentValue::Comma) {
            continue;
        }
        if !first {
            out.push_str(", ");
        }
        first = false;
        write_value(value, Style::LENGTH, out);
    }
}

/// Splits values at top-level commas.
pub(crate) fn split_commas(values: &[ComponentValue]) -> Vec<&[ComponentValue]> {
    values
        .split(|v| matches!(v, ComponentValue::Comma))
        .map(trimmed)
        .collect()
}

/// The values without leading and trailing whitespace.
pub(crate) fn trimmed(values: &[ComponentValue]) -> &[ComponentValue] {
    let start = values
        .iter()
        .position(|v| !matches!(v, ComponentValue::Whitespace))
        .unwrap_or(values.len());
    let end = values
        .iter()
        .rposition(|v| !matches!(v, ComponentValue::Whitespace))
        .map_or(start, |i| i + 1);
    values.get(start..end).unwrap_or(&[])
}

/// Writes a color value (a hash or a color function): as `rgb()` or
/// `rgba()` if swb parses it, else as written.
fn write_color_value(value: &ComponentValue, out: &mut String) {
    let values = std::slice::from_ref(value);
    let mut p = Parser::new(values);
    match parse_color(&mut p) {
        Ok(Color::Rgba(c)) => write_rgba(c, out),
        _ => out.push_str(&swb_css::serialize_component_values(values)),
    }
}

/// `rgb(r, g, b)` or `rgba(r, g, b, a)`.
pub(crate) fn write_rgba(c: Rgba, out: &mut String) {
    if c.a == 255 {
        let _ = write!(out, "rgb({}, {}, {})", c.r, c.g, c.b);
    } else {
        let _ = write!(
            out,
            "rgba({}, {}, {}, {})",
            c.r,
            c.g,
            c.b,
            format_alpha(c.a)
        );
    }
}

/// The alpha of an 8-bit channel as Chromium writes it: two decimals if
/// they give the same channel back, else three.
pub(crate) fn format_alpha(a: u8) -> String {
    let fraction = f64::from(a) / 255.0;
    let two = (fraction * 100.0).round() / 100.0;
    let value = if (two * 255.0).round() == f64::from(a) {
        two
    } else {
        (fraction * 1000.0).round() / 1000.0
    };
    format_number(value)
}

/// A number as C's `printf("%g")` writes it with six significant digits
/// (Chromium's form).
pub(crate) fn format_number(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return if value.is_finite() {
            "0".to_owned()
        } else {
            format!("{value}")
        };
    }
    // Rounded to six significant digits first: the exponent is that of the
    // rounded value (999999.5 is 1e+06).
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-4..6).contains(&exponent) {
        let mantissa = trim_zeros(mantissa);
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exponent.abs());
    }
    let decimals = usize::try_from(5 - exponent).unwrap_or(0);
    trim_zeros(&format!("{value:.decimals$}")).to_owned()
}

/// Removes trailing zeros of a decimal fraction (and its point).
fn trim_zeros(text: &str) -> &str {
    if !text.contains('.') {
        return text;
    }
    text.trim_end_matches('0').trim_end_matches('.')
}

// ----- calc() -----

/// A sum of terms: the value of each unit ("" for numbers, "%" for
/// percentages), in the order of first appearance.
#[derive(Clone, Debug, Default)]
struct Sum {
    terms: Vec<(String, f64)>,
}

impl Sum {
    fn single(unit: &str, value: f64) -> Sum {
        Sum {
            terms: vec![(unit.to_owned(), value)],
        }
    }

    fn add(mut self, other: Sum, sign: f64) -> Sum {
        for (unit, value) in other.terms {
            match self.terms.iter_mut().find(|(u, _)| *u == unit) {
                Some(term) => term.1 += sign * value,
                None => self.terms.push((unit, sign * value)),
            }
        }
        self
    }

    /// The number if the sum is a plain number.
    fn number(&self) -> Option<f64> {
        match self.terms.as_slice() {
            [(unit, value)] if unit.is_empty() => Some(*value),
            [] => Some(0.0),
            _ => None,
        }
    }

    fn scale(mut self, factor: f64) -> Sum {
        for term in &mut self.terms {
            term.1 *= factor;
        }
        self
    }

    /// The terms in the order of CSS Values 4 §10.12: the number, the
    /// percentage, then the dimensions by unit.
    fn sorted(&self) -> Vec<(String, f64)> {
        let mut terms = self.terms.clone();
        terms.sort_by_key(|t| rank(&t.0));
        terms
    }

    fn write(&self, out: &mut String) {
        for (i, (unit, value)) in self.sorted().iter().enumerate() {
            if i == 0 {
                out.push_str(&format_number(*value));
            } else if *value < 0.0 {
                out.push_str(" - ");
                out.push_str(&format_number(-value));
            } else {
                out.push_str(" + ");
                out.push_str(&format_number(*value));
            }
            out.push_str(unit);
        }
    }
}

fn rank(unit: &str) -> (u8, String) {
    match unit {
        "" => (0, String::new()),
        "%" => (1, String::new()),
        other => (2, other.to_owned()),
    }
}

/// Writes `calc()`, `min()`, `max()` or `clamp()`: computed where the
/// terms allow it, else as written with the spacing normalized.
fn write_math(f: &Function, lower: &str, style: Style, out: &mut String) {
    let simplified = match lower {
        "calc" | "-webkit-calc" => parse_sum(trimmed(&f.arguments)),
        "min" | "max" => {
            let sums: Option<Vec<Sum>> = split_commas(&f.arguments)
                .into_iter()
                .map(parse_sum)
                .collect();
            sums.and_then(|sums| min_max(&sums, lower == "max"))
        }
        _ => None,
    };
    if let Some(sum) = simplified {
        out.push_str("calc(");
        if sum.terms.is_empty() {
            out.push('0');
        } else {
            sum.write(out);
        }
        out.push(')');
        return;
    }
    let name = if lower == "-webkit-calc" {
        "calc"
    } else {
        lower
    };
    out.push_str(name);
    out.push('(');
    let parts = split_commas(&f.arguments);
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        match parse_sum(part) {
            Some(sum) if !sum.terms.is_empty() => sum.write(out),
            _ => write_list(part, style, out),
        }
    }
    out.push(')');
}

/// The value of `min()` or `max()` of sums of one and the same unit.
fn min_max(sums: &[Sum], max: bool) -> Option<Sum> {
    let mut unit: Option<&str> = None;
    let mut best: Option<f64> = None;
    for sum in sums {
        let [(u, v)] = sum.terms.as_slice() else {
            return None;
        };
        if unit.is_some_and(|x| x != u) {
            return None;
        }
        unit = Some(u);
        best = Some(match best {
            Some(b) if max => b.max(*v),
            Some(b) => b.min(*v),
            None => *v,
        });
    }
    Some(Sum::single(unit?, best?))
}

/// Parses a sum of products of numbers, percentages, dimensions and
/// parenthesized sums. `None` for anything else (`var()`, other
/// functions, a product of two dimensions).
fn parse_sum(values: &[ComponentValue]) -> Option<Sum> {
    let tokens: Vec<&ComponentValue> = values
        .iter()
        .filter(|v| !matches!(v, ComponentValue::Whitespace))
        .collect();
    let mut pos = 0;
    let sum = sum_at(&tokens, &mut pos)?;
    (pos == tokens.len()).then_some(sum)
}

fn sum_at(tokens: &[&ComponentValue], pos: &mut usize) -> Option<Sum> {
    let mut sum = product_at(tokens, pos)?;
    loop {
        let sign = match tokens.get(*pos) {
            Some(ComponentValue::Delim('+')) => 1.0,
            Some(ComponentValue::Delim('-')) => -1.0,
            _ => return Some(sum),
        };
        *pos += 1;
        let next = product_at(tokens, pos)?;
        sum = sum.add(next, sign);
    }
}

fn product_at(tokens: &[&ComponentValue], pos: &mut usize) -> Option<Sum> {
    let mut value = factor_at(tokens, pos)?;
    loop {
        let divide = match tokens.get(*pos) {
            Some(ComponentValue::Delim('*')) => false,
            Some(ComponentValue::Delim('/')) => true,
            _ => return Some(value),
        };
        *pos += 1;
        let right = factor_at(tokens, pos)?;
        value = match (value.number(), right.number()) {
            (_, Some(n)) if divide => {
                if n == 0.0 {
                    return None;
                }
                value.scale(1.0 / n)
            }
            (_, Some(n)) => value.scale(n),
            (Some(n), None) if !divide => right.scale(n),
            _ => return None,
        };
    }
}

fn factor_at(tokens: &[&ComponentValue], pos: &mut usize) -> Option<Sum> {
    let token = tokens.get(*pos)?;
    *pos += 1;
    match token {
        ComponentValue::Number(n) => Some(Sum::single("", f64::from(n.value))),
        ComponentValue::Percentage(n) => Some(Sum::single("%", f64::from(n.value))),
        ComponentValue::Dimension { number, unit } => Some(Sum::single(
            &unit.to_ascii_lowercase(),
            f64::from(number.value),
        )),
        ComponentValue::Block(block) if block.kind == BlockKind::Paren => {
            parse_sum(&block.contents)
        }
        ComponentValue::Function(f) if f.name.eq_ignore_ascii_case("calc") => {
            parse_sum(&f.arguments)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ser(css: &str, style: Style) -> String {
        serialize(&swb_css::parse_component_values(css), style)
    }

    #[test]
    fn numbers_have_six_significant_digits() {
        let cases = [
            (1.234_567_89, "1.23457"),
            (0.0001, "0.0001"),
            (1e-7, "1e-07"),
            (123_456_789.0, "1.23457e+08"),
            (1_234_567.5, "1.23457e+06"),
            (33.333_333, "33.3333"),
            (1e21, "1e+21"),
            (0.000_001, "1e-06"),
            (-0.5, "-0.5"),
            (100.0, "100"),
            (999_999.5, "1e+06"),
            (2_147_483_647.0, "2.14748e+09"),
        ];
        for (value, text) in cases {
            assert_eq!(format_number(value), text, "{value}");
        }
    }

    #[test]
    fn alpha_has_two_or_three_decimals() {
        assert_eq!(format_alpha(136), "0.533");
        assert_eq!(format_alpha(128), "0.5");
        assert_eq!(format_alpha(31), "0.12");
        assert_eq!(format_alpha(77), "0.3");
        assert_eq!(format_alpha(0), "0");
    }

    #[test]
    fn values_are_normalized() {
        assert_eq!(ser("10.0PX", Style::LENGTH), "10px");
        assert_eq!(ser("0", Style::LENGTH), "0px");
        assert_eq!(ser("calc( 10px + 5px )", Style::LENGTH), "calc(15px)");
        assert_eq!(ser("calc(100% - 10px)", Style::LENGTH), "calc(100% - 10px)");
        assert_eq!(ser("calc(100%/3)", Style::LENGTH), "calc(33.3333%)");
        assert_eq!(ser("calc(10px*2)", Style::LENGTH), "calc(20px)");
        assert_eq!(ser("max(1px,2px)", Style::LENGTH), "calc(2px)");
        assert_eq!(ser("min(10px, 5%)", Style::LENGTH), "min(10px, 5%)");
        assert_eq!(ser("#f00", Style::PLAIN), "rgb(255, 0, 0)");
        assert_eq!(ser("rgba(0,0,0,.5)", Style::PLAIN), "rgba(0, 0, 0, 0.5)");
        assert_eq!(
            ser("TRANSLATE3D(10px,20px,0)", Style::PLAIN),
            "translate3d(10px, 20px, 0px)"
        );
        assert_eq!(ser("rotate(0)", Style::PLAIN), "rotate(0deg)");
        assert_eq!(ser("translatex(1PX)", Style::PLAIN), "translateX(1px)");
        assert_eq!(ser("url(a.png)", Style::PLAIN), "url(\"a.png\")");
        assert_eq!(ser("url( 'a b' )", Style::PLAIN), "url(\"a b\")");
        assert_eq!(ser("'x'  /  \"y\"", Style::PLAIN), "\"x\" / \"y\"");
        assert_eq!(
            ser("counters(Item, '.')", Style::PLAIN),
            "counters(Item, \".\")"
        );
        assert_eq!(
            ser("rect(1px 2px 3px 4px)", Style::PLAIN),
            "rect(1px, 2px, 3px, 4px)"
        );
        assert_eq!(ser("[A] 1fr", Style::LENGTH), "[A] 1fr");
    }
}
