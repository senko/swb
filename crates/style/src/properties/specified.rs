//! Specified values that need context to compute: font sizes and weights,
//! line heights, `text-align`, sizes, background positions and sizes,
//! `vertical-align`, `flex-basis` and `content`.
//!
//! Keyword properties and colors store their computed value directly; see
//! [`super::ids::LonghandValue`].

use std::sync::Arc;

use super::compute::ComputeContext;
use crate::parse::image::SpecifiedImage;
use crate::values::{
    BackgroundSize, Content, ContentItem, FlexBasis, FontSizeKeyword, FontSizeOrigin,
    LengthPercentage, LengthPercentageOrAuto, LineHeight, ListStyleType, MaxSize,
    PositionComponent, Size, SpecifiedLengthPercentage as Lp, TextAlign, VerticalAlign,
    VerticalAlignKeyword,
};

/// The ratio between adjacent font sizes for `larger` and `smaller`.
/// <https://www.w3.org/TR/css-fonts-4/#relative-size-value>
const FONT_SIZE_RATIO: f32 = 1.2;

/// A specified `font-size`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedFontSize {
    /// An absolute-size keyword.
    Keyword(FontSizeKeyword),
    /// `larger`.
    Larger,
    /// `smaller`.
    Smaller,
    /// `math`: the parent's size (swb does not support `math-depth`).
    Math,
    /// A length or percentage of the parent's font size.
    Length(Lp),
}

impl SpecifiedFontSize {
    /// Computes the font size and how it was derived. `monospace` tells
    /// whether the element's font family is the generic `monospace`.
    ///
    /// The derivation follows Blink (`StyleBuilderConverter::ConvertFontSize`):
    /// percentages, `larger`, `smaller` and the font-relative units `em`,
    /// `ex` and `ch` keep the parent's origin; other lengths, `rem` and math
    /// functions are absolute.
    pub(crate) fn compute(
        &self,
        cx: &ComputeContext<'_>,
        monospace: bool,
    ) -> (f32, FontSizeOrigin) {
        let parent = cx.parent;
        let relative = match parent.font_size_origin {
            FontSizeOrigin::Absolute => FontSizeOrigin::Absolute,
            _ => FontSizeOrigin::RelativeToKeyword,
        };
        match self {
            SpecifiedFontSize::Keyword(k) => {
                (k.to_px(monospace, cx.quirks), FontSizeOrigin::Keyword(*k))
            }
            SpecifiedFontSize::Larger => (parent.font_size * FONT_SIZE_RATIO, relative),
            SpecifiedFontSize::Smaller => (parent.font_size / FONT_SIZE_RATIO, relative),
            SpecifiedFontSize::Math => (parent.font_size, relative),
            SpecifiedFontSize::Length(lp) => {
                let font_relative = match lp {
                    Lp::Percentage(_) => true,
                    Lp::Length(l) => matches!(
                        l.unit,
                        crate::values::LengthUnit::Em
                            | crate::values::LengthUnit::Ex
                            | crate::values::LengthUnit::Ch
                    ),
                    Lp::Calc(_) => false,
                };
                // Percentages refer to the parent's font size, which is the
                // font size in `cx.lengths` during font-size computation.
                let size = lp.compute(&cx.lengths).resolve(parent.font_size);
                let origin = if font_relative {
                    relative
                } else {
                    FontSizeOrigin::Absolute
                };
                (size.max(0.0), origin)
            }
        }
    }
}

/// A specified `font-weight`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SpecifiedFontWeight {
    /// A number from 1 to 1000 (`normal` is 400, `bold` is 700).
    Absolute(f32),
    /// `bolder`.
    Bolder,
    /// `lighter`.
    Lighter,
}

impl SpecifiedFontWeight {
    /// Computes the weight. Relative weights follow the table in CSS Fonts 4.
    /// <https://www.w3.org/TR/css-fonts-4/#relative-weights>
    pub(crate) fn compute(self, parent: f32) -> f32 {
        match self {
            SpecifiedFontWeight::Absolute(w) => w,
            SpecifiedFontWeight::Bolder => {
                if parent < 350.0 {
                    400.0
                } else if parent < 550.0 {
                    700.0
                } else if parent < 900.0 {
                    900.0
                } else {
                    parent
                }
            }
            SpecifiedFontWeight::Lighter => {
                if parent < 100.0 {
                    parent
                } else if parent < 550.0 {
                    100.0
                } else if parent < 750.0 {
                    400.0
                } else {
                    700.0
                }
            }
        }
    }
}

/// A specified `line-height`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedLineHeight {
    /// `normal`.
    Normal,
    /// A number (inherited as a number).
    Number(f32),
    /// A length or percentage (computes to px).
    LengthPercentage(Lp),
}

impl SpecifiedLineHeight {
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> LineHeight {
        match self {
            SpecifiedLineHeight::Normal => LineHeight::Normal,
            SpecifiedLineHeight::Number(n) => LineHeight::Number(*n),
            SpecifiedLineHeight::LengthPercentage(lp) => {
                LineHeight::Px(lp.compute(&cx.lengths).resolve(cx.lengths.font_size))
            }
        }
    }
}

/// A specified `text-align`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SpecifiedTextAlign {
    /// A keyword.
    Keyword(TextAlign),
    /// `-internal-center` (UA stylesheet only): `center`, unless the parent's
    /// `text-align` is not the initial value, in which case it inherits.
    /// It implements the `th` rule of the HTML rendering section:
    /// <https://html.spec.whatwg.org/multipage/rendering.html#tables-2>
    InternalCenter,
}

impl SpecifiedTextAlign {
    pub(crate) fn compute(self, cx: &ComputeContext<'_>) -> TextAlign {
        match self {
            SpecifiedTextAlign::InternalCenter => {
                if cx.parent.text_align == TextAlign::Start {
                    TextAlign::Center
                } else {
                    cx.parent.text_align
                }
            }
            // `match-parent` computes to the parent's value. CSS Text 3
            // resolves `start` and `end` against the parent's direction;
            // Chromium keeps them (so that `th` in a list item is still
            // centered), and swb follows Chromium.
            // <https://www.w3.org/TR/css-text-3/#valdef-text-align-match-parent>
            SpecifiedTextAlign::Keyword(TextAlign::MatchParent) => cx.parent.text_align,
            SpecifiedTextAlign::Keyword(k) => k,
        }
    }
}

/// A specified `width`, `height`, `min-*` or `max-*` value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedSize {
    /// `auto` (also `stretch` and `-webkit-fill-available`).
    Auto,
    /// `none` (only for `max-width` and `max-height`).
    None,
    /// A length or percentage.
    LengthPercentage(Lp),
    /// `min-content`.
    MinContent,
    /// `max-content`.
    MaxContent,
    /// `fit-content` or `fit-content(<length-percentage>)`.
    FitContent(Option<Lp>),
}

impl SpecifiedSize {
    /// Computes a `width`/`height`/`min-*` value.
    pub(crate) fn compute_size(&self, cx: &ComputeContext<'_>) -> Size {
        match self {
            SpecifiedSize::Auto | SpecifiedSize::None => Size::Auto,
            SpecifiedSize::LengthPercentage(lp) => Size::LengthPercentage(cx.non_negative(lp)),
            SpecifiedSize::MinContent => Size::MinContent,
            SpecifiedSize::MaxContent => Size::MaxContent,
            SpecifiedSize::FitContent(lp) => {
                Size::FitContent(lp.as_ref().map(|lp| cx.non_negative(lp)))
            }
        }
    }

    /// Computes a `max-width`/`max-height` value.
    pub(crate) fn compute_max_size(&self, cx: &ComputeContext<'_>) -> MaxSize {
        match self {
            SpecifiedSize::Auto | SpecifiedSize::None => MaxSize::None,
            SpecifiedSize::LengthPercentage(lp) => MaxSize::LengthPercentage(cx.non_negative(lp)),
            SpecifiedSize::MinContent => MaxSize::MinContent,
            SpecifiedSize::MaxContent => MaxSize::MaxContent,
            SpecifiedSize::FitContent(lp) => {
                MaxSize::FitContent(lp.as_ref().map(|lp| cx.non_negative(lp)))
            }
        }
    }
}

/// One axis of a specified `background-position`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpecifiedPosition {
    /// The offset from the start edge, or from the end edge if `from_end`.
    pub(crate) offset: Lp,
    /// True if the offset is measured from the end edge.
    pub(crate) from_end: bool,
}

impl SpecifiedPosition {
    /// The position at a fraction of the box (`center` is 0.5).
    pub(crate) fn percent(fraction: f32) -> Self {
        SpecifiedPosition {
            offset: Lp::Percentage(fraction),
            from_end: false,
        }
    }

    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> PositionComponent {
        PositionComponent {
            offset: self.offset.compute(&cx.lengths),
            from_end: self.from_end,
        }
    }
}

/// A specified `background-size` layer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedBackgroundSize {
    /// Width and height; `None` is `auto`.
    Explicit(Option<Lp>, Option<Lp>),
    /// `cover`.
    Cover,
    /// `contain`.
    Contain,
}

impl SpecifiedBackgroundSize {
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> BackgroundSize {
        let axis = |v: &Option<Lp>| match v {
            None => LengthPercentageOrAuto::Auto,
            Some(lp) => LengthPercentageOrAuto::LengthPercentage(cx.non_negative(lp)),
        };
        match self {
            SpecifiedBackgroundSize::Explicit(None, None) => BackgroundSize::Auto,
            SpecifiedBackgroundSize::Explicit(w, h) => BackgroundSize::Explicit(axis(w), axis(h)),
            SpecifiedBackgroundSize::Cover => BackgroundSize::Cover,
            SpecifiedBackgroundSize::Contain => BackgroundSize::Contain,
        }
    }
}

/// A specified `vertical-align`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedVerticalAlign {
    /// A keyword.
    Keyword(VerticalAlignKeyword),
    /// A length or a percentage of the line height.
    LengthPercentage(Lp),
}

impl SpecifiedVerticalAlign {
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> VerticalAlign {
        match self {
            SpecifiedVerticalAlign::Keyword(k) => VerticalAlign::Keyword(*k),
            SpecifiedVerticalAlign::LengthPercentage(lp) => {
                VerticalAlign::LengthPercentage(lp.compute(&cx.lengths))
            }
        }
    }
}

/// A specified `flex-basis`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedFlexBasis {
    /// `content`.
    Content,
    /// A `width` value.
    Size(SpecifiedSize),
}

impl SpecifiedFlexBasis {
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> FlexBasis {
        match self {
            SpecifiedFlexBasis::Content => FlexBasis::Content,
            SpecifiedFlexBasis::Size(s) => FlexBasis::Size(s.compute_size(cx)),
        }
    }
}

/// One specified item of the `content` property.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedContentItem {
    /// A string.
    String(Arc<str>),
    /// `attr(name)`: computes to the attribute's value.
    Attr(Arc<str>),
    /// An image (`None` if swb cannot render it).
    Image(Option<SpecifiedImage>),
    /// `counter()` or `counters()`.
    Counter(Arc<str>, ListStyleType),
    /// `open-quote`.
    OpenQuote,
    /// `close-quote`.
    CloseQuote,
}

/// A specified `content` value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedContent {
    /// `normal`.
    Normal,
    /// `none`.
    None,
    /// A list of items.
    Items(Arc<[SpecifiedContentItem]>),
}

impl SpecifiedContent {
    /// Computes the value. `attr()` becomes the attribute's string value
    /// (empty if the attribute is missing), as CSS Values 5 defines.
    /// <https://www.w3.org/TR/css-values-5/#attr-notation>
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> Content {
        match self {
            SpecifiedContent::Normal => Content::Normal,
            SpecifiedContent::None => Content::None,
            SpecifiedContent::Items(items) => Content::Items(
                items
                    .iter()
                    .filter_map(|item| {
                        Some(match item {
                            SpecifiedContentItem::String(s) => ContentItem::String(Arc::clone(s)),
                            SpecifiedContentItem::Attr(name) => {
                                let value =
                                    cx.element.and_then(|e| e.attr(name)).unwrap_or_default();
                                ContentItem::String(Arc::from(value))
                            }
                            SpecifiedContentItem::Image(image) => {
                                ContentItem::Image(image.as_ref()?.compute(&cx.lengths))
                            }
                            SpecifiedContentItem::Counter(name, style) => {
                                ContentItem::Counter(Arc::clone(name), *style)
                            }
                            SpecifiedContentItem::OpenQuote => ContentItem::OpenQuote,
                            SpecifiedContentItem::CloseQuote => ContentItem::CloseQuote,
                        })
                    })
                    .collect(),
            ),
        }
    }
}

/// Computes a length-percentage that cannot be negative: plain lengths
/// are clamped to zero (CSS clamps `calc()` results to the allowed range).
pub(crate) fn clamp_non_negative(value: LengthPercentage) -> LengthPercentage {
    match value {
        LengthPercentage::Px(v) => LengthPercentage::Px(v.max(0.0)),
        LengthPercentage::Percent(p) => LengthPercentage::Percent(p.max(0.0)),
        calc @ LengthPercentage::Calc(_) => calc,
    }
}
