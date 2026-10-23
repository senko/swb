//! Reflected IDL attributes (HTML §2.6.1,
//! <https://html.spec.whatwg.org/multipage/common-dom-interfaces.html#reflecting-content-attributes-in-idl-attributes>):
//! the getter and setter steps of `[Reflect]`, `[ReflectURL]`,
//! `[ReflectNonNegative]`, `[ReflectPositive]`,
//! `[ReflectPositiveWithFallback]`, `[ReflectRange]` and
//! `[ReflectDefault]` for `DOMString`, `DOMString?`, `USVString`,
//! `boolean`, `long` and `unsigned long`, and the enumerated attributes
//! that are limited to only known values.
//!
//! HTML describes the keywords of enumerated attributes in prose; swb's
//! IDL files give them with extended attributes of its own
//! (`[SwbKeywords="ltr rtl auto", SwbMissing="", SwbInvalid=""]`, see the
//! generator), which become a [`Keywords`] table. The content attributes
//! are read and written through [`Host`].

use super::{Cx, Exception, Host, NodeId, Result};
use swb_dom::{parse_integer, parse_non_negative_integer};

/// The keywords of an enumerated attribute that is limited to only known
/// values.
#[derive(Clone, Copy, Debug)]
pub struct Keywords {
    /// Each keyword (ASCII lowercase) and the canonical keyword of its
    /// state.
    pub keywords: &'static [(&'static str, &'static str)],
    /// The canonical keyword of the missing value default (`None`: no
    /// state, the getter returns the empty string or null).
    pub missing: Option<&'static str>,
    /// The canonical keyword of the invalid value default (`None`: the
    /// empty string or null).
    pub invalid: Option<&'static str>,
}

impl Keywords {
    /// The canonical keyword of a content attribute value, or `None` for a
    /// state without a keyword.
    fn canonical(&self, value: Option<&str>) -> Option<&'static str> {
        let Some(value) = value else {
            return self.missing;
        };
        self.keywords
            .iter()
            .find(|(keyword, _)| keyword.eq_ignore_ascii_case(value))
            .map(|&(_, canonical)| canonical)
            .or(self.invalid)
    }
}

/// `DOMString`: the value, or the empty string.
pub fn get_string<H: Host>(cx: &mut Cx<'_>, this: NodeId, name: &str) -> Result<String> {
    Ok(H::reflect_get(cx, this, name)?.unwrap_or_default())
}

/// `DOMString` and `USVString` setter (also `DOMString?` with a value).
pub fn set_string<H: Host>(cx: &mut Cx<'_>, this: NodeId, name: &str, value: &str) -> Result<()> {
    H::reflect_set(cx, this, name, value)
}

/// `DOMString?`: the value, or null.
pub fn get_nullable_string<H: Host>(
    cx: &mut Cx<'_>,
    this: NodeId,
    name: &str,
) -> Result<Option<String>> {
    H::reflect_get(cx, this, name)
}

/// `DOMString?` setter: null removes the attribute.
pub fn set_nullable_string<H: Host>(
    cx: &mut Cx<'_>,
    this: NodeId,
    name: &str,
    value: Option<&str>,
) -> Result<()> {
    match value {
        Some(value) => H::reflect_set(cx, this, name, value),
        None => H::reflect_remove(cx, this, name),
    }
}

/// An enumerated `DOMString` limited to only known values.
pub fn get_enumerated<H: Host>(
    cx: &mut Cx<'_>,
    this: NodeId,
    name: &str,
    keywords: &Keywords,
) -> Result<String> {
    let value = H::reflect_get(cx, this, name)?;
    Ok(keywords
        .canonical(value.as_deref())
        .unwrap_or_default()
        .to_owned())
}

/// An enumerated `DOMString?` limited to only known values: null for a
/// state without a keyword.
pub fn get_nullable_enumerated<H: Host>(
    cx: &mut Cx<'_>,
    this: NodeId,
    name: &str,
    keywords: &Keywords,
) -> Result<Option<String>> {
    let value = H::reflect_get(cx, this, name)?;
    Ok(keywords.canonical(value.as_deref()).map(str::to_owned))
}

/// `[ReflectURL] USVString`: the value parsed relative to the document's
/// base URL, or the value itself if it does not parse; the empty string
/// without the attribute.
pub fn get_url<H: Host>(cx: &mut Cx<'_>, this: NodeId, name: &str) -> Result<String> {
    let Some(value) = H::reflect_get(cx, this, name)? else {
        return Ok(String::new());
    };
    Ok(H::reflect_url(cx, this, &value).unwrap_or(value))
}

/// `boolean`: whether the attribute is present.
pub fn get_boolean<H: Host>(cx: &mut Cx<'_>, this: NodeId, name: &str) -> Result<bool> {
    Ok(H::reflect_get(cx, this, name)?.is_some())
}

/// `boolean` setter: present (empty) for true, removed for false.
pub fn set_boolean<H: Host>(cx: &mut Cx<'_>, this: NodeId, name: &str, value: bool) -> Result<()> {
    if value {
        H::reflect_set(cx, this, name, "")
    } else {
        H::reflect_remove(cx, this, name)
    }
}

/// `long`, optionally limited to only non-negative numbers, with an
/// optional default.
pub fn get_long<H: Host>(
    cx: &mut Cx<'_>,
    this: NodeId,
    name: &str,
    non_negative: bool,
    default: Option<i32>,
) -> Result<i32> {
    if let Some(value) = H::reflect_get(cx, this, name)? {
        let parsed = if non_negative {
            parse_non_negative_integer(&value)
        } else {
            parse_integer(&value)
        };
        if let Some(parsed) = parsed.and_then(|v| i32::try_from(v).ok()) {
            return Ok(parsed);
        }
    }
    Ok(default.unwrap_or(if non_negative { -1 } else { 0 }))
}

/// `long` setter.
pub fn set_long<H: Host>(
    cx: &mut Cx<'_>,
    this: NodeId,
    name: &str,
    non_negative: bool,
    value: i32,
) -> Result<()> {
    if non_negative && value < 0 {
        return Err(Exception::dom(
            "IndexSizeError",
            format!("The value provided ({value}) is not positive or 0."),
        ));
    }
    H::reflect_set(cx, this, name, &value.to_string())
}

/// The variants of `unsigned long` reflection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsignedLimit {
    /// `[Reflect]`: 0 to 2147483647.
    None,
    /// `[ReflectPositive]`: 1 to 2147483647; 0 throws on setting.
    Positive,
    /// `[ReflectPositiveWithFallback]`: 1 to 2147483647; 0 sets the
    /// default.
    PositiveWithFallback,
    /// `[ReflectRange=(min, max)]`: clamped to the range on getting.
    Range(u32, u32),
}

/// `unsigned long`.
pub fn get_unsigned_long<H: Host>(
    cx: &mut Cx<'_>,
    this: NodeId,
    name: &str,
    limit: UnsignedLimit,
    default: Option<u32>,
) -> Result<u32> {
    let (minimum, maximum) = match limit {
        UnsignedLimit::None => (0, 2_147_483_647),
        UnsignedLimit::Positive | UnsignedLimit::PositiveWithFallback => (1, 2_147_483_647),
        UnsignedLimit::Range(min, max) => (min, max),
    };
    if let Some(value) = H::reflect_get(cx, this, name)?
        && let Some(parsed) = parse_non_negative_integer(&value)
    {
        if parsed >= i64::from(minimum) && parsed <= i64::from(maximum) {
            return Ok(parsed as u32);
        }
        if let UnsignedLimit::Range(min, max) = limit {
            return Ok(if parsed < i64::from(min) { min } else { max });
        }
    }
    Ok(default.unwrap_or(minimum))
}

/// `unsigned long` setter.
pub fn set_unsigned_long<H: Host>(
    cx: &mut Cx<'_>,
    this: NodeId,
    name: &str,
    limit: UnsignedLimit,
    default: Option<u32>,
    value: u32,
) -> Result<()> {
    if limit == UnsignedLimit::Positive && value == 0 {
        return Err(Exception::dom(
            "IndexSizeError",
            "The value provided is 0, which is an invalid size.",
        ));
    }
    let minimum = match limit {
        UnsignedLimit::Positive | UnsignedLimit::PositiveWithFallback => 1,
        UnsignedLimit::None | UnsignedLimit::Range(..) => 0,
    };
    let mut new_value = default.unwrap_or(minimum);
    if value >= minimum && value <= 2_147_483_647 {
        new_value = value;
    }
    H::reflect_set(cx, this, name, &new_value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_states() {
        let cross_origin = Keywords {
            keywords: &[
                ("", "anonymous"),
                ("anonymous", "anonymous"),
                ("use-credentials", "use-credentials"),
            ],
            missing: None,
            invalid: Some("anonymous"),
        };
        assert_eq!(cross_origin.canonical(None), None);
        assert_eq!(cross_origin.canonical(Some("")), Some("anonymous"));
        assert_eq!(
            cross_origin.canonical(Some("USE-credentials")),
            Some("use-credentials")
        );
        assert_eq!(cross_origin.canonical(Some("xyz")), Some("anonymous"));
        let dir = Keywords {
            keywords: &[("ltr", "ltr"), ("rtl", "rtl"), ("auto", "auto")],
            missing: None,
            invalid: None,
        };
        assert_eq!(dir.canonical(Some("RTL")), Some("rtl"));
        assert_eq!(dir.canonical(Some("x")), None);
    }
}
