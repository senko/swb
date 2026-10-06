//! Shorthand properties: parsing into longhand declarations.
//!
//! Each shorthand sets all of its longhands; omitted parts get their
//! initial values. Logical shorthands (`margin-block`, `border-inline`,
//! ...) map to physical sides for horizontal, left-to-right text.

use std::sync::Arc;

use swb_css::{ParseError, Parser};

use super::ids::{LonghandId, LonghandValue};
use super::longhand::{
    AlignKind, keyword, lp_or_auto, non_negative_length, padding, parse_alignment,
    parse_background_repeat, parse_background_size, parse_bg_position, parse_border_style,
    parse_box, parse_flex_basis, parse_font_family, parse_font_size, parse_font_stretch,
    parse_font_style, parse_font_weight, parse_gap, parse_image_or_none, parse_line_height,
    parse_line_width, parse_list_style_type, parse_outline_color, parse_outline_style,
    parse_text_decoration_line,
};
use super::specified::{
    SpecifiedBackgroundSize, SpecifiedFlexBasis, SpecifiedFontSize, SpecifiedFontWeight,
    SpecifiedLineHeight, SpecifiedPosition, SpecifiedSize,
};
use crate::parse::color::parse_color;
use crate::parse::grid;
use crate::parse::image::{SpecifiedImage, looks_like_image, parse_image};
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::parse::{ParseResult, ParserContext, parse_non_negative_number};
use crate::values::{
    BackgroundAttachment, BackgroundBox, BackgroundRepeatKeyword, BorderStyle, Color,
    FlexDirection, FlexWrap, FontFamily, FontStyle, FontVariantCaps, GenericFamily, Length,
    ListStylePosition, ListStyleType, OutlineStyle, Overflow, Rgba,
    SpecifiedLengthPercentage as Lp, TextDecorationLine, TextDecorationStyle,
};

macro_rules! shorthands {
    ($($id:ident $name:literal [$($longhand:ident),+];)+) => {
        /// A shorthand property.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum ShorthandId {
            $($id),+
        }

        impl ShorthandId {
            /// Looks up a shorthand by its CSS name (lowercase).
            pub(crate) fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(ShorthandId::$id),)+
                    _ => None,
                }
            }

            /// The longhands that this shorthand sets.
            pub(crate) fn longhands(self) -> &'static [LonghandId] {
                match self {
                    $(ShorthandId::$id => &[$(LonghandId::$longhand),+]),+
                }
            }
        }
    };
}

shorthands! {
    Margin "margin" [MarginTop, MarginRight, MarginBottom, MarginLeft];
    MarginBlock "margin-block" [MarginTop, MarginBottom];
    MarginInline "margin-inline" [MarginLeft, MarginRight];
    Padding "padding" [PaddingTop, PaddingRight, PaddingBottom, PaddingLeft];
    PaddingBlock "padding-block" [PaddingTop, PaddingBottom];
    PaddingInline "padding-inline" [PaddingLeft, PaddingRight];
    Inset "inset" [Top, Right, Bottom, Left];
    InsetBlock "inset-block" [Top, Bottom];
    InsetInline "inset-inline" [Left, Right];
    Border "border" [
        BorderTopWidth, BorderRightWidth, BorderBottomWidth, BorderLeftWidth,
        BorderTopStyle, BorderRightStyle, BorderBottomStyle, BorderLeftStyle,
        BorderTopColor, BorderRightColor, BorderBottomColor, BorderLeftColor
    ];
    BorderTop "border-top" [BorderTopWidth, BorderTopStyle, BorderTopColor];
    BorderRight "border-right" [BorderRightWidth, BorderRightStyle, BorderRightColor];
    BorderBottom "border-bottom" [BorderBottomWidth, BorderBottomStyle, BorderBottomColor];
    BorderLeft "border-left" [BorderLeftWidth, BorderLeftStyle, BorderLeftColor];
    BorderBlock "border-block" [
        BorderTopWidth, BorderBottomWidth, BorderTopStyle, BorderBottomStyle,
        BorderTopColor, BorderBottomColor
    ];
    BorderInline "border-inline" [
        BorderLeftWidth, BorderRightWidth, BorderLeftStyle, BorderRightStyle,
        BorderLeftColor, BorderRightColor
    ];
    BorderWidth "border-width" [BorderTopWidth, BorderRightWidth, BorderBottomWidth, BorderLeftWidth];
    BorderStyle "border-style" [BorderTopStyle, BorderRightStyle, BorderBottomStyle, BorderLeftStyle];
    BorderColor "border-color" [BorderTopColor, BorderRightColor, BorderBottomColor, BorderLeftColor];
    BorderBlockWidth "border-block-width" [BorderTopWidth, BorderBottomWidth];
    BorderBlockStyle "border-block-style" [BorderTopStyle, BorderBottomStyle];
    BorderBlockColor "border-block-color" [BorderTopColor, BorderBottomColor];
    BorderInlineWidth "border-inline-width" [BorderLeftWidth, BorderRightWidth];
    BorderInlineStyle "border-inline-style" [BorderLeftStyle, BorderRightStyle];
    BorderInlineColor "border-inline-color" [BorderLeftColor, BorderRightColor];
    BorderRadius "border-radius" [
        BorderTopLeftRadius, BorderTopRightRadius, BorderBottomRightRadius, BorderBottomLeftRadius
    ];
    BorderSpacing "border-spacing" [BorderSpacingHorizontal, BorderSpacingVertical];
    Outline "outline" [OutlineColor, OutlineStyle, OutlineWidth];
    Background "background" [
        BackgroundColor, BackgroundImage, BackgroundPositionX, BackgroundPositionY,
        BackgroundSize, BackgroundRepeat, BackgroundAttachment, BackgroundOrigin, BackgroundClip
    ];
    BackgroundPosition "background-position" [BackgroundPositionX, BackgroundPositionY];
    Mask "mask" [
        MaskImage, MaskPositionX, MaskPositionY, MaskSize, MaskRepeat, MaskOrigin, MaskClip,
        MaskComposite, MaskMode
    ];
    WebkitMask "-webkit-mask" [
        MaskImage, MaskPositionX, MaskPositionY, MaskSize, MaskRepeat, MaskOrigin, MaskClip,
        MaskComposite, MaskMode
    ];
    MaskPosition "mask-position" [MaskPositionX, MaskPositionY];
    WebkitMaskPosition "-webkit-mask-position" [MaskPositionX, MaskPositionY];
    WebkitMaskOrigin "-webkit-mask-origin" [MaskOrigin];
    WebkitMaskClip "-webkit-mask-clip" [MaskClip];
    WebkitMaskComposite "-webkit-mask-composite" [MaskComposite];
    Font "font" [
        FontStyle, FontVariantCaps, FontWeight, FontStretch, FontSize, LineHeight, FontFamily
    ];
    FontVariant "font-variant" [FontVariantCaps];
    ListStyle "list-style" [ListStyleType, ListStylePosition, ListStyleImage];
    TextDecoration "text-decoration" [TextDecorationLine, TextDecorationStyle, TextDecorationColor];
    Flex "flex" [FlexGrow, FlexShrink, FlexBasis];
    FlexFlow "flex-flow" [FlexDirection, FlexWrap];
    Gap "gap" [RowGap, ColumnGap];
    Overflow "overflow" [OverflowX, OverflowY];
    PlaceContent "place-content" [AlignContent, JustifyContent];
    PlaceItems "place-items" [AlignItems, JustifyItems];
    PlaceSelf "place-self" [AlignSelf, JustifySelf];
    GridRow "grid-row" [GridRowStart, GridRowEnd];
    GridColumn "grid-column" [GridColumnStart, GridColumnEnd];
    GridArea "grid-area" [GridRowStart, GridColumnStart, GridRowEnd, GridColumnEnd];
    GridTemplate "grid-template" [GridTemplateRows, GridTemplateColumns, GridTemplateAreas];
    Grid "grid" [
        GridTemplateRows, GridTemplateColumns, GridTemplateAreas, GridAutoRows, GridAutoColumns,
        GridAutoFlow
    ];
}

impl ShorthandId {
    /// The shorthand for an alias name (`grid-gap`, logical border
    /// shorthands, prefixed names).
    pub(crate) fn from_alias(name: &str) -> Option<Self> {
        Some(match name {
            "grid-gap" => ShorthandId::Gap,
            "border-block-start" => ShorthandId::BorderTop,
            "border-block-end" => ShorthandId::BorderBottom,
            "border-inline-start" => ShorthandId::BorderLeft,
            "border-inline-end" => ShorthandId::BorderRight,
            "-webkit-flex" => ShorthandId::Flex,
            "-webkit-flex-flow" => ShorthandId::FlexFlow,
            "-webkit-border-radius" | "-moz-border-radius" => ShorthandId::BorderRadius,
            _ => return None,
        })
    }

    /// Parses the whole value and appends the longhand values to `out`.
    #[allow(clippy::too_many_lines)] // One arm per shorthand.
    pub(crate) fn parse(
        self,
        p: &mut Parser<'_>,
        cx: &ParserContext,
        out: &mut Vec<LonghandValue>,
    ) -> ParseResult<()> {
        let quirks = cx.quirks;
        p.parse_entirely(|p| {
            use ShorthandId as S;
            match self {
                S::Margin => four_sides(p, out, |p| lp_or_auto(p, quirks), margin_values),
                S::MarginBlock => two_sides(
                    p,
                    out,
                    |p| lp_or_auto(p, false),
                    |[a, b]| vec![LonghandValue::MarginTop(a), LonghandValue::MarginBottom(b)],
                ),
                S::MarginInline => two_sides(
                    p,
                    out,
                    |p| lp_or_auto(p, false),
                    |[a, b]| vec![LonghandValue::MarginLeft(a), LonghandValue::MarginRight(b)],
                ),
                S::Padding => four_sides(
                    p,
                    out,
                    |p| padding(p, quirks),
                    |[t, r, b, l]| {
                        vec![
                            LonghandValue::PaddingTop(t),
                            LonghandValue::PaddingRight(r),
                            LonghandValue::PaddingBottom(b),
                            LonghandValue::PaddingLeft(l),
                        ]
                    },
                ),
                S::PaddingBlock => two_sides(
                    p,
                    out,
                    |p| padding(p, false),
                    |[a, b]| {
                        vec![
                            LonghandValue::PaddingTop(a),
                            LonghandValue::PaddingBottom(b),
                        ]
                    },
                ),
                S::PaddingInline => two_sides(
                    p,
                    out,
                    |p| padding(p, false),
                    |[a, b]| {
                        vec![
                            LonghandValue::PaddingLeft(a),
                            LonghandValue::PaddingRight(b),
                        ]
                    },
                ),
                S::Inset => four_sides(
                    p,
                    out,
                    |p| lp_or_auto(p, false),
                    |[t, r, b, l]| {
                        vec![
                            LonghandValue::Top(t),
                            LonghandValue::Right(r),
                            LonghandValue::Bottom(b),
                            LonghandValue::Left(l),
                        ]
                    },
                ),
                S::InsetBlock => two_sides(
                    p,
                    out,
                    |p| lp_or_auto(p, false),
                    |[a, b]| vec![LonghandValue::Top(a), LonghandValue::Bottom(b)],
                ),
                S::InsetInline => two_sides(
                    p,
                    out,
                    |p| lp_or_auto(p, false),
                    |[a, b]| vec![LonghandValue::Left(a), LonghandValue::Right(b)],
                ),
                S::Border => {
                    let (w, s, c) = parse_border_side(p)?;
                    for side in Side::ALL {
                        out.extend(side.border(w.clone(), s, c));
                    }
                    Ok(())
                }
                S::BorderTop | S::BorderRight | S::BorderBottom | S::BorderLeft => {
                    let side = match self {
                        S::BorderTop => Side::Top,
                        S::BorderRight => Side::Right,
                        S::BorderBottom => Side::Bottom,
                        _ => Side::Left,
                    };
                    let (w, s, c) = parse_border_side(p)?;
                    out.extend(side.border(w, s, c));
                    Ok(())
                }
                S::BorderBlock | S::BorderInline => {
                    let sides = if self == S::BorderBlock {
                        [Side::Top, Side::Bottom]
                    } else {
                        [Side::Left, Side::Right]
                    };
                    let (w, s, c) = parse_border_side(p)?;
                    for side in sides {
                        out.extend(side.border(w.clone(), s, c));
                    }
                    Ok(())
                }
                S::BorderWidth => four_sides(
                    p,
                    out,
                    |p| parse_line_width(p, quirks),
                    |v| Side::zip(v, Side::width),
                ),
                S::BorderStyle => {
                    four_sides(p, out, parse_border_style, |v| Side::zip(v, Side::style))
                }
                S::BorderColor => four_sides(p, out, parse_color, |v| Side::zip(v, Side::color)),
                S::BorderBlockWidth => two_sides(
                    p,
                    out,
                    |p| parse_line_width(p, false),
                    |[a, b]| vec![Side::Top.width(a), Side::Bottom.width(b)],
                ),
                S::BorderBlockStyle => two_sides(p, out, parse_border_style, |[a, b]| {
                    vec![Side::Top.style(a), Side::Bottom.style(b)]
                }),
                S::BorderBlockColor => two_sides(p, out, parse_color, |[a, b]| {
                    vec![Side::Top.color(a), Side::Bottom.color(b)]
                }),
                S::BorderInlineWidth => two_sides(
                    p,
                    out,
                    |p| parse_line_width(p, false),
                    |[a, b]| vec![Side::Left.width(a), Side::Right.width(b)],
                ),
                S::BorderInlineStyle => two_sides(p, out, parse_border_style, |[a, b]| {
                    vec![Side::Left.style(a), Side::Right.style(b)]
                }),
                S::BorderInlineColor => two_sides(p, out, parse_color, |[a, b]| {
                    vec![Side::Left.color(a), Side::Right.color(b)]
                }),
                S::BorderRadius => parse_border_radius(p, out),
                S::BorderSpacing => {
                    let h = non_negative_length(p, quirks)?;
                    let v = non_negative_length(p, quirks).unwrap_or_else(|_| h.clone());
                    out.push(LonghandValue::BorderSpacingHorizontal(h));
                    out.push(LonghandValue::BorderSpacingVertical(v));
                    Ok(())
                }
                S::Outline => parse_outline(p, out),
                S::Background => parse_background(p, cx, out),
                S::BackgroundPosition => {
                    let positions: Vec<(SpecifiedPosition, SpecifiedPosition)> =
                        p.parse_comma_separated(|p| parse_bg_position(p, quirks))?;
                    let (x, y): (Vec<_>, Vec<_>) = positions.into_iter().unzip();
                    out.push(LonghandValue::BackgroundPositionX(Arc::from(x)));
                    out.push(LonghandValue::BackgroundPositionY(Arc::from(y)));
                    Ok(())
                }
                S::Mask
                | S::WebkitMask
                | S::MaskPosition
                | S::WebkitMaskPosition
                | S::WebkitMaskOrigin
                | S::WebkitMaskClip
                | S::WebkitMaskComposite => super::mask::parse_shorthand(self, p, cx, out),
                S::Font => parse_font(p, out),
                S::FontVariant => {
                    out.push(LonghandValue::FontVariantCaps(parse_font_variant(p)?));
                    Ok(())
                }
                S::ListStyle => parse_list_style(p, cx, out),
                S::TextDecoration => parse_text_decoration(p, out),
                S::Flex => parse_flex(p, out),
                S::FlexFlow => parse_flex_flow(p, out),
                S::Gap => {
                    let row = parse_gap(p)?;
                    let column = if p.is_exhausted() {
                        row.clone()
                    } else {
                        parse_gap(p)?
                    };
                    out.push(LonghandValue::RowGap(row));
                    out.push(LonghandValue::ColumnGap(column));
                    Ok(())
                }
                S::Overflow => {
                    let x = keyword(p, Overflow::from_ident)?;
                    let y = keyword(p, Overflow::from_ident).unwrap_or(x);
                    out.push(LonghandValue::OverflowX(x));
                    out.push(LonghandValue::OverflowY(y));
                    Ok(())
                }
                S::PlaceContent => {
                    let align = parse_alignment(p, AlignKind::AlignContent)?;
                    let justify = if p.is_exhausted() {
                        if align == crate::values::Alignment::Baseline {
                            crate::values::Alignment::Start
                        } else {
                            align
                        }
                    } else {
                        parse_alignment(p, AlignKind::JustifyContent)?
                    };
                    out.push(LonghandValue::AlignContent(align));
                    out.push(LonghandValue::JustifyContent(justify));
                    Ok(())
                }
                // An omitted second value copies the first (CSS Align 3
                // §6.3, §6.1).
                S::PlaceItems => {
                    let align = parse_alignment(p, AlignKind::AlignItems)?;
                    let justify = if p.is_exhausted() {
                        align
                    } else {
                        parse_alignment(p, AlignKind::JustifyItems)?
                    };
                    out.push(LonghandValue::AlignItems(align));
                    out.push(LonghandValue::JustifyItems(justify));
                    Ok(())
                }
                S::PlaceSelf => {
                    let align = parse_alignment(p, AlignKind::AlignSelf)?;
                    let justify = if p.is_exhausted() {
                        align
                    } else {
                        parse_alignment(p, AlignKind::JustifySelf)?
                    };
                    out.push(LonghandValue::AlignSelf(align));
                    out.push(LonghandValue::JustifySelf(justify));
                    Ok(())
                }
                S::GridRow => {
                    let (start, end) = grid::parse_grid_line_pair(p)?;
                    out.push(LonghandValue::GridRowStart(start));
                    out.push(LonghandValue::GridRowEnd(end));
                    Ok(())
                }
                S::GridColumn => {
                    let (start, end) = grid::parse_grid_line_pair(p)?;
                    out.push(LonghandValue::GridColumnStart(start));
                    out.push(LonghandValue::GridColumnEnd(end));
                    Ok(())
                }
                S::GridArea => {
                    let [row_start, column_start, row_end, column_end] = grid::parse_grid_area(p)?;
                    out.push(LonghandValue::GridRowStart(row_start));
                    out.push(LonghandValue::GridColumnStart(column_start));
                    out.push(LonghandValue::GridRowEnd(row_end));
                    out.push(LonghandValue::GridColumnEnd(column_end));
                    Ok(())
                }
                S::GridTemplate => {
                    push_grid_template(grid::parse_grid_template(p)?, out);
                    Ok(())
                }
                S::Grid => {
                    let g = grid::parse_grid(p)?;
                    push_grid_template(g.template, out);
                    out.push(LonghandValue::GridAutoRows(g.auto_rows));
                    out.push(LonghandValue::GridAutoColumns(g.auto_columns));
                    out.push(LonghandValue::GridAutoFlow(g.auto_flow));
                    Ok(())
                }
            }
        })
    }
}

/// Appends the longhands of the `grid-template` shorthand.
fn push_grid_template(t: grid::GridTemplate, out: &mut Vec<LonghandValue>) {
    out.push(LonghandValue::GridTemplateRows(t.rows));
    out.push(LonghandValue::GridTemplateColumns(t.columns));
    out.push(LonghandValue::GridTemplateAreas(t.areas));
}

/// Parses one to four values and expands them to four (top, right,
/// bottom, left, or the corners from top-left), as the box shorthands do.
#[allow(clippy::many_single_char_names)]
fn one_to_four<'i, T: Clone>(
    p: &mut Parser<'i>,
    mut item: impl FnMut(&mut Parser<'i>) -> ParseResult<T>,
) -> ParseResult<[T; 4]> {
    let mut values = vec![item(p)?];
    while values.len() < 4 {
        match item(p) {
            Ok(v) => values.push(v),
            Err(_) => break,
        }
    }
    Ok(match values.as_slice() {
        [a] => [a.clone(), a.clone(), a.clone(), a.clone()],
        [a, b] => [a.clone(), b.clone(), a.clone(), b.clone()],
        [a, b, c] => [a.clone(), b.clone(), c.clone(), b.clone()],
        [a, b, c, d] => [a.clone(), b.clone(), c.clone(), d.clone()],
        _ => return Err(ParseError::Invalid),
    })
}

/// Parses one to four values and expands them to top, right, bottom,
/// left.
fn four_sides<'i, T: Clone>(
    p: &mut Parser<'i>,
    out: &mut Vec<LonghandValue>,
    item: impl FnMut(&mut Parser<'i>) -> ParseResult<T>,
    build: impl FnOnce([T; 4]) -> Vec<LonghandValue>,
) -> ParseResult<()> {
    out.extend(build(one_to_four(p, item)?));
    Ok(())
}

/// Parses one or two values (start and end).
fn two_sides<'i, T: Clone>(
    p: &mut Parser<'i>,
    out: &mut Vec<LonghandValue>,
    mut item: impl FnMut(&mut Parser<'i>) -> ParseResult<T>,
    build: impl FnOnce([T; 2]) -> Vec<LonghandValue>,
) -> ParseResult<()> {
    let a = item(p)?;
    let b = if p.is_exhausted() {
        a.clone()
    } else {
        item(p)?
    };
    out.extend(build([a, b]));
    Ok(())
}

fn margin_values([t, r, b, l]: [Option<Lp>; 4]) -> Vec<LonghandValue> {
    vec![
        LonghandValue::MarginTop(t),
        LonghandValue::MarginRight(r),
        LonghandValue::MarginBottom(b),
        LonghandValue::MarginLeft(l),
    ]
}

/// A physical side of a box, for the border shorthands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    const ALL: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

    fn width(self, v: Lp) -> LonghandValue {
        match self {
            Side::Top => LonghandValue::BorderTopWidth(v),
            Side::Right => LonghandValue::BorderRightWidth(v),
            Side::Bottom => LonghandValue::BorderBottomWidth(v),
            Side::Left => LonghandValue::BorderLeftWidth(v),
        }
    }

    fn style(self, v: BorderStyle) -> LonghandValue {
        match self {
            Side::Top => LonghandValue::BorderTopStyle(v),
            Side::Right => LonghandValue::BorderRightStyle(v),
            Side::Bottom => LonghandValue::BorderBottomStyle(v),
            Side::Left => LonghandValue::BorderLeftStyle(v),
        }
    }

    fn color(self, v: Color) -> LonghandValue {
        match self {
            Side::Top => LonghandValue::BorderTopColor(v),
            Side::Right => LonghandValue::BorderRightColor(v),
            Side::Bottom => LonghandValue::BorderBottomColor(v),
            Side::Left => LonghandValue::BorderLeftColor(v),
        }
    }

    fn border(self, w: Lp, s: BorderStyle, c: Color) -> [LonghandValue; 3] {
        [self.width(w), self.style(s), self.color(c)]
    }

    fn zip<T>(values: [T; 4], f: impl Fn(Side, T) -> LonghandValue) -> Vec<LonghandValue> {
        Side::ALL
            .into_iter()
            .zip(values)
            .map(|(s, v)| f(s, v))
            .collect()
    }
}

/// `<line-width> || <line-style> || <color>`, with initial values for the
/// omitted parts (`medium`, `none`, `currentColor`).
fn parse_border_side(p: &mut Parser<'_>) -> ParseResult<(Lp, BorderStyle, Color)> {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    while !p.is_exhausted() {
        if width.is_none()
            && let Ok(w) = parse_line_width(p, false)
        {
            width = Some(w);
        } else if style.is_none()
            && let Ok(s) = parse_border_style(p)
        {
            style = Some(s);
        } else if color.is_none()
            && let Ok(c) = parse_color(p)
        {
            color = Some(c);
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    if width.is_none() && style.is_none() && color.is_none() {
        return Err(ParseError::EndOfInput);
    }
    Ok((
        width.unwrap_or(Lp::Length(Length::px(3.0))),
        style.unwrap_or(BorderStyle::None),
        color.unwrap_or(Color::CurrentColor),
    ))
}

/// `outline`: `<color> || <style> || <width>`.
fn parse_outline(p: &mut Parser<'_>, out: &mut Vec<LonghandValue>) -> ParseResult<()> {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    while !p.is_exhausted() {
        if width.is_none()
            && let Ok(w) = parse_line_width(p, false)
        {
            width = Some(w);
        } else if style.is_none()
            && let Ok(s) = parse_outline_style(p)
        {
            style = Some(s);
        } else if color.is_none()
            && let Ok(c) = parse_outline_color(p)
        {
            color = Some(c);
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    if width.is_none() && style.is_none() && color.is_none() {
        return Err(ParseError::EndOfInput);
    }
    out.push(LonghandValue::OutlineColor(
        color.unwrap_or(Color::CurrentColor),
    ));
    out.push(LonghandValue::OutlineStyle(
        style.unwrap_or(OutlineStyle::None),
    ));
    out.push(LonghandValue::OutlineWidth(
        width.unwrap_or(Lp::Length(Length::px(3.0))),
    ));
    Ok(())
}

/// `border-radius`: `<length-percentage [0,∞]>{1,4} [ / <length-percentage
/// [0,∞]>{1,4} ]?`.
fn parse_border_radius(p: &mut Parser<'_>, out: &mut Vec<LonghandValue>) -> ParseResult<()> {
    fn corners(p: &mut Parser<'_>) -> ParseResult<[Lp; 4]> {
        one_to_four(p, |p| {
            parse_length_percentage(p, LengthOptions::NON_NEGATIVE)
        })
    }
    let horizontal = corners(p)?;
    let vertical = if p.expect_delim('/').is_ok() {
        corners(p)?
    } else {
        horizontal.clone()
    };
    let [h0, h1, h2, h3] = horizontal;
    let [v0, v1, v2, v3] = vertical;
    out.push(LonghandValue::BorderTopLeftRadius((h0, v0)));
    out.push(LonghandValue::BorderTopRightRadius((h1, v1)));
    out.push(LonghandValue::BorderBottomRightRadius((h2, v2)));
    out.push(LonghandValue::BorderBottomLeftRadius((h3, v3)));
    Ok(())
}

/// One layer of the `background` shorthand.
#[derive(Default)]
struct BackgroundLayer {
    color: Option<Color>,
    /// True once an image (or `none`) is given.
    has_image: bool,
    image: Option<SpecifiedImage>,
    position: Option<(SpecifiedPosition, SpecifiedPosition)>,
    size: Option<SpecifiedBackgroundSize>,
    repeat: Option<(BackgroundRepeatKeyword, BackgroundRepeatKeyword)>,
    attachment: Option<BackgroundAttachment>,
    origin: Option<BackgroundBox>,
    clip: Option<BackgroundBox>,
}

/// `background`. <https://www.w3.org/TR/css-backgrounds-3/#background>
fn parse_background(
    p: &mut Parser<'_>,
    cx: &ParserContext,
    out: &mut Vec<LonghandValue>,
) -> ParseResult<()> {
    let layers = p.parse_comma_separated(|p| parse_background_layer(p, cx))?;
    let last = layers.len().saturating_sub(1);
    if layers[..last].iter().any(|l| l.color.is_some()) {
        return Err(ParseError::Invalid);
    }
    let color = layers
        .last()
        .and_then(|l| l.color)
        .unwrap_or(Color::Rgba(Rgba::TRANSPARENT));
    let mut images = Vec::new();
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut sizes = Vec::new();
    let mut repeats = Vec::new();
    let mut attachments = Vec::new();
    let mut origins = Vec::new();
    let mut clips = Vec::new();
    for layer in layers {
        images.push(layer.image);
        let (x, y) = layer.position.unwrap_or_else(|| {
            (
                SpecifiedPosition::percent(0.0),
                SpecifiedPosition::percent(0.0),
            )
        });
        xs.push(x);
        ys.push(y);
        sizes.push(
            layer
                .size
                .unwrap_or(SpecifiedBackgroundSize::Explicit(None, None)),
        );
        repeats.push(layer.repeat.unwrap_or((
            BackgroundRepeatKeyword::Repeat,
            BackgroundRepeatKeyword::Repeat,
        )));
        attachments.push(layer.attachment.unwrap_or(BackgroundAttachment::Scroll));
        origins.push(layer.origin.unwrap_or(BackgroundBox::PaddingBox));
        clips.push(
            layer
                .clip
                .or(layer.origin)
                .unwrap_or(BackgroundBox::BorderBox),
        );
    }
    out.push(LonghandValue::BackgroundColor(color));
    out.push(LonghandValue::BackgroundImage(Arc::from(images)));
    out.push(LonghandValue::BackgroundPositionX(Arc::from(xs)));
    out.push(LonghandValue::BackgroundPositionY(Arc::from(ys)));
    out.push(LonghandValue::BackgroundSize(Arc::from(sizes)));
    out.push(LonghandValue::BackgroundRepeat(Arc::from(repeats)));
    out.push(LonghandValue::BackgroundAttachment(Arc::from(attachments)));
    out.push(LonghandValue::BackgroundOrigin(Arc::from(origins)));
    out.push(LonghandValue::BackgroundClip(Arc::from(clips)));
    Ok(())
}

fn parse_background_layer(p: &mut Parser<'_>, cx: &ParserContext) -> ParseResult<BackgroundLayer> {
    let mut layer = BackgroundLayer::default();
    // A layer must have at least one component (`background: ;` and a
    // trailing comma are invalid).
    if p.is_exhausted() {
        return Err(ParseError::EndOfInput);
    }
    while !p.is_exhausted() {
        let next = p.peek();
        if !layer.has_image && next.is_some_and(looks_like_image) {
            layer.image = parse_image(p, cx)?;
            layer.has_image = true;
        } else if !layer.has_image && p.expect_ident_matching("none").is_ok() {
            layer.has_image = true;
        } else if layer.position.is_none()
            && let Ok(pos) = parse_bg_position(p, false)
        {
            layer.position = Some(pos);
            if p.expect_delim('/').is_ok() {
                layer.size = Some(parse_background_size(p)?);
            }
        } else if layer.repeat.is_none()
            && let Ok(r) = parse_background_repeat(p)
        {
            layer.repeat = Some(r);
        } else if layer.attachment.is_none()
            && let Ok(a) = keyword(p, BackgroundAttachment::from_ident)
        {
            layer.attachment = Some(a);
        } else if layer.origin.is_none()
            && let Ok(b) = parse_box(p)
        {
            layer.origin = Some(b);
        } else if layer.clip.is_none()
            && let Ok(b) = parse_box(p)
        {
            layer.clip = Some(b);
        } else if layer.color.is_none()
            && let Ok(c) = parse_color(p)
        {
            layer.color = Some(c);
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    Ok(layer)
}

/// `font`. <https://www.w3.org/TR/css-fonts-4/#font-prop>
///
/// System font keywords (`caption`, `menu`, ...) set `system-ui` at 13px.
fn parse_font(p: &mut Parser<'_>, out: &mut Vec<LonghandValue>) -> ParseResult<()> {
    let system = p.try_parse(|p| {
        let k = p.expect_one_of(&[
            ("caption", ()),
            ("icon", ()),
            ("menu", ()),
            ("message-box", ()),
            ("small-caption", ()),
            ("status-bar", ()),
        ]);
        p.expect_exhausted()?;
        k
    });
    if system.is_ok() {
        out.extend([
            LonghandValue::FontStyle(FontStyle::Normal),
            LonghandValue::FontVariantCaps(FontVariantCaps::Normal),
            LonghandValue::FontWeight(SpecifiedFontWeight::Absolute(400.0)),
            LonghandValue::FontStretch(100.0),
            LonghandValue::FontSize(SpecifiedFontSize::Length(Lp::Length(Length::px(13.0)))),
            LonghandValue::LineHeight(SpecifiedLineHeight::Normal),
            LonghandValue::FontFamily(Arc::from([FontFamily::Generic(GenericFamily::SystemUi)])),
        ]);
        return Ok(());
    }
    let mut style = None;
    let mut caps = None;
    let mut weight = None;
    let mut stretch = None;
    for _ in 0..4 {
        if p.expect_ident_matching("normal").is_ok() {
            continue;
        }
        if style.is_none()
            && let Ok(s) = parse_font_style(p)
        {
            style = Some(s);
        } else if caps.is_none() && p.expect_ident_matching("small-caps").is_ok() {
            caps = Some(FontVariantCaps::SmallCaps);
        } else if weight.is_none()
            && let Ok(w) = parse_font_weight(p)
        {
            weight = Some(w);
        } else if stretch.is_none()
            && let Ok(s) = p.try_parse(|p| {
                // Only keywords: a percentage would be ambiguous with the
                // font size.
                if matches!(p.peek(), Some(swb_css::ComponentValue::Ident(_))) {
                    parse_font_stretch(p)
                } else {
                    Err(ParseError::Unexpected)
                }
            })
        {
            stretch = Some(s);
        } else {
            break;
        }
    }
    let size = parse_font_size(p, false)?;
    let line_height = if p.expect_delim('/').is_ok() {
        parse_line_height(p)?
    } else {
        SpecifiedLineHeight::Normal
    };
    let family = parse_font_family(p)?;
    out.extend([
        LonghandValue::FontStyle(style.unwrap_or(FontStyle::Normal)),
        LonghandValue::FontVariantCaps(caps.unwrap_or(FontVariantCaps::Normal)),
        LonghandValue::FontWeight(weight.unwrap_or(SpecifiedFontWeight::Absolute(400.0))),
        LonghandValue::FontStretch(stretch.unwrap_or(100.0)),
        LonghandValue::FontSize(size),
        LonghandValue::LineHeight(line_height),
        LonghandValue::FontFamily(family),
    ]);
    Ok(())
}

/// The keywords of the `font-variant-*` longhands other than caps, which
/// swb accepts and ignores.
const OTHER_FONT_VARIANT_KEYWORDS: &[&str] = &[
    "common-ligatures",
    "no-common-ligatures",
    "discretionary-ligatures",
    "no-discretionary-ligatures",
    "historical-ligatures",
    "no-historical-ligatures",
    "contextual",
    "no-contextual",
    "lining-nums",
    "oldstyle-nums",
    "proportional-nums",
    "tabular-nums",
    "diagonal-fractions",
    "stacked-fractions",
    "ordinal",
    "slashed-zero",
    "jis78",
    "jis83",
    "jis90",
    "jis04",
    "simplified",
    "traditional",
    "full-width",
    "proportional-width",
    "ruby",
    "sub",
    "super",
    "historical-forms",
    "emoji",
    "text",
    "unicode",
];

/// `font-variant`: `normal | none | <keywords>`. Only the caps part is
/// stored. <https://www.w3.org/TR/css-fonts-4/#font-variant-prop>
fn parse_font_variant(p: &mut Parser<'_>) -> ParseResult<FontVariantCaps> {
    if p.expect_one_of(&[("normal", ()), ("none", ())]).is_ok() {
        return Ok(FontVariantCaps::Normal);
    }
    let mut caps = None;
    let mut any = false;
    while !p.is_exhausted() {
        if caps.is_none()
            && let Ok(c) = keyword(p, FontVariantCaps::from_ident)
        {
            caps = Some(c);
        } else {
            let other = keyword(p, |i| {
                OTHER_FONT_VARIANT_KEYWORDS
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(i))
                    .then_some(())
            });
            if other.is_err() && p.expect_function().is_err() {
                return Err(ParseError::Unexpected);
            }
        }
        any = true;
    }
    if !any {
        return Err(ParseError::EndOfInput);
    }
    Ok(caps.unwrap_or(FontVariantCaps::Normal))
}

/// `list-style`: `<type> || <position> || <image>`, where `none` sets
/// whichever of type and image is not given otherwise.
/// <https://www.w3.org/TR/css-lists-3/#list-style-property>
fn parse_list_style(
    p: &mut Parser<'_>,
    cx: &ParserContext,
    out: &mut Vec<LonghandValue>,
) -> ParseResult<()> {
    let mut nones = 0;
    let mut position = None;
    let mut image: Option<Option<SpecifiedImage>> = None;
    let mut list_type = None;
    while !p.is_exhausted() {
        if p.expect_ident_matching("none").is_ok() {
            nones += 1;
        } else if position.is_none()
            && let Ok(pos) = keyword(p, ListStylePosition::from_ident)
        {
            position = Some(pos);
        } else if image.is_none() && p.peek().is_some_and(looks_like_image) {
            image = Some(parse_image_or_none(p, cx)?);
        } else if list_type.is_none()
            && let Ok(t) = parse_list_style_type(p)
        {
            list_type = Some(t);
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    match nones {
        0 => {}
        1 if list_type.is_none() => list_type = Some(ListStyleType::None),
        1 if image.is_none() => image = Some(None),
        2 if list_type.is_none() && image.is_none() => {
            list_type = Some(ListStyleType::None);
            image = Some(None);
        }
        _ => return Err(ParseError::Invalid),
    }
    if nones == 0 && position.is_none() && image.is_none() && list_type.is_none() {
        return Err(ParseError::EndOfInput);
    }
    out.push(LonghandValue::ListStyleType(
        list_type.unwrap_or(ListStyleType::Disc),
    ));
    out.push(LonghandValue::ListStylePosition(
        position.unwrap_or(ListStylePosition::Outside),
    ));
    out.push(LonghandValue::ListStyleImage(image.unwrap_or(None)));
    Ok(())
}

/// `text-decoration`: `<line> || <style> || <color> || <thickness>`. The
/// thickness is accepted and ignored.
fn parse_text_decoration(p: &mut Parser<'_>, out: &mut Vec<LonghandValue>) -> ParseResult<()> {
    let mut line = None;
    let mut style = None;
    let mut color = None;
    let mut thickness = false;
    while !p.is_exhausted() {
        if line.is_none()
            && let Ok(l) = parse_text_decoration_line(p)
        {
            line = Some(l);
        } else if style.is_none()
            && let Ok(s) = keyword(p, TextDecorationStyle::from_ident)
        {
            style = Some(s);
        } else if color.is_none()
            && let Ok(c) = parse_color(p)
        {
            color = Some(c);
        } else if !thickness
            && (p.expect_one_of(&[("auto", ()), ("from-font", ())]).is_ok()
                || parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE).is_ok())
        {
            thickness = true;
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    if line.is_none() && style.is_none() && color.is_none() && !thickness {
        return Err(ParseError::EndOfInput);
    }
    out.push(LonghandValue::TextDecorationLine(
        line.unwrap_or(TextDecorationLine::empty()),
    ));
    out.push(LonghandValue::TextDecorationStyle(
        style.unwrap_or(TextDecorationStyle::Solid),
    ));
    out.push(LonghandValue::TextDecorationColor(
        color.unwrap_or(Color::CurrentColor),
    ));
    Ok(())
}

/// `flex`: `none | [ <'flex-grow'> <'flex-shrink'>? || <'flex-basis'> ]`.
/// An omitted basis is `0%` (as in Chromium).
/// <https://www.w3.org/TR/css-flexbox-1/#flex-property>
fn parse_flex(p: &mut Parser<'_>, out: &mut Vec<LonghandValue>) -> ParseResult<()> {
    let auto_basis = SpecifiedFlexBasis::Size(SpecifiedSize::Auto);
    let (grow, shrink, basis) = if p.expect_ident_matching("none").is_ok() {
        (0.0, 0.0, auto_basis)
    } else {
        let mut grow = None;
        let mut shrink = None;
        let mut basis = None;
        while !p.is_exhausted() {
            if grow.is_none()
                && let Ok(g) = parse_non_negative_number(p)
            {
                grow = Some(g);
                if let Ok(s) = parse_non_negative_number(p) {
                    shrink = Some(s);
                }
            } else if basis.is_none()
                && let Ok(b) = parse_flex_basis(p)
            {
                basis = Some(b);
            } else {
                return Err(ParseError::Unexpected);
            }
        }
        if grow.is_none() && basis.is_none() {
            return Err(ParseError::EndOfInput);
        }
        let basis = basis.unwrap_or(SpecifiedFlexBasis::Size(SpecifiedSize::LengthPercentage(
            Lp::Percentage(0.0),
        )));
        (grow.unwrap_or(1.0), shrink.unwrap_or(1.0), basis)
    };
    out.push(LonghandValue::FlexGrow(grow));
    out.push(LonghandValue::FlexShrink(shrink));
    out.push(LonghandValue::FlexBasis(basis));
    Ok(())
}

/// `flex-flow`: `<'flex-direction'> || <'flex-wrap'>`.
fn parse_flex_flow(p: &mut Parser<'_>, out: &mut Vec<LonghandValue>) -> ParseResult<()> {
    let mut direction = None;
    let mut wrap = None;
    while !p.is_exhausted() {
        if direction.is_none()
            && let Ok(d) = keyword(p, FlexDirection::from_ident)
        {
            direction = Some(d);
        } else if wrap.is_none()
            && let Ok(w) = keyword(p, FlexWrap::from_ident)
        {
            wrap = Some(w);
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    if direction.is_none() && wrap.is_none() {
        return Err(ParseError::EndOfInput);
    }
    out.push(LonghandValue::FlexDirection(
        direction.unwrap_or(FlexDirection::Row),
    ));
    out.push(LonghandValue::FlexWrap(wrap.unwrap_or(FlexWrap::Nowrap)));
    Ok(())
}
