//! Display list construction, rasterization and image decoding.
//!
//! [`build_display_list`] turns a fragment tree into drawing commands;
//! [`rasterize`] draws them into a pixmap with tiny-skia. The display list
//! does not depend on tiny-skia, so another backend can replace the
//! rasterizer.

mod background;
mod display_list;
mod image;
mod raster;

pub use display_list::{
    DisplayItem, DisplayList, Highlights, ImageRef, ImageSizes, NoHighlights, Radii,
    SELECTION_BACKGROUND, SELECTION_TEXT, build_display_list,
};
pub use image::{DecodedImage, ImageError, decode};
pub use raster::{ImageSource, RasterParams, rasterize};
pub use tiny_skia::Pixmap;

/// Fills the whole pixmap with one color.
pub fn fill(target: &mut Pixmap, color: swb_style::Rgba) {
    target.fill(tiny_skia::Color::from_rgba8(
        color.r, color.g, color.b, color.a,
    ));
}
