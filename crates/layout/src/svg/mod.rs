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
//! because percentages and the `viewBox` transform depend on it; the box
//! dump asks for the bounding boxes of the elements
//! ([`SvgContent::element_boxes`]).
//!
//! Elements: `g` and `a` (groups), `path`, `rect`, `circle`, `ellipse`,
//! `line`, `polyline` and `polygon`, and `use` with a reference to one of
//! these (a copy of the referenced element inside two groups: the `use`
//! element and its `x`/`y` translation; the copy's styles come from
//! `StyleMap::use_instance`). A `use` of a `symbol` draws the symbol's
//! content in a new viewport ([`SymbolView`], SVG 2 §5.6: its size is the
//! `width` and `height` of the `use`, else of the `symbol`, else 100 %; the
//! `x`, `y` and `transform` of the symbol place it; the
//! `viewBox` and `preserveAspectRatio` of the symbol; a clip to the viewport
//! unless `overflow` is `visible` or `auto`; a size of zero, a negative or
//! invalid `use` size, and a `viewBox` of zero width or height draw nothing;
//! percentages inside refer to the `viewBox`, else to the viewport). The
//! shapes of a copy report the outermost `use` element for hit testing.
//! `defs`, `clipPath` and the elements that only they refer to draw nothing
//! themselves. Every other element draws nothing, with its subtree: `title`,
//! `desc`, `style`, `metadata`, an unused `symbol`, nested `svg`, `text`,
//! `foreignObject` and unknown elements.
//!
//! `clip-path: url(#id)` (`clip.rs`) refers to a `clipPath` element of
//! the document, also in another `<svg>`. The references are resolved when
//! the content is built; the clip geometry is resolved with the matrices
//! when the commands are made.
//!
//! Limits, for hostile documents: per box tree (document) at most
//! [`MAX_SHAPES`] shapes, [`MAX_GROUPS`] groups and [`MAX_SEGMENTS`] path
//! segments; groups nest at most [`MAX_DEPTH`] deep below the `<svg>`
//! element (a `use` is two groups); `clip-path` references nest at most
//! [`MAX_CLIP_DEPTH`] deep and a reference cycle is dropped. Content beyond
//! a limit is not drawn and a warning is logged once per box tree. The
//! style crate bounds the copies of `use` (`MAX_INSTANCE_ELEMENTS`,
//! `MAX_USE_DEPTH`). Paint bounds the rasterization work (`swb_paint`).
//!
//! Opacity: a group (or a shape with a fill and a stroke) with `opacity`
//! below 1 is composited as a layer. Layers cost memory and time in
//! proportion to their area, so a document has at most [`MAX_LAYERS`] of
//! them. Past the limit, the opacity multiplies the alpha of each paint
//! inside instead, which differs where shapes overlap. An element with a
//! `mix-blend-mode` is a layer too, and has no blend past the limit. A
//! group that holds one shape with one paint needs no layer (the result
//! is the same) and does not count. Clips that need a layer have their own limit,
//! [`MAX_CLIP_LAYERS`].

mod clip;
mod draw;
mod path;
mod viewport;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use swb_dom::{Document, ElementData, NodeId, ns};
use swb_style::{ComputedStyle, Display, InstanceId, LengthPercentage, Overflow, StyleMap};

pub(crate) use clip::ClipDef;
pub use clip::{ClipPath, ClipRegion, ClipShape};
pub use draw::{HitShape, StrokeStyle, SvgDrawItem};
pub use path::{PathSegment, SvgPath, path_from_data};
pub(crate) use path::{ellipse_segments, rounded_rect_segments};
pub(crate) use viewport::natural_size;
pub(crate) use viewport::{PreserveAspectRatio, view_box_transform};

use crate::geom::Point;

/// The most shapes that the inline SVG of one document draws.
pub(crate) const MAX_SHAPES: usize = 50_000;

/// The most groups (`g`, `a`, `use`) of the inline SVG of one document.
pub(crate) const MAX_GROUPS: usize = 50_000;

/// The most path segments of all shapes of one document (a basic shape
/// counts its segments too; an arc is up to four).
pub(crate) const MAX_SEGMENTS: usize = 1_000_000;

/// The deepest nesting of groups below an `<svg>` element. Real files
/// nest a few levels; the limit bounds the recursion of [`build`] and the
/// opacity layers of paint.
pub(crate) const MAX_DEPTH: usize = 64;

/// The most opacity layers (groups and shapes with a fill and a stroke
/// that need a layer) of the inline SVG of one document.
pub(crate) const MAX_LAYERS: usize = 256;

/// The most clip groups of one document that may need a layer: clips
/// that are not one axis-aligned rectangle (a rectangle is a plain clip
/// rectangle without a layer). A clip past the limit is ignored.
pub(crate) const MAX_CLIP_LAYERS: usize = 256;

/// The longest chain of `clip-path` references: an element's clip path,
/// the clip path of that clip path, and so on; also the nesting of
/// the `clip-path` of a shape inside a clip path. A cycle is cut where it
/// closes.
pub(crate) const MAX_CLIP_DEPTH: usize = 8;

/// What the descendants of one outer `<svg>` element draw, in the
/// element's user space.
#[derive(Debug, PartialEq)]
pub struct SvgContent {
    /// The `viewBox`, if valid.
    pub(crate) view_box: Option<crate::geom::Rect>,
    /// The `preserveAspectRatio`.
    pub(crate) preserve_aspect_ratio: PreserveAspectRatio,
    /// The groups and shapes in paint order.
    nodes: Vec<Node>,
}

/// One entry of [`SvgContent::nodes`].
#[derive(Debug, PartialEq)]
enum Node {
    /// The start of a group.
    BeginGroup(Box<Group>),
    /// The end of the innermost group.
    EndGroup,
    /// A shape.
    Shape(Box<Shape>),
}

/// A group: `g`, `a`, `use`, and the translation inside a `use`.
#[derive(Debug, PartialEq)]
struct Group {
    /// The style of the group (`opacity`, `transform`); `None` for the
    /// translation of a `use` element.
    style: Option<Arc<ComputedStyle>>,
    /// True if its `opacity` (below 1) is composited as a layer; false
    /// if it multiplies the alpha of the paints inside.
    layer: bool,
    /// The element, for the box dump; `None` for the translation of a `use`
    /// and for the elements of the copy that a `use` draws.
    node: Option<NodeId>,
    /// The clip path.
    clip: Option<Arc<ClipDef>>,
    /// What the group does besides its style.
    kind: GroupKind,
}

/// The special kinds of group: a group has at most one of them.
#[derive(Debug, PartialEq)]
enum GroupKind {
    /// A plain group (`g`, `a`, the copy of an element that a `use` draws).
    Plain,
    /// The translation by the `x` and `y` of a `use` element.
    UseOffset((LengthPercentage, LengthPercentage)),
    /// The instance of a `symbol`: the viewport it establishes.
    Symbol(SymbolView),
}

/// The viewport of the instance of a `symbol` (SVG 2 §5.6, "The `symbol`
/// element"; <https://svgwg.org/svg2-draft/struct.html#SymbolElement>): a
/// new viewport at the origin of its group (the translation by the `x` and
/// `y` of the `use` element is the group around it), with the `viewBox` and
/// `preserveAspectRatio` of the symbol.
#[derive(Debug, PartialEq)]
struct SymbolView {
    /// The size of the viewport: the `width` and `height` of the `use`
    /// element, else those of the `symbol`, else `100%` (of the viewport
    /// around).
    width: LengthPercentage,
    height: LengthPercentage,
    /// The `x` and `y` of the symbol: the viewport's offset (0 if missing
    /// or invalid).
    x: LengthPercentage,
    y: LengthPercentage,
    /// The `viewBox` of the symbol, if valid.
    view_box: Option<crate::geom::Rect>,
    preserve_aspect_ratio: PreserveAspectRatio,
    /// True if the content is clipped to the viewport: `overflow` other
    /// than `visible` and `auto` (the user-agent style sheet gives `symbol`
    /// `overflow: hidden`).
    clip: bool,
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
    /// The element, for the box dump and hit testing; `None` in the copy
    /// that a `use` draws.
    node: Option<NodeId>,
    /// The clip path.
    clip: Option<Arc<ClipDef>>,
    /// In the copy that a `use` draws: the outermost `use` element, which
    /// hit testing reports for the shape (in Chromium, the `use` is the
    /// target of events in its instance). `None` outside a copy.
    hit: Option<NodeId>,
}

/// The geometry of a shape, with lengths that can be percentages of the
/// SVG viewport.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Geometry {
    /// A parsed path (`path`, `polyline`, `polygon`).
    Path(Arc<SvgPath>),
    /// A path without a segment that draws: its one point (path data that
    /// only moves, a `polygon` of one point), or `None` for no point at all.
    /// It draws nothing but has a bounding box (at (0, 0) without a
    /// point). Only a point takes part in the bounding box of the group.
    Point(Option<Point>),
    /// `rect`: x, y, width, height (zero if missing or negative), and `rx`,
    /// `ry` (`None` for `auto`).
    Rect {
        x: LengthPercentage,
        y: LengthPercentage,
        width: LengthPercentage,
        height: LengthPercentage,
        rx: Option<LengthPercentage>,
        ry: Option<LengthPercentage>,
    },
    /// `circle`: center and radius (zero if missing or negative).
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

/// A limit that counts down and warns once when it acts.
#[derive(Debug)]
struct Counter {
    /// What may still be taken.
    left: usize,
    /// True once the warning was logged.
    warned: bool,
}

impl Counter {
    fn new(left: usize) -> Counter {
        Counter {
            left,
            warned: false,
        }
    }

    /// Takes `n` or nothing; false (with a warning, once) if fewer are
    /// left.
    fn take(&mut self, n: usize, message: std::fmt::Arguments<'_>) -> bool {
        if self.left >= n {
            self.left -= n;
            return true;
        }
        self.warn(message);
        false
    }

    /// Logs `message`, only the first time.
    fn warn(&mut self, message: std::fmt::Arguments<'_>) {
        if !self.warned {
            self.warned = true;
            log::warn!("{message}");
        }
    }
}

/// The limits of the inline SVG of one box tree, and whether they acted.
#[derive(Debug)]
pub(crate) struct SvgBudget {
    shapes: Counter,
    groups: Counter,
    segments: Counter,
    /// Only warns: the depth is a limit of one `<svg>`, not a budget.
    depth: Counter,
    layers: Counter,
    clip_layers: Counter,
    /// The element for each `id` of the document, built when the first
    /// reference needs it.
    ids: Option<HashMap<Box<str>, NodeId>>,
    /// The clip paths built so far, by `clipPath` element (`None` if the
    /// element is not a valid clip path).
    clips: HashMap<NodeId, Option<Arc<ClipDef>>>,
    /// The `clipPath` elements being built (to cut cycles).
    clip_stack: Vec<NodeId>,
}

impl Default for SvgBudget {
    fn default() -> Self {
        SvgBudget {
            shapes: Counter::new(MAX_SHAPES),
            groups: Counter::new(MAX_GROUPS),
            segments: Counter::new(MAX_SEGMENTS),
            depth: Counter::new(0),
            layers: Counter::new(MAX_LAYERS),
            clip_layers: Counter::new(MAX_CLIP_LAYERS),
            ids: None,
            clips: HashMap::new(),
            clip_stack: Vec::new(),
        }
    }
}

impl SvgBudget {
    /// Takes one opacity layer; false (with a warning, once) if there is
    /// none left.
    fn take_layer(&mut self) -> bool {
        self.layers.take(
            1,
            format_args!(
                "inline SVG: more than {MAX_LAYERS} opacity layers in the document; the \
                 opacity of the rest applies to each paint"
            ),
        )
    }

    /// Takes one clip layer; false (with a warning, once) if there is
    /// none left.
    fn take_clip_layer(&mut self) -> bool {
        self.clip_layers.take(
            1,
            format_args!(
                "inline SVG: more than {MAX_CLIP_LAYERS} clip paths that need a layer in the \
                 document; the rest are ignored"
            ),
        )
    }

    /// Takes one group; false (with a warning, once) if there is none
    /// left.
    fn take_group(&mut self) -> bool {
        self.take_groups(1)
    }

    /// Takes `n` groups or none; false (with a warning, once) if fewer
    /// are left.
    fn take_groups(&mut self, n: usize) -> bool {
        self.groups.take(
            n,
            format_args!(
                "inline SVG: more than {MAX_GROUPS} groups in the document; the rest are not \
                 drawn"
            ),
        )
    }

    fn segments_ran_out(&mut self) {
        self.segments.warn(format_args!(
            "inline SVG: more than {MAX_SEGMENTS} path segments in the document; \
             the rest is not drawn"
        ));
    }

    /// The element with `id` in `doc`.
    fn element_by_id(&mut self, doc: &Document, id: &str) -> Option<NodeId> {
        self.ids
            .get_or_insert_with(|| {
                doc.element_ids()
                    .into_iter()
                    .map(|(id, node)| (Box::from(id), node))
                    .collect()
            })
            .get(id)
            .copied()
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
        scope: None,
        copy: false,
        rotated: false,
        hit: None,
    };
    builder.children(node, 0);
    content
}

struct Builder<'a> {
    doc: &'a Document,
    styles: &'a StyleMap,
    budget: &'a mut SvgBudget,
    nodes: &'a mut Vec<Node>,
    /// The instance tree whose styles the elements have: `None` for the
    /// document, else the copy that a `use` draws.
    scope: Option<InstanceId>,
    /// True inside the copy of a `use`: its nodes are not DOM elements of
    /// the `<svg>`.
    copy: bool,
    /// True if an element on the way down has a `transform` that rotates
    /// or skews (a clip rectangle is then not axis-aligned).
    rotated: bool,
    /// The outermost `use` element of the copy being built, if any.
    hit: Option<NodeId>,
}

impl Builder<'_> {
    /// Adds the content of the element children of `parent`, which is
    /// `depth` groups below the `<svg>` element.
    fn children(&mut self, parent: NodeId, depth: usize) {
        let doc = self.doc;
        for child in doc.element_children(parent) {
            self.element(child, depth);
        }
    }

    /// Adds the content of the SVG element `node`.
    fn element(&mut self, node: NodeId, depth: usize) {
        let (doc, styles) = (self.doc, self.styles);
        let Some(e) = doc.element(node).filter(|e| e.name.ns == ns!(svg)) else {
            return;
        };
        let Some(style) = styles.style_in(self.scope, node) else {
            return;
        };
        if style.display == Display::None {
            return;
        }
        match &**e.local_name() {
            "g" | "a" => self.group(node, style, depth),
            "use" => self.use_element(node, e, style, depth),
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                self.shape(node, e, style);
            }
            _ => {}
        }
    }

    /// The identity of `node` in the box dump: none in a copy.
    fn dom_node(&self, node: NodeId) -> Option<NodeId> {
        (!self.copy).then_some(node)
    }

    /// Warns (once) and returns true if `depth` is too deep for a group.
    fn too_deep(&mut self, depth: usize) -> bool {
        if depth < MAX_DEPTH {
            return false;
        }
        self.budget.depth.warn(format_args!(
            "inline SVG: groups nested more than {MAX_DEPTH} deep; their content is \
             not drawn"
        ));
        true
    }

    fn group(&mut self, node: NodeId, style: &Arc<ComputedStyle>, depth: usize) {
        if self.too_deep(depth) || !self.budget.take_group() {
            return;
        }
        let rotated = self.rotated;
        self.rotated |= rotates(style);
        let clip = self.clip_for(style);
        let start = self.nodes.len();
        self.nodes.push(Node::BeginGroup(Box::new(Group {
            style: Some(Arc::clone(style)),
            layer: false,
            node: self.dom_node(node),
            clip,
            kind: GroupKind::Plain,
        })));
        self.children(node, depth + 1);
        self.rotated = rotated;
        let empty = self.nodes.len() == start + 1;
        self.end_group(start, style, self.dom_node(node).is_some(), empty);
    }

    /// Closes the group that began at `start`. An empty group draws
    /// nothing; it stays if it is an element of the `<svg>` (`keep`), for
    /// the box dump.
    fn end_group(&mut self, start: usize, style: &Arc<ComputedStyle>, keep: bool, empty: bool) {
        if empty && !keep {
            self.nodes.truncate(start);
            return;
        }
        self.nodes.push(Node::EndGroup);
        let needs_layer =
            style.has_blend_mode() || (style.opacity < 1.0 && !self.single_paint(start + 1));
        if !empty && needs_layer {
            let layer = self.budget.take_layer();
            if let Some(Node::BeginGroup(group)) = self.nodes.get_mut(start) {
                group.layer = layer;
            }
        }
    }

    /// A `use` element: two groups (the element and the translation by its
    /// `x` and `y`) around the copy of the referenced element. Without a
    /// copy (a bad reference, a cycle, a limit) the groups are empty.
    fn use_element(
        &mut self,
        node: NodeId,
        e: &ElementData,
        style: &Arc<ComputedStyle>,
        depth: usize,
    ) {
        if self.too_deep(depth + 1) || !self.budget.take_groups(2) {
            return;
        }
        let rotated = self.rotated;
        self.rotated |= rotates(style);
        let clip = self.clip_for(style);
        let length = |name: &str| attribute_length(e, name, style.font_size);
        let offset = (
            length("x").unwrap_or(LengthPercentage::ZERO),
            length("y").unwrap_or(LengthPercentage::ZERO),
        );
        let start = self.nodes.len();
        self.nodes.push(Node::BeginGroup(Box::new(Group {
            style: Some(Arc::clone(style)),
            layer: false,
            node: self.dom_node(node),
            clip,
            kind: GroupKind::Plain,
        })));
        self.nodes.push(Node::BeginGroup(Box::new(Group {
            style: None,
            layer: false,
            node: None,
            clip: None,
            kind: GroupKind::UseOffset(offset),
        })));
        if let Some(instance) = self.styles.use_instance(self.scope, node) {
            let target = self.styles.instance_target(instance);
            let saved = (self.scope, self.copy, self.hit);
            self.hit = self.hit.or(self.dom_node(node));
            self.scope = Some(instance);
            self.copy = true;
            if let Some(target) = target {
                self.instance(target, e, depth + 2);
            }
            (self.scope, self.copy, self.hit) = saved;
        }
        self.nodes.push(Node::EndGroup);
        self.rotated = rotated;
        let empty = self.nodes.len() == start + 3;
        self.end_group(start, style, self.dom_node(node).is_some(), empty);
    }

    /// The copy that a `use` draws: the element `target`; for a `symbol`,
    /// the new viewport that the `use` element `e` makes of it.
    fn instance(&mut self, target: NodeId, e: &ElementData, depth: usize) {
        let is_symbol = self
            .doc
            .element(target)
            .is_some_and(|t| t.name.ns == ns!(svg) && &**t.local_name() == "symbol");
        if is_symbol {
            self.symbol(target, e, depth);
        } else {
            self.element(target, depth);
        }
    }

    /// The instance of the `symbol` element `target`, drawn by the `use`
    /// element `use_element`: a group with a viewport, around the content
    /// of the symbol. The `symbol` itself draws nothing; only an instance
    /// does.
    fn symbol(&mut self, target: NodeId, use_element: &ElementData, depth: usize) {
        let (doc, styles) = (self.doc, self.styles);
        let Some(symbol) = doc.element(target) else {
            return;
        };
        let Some(style) = styles.style_in(self.scope, target) else {
            return;
        };
        if style.display == Display::None || self.too_deep(depth) || !self.budget.take_group() {
            return;
        }
        // A `viewBox` with a zero width or height draws nothing (SVG 2 §8.2).
        if viewport::view_box_disables_rendering(symbol) {
            return;
        }
        let zero = LengthPercentage::Px(0.0);
        // Chromium (measured, `tools/probes/svg-symbol.json`): an attribute
        // of the `use` element that is not a non-negative length (`auto`,
        // `-5`, `foo`, empty) draws nothing, whatever the symbol says; on
        // the symbol, a negative one draws nothing and an invalid one
        // (`auto` too) counts as missing.
        let size = |name: &str| {
            if use_element.attr(name).is_some() {
                return match attribute_length(use_element, name, style.font_size) {
                    Some(v) if !is_negative(&v) => v,
                    _ => zero.clone(),
                };
            }
            match attribute_length(symbol, name, style.font_size) {
                Some(v) if is_negative(&v) => zero.clone(),
                Some(v) => v,
                None => LengthPercentage::Percent(1.0),
            }
        };
        let offset = |name: &str| {
            attribute_length(symbol, name, style.font_size).unwrap_or_else(|| zero.clone())
        };
        // A rotated viewport clips with a path, which takes a layer.
        let clip = !matches!(style.overflow_x, Overflow::Visible | Overflow::Auto)
            && (!self.rotated || self.budget.take_clip_layer());
        let view = SymbolView {
            width: size("width"),
            height: size("height"),
            x: offset("x"),
            y: offset("y"),
            view_box: viewport::view_box(symbol),
            preserve_aspect_ratio: viewport::preserve_aspect_ratio(symbol),
            clip,
        };
        let start = self.nodes.len();
        self.nodes.push(Node::BeginGroup(Box::new(Group {
            style: Some(Arc::clone(style)),
            layer: false,
            node: None,
            clip: None,
            kind: GroupKind::Symbol(view),
        })));
        self.children(target, depth + 1);
        let empty = self.nodes.len() == start + 1;
        self.end_group(start, style, false, empty);
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

    fn shape(&mut self, node: NodeId, e: &ElementData, style: &Arc<ComputedStyle>) {
        if self.budget.shapes.left == 0 {
            self.budget.shapes.warn(format_args!(
                "inline SVG: more than {MAX_SHAPES} shapes in the document; the rest is \
                 not drawn"
            ));
            return;
        }
        let Some(geometry) = self.geometry(e, style.font_size) else {
            return;
        };
        self.budget.shapes.left -= 1;
        let needs_layer = (style.opacity < 1.0 && has_two_paints(style)) || style.has_blend_mode();
        let layered = !needs_layer || self.budget.take_layer();
        let rotated = self.rotated;
        self.rotated |= rotates(style);
        let clip = self.clip_for(style);
        self.rotated = rotated;
        self.nodes.push(Node::Shape(Box::new(Shape {
            style: Arc::clone(style),
            layered,
            geometry,
            node: self.dom_node(node),
            clip,
            hit: self.hit,
        })));
    }

    /// The geometry of shape element `e`. Invalid values make a shape that
    /// draws nothing but still has a bounding box (SVG 2 §10: a negative
    /// size or radius, a missing `d`, ...). `None` if the segment budget
    /// is used up.
    fn geometry(&mut self, e: &ElementData, font_size: f32) -> Option<Geometry> {
        let length = |name: &str| attribute_length(e, name, font_size);
        let zero = || LengthPercentage::ZERO;
        let size = |name: &str| {
            length(name)
                .filter(|v| !is_negative(v))
                .unwrap_or_else(zero)
        };
        Some(match &**e.local_name() {
            "path" => {
                let mut b = path::PathBuilder::new(&mut self.budget.segments.left);
                if let Some(d) = e.attr("d") {
                    path::parse_path_data(d, &mut b);
                }
                let exhausted = b.exhausted;
                let point = b.lone_point();
                let path = b.finish();
                if exhausted {
                    self.budget.segments_ran_out();
                }
                match path {
                    Some(path) => Geometry::Path(Arc::new(path)),
                    None => Geometry::Point(point),
                }
            }
            name @ ("polyline" | "polygon") => {
                let (path, point, exhausted) = path::points_path(
                    e.attr("points").unwrap_or(""),
                    name == "polygon",
                    &mut self.budget.segments.left,
                );
                if exhausted {
                    self.budget.segments_ran_out();
                }
                match path {
                    Some(path) => Geometry::Path(path),
                    None => Geometry::Point(point),
                }
            }
            "rect" => {
                self.charge(10)?;
                Geometry::Rect {
                    x: length("x").unwrap_or_else(zero),
                    y: length("y").unwrap_or_else(zero),
                    width: size("width"),
                    height: size("height"),
                    rx: length("rx").filter(|v| !is_negative(v)),
                    ry: length("ry").filter(|v| !is_negative(v)),
                }
            }
            "circle" => {
                self.charge(6)?;
                Geometry::Circle {
                    cx: length("cx").unwrap_or_else(zero),
                    cy: length("cy").unwrap_or_else(zero),
                    r: size("r"),
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
        if self.budget.segments.left < segments {
            self.budget.segments.left = 0;
            self.budget.segments_ran_out();
            return None;
        }
        self.budget.segments.left -= segments;
        Some(())
    }
}

/// True if the `transform` of `style` rotates or skews (its matrix is not
/// a scale and a translation).
fn rotates(style: &ComputedStyle) -> bool {
    if style.transform.is_empty() {
        return false;
    }
    let m = crate::positioned::style_transform(style, crate::geom::Rect::new(0.0, 0.0, 1.0, 1.0));
    !m.keeps_axes()
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
    let length = svgtypes::Length::from_str(e.attr(name)?.trim()).ok()?;
    if length.unit == svgtypes::LengthUnit::Percent {
        let n = length.number as f32;
        return n.is_finite().then(|| LengthPercentage::Percent(n / 100.0));
    }
    viewport::length_px(&length, font_size)
        .map(|px| LengthPercentage::Px(crate::geom::clamp_length(px)))
}

/// A point from two lengths resolved against the viewport size.
fn resolve_point(x: &LengthPercentage, y: &LengthPercentage, viewport: (f32, f32)) -> Point {
    Point::new(x.resolve(viewport.0), y.resolve(viewport.1))
}

#[cfg(test)]
mod tests;
