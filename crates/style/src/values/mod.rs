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

/// The computed `contain`: the containment types that apply to the box
/// (`strict` and `content` are expanded).
/// <https://www.w3.org/TR/css-contain-2/#contain-property>
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Contain {
    /// Size containment: the box is sized as if it had no content.
    pub size: bool,
    /// Inline-size containment (only the inline axis).
    pub inline_size: bool,
    /// Layout containment.
    pub layout: bool,
    /// Style containment.
    pub style: bool,
    /// Paint containment.
    pub paint: bool,
}

impl Contain {
    /// `none`, the initial value.
    pub const NONE: Contain = Contain {
        size: false,
        inline_size: false,
        layout: false,
        style: false,
        paint: false,
    };

    /// `content`: `layout paint style`.
    pub const CONTENT: Contain = Contain {
        layout: true,
        paint: true,
        style: true,
        ..Contain::NONE
    };

    /// `strict`: `size layout paint style`.
    pub const STRICT: Contain = Contain {
        size: true,
        ..Contain::CONTENT
    };
}

/// The computed `contain-intrinsic-width` or `-height`:
/// `auto? [none | <length>]`
/// (<https://www.w3.org/TR/css-sizing-4/#intrinsic-size-override>).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContainIntrinsic {
    /// True if the value starts with `auto`: a box that skips its content
    /// (`content-visibility: auto`) remembers its last size. swb has no
    /// such boxes, so the flag changes nothing.
    pub auto: bool,
    /// The length in px; `None` for `none`.
    pub length: Option<f32>,
}

impl ContainIntrinsic {
    /// `none`, the initial value.
    pub const NONE: ContainIntrinsic = ContainIntrinsic {
        auto: false,
        length: None,
    };

    /// The size that a box with size containment has in this axis:
    /// the length, or 0 for `none`.
    pub fn size(&self) -> f32 {
        self.length.unwrap_or(0.0)
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
    /// A string (also the computed value of `attr()`, and of `counter()`
    /// and `counters()` after the counters of the document are resolved).
    String(Arc<str>),
    /// An image.
    Image(Image),
    /// `counter(name, style)` (no separator) or
    /// `counters(name, separator, style)`.
    Counter {
        /// The counter name.
        name: Arc<str>,
        /// The separator of `counters()`; `None` for `counter()`.
        separator: Option<Arc<str>>,
        /// The counter style.
        style: ListStyleType,
    },
    /// `open-quote`.
    OpenQuote,
    /// `close-quote`.
    CloseQuote,
}

/// The computed `counter-reset`, `counter-increment` or `counter-set`:
/// counter names with their integers, in source order. `none` is empty.
/// <https://www.w3.org/TR/css-lists-3/#auto-numbering>
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CounterList(Option<CounterEntries>);

/// The `(name, integer)` pairs of a [`CounterList`].
type CounterEntries = Arc<[(Arc<str>, i32)]>;

impl CounterList {
    /// A list of `(name, integer)` pairs.
    pub(crate) fn new(entries: Vec<(Arc<str>, i32)>) -> Self {
        if entries.is_empty() {
            CounterList(None)
        } else {
            CounterList(Some(Arc::from(entries)))
        }
    }

    /// True for `none`.
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// The address of the shared list (0 for `none`): the copies of one
    /// computed value have the same address.
    pub(crate) fn address(&self) -> usize {
        self.0
            .as_ref()
            .map_or(0, |list| Arc::as_ptr(list).cast::<()>().addr())
    }

    /// The `(name, integer)` pairs in source order.
    pub fn iter(&self) -> impl Iterator<Item = (&Arc<str>, i32)> {
        self.0.iter().flat_map(|e| e.iter().map(|(n, v)| (n, *v)))
    }
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
