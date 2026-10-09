//! Property declarations: parsing declarations into longhand values, and
//! computing them.
//!
//! A declaration parses into one [`PropertyDeclaration`] per longhand.
//! Shorthands expand at parse time. Values that contain `var()` or `env()`
//! stay unparsed until computed-value time
//! (<https://www.w3.org/TR/css-variables-1/#variables-in-shorthands>).
//! CSS-wide keywords apply to every longhand of a shorthand.
//!
//! Unknown and unsupported properties are dropped (logged at trace level:
//! real pages have hundreds of them).

pub(crate) mod compute;
pub(crate) mod ids;
pub(crate) mod longhand;
pub(crate) mod mask;
pub(crate) mod shorthand;
pub(crate) mod specified;
pub(crate) mod svg;
pub(crate) mod transform;

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use log::trace;
use swb_css::{ComponentValue, Declaration, Parser, contains_function, trim_whitespace};

pub(crate) use ids::LonghandId;
use ids::LonghandValue;
use shorthand::ShorthandId;

use crate::parse::{ParseResult, ParserContext, is_custom_property_name};

/// A CSS-wide keyword.
/// <https://www.w3.org/TR/css-cascade-5/#defaulting-keywords>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CssWideKeyword {
    /// `initial`.
    Initial,
    /// `inherit`.
    Inherit,
    /// `unset`.
    Unset,
    /// `revert`, and `revert-layer` (swb flattens cascade layers, so
    /// rolling back a layer means rolling back to the previous origin). In
    /// the author origin it rolls back to the user-agent origin; in the
    /// user-agent origin it behaves as `unset`.
    Revert,
}

impl CssWideKeyword {
    /// The keyword if `value` consists of exactly one CSS-wide keyword.
    pub(crate) fn from_value(value: &[ComponentValue]) -> Option<Self> {
        match trim_whitespace(value) {
            [ComponentValue::Ident(ident)] => Self::from_ident(ident),
            _ => None,
        }
    }

    /// Parses a CSS-wide keyword (ASCII case-insensitive).
    pub(crate) fn from_ident(ident: &str) -> Option<Self> {
        match ident.to_ascii_lowercase().as_str() {
            "initial" => Some(CssWideKeyword::Initial),
            "inherit" => Some(CssWideKeyword::Inherit),
            "unset" => Some(CssWideKeyword::Unset),
            "revert" | "revert-layer" => Some(CssWideKeyword::Revert),
            _ => None,
        }
    }
}

/// A property that a declaration names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PropertyId {
    /// A longhand.
    Longhand(LonghandId),
    /// A shorthand.
    Shorthand(ShorthandId),
    /// The `all` shorthand.
    All,
}

impl PropertyId {
    /// The longhands that the property sets.
    pub(crate) fn longhands(self) -> Cow<'static, [LonghandId]> {
        match self {
            PropertyId::Longhand(id) => Cow::Owned(vec![id]),
            PropertyId::Shorthand(s) => Cow::Borrowed(s.longhands()),
            // `all` excludes `direction` and `unicode-bidi`.
            // <https://www.w3.org/TR/css-cascade-4/#all-shorthand>
            PropertyId::All => Cow::Owned(
                LonghandId::ALL
                    .iter()
                    .copied()
                    .filter(|id| !matches!(id, LonghandId::Direction | LonghandId::UnicodeBidi))
                    .collect(),
            ),
        }
    }
}

/// A declaration value that contains `var()` or `env()`, kept as written.
#[derive(Debug, PartialEq)]
pub(crate) struct UnparsedValue {
    /// The property as declared.
    pub(crate) property: PropertyId,
    /// The value.
    pub(crate) tokens: Vec<ComponentValue>,
    /// The context to parse the value with after substitution.
    pub(crate) context: ParserContext,
}

/// The value of a custom property declaration.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CustomDeclaration {
    /// A value (component values, trimmed).
    Value(Arc<[ComponentValue]>),
    /// A CSS-wide keyword.
    CssWide(CssWideKeyword),
}

/// A parsed declaration of one longhand or custom property.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PropertyDeclaration {
    /// A specified value.
    Value(LonghandValue),
    /// A CSS-wide keyword.
    CssWide(LonghandId, CssWideKeyword),
    /// A value with `var()` references, substituted at computed-value time.
    WithVariables(LonghandId, Arc<UnparsedValue>),
    /// A custom property.
    Custom(Arc<str>, CustomDeclaration),
}

impl PropertyDeclaration {
    /// The longhand this declaration sets, or `None` for a custom property.
    pub(crate) fn longhand(&self) -> Option<LonghandId> {
        match self {
            PropertyDeclaration::Value(v) => Some(v.id()),
            PropertyDeclaration::CssWide(id, _) | PropertyDeclaration::WithVariables(id, _) => {
                Some(*id)
            }
            PropertyDeclaration::Custom(..) => None,
        }
    }
}

/// The declarations of a style rule or `style` attribute, split by
/// importance. Within each list, a later declaration wins over an earlier
/// one for the same property; duplicates are removed at parse time.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DeclarationBlock {
    /// Normal declarations.
    pub(crate) normal: Vec<PropertyDeclaration>,
    /// `!important` declarations.
    pub(crate) important: Vec<PropertyDeclaration>,
}

impl DeclarationBlock {
    /// Parses declarations. Invalid declarations are dropped.
    pub(crate) fn parse(declarations: &[Declaration], cx: &ParserContext) -> Self {
        let mut block = DeclarationBlock::default();
        for d in declarations {
            let target = if d.important {
                &mut block.important
            } else {
                &mut block.normal
            };
            parse_declaration(&d.name, &d.value, cx, target);
        }
        dedup(&mut block.normal);
        dedup(&mut block.important);
        block
    }

    /// True if the block has no declarations.
    pub(crate) fn is_empty(&self) -> bool {
        self.normal.is_empty() && self.important.is_empty()
    }
}

/// Removes declarations that a later declaration of the same property
/// overrides.
fn dedup(list: &mut Vec<PropertyDeclaration>) {
    if list.len() < 2 {
        return;
    }
    let mut seen = [false; LonghandId::COUNT];
    let mut seen_custom: HashSet<Arc<str>> = HashSet::new();
    let mut keep = vec![true; list.len()];
    for (i, decl) in list.iter().enumerate().rev() {
        let duplicate = match decl {
            PropertyDeclaration::Custom(name, _) => !seen_custom.insert(Arc::clone(name)),
            other => {
                let index = other.longhand().map_or(0, |id| id as usize);
                std::mem::replace(&mut seen[index], true)
            }
        };
        keep[i] = !duplicate;
    }
    let mut keep = keep.into_iter();
    list.retain(|_| keep.next().unwrap_or(true));
}

/// Longhands with another name: legacy and vendor-prefixed aliases, and
/// logical properties (mapped for horizontal, left-to-right text).
fn longhand_alias(name: &str) -> Option<LonghandId> {
    use LonghandId as L;
    Some(match name {
        "word-wrap" => L::OverflowWrap,
        "font-width" => L::FontStretch,
        "-webkit-box-sizing" | "-moz-box-sizing" => L::BoxSizing,
        "-webkit-user-select" | "-moz-user-select" | "-ms-user-select" => L::UserSelect,
        "-webkit-hyphens" | "-ms-hyphens" | "-moz-hyphens" => L::Hyphens,
        "-webkit-flex-direction" => L::FlexDirection,
        "-webkit-flex-wrap" => L::FlexWrap,
        "-webkit-flex-grow" => L::FlexGrow,
        "-webkit-flex-shrink" => L::FlexShrink,
        "-webkit-flex-basis" => L::FlexBasis,
        "-webkit-order" => L::Order,
        "-webkit-justify-content" => L::JustifyContent,
        "-webkit-align-items" => L::AlignItems,
        "-webkit-align-self" => L::AlignSelf,
        "-webkit-align-content" => L::AlignContent,
        "grid-row-gap" => L::RowGap,
        "grid-column-gap" => L::ColumnGap,
        "margin-block-start" => L::MarginTop,
        "margin-block-end" => L::MarginBottom,
        "margin-inline-start" => L::MarginLeft,
        "margin-inline-end" => L::MarginRight,
        "padding-block-start" => L::PaddingTop,
        "padding-block-end" => L::PaddingBottom,
        "padding-inline-start" => L::PaddingLeft,
        "padding-inline-end" => L::PaddingRight,
        "inset-block-start" => L::Top,
        "inset-block-end" => L::Bottom,
        "inset-inline-start" => L::Left,
        "inset-inline-end" => L::Right,
        "border-block-start-width" => L::BorderTopWidth,
        "border-block-end-width" => L::BorderBottomWidth,
        "border-inline-start-width" => L::BorderLeftWidth,
        "border-inline-end-width" => L::BorderRightWidth,
        "border-block-start-style" => L::BorderTopStyle,
        "border-block-end-style" => L::BorderBottomStyle,
        "border-inline-start-style" => L::BorderLeftStyle,
        "border-inline-end-style" => L::BorderRightStyle,
        "border-block-start-color" => L::BorderTopColor,
        "border-block-end-color" => L::BorderBottomColor,
        "border-inline-start-color" => L::BorderLeftColor,
        "border-inline-end-color" => L::BorderRightColor,
        "border-start-start-radius" | "-webkit-border-top-left-radius" => L::BorderTopLeftRadius,
        "border-start-end-radius" | "-webkit-border-top-right-radius" => L::BorderTopRightRadius,
        "border-end-start-radius" | "-webkit-border-bottom-left-radius" => {
            L::BorderBottomLeftRadius
        }
        "border-end-end-radius" | "-webkit-border-bottom-right-radius" => {
            L::BorderBottomRightRadius
        }
        "inline-size" => L::Width,
        "block-size" => L::Height,
        "min-inline-size" => L::MinWidth,
        "min-block-size" => L::MinHeight,
        "max-inline-size" => L::MaxWidth,
        "max-block-size" => L::MaxHeight,
        "-webkit-mask-image" => L::MaskImage,
        "-webkit-mask-size" => L::MaskSize,
        "-webkit-mask-repeat" => L::MaskRepeat,
        "contain-intrinsic-inline-size" => L::ContainIntrinsicWidth,
        "contain-intrinsic-block-size" => L::ContainIntrinsicHeight,
        "overflow-inline" => L::OverflowX,
        "overflow-block" => L::OverflowY,
        "-webkit-transform" => L::Transform,
        "-webkit-transform-origin" => L::TransformOrigin,
        _ => return None,
    })
}

/// Properties that swb accepts (they are valid, for example in
/// `@supports`) but does not store.
const IGNORED_PROPERTIES: &[&str] = &[
    "text-decoration-thickness",
    "text-underline-offset",
    "text-underline-position",
    "text-decoration-skip-ink",
];

/// Looks up a property name (lowercase). The flag is true if the name is
/// the property's own name, false for aliases (the quirks mode unitless
/// length quirk applies only to the listed names).
fn lookup_property(name: &str) -> Option<(PropertyId, bool)> {
    // Internal longhands (set through shorthands) are not valid names.
    if name.starts_with("-swb-") {
        return None;
    }
    if name == "all" {
        return Some((PropertyId::All, true));
    }
    if let Some(id) = LonghandId::from_name(name) {
        return Some((PropertyId::Longhand(id), true));
    }
    if let Some(s) = ShorthandId::from_name(name) {
        return Some((PropertyId::Shorthand(s), true));
    }
    if let Some(id) = longhand_alias(name) {
        return Some((PropertyId::Longhand(id), false));
    }
    ShorthandId::from_alias(name).map(|s| (PropertyId::Shorthand(s), false))
}

/// True if a value contains `var()` or `env()` references.
pub(crate) fn has_references(value: &[ComponentValue]) -> bool {
    contains_function(value, "var") || contains_function(value, "env")
}

/// Parses one declaration and appends the result to `out`. Returns false
/// if the declaration is invalid or unsupported.
pub(crate) fn parse_declaration(
    name: &str,
    value: &[ComponentValue],
    cx: &ParserContext,
    out: &mut Vec<PropertyDeclaration>,
) -> bool {
    if is_custom_property_name(name) {
        let decl = match CssWideKeyword::from_value(value) {
            Some(k) => CustomDeclaration::CssWide(k),
            None => CustomDeclaration::Value(Arc::from(trim_whitespace(value))),
        };
        out.push(PropertyDeclaration::Custom(Arc::from(name), decl));
        return true;
    }
    let Some((property, canonical)) = lookup_property(name) else {
        if let Some(valid) = mask::ignored_property(name, value) {
            return valid;
        }
        if IGNORED_PROPERTIES.contains(&name) {
            return true;
        }
        trace!("unsupported property `{name}`");
        return false;
    };
    if let Some(keyword) = CssWideKeyword::from_value(value) {
        out.extend(
            property
                .longhands()
                .iter()
                .map(|&id| PropertyDeclaration::CssWide(id, keyword)),
        );
        return true;
    }
    if property == PropertyId::All {
        return false;
    }
    let cx = if canonical || !cx.quirks {
        Cow::Borrowed(cx)
    } else {
        Cow::Owned(ParserContext {
            quirks: false,
            ..cx.clone()
        })
    };
    if has_references(value) {
        let unparsed = Arc::new(UnparsedValue {
            property,
            tokens: value.to_vec(),
            context: cx.into_owned(),
        });
        out.extend(
            property
                .longhands()
                .iter()
                .map(|&id| PropertyDeclaration::WithVariables(id, Arc::clone(&unparsed))),
        );
        return true;
    }
    let mut values = Vec::new();
    if parse_property_value(property, value, &cx, &mut values).is_err() {
        trace!("invalid value for `{name}`");
        return false;
    }
    out.extend(values.into_iter().map(PropertyDeclaration::Value));
    true
}

/// Parses the value of a longhand or shorthand (without CSS-wide keywords
/// or `var()`) into longhand values.
pub(crate) fn parse_property_value(
    property: PropertyId,
    value: &[ComponentValue],
    cx: &ParserContext,
    out: &mut Vec<LonghandValue>,
) -> ParseResult<()> {
    let mut p = Parser::new(value);
    match property {
        PropertyId::Longhand(id) => {
            out.push(longhand::parse_longhand(id, &mut p, cx)?);
            Ok(())
        }
        PropertyId::Shorthand(s) => s.parse(&mut p, cx, out),
        PropertyId::All => Err(swb_css::ParseError::Invalid),
    }
}

/// True if swb supports the declaration, for `@supports`. A declaration
/// with `var()` is supported if the property is.
pub(crate) fn is_supported(declaration: &Declaration) -> bool {
    let cx = ParserContext::author(None, false);
    let mut out = Vec::new();
    parse_declaration(&declaration.name, &declaration.value, &cx, &mut out)
}

#[cfg(test)]
mod tests {
    //! Parsing and computing of property values, one declaration block at
    //! a time (without the cascade).

    use swb_css::parse_style_attribute;

    use super::compute::{ComputeContext, apply};
    use super::*;
    use crate::ComputedStyle;
    use crate::values::*;

    /// Parses `css` as a declaration list and computes it on top of the
    /// initial style (font properties first, as the cascade does).
    fn style_with(css: &str, quirks: bool) -> ComputedStyle {
        let cx = ParserContext::author(None, quirks);
        let block = DeclarationBlock::parse(&parse_style_attribute(css), &cx);
        let initial = ComputedStyle::initial();
        let mut s = (*initial).clone();
        let mut ccx = ComputeContext {
            parent: &initial,
            lengths: LengthContext::DEFAULT,
            element: None,
            quirks,
        };
        let is_font =
            |v: &LonghandValue| matches!(v.id(), LonghandId::FontFamily | LonghandId::FontSize);
        for decl in &block.normal {
            if let PropertyDeclaration::Value(v) = decl
                && is_font(v)
            {
                apply(v, &ccx, &mut s);
            }
        }
        ccx.lengths.font_size = s.font_size;
        for decl in &block.normal {
            if let PropertyDeclaration::Value(v) = decl
                && !is_font(v)
            {
                apply(v, &ccx, &mut s);
            }
        }
        s
    }

    fn style(css: &str) -> ComputedStyle {
        style_with(css, false)
    }

    fn valid(name: &str, value: &str) -> bool {
        let values = swb_css::parse_component_values(value);
        let mut out = Vec::new();
        parse_declaration(name, &values, &ParserContext::author(None, false), &mut out)
    }

    fn px(v: f32) -> LengthPercentage {
        LengthPercentage::Px(v)
    }

    fn lpa(v: f32) -> LengthPercentageOrAuto {
        LengthPercentageOrAuto::px(v)
    }

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::Rgba(Rgba::rgb(r, g, b))
    }

    #[test]
    fn display_values() {
        let cases: &[(&str, Display)] = &[
            ("block", Display::Block),
            ("INLINE-BLOCK", Display::InlineBlock),
            ("block flow", Display::Block),
            ("inline flow-root", Display::InlineBlock),
            ("block flex", Display::Flex),
            ("inline flex", Display::InlineFlex),
            ("flex inline", Display::InlineFlex),
            ("block grid", Display::Grid),
            ("inline table", Display::InlineTable),
            ("list-item", Display::ListItem),
            ("block list-item", Display::ListItem),
            ("inline list-item", Display::ListItem),
            ("list-item flow-root", Display::ListItem),
            ("ruby", Display::Inline),
            ("ruby-text", Display::Inline),
            ("run-in", Display::Block),
            ("-webkit-box", Display::Flex),
            ("-webkit-inline-box", Display::InlineFlex),
            ("contents", Display::Contents),
            ("none", Display::None),
            ("table-cell", Display::TableCell),
        ];
        for (value, expected) in cases {
            assert_eq!(
                style(&format!("display: {value}")).display,
                *expected,
                "{value}"
            );
        }
        for value in [
            "blocky",
            "block block",
            "list-item table",
            "inline block flow list-item x",
        ] {
            assert!(!valid("display", value), "{value}");
        }
    }

    #[test]
    fn box_model_shorthands() {
        let s = style("margin: 1px 2px 3px 4px; padding: 5px 6px");
        assert_eq!(
            [s.margin_top, s.margin_right, s.margin_bottom, s.margin_left],
            [lpa(1.0), lpa(2.0), lpa(3.0), lpa(4.0)]
        );
        assert_eq!(
            [
                s.padding_top,
                s.padding_right,
                s.padding_bottom,
                s.padding_left
            ],
            [px(5.0), px(6.0), px(5.0), px(6.0)]
        );
        let s = style("margin: 0 auto");
        assert_eq!(s.margin_left, LengthPercentageOrAuto::Auto);
        assert_eq!(s.margin_top, lpa(0.0));
        let s = style("margin: 1px 2px 3px");
        assert_eq!(s.margin_left, lpa(2.0));
        assert_eq!(s.margin_bottom, lpa(3.0));
        let s = style("margin-inline: 5px auto; margin-block: 7px");
        assert_eq!(s.margin_left, lpa(5.0));
        assert_eq!(s.margin_right, LengthPercentageOrAuto::Auto);
        assert_eq!(
            (s.margin_top.clone(), s.margin_bottom.clone()),
            (lpa(7.0), lpa(7.0))
        );
        let s = style("margin-inline-start: 3px; padding-inline-end: 4%; padding-block: 1px 2px");
        assert_eq!(s.margin_left, lpa(3.0));
        assert_eq!(s.padding_right, LengthPercentage::Percent(0.04));
        assert_eq!(
            (s.padding_top.clone(), s.padding_bottom.clone()),
            (px(1.0), px(2.0))
        );
        let s = style("inset: 1px 2px; inset-inline-start: 9px");
        assert_eq!(s.top, lpa(1.0));
        assert_eq!(s.left, lpa(9.0));
        assert!(!valid("padding", "-1px"));
        assert!(!valid("margin", "1px 2px 3px 4px 5px"));
        assert!(!valid("padding", "auto"));
        let s = style("width: 50%; height: 2em; min-width: min-content; max-width: none");
        assert_eq!(
            s.width,
            Size::LengthPercentage(LengthPercentage::Percent(0.5))
        );
        assert_eq!(s.height, Size::LengthPercentage(px(32.0)));
        assert_eq!(s.min_width, Size::MinContent);
        assert_eq!(s.max_width, MaxSize::None);
        let s = style(
            "width: fit-content(100px); max-height: max-content; height: -webkit-fill-available",
        );
        assert_eq!(s.width, Size::FitContent(Some(px(100.0))));
        assert_eq!(s.max_height, MaxSize::MaxContent);
        assert_eq!(s.height, Size::Auto);
        assert!(!valid("width", "-10px"));
        assert!(!valid("max-width", "auto"));
        assert!(!valid("min-width", "none"));
        let s = style("box-sizing: border-box; aspect-ratio: 16 / 9");
        assert_eq!(s.box_sizing, BoxSizing::BorderBox);
        assert!(!s.aspect_ratio.auto);
        assert!((s.aspect_ratio.ratio.unwrap_or(0.0) - 16.0 / 9.0).abs() < 1e-6);
        let ratio = |auto, ratio| AspectRatio { auto, ratio };
        assert_eq!(
            style("aspect-ratio: auto 2").aspect_ratio,
            ratio(true, Some(2.0))
        );
        assert_eq!(
            style("aspect-ratio: 2 auto").aspect_ratio,
            ratio(true, Some(2.0))
        );
        assert_eq!(style("aspect-ratio: auto").aspect_ratio, AspectRatio::AUTO);
        // A degenerate ratio behaves as `auto`.
        assert_eq!(
            style("aspect-ratio: 0 / 1").aspect_ratio,
            ratio(false, None)
        );
        assert!(!valid("aspect-ratio", "auto auto"));
        assert!(!valid("aspect-ratio", "-1"));
    }

    /// Measured in Chromium 148 (`tools/probes/host-sizes-auto.json`).
    #[test]
    fn contain_and_intrinsic_size() {
        let contain = |v: &str| style(&format!("contain: {v}")).contain;
        assert_eq!(contain("none"), Contain::NONE);
        assert_eq!(contain("strict"), Contain::STRICT);
        assert_eq!(contain("CONTENT"), Contain::CONTENT);
        assert_eq!(
            contain("paint size"),
            Contain {
                size: true,
                paint: true,
                ..Contain::NONE
            }
        );
        assert_eq!(contain("size layout paint style"), Contain::STRICT);
        assert!(contain("inline-size layout").inline_size);
        for bad in [
            "size size",
            "none size",
            "size foo",
            "",
            "size inline-size",
            "strict size",
            "layout,",
        ] {
            assert!(!valid("contain", bad), "{bad}");
        }
        let cis = |v: &str| {
            let s = style(&format!("contain-intrinsic-size: {v}"));
            (s.contain_intrinsic_width, s.contain_intrinsic_height)
        };
        let len = |auto, px| ContainIntrinsic {
            auto,
            length: Some(px),
        };
        assert_eq!(cis("10px"), (len(false, 10.0), len(false, 10.0)));
        assert_eq!(cis("10px 20px"), (len(false, 10.0), len(false, 20.0)));
        assert_eq!(cis("auto 10px"), (len(true, 10.0), len(true, 10.0)));
        assert_eq!(cis("10px auto 20px"), (len(false, 10.0), len(true, 20.0)));
        assert_eq!(cis("10px none"), (len(false, 10.0), ContainIntrinsic::NONE));
        assert_eq!(cis("1em 2em"), (len(false, 16.0), len(false, 32.0)));
        assert_eq!(cis("10px auto"), (len(false, 10.0), len(false, 10.0)));
        for bad in ["auto", "10%", "-10px", "5px 6px 7px", "auto auto 5px"] {
            assert!(!valid("contain-intrinsic-size", bad), "{bad}");
        }
        let s = style("contain-intrinsic-height: auto 5px; contain-intrinsic-inline-size: 7px");
        assert_eq!(s.contain_intrinsic_height, len(true, 5.0));
        assert_eq!(s.contain_intrinsic_width, len(false, 7.0));
        assert!(!valid("contain-intrinsic-width", "10px 20px"));
        assert!(!valid("contain-intrinsic-width", "5%"));
    }

    /// CSS Sizing 4 §7.1: `auto && <ratio>` uses the natural aspect ratio
    /// of a replaced element if it has one.
    #[test]
    fn preferred_aspect_ratio() {
        let ratio = |auto, ratio| AspectRatio { auto, ratio };
        assert_eq!(ratio(true, Some(2.0)).preferred(Some(1.5)), Some(1.5));
        assert_eq!(ratio(true, Some(2.0)).preferred(None), Some(2.0));
        assert_eq!(ratio(false, Some(2.0)).preferred(Some(1.5)), Some(2.0));
        assert_eq!(ratio(false, None).preferred(Some(1.5)), Some(1.5));
        assert_eq!(AspectRatio::AUTO.preferred(None), None);
    }

    #[test]
    fn object_position() {
        let s = style("object-position: right 10px bottom 20%");
        let [x, y] = &s.object_position;
        assert_eq!(
            (x.offset.clone(), x.from_end),
            (LengthPercentage::Px(10.0), true)
        );
        assert_eq!(
            (y.offset.clone(), y.from_end),
            (LengthPercentage::Percent(0.2), true)
        );
        let initial = ComputedStyle::initial();
        assert_eq!(
            initial.object_position,
            [PositionComponent::CENTER, PositionComponent::CENTER]
        );
        assert_eq!(
            style("object-position: 25%").object_position[1],
            PositionComponent::CENTER
        );
        // `<position>` has no three-value form.
        assert!(!valid("object-position", "right 10px top"));
        assert!(!valid("object-position", "top 10px"));
    }

    #[test]
    fn borders() {
        let s = style("border: 2px solid red");
        assert_eq!(s.border_widths(), [2.0; 4]);
        assert_eq!(s.border_left_style, BorderStyle::Solid);
        assert_eq!(s.border_bottom_color, rgb(255, 0, 0));
        let s =
            style("border: thin dotted; border-right: thick double #00f; border-top-width: medium");
        assert_eq!(s.border_widths(), [3.0, 5.0, 1.0, 1.0]);
        assert_eq!(s.border_top_color, Color::CurrentColor);
        assert_eq!(s.border_right_color, rgb(0, 0, 255));
        // Widths snap like Chromium at device pixel ratio 1.
        let s = style("border-width: 0.3px 1.7px 2.5px 0");
        assert_eq!(s.border_widths(), [1.0, 1.0, 2.0, 0.0]);
        let s =
            style("border-width: 1px 2px; border-style: solid none; border-color: red blue green");
        assert_eq!(s.border_widths(), [1.0, 2.0, 1.0, 2.0]);
        assert_eq!(s.border_right_style, BorderStyle::None);
        assert_eq!(s.border_left_color, rgb(0, 0, 255));
        assert_eq!(s.border_bottom_color, rgb(0, 128, 0));
        let s = style("border-block: 4px solid; border-inline-start-color: red");
        assert_eq!((s.border_top_width, s.border_bottom_width), (4.0, 4.0));
        assert_eq!(s.border_left_color, rgb(255, 0, 0));
        assert!(!valid("border", "2px 3px solid"));
        assert!(!valid("border-width", "-1px"));
        assert!(!valid("border-style", "wavy"));
        let s = style("border-radius: 10px 5% / 20px");
        assert_eq!(s.border_top_left_radius.horizontal, px(10.0));
        assert_eq!(
            s.border_top_right_radius.horizontal,
            LengthPercentage::Percent(0.05)
        );
        assert_eq!(s.border_bottom_left_radius.vertical, px(20.0));
        let s = style("border-top-left-radius: 3px 4px; border-end-end-radius: 6px");
        assert_eq!(s.border_top_left_radius.vertical, px(4.0));
        assert_eq!(s.border_bottom_right_radius.horizontal, px(6.0));
        let s = style("outline: 2px auto red; outline-offset: -1px");
        assert_eq!(s.outline_style, OutlineStyle::Auto);
        assert_eq!(s.outline_width, 2.0);
        assert_eq!(s.outline_color, rgb(255, 0, 0));
        assert_eq!(s.outline_offset, -1.0);
        assert!(!valid("outline-style", "hidden"));
    }

    #[test]
    fn backgrounds() {
        let s = style(
            "background: #fff url(a.png) no-repeat right 10px top / 50% auto fixed content-box",
        );
        assert_eq!(s.background_color, rgb(255, 255, 255));
        assert_eq!(&*s.background_image, &[Some(Image::Url("a.png".into()))]);
        assert_eq!(
            s.background_position_x[0],
            PositionComponent {
                offset: px(10.0),
                from_end: true
            }
        );
        assert_eq!(
            s.background_position_y[0].offset,
            LengthPercentage::Percent(0.0)
        );
        assert_eq!(
            s.background_size[0],
            BackgroundSize::Explicit(
                LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Percent(0.5)),
                LengthPercentageOrAuto::Auto
            )
        );
        assert_eq!(
            s.background_repeat[0],
            (
                BackgroundRepeatKeyword::NoRepeat,
                BackgroundRepeatKeyword::NoRepeat
            )
        );
        assert_eq!(s.background_attachment[0], BackgroundAttachment::Fixed);
        assert_eq!(s.background_origin[0], BackgroundBox::ContentBox);
        assert_eq!(s.background_clip[0], BackgroundBox::ContentBox);
        // Two layers; the color goes with the last one.
        let s = style("background: url(a.png), linear-gradient(red, blue) repeat-x red");
        assert_eq!(s.background_image.len(), 2);
        assert!(matches!(
            s.background_image[1],
            Some(Image::LinearGradient(_))
        ));
        assert_eq!(
            s.background_repeat[1],
            (
                BackgroundRepeatKeyword::Repeat,
                BackgroundRepeatKeyword::NoRepeat
            )
        );
        assert_eq!(s.background_color, rgb(255, 0, 0));
        assert!(!valid("background", "red, url(a.png)"));
        assert!(!valid("background", ""));
        assert!(!valid("background", "url(a.png),"));
        assert!(!valid("background", ", red"));
        // The shorthand resets the color.
        let s = style("background-color: red; background: url(a.png)");
        assert_eq!(s.background_color, Color::TRANSPARENT);
        let s = style("background: none");
        assert_eq!(&*s.background_image, &[None]);
        let s = style("background-position: center bottom 5px, 20% 30%");
        assert_eq!(
            s.background_position_x[0].offset,
            LengthPercentage::Percent(0.5)
        );
        assert!(s.background_position_y[0].from_end);
        assert_eq!(
            s.background_position_y[1].offset,
            LengthPercentage::Percent(0.3)
        );
        let s = style("background-position: top");
        assert_eq!(
            s.background_position_x[0].offset,
            LengthPercentage::Percent(0.5)
        );
        assert_eq!(
            s.background_position_y[0].offset,
            LengthPercentage::Percent(0.0)
        );
        let s = style("background-position: right top");
        assert_eq!(
            s.background_position_x[0].offset,
            LengthPercentage::Percent(1.0)
        );
        let s = style("background-position-x: right 4px; background-position-y: 7px, center");
        assert!(s.background_position_x[0].from_end);
        assert_eq!(s.background_position_y.len(), 2);
        assert!(!valid("background-position", "top left 10px 20px 30px"));
        assert!(!valid("background-position", "left right"));
        let s = style(
            "background-size: cover, 10px; background-repeat: space round; background-clip: text",
        );
        assert_eq!(s.background_size[0], BackgroundSize::Cover);
        assert_eq!(
            s.background_repeat[0],
            (
                BackgroundRepeatKeyword::Space,
                BackgroundRepeatKeyword::Round
            )
        );
        assert_eq!(s.background_clip[0], BackgroundBox::BorderBox);
        let s = style("background-image: radial-gradient(red, blue), image-set('a.png' 1x)");
        assert_eq!(s.background_image[0], None);
        assert_eq!(s.background_image[1], Some(Image::Url("a.png".into())));
    }

    #[test]
    fn fonts() {
        let s =
            style("font: italic small-caps bold 12px/1.5 \"Helvetica Neue\", Arial, sans-serif");
        assert_eq!(s.font_style, FontStyle::Italic);
        assert_eq!(s.font_variant_caps, FontVariantCaps::SmallCaps);
        assert_eq!(s.font_weight, 700.0);
        assert_eq!(s.font_size, 12.0);
        assert_eq!(s.line_height, LineHeight::Number(1.5));
        assert_eq!(
            &*s.font_family,
            &[
                FontFamily::Named("Helvetica Neue".into()),
                FontFamily::Named("Arial".into()),
                FontFamily::Generic(GenericFamily::SansSerif)
            ]
        );
        // The shorthand resets the two settings (measured in Chromium 148);
        // `font-variant` does not.
        let css = "font-variation-settings: 'wght' 9; font-feature-settings: 'liga' 0;";
        let s = style(&format!("{css} font: 12px serif"));
        assert!(s.font_variation_settings.is_empty() && s.font_feature_settings.is_empty());
        let s = style(&format!("{css} font: menu"));
        assert!(s.font_variation_settings.is_empty() && s.font_feature_settings.is_empty());
        let s = style(&format!("{css} font-variant: small-caps"));
        assert_eq!(s.font_variation_settings.len(), 1);
        assert_eq!(s.font_feature_settings.len(), 1);
        let s = style("font: 13px Times New Roman");
        assert_eq!(
            &*s.font_family,
            &[FontFamily::Named("Times New Roman".into())]
        );
        let s = style("font: condensed 300 2em/20px monospace");
        assert_eq!(s.font_stretch, 75.0);
        assert_eq!(s.font_weight, 300.0);
        assert_eq!(s.font_size, 32.0);
        assert_eq!(s.line_height, LineHeight::Px(20.0));
        let s = style("font: menu");
        assert_eq!(
            &*s.font_family,
            &[FontFamily::Generic(GenericFamily::SystemUi)]
        );
        assert_eq!(s.font_size, 13.0);
        assert!(!valid("font", "12px"));
        assert!(!valid("font", "bold serif"));
        let s = style("font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI'");
        assert_eq!(
            s.font_family[0],
            FontFamily::Generic(GenericFamily::SystemUi)
        );
        assert_eq!(
            s.font_family[1],
            FontFamily::Generic(GenericFamily::SystemUi)
        );
        assert!(!valid("font-family", "serif, inherit"));
        assert!(!valid("font-family", "a,,b"));
    }

    #[test]
    fn font_longhands() {
        let sizes: &[(&str, f32)] = &[
            ("xx-small", 9.0),
            ("x-small", 10.0),
            ("small", 13.0),
            ("medium", 16.0),
            ("large", 18.0),
            ("x-large", 24.0),
            ("xx-large", 32.0),
            ("xxx-large", 48.0),
            ("larger", 19.2),
            ("smaller", 16.0 / 1.2),
            ("150%", 24.0),
            ("2em", 32.0),
            ("1rem", 16.0),
            ("12pt", 16.0),
            ("calc(10px + 1em)", 26.0),
        ];
        for (value, expected) in sizes {
            let s = style(&format!("font-size: {value}"));
            assert!(
                (s.font_size - expected).abs() < 1e-4,
                "{value}: {}",
                s.font_size
            );
        }
        assert!(!valid("font-size", "-1px"));
        assert!(!valid("font-size", "12"));
        let weights: &[(&str, f32)] = &[
            ("normal", 400.0),
            ("bold", 700.0),
            ("1", 1.0),
            ("1000", 1000.0),
            ("bolder", 700.0),
            ("lighter", 100.0),
        ];
        for (value, expected) in weights {
            assert_eq!(
                style(&format!("font-weight: {value}")).font_weight,
                *expected,
                "{value}"
            );
        }
        assert!(!valid("font-weight", "0"));
        assert!(!valid("font-weight", "1001"));
        assert_eq!(
            style("font-style: oblique 10deg").font_style,
            FontStyle::Oblique
        );
        assert!(!valid("font-style", "oblique 100deg"));
        assert_eq!(style("font-stretch: 120%").font_stretch, 120.0);
        assert_eq!(
            style("font-variant: small-caps tabular-nums").font_variant_caps,
            FontVariantCaps::SmallCaps
        );
        assert!(valid("font-variant", "common-ligatures oldstyle-nums"));
        assert!(!valid("font-variant", "bogus"));
        let s = style("font-variation-settings: 'wght' 660, 'wdth' 80, 'wght' 700");
        assert_eq!(
            &*s.font_variation_settings,
            &[(*b"wdth", 80.0), (*b"wght", 700.0)]
        );
        let s = style("font-feature-settings: 'liga' off, 'ss01'");
        assert_eq!(&*s.font_feature_settings, &[(*b"liga", 0), (*b"ss01", 1)]);
        assert!(!valid("font-variation-settings", "'wght'"));
        assert!(!valid("font-feature-settings", "'liga' 1.5"));
        assert!(valid("font-feature-settings", "normal"));
        let heights: &[(&str, LineHeight)] = &[
            ("normal", LineHeight::Normal),
            ("1.2", LineHeight::Number(1.2)),
            ("20px", LineHeight::Px(20.0)),
            ("150%", LineHeight::Px(24.0)),
            ("2em", LineHeight::Px(32.0)),
        ];
        for (value, expected) in heights {
            assert_eq!(
                style(&format!("line-height: {value}")).line_height,
                *expected,
                "{value}"
            );
        }
        assert!(!valid("line-height", "-1"));
    }

    #[test]
    fn text_properties() {
        let aligns: &[(&str, TextAlign)] = &[
            ("left", TextAlign::Left),
            ("center", TextAlign::Center),
            ("-webkit-center", TextAlign::WebkitCenter),
            ("justify-all", TextAlign::Justify),
            ("start", TextAlign::Start),
            ("end", TextAlign::End),
            ("-webkit-left", TextAlign::WebkitLeft),
        ];
        for (value, expected) in aligns {
            assert_eq!(
                style(&format!("text-align: {value}")).text_align,
                *expected,
                "{value}"
            );
        }
        assert!(!valid("text-align", "-internal-center"));
        let spaces: &[(&str, WhiteSpace)] = &[
            ("nowrap", WhiteSpace::Nowrap),
            ("pre-wrap", WhiteSpace::PreWrap),
            ("break-spaces", WhiteSpace::BreakSpaces),
            ("preserve nowrap", WhiteSpace::Pre),
            ("collapse", WhiteSpace::Normal),
            ("nowrap collapse", WhiteSpace::Nowrap),
            ("preserve-breaks", WhiteSpace::PreLine),
            ("-moz-pre-wrap", WhiteSpace::PreWrap),
        ];
        for (value, expected) in spaces {
            assert_eq!(
                style(&format!("white-space: {value}")).white_space,
                *expected,
                "{value}"
            );
        }
        assert!(!valid("white-space", "nowrap nowrap"));
        let s = style("text-decoration: underline dotted red 2px");
        assert_eq!(s.text_decoration_line, TextDecorationLine::UNDERLINE);
        assert_eq!(s.text_decoration_style, TextDecorationStyle::Dotted);
        assert_eq!(s.text_decoration_color, rgb(255, 0, 0));
        let s = style("text-decoration-line: underline line-through");
        assert_eq!(
            s.text_decoration_line,
            TextDecorationLine::UNDERLINE | TextDecorationLine::LINE_THROUGH
        );
        assert!(
            style("text-decoration: none")
                .text_decoration_line
                .is_empty()
        );
        assert!(!valid("text-decoration-line", "underline underline"));
        assert!(valid("text-underline-offset", "2px"));
    }

    #[test]
    fn more_text_properties() {
        let s = style("letter-spacing: 0.1em; word-spacing: 2px; text-indent: -10px hanging");
        assert!((s.letter_spacing - 1.6).abs() < 1e-5);
        assert_eq!(s.word_spacing, 2.0);
        assert_eq!(s.text_indent, px(-10.0));
        assert_eq!(style("letter-spacing: normal").letter_spacing, 0.0);
        let s = style(
            "word-wrap: break-word; word-break: break-all; -webkit-hyphens: auto; tab-size: 4",
        );
        assert_eq!(s.overflow_wrap, OverflowWrap::BreakWord);
        assert_eq!(s.word_break, WordBreak::BreakAll);
        assert_eq!(s.hyphens, Hyphens::Auto);
        assert_eq!(s.tab_size, 4.0);
        let s = style(
            "text-transform: uppercase; text-overflow: ellipsis; direction: rtl; unicode-bidi: -webkit-isolate",
        );
        assert_eq!(s.text_transform, TextTransform::Uppercase);
        assert_eq!(s.text_overflow, TextOverflow::Ellipsis);
        assert_eq!(s.direction, Direction::Rtl);
        assert_eq!(s.unicode_bidi, UnicodeBidi::Isolate);
        let valigns: &[(&str, VerticalAlign)] = &[
            (
                "middle",
                VerticalAlign::Keyword(VerticalAlignKeyword::Middle),
            ),
            (
                "text-top",
                VerticalAlign::Keyword(VerticalAlignKeyword::TextTop),
            ),
            ("-0.2em", VerticalAlign::LengthPercentage(px(-3.2))),
            (
                "10%",
                VerticalAlign::LengthPercentage(LengthPercentage::Percent(0.1)),
            ),
        ];
        for (value, expected) in valigns {
            assert_eq!(
                style(&format!("vertical-align: {value}")).vertical_align,
                *expected,
                "{value}"
            );
        }
        let s = style(
            "cursor: url(a.cur) 2 3, url(b.png), pointer; user-select: none; pointer-events: visiblePainted",
        );
        assert_eq!(s.cursor, Cursor::Pointer);
        assert_eq!(s.user_select, UserSelect::None);
        assert_eq!(s.pointer_events, PointerEvents::VisiblePainted);
        assert!(!valid("cursor", "url(a.cur)"));
        assert_eq!(
            style("-webkit-user-select: text").user_select,
            UserSelect::Text
        );
        assert_eq!(style("color: currentColor").color, Rgba::BLACK);
        assert_eq!(
            style("color: rgb(1 2 3 / 50%)").color,
            Rgba::new(1, 2, 3, 128)
        );
    }

    #[test]
    fn lists_and_content() {
        let s = style("list-style: square inside");
        assert_eq!(s.list_style_type, ListStyleType::Square);
        assert_eq!(s.list_style_position, ListStylePosition::Inside);
        assert_eq!(s.list_style_image, None);
        let s = style("list-style: none");
        assert_eq!(s.list_style_type, ListStyleType::None);
        let s = style("list-style: url(dot.png) none");
        assert_eq!(s.list_style_type, ListStyleType::None);
        assert_eq!(s.list_style_image, Some(Image::Url("dot.png".into())));
        let s = style("list-style: none url(dot.png)");
        assert_eq!(s.list_style_type, ListStyleType::None);
        assert!(!valid("list-style", "none none none"));
        assert!(!valid("list-style", "disc none url(a.png)"));
        assert_eq!(
            style("list-style-type: '-'").list_style_type,
            ListStyleType::None
        );
        assert_eq!(
            style("list-style-type: armenian").list_style_type,
            ListStyleType::Decimal
        );
        assert_eq!(
            style("list-style-type: lower-latin").list_style_type,
            ListStyleType::LowerAlpha
        );
        let s = style(
            r#"content: "a" attr(title) counter(item, upper-roman) open-quote url(x.png) / "alt""#,
        );
        let Content::Items(items) = &s.content else {
            panic!("expected items");
        };
        assert_eq!(items.len(), 5);
        assert_eq!(items[0], ContentItem::String("a".into()));
        // No element: attr() is empty.
        assert_eq!(items[1], ContentItem::String("".into()));
        assert_eq!(
            items[2],
            ContentItem::Counter {
                name: "item".into(),
                separator: None,
                style: ListStyleType::UpperRoman
            }
        );
        assert_eq!(items[3], ContentItem::OpenQuote);
        assert_eq!(style("content: none").content, Content::None);
        assert_eq!(style("content: normal").content, Content::Normal);
        assert_eq!(
            style("content: counters(a, '.', lower-alpha)").content,
            Content::Items(Arc::from([ContentItem::Counter {
                name: "a".into(),
                separator: Some(".".into()),
                style: ListStyleType::LowerAlpha
            }]))
        );
        assert!(valid("content", "no-open-quote"));
        assert!(valid("content", "no-close-quote 'x'"));
        assert!(!valid("content", "bogus"));
        assert!(!valid("content", "'a' /"));
    }

    /// `counter()` and `counters()` as Chromium 148 accepts them
    /// (`CSS.supports`).
    #[test]
    fn counter_functions() {
        for value in [
            "counter(c)",
            "counter(c, none)",
            "counter(none)",
            "counter(c, foo)",
            "counters(c, '.')",
            "counters(c, \".\", decimal)",
            "counters( c , '.' )",
        ] {
            assert!(valid("content", value), "{value}");
        }
        for value in [
            "counter(c, default)",
            "counter(c, inherit)",
            "counter(inherit)",
            "counter(c,)",
            "counters(c)",
            "counters(c, '.',)",
            "counter(c, symbols(cyclic '*'))",
            "counter(c, 'x')",
            "counter(1)",
            "counter(c, decimal, x)",
        ] {
            assert!(!valid("content", value), "{value}");
        }
    }

    /// `counter-reset`, `counter-increment` and `counter-set` as Chromium
    /// 148 accepts them (`CSS.supports`), and their computed values.
    #[test]
    fn counter_properties() {
        for value in [
            "c",
            "C",
            "c calc(1.5)",
            "c calc(1 + 2)",
            "c 1 c 2",
            "c +3",
            "auto",
            "none",
            "a 1 b -2 c",
        ] {
            assert!(valid("counter-reset", value), "{value}");
            assert!(valid("counter-increment", value), "{value}");
            assert!(valid("counter-set", value), "{value}");
        }
        for value in [
            "reversed(x)",
            "reversed(x) 3",
            "3 c",
            "c, d",
            "none none",
            "c none",
            "none 3",
            "Initial 2",
            "revert 2",
            "revert-layer 2",
            "default",
            "c 1.5",
            "c 1e3",
            "c 2px",
        ] {
            assert!(!valid("counter-reset", value), "{value}");
            assert!(!valid("counter-set", value), "{value}");
        }
        let entries = |list: &CounterList| -> Vec<(String, i32)> {
            list.iter().map(|(n, v)| (n.to_string(), v)).collect()
        };
        let s = style(
            "counter-reset: a b 3 a 99999999999; counter-increment: a b -2; \
             counter-set: x calc(-1.5)",
        );
        assert_eq!(
            entries(&s.counter_reset),
            [("a".into(), 0), ("b".into(), 3), ("a".into(), i32::MAX)]
        );
        assert_eq!(
            entries(&s.counter_increment),
            [("a".into(), 1), ("b".into(), -2)]
        );
        assert_eq!(entries(&s.counter_set), [("x".into(), -1)]);
        assert!(style("counter-reset: none").counter_reset.is_empty());
        assert!(ComputedStyle::initial().counter_increment.is_empty());
        // At most 256 counters per value.
        let names: Vec<String> = (0..300).map(|i| format!("c{i}")).collect();
        let s = style(&format!("counter-reset: {}", names.join(" ")));
        assert_eq!(s.counter_reset.iter().count(), 256);
    }

    #[test]
    fn flexbox_and_alignment() {
        let flex: &[(&str, (f32, f32, FlexBasis))] = &[
            ("none", (0.0, 0.0, FlexBasis::Size(Size::Auto))),
            ("auto", (1.0, 1.0, FlexBasis::Size(Size::Auto))),
            (
                "1",
                (
                    1.0,
                    1.0,
                    FlexBasis::Size(Size::LengthPercentage(LengthPercentage::Percent(0.0))),
                ),
            ),
            (
                "2 3",
                (
                    2.0,
                    3.0,
                    FlexBasis::Size(Size::LengthPercentage(LengthPercentage::Percent(0.0))),
                ),
            ),
            (
                "0 0 10px",
                (0.0, 0.0, FlexBasis::Size(Size::LengthPercentage(px(10.0)))),
            ),
            (
                "30%",
                (
                    1.0,
                    1.0,
                    FlexBasis::Size(Size::LengthPercentage(LengthPercentage::Percent(0.3))),
                ),
            ),
            ("content 2", (2.0, 1.0, FlexBasis::Content)),
        ];
        for (value, (grow, shrink, basis)) in flex {
            let s = style(&format!("flex: {value}"));
            assert_eq!(
                (s.flex_grow, s.flex_shrink, &s.flex_basis),
                (*grow, *shrink, basis),
                "{value}"
            );
        }
        assert!(!valid("flex", "1 2 3"));
        assert!(!valid("flex", "-1"));
        let s = style("flex-flow: column wrap; order: -2; gap: 10px 5%");
        assert_eq!(s.flex_direction, FlexDirection::Column);
        assert_eq!(s.flex_wrap, FlexWrap::Wrap);
        assert_eq!(s.order, -2);
        assert_eq!(s.row_gap, Gap::LengthPercentage(px(10.0)));
        assert_eq!(
            s.column_gap,
            Gap::LengthPercentage(LengthPercentage::Percent(0.05))
        );
        assert_eq!(
            style("grid-gap: 3px").column_gap,
            Gap::LengthPercentage(px(3.0))
        );
        assert_eq!(style("grid-row-gap: normal").row_gap, Gap::Normal);
        let aligns: &[(&str, &str, Alignment)] = &[
            ("justify-content", "space-between", Alignment::SpaceBetween),
            ("justify-content", "safe center", Alignment::Center),
            ("justify-content", "left", Alignment::Left),
            ("align-items", "first baseline", Alignment::Baseline),
            ("align-items", "flex-end", Alignment::FlexEnd),
            ("align-self", "auto", Alignment::Auto),
            ("align-content", "space-evenly", Alignment::SpaceEvenly),
            ("align-content", "last baseline", Alignment::Baseline),
            ("-webkit-align-items", "center", Alignment::Center),
        ];
        for (name, value, expected) in aligns {
            let s = style(&format!("{name}: {value}"));
            let actual = match *name {
                "justify-content" => s.justify_content,
                "align-items" | "-webkit-align-items" => s.align_items,
                "align-self" => s.align_self,
                _ => s.align_content,
            };
            assert_eq!(actual, *expected, "{name}: {value}");
        }
        assert!(!valid("align-items", "space-between"));
        assert!(!valid("justify-content", "baseline"));
        assert!(!valid("justify-content", "first baseline"));
        assert!(valid("align-content", "baseline"));
        assert!(!valid("align-items", "auto"));
        assert!(!valid("align-content", "left"));
        let s = style(
            "place-content: center space-around; place-items: end start; place-self: stretch",
        );
        assert_eq!(s.align_content, Alignment::Center);
        assert_eq!(s.justify_content, Alignment::SpaceAround);
        assert_eq!(s.align_items, Alignment::End);
        assert_eq!(s.justify_items, Alignment::Start);
        assert_eq!(s.align_self, Alignment::Stretch);
        assert_eq!(s.justify_self, Alignment::Stretch);
    }

    #[test]
    fn justify_items_and_self() {
        let justify_items = |css: &str| style(css).justify_items;
        assert_eq!(justify_items("place-items: center"), Alignment::Center);
        assert_eq!(
            justify_items("justify-items: legacy center"),
            Alignment::Center
        );
        assert_eq!(
            justify_items("justify-items: right legacy"),
            Alignment::Right
        );
        assert_eq!(justify_items("justify-items: legacy"), Alignment::Normal);
        assert_eq!(style("justify-self: auto").justify_self, Alignment::Auto);
        assert!(!valid("justify-items", "legacy start"));
        assert!(!valid("justify-items", "auto"));
        assert!(!valid("justify-items", "space-between"));
        assert!(!valid("justify-self", "legacy"));
    }

    #[test]
    fn grid_properties() {
        let s = style(
            "grid-template: min-content 1fr / 12.25rem minmax(0, 1fr); \
             grid-template-areas: 'a b' 'c c'; grid-auto-flow: column dense; \
             grid-auto-rows: 10px 2em; grid-area: b / 2 / span 3",
        );
        assert_eq!(s.grid_template_rows.entries().len(), 2);
        let TrackListValue::Track(first) = &s.grid_template_columns.entries()[0].value else {
            panic!("a track");
        };
        assert_eq!(first, &TrackSize::Breadth(TrackBreadth::Length(px(196.0))));
        assert!(
            s.grid_template_areas
                .as_ref()
                .is_some_and(|a| a.areas().len() == 3)
        );
        assert_eq!(
            s.grid_auto_flow,
            GridAutoFlow {
                column: true,
                dense: true
            }
        );
        assert_eq!(
            &*s.grid_auto_rows,
            &[
                TrackSize::Breadth(TrackBreadth::Length(px(10.0))),
                TrackSize::Breadth(TrackBreadth::Length(px(32.0)))
            ]
        );
        assert_eq!(s.grid_row_start, GridLine::Name("b".into()));
        assert_eq!(s.grid_column_start, GridLine::Line(2, None));
        assert_eq!(s.grid_row_end, GridLine::Span(3, None));
        assert_eq!(s.grid_column_end, GridLine::Auto);
        // `grid` resets the template when it sets the auto tracks.
        let s = style("grid-template-columns: 10px; grid: auto-flow / 1fr");
        assert!(!s.grid_auto_flow.column);
        assert_eq!(s.grid_template_columns.entries().len(), 1);
        assert!(s.grid_template_rows.is_none());
        let s = style("grid-row: x; grid-column: 3 / -1");
        assert_eq!(s.grid_row_end, GridLine::Name("x".into()));
        assert_eq!(s.grid_column_end, GridLine::Line(-1, None));
        assert!(valid(
            "grid-template-columns",
            "repeat(auto-fill, minmax(10rem, 1fr))"
        ));
        assert!(!valid("grid-template-columns", "subgrid"));
        assert!(!valid("grid-row", "span 0"));
        assert!(!valid("grid-template-areas", "'a b' 'a a'"));
        assert!(valid("grid", "none"));
    }

    #[test]
    fn transforms_and_clip() {
        let s = style(
            "font-size: 10px; transform: translate(1em, 50%) rotate(0.25turn); \
             transform-origin: right 2em; clip: rect(1em, auto, 2px, 0)",
        );
        assert_eq!(
            s.transform.to_vec(),
            vec![
                TransformFunction::Translate(
                    LengthPercentage::Px(10.0),
                    LengthPercentage::Percent(0.5)
                ),
                TransformFunction::Rotate(90.0),
            ]
        );
        assert_eq!(
            s.transform_origin,
            TransformOrigin {
                x: LengthPercentage::Percent(1.0),
                y: LengthPercentage::Px(20.0)
            }
        );
        assert_eq!(
            s.clip,
            Some(ClipRect {
                top: Some(10.0),
                right: None,
                bottom: Some(2.0),
                left: Some(0.0)
            })
        );
        assert!(style("-webkit-transform: scale(2)").has_transform());
        assert!(!style("transform: none").has_transform());
        assert!(style("transform: translateZ(0)").has_transform());
        assert!(!valid("transform", "rotateY(10deg)"));
    }

    #[test]
    fn positioning_and_misc() {
        let s = style(
            "position: -webkit-sticky; top: 0; left: auto; z-index: 10; float: inline-start; clear: both",
        );
        assert_eq!(s.position, Position::Sticky);
        assert_eq!(s.top, lpa(0.0));
        assert_eq!(s.left, LengthPercentageOrAuto::Auto);
        assert_eq!(s.z_index, ZIndex::Integer(10));
        assert_eq!(s.float, Float::Left);
        assert_eq!(s.clear, Clear::Both);
        assert!(!valid("z-index", "1.5"));
        let s = style("opacity: 50%; visibility: hidden; overflow: hidden auto; object-fit: cover");
        assert_eq!(s.opacity, 0.5);
        assert_eq!(s.visibility, Visibility::Hidden);
        assert_eq!(
            (s.overflow_x, s.overflow_y),
            (Overflow::Hidden, Overflow::Auto)
        );
        assert_eq!(s.object_fit, ObjectFit::Cover);
        assert_eq!(
            style("scrollbar-width: none").scrollbar_width,
            ScrollbarWidth::None
        );
        assert_eq!(
            style("scrollbar-width: thin").scrollbar_width,
            ScrollbarWidth::Thin
        );
        assert_eq!(
            style("scrollbar-width: bogus").scrollbar_width,
            ScrollbarWidth::Auto
        );
        assert_eq!(style("opacity: 2").opacity, 1.0);
        assert_eq!(style("overflow: overlay").overflow_y, Overflow::Auto);
        let s = style(
            "border-collapse: collapse; border-spacing: 3px 4px; table-layout: fixed; caption-side: bottom; empty-cells: hide",
        );
        assert_eq!(s.border_collapse, BorderCollapse::Collapse);
        assert_eq!(
            (s.border_spacing_horizontal, s.border_spacing_vertical),
            (3.0, 4.0)
        );
        assert_eq!(s.table_layout, TableLayout::Fixed);
        assert_eq!(s.caption_side, CaptionSide::Bottom);
        assert_eq!(s.empty_cells, EmptyCells::Hide);
        assert!(!valid("border-spacing", "-1px"));
        assert_eq!(style("border-spacing: 5px").border_spacing_vertical, 5.0);
    }

    #[test]
    fn units_and_quirky_lengths() {
        let s = style("width: 10vw; height: 50vh; margin-left: 1in; margin-top: calc(100% - 2em)");
        assert_eq!(s.width, Size::LengthPercentage(px(80.0)));
        assert_eq!(s.height, Size::LengthPercentage(px(300.0)));
        assert_eq!(s.margin_left, lpa(96.0));
        let top = s.margin_top.non_auto().expect("length").clone();
        assert_eq!(top.resolve(100.0), 68.0);
        // Unitless lengths are valid only in quirks mode, only for the
        // listed properties, and not in shorthands like `border`.
        let q = style_with(
            "width: 100; margin: 5; border-width: 2; line-height: 20; border: 3 solid",
            true,
        );
        assert_eq!(q.width, Size::LengthPercentage(px(100.0)));
        assert_eq!(q.margin_left, lpa(5.0));
        assert_eq!(q.border_top_width, 2.0);
        assert_eq!(q.line_height, LineHeight::Number(20.0));
        let n = style_with("width: 100", false);
        assert_eq!(n.width, Size::Auto);
        let q = style_with("margin-inline-start: 5", true);
        assert_eq!(q.margin_left, lpa(0.0));
    }

    #[test]
    fn css_wide_keywords_and_variables_parse() {
        let parse = |name: &str, value: &str| {
            let values = swb_css::parse_component_values(value);
            let mut out = Vec::new();
            let ok =
                parse_declaration(name, &values, &ParserContext::author(None, false), &mut out);
            (ok, out)
        };
        let (ok, out) = parse("margin", "inherit");
        assert!(ok);
        assert_eq!(out.len(), 4);
        assert!(
            out.iter()
                .all(|d| matches!(d, PropertyDeclaration::CssWide(_, CssWideKeyword::Inherit)))
        );
        let (_, out) = parse("color", "revert-layer");
        assert!(matches!(
            out[0],
            PropertyDeclaration::CssWide(_, CssWideKeyword::Revert)
        ));
        let (ok, out) = parse("border", "var(--b) solid");
        assert!(ok);
        assert_eq!(out.len(), 12);
        assert!(
            matches!(&out[0], PropertyDeclaration::WithVariables(LonghandId::BorderTopWidth, u) if u.property == PropertyId::Shorthand(ShorthandId::Border))
        );
        let (ok, out) = parse("--Foo", " 1px  ");
        assert!(ok);
        assert!(
            matches!(&out[0], PropertyDeclaration::Custom(name, CustomDeclaration::Value(v)) if &**name == "--Foo" && v.len() == 1)
        );
        let (ok, out) = parse("all", "initial");
        assert!(ok);
        assert_eq!(out.len(), LonghandId::COUNT - 2);
        assert!(!parse("all", "red").0);
        assert!(!parse("frobnicate", "1").0);
        assert!(!parse("-webkit-mask-box-image", "none").0);
        assert!(!parse("color", "inherit red").0);
    }

    #[test]
    fn duplicates_in_a_block() {
        let cx = ParserContext::author(None, false);
        let block = DeclarationBlock::parse(
            &parse_style_attribute(
                "color: red; color: blue; --x: 1; --x: 2; width: 1px !important",
            ),
            &cx,
        );
        assert_eq!(block.normal.len(), 2);
        assert_eq!(block.important.len(), 1);
        assert!(
            matches!(&block.normal[0], PropertyDeclaration::Value(LonghandValue::Color(c)) if *c == rgb(0, 0, 255))
        );
    }

    #[test]
    fn supports() {
        let check = |css: &str| {
            swb_css::SupportsCondition::parse_str(css)
                .expect("valid condition")
                .evaluate(&is_supported)
        };
        assert!(check("(display: grid)"));
        assert!(check("(display: flex) and (gap: 1px)"));
        assert!(!check("(display: grid-lanes)"));
        assert!(check("(grid-template-columns: repeat(2, [a] 1fr))"));
        assert!(check("(grid-area: a / b)"));
        assert!(check("(justify-self: center)"));
        assert!(!check("(justify-items: banana)"));
        assert!(!check("(grid-template-rows: subgrid)"));
        assert!(!check("(grid-template-rows: masonry)"));
        assert!(check("(color: var(--x))"));
        assert!(check("(--x: anything)"));
        assert!(check("(mask-image: none)"));
        assert!(check("(-webkit-mask-image: none) or (mask-image: none)"));
        assert!(check("not (backdrop-filter: blur(1px))"));
    }

    #[test]
    fn column_width_and_count() {
        let s = style("column-width: 2em; column-count: 3; font-size: 10px");
        assert_eq!((s.column_width, s.column_count), (Some(20.0), Some(3)));
        let s = style("columns: 3 200px");
        assert_eq!((s.column_width, s.column_count), (Some(200.0), Some(3)));
        let s = style("columns: 200px; columns: auto 2");
        assert_eq!((s.column_width, s.column_count), (None, Some(2)));
        assert_eq!(style("columns: auto").column_count, None);
        assert!(!valid("column-count", "0"));
        assert!(!valid("column-width", "-1px"));
        assert!(!valid("column-width", "10%"));
        assert!(!valid("columns", "1 2"));
        assert!(!valid("columns", "auto auto auto"));
    }
}
