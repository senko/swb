//! The An+B microsyntax, used by `:nth-child()` and related pseudo-classes.
//!
//! <https://www.w3.org/TR/css-syntax-3/#anb-microsyntax>
//!
//! The tokenizer splits An+B text in surprising ways (`2n+1` is a dimension
//! `2n` and a number `+1`; `2n-1` is one dimension with unit `n-1`; `-n-1`
//! is one identifier), so the parser works on tokens as the specification
//! describes.

use crate::cursor::{ParseError, Parser};
use crate::values::ComponentValue;

/// An `An+B` value: it matches the indices `A*n + B` for integers `n >= 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnB {
    /// The step.
    pub a: i32,
    /// The offset.
    pub b: i32,
}

impl AnB {
    /// True if a 1-based `index` is `A*n + B` for some integer `n >= 0`.
    pub fn matches(self, index: i32) -> bool {
        let (a, b, index) = (i64::from(self.a), i64::from(self.b), i64::from(index));
        if a == 0 {
            return index == b;
        }
        let diff = index - b;
        diff % a == 0 && diff / a >= 0
    }

    /// Parses An+B from the cursor. Trailing input (such as `of S`) is left
    /// unconsumed.
    pub fn parse(p: &mut Parser<'_>) -> Result<AnB, ParseError> {
        p.try_parse(parse_anb)
    }
}

fn parse_anb(p: &mut Parser<'_>) -> Result<AnB, ParseError> {
    match p.next().ok_or(ParseError::EndOfInput)? {
        ComponentValue::Ident(s) if s.eq_ignore_ascii_case("odd") => Ok(AnB { a: 2, b: 1 }),
        ComponentValue::Ident(s) if s.eq_ignore_ascii_case("even") => Ok(AnB { a: 2, b: 0 }),
        ComponentValue::Ident(s) => {
            let (a, rest) = if let Some(rest) = strip_prefix_ignore_case(s, "-n") {
                (-1, rest)
            } else if let Some(rest) = strip_prefix_ignore_case(s, "n") {
                (1, rest)
            } else {
                return Err(ParseError::Invalid);
            };
            parse_after_n(a, rest, p)
        }
        ComponentValue::Number(n) => n
            .int_value
            .map(|b| AnB { a: 0, b })
            .ok_or(ParseError::Invalid),
        ComponentValue::Dimension { number, unit } => {
            let a = number.int_value.ok_or(ParseError::Invalid)?;
            let rest = strip_prefix_ignore_case(unit, "n").ok_or(ParseError::Invalid)?;
            parse_after_n(a, rest, p)
        }
        // `'+'? n...`: no whitespace between `+` and the identifier.
        ComponentValue::Delim('+') => match p.next_including_whitespace() {
            Some(ComponentValue::Ident(s)) => {
                let rest = strip_prefix_ignore_case(s, "n").ok_or(ParseError::Invalid)?;
                parse_after_n(1, rest, p)
            }
            _ => Err(ParseError::Invalid),
        },
        _ => Err(ParseError::Unexpected),
    }
}

/// Parses what follows the `n`: `rest` is the part of the token after `n`.
fn parse_after_n(a: i32, rest: &str, p: &mut Parser<'_>) -> Result<AnB, ParseError> {
    if rest.is_empty() {
        let b = parse_optional_b(p)?;
        return Ok(AnB { a, b });
    }
    if rest == "-" {
        // `n- 1`: a signless integer must follow.
        let b = parse_signless_integer(p)?;
        return Ok(AnB {
            a,
            b: b.saturating_neg(),
        });
    }
    match rest.strip_prefix('-') {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            let b = digits.parse::<i64>().unwrap_or(i64::MAX);
            let b = (-b).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            Ok(AnB { a, b })
        }
        _ => Err(ParseError::Invalid),
    }
}

/// Parses an optional `B` after `An`: a signed integer, or `+`/`-` and a
/// signless integer. Returns 0 if neither follows.
fn parse_optional_b(p: &mut Parser<'_>) -> Result<i32, ParseError> {
    let saved = *p;
    match p.next() {
        Some(ComponentValue::Number(n)) if n.has_sign => n.int_value.ok_or(ParseError::Invalid),
        Some(ComponentValue::Delim('+')) => parse_signless_integer(p),
        Some(ComponentValue::Delim('-')) => Ok(parse_signless_integer(p)?.saturating_neg()),
        _ => {
            *p = saved;
            Ok(0)
        }
    }
}

fn parse_signless_integer(p: &mut Parser<'_>) -> Result<i32, ParseError> {
    match p.next() {
        Some(ComponentValue::Number(n)) if !n.has_sign => n.int_value.ok_or(ParseError::Invalid),
        Some(_) => Err(ParseError::Unexpected),
        None => Err(ParseError::EndOfInput),
    }
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_component_values;

    fn parse(css: &str) -> Option<(i32, i32)> {
        let values = parse_component_values(css);
        let mut p = Parser::new(&values);
        let anb = AnB::parse(&mut p).ok()?;
        p.is_exhausted().then_some((anb.a, anb.b))
    }

    #[test]
    fn valid_forms() {
        let cases: &[(&str, (i32, i32))] = &[
            ("odd", (2, 1)),
            ("EVEN", (2, 0)),
            ("5", (0, 5)),
            ("+5", (0, 5)),
            ("-5", (0, -5)),
            ("n", (1, 0)),
            ("N", (1, 0)),
            ("+n", (1, 0)),
            ("-n", (-1, 0)),
            ("2n", (2, 0)),
            ("2n+1", (2, 1)),
            ("2n-1", (2, -1)),
            ("2N-1", (2, -1)),
            ("2n + 1", (2, 1)),
            ("2n - 1", (2, -1)),
            ("2n+ 1", (2, 1)),
            ("2n -1", (2, -1)),
            ("2n- 1", (2, -1)),
            ("-n+3", (-1, 3)),
            ("-n-3", (-1, -3)),
            ("-n- 3", (-1, -3)),
            ("n- 1", (1, -1)),
            ("+n-1", (1, -1)),
            ("+n+1", (1, 1)),
            ("-2n+3", (-2, 3)),
            ("0n+0", (0, 0)),
            (" 3n + 2 ", (3, 2)),
            ("10n-007", (10, -7)),
        ];
        for (css, expected) in cases {
            assert_eq!(parse(css), Some(*expected), "{css}");
        }
    }

    #[test]
    fn invalid_forms() {
        let cases = [
            "", "x", "1.5", "2.0n", "2n+1.5", "+ n", "- n", "+-n", "2n + +1", "2n 1", "n-",
            "n- +1", "n-a", "--n", "2m", "odd 1", "n+", "3n- -1", "2n- 1.0",
        ];
        for css in cases {
            assert_eq!(parse(css), None, "{css}");
        }
    }

    #[test]
    fn matching() {
        let anb = |a, b| AnB { a, b };
        let indices = |x: AnB| (1..=10).filter(|&i| x.matches(i)).collect::<Vec<_>>();
        assert_eq!(indices(anb(2, 1)), vec![1, 3, 5, 7, 9]);
        assert_eq!(indices(anb(2, 0)), vec![2, 4, 6, 8, 10]);
        assert_eq!(indices(anb(0, 3)), vec![3]);
        assert_eq!(indices(anb(-1, 3)), vec![1, 2, 3]);
        assert_eq!(indices(anb(3, -1)), vec![2, 5, 8]);
        assert_eq!(indices(anb(-2, 5)), vec![1, 3, 5]);
        assert_eq!(indices(anb(1, 0)), (1..=10).collect::<Vec<_>>());
        assert!(anb(i32::MIN, i32::MAX).matches(i32::MAX));
    }
}
