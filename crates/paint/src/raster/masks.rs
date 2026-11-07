//! Mask groups in the rasterizer (CSS Masking 1 §7): the work and the budget of
//! a mask, and the multiplication of a group's layer by the mask layers. The
//! name differs from `crate::mask`, which has the mask layers themselves.

use swb_layout::Rect;

use super::{
    ImageSource, IntSize, Layer, LayerKind, LayerPixels, Mask, MaskLayer, MaskLayerImage, Pixmap,
    ROW_COST_PIXELS, Rasterizer, Rgba, mask,
};

impl Rasterizer<'_> {
    /// Multiplies the pixels of a mask group's layer (at `origin` on the
    /// target) by the mask. The bottom layer's operator is ignored (CSS
    /// Masking 1 §7.8). Returns false if the mask is transparent
    /// everywhere or its temporary layer does not fit into the layer
    /// budget: the group then draws nothing. (Its work was charged when
    /// the group started.)
    pub(super) fn apply_mask(
        &mut self,
        pixmap: &mut Pixmap,
        origin: (i32, i32),
        layers: &[MaskLayer],
        images: &dyn ImageSource,
    ) -> bool {
        let Some(size) = IntSize::from_wh(pixmap.width(), pixmap.height()) else {
            return false;
        };
        let pixels = u64::from(size.width()) * u64::from(size.height());
        // The temporary layer of each mask layer.
        if self.budget.layer_pixels + pixels > self.budget.max_layer_pixels {
            self.budget.skipped.layers = true;
            return false;
        }
        let mut result: Option<Vec<u8>> = None;
        for layer in layers.iter().rev() {
            let source = match self.mask_layer_pixels(layer, origin, size, images) {
                LayerPixels::Transparent => None,
                LayerPixels::Pixels(p) => Some(mask::coverage(p.data(), layer.luminance)),
                LayerPixels::OverBudget => return false,
            };
            result = Some(match result {
                None => source.unwrap_or_else(|| vec![0; pixmap.data().len() / 4]),
                Some(mut destination) => {
                    mask::composite(layer.composite, source.as_deref(), &mut destination);
                    destination
                }
            });
        }
        let Some(values) = result.filter(|v| v.iter().any(|&a| a != 0)) else {
            return false;
        };
        let Some(coverage) = Mask::from_vec(values, size) else {
            return false;
        };
        pixmap.apply_mask(&coverage);
        true
    }

    /// Renders a mask layer into a layer of `size` at `origin` (target
    /// device px).
    fn mask_layer_pixels(
        &mut self,
        layer: &MaskLayer,
        origin: (i32, i32),
        size: IntSize,
        images: &dyn ImageSource,
    ) -> LayerPixels {
        if matches!(layer.image, MaskLayerImage::Empty) {
            return LayerPixels::Transparent;
        }
        // Drawing goes to the innermost layer: push a temporary one.
        let Some(pixmap) = self.layer_pixmap(size.width(), size.height()) else {
            return LayerPixels::OverBudget;
        };
        self.layers.push(Layer {
            pixmap: Some(pixmap),
            direct: false,
            origin,
            opacity: 1.0,
            kind: LayerKind::Opacity,
            rect_clip: false,
        });
        self.mask = None;
        match &layer.image {
            MaskLayerImage::Empty => {}
            // White: opaque for alpha and for luminance.
            MaskLayerImage::Opaque(area) => {
                self.fill_rect(*area, &[(0.0, 0.0); 4], Rgba::WHITE);
            }
            MaskLayerImage::Image { image, area, tile } => {
                if let Some(decoded) = images.image(image) {
                    self.decoded_image(decoded, images.vector_cache(), *area, *tile, *area);
                }
            }
            MaskLayerImage::Gradient {
                gradient,
                current_color,
                area,
                tile,
            } => self.gradient_tiles(gradient, *current_color, *area, *tile),
        }
        self.mask = None;
        let Some(pixmap) = self.layers.pop().and_then(|l| l.pixmap) else {
            return LayerPixels::OverBudget;
        };
        self.release(&pixmap);
        LayerPixels::Pixels(pixmap)
    }

    /// Adds `pixels` to the mask work of this strip (see
    /// [`MAX_MASK_PIXELS`]).
    pub(super) fn add_mask_work(&mut self, pixels: u64) {
        self.budget.mask_pixels = self.budget.mask_pixels.saturating_add(pixels);
    }

    /// The work of a mask group whose layer is `area` (device px), for the
    /// mask budget: for each mask layer, the layer's pixels and rows, and
    /// for gradients the tiles that they render or draw (the same plan as
    /// `gradient_tiles`, which then charges nothing).
    pub(super) fn mask_work(&self, area: Rect, layers: &[MaskLayer]) -> u64 {
        let pixels = (area.width as u64).saturating_mul(area.height as u64);
        let rows = (area.height as u64).saturating_mul(ROW_COST_PIXELS);
        layers
            .iter()
            .map(|layer| {
                let tiles = match &layer.image {
                    MaskLayerImage::Gradient {
                        area: layer_area,
                        tile,
                        ..
                    } => area
                        .intersection(&self.to_device(*layer_area))
                        .map_or(0, |visible| {
                            self.tile_plan(*layer_area, *tile, visible).work()
                        }),
                    _ => 0,
                };
                pixels.saturating_add(rows).saturating_add(tiles)
            })
            .fold(0, u64::saturating_add)
    }

    /// True if `work` fits into the rest of the mask budget. A mask group
    /// whose work does not fit draws nothing.
    pub(super) fn mask_work_fits(&mut self, work: u64) -> bool {
        let fits = self.budget.mask_pixels.saturating_add(work) <= self.budget.max_mask_pixels;
        if !fits {
            self.budget.skipped.masks = true;
        }
        fits
    }
}
