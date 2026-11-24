//! Display list construction, rasterization and image decoding.
//!
//! [`build_display_list`] turns a fragment tree into drawing commands;
//! [`rasterize`] draws them into a pixmap with tiny-skia, a tall target
//! (a full-page screenshot) in strips ([`rasterize_in_strips`]). The display list
//! does not depend on tiny-skia, so another backend can replace the
//! rasterizer. Images are raster images or SVG images (`svg`), which are
//! rendered at the size they are drawn at. Large raster images are drawn
//! from reduced levels (`reduce`). Masks (`mask`) multiply a group of
//! drawing commands by images or gradients. Form controls (`control`) and
//! media controls (`media`) have their own look. `group_bounds` finds the
//! bounds of the transform, opacity and mask groups of a finished list.
//!
//! Frames that change little (ADR 0044 section 18): `damage` compares the
//! display list of a frame with the last one and gives the device
//! rectangles that changed; [`rasterize_region`] draws only those, with the
//! pixels of a full raster. A [`RasterCache`] keeps the coverage of clips
//! and the layers of composited groups (the transform groups of running
//! `transform` and `opacity` animations) between rasters (ADR 0044 section
//! 19).
//!
//! Inline SVG (ADR 0023): `inline_svg` turns the layout of an `<svg>`
//! element into drawing commands, `hit_path` tests points against its
//! shapes, and `path_cost` is the cost model that bounds the
//! rasterization of paths (shared with the estimate for SVG images).

mod animation_bounds;
mod background;
mod box_shadow;
mod clip_path;
mod control;
mod corner_shape;
mod damage;
mod display_list;
mod filter;
mod group_bounds;
mod hit_path;
mod image;
mod inline_svg;
mod mask;
mod media;
mod path_cost;
mod raster;
mod reduce;
mod resize_handle;
mod rope;
mod scroll_indicator;
mod svg;

pub use animation_bounds::{Keys, animation_bounds};
pub use damage::{
    Damage, ListDamage, MAX_RECTS, MediaKeys, Side, backdrop_boxes, hold_backdrop_boxes,
    list_damage, media_keys, merge_damage,
};
pub use display_list::{
    DisplayItem, DisplayList, Highlights, Hit, ImageRef, ImageSizes, NoHighlights, Radii,
    SELECTION_BACKGROUND, Scrolling, build_display_list,
};
pub use group_bounds::transform_ends;
pub use hit_path::HitWork;
pub use image::{
    DecodedImage, ImageError, SVG_MIME_TYPE, decode, decode_in_document, decode_with_type,
    is_supported_image_type, raster_size,
};
pub use mask::{MaskLayer, MaskLayerImage};
pub use raster::{
    CacheStats, FrameContent, ImageSource, PixelRect, RasterCache, RasterParams, RasterReport,
    rasterize, rasterize_in_strips, rasterize_region,
};
pub use scroll_indicator::scroll_indicators;
pub use svg::{CountBudget, VectorCache};
pub use tiny_skia::Pixmap;

/// Fills the whole pixmap with one color.
pub fn fill(target: &mut Pixmap, color: swb_style::Rgba) {
    target.fill(tiny_skia::Color::from_rgba8(
        color.r, color.g, color.b, color.a,
    ));
}
