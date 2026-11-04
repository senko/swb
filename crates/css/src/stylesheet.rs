//! The stylesheet model: rules and declarations.
//!
//! The parser ([`crate::parse_stylesheet`]) builds these types. Property
//! values stay as component values; the style crate parses them.
//!
//! Supported rules: style rules, `@media`, `@supports`, `@import`,
//! `@font-face` and `@keyframes` (kept for the CSSOM; swb runs no
//! animations). The rules of `@layer` blocks join the parent rule list
//! (layer order is ignored). `@charset` and `@layer` statements are
//! ignored. All other at-rules (`@page`, `@namespace`, `@counter-style`,
//! `@property`, `@container`, `@-webkit-keyframes` and unknown ones) are
//! dropped.

use crate::cursor::Parser;
use crate::media::MediaQueryList;
use crate::selector::SelectorList;
use crate::supports::{SupportsCondition, parse_condition_or_declaration};
use crate::values::ComponentValue;

/// A parsed stylesheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stylesheet {
    /// The top-level rules, in source order.
    pub rules: Vec<CssRule>,
}

/// A rule.
#[derive(Clone, Debug, PartialEq)]
pub enum CssRule {
    /// A style rule: `selectors { declarations }`.
    Style(StyleRule),
    /// `@media`.
    Media(MediaRule),
    /// `@import`.
    Import(ImportRule),
    /// `@font-face`.
    FontFace(FontFaceRule),
    /// `@supports`.
    Supports(SupportsRule),
    /// `@keyframes`.
    Keyframes(KeyframesRule),
}

/// A style rule.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleRule {
    /// The selectors. A rule whose selector list does not parse is dropped
    /// during parsing, so this list is valid.
    pub selectors: SelectorList,
    /// The declarations, in source order.
    pub declarations: Vec<Declaration>,
}

/// A declaration: `name: value [!important]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    /// The property name. ASCII-lowercased, except for custom properties
    /// (`--foo`), which keep their case.
    pub name: String,
    /// The value, without leading and trailing whitespace and without
    /// `!important`.
    pub value: Vec<ComponentValue>,
    /// True if the declaration ends with `!important`.
    pub important: bool,
}

/// An `@media` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaRule {
    /// The media query list.
    pub media: MediaQueryList,
    /// The rules inside the block.
    pub rules: Vec<CssRule>,
}

/// An `@import` rule.
///
/// `layer(...)` in the prelude is ignored. Load the stylesheet only if
/// `supports` (if any) is true and `media` matches.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportRule {
    /// The URL, as written (not resolved against a base URL).
    pub url: String,
    /// The condition of `supports(...)`, if the prelude has one. A bare
    /// declaration (`supports(display: grid)`) becomes
    /// [`SupportsCondition::Declaration`].
    pub supports: Option<SupportsCondition>,
    /// The media query list. Empty if the prelude has none.
    pub media: MediaQueryList,
}

impl ImportRule {
    /// Parses the prelude of an `@import` rule.
    /// <https://www.w3.org/TR/css-cascade-5/#at-import>
    pub(crate) fn parse(prelude: &[ComponentValue]) -> Option<ImportRule> {
        let mut p = Parser::new(prelude);
        let url = p.expect_url_or_string().ok()?.to_owned();
        let _ = p.try_parse(|p| match p.next() {
            Some(v) if v.is_ident("layer") || v.is_function("layer") => Ok(()),
            _ => Err(()),
        });
        let supports = p
            .expect_function_matching("supports")
            .ok()
            .map(|args| parse_condition_or_declaration(args.remaining()));
        Some(ImportRule {
            url,
            supports,
            media: MediaQueryList::parse(p.remaining()),
        })
    }
}

/// An `@font-face` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFaceRule {
    /// The descriptors, in source order. `unicode-range` values are
    /// [`ComponentValue::UnicodeRange`] values when they parse.
    pub declarations: Vec<Declaration>,
}

/// An `@supports` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct SupportsRule {
    /// The condition.
    pub condition: SupportsCondition,
    /// The condition as written, serialized from its component values
    /// without the surrounding whitespace (CSSOM `conditionText`: Chromium
    /// keeps the text, `(a:b)` stays without a space).
    pub text: String,
    /// The rules inside the block.
    pub rules: Vec<CssRule>,
}

/// An `@keyframes` rule (CSS Animations 1 §4). swb runs no animations; the
/// rule exists for the CSSOM (`CSSKeyframesRule`).
#[derive(Clone, Debug, PartialEq)]
pub struct KeyframesRule {
    /// The name: an identifier or a string.
    pub name: String,
    /// The keyframes that have valid selectors, in source order.
    pub keyframes: Vec<Keyframe>,
}

/// One keyframe of an `@keyframes` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    /// The offsets in percent (`from` is 0, `to` 100), in source order.
    pub keys: Vec<f32>,
    /// The declarations.
    pub declarations: Vec<Declaration>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_prelude() {
        let rule = ImportRule::parse(&crate::parse_component_values("url(\"x.css\") layer print"))
            .expect("valid @import");
        assert_eq!(rule.url, "x.css");
        assert!(!rule.media.is_empty());
        assert_eq!(rule.supports, None);
        let rule = ImportRule::parse(&crate::parse_component_values(
            "'a.css' supports(display: grid) screen",
        ))
        .expect("valid @import");
        let Some(SupportsCondition::Declaration(d)) = &rule.supports else {
            panic!("expected a declaration condition");
        };
        assert_eq!(d.name, "display");
        assert!(!rule.media.is_empty());
        let rule = ImportRule::parse(&crate::parse_component_values(
            "'a.css' supports((display: grid) and (not (x: y)))",
        ))
        .expect("valid @import");
        assert!(matches!(rule.supports, Some(SupportsCondition::And(_))));
        assert!(ImportRule::parse(&crate::parse_component_values("foo")).is_none());
    }
}
