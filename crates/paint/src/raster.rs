//! CPU rasterization of a display list with tiny-skia.
//!
//! Coordinates in the display list are CSS px relative to the document.
//! The rasterizer converts them to device px of the target (it subtracts
//! the scroll offset and multiplies by the device scale factor) and draws
//! in device px.
//!
//! - Rectangles without rounded corners, clips and the edges of borders
//!   without rounded corners are snapped to device pixels and filled
//!   without anti-aliasing, so that adjacent boxes have no seams.
//! - An opacity group draws into a layer that covers only the visible part
//!   of the group's bounds; the layer is then composited with the group
//!   opacity.
//! - Glyphs come from the `text` crate as alpha masks and are blended by
//!   [`blit_mask`].
//! - Vector (SVG) images are rendered at the device pixel size of one tile
//!   and then drawn like raster images.

use std::sync::Arc;

use swb_layout::{Point, Rect};
use swb_style::{BorderStyle, Rgba};
use swb_text::FontContext;
use tiny_skia::{
    FillRule, FilterQuality, GradientStop, IntSize, LinearGradient, Mask, Paint, Path, PathBuilder,
    Pattern, Pixmap, PixmapPaint, Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

use crate::display_list::{DisplayItem, DisplayList, ImageRef, Radii};
use crate::image::{DecodedImage, ImageKind, MAX_DIMENSION};
use crate::svg::{FrameBudget, MAX_RENDER_PIXELS, VectorCache};

/// Supplies decoded images to the rasterizer.
pub trait ImageSource {
    /// The decoded image, if it is loaded.
    fn image(&self, image: &ImageRef) -> Option<&DecodedImage>;

    /// The cache for vector images rendered at their drawn size. Without a
    /// cache, vector images are rendered for every draw.
    fn vector_cache(&self) -> Option<&VectorCache>;
}

/// Where and how to rasterize.
#[derive(Clone, Copy, Debug)]
pub struct RasterParams {
    /// The scroll offset in CSS px: this document point maps to the target's
    /// top-left corner.
    pub scroll: Point,
    /// Device pixels per CSS px.
    pub scale: f32,
}

/// Rasterizes a display list into `target`. The target is not cleared.
///
/// New renderings of SVG images share one work budget per call. When it is
/// used up, later SVG images are drawn from a cached rendering of another
/// size, or not at all; a later call (a repaint) renders them if its budget
/// allows. Normal pages stay far below the budget.
pub fn rasterize(
    list: &DisplayList,
    target: &mut Pixmap,
    params: RasterParams,
    fonts: &mut FontContext,
    images: &dyn ImageSource,
) {
    let viewport = Rect::new(
        params.scroll.x,
        params.scroll.y,
        target.width() as f32 / params.scale,
        target.height() as f32 / params.scale,
    );
    let vectors = images
        .vector_cache()
        .map_or_else(FrameBudget::new, VectorCache::begin_frame);
    let mut r = Rasterizer {
        target,
        params,
        layers: Vec::new(),
        clips: Vec::new(),
        mask: None,
        vectors,
    };
    for item in &list.items {
        if item
            .bounds()
            .is_some_and(|b| b.intersection(&viewport).is_none())
        {
            continue;
        }
        r.item(item, fonts, images);
    }
    // Close unbalanced opacity groups.
    while !r.layers.is_empty() {
        r.pop_layer();
    }
    if r.vectors.skipped() {
        log::warn!(
            "SVG images: the rendering budget of this frame ran out; some images \
             use a rendering of another size or are not drawn"
        );
    }
}

/// The layer of an open opacity group.
struct Layer {
    /// The pixels; `None` if the group draws nothing visible (opacity 0,
    /// or bounds outside the clip). Drawing is then skipped.
    pixmap: Option<Pixmap>,
    /// The position of the layer's top-left pixel on the target.
    origin: (i32, i32),
    opacity: f32,
}

struct Rasterizer<'a> {
    target: &'a mut Pixmap,
    params: RasterParams,
    /// Open opacity groups, innermost last.
    layers: Vec<Layer>,
    /// Clip rectangles in target device px, snapped to pixels; each entry
    /// is already intersected with the previous one.
    clips: Vec<Rect>,
    /// The mask of the innermost clip for the current surface, created on
    /// demand.
    mask: Option<Mask>,
    /// The rendering budget of this frame for SVG images.
    vectors: FrameBudget,
}

impl Rasterizer<'_> {
    fn item(&mut self, item: &DisplayItem, fonts: &mut FontContext, images: &dyn ImageSource) {
        match item {
            DisplayItem::Rect { rect, radii, color } => self.fill_rect(*rect, radii, *color),
            DisplayItem::Border {
                rect,
                widths,
                colors,
                styles,
                radii,
            } => self.border(*rect, *widths, *colors, *styles, radii),
            DisplayItem::Text {
                origin,
                font,
                size,
                glyphs,
                color,
            } => self.text(*origin, *font, *size, glyphs, *color, fonts),
            DisplayItem::Image {
                image,
                rect,
                tile,
                clip,
            } => {
                if let Some(decoded) = images.image(image) {
                    self.decoded_image(decoded, images.vector_cache(), *rect, *tile, *clip);
                }
            }
            DisplayItem::LinearGradient {
                rect,
                clip,
                gradient,
                current_color,
            } => self.linear_gradient(*rect, *clip, gradient, *current_color),
            DisplayItem::PushClip(rect) => {
                let device = snap(self.to_device(*rect));
                let clip = match self.clips.last() {
                    Some(c) => c.intersection(&device).unwrap_or_default(),
                    None => device,
                };
                self.clips.push(clip);
                self.mask = None;
            }
            DisplayItem::PopClip => {
                self.clips.pop();
                self.mask = None;
            }
            DisplayItem::PushOpacity { opacity, bounds } => self.push_layer(*opacity, *bounds),
            DisplayItem::PopOpacity => {
                if !self.layers.is_empty() {
                    self.pop_layer();
                }
            }
            DisplayItem::HitRegion { .. } => {}
        }
    }

    // ----- Coordinates and surfaces -----

    fn to_device(&self, r: Rect) -> Rect {
        let p = self.to_device_point(r.origin());
        let s = self.params.scale;
        Rect::new(p.x, p.y, r.width * s, r.height * s)
    }

    fn to_device_point(&self, p: Point) -> Point {
        let s = self.params.scale;
        Point::new(
            (p.x - self.params.scroll.x) * s,
            (p.y - self.params.scroll.y) * s,
        )
    }

    /// The current drawing surface in target device px, or `None` while
    /// drawing is skipped.
    fn surface_rect(&self) -> Option<Rect> {
        match self.layers.last() {
            None => Some(Rect::new(
                0.0,
                0.0,
                self.target.width() as f32,
                self.target.height() as f32,
            )),
            Some(layer) => layer.pixmap.as_ref().map(|p| {
                Rect::new(
                    layer.origin.0 as f32,
                    layer.origin.1 as f32,
                    p.width() as f32,
                    p.height() as f32,
                )
            }),
        }
    }

    fn surface_mut(&mut self) -> Option<&mut Pixmap> {
        match self.layers.last_mut() {
            None => Some(self.target),
            Some(layer) => layer.pixmap.as_mut(),
        }
    }

    /// The part of the current surface inside the clip, in target device
    /// px.
    fn visible_area(&self) -> Option<Rect> {
        let surface = self.surface_rect()?;
        match self.clips.last() {
            Some(clip) => clip.intersection(&surface),
            None => Some(surface),
        }
    }

    /// Draws on the current surface if a part of `bounds` (target device
    /// px) is visible. `paint` gets the surface, the transform from target
    /// device px to surface px, and the clip mask (`None` if `bounds` is
    /// inside the clip).
    fn draw(&mut self, bounds: Rect, paint: impl FnOnce(&mut Pixmap, Transform, Option<&Mask>)) {
        let Some(surface) = self.surface_rect() else {
            return;
        };
        let clip = self.clips.last().copied();
        let visible = match clip {
            Some(c) => c.intersection(&surface),
            None => Some(surface),
        };
        if visible.and_then(|v| v.intersection(&bounds)).is_none() {
            return;
        }
        let needs_mask = clip.is_some_and(|c| !contains(&c, &bounds));
        if needs_mask && self.mask.is_none() {
            self.mask = clip.and_then(|c| clip_mask(c, surface));
            if self.mask.is_none() {
                return;
            }
        }
        let transform = Transform::from_translate(-surface.x, -surface.y);
        let Rasterizer {
            target,
            layers,
            mask,
            ..
        } = self;
        let pixmap = match layers.last_mut() {
            None => &mut **target,
            Some(layer) => match layer.pixmap.as_mut() {
                Some(p) => p,
                None => return,
            },
        };
        let mask = if needs_mask { mask.as_ref() } else { None };
        paint(pixmap, transform, mask);
    }

    // ----- Opacity groups -----

    fn push_layer(&mut self, opacity: f32, bounds: Rect) {
        self.mask = None;
        let area = self
            .visible_area()
            .and_then(|v| snap_out(self.to_device(bounds)).intersection(&v));
        let layer = match area {
            Some(a) if opacity > 0.0 => Layer {
                pixmap: Pixmap::new(a.width as u32, a.height as u32),
                origin: (a.x as i32, a.y as i32),
                opacity,
            },
            _ => Layer {
                pixmap: None,
                origin: (0, 0),
                opacity,
            },
        };
        self.layers.push(layer);
    }

    fn pop_layer(&mut self) {
        self.mask = None;
        let Some(layer) = self.layers.pop() else {
            return;
        };
        let (Some(pixmap), Some(parent)) = (layer.pixmap, self.surface_rect()) else {
            return;
        };
        let x = layer.origin.0 - parent.x as i32;
        let y = layer.origin.1 - parent.y as i32;
        let paint = PixmapPaint {
            opacity: layer.opacity,
            ..PixmapPaint::default()
        };
        if let Some(surface) = self.surface_mut() {
            surface.draw_pixmap(x, y, pixmap.as_ref(), &paint, Transform::identity(), None);
        }
    }

    // ----- Rectangles and borders -----

    fn fill_rect(&mut self, rect: Rect, radii: &Radii, color: Rgba) {
        if color.is_transparent() {
            return;
        }
        let device = self.to_device(rect);
        if has_radius(radii) {
            let radii = scale_radii(radii, self.params.scale);
            let Some(path) = rounded_rect_path(device, &radii) else {
                return;
            };
            let paint = solid_paint(color, true);
            self.draw(device, |p, t, m| {
                p.fill_path(&path, &paint, FillRule::Winding, t, m);
            });
        } else {
            let snapped = snap(device);
            let Some(r) = skia_rect(snapped) else {
                return;
            };
            let paint = solid_paint(color, false);
            self.draw(snapped, |p, t, m| p.fill_rect(r, &paint, t, m));
        }
    }

    fn border(
        &mut self,
        rect: Rect,
        widths: [f32; 4],
        colors: [Rgba; 4],
        styles: [BorderStyle; 4],
        radii: &Radii,
    ) {
        let s = self.params.scale;
        let geometry = BorderGeometry::new(
            self.to_device(rect),
            widths.map(|w| w.max(0.0) * s),
            scale_radii(radii, s),
        );
        let has_width = |i: usize| widths[i] > 0.0 && !styles[i].has_no_width();
        let first = (0..4).find(|&i| has_width(i));
        let Some(first) = first else {
            return;
        };
        let uniform = (0..4)
            .filter(|&i| has_width(i))
            .all(|i| colors[i] == colors[first] && styles[i] == styles[first]);
        if uniform && matches!(styles[first], BorderStyle::Solid | BorderStyle::Double) {
            self.uniform_border(&geometry, colors[first], styles[first]);
        } else {
            self.border_sides(&geometry, colors, styles);
        }
    }

    /// A border whose sides have one color and a solid or double style:
    /// one ring, or two rings for `double`.
    fn uniform_border(&mut self, geometry: &BorderGeometry, color: Rgba, style: BorderStyle) {
        if color.is_transparent() {
            return;
        }
        // Like browsers, draw `double` as solid if the lines would be
        // thinner than one pixel.
        let thinnest = geometry
            .widths
            .iter()
            .copied()
            .filter(|w| *w > 0.0)
            .fold(f32::INFINITY, f32::min);
        let bands: &[(f32, f32)] = if style == BorderStyle::Double && thinnest >= 3.0 {
            &[(0.0, 1.0 / 3.0), (2.0 / 3.0, 1.0)]
        } else {
            &[(0.0, 1.0)]
        };
        let paint = solid_paint(color, true);
        for &(from, to) in bands {
            let Some(path) = geometry.ring(from, to) else {
                continue;
            };
            self.draw(geometry.outer, |p, t, m| {
                p.fill_path(&path, &paint, FillRule::EvenOdd, t, m);
            });
        }
    }

    /// A border with different sides: each side is painted separately, with
    /// the 3D shading of `inset`, `outset`, `groove` and `ridge`. `double`
    /// is painted as solid here.
    fn border_sides(
        &mut self,
        geometry: &BorderGeometry,
        colors: [Rgba; 4],
        styles: [BorderStyle; 4],
    ) {
        // Whole sides, the outer halves of grooves and ridges, and their
        // inner halves.
        let mut whole = [None; 4];
        let mut outer_half = [None; 4];
        let mut inner_half = [None; 4];
        for side in 0..4 {
            let (color, style) = (colors[side], styles[side]);
            if geometry.widths[side] <= 0.0 || color.is_transparent() || style.has_no_width() {
                continue;
            }
            match style {
                BorderStyle::Dashed | BorderStyle::Dotted => {
                    self.dashed_side(geometry, side, color, style);
                }
                BorderStyle::Groove => {
                    outer_half[side] = Some(shade(color, BorderStyle::Inset, side));
                    inner_half[side] = Some(shade(color, BorderStyle::Outset, side));
                }
                BorderStyle::Ridge => {
                    outer_half[side] = Some(shade(color, BorderStyle::Outset, side));
                    inner_half[side] = Some(shade(color, BorderStyle::Inset, side));
                }
                _ => whole[side] = Some(shade(color, style, side)),
            }
        }
        self.fill_sides(geometry, (0.0, 1.0), whole);
        self.fill_sides(geometry, (0.0, 0.5), outer_half);
        self.fill_sides(geometry, (0.5, 1.0), inner_half);
    }

    /// Fills the band `from..to` (fractions of the border width, 0 is the
    /// outer edge) of the given sides (see [`side_polygons`]). With rounded
    /// corners, the sides are drawn into a temporary layer and cut to the
    /// rounded ring.
    fn fill_sides(
        &mut self,
        geometry: &BorderGeometry,
        band: (f32, f32),
        colors: [Option<Rgba>; 4],
    ) {
        if colors.iter().all(Option::is_none) {
            return;
        }
        let (outer, _) = geometry.edge(band.0);
        let (inner, _) = geometry.edge(band.1);
        let painted = colors.map(|c| c.is_some());
        let polygons = side_polygons(outer, inner, geometry.has_radius, painted);
        // Sides of the same color are filled as one path, so that there is
        // no anti-aliasing seam on the diagonal between them.
        let mut by_color: Vec<(Rgba, PathBuilder)> = Vec::new();
        for (polygon, color) in polygons.iter().zip(colors) {
            let Some(color) = color else {
                continue;
            };
            let index = by_color
                .iter()
                .position(|(c, _)| *c == color)
                .unwrap_or_else(|| {
                    by_color.push((color, PathBuilder::new()));
                    by_color.len() - 1
                });
            push_polygon(&mut by_color[index].1, polygon);
        }
        let sides: Vec<(Path, Paint<'static>)> = by_color
            .into_iter()
            .filter_map(|(color, pb)| Some((pb.finish()?, solid_paint(color, true))))
            .collect();
        if !geometry.has_radius {
            for (path, paint) in &sides {
                self.draw(outer, |p, t, m| {
                    p.fill_path(path, paint, FillRule::Winding, t, m);
                });
            }
            return;
        }
        let Some(ring) = geometry.ring(band.0, band.1) else {
            return;
        };
        let Some(area) = self
            .visible_area()
            .and_then(|v| snap_out(outer).intersection(&v))
        else {
            return;
        };
        let (Some(mut layer), Some(mut ring_mask)) = (
            Pixmap::new(area.width as u32, area.height as u32),
            Mask::new(area.width as u32, area.height as u32),
        ) else {
            return;
        };
        let to_layer = Transform::from_translate(-area.x, -area.y);
        for (path, paint) in &sides {
            layer.fill_path(path, paint, FillRule::Winding, to_layer, None);
        }
        ring_mask.fill_path(&ring, FillRule::EvenOdd, true, to_layer);
        layer.apply_mask(&ring_mask);
        self.draw(area, |p, t, m| {
            p.draw_pixmap(
                area.x as i32,
                area.y as i32,
                layer.as_ref(),
                &PixmapPaint::default(),
                t,
                m,
            );
        });
    }

    /// A dashed or dotted side: a stroke along the middle of the side.
    /// Rounded corners are not followed.
    fn dashed_side(
        &mut self,
        geometry: &BorderGeometry,
        side: usize,
        color: Rgba,
        style: BorderStyle,
    ) {
        let rect = geometry.outer;
        let width = geometry.widths[side];
        let half = width / 2.0;
        let (x0, y0, x1, y1) = match side {
            0 => (rect.x, rect.y + half, rect.right(), rect.y + half),
            1 => (
                rect.right() - half,
                rect.y,
                rect.right() - half,
                rect.bottom(),
            ),
            2 => (
                rect.right(),
                rect.bottom() - half,
                rect.x,
                rect.bottom() - half,
            ),
            _ => (rect.x + half, rect.bottom(), rect.x + half, rect.y),
        };
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        let Some(path) = pb.finish() else {
            return;
        };
        let dash = if style == BorderStyle::Dotted {
            StrokeDash::new(vec![width, width], 0.0)
        } else {
            StrokeDash::new(vec![width * 3.0, width * 3.0], 0.0)
        };
        let stroke = Stroke {
            width,
            dash,
            ..Stroke::default()
        };
        let paint = solid_paint(color, true);
        self.draw(rect, |p, t, m| p.stroke_path(&path, &paint, &stroke, t, m));
    }

    // ----- Text -----

    fn text(
        &mut self,
        origin: Point,
        font: swb_text::FontId,
        size: f32,
        glyphs: &[swb_layout::PositionedGlyph],
        color: Rgba,
        fonts: &mut FontContext,
    ) {
        let (Some(surface), Some(visible)) = (self.surface_rect(), self.visible_area()) else {
            return;
        };
        let scale = self.params.scale;
        let device_size = size * scale;
        let start = self.to_device_point(origin);
        let baseline_y = start.y.round();
        // The clip in surface px.
        let clip = Rect::new(
            visible.x - surface.x,
            visible.y - surface.y,
            visible.width,
            visible.height,
        );
        let Some(pixmap) = self.surface_mut() else {
            return;
        };
        for g in glyphs {
            let x = start.x + g.x * scale;
            let y = baseline_y + (g.y * scale).round();
            // Skip glyphs that cannot reach the visible area (the same
            // margins as `DisplayItem::bounds`) before rasterizing them.
            if x - device_size > visible.right()
                || x + 4.0 * device_size < visible.x
                || y - 3.0 * device_size > visible.bottom()
                || y + 2.0 * device_size < visible.y
            {
                continue;
            }
            let (pixel_x, subpixel) = subpixel_position(x);
            let Some(mask) = fonts.glyph_mask(font, g.id, device_size, subpixel) else {
                continue;
            };
            let left = (pixel_x as i64).saturating_add(i64::from(mask.left)) - surface.x as i64;
            let top = (y as i64).saturating_add(i64::from(mask.top)) - surface.y as i64;
            let alpha = AlphaMask {
                data: &mask.data,
                width: mask.width,
                height: mask.height,
            };
            blit_mask(pixmap, &alpha, (left, top), color, clip);
        }
    }

    // ----- Images and gradients -----

    /// Draws a raster image, or a vector image rendered for the device
    /// pixel size of `tile`.
    fn decoded_image(
        &mut self,
        image: &DecodedImage,
        cache: Option<&VectorCache>,
        area: Rect,
        tile: Rect,
        clip: Rect,
    ) {
        let svg = match image.kind() {
            ImageKind::Raster(pixmap) => return self.image(pixmap, area, tile, clip),
            ImageKind::Vector(svg) => svg,
        };
        let Some(size) = vector_render_size(self.to_device(tile)) else {
            return;
        };
        let concrete = (tile.width, tile.height);
        let pixmap = match cache {
            Some(cache) => cache.get(svg, size, concrete, &mut self.vectors),
            None if self.vectors.take(svg.render_work(size, concrete)) => {
                svg.render(size, concrete).map(Arc::new)
            }
            None => None,
        };
        if let Some(pixmap) = pixmap {
            self.image(&pixmap, area, tile, clip);
        }
    }

    fn image(&mut self, pixmap: &Pixmap, area: Rect, tile: Rect, clip: Rect) {
        // How far the painted area may extend past the tile and still count
        // as a single, untiled image (CSS px).
        const EPSILON: f32 = 0.01;
        if tile.width <= 0.0 || tile.height <= 0.0 {
            return;
        }
        let Some(area) = area.intersection(&clip) else {
            return;
        };
        let tiled = area.x < tile.x - EPSILON
            || area.y < tile.y - EPSILON
            || area.right() > tile.right() + EPSILON
            || area.bottom() > tile.bottom() + EPSILON;
        let tile = self.to_device(tile);
        let area = snap(self.to_device(area));
        let Some(r) = skia_rect(area) else {
            return;
        };
        let sx = tile.width / pixmap.width() as f32;
        let sy = tile.height / pixmap.height() as f32;
        let quality = if (sx - 1.0).abs() < 0.01 && (sy - 1.0).abs() < 0.01 {
            FilterQuality::Nearest
        } else {
            FilterQuality::Bilinear
        };
        // A single image clamps at its edges; repeating one would blend
        // the opposite edge into the border pixels.
        let spread = if tiled {
            SpreadMode::Repeat
        } else {
            SpreadMode::Pad
        };
        let paint = Paint {
            shader: Pattern::new(
                pixmap.as_ref(),
                spread,
                quality,
                1.0,
                Transform::from_row(sx, 0.0, 0.0, sy, tile.x, tile.y),
            ),
            anti_alias: false,
            ..Paint::default()
        };
        self.draw(area, |p, t, m| p.fill_rect(r, &paint, t, m));
    }

    /// Fills the part of `rect` inside `clip` with a linear gradient. The
    /// gradient line goes through the center of `rect` at the gradient
    /// angle, with the length that CSS Images 3 §3.1.1 defines
    /// (<https://www.w3.org/TR/css-images-3/#linear-gradient-syntax>).
    fn linear_gradient(
        &mut self,
        rect: Rect,
        clip: Rect,
        gradient: &swb_style::LinearGradient,
        current: Rgba,
    ) {
        let n = gradient.stops.len();
        let Some(fill) = rect.intersection(&clip) else {
            return;
        };
        if n == 0 {
            return;
        }
        let angle = gradient.angle_deg.to_radians();
        let (sin, cos) = angle.sin_cos();
        let half_len = f32::midpoint(rect.width * sin.abs(), rect.height * cos.abs());
        let cx = rect.x + rect.width / 2.0;
        let cy = rect.y + rect.height / 2.0;
        let start = self.to_device_point(Point::new(cx - sin * half_len, cy + cos * half_len));
        let end = self.to_device_point(Point::new(cx + sin * half_len, cy - cos * half_len));
        let line_len = half_len * 2.0;
        let stops: Vec<GradientStop> = gradient
            .stops
            .iter()
            .enumerate()
            .map(|(i, (color, pos))| {
                let t = pos.as_ref().map_or_else(
                    || {
                        if n > 1 {
                            i as f32 / (n - 1) as f32
                        } else {
                            0.0
                        }
                    },
                    |p| {
                        if line_len > 0.0 {
                            p.resolve(line_len) / line_len
                        } else {
                            0.0
                        }
                    },
                );
                let c = color.resolve(current);
                GradientStop::new(t.clamp(0.0, 1.0), to_skia_color(c))
            })
            .collect();
        let spread = if gradient.repeating {
            SpreadMode::Repeat
        } else {
            SpreadMode::Pad
        };
        let shader = LinearGradient::new(
            tiny_skia::Point::from_xy(start.x, start.y),
            tiny_skia::Point::from_xy(end.x, end.y),
            stops,
            spread,
            Transform::identity(),
        );
        let Some(shader) = shader else {
            // A single stop or degenerate line: fill with the first color.
            if let Some((c, _)) = gradient.stops.first() {
                self.fill_rect(fill, &[(0.0, 0.0); 4], c.resolve(current));
            }
            return;
        };
        let area = snap(self.to_device(fill));
        let Some(r) = skia_rect(area) else {
            return;
        };
        let paint = Paint {
            shader,
            anti_alias: false,
            ..Paint::default()
        };
        self.draw(area, |p, t, m| p.fill_rect(r, &paint, t, m));
    }
}

/// The geometry of a border box in device px.
struct BorderGeometry {
    /// The outer edge, snapped to pixels if there are no rounded corners.
    outer: Rect,
    /// Widths: top, right, bottom, left.
    widths: [f32; 4],
    /// Outer corner radii.
    radii: Radii,
    has_radius: bool,
}

impl BorderGeometry {
    fn new(outer: Rect, widths: [f32; 4], radii: Radii) -> Self {
        let has_radius = has_radius(&radii);
        BorderGeometry {
            outer: if has_radius { outer } else { snap(outer) },
            widths,
            radii,
            has_radius,
        }
    }

    /// The edge at `fraction` of the border width from the outer edge (0
    /// is the outer edge, 1 the inner edge), with its corner radii.
    fn edge(&self, fraction: f32) -> (Rect, Radii) {
        let [top, right, bottom, left] = self.widths.map(|w| w * fraction);
        let o = self.outer;
        let rect = Rect::new(
            o.x + left,
            o.y + top,
            (o.width - left - right).max(0.0),
            (o.height - top - bottom).max(0.0),
        );
        if !self.has_radius {
            return (snap(rect), [(0.0, 0.0); 4]);
        }
        let r = self.radii;
        let radii = [
            ((r[0].0 - left).max(0.0), (r[0].1 - top).max(0.0)),
            ((r[1].0 - right).max(0.0), (r[1].1 - top).max(0.0)),
            ((r[2].0 - right).max(0.0), (r[2].1 - bottom).max(0.0)),
            ((r[3].0 - left).max(0.0), (r[3].1 - bottom).max(0.0)),
        ];
        (rect, radii)
    }

    /// The area between the edges at `from` and `to` (see [`Self::edge`]),
    /// filled with the even-odd rule.
    fn ring(&self, from: f32, to: f32) -> Option<Path> {
        let (outer, outer_radii) = self.edge(from);
        let (inner, inner_radii) = self.edge(to);
        let mut pb = PathBuilder::new();
        push_rounded_rect(&mut pb, outer, &outer_radii);
        push_rounded_rect(&mut pb, inner, &inner_radii);
        pb.finish()
    }
}

/// The area of each side (top, right, bottom, left) between the outer and
/// inner border edges, split at the lines through the corners. Without
/// rounded corners the sides are trapezoids. With rounded corners the
/// sides are extended to the center of the box, so that together they
/// cover the whole rounded ring, including the curved corner areas inside
/// the inner rectangle. A corner next to a side that is not painted
/// (`painted[i]` is false) belongs entirely to the painted side, so its
/// tapering end is drawn as in browsers.
fn side_polygons(
    outer: Rect,
    inner: Rect,
    to_center: bool,
    painted: [bool; 4],
) -> [Vec<(f32, f32)>; 4] {
    let outer_corners = [
        (outer.x, outer.y),
        (outer.right(), outer.y),
        (outer.right(), outer.bottom()),
        (outer.x, outer.bottom()),
    ];
    let inner_corners = [
        (inner.x, inner.y),
        (inner.right(), inner.y),
        (inner.right(), inner.bottom()),
        (inner.x, inner.bottom()),
    ];
    if !to_center {
        return [0, 1, 2, 3].map(|side| {
            let (start, end) = (side, (side + 1) % 4);
            vec![
                outer_corners[start],
                outer_corners[end],
                inner_corners[end],
                inner_corners[start],
            ]
        });
    }
    let center = (
        f32::midpoint(inner.x, inner.right()),
        f32::midpoint(inner.y, inner.bottom()),
    );
    // The point where `side` ends at `corner` (`side` or `side + 1`): the
    // inner corner, or, if the other side at that corner is not painted,
    // the point of the outer edge level with the center.
    let corner_end = |side: usize, corner: usize| {
        let other = if corner == side {
            (side + 3) % 4
        } else {
            (side + 1) % 4
        };
        if painted[other] {
            inner_corners[corner]
        } else if side.is_multiple_of(2) {
            // Top and bottom sides extend along the left/right edge.
            (outer_corners[corner].0, center.1)
        } else {
            // Left and right sides extend along the top/bottom edge.
            (center.0, outer_corners[corner].1)
        }
    };
    [0, 1, 2, 3].map(|side| {
        let (start, end) = (side, (side + 1) % 4);
        vec![
            outer_corners[start],
            outer_corners[end],
            corner_end(side, end),
            center,
            corner_end(side, start),
        ]
    })
}

/// Appends a closed polygon to a path.
fn push_polygon(pb: &mut PathBuilder, points: &[(f32, f32)]) {
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.close();
}

/// The pixel size to render a vector image at for a tile of `device` px:
/// the tile size, rounded. A size above [`MAX_RENDER_PIXELS`] or
/// [`MAX_DIMENSION`] is reduced, keeping its proportions; the drawing then
/// scales the rendering up. `None` for an empty tile.
fn vector_render_size(device: Rect) -> Option<IntSize> {
    let valid = |v: f32| v > 0.0 && v.is_finite();
    if !(valid(device.width) && valid(device.height)) {
        return None;
    }
    // f64: the product of two large f32 sizes overflows f32.
    let mut width = f64::from(device.width).round().max(1.0);
    let mut height = f64::from(device.height).round().max(1.0);
    let max_dimension = f64::from(MAX_DIMENSION);
    let reduction = (f64::from(MAX_RENDER_PIXELS) / (width * height))
        .sqrt()
        .min(max_dimension / width)
        .min(max_dimension / height);
    if reduction < 1.0 {
        width = (width * reduction).floor().max(1.0);
        height = (height * reduction).floor().max(1.0);
    }
    IntSize::from_wh(width as u32, height as u32)
}

/// A mask that covers `clip` (target device px) on a surface at `surface`.
fn clip_mask(clip: Rect, surface: Rect) -> Option<Mask> {
    let mut mask = Mask::new(surface.width as u32, surface.height as u32)?;
    if let Some(rect) = tiny_skia::Rect::from_xywh(
        clip.x - surface.x,
        clip.y - surface.y,
        clip.width,
        clip.height,
    ) {
        mask.fill_path(
            &PathBuilder::from_rect(rect),
            FillRule::Winding,
            false,
            Transform::identity(),
        );
    }
    Some(mask)
}

/// True if `outer` contains all of `inner`.
fn contains(outer: &Rect, inner: &Rect) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.right() >= inner.right()
        && outer.bottom() >= inner.bottom()
}

/// Rounds the edges of a rectangle to whole pixels. A rectangle that is
/// not empty stays at least one pixel wide and high.
fn snap(r: Rect) -> Rect {
    let snap_axis = |start: f32, size: f32| {
        let a = start.round();
        let mut b = (start + size).round();
        if size > 0.0 && b <= a {
            b = a + 1.0;
        }
        (a, (b - a).max(0.0))
    };
    let (x, width) = snap_axis(r.x, r.width);
    let (y, height) = snap_axis(r.y, r.height);
    Rect::new(x, y, width, height)
}

/// The smallest rectangle with whole-pixel edges that contains `r`.
fn snap_out(r: Rect) -> Rect {
    let x = r.x.floor();
    let y = r.y.floor();
    Rect::new(x, y, r.right().ceil() - x, r.bottom().ceil() - y)
}

fn skia_rect(r: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_xywh(r.x, r.y, r.width, r.height)
}

/// The pixel column and the subpixel offset in quarter pixels (0 to 3) of
/// a glyph at device x position `x`.
fn subpixel_position(x: f32) -> (f32, u8) {
    let floor = x.floor();
    let quarters = ((x - floor) * 4.0).round();
    if quarters >= 4.0 {
        (floor + 1.0, 0)
    } else {
        (floor, quarters as u8)
    }
}

fn has_radius(radii: &Radii) -> bool {
    radii.iter().any(|(h, v)| *h > 0.0 || *v > 0.0)
}

fn scale_radii(radii: &Radii, s: f32) -> Radii {
    radii.map(|(h, v)| (h * s, v * s))
}

fn to_skia_color(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn solid_paint(c: Rgba, anti_alias: bool) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(to_skia_color(c)),
        anti_alias,
        ..Paint::default()
    }
}

/// Shading for 3D border styles, as browsers do it: the top and left sides
/// of `inset`/`groove` are darker, the others lighter (and the reverse for
/// `outset`/`ridge`).
fn shade(c: Rgba, style: BorderStyle, side: usize) -> Rgba {
    let top_left = side == 0 || side == 3;
    let darken = match style {
        BorderStyle::Inset | BorderStyle::Groove => top_left,
        BorderStyle::Outset | BorderStyle::Ridge => !top_left,
        _ => return c,
    };
    if darken {
        Rgba::new(c.r / 3 * 2, c.g / 3 * 2, c.b / 3 * 2, c.a)
    } else {
        c
    }
}

fn rounded_rect_path(rect: Rect, radii: &Radii) -> Option<Path> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let mut pb = PathBuilder::new();
    push_rounded_rect(&mut pb, rect, radii);
    pb.finish()
}

/// Appends a rectangle with elliptical corners to `pb`.
fn push_rounded_rect(pb: &mut PathBuilder, r: Rect, radii: &Radii) {
    // Control point factor for a quarter ellipse with cubic Béziers.
    const K: f32 = 0.552_284_8;
    if r.width <= 0.0 || r.height <= 0.0 {
        return;
    }
    let [(tlx, tly), (trx, try_), (brx, bry), (blx, bly)] = *radii;
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    pb.move_to(x0 + tlx, y0);
    pb.line_to(x1 - trx, y0);
    if trx > 0.0 || try_ > 0.0 {
        pb.cubic_to(
            x1 - trx + trx * K,
            y0,
            x1,
            y0 + try_ - try_ * K,
            x1,
            y0 + try_,
        );
    }
    pb.line_to(x1, y1 - bry);
    if brx > 0.0 || bry > 0.0 {
        pb.cubic_to(x1, y1 - bry + bry * K, x1 - brx + brx * K, y1, x1 - brx, y1);
    }
    pb.line_to(x0 + blx, y1);
    if blx > 0.0 || bly > 0.0 {
        pb.cubic_to(x0 + blx - blx * K, y1, x0, y1 - bly + bly * K, x0, y1 - bly);
    }
    pb.line_to(x0, y0 + tly);
    if tlx > 0.0 || tly > 0.0 {
        pb.cubic_to(x0, y0 + tly - tly * K, x0 + tlx - tlx * K, y0, x0 + tlx, y0);
    }
    pb.close();
}

/// An alpha mask: one coverage byte per pixel, row by row.
pub(crate) struct AlphaMask<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Blends `mask` in `color` into a premultiplied RGBA pixmap, with the
/// mask's top-left corner at `origin` (pixmap px), clipped to `clip`.
pub(crate) fn blit_mask(
    pixmap: &mut Pixmap,
    mask: &AlphaMask<'_>,
    origin: (i64, i64),
    color: Rgba,
    clip: Rect,
) {
    let (left, top) = origin;
    let width = i64::from(pixmap.width());
    let height = i64::from(pixmap.height());
    let x_min = left.max(clip.x.floor() as i64).max(0);
    let y_min = top.max(clip.y.floor() as i64).max(0);
    let x_max = left
        .saturating_add(i64::from(mask.width))
        .min(clip.right().ceil() as i64)
        .min(width);
    let y_max = top
        .saturating_add(i64::from(mask.height))
        .min(clip.bottom().ceil() as i64)
        .min(height);
    if x_min >= x_max || y_min >= y_max {
        return;
    }
    let alpha = u32::from(color.a);
    let (red, green, blue) = (u32::from(color.r), u32::from(color.g), u32::from(color.b));
    let mask_width = mask.width as usize;
    let data = pixmap.data_mut();
    // All indices below are inside the mask and the pixmap: the ranges
    // were clamped to both above.
    for row in y_min..y_max {
        let mask_row = (row - top) as usize * mask_width;
        let pixel_row = (row * width) as usize * 4;
        for column in x_min..x_max {
            let Some(&coverage) = mask.data.get(mask_row + (column - left) as usize) else {
                continue;
            };
            let a = u32::from(coverage) * alpha / 255;
            if a == 0 {
                continue;
            }
            let i = pixel_row + column as usize * 4;
            let inverse = 255 - a;
            let blend =
                |src: u32, dst: u8| ((src * a + u32::from(dst) * inverse + 127) / 255) as u8;
            data[i] = blend(red, data[i]);
            data[i + 1] = blend(green, data[i + 1]);
            data[i + 2] = blend(blue, data[i + 2]);
            data[i + 3] = ((a * 255 + u32::from(data[i + 3]) * inverse + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blit_blends_and_clips() {
        let mut pixmap = Pixmap::new(4, 4).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        let data = [255u8, 128, 0, 255];
        let mask = AlphaMask {
            data: &data,
            width: 2,
            height: 2,
        };
        let clip = Rect::new(0.0, 0.0, 4.0, 4.0);
        blit_mask(&mut pixmap, &mask, (3, 3), Rgba::BLACK, clip);
        // Only the top-left mask pixel lands inside the 4x4 pixmap.
        let px = pixmap.pixel(3, 3).unwrap();
        assert_eq!((px.red(), px.alpha()), (0, 255));
        let untouched = pixmap.pixel(2, 2).unwrap();
        assert_eq!(untouched.red(), 255);
    }

    #[test]
    fn blit_far_away_does_not_overflow() {
        let mut pixmap = Pixmap::new(4, 4).unwrap();
        let mask = AlphaMask {
            data: &[255; 4],
            width: 2,
            height: 2,
        };
        let clip = Rect::new(0.0, 0.0, 4.0, 4.0);
        blit_mask(&mut pixmap, &mask, (i64::MAX - 1, 0), Rgba::BLACK, clip);
        blit_mask(&mut pixmap, &mask, (0, i64::MAX), Rgba::BLACK, clip);
        assert!(pixmap.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn half_coverage_is_grey() {
        let mut pixmap = Pixmap::new(1, 1).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        let mask = AlphaMask {
            data: &[128],
            width: 1,
            height: 1,
        };
        blit_mask(
            &mut pixmap,
            &mask,
            (0, 0),
            Rgba::BLACK,
            Rect::new(0.0, 0.0, 1.0, 1.0),
        );
        let px = pixmap.pixel(0, 0).unwrap();
        assert!((126..=128).contains(&px.red()));
    }

    #[test]
    fn subpixel_positions_round_to_the_next_pixel() {
        assert_eq!(subpixel_position(10.0), (10.0, 0));
        assert_eq!(subpixel_position(10.3), (10.0, 1));
        assert_eq!(subpixel_position(10.6), (10.0, 2));
        assert_eq!(subpixel_position(10.8), (10.0, 3));
        assert_eq!(subpixel_position(10.9), (11.0, 0));
        assert_eq!(subpixel_position(-0.05), (0.0, 0));
    }

    struct NoImages;

    impl ImageSource for NoImages {
        fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
            None
        }

        fn vector_cache(&self) -> Option<&VectorCache> {
            None
        }
    }

    struct OneImage(DecodedImage, Option<VectorCache>);

    impl ImageSource for OneImage {
        fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
            Some(&self.0)
        }

        fn vector_cache(&self) -> Option<&VectorCache> {
            self.1.as_ref()
        }
    }

    const BLUE: Rgba = Rgba::rgb(0, 0, 255);
    const RED: Rgba = Rgba::rgb(255, 0, 0);

    /// Rasterizes `items` on a white 100x100 target at `scale`.
    fn render_with(items: Vec<DisplayItem>, scale: f32, images: &dyn ImageSource) -> Pixmap {
        let size = (100.0 * scale) as u32;
        let mut target = Pixmap::new(size, size).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let params = RasterParams {
            scroll: Point::default(),
            scale,
        };
        let mut fonts = FontContext::for_tests();
        rasterize(
            &DisplayList { items },
            &mut target,
            params,
            &mut fonts,
            images,
        );
        target
    }

    fn render(items: Vec<DisplayItem>) -> Pixmap {
        render_with(items, 1.0, &NoImages)
    }

    fn rgb(p: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
        let c = p.pixel(x, y).unwrap().demultiply();
        (c.red(), c.green(), c.blue())
    }

    fn border(widths: [f32; 4], colors: [Rgba; 4], style: BorderStyle, radius: f32) -> DisplayItem {
        DisplayItem::Border {
            rect: Rect::new(10.0, 10.0, 80.0, 60.0),
            widths,
            colors,
            styles: [style; 4],
            radii: [(radius, radius); 4],
        }
    }

    #[test]
    fn rounded_border_keeps_the_color_of_each_side() {
        // Only the bottom side has a width; the top color must not be used.
        let p = render(vec![border(
            [0.0, 0.0, 10.0, 0.0],
            [Rgba::BLACK, Rgba::BLACK, BLUE, Rgba::BLACK],
            BorderStyle::Solid,
            4.0,
        )]);
        assert_eq!(rgb(&p, 50, 65), (0, 0, 255));
        assert_eq!(rgb(&p, 50, 30), (255, 255, 255));
        // The rounded corner cuts the bottom-left pixel.
        assert_eq!(rgb(&p, 10, 69), (255, 255, 255));
    }

    #[test]
    fn rounded_border_has_no_gaps_at_the_corners() {
        // The spinner pattern: a circle with one side in another color. The
        // ring at 45° lies inside the inner rectangle's corner, outside the
        // side trapezoids.
        let item = DisplayItem::Border {
            rect: Rect::new(10.0, 10.0, 80.0, 80.0),
            widths: [10.0; 4],
            colors: [BLUE, RED, RED, RED],
            styles: [BorderStyle::Solid; 4],
            radii: [(40.0, 40.0); 4],
        };
        let p = render(vec![item]);
        // A point on the ring's center line at 45° (top-left corner).
        let d = 35.0 * std::f32::consts::FRAC_1_SQRT_2;
        let (x, y) = ((50.0 - d) as u32, (50.0 - d) as u32);
        assert_ne!(
            rgb(&p, x, y),
            (255, 255, 255),
            "corner pixel ({x}, {y}) is painted"
        );
        // The bottom-right corner belongs to the right/bottom sides.
        let (x, y) = ((50.0 + d) as u32, (50.0 + d) as u32);
        assert_eq!(rgb(&p, x, y), (255, 0, 0));
    }

    #[test]
    fn rounded_corner_belongs_to_the_painted_side() {
        // Only the top side has a width: its rounded ends taper into the
        // corners, which must be painted although they lie below the inner
        // corner line.
        let item = DisplayItem::Border {
            rect: Rect::new(10.0, 10.0, 80.0, 60.0),
            widths: [4.0, 0.0, 0.0, 0.0],
            colors: [BLUE; 4],
            styles: [BorderStyle::Solid; 4],
            radii: [(20.0, 20.0); 4],
        };
        let p = render(vec![item]);
        // Near the left end of the taper: the ring at x = 12.5 spans
        // y = 21.3..23.0, below the line from the inner corner (10, 14) to
        // the center, which used to give this area to the left side.
        let (r, _, b) = rgb(&p, 12, 22);
        assert!(
            b == 255 && r < 200,
            "taper is painted: {:?}",
            rgb(&p, 12, 22)
        );
        // Further down the left edge nothing is painted.
        assert_eq!(rgb(&p, 11, 50), (255, 255, 255));
    }

    #[test]
    fn uniform_inset_border_is_shaded() {
        let gray = Rgba::rgb(150, 150, 150);
        let p = render(vec![border([10.0; 4], [gray; 4], BorderStyle::Inset, 0.0)]);
        assert_eq!(rgb(&p, 50, 14), (100, 100, 100), "top is darker");
        assert_eq!(rgb(&p, 50, 65), (150, 150, 150), "bottom keeps the color");
    }

    #[test]
    fn double_border_has_two_lines() {
        let p = render(vec![border([9.0; 4], [BLUE; 4], BorderStyle::Double, 0.0)]);
        assert_eq!(rgb(&p, 50, 11), (0, 0, 255), "outer line");
        assert_eq!(rgb(&p, 50, 14), (255, 255, 255), "gap");
        assert_eq!(rgb(&p, 50, 17), (0, 0, 255), "inner line");
    }

    #[test]
    fn adjacent_rects_have_no_seam_at_fractional_scale() {
        let rect = |y: f32| DisplayItem::Rect {
            rect: Rect::new(0.0, y, 50.0, 13.0),
            radii: [(0.0, 0.0); 4],
            color: Rgba::BLACK,
        };
        let p = render_with(vec![rect(0.0), rect(13.0)], 1.25, &NoImages);
        for y in 0..32 {
            assert_eq!(rgb(&p, 10, y), (0, 0, 0), "row {y}");
        }
    }

    #[test]
    fn scaled_image_does_not_bleed_at_its_edges() {
        // 4x4: black, with a white bottom row.
        let mut image = Pixmap::new(4, 4).unwrap();
        image.fill(tiny_skia::Color::BLACK);
        for x in 0..4 {
            let i = (3 * 4 + x) * 4;
            image.data_mut()[i..i + 4].copy_from_slice(&[255; 4]);
        }
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let item = DisplayItem::Image {
            image: ImageRef::Url("x".into()),
            rect,
            tile: rect,
            clip: rect,
        };
        let p = render_with(
            vec![item],
            1.0,
            &OneImage(DecodedImage::from_pixmap(image), None),
        );
        assert_eq!(rgb(&p, 50, 0), (0, 0, 0));
    }

    fn svg(source: &str) -> DecodedImage {
        crate::decode_with_type(source.as_bytes(), Some(crate::SVG_MIME_TYPE)).unwrap()
    }

    fn image_item(tile: Rect, area: Rect) -> DisplayItem {
        DisplayItem::Image {
            image: ImageRef::Url("x".into()),
            rect: area,
            tile,
            clip: area,
        }
    }

    #[test]
    fn vector_images_are_sharp_at_any_scale() {
        // A 1x1 checkerboard of four squares: rendered at the device size,
        // the edge between the squares is a sharp pixel edge, also at 3x.
        let image = svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2">
            <rect width="1" height="1"/><rect x="1" y="1" width="1" height="1"/></svg>"#,
        );
        let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
        let images = OneImage(image, Some(VectorCache::default()));
        for scale in [1.0, 2.0, 3.0] {
            let p = render_with(vec![image_item(rect, rect)], scale, &images);
            let at = |x: f32, y: f32| rgb(&p, (x * scale) as u32, (y * scale) as u32);
            let edge = 20.0 * scale;
            assert_eq!(rgb(&p, edge as u32 - 1, (15.0 * scale) as u32), (0, 0, 0));
            assert_eq!(rgb(&p, edge as u32, (15.0 * scale) as u32), (255, 255, 255));
            assert_eq!(at(25.0, 25.0), (0, 0, 0));
            assert_eq!(at(5.0, 5.0), (255, 255, 255), "outside the image");
        }
    }

    #[test]
    fn vector_images_tile_at_the_tile_size() {
        // A 10x10 tile: black left half.
        let image = svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
            <rect width="2" height="4"/></svg>"#,
        );
        let tile = Rect::new(0.0, 0.0, 10.0, 10.0);
        let area = Rect::new(0.0, 0.0, 40.0, 10.0);
        let p = render_with(vec![image_item(tile, area)], 1.0, &OneImage(image, None));
        for x in [0, 4, 10, 14, 30, 34] {
            assert_eq!(rgb(&p, x, 5), (0, 0, 0), "x = {x}");
            assert_eq!(rgb(&p, x + 5, 5), (255, 255, 255), "x = {}", x + 5);
        }
        assert_eq!(rgb(&p, 45, 5), (255, 255, 255), "outside the area");
    }

    #[test]
    fn render_sizes_are_bounded() {
        let size = |w: f32, h: f32| {
            vector_render_size(Rect::new(0.0, 0.0, w, h)).map(|s| (s.width(), s.height()))
        };
        assert_eq!(size(10.4, 0.2), Some((10, 1)));
        assert_eq!(size(0.0, 10.0), None);
        assert_eq!(size(f32::NAN, 10.0), None);
        assert_eq!(size(f32::INFINITY, 10.0), None);
        for huge in [1e9, 1e30] {
            let (w, h) = size(huge, huge).unwrap();
            assert!(w * h <= MAX_RENDER_PIXELS && w == h && w > 1000, "{w}x{h}");
        }
        let (w, h) = size(1e6, 2.0).unwrap();
        assert_eq!((w, h), (MAX_DIMENSION, 1));
    }

    #[test]
    fn opacity_group_is_composited() {
        let rect = Rect::new(20.0, 20.0, 10.0, 10.0);
        let p = render(vec![
            DisplayItem::PushOpacity {
                opacity: 0.5,
                bounds: rect,
            },
            DisplayItem::Rect {
                rect,
                radii: [(0.0, 0.0); 4],
                color: RED,
            },
            DisplayItem::PopOpacity,
        ]);
        let (r, g, b) = rgb(&p, 25, 25);
        assert_eq!(r, 255);
        assert!((126..=129).contains(&g) && g == b, "{g} {b}");
        assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    }

    #[test]
    fn transparent_group_draws_nothing() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let p = render(vec![
            DisplayItem::PushOpacity {
                opacity: 0.0,
                bounds: rect,
            },
            DisplayItem::Rect {
                rect,
                radii: [(0.0, 0.0); 4],
                color: RED,
            },
            DisplayItem::PopOpacity,
        ]);
        assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    }

    #[test]
    fn clip_applies_inside_opacity_layers() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let p = render(vec![
            DisplayItem::PushClip(Rect::new(0.0, 0.0, 50.0, 100.0)),
            DisplayItem::PushOpacity {
                opacity: 0.5,
                bounds: rect,
            },
            DisplayItem::Rect {
                rect,
                radii: [(4.0, 4.0); 4],
                color: RED,
            },
            DisplayItem::PopOpacity,
            DisplayItem::PopClip,
        ]);
        assert_ne!(rgb(&p, 25, 50), (255, 255, 255));
        assert_eq!(rgb(&p, 75, 50), (255, 255, 255));
    }

    #[test]
    fn snapping_keeps_thin_rects_visible() {
        assert_eq!(
            snap(Rect::new(0.4, 1.6, 2.2, 0.3)),
            Rect::new(0.0, 2.0, 3.0, 1.0)
        );
        assert_eq!(snap(Rect::new(1.0, 1.0, 0.0, 0.0)).width, 0.0);
    }
}
