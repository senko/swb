//! Rasterization of the path items of inline SVG (ADR 0023) with tiny-skia,
//! within a work budget per strip.
//!
//! The estimate uses the weights of `crate::svg::cost` (the estimate for
//! SVG images, measured with tiny-skia): a fixed cost per path and per
//! segment, the device pixels that the path's bounds cover inside the
//! visible area, and a cost per dash. Opaque and translucent pixels cost
//! the same: tiny-skia's anti-aliased path fill takes 2.6 ns per pixel of
//! either, measured on the hostile-page machine (release build, 100 and
//! 200 viewport-size rects, circles, rotated rects and wide strokes: 1.5
//! to 2.7 ms per 1280 x 800 viewport). The weight for opaque pixels in
//! `crate::svg::cost` (0.125 ns) is for resvg's cached rendering of
//! images and is too low here. SVG images are rendered once into a cache at a
//! resolution that their estimate allows; inline paths are drawn
//! directly for every frame, so a path whose work does not fit into the
//! rest of the strip's budget is not drawn instead. A stroke with more
//! than [`MAX_DASHES`] dashes is drawn solid.

use swb_layout::svg::{StrokeStyle, SvgPath};
use swb_layout::{Matrix, Rect};
use swb_style::{FillRule, Rgba, StrokeLinecap, StrokeLinejoin};
use tiny_skia::{LineCap, LineJoin, Path, PathBuilder, Stroke, StrokeDash, Transform};

use super::{Rasterizer, solid_paint};
use crate::display_list::{path_bounds, stroke_extent};
use crate::svg::cost::{BLEND, DASH, PATH, SEGMENT};

/// The path work of one strip (the default of `Budget::max_path_work`),
/// in the units of `crate::svg::cost` (about 0.3 ns each): about 0.6 s.
/// The 459 icon paths of Ars Technica need about 0.1 % of it.
pub(super) const MAX_PATH_WORK: f64 = 2.0e9;

/// The most dashes of one stroke (an estimate from the length of the
/// path's control polygon). tiny-skia refuses more than a million.
pub(super) const MAX_DASHES: f32 = 100_000.0;

/// The segments of the outline of a stroke per segment of its path.
const STROKE_SEGMENTS: f64 = 4.0;

impl Rasterizer<'_> {
    /// Fills `path` (drawn with `transform`, list coordinates).
    pub(super) fn fill_svg_path(
        &mut self,
        path: &SvgPath,
        transform: &Matrix,
        color: Rgba,
        rule: FillRule,
    ) {
        let Some((device, bounds, work)) = self.plan_path(path, transform, color, 0.0) else {
            return;
        };
        // A fill without area draws nothing (and tiny-skia logs a warning).
        let area = device.map_rect(&path.bounds());
        if area.width < 1e-6 || area.height < 1e-6 {
            return;
        }
        if !self.path_work_fits(work) {
            return;
        }
        let Some(sk_path) = skia_path(path) else {
            return;
        };
        let rule = match rule {
            FillRule::NonZero => tiny_skia::FillRule::Winding,
            FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
        };
        let paint = solid_paint(color, true);
        let ts = skia_transform(&device);
        self.draw(bounds, |p, t, m| {
            p.fill_path(&sk_path, &paint, rule, t.pre_concat(ts), m);
        });
    }

    /// Strokes `path` (drawn with `transform`, list coordinates).
    pub(super) fn stroke_svg_path(
        &mut self,
        path: &SvgPath,
        transform: &Matrix,
        color: Rgba,
        stroke: &StrokeStyle,
    ) {
        let Some((device, bounds, mut work)) =
            self.plan_path(path, transform, color, stroke_extent(stroke))
        else {
            return;
        };
        work += SEGMENT * (STROKE_SEGMENTS - 1.0) * path.segments().len() as f64;
        let dash = stroke.dashes.as_ref().and_then(|dashes| {
            let period: f32 = dashes.iter().sum();
            let count = path.length_bound() / period * dashes.len() as f32;
            if !(count.is_finite() && count <= MAX_DASHES) {
                self.budget.skipped.dashes = true;
                return None;
            }
            work += DASH * f64::from(count);
            StrokeDash::new(dashes.to_vec(), stroke.dash_offset)
        });
        if !self.path_work_fits(work) {
            return;
        }
        let Some(sk_path) = skia_path(path) else {
            return;
        };
        let sk_stroke = Stroke {
            width: stroke.width,
            miter_limit: stroke.miter_limit,
            line_cap: match stroke.cap {
                StrokeLinecap::Butt => LineCap::Butt,
                StrokeLinecap::Round => LineCap::Round,
                StrokeLinecap::Square => LineCap::Square,
            },
            line_join: match stroke.join {
                StrokeLinejoin::Miter => LineJoin::Miter,
                StrokeLinejoin::Round => LineJoin::Round,
                StrokeLinejoin::Bevel => LineJoin::Bevel,
            },
            dash,
        };
        let paint = solid_paint(color, true);
        let ts = skia_transform(&device);
        self.draw(bounds, |p, t, m| {
            p.stroke_path(&sk_path, &paint, &sk_stroke, t.pre_concat(ts), m);
        });
    }

    /// The transform from the path to target device px, the device bounds
    /// of what the path draws (with `grow` around its points, in path
    /// units), and the work of a fill; `None` if nothing is visible or the
    /// geometry is not finite.
    fn plan_path(
        &self,
        path: &SvgPath,
        transform: &Matrix,
        color: Rgba,
        grow: f32,
    ) -> Option<(Matrix, Rect, f64)> {
        if color.is_transparent() {
            return None;
        }
        let device = self.device_matrix(transform);
        if !device.is_finite() {
            return None;
        }
        let bounds = path_bounds(path, &device, grow);
        if ![bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        let visible = self.visible_area()?.intersection(&bounds)?;
        let pixels = f64::from(visible.width) * f64::from(visible.height);
        let work = PATH + SEGMENT * path.segments().len() as f64 + BLEND * pixels;
        Some((device, bounds, work))
    }

    /// The matrix from list coordinates through `m` to target device px.
    fn device_matrix(&self, m: &Matrix) -> Matrix {
        let s = self.params.scale;
        let to_device = Matrix::new(
            s,
            0.0,
            0.0,
            s,
            (self.translation.x - self.params.scroll.x) * s,
            (self.translation.y - self.params.scroll.y) * s - self.offset_y,
        );
        to_device.multiply(m)
    }

    /// True if `work` fits into the rest of the path budget of the strip
    /// (and charges it). A path that does not fit is not drawn.
    fn path_work_fits(&mut self, work: f64) -> bool {
        let total = self.budget.path_work + work;
        if total.is_nan() || total > self.budget.max_path_work {
            self.budget.skipped.paths = true;
            return false;
        }
        self.budget.path_work = total;
        true
    }
}

/// The path as a tiny-skia path, or `None` if tiny-skia rejects it.
fn skia_path(path: &SvgPath) -> Option<Path> {
    use swb_layout::svg::PathSegment as S;
    let mut pb = PathBuilder::with_capacity(path.segments().len(), path.segments().len() * 3);
    for segment in path.segments() {
        match *segment {
            S::MoveTo(p) => pb.move_to(p.x, p.y),
            S::LineTo(p) => pb.line_to(p.x, p.y),
            S::QuadTo(c, p) => pb.quad_to(c.x, c.y, p.x, p.y),
            S::CubicTo(c1, c2, p) => pb.cubic_to(c1.x, c1.y, c2.x, c2.y, p.x, p.y),
            S::Close => pb.close(),
        }
    }
    pb.finish()
}

fn skia_transform(m: &Matrix) -> Transform {
    Transform::from_row(m.a, m.b, m.c, m.d, m.e, m.f)
}
