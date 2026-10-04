//! An estimate of the time and memory that rendering an SVG image takes,
//! to bound both for one rendering.
//!
//! Time: the work for a rendering of `p` pixels is
//! `fixed + linear * p + quadratic * p²` units; one unit is about the time
//! to fill one pixel of a path (0.3 ns on a 2025 desktop CPU).
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
//!   dashes (tiny-skia makes every dash a path segment) and the decoding of
//!   embedded raster images.
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
//! The weights were measured with resvg 0.48 in release builds.

use std::collections::HashMap;

use usvg::{self, Group, Node, NonZeroRect, Paint, filter};

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
const DASH: f64 = 200.0;
/// The cost of decoding one pixel of an embedded raster image.
const RASTER_DECODE: f64 = 10.0;
/// tiny-skia makes at most this many dashes per path.
const MAX_DASHES: f64 = 1_000_000.0;
/// resvg clips layers to 5 × 5 canvases (`max_filter_bbox`).
const MAX_LAYER: f64 = 25.0;
/// The cost of rendering a path, apart from its pixels and segments.
const PATH: f64 = 1500.0;
/// The cost of one segment of a path.
const SEGMENT: f64 = 30.0;
/// The cost of a group, apart from its layer.
const GROUP: f64 = 100.0;
/// The cost of blending one pixel: translucent paint, gradients, patterns,
/// images, layers, masks. (Measured: 1.9 ns per pixel for half-transparent
/// fills, 0.125 ns for opaque ones.)
const BLEND: f64 = 8.0;

/// The estimate for a render tree.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Cost {
    pub(super) fixed: f64,
    linear: f64,
    quadratic: f64,
    /// The largest sum of nested layers, in canvases (the canvas itself is
    /// not counted).
    layers: f64,
}

impl Cost {
    /// The largest number of pixels whose rendering stays within `work`
    /// units and whose layers stay within `layer_pixels`, or `None` if not
    /// even one pixel does.
    /// A cost that is not finite (an overflow, or NaN from one) admits
    /// nothing.
    pub(super) fn max_pixels(&self, work: f64, layer_pixels: f64) -> Option<f64> {
        let terms = [self.fixed, self.linear, self.quadratic, self.layers];
        if !terms.iter().all(|t| t.is_finite()) {
            return None;
        }
        let work = work - self.fixed;
        if work.is_nan() || work <= 0.0 {
            return None;
        }
        let (a, b) = (self.quadratic, self.linear);
        let by_work = if a > 0.0 {
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
            .then(|| by_work.min(by_memory))
    }

    /// The work of a rendering of `pixels` canvas pixels.
    pub(super) fn work(&self, pixels: f64) -> f64 {
        self.fixed + self.linear * pixels + self.quadratic * pixels * pixels
    }
}

/// Estimates the rendering of `tree`.
pub(super) fn estimate(tree: &usvg::Tree) -> Cost {
    let canvas = f64::from(tree.size().width()) * f64::from(tree.size().height());
    let mut estimator = Estimator {
        canvas,
        referenced: HashMap::new(),
    };
    estimator.subtree(tree.root(), CANVAS)
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
        cost.linear += BLEND * size + inner.linear * scale;
        cost.quadratic += inner.quadratic * scale;
        cost.layers = cost.layers.max(above + size + inner.layers);
    }

    /// Adds an `<image>`: its area, and decoding (raster) or a nested
    /// rendering (SVG).
    fn add_image(&self, cost: &mut Cost, image: &usvg::Image, surface: Surface) {
        let area = image.abs_bounding_box().to_non_zero_rect();
        cost.linear += BLEND * self.fraction(area, surface.size);
        if let usvg::ImageKind::SVG(nested) = image.kind() {
            // resvg renders a nested SVG image into a layer of the whole
            // canvas.
            let inner = estimate(nested);
            cost.fixed += inner.fixed;
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

    /// Adds a path: its outline, its area, its dashes and its pattern tiles.
    fn add_path(&mut self, cost: &mut Cost, path: &usvg::Path, surface: Surface) {
        cost.fixed += PATH + SEGMENT * path.data().verbs().len() as f64;
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
                    count.min(MAX_DASHES)
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
    use super::*;

    fn cost(content: &str) -> Cost {
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">{content}</svg>"#
        );
        estimate(&usvg::Tree::from_str(&source, &usvg::Options::default()).unwrap())
    }

    const WORK: f64 = 1e9;
    const LAYERS: f64 = 1e8;

    #[test]
    fn paths_count_by_area() {
        let full = cost("<rect width='10' height='10'/>");
        assert!((full.linear - 1.0).abs() < 0.01, "{full:?}");
        let quarter = cost("<rect width='5' height='5'/>");
        assert!((quarter.linear - 0.25).abs() < 0.01, "{quarter:?}");
        let many = cost(&"<rect width='10' height='10'/>".repeat(100));
        assert!((many.linear - 100.0).abs() < 0.5, "{many:?}");
        assert_eq!(
            many.max_pixels(WORK, LAYERS),
            Some((WORK - many.fixed) / many.linear)
        );
        // Every path costs a fixed amount too.
        assert!(many.fixed >= 100.0 * PATH, "{many:?}");
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
        let pixels = morphology.max_pixels(WORK, LAYERS).unwrap();
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
        let pixels = layers.max_pixels(WORK, LAYERS).unwrap();
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
        assert_eq!(pattern.max_pixels(WORK, 1e5), None);
    }

    #[test]
    fn costs_that_are_not_finite_admit_nothing() {
        let base = Cost {
            fixed: 0.0,
            linear: 1.0,
            quadratic: 0.0,
            layers: 0.0,
        };
        assert_eq!(base.max_pixels(WORK, LAYERS), Some(WORK));
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
            assert_eq!(cost.max_pixels(WORK, LAYERS), None, "{cost:?}");
        }
        // A large but finite quadratic term gives a small size.
        let steep = Cost {
            quadratic: 1.0,
            ..base
        };
        let pixels = steep.max_pixels(WORK, LAYERS).unwrap();
        assert!((steep.work(pixels) - WORK).abs() / WORK < 1e-9, "{pixels}");
    }
}
