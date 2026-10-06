//! Computed values of the mask properties (CSS Masking 1 §7,
//! <https://www.w3.org/TR/css-masking-1/#positioned-masks>).

use super::Image;

/// The number of mask layers that swb keeps and paints. The computed
/// value of each mask property keeps at most this many entries: layer
/// `i` uses entry `i` modulo the list length, so the entries after the
/// first `MAX_MASK_LAYERS` never matter for the layers that are painted.
/// This bounds the memory and the paint time of hostile values (a list of
/// thousands of layers on every element).
pub const MAX_MASK_LAYERS: usize = 32;

/// One layer of `mask-image`.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum MaskImage {
    /// `none`: a layer of transparent black. An element whose layers are
    /// all `none` is not masked.
    #[default]
    None,
    /// An image.
    Image(Image),
    /// A reference that never renders: an empty URL, or a reference to an
    /// SVG `<mask>` element (`url(#id)`, not supported). It paints as an
    /// image that failed to load: transparent black.
    Failed,
    /// A valid image that swb cannot draw (radial and conic gradients,
    /// `-webkit-gradient()`). It is opaque in the whole mask painting area,
    /// so the element is visible there, unmasked.
    Unsupported,
}

/// One layer of `mask-mode`.
/// <https://www.w3.org/TR/css-masking-1/#the-mask-mode>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MaskMode {
    /// `match-source`: alpha for images (luminance only applies to SVG
    /// `<mask>` elements, which are not supported).
    #[default]
    MatchSource,
    /// `alpha`.
    Alpha,
    /// `luminance`.
    Luminance,
}

/// One layer of `mask-clip`. `fill-box` is stored as `ContentBox`,
/// `stroke-box` and `view-box` as `BorderBox` (their used values for
/// boxes with a CSS layout box), and the legacy `-webkit-mask-clip: text`
/// as `BorderBox` (swb does not clip to text).
/// <https://www.w3.org/TR/css-masking-1/#the-mask-clip>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MaskClip {
    /// `border-box`.
    #[default]
    BorderBox,
    /// `padding-box`.
    PaddingBox,
    /// `content-box`.
    ContentBox,
    /// `no-clip`.
    NoClip,
}

/// A compositing operator of `mask-composite` (CSS Masking 1 §7.8,
/// <https://www.w3.org/TR/css-masking-1/#the-mask-composite>) or of the
/// legacy `-webkit-mask-composite`. The standard keywords are `add`
/// (`SourceOver`), `subtract` (`SourceOut`), `intersect` (`SourceIn`) and
/// `exclude` (`Xor`); the others are Porter-Duff operators that only the
/// legacy property accepts. The source is the layer, the destination
/// the layers below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CompositeOperator {
    /// `add`, `source-over`.
    #[default]
    SourceOver,
    /// `subtract`, `source-out`.
    SourceOut,
    /// `intersect`, `source-in`.
    SourceIn,
    /// `exclude`, `xor`.
    Xor,
    /// `clear`.
    Clear,
    /// `copy`.
    Copy,
    /// `source-atop`.
    SourceAtop,
    /// `destination-over`.
    DestinationOver,
    /// `destination-in`.
    DestinationIn,
    /// `destination-out`.
    DestinationOut,
    /// `destination-atop`.
    DestinationAtop,
    /// `plus-lighter`.
    PlusLighter,
}
