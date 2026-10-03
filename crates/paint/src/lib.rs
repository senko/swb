//! Display list construction, rasterization and image decoding.
//!
//! [`build_display_list`] turns a fragment tree into drawing commands;
//! [`rasterize`] draws them into a pixmap with tiny-skia. The display list
//! does not depend on tiny-skia, so another backend can replace the
//! rasterizer. Images are raster images or SVG images (`svg`), which are
//! rendered at the size they are drawn at.

mod background;
mod control;
mod display_list;
mod image;
mod raster;
mod svg;

pub use display_list::{
    DisplayItem, DisplayList, Highlights, ImageRef, ImageSizes, NoHighlights, Radii,
    SELECTION_BACKGROUND, SELECTION_TEXT, build_display_list,
};
pub use image::{DecodedImage, ImageError, SVG_MIME_TYPE, decode, decode_with_type};
pub use raster::{ImageSource, RasterParams, rasterize};
pub use svg::VectorCache;
pub use tiny_skia::Pixmap;

/// Fills the whole pixmap with one color.
pub fn fill(target: &mut Pixmap, color: swb_style::Rgba) {
    target.fill(tiny_skia::Color::from_rgba8(
        color.r, color.g, color.b, color.a,
    ));
}
