//! SVG images: an `<img>` or a CSS image whose response has the type
//! `image/svg+xml` (ADR 0011).
//!
//! usvg converts the document into a render tree and resvg renders the
//! tree with tiny-skia. This module adds what a browser needs:
//!
//! - Limits, checked before usvg converts a document: the source size, XML
//!   entity expansion ([`entities`]), the number of XML nodes and their
//!   nesting depth, the work of style sheets ([`css`]), and the size and
//!   depth of the render tree with all the copies usvg makes
//!   ([`expansion`]). One budget of elements, path
//!   segments and embedded raster pixels covers an image and all the SVG
//!   images embedded in it.
//! - Limits on rendering: the estimated time and layer memory of one
//!   rendering ([`cost`]); a rendering above the budget is made at a lower
//!   resolution, and an image that cannot be rendered within it even at one
//!   pixel is not drawn. One frame renders new renderings up to its own
//!   budget ([`VectorCache`]). A panic in usvg or resvg (they have
//!   `unwrap`s on derived values) is caught, as defense in depth.
//! - No resource loading. An `<image>` element can only show a `data:` URL:
//!   a raster image within the limits of [`crate::decode`], or an SVG image
//!   (at most [`MAX_NESTING`] levels deep). All other references are
//!   ignored; usvg would read them from the file system.
//! - Natural dimensions from the root element's `width`, `height` and
//!   `viewBox` (<https://svgwg.org/svg2-draft/coords.html#SizingSVGInCSS>).
//! - Rendering for a concrete object size (the size the image is drawn at):
//!   the `viewBox` maps into it as `preserveAspectRatio` says. Without a
//!   `viewBox`, each axis that has a natural dimension is scaled to the
//!   concrete size, and the other axis is not scaled (measured with
//!   Chromium 148). The root viewport clips the content.
//!
//! Deliberate deviations: `<text>` is not drawn (no fonts, see ADR 0011);
//! only UTF-8 sources are accepted; percentages in an SVG image without
//! `viewBox` and with a percentage or missing size resolve against 300×150,
//! not against the concrete size; `ex` is half an `em`.

mod cache;
pub(crate) mod cost;
mod css;
mod entities;
mod expansion;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use svgtypes::{Align, LengthUnit};
use swb_layout::{DEFAULT_OBJECT_SIZE, NaturalSize};
use tiny_skia::{IntSize, NonZeroRect, Pixmap, Transform};

pub(crate) use cache::FrameBudget;
pub use cache::VectorCache;
pub use cost::CountBudget;

use crate::image::{ImageError, SVG_MIME_TYPE};

/// SVG sources larger than this are rejected.
const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;

/// The maximum number of XML nodes (elements, text, comments) in a source.
const MAX_NODES: u32 = 500_000;

/// The maximum nesting depth of elements in a source.
const MAX_DEPTH: usize = 256;

/// The maximum number of bytes that XML entity references may add.
const MAX_ENTITY_EXPANSION: usize = 1024 * 1024;

/// The limits of the conversion of one document (see [`expansion`] and
/// [`css`]). The effective depth of 1024 is usvg's own limit; it fits a
/// 2 MiB stack.
const CONVERSION_LIMITS: expansion::Limits = expansion::Limits {
    depth: 1024.0,
    css: css::Limits {
        parse: 5e8,
        declarations: 1e5,
        matches: 5e7,
    },
    link_scan: 5e7,
};

/// The budget of an image together with its embedded images.
const BUDGET: Budget = Budget {
    elements: 200_000.0,
    segments: 2_000_000.0,
    raster_pixels: 16.0 * 1024.0 * 1024.0,
};

/// The maximum number of levels of SVG images in `data:` URLs inside an SVG
/// image.
const MAX_NESTING: u32 = 4;

/// Renderings with more pixels than this are made at a lower resolution
/// and scaled up when drawn: 16 Mpx is 64 MiB as RGBA.
pub(crate) const MAX_RENDER_PIXELS: u32 = 16 * 1024 * 1024;

/// The work budget of one rendering (see [`cost`]): about 0.6 s.
const MAX_RENDER_WORK: f64 = 2e9;

/// The work budget of the new renderings of one frame: about 1.2 s. Images
/// beyond it are drawn from the closest cached rendering, or in a later
/// frame.
const MAX_FRAME_WORK: f64 = 2.0 * MAX_RENDER_WORK;

/// The budget for the layers of one rendering: 32 Mpx is 128 MiB.
const MAX_LAYER_PIXELS: f64 = 32.0 * 1024.0 * 1024.0;

/// The font size for `em` units: the initial value of `font-size`.
const FONT_SIZE: f64 = 16.0;

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// The source of [`SvgImage::id`].
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A decoded SVG image.
#[derive(Debug)]
pub(crate) struct SvgImage {
    /// The render tree. `None` if usvg found no valid size (a zero or
    /// negative `width` or `height`): the image then has natural dimensions
    /// but draws nothing.
    tree: Option<usvg::Tree>,
    natural: NaturalSize,
    view_box: Option<ViewBox>,
    /// The mapping that usvg applied to the content: the root's `viewBox`
    /// into the size usvg computed for the root. `None` without a
    /// `viewBox` or a tree.
    root_mapping: Option<Mapping>,
    /// The estimated cost of a rendering.
    cost: cost::Cost,
    /// Identifies the image in a [`VectorCache`].
    id: u64,
}

/// Parses SVG data, with a budget of its own for counting spans.
#[cfg(test)]
pub(crate) fn decode(data: &[u8]) -> Result<SvgImage, ImageError> {
    decode_counting(data, &CountBudget::default())
}

/// Parses SVG data. Counting the spans of its paths takes from `counting`.
pub(crate) fn decode_counting(data: &[u8], counting: &CountBudget) -> Result<SvgImage, ImageError> {
    let budget = Arc::new(Mutex::new(BUDGET));
    let converted = convert(data, 0, CONVERSION_LIMITS.depth, &budget)?;
    let tree = converted.tree;
    let cost = tree
        .as_ref()
        .map(|tree| cost::estimate(tree, counting))
        .unwrap_or_default();
    if cost.fixed > MAX_RENDER_WORK {
        return Err(ImageError::Svg("too expensive to render".into()));
    }
    let root_mapping = match (&tree, &converted.view_box) {
        (Some(tree), Some(view_box)) => Some(view_box.mapping(
            f64::from(tree.size().width()),
            f64::from(tree.size().height()),
        )),
        _ => None,
    };
    Ok(SvgImage {
        cost,
        tree,
        natural: converted.natural,
        view_box: converted.view_box,
        root_mapping,
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
    })
}

/// What an image and its embedded images may still use.
#[derive(Clone, Copy, Debug)]
struct Budget {
    /// Elements of the render trees, with copies and marker instances.
    elements: f64,
    /// Path segments of the render trees.
    segments: f64,
    /// Pixels of embedded raster images, with copies.
    raster_pixels: f64,
}

/// A budget shared by the conversion of an image and of the images
/// embedded in it (usvg converts an `<image>` again for every copy).
type SharedBudget = Arc<Mutex<Budget>>;

/// Takes `elements`, `segments` and `raster_pixels` from the budget, or
/// returns false (and takes nothing) if it does not have them.
fn take(budget: &SharedBudget, elements: f64, segments: f64, raster_pixels: f64) -> bool {
    let mut budget = budget.lock().unwrap_or_else(PoisonError::into_inner);
    let left = Budget {
        elements: budget.elements - elements,
        segments: budget.segments - segments,
        raster_pixels: budget.raster_pixels - raster_pixels,
    };
    let enough = left.elements >= 0.0 && left.segments >= 0.0 && left.raster_pixels >= 0.0;
    if enough {
        *budget = left;
    }
    enough
}

/// A document converted into a render tree.
struct Converted {
    /// `None` if usvg found no valid size (see [`SvgImage::tree`]).
    tree: Option<usvg::Tree>,
    natural: NaturalSize,
    view_box: Option<ViewBox>,
}

/// Parses and converts SVG data for an image at `nesting` levels of
/// nesting (0 for the image itself), with an effective depth of at most
/// `depth`, within the limits and the budget.
fn convert(
    data: &[u8],
    nesting: u32,
    depth: f64,
    budget: &SharedBudget,
) -> Result<Converted, ImageError> {
    let text = source_text(data)?;
    let xml = parse_xml(text)?;
    let root = xml.root_element();
    if !root.has_tag_name((SVG_NAMESPACE, "svg")) {
        return Err(ImageError::Svg("the root element is not <svg>".into()));
    }
    let limits = expansion::Limits {
        depth,
        ..CONVERSION_LIMITS
    };
    let usage = expansion::measure(&xml, &limits).map_err(|e| ImageError::Svg(e.into()))?;
    if !take(budget, usage.elements, usage.segments, 0.0) {
        return Err(ImageError::Svg("too many elements or path segments".into()));
    }
    let (natural, view_box) = natural_size(root);
    // Embedded images are converted inside this conversion, so their depth
    // adds to this document's.
    let options = options(nesting, depth - usage.depth, budget);
    let tree = match catch_unwind(AssertUnwindSafe(|| {
        usvg::Tree::from_xmltree(&xml, &options)
    })) {
        Ok(Ok(tree)) => Some(tree),
        Ok(Err(usvg::Error::InvalidSize)) => None,
        Ok(Err(e)) => return Err(ImageError::Svg(e.to_string())),
        Err(_) => return Err(ImageError::Svg("usvg panicked".into())),
    };
    Ok(Converted {
        tree,
        natural,
        view_box,
    })
}

impl SvgImage {
    /// The natural dimensions, from the root element.
    pub(crate) fn natural_size(&self) -> NaturalSize {
        self.natural
    }

    /// A number that identifies the image during the process's lifetime.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Renders the image for a concrete object size of `concrete` CSS px
    /// into a new pixmap of `size` pixels. If that would exceed the
    /// rendering budget, the pixmap is smaller (the drawing scales it up);
    /// if even one pixel would, the pixmap is one transparent pixel.
    pub(crate) fn render(&self, size: IntSize, concrete: (f32, f32)) -> Option<Pixmap> {
        let (Some(tree), Some((size, content))) = (&self.tree, self.plan(size, concrete)) else {
            return Pixmap::new(1, 1);
        };
        let mut pixmap = Pixmap::new(size.width(), size.height())?;
        let to_pixels = Transform::from_scale(
            size.width() as f32 / concrete.0,
            size.height() as f32 / concrete.1,
        );
        let transform = to_pixels.pre_concat(content);
        let rendered = catch_unwind(AssertUnwindSafe(|| {
            resvg::render(tree, transform, &mut pixmap.as_mut());
        }));
        if rendered.is_err() {
            log::warn!("SVG image: resvg panicked");
            return Pixmap::new(1, 1);
        }
        Some(pixmap)
    }

    /// The estimated work of `render(size, concrete)` (see [`cost`]).
    pub(crate) fn render_work(&self, size: IntSize, concrete: (f32, f32)) -> f64 {
        self.plan(size, concrete).map_or(0.0, |(size, content)| {
            let pixels = f64::from(size.width()) * f64::from(size.height());
            self.cost.work(pixels * self.overdraw(&content, concrete))
        })
    }

    /// The pixmap size that `render` uses (`size`, or smaller to stay
    /// within the budget) and the content transform; `None` if nothing is
    /// drawn.
    fn plan(&self, size: IntSize, concrete: (f32, f32)) -> Option<(IntSize, Transform)> {
        self.tree.as_ref()?;
        let content = self.content_transform(concrete)?;
        let size = self.affordable_size(size, self.overdraw(&content, concrete))?;
        Some((size, content))
    }

    /// How many pixels of the tree's canvas each pixel of the pixmap
    /// covers, at least 1. The cost estimate counts fractions of the
    /// canvas; the canvas is larger than the drawn box when the `viewBox`
    /// does not fit the root's size (`slice`, another aspect ratio) or an
    /// axis without a natural size is not scaled. The factor does not
    /// depend on the pixmap size.
    fn overdraw(&self, content: &Transform, concrete: (f32, f32)) -> f64 {
        let Some(tree) = &self.tree else {
            return 1.0;
        };
        let canvas = f64::from(tree.size().width()) * f64::from(tree.size().height());
        let determinant = (f64::from(content.sx) * f64::from(content.sy)
            - f64::from(content.kx) * f64::from(content.ky))
        .abs();
        let factor = canvas * determinant / (f64::from(concrete.0) * f64::from(concrete.1));
        if factor.is_nan() {
            f64::INFINITY
        } else {
            factor.max(1.0)
        }
    }

    /// `size`, reduced so that rendering it (with `overdraw` canvas pixels
    /// per pixel) stays within the rendering budget, keeping its
    /// proportions; `None` if not even one pixel does.
    fn affordable_size(&self, size: IntSize, overdraw: f64) -> Option<IntSize> {
        let pixels = f64::from(size.width()) * f64::from(size.height());
        let max_pixels = self
            .cost
            .max_pixels(MAX_RENDER_WORK, MAX_LAYER_PIXELS, pixels * overdraw)
            .map(|max| max / overdraw)
            .filter(|max| *max >= 1.0);
        let Some(max_pixels) = max_pixels else {
            log::warn!("SVG image: too expensive to render");
            return None;
        };
        if pixels > max_pixels {
            log::debug!("SVG image: rendering {size:?} in at most {max_pixels:.0} pixels");
        }
        fit_pixels(size, max_pixels)
    }

    /// The transform from the render tree to the concrete object box (CSS
    /// px), or `None` if the box is empty or the transform is not finite.
    fn content_transform(&self, (width, height): (f32, f32)) -> Option<Transform> {
        if !(width > 0.0 && height > 0.0) {
            return None;
        }
        if let Some(view_box) = &self.view_box {
            // The view box mapped into the box, after undoing usvg's
            // mapping into the root's size (both scale and translate only).
            let to_box = view_box.mapping(f64::from(width), f64::from(height));
            return to_box.after_inverse_of(self.root_mapping?);
        }
        let scale = |concrete: f32, natural: Option<f32>| {
            natural.filter(|n| *n > 0.0).map_or(1.0, |n| concrete / n)
        };
        Some(Transform::from_scale(
            scale(width, self.natural.width),
            scale(height, self.natural.height),
        ))
    }
}

/// The source as text: at most [`MAX_SOURCE_BYTES`], UTF-8, optionally with
/// a byte order mark.
fn source_text(data: &[u8]) -> Result<&str, ImageError> {
    if data.len() > MAX_SOURCE_BYTES {
        return Err(ImageError::Svg(format!(
            "larger than {MAX_SOURCE_BYTES} bytes"
        )));
    }
    // Chromium decompresses svgz only with `Content-Encoding: gzip`, which
    // the network layer handles.
    if data.starts_with(&[0x1f, 0x8b]) {
        return Err(ImageError::Svg("gzip data (svgz)".into()));
    }
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    std::str::from_utf8(data).map_err(|_| ImageError::Svg("not UTF-8".into()))
}

/// Parses XML within the limits on entity expansion, nodes and depth.
fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, ImageError> {
    if let Some(problem) = entities::check(text, MAX_ENTITY_EXPANSION) {
        return Err(ImageError::Svg(problem.into()));
    }
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        nodes_limit: MAX_NODES,
        ..roxmltree::ParsingOptions::default()
    };
    let xml = roxmltree::Document::parse_with_options(text, options)
        .map_err(|e| ImageError::Svg(e.to_string()))?;
    // The depth of each node, by node id (ids count the nodes in document
    // order, so parents come before children).
    let mut depths = vec![0_usize; xml.descendants().count()];
    for node in xml.descendants() {
        let parent = node
            .parent()
            .map_or(0, |p| depths.get(p.id().get_usize()).copied().unwrap_or(0));
        let depth = parent + usize::from(node.is_element());
        if depth > MAX_DEPTH {
            return Err(ImageError::Svg(format!(
                "elements nested deeper than {MAX_DEPTH}"
            )));
        }
        if let Some(slot) = depths.get_mut(node.id().get_usize()) {
            *slot = depth;
        }
    }
    Ok(xml)
}

/// usvg options for an image at `nesting` levels of nesting (0 for the
/// image itself). Embedded SVG images may use an effective depth of
/// `depth` and the rest of `budget`.
fn options(nesting: u32, depth: f64, budget: &SharedBudget) -> usvg::Options<'static> {
    let budget = Arc::clone(budget);
    usvg::Options {
        font_size: FONT_SIZE as f32,
        default_size: usvg::Size::from_wh(DEFAULT_OBJECT_SIZE.0, DEFAULT_OBJECT_SIZE.1)
            .expect("the default object size is positive"),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, _| {
                embedded_image(mime, &data, nesting, depth, &budget)
            }),
            resolve_string: Box::new(|href, _| {
                log::debug!("SVG image: not loading {href}: only data: URLs are allowed");
                None
            }),
        },
        ..usvg::Options::default()
    }
}

/// The image of a `data:` URL in an `<image>` element of an image at
/// `nesting` levels of nesting.
fn embedded_image(
    mime: &str,
    data: &Arc<Vec<u8>>,
    nesting: u32,
    depth: f64,
    budget: &SharedBudget,
) -> Option<usvg::ImageKind> {
    if mime == SVG_MIME_TYPE {
        if nesting >= MAX_NESTING {
            log::warn!("SVG image: SVG images nested more than {MAX_NESTING} levels deep");
            return None;
        }
        return match convert(data, nesting + 1, depth, budget) {
            Ok(Converted {
                tree: Some(tree), ..
            }) => Some(usvg::ImageKind::SVG(tree)),
            Ok(_) => None,
            Err(e) => {
                log::warn!("SVG image: embedded SVG image: {e}");
                None
            }
        };
    }
    let (format, width, height) = match crate::image::check_raster(data) {
        Ok(checked) => checked,
        Err(e) => {
            log::warn!("SVG image: embedded image: {e}");
            return None;
        }
    };
    if !take(budget, 0.0, 0.0, f64::from(width) * f64::from(height)) {
        log::warn!("SVG image: too many pixels in embedded images");
        return None;
    }
    let data = Arc::clone(data);
    match format {
        image::ImageFormat::Png => Some(usvg::ImageKind::PNG(data)),
        image::ImageFormat::Jpeg => Some(usvg::ImageKind::JPEG(data)),
        image::ImageFormat::Gif => Some(usvg::ImageKind::GIF(data)),
        image::ImageFormat::WebP => Some(usvg::ImageKind::WEBP(data)),
        format => {
            log::warn!("SVG image: embedded {format:?} images are not supported");
            None
        }
    }
}

/// The natural dimensions and the `viewBox` of a root `<svg>` element.
fn natural_size(root: roxmltree::Node<'_, '_>) -> (NaturalSize, Option<ViewBox>) {
    let width = root.attribute("width").and_then(absolute_length);
    let height = root.attribute("height").and_then(absolute_length);
    let view_box = root
        .attribute("viewBox")
        .and_then(|v| ViewBox::parse(v, root.attribute("preserveAspectRatio")));
    let ratio = match (width, height) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some(f64::from(w) / f64::from(h)),
        _ => view_box
            .as_ref()
            .map(|v| f64::from(v.rect.width()) / f64::from(v.rect.height())),
    };
    // Limited, so that layout can multiply and divide lengths by it.
    let max = f64::from(swb_style::Length::MAX_PX);
    let natural = NaturalSize {
        width,
        height,
        ratio: ratio
            .filter(|r| r.is_finite() && *r > 0.0)
            .map(|r| r.clamp(1.0 / max, max) as f32),
    };
    (natural, view_box)
}

/// An absolute length in CSS px, clamped to 0 and to the largest layout
/// length (as Chromium does).
fn absolute_length(value: &str) -> Option<f32> {
    let px = length_px(value)? as f32;
    (!px.is_nan()).then(|| px.clamp(0.0, swb_style::Length::MAX_PX))
}

/// A length in CSS px, as usvg converts it (`em` is the initial font size,
/// `ex` half of it). Percentages and invalid values (`auto`) are `None`.
fn length_px(value: &str) -> Option<f64> {
    let length: svgtypes::Length = value.parse().ok()?;
    let px_per_unit = match length.unit {
        LengthUnit::None | LengthUnit::Px => 1.0,
        LengthUnit::Em => FONT_SIZE,
        LengthUnit::Ex => FONT_SIZE / 2.0,
        LengthUnit::In => 96.0,
        LengthUnit::Cm => 96.0 / 2.54,
        LengthUnit::Mm => 96.0 / 25.4,
        LengthUnit::Pt => 96.0 / 72.0,
        LengthUnit::Pc => 16.0,
        LengthUnit::Percent => return None,
    };
    Some(length.number * px_per_unit)
}

/// The `viewBox` and `preserveAspectRatio` attributes of the root element.
#[derive(Clone, Copy, Debug)]
struct ViewBox {
    rect: NonZeroRect,
    align: Align,
    slice: bool,
}

impl ViewBox {
    /// Parses the attributes as usvg does: a `viewBox` that is not a valid
    /// non-empty rectangle is ignored.
    fn parse(view_box: &str, aspect: Option<&str>) -> Option<ViewBox> {
        let v: svgtypes::ViewBox = view_box.parse().ok()?;
        let rect = NonZeroRect::from_xywh(v.x as f32, v.y as f32, v.w as f32, v.h as f32)?;
        let aspect: svgtypes::AspectRatio = aspect.and_then(|a| a.parse().ok()).unwrap_or_default();
        Some(ViewBox {
            rect,
            align: aspect.align,
            slice: aspect.slice,
        })
    }

    /// The mapping of the view box into a viewport of `width` × `height` at
    /// the origin
    /// (<https://svgwg.org/svg2-draft/coords.html#ComputingAViewportsTransform>).
    fn mapping(&self, width: f64, height: f64) -> Mapping {
        let rect = self.rect;
        let (left, top) = (f64::from(rect.x()), f64::from(rect.y()));
        let (box_width, box_height) = (f64::from(rect.width()), f64::from(rect.height()));
        let mut sx = width / box_width;
        let mut sy = height / box_height;
        if self.align != Align::None {
            let scale = if self.slice { sx.max(sy) } else { sx.min(sy) };
            (sx, sy) = (scale, scale);
        }
        let (fx, fy) = match self.align {
            Align::None | Align::XMinYMin => (0.0, 0.0),
            Align::XMidYMin => (0.5, 0.0),
            Align::XMaxYMin => (1.0, 0.0),
            Align::XMinYMid => (0.0, 0.5),
            Align::XMidYMid => (0.5, 0.5),
            Align::XMaxYMid => (1.0, 0.5),
            Align::XMinYMax => (0.0, 1.0),
            Align::XMidYMax => (0.5, 1.0),
            Align::XMaxYMax => (1.0, 1.0),
        };
        Mapping {
            sx,
            sy,
            tx: -left * sx + (width - box_width * sx) * fx,
            ty: -top * sy + (height - box_height * sy) * fy,
        }
    }
}

/// A scale followed by a translation, in f64 (a view box mapping).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mapping {
    sx: f64,
    sy: f64,
    tx: f64,
    ty: f64,
}

impl Mapping {
    /// This mapping after the inverse of `inner`, as a transform, or `None`
    /// if `inner` cannot be inverted or the result is not finite. Computed
    /// directly, so a mapping with a tiny scale (a root of 0.001 px with a
    /// large view box) still inverts.
    fn after_inverse_of(self, inner: Mapping) -> Option<Transform> {
        let sx = self.sx / inner.sx;
        let sy = self.sy / inner.sy;
        let tx = self.tx - sx * inner.tx;
        let ty = self.ty - sy * inner.ty;
        let values = [sx, sy, tx, ty].map(|v| v as f32);
        values
            .iter()
            .all(|v| v.is_finite())
            .then(|| Transform::from_row(values[0], 0.0, 0.0, values[1], values[2], values[3]))
    }
}

/// `size`, or a smaller size with about the same proportions that has at
/// most `max_pixels` pixels; `None` if `max_pixels` is below 1. A side
/// that would round to 0 is 1 px, and the other side then gets all the
/// pixels, so very long thin sizes stay within `max_pixels` too.
fn fit_pixels(size: IntSize, max_pixels: f64) -> Option<IntSize> {
    let (width, height) = (f64::from(size.width()), f64::from(size.height()));
    if max_pixels.is_nan() || max_pixels < 1.0 {
        return None;
    }
    if width * height <= max_pixels {
        return Some(size);
    }
    let reduction = (max_pixels / (width * height)).sqrt();
    // At least 1 px; at most `max_pixels` px (`as` saturates).
    let reduce = |n: f64| ((n * reduction).floor().max(1.0)).min(max_pixels.floor()) as u32;
    let (mut w, mut h) = (reduce(width), reduce(height));
    // Too many pixels if a side was raised to 1 (or by rounding): the
    // longer side gets what the shorter one leaves. Each side is at most
    // `max_pixels`, so that is at least 1.
    let fit = |short: u32| (max_pixels / f64::from(short)).floor() as u32;
    if f64::from(w) * f64::from(h) > max_pixels {
        if w >= h {
            w = fit(h);
        } else {
            h = fit(w);
        }
    }
    IntSize::from_wh(w, h)
}

#[cfg(test)]
mod tests;
