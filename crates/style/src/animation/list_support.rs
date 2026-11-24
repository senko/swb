//! Helpers that the `animation-*` and `transition-*` longhands share:
//! the repetition of a list to the number of items (CSS Animations 1
//! §4.2, CSS Transitions 1 §2), the comma-separated list grammar, a
//! non-negative `<time>` and the reserved `<custom-ident>` words.

use std::sync::Arc;

use swb_css::{ParseError, Parser};

use super::time::{Time, parse_time};
use crate::parse::ParseResult;

/// Item `i` of a list, repeating the list (a list is never empty).
pub(super) fn cycle<T>(list: &[T], i: usize) -> Option<&T> {
    list.get(i % list.len().max(1))
}

/// A comma-separated list of at least one item.
pub(super) fn list<T>(
    p: &mut Parser<'_>,
    item: impl FnMut(&mut Parser<'_>) -> ParseResult<T>,
) -> ParseResult<Arc<[T]>> {
    Ok(Arc::from(p.parse_comma_separated(item)?))
}

/// A `<time>` not below 0 (a math function may be; it computes to 0).
pub(super) fn parse_nonneg_time(p: &mut Parser<'_>) -> ParseResult<Time> {
    let t = parse_time(p)?;
    if t.seconds < 0.0 && !t.is_math() {
        return Err(ParseError::Invalid);
    }
    Ok(t)
}

/// The words that cannot be a `<custom-ident>` of `animation-name` and
/// `transition-property`: the CSS-wide keywords and `default`.
const RESERVED_WIDE: [&str; 6] = [
    "initial",
    "inherit",
    "unset",
    "revert",
    "revert-layer",
    "default",
];

/// True if `ident` is one of [`RESERVED_WIDE`] or of `extra` (the keyword
/// of the property, which is no `<custom-ident>` either).
pub(super) fn is_reserved(ident: &str, extra: &[&str]) -> bool {
    RESERVED_WIDE
        .iter()
        .chain(extra)
        .any(|k| ident.eq_ignore_ascii_case(k))
}
