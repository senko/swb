//! Inline SVG: the content of an `<svg>` element in an HTML document
//! (ADR 0023).
//!
//! The outer `<svg>` element is a replaced box (`box_tree.rs`), sized by
//! the CSS rules for replaced elements with the natural size of
//! `viewport.rs`. Its descendants get no boxes. When the box tree is
//! built, [`build`] walks them once and keeps what they draw as an
//! [`SvgContent`]: groups and shapes with their computed styles, and the
//! paths of `path`, `polyline` and `polygon` elements, parsed into user
//! units (`path.rs`). Paint asks the content for its drawing commands
//! ([`SvgContent::draw`], `draw.rs`) with the size of the content box,
//! because percentages and the `viewBox` transform depend on it.
//!
//! Elements: `g` and `a` (groups), `path`, `rect`, `circle`, `ellipse`,
//! `line`, `polyline` and `polygon`. Every other element draws nothing,
//! with its subtree: `defs`, `clipPath`, `title`, `desc`, `style`,
//! `metadata`, nested `svg`, `use`, `text`, `foreignObject` and unknown
//! elements.
//!
//! Limits, for hostile documents: per box tree (document) at most
//! [`MAX_SHAPES`] shapes and [`MAX_SEGMENTS`] path segments; groups nest at
//! most [`MAX_DEPTH`] deep below the `<svg>` element. Content beyond a
//! limit is not drawn and a warning is logged once per box tree. Paint
//! bounds the rasterization work (`swb_paint`).
//!
//! Opacity: a group (or a shape with a fill and a stroke) with `opacity`
//! below 1 is composited as a layer. Layers cost memory and time in
//! proportion to their area, so a document has at most [`MAX_LAYERS`] of
//! them. Past the limit, the opacity multiplies the alpha of each paint
//! inside instead, which differs where shapes overlap. A group that holds
//! one shape with one paint needs no layer (the result is the same) and
//! does not count.

mod draw;
mod path;
mod viewport;

use std::str::FromStr;
use std::sync::Arc;

use swb_dom::{Document, ElementData, NodeId, ns};
use swb_style::{ComputedStyle, Display, LengthPercentage, StyleMap};

pub use draw::{StrokeStyle, SvgDrawItem};
pub use path::{PathSegment, SvgPath};
pub(crate) use viewport::natural_size;
pub use viewport::{Align, PreserveAspectRatio, view_box_transform};

use crate::geom::Point;

/// The most shapes that the inline SVG of one document draws.
pub const MAX_SHAPES: usize = 50_000;

/// The most path segments of all shapes of one document (a basic shape
/// counts its segments too; an arc is up to four).
pub const MAX_SEGMENTS: usize = 1_000_000;

/// The deepest nesting of groups below an `<svg>` element. Real files
/// nest a few levels; the limit bounds the recursion of [`build`] and the
/// opacity layers of paint.
pub const MAX_DEPTH: usize = 64;

/// The most opacity layers (groups and shapes with a fill and a stroke
/// that need a layer) of the inline SVG of one document.
pub const MAX_LAYERS: usize = 256;

/// What the descendants of one outer `<svg>` element draw, in the
/// element's user space.
#[derive(Debug, PartialEq)]
pub struct SvgContent {
    /// The `viewBox`, if valid.
    pub view_box: Option<crate::geom::Rect>,
    /// The `preserveAspectRatio`.
    pub preserve_aspect_ratio: PreserveAspectRatio,
    /// The groups and shapes in paint order.
    nodes: Vec<Node>,
}

/// One entry of [`SvgContent::nodes`].
#[derive(Debug, PartialEq)]
enum Node {
    /// The start of a group with its style (`opacity`, `transform`).
    BeginGroup {
        /// The style of the group.
        style: Arc<ComputedStyle>,
        /// True if its `opacity` (below 1) is composited as a layer; false
        /// if it multiplies the alpha of the paints inside.
        layer: bool,
    },
    /// The end of the innermost group.
    EndGroup,
    /// A shape.
    Shape(Box<Shape>),
}

/// A shape element.
#[derive(Debug, PartialEq)]
struct Shape {
    style: Arc<ComputedStyle>,
    /// False if the shape must not use a layer for its `opacity` (the
    /// document is out of layers): the opacity multiplies the alpha of
    /// each paint.
    layered: bool,
    geometry: Geometry,
}

/// The geometry of a shape, with lengths that can be percentages of the
/// SVG viewport.
#[derive(Debug, PartialEq)]
enum Geometry {
    /// A parsed path (`path`, `polyline`, `polygon`).
    Path(Arc<SvgPath>),
    /// `rect`: x, y, width, height, and `rx`, `ry` (`None` for `auto`).
    Rect {
        x: LengthPercentage,
        y: LengthPercentage,
        width: LengthPercentage,
        height: LengthPercentage,
        rx: Option<LengthPercentage>,
        ry: Option<LengthPercentage>,
    },
    /// `circle`: center and radius.
    Circle {
        cx: LengthPercentage,
        cy: LengthPercentage,
        r: LengthPercentage,
    },
    /// `ellipse`: center and radii (`None` for `auto`).
    Ellipse {
        cx: LengthPercentage,
        cy: LengthPercentage,
        rx: Option<LengthPercentage>,
        ry: Option<LengthPercentage>,
    },
    /// `line`.
    Line {
        x1: LengthPercentage,
        y1: LengthPercentage,
        x2: LengthPercentage,
        y2: LengthPercentage,
    },
}

/// The limits of the inline SVG of one box tree, and whether they acted.
#[derive(Debug)]
pub(crate) struct SvgBudget {
    shapes: usize,
    segments: usize,
    warned_shapes: bool,
    warned_segments: bool,
    warned_depth: bool,
    layers: usize,
    warned_layers: bool,
}

impl Default for SvgBudget {
    fn default() -> Self {
        SvgBudget {
            shapes: MAX_SHAPES,
            segments: MAX_SEGMENTS,
            warned_shapes: false,
            warned_segments: false,
            warned_depth: false,
            layers: MAX_LAYERS,
            warned_layers: false,
        }
    }
}

impl SvgBudget {
    /// Takes one opacity layer; false (with a warning, once) if there is
    /// none left.
    fn take_layer(&mut self) -> bool {
        if self.layers > 0 {
            self.layers -= 1;
            return true;
        }
        if !self.warned_layers {
            self.warned_layers = true;
            log::warn!(
                "inline SVG: more than {MAX_LAYERS} opacity layers in the document; the \
                 opacity of the rest applies to each paint"
            );
        }
        false
    }

    fn segments_ran_out(&mut self) {
        if !self.warned_segments {
            self.warned_segments = true;
            log::warn!(
                "inline SVG: more than {MAX_SEGMENTS} path segments in the document; \
                 the rest is not drawn"
            );
        }
    }
}

/// Builds the content of the outer `<svg>` element `node`.
pub(crate) fn build(
    doc: &Document,
    styles: &StyleMap,
    node: NodeId,
    budget: &mut SvgBudget,
) -> SvgContent {
    let element = doc.element(node);
    let mut content = SvgContent {
        view_box: element.and_then(viewport::view_box),
        preserve_aspect_ratio: element
            .map(viewport::preserve_aspect_ratio)
            .unwrap_or_default(),
        nodes: Vec::new(),
    };
    let mut builder = Builder {
        doc,
        styles,
        budget,
        nodes: &mut content.nodes,
    };
    builder.children(node, 0);
    content
}

struct Builder<'a> {
    doc: &'a Document,
    styles: &'a StyleMap,
    budget: &'a mut SvgBudget,
    nodes: &'a mut Vec<Node>,
}

impl Builder<'_> {
    /// Adds the content of the element children of `parent`, which is
    /// `depth` groups below the `<svg>` element.
    fn children(&mut self, parent: NodeId, depth: usize) {
        for child in self.doc.element_children(parent) {
            let Some(e) = self.doc.element(child).filter(|e| e.name.ns == ns!(svg)) else {
                continue;
            };
            let Some(style) = self.styles.get(child) else {
                continue;
            };
            if style.display == Display::None {
                continue;
            }
            match &**e.local_name() {
                "g" | "a" => self.group(child, style, depth),
                "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                    self.shape(e, style);
                }
                _ => {}
            }
        }
    }

    fn group(&mut self, node: NodeId, style: &Arc<ComputedStyle>, depth: usize) {
        if depth >= MAX_DEPTH {
            if !self.budget.warned_depth {
                self.budget.warned_depth = true;
                log::warn!(
                    "inline SVG: groups nested more than {MAX_DEPTH} deep; their content is \
                     not drawn"
                );
            }
            return;
        }
        let start = self.nodes.len();
        self.nodes.push(Node::BeginGroup {
            style: Arc::clone(style),
            layer: false,
        });
        self.children(node, depth + 1);
        // An empty group draws nothing.
        if self.nodes.len() == start + 1 {
            self.nodes.pop();
            return;
        }
        self.nodes.push(Node::EndGroup);
        if style.opacity < 1.0 && !self.single_paint(start + 1) {
            let layer = self.budget.take_layer();
            if let Some(Node::BeginGroup { layer: l, .. }) = self.nodes.get_mut(start) {
                *l = layer;
            }
        }
    }

    /// True if the nodes from `from` to the end of the group are one shape
    /// with one paint: the group's opacity is the same as the alpha of
    /// that paint.
    fn single_paint(&self, from: usize) -> bool {
        match self.nodes.get(from..) {
            Some([Node::Shape(shape), Node::EndGroup]) => !has_two_paints(&shape.style),
            _ => false,
        }
    }

    fn shape(&mut self, e: &ElementData, style: &Arc<ComputedStyle>) {
        if self.budget.shapes == 0 {
            if !self.budget.warned_shapes {
                self.budget.warned_shapes = true;
                log::warn!(
                    "inline SVG: more than {MAX_SHAPES} shapes in the document; the rest is \
                     not drawn"
                );
            }
            return;
        }
        let Some(geometry) = self.geometry(e, style.font_size) else {
            return;
        };
        self.budget.shapes -= 1;
        let layered = !(style.opacity < 1.0 && has_two_paints(style)) || self.budget.take_layer();
        self.nodes.push(Node::Shape(Box::new(Shape {
            style: Arc::clone(style),
            layered,
            geometry,
        })));
    }

    /// The geometry of shape element `e`, or `None` if it draws nothing
    /// (no path, a negative or zero size).
    fn geometry(&mut self, e: &ElementData, font_size: f32) -> Option<Geometry> {
        let length = |name: &str| attribute_length(e, name, font_size);
        let zero = || LengthPercentage::ZERO;
        Some(match &**e.local_name() {
            "path" => {
                let mut b = path::PathBuilder::new(&mut self.budget.segments);
                path::parse_path_data(e.attr("d")?, &mut b);
                let exhausted = b.exhausted;
                let path = b.finish();
                if exhausted {
                    self.budget.segments_ran_out();
                }
                Geometry::Path(Arc::new(path?))
            }
            name @ ("polyline" | "polygon") => {
                let (path, exhausted) = path::points_path(
                    e.attr("points")?,
                    name == "polygon",
                    &mut self.budget.segments,
                );
                if exhausted {
                    self.budget.segments_ran_out();
                }
                Geometry::Path(path?)
            }
            "rect" => {
                let width = length("width")?;
                let height = length("height")?;
                if is_negative(&width) || is_negative(&height) {
                    return None;
                }
                self.charge(10)?;
                Geometry::Rect {
                    x: length("x").unwrap_or_else(zero),
                    y: length("y").unwrap_or_else(zero),
                    width,
                    height,
                    rx: length("rx").filter(|v| !is_negative(v)),
                    ry: length("ry").filter(|v| !is_negative(v)),
                }
            }
            "circle" => {
                let r = length("r")?;
                if is_negative(&r) {
                    return None;
                }
                self.charge(6)?;
                Geometry::Circle {
                    cx: length("cx").unwrap_or_else(zero),
                    cy: length("cy").unwrap_or_else(zero),
                    r,
                }
            }
            "ellipse" => {
                self.charge(6)?;
                Geometry::Ellipse {
                    cx: length("cx").unwrap_or_else(zero),
                    cy: length("cy").unwrap_or_else(zero),
                    rx: length("rx").filter(|v| !is_negative(v)),
                    ry: length("ry").filter(|v| !is_negative(v)),
                }
            }
            "line" => {
                self.charge(2)?;
                Geometry::Line {
                    x1: length("x1").unwrap_or_else(zero),
                    y1: length("y1").unwrap_or_else(zero),
                    x2: length("x2").unwrap_or_else(zero),
                    y2: length("y2").unwrap_or_else(zero),
                }
            }
            _ => return None,
        })
    }

    /// Takes `segments` from the segment budget, or returns `None` if they
    /// do not fit.
    fn charge(&mut self, segments: usize) -> Option<()> {
        if self.budget.segments < segments {
            self.budget.segments = 0;
            self.budget.segments_ran_out();
            return None;
        }
        self.budget.segments -= segments;
        Some(())
    }
}

/// True if `style` may give a shape both a fill and a stroke (a
/// conservative answer: the drawing code can find that one is invisible).
fn has_two_paints(style: &ComputedStyle) -> bool {
    let stroke_width_zero = matches!(style.stroke_width, LengthPercentage::Px(w) if w <= 0.0);
    style.fill.used_color(style.color).is_some()
        && style.stroke.used_color(style.color).is_some()
        && !stroke_width_zero
}

/// True for a negative length or percentage.
fn is_negative(v: &LengthPercentage) -> bool {
    match v {
        LengthPercentage::Px(v) | LengthPercentage::Percent(v) => *v < 0.0,
        LengthPercentage::Calc(_) => false,
    }
}

/// A geometry attribute (`x`, `width`, `r`, ...) as a length in px or a
/// percentage of the SVG viewport; `None` if it is missing or invalid.
/// `em` refers to the element's font size, `ex` is half of it.
fn attribute_length(e: &ElementData, name: &str, font_size: f32) -> Option<LengthPercentage> {
    use svgtypes::LengthUnit as U;
    let length = svgtypes::Length::from_str(e.attr(name)?.trim()).ok()?;
    let n = length.number as f32;
    let px = |v: f32| {
        v.is_finite()
            .then(|| LengthPercentage::Px(crate::geom::clamp_length(v)))
    };
    match length.unit {
        U::None | U::Px => px(n),
        U::Em => px(n * font_size),
        U::Ex => px(n * font_size / 2.0),
        U::In => px(n * 96.0),
        U::Cm => px(n * 96.0 / 2.54),
        U::Mm => px(n * 96.0 / 25.4),
        U::Pt => px(n * 4.0 / 3.0),
        U::Pc => px(n * 16.0),
        U::Percent => n.is_finite().then(|| LengthPercentage::Percent(n / 100.0)),
    }
}

/// A point from two lengths resolved against the viewport size.
fn resolve_point(x: &LengthPercentage, y: &LengthPercentage, viewport: (f32, f32)) -> Point {
    Point::new(x.resolve(viewport.0), y.resolve(viewport.1))
}

#[cfg(test)]
mod tests;
