//! Parsers for CSS value types: numbers, angles, lengths (with `calc()`),
//! colors and images.
//!
//! The property parsers in [`crate::properties`] combine these. All
//! parsers work on a [`swb_css::Parser`] cursor and leave the position
//! unchanged on error.

pub(crate) mod color;
pub(crate) mod image;
pub(crate) mod length;

use std::sync::Arc;

use swb_css::{ComponentValue, ParseError, Parser};
use url::Url;

/// Where a declaration comes from. It decides which internal keywords are
/// accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The user-agent stylesheet.
    UserAgent,
    /// Author stylesheets, `style` attributes and presentational hints.
    Author,
}

/// Settings for parsing property values.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParserContext {
    /// The base URL for relative `url()` values.
    pub(crate) base_url: Option<Arc<Url>>,
    /// True when parsing author CSS of a quirks-mode document (enables the
    /// unitless length quirk).
    pub(crate) quirks: bool,
    /// The origin of the CSS.
    pub(crate) origin: Origin,
}

impl ParserContext {
    /// A context for the user-agent stylesheet.
    pub(crate) fn user_agent() -> Self {
        ParserContext {
            base_url: None,
            quirks: false,
            origin: Origin::UserAgent,
        }
    }

    /// A context for author CSS.
    pub(crate) fn author(base_url: Option<Arc<Url>>, quirks: bool) -> Self {
        ParserContext {
            base_url,
            quirks,
            origin: Origin::Author,
        }
    }

    /// Resolves a URL from a `url()` value against the base URL. Returns
    /// the input unchanged if there is no base URL or resolution fails.
    pub(crate) fn resolve_url(&self, url: &str) -> Arc<str> {
        let resolved = match &self.base_url {
            Some(base) => base.join(url).ok(),
            None => Url::parse(url).ok(),
        };
        match resolved {
            Some(u) => Arc::from(u.as_str()),
            None => Arc::from(url),
        }
    }
}

/// The result type of value parsers.
pub(crate) type ParseResult<T> = Result<T, ParseError>;

/// Consumes a `<number>`, including a `calc()` expression that has no
/// units.
pub(crate) fn parse_number(p: &mut Parser<'_>) -> ParseResult<f32> {
    if let Ok(n) = p.expect_number() {
        return Ok(n);
    }
    p.try_parse(|p| {
        let node = length::parse_math_function(p)?;
        node.as_number().ok_or(ParseError::Invalid)
    })
}

/// Consumes a non-negative `<number>`.
pub(crate) fn parse_non_negative_number(p: &mut Parser<'_>) -> ParseResult<f32> {
    p.try_parse(|p| {
        let n = parse_number(p)?;
        if n < 0.0 {
            Err(ParseError::Invalid)
        } else {
            Ok(n)
        }
    })
}

/// Consumes an `<integer>`, including `calc()` that evaluates to a number
/// (rounded to the nearest integer).
pub(crate) fn parse_integer(p: &mut Parser<'_>) -> ParseResult<i32> {
    if let Ok(n) = p.expect_integer() {
        return Ok(n);
    }
    p.try_parse(|p| {
        let node = length::parse_math_function(p)?;
        let n = node.as_number().ok_or(ParseError::Invalid)?;
        Ok(n.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32)
    })
}

/// Converts an angle dimension to degrees.
/// <https://www.w3.org/TR/css-values-4/#angles>
fn angle_to_degrees(value: f32, unit: &str) -> Option<f32> {
    let degrees = match unit.to_ascii_lowercase().as_str() {
        "deg" => value,
        "grad" => value * 0.9,
        "rad" => value.to_degrees(),
        "turn" => value * 360.0,
        _ => return None,
    };
    Some(degrees)
}

/// Consumes an `<angle>` and returns it in degrees. With `allow_zero`, a
/// unitless zero is accepted (as in gradients).
pub(crate) fn parse_angle(p: &mut Parser<'_>, allow_zero: bool) -> ParseResult<f32> {
    p.try_parse(|p| match p.next() {
        Some(ComponentValue::Dimension { number, unit }) => {
            angle_to_degrees(number.value, unit).ok_or(ParseError::Unexpected)
        }
        Some(ComponentValue::Number(n)) if allow_zero && n.value == 0.0 => Ok(0.0),
        Some(_) => Err(ParseError::Unexpected),
        None => Err(ParseError::EndOfInput),
    })
}

/// Consumes a `<hue>`: a number (degrees) or an angle.
pub(crate) fn parse_hue(p: &mut Parser<'_>) -> ParseResult<f32> {
    if let Ok(n) = parse_number(p) {
        return Ok(n);
    }
    parse_angle(p, false)
}

/// True if `name` is a custom property name (`--foo`).
pub(crate) fn is_custom_property_name(name: &str) -> bool {
    name.starts_with("--")
}

#[cfg(test)]
pub(crate) mod test_util {
    //! Helpers for unit tests of value parsers.

    use swb_css::{ComponentValue, Parser, parse_component_values};

    use super::ParseResult;

    /// Parses `css` with `f`, requiring that `f` consumes all input.
    pub(crate) fn parse_all<T>(
        css: &str,
        f: impl FnOnce(&mut Parser<'_>) -> ParseResult<T>,
    ) -> ParseResult<T> {
        let values: Vec<ComponentValue> = parse_component_values(css);
        let mut p = Parser::new(&values);
        p.parse_entirely(f)
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::parse_all;
    use super::*;

    #[test]
    fn numbers_and_angles() {
        assert_eq!(parse_all("1.5", parse_number), Ok(1.5));
        assert_eq!(parse_all("calc(2 * 3)", parse_number), Ok(6.0));
        assert!(parse_all("calc(2px)", parse_number).is_err());
        assert!(parse_all("-1", parse_non_negative_number).is_err());
        assert_eq!(parse_all("7", parse_integer), Ok(7));
        assert!(parse_all("7.5", parse_integer).is_err());
        assert_eq!(parse_all("90deg", |p| parse_angle(p, false)), Ok(90.0));
        assert_eq!(parse_all("0.5turn", |p| parse_angle(p, false)), Ok(180.0));
        assert_eq!(parse_all("100grad", |p| parse_angle(p, false)), Ok(90.0));
        assert!(parse_all("0", |p| parse_angle(p, false)).is_err());
        assert_eq!(parse_all("0", |p| parse_angle(p, true)), Ok(0.0));
        assert_eq!(parse_all("120", parse_hue), Ok(120.0));
    }

    #[test]
    fn url_resolution() {
        let base = Arc::new(Url::parse("https://example.com/a/b.css").expect("valid URL"));
        let cx = ParserContext::author(Some(base), false);
        assert_eq!(&*cx.resolve_url("c.png"), "https://example.com/a/c.png");
        assert_eq!(&*cx.resolve_url("/d.png"), "https://example.com/d.png");
        let cx = ParserContext::user_agent();
        assert_eq!(&*cx.resolve_url("rel.png"), "rel.png");
    }
}
