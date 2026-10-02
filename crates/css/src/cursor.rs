//! A cursor over component values, for parsing property values, preludes
//! and function arguments.
//!
//! [`Parser`] is `Copy`, so backtracking is a copy of the cursor. The
//! `expect_*` methods consume a value only on success: on error the position
//! does not change. This makes optional keywords cheap:
//! `if p.expect_ident_matching("inset").is_ok() { ... }`.
//!
//! Most methods skip whitespace before the value they read. Methods whose
//! name contains `including_whitespace` do not.

use crate::tokenizer::Number;
use crate::values::{BlockKind, ComponentValue};

/// An error from parsing component values.
///
/// The variants carry no data, so that backtracking (which creates and
/// drops many errors) does not allocate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The input ended before the expected value.
    #[error("unexpected end of input")]
    EndOfInput,
    /// The next value has the wrong type.
    #[error("unexpected value")]
    Unexpected,
    /// The value has the right type but is not valid here (for example a
    /// negative length where only positive ones are allowed).
    #[error("invalid value")]
    Invalid,
}

/// A cursor over a slice of component values.
#[derive(Clone, Copy, Debug)]
pub struct Parser<'a> {
    input: &'a [ComponentValue],
    pos: usize,
}

impl<'a> Parser<'a> {
    /// Creates a cursor at the start of `input`.
    pub fn new(input: &'a [ComponentValue]) -> Self {
        Parser { input, pos: 0 }
    }

    /// The current position, for [`Parser::reset`].
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Moves back (or forward) to a position from [`Parser::position`].
    pub fn reset(&mut self, position: usize) {
        self.pos = position.min(self.input.len());
    }

    /// The values that are not consumed yet, including whitespace.
    pub fn remaining(&self) -> &'a [ComponentValue] {
        self.input.get(self.pos..).unwrap_or_default()
    }

    /// True if only whitespace remains.
    pub fn is_exhausted(&self) -> bool {
        self.remaining().iter().all(ComponentValue::is_whitespace)
    }

    /// Returns an error unless only whitespace remains. Consumes the
    /// trailing whitespace.
    pub fn expect_exhausted(&mut self) -> Result<(), ParseError> {
        if self.is_exhausted() {
            self.pos = self.input.len();
            Ok(())
        } else {
            Err(ParseError::Unexpected)
        }
    }

    /// Skips whitespace. Returns true if there was any.
    pub fn skip_whitespace(&mut self) -> bool {
        let start = self.pos;
        while self
            .input
            .get(self.pos)
            .is_some_and(ComponentValue::is_whitespace)
        {
            self.pos += 1;
        }
        self.pos > start
    }

    /// Returns the next value that is not whitespace and consumes it.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&'a ComponentValue> {
        self.skip_whitespace();
        self.next_including_whitespace()
    }

    /// Returns the next value, whitespace included, and consumes it.
    pub fn next_including_whitespace(&mut self) -> Option<&'a ComponentValue> {
        let value = self.input.get(self.pos)?;
        self.pos += 1;
        Some(value)
    }

    /// Returns the next value that is not whitespace, without consuming it.
    pub fn peek(&self) -> Option<&'a ComponentValue> {
        self.remaining().iter().find(|v| !v.is_whitespace())
    }

    /// Returns the next value, whitespace included, without consuming it.
    pub fn peek_including_whitespace(&self) -> Option<&'a ComponentValue> {
        self.input.get(self.pos)
    }

    /// Runs `f`. If it fails, restores the position from before the call.
    pub fn try_parse<T, E>(
        &mut self,
        f: impl FnOnce(&mut Parser<'a>) -> Result<T, E>,
    ) -> Result<T, E> {
        let saved = *self;
        let result = f(self);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    /// Runs `f`, then requires that only whitespace remains. On error the
    /// position is restored.
    pub fn parse_entirely<T, E: From<ParseError>>(
        &mut self,
        f: impl FnOnce(&mut Parser<'a>) -> Result<T, E>,
    ) -> Result<T, E> {
        self.try_parse(|p| {
            let result = f(p)?;
            p.expect_exhausted()?;
            Ok(result)
        })
    }

    /// Parses the rest of the input as a comma-separated list. `f` runs once
    /// per item, on a cursor that covers only that item, and must consume
    /// the whole item. Empty items are errors (unless `f` accepts empty
    /// input). On error the position is restored.
    pub fn parse_comma_separated<T, E: From<ParseError>>(
        &mut self,
        mut f: impl FnMut(&mut Parser<'a>) -> Result<T, E>,
    ) -> Result<Vec<T>, E> {
        let mut results = Vec::new();
        for item in self.remaining().split(ComponentValue::is_comma) {
            let mut item_parser = Parser::new(item);
            results.push(item_parser.parse_entirely(&mut f)?);
        }
        self.pos = self.input.len();
        Ok(results)
    }

    /// Consumes the next value if `f` maps it to `Some`.
    fn expect_map<T>(
        &mut self,
        f: impl FnOnce(&'a ComponentValue) -> Option<T>,
    ) -> Result<T, ParseError> {
        let mut p = *self;
        let value = p.next().ok_or(ParseError::EndOfInput)?;
        let result = f(value).ok_or(ParseError::Unexpected)?;
        *self = p;
        Ok(result)
    }

    /// Consumes an identifier and returns it as written.
    pub fn expect_ident(&mut self) -> Result<&'a str, ParseError> {
        self.expect_map(ComponentValue::as_ident)
    }

    /// Consumes an identifier that matches `name` ASCII case-insensitively.
    pub fn expect_ident_matching(&mut self, name: &str) -> Result<(), ParseError> {
        self.expect_map(|v| v.is_ident(name).then_some(()))
    }

    /// Consumes an identifier that matches one of the names in `options`
    /// (ASCII case-insensitively) and returns the value paired with it. For
    /// keyword properties:
    /// `p.expect_one_of(&[("block", Display::Block), ("none", Display::None)])`.
    pub fn expect_one_of<T: Copy>(&mut self, options: &[(&str, T)]) -> Result<T, ParseError> {
        self.expect_map(|v| {
            let ident = v.as_ident()?;
            options
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(ident))
                .map(|&(_, value)| value)
        })
    }

    /// Consumes a quoted string.
    pub fn expect_string(&mut self) -> Result<&'a str, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::String(s) => Some(&**s),
            _ => None,
        })
    }

    /// Consumes an identifier or a quoted string.
    pub fn expect_ident_or_string(&mut self) -> Result<&'a str, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Ident(s) | ComponentValue::String(s) => Some(&**s),
            _ => None,
        })
    }

    /// Consumes a URL: `url(foo)` or `url("foo")`.
    pub fn expect_url(&mut self) -> Result<&'a str, ParseError> {
        self.expect_map(url_value)
    }

    /// Consumes a URL or a quoted string (as in `@import "foo.css"`).
    pub fn expect_url_or_string(&mut self) -> Result<&'a str, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::String(s) => Some(&**s),
            v => url_value(v),
        })
    }

    /// Consumes a number and returns its value.
    pub fn expect_number(&mut self) -> Result<f32, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Number(n) => Some(n.value),
            _ => None,
        })
    }

    /// Consumes a number and returns all of its token data (value, integer
    /// flag, sign flag).
    pub fn expect_number_token(&mut self) -> Result<Number, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Number(n) => Some(*n),
            _ => None,
        })
    }

    /// Consumes a number with the "integer" type flag.
    pub fn expect_integer(&mut self) -> Result<i32, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Number(n) => n.int_value,
            _ => None,
        })
    }

    /// Consumes a percentage and returns the number before `%` (50 for
    /// `50%`).
    pub fn expect_percentage(&mut self) -> Result<f32, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Percentage(n) => Some(n.value),
            _ => None,
        })
    }

    /// Consumes a dimension and returns the number and the unit as written.
    pub fn expect_dimension(&mut self) -> Result<(f32, &'a str), ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Dimension { number, unit } => Some((number.value, &**unit)),
            _ => None,
        })
    }

    /// Consumes a hash (`#abc`) and returns the value without `#`.
    pub fn expect_hash(&mut self) -> Result<&'a str, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Hash { value, .. } => Some(&**value),
            _ => None,
        })
    }

    /// Consumes the delim `c`.
    pub fn expect_delim(&mut self, c: char) -> Result<(), ParseError> {
        self.expect_map(|v| v.is_delim(c).then_some(()))
    }

    /// Consumes a comma.
    pub fn expect_comma(&mut self) -> Result<(), ParseError> {
        self.expect_map(|v| v.is_comma().then_some(()))
    }

    /// Consumes a colon.
    pub fn expect_colon(&mut self) -> Result<(), ParseError> {
        self.expect_map(|v| matches!(v, ComponentValue::Colon).then_some(()))
    }

    /// Consumes a function. Returns its name as written and a cursor over
    /// its arguments.
    pub fn expect_function(&mut self) -> Result<(&'a str, Parser<'a>), ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Function(f) => Some((&*f.name, Parser::new(&f.arguments))),
            _ => None,
        })
    }

    /// Consumes a function whose name matches `name` ASCII
    /// case-insensitively. Returns a cursor over its arguments.
    pub fn expect_function_matching(&mut self, name: &str) -> Result<Parser<'a>, ParseError> {
        self.expect_map(|v| match v {
            ComponentValue::Function(f) if f.name.eq_ignore_ascii_case(name) => {
                Some(Parser::new(&f.arguments))
            }
            _ => None,
        })
    }

    /// Consumes a simple block of `kind`. Returns a cursor over its
    /// contents.
    pub fn expect_block(&mut self, kind: BlockKind) -> Result<Parser<'a>, ParseError> {
        self.expect_map(|v| v.as_block(kind).map(Parser::new))
    }
}

/// The URL of `url(foo)` or `url("foo")`.
fn url_value(value: &ComponentValue) -> Option<&str> {
    match value {
        ComponentValue::Url(url) => Some(url),
        ComponentValue::Function(f) if f.name.eq_ignore_ascii_case("url") => {
            let mut args = Parser::new(&f.arguments);
            let url = args.expect_string().ok()?;
            // CSS Values 4 allows URL modifiers after the string; none are
            // defined yet, so anything else makes the URL invalid.
            args.is_exhausted().then_some(url)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_component_values;

    #[test]
    fn next_and_peek() {
        let values = parse_component_values(" a  b ");
        let mut p = Parser::new(&values);
        assert!(p.peek().is_some_and(|v| v.is_ident("a")));
        assert_eq!(
            p.peek_including_whitespace(),
            Some(&ComponentValue::Whitespace)
        );
        assert!(p.next().is_some_and(|v| v.is_ident("a")));
        assert_eq!(
            p.next_including_whitespace(),
            Some(&ComponentValue::Whitespace)
        );
        assert!(p.next().is_some_and(|v| v.is_ident("b")));
        assert!(p.is_exhausted());
        assert_eq!(p.next(), None);
        assert_eq!(p.expect_exhausted(), Ok(()));
    }

    #[test]
    fn expect_does_not_consume_on_error() {
        let values = parse_component_values("auto 10px");
        let mut p = Parser::new(&values);
        assert_eq!(p.expect_number(), Err(ParseError::Unexpected));
        assert_eq!(p.expect_ident_matching("none"), Err(ParseError::Unexpected));
        assert_eq!(p.expect_ident_matching("AUTO"), Ok(()));
        assert_eq!(p.expect_dimension(), Ok((10.0, "px")));
        assert_eq!(p.expect_ident(), Err(ParseError::EndOfInput));
    }

    #[test]
    fn typed_expectations() {
        let values =
            parse_component_values("x 'str' url(a.png) url('b.png') 1.5 7 50% #fff , : / f(1) [z]");
        let mut p = Parser::new(&values);
        assert_eq!(p.expect_ident_or_string(), Ok("x"));
        assert_eq!(p.expect_string(), Ok("str"));
        assert_eq!(p.expect_url(), Ok("a.png"));
        assert_eq!(p.expect_url_or_string(), Ok("b.png"));
        assert_eq!(p.expect_integer(), Err(ParseError::Unexpected));
        assert_eq!(p.expect_number(), Ok(1.5));
        assert_eq!(p.expect_integer(), Ok(7));
        assert_eq!(p.expect_percentage(), Ok(50.0));
        assert_eq!(p.expect_hash(), Ok("fff"));
        assert_eq!(p.expect_comma(), Ok(()));
        assert_eq!(p.expect_colon(), Ok(()));
        assert_eq!(p.expect_delim('/'), Ok(()));
        let (name, mut args) = p.expect_function().expect("function");
        assert_eq!(name, "f");
        assert_eq!(args.expect_integer(), Ok(1));
        let mut block = p.expect_block(BlockKind::Square).expect("block");
        assert_eq!(block.expect_ident(), Ok("z"));
        assert!(p.is_exhausted());
    }

    #[test]
    fn keywords_and_number_tokens() {
        #[derive(Clone, Copy, Debug, PartialEq)]
        enum Display {
            Block,
            None,
        }
        let values = parse_component_values("NONE x +3");
        let mut p = Parser::new(&values);
        let options = [("block", Display::Block), ("none", Display::None)];
        assert_eq!(p.expect_one_of(&options), Ok(Display::None));
        assert_eq!(p.expect_one_of(&options), Err(ParseError::Unexpected));
        assert_eq!(p.expect_ident(), Ok("x"));
        let n = p.expect_number_token().expect("number");
        assert_eq!((n.value, n.int_value, n.has_sign), (3.0, Some(3), true));
    }

    #[test]
    fn functions() {
        let values = parse_component_values("RGB(1 2 3)");
        let mut p = Parser::new(&values);
        assert!(p.clone().expect_function_matching("hsl").is_err());
        let mut args = p.expect_function_matching("rgb").expect("rgb()");
        let channels: Result<Vec<i32>, ParseError> =
            (0..3).map(|_| args.expect_integer()).collect();
        assert_eq!(channels, Ok(vec![1, 2, 3]));
        assert!(args.is_exhausted());
    }

    #[test]
    fn try_parse_restores() {
        let values = parse_component_values("a b");
        let mut p = Parser::new(&values);
        let result: Result<(), ParseError> = p.try_parse(|p| {
            p.expect_ident()?;
            p.expect_number()?;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(p.position(), 0);
        let _ = p.expect_ident();
        let pos = p.position();
        let _ = p.expect_ident();
        p.reset(pos);
        assert_eq!(p.expect_ident(), Ok("b"));
    }

    #[test]
    fn comma_separated() {
        let values = parse_component_values("a, b c , d");
        let mut p = Parser::new(&values);
        let result = p.parse_comma_separated(Parser::expect_ident);
        assert_eq!(result, Err(ParseError::Unexpected));
        assert_eq!(p.position(), 0);
        let result = p.parse_comma_separated(|p| {
            let mut words = vec![p.expect_ident()?];
            while let Ok(w) = p.expect_ident() {
                words.push(w);
            }
            Ok::<_, ParseError>(words)
        });
        assert_eq!(result, Ok(vec![vec!["a"], vec!["b", "c"], vec!["d"]]));
        assert!(p.is_exhausted());
        let values = parse_component_values("a,");
        let mut p = Parser::new(&values);
        assert_eq!(
            p.parse_comma_separated(Parser::expect_ident),
            Err(ParseError::EndOfInput)
        );
    }

    #[test]
    fn parse_entirely() {
        let values = parse_component_values("1 2");
        let mut p = Parser::new(&values);
        assert_eq!(
            p.parse_entirely(Parser::expect_number),
            Err(ParseError::Unexpected)
        );
        assert_eq!(p.position(), 0);
        assert_eq!(
            p.parse_entirely(|p| Ok::<_, ParseError>((p.expect_number()?, p.expect_number()?))),
            Ok((1.0, 2.0))
        );
    }

    #[test]
    fn url_with_modifiers_is_rejected() {
        let values = parse_component_values("url('a' b)");
        assert!(Parser::new(&values).expect_url().is_err());
    }
}
