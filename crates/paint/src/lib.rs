//! Display list construction, rasterization and image decoding.
//!
//! [`build_display_list`] turns a fragment tree into drawing commands;
//! [`rasterize`] draws them into a pixmap with tiny-skia, a tall target
//! (a full-page screenshot) in strips ([`rasterize_in_strips`]). The display list
//! does not depend on tiny-skia, so another backend can replace the
//! rasterizer. Images are raster images or SVG images (`svg`), which are
//! rendered at the size they are drawn at. Masks (`mask`) multiply a
//! group of drawing commands by images or gradients. Form controls
//! (`control`) and media controls (`media`) have their own look.

mod background;
mod control;
mod display_list;
mod group_bounds;
mod hit_path;
mod image;
mod inline_svg;
mod mask;
mod media;
mod raster;
mod rope;
mod scroll_indicator;
mod svg;

pub use display_list::{
    DisplayItem, DisplayList, Highlights, ImageRef, ImageSizes, NoHighlights, Radii,
    SELECTION_BACKGROUND, Scrolling, build_display_list,
};
pub use image::{
    DecodedImage, ImageError, SVG_MIME_TYPE, decode, decode_with_type, is_supported_image_type,
};
pub use mask::{MaskLayer, MaskLayerImage};
pub use raster::{ImageSource, RasterParams, rasterize, rasterize_in_strips};
pub use scroll_indicator::scroll_indicators;
pub use svg::VectorCache;
pub use tiny_skia::Pixmap;

/// Fills the whole pixmap with one color.
pub fn fill(target: &mut Pixmap, color: swb_style::Rgba) {
    target.fill(tiny_skia::Color::from_rgba8(
        color.r, color.g, color.b, color.a,
    ));
}
