//! The stylesheet model: rules and declarations.
//!
//! The parser ([`crate::parse_stylesheet`]) builds these types. Property
//! values stay as component values; the style crate parses them.
//!
//! Supported rules: style rules, `@media`, `@supports`, `@import`,
//! `@font-face`, `@keyframes` (kept for the CSSOM; swb runs no animations)
//! and the cascade layer rules: `@layer` blocks ([`LayerBlockRule`], named
//! or anonymous, with the rules inside) and `@layer` statements
//! ([`LayerStatementRule`]); `@import` keeps its `layer` and `layer(name)`.
//! The model keeps the layer structure; the style crate's stylist decides
//! the order (CSS Cascade 5, <https://www.w3.org/TR/css-cascade-5/#layering>).
//! `@charset` is ignored. All other at-rules (`@page`, `@namespace`,
//! `@counter-style`, `@property`, `@container`, `@-webkit-keyframes` and
//! unknown ones) are dropped.

use std::fmt;

use crate::cursor::Parser;
use crate::media::MediaQueryList;
use crate::selector::SelectorList;
use crate::serialize::{serialize_component_values, serialize_identifier};
use crate::supports::{SupportsCondition, parse_condition_or_declaration};
use crate::values::{ComponentValue, trim_whitespace};

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
    /// `@layer name { ... }` and `@layer { ... }`.
    LayerBlock(LayerBlockRule),
    /// `@layer a, b.c;`.
    LayerStatement(LayerStatementRule),
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

/// A cascade layer name: identifiers joined by `.`, such as `a.b.c`
/// (a sublayer `c` of `b` of `a`).
/// <https://www.w3.org/TR/css-cascade-5/#typedef-layer-name>
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LayerName {
    segments: Vec<Box<str>>,
}

impl LayerName {
    /// Parses a layer name from component values without surrounding
    /// whitespace: an identifier, then any number of `.` and identifier
    /// with nothing between them (not even whitespace). Comments are gone
    /// after tokenization, so `a/**/.b` is the name `a.b`. CSS-wide
    /// keywords are valid names (measured in Chromium 148).
    pub fn parse(values: &[ComponentValue]) -> Option<LayerName> {
        let mut segments = Vec::new();
        let mut iter = values.iter();
        loop {
            match iter.next()? {
                ComponentValue::Ident(segment) => segments.push(segment.clone()),
                _ => return None,
            }
            match iter.next() {
                None => return Some(LayerName { segments }),
                Some(v) if v.is_delim('.') => {}
                Some(_) => return None,
            }
        }
    }

    /// The identifiers, outermost layer first. Never empty.
    pub fn segments(&self) -> &[Box<str>] {
        &self.segments
    }
}

impl fmt::Display for LayerName {
    /// The CSSOM text: the identifiers serialized and joined with `.`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            let mut text = String::new();
            serialize_identifier(segment, &mut text);
            f.write_str(&text)?;
        }
        Ok(())
    }
}

/// An `@layer` block rule: `@layer name { rules }`, or an anonymous layer
/// `@layer { rules }`.
/// <https://www.w3.org/TR/css-cascade-5/#layer-block>
#[derive(Clone, Debug, PartialEq)]
pub struct LayerBlockRule {
    /// The name; `None` for an anonymous layer.
    pub name: Option<LayerName>,
    /// The rules inside the block.
    pub rules: Vec<CssRule>,
}

/// An `@layer` statement rule: `@layer a, b.c;`.
/// <https://www.w3.org/TR/css-cascade-5/#layer-empty>
#[derive(Clone, Debug, PartialEq)]
pub struct LayerStatementRule {
    /// The names, in source order. Never empty.
    pub names: Vec<LayerName>,
}

/// The `layer` part of an `@import` rule.
#[derive(Clone, Debug, PartialEq)]
pub enum ImportLayer {
    /// `layer`: an anonymous layer.
    Anonymous,
    /// `layer(name)`.
    Named(LayerName),
}

/// An `@import` rule.
///
/// swb does not load the imported sheet, but the rule still declares its
/// layer (Chromium does the same for a sheet that fails to load). Load the
/// stylesheet only if `supports` (if any) is true and `media` matches.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportRule {
    /// The URL, as written (not resolved against a base URL).
    pub url: String,
    /// The layer of the imported rules, if the prelude has `layer` or
    /// `layer(name)`. A `layer(...)` with an invalid name is not a layer:
    /// it stays in the media query list, which then matches nothing.
    pub layer: Option<ImportLayer>,
    /// The condition of `supports(...)`, if the prelude has one. A bare
    /// declaration (`supports(display: grid)`) becomes
    /// [`SupportsCondition::Declaration`].
    pub supports: Option<SupportsCondition>,
    /// The text inside `supports(...)`, serialized from its component
    /// values (CSSOM `supportsText`).
    pub supports_text: Option<String>,
    /// The media query list. Empty if the prelude has none.
    pub media: MediaQueryList,
}

impl ImportRule {
    /// Parses the prelude of an `@import` rule.
    /// <https://www.w3.org/TR/css-cascade-5/#at-import>
    pub(crate) fn parse(prelude: &[ComponentValue]) -> Option<ImportRule> {
        let mut p = Parser::new(prelude);
        let url = p.expect_url_or_string().ok()?.to_owned();
        let layer = p
            .try_parse(|p| match p.next() {
                Some(v) if v.is_ident("layer") => Ok(ImportLayer::Anonymous),
                Some(v) => v
                    .as_function()
                    .filter(|f| f.name.eq_ignore_ascii_case("layer"))
                    .and_then(|f| LayerName::parse(trim_whitespace(&f.arguments)))
                    .map(ImportLayer::Named)
                    .ok_or(()),
                None => Err(()),
            })
            .ok();
        let supports_args = p.expect_function_matching("supports").ok();
        let supports = supports_args.map(|args| parse_condition_or_declaration(args.remaining()));
        let supports_text =
            supports_args.map(|args| serialize_component_values(trim_whitespace(args.remaining())));
        Some(ImportRule {
            url,
            layer,
            supports,
            supports_text,
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
