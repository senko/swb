//! CSS masks (CSS Masking 1 §7,
//! <https://www.w3.org/TR/css-masking-1/#positioned-masks>).
//!
//! A masked box paints itself and its descendants into a group
//! ([`crate::DisplayItem::PushMask`] to `PopMask`), like an opacity group.
//! [`layers`] computes the mask layers of the box for the display list:
//! each layer image is positioned, sized, tiled and clipped like a
//! background layer (`crate::background`), with `mask-origin` and
//! `mask-clip` instead of the background boxes. When the group closes, the
//! rasterizer renders each layer, takes its alpha or its luminance
//! ([`coverage`]), composites the layers from the bottom up
//! ([`composite`]) and multiplies the group's pixels by the result.
//!
//! Chromium is the reference where the specification leaves room
//! (measured with Chromium 148):
//!
//! - A layer whose image has not loaded, failed to load, or refers to an
//!   SVG `<mask>` element is transparent black, as the specification says
//!   for failed images (with one layer, the box is hidden).
//! - With `no-clip`, the mask painting area is the bounding box of the
//!   border box, the outline and the descendants that have their own layer
//!   in Chromium (positioned ones, and ones with opacity or a mask) with
//!   their overflow; in-flow overflow outside it stays hidden.
//! - A mask of the root element also masks the canvas background.
//! - Masks do not affect hit testing ([`crate::DisplayList::hit_test`]
//!   ignores them), and a masked box is not a containing block for
//!   positioned descendants.

use std::sync::Arc;

use swb_layout::{BoxFragment, NaturalSize, Point, Rect};
use swb_style::{
    BackgroundBox, BorderStyle, CompositeOperator, ComputedStyle, Image, LinearGradient,
    MAX_MASK_LAYERS, MaskClip, MaskImage, MaskMode, Rgba,
};

use crate::background;
use crate::display_list::{
    DisplayItem, ImageRef, ImageSizes, layer_value, outset, with_descendants,
};

/// One layer of a mask, in document coordinates (CSS px).
#[derive(Clone, Debug)]
pub struct MaskLayer {
    /// What the layer paints. Outside of it, the layer is transparent
    /// black.
    pub image: MaskLayerImage,
    /// True to use the luminance of the image (multiplied by its alpha)
    /// instead of its alpha.
    pub luminance: bool,
    /// How the layer combines with the layers below it (ignored for the
    /// bottom layer).
    pub composite: CompositeOperator,
}

/// The image of a mask layer.
#[derive(Clone, Debug)]
pub enum MaskLayerImage {
    /// Transparent black: `none`, or an image that is not available.
    Empty,
    /// Opaque in this area: an image that swb cannot draw, shown unmasked
    /// in the mask painting area.
    Opaque(Rect),
    /// An image, tiled as for backgrounds.
    Image {
        /// The image.
        image: ImageRef,
        /// The area the tiles cover, inside the mask painting area.
        area: Rect,
        /// The size and origin of one tile.
        tile: Rect,
    },
    /// A linear gradient; each tile contains the whole gradient.
    Gradient {
        /// The gradient.
        gradient: Arc<LinearGradient>,
        /// The current color for `currentColor` stops.
        current_color: Rgba,
        /// The area the tiles cover, inside the mask painting area.
        area: Rect,
        /// The size and origin of one tile.
        tile: Rect,
    },
}

impl MaskLayer {
    /// The area outside which the layer is transparent, or `None` if it is
    /// transparent everywhere.
    pub(crate) fn extent(&self) -> Option<Rect> {
        let area = match &self.image {
            MaskLayerImage::Empty => return None,
            MaskLayerImage::Opaque(area)
            | MaskLayerImage::Image { area, .. }
            | MaskLayerImage::Gradient { area, .. } => *area,
        };
        (area.width > 0.0 && area.height > 0.0).then_some(area)
    }
}

/// The boxes of a masked box that mask layers use, in document
/// coordinates.
pub(crate) struct MaskBoxes {
    border: Rect,
    padding: Rect,
    content: Rect,
    /// The mask painting area of `no-clip`: the border box, the outline
    /// and the descendants with their own layer.
    ink: Rect,
}

impl MaskBoxes {
    /// The boxes of `b`, whose parent's border box starts at `origin`;
    /// `layered` is the area of its descendants with their own layer (see
    /// [`needs_positioned_area`]).
    pub(crate) fn of(b: &BoxFragment, origin: Point, layered: Option<Rect>) -> Self {
        let border = b.border_rect.translate(origin);
        let style = &b.style;
        // An outline extends `width + offset` outside the border box;
        // Chromium's focus ring (`auto`) is drawn around the box and its
        // descendants, up to 3 px outside the offset.
        let outline = match style.outline_style.border_style() {
            None => Some(outset(
                with_descendants(b, origin),
                style.outline_offset + 3.0,
            )),
            Some(BorderStyle::None) => None,
            Some(_) if style.outline_width <= 0.0 => None,
            Some(_) => Some(outset(border, style.outline_width + style.outline_offset)),
        };
        MaskBoxes {
            border,
            padding: b.padding_rect().translate(origin),
            content: b.content_rect().translate(origin),
            ink: [outline, layered]
                .into_iter()
                .flatten()
                .fold(border, |ink, r| ink.union(&r)),
        }
    }

    fn positioning_area(&self, origin: BackgroundBox) -> Rect {
        match origin {
            BackgroundBox::BorderBox => self.border,
            BackgroundBox::PaddingBox => self.padding,
            BackgroundBox::ContentBox => self.content,
        }
    }

    fn painting_area(&self, clip: MaskClip) -> Rect {
        match clip {
            MaskClip::BorderBox => self.border,
            MaskClip::PaddingBox => self.padding,
            MaskClip::ContentBox => self.content,
            MaskClip::NoClip => self.ink,
        }
    }
}

/// True if the mask of a box with `style` needs the area of its positioned
/// descendants and nested groups: with `no-clip`, Chromium's mask painting
/// area is the bounding box of the border box, the outline and the
/// descendants with their own layer (positioned, opacity, mask), with
/// their own overflow.
pub(crate) fn needs_positioned_area(style: &ComputedStyle) -> bool {
    style.has_mask() && style.mask_clip.contains(&MaskClip::NoClip)
}

/// The ink of `items` inside the clips that they open themselves (a
/// `PopClip` without a `PushClip` in `items` is ignored): the area that a
/// descendant with its own layer draws, for the `no-clip` area. Chromium
/// clips that area by the descendant's own overflow clip, not by those of
/// the boxes around it.
pub(crate) fn clipped_ink(items: &[DisplayItem]) -> Option<Rect> {
    // The open clips; `None` for a clip that hides everything.
    let mut clips: Vec<Option<Rect>> = Vec::new();
    let mut area: Option<Rect> = None;
    for item in items {
        match item {
            DisplayItem::PushClip(rect) => {
                let clip = match clips.last() {
                    None => Some(*rect),
                    Some(outer) => outer.and_then(|c| c.intersection(rect)),
                };
                clips.push(clip);
            }
            DisplayItem::PopClip => {
                clips.pop();
            }
            _ => {
                let ink = item.ink_bounds().and_then(|b| match clips.last() {
                    None => Some(b),
                    Some(clip) => clip.and_then(|c| b.intersection(&c)),
                });
                if let Some(ink) = ink {
                    area = Some(area.map_or(ink, |a| a.union(&ink)));
                }
            }
        }
    }
    area
}

/// The area of the opacity and mask groups in `items` that are not inside
/// another group of `items`: the [`clipped_ink`] of each group, inside the
/// bounds of a mask group.
pub(crate) fn group_area(items: &[DisplayItem]) -> Option<Rect> {
    let mut area: Option<Rect> = None;
    let mut rest = items;
    while let Some(start) = rest.iter().position(is_group_start) {
        let group = rest.get(start..).unwrap_or_default();
        // The matching end of the group (or the end of the items).
        let mut depth = 0usize;
        let end = group
            .iter()
            .position(|item| {
                if is_group_start(item) {
                    depth += 1;
                } else if matches!(item, DisplayItem::PopOpacity | DisplayItem::PopMask) {
                    depth = depth.saturating_sub(1);
                }
                depth == 0
            })
            .unwrap_or(group.len());
        let inner = group.get(1..end).unwrap_or_default();
        let ink = clipped_ink(inner).and_then(|ink| match group.first() {
            Some(DisplayItem::PushMask { bounds, .. }) => ink.intersection(bounds),
            _ => Some(ink),
        });
        if let Some(ink) = ink {
            area = Some(area.map_or(ink, |a| a.union(&ink)));
        }
        rest = group.get(end + 1..).unwrap_or_default();
    }
    area
}

fn is_group_start(item: &DisplayItem) -> bool {
    matches!(
        item,
        DisplayItem::PushOpacity { .. } | DisplayItem::PushMask { .. }
    )
}

/// The mask layers of a box with `style`, top first. The style keeps at
/// most [`MAX_MASK_LAYERS`] layers.
pub(crate) fn layers(
    style: &ComputedStyle,
    boxes: &MaskBoxes,
    images: &dyn ImageSizes,
) -> Vec<MaskLayer> {
    style
        .mask_image
        .iter()
        .take(MAX_MASK_LAYERS)
        .enumerate()
        .map(|(i, image)| MaskLayer {
            image: layer_image(style, i, image, boxes, images),
            luminance: layer_value(&style.mask_mode, i) == Some(&MaskMode::Luminance),
            composite: layer_value(&style.mask_composite, i)
                .copied()
                .unwrap_or_default(),
        })
        .collect()
}

/// The image of layer `i`, positioned and tiled.
fn layer_image(
    style: &ComputedStyle,
    i: usize,
    image: &MaskImage,
    boxes: &MaskBoxes,
    images: &dyn ImageSizes,
) -> MaskLayerImage {
    let (Some(&origin), Some(&clip), Some(size), Some(position_x), Some(position_y), Some(&repeat)) = (
        layer_value(&style.mask_origin, i),
        layer_value(&style.mask_clip, i),
        layer_value(&style.mask_size, i),
        layer_value(&style.mask_position_x, i),
        layer_value(&style.mask_position_y, i),
        layer_value(&style.mask_repeat, i),
    ) else {
        return MaskLayerImage::Empty;
    };
    let positioning = boxes.positioning_area(origin);
    let painting = boxes.painting_area(clip);
    let layer = background::Layer {
        size,
        position_x,
        position_y,
        repeat,
    };
    match image {
        MaskImage::None | MaskImage::Failed => MaskLayerImage::Empty,
        MaskImage::Unsupported => MaskLayerImage::Opaque(painting),
        MaskImage::Image(Image::Url(url)) => {
            let image = ImageRef::Url(Arc::clone(url));
            // Not loaded (yet, or ever): transparent black.
            let Some(natural) = images.size(&image) else {
                return MaskLayerImage::Empty;
            };
            let (tile, area) = background::tile(&layer, positioning, painting, &natural);
            MaskLayerImage::Image { image, area, tile }
        }
        MaskImage::Image(Image::LinearGradient(gradient)) => {
            // A gradient has no natural size (CSS Images 3 §3.1).
            let (tile, area) =
                background::tile(&layer, positioning, painting, &NaturalSize::default());
            MaskLayerImage::Gradient {
                gradient: Arc::clone(gradient),
                current_color: style.color,
                area,
                tile,
            }
        }
    }
}

/// The mask values (0 to 255) of a rendered layer: the alpha of each
/// pixel, or its luminance times its alpha. With premultiplied colors, the
/// latter is the luminance of the premultiplied values. The coefficients
/// are those of `feColorMatrix`'s `luminanceToAlpha`, applied to sRGB
/// values as Chromium does (CSS Masking 1 §7.10.1,
/// <https://www.w3.org/TR/css-masking-1/#MaskValues>).
pub(crate) fn coverage(premultiplied_rgba: &[u8], luminance: bool) -> Vec<u8> {
    premultiplied_rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, a]| {
            if luminance {
                let sum = 2125 * u32::from(r) + 7154 * u32::from(g) + 721 * u32::from(b);
                ((sum + 5000) / 10_000).min(255) as u8
            } else {
                a
            }
        })
        .collect()
}

/// Composites a layer (`source`, `None` for transparent black) onto the
/// result of the layers below it (`destination`).
pub(crate) fn composite(op: CompositeOperator, source: Option<&[u8]>, destination: &mut [u8]) {
    match source {
        Some(source) => {
            for (d, &s) in destination.iter_mut().zip(source) {
                *d = composite_value(op, s, *d);
            }
        }
        None => {
            for d in destination.iter_mut() {
                *d = composite_value(op, 0, *d);
            }
        }
    }
}

/// Composites one coverage value (0 to 255) of a layer (`source`) onto
/// the value of the layers below it (`destination`) with a Porter-Duff
/// operator. For coverage alone, `source-atop` keeps the destination and
/// `destination-atop` gives the source.
fn composite_value(op: CompositeOperator, source: u8, destination: u8) -> u8 {
    use CompositeOperator as C;
    let (s, d) = (u32::from(source), u32::from(destination));
    let mul = |a: u32, b: u32| (a * b + 127) / 255;
    let value = match op {
        C::SourceOver | C::DestinationOver => s + mul(d, 255 - s),
        C::SourceOut => mul(s, 255 - d),
        C::SourceIn | C::DestinationIn => mul(s, d),
        C::Xor => mul(s, 255 - d) + mul(d, 255 - s),
        C::Clear => 0,
        C::Copy | C::DestinationAtop => s,
        C::SourceAtop => d,
        C::DestinationOut => mul(d, 255 - s),
        C::PlusLighter => s + d,
    };
    value.min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luminance_and_alpha() {
        // Opaque mid grey, half-transparent black, opaque white.
        let pixels = [128, 128, 128, 255, 0, 0, 0, 128, 255, 255, 255, 255];
        assert_eq!(coverage(&pixels, false), [255, 128, 255]);
        assert_eq!(coverage(&pixels, true), [128, 0, 255]);
    }

    #[test]
    fn operators_on_coverage() {
        use CompositeOperator as Op;
        // (operator, [s=255 d=255, s=255 d=0, s=0 d=255, s=128 d=128])
        let cases = [
            (Op::SourceOver, [255, 255, 255, 192]),
            (Op::SourceOut, [0, 255, 0, 64]),
            (Op::SourceIn, [255, 0, 0, 64]),
            (Op::Xor, [0, 255, 255, 128]),
            (Op::Clear, [0, 0, 0, 0]),
            (Op::Copy, [255, 255, 0, 128]),
            (Op::SourceAtop, [255, 0, 255, 128]),
            (Op::DestinationOut, [0, 0, 255, 64]),
            (Op::PlusLighter, [255, 255, 255, 255]),
        ];
        for (op, expected) in cases {
            let actual = [(255, 255), (255, 0), (0, 255), (128, 128)]
                .map(|(s, d)| composite_value(op, s, d));
            assert_eq!(actual, expected, "{op:?}");
        }
    }

    #[test]
    fn transparent_source() {
        let mut d = [255, 100, 0];
        composite(CompositeOperator::SourceIn, None, &mut d);
        assert_eq!(d, [0, 0, 0]);
        let mut d = [255, 100, 0];
        composite(CompositeOperator::SourceOver, None, &mut d);
        assert_eq!(d, [255, 100, 0]);
        let mut d = [255, 100, 0];
        composite(CompositeOperator::Xor, Some(&[255, 255, 255]), &mut d);
        assert_eq!(d, [0, 155, 255]);
    }
}
