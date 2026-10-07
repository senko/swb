//! Presentational hints: HTML attributes that map to CSS properties.
//!
//! <https://html.spec.whatwg.org/multipage/rendering.html>
//!
//! Hints are author-level declarations with specificity zero that come
//! before all author rules. The hints that are plain CSS rules in the HTML
//! specification (for example `td[nowrap] { white-space: nowrap }`) are in
//! `hints.css`; the stylist adds them as hint rules. This module computes
//! the hints that need attribute parsing: legacy colors, dimensions, pixel
//! lengths, legacy font sizes, and the table attributes that affect cells
//! (`cellpadding`, `border`).
//!
//! Alignment attributes (`align` on `div`, `p`, headings and table parts)
//! use the `-webkit-` text-align values as Chromium does, so that block
//! children are aligned too ("align descendants" in the specification).

use std::sync::Arc;

use swb_css::Parser;
use swb_dom::{Document, ElementData, NodeId, is_html_whitespace};

use crate::parse::ParserContext;
use crate::parse::color::parse_legacy_color;
use crate::parse::image::SpecifiedImage;
use crate::properties::PropertyDeclaration;
use crate::properties::ids::LonghandValue;
use crate::properties::longhand::parse_font_family;
use crate::properties::specified::{SpecifiedFontSize, SpecifiedSize};
use crate::values::{
    AspectRatio, BorderStyle, CaptionSide, Color, FontFamily, FontSizeKeyword, Length, Rgba,
    SpecifiedLengthPercentage as Lp, TextAlign, WhiteSpace,
};

/// Document-wide data for hints.
#[derive(Clone, Debug)]
pub(crate) struct HintContext {
    /// For `background` attributes.
    parser: ParserContext,
    /// True in quirks mode.
    quirks: bool,
    /// The `link` attribute color of the body element.
    link_color: Option<Rgba>,
}

impl HintContext {
    /// Creates the context for a document.
    pub(crate) fn new(doc: &Document, parser: ParserContext, quirks: bool) -> Self {
        let link_color = doc
            .body()
            .and_then(|b| doc.element(b))
            .filter(|b| b.is_html_named(&swb_dom::local_name!("body")))
            .and_then(|b| b.attr("link"))
            .and_then(parse_legacy_color);
        HintContext {
            parser,
            quirks,
            link_color,
        }
    }
}

/// A parsed dimension value: a length in px or a percentage.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Dimension {
    /// Pixels.
    Length(f32),
    /// A percentage (50 for 50%).
    Percentage(f32),
}

impl Dimension {
    fn to_lp(self) -> Lp {
        match self {
            Dimension::Length(v) => Lp::Length(Length::px(v)),
            Dimension::Percentage(v) => Lp::Percentage(v / 100.0),
        }
    }
}

/// The rules for parsing dimension values.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-dimension-values>
fn parse_dimension(input: &str) -> Option<Dimension> {
    let input = input.trim_start_matches(is_html_whitespace);
    let digits = input.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let mut value: f64 = input[..digits].parse().ok()?;
    let mut rest = &input[digits..];
    if let Some(after_dot) = rest.strip_prefix('.') {
        let fraction = after_dot.bytes().take_while(u8::is_ascii_digit).count();
        if fraction > 0 {
            let mut divisor = 1.0;
            for b in after_dot[..fraction].bytes() {
                divisor *= 10.0;
                value += f64::from(b - b'0') / divisor;
            }
        }
        rest = &after_dot[fraction..];
    }
    // Huge values must not become infinite lengths.
    let value = (value as f32).min(Length::MAX_PX);
    if rest.starts_with('%') {
        Some(Dimension::Percentage(value))
    } else {
        Some(Dimension::Length(value))
    }
}

/// The rules for parsing nonzero dimension values.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-non-zero-dimension-values>
fn parse_nonzero_dimension(input: &str) -> Option<Dimension> {
    parse_dimension(input).filter(|d| match d {
        Dimension::Length(v) | Dimension::Percentage(v) => *v != 0.0,
    })
}

/// The rules for parsing integers (leading whitespace, optional sign,
/// digits; trailing text is ignored).
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-integers>
fn parse_integer(input: &str) -> Option<i64> {
    let input = input.trim_start_matches(is_html_whitespace);
    let (negative, rest) = match input.as_bytes().first() {
        Some(b'-') => (true, &input[1..]),
        Some(b'+') => (false, &input[1..]),
        _ => (false, input),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let value: i64 = rest[..digits.min(18)].parse().ok()?;
    Some(if negative { -value } else { value })
}

/// The rules for parsing non-negative integers.
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-non-negative-integers>
fn parse_non_negative_integer(input: &str) -> Option<i64> {
    parse_integer(input).filter(|v| *v >= 0)
}

/// A length in px, clamped to [`Length::MAX_PX`] so that huge attribute
/// values do not become infinite lengths.
fn px(v: f32) -> Lp {
    Lp::Length(Length::px(Length::clamp_px(v)))
}

/// Appends the presentational hints of an HTML element to `out`.
pub(crate) fn collect_hints(
    doc: &Document,
    node: NodeId,
    e: &ElementData,
    cx: &HintContext,
    out: &mut Vec<PropertyDeclaration>,
) {
    if !e.is_html() {
        return;
    }
    let mut push = |v: LonghandValue| out.push(PropertyDeclaration::Value(v));
    let name = &**e.local_name();
    match name {
        "body" => {
            background_hints(e, cx, &mut push);
            if let Some(c) = e.attr("text").and_then(parse_legacy_color) {
                push(LonghandValue::Color(Color::Rgba(c)));
            }
            body_margins(e, &mut push);
        }
        "table" => table_hints(e, cx, &mut push),
        "thead" | "tbody" | "tfoot" | "tr" => {
            background_hints(e, cx, &mut push);
            align_hint(e, &mut push);
            size_hint(e, "height", false, LonghandValue::Height, &mut push);
        }
        "td" | "th" => cell_hints(doc, node, e, cx, &mut push),
        "col" | "colgroup" => {
            size_hint(e, "width", false, LonghandValue::Width, &mut push);
        }
        "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => align_hint(e, &mut push),
        "caption" => {
            if let Some(side) = e.attr("align").and_then(CaptionSide::from_ident) {
                push(LonghandValue::CaptionSide(side));
            }
        }
        // `canvas` maps its attributes only to `aspect-ratio` (not
        // supported), not to `width` and `height`.
        "img" | "object" | "embed" | "iframe" | "video" | "input" => {
            if name == "input"
                && !e
                    .attr("type")
                    .is_some_and(|t| t.eq_ignore_ascii_case("image"))
            {
                return;
            }
            replaced_hints(e, name, &mut push);
        }
        "hr" => hr_hints(e, &mut push),
        "font" => font_hints(e, &mut push),
        "a" | "area" => {
            if let Some(c) = cx.link_color
                && e.has_attr("href")
            {
                push(LonghandValue::Color(Color::Rgba(c)));
            }
        }
        _ => {}
    }
}

/// A dimension attribute (`width`, `height`) as a size hint. With
/// `nonzero`, zero values are ignored.
fn size_hint(
    e: &ElementData,
    attr: &str,
    nonzero: bool,
    property: fn(SpecifiedSize) -> LonghandValue,
    push: &mut impl FnMut(LonghandValue),
) -> Option<Dimension> {
    let parse = if nonzero {
        parse_nonzero_dimension
    } else {
        parse_dimension
    };
    let d = e.attr(attr).and_then(parse)?;
    push(property(SpecifiedSize::LengthPercentage(d.to_lp())));
    Some(d)
}

/// Hints of `table` elements.
fn table_hints(e: &ElementData, cx: &HintContext, push: &mut impl FnMut(LonghandValue)) {
    background_hints(e, cx, push);
    size_hint(e, "width", true, LonghandValue::Width, push);
    size_hint(e, "height", false, LonghandValue::Height, push);
    if let Some(v) = e.attr("cellspacing").and_then(parse_non_negative_integer) {
        push(LonghandValue::BorderSpacingHorizontal(px(v as f32)));
        push(LonghandValue::BorderSpacingVertical(px(v as f32)));
    }
    if let Some(border) = e.attr("border") {
        let width = parse_non_negative_integer(border).unwrap_or(1) as f32;
        set_border(
            push,
            Some(px(width)),
            (width > 0.0).then_some(BorderStyle::Outset),
        );
    }
    if let Some(c) = e.attr("bordercolor").and_then(parse_legacy_color) {
        let c = Color::Rgba(c);
        push(LonghandValue::BorderTopColor(c));
        push(LonghandValue::BorderRightColor(c));
        push(LonghandValue::BorderBottomColor(c));
        push(LonghandValue::BorderLeftColor(c));
    }
}

/// Hints of `td` and `th` elements, including those from their table.
fn cell_hints(
    doc: &Document,
    node: NodeId,
    e: &ElementData,
    cx: &HintContext,
    push: &mut impl FnMut(LonghandValue),
) {
    background_hints(e, cx, push);
    align_hint(e, push);
    let width = size_hint(e, "width", true, LonghandValue::Width, push);
    size_hint(e, "height", true, LonghandValue::Height, push);
    // The nowrap rule is in hints.css; in quirks mode a fixed width
    // overrides it.
    if cx.quirks && e.has_attr("nowrap") && matches!(width, Some(Dimension::Length(_))) {
        push(LonghandValue::WhiteSpace(WhiteSpace::Normal));
    }
    cell_hints_from_table(doc, node, push);
}

/// `bgcolor` and `background` on `body` and table elements.
fn background_hints(e: &ElementData, cx: &HintContext, push: &mut impl FnMut(LonghandValue)) {
    if let Some(c) = e.attr("bgcolor").and_then(parse_legacy_color) {
        push(LonghandValue::BackgroundColor(Color::Rgba(c)));
    }
    if let Some(url) = e.attr("background").filter(|v| !v.trim().is_empty()) {
        let image = SpecifiedImage::Url(cx.parser.resolve_url(url.trim()));
        push(LonghandValue::BackgroundImage(Arc::from([Some(image)])));
    }
}

/// The body margin attributes.
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-page>
fn body_margins(e: &ElementData, push: &mut impl FnMut(LonghandValue)) {
    let attr = |names: [&str; 2]| {
        names
            .iter()
            .find_map(|n| e.attr(n))
            .and_then(parse_non_negative_integer)
    };
    if let Some(v) = attr(["marginheight", "topmargin"]) {
        push(LonghandValue::MarginTop(Some(px(v as f32))));
        push(LonghandValue::MarginBottom(Some(px(v as f32))));
    }
    if let Some(v) = attr(["marginwidth", "leftmargin"]) {
        push(LonghandValue::MarginLeft(Some(px(v as f32))));
        push(LonghandValue::MarginRight(Some(px(v as f32))));
    }
}

/// `align` on `div`, `p`, headings and table parts.
fn align_hint(e: &ElementData, push: &mut impl FnMut(LonghandValue)) {
    let Some(value) = e.attr("align") else {
        return;
    };
    let align = match value.trim().to_ascii_lowercase().as_str() {
        "left" => TextAlign::WebkitLeft,
        "right" => TextAlign::WebkitRight,
        "center" | "middle" => TextAlign::WebkitCenter,
        "justify" => TextAlign::Justify,
        _ => return,
    };
    push(LonghandValue::TextAlign(
        crate::properties::specified::SpecifiedTextAlign::Keyword(align),
    ));
}

/// Sets the four border widths and styles.
fn set_border(push: &mut impl FnMut(LonghandValue), width: Option<Lp>, style: Option<BorderStyle>) {
    if let Some(w) = width {
        push(LonghandValue::BorderTopWidth(w.clone()));
        push(LonghandValue::BorderRightWidth(w.clone()));
        push(LonghandValue::BorderBottomWidth(w.clone()));
        push(LonghandValue::BorderLeftWidth(w));
    }
    if let Some(s) = style {
        push(LonghandValue::BorderTopStyle(s));
        push(LonghandValue::BorderRightStyle(s));
        push(LonghandValue::BorderBottomStyle(s));
        push(LonghandValue::BorderLeftStyle(s));
    }
}

/// The table that a cell belongs to: the parent row's parent, or its
/// parent if that is a row group.
fn cell_table(doc: &Document, cell: NodeId) -> Option<&ElementData> {
    let row = doc.parent_element(cell)?;
    if !doc.element(row)?.is_html_named(&swb_dom::local_name!("tr")) {
        return None;
    }
    let mut parent = doc.parent_element(row)?;
    let parent_data = doc.element(parent)?;
    if parent_data.is_html() && matches!(&**parent_data.local_name(), "tbody" | "thead" | "tfoot") {
        parent = doc.parent_element(parent)?;
    }
    doc.element(parent)
        .filter(|t| t.is_html_named(&swb_dom::local_name!("table")))
}

/// Hints on cells from their table: `cellpadding` (padding) and a nonzero
/// `border` (1px inset cell borders).
fn cell_hints_from_table(doc: &Document, cell: NodeId, push: &mut impl FnMut(LonghandValue)) {
    let Some(table) = cell_table(doc, cell) else {
        return;
    };
    if let Some(v) = table
        .attr("cellpadding")
        .and_then(parse_non_negative_integer)
    {
        let v = px(v as f32);
        push(LonghandValue::PaddingTop(v.clone()));
        push(LonghandValue::PaddingRight(v.clone()));
        push(LonghandValue::PaddingBottom(v.clone()));
        push(LonghandValue::PaddingLeft(v));
    }
    if let Some(border) = table.attr("border")
        && parse_non_negative_integer(border).is_none_or(|v| v > 0)
    {
        set_border(push, Some(px(1.0)), Some(BorderStyle::Inset));
    }
}

/// Dimension, spacing and border attributes of replaced elements.
fn replaced_hints(e: &ElementData, name: &str, push: &mut impl FnMut(LonghandValue)) {
    let width = size_hint(e, "width", false, LonghandValue::Width, push);
    let height = size_hint(e, "height", false, LonghandValue::Height, push);
    // "Map to the aspect-ratio property (using dimension rules)": `auto
    // w / h` if both are lengths. Only for `video`: HTML also maps it on
    // `img` and image buttons, which do not need it yet (their images
    // have a natural ratio once loaded).
    if name == "video"
        && let (Some(Dimension::Length(w)), Some(Dimension::Length(h))) = (width, height)
    {
        push(LonghandValue::AspectRatio(AspectRatio {
            auto: true,
            ratio: (w > 0.0 && h > 0.0).then(|| w / h),
        }));
    }
    if matches!(name, "img" | "object" | "embed" | "input") {
        if let Some(d) = e.attr("hspace").and_then(parse_dimension) {
            push(LonghandValue::MarginLeft(Some(d.to_lp())));
            push(LonghandValue::MarginRight(Some(d.to_lp())));
        }
        if let Some(d) = e.attr("vspace").and_then(parse_dimension) {
            push(LonghandValue::MarginTop(Some(d.to_lp())));
            push(LonghandValue::MarginBottom(Some(d.to_lp())));
        }
        if matches!(name, "img" | "object" | "input")
            && let Some(v) = e.attr("border").and_then(parse_non_negative_integer)
            && v > 0
        {
            set_border(push, Some(px(v as f32)), Some(BorderStyle::Solid));
        }
    }
    if name == "iframe"
        && let Some(fb) = e.attr("frameborder")
        && parse_integer(fb).is_none_or(|v| v == 0)
    {
        set_border(push, Some(px(0.0)), None);
    }
}

/// `width`, `size`, `color` and `noshade` on `hr`.
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-hr-element-2>
fn hr_hints(e: &ElementData, push: &mut impl FnMut(LonghandValue)) {
    size_hint(e, "width", false, LonghandValue::Width, push);
    let color = e.attr("color").and_then(parse_legacy_color);
    if let Some(c) = color {
        push(LonghandValue::Color(Color::Rgba(c)));
    }
    let size = e.attr("size").and_then(parse_non_negative_integer);
    if color.is_some() || e.has_attr("noshade") {
        if let Some(s) = size {
            set_border(push, Some(px(s as f32 / 2.0)), None);
        }
    } else if let Some(s) = size {
        if s == 1 {
            push(LonghandValue::BorderBottomWidth(px(0.0)));
        } else if s > 1 {
            push(LonghandValue::Height(SpecifiedSize::LengthPercentage(px(
                s as f32 - 2.0,
            ))));
        }
    }
}

/// `color`, `face` and `size` on `font`.
/// <https://html.spec.whatwg.org/multipage/rendering.html#phrasing-content-3>
fn font_hints(e: &ElementData, push: &mut impl FnMut(LonghandValue)) {
    if let Some(c) = e.attr("color").and_then(parse_legacy_color) {
        push(LonghandValue::Color(Color::Rgba(c)));
    }
    if let Some(face) = e.attr("face").filter(|f| !f.trim().is_empty()) {
        let values = swb_css::parse_component_values(face);
        let families = Parser::new(&values)
            .parse_entirely(parse_font_family)
            .unwrap_or_else(|_| Arc::from([FontFamily::Named(Arc::from(face.trim()))]));
        push(LonghandValue::FontFamily(families));
    }
    if let Some(size) = e.attr("size").and_then(parse_legacy_font_size) {
        let keyword = FontSizeKeyword::from_legacy_size(size);
        push(LonghandValue::FontSize(SpecifiedFontSize::Keyword(keyword)));
    }
}

/// The rules for parsing a legacy font size: returns 1 to 7.
/// <https://html.spec.whatwg.org/multipage/rendering.html#rules-for-parsing-a-legacy-font-size>
fn parse_legacy_font_size(input: &str) -> Option<i32> {
    let input = input.trim_start_matches(is_html_whitespace);
    let (mode, rest) = match input.as_bytes().first()? {
        b'+' => (1, &input[1..]),
        b'-' => (-1, &input[1..]),
        _ => (0, input),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let value: i64 = rest[..digits.min(9)].parse().ok()?;
    let value = match mode {
        1 => 3 + value,
        -1 => 3 - value,
        _ => value,
    };
    Some(value.clamp(1, 7) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions() {
        assert_eq!(parse_dimension("85%"), Some(Dimension::Percentage(85.0)));
        assert_eq!(parse_dimension(" 18"), Some(Dimension::Length(18.0)));
        assert_eq!(parse_dimension("18px"), Some(Dimension::Length(18.0)));
        assert_eq!(parse_dimension("12.5%"), Some(Dimension::Percentage(12.5)));
        assert_eq!(parse_dimension("12."), Some(Dimension::Length(12.0)));
        assert_eq!(parse_dimension("0"), Some(Dimension::Length(0.0)));
        assert_eq!(parse_dimension("x"), None);
        assert_eq!(parse_dimension(""), None);
        assert_eq!(parse_dimension("-5"), None);
        assert_eq!(parse_nonzero_dimension("0"), None);
        assert_eq!(parse_nonzero_dimension("0%"), None);
        let huge = "9".repeat(400);
        assert_eq!(
            parse_dimension(&huge),
            Some(Dimension::Length(Length::MAX_PX))
        );
    }

    #[test]
    fn integers_and_font_sizes() {
        assert_eq!(parse_integer(" 42abc"), Some(42));
        assert_eq!(parse_integer("-3"), Some(-3));
        assert_eq!(parse_integer("+3"), Some(3));
        assert_eq!(parse_integer("x"), None);
        assert_eq!(parse_non_negative_integer("-1"), None);
        assert_eq!(parse_legacy_font_size("2"), Some(2));
        assert_eq!(parse_legacy_font_size("+1"), Some(4));
        assert_eq!(parse_legacy_font_size("-2"), Some(1));
        assert_eq!(parse_legacy_font_size("10"), Some(7));
        assert_eq!(parse_legacy_font_size("0"), Some(1));
        assert_eq!(parse_legacy_font_size("x"), None);
    }
}
