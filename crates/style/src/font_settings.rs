//! `font-variation-settings` and `font-feature-settings`, also the
//! descriptors of the same names in `@font-face`.
//!
//! <https://drafts.csswg.org/css-fonts-4/#font-variation-settings-def>
//! and <https://drafts.csswg.org/css-fonts-4/#font-feature-settings-prop>.
//!
//! Values that the specification leaves open follow Chromium 148, measured
//! with `tools/probes/web-fonts-2.json` (`settings-computed`):
//!
//! - The computed value is sorted by tag, and a repeated tag keeps its
//!   last value.
//! - A tag is a string of exactly four characters in U+0020..U+007E; tags
//!   are case-sensitive. Any invalid entry makes the declaration invalid.
//! - Variation values are numbers (also `calc()`); a value outside the
//!   range of a finite `f32` is clamped (`1e400` is `f32::MAX`).
//! - Feature values are integers, `on` (1) or `off` (0), and 1 without a
//!   value. Negative values are valid. Values are clamped to `i32`.
//!   `1.5` is invalid.
//!
//! A declaration keeps at most [`MAX_SETTINGS`] distinct tags (the first
//! ones in source order). A font has few axes and features, so more
//! entries only slow down shaping, and the list is part of every font
//! instance key and shape plan key: a hostile style sheet would otherwise
//! multiply that cost by its size.

use std::collections::BTreeMap;
use std::sync::Arc;

use swb_css::{ParseError, Parser};

use crate::parse::{ParseResult, parse_integer, parse_number};

/// An OpenType tag: four ASCII characters.
pub type FontTag = [u8; 4];

/// The computed `font-variation-settings`: (axis tag, value), sorted by
/// tag, without repeated tags. `normal` is the empty list.
pub type FontVariationSettings = Arc<[(FontTag, f32)]>;

/// The computed `font-feature-settings`: (feature tag, value), sorted by
/// tag, without repeated tags. `normal` is the empty list. 0 turns the
/// feature off; other values turn it on or select an alternate.
pub type FontFeatureSettings = Arc<[(FontTag, i32)]>;

/// A sorted list of settings with values of type `V`.
type Settings<V> = Arc<[(FontTag, V)]>;

/// The most distinct tags that one declaration keeps (see the module
/// documentation).
pub const MAX_SETTINGS: usize = 64;

/// Parses `normal | [ <string> <number> ]#`.
pub(crate) fn parse_variation_settings(p: &mut Parser<'_>) -> ParseResult<FontVariationSettings> {
    parse_settings(p, "font-variation-settings", |p| {
        let tag = parse_tag(p)?;
        let value = parse_number(p)?;
        Ok((tag, finite(value)))
    })
}

/// Parses `normal | [ <string> [ <integer> | on | off ]? ]#`.
pub(crate) fn parse_feature_settings(p: &mut Parser<'_>) -> ParseResult<FontFeatureSettings> {
    parse_settings(p, "font-feature-settings", |p| {
        let tag = parse_tag(p)?;
        let value = if p.expect_ident_matching("on").is_ok() {
            1
        } else if p.expect_ident_matching("off").is_ok() {
            0
        } else if p.is_exhausted() {
            1
        } else {
            parse_feature_value(p)?
        };
        Ok((tag, value))
    })
}

/// An `<integer>`; an integer outside `i32` is clamped.
fn parse_feature_value(p: &mut Parser<'_>) -> ParseResult<i32> {
    if let Ok(n) = p.try_parse(parse_integer) {
        return Ok(n);
    }
    // The tokenizer gives no integer value to a number outside `i32`.
    p.try_parse(|p| {
        let n = p.expect_number()?;
        if n.abs() >= 2_147_483_648.0 && n.fract() == 0.0 {
            Ok(n.clamp(i32::MIN as f32, i32::MAX as f32) as i32)
        } else {
            Err(ParseError::Invalid)
        }
    })
}

/// `normal`, or a comma-separated list of settings: sorted by tag, a
/// repeated tag keeps its last value, at most [`MAX_SETTINGS`] tags.
fn parse_settings<V: Copy>(
    p: &mut Parser<'_>,
    name: &str,
    item: impl FnMut(&mut Parser<'_>) -> ParseResult<([u8; 4], V)>,
) -> ParseResult<Settings<V>> {
    if p.expect_ident_matching("normal").is_ok() {
        return Ok(Arc::from([]));
    }
    let items = p.parse_comma_separated(item)?;
    let mut map: BTreeMap<[u8; 4], V> = BTreeMap::new();
    let mut dropped = false;
    for (tag, value) in items {
        if map.len() < MAX_SETTINGS || map.contains_key(&tag) {
            map.insert(tag, value);
        } else {
            dropped = true;
        }
    }
    if dropped {
        log::warn!("{name}: more than {MAX_SETTINGS} tags; ignoring the rest");
    }
    Ok(map.into_iter().collect())
}

/// A tag: a string of four characters in U+0020..U+007E.
fn parse_tag(p: &mut Parser<'_>) -> ParseResult<FontTag> {
    let s = p.expect_string()?;
    let bytes = s.as_bytes();
    match <[u8; 4]>::try_from(bytes) {
        Ok(tag) if tag.iter().all(|b| (0x20..=0x7E).contains(b)) => Ok(tag),
        _ => Err(ParseError::Invalid),
    }
}

/// A finite value: infinities become the largest `f32`, NaN becomes 0.
fn finite(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(f32::MIN, f32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::{ComponentValue, parse_component_values};

    fn parse<T>(
        css: &str,
        f: impl FnOnce(&mut Parser<'_>) -> ParseResult<T>,
    ) -> Result<T, ParseError> {
        let values: Vec<ComponentValue> = parse_component_values(css);
        Parser::new(&values).parse_entirely(f)
    }

    fn var(css: &str) -> Option<Vec<(String, f32)>> {
        parse(css, parse_variation_settings).ok().map(|s| {
            s.iter()
                .map(|(t, v)| (String::from_utf8_lossy(t).into_owned(), *v))
                .collect()
        })
    }

    fn feat(css: &str) -> Option<Vec<(String, i32)>> {
        parse(css, parse_feature_settings).ok().map(|s| {
            s.iter()
                .map(|(t, v)| (String::from_utf8_lossy(t).into_owned(), *v))
                .collect()
        })
    }

    fn pair(tag: &str, value: f32) -> (String, f32) {
        (tag.to_owned(), value)
    }

    #[test]
    fn variation_values() {
        assert_eq!(var("normal"), Some(vec![]));
        assert_eq!(var("'wght' 660"), Some(vec![pair("wght", 660.0)]));
        assert_eq!(
            var("\"wdth\" 125.50, 'wght' 900, 'wdth' 100"),
            Some(vec![pair("wdth", 100.0), pair("wght", 900.0)])
        );
        assert_eq!(var("'wg t' 1"), Some(vec![pair("wg t", 1.0)]));
        assert_eq!(
            var("'wght' calc(400 + 500)"),
            Some(vec![pair("wght", 900.0)])
        );
        assert_eq!(var("'wght' 9e2"), Some(vec![pair("wght", 900.0)]));
        assert_eq!(var("'wght' +900"), Some(vec![pair("wght", 900.0)]));
        // Uppercase sorts before lowercase.
        assert_eq!(
            var("'wght' 1, 'SWBX' 2"),
            Some(vec![pair("SWBX", 2.0), pair("wght", 1.0)])
        );
    }

    #[test]
    fn variation_values_are_finite() {
        assert_eq!(var("'wght' 1e400"), Some(vec![pair("wght", f32::MAX)]));
        assert_eq!(var("'wght' -1e400"), Some(vec![pair("wght", f32::MIN)]));
        assert_eq!(var("'wght' 1e10"), Some(vec![pair("wght", 1e10)]));
    }

    #[test]
    fn invalid_variation_settings() {
        for css in [
            "",
            "'wght'",
            "wght 900",
            "'wght' 900px",
            "'wght' 90%",
            "'wgh' 900",
            "'wghtt' 900",
            "'' 900",
            "'wgh\\e9' 900",
            "'wgh\\7f' 900",
            "'wght' 900,",
            "'wght' 900 'wdth' 125",
            "normal, 'wght' 900",
            "'wght' 900, 'ab'",
            "none",
        ] {
            assert_eq!(var(css), None, "{css}");
        }
    }

    #[test]
    fn feature_values() {
        assert_eq!(feat("normal"), Some(vec![]));
        assert_eq!(feat("'liga' 0"), Some(vec![("liga".into(), 0)]));
        assert_eq!(feat("'liga'"), Some(vec![("liga".into(), 1)]));
        assert_eq!(feat("'liga' on"), Some(vec![("liga".into(), 1)]));
        assert_eq!(feat("'liga' OFF"), Some(vec![("liga".into(), 0)]));
        assert_eq!(
            feat("'liga' on, 'kern' off, 'salt' 3, 'liga' 0"),
            Some(vec![
                ("kern".into(), 0),
                ("liga".into(), 0),
                ("salt".into(), 3)
            ])
        );
        assert_eq!(feat("'liga' -1"), Some(vec![("liga".into(), -1)]));
        assert_eq!(feat("'liga' calc(1 + 1)"), Some(vec![("liga".into(), 2)]));
        assert_eq!(
            feat("'liga' 99999999999"),
            Some(vec![("liga".into(), i32::MAX)])
        );
        assert_eq!(
            feat("'liga' -99999999999"),
            Some(vec![("liga".into(), i32::MIN)])
        );
        assert_eq!(feat("'LIGA' 1"), Some(vec![("LIGA".into(), 1)]));
    }

    #[test]
    fn invalid_feature_settings() {
        for css in [
            "",
            "liga 1",
            "'ligaa' 1",
            "'liga' 1.5",
            "'liga' 1px",
            "'liga' 1 'kern' 0",
            "'liga' 1,",
            "'liga' maybe",
            "'lig' 1",
        ] {
            assert_eq!(feat(css), None, "{css}");
        }
    }

    #[test]
    fn long_lists_are_limited() {
        let css: Vec<String> = (0..1000).map(|i| format!("'a{i:03}' {i}")).collect();
        let list = var(&css.join(",")).unwrap();
        assert_eq!(list.len(), MAX_SETTINGS);
        // The first entries in source order stay; a repeated tag still
        // updates its value.
        assert_eq!(list[0], pair("a000", 0.0));
        let mut css = css[..MAX_SETTINGS].join(",");
        css.push_str(",'a000' 5, 'zzzz' 1");
        let list = var(&css).unwrap();
        assert_eq!(list.len(), MAX_SETTINGS);
        assert_eq!(list[0], pair("a000", 5.0));
    }
}
