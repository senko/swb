//! An estimate of the time and memory that rendering an SVG image takes,
//! to bound both for one rendering.
//!
//! Time: the work for a rendering of `p` pixels is
//! `fixed + rows * sqrt(p) + linear * p + quadratic * p²` units; one unit is
//! about the time to fill one pixel of a path (0.3 ns on a 2025 desktop
//! CPU).
//!
//! - `linear` counts how often the content covers the canvas: every path,
//!   image and layer adds the fraction of the canvas that its bounding box
//!   covers, times [`BLEND`] if its pixels are blended (translucent paint,
//!   gradients, patterns, images, layers; opaque colors count 1), and
//!   filter primitives add their cost per pixel (blurs, lighting,
//!   convolution matrices, noise octaves).
//! - `quadratic` comes from morphology filters: resvg visits `(2r)²`
//!   pixels per pixel, and the radius in pixels grows with the rendering
//!   size.
//! - `fixed` does not depend on the size: every path and its segments,
//!   dashes (tiny-skia makes every dash a path segment), the decoding of
//!   embedded raster images, and the edges of paths that do not depend on
//!   the rows they cross (`edges.rs`: the pairs of edges that can cross make
//!   the time of a dense path grow with the square of its segments, and a
//!   lower resolution does not remove them).
//! - `rows` is the cost of the rows that the edges of paths cross. The
//!   heights of the edges are in canvas units; a rendering of `p` pixels
//!   makes them `sqrt(p / canvas)` times as tall, so the term is
//!   `rows * sqrt(p)`.
//!
//! resvg renders clip paths, masks, pattern tiles and `feImage` content
//! for every element that uses them, also when usvg shares one tree
//! between the uses; their cost counts per use but is computed once.
//!
//! Memory: resvg allocates a pixmap for every layer (groups with opacity,
//! clip paths, masks, filter results, nested SVG images, pattern tiles).
//! Layers can be up to 5 × 5 times the canvas, and nested layers exist at
//! the same time. `layers` is the largest sum of nested layer sizes, in
//! canvases.
//!
//! The weights were measured with resvg 0.48 in release builds. The
//! rasterizer uses the path weights for inline SVG too (`raster/path.rs`).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use usvg::{self, Group, Node, NonZeroRect, Paint, filter};

use super::edges::{Draw, EdgeSweep, Edges, SWEEP_SEGMENT, Segment, Window, Work};
use super::spans::{self, Grid, Rule, SCALES, SPAN, SPAN_SKIP, SpanCount};

/// The cost of one neighbor of a morphology filter.
const MORPHOLOGY: f64 = 32.0;
/// The cost of one cell of a convolution matrix.
const CONVOLUTION: f64 = 16.0;
/// The cost of one octave of noise.
const TURBULENCE: f64 = 200.0;
/// The cost of a blur or drop shadow per pixel.
const BLUR: f64 = 300.0;
/// The cost of lighting per pixel.
const LIGHTING: f64 = 120.0;
/// The cost of other filter primitives per pixel.
const OTHER_FILTER: f64 = 30.0;
/// The cost of one dash.
pub(crate) const DASH: f64 = 200.0;
/// The cost of decoding one pixel of an embedded raster image.
const RASTER_DECODE: f64 = 10.0;
/// tiny-skia makes at most this many dashes per path.
const MAX_CHARGED_DASHES: f64 = 1_000_000.0;
/// The most dash array entries (dashes and gaps) along a stroke whose
/// outline is made to count its spans, as `MAX_DASHES` in `raster/path.rs`
/// counts them; a path with more is too expensive (`MAX_REJECT`).
const MAX_OUTLINE_DASHES: f64 = 100_000.0;
/// Added to the fixed cost of what is too expensive to look at: more than
/// the render budget of `svg/mod.rs`.
const MAX_REJECT: f64 = 4.0e9;
/// resvg clips layers to 5 × 5 canvases (`max_filter_bbox`).
const MAX_LAYER: f64 = 25.0;
/// The cost of rendering a path, apart from its pixels and segments.
pub(crate) const PATH: f64 = 1500.0;
/// The cost of one segment of a path.
pub(crate) const SEGMENT: f64 = 30.0;
/// The cost of a group, apart from its layer.
const GROUP: f64 = 100.0;
/// The cost of blending one pixel: translucent paint, gradients, patterns,
/// images, layers, masks. (Measured: 1.9 ns per pixel for half-transparent
/// fills, 0.125 ns for opaque ones.)
pub(crate) const BLEND: f64 = 8.0;

/// The most work of counting the spans of the paths of one image
/// (`spans.rs`), in units: about 0.3 s, once per image. Paths beyond it are
/// charged the bound on their spans.
const MAX_COUNT_WORK: f64 = 1.0e9;

/// The most work of counting spans for all the images of one document, in
/// units: about 0.6 s. Counting at 0.22 s per image takes 7 s for 30
/// images of 250,000 segments, without this limit.
const MAX_DOCUMENT_COUNT_WORK: f64 = 2.0e9;

/// What counting spans (`spans.rs`) may still take in one document, shared
/// by its SVG images. Past it, images are charged the bound on their spans
/// without counting (so they render at a lower resolution, or not at all).
#[derive(Debug)]
pub struct CountBudget {
    left: Mutex<f64>,
}

impl Default for CountBudget {
    fn default() -> Self {
        CountBudget {
            left: Mutex::new(MAX_DOCUMENT_COUNT_WORK),
        }
    }
}

impl CountBudget {
    /// Takes up to the work of one image from the budget and returns it.
    fn start(&self) -> f64 {
        let mut left = self.left.lock().unwrap_or_else(PoisonError::into_inner);
        let taken = left.min(MAX_COUNT_WORK);
        *left -= taken;
        taken
    }

    /// Gives back the work that an image did not use.
    fn finish(&self, unused: f64) {
        let mut left = self.left.lock().unwrap_or_else(PoisonError::into_inner);
        *left += unused;
    }
}

/// The estimate for a render tree.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Cost {
    pub(super) fixed: f64,
    /// The coefficient of the square root of the pixels: the rows that the
    /// edges cross.
    rows: f64,
    linear: f64,
    quadratic: f64,
    /// The largest sum of nested layers, in canvases (the canvas itself is
    /// not counted).
    layers: f64,
    /// The work of the spans of anti-aliased fills that are narrower than a
    /// pixel (`spans.rs`), per row of the canvas, for a rendering at
    /// `spans::scale(i)` pixels per user unit: at a scale `s` from that one
    /// to the next, the work is `s` times `spans[i]` (spans only get wider
    /// at a larger scale).
    spans: [f64; SCALES],
    /// The bound on the same work, per row of the canvas, at any scale.
    span_bound: f64,
    /// The area of the canvas in user units (for the scale of a rendering).
    canvas: f64,
}

impl Cost {
    /// The largest number of pixels, at most `up_to`, whose rendering stays
    /// within `work` units and whose layers stay within `layer_pixels`, or
    /// `None` if not even one pixel does. The work of the spans can make a
    /// smaller rendering more expensive than a larger one, so the search
    /// goes down from `up_to`.
    ///
    /// A cost that is not finite (an overflow, or NaN from one) admits
    /// nothing.
    pub(super) fn max_pixels(&self, work: f64, layer_pixels: f64, up_to: f64) -> Option<f64> {
        let terms = [
            self.fixed,
            self.rows,
            self.linear,
            self.quadratic,
            self.layers,
            self.span_bound,
        ];
        if !terms.iter().chain(&self.spans).all(|t| t.is_finite()) {
            return None;
        }
        let work = work - self.fixed;
        if work.is_nan() || work <= 0.0 {
            return None;
        }
        let (a, b) = (self.quadratic, self.linear);
        let by_work = if self.rows > 0.0 || self.span_bound > 0.0 {
            self.pixels_for(work, up_to)
        } else if a > 0.0 {
            // The positive root of a p² + b p = work, in the form that
            // does not cancel; an overflow in the root gives 0, not
            // infinity.
            2.0 * work / (b + b.mul_add(b, 4.0 * a * work).sqrt())
        } else if b > 0.0 {
            work / b
        } else {
            f64::INFINITY
        };
        let by_memory = if self.layers > 0.0 {
            layer_pixels / self.layers
        } else {
            f64::INFINITY
        };
        // `f64::min` ignores NaN, so each bound is checked.
        [by_work, by_memory]
            .iter()
            .all(|p| *p >= 1.0)
            .then(|| by_work.min(by_memory).min(up_to.max(1.0)))
    }

    /// The work of a rendering of `pixels` canvas pixels.
    pub(super) fn work(&self, pixels: f64) -> f64 {
        let side = pixels.sqrt();
        self.fixed
            + self.rows * side
            + self.linear * pixels
            + self.quadratic * pixels * pixels
            + side * self.span_rows(self.piece(side))
    }

    /// The largest scale of the spans: that of the largest rendering.
    fn largest(&self) -> f64 {
        largest_scale(self.canvas)
    }

    /// The square root of the canvas: a rendering of `side²` canvas pixels
    /// has `side / root` pixels per user unit.
    fn root(&self) -> f64 {
        if self.canvas > 0.0 {
            self.canvas.sqrt()
        } else {
            1.0
        }
    }

    /// The piece of the work of the spans for a rendering of `side²` canvas
    /// pixels: 0 below `spans::scale(0)`, `i + 1` from `spans::scale(i)`.
    fn piece(&self, side: f64) -> usize {
        let s = side / self.root();
        (0..SCALES)
            .take_while(|&i| s >= spans::scale(self.largest(), i))
            .count()
    }

    /// The work of the spans in `piece`, per pixel of the side of the
    /// rendering: per row of the canvas, the bound or the count at the
    /// lowest scale of the piece (spans only get wider at a larger scale),
    /// divided by the square root of the canvas.
    fn span_rows(&self, piece: usize) -> f64 {
        let per_row = match piece {
            0 => self.span_bound,
            _ => self.spans[piece - 1],
        };
        per_row / self.root()
    }

    /// The number of pixels, at most `up_to`, whose work without the fixed
    /// part stays within `work` (positive). In each piece of the work of
    /// the spans the work grows with the size; going down from the piece of
    /// `up_to`, the first piece with a size within the work has the answer,
    /// found by bisection on the square root of the pixels. Infinite if the
    /// work does not grow.
    fn pixels_for(&self, work: f64, up_to: f64) -> f64 {
        let top = up_to.sqrt();
        for piece in (0..=self.piece(top)).rev() {
            let spans = self.span_rows(piece);
            let beyond = |side: f64| {
                side * (self.rows + spans + side * (self.linear + side * side * self.quadratic))
                    > work
            };
            let from = match piece {
                0 => 0.0,
                _ => spans::scale(self.largest(), piece - 1) * self.root(),
            };
            if beyond(from) {
                continue;
            }
            let (mut low, mut high) = (from, top);
            if high.is_infinite() {
                high = from.max(1.0);
                while !beyond(high) {
                    low = high;
                    high *= 2.0;
                    if high > 1e150 {
                        return f64::INFINITY;
                    }
                }
            } else if !beyond(high) {
                return up_to;
            }
            for _ in 0..80 {
                let middle = f64::midpoint(low, high);
                if beyond(middle) {
                    high = middle;
                } else {
                    low = middle;
                }
            }
            // Rounded down, so that the work fits.
            return low * low;
        }
        0.0
    }
}

/// Estimates the rendering of `tree`. Counting spans takes from `budget`.
pub(super) fn estimate(tree: &usvg::Tree, budget: &CountBudget) -> Cost {
    let mut count_left = budget.start();
    let cost = estimate_counting(tree, &mut count_left);
    budget.finish(count_left);
    cost
}

/// Estimates the rendering of `tree`; counting spans may take up to
/// `count_left` and reduces it.
fn estimate_counting(tree: &usvg::Tree, count_left: &mut f64) -> Cost {
    let canvas = f64::from(tree.size().width()) * f64::from(tree.size().height());
    let mut estimator = Estimator {
        canvas,
        referenced: HashMap::new(),
        edges: true,
        count_left: *count_left,
    };
    let cost = estimator.subtree(tree.root(), CANVAS);
    *count_left = estimator.count_left;
    Cost { canvas, ..cost }
}

/// The surface that a group draws into.
#[derive(Clone, Copy, Debug)]
struct Surface {
    /// The layers that exist while it draws, in canvases.
    above: f64,
    /// The size of the surface (the canvas, a layer or a pattern tile), in
    /// canvases: content cannot cover more.
    size: f64,
}

/// The canvas itself.
const CANVAS: Surface = Surface {
    above: 0.0,
    size: 1.0,
};

struct Estimator {
    /// The area of the canvas in user units.
    canvas: f64,
    /// The costs of the contents of clip paths, masks, patterns and
    /// `feImage`s, by root group. resvg renders them for every use; usvg
    /// shares one root between uses when it can.
    referenced: HashMap<*const Group, Cost>,
    /// Whether to count the edges of paths (`edges.rs`); a test switches it
    /// off to see the other terms alone.
    edges: bool,
    /// The work that counting spans may still take (`MAX_COUNT_WORK`).
    count_left: f64,
}

impl Estimator {
    /// The cost of `root` and its descendants drawn on `surface`. The walk
    /// is iterative; it recurses only into referenced content, whose depth
    /// the conversion limits.
    fn subtree(&mut self, root: &Group, surface: Surface) -> Cost {
        let mut cost = Cost::default();
        let mut groups = vec![(root, surface)];
        while let Some((group, surface)) = groups.pop() {
            self.add_group(&mut cost, &mut groups, group, surface);
        }
        cost
    }

    /// The fraction of the canvas that `rect` covers, at most `cap`.
    fn fraction(&self, rect: Option<NonZeroRect>, cap: f64) -> f64 {
        let area = rect.map_or(0.0, |r| f64::from(r.width()) * f64::from(r.height()));
        let fraction = area / self.canvas;
        if fraction.is_nan() {
            cap
        } else {
            fraction.min(cap)
        }
    }

    /// Adds the cost of `group` itself and of its paths and images; pushes
    /// its child groups.
    fn add_group<'a>(
        &mut self,
        cost: &mut Cost,
        groups: &mut Vec<(&'a Group, Surface)>,
        group: &'a Group,
        mut surface: Surface,
    ) {
        cost.fixed += GROUP;
        let layer = self.fraction(Some(group.abs_layer_bounding_box()), MAX_LAYER);
        if group.should_isolate() {
            cost.linear += BLEND * layer;
            surface = Surface {
                above: surface.above + layer,
                size: layer,
            };
        }
        self.add_filters(cost, group, layer, surface.above);
        // Clip paths and masks draw into a pixmap of the current surface's
        // size.
        let roots = [
            group.clip_path().map(usvg::ClipPath::root),
            group.mask().map(usvg::Mask::root),
        ];
        for root in roots.into_iter().flatten() {
            self.add_referenced(cost, root, surface.above, surface.size);
        }
        cost.layers = cost.layers.max(surface.above);
        for child in group.children() {
            match child {
                Node::Group(g) => groups.push((g, surface)),
                Node::Path(path) => self.add_path(cost, path, surface),
                Node::Image(image) => self.add_image(cost, image, surface),
                Node::Text(_) => {}
            }
        }
    }

    /// Adds content that resvg renders into a pixmap of `size` canvases
    /// with `above` canvases of layers above it: a clip path, mask, pattern
    /// tile or `feImage`. Its own cost is computed once per root.
    fn add_referenced(&mut self, cost: &mut Cost, root: &Group, above: f64, size: f64) {
        let key = std::ptr::from_ref(root);
        let inner = if let Some(&inner) = self.referenced.get(&key) {
            inner
        } else {
            let inner = self.subtree(root, CANVAS);
            self.referenced.insert(key, inner);
            inner
        };
        // The content was estimated on one canvas; the pixmap can be larger.
        let scale = size.max(1.0);
        cost.fixed += inner.fixed;
        cost.rows += inner.rows * scale.sqrt();
        for (outer, inner) in cost.spans.iter_mut().zip(inner.spans) {
            *outer += inner * scale.sqrt();
        }
        cost.span_bound += inner.span_bound * scale.sqrt();
        cost.linear += BLEND * size + inner.linear * scale;
        cost.quadratic += inner.quadratic * scale;
        cost.layers = cost.layers.max(above + size + inner.layers);
    }

    /// Adds an `<image>`: its area, and decoding (raster) or a nested
    /// rendering (SVG).
    fn add_image(&mut self, cost: &mut Cost, image: &usvg::Image, surface: Surface) {
        let area = image.abs_bounding_box().to_non_zero_rect();
        cost.linear += BLEND * self.fraction(area, surface.size);
        if let usvg::ImageKind::SVG(nested) = image.kind() {
            // resvg renders a nested SVG image into a layer of the whole
            // canvas.
            let inner = estimate_counting(nested, &mut self.count_left);
            cost.fixed += inner.fixed;
            cost.rows += inner.rows;
            // Its scales differ from this tree's: the largest work of its
            // spans at any of its scales.
            let most = inner.spans.iter().fold(0.0f64, |a, b| a.max(*b));
            for outer in &mut cost.spans {
                *outer += most;
            }
            cost.span_bound += inner.span_bound;
            cost.linear += BLEND + inner.linear;
            cost.quadratic += inner.quadratic;
            cost.layers = cost.layers.max(surface.above + 1.0 + inner.layers);
        } else {
            let size = image.size();
            cost.fixed += RASTER_DECODE * f64::from(size.width()) * f64::from(size.height());
        }
    }

    /// Adds the filters of a group whose layer covers `layer` canvases, with
    /// `above` canvases of layers above it.
    fn add_filters(&mut self, cost: &mut Cost, group: &Group, layer: f64, above: f64) {
        let (sx, sy) = group.abs_transform().get_scale();
        for filter in group.filters() {
            // The source, the result and one pixmap per primitive result.
            let results = 2.0 + filter.primitives().len() as f64;
            cost.layers = cost.layers.max(above + layer * results);
            for primitive in filter.primitives() {
                let per_pixel = match primitive.kind() {
                    filter::Kind::Morphology(m) => {
                        let rx = f64::from(m.radius_x().get()) * f64::from(sx);
                        let ry = f64::from(m.radius_y().get()) * f64::from(sy);
                        // Per pixel: (2 rx)(2 ry) in pixels, which grows with
                        // the rendering size; see `quadratic`.
                        cost.quadratic += layer * MORPHOLOGY * 4.0 * rx * ry / self.canvas;
                        0.0
                    }
                    filter::Kind::ConvolveMatrix(c) => {
                        let matrix = c.matrix();
                        CONVOLUTION * f64::from(matrix.columns()) * f64::from(matrix.rows())
                    }
                    filter::Kind::Turbulence(t) => TURBULENCE * f64::from(t.num_octaves()),
                    filter::Kind::GaussianBlur(_) | filter::Kind::DropShadow(_) => BLUR,
                    filter::Kind::DiffuseLighting(_) | filter::Kind::SpecularLighting(_) => {
                        LIGHTING
                    }
                    filter::Kind::Image(image) => {
                        self.add_referenced(cost, image.root(), above + layer * results, layer);
                        OTHER_FILTER
                    }
                    _ => OTHER_FILTER,
                };
                cost.linear += layer * per_pixel;
            }
        }
    }

    /// Adds the scan conversion of the edges of a path (see `edges.rs`). A
    /// stroke counts as an outline, also if it is thin enough for a
    /// hairline at some sizes: whether it is depends on the size of the
    /// rendering, and the outline costs more.
    fn add_edges(&mut self, cost: &mut Cost, path: &usvg::Path) {
        let (edges, window) = path_edges(path);
        if let Some(fill) = path.fill() {
            self.add_spans(cost, || segments(path), (&edges, window), fill.rule());
        }
        // A fill and a stroke scan-convert the path separately.
        let mut work = Work::default();
        if path.fill().is_some() {
            work = work + edges.work(true, Draw::Fill);
        }
        if let Some(stroke) = path.stroke() {
            work = work + edges.work(true, Draw::Stroke);
            self.add_dash_spans(cost, path, stroke, window);
        }
        cost.fixed += SWEEP_SEGMENT * path.data().verbs().len() as f64 + work.fixed;
        // The heights are in canvas units: `rows` pixel rows for a canvas
        // of `canvas` pixels is `sqrt(pixels / canvas)` per unit.
        cost.rows += work.rows / self.canvas.sqrt();
    }

    /// Adds the spans of the dashes of `stroke` (`spans.rs`): the outline
    /// of the dashed stroke is made with tiny-skia as in `raster/path.rs`
    /// (`dashed_span_work`), charged for its segments, and counted as a
    /// non-zero fill. A path with more than [`MAX_OUTLINE_DASHES`] dash
    /// array entries along it is too expensive.
    fn add_dash_spans(
        &mut self,
        cost: &mut Cost,
        path: &usvg::Path,
        stroke: &usvg::Stroke,
        window: Window,
    ) {
        use usvg::tiny_skia_path as sk;
        let Some(dashes) = stroke.dasharray() else {
            return;
        };
        let interval: f32 = dashes.iter().sum();
        let count = f64::from(control_polygon_length(path.data())) * dashes.len() as f64
            / f64::from(interval);
        if count > MAX_OUTLINE_DASHES {
            cost.fixed += MAX_REJECT;
            return;
        }
        let (sx, sy) = path.abs_transform().get_scale();
        let res = sx.max(sy).clamp(1.0, 16.0);
        let outline = sk::StrokeDash::new(dashes.to_vec(), stroke.dashoffset())
            .and_then(|dash| path.data().dash(&dash, res))
            .and_then(|dashed| dashed.stroke(&skia_stroke(stroke), res));
        let Some(outline) = outline else {
            return;
        };
        cost.fixed += (SEGMENT + SWEEP_SEGMENT) * outline.len() as f64;
        let transform = path.abs_transform();
        let mut sweep = EdgeSweep::new(window.0, window.1, 0.0, outline.len() + 1);
        sweep.path(transformed(&outline, transform), true);
        let edges = sweep.finish();
        self.add_spans(
            cost,
            || transformed(&outline, transform),
            (&edges, window),
            usvg::FillRule::NonZero,
        );
    }

    /// Adds the spans of the fill of `path` (`spans.rs`): the bound, and the
    /// count at each of the scales if the bound is not small at the largest
    /// rendering and the count fits into what is left of
    /// `MAX_COUNT_WORK`. Fills count as anti-aliased.
    fn add_spans<I: IntoIterator<Item = Segment>>(
        &mut self,
        cost: &mut Cost,
        segments: impl FnOnce() -> I,
        (edges, (rows, columns)): (&Edges, Window),
        rule: usvg::FillRule,
    ) {
        // In rows of the canvas: `span_work` scales them.
        let bound = SPAN * edges.span_bound;
        cost.span_bound += bound;
        let largest = largest_scale(self.canvas);
        let rule = match rule {
            usvg::FillRule::NonZero => Rule::NonZero,
            usvg::FillRule::EvenOdd => Rule::EvenOdd,
        };
        let grid = Grid::Scaled(largest_scale(self.canvas));
        let mut count = SpanCount::new(rows, columns, edges, rule, grid);
        let small = bound * largest <= SPAN_SKIP;
        let groups = if small || count.work() > self.count_left {
            None
        } else {
            self.count_left -= count.work();
            count.path(segments());
            count.finish()
        };
        for (i, work) in cost.spans.iter_mut().enumerate() {
            *work += groups.map_or(bound, |g| (SPAN * g[i]).min(bound));
        }
    }

    /// Adds a path: its outline, its area, its dashes and its pattern tiles.
    fn add_path(&mut self, cost: &mut Cost, path: &usvg::Path, surface: Surface) {
        cost.fixed += PATH + SEGMENT * path.data().verbs().len() as f64;
        if self.edges {
            self.add_edges(cost, path);
        }
        let area = path.abs_stroke_bounding_box().to_non_zero_rect();
        cost.linear += paint_weight(path) * self.fraction(area, surface.size);
        if let Some(dashes) = path.stroke().and_then(usvg::Stroke::dasharray) {
            let interval: f32 = dashes.iter().sum();
            let length = control_polygon_length(path.data());
            let count = f64::from(length) * (dashes.len() / 2) as f64 / f64::from(interval);
            cost.fixed += DASH
                * if count.is_nan() {
                    0.0
                } else {
                    count.min(MAX_CHARGED_DASHES)
                };
        }
        let paints = [
            path.fill().map(usvg::Fill::paint),
            path.stroke().map(usvg::Stroke::paint),
        ];
        for paint in paints.into_iter().flatten() {
            if let Paint::Pattern(pattern) = paint {
                // resvg renders the tile for every path, at the tile's size
                // in pixels, which is not limited by the canvas.
                let (sx, sy) = path
                    .abs_transform()
                    .pre_concat(pattern.transform())
                    .get_scale();
                let rect = pattern.rect();
                let tile = f64::from(rect.width())
                    * f64::from(sx)
                    * f64::from(rect.height())
                    * f64::from(sy)
                    / self.canvas;
                let tile = if tile.is_nan() { f64::INFINITY } else { tile };
                self.add_referenced(cost, pattern.root(), surface.above, tile);
            }
        }
    }
}

/// The scale of the largest rendering of a canvas of `canvas` square
/// units (`MAX_RENDER_PIXELS`), in pixels per unit; 1 for an empty canvas.
fn largest_scale(canvas: f64) -> f64 {
    let scale = (f64::from(super::MAX_RENDER_PIXELS) / canvas).sqrt();
    if scale.is_finite() { scale } else { 1.0 }
}

/// The edges of `path` in the units of the tree's canvas (see
/// `edges.rs`), and the window of rows and columns of the sweep.
fn path_edges(path: &usvg::Path) -> (Edges, Window) {
    let transform = path.abs_transform();
    let (sx, sy) = transform.get_scale();
    let grow = path.stroke().map_or(0.0, |s| {
        let miter = match s.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => s.miterlimit().get(),
            usvg::LineJoin::Round | usvg::LineJoin::Bevel => 1.0,
        };
        s.width().get() / 2.0 * miter.max(std::f32::consts::SQRT_2) * sx.max(sy)
    });
    let bounds = path.abs_bounding_box();
    let rows = (bounds.top() - grow, bounds.bottom() + grow);
    let columns = (bounds.left() - grow, bounds.right() + grow);
    let mut sweep = EdgeSweep::new(rows, columns, grow, path.data().verbs().len() + 1);
    sweep.path(segments(path), path.fill().is_some());
    (sweep.finish(), (rows, columns))
}

/// The stroke parameters of tiny-skia for `stroke`, without dashes.
fn skia_stroke(stroke: &usvg::Stroke) -> usvg::tiny_skia_path::Stroke {
    use usvg::tiny_skia_path as sk;
    sk::Stroke {
        width: stroke.width().get(),
        miter_limit: stroke.miterlimit().get(),
        line_cap: match stroke.linecap() {
            usvg::LineCap::Butt => sk::LineCap::Butt,
            usvg::LineCap::Round => sk::LineCap::Round,
            usvg::LineCap::Square => sk::LineCap::Square,
        },
        line_join: match stroke.linejoin() {
            usvg::LineJoin::Miter => sk::LineJoin::Miter,
            usvg::LineJoin::MiterClip => sk::LineJoin::MiterClip,
            usvg::LineJoin::Round => sk::LineJoin::Round,
            usvg::LineJoin::Bevel => sk::LineJoin::Bevel,
        },
        dash: None,
    }
}

/// The segments of `path` in the units of the tree's canvas.
fn segments(path: &usvg::Path) -> impl Iterator<Item = Segment> + '_ {
    transformed(path.data(), path.abs_transform())
}

/// The segments of `data` moved by `transform`.
fn transformed(
    data: &usvg::tiny_skia_path::Path,
    transform: usvg::Transform,
) -> impl Iterator<Item = Segment> + '_ {
    use usvg::tiny_skia_path::PathSegment as S;
    let point = move |p: usvg::tiny_skia_path::Point| {
        (
            p.x * transform.sx + p.y * transform.kx + transform.tx,
            p.x * transform.ky + p.y * transform.sy + transform.ty,
        )
    };
    data.segments().map(move |segment| match segment {
        S::MoveTo(p) => Segment::Move(point(p)),
        S::LineTo(p) => Segment::Line(point(p)),
        S::QuadTo(c, p) => Segment::Quad(point(c), point(p)),
        S::CubicTo(c1, c2, p) => Segment::Cubic(point(c1), point(c2), point(p)),
        S::Close => Segment::Close,
    })
}

/// The cost per pixel of painting a path: 1 for opaque colors (tiny-skia
/// fills them without blending), [`BLEND`] otherwise.
fn paint_weight(path: &usvg::Path) -> f64 {
    let opaque = |paint: &Paint, opacity: f32| matches!(paint, Paint::Color(_)) && opacity >= 1.0;
    let fill_opaque = path
        .fill()
        .is_none_or(|f| opaque(f.paint(), f.opacity().get()));
    let stroke_opaque = path
        .stroke()
        .is_none_or(|s| opaque(s.paint(), s.opacity().get()));
    if fill_opaque && stroke_opaque {
        1.0
    } else {
        BLEND
    }
}

/// The length of the control polygon of a path: at least the length of
/// the path.
fn control_polygon_length(path: &usvg::tiny_skia_path::Path) -> f32 {
    path.points()
        .windows(2)
        .map(|pair| (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y))
        .sum()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use super::*;

    fn tree(content: &str) -> usvg::Tree {
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">{content}</svg>"#
        );
        usvg::Tree::from_str(&source, &usvg::Options::default()).unwrap()
    }

    fn cost(content: &str) -> Cost {
        estimate(&tree(content), &CountBudget::default())
    }

    /// The estimate without the edges of paths: the area, layers and the
    /// fixed costs.
    fn cost_without_edges(content: &str) -> Cost {
        let tree = tree(content);
        let canvas = f64::from(tree.size().width()) * f64::from(tree.size().height());
        let mut estimator = Estimator {
            canvas,
            referenced: HashMap::new(),
            edges: false,
            count_left: MAX_COUNT_WORK,
        };
        estimator.subtree(tree.root(), CANVAS)
    }

    const WORK: f64 = 1e9;
    /// The render budget of `svg/mod.rs`.
    const MAX_WORK: f64 = 2e9;
    const LAYERS: f64 = 1e8;

    #[test]
    fn paths_count_by_area() {
        let full = cost_without_edges("<rect width='10' height='10'/>");
        assert!((full.linear - 1.0).abs() < 0.01, "{full:?}");
        let quarter = cost_without_edges("<rect width='5' height='5'/>");
        assert!((quarter.linear - 0.25).abs() < 0.01, "{quarter:?}");
        let many = cost_without_edges(&"<rect width='10' height='10'/>".repeat(100));
        assert!((many.linear - 100.0).abs() < 0.5, "{many:?}");
        assert_eq!(
            many.max_pixels(WORK, LAYERS, f64::INFINITY),
            Some((WORK - many.fixed) / many.linear)
        );
        // Every path costs a fixed amount too.
        assert!(many.fixed >= 100.0 * PATH, "{many:?}");
    }

    /// A path of `curves` overlapping cubic curves that reach across the
    /// whole height of the 10 x 10 canvas.
    fn dense(curves: usize) -> String {
        let mut d = String::from("M0 0");
        for i in 0..curves {
            let k = (i % 5) as f32 * 0.1;
            write!(d, "C{k} 0 {} 10 {} 5", 10.0 - k, (7 * i) % 9).unwrap();
        }
        format!("<path d='{d}'/>")
    }

    #[test]
    fn overlapping_edges_make_the_cost_quadratic() {
        let (one, two) = (cost(&dense(2000)), cost(&dense(4000)));
        // Twice the curves, four times the pairs of edges (the rest of the
        // fixed cost grows only with the curves).
        assert!(two.fixed > 3.0 * one.fixed, "{one:?} {two:?}");
        assert!(one.fixed < 1e8, "{one:?}");
        // 40,000 curves take seconds to render at any resolution: the
        // image is too expensive.
        let many = cost(&dense(40_000));
        assert!(many.fixed > 2e9, "{many:?}");
        // The same curves apart from each other are cheap: each curve
        // spans a tenth of the height.
        let mut apart = String::new();
        for i in 0..1000 {
            let y = (i % 10) as f32;
            write!(
                apart,
                "<path d='M0 {y}C2 {y} 8 {} 9 {}'/>",
                y + 1.0,
                y + 0.5
            )
            .unwrap();
        }
        let apart = cost(&apart);
        assert!(apart.fixed < two.fixed / 3.0, "{apart:?} {two:?}");
    }

    /// A generator of numbers in 0 to 10 for tests (a linear congruential
    /// generator).
    fn numbers(seed: u32) -> impl FnMut() -> f32 {
        let mut state = seed;
        move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32 * 10.0
        }
    }

    #[test]
    fn moderately_dense_images_are_affordable() {
        // 100 paths of 400 random curves each across the canvas took 0.15 s
        // inline (ADR 0023): affordable, and at a large size they get a
        // lower resolution instead of being rejected.
        let mut random = numbers(5);
        for (paths, curves) in [(100, 400), (30, 1000), (10, 2000), (3, 5000)] {
            let mut body = String::new();
            for _ in 0..paths {
                let mut d = format!("M{} {}", random(), random());
                for _ in 0..curves {
                    let c = [(); 6].map(|()| random());
                    write!(d, "C{} {} {} {} {} {}", c[0], c[1], c[2], c[3], c[4], c[5]).unwrap();
                }
                write!(body, "<path d='{d}'/>").unwrap();
            }
            let cost = cost(&body);
            assert!(cost.fixed < MAX_WORK / 2.0, "{paths}x{curves}: {cost:?}");
            // The canvas has 100 pixels.
            let at_100px = cost.max_pixels(MAX_WORK, LAYERS, f64::INFINITY).unwrap() / 100.0;
            assert!(at_100px > 1000.0, "{paths}x{curves}: {at_100px}");
            // The work at the largest size is the budget.
            let pixels = cost.max_pixels(MAX_WORK, LAYERS, f64::INFINITY).unwrap();
            assert!((cost.work(pixels) - MAX_WORK).abs() / MAX_WORK < 1e-6);
        }
    }

    #[test]
    fn a_stroked_random_walk_is_affordable() {
        let mut random = numbers(9);
        let (mut x, mut y) = (5.0f32, 5.0f32);
        let mut d = format!("M{x} {y}");
        for _ in 0..40_000 {
            x = (x + random() / 10.0 - 0.5).clamp(0.0, 10.0);
            y = (y + random() / 10.0 - 0.5).clamp(0.0, 10.0);
            write!(d, "L{x} {y}").unwrap();
        }
        let walk = cost(&format!(
            "<path d='{d}' fill='none' stroke='black' stroke-width='0.1'/>"
        ));
        assert!(
            walk.max_pixels(MAX_WORK, LAYERS, f64::INFINITY)
                .is_some_and(|p| p > 1e5),
            "{walk:?}"
        );
    }

    #[test]
    fn spans_narrower_than_a_pixel_depend_on_the_size() {
        // A comb of 500 teeth across the 10 x 10 canvas, 0.01 units wide:
        // inside a pixel up to about 100 pixels per unit.
        let mut d = String::new();
        for i in 0..500 {
            let x = i as f32 / 50.0;
            write!(d, "M{x} 0v10h0.01v-10z").unwrap();
        }
        let comb = cost(&format!("<path d='{d}'/>"));
        // At 4,000 x 4,000 pixels the teeth are 4 px wide.
        assert_eq!(comb.max_pixels(MAX_WORK, LAYERS, 1.6e7), Some(1.6e7));
        // At 1,000 x 1,000 they are 1 px wide: a smaller rendering.
        let reduced = comb.max_pixels(MAX_WORK, LAYERS, 1e6).unwrap();
        assert!(reduced < 3e5, "{reduced}");
        assert!(comb.work(reduced) <= MAX_WORK * 1.0001);
        // The same comb stroked has no spans of a fill.
        let stroked = cost(&format!("<path d='{d}' fill='none' stroke='black'/>"));
        assert_eq!(stroked.span_bound, 0.0);
    }

    #[test]
    fn thin_dashes_of_wide_strokes_are_charged_as_spans() {
        let stroke = |width: f32, dash: f32| {
            let path = format!(
                "<path d='M0 5H10' fill='none' stroke='black' stroke-width='{width}' \
                 stroke-dasharray='{dash} {dash}'/>"
            );
            cost(&path)
        };
        // Dashes of 0.002 units on a canvas of 10 units are narrower than a
        // pixel up to 500 pixels per unit: a comb.
        let thin = stroke(10.0, 0.002);
        assert!(thin.span_bound > 1e6, "{thin:?}");
        let reduced = thin.max_pixels(MAX_WORK, LAYERS, 1.6e7).unwrap();
        assert!(reduced < 1.6e7, "{reduced}");
        // Dashes of 1 unit, or a stroke that is a hairline, are not.
        assert!(stroke(10.0, 1.0).span_bound < 1e4);
        assert_eq!(
            stroke(0.0001, 0.002).max_pixels(MAX_WORK, LAYERS, 1.6e7),
            Some(1.6e7)
        );
        // More dashes than the limit: too expensive.
        assert!(stroke(1.0, 1e-5).fixed > MAX_WORK);
    }

    #[test]
    fn counting_spans_has_a_budget_per_document() {
        // A filled walk of 40,000 segments: the bound on its spans is
        // large, so the count runs.
        let mut d = String::from("M0 5");
        let mut state = 3u32;
        for i in 0..40_000 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            write!(
                d,
                "L{} {}",
                i as f32 / 4_000.0,
                4.0 + (state >> 24) as f32 / 128.0
            )
            .unwrap();
        }
        let image = tree(&format!("<path d='{d}z'/>"));
        let left = |budget: &CountBudget| *budget.left.lock().unwrap();
        let budget = CountBudget::default();
        let counted = estimate(&image, &budget);
        let used = MAX_DOCUMENT_COUNT_WORK - left(&budget);
        assert!(used > 1e6, "{used}");
        // The images that follow count until the budget is gone, never
        // beyond it; then they are charged the bound.
        let mut last = counted;
        for _ in 0..(MAX_DOCUMENT_COUNT_WORK / used) as usize + 2 {
            last = estimate(&image, &budget);
            assert!(left(&budget) >= 0.0);
        }
        assert!(left(&budget) < used);
        let work = |cost: &Cost| cost.spans.iter().sum::<f64>();
        assert!(work(&last) > work(&counted), "{last:?} {counted:?}");
        assert!((work(&last) - SCALES as f64 * last.span_bound).abs() < 1e-3 * work(&last));
    }

    #[test]
    fn referenced_content_counts_per_use() {
        let clip = format!(
            "<clipPath id='c'>{}</clipPath>{}",
            "<rect width='1' height='1'/>".repeat(100),
            "<rect width='10' height='10' clip-path='url(#c)'/>".repeat(100)
        );
        let shared = cost(&clip);
        assert!(shared.fixed >= 100.0 * 100.0 * PATH, "{shared:?}");
    }

    #[test]
    fn morphology_grows_with_the_square() {
        let morphology = cost(
            "<filter id='f'><feMorphology radius='5'/></filter>
             <rect width='10' height='10' filter='url(#f)'/>",
        );
        assert!(morphology.quadratic > 0.0, "{morphology:?}");
        let pixels = morphology.max_pixels(WORK, LAYERS, f64::INFINITY).unwrap();
        assert!((morphology.work(pixels) - WORK).abs() / WORK < 1e-6);
    }

    #[test]
    fn noise_octaves_count() {
        let noise = cost(
            "<filter id='f'><feTurbulence baseFrequency='0.1' numOctaves='1000000'/></filter>
             <rect width='10' height='10' filter='url(#f)'/>",
        );
        assert!(noise.linear > 1e6, "{noise:?}");
    }

    #[test]
    fn nested_layers_add_up() {
        let open = "<g opacity='0.5'>".repeat(20);
        let close = "</g>".repeat(20);
        let layers = cost(&format!(
            "{open}<rect x='-20' y='-20' width='50' height='50'/>{close}"
        ));
        assert!(layers.layers >= 20.0 * 24.0, "{layers:?}");
        let pixels = layers.max_pixels(WORK, LAYERS, f64::INFINITY).unwrap();
        assert!(pixels * layers.layers <= LAYERS * 1.0001);
    }

    #[test]
    fn dashes_and_pattern_tiles_count() {
        let dashes = cost("<path d='M0 0 L10 10' stroke='black' stroke-dasharray='0.00002'/>");
        assert!(dashes.fixed >= DASH * 300_000.0, "{dashes:?}");
        let pattern = cost(
            "<pattern id='p' width='6000' height='6000' patternUnits='userSpaceOnUse'>
               <rect width='1' height='1'/></pattern>
             <rect width='10' height='10' fill='url(#p)'/>",
        );
        assert!(pattern.layers > 100_000.0, "{pattern:?}");
        assert_eq!(pattern.max_pixels(WORK, 1e5, f64::INFINITY), None);
    }

    #[test]
    fn costs_that_are_not_finite_admit_nothing() {
        let base = Cost {
            fixed: 0.0,
            rows: 0.0,
            linear: 1.0,
            quadratic: 0.0,
            layers: 0.0,
            ..Cost::default()
        };
        assert_eq!(base.max_pixels(WORK, LAYERS, f64::INFINITY), Some(WORK));
        let bad = [
            Cost {
                quadratic: f64::INFINITY,
                ..base
            },
            Cost {
                quadratic: f64::NAN,
                ..base
            },
            Cost {
                linear: f64::INFINITY,
                ..base
            },
            Cost {
                linear: f64::NAN,
                ..base
            },
            Cost {
                fixed: f64::NAN,
                ..base
            },
            Cost {
                layers: f64::NAN,
                ..base
            },
            Cost {
                layers: f64::INFINITY,
                ..base
            },
            // Finite, but the root overflows.
            Cost {
                linear: 1e200,
                quadratic: 1e300,
                ..base
            },
        ];
        for cost in bad {
            assert_eq!(
                cost.max_pixels(WORK, LAYERS, f64::INFINITY),
                None,
                "{cost:?}"
            );
        }
        // A large but finite quadratic term gives a small size.
        let steep = Cost {
            quadratic: 1.0,
            ..base
        };
        let pixels = steep.max_pixels(WORK, LAYERS, f64::INFINITY).unwrap();
        assert!((steep.work(pixels) - WORK).abs() / WORK < 1e-9, "{pixels}");
    }
}
