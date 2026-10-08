//! Rasterization of the path items of inline SVG (ADR 0023) with tiny-skia,
//! within a work budget per strip.
//!
//! The estimate uses the weights of `crate::path_cost` (the cost model that
//! the estimate for SVG images shares, measured with tiny-skia): a fixed
//! cost per path and per segment,
//! the device pixels that the path's bounds cover inside the visible area, a
//! cost per dash, and the cost of the edges (`crate::path_cost::edges`: the rows
//! that they cross and the pairs of edges whose boxes overlap, so a dense
//! path takes time in proportion to the square of its segments; a stroke of
//! one pixel or less takes time in proportion to its length), and for an
//! anti-aliased fill the groups of spans narrower than a pixel
//! (`crate::path_cost::spans`).
//!
//! Opaque and translucent pixels cost the same: tiny-skia's anti-aliased
//! path fill takes 2.6 ns per pixel of either, measured on the hostile-page
//! machine (release build, 100 and 200 viewport-size rects, circles, rotated
//! rects and wide strokes: 1.5 to 2.7 ms per 1280 x 800 viewport). The
//! weight for opaque pixels in `crate::path_cost` (1 unit, about 0.3 ns,
//! rounded up from 0.125 ns measured) is for resvg's cached rendering of
//! images and is too low here.
//!
//! SVG images are rendered once into a cache at a resolution that their
//! estimate allows. Inline paths are drawn directly for every frame, so a
//! path whose work does not fit into the rest of the strip's budget is not
//! drawn instead. A stroke with more than [`MAX_DASHES`] dashes is drawn
//! solid.

use std::sync::Arc;

use swb_layout::bezier::at_f32;
use swb_layout::svg::{StrokeStyle, SvgPath};
use swb_layout::{Matrix, Point, Rect};
use swb_style::{FillRule, Rgba, StrokeLinecap, StrokeLinejoin};
use tiny_skia::{LineCap, LineJoin, Path, PathBuilder, Stroke, StrokeDash, Transform};

use super::{LayerKind, NewLayer, Rasterizer, SvgClipLayer, solid_paint};
use crate::display_list::{DisplayItem, path_bounds, stroke_extent};
use crate::path_cost::edges::{Draw, EdgeSweep, Segment, flat_steps};
use crate::path_cost::spans::{self, Rule};
use crate::path_cost::{BLEND, DASH, MAX_DASHES, PATH, SEGMENT, dash_outline, outline_charge};

/// The path work of one strip (the default of `Budget::max_path_work`),
/// in the units of `crate::path_cost` (about 0.3 ns each): about 0.6 s.
/// The 459 icon paths of Ars Technica need about 0.1 % of it.
pub(super) const MAX_PATH_WORK: f64 = 2.0e9;

/// The segments of the outline of a stroke per segment of its path.
const STROKE_SEGMENTS: f64 = 4.0;

/// How tiny-skia scan-converts a path: what the work of the edges depends
/// on.
#[derive(Clone, Copy, Debug)]
pub(super) struct Scan {
    /// Whether the edges are anti-aliased.
    pub(super) anti_alias: bool,
    /// The fill rule of a fill (the spans of an anti-aliased fill have a
    /// cost of their own, `crate::path_cost::spans`).
    rule: Option<Rule>,
    /// The width of the stroke, in path units, if the path is stroked (its
    /// outline has more edges).
    stroke_width: Option<f32>,
    /// How far the outline reaches beyond the points of the path, in path
    /// units.
    pub(super) grow: f32,
}

impl Scan {
    /// A fill with `rule`.
    pub(super) fn fill(anti_alias: bool, rule: FillRule) -> Scan {
        Scan {
            anti_alias,
            rule: Some(rule.into()),
            stroke_width: None,
            grow: 0.0,
        }
    }

    /// A stroke of `width` whose outline reaches `grow` beyond the path.
    fn stroke(anti_alias: bool, width: f32, grow: f32) -> Scan {
        Scan {
            anti_alias,
            rule: None,
            stroke_width: Some(width),
            grow,
        }
    }
}

impl Rasterizer<'_> {
    /// Draws an item of inline SVG content: a fill, a stroke, or the start
    /// of a clip group.
    pub(super) fn svg_item(&mut self, item: &DisplayItem) {
        match item {
            DisplayItem::FillPath {
                path,
                transform,
                color,
                rule,
                anti_alias,
            } => self.fill_svg_path(path, transform, (*color, *anti_alias), *rule),
            DisplayItem::StrokePath {
                path,
                transform,
                color,
                stroke,
                anti_alias,
            } => self.stroke_svg_path(path, transform, (*color, *anti_alias), stroke),
            DisplayItem::PushSvgClip {
                clip,
                transform,
                bounds,
                rect_fallback,
            } => {
                let kind = LayerKind::SvgClip(SvgClipLayer {
                    clip: Arc::clone(clip),
                    transform: *transform,
                    rect_fallback: *rect_fallback,
                });
                self.push_layer(NewLayer::new(*bounds, kind));
            }
            _ => {}
        }
    }

    /// Fills `path` (drawn with `transform`, list coordinates).
    pub(super) fn fill_svg_path(
        &mut self,
        path: &SvgPath,
        transform: &Matrix,
        (color, anti_alias): (Rgba, bool),
        rule: FillRule,
    ) {
        let Some((device, bounds, work)) =
            self.plan_path(path, transform, color, Scan::fill(anti_alias, rule))
        else {
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
        let rule = skia_rule(rule);
        let paint = solid_paint(color, anti_alias);
        if !anti_alias {
            // Without anti-aliasing, a pixel is in if its center is
            // inside the outline (what Chromium does: a circle covers
            // exactly the pixels whose centers are inside). tiny-skia
            // cuts curves coarsely in this mode and loses edge pixels, so
            // the curves are cut into lines here.
            let Some(flat) = flat_device_path(path, &device) else {
                return;
            };
            self.draw(bounds, |p, t, m| {
                p.fill_path(&flat, &paint, rule, t, m);
            });
            return;
        }
        let Some(sk_path) = skia_path(path) else {
            return;
        };
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
        (color, anti_alias): (Rgba, bool),
        stroke: &StrokeStyle,
    ) {
        let scan = Scan::stroke(anti_alias, stroke.width, stroke_extent(stroke));
        let Some((device, bounds, mut work)) = self.plan_path(path, transform, color, scan) else {
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
        if let Some(dash) = &dash {
            // The dashes are separate shapes: charge the spans of the
            // outline, and the work of making it.
            let window = self.visible_area().and_then(|v| v.intersection(&bounds));
            let room = self.budget.max_path_work - self.budget.path_work;
            let extra =
                window.map(|v| dashed_span_work(&sk_path, (stroke, dash), &device, v, room));
            if !self.path_work_fits(extra.unwrap_or(0.0)) {
                return;
            }
        }
        let sk_stroke = skia_stroke(stroke, dash);
        let paint = solid_paint(color, anti_alias);
        let ts = skia_transform(&device);
        self.draw(bounds, |p, t, m| {
            p.stroke_path(&sk_path, &paint, &sk_stroke, t.pre_concat(ts), m);
        });
    }

    /// The transform from the path to target device px, the device bounds
    /// of what the path draws (with `scan.grow` around its points, in path
    /// units), and the work of a fill; `None` if nothing is visible, the
    /// geometry is not finite or the work does not fit. The cost of looking
    /// at the segments is charged at once, also for a path that does not
    /// fit, so that many references to a path that is too expensive cannot
    /// repeat the estimate for free.
    fn plan_path(
        &mut self,
        path: &SvgPath,
        transform: &Matrix,
        color: Rgba,
        scan: Scan,
    ) -> Option<(Matrix, Rect, f64)> {
        if color.is_transparent() {
            return None;
        }
        let device = self.device_matrix(transform);
        if !device.is_finite() {
            return None;
        }
        let bounds = path_bounds(path, &device, scan.grow);
        if ![bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        let visible = self.visible_area()?.intersection(&bounds)?;
        let rows = (visible.y, visible.y + visible.height);
        let columns = (visible.x, visible.x + visible.width);
        let work = self.fill_work(path, &device, visible, (rows, columns), scan)?;
        Some((device, bounds, work))
    }

    /// The work of a path with `visible` device px of area (its bounds
    /// inside the visible area), whose edges are counted inside the `rows`
    /// and `columns` of the target: the path, its pixels and the edges. The
    /// cost of looking at the segments is charged at once, also for a path
    /// that does not fit, so that many references to a path that is too
    /// expensive cannot repeat the estimate for free; `None` if even that
    /// does not fit. The rest is returned, not charged.
    pub(super) fn fill_work(
        &mut self,
        path: &SvgPath,
        device: &Matrix,
        visible: Rect,
        window: ((f32, f32), (f32, f32)),
        scan: Scan,
    ) -> Option<f64> {
        let pixels = f64::from(visible.width) * f64::from(visible.height);
        if !self.path_work_fits(outline_charge(path.segments().len())) {
            return None;
        }
        let base = PATH + BLEND * pixels;
        let room = self.budget.max_path_work - self.budget.path_work - base;
        let edges = edge_work(path, device, window, scan, room);
        Some(base + edges)
    }

    /// The matrix from list coordinates through `m` to target device px.
    pub(super) fn device_matrix(&self, m: &Matrix) -> Matrix {
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
    pub(super) fn path_work_fits(&mut self, work: f64) -> bool {
        let total = self.budget.path_work + work;
        if total.is_nan() || total > self.budget.max_path_work {
            self.budget.skipped.paths = true;
            return false;
        }
        self.budget.path_work = total;
        true
    }
}

/// The work of tiny-skia's scan conversion of `path` drawn with `device`,
/// inside the `rows` and `columns` of the target: the edges' rows and the
/// pairs of edges whose boxes overlap (see [`crate::path_cost::edges`]), and for
/// an anti-aliased fill the spans narrower than a pixel
/// ([`crate::path_cost::spans`]). Linear in the number of segments. The spans are
/// counted only if the count fits into `room` (the rest of the budget).
pub(super) fn edge_work(
    path: &SvgPath,
    device: &Matrix,
    (rows, columns): ((f32, f32), (f32, f32)),
    scan: Scan,
    room: f64,
) -> f64 {
    use swb_layout::svg::PathSegment as S;
    let scale = matrix_scale(device);
    // A stroke of one device pixel or less is a hairline: it has no outline.
    let draw = match scan.stroke_width {
        None => Draw::Fill,
        Some(width) if width * scale <= 1.0 => Draw::Hairline,
        Some(_) => Draw::Stroke,
    };
    let grow = if draw == Draw::Hairline {
        0.0
    } else {
        scan.grow * scale
    };
    let mut sweep = EdgeSweep::new(rows, columns, grow, path.segments().len() + 1);
    let point = |p: Point| {
        let q = device.apply(p);
        (q.x, q.y)
    };
    let segments = || {
        path.segments().iter().map(move |segment| match *segment {
            S::MoveTo(p) => Segment::Move(point(p)),
            S::LineTo(p) => Segment::Line(point(p)),
            S::QuadTo(c, p) => Segment::Quad(point(c), point(p)),
            S::CubicTo(c1, c2, p) => Segment::Cubic(point(c1), point(c2), point(p)),
            S::Close => Segment::Close,
        })
    };
    sweep.path(segments(), draw == Draw::Fill);
    let edges = sweep.finish();
    let work = edges.work(scan.anti_alias, draw).total();
    match scan.rule {
        // Without anti-aliasing, the spans cost nothing extra (measured).
        Some(rule) if scan.anti_alias => {
            let room = room - work;
            work + spans::device_work(&edges, (rows, columns), rule, room, segments)
        }
        _ => work,
    }
}

/// The stroke parameters of tiny-skia for `stroke` (with `dash`).
fn skia_stroke(stroke: &StrokeStyle, dash: Option<StrokeDash>) -> Stroke {
    Stroke {
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
    }
}

/// The largest scale of `device`, to turn path units into device px.
fn matrix_scale(device: &Matrix) -> f32 {
    device.a.hypot(device.b).max(device.c.hypot(device.d))
}

/// The work of the dashes of a stroke as separate spans: the dashes of
/// `path` are made into the outline of the stroke (as tiny-skia does when
/// it draws), and the outline is charged as a non-zero fill, with the work
/// of making it. Dashes are the one way that a stroke of more than one
/// pixel has spans narrower than a pixel (measured: parallel strokes
/// wider than a pixel, with gaps of 0.15 px, 800 to 900 of them, take
/// 40 ms; a dashed line 780 wide with dashes of 0.2 px, 6 s). A hairline
/// has no outline and is not charged. `window` is the visible part of the
/// stroke in device px; the result is infinite if it does not fit into
/// `room`.
fn dashed_span_work(
    path: &Path,
    (stroke, dash): (&StrokeStyle, &StrokeDash),
    device: &Matrix,
    window: Rect,
    room: f64,
) -> f64 {
    let scale = matrix_scale(device);
    if stroke.width * scale <= 1.0 {
        return 0.0;
    }
    // The curves of the outline do not need more detail than this.
    let outline = dash_outline(path, dash, &skia_stroke(stroke, None), scale);
    let Some(outline) = outline else {
        return 0.0;
    };
    let made = outline_charge(outline.len());
    if made > room {
        return f64::INFINITY;
    }
    let point = |p: tiny_skia::Point| {
        let q = device.apply(Point { x: p.x, y: p.y });
        (q.x, q.y)
    };
    let segments = || {
        outline
            .segments()
            .map(move |segment| Segment::from_skia(segment, point))
    };
    let rows = (window.y, window.y + window.height);
    let columns = (window.x, window.x + window.width);
    let mut sweep = EdgeSweep::new(rows, columns, 0.0, outline.len() + 1);
    sweep.path(segments(), true);
    let edges = sweep.finish();
    made + spans::device_work(
        &edges,
        (rows, columns),
        Rule::NonZero,
        room - made,
        segments,
    )
}

/// The path as a tiny-skia path, or `None` if tiny-skia rejects it.
pub(super) fn skia_path(path: &SvgPath) -> Option<Path> {
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

/// The path in device px (`device` maps its coordinates there), with each
/// curve cut into lines a pixel or less long (see [`flat_steps`]).
fn flat_device_path(path: &SvgPath, device: &Matrix) -> Option<Path> {
    use swb_layout::svg::PathSegment as S;
    let mut pb = PathBuilder::with_capacity(path.segments().len() * 4, path.segments().len() * 8);
    let mut current = Point::default();
    let steps = |points: &[Point]| {
        let mut xy = [(0.0, 0.0); 4];
        for (slot, p) in xy.iter_mut().zip(points) {
            *slot = (p.x, p.y);
        }
        flat_steps(&xy[..points.len().min(4)])
    };
    for segment in path.segments() {
        match *segment {
            S::MoveTo(p) => {
                current = device.apply(p);
                pb.move_to(current.x, current.y);
            }
            S::LineTo(p) => {
                current = device.apply(p);
                pb.line_to(current.x, current.y);
            }
            S::QuadTo(c, p) => {
                let (c, p) = (device.apply(c), device.apply(p));
                let n = steps(&[current, c, p]);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    pb.line_to(
                        at_f32(&[current.x, c.x, p.x], t),
                        at_f32(&[current.y, c.y, p.y], t),
                    );
                }
                current = p;
            }
            S::CubicTo(c1, c2, p) => {
                let (c1, c2, p) = (device.apply(c1), device.apply(c2), device.apply(p));
                let n = steps(&[current, c1, c2, p]);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    pb.line_to(
                        at_f32(&[current.x, c1.x, c2.x, p.x], t),
                        at_f32(&[current.y, c1.y, c2.y, p.y], t),
                    );
                }
                current = p;
            }
            S::Close => pb.close(),
        }
    }
    pb.finish()
}

/// The tiny-skia fill rule.
pub(super) fn skia_rule(rule: FillRule) -> tiny_skia::FillRule {
    match rule {
        FillRule::NonZero => tiny_skia::FillRule::Winding,
        FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
    }
}

pub(super) fn skia_transform(m: &Matrix) -> Transform {
    Transform::from_row(m.a, m.b, m.c, m.d, m.e, m.f)
}
