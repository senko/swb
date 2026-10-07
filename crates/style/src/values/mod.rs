//! Value types for CSS properties: specified values (as parsed) and computed
//! values (as stored in [`crate::ComputedStyle`]).

mod color;
mod grid;
mod keywords;
mod length;
mod mask;
mod transform;

use std::sync::Arc;

pub use color::{Color, Rgba};
pub(crate) use color::{named_color, system_color};
pub use grid::{
    GenericTrackBreadth, GenericTrackList, GenericTrackListEntry, GenericTrackListValue,
    GenericTrackRepeat, GenericTrackSize, GridAutoFlow, GridLine, GridTemplateAreas, LineName,
    LineNameTable, LineNames, NamePosition, NamedArea, RepeatCount, TrackBreadth, TrackList,
    TrackListEntry, TrackListValue, TrackRepeat, TrackSize,
};
pub(crate) use grid::{SpecifiedTrackList, SpecifiedTrackSize};
pub use keywords::*;
pub use length::{
    CalcNode, ComputedCalc, Length, LengthContext, LengthPercentage, LengthPercentageOrAuto,
    LengthUnit, MaxSize, Size, SpecifiedLengthPercentage,
};
pub use mask::{CompositeOperator, MAX_MASK_LAYERS, MaskClip, MaskImage, MaskMode};
pub use transform::{ClipRect, TransformFunction, TransformOrigin};

/// A generic font family keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GenericFamily {
    /// `serif`
    Serif,
    /// `sans-serif`
    SansSerif,
    /// `monospace`
    Monospace,
    /// `cursive`
    Cursive,
    /// `fantasy`
    Fantasy,
    /// `system-ui`
    SystemUi,
    /// `math`
    Math,
    /// `emoji`
    Emoji,
}

impl GenericFamily {
    /// Parses a generic family keyword (ASCII case-insensitive).
    pub fn from_ident(ident: &str) -> Option<Self> {
        let g = match ident.to_ascii_lowercase().as_str() {
            "serif" => Self::Serif,
            "sans-serif" => Self::SansSerif,
            "monospace" => Self::Monospace,
            "cursive" => Self::Cursive,
            "fantasy" => Self::Fantasy,
            "system-ui" | "-apple-system" | "blinkmacsystemfont" => Self::SystemUi,
            "math" => Self::Math,
            "emoji" => Self::Emoji,
            _ => return None,
        };
        Some(g)
    }
}

/// One entry of `font-family`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontFamily {
    /// A family name, for example "Times New Roman".
    Named(Arc<str>),
    /// A generic family.
    Generic(GenericFamily),
}

/// The computed `line-height`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum LineHeight {
    /// `normal`: use the font's metrics.
    #[default]
    Normal,
    /// A multiple of the font size. Inherited as the number.
    Number(f32),
    /// A length in px. Percentages compute to this.
    Px(f32),
}

impl LineHeight {
    /// The used line height for a font size, or `None` for `normal`.
    pub fn resolve(self, font_size: f32) -> Option<f32> {
        match self {
            LineHeight::Normal => None,
            LineHeight::Number(n) => Some(n * font_size),
            LineHeight::Px(px) => Some(px),
        }
    }
}

/// The computed `vertical-align`.
#[derive(Clone, Debug, PartialEq)]
pub enum VerticalAlign {
    /// A keyword.
    Keyword(VerticalAlignKeyword),
    /// A length or a percentage of the line height.
    LengthPercentage(LengthPercentage),
}

impl Default for VerticalAlign {
    fn default() -> Self {
        VerticalAlign::Keyword(VerticalAlignKeyword::Baseline)
    }
}

bitflags::bitflags! {
    /// The `text-decoration-line` property.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct TextDecorationLine: u8 {
        /// `underline`
        const UNDERLINE = 1;
        /// `overline`
        const OVERLINE = 2;
        /// `line-through`
        const LINE_THROUGH = 4;
    }
}

/// An image reference (for backgrounds, list markers, `content`).
#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    /// `url(...)`, already resolved against the stylesheet's base URL.
    Url(Arc<str>),
    /// A linear gradient.
    LinearGradient(Arc<LinearGradient>),
}

/// A computed `linear-gradient()`.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradient {
    /// The gradient line angle in degrees (0 = to top, 90 = to right).
    pub angle_deg: f32,
    /// Color stops, positions as fractions of the gradient line (resolved
    /// at computed-value time when given as percentages; `None` means
    /// "distribute evenly").
    pub stops: Vec<(Color, Option<LengthPercentage>)>,
    /// True for `repeating-linear-gradient()`.
    pub repeating: bool,
}

/// One axis of a `background-position`.
#[derive(Clone, Debug, PartialEq)]
pub struct PositionComponent {
    /// The offset from the start edge (a percentage aligns the image's
    /// matching point with the box's matching point).
    pub offset: LengthPercentage,
    /// True if `offset` is measured from the end edge (`right 10px`).
    pub from_end: bool,
}

impl PositionComponent {
    /// `center` (50%).
    pub const CENTER: PositionComponent = PositionComponent {
        offset: LengthPercentage::Percent(0.5),
        from_end: false,
    };
}

impl Default for PositionComponent {
    fn default() -> Self {
        PositionComponent {
            offset: LengthPercentage::Percent(0.0),
            from_end: false,
        }
    }
}

/// The `background-size` of one layer.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum BackgroundSize {
    /// Width and height; `Auto` keeps the aspect ratio.
    Explicit(LengthPercentageOrAuto, LengthPercentageOrAuto),
    /// `cover`.
    Cover,
    /// `contain`.
    Contain,
    /// `auto auto`.
    #[default]
    Auto,
}

/// The radius of one border corner: horizontal and vertical.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CornerRadius {
    /// Horizontal radius (percentages refer to the border box width).
    pub horizontal: LengthPercentage,
    /// Vertical radius (percentages refer to the border box height).
    pub vertical: LengthPercentage,
}

/// The computed `aspect-ratio`: `auto || <ratio>`
/// (<https://www.w3.org/TR/css-sizing-4/#aspect-ratio>).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AspectRatio {
    /// True if the value contains `auto`: a replaced element with a
    /// natural aspect ratio uses that ratio instead of `ratio`.
    pub auto: bool,
    /// The ratio (width / height); `None` if the value has none or it is
    /// degenerate (then the property behaves as `auto`).
    pub ratio: Option<f32>,
}

impl AspectRatio {
    /// `auto`, the initial value.
    pub const AUTO: AspectRatio = AspectRatio {
        auto: true,
        ratio: None,
    };

    /// The preferred aspect ratio of a box whose natural aspect ratio is
    /// `natural` (`None` for boxes without one, like non-replaced boxes).
    pub fn preferred(&self, natural: Option<f32>) -> Option<f32> {
        if self.auto {
            natural.or(self.ratio)
        } else {
            self.ratio.or(natural)
        }
    }
}

impl Default for AspectRatio {
    fn default() -> Self {
        AspectRatio::AUTO
    }
}

/// The computed `z-index`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZIndex {
    /// `auto`.
    #[default]
    Auto,
    /// An integer.
    Integer(i32),
}

/// The computed `flex-basis`.
#[derive(Clone, Debug, PartialEq)]
pub enum FlexBasis {
    /// `content`.
    Content,
    /// A `width`/`height` value (`auto` means "use the main size property").
    Size(Size),
}

impl Default for FlexBasis {
    fn default() -> Self {
        FlexBasis::Size(Size::Auto)
    }
}

/// One item of the `content` property.
#[derive(Clone, Debug, PartialEq)]
pub enum ContentItem {
    /// A string (also the computed value of `attr()`).
    String(Arc<str>),
    /// An image.
    Image(Image),
    /// `counter(name, style)`.
    Counter(Arc<str>, ListStyleType),
    /// `open-quote`.
    OpenQuote,
    /// `close-quote`.
    CloseQuote,
}

/// The computed `content` property.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Content {
    /// `normal`.
    #[default]
    Normal,
    /// `none`.
    None,
    /// A list of items.
    Items(Arc<[ContentItem]>),
}

/// The `gap` value for one axis (`row-gap`, `column-gap`).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Gap {
    /// `normal` (zero for flex and grid).
    #[default]
    Normal,
    /// A length or percentage.
    LengthPercentage(LengthPercentage),
}
