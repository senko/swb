//! The stylesheet model: rules and declarations.
//!
//! The parser ([`crate::parse_stylesheet`]) builds these types. Property
//! values stay as component values; the style crate parses them.
//!
//! Supported rules: style rules, `@media`, `@supports`, `@import` and
//! `@font-face`. The rules of `@layer` blocks join the parent rule list
//! (layer order is ignored). `@charset` and `@layer` statements are
//! ignored. All other at-rules (`@keyframes`, `@page`, `@namespace`,
//! `@counter-style`, `@property`, `@container` and unknown ones) are
//! dropped.

use crate::cursor::Parser;
use crate::media::{MediaEnvironment, MediaQueryList};
use crate::parser::parse_declaration_from_values;
use crate::selector::SelectorList;
use crate::supports::SupportsCondition;
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

impl Declaration {
    /// True if this declares a custom property (`--name`).
    pub fn is_custom_property(&self) -> bool {
        self.name.starts_with("--")
    }

    /// A cursor over the value.
    pub fn parser(&self) -> Parser<'_> {
        Parser::new(&self.value)
    }
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
        let supports = p.expect_function_matching("supports").ok().map(|args| {
            let args = args.remaining();
            SupportsCondition::parse(args)
                .ok()
                .or_else(|| parse_declaration_from_values(args).map(SupportsCondition::Declaration))
                .unwrap_or(SupportsCondition::Unknown)
        });
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
    /// The rules inside the block.
    pub rules: Vec<CssRule>,
}

impl Stylesheet {
    /// Parses a stylesheet. Same as [`crate::parse_stylesheet`].
    pub fn parse(css: &str) -> Stylesheet {
        crate::parser::parse_stylesheet(css)
    }

    /// Calls `f` for each style rule that applies, in source order.
    ///
    /// Descends into `@media` rules whose media list matches `env` and into
    /// `@supports` rules whose condition is true. `supports_declaration`
    /// tells whether a property declaration is supported (see
    /// [`SupportsCondition::evaluate`]). Does not follow `@import`.
    pub fn for_each_style_rule<'a>(
        &'a self,
        env: &MediaEnvironment,
        supports_declaration: &dyn Fn(&Declaration) -> bool,
        f: &mut dyn FnMut(&'a StyleRule),
    ) {
        visit_rules(&self.rules, env, supports_declaration, f);
    }
}

fn visit_rules<'a>(
    rules: &'a [CssRule],
    env: &MediaEnvironment,
    supports_declaration: &dyn Fn(&Declaration) -> bool,
    f: &mut dyn FnMut(&'a StyleRule),
) {
    for rule in rules {
        match rule {
            CssRule::Style(style) => f(style),
            CssRule::Media(media) => {
                if media.media.matches(env) {
                    visit_rules(&media.rules, env, supports_declaration, f);
                }
            }
            CssRule::Supports(supports) => {
                if supports.condition.evaluate(supports_declaration) {
                    visit_rules(&supports.rules, env, supports_declaration, f);
                }
            }
            CssRule::Import(_) | CssRule::FontFace(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MediaType;

    #[test]
    fn effective_style_rules() {
        let sheet = Stylesheet::parse(
            "a { x: y } @media print { b { x: y } } @media screen { c { x: y } } \
             @supports (display: grid) { d { x: y } } @supports (display: nope) { e { x: y } }",
        );
        let env = MediaEnvironment {
            media_type: MediaType::Screen,
            ..MediaEnvironment::default()
        };
        let supported = |d: &Declaration| d.name == "display" && d.value[0].is_ident("grid");
        let mut seen = Vec::new();
        sheet.for_each_style_rule(&env, &supported, &mut |rule| {
            seen.push(rule.selectors.to_string());
        });
        assert_eq!(seen, vec!["a", "c", "d"]);
    }

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

    #[test]
    fn declaration_helpers() {
        let decls = crate::parse_style_attribute("--X: 1; width: 10px");
        assert!(decls[0].is_custom_property());
        assert!(!decls[1].is_custom_property());
        let mut p = decls[1].parser();
        assert_eq!(p.expect_dimension(), Ok((10.0, "px")));
    }
}
