//! `@supports` conditions.
//!
//! <https://www.w3.org/TR/css-conditional-3/#at-supports> and
//! <https://www.w3.org/TR/css-conditional-4/#at-supports-ext> (`selector()`).
//!
//! The css crate does not know which properties and values are supported,
//! so [`SupportsCondition::evaluate`] asks the caller about each
//! declaration. `selector()` is evaluated here: a selector is supported if
//! it parses. `<general-enclosed>` (including `font-tech()` and
//! `font-format()`) evaluates to false.

use crate::cursor::{ParseError, Parser};
use crate::parser::parse_declaration_from_values;
use crate::selector::SelectorList;
use crate::stylesheet::Declaration;
use crate::values::{BlockKind, ComponentValue};

/// A parsed `@supports` condition.
#[derive(Clone, Debug, PartialEq)]
pub enum SupportsCondition {
    /// `not <condition>`.
    Not(Box<SupportsCondition>),
    /// `<condition> and <condition> ...`.
    And(Vec<SupportsCondition>),
    /// `<condition> or <condition> ...`.
    Or(Vec<SupportsCondition>),
    /// `(property: value)`.
    Declaration(Declaration),
    /// `selector(...)`: true if the selector parses.
    Selector(bool),
    /// `<general-enclosed>`: always false.
    Unknown,
}

impl SupportsCondition {
    /// Parses a condition (the prelude of `@supports`).
    pub fn parse(input: &[ComponentValue]) -> Result<SupportsCondition, ParseError> {
        Parser::new(input).parse_entirely(parse_condition)
    }

    /// Parses a condition from text.
    pub fn parse_str(css: &str) -> Result<SupportsCondition, ParseError> {
        Self::parse(&crate::parse_component_values(css))
    }

    /// Evaluates the condition. `supports_declaration` returns true if the
    /// property and value of a declaration are supported.
    pub fn evaluate(&self, supports_declaration: &dyn Fn(&Declaration) -> bool) -> bool {
        match self {
            SupportsCondition::Not(c) => !c.evaluate(supports_declaration),
            SupportsCondition::And(list) => list.iter().all(|c| c.evaluate(supports_declaration)),
            SupportsCondition::Or(list) => list.iter().any(|c| c.evaluate(supports_declaration)),
            SupportsCondition::Declaration(d) => supports_declaration(d),
            SupportsCondition::Selector(supported) => *supported,
            SupportsCondition::Unknown => false,
        }
    }
}

/// `<supports-condition>`
fn parse_condition(p: &mut Parser<'_>) -> Result<SupportsCondition, ParseError> {
    if p.expect_ident_matching("not").is_ok() {
        return Ok(SupportsCondition::Not(Box::new(parse_in_parens(p)?)));
    }
    let first = parse_in_parens(p)?;
    let mut rest = Vec::new();
    let mut is_and = None;
    loop {
        let and = if p.expect_ident_matching("and").is_ok() {
            true
        } else if p.expect_ident_matching("or").is_ok() {
            false
        } else {
            break;
        };
        if is_and.is_some_and(|previous| previous != and) {
            return Err(ParseError::Invalid);
        }
        is_and = Some(and);
        rest.push(parse_in_parens(p)?);
    }
    Ok(match is_and {
        None => first,
        Some(and) => {
            rest.insert(0, first);
            if and {
                SupportsCondition::And(rest)
            } else {
                SupportsCondition::Or(rest)
            }
        }
    })
}

/// `<supports-in-parens>`
fn parse_in_parens(p: &mut Parser<'_>) -> Result<SupportsCondition, ParseError> {
    let value = p.peek().ok_or(ParseError::EndOfInput)?;
    let condition = if let Some(contents) = value.as_block(BlockKind::Paren) {
        parse_condition_or_declaration(contents)
    } else if let Some(f) = value.as_function() {
        if f.name.eq_ignore_ascii_case("selector") {
            let list = SelectorList::parse(&f.arguments);
            SupportsCondition::Selector(list.is_ok_and(|l| l.len() == 1))
        } else {
            SupportsCondition::Unknown
        }
    } else {
        return Err(ParseError::Unexpected);
    };
    p.next();
    Ok(condition)
}

/// Parses the contents of `( ... )` in a condition, or of `supports( ... )`
/// in `@import`: a condition or a declaration. Anything else is
/// `<general-enclosed>`.
pub(crate) fn parse_condition_or_declaration(values: &[ComponentValue]) -> SupportsCondition {
    if let Ok(condition) = SupportsCondition::parse(values) {
        condition
    } else if let Some(declaration) = parse_declaration_from_values(values) {
        SupportsCondition::Declaration(declaration)
    } else {
        SupportsCondition::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supports `display: grid|flex|block` and `color: <anything>`.
    fn supported(d: &Declaration) -> bool {
        match d.name.as_str() {
            "display" => {
                d.value.len() == 1
                    && ["grid", "flex", "block"]
                        .iter()
                        .any(|k| d.value[0].is_ident(k))
            }
            "color" => true,
            _ => false,
        }
    }

    fn eval(css: &str) -> Option<bool> {
        SupportsCondition::parse_str(css)
            .ok()
            .map(|c| c.evaluate(&supported))
    }

    #[test]
    fn evaluation_table() {
        let cases: &[(&str, Option<bool>)] = &[
            ("(display: grid)", Some(true)),
            ("(display:grid)", Some(true)),
            ("( display : grid )", Some(true)),
            ("(DISPLAY: grid)", Some(true)),
            ("(display: grid !important)", Some(true)),
            ("(display: nope)", Some(false)),
            ("(unknown: x)", Some(false)),
            ("not (display: nope)", Some(true)),
            ("(display: grid) and (color: red)", Some(true)),
            ("(display: grid) and (display: nope)", Some(false)),
            ("(display: nope) or (display: flex)", Some(true)),
            (
                "(display: grid) and (color: red) and (display: block)",
                Some(true),
            ),
            (
                "((display: grid) or (x: y)) and (not (display: nope))",
                Some(true),
            ),
            ("selector(a > b)", Some(true)),
            ("selector(:has(a))", Some(true)),
            ("selector(::-webkit-scrollbar)", Some(false)),
            ("selector(a, b)", Some(false)),
            ("not selector(:focus-visible)", Some(false)),
            ("font-tech(color-COLRv1)", Some(false)),
            ("(foo bar)", Some(false)),
            ("not (foo bar)", Some(true)),
            ("unknown(x)", Some(false)),
            // Invalid conditions.
            ("", None),
            ("display: grid", None),
            ("(display: grid) and (color: red) or (x: y)", None),
            ("(display: grid) (color: red)", None),
            ("not", None),
            ("(display: grid) and", None),
            ("x (display: grid)", None),
        ];
        for (css, expected) in cases {
            assert_eq!(eval(css), *expected, "{css}");
        }
    }

    #[test]
    fn not_without_space_is_general_enclosed() {
        // `not(` is a function token.
        assert_eq!(
            SupportsCondition::parse_str("not(display: nope)"),
            Ok(SupportsCondition::Unknown)
        );
    }
}
