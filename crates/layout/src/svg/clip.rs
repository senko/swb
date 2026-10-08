//! Clip paths: `clip-path: url(#id)` on SVG elements (CSS Masking 1 §5,
//! <https://drafts.fxtf.org/css-masking/#the-clip-path>; SVG 2 §14.3).
//!
//! A reference is resolved when the content is built ([`ClipDef`]); the
//! geometry is resolved when the drawing commands are made
//! ([`resolve`]), because the matrices, the viewport and the bounding box
//! of the clipped element are known only then. What Chromium 148 does
//! where the specifications leave room (`tools/probes/inline-svg.json`,
//! cases `clip-*`):
//!
//! - The reference is looked up in the whole document (also a `clipPath` in
//!   another `<svg>`; the first element with the `id` wins). A missing
//!   element, an element that is not a `clipPath`, a `clipPath` that has
//!   `display: none` or lies inside a `display: none` subtree, `url(x)`
//!   without `#`, and a list of two `url()` values all leave the element
//!   unclipped. A reference cycle is cut where it closes.
//! - The user space of the clip is the user space of the clipped element,
//!   including its own `transform`. `clipPathUnits="objectBoundingBox"`
//!   maps the unit square to the fill bounding box of the element (a group's
//!   box is the union of its children's); an empty box clips everything.
//!   The `transform` of the `clipPath` applies in the user space, outside
//!   the bounding box mapping.
//! - The clip is the union of the fill geometry of the children `path`,
//!   `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon` and `use`
//!   that refers directly to one of these; `g`, `text` and the rest are
//!   ignored. A child with `display: none` or a `visibility` other than
//!   `visible` is skipped. `fill`, `stroke` and `opacity` do not matter.
//!   The fill rule is `clip-rule`, inherited from the `clipPath`'s own
//!   ancestors. An empty clip path clips everything.
//! - `clip-path` on a `clipPath` and on a child intersect.
//!
//! A clip that is one axis-aligned rectangle becomes a plain clip
//! rectangle ([`ClipRegion::Rect`]); any other clip needs a layer.

use std::sync::Arc;

use swb_dom::{ElementData, NodeId, ns};
use swb_style::{ClipPath as ClipPathValue, ComputedStyle, Display, FillRule, Visibility};

use super::draw::{Viewport, shape_geometry};
use super::path::SvgPath;
use super::{Builder, Geometry, MAX_CLIP_DEPTH, attribute_length, rotates};
use crate::geom::{Matrix, Rect};
use crate::positioned::style_transform;

/// The most clip shapes that the commands of one `<svg>` hold.
pub(super) const MAX_CLIP_SHAPES: usize = 20_000;

/// A `clipPath` element, ready to be resolved against an element.
#[derive(Debug, PartialEq)]
pub(crate) struct ClipDef {
    /// True for `clipPathUnits="objectBoundingBox"`.
    units_bbox: bool,
    /// The style of the `clipPath` element (its `transform`).
    style: Arc<ComputedStyle>,
    shapes: Vec<ClipShapeDef>,
    /// The `clip-path` of the `clipPath` element.
    outer: Option<Arc<ClipDef>>,
    /// True if the clip can be one axis-aligned rectangle, if the matrix of
    /// the clipped element does not rotate: it then needs no layer.
    pub(super) maybe_rect: bool,
}

/// A child of a `clipPath`.
#[derive(Debug, PartialEq)]
struct ClipShapeDef {
    /// The style of the shape (`transform`, `clip-rule`).
    style: Arc<ComputedStyle>,
    /// The `use` element that refers to the shape, if it does.
    use_element: Option<UseDef>,
    geometry: Geometry,
    /// The `clip-path` of the child (of the `use`, for a `use`).
    clip: Option<Arc<ClipDef>>,
    /// For a `use` child: the `clip-path` of the shape that it refers to.
    target_clip: Option<Arc<ClipDef>>,
}

/// The `use` element of a `use` child of a `clipPath`.
#[derive(Debug, PartialEq)]
struct UseDef {
    style: Arc<ComputedStyle>,
    x: swb_style::LengthPercentage,
    y: swb_style::LengthPercentage,
}

/// The area that remains visible, in the coordinates of the content box.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipRegion {
    /// A rectangle; an empty one hides everything.
    Rect(Rect),
    /// Anything else.
    Path(Arc<ClipPath>),
}

/// A resolved clip path: the union of the shapes, intersected with the
/// `outer` region.
#[derive(Debug, PartialEq)]
pub struct ClipPath {
    /// The shapes; their union is visible.
    pub shapes: Vec<ClipShape>,
    /// The region of the `clip-path` of the `clipPath` element, if it has
    /// one.
    pub outer: Option<ClipRegion>,
}

/// One shape of a [`ClipPath`].
#[derive(Debug, PartialEq)]
pub struct ClipShape {
    /// The path in user units.
    pub path: Arc<SvgPath>,
    /// From user units to the content box.
    pub transform: Matrix,
    /// The fill rule (`clip-rule`).
    pub rule: FillRule,
    /// The region of the shape's own `clip-path`.
    pub clip: Option<ClipRegion>,
}

const SHAPE_NAMES: [&str; 7] = [
    "path", "rect", "circle", "ellipse", "line", "polyline", "polygon",
];

impl Builder<'_> {
    /// The clip path that the `clip-path` of `style` refers to, if it
    /// refers to a valid one.
    pub(super) fn clip_reference(&mut self, style: &ComputedStyle) -> Option<Arc<ClipDef>> {
        let ClipPathValue::Url(url) = &style.clip_path else {
            return None;
        };
        let id = url.strip_prefix('#')?;
        let node = self.budget.element_by_id(self.doc, id)?;
        self.clip_def(node)
    }

    /// The clip path for an element with `style`. A clip that may need a
    /// layer takes one from the document's budget; without one left, the
    /// element is not clipped.
    pub(super) fn clip_for(&mut self, style: &ComputedStyle) -> Option<Arc<ClipDef>> {
        let def = self.clip_reference(style)?;
        if (!def.maybe_rect || self.rotated) && !self.budget.take_clip_layer() {
            return None;
        }
        Some(def)
    }

    /// The clip path `node`, built once per document.
    fn clip_def(&mut self, node: NodeId) -> Option<Arc<ClipDef>> {
        if let Some(def) = self.budget.clips.get(&node) {
            return def.clone();
        }
        if self.budget.clip_stack.contains(&node) || self.budget.clip_stack.len() >= MAX_CLIP_DEPTH
        {
            return None;
        }
        self.budget.clip_stack.push(node);
        let def = self.build_clip(node).map(Arc::new);
        self.budget.clip_stack.pop();
        self.budget.clips.insert(node, def.clone());
        def
    }

    /// Reads the `clipPath` element `node`; `None` if it is not one, or has
    /// no style (it is in a `display: none` subtree) or `display: none`.
    fn build_clip(&mut self, node: NodeId) -> Option<ClipDef> {
        let (doc, styles) = (self.doc, self.styles);
        let e = doc
            .element(node)
            .filter(|e| e.name.ns == ns!(svg) && &**e.local_name() == "clipPath")?;
        let style = styles.get(node)?;
        if style.display == Display::None {
            return None;
        }
        let outer = self.clip_reference(style);
        let mut shapes = Vec::new();
        for child in doc.element_children(node) {
            if let Some(shape) = self.clip_shape(child) {
                shapes.push(shape);
            }
        }
        let maybe_rect =
            outer.is_none() && !rotates(style) && matches!(&shapes[..], [one] if one.maybe_rect());
        Some(ClipDef {
            units_bbox: e.attr("clipPathUnits") == Some("objectBoundingBox"),
            style: Arc::clone(style),
            shapes,
            outer,
            maybe_rect,
        })
    }

    /// The child `child` of a `clipPath`, if it takes part in the clip.
    fn clip_shape(&mut self, child: NodeId) -> Option<ClipShapeDef> {
        let (doc, styles) = (self.doc, self.styles);
        let e = doc.element(child).filter(|e| e.name.ns == ns!(svg))?;
        let style = styles.get(child)?;
        if !paints_clip(style) {
            return None;
        }
        if SHAPE_NAMES.contains(&&**e.local_name()) {
            let geometry = self.clip_geometry(e, style)?;
            return Some(ClipShapeDef {
                style: Arc::clone(style),
                use_element: None,
                geometry,
                clip: self.clip_reference(style),
                target_clip: None,
            });
        }
        if &**e.local_name() != "use" {
            return None;
        }
        // A `use` refers to a shape directly (not to a group).
        let instance = styles.use_instance(None, child)?;
        let target = styles.instance_target(instance)?;
        let target_element = doc
            .element(target)
            .filter(|t| t.name.ns == ns!(svg) && SHAPE_NAMES.contains(&&**t.local_name()))?;
        let target_style = styles.style_in(Some(instance), target)?;
        if !paints_clip(target_style) {
            return None;
        }
        let geometry = self.clip_geometry(target_element, target_style)?;
        let length = |name: &str| attribute_length(e, name, style.font_size);
        Some(ClipShapeDef {
            style: Arc::clone(target_style),
            use_element: Some(UseDef {
                style: Arc::clone(style),
                x: length("x").unwrap_or(swb_style::LengthPercentage::ZERO),
                y: length("y").unwrap_or(swb_style::LengthPercentage::ZERO),
            }),
            geometry,
            clip: self.clip_reference(style),
            target_clip: self.clip_reference(target_style),
        })
    }

    /// The geometry of a shape in a clip path, counted against the shape
    /// and segment limits of the document.
    fn clip_geometry(&mut self, e: &ElementData, style: &ComputedStyle) -> Option<Geometry> {
        if self.budget.shapes.left == 0 {
            return None;
        }
        let geometry = self.geometry(e, style.font_size)?;
        self.budget.shapes.left -= 1;
        Some(geometry)
    }
}

/// True if a child of a `clipPath` with `style` takes part in the clip.
fn paints_clip(style: &ComputedStyle) -> bool {
    style.display != Display::None && style.visibility == Visibility::Visible
}

impl ClipShapeDef {
    /// True if the shape is an axis-aligned rectangle when the matrix of
    /// the clipped element does not rotate.
    fn maybe_rect(&self) -> bool {
        let transforms_keep_axes =
            !rotates(&self.style) && self.use_element.as_ref().is_none_or(|u| !rotates(&u.style));
        let rectangular = match &self.geometry {
            Geometry::Rect { rx, ry, .. } => {
                let zero = |v: &Option<swb_style::LengthPercentage>| {
                    v.as_ref()
                        .is_none_or(|v| *v == swb_style::LengthPercentage::ZERO)
                };
                zero(rx) && zero(ry)
            }
            Geometry::Path(path) => path.as_rect().is_some(),
            _ => false,
        };
        self.clip.is_none() && self.target_clip.is_none() && transforms_keep_axes && rectangular
    }
}

/// What [`resolve`] needs from the drawing pass.
pub(super) struct ClipEnv<'a> {
    pub(super) viewport: Viewport,
    /// The reference box of percentages in `transform`.
    pub(super) reference: Rect,
    /// The shapes that may still be resolved in this pass.
    pub(super) shapes_left: &'a mut usize,
    /// Whether the shape limit was reported (once per `<svg>`).
    pub(super) warned: &'a mut bool,
}

/// True if `m` keeps axes parallel (no rotation or skew).
fn axis_aligned(m: &Matrix) -> bool {
    m.is_finite() && m.keeps_axes()
}

/// The visible region of clip path `def` for an element whose user space
/// is `matrix` (to the content box) and whose fill bounding box is `bbox`
/// (in that user space). `None` if the clip cannot be resolved (a limit):
/// the element is then not clipped.
pub(super) fn resolve(
    def: &ClipDef,
    matrix: &Matrix,
    bbox: Option<Rect>,
    env: &mut ClipEnv<'_>,
    depth: usize,
) -> Option<ClipRegion> {
    if depth > 2 * MAX_CLIP_DEPTH {
        return None;
    }
    let mut space = *matrix;
    if !def.style.transform.is_empty() {
        space = space.multiply(&style_transform(&def.style, env.reference));
    }
    if def.units_bbox {
        let Some(b) = bbox.filter(|b| b.width > 0.0 && b.height > 0.0) else {
            return Some(ClipRegion::Rect(Rect::default()));
        };
        space = space.multiply(&Matrix::new(b.width, 0.0, 0.0, b.height, b.x, b.y));
    }
    let mut shapes = Vec::new();
    for shape in &def.shapes {
        if *env.shapes_left == 0 {
            if !*env.warned {
                *env.warned = true;
                log::warn!(
                    "inline SVG: too many clip path shapes in one <svg>; clip paths ignored"
                );
            }
            return None;
        }
        *env.shapes_left -= 1;
        if let Some(resolved) = resolve_shape(shape, &space, env, depth) {
            shapes.push(resolved);
        }
    }
    let outer = def
        .outer
        .as_ref()
        .and_then(|o| resolve(o, matrix, bbox, env, depth + 1));
    Some(region(shapes, outer))
}

/// Resolves one child of a clip path in user space `space`.
fn resolve_shape(
    shape: &ClipShapeDef,
    space: &Matrix,
    env: &mut ClipEnv<'_>,
    depth: usize,
) -> Option<ClipShape> {
    let mut m = *space;
    let mut clip_space = *space;
    if let Some(u) = &shape.use_element {
        if !u.style.transform.is_empty() {
            m = m.multiply(&style_transform(&u.style, env.reference));
        }
        clip_space = m;
        let (w, h) = (env.viewport.width, env.viewport.height);
        m = m.multiply(&Matrix::translate(u.x.resolve(w), u.y.resolve(h)));
    }
    if !shape.style.transform.is_empty() {
        m = m.multiply(&style_transform(&shape.style, env.reference));
    }
    if shape.use_element.is_none() {
        clip_space = m;
    }
    let geometry = shape_geometry(&shape.geometry, env.viewport);
    let path = geometry.path?;
    let rule = shape.style.clip_rule;
    let mut resolve_in =
        |c: &Arc<ClipDef>, space: &Matrix| resolve(c, space, Some(geometry.bbox), env, depth + 1);
    let own = shape.clip.as_ref().and_then(|c| resolve_in(c, &clip_space));
    let target = shape.target_clip.as_ref().and_then(|c| resolve_in(c, &m));
    let clip = match (own, target) {
        (Some(own), Some(target)) => Some(both(&path, m, rule, own, target)),
        (own, target) => own.or(target),
    };
    Some(ClipShape {
        path,
        transform: m,
        rule,
        clip,
    })
}

/// The intersection of regions `a` and `b` as a region, for the shape
/// `path` that both clip: the region of the shape clipped by `b`, inside
/// `a`. (The shape's anti-aliased edge counts twice where the shape is
/// clipped again; this happens only for a `use` in a `clipPath` where both
/// the `use` and the shape it refers to have a `clip-path`.)
fn both(
    path: &Arc<SvgPath>,
    transform: Matrix,
    rule: FillRule,
    a: ClipRegion,
    b: ClipRegion,
) -> ClipRegion {
    let inner = ClipShape {
        path: Arc::clone(path),
        transform,
        rule,
        clip: Some(b),
    };
    ClipRegion::Path(Arc::new(ClipPath {
        shapes: vec![inner],
        outer: Some(a),
    }))
}

/// The region of `shapes` (their union) inside `outer`.
fn region(shapes: Vec<ClipShape>, outer: Option<ClipRegion>) -> ClipRegion {
    let rect_of = |shape: &ClipShape| {
        (shape.clip.is_none() && axis_aligned(&shape.transform))
            .then(|| shape.path.as_rect().map(|r| shape.transform.map_rect(&r)))
            .flatten()
    };
    let empty = ClipRegion::Rect(Rect::default());
    match (&shapes[..], &outer) {
        ([], _) => empty,
        ([one], None) if rect_of(one).is_some() => {
            ClipRegion::Rect(rect_of(one).unwrap_or_default())
        }
        ([one], Some(ClipRegion::Rect(o))) if rect_of(one).is_some() => {
            match rect_of(one).and_then(|r| r.intersection(o)) {
                Some(r) => ClipRegion::Rect(r),
                None => empty,
            }
        }
        (_, Some(ClipRegion::Rect(o))) if o.width <= 0.0 || o.height <= 0.0 => empty,
        _ => ClipRegion::Path(Arc::new(ClipPath { shapes, outer })),
    }
}
