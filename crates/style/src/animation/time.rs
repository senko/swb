//! `<time>` and `<number>` values of the animation properties, with the
//! math functions (`calc()`, `min()`, `max()`, `clamp()`) evaluated at parse
//! time by the shared evaluator of `crate::parse::length`
//! ([`parse_context_free_math`]): times and numbers have no context, so a
//! math function is a value in seconds or a plain number at once.
//!
//! <https://www.w3.org/TR/css-values-4/#time>,
//! <https://www.w3.org/TR/css-values-4/#calc-func>.
//!
//! The texts follow Chromium 148 (`tools/probes/css-animations-3a.json`,
//! cases `longhand-values` and `number-ranges`): a literal keeps its unit
//! (`300ms`, `1e3ms` is `1000ms`), a math function is `calc()` of seconds
//! (`calc(1000ms)` is `calc(1s)`, `min(1s, 2s)` is `calc(1s)`), and the
//! computed value is in seconds (`0.3s`).
//!
//! The range of numbers, as Chromium keeps it: a literal saturates to the
//! `f32` range (`1e40s` is `3.40282e+38s`, also inside `calc()`); a math
//! function whose result is infinite or NaN stays valid and keeps that in
//! its text (`calc(infinity * 1s)`, `calc(-infinity)`, `calc(NaN * 1s)`),
//! and its value is the largest finite `f64` with the sign (NaN is 0), so
//! `getComputedStyle` shows `1.79769e+308s`. Every value here is finite.

use swb_css::{ComponentValue, ParseError, Parser};

use crate::cssom::format_number;
use crate::parse::ParseResult;
use crate::parse::length::{CalcKind, parse_context_free_math};

/// A specified `<time>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Time {
    /// The value in seconds (finite).
    pub seconds: f64,
    /// How it was written.
    pub(crate) written: Written,
}

/// How a time or number was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Written {
    /// A literal in seconds (or a plain number).
    Plain,
    /// A literal in milliseconds.
    Milliseconds,
    /// A math function with a finite result.
    Math,
    /// A math function whose result is +∞ (the value is `f64::MAX`).
    Infinity,
    /// A math function whose result is −∞ (the value is `-f64::MAX`).
    NegInfinity,
    /// A math function whose result is NaN (the value is 0).
    NaN,
}

impl Written {
    /// How a math function with the result `value` is written, and its
    /// finite value.
    pub(crate) fn math(value: f64) -> (Written, f64) {
        if value.is_nan() {
            (Written::NaN, 0.0)
        } else if value == f64::INFINITY {
            (Written::Infinity, f64::MAX)
        } else if value == f64::NEG_INFINITY {
            (Written::NegInfinity, -f64::MAX)
        } else {
            (Written::Math, value)
        }
    }

    /// The text of a math function that is not finite: `calc(infinity)`
    /// or, with `unit`, `calc(infinity * 1s)`.
    fn non_finite_text(self, unit: &str) -> Option<String> {
        let word = match self {
            Written::Infinity => "infinity",
            Written::NegInfinity => "-infinity",
            Written::NaN => "NaN",
            _ => return None,
        };
        Some(if unit.is_empty() {
            format!("calc({word})")
        } else {
            format!("calc({word} * 1{unit})")
        })
    }

    fn is_math(self) -> bool {
        !matches!(self, Written::Plain | Written::Milliseconds)
    }
}

/// A literal saturated to the `f32` range, as Chromium keeps it.
pub(crate) fn saturate_literal(value: f64) -> f64 {
    let max = f64::from(f32::MAX);
    value.clamp(-max, max)
}

impl Time {
    /// A literal time in seconds.
    pub fn seconds(seconds: f64) -> Time {
        Time {
            seconds,
            written: Written::Plain,
        }
    }

    /// The text that `element.style` shows.
    pub(crate) fn declared(self) -> String {
        if let Some(text) = self.written.non_finite_text("s") {
            return text;
        }
        match self.written {
            Written::Milliseconds => format!("{}ms", format_number(self.seconds * 1000.0)),
            Written::Math => format!("calc({}s)", format_number(self.seconds)),
            _ => format!("{}s", format_number(self.seconds)),
        }
    }

    /// True if the time was written as a math function.
    pub(crate) fn is_math(self) -> bool {
        self.written.is_math()
    }
}

/// The computed text of a time in seconds (`0.3s`).
pub(crate) fn seconds_text(seconds: f64) -> String {
    format!("{}s", format_number(seconds))
}

/// A specified `<number>` that may be a math function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Number {
    /// The value (finite).
    pub value: f64,
    /// How it was written.
    pub(crate) written: Written,
}

impl Number {
    /// A literal number.
    pub fn literal(value: f64) -> Number {
        Number {
            value,
            written: Written::Plain,
        }
    }

    /// The text that `element.style` shows: the number, or `calc(n)`.
    pub(crate) fn declared(self) -> String {
        if let Some(text) = self.written.non_finite_text("") {
            return text;
        }
        if self.is_math() {
            format!("calc({})", format_number(self.value))
        } else {
            format_number(self.value)
        }
    }

    /// True if written as a math function.
    pub(crate) fn is_math(self) -> bool {
        self.written.is_math()
    }

    /// True if written as a math function whose result is NaN.
    pub(crate) fn is_nan(self) -> bool {
        self.written == Written::NaN
    }
}

/// Consumes a `<time>`: a dimension in `s` or `ms`, or a math function of
/// times.
pub(crate) fn parse_time(p: &mut Parser<'_>) -> ParseResult<Time> {
    p.try_parse(|p| match p.peek() {
        Some(ComponentValue::Dimension { number, unit }) => {
            let value = saturate_literal(number.value_f64);
            let (seconds, written) = if unit.eq_ignore_ascii_case("s") {
                (value, Written::Plain)
            } else if unit.eq_ignore_ascii_case("ms") {
                (value / 1000.0, Written::Milliseconds)
            } else {
                return Err(ParseError::Unexpected);
            };
            p.next();
            Ok(Time { seconds, written })
        }
        Some(ComponentValue::Function(_)) => {
            let (value, kind) = parse_context_free_math(p)?;
            if kind != CalcKind::Time {
                return Err(ParseError::Invalid);
            }
            let (written, seconds) = Written::math(value);
            Ok(Time { seconds, written })
        }
        _ => Err(ParseError::Unexpected),
    })
}

/// Consumes a `<number>` or a math function of numbers.
pub(crate) fn parse_number(p: &mut Parser<'_>) -> ParseResult<Number> {
    p.try_parse(|p| match p.peek() {
        Some(ComponentValue::Number(n)) => {
            let value = saturate_literal(n.value_f64);
            p.next();
            Ok(Number {
                value,
                written: Written::Plain,
            })
        }
        Some(ComponentValue::Function(_)) => {
            let (value, kind) = parse_context_free_math(p)?;
            if kind != CalcKind::Number {
                return Err(ParseError::Invalid);
            }
            let (written, value) = Written::math(value);
            Ok(Number { value, written })
        }
        _ => Err(ParseError::Unexpected),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::test_util::parse_all;

    fn time(css: &str) -> Option<(f64, String)> {
        parse_all(css, parse_time)
            .ok()
            .map(|t| (t.seconds, t.declared()))
    }

    fn check(cases: &[(&str, Option<(f64, &str)>)], parse: impl Fn(&str) -> Option<(f64, String)>) {
        for &(css, want) in cases {
            let got = parse(css);
            let want = want.map(|(s, t)| (s, t.to_owned()));
            match (&got, &want) {
                (Some((a, ta)), Some((b, tb))) => {
                    assert!(
                        (a - b).abs() <= 1e-6 * b.abs().max(1.0),
                        "{css}: {a} != {b}"
                    );
                    assert_eq!(ta, tb, "{css}");
                }
                _ => assert_eq!(got, want, "{css}"),
            }
        }
    }

    #[test]
    fn literal_and_math_times() {
        // The rows of `longhand-values` (css-animations-3a.json).
        check(
            &[
                ("1s", Some((1.0, "1s"))),
                ("300ms", Some((0.3, "300ms"))),
                ("1S", Some((1.0, "1s"))),
                (".5s", Some((0.5, "0.5s"))),
                ("1e3ms", Some((1.0, "1000ms"))),
                ("calc(1s + 500ms)", Some((1.5, "calc(1.5s)"))),
                ("calc(2s * 2)", Some((4.0, "calc(4s)"))),
                ("calc(1s - 2s)", Some((-1.0, "calc(-1s)"))),
                ("calc(1000ms)", Some((1.0, "calc(1s)"))),
                ("min(1s, 2s)", Some((1.0, "calc(1s)"))),
                ("max(1s, 2s)", Some((2.0, "calc(2s)"))),
                ("calc(10ms * 3)", Some((0.03, "calc(0.03s)"))),
                ("clamp(1s, 5s, 3s)", Some((3.0, "calc(3s)"))),
                ("0", None),
                ("1", None),
                ("1px", None),
                ("calc(1s + 1)", None),
                ("calc(1s * 1s)", None),
                ("calc(1)", None),
                ("calc(1s + 10%)", None),
                ("calc(1px)", None),
            ],
            time,
        );
    }

    /// The rows of `number-ranges` (css-animations-3a.json): literals
    /// saturate to the `f32` range, non-finite math results keep their
    /// text and have the largest finite value.
    #[test]
    fn the_range_of_times() {
        let max = f64::from(f32::MAX);
        check(
            &[
                ("1e40s", Some((max, "3.40282e+38s"))),
                ("1e999s", Some((max, "3.40282e+38s"))),
                ("1e999ms", Some((max / 1000.0, "3.40282e+38ms"))),
                ("3.4e38s", Some((3.4e38, "3.4e+38s"))),
                ("calc(1e999s)", Some((max, "calc(3.40282e+38s)"))),
                ("calc(1s*1e39)", Some((max, "calc(3.40282e+38s)"))),
                ("calc(1ms*1e30)", Some((1e27, "calc(1e+27s)"))),
                ("calc(1s/0)", Some((f64::MAX, "calc(infinity * 1s)"))),
                ("calc(-1s/0)", Some((-f64::MAX, "calc(-infinity * 1s)"))),
                ("calc(0s/0)", Some((0.0, "calc(NaN * 1s)"))),
                (
                    "calc(infinity * 1s)",
                    Some((f64::MAX, "calc(infinity * 1s)")),
                ),
                ("calc(NaN * 1s)", Some((0.0, "calc(NaN * 1s)"))),
                ("calc(1s*NaN)", Some((0.0, "calc(NaN * 1s)"))),
                ("min(1s, calc(1s/0))", Some((1.0, "calc(1s)"))),
                ("min(1s, calc(0s/0))", Some((0.0, "calc(NaN * 1s)"))),
            ],
            time,
        );
    }

    #[test]
    fn numbers() {
        let n = |css: &str| {
            parse_all(css, parse_number)
                .ok()
                .map(|n| (n.value, n.declared()))
        };
        let max = f64::from(f32::MAX);
        check(
            &[
                ("2.5", Some((2.5, "2.5"))),
                ("1e2", Some((100.0, "100"))),
                ("calc(1 + 1)", Some((2.0, "calc(2)"))),
                ("calc(-1)", Some((-1.0, "calc(-1)"))),
                ("1e999", Some((max, "3.40282e+38"))),
                ("1e40", Some((max, "3.40282e+38"))),
                ("calc(1e39)", Some((max, "calc(3.40282e+38)"))),
                ("calc(1/0)", Some((f64::MAX, "calc(infinity)"))),
                ("calc(-1/0)", Some((-f64::MAX, "calc(-infinity)"))),
                ("calc(0/0)", Some((0.0, "calc(NaN)"))),
                ("calc(infinity)", Some((f64::MAX, "calc(infinity)"))),
                ("calc(-INFINITY)", Some((-f64::MAX, "calc(-infinity)"))),
                ("calc(nan)", Some((0.0, "calc(NaN)"))),
                ("calc(pi)", Some((std::f64::consts::PI, "calc(3.14159)"))),
                ("calc(1s)", None),
                ("1s", None),
                ("calc(50%)", None),
            ],
            n,
        );
    }
}
