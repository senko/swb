//! The drawing commands of inline SVG content for a content box of a given
//! size: transforms composed into one matrix per shape, percentages
//! resolved, paints resolved to colors.
//!
//! Painting follows SVG 2 §13 (<https://svgwg.org/svg2-draft/painting.html>):
//! a shape fills, then strokes. `fill-opacity` and `stroke-opacity` apply
//! to each paint on its own; `opacity` applies to the shape or group as a
//! whole (a layer), except for a shape with only one paint, where it is
//! the same as multiplying the paint's alpha. Shapes with `visibility`
//! other than `visible` draw nothing; their descendants can.
//!
//! Percentages (SVG 2 §8.9, <https://svgwg.org/svg2-draft/coords.html#Units>)
//! refer to the width, the height or the normalized diagonal
//! (`sqrt((w² + h²) / 2)`) of the `viewBox` if there is one, else of the
//! content box. `transform` uses the view box as the reference box
//! (`transform-box: view-box`, the initial value) with its origin at the
//! origin of the user space.

use std::sync::Arc;

use swb_style::{
    ComputedStyle, FillRule, LengthPercentage, Rgba, StrokeLinecap, StrokeLinejoin, Visibility,
};

use super::path::{self, SvgPath};
use super::{Geometry, Node, Shape, SvgContent, resolve_point, view_box_transform};
use crate::geom::{Matrix, Rect, Size};
use crate::positioned::style_transform;

/// The stroke of a shape, in user units.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    /// The stroke width (positive).
    pub width: f32,
    /// The line cap.
    pub cap: StrokeLinecap,
    /// The line join.
    pub join: StrokeLinejoin,
    /// The miter limit (at least 1).
    pub miter_limit: f32,
    /// The dash lengths (an even number, not all zero), or `None` for a
    /// solid stroke.
    pub dashes: Option<Arc<[f32]>>,
    /// The distance into the dash pattern at the start of the path.
    pub dash_offset: f32,
}

/// One drawing command of SVG content.
#[derive(Clone, Debug, PartialEq)]
pub enum SvgDrawItem {
    /// Start a group that is composited with the opacity.
    PushOpacity(f32),
    /// End the innermost opacity group.
    PopOpacity,
    /// Fill a path.
    Fill {
        /// The path in user units.
        path: Arc<SvgPath>,
        /// From user units to the content box (its top-left corner is the
        /// origin).
        transform: Matrix,
        /// The color, with `fill-opacity` (and a folded `opacity`).
        color: Rgba,
        /// The fill rule.
        rule: FillRule,
    },
    /// Stroke a path.
    Stroke {
        /// The path in user units.
        path: Arc<SvgPath>,
        /// From user units to the content box.
        transform: Matrix,
        /// The color, with `stroke-opacity` (and a folded `opacity`).
        color: Rgba,
        /// The stroke in user units (the transform applies to it).
        stroke: Arc<StrokeStyle>,
    },
}

/// The basis of percentages: the width and height of the SVG viewport and
/// its normalized diagonal.
#[derive(Clone, Copy)]
struct Viewport {
    width: f32,
    height: f32,
    diagonal: f32,
}

impl SvgContent {
    /// The drawing commands of the content for a content box of `size`,
    /// appended to `out`. Groups are balanced.
    pub fn draw(&self, size: Size, out: &mut Vec<SvgDrawItem>) {
        let (base, reference) = match self.view_box {
            Some(vb) => (
                view_box_transform(vb, self.preserve_aspect_ratio, size),
                Size::new(vb.width, vb.height),
            ),
            None => (Matrix::IDENTITY, size),
        };
        let viewport = Viewport {
            width: reference.width,
            height: reference.height,
            diagonal: f32::midpoint(reference.width.powi(2), reference.height.powi(2)).sqrt(),
        };
        let reference = Rect::new(0.0, 0.0, reference.width, reference.height);
        // The transform of each open group, whether it pushed a layer, and
        // the opacity that applied to the paints before it.
        let mut stack: Vec<(Matrix, bool, f32)> = Vec::new();
        let mut matrix = base;
        // The opacity of the groups without a layer, which multiplies the
        // alpha of each paint.
        let mut folded = 1.0_f32;
        for node in &self.nodes {
            match node {
                Node::BeginGroup { style, layer } => {
                    let layer = *layer && style.opacity < 1.0;
                    stack.push((matrix, layer, folded));
                    if layer {
                        out.push(SvgDrawItem::PushOpacity(style.opacity));
                    } else if style.opacity < 1.0 {
                        folded *= style.opacity.max(0.0);
                    }
                    matrix = own_transform(&matrix, style, reference);
                }
                Node::EndGroup => {
                    if let Some((outer, layer, outer_folded)) = stack.pop() {
                        matrix = outer;
                        folded = outer_folded;
                        if layer {
                            out.push(SvgDrawItem::PopOpacity);
                        }
                    }
                }
                Node::Shape(shape) => {
                    let transform = own_transform(&matrix, &shape.style, reference);
                    draw_shape(shape, transform, (viewport, folded), out);
                }
            }
        }
        while let Some((_, layer, _)) = stack.pop() {
            if layer {
                out.push(SvgDrawItem::PopOpacity);
            }
        }
    }
}

/// `outer` followed by the element's own `transform`.
fn own_transform(outer: &Matrix, style: &ComputedStyle, reference: Rect) -> Matrix {
    if style.transform.is_empty() {
        *outer
    } else {
        outer.multiply(&style_transform(style, reference))
    }
}

/// The commands of one shape.
///
/// `folded` is the opacity of the enclosing groups that have no layer.
fn draw_shape(
    shape: &Shape,
    transform: Matrix,
    (viewport, folded): (Viewport, f32),
    out: &mut Vec<SvgDrawItem>,
) {
    let style = &shape.style;
    if style.visibility != Visibility::Visible {
        return;
    }
    let fill = style
        .fill
        .used_color(style.color)
        .map(|c| with_alpha(c, style.fill_opacity))
        .filter(|c| !c.is_transparent());
    let stroke = stroke_style(style, viewport).and_then(|s| {
        style
            .stroke
            .used_color(style.color)
            .map(|c| (with_alpha(c, style.stroke_opacity), s))
            .filter(|(c, _)| !c.is_transparent())
    });
    if fill.is_none() && stroke.is_none() {
        return;
    }
    let Some(path) = shape_path(&shape.geometry, viewport) else {
        return;
    };
    let opacity = style.opacity;
    // With one paint, the opacity multiplies its alpha; with two, the
    // shape is a group.
    let layer = opacity < 1.0 && shape.layered && fill.is_some() && stroke.is_some();
    let paint_opacity = if layer { folded } else { opacity * folded };
    let fold = |c: Rgba| with_alpha(c, paint_opacity);
    if layer {
        out.push(SvgDrawItem::PushOpacity(opacity));
    }
    if let Some(color) = fill.map(fold) {
        out.push(SvgDrawItem::Fill {
            path: Arc::clone(&path),
            transform,
            color,
            rule: style.fill_rule,
        });
    }
    if let Some((color, stroke)) = stroke {
        out.push(SvgDrawItem::Stroke {
            path,
            transform,
            color: fold(color),
            stroke: Arc::new(stroke),
        });
    }
    if layer {
        out.push(SvgDrawItem::PopOpacity);
    }
}

/// `color` with its alpha multiplied by `alpha`.
fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
    if alpha >= 1.0 {
        return color;
    }
    Rgba {
        a: (f32::from(color.a) * alpha.max(0.0)).round() as u8,
        ..color
    }
}

/// The stroke of `style`, or `None` if its width is 0.
fn stroke_style(style: &ComputedStyle, viewport: Viewport) -> Option<StrokeStyle> {
    let width = style.stroke_width.resolve(viewport.diagonal);
    if !(width > 0.0 && width.is_finite()) {
        return None;
    }
    Some(StrokeStyle {
        width,
        cap: style.stroke_linecap,
        join: style.stroke_linejoin,
        miter_limit: style.stroke_miterlimit.max(1.0),
        dashes: dashes(&style.stroke_dasharray, viewport.diagonal),
        dash_offset: style.stroke_dashoffset.resolve(viewport.diagonal),
    })
    .filter(|s| s.dash_offset.is_finite())
}

/// The dash lengths of a `stroke-dasharray`, repeated to an even count
/// (SVG 2 §13.5.7); `None` for a solid stroke (`none`, or all zero).
fn dashes(array: &[LengthPercentage], diagonal: f32) -> Option<Arc<[f32]>> {
    let values: Vec<f32> = array.iter().map(|v| v.resolve(diagonal).max(0.0)).collect();
    let sum: f32 = values.iter().sum();
    if !(sum > 0.0 && sum.is_finite()) {
        return None;
    }
    let repeat = if values.len() % 2 == 1 { 2 } else { 1 };
    Some(
        values
            .iter()
            .cycle()
            .take(values.len() * repeat)
            .copied()
            .collect(),
    )
}

/// The path of a shape, or `None` if it draws nothing.
fn shape_path(geometry: &Geometry, viewport: Viewport) -> Option<Arc<SvgPath>> {
    let size = (viewport.width, viewport.height);
    // Basic shapes have at most 10 segments; their count was charged to
    // the document when the content was built.
    let mut budget = 16;
    let path = match geometry {
        Geometry::Path(p) => return Some(Arc::clone(p)),
        Geometry::Rect {
            x,
            y,
            width,
            height,
            rx,
            ry,
        } => {
            let origin = resolve_point(x, y, size);
            let corner = resolve_point(width, height, size);
            let (w, h) = (corner.x, corner.y);
            if !(w > 0.0 && h > 0.0) {
                return None;
            }
            // SVG 2 §10.2: `auto` takes the other radius; both are
            // clamped to half the size.
            let rx = rx.as_ref().map(|v| v.resolve(viewport.width));
            let ry = ry.as_ref().map(|v| v.resolve(viewport.height));
            let (rx, ry) = match (rx, ry) {
                (Some(rx), Some(ry)) => (rx, ry),
                (Some(r), None) | (None, Some(r)) => (r, r),
                (None, None) => (0.0, 0.0),
            };
            let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
            let rect = Rect::new(origin.x, origin.y, w, h);
            path::rect_path(rect, rx, ry, &mut budget)
        }
        Geometry::Circle { cx, cy, r } => {
            let c = resolve_point(cx, cy, size);
            let r = r.resolve(viewport.diagonal);
            if r.is_nan() || r <= 0.0 {
                return None;
            }
            path::ellipse_path(c.x, c.y, r, r, &mut budget)
        }
        Geometry::Ellipse { cx, cy, rx, ry } => {
            let c = resolve_point(cx, cy, size);
            let rx = rx.as_ref().map(|v| v.resolve(viewport.width));
            let ry = ry.as_ref().map(|v| v.resolve(viewport.height));
            let (rx, ry) = match (rx, ry) {
                (Some(rx), Some(ry)) => (rx, ry),
                (Some(r), None) | (None, Some(r)) => (r, r),
                (None, None) => return None,
            };
            if !(rx > 0.0 && ry > 0.0) {
                return None;
            }
            path::ellipse_path(c.x, c.y, rx, ry, &mut budget)
        }
        Geometry::Line { x1, y1, x2, y2 } => path::line_path(
            resolve_point(x1, y1, size),
            resolve_point(x2, y2, size),
            &mut budget,
        ),
    };
    path.map(Arc::new)
}
