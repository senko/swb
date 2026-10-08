//! The drawing commands of inline SVG content for a content box of a given
//! size: transforms composed into one matrix per shape, percentages
//! resolved, paints resolved to colors, clip paths resolved to regions. The
//! same pass gives the bounding boxes of the elements (for the box dump).
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
//!
//! Bounding boxes (measured in Chromium 148, `tools/probes/inline-svg.json`,
//! cases `box-*`): the box of an element is its fill bounding box (curves
//! by their extrema, no stroke, no clip, no `visibility`) under the
//! element's full matrix. A group's box is its matrix applied to the union
//! of its children's boxes in the group's user space. Shapes that draw
//! nothing still have a box (a `rect` with a zero or negative size keeps
//! its position and a zero size), but only shapes with a real extent
//! take part in the union: a `rect`, `circle` or `ellipse` of size zero,
//! a path without data and an empty group do not; a path of one point and a
//! `line` do (also of length zero).

use std::sync::Arc;

use swb_dom::NodeId;
use swb_style::{
    ComputedStyle, FillRule, LengthPercentage, PointerEvents, Rgba, ShapeRendering, StrokeLinecap,
    StrokeLinejoin, SvgPaint, Visibility,
};

use super::clip::{ClipEnv, ClipRegion, MAX_CLIP_SHAPES, resolve};
use super::path::{self, SvgPath};
use super::{Geometry, Group, Node, Shape, SvgContent, resolve_point, view_box_transform};
use crate::geom::{Matrix, Point, Rect, Size};
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

/// What a shape reacts to at a point: its fill, its stroke, or both
/// (`pointer-events`, SVG 2 §16.4).
#[derive(Clone, Debug, PartialEq)]
pub struct HitShape {
    /// The shape element.
    pub node: NodeId,
    /// The path in user units.
    pub path: Arc<SvgPath>,
    /// From user units to the content box.
    pub transform: Matrix,
    /// The fill rule if the fill takes part.
    pub fill: Option<FillRule>,
    /// The stroke width in user units if the stroke takes part.
    pub stroke_width: Option<f32>,
}

/// One drawing command of SVG content.
#[derive(Clone, Debug, PartialEq)]
pub enum SvgDrawItem {
    /// Start a group that is composited with the opacity.
    PushOpacity(f32),
    /// End the innermost opacity group.
    PopOpacity,
    /// Start clipping to a region.
    PushClip(ClipRegion),
    /// End the innermost clip.
    PopClip,
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
        /// False for `shape-rendering: optimizeSpeed` and `crispEdges`:
        /// the edges are not anti-aliased.
        anti_alias: bool,
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
        /// See `Fill`.
        anti_alias: bool,
    },
    /// The area of a shape that mouse events find (no pixels).
    Hit(Box<HitShape>),
}

/// The basis of percentages: the width and height of the SVG viewport and
/// its normalized diagonal.
#[derive(Clone, Copy)]
pub(super) struct Viewport {
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) diagonal: f32,
}

/// The geometry of one shape for a viewport.
pub(super) struct ShapeGeometry {
    /// The path, if the shape draws.
    pub(super) path: Option<Arc<SvgPath>>,
    /// The fill bounding box (a point or a line for a degenerate shape).
    pub(super) bbox: Rect,
    /// True if the box takes part in the box of the parent group.
    pub(super) contributes: bool,
}

/// What the pass over the nodes found out about each node.
#[derive(Clone)]
struct Resolved {
    /// The element's own `transform` (the translation for the group of a
    /// `use`).
    own: Matrix,
    /// From the element's user space (after its own transform) to the
    /// content box.
    total: Matrix,
    /// The fill bounding box in the element's user space; for a group, the
    /// union of its children's boxes. `None` if there is none.
    bbox: Option<Rect>,
    /// For a `use` element without content: where its box is.
    empty_offset: Option<Matrix>,
    /// For a shape.
    geometry: Option<Arc<ShapeGeometry>>,
}

impl Resolved {
    fn none() -> Self {
        Resolved {
            own: Matrix::IDENTITY,
            total: Matrix::IDENTITY,
            bbox: None,
            empty_offset: None,
            geometry: None,
        }
    }
}

/// A group that the pass over the nodes has not left yet.
struct Open {
    index: usize,
    /// The union so far of the children's boxes, in the group's user space.
    acc: Option<Rect>,
    /// The matrix of the group around it.
    outer: Matrix,
    /// The translation of the `use` element inside, once it is known.
    offset: Option<Matrix>,
}

/// The viewport, the matrix of the user space of the `<svg>` and the
/// reference box of `transform`.
struct Space {
    base: Matrix,
    viewport: Viewport,
    reference: Rect,
}

impl SvgContent {
    fn space(&self, size: Size) -> Space {
        let (base, reference) = match self.view_box {
            Some(vb) => (
                view_box_transform(vb, self.preserve_aspect_ratio, size),
                Size::new(vb.width, vb.height),
            ),
            None => (Matrix::IDENTITY, size),
        };
        Space {
            base,
            viewport: Viewport {
                width: reference.width,
                height: reference.height,
                diagonal: f32::midpoint(reference.width.powi(2), reference.height.powi(2)).sqrt(),
            },
            reference: Rect::new(0.0, 0.0, reference.width, reference.height),
        }
    }

    /// The matrices, geometry and bounding box of every node, for a
    /// content box of `size`; one entry per node, in the same order.
    fn resolve(&self, space: &Space) -> Vec<Resolved> {
        let mut out: Vec<Resolved> = Vec::with_capacity(self.nodes.len());
        let mut open: Vec<Open> = Vec::new();
        let mut parent = space.base;
        for node in &self.nodes {
            match node {
                Node::BeginGroup(group) => {
                    let own = group_transform(group, space);
                    let total = parent.multiply(&own);
                    open.push(Open {
                        index: out.len(),
                        acc: None,
                        outer: parent,
                        offset: None,
                    });
                    out.push(Resolved {
                        own,
                        total,
                        ..Resolved::none()
                    });
                    parent = total;
                }
                Node::EndGroup => {
                    out.push(Resolved::none());
                    let Some(frame) = open.pop() else {
                        continue;
                    };
                    parent = frame.outer;
                    close_group(&mut out, &mut open, &frame, self);
                }
                Node::Shape(shape) => {
                    let own = own_transform(&Matrix::IDENTITY, &shape.style, space.reference);
                    let total = parent.multiply(&own);
                    let geometry = shape_geometry(&shape.geometry, space.viewport);
                    let contribution = geometry.contributes.then(|| own.map_rect(&geometry.bbox));
                    if let Some(top) = open.last_mut() {
                        top.acc = union(top.acc, contribution);
                    }
                    out.push(Resolved {
                        own,
                        total,
                        bbox: Some(geometry.bbox),
                        geometry: Some(Arc::new(geometry)),
                        ..Resolved::none()
                    });
                }
            }
        }
        out
    }

    /// The drawing commands of the content for a content box of `size`,
    /// appended to `out`. Groups are balanced.
    pub fn draw(&self, size: Size, out: &mut Vec<SvgDrawItem>) {
        let space = self.space(size);
        let resolved = self.resolve(&space);
        let mut shapes_left = MAX_CLIP_SHAPES;
        let mut warned = false;
        let mut env = ClipEnv {
            viewport: space.viewport,
            reference: space.reference,
            shapes_left: &mut shapes_left,
            warned: &mut warned,
        };
        // Whether each open group pushed a layer and a clip, and the
        // opacity that applied to the paints before it.
        let mut stack: Vec<(bool, bool, f32)> = Vec::new();
        // The opacity of the groups without a layer, which multiplies the
        // alpha of each paint.
        let mut folded = 1.0_f32;
        for (node, r) in self.nodes.iter().zip(&resolved) {
            match node {
                Node::BeginGroup(group) => {
                    let clipped = push_clip(group.clip.as_deref(), r, &mut env, out);
                    let opacity = group.style.as_ref().map_or(1.0, |s| s.opacity);
                    let layer = group.layer && opacity < 1.0;
                    stack.push((layer, clipped, folded));
                    if layer {
                        out.push(SvgDrawItem::PushOpacity(opacity));
                    } else if opacity < 1.0 {
                        folded *= opacity.max(0.0);
                    }
                }
                Node::EndGroup => {
                    if let Some((layer, clipped, outer_folded)) = stack.pop() {
                        folded = outer_folded;
                        if layer {
                            out.push(SvgDrawItem::PopOpacity);
                        }
                        if clipped {
                            out.push(SvgDrawItem::PopClip);
                        }
                    }
                }
                Node::Shape(shape) => {
                    let clipped = push_clip(shape.clip.as_deref(), r, &mut env, out);
                    draw_shape(shape, r, (space.viewport, folded), out);
                    if clipped {
                        out.push(SvgDrawItem::PopClip);
                    }
                }
            }
        }
        while let Some((layer, clipped, _)) = stack.pop() {
            if layer {
                out.push(SvgDrawItem::PopOpacity);
            }
            if clipped {
                out.push(SvgDrawItem::PopClip);
            }
        }
    }

    /// The bounding box of every element of the `<svg>` that has a box in
    /// Chromium (`g`, `a`, `use` and the shapes; not the elements in a
    /// copy that a `use` draws), in the coordinates of the content box of
    /// size `size`, for the box dump.
    pub fn element_boxes(&self, size: Size) -> Vec<(NodeId, Rect)> {
        let space = self.space(size);
        let resolved = self.resolve(&space);
        let mut boxes = Vec::new();
        for (node, r) in self.nodes.iter().zip(&resolved) {
            let id = match node {
                Node::BeginGroup(group) => group.node,
                Node::Shape(shape) => shape.node,
                Node::EndGroup => None,
            };
            let Some(id) = id else {
                continue;
            };
            let local = r.bbox.unwrap_or_else(|| {
                r.empty_offset
                    .map_or_else(Rect::default, |m| m.map_rect(&Rect::default()))
            });
            boxes.push((id, r.total.map_rect(&local)));
        }
        boxes
    }
}

/// Ends the group `frame`: stores its box and adds it to the group around.
fn close_group(out: &mut [Resolved], open: &mut [Open], frame: &Open, content: &SvgContent) {
    let Some(Node::BeginGroup(group)) = content.nodes.get(frame.index) else {
        return;
    };
    let Some(r) = out.get_mut(frame.index) else {
        return;
    };
    r.bbox = frame.acc;
    r.empty_offset = frame.offset;
    let Some(top) = open.last_mut() else {
        return;
    };
    // The translation group of a `use` tells the `use` where it is.
    if group.offset.is_some() {
        top.offset = Some(r.own);
    }
    if let Some(b) = r.bbox {
        top.acc = union(top.acc, Some(r.own.map_rect(&b)));
    }
}

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(&b)),
        (a, b) => a.or(b),
    }
}

/// The matrix of a group's own `transform`, or the translation of a
/// `use` element.
fn group_transform(group: &Group, space: &Space) -> Matrix {
    if let Some((x, y)) = &group.offset {
        return Matrix::translate(
            x.resolve(space.viewport.width),
            y.resolve(space.viewport.height),
        );
    }
    match &group.style {
        Some(style) => own_transform(&Matrix::IDENTITY, style, space.reference),
        None => Matrix::IDENTITY,
    }
}

/// Resolves the clip path of the node with `r` and pushes the command;
/// true if it pushed one.
fn push_clip(
    clip: Option<&super::ClipDef>,
    r: &Resolved,
    env: &mut ClipEnv<'_>,
    out: &mut Vec<SvgDrawItem>,
) -> bool {
    let Some(region) = clip.and_then(|c| resolve(c, &r.total, r.bbox, env, 0)) else {
        return false;
    };
    out.push(SvgDrawItem::PushClip(region));
    true
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
    r: &Resolved,
    (viewport, folded): (Viewport, f32),
    out: &mut Vec<SvgDrawItem>,
) {
    let style = &shape.style;
    let Some(path) = r.geometry.as_ref().and_then(|g| g.path.clone()) else {
        return;
    };
    let transform = r.total;
    if let Some(node) = shape.node {
        hit_shape(node, style, &path, transform, viewport, out);
    }
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
    let anti_alias = !matches!(
        style.shape_rendering,
        ShapeRendering::OptimizeSpeed | ShapeRendering::CrispEdges
    );
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
            anti_alias,
        });
    }
    if let Some((color, stroke)) = stroke {
        out.push(SvgDrawItem::Stroke {
            path,
            transform,
            color: fold(color),
            stroke: Arc::new(stroke),
            anti_alias,
        });
    }
    if layer {
        out.push(SvgDrawItem::PopOpacity);
    }
}

/// The hit command of a shape element, if `pointer-events` lets its fill
/// or stroke take part (SVG 2 §16.4).
fn hit_shape(
    node: NodeId,
    style: &ComputedStyle,
    path: &Arc<SvgPath>,
    transform: Matrix,
    viewport: Viewport,
    out: &mut Vec<SvgDrawItem>,
) {
    let visible = style.visibility == Visibility::Visible;
    let fill_painted = !matches!(style.fill, SvgPaint::None);
    let stroke_painted = !matches!(style.stroke, SvgPaint::None);
    let (fill, stroke) = match style.pointer_events {
        PointerEvents::None => (false, false),
        PointerEvents::Auto | PointerEvents::VisiblePainted => {
            (visible && fill_painted, visible && stroke_painted)
        }
        PointerEvents::VisibleFill => (visible, false),
        PointerEvents::VisibleStroke => (false, visible),
        PointerEvents::Visible => (visible, visible),
        PointerEvents::Painted => (fill_painted, stroke_painted),
        PointerEvents::Fill => (true, false),
        PointerEvents::Stroke => (false, true),
        PointerEvents::All => (true, true),
    };
    let stroke_width = stroke
        .then(|| style.stroke_width.resolve(viewport.diagonal))
        .filter(|w| *w > 0.0 && w.is_finite());
    if !fill && stroke_width.is_none() {
        return;
    }
    out.push(SvgDrawItem::Hit(Box::new(HitShape {
        node,
        path: Arc::clone(path),
        transform,
        fill: fill.then_some(style.fill_rule),
        stroke_width,
    })));
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

/// The path and the bounding box of a shape, for a viewport.
pub(super) fn shape_geometry(geometry: &Geometry, viewport: Viewport) -> ShapeGeometry {
    let size = (viewport.width, viewport.height);
    // Basic shapes have at most 10 segments; their count was charged to
    // the document when the content was built.
    let mut budget = 16;
    let point_box = |p: Point| Rect::new(p.x, p.y, 0.0, 0.0);
    let nothing = |bbox: Rect| ShapeGeometry {
        path: None,
        bbox,
        contributes: false,
    };
    let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    match geometry {
        Geometry::Path(p) => ShapeGeometry {
            path: Some(Arc::clone(p)),
            bbox: p.fill_bounds().unwrap_or_default(),
            contributes: true,
        },
        Geometry::Point(Some(p)) => ShapeGeometry {
            path: None,
            bbox: point_box(*p),
            contributes: true,
        },
        Geometry::Point(None) => nothing(Rect::default()),
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
            let (w, h) = (finite(corner.x), finite(corner.y));
            let bbox = Rect::new(origin.x, origin.y, w, h);
            if !(w > 0.0 && h > 0.0) {
                return nothing(bbox);
            }
            // SVG 2 §10.2: `auto` takes the other radius; both are
            // clamped to half the size.
            let (rx, ry) = radii(rx.as_ref(), ry.as_ref(), viewport);
            let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
            ShapeGeometry {
                path: path::rect_path(bbox, rx, ry, &mut budget).map(Arc::new),
                bbox,
                contributes: true,
            }
        }
        Geometry::Circle { cx, cy, r } => {
            let c = resolve_point(cx, cy, size);
            let r = finite(r.resolve(viewport.diagonal));
            ellipse(c, r, r, &mut budget)
        }
        Geometry::Ellipse { cx, cy, rx, ry } => {
            let c = resolve_point(cx, cy, size);
            let (rx, ry) = radii(rx.as_ref(), ry.as_ref(), viewport);
            ellipse(c, finite(rx), finite(ry), &mut budget)
        }
        Geometry::Line { x1, y1, x2, y2 } => {
            let from = resolve_point(x1, y1, size);
            let to = resolve_point(x2, y2, size);
            let bbox = Rect::new(
                from.x.min(to.x),
                from.y.min(to.y),
                (to.x - from.x).abs(),
                (to.y - from.y).abs(),
            );
            ShapeGeometry {
                path: path::line_path(from, to, &mut budget).map(Arc::new),
                bbox,
                contributes: true,
            }
        }
    }
}

/// The resolved radii `rx`, `ry` of a `rect` or `ellipse`: `auto` takes
/// the other radius, both `auto` give 0 (SVG 2 §10.2).
fn radii(
    rx: Option<&LengthPercentage>,
    ry: Option<&LengthPercentage>,
    viewport: Viewport,
) -> (f32, f32) {
    let rx = rx.map(|v| v.resolve(viewport.width));
    let ry = ry.map(|v| v.resolve(viewport.height));
    match (rx, ry) {
        (Some(rx), Some(ry)) => (rx, ry),
        (Some(r), None) | (None, Some(r)) => (r, r),
        (None, None) => (0.0, 0.0),
    }
}

/// An ellipse (or circle) of radii `rx`, `ry` (not negative) around `c`.
fn ellipse(c: Point, rx: f32, ry: f32, budget: &mut usize) -> ShapeGeometry {
    let bbox = Rect::new(c.x - rx, c.y - ry, 2.0 * rx, 2.0 * ry);
    if !(rx > 0.0 && ry > 0.0) {
        return ShapeGeometry {
            path: None,
            bbox: Rect::new(c.x, c.y, 0.0, 0.0),
            contributes: false,
        };
    }
    ShapeGeometry {
        path: path::ellipse_path(c.x, c.y, rx, ry, budget).map(Arc::new),
        bbox,
        contributes: true,
    }
}
