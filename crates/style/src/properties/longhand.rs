//! Parsers for longhand property values.
//!
//! [`parse_longhand`] parses the value of one longhand declaration. The
//! helper parsers are shared with the shorthands. The grammar of each
//! property is in the CSS specification that defines it; comments name
//! the specification where the grammar is not obvious.

use std::sync::Arc;

use swb_css::{ParseError, Parser};

use super::CssWideKeyword;
use super::ids::{LonghandId, LonghandValue};
use super::specified::{
    SpecifiedBackgroundSize, SpecifiedContainIntrinsic, SpecifiedContent, SpecifiedContentItem,
    SpecifiedFlexBasis, SpecifiedFontSize, SpecifiedFontWeight, SpecifiedLineHeight,
    SpecifiedPosition, SpecifiedSize, SpecifiedTextAlign, SpecifiedVerticalAlign,
};
use super::transform;
use crate::font_settings::{parse_feature_settings, parse_variation_settings};
use crate::parse::color::parse_color;
use crate::parse::grid;
use crate::parse::image::{SpecifiedImage, parse_image};
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::parse::{
    Origin, ParseResult, ParserContext, parse_angle, parse_integer, parse_non_negative_number,
    parse_number,
};
use crate::values::{
    Alignment, AspectRatio, BackgroundAttachment, BackgroundBox, BackgroundRepeatKeyword,
    BorderCollapse, BorderStyle, BoxSizing, CaptionSide, Clear, Contain, CounterList, Cursor,
    Direction, Display, EmptyCells, FlexDirection, FlexWrap, Float, FontFamily, FontSizeKeyword,
    FontStyle, FontVariantCaps, GenericFamily, Hyphens, Length, ListStylePosition, ListStyleType,
    ObjectFit, OutlineStyle, Overflow, OverflowWrap, PointerEvents, Position,
    SpecifiedLengthPercentage as Lp, TableLayout, TextAlign, TextDecorationLine,
    TextDecorationStyle, TextOverflow, TextTransform, UnicodeBidi, UserSelect,
    VerticalAlignKeyword, Visibility, WhiteSpace, WordBreak, ZIndex,
};

/// True for the properties where the quirks mode unitless length quirk
/// applies. <https://www.w3.org/TR/css-values-4/#quirky-lengths>
fn allows_quirky_length(id: LonghandId) -> bool {
    use LonghandId as L;
    matches!(
        id,
        L::BackgroundPositionX
            | L::BackgroundPositionY
            | L::BorderSpacingHorizontal
            | L::BorderSpacingVertical
            | L::BorderTopWidth
            | L::BorderRightWidth
            | L::BorderBottomWidth
            | L::BorderLeftWidth
            | L::Bottom
            | L::FontSize
            | L::Height
            | L::Left
            | L::LetterSpacing
            | L::MarginTop
            | L::MarginRight
            | L::MarginBottom
            | L::MarginLeft
            | L::MaxHeight
            | L::MaxWidth
            | L::MinHeight
            | L::MinWidth
            | L::PaddingTop
            | L::PaddingRight
            | L::PaddingBottom
            | L::PaddingLeft
            | L::Right
            | L::TextIndent
            | L::Top
            | L::VerticalAlign
            | L::Width
            | L::WordSpacing
    )
}

/// Parses the whole value of a longhand declaration.
#[allow(clippy::too_many_lines)] // One arm per longhand.
pub(crate) fn parse_longhand(
    id: LonghandId,
    p: &mut Parser<'_>,
    cx: &ParserContext,
) -> ParseResult<LonghandValue> {
    use LonghandId as L;
    use LonghandValue as V;
    let quirky = cx.quirks && allows_quirky_length(id);
    p.parse_entirely(|p| {
        Ok(match id {
            L::Color => V::Color(parse_color(p)?),
            L::FontFamily => V::FontFamily(parse_font_family(p)?),
            L::FontSize => V::FontSize(parse_font_size(p, quirky)?),
            L::FontWeight => V::FontWeight(parse_font_weight(p)?),
            L::FontStyle => V::FontStyle(parse_font_style(p)?),
            L::FontStretch => V::FontStretch(parse_font_stretch(p)?),
            L::FontVariantCaps => V::FontVariantCaps(keyword(p, FontVariantCaps::from_ident)?),
            L::FontVariationSettings => V::FontVariationSettings(parse_variation_settings(p)?),
            L::FontFeatureSettings => V::FontFeatureSettings(parse_feature_settings(p)?),
            L::LineHeight => V::LineHeight(parse_line_height(p)?),
            L::TextAlign => V::TextAlign(parse_text_align(p, cx)?),
            L::TextIndent => V::TextIndent(parse_text_indent(p, quirky)?),
            L::TextTransform => V::TextTransform(keyword(p, TextTransform::from_ident)?),
            L::WhiteSpace => V::WhiteSpace(parse_white_space(p)?),
            L::LetterSpacing => V::LetterSpacing(parse_spacing(p, quirky, true)?),
            L::WordSpacing => V::WordSpacing(parse_spacing(p, quirky, false)?),
            L::WordBreak => V::WordBreak(parse_word_break(p)?),
            L::OverflowWrap => V::OverflowWrap(keyword(p, OverflowWrap::from_ident)?),
            L::Hyphens => V::Hyphens(keyword(p, Hyphens::from_ident)?),
            L::TabSize => V::TabSize(parse_non_negative_number(p)?),
            L::Visibility => V::Visibility(keyword(p, Visibility::from_ident)?),
            L::ListStyleType => V::ListStyleType(parse_list_style_type(p)?),
            L::ListStylePosition => {
                V::ListStylePosition(keyword(p, ListStylePosition::from_ident)?)
            }
            L::ListStyleImage => V::ListStyleImage(parse_image_or_none(p, cx)?),
            L::Cursor => V::Cursor(parse_cursor(p, cx)?),
            L::Direction => V::Direction(keyword(p, Direction::from_ident)?),
            L::BorderCollapse => V::BorderCollapse(keyword(p, BorderCollapse::from_ident)?),
            L::BorderSpacingHorizontal => {
                V::BorderSpacingHorizontal(non_negative_length(p, quirky)?)
            }
            L::BorderSpacingVertical => V::BorderSpacingVertical(non_negative_length(p, quirky)?),
            L::CaptionSide => V::CaptionSide(parse_caption_side(p)?),
            L::EmptyCells => V::EmptyCells(keyword(p, EmptyCells::from_ident)?),
            L::PointerEvents => V::PointerEvents(parse_pointer_events(p)?),
            L::Display => V::Display(parse_display(p)?),
            L::Position => V::Position(keyword(p, Position::from_ident)?),
            L::Float => V::Float(keyword(p, Float::from_ident)?),
            L::Clear => V::Clear(keyword(p, Clear::from_ident)?),
            L::Top => V::Top(lp_or_auto(p, quirky)?),
            L::Right => V::Right(lp_or_auto(p, quirky)?),
            L::Bottom => V::Bottom(lp_or_auto(p, quirky)?),
            L::Left => V::Left(lp_or_auto(p, quirky)?),
            L::ZIndex => V::ZIndex(parse_z_index(p)?),
            L::Transform => V::Transform(transform::parse_transform(p)?),
            L::TransformOrigin => V::TransformOrigin(transform::parse_transform_origin(p)?),
            L::Clip => V::Clip(transform::parse_clip(p)?),
            L::Width => V::Width(parse_size(p, quirky, false)?),
            L::Height => V::Height(parse_size(p, quirky, false)?),
            L::MinWidth => V::MinWidth(parse_size(p, quirky, false)?),
            L::MinHeight => V::MinHeight(parse_size(p, quirky, false)?),
            L::MaxWidth => V::MaxWidth(parse_size(p, quirky, true)?),
            L::MaxHeight => V::MaxHeight(parse_size(p, quirky, true)?),
            L::BoxSizing => V::BoxSizing(keyword(p, BoxSizing::from_ident)?),
            L::AspectRatio => V::AspectRatio(parse_aspect_ratio(p)?),
            L::MarginTop => V::MarginTop(lp_or_auto(p, quirky)?),
            L::MarginRight => V::MarginRight(lp_or_auto(p, quirky)?),
            L::MarginBottom => V::MarginBottom(lp_or_auto(p, quirky)?),
            L::MarginLeft => V::MarginLeft(lp_or_auto(p, quirky)?),
            L::PaddingTop => V::PaddingTop(padding(p, quirky)?),
            L::PaddingRight => V::PaddingRight(padding(p, quirky)?),
            L::PaddingBottom => V::PaddingBottom(padding(p, quirky)?),
            L::PaddingLeft => V::PaddingLeft(padding(p, quirky)?),
            L::BorderTopWidth => V::BorderTopWidth(parse_line_width(p, quirky)?),
            L::BorderRightWidth => V::BorderRightWidth(parse_line_width(p, quirky)?),
            L::BorderBottomWidth => V::BorderBottomWidth(parse_line_width(p, quirky)?),
            L::BorderLeftWidth => V::BorderLeftWidth(parse_line_width(p, quirky)?),
            L::BorderTopStyle => V::BorderTopStyle(parse_border_style(p)?),
            L::BorderRightStyle => V::BorderRightStyle(parse_border_style(p)?),
            L::BorderBottomStyle => V::BorderBottomStyle(parse_border_style(p)?),
            L::BorderLeftStyle => V::BorderLeftStyle(parse_border_style(p)?),
            L::BorderTopColor => V::BorderTopColor(parse_color(p)?),
            L::BorderRightColor => V::BorderRightColor(parse_color(p)?),
            L::BorderBottomColor => V::BorderBottomColor(parse_color(p)?),
            L::BorderLeftColor => V::BorderLeftColor(parse_color(p)?),
            L::BorderTopLeftRadius => V::BorderTopLeftRadius(parse_corner_radius(p)?),
            L::BorderTopRightRadius => V::BorderTopRightRadius(parse_corner_radius(p)?),
            L::BorderBottomRightRadius => V::BorderBottomRightRadius(parse_corner_radius(p)?),
            L::BorderBottomLeftRadius => V::BorderBottomLeftRadius(parse_corner_radius(p)?),
            L::OutlineWidth => V::OutlineWidth(parse_line_width(p, false)?),
            L::OutlineStyle => V::OutlineStyle(parse_outline_style(p)?),
            L::OutlineColor => V::OutlineColor(parse_outline_color(p)?),
            L::OutlineOffset => {
                V::OutlineOffset(parse_length_percentage(p, LengthOptions::LENGTH)?)
            }
            L::BackgroundColor => V::BackgroundColor(parse_color(p)?),
            L::BackgroundImage => V::BackgroundImage(list(p, |p| parse_image_or_none(p, cx))?),
            L::BackgroundPositionX => {
                V::BackgroundPositionX(list(p, |p| parse_position_axis(p, true, quirky))?)
            }
            L::BackgroundPositionY => {
                V::BackgroundPositionY(list(p, |p| parse_position_axis(p, false, quirky))?)
            }
            L::BackgroundSize => V::BackgroundSize(list(p, parse_background_size)?),
            L::BackgroundRepeat => V::BackgroundRepeat(list(p, parse_background_repeat)?),
            L::BackgroundOrigin => V::BackgroundOrigin(list(p, parse_box)?),
            L::BackgroundClip => V::BackgroundClip(list(p, parse_box)?),
            L::BackgroundAttachment => {
                V::BackgroundAttachment(list(p, |p| keyword(p, BackgroundAttachment::from_ident))?)
            }
            L::MaskImage
            | L::MaskMode
            | L::MaskPositionX
            | L::MaskPositionY
            | L::MaskSize
            | L::MaskRepeat
            | L::MaskOrigin
            | L::MaskClip
            | L::MaskComposite => super::mask::parse_longhand(id, p, cx)?,
            L::OverflowX => V::OverflowX(keyword(p, Overflow::from_ident)?),
            L::OverflowY => V::OverflowY(keyword(p, Overflow::from_ident)?),
            L::TextOverflow => V::TextOverflow(keyword(p, TextOverflow::from_ident)?),
            L::Opacity => V::Opacity(parse_opacity(p)?),
            L::VerticalAlign => V::VerticalAlign(parse_vertical_align(p, quirky)?),
            L::TextDecorationLine => V::TextDecorationLine(parse_text_decoration_line(p)?),
            L::TextDecorationColor => V::TextDecorationColor(parse_color(p)?),
            L::TextDecorationStyle => {
                V::TextDecorationStyle(keyword(p, TextDecorationStyle::from_ident)?)
            }
            L::FlexDirection => V::FlexDirection(keyword(p, FlexDirection::from_ident)?),
            L::FlexWrap => V::FlexWrap(keyword(p, FlexWrap::from_ident)?),
            L::FlexGrow => V::FlexGrow(parse_non_negative_number(p)?),
            L::FlexShrink => V::FlexShrink(parse_non_negative_number(p)?),
            L::FlexBasis => V::FlexBasis(parse_flex_basis(p)?),
            L::Order => V::Order(parse_integer(p)?),
            L::JustifyContent => V::JustifyContent(parse_alignment(p, AlignKind::JustifyContent)?),
            L::AlignItems => V::AlignItems(parse_alignment(p, AlignKind::AlignItems)?),
            L::AlignSelf => V::AlignSelf(parse_alignment(p, AlignKind::AlignSelf)?),
            L::AlignContent => V::AlignContent(parse_alignment(p, AlignKind::AlignContent)?),
            L::RowGap => V::RowGap(parse_gap(p)?),
            L::ColumnGap => V::ColumnGap(parse_gap(p)?),
            L::ColumnWidth => V::ColumnWidth(parse_column_width(p)?),
            L::ColumnCount => V::ColumnCount(parse_column_count(p)?),
            L::JustifyItems => V::JustifyItems(parse_alignment(p, AlignKind::JustifyItems)?),
            L::JustifySelf => V::JustifySelf(parse_alignment(p, AlignKind::JustifySelf)?),
            L::GridTemplateColumns => V::GridTemplateColumns(grid::parse_track_list(p)?),
            L::GridTemplateRows => V::GridTemplateRows(grid::parse_track_list(p)?),
            L::GridTemplateAreas => V::GridTemplateAreas(grid::parse_template_areas(p)?),
            L::GridAutoColumns => V::GridAutoColumns(grid::parse_track_sizes(p)?),
            L::GridAutoRows => V::GridAutoRows(grid::parse_track_sizes(p)?),
            L::GridAutoFlow => V::GridAutoFlow(grid::parse_auto_flow(p)?),
            L::GridRowStart => V::GridRowStart(grid::parse_grid_line(p)?),
            L::GridRowEnd => V::GridRowEnd(grid::parse_grid_line(p)?),
            L::GridColumnStart => V::GridColumnStart(grid::parse_grid_line(p)?),
            L::GridColumnEnd => V::GridColumnEnd(grid::parse_grid_line(p)?),
            L::TableLayout => V::TableLayout(keyword(p, TableLayout::from_ident)?),
            L::Content => V::Content(parse_content(p, cx)?),
            L::CounterReset => V::CounterReset(parse_counter_list(p, 0)?),
            L::CounterIncrement => V::CounterIncrement(parse_counter_list(p, 1)?),
            L::CounterSet => V::CounterSet(parse_counter_list(p, 0)?),
            L::Contain => V::Contain(parse_contain(p)?),
            L::ContainIntrinsicWidth => {
                V::ContainIntrinsicWidth(parse_contain_intrinsic(p, quirky)?)
            }
            L::ContainIntrinsicHeight => {
                V::ContainIntrinsicHeight(parse_contain_intrinsic(p, quirky)?)
            }
            L::ObjectFit => V::ObjectFit(keyword(p, ObjectFit::from_ident)?),
            L::ObjectPosition => {
                let (x, y) = parse_position(p)?;
                V::ObjectPosition([x, y])
            }
            L::UserSelect => V::UserSelect(parse_user_select(p)?),
            L::UnicodeBidi => V::UnicodeBidi(parse_unicode_bidi(p)?),
        })
    })
}

// ----- Generic helpers -----

/// Consumes an identifier and maps it with `from_ident`.
pub(crate) fn keyword<T>(
    p: &mut Parser<'_>,
    from_ident: impl FnOnce(&str) -> Option<T>,
) -> ParseResult<T> {
    p.try_parse(|p| from_ident(p.expect_ident()?).ok_or(ParseError::Unexpected))
}

/// Parses a comma-separated list into a shared slice.
pub(crate) fn list<'i, T>(
    p: &mut Parser<'i>,
    f: impl FnMut(&mut Parser<'i>) -> ParseResult<T>,
) -> ParseResult<Arc<[T]>> {
    Ok(Arc::from(p.parse_comma_separated(f)?))
}

/// `<length-percentage> | auto`; `auto` is `None`.
pub(crate) fn lp_or_auto(p: &mut Parser<'_>, quirky: bool) -> ParseResult<Option<Lp>> {
    if p.expect_ident_matching("auto").is_ok() {
        return Ok(None);
    }
    parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE.with_quirks(quirky)).map(Some)
}

/// `<length-percentage [0,∞]>` (padding).
pub(crate) fn padding(p: &mut Parser<'_>, quirky: bool) -> ParseResult<Lp> {
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE.with_quirks(quirky))
}

/// `<length [0,∞]>`.
pub(crate) fn non_negative_length(p: &mut Parser<'_>, quirky: bool) -> ParseResult<Lp> {
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE_LENGTH.with_quirks(quirky))
}

/// `none | <image>`; `none` (and images swb cannot render) is `None`.
pub(crate) fn parse_image_or_none(
    p: &mut Parser<'_>,
    cx: &ParserContext,
) -> ParseResult<Option<SpecifiedImage>> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(None);
    }
    parse_image(p, cx)
}

// ----- Fonts -----

/// `font-family`: a comma-separated list of family names and generic
/// families. <https://www.w3.org/TR/css-fonts-4/#font-family-prop>
pub(crate) fn parse_font_family(p: &mut Parser<'_>) -> ParseResult<Arc<[FontFamily]>> {
    list(p, |item| {
        if let Ok(s) = item.expect_string() {
            return Ok(FontFamily::Named(Arc::from(s)));
        }
        let mut words = vec![item.expect_ident()?];
        while let Ok(w) = item.expect_ident() {
            words.push(w);
        }
        if let [single] = words.as_slice() {
            if let Some(g) = GenericFamily::from_ident(single) {
                return Ok(FontFamily::Generic(g));
            }
            if is_reserved_ident(single) {
                return Err(ParseError::Invalid);
            }
        }
        Ok(FontFamily::Named(Arc::from(words.join(" "))))
    })
}

/// True for the identifiers that cannot be a family or counter style
/// name: the CSS-wide keywords and `default`.
fn is_reserved_ident(ident: &str) -> bool {
    CssWideKeyword::from_ident(ident).is_some() || ident.eq_ignore_ascii_case("default")
}

/// `font-size`. <https://www.w3.org/TR/css-fonts-4/#font-size-prop>
pub(crate) fn parse_font_size(p: &mut Parser<'_>, quirky: bool) -> ParseResult<SpecifiedFontSize> {
    if let Ok(k) = keyword(p, FontSizeKeyword::from_ident) {
        return Ok(SpecifiedFontSize::Keyword(k));
    }
    if let Ok(v) = p.expect_one_of(&[("larger", 0), ("smaller", 1), ("math", 2)]) {
        return Ok(match v {
            0 => SpecifiedFontSize::Larger,
            1 => SpecifiedFontSize::Smaller,
            _ => SpecifiedFontSize::Math,
        });
    }
    let lp = parse_length_percentage(p, LengthOptions::NON_NEGATIVE.with_quirks(quirky))?;
    Ok(SpecifiedFontSize::Length(lp))
}

/// `font-weight`. <https://www.w3.org/TR/css-fonts-4/#font-weight-prop>
pub(crate) fn parse_font_weight(p: &mut Parser<'_>) -> ParseResult<SpecifiedFontWeight> {
    if let Ok(v) = p.expect_one_of(&[
        ("normal", SpecifiedFontWeight::Absolute(400.0)),
        ("bold", SpecifiedFontWeight::Absolute(700.0)),
        ("bolder", SpecifiedFontWeight::Bolder),
        ("lighter", SpecifiedFontWeight::Lighter),
    ]) {
        return Ok(v);
    }
    p.try_parse(|p| {
        let w = parse_number(p)?;
        if (1.0..=1000.0).contains(&w) {
            Ok(SpecifiedFontWeight::Absolute(w))
        } else {
            Err(ParseError::Invalid)
        }
    })
}

/// `font-style`: `normal | italic | oblique <angle>?`.
pub(crate) fn parse_font_style(p: &mut Parser<'_>) -> ParseResult<FontStyle> {
    let style = keyword(p, FontStyle::from_ident)?;
    if style == FontStyle::Oblique
        && let Ok(angle) = p.try_parse(|p| parse_angle(p, false))
        && !(-90.0..=90.0).contains(&angle)
    {
        return Err(ParseError::Invalid);
    }
    Ok(style)
}

/// `font-stretch` (`font-width`): a keyword or a percentage.
pub(crate) fn parse_font_stretch(p: &mut Parser<'_>) -> ParseResult<f32> {
    if let Ok(v) = p.expect_one_of(&[
        ("ultra-condensed", 50.0),
        ("extra-condensed", 62.5),
        ("condensed", 75.0),
        ("semi-condensed", 87.5),
        ("normal", 100.0),
        ("semi-expanded", 112.5),
        ("expanded", 125.0),
        ("extra-expanded", 150.0),
        ("ultra-expanded", 200.0),
    ]) {
        return Ok(v);
    }
    p.try_parse(|p| {
        let v = p.expect_percentage()?;
        if v >= 0.0 {
            Ok(v)
        } else {
            Err(ParseError::Invalid)
        }
    })
}

/// `line-height`: `normal | <number [0,∞]> | <length-percentage [0,∞]>`.
pub(crate) fn parse_line_height(p: &mut Parser<'_>) -> ParseResult<SpecifiedLineHeight> {
    if p.expect_ident_matching("normal").is_ok() {
        return Ok(SpecifiedLineHeight::Normal);
    }
    if let Ok(n) = parse_non_negative_number(p) {
        return Ok(SpecifiedLineHeight::Number(n));
    }
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE)
        .map(SpecifiedLineHeight::LengthPercentage)
}

// ----- Text -----

/// `text-align`, including the `-webkit-` keywords.
fn parse_text_align(p: &mut Parser<'_>, cx: &ParserContext) -> ParseResult<SpecifiedTextAlign> {
    p.try_parse(|p| {
        let ident = p.expect_ident()?;
        if let Some(k) = TextAlign::from_ident(ident) {
            return Ok(SpecifiedTextAlign::Keyword(k));
        }
        let k = match ident.to_ascii_lowercase().as_str() {
            "justify-all" => TextAlign::Justify,
            "-webkit-auto" => TextAlign::Start,
            "-webkit-match-parent" => TextAlign::MatchParent,
            "-moz-center" | "-khtml-center" => TextAlign::WebkitCenter,
            "-moz-left" | "-khtml-left" => TextAlign::WebkitLeft,
            "-moz-right" | "-khtml-right" => TextAlign::WebkitRight,
            "-internal-center" if cx.origin == Origin::UserAgent => {
                return Ok(SpecifiedTextAlign::InternalCenter);
            }
            _ => return Err(ParseError::Unexpected),
        };
        Ok(SpecifiedTextAlign::Keyword(k))
    })
}

/// `text-indent`: `<length-percentage> && hanging? && each-line?`. The
/// keywords are accepted and ignored.
fn parse_text_indent(p: &mut Parser<'_>, quirky: bool) -> ParseResult<Lp> {
    let mut value = None;
    for _ in 0..3 {
        if value.is_none()
            && let Ok(v) =
                parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE.with_quirks(quirky))
        {
            value = Some(v);
            continue;
        }
        if p.expect_one_of(&[("hanging", ()), ("each-line", ())])
            .is_err()
        {
            break;
        }
    }
    value.ok_or(ParseError::Invalid)
}

/// `white-space`: the CSS 2 keywords, and the CSS Text 4 shorthand form
/// (`<white-space-collapse> || <text-wrap-mode> || <white-space-trim>`).
/// <https://www.w3.org/TR/css-text-4/#white-space-property>
fn parse_white_space(p: &mut Parser<'_>) -> ParseResult<WhiteSpace> {
    let single = p.try_parse(|p| {
        let v = keyword(p, |i| {
            WhiteSpace::from_ident(i).or_else(|| match i.to_ascii_lowercase().as_str() {
                "-webkit-nowrap" => Some(WhiteSpace::Nowrap),
                "-moz-pre-wrap" | "-o-pre-wrap" | "-pre-wrap" => Some(WhiteSpace::PreWrap),
                _ => None,
            })
        })?;
        p.expect_exhausted()?;
        Ok::<_, ParseError>(v)
    });
    if let Ok(v) = single {
        return Ok(v);
    }
    let mut collapse = None;
    let mut wrap = None;
    let mut trim = false;
    while !p.is_exhausted() {
        if collapse.is_none()
            && let Ok(c) = p.expect_one_of(&[
                ("collapse", SpaceHandling::Collapse),
                ("preserve", SpaceHandling::Preserve),
                ("preserve-breaks", SpaceHandling::PreserveBreaks),
                ("preserve-spaces", SpaceHandling::Preserve),
                ("break-spaces", SpaceHandling::BreakSpaces),
            ])
        {
            collapse = Some(c);
        } else if wrap.is_none()
            && let Ok(w) = p.expect_one_of(&[("wrap", true), ("nowrap", false)])
        {
            wrap = Some(w);
        } else if !trim
            && p.expect_one_of(&[
                ("none", ()),
                ("discard-before", ()),
                ("discard-after", ()),
                ("discard-inner", ()),
            ])
            .is_ok()
        {
            trim = true;
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    if collapse.is_none() && wrap.is_none() && !trim {
        return Err(ParseError::Invalid);
    }
    Ok(
        match (
            collapse.unwrap_or(SpaceHandling::Collapse),
            wrap.unwrap_or(true),
        ) {
            (SpaceHandling::Collapse, true) => WhiteSpace::Normal,
            (SpaceHandling::Collapse | SpaceHandling::PreserveBreaks, false) => WhiteSpace::Nowrap,
            (SpaceHandling::Preserve, true) => WhiteSpace::PreWrap,
            (SpaceHandling::Preserve | SpaceHandling::BreakSpaces, false) => WhiteSpace::Pre,
            (SpaceHandling::PreserveBreaks, true) => WhiteSpace::PreLine,
            (SpaceHandling::BreakSpaces, true) => WhiteSpace::BreakSpaces,
        },
    )
}

/// `<white-space-collapse>` values.
#[derive(Clone, Copy, PartialEq)]
enum SpaceHandling {
    Collapse,
    Preserve,
    PreserveBreaks,
    BreakSpaces,
}

/// `letter-spacing` (`normal | <length-percentage>`) and `word-spacing`
/// (`normal | <length>`). `normal` is zero.
fn parse_spacing(p: &mut Parser<'_>, quirky: bool, percentage: bool) -> ParseResult<Lp> {
    if p.expect_ident_matching("normal").is_ok() {
        return Ok(Lp::ZERO);
    }
    let opts = LengthOptions {
        percentage,
        negative: true,
        quirky,
    };
    parse_length_percentage(p, opts)
}

/// `word-break`. `auto-phrase` is treated as `normal`.
fn parse_word_break(p: &mut Parser<'_>) -> ParseResult<WordBreak> {
    keyword(p, |i| {
        WordBreak::from_ident(i).or_else(|| {
            i.eq_ignore_ascii_case("auto-phrase")
                .then_some(WordBreak::Normal)
        })
    })
}

/// `text-decoration-line`: `none | [underline || overline || line-through
/// || blink]`. `blink` and the spelling/grammar error values are accepted
/// and ignored.
pub(crate) fn parse_text_decoration_line(p: &mut Parser<'_>) -> ParseResult<TextDecorationLine> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(TextDecorationLine::empty());
    }
    let mut lines = TextDecorationLine::empty();
    let mut blink = false;
    let mut any = false;
    loop {
        let flag = p.expect_one_of(&[
            ("underline", TextDecorationLine::UNDERLINE),
            ("overline", TextDecorationLine::OVERLINE),
            ("line-through", TextDecorationLine::LINE_THROUGH),
            ("blink", TextDecorationLine::empty()),
            ("spelling-error", TextDecorationLine::empty()),
            ("grammar-error", TextDecorationLine::empty()),
        ]);
        let Ok(flag) = flag else {
            break;
        };
        if flag.is_empty() {
            if blink {
                return Err(ParseError::Invalid);
            }
            blink = true;
        } else if lines.contains(flag) {
            return Err(ParseError::Invalid);
        }
        lines |= flag;
        any = true;
    }
    if any {
        Ok(lines)
    } else {
        Err(ParseError::Unexpected)
    }
}

/// `vertical-align`: a keyword or a length-percentage.
fn parse_vertical_align(p: &mut Parser<'_>, quirky: bool) -> ParseResult<SpecifiedVerticalAlign> {
    if let Ok(k) = keyword(p, |i| {
        VerticalAlignKeyword::from_ident(i).or_else(|| {
            i.eq_ignore_ascii_case("-webkit-baseline-middle")
                .then_some(VerticalAlignKeyword::Middle)
        })
    }) {
        return Ok(SpecifiedVerticalAlign::Keyword(k));
    }
    parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE.with_quirks(quirky))
        .map(SpecifiedVerticalAlign::LengthPercentage)
}

/// `cursor`: `[<url> [<x> <y>]?,]* <keyword>`. The images are skipped.
fn parse_cursor(p: &mut Parser<'_>, cx: &ParserContext) -> ParseResult<Cursor> {
    loop {
        let skipped = p.try_parse(|p| {
            parse_image(p, cx)?;
            let _ = p.try_parse(|p| Ok::<_, ParseError>((parse_number(p)?, parse_number(p)?)));
            p.expect_comma()
        });
        if skipped.is_err() {
            break;
        }
    }
    keyword(p, |i| {
        Cursor::from_ident(i).or_else(|| match i.to_ascii_lowercase().as_str() {
            "hand" => Some(Cursor::Pointer),
            "-webkit-grab" | "-moz-grab" => Some(Cursor::Grab),
            "-webkit-grabbing" | "-moz-grabbing" => Some(Cursor::Grabbing),
            "-webkit-zoom-in" | "-moz-zoom-in" => Some(Cursor::ZoomIn),
            "-webkit-zoom-out" | "-moz-zoom-out" => Some(Cursor::ZoomOut),
            _ => None,
        })
    })
}

fn parse_user_select(p: &mut Parser<'_>) -> ParseResult<UserSelect> {
    keyword(p, |i| {
        UserSelect::from_ident(i).or_else(|| {
            i.eq_ignore_ascii_case("-moz-none")
                .then_some(UserSelect::None)
        })
    })
}

fn parse_unicode_bidi(p: &mut Parser<'_>) -> ParseResult<UnicodeBidi> {
    keyword(p, |i| {
        UnicodeBidi::from_ident(i).or_else(|| match i.to_ascii_lowercase().as_str() {
            "-webkit-isolate" | "-moz-isolate" => Some(UnicodeBidi::Isolate),
            "-webkit-isolate-override" | "-moz-isolate-override" => {
                Some(UnicodeBidi::IsolateOverride)
            }
            "-webkit-plaintext" | "-moz-plaintext" => Some(UnicodeBidi::Plaintext),
            _ => None,
        })
    })
}

/// `pointer-events`: the SVG-only values behave as `auto` on HTML content.
fn parse_pointer_events(p: &mut Parser<'_>) -> ParseResult<PointerEvents> {
    keyword(p, |i| {
        PointerEvents::from_ident(i).or_else(|| {
            [
                "all",
                "visiblepainted",
                "visiblefill",
                "visiblestroke",
                "visible",
                "painted",
                "fill",
                "stroke",
                "bounding-box",
            ]
            .iter()
            .any(|k| k.eq_ignore_ascii_case(i))
            .then_some(PointerEvents::Auto)
        })
    })
}

// ----- Lists and generated content -----

/// `list-style-type`: a counter style name or a string. Unknown counter
/// style names compute to `decimal` (CSS Counter Styles 3, section 3.1);
/// strings and `symbols()` are not supported and compute to `none`.
pub(crate) fn parse_list_style_type(p: &mut Parser<'_>) -> ParseResult<ListStyleType> {
    if p.expect_string().is_ok() || p.expect_function_matching("symbols").is_ok() {
        return Ok(ListStyleType::None);
    }
    parse_counter_style_name(p)
}

/// A `<counter-style-name>`: unknown names compute to `decimal`.
fn parse_counter_style_name(p: &mut Parser<'_>) -> ParseResult<ListStyleType> {
    let ident = p.expect_ident()?;
    if let Some(t) = ListStyleType::from_ident(ident) {
        return Ok(t);
    }
    if is_reserved_ident(ident) {
        return Err(ParseError::Invalid);
    }
    Ok(ListStyleType::Decimal)
}

/// The maximum number of counters in one `counter-reset`,
/// `counter-increment` or `counter-set` value. Later ones are dropped, so
/// that a long value on many elements cannot make the counter pass slow.
const MAX_COUNTERS_PER_VALUE: usize = 256;

/// `counter-reset`, `counter-increment` and `counter-set`:
/// `none | [<counter-name> <integer>?]+`, where the integer defaults to
/// `default`. `reversed()` is not supported (as in Chromium 148, where
/// `@supports` rejects it).
/// <https://www.w3.org/TR/css-lists-3/#counter-properties>
fn parse_counter_list(p: &mut Parser<'_>, default: i32) -> ParseResult<CounterList> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(CounterList::default());
    }
    let mut entries = Vec::new();
    while let Ok(name) = p.try_parse(parse_counter_name) {
        let value = p.try_parse(parse_integer).unwrap_or(default);
        entries.push((name, value));
    }
    if entries.is_empty() {
        return Err(ParseError::Unexpected);
    }
    if entries.len() > MAX_COUNTERS_PER_VALUE {
        log::warn!(
            "a counter property names {} counters; only the first {MAX_COUNTERS_PER_VALUE} are used",
            entries.len()
        );
        entries.truncate(MAX_COUNTERS_PER_VALUE);
    }
    Ok(CounterList::new(entries))
}

/// A `<counter-name>` in the counter properties: a `<custom-ident>` other
/// than `none`. Names are case-sensitive.
fn parse_counter_name(p: &mut Parser<'_>) -> ParseResult<Arc<str>> {
    let ident = p.expect_ident()?;
    if is_reserved_ident(ident) || ident.eq_ignore_ascii_case("none") {
        return Err(ParseError::Invalid);
    }
    Ok(Arc::from(ident))
}

/// `content`. <https://www.w3.org/TR/css-content-3/#content-property>
fn parse_content(p: &mut Parser<'_>, cx: &ParserContext) -> ParseResult<SpecifiedContent> {
    if p.expect_ident_matching("normal").is_ok() {
        return Ok(SpecifiedContent::Normal);
    }
    if p.expect_ident_matching("none").is_ok() {
        return Ok(SpecifiedContent::None);
    }
    let mut items = Vec::new();
    // `no-open-quote` and `no-close-quote` produce no item but count.
    let mut parsed = 0;
    loop {
        if let Ok(s) = p.expect_string() {
            items.push(SpecifiedContentItem::String(Arc::from(s)));
        } else if let Ok(q) = p.expect_one_of(&[
            ("open-quote", Some(true)),
            ("close-quote", Some(false)),
            ("no-open-quote", None),
            ("no-close-quote", None),
        ]) {
            items.extend(q.map(|open| {
                if open {
                    SpecifiedContentItem::OpenQuote
                } else {
                    SpecifiedContentItem::CloseQuote
                }
            }));
        } else if let Ok(item) = p.try_parse(parse_content_function) {
            items.push(item);
        } else if let Ok(image) = parse_image(p, cx) {
            items.push(SpecifiedContentItem::Image(image));
        } else {
            break;
        }
        parsed += 1;
    }
    if parsed == 0 {
        return Err(ParseError::Unexpected);
    }
    // Alternative text after `/` is accepted and ignored.
    if p.expect_delim('/').is_ok() {
        let mut any = false;
        while p.expect_string().is_ok() || p.try_parse(parse_content_function).is_ok() {
            any = true;
        }
        if !any {
            return Err(ParseError::Invalid);
        }
    }
    Ok(SpecifiedContent::Items(Arc::from(items)))
}

/// `attr()`, `counter()` and `counters()` in `content`.
fn parse_content_function(p: &mut Parser<'_>) -> ParseResult<SpecifiedContentItem> {
    let (name, mut args) = p.expect_function()?;
    match name.to_ascii_lowercase().as_str() {
        "attr" => {
            let attr = args.expect_ident()?;
            // The attribute type and fallback (CSS Values 5) are ignored.
            Ok(SpecifiedContentItem::Attr(Arc::from(
                attr.to_ascii_lowercase(),
            )))
        }
        // https://www.w3.org/TR/css-lists-3/#counter-functions. The name is
        // a `<custom-ident>` (Chromium also accepts `none` here). The style
        // is a counter style name: strings and `symbols()` are invalid, as
        // in Chromium 148.
        "counter" | "counters" => {
            let counter = args.expect_ident()?;
            if is_reserved_ident(counter) {
                return Err(ParseError::Invalid);
            }
            let separator = if name.eq_ignore_ascii_case("counters") {
                args.expect_comma()?;
                Some(Arc::from(args.expect_string()?))
            } else {
                None
            };
            let style = if args.expect_comma().is_ok() {
                parse_counter_style_name(&mut args)?
            } else {
                ListStyleType::Decimal
            };
            args.expect_exhausted()?;
            Ok(SpecifiedContentItem::Counter {
                name: Arc::from(counter),
                separator,
                style,
            })
        }
        _ => Err(ParseError::Unexpected),
    }
}

// ----- Box model -----

/// `<line-width>`: `thin | medium | thick | <length [0,∞]>`.
pub(crate) fn parse_line_width(p: &mut Parser<'_>, quirky: bool) -> ParseResult<Lp> {
    if let Ok(px) = p.expect_one_of(&[("thin", 1.0), ("medium", 3.0), ("thick", 5.0)]) {
        return Ok(Lp::Length(Length::px(px)));
    }
    non_negative_length(p, quirky)
}

/// `<line-style>`.
pub(crate) fn parse_border_style(p: &mut Parser<'_>) -> ParseResult<BorderStyle> {
    keyword(p, BorderStyle::from_ident)
}

/// `outline-style`: `auto | <outline-line-style>` (`hidden` is not
/// allowed).
pub(crate) fn parse_outline_style(p: &mut Parser<'_>) -> ParseResult<OutlineStyle> {
    keyword(p, OutlineStyle::from_ident)
}

/// `outline-color`: a color, or `invert` (drawn with `currentColor`).
pub(crate) fn parse_outline_color(p: &mut Parser<'_>) -> ParseResult<crate::values::Color> {
    if p.expect_ident_matching("invert").is_ok() || p.expect_ident_matching("auto").is_ok() {
        return Ok(crate::values::Color::CurrentColor);
    }
    parse_color(p)
}

/// One corner of `border-radius`: one or two non-negative
/// length-percentages.
fn parse_corner_radius(p: &mut Parser<'_>) -> ParseResult<(Lp, Lp)> {
    let h = parse_length_percentage(p, LengthOptions::NON_NEGATIVE)?;
    let v = parse_length_percentage(p, LengthOptions::NON_NEGATIVE).unwrap_or_else(|_| h.clone());
    Ok((h, v))
}

/// `width`, `height`, `min-*` and `max-*`.
/// <https://www.w3.org/TR/css-sizing-3/#sizing-properties>
///
/// `stretch`, `-webkit-fill-available` and `-moz-available` compute to
/// `auto` (`none` for the maximums): swb has no stretch sizing.
fn parse_size(p: &mut Parser<'_>, quirky: bool, max: bool) -> ParseResult<SpecifiedSize> {
    if let Ok(lp) = parse_length_percentage(p, LengthOptions::NON_NEGATIVE.with_quirks(quirky)) {
        return Ok(SpecifiedSize::LengthPercentage(lp));
    }
    if let Ok(mut args) = p.expect_function_matching("fit-content") {
        let lp =
            args.parse_entirely(|a| parse_length_percentage(a, LengthOptions::NON_NEGATIVE))?;
        return Ok(SpecifiedSize::FitContent(Some(lp)));
    }
    let auto = if max {
        SpecifiedSize::None
    } else {
        SpecifiedSize::Auto
    };
    p.try_parse(|p| {
        let ident = p.expect_ident()?.to_ascii_lowercase();
        Ok(match ident.as_str() {
            "auto" if !max => SpecifiedSize::Auto,
            "none" if max => SpecifiedSize::None,
            "min-content" | "-webkit-min-content" | "-moz-min-content" => SpecifiedSize::MinContent,
            "max-content" | "-webkit-max-content" | "-moz-max-content" => SpecifiedSize::MaxContent,
            "fit-content" | "-webkit-fit-content" | "-moz-fit-content" => {
                SpecifiedSize::FitContent(None)
            }
            "stretch" | "-webkit-fill-available" | "-moz-available" => auto.clone(),
            _ => return Err(ParseError::Unexpected),
        })
    })
}

/// `aspect-ratio`: `auto || <ratio>`. A degenerate ratio (with a zero) is
/// kept as no ratio, so the value behaves as `auto`.
fn parse_aspect_ratio(p: &mut Parser<'_>) -> ParseResult<AspectRatio> {
    let auto = p.expect_ident_matching("auto").is_ok();
    let ratio = p.try_parse(|p| {
        let w = parse_non_negative_number(p)?;
        let h = if p.expect_delim('/').is_ok() {
            parse_non_negative_number(p)?
        } else {
            1.0
        };
        Ok::<_, ParseError>((w, h))
    });
    let auto = auto || p.expect_ident_matching("auto").is_ok();
    let ratio = match ratio {
        Ok((w, h)) if w > 0.0 && h > 0.0 => Some(w / h),
        Ok(_) => None,
        Err(_) if auto => None,
        Err(e) => return Err(e),
    };
    Ok(AspectRatio { auto, ratio })
}

/// `contain`: `none | strict | content | [size || layout || style || paint
/// || inline-size]`. `size` and `inline-size` exclude each other, and a
/// type cannot repeat.
/// <https://www.w3.org/TR/css-contain-2/#contain-property>
fn parse_contain(p: &mut Parser<'_>) -> ParseResult<Contain> {
    let first = p.expect_ident()?.to_ascii_lowercase();
    match first.as_str() {
        "none" => return Ok(Contain::NONE),
        "strict" => return Ok(Contain::STRICT),
        "content" => return Ok(Contain::CONTENT),
        _ => {}
    }
    let mut contain = Contain::NONE;
    let mut word = Some(first);
    while let Some(name) = word {
        let flag = match name.as_str() {
            "size" => &mut contain.size,
            "inline-size" => &mut contain.inline_size,
            "layout" => &mut contain.layout,
            "style" => &mut contain.style,
            "paint" => &mut contain.paint,
            _ => return Err(ParseError::Unexpected),
        };
        if std::mem::replace(flag, true) {
            return Err(ParseError::Invalid);
        }
        word = p.expect_ident().ok().map(str::to_ascii_lowercase);
    }
    if contain.size && contain.inline_size {
        return Err(ParseError::Invalid);
    }
    Ok(contain)
}

/// `contain-intrinsic-width` and `-height`: `auto? [none | <length>]`.
/// <https://www.w3.org/TR/css-sizing-4/#intrinsic-size-override>
pub(crate) fn parse_contain_intrinsic(
    p: &mut Parser<'_>,
    quirky: bool,
) -> ParseResult<SpecifiedContainIntrinsic> {
    let auto = p.expect_ident_matching("auto").is_ok();
    if p.expect_ident_matching("none").is_ok() {
        return Ok(SpecifiedContainIntrinsic { auto, length: None });
    }
    let length = non_negative_length(p, quirky)?;
    Ok(SpecifiedContainIntrinsic {
        auto,
        length: Some(length),
    })
}

/// `z-index`: `auto | <integer>`.
fn parse_z_index(p: &mut Parser<'_>) -> ParseResult<ZIndex> {
    if p.expect_ident_matching("auto").is_ok() {
        return Ok(ZIndex::Auto);
    }
    parse_integer(p).map(ZIndex::Integer)
}

/// `opacity`: a number or a percentage (clamped when computed).
fn parse_opacity(p: &mut Parser<'_>) -> ParseResult<f32> {
    if let Ok(v) = p.expect_percentage() {
        return Ok(v / 100.0);
    }
    parse_number(p)
}

/// `caption-side`: `top | bottom` and the logical `block-start |
/// block-end`.
fn parse_caption_side(p: &mut Parser<'_>) -> ParseResult<CaptionSide> {
    keyword(p, |i| {
        CaptionSide::from_ident(i).or_else(|| match i.to_ascii_lowercase().as_str() {
            "block-start" => Some(CaptionSide::Top),
            "block-end" => Some(CaptionSide::Bottom),
            _ => None,
        })
    })
}

/// `display`, including the two-value syntax and legacy keywords.
/// <https://www.w3.org/TR/css-display-3/#the-display-properties>
fn parse_display(p: &mut Parser<'_>) -> ParseResult<Display> {
    let mut words = Vec::new();
    while let Ok(w) = p.expect_ident() {
        words.push(w.to_ascii_lowercase());
        if words.len() > 3 {
            return Err(ParseError::Invalid);
        }
    }
    match words.as_slice() {
        [] => Err(ParseError::Unexpected),
        [single] => single_display_keyword(single).ok_or(ParseError::Invalid),
        multiple => multi_keyword_display(multiple).ok_or(ParseError::Invalid),
    }
}

fn single_display_keyword(word: &str) -> Option<Display> {
    if let Some(d) = Display::from_ident(word) {
        return Some(d);
    }
    Some(match word {
        "flow" | "run-in" => Display::Block,
        "ruby" | "ruby-base" | "ruby-text" | "ruby-base-container" | "ruby-text-container" => {
            Display::Inline
        }
        "-webkit-box" | "-webkit-flex" | "-moz-box" | "-ms-flexbox" => Display::Flex,
        "-webkit-inline-box" | "-webkit-inline-flex" | "-moz-inline-box" | "-ms-inline-flexbox" => {
            Display::InlineFlex
        }
        "-ms-grid" => Display::Grid,
        "-ms-inline-grid" => Display::InlineGrid,
        _ => return None,
    })
}

/// The multi-keyword `display` syntax: `<display-outside> ||
/// <display-inside>` and `<display-outside>? && [flow | flow-root]? &&
/// list-item`. `inline list-item` maps to `list-item` (swb has no inline
/// list items).
fn multi_keyword_display(words: &[String]) -> Option<Display> {
    let mut outside = None;
    let mut inside = None;
    let mut list_item = false;
    for w in words {
        match w.as_str() {
            "block" | "inline" | "run-in" if outside.is_none() => outside = Some(w.as_str()),
            "flow" | "flow-root" | "table" | "flex" | "grid" | "ruby" if inside.is_none() => {
                inside = Some(w.as_str());
            }
            "list-item" if !list_item => list_item = true,
            _ => return None,
        }
    }
    if list_item {
        return matches!(inside, None | Some("flow" | "flow-root")).then_some(Display::ListItem);
    }
    let inline = outside == Some("inline");
    Some(match (inline, inside.unwrap_or("flow")) {
        (_, "ruby") | (true, "flow") => Display::Inline,
        (false, "flow") => Display::Block,
        (false, "flow-root") => Display::FlowRoot,
        (true, "flow-root") => Display::InlineBlock,
        (false, "table") => Display::Table,
        (true, "table") => Display::InlineTable,
        (false, "flex") => Display::Flex,
        (true, "flex") => Display::InlineFlex,
        (false, "grid") => Display::Grid,
        (true, "grid") => Display::InlineGrid,
        _ => return None,
    })
}

// ----- Backgrounds -----

/// One keyword or length of a `<bg-position>`.
#[derive(Clone, Debug, PartialEq)]
enum PositionItem {
    Left,
    Right,
    Top,
    Bottom,
    Center,
    Length(Lp),
}

impl PositionItem {
    fn is_horizontal_keyword(&self) -> bool {
        matches!(self, PositionItem::Left | PositionItem::Right)
    }

    fn is_vertical_keyword(&self) -> bool {
        matches!(self, PositionItem::Top | PositionItem::Bottom)
    }

    fn is_keyword(&self) -> bool {
        !matches!(self, PositionItem::Length(_))
    }
}

fn position_item(p: &mut Parser<'_>, quirky: bool) -> ParseResult<PositionItem> {
    if let Ok(k) = p.expect_one_of(&[
        ("left", 0),
        ("right", 1),
        ("top", 2),
        ("bottom", 3),
        ("center", 4),
    ]) {
        return Ok(match k {
            0 => PositionItem::Left,
            1 => PositionItem::Right,
            2 => PositionItem::Top,
            3 => PositionItem::Bottom,
            _ => PositionItem::Center,
        });
    }
    parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE.with_quirks(quirky))
        .map(PositionItem::Length)
}

/// A position from a keyword and an optional offset.
fn position_from(keyword: &PositionItem, offset: Option<Lp>) -> SpecifiedPosition {
    match (keyword, offset) {
        (PositionItem::Center, _) => SpecifiedPosition::percent(0.5),
        (PositionItem::Left | PositionItem::Top, None) => SpecifiedPosition::percent(0.0),
        (PositionItem::Right | PositionItem::Bottom, None) => SpecifiedPosition::percent(1.0),
        (PositionItem::Left | PositionItem::Top, Some(o)) => SpecifiedPosition {
            offset: o,
            from_end: false,
        },
        (PositionItem::Right | PositionItem::Bottom, Some(o)) => SpecifiedPosition {
            offset: o,
            from_end: true,
        },
        (PositionItem::Length(l), _) => SpecifiedPosition {
            offset: l.clone(),
            from_end: false,
        },
    }
}

/// `<bg-position>`: one to four values.
/// <https://www.w3.org/TR/css-backgrounds-3/#typedef-bg-position>
pub(crate) fn parse_bg_position(
    p: &mut Parser<'_>,
    quirky: bool,
) -> ParseResult<(SpecifiedPosition, SpecifiedPosition)> {
    parse_position_items(p, quirky, true)
}

/// `<position>`: one, two or four values (no three-value form).
/// <https://www.w3.org/TR/css-values-4/#position>
pub(crate) fn parse_position(
    p: &mut Parser<'_>,
) -> ParseResult<(SpecifiedPosition, SpecifiedPosition)> {
    parse_position_items(p, false, false)
}

fn parse_position_items(
    p: &mut Parser<'_>,
    quirky: bool,
    three_values: bool,
) -> ParseResult<(SpecifiedPosition, SpecifiedPosition)> {
    p.try_parse(|p| {
        let mut items = Vec::new();
        while items.len() < 4 {
            match position_item(p, quirky) {
                Ok(item) => items.push(item),
                Err(_) => break,
            }
        }
        if items.is_empty() {
            return Err(ParseError::Unexpected);
        }
        if items.len() == 3 && !three_values {
            return Err(ParseError::Invalid);
        }
        interpret_position(&items).ok_or(ParseError::Invalid)
    })
}

fn interpret_position(items: &[PositionItem]) -> Option<(SpecifiedPosition, SpecifiedPosition)> {
    use PositionItem as I;
    let center = I::Center;
    match items {
        [a] => {
            if a.is_vertical_keyword() {
                Some((position_from(&center, None), position_from(a, None)))
            } else {
                Some((position_from(a, None), position_from(&center, None)))
            }
        }
        [a, b] => {
            if a.is_keyword() && b.is_keyword() {
                let (x, y) = if a.is_vertical_keyword() || b.is_horizontal_keyword() {
                    (b, a)
                } else {
                    (a, b)
                };
                if x.is_vertical_keyword() || y.is_horizontal_keyword() {
                    return None;
                }
                return Some((position_from(x, None), position_from(y, None)));
            }
            if a.is_vertical_keyword() || b.is_horizontal_keyword() {
                return None;
            }
            Some((position_from(a, None), position_from(b, None)))
        }
        [_, _, _] | [_, _, _, _] => {
            // Keyword/offset pairs: `right 10px bottom 20px`, `center top 5px`.
            let mut groups: Vec<(&PositionItem, Option<Lp>)> = Vec::new();
            for item in items {
                match item {
                    I::Length(l) => {
                        let last = groups.last_mut()?;
                        if last.1.is_some() || matches!(last.0, I::Center) {
                            return None;
                        }
                        last.1 = Some(l.clone());
                    }
                    k => groups.push((k, None)),
                }
            }
            let [(k1, o1), (k2, o2)]: [(&PositionItem, Option<Lp>); 2] = groups.try_into().ok()?;
            let (x, y) = if k1.is_vertical_keyword() || k2.is_horizontal_keyword() {
                ((k2, o2), (k1, o1))
            } else {
                ((k1, o1), (k2, o2))
            };
            if x.0.is_vertical_keyword() || y.0.is_horizontal_keyword() {
                return None;
            }
            Some((position_from(x.0, x.1), position_from(y.0, y.1)))
        }
        _ => None,
    }
}

/// `background-position-x` / `-y`: `center | [start-keyword | end-keyword]?
/// <length-percentage>?`.
/// <https://www.w3.org/TR/css-backgrounds-4/#background-position-longhands>
pub(crate) fn parse_position_axis(
    p: &mut Parser<'_>,
    horizontal: bool,
    quirky: bool,
) -> ParseResult<SpecifiedPosition> {
    if p.expect_ident_matching("center").is_ok() {
        return Ok(SpecifiedPosition::percent(0.5));
    }
    let keywords: &[(&str, bool)] = if horizontal {
        &[
            ("left", false),
            ("right", true),
            ("x-start", false),
            ("x-end", true),
        ]
    } else {
        &[
            ("top", false),
            ("bottom", true),
            ("y-start", false),
            ("y-end", true),
        ]
    };
    let from_end = p.expect_one_of(keywords).ok();
    let offset = parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE.with_quirks(quirky));
    match (from_end, offset) {
        (None, Err(e)) => Err(e),
        (None, Ok(offset)) => Ok(SpecifiedPosition {
            offset,
            from_end: false,
        }),
        (Some(end), Err(_)) => Ok(SpecifiedPosition::percent(if end { 1.0 } else { 0.0 })),
        (Some(from_end), Ok(offset)) => Ok(SpecifiedPosition { offset, from_end }),
    }
}

/// `<bg-size>`: `[<length-percentage [0,∞]> | auto]{1,2} | cover | contain`.
pub(crate) fn parse_background_size(p: &mut Parser<'_>) -> ParseResult<SpecifiedBackgroundSize> {
    if let Ok(cover) = p.expect_one_of(&[("cover", true), ("contain", false)]) {
        return Ok(if cover {
            SpecifiedBackgroundSize::Cover
        } else {
            SpecifiedBackgroundSize::Contain
        });
    }
    let axis = |p: &mut Parser<'_>| -> ParseResult<Option<Lp>> {
        if p.expect_ident_matching("auto").is_ok() {
            return Ok(None);
        }
        parse_length_percentage(p, LengthOptions::NON_NEGATIVE).map(Some)
    };
    let w = axis(p)?;
    let h = axis(p).unwrap_or(None);
    Ok(SpecifiedBackgroundSize::Explicit(w, h))
}

/// `<repeat-style>`: `repeat-x | repeat-y | <keyword>{1,2}`.
pub(crate) fn parse_background_repeat(
    p: &mut Parser<'_>,
) -> ParseResult<(BackgroundRepeatKeyword, BackgroundRepeatKeyword)> {
    use BackgroundRepeatKeyword as R;
    let first = keyword(p, R::from_ident)?;
    match first {
        R::RepeatX => Ok((R::Repeat, R::NoRepeat)),
        R::RepeatY => Ok((R::NoRepeat, R::Repeat)),
        _ => {
            let second = p
                .try_parse(|p| match keyword(p, R::from_ident)? {
                    R::RepeatX | R::RepeatY => Err(ParseError::Invalid),
                    k => Ok(k),
                })
                .unwrap_or(first);
            Ok((first, second))
        }
    }
}

/// `<visual-box>` for `background-origin` and `background-clip`. `text`
/// (and `-webkit-text`) clip to the border box: swb does not clip
/// backgrounds to text.
pub(crate) fn parse_box(p: &mut Parser<'_>) -> ParseResult<BackgroundBox> {
    keyword(p, |i| {
        BackgroundBox::from_ident(i).or_else(|| {
            (i.eq_ignore_ascii_case("text") || i.eq_ignore_ascii_case("-webkit-text"))
                .then_some(BackgroundBox::BorderBox)
        })
    })
}

// ----- Flexbox and alignment -----

/// `flex-basis`: `content | <'width'>`.
pub(crate) fn parse_flex_basis(p: &mut Parser<'_>) -> ParseResult<SpecifiedFlexBasis> {
    if p.expect_ident_matching("content").is_ok() {
        return Ok(SpecifiedFlexBasis::Content);
    }
    parse_size(p, false, false).map(SpecifiedFlexBasis::Size)
}

/// The box alignment property being parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AlignKind {
    JustifyContent,
    AlignContent,
    AlignItems,
    AlignSelf,
    JustifyItems,
    JustifySelf,
}

/// The positions that can follow `legacy` in `justify-items`.
const LEGACY_POSITIONS: &[(&str, Alignment)] = &[
    ("left", Alignment::Left),
    ("right", Alignment::Right),
    ("center", Alignment::Center),
];

/// Box alignment values. `first baseline` and `last baseline` map to
/// `baseline`; `safe` and `unsafe` are ignored; `legacy` (for
/// `justify-items`) maps to `normal`, `legacy center` to `center`.
/// <https://www.w3.org/TR/css-align-3/>
pub(crate) fn parse_alignment(p: &mut Parser<'_>, kind: AlignKind) -> ParseResult<Alignment> {
    use AlignKind as K;
    p.try_parse(|p| {
        let is_content = matches!(kind, K::JustifyContent | K::AlignContent);
        let is_self = matches!(kind, K::AlignSelf | K::JustifySelf);
        let is_justify = matches!(kind, K::JustifyContent | K::JustifyItems | K::JustifySelf);
        if is_self && p.expect_ident_matching("auto").is_ok() {
            return Ok(Alignment::Auto);
        }
        if p.expect_ident_matching("normal").is_ok() {
            return Ok(Alignment::Normal);
        }
        if p.expect_ident_matching("stretch").is_ok() {
            return Ok(Alignment::Stretch);
        }
        // `justify-content` has no baseline values.
        if kind != K::JustifyContent {
            if p.expect_one_of(&[("first", ()), ("last", ())]).is_ok() {
                p.expect_ident_matching("baseline")?;
                return Ok(Alignment::Baseline);
            }
            if p.expect_ident_matching("baseline").is_ok() {
                return Ok(Alignment::Baseline);
            }
        }
        if is_content
            && let Ok(v) = p.expect_one_of(&[
                ("space-between", Alignment::SpaceBetween),
                ("space-around", Alignment::SpaceAround),
                ("space-evenly", Alignment::SpaceEvenly),
            ])
        {
            return Ok(v);
        }
        // `legacy` alone behaves as `normal`; `legacy left | right |
        // center` as the position (Chromium's `ResolvedSelfAlignment`).
        // The inheritance of `legacy` values is not supported.
        if kind == K::JustifyItems && p.expect_ident_matching("legacy").is_ok() {
            return Ok(p
                .expect_one_of(LEGACY_POSITIONS)
                .unwrap_or(Alignment::Normal));
        }
        let _ = p.expect_one_of(&[("safe", ()), ("unsafe", ())]);
        let ident = p.expect_ident()?.to_ascii_lowercase();
        let value = match ident.as_str() {
            "center" | "anchor-center" => Alignment::Center,
            "start" => Alignment::Start,
            "end" => Alignment::End,
            "flex-start" => Alignment::FlexStart,
            "flex-end" => Alignment::FlexEnd,
            "self-start" if !is_content => Alignment::SelfStart,
            "self-end" if !is_content => Alignment::SelfEnd,
            "left" if is_justify => Alignment::Left,
            "right" if is_justify => Alignment::Right,
            _ => return Err(ParseError::Unexpected),
        };
        // Only `left`, `right` and `center` pair with `legacy`.
        if kind == K::JustifyItems
            && p.expect_ident_matching("legacy").is_ok()
            && !matches!(
                value,
                Alignment::Left | Alignment::Right | Alignment::Center
            )
        {
            return Err(ParseError::Invalid);
        }
        Ok(value)
    })
}

/// `column-width`: `auto | <length [0,∞]>`.
pub(crate) fn parse_column_width(p: &mut Parser<'_>) -> ParseResult<Option<Lp>> {
    if p.expect_ident_matching("auto").is_ok() {
        return Ok(None);
    }
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE_LENGTH).map(Some)
}

/// `column-count`: `auto | <integer [1,∞]>`.
pub(crate) fn parse_column_count(p: &mut Parser<'_>) -> ParseResult<Option<u32>> {
    if p.expect_ident_matching("auto").is_ok() {
        return Ok(None);
    }
    let n = parse_integer(p)?;
    u32::try_from(n)
        .ok()
        .filter(|n| *n >= 1)
        .map(Some)
        .ok_or(ParseError::Invalid)
}

/// `row-gap` / `column-gap`: `normal | <length-percentage [0,∞]>`.
pub(crate) fn parse_gap(p: &mut Parser<'_>) -> ParseResult<Option<Lp>> {
    if p.expect_ident_matching("normal").is_ok() {
        return Ok(None);
    }
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE).map(Some)
}
