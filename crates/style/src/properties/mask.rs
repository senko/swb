//! The mask properties: CSS Masking 1 §7
//! (<https://www.w3.org/TR/css-masking-1/#positioned-masks>) with the
//! property names and syntax of Chromium.
//!
//! Longhands: `mask-image`, `mask-mode`, `-webkit-mask-position-x`,
//! `-webkit-mask-position-y`, `mask-size`, `mask-repeat`, `mask-origin`,
//! `mask-clip` and `mask-composite`. In Chromium `mask-position` is a
//! shorthand of the two `-webkit-` position longhands, so swb does the
//! same.
//!
//! Chromium's `-webkit-` names come in two kinds (checked with
//! `CSS.supports()` in Chromium 148):
//!
//! - Aliases with the same syntax: `-webkit-mask-image`,
//!   `-webkit-mask-size` and `-webkit-mask-repeat` (in
//!   `super::longhand_alias`).
//! - Legacy forms that set the same longhands with another syntax, parsed
//!   here as shorthands: `-webkit-mask` and `-webkit-mask-clip` accept
//!   `border`, `padding`, `content` and `text` but not `no-clip` or
//!   `fill-box`; `-webkit-mask-origin` accepts the first three;
//!   `-webkit-mask-composite` takes the Porter-Duff keywords
//!   (`source-over`, `xor`, ...) instead of `add`, `subtract`, ...;
//!   `-webkit-mask-position`, like the `mask` shorthand, accepts the
//!   three-value position that `mask-position` rejects.
//!
//! `mask-type` (for SVG `<mask>` elements, which swb does not support) is
//! validated and ignored. `mask-border` and `-webkit-mask-box-image` are
//! not supported.

use std::sync::Arc;

use swb_css::{ComponentValue, ParseError, Parser};

use super::CssWideKeyword;
use super::compute::ComputeContext;
use super::has_references;
use super::ids::{LonghandId, LonghandValue};
use super::longhand::{
    keyword, list, parse_background_repeat, parse_background_size, parse_bg_position,
    parse_position, parse_position_axis,
};
use super::shorthand::ShorthandId;
use super::specified::{SpecifiedBackgroundSize, SpecifiedPosition};
use crate::ComputedStyle;
use crate::parse::image::{SpecifiedImage, looks_like_image, parse_image};
use crate::parse::{ParseResult, ParserContext};
use crate::values::{
    BackgroundBox, BackgroundRepeatKeyword, CompositeOperator, LengthContext, MAX_MASK_LAYERS,
    MaskClip, MaskImage, MaskMode,
};

/// The standard syntax or Chromium's legacy `-webkit-` syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Syntax {
    Standard,
    Legacy,
}

/// A specified layer of `mask-image`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedMaskImage {
    /// `none`.
    None,
    /// An image.
    Image(SpecifiedImage),
    /// A reference that never renders (see [`MaskImage::Failed`]).
    Failed,
    /// An image that swb cannot draw (see [`MaskImage::Unsupported`]).
    Unsupported,
}

impl SpecifiedMaskImage {
    fn compute(&self, lengths: &LengthContext) -> MaskImage {
        match self {
            SpecifiedMaskImage::None => MaskImage::None,
            SpecifiedMaskImage::Image(image) => MaskImage::Image(image.compute(lengths)),
            SpecifiedMaskImage::Failed => MaskImage::Failed,
            SpecifiedMaskImage::Unsupported => MaskImage::Unsupported,
        }
    }
}

/// Parses the value of a mask longhand.
pub(crate) fn parse_longhand(
    id: LonghandId,
    p: &mut Parser<'_>,
    cx: &ParserContext,
) -> ParseResult<LonghandValue> {
    use LonghandId as L;
    use LonghandValue as V;
    Ok(match id {
        L::MaskImage => {
            let images = list(p, |p| parse_mask_image(p, cx))?;
            log_if_too_many(images.len());
            V::MaskImage(images)
        }
        L::MaskMode => V::MaskMode(list(p, parse_mask_mode)?),
        L::MaskPositionX => V::MaskPositionX(list(p, |p| parse_axis(p, true))?),
        L::MaskPositionY => V::MaskPositionY(list(p, |p| parse_axis(p, false))?),
        L::MaskSize => V::MaskSize(list(p, parse_background_size)?),
        L::MaskRepeat => V::MaskRepeat(list(p, parse_background_repeat)?),
        L::MaskOrigin => V::MaskOrigin(list(p, |p| parse_origin(p, Syntax::Standard))?),
        L::MaskClip => V::MaskClip(list(p, |p| parse_clip(p, Syntax::Standard))?),
        L::MaskComposite => V::MaskComposite(list(p, |p| parse_composite(p, Syntax::Standard))?),
        _ => return Err(ParseError::Invalid),
    })
}

/// Parses the value of a mask shorthand (or legacy form) into `out`.
pub(crate) fn parse_shorthand(
    id: ShorthandId,
    p: &mut Parser<'_>,
    cx: &ParserContext,
    out: &mut Vec<LonghandValue>,
) -> ParseResult<()> {
    use LonghandValue as V;
    use ShorthandId as S;
    match id {
        S::Mask => parse_mask(p, cx, Syntax::Standard, out),
        S::WebkitMask => parse_mask(p, cx, Syntax::Legacy, out),
        S::MaskPosition | S::WebkitMaskPosition => {
            let positions: Vec<(SpecifiedPosition, SpecifiedPosition)> = if id == S::MaskPosition {
                p.parse_comma_separated(parse_position)?
            } else {
                p.parse_comma_separated(|p| parse_bg_position(p, false))?
            };
            let (x, y): (Vec<_>, Vec<_>) = positions.into_iter().unzip();
            out.push(V::MaskPositionX(Arc::from(x)));
            out.push(V::MaskPositionY(Arc::from(y)));
            Ok(())
        }
        S::WebkitMaskOrigin => {
            out.push(V::MaskOrigin(list(p, |p| parse_origin(p, Syntax::Legacy))?));
            Ok(())
        }
        S::WebkitMaskClip => {
            out.push(V::MaskClip(list(p, |p| parse_clip(p, Syntax::Legacy))?));
            Ok(())
        }
        S::WebkitMaskComposite => {
            out.push(V::MaskComposite(list(p, |p| {
                parse_composite(p, Syntax::Legacy)
            })?));
            Ok(())
        }
        _ => Err(ParseError::Invalid),
    }
}

/// Stores the computed value of a mask longhand in `s`. Lists keep their
/// first [`MAX_MASK_LAYERS`] entries.
pub(crate) fn apply(value: &LonghandValue, cx: &ComputeContext<'_>, s: &mut ComputedStyle) {
    use LonghandValue as V;
    match value {
        V::MaskImage(v) => s.mask_image = layers(v, |i| i.compute(&cx.lengths)),
        V::MaskMode(v) => s.mask_mode = truncated(v),
        V::MaskPositionX(v) => s.mask_position_x = layers(v, |p| p.compute(cx)),
        V::MaskPositionY(v) => s.mask_position_y = layers(v, |p| p.compute(cx)),
        V::MaskSize(v) => s.mask_size = layers(v, |b| b.compute(cx)),
        V::MaskRepeat(v) => s.mask_repeat = truncated(v),
        V::MaskOrigin(v) => s.mask_origin = truncated(v),
        V::MaskClip(v) => s.mask_clip = truncated(v),
        V::MaskComposite(v) => s.mask_composite = truncated(v),
        _ => {}
    }
}

/// Logs a declaration with more layers than swb keeps (at debug level: a
/// value with `var()` is parsed again for every element).
fn log_if_too_many(layers: usize) {
    if layers > MAX_MASK_LAYERS {
        log::debug!("mask: {layers} layers, only the first {MAX_MASK_LAYERS} are used");
    }
}

/// The computed values of the first [`MAX_MASK_LAYERS`] entries.
fn layers<S, C>(specified: &[S], compute: impl FnMut(&S) -> C) -> Arc<[C]> {
    specified
        .iter()
        .take(MAX_MASK_LAYERS)
        .map(compute)
        .collect()
}

/// The list, shared if it is short enough.
fn truncated<T: Clone>(list: &Arc<[T]>) -> Arc<[T]> {
    if list.len() <= MAX_MASK_LAYERS {
        Arc::clone(list)
    } else {
        Arc::from(&list[..MAX_MASK_LAYERS])
    }
}

/// For a property that swb accepts but ignores: `Some(valid)`. Only
/// `mask-type` (`alpha | luminance`).
/// <https://www.w3.org/TR/css-masking-1/#the-mask-type>
pub(crate) fn ignored_property(name: &str, value: &[ComponentValue]) -> Option<bool> {
    if name != "mask-type" {
        return None;
    }
    if CssWideKeyword::from_value(value).is_some() || has_references(value) {
        return Some(true);
    }
    let mut p = Parser::new(value);
    Some(
        p.parse_entirely(|p| p.expect_one_of(&[("alpha", ()), ("luminance", ())]))
            .is_ok(),
    )
}

/// `<mask-reference>`: `none | <image>` (and `url()` references to SVG
/// `<mask>` elements, which fail). Chromium rejects `element()`,
/// `paint()` and `cross-fade()` here.
fn parse_mask_image(p: &mut Parser<'_>, cx: &ParserContext) -> ParseResult<SpecifiedMaskImage> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(SpecifiedMaskImage::None);
    }
    if let Ok(url) = p.expect_url() {
        let url = url.trim();
        // A fragment-only URL refers to a `<mask>` element in the
        // document (CSS Values 4 §4.5.1); an empty URL fails to load.
        if url.is_empty() || url.starts_with('#') {
            return Ok(SpecifiedMaskImage::Failed);
        }
        return Ok(SpecifiedMaskImage::Image(SpecifiedImage::Url(
            cx.resolve_url(url),
        )));
    }
    if let Some(ComponentValue::Function(f)) = p.peek() {
        let name = f.name.to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "element" | "-moz-element" | "paint" | "cross-fade"
        ) {
            return Err(ParseError::Unexpected);
        }
    }
    Ok(match parse_image(p, cx)? {
        Some(image) => SpecifiedMaskImage::Image(image),
        None => SpecifiedMaskImage::Unsupported,
    })
}

/// `<masking-mode>`: `alpha | luminance | match-source`.
fn parse_mask_mode(p: &mut Parser<'_>) -> ParseResult<MaskMode> {
    p.expect_one_of(&[
        ("alpha", MaskMode::Alpha),
        ("luminance", MaskMode::Luminance),
        ("match-source", MaskMode::MatchSource),
    ])
}

/// One axis of the position: like `background-position-x`/`-y`, without
/// the logical keywords (`x-start`, ...), which Chromium rejects.
fn parse_axis(p: &mut Parser<'_>, horizontal: bool) -> ParseResult<SpecifiedPosition> {
    let logical = ["x-start", "x-end", "y-start", "y-end"];
    if p.peek()
        .is_some_and(|v| logical.iter().any(|k| v.is_ident(k)))
    {
        return Err(ParseError::Unexpected);
    }
    parse_position_axis(p, horizontal, false)
}

/// A box keyword of `mask-origin`, `mask-clip` or the shorthands.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BoxKeyword {
    /// A box (valid for the origin and the clip).
    Box(BackgroundBox),
    /// `no-clip` (standard syntax) or `text` (legacy syntax): only for the
    /// clip.
    ClipOnly(MaskClip),
}

/// A box keyword. Standard: `<coord-box> | no-clip`, where `fill-box` is
/// used as `content-box` and `stroke-box` and `view-box` as `border-box`
/// (CSS Masking 1 §7.5). Legacy: `border | padding | content` and the
/// three `*-box` keywords, and `text` (clipped to the border box: swb
/// does not clip to text).
fn parse_box_keyword(p: &mut Parser<'_>, syntax: Syntax) -> ParseResult<BoxKeyword> {
    use BackgroundBox as B;
    keyword(p, |ident| {
        let ident = ident.to_ascii_lowercase();
        Some(match (ident.as_str(), syntax) {
            ("border-box", _)
            | ("stroke-box" | "view-box", Syntax::Standard)
            | ("border", Syntax::Legacy) => BoxKeyword::Box(B::BorderBox),
            ("padding-box", _) | ("padding", Syntax::Legacy) => BoxKeyword::Box(B::PaddingBox),
            ("content-box", _) | ("fill-box", Syntax::Standard) | ("content", Syntax::Legacy) => {
                BoxKeyword::Box(B::ContentBox)
            }
            ("no-clip", Syntax::Standard) => BoxKeyword::ClipOnly(MaskClip::NoClip),
            ("text", Syntax::Legacy) => BoxKeyword::ClipOnly(MaskClip::BorderBox),
            _ => return None,
        })
    })
}

/// `mask-origin` and `-webkit-mask-origin`.
/// <https://www.w3.org/TR/css-masking-1/#the-mask-origin>
fn parse_origin(p: &mut Parser<'_>, syntax: Syntax) -> ParseResult<BackgroundBox> {
    p.try_parse(|p| match parse_box_keyword(p, syntax)? {
        BoxKeyword::Box(b) => Ok(b),
        BoxKeyword::ClipOnly(_) => Err(ParseError::Unexpected),
    })
}

/// `mask-clip` and `-webkit-mask-clip`.
fn parse_clip(p: &mut Parser<'_>, syntax: Syntax) -> ParseResult<MaskClip> {
    Ok(match parse_box_keyword(p, syntax)? {
        BoxKeyword::Box(b) => clip_of(b),
        BoxKeyword::ClipOnly(clip) => clip,
    })
}

fn clip_of(b: BackgroundBox) -> MaskClip {
    match b {
        BackgroundBox::BorderBox => MaskClip::BorderBox,
        BackgroundBox::PaddingBox => MaskClip::PaddingBox,
        BackgroundBox::ContentBox => MaskClip::ContentBox,
    }
}

/// `<compositing-operator>` (`add | subtract | intersect | exclude`), or
/// the operators of `-webkit-mask-composite`.
fn parse_composite(p: &mut Parser<'_>, syntax: Syntax) -> ParseResult<CompositeOperator> {
    use CompositeOperator as C;
    match syntax {
        Syntax::Standard => p.expect_one_of(&[
            ("add", C::SourceOver),
            ("subtract", C::SourceOut),
            ("intersect", C::SourceIn),
            ("exclude", C::Xor),
        ]),
        Syntax::Legacy => p.expect_one_of(&[
            ("source-over", C::SourceOver),
            ("source-out", C::SourceOut),
            ("source-in", C::SourceIn),
            ("xor", C::Xor),
            ("clear", C::Clear),
            ("copy", C::Copy),
            ("source-atop", C::SourceAtop),
            ("destination-over", C::DestinationOver),
            ("destination-in", C::DestinationIn),
            ("destination-out", C::DestinationOut),
            ("destination-atop", C::DestinationAtop),
            ("plus-lighter", C::PlusLighter),
        ]),
    }
}

/// One layer of the `mask` shorthand.
#[derive(Default)]
struct MaskLayer {
    image: Option<SpecifiedMaskImage>,
    position: Option<(SpecifiedPosition, SpecifiedPosition)>,
    size: Option<SpecifiedBackgroundSize>,
    repeat: Option<(BackgroundRepeatKeyword, BackgroundRepeatKeyword)>,
    /// Up to two box keywords, at most one of them clip-only.
    boxes: Vec<BoxKeyword>,
    composite: Option<CompositeOperator>,
    mode: Option<MaskMode>,
}

impl MaskLayer {
    /// The origin and the clip: one box sets both; two set the origin
    /// and the clip; a clip-only keyword sets the clip.
    fn origin_and_clip(&self) -> (BackgroundBox, MaskClip) {
        let origin = self.boxes.iter().find_map(|b| match b {
            BoxKeyword::Box(b) => Some(*b),
            BoxKeyword::ClipOnly(_) => None,
        });
        let clip_only = self.boxes.iter().find_map(|b| match b {
            BoxKeyword::ClipOnly(c) => Some(*c),
            BoxKeyword::Box(_) => None,
        });
        let clip = match (clip_only, self.boxes.as_slice()) {
            (Some(clip), _) => clip,
            (None, [_, BoxKeyword::Box(second)]) => clip_of(*second),
            (None, _) => clip_of(origin.unwrap_or(BackgroundBox::BorderBox)),
        };
        (origin.unwrap_or(BackgroundBox::BorderBox), clip)
    }
}

/// `mask` and `-webkit-mask`: `<mask-layer>#`, where `<mask-layer> =
/// <mask-reference> || <position> [ / <bg-size> ]? || <repeat-style> ||
/// <geometry-box> || [ <geometry-box> | no-clip ] ||
/// <compositing-operator> || <masking-mode>`. Like Chromium, the position
/// may have three values. Omitted parts get their initial values.
/// <https://www.w3.org/TR/css-masking-1/#the-mask>
fn parse_mask(
    p: &mut Parser<'_>,
    cx: &ParserContext,
    syntax: Syntax,
    out: &mut Vec<LonghandValue>,
) -> ParseResult<()> {
    let layers = p.parse_comma_separated(|p| parse_mask_layer(p, cx, syntax))?;
    log_if_too_many(layers.len());
    let mut images = Vec::with_capacity(layers.len());
    let mut xs = Vec::with_capacity(layers.len());
    let mut ys = Vec::with_capacity(layers.len());
    let mut sizes = Vec::with_capacity(layers.len());
    let mut repeats = Vec::with_capacity(layers.len());
    let mut origins = Vec::with_capacity(layers.len());
    let mut clips = Vec::with_capacity(layers.len());
    let mut composites = Vec::with_capacity(layers.len());
    let mut modes = Vec::with_capacity(layers.len());
    for layer in layers {
        let (origin, clip) = layer.origin_and_clip();
        images.push(layer.image.unwrap_or(SpecifiedMaskImage::None));
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
        origins.push(origin);
        clips.push(clip);
        composites.push(layer.composite.unwrap_or_default());
        modes.push(layer.mode.unwrap_or_default());
    }
    out.push(LonghandValue::MaskImage(Arc::from(images)));
    out.push(LonghandValue::MaskPositionX(Arc::from(xs)));
    out.push(LonghandValue::MaskPositionY(Arc::from(ys)));
    out.push(LonghandValue::MaskSize(Arc::from(sizes)));
    out.push(LonghandValue::MaskRepeat(Arc::from(repeats)));
    out.push(LonghandValue::MaskOrigin(Arc::from(origins)));
    out.push(LonghandValue::MaskClip(Arc::from(clips)));
    out.push(LonghandValue::MaskComposite(Arc::from(composites)));
    out.push(LonghandValue::MaskMode(Arc::from(modes)));
    Ok(())
}

fn parse_mask_layer(
    p: &mut Parser<'_>,
    cx: &ParserContext,
    syntax: Syntax,
) -> ParseResult<MaskLayer> {
    let mut layer = MaskLayer::default();
    // A layer must have at least one component.
    if p.is_exhausted() {
        return Err(ParseError::EndOfInput);
    }
    while !p.is_exhausted() {
        let next = p.peek();
        if layer.image.is_none()
            && (next.is_some_and(looks_like_image) || next.is_some_and(|v| v.is_ident("none")))
        {
            layer.image = Some(parse_mask_image(p, cx)?);
        } else if layer.position.is_none()
            && let Ok(position) = parse_bg_position(p, false)
        {
            layer.position = Some(position);
            if p.expect_delim('/').is_ok() {
                layer.size = Some(parse_background_size(p)?);
            }
        } else if layer.repeat.is_none()
            && let Ok(repeat) = parse_background_repeat(p)
        {
            layer.repeat = Some(repeat);
        } else if let Ok(b) = p.try_parse(|p| next_box(p, syntax, &layer.boxes)) {
            layer.boxes.push(b);
        } else if layer.composite.is_none()
            && let Ok(composite) = parse_composite(p, Syntax::Standard)
        {
            layer.composite = Some(composite);
        } else if layer.mode.is_none()
            && let Ok(mode) = parse_mask_mode(p)
        {
            layer.mode = Some(mode);
        } else {
            return Err(ParseError::Unexpected);
        }
    }
    Ok(layer)
}

/// Consumes a box keyword if the layer has room for it: at most two box
/// keywords, at most one of them clip-only.
fn next_box(p: &mut Parser<'_>, syntax: Syntax, boxes: &[BoxKeyword]) -> ParseResult<BoxKeyword> {
    if boxes.len() >= 2 {
        return Err(ParseError::Unexpected);
    }
    let b = parse_box_keyword(p, syntax)?;
    let has_clip_only = boxes.iter().any(|b| matches!(b, BoxKeyword::ClipOnly(_)));
    if matches!(b, BoxKeyword::ClipOnly(_)) && has_clip_only {
        return Err(ParseError::Invalid);
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use swb_css::parse_component_values;

    use super::super::{PropertyDeclaration, is_supported, parse_declaration};
    use super::*;
    use crate::values::{BackgroundSize, Image, LengthPercentage, LengthPercentageOrAuto};

    /// The longhand values of one declaration.
    fn parse(name: &str, value: &str) -> Option<Vec<LonghandValue>> {
        let values = parse_component_values(value);
        let mut out = Vec::new();
        let cx = ParserContext::author(None, false);
        if !parse_declaration(name, &values, &cx, &mut out) {
            return None;
        }
        Some(
            out.into_iter()
                .filter_map(|d| match d {
                    PropertyDeclaration::Value(v) => Some(v),
                    _ => None,
                })
                .collect(),
        )
    }

    fn supports(name: &str, value: &str) -> bool {
        let declaration = swb_css::Declaration {
            name: name.into(),
            value: parse_component_values(value),
            important: false,
        };
        is_supported(&declaration)
    }

    /// `CSS.supports()` answers of Chromium 148 for the mask properties.
    const SUPPORTS: &[(&str, &str, bool)] = &[
        ("mask-image", "none", true),
        ("mask-image", "url(a.png), none", true),
        ("mask-image", "linear-gradient(red, blue)", true),
        ("mask-image", "radial-gradient(red, blue)", true),
        ("mask-image", "conic-gradient(red, blue)", true),
        ("mask-image", "url(#m)", true),
        ("mask-image", "url()", true),
        ("mask-image", "image-set(\"a.png\" 1x)", true),
        ("mask-image", "element(#a)", false),
        ("mask-image", "paint(foo)", false),
        (
            "mask-image",
            "cross-fade(url(a.png), url(b.png), 50%)",
            false,
        ),
        ("mask-image", "url(a.png) url(b.png)", false),
        ("-webkit-mask-image", "none", true),
        ("mask-mode", "alpha, luminance", true),
        ("mask-mode", "match-source", true),
        ("mask-mode", "auto", false),
        ("-webkit-mask-mode", "alpha", false),
        ("mask-repeat", "repeat no-repeat, repeat-y", true),
        ("mask-repeat", "repeat-x repeat", false),
        ("-webkit-mask-repeat", "space round", true),
        ("-webkit-mask-repeat-x", "repeat", false),
        ("mask-position", "center", true),
        ("mask-position", "left 10px top 5px", true),
        ("mask-position", "left 10px top", false),
        ("mask-position", "x-start", false),
        ("mask-position-x", "10px", false),
        ("-webkit-mask-position", "left 10px top", true),
        ("-webkit-mask-position-x", "right 10px", true),
        ("-webkit-mask-position-x", "10px, 20px", true),
        ("-webkit-mask-position-x", "x-start", false),
        ("-webkit-mask-position-x", "top", false),
        ("-webkit-mask-position-x", "center 10px", false),
        ("-webkit-mask-position-y", "bottom 3px", true),
        ("-webkit-mask-position-y", "left", false),
        ("mask-size", "calc(max(1rem, 10px))", true),
        ("mask-size", "10px, contain", true),
        ("mask-size", "-1px", false),
        ("mask-size", "10px 20px 30px", false),
        ("-webkit-mask-size", "contain", true),
        ("mask-origin", "fill-box", true),
        ("mask-origin", "view-box", true),
        ("mask-origin", "border", false),
        ("mask-origin", "no-clip", false),
        ("mask-origin", "margin-box", false),
        ("-webkit-mask-origin", "padding", true),
        ("-webkit-mask-origin", "content-box", true),
        ("-webkit-mask-origin", "fill-box", false),
        ("-webkit-mask-origin", "text", false),
        ("mask-clip", "no-clip", true),
        ("mask-clip", "stroke-box", true),
        ("mask-clip", "text", false),
        ("mask-clip", "content", false),
        ("-webkit-mask-clip", "border, text", true),
        ("-webkit-mask-clip", "padding-box", true),
        ("-webkit-mask-clip", "no-clip", false),
        ("-webkit-mask-clip", "fill-box", false),
        ("mask-composite", "add, exclude", true),
        ("mask-composite", "ADD", true),
        ("mask-composite", "xor", false),
        ("mask-composite", "source-over", false),
        ("-webkit-mask-composite", "source-over, xor", true),
        ("-webkit-mask-composite", "destination-atop", true),
        ("-webkit-mask-composite", "plus-lighter", true),
        ("-webkit-mask-composite", "add", false),
        ("-webkit-mask-composite", "plus-darker", false),
        ("mask-type", "alpha", true),
        ("mask-type", "luminance", true),
        ("mask-type", "match-source", false),
        ("mask-type", "alpha, luminance", false),
        ("mask-border", "url(a.png) 30 repeat", false),
        ("mask", "none", true),
        ("mask", "none, none", true),
        ("mask", "url(a.png) center / 10px no-repeat", true),
        (
            "mask",
            "url(a.png) repeat-x 10px 10px / 5px auto luminance subtract",
            true,
        ),
        ("mask", "luminance url(a.png)", true),
        ("mask", "url(a.png) padding-box content-box", true),
        ("mask", "url(a.png) no-clip padding-box", true),
        ("mask", "url(a.png) border-box no-clip", true),
        ("mask", "url(a.png) no-clip no-clip", false),
        ("mask", "url(a.png) padding-box content-box no-clip", false),
        ("mask", "url(a.png) view-box stroke-box", true),
        ("mask", "url(a.png) left 10px top / 10px", true),
        ("mask", "url(a.png) 10px 20px 30px", false),
        ("mask", "url(a.png) alpha alpha", false),
        ("mask", "url(a.png) add subtract", false),
        ("mask", "url(a.png) / 10px", false),
        ("mask", "10px / 5px url(a.png)", true),
        ("mask", "url(a.png),", false),
        ("mask", ", url(a.png)", false),
        ("mask", "url(a.png) url(b.png)", false),
        ("mask", "url(a.png) repeat-x repeat-y", false),
        ("mask", "border-box", true),
        ("mask", "add", true),
        ("mask", "url(a.png) border", false),
        ("mask", "url(a.png) xor", false),
        ("mask", "url(a.png) text", false),
        ("mask", "red", false),
        ("-webkit-mask", "url(a.png) padding content", true),
        ("-webkit-mask", "url(a.png) text padding", true),
        ("-webkit-mask", "url(a.png) text text", false),
        (
            "-webkit-mask",
            "url(a.png) padding-box border-box content-box",
            false,
        ),
        ("-webkit-mask", "url(a.png) luminance subtract", true),
        ("-webkit-mask", "url(a.png) add", true),
        ("-webkit-mask", "url(a.png) xor", false),
        ("-webkit-mask", "url(a.png) fill-box", false),
        ("-webkit-mask", "url(a.png) no-clip", false),
    ];

    #[test]
    fn supports_like_chromium() {
        for &(name, value, expected) in SUPPORTS {
            assert_eq!(supports(name, value), expected, "{name}: {value}");
        }
    }

    #[test]
    fn aliases_set_the_longhands() {
        let v = parse("-webkit-mask-image", "url(a.png)").unwrap();
        assert!(matches!(&v[..], [LonghandValue::MaskImage(_)]));
        let v = parse("-webkit-mask-position", "center").unwrap();
        assert!(matches!(
            &v[..],
            [
                LonghandValue::MaskPositionX(_),
                LonghandValue::MaskPositionY(_)
            ]
        ));
        let v = parse("-webkit-mask-composite", "xor").unwrap();
        assert_eq!(
            v,
            [LonghandValue::MaskComposite(Arc::from([
                CompositeOperator::Xor
            ]))]
        );
        let v = parse("-webkit-mask-clip", "content, text").unwrap();
        assert_eq!(
            v,
            [LonghandValue::MaskClip(Arc::from([
                MaskClip::ContentBox,
                MaskClip::BorderBox
            ]))]
        );
    }

    #[test]
    fn mask_images() {
        let v = parse(
            "mask-image",
            "none, url(#m), url(''), radial-gradient(red, blue)",
        )
        .unwrap();
        assert_eq!(
            v,
            [LonghandValue::MaskImage(Arc::from([
                SpecifiedMaskImage::None,
                SpecifiedMaskImage::Failed,
                SpecifiedMaskImage::Failed,
                SpecifiedMaskImage::Unsupported,
            ]))]
        );
        let v = parse("mask-image", "url(a.svg)").unwrap();
        let [LonghandValue::MaskImage(images)] = &v[..] else {
            panic!("{v:?}");
        };
        assert_eq!(
            images[0].compute(&LengthContext::DEFAULT),
            MaskImage::Image(Image::Url("a.svg".into()))
        );
    }

    /// The shorthand's value for each longhand.
    fn shorthand(value: &str) -> Vec<LonghandValue> {
        parse("mask", value).unwrap_or_else(|| panic!("invalid: {value}"))
    }

    fn origin_and_clip(value: &str, syntax: &str) -> (BackgroundBox, MaskClip) {
        let v = parse(syntax, value).unwrap_or_else(|| panic!("invalid: {value}"));
        let origin = v.iter().find_map(|v| match v {
            LonghandValue::MaskOrigin(o) => Some(o[0]),
            _ => None,
        });
        let clip = v.iter().find_map(|v| match v {
            LonghandValue::MaskClip(c) => Some(c[0]),
            _ => None,
        });
        (origin.unwrap(), clip.unwrap())
    }

    #[test]
    fn shorthand_boxes() {
        use BackgroundBox as B;
        let cases = [
            ("url(a.png)", "mask", (B::BorderBox, MaskClip::BorderBox)),
            (
                "url(a.png) padding-box",
                "mask",
                (B::PaddingBox, MaskClip::PaddingBox),
            ),
            (
                "url(a.png) padding-box content-box",
                "mask",
                (B::PaddingBox, MaskClip::ContentBox),
            ),
            (
                "url(a.png) no-clip",
                "mask",
                (B::BorderBox, MaskClip::NoClip),
            ),
            (
                "url(a.png) no-clip padding-box",
                "mask",
                (B::PaddingBox, MaskClip::NoClip),
            ),
            (
                "url(a.png) fill-box",
                "mask",
                (B::ContentBox, MaskClip::ContentBox),
            ),
            (
                "url(a.png) text padding",
                "-webkit-mask",
                (B::PaddingBox, MaskClip::BorderBox),
            ),
            (
                "url(a.png) content",
                "-webkit-mask",
                (B::ContentBox, MaskClip::ContentBox),
            ),
        ];
        for (value, name, expected) in cases {
            assert_eq!(origin_and_clip(value, name), expected, "{name}: {value}");
        }
    }

    #[test]
    fn shorthand_resets_all_longhands() {
        let v = shorthand("url(a.png) right 3px bottom / 10px auto no-repeat luminance subtract");
        assert_eq!(v.len(), ShorthandId::Mask.longhands().len());
        for value in &v {
            match value {
                LonghandValue::MaskImage(i) => {
                    assert!(matches!(&i[..], [SpecifiedMaskImage::Image(_)]));
                }
                LonghandValue::MaskPositionX(x) => assert!(x[0].from_end),
                LonghandValue::MaskPositionY(y) => {
                    assert_eq!(y[0], SpecifiedPosition::percent(1.0));
                }
                LonghandValue::MaskSize(s) => {
                    let cx = ComputeContext {
                        parent: &ComputedStyle::initial(),
                        lengths: LengthContext::DEFAULT,
                        element: None,
                        quirks: false,
                    };
                    assert_eq!(
                        s[0].compute(&cx),
                        BackgroundSize::Explicit(
                            LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(10.0)),
                            LengthPercentageOrAuto::Auto
                        )
                    );
                }
                LonghandValue::MaskRepeat(r) => assert_eq!(
                    r[0],
                    (
                        BackgroundRepeatKeyword::NoRepeat,
                        BackgroundRepeatKeyword::NoRepeat
                    )
                ),
                LonghandValue::MaskMode(m) => assert_eq!(&m[..], [MaskMode::Luminance]),
                LonghandValue::MaskComposite(c) => {
                    assert_eq!(&c[..], [CompositeOperator::SourceOut]);
                }
                LonghandValue::MaskOrigin(_) | LonghandValue::MaskClip(_) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        // Defaults for omitted parts.
        let v = shorthand("add");
        assert!(v.contains(&LonghandValue::MaskImage(Arc::from([
            SpecifiedMaskImage::None
        ]))));
        assert!(v.contains(&LonghandValue::MaskMode(Arc::from([MaskMode::MatchSource]))));
    }
}
