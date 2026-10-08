//! The cost model of scan-converting paths with tiny-skia: the weights of
//! a path, its segments, its pixels and its dashes, the cost of the edges
//! ([`edges`]) and the cost of spans narrower than a pixel ([`spans`])
//! (ADR 0011, ADR 0023).
//!
//! Two users share the model: the estimate for SVG images (`svg/cost.rs`,
//! which renders once into a cache at a resolution that the estimate
//! allows) and the rasterizer of inline SVG (`raster/path.rs` and
//! `raster/svg_clip.rs`, which draws every frame within a work budget per
//! strip). The weights were measured with tiny-skia in release builds.
//! One unit of work is about 0.3 ns on a 2025 desktop CPU.

pub(crate) mod edges;
pub(crate) mod spans;

use tiny_skia::{Path, Stroke, StrokeDash};

/// The cost of one dash.
pub(crate) const DASH: f64 = 200.0;
/// The cost of rendering a path, apart from its pixels and segments.
pub(crate) const PATH: f64 = 1500.0;
/// The cost of one segment of a path.
pub(crate) const SEGMENT: f64 = 30.0;
/// The cost of blending one pixel: translucent paint, gradients, patterns,
/// images, layers, masks. (Measured: 1.9 ns per pixel for half-transparent
/// fills, 0.125 ns for opaque ones; an opaque pixel counts 1 unit, which
/// is 0.3 ns, rounded up.)
pub(crate) const BLEND: f64 = 8.0;

/// The most dash array entries (dashes and gaps) along a stroke whose
/// outline is made to count its spans. tiny-skia refuses more than a
/// million; a stroke with more is drawn solid (inline SVG) or is too
/// expensive (SVG images).
pub(crate) const MAX_DASHES: f32 = 100_000.0;

/// How far a stroke of `width` reaches beyond the points of its path: half
/// its width, times the miter limit for miter joins (`miter`), and at
/// least √2 for square caps.
pub(crate) fn stroke_reach(width: f32, miter: Option<f32>) -> f32 {
    width / 2.0 * miter.unwrap_or(1.0).max(std::f32::consts::SQRT_2)
}

/// The work of making an outline of `len` segments and of looking at them
/// (also for an outline that is not drawn).
pub(crate) fn outline_charge(len: usize) -> f64 {
    (SEGMENT + edges::SWEEP_SEGMENT) * len as f64
}

/// The outline of `path` dashed with `dash` and stroked with `stroke`
/// (which has no dashes of its own), as tiny-skia makes it when it draws a
/// dashed stroke. `scale` is the scale from path units to device px; the
/// curves of the outline need no more detail than a scale of 1 to 16
/// gives. `None` if tiny-skia rejects the path.
pub(crate) fn dash_outline(
    path: &Path,
    dash: &StrokeDash,
    stroke: &Stroke,
    scale: f32,
) -> Option<Path> {
    let res = scale.clamp(1.0, 16.0);
    path.dash(dash, res)
        .and_then(|dashed| dashed.stroke(stroke, res))
}
