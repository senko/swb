//! Lengths, percentages and math functions.
//!
//! - `<length>`, `<percentage>`, `<length-percentage>`:
//!   <https://www.w3.org/TR/css-values-4/#lengths>
//! - Math functions `calc()`, `min()`, `max()`, `clamp()`:
//!   <https://www.w3.org/TR/css-values-4/#math>
//! - The unitless length quirk (quirky lengths):
//!   <https://www.w3.org/TR/css-values-4/#quirky-lengths>

use swb_css::{BlockKind, ComponentValue, ParseError, Parser};

use super::ParseResult;
use crate::values::{CalcNode, Length, LengthContext, LengthUnit, SpecifiedLengthPercentage};

/// What a length parser accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LengthOptions {
    /// Accept percentages.
    pub(crate) percentage: bool,
    /// Accept negative values.
    pub(crate) negative: bool,
    /// Accept unitless numbers as px (the quirks mode unitless length
    /// quirk).
    pub(crate) quirky: bool,
}

impl LengthOptions {
    /// `<length>`.
    pub(crate) const LENGTH: Self = LengthOptions {
        percentage: false,
        negative: true,
        quirky: false,
    };
    /// `<length [0,∞]>`.
    pub(crate) const NON_NEGATIVE_LENGTH: Self = LengthOptions {
        percentage: false,
        negative: false,
        quirky: false,
    };
    /// `<length-percentage>`.
    pub(crate) const LENGTH_PERCENTAGE: Self = LengthOptions {
        percentage: true,
        negative: true,
        quirky: false,
    };
    /// `<length-percentage [0,∞]>`.
    pub(crate) const NON_NEGATIVE: Self = LengthOptions {
        percentage: true,
        negative: false,
        quirky: false,
    };

    /// These options with quirky lengths allowed or not.
    pub(crate) fn with_quirks(self, quirky: bool) -> Self {
        LengthOptions { quirky, ..self }
    }
}

/// Converts a length dimension. Returns `None` for unknown units.
fn length_from_dimension(value: f32, unit: &str) -> Option<Length> {
    LengthUnit::from_name(unit).map(|unit| Length { value, unit })
}

/// Consumes a `<length>` or `<length-percentage>`, depending on `opts`.
pub(crate) fn parse_length_percentage(
    p: &mut Parser<'_>,
    opts: LengthOptions,
) -> ParseResult<SpecifiedLengthPercentage> {
    p.try_parse(|p| {
        if p.peek()
            .and_then(ComponentValue::as_function)
            .is_some_and(|f| is_math_function(&f.name))
        {
            return parse_math_function(p)?.into_length_percentage(opts.percentage);
        }
        let value = p.next().ok_or(ParseError::EndOfInput)?;
        let negative_ok = |v: f32| opts.negative || v >= 0.0;
        match value {
            ComponentValue::Dimension { number, unit } => {
                let length =
                    length_from_dimension(number.value, unit).ok_or(ParseError::Unexpected)?;
                if !negative_ok(number.value) {
                    return Err(ParseError::Invalid);
                }
                Ok(SpecifiedLengthPercentage::Length(length))
            }
            ComponentValue::Number(n) if n.value == 0.0 => {
                Ok(SpecifiedLengthPercentage::Length(Length::px(0.0)))
            }
            ComponentValue::Number(n) if opts.quirky => {
                if !negative_ok(n.value) {
                    return Err(ParseError::Invalid);
                }
                Ok(SpecifiedLengthPercentage::Length(Length::px(n.value)))
            }
            ComponentValue::Percentage(n) if opts.percentage => {
                if !negative_ok(n.value) {
                    return Err(ParseError::Invalid);
                }
                Ok(SpecifiedLengthPercentage::Percentage(n.value / 100.0))
            }
            _ => Err(ParseError::Unexpected),
        }
    })
}

fn is_math_function(name: &str) -> bool {
    ["calc", "-webkit-calc", "-moz-calc", "min", "max", "clamp"]
        .iter()
        .any(|f| f.eq_ignore_ascii_case(name))
}

/// The type of a math expression.
/// <https://www.w3.org/TR/css-values-4/#calc-type-checking>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CalcKind {
    /// A plain number.
    Number,
    /// A length.
    Length,
    /// A percentage.
    Percentage,
    /// A length mixed with a percentage.
    LengthPercentage,
}

impl CalcKind {
    /// The type of a sum of two values, or `None` if they cannot be added.
    fn add(self, other: CalcKind) -> Option<CalcKind> {
        use CalcKind::{Length, LengthPercentage, Number, Percentage};
        match (self, other) {
            (Number, Number) => Some(Number),
            (Number, _) | (_, Number) => None,
            (Length, Length) => Some(Length),
            (Percentage, Percentage) => Some(Percentage),
            _ => Some(LengthPercentage),
        }
    }
}

/// A parsed math expression and its type.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MathExpr {
    /// The expression.
    pub(crate) node: CalcNode,
    /// Its type.
    pub(crate) kind: CalcKind,
}

impl MathExpr {
    /// The value, if the expression is a plain number.
    pub(crate) fn as_number(&self) -> Option<f32> {
        if self.kind != CalcKind::Number {
            return None;
        }
        let v = self.node.compute(&LengthContext::DEFAULT).resolve(0.0);
        v.is_finite().then_some(v)
    }

    /// Converts to a specified length-percentage. Fails for numbers, and
    /// for percentages unless `allow_percentage`.
    fn into_length_percentage(
        self,
        allow_percentage: bool,
    ) -> ParseResult<SpecifiedLengthPercentage> {
        match self.kind {
            CalcKind::Number => Err(ParseError::Invalid),
            CalcKind::Percentage | CalcKind::LengthPercentage if !allow_percentage => {
                Err(ParseError::Invalid)
            }
            _ => Ok(match self.node {
                CalcNode::Length(l) => SpecifiedLengthPercentage::Length(l),
                CalcNode::Percentage(p) => SpecifiedLengthPercentage::Percentage(p),
                node => SpecifiedLengthPercentage::Calc(Box::new(node)),
            }),
        }
    }
}

/// Consumes a math function: `calc()`, `min()`, `max()` or `clamp()`.
pub(crate) fn parse_math_function(p: &mut Parser<'_>) -> ParseResult<MathExpr> {
    p.try_parse(|p| {
        let (name, mut args) = p.expect_function()?;
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "calc" | "-webkit-calc" | "-moz-calc" => args.parse_entirely(parse_sum),
            "min" | "max" => {
                let items = args.parse_comma_separated(parse_sum)?;
                let kind = combined_kind(&items)?;
                let nodes = items.into_iter().map(|e| e.node).collect();
                let node = if name == "min" {
                    CalcNode::Min(nodes)
                } else {
                    CalcNode::Max(nodes)
                };
                Ok(MathExpr { node, kind })
            }
            "clamp" => {
                let items = args.parse_comma_separated(parse_sum)?;
                let kind = combined_kind(&items)?;
                let [lo, value, hi]: [MathExpr; 3] =
                    items.try_into().map_err(|_| ParseError::Invalid)?;
                Ok(MathExpr {
                    node: CalcNode::Clamp(
                        Box::new(lo.node),
                        Box::new(value.node),
                        Box::new(hi.node),
                    ),
                    kind,
                })
            }
            _ => Err(ParseError::Unexpected),
        }
    })
}

fn combined_kind(items: &[MathExpr]) -> ParseResult<CalcKind> {
    let mut iter = items.iter();
    let first = iter.next().ok_or(ParseError::EndOfInput)?.kind;
    iter.try_fold(first, |acc, e| acc.add(e.kind).ok_or(ParseError::Invalid))
}

/// `<calc-sum> = <calc-product> [ [ '+' | '-' ] <calc-product> ]*`. The
/// operators must have whitespace on both sides.
fn parse_sum(p: &mut Parser<'_>) -> ParseResult<MathExpr> {
    let first = parse_product(p)?;
    let mut kind = first.kind;
    let mut terms = vec![first.node];
    while let Ok((negate, rhs)) = p.try_parse(|p| {
        if !p.skip_whitespace() {
            return Err(ParseError::Unexpected);
        }
        let negate = match p.next_including_whitespace() {
            Some(v) if v.is_delim('+') => false,
            Some(v) if v.is_delim('-') => true,
            _ => return Err(ParseError::Unexpected),
        };
        if !p.skip_whitespace() {
            return Err(ParseError::Invalid);
        }
        Ok((negate, parse_product(p)?))
    }) {
        kind = kind.add(rhs.kind).ok_or(ParseError::Invalid)?;
        terms.push(if negate {
            negate_node(rhs.node)
        } else {
            rhs.node
        });
    }
    let node = if terms.len() == 1 {
        terms.pop().ok_or(ParseError::EndOfInput)?
    } else {
        CalcNode::Sum(terms)
    };
    Ok(MathExpr { node, kind })
}

fn negate_node(node: CalcNode) -> CalcNode {
    match node {
        CalcNode::Number(n) => CalcNode::Number(-n),
        CalcNode::Percentage(n) => CalcNode::Percentage(-n),
        CalcNode::Length(l) => CalcNode::Length(Length {
            value: -l.value,
            unit: l.unit,
        }),
        other => CalcNode::Product(Box::new(CalcNode::Number(-1.0)), Box::new(other)),
    }
}

/// `<calc-product> = <calc-value> [ [ '*' | '/' ] <calc-value> ]*`.
fn parse_product(p: &mut Parser<'_>) -> ParseResult<MathExpr> {
    let mut acc = parse_value(p)?;
    loop {
        let op = p.try_parse(|p| match p.next() {
            Some(v) if v.is_delim('*') => Ok('*'),
            Some(v) if v.is_delim('/') => Ok('/'),
            _ => Err(ParseError::Unexpected),
        });
        let Ok(op) = op else {
            break;
        };
        let rhs = parse_value(p)?;
        acc = if op == '*' {
            let ((CalcKind::Number, kind) | (kind, CalcKind::Number)) = (acc.kind, rhs.kind) else {
                return Err(ParseError::Invalid);
            };
            MathExpr {
                node: CalcNode::Product(Box::new(acc.node), Box::new(rhs.node)),
                kind,
            }
        } else {
            if rhs.kind != CalcKind::Number {
                return Err(ParseError::Invalid);
            }
            MathExpr {
                node: CalcNode::Quotient(Box::new(acc.node), Box::new(rhs.node)),
                kind: acc.kind,
            }
        };
    }
    Ok(acc)
}

/// `<calc-value>`: a number, dimension, percentage, constant, nested math
/// function or parenthesized sum.
fn parse_value(p: &mut Parser<'_>) -> ParseResult<MathExpr> {
    let value = p.peek().ok_or(ParseError::EndOfInput)?;
    let expr = match value {
        ComponentValue::Number(n) => MathExpr {
            node: CalcNode::Number(n.value),
            kind: CalcKind::Number,
        },
        ComponentValue::Percentage(n) => MathExpr {
            node: CalcNode::Percentage(n.value / 100.0),
            kind: CalcKind::Percentage,
        },
        ComponentValue::Dimension { number, unit } => MathExpr {
            node: CalcNode::Length(
                length_from_dimension(number.value, unit).ok_or(ParseError::Unexpected)?,
            ),
            kind: CalcKind::Length,
        },
        ComponentValue::Ident(name) => {
            let v = match name.to_ascii_lowercase().as_str() {
                "e" => std::f32::consts::E,
                "pi" => std::f32::consts::PI,
                "infinity" => f32::MAX,
                "-infinity" => f32::MIN,
                _ => return Err(ParseError::Unexpected),
            };
            MathExpr {
                node: CalcNode::Number(v),
                kind: CalcKind::Number,
            }
        }
        ComponentValue::Function(_) => return parse_math_function(p),
        ComponentValue::Block(b) if b.kind == BlockKind::Paren => {
            let expr = Parser::new(&b.contents).parse_entirely(parse_sum)?;
            p.next();
            return Ok(expr);
        }
        _ => return Err(ParseError::Unexpected),
    };
    p.next();
    Ok(expr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::test_util::parse_all;
    use crate::values::LengthPercentage;

    fn lp(css: &str, opts: LengthOptions) -> Option<LengthPercentage> {
        parse_all(css, |p| parse_length_percentage(p, opts))
            .ok()
            .map(|v| v.compute(&LengthContext::DEFAULT))
    }

    #[test]
    fn simple_lengths() {
        let o = LengthOptions::LENGTH_PERCENTAGE;
        let cases: &[(&str, Option<LengthPercentage>)] = &[
            ("10px", Some(LengthPercentage::Px(10.0))),
            ("2em", Some(LengthPercentage::Px(32.0))),
            ("1in", Some(LengthPercentage::Px(96.0))),
            ("50%", Some(LengthPercentage::Percent(0.5))),
            ("0", Some(LengthPercentage::Px(0.0))),
            ("-5px", Some(LengthPercentage::Px(-5.0))),
            ("10", None),
            ("10foo", None),
            ("auto", None),
            ("1vw", Some(LengthPercentage::Px(8.0))),
            ("2cqw", Some(LengthPercentage::Px(16.0))),
        ];
        for (css, expected) in cases {
            assert_eq!(lp(css, o), *expected, "{css}");
        }
        assert_eq!(lp("50%", LengthOptions::LENGTH), None);
        assert_eq!(lp("-1px", LengthOptions::NON_NEGATIVE), None);
        assert_eq!(lp("-1%", LengthOptions::NON_NEGATIVE), None);
        assert_eq!(
            lp("12", o.with_quirks(true)),
            Some(LengthPercentage::Px(12.0))
        );
    }

    #[test]
    fn calc_expressions() {
        let o = LengthOptions::LENGTH_PERCENTAGE;
        let px = |v| Some(LengthPercentage::Px(v));
        assert_eq!(lp("calc(10px + 2em)", o), px(42.0));
        assert_eq!(lp("calc(10px - 2px)", o), px(8.0));
        assert_eq!(lp("calc(2 * 3px)", o), px(6.0));
        assert_eq!(lp("calc(3px * 2)", o), px(6.0));
        assert_eq!(lp("calc(10px / 4)", o), px(2.5));
        assert_eq!(lp("calc((1px + 2px) * 2)", o), px(6.0));
        assert_eq!(lp("calc(calc(1px + 1px) * 2)", o), px(4.0));
        assert_eq!(lp("min(10px, 5px)", o), px(5.0));
        assert_eq!(lp("max(10px, 5px, 1em)", o), px(16.0));
        assert_eq!(lp("clamp(1px, 5px, 3px)", o), px(3.0));
        assert_eq!(lp("calc(50%)", o), Some(LengthPercentage::Percent(0.5)));
        assert_eq!(lp("-webkit-calc(1px + 1px)", o), px(2.0));
        let mixed = lp("calc(100% - 20px)", o).expect("valid calc");
        assert_eq!(mixed.resolve(200.0), 180.0);
        let nested = lp("max(1em, min(50%, 300px))", o).expect("valid max");
        assert_eq!(nested.resolve(1000.0), 300.0);
        assert_eq!(nested.resolve(20.0), 16.0);
        // Invalid expressions.
        for css in [
            "calc(10px+2px)",
            "calc(10px -2px)",
            "calc(10px * 2px)",
            "calc(10px / 2px)",
            "calc(1 + 1px)",
            "calc(5)",
            "calc()",
            "clamp(1px, 2px)",
            "calc(1px +)",
            "round(1px, 2px)",
        ] {
            assert_eq!(lp(css, o), None, "{css}");
        }
        assert_eq!(lp("calc(50% + 1px)", LengthOptions::LENGTH), None);
    }

    #[test]
    fn numbers_in_calc() {
        let n = |css| {
            parse_all(css, parse_math_function)
                .ok()
                .and_then(|e| e.as_number())
        };
        assert_eq!(n("calc(1 + 2 * 3)"), Some(7.0));
        assert_eq!(n("calc(pi * 0)"), Some(0.0));
        assert_eq!(n("min(3, 2, 5)"), Some(2.0));
        assert_eq!(n("calc(1px)"), None);
    }
}
