//! Blend groups (`mix-blend-mode`, CSS Compositing and Blending 1 §3.2,
//! <https://drafts.fxtf.org/compositing-1/#mix-blend-mode>): the tiny-skia
//! operator of a mode, the fast path of a group that holds one image, and
//! the blend of a group through the layers of rounded `overflow` clips (the
//! backdrop is the backdrop root's pixels, see `backdrop.rs`). The plain
//! source-over of a layer is in `composite.rs`.

use swb_layout::Rect;
use swb_style::MixBlendMode;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

use super::{ImageSource, Layer, Rasterizer};
use crate::display_list::DisplayItem;

/// The index of the image and of the closing `PopOpacity` of the group
/// that starts at `range.start`, if the group draws exactly one image:
/// hit items and clip pairs after the image draw nothing. `None` if the
/// group holds anything else, or ends outside `range`.
pub(super) fn single_image_group(
    items: &[DisplayItem],
    range: std::ops::Range<usize>,
) -> Option<(usize, usize)> {
    let mut image = None;
    let mut clips = 0usize;
    for at in range.start + 1..range.end {
        match items.get(at)? {
            DisplayItem::HitRegion { .. } | DisplayItem::HitShape { .. } => {}
            DisplayItem::Image { .. } if image.is_none() && clips == 0 => image = Some(at),
            // A clip after the image cuts nothing that is drawn.
            DisplayItem::PushClip(_) if image.is_some() => clips += 1,
            DisplayItem::PopClip if clips > 0 => clips -= 1,
            DisplayItem::PopOpacity if clips == 0 => return image.zip(Some(at)),
            _ => return None,
        }
    }
    None
}

/// The tiny-skia operator of a `mix-blend-mode` (`plus-lighter` is `Plus`).
pub(super) fn blend_mode(mode: MixBlendMode) -> tiny_skia::BlendMode {
    use MixBlendMode as M;
    use tiny_skia::BlendMode as B;
    match mode {
        M::Normal => B::SourceOver,
        M::Multiply => B::Multiply,
        M::Screen => B::Screen,
        M::Overlay => B::Overlay,
        M::Darken => B::Darken,
        M::Lighten => B::Lighten,
        M::ColorDodge => B::ColorDodge,
        M::ColorBurn => B::ColorBurn,
        M::HardLight => B::HardLight,
        M::SoftLight => B::SoftLight,
        M::Difference => B::Difference,
        M::Exclusion => B::Exclusion,
        M::Hue => B::Hue,
        M::Saturation => B::Saturation,
        M::Color => B::Color,
        M::Luminosity => B::Luminosity,
        M::PlusLighter => B::Plus,
    }
}

/// True for a layer that holds pixels and is no direct layer: the layer of
/// a rounded `overflow` clip, which draws over the backdrop root.
fn is_clip_layer(layer: &Layer) -> bool {
    !layer.direct && layer.pixmap.is_some()
}

impl Rasterizer<'_> {
    /// Draws the blend group that starts at `range.start` without a layer
    /// if it holds one image: the image draws with the group's operator
    /// and opacity straight onto the surface. Returns the index after the
    /// group, or `None` if the group is not of this kind (the general path
    /// draws it).
    ///
    /// The result equals the layer's: the layer starts transparent, so the
    /// image's pixels (scaled by the opacity) are what the group blends
    /// with the surface, and the clips are whole-pixel rectangles with a
    /// mask of coverage 0 or 1, so the clip cuts the same pixels before or
    /// after the blend. Only the layer's rounding of the image to 8 bits
    /// before the blend is gone. A rounded `overflow` clip layer above the
    /// backdrop root, and a group that holds fixed content, take the
    /// general path.
    pub(super) fn blend_image_group(
        &mut self,
        items: &[DisplayItem],
        range: std::ops::Range<usize>,
        images: &dyn ImageSource,
    ) -> Option<usize> {
        let DisplayItem::PushOpacity {
            opacity,
            blend,
            fixed_bounds: None,
            escapes_clips: false,
            ..
        } = items.get(range.start)?
        else {
            return None;
        };
        if *blend == MixBlendMode::Normal || opacity.is_nan() || *opacity <= 0.0 {
            return None;
        }
        let (image_at, pop_at) = single_image_group(items, range)?;
        if self.surface_rect().is_none() || self.has_clip_layer_above_root() {
            return None;
        }
        let DisplayItem::Image {
            image,
            rect,
            tile,
            clip,
        } = items.get(image_at)?
        else {
            return None;
        };
        self.image_blend = Some((blend_mode(*blend), opacity.min(1.0)));
        if let Some(decoded) = images.image(image) {
            self.decoded_image(
                decoded,
                images.vector_cache(),
                (*rect, *tile, *clip),
                images.rounds_size(image),
            );
        }
        self.image_blend = None;
        Some(pop_at + 1)
    }

    /// True if a layer that is no backdrop root (the rounded `overflow`
    /// clip of an ancestor) is open above the backdrop root: a blend group
    /// then goes through [`Rasterizer::blend_through_clip_layers`].
    pub(super) fn has_clip_layer_above_root(&self) -> bool {
        self.backdrop_root(0)
            .is_some_and(|(_, above)| above.iter().any(is_clip_layer))
    }

    /// Composites the layer of a `mix-blend-mode` group (`group` at
    /// `origin` on the target) that is drawn into the layer of a rounded
    /// `overflow` clip: the clip layer is no isolated group in Chromium,
    /// so the group blends with the backdrop root's pixels with the clip
    /// layers drawn over them, and the result replaces the clip layer's
    /// pixels under the group (the clip layer then composites over the
    /// root with the same pixels). Returns false if no such layer is
    /// open: the group blends with its surface as it is.
    pub(super) fn blend_through_clip_layers(
        &mut self,
        group: &Pixmap,
        origin: (i32, i32),
        (opacity, mode): (f32, tiny_skia::BlendMode),
    ) -> bool {
        let Some((root, above)) = self.backdrop_root(0) else {
            return false;
        };
        if !above.iter().any(is_clip_layer) {
            return false;
        }
        let bounds = Rect::new(
            origin.0 as f32,
            origin.1 as f32,
            group.width() as f32,
            group.height() as f32,
        );
        let Some(area) = bounds.intersection(&root.rect) else {
            return true;
        };
        let Some(mut tmp) = Pixmap::new(area.width as u32, area.height as u32) else {
            return true;
        };
        self.read_backdrop(0, area, &mut tmp);
        let paint = PixmapPaint {
            opacity,
            blend_mode: mode,
            ..PixmapPaint::default()
        };
        tmp.draw_pixmap(
            origin.0 - area.x as i32,
            origin.1 - area.y as i32,
            group.as_ref(),
            &paint,
            Transform::identity(),
            None,
        );
        let Some(surface) = self.surface_rect() else {
            return true;
        };
        let replace = PixmapPaint {
            blend_mode: tiny_skia::BlendMode::Source,
            ..PixmapPaint::default()
        };
        if let Some(mut surface_pixels) = self.surface_mut() {
            surface_pixels.draw_pixmap(
                (area.x - surface.x) as i32,
                (area.y - surface.y) as i32,
                tmp.as_ref(),
                &replace,
                Transform::identity(),
                None,
            );
        }
        true
    }
}
