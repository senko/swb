//! Upper bounds on the work that usvg does to convert a document, checked
//! before the conversion.
//!
//! The render tree can be much larger than the source, and the conversion
//! can recurse deeper than the source nests:
//!
//! - usvg copies the referenced content for every `<use>` element. Its own
//!   limit (1,000,000 elements) is too high for an image.
//! - usvg converts patterns, masks and clip paths again for each element
//!   that refers to them, unless their units make them shareable, and it
//!   follows chains of such references recursively. It removes only cycles
//!   of one or two references; longer cycles recurse forever.
//! - usvg makes one copy of a marker's content for every vertex of a path
//!   that uses the marker, without a limit; markers inside markers
//!   multiply.
//! - An arc becomes cubic curves whose number grows with the radius
//!   (kurbo's `Arc::append_iter`): one short arc, circle or rounded corner
//!   with a huge radius becomes millions of curves.
//! - Every `<image>` element decodes its `data:` URL.
//! - usvg applies style sheets to every element (see [`css`]), and its
//!   check for recursive links in clip paths, masks, filters and patterns
//!   scans the linked content once per link and again for every recursive
//!   link it removes.
//!
//! The bounds follow usvg's rules for references: a `<use>` refers to the
//! first element with the id; `url()` and other `href` references to the
//! last SVG element with it; ids in `url()` are parsed as svgtypes does.
//! References that rules of style sheets give count for the elements that
//! the rules match. Cycles of `url()` references are rejected. If the
//! document has markers, every vertex of every path, line, polyline and
//! polygon counts as getting the largest marker, and markers nest unless
//! nothing can set a marker property inside a marker. The bounds
//! over-count; they must never under-count what usvg does.

use std::collections::{HashMap, HashSet};

use roxmltree::{Document, Node, NodeId};
use svgtypes::PathSegment;

use super::css::{self, Css, LINK_PROPERTIES};
use super::{DEFAULT_OBJECT_SIZE, SVG_NAMESPACE, length_px};

/// The limits of one document.
pub(super) struct Limits {
    /// The effective nesting depth: element nesting, plus `<use>` copies,
    /// plus [`LINK_DEPTH`] for every other reference.
    pub(super) depth: f64,
    /// The limits of style sheets.
    pub(super) css: css::Limits,
    /// Elements that usvg visits to find recursive links.
    pub(super) link_scan: f64,
}

/// What converting a document uses: elements and path segments of the
/// render tree (with copies and marker instances), and the effective
/// nesting depth.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Usage {
    pub(super) elements: f64,
    pub(super) segments: f64,
    pub(super) depth: f64,
}

/// The depth that a reference to a pattern, mask, clip path, filter or
/// gradient adds: following one costs usvg and resvg more stack than one
/// level of element nesting. Chains of references at the limit fit a 2 MiB
/// stack (tested in `svg/tests.rs`); in a measurement they also fit
/// 512 KiB.
pub(super) const LINK_DEPTH: f64 = 16.0;

/// The cost of an `<image>` element in elements per byte of its `href`
/// (the size of a `data:` URL).
const IMAGE_BYTES_PER_ELEMENT: f64 = 32.0;

/// Elements whose `href` usvg follows (besides `<use>`).
const HREF_ELEMENTS: [&str; 6] = [
    "linearGradient",
    "radialGradient",
    "pattern",
    "filter",
    "feImage",
    "textPath",
];

/// Elements that `url()` references lead to.
const LINK_TARGETS: [&str; 6] = [
    "clipPath",
    "mask",
    "pattern",
    "filter",
    "linearGradient",
    "radialGradient",
];

/// The elements whose links usvg checks for recursion.
const LINK_CHECKED: [&str; 4] = ["clipPath", "mask", "filter", "pattern"];

/// Measures what converting `xml` uses, or describes the first limit that
/// it exceeds.
pub(super) fn measure(xml: &Document<'_>, limits: &Limits) -> Result<Usage, &'static str> {
    let mut analysis = Analysis::new(xml);
    let tree = analysis.tree_size(xml, limits.depth);
    analysis.css = css::analyze(xml, tree.elements, tree.depth, &limits.css)?;
    if analysis.link_scan(xml) > limits.link_scan {
        return Err("too many links in clip paths, masks, filters or patterns");
    }
    let mut counter = Counter {
        analysis: &analysis,
        counts: HashMap::new(),
        path: Vec::new(),
        active: HashSet::new(),
        shared: HashSet::new(),
        shared_total: Count::default(),
        max_depth: limits.depth,
    };
    let mut total = counter.count(xml.root_element(), 0.0, Edge::Tree);
    total.add(counter.shared_total, 0.0);
    counter.add_markers(&mut total);
    if total.depth.is_nan() || total.depth > limits.depth {
        return Err("elements or references nested too deeply, or reference cycles");
    }
    Ok(Usage {
        elements: total.elements,
        segments: total.segments,
        depth: total.depth,
    })
}

/// How an element refers to another.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Link {
    /// `url()` in a property: usvg follows cycles longer than two forever.
    Url,
    /// `href` of a gradient, pattern, filter or `feImage`.
    Href,
}

/// Facts about the document that the counts need.
struct Analysis<'a, 'input> {
    /// Elements by `id` for `<use>`: the first element with an id wins.
    use_ids: HashMap<&'a str, Node<'a, 'input>>,
    /// SVG elements by `id` for other references: the last one wins.
    link_ids: HashMap<&'a str, Node<'a, 'input>>,
    css: Css<'a>,
    markers: Vec<Node<'a, 'input>>,
    /// An upper bound of the viewport size, for percentages.
    viewport: f64,
    /// True if the document sets a font size (`em` units then vary).
    sets_font_size: bool,
}

impl<'a, 'input> Analysis<'a, 'input> {
    fn new(xml: &'a Document<'input>) -> Self {
        let mut use_ids = HashMap::new();
        let mut link_ids = HashMap::new();
        let mut markers = Vec::new();
        let mut viewport = DEFAULT_OBJECT_SIZE.0.max(DEFAULT_OBJECT_SIZE.1);
        let mut sets_font_size = false;
        for node in xml.descendants().filter(Node::is_element) {
            if let Some(id) = node.attribute("id") {
                use_ids.entry(id).or_insert(node);
                if is_svg(node) {
                    link_ids.insert(id, node);
                }
            }
            if node.has_tag_name((SVG_NAMESPACE, "marker")) {
                markers.push(node);
            }
            if node.has_tag_name((SVG_NAMESPACE, "svg")) {
                viewport = viewport.max(svg_extent(node));
            }
            sets_font_size |= node.attributes().any(|a| {
                a.name() == "font-size" || a.name() == "font" || a.value().contains("font")
            }) || node.text().is_some_and(|t| t.contains("font"));
        }
        Analysis {
            use_ids,
            link_ids,
            css: Css::default(),
            markers,
            viewport: f64::from(viewport),
            sets_font_size,
        }
    }

    /// The number of elements and the depth of usvg's tree: the document
    /// with `<use>` copies (style sheets apply to it).
    fn tree_size(&self, xml: &Document<'_>, max_depth: f64) -> Count {
        let mut sizes = HashMap::new();
        let mut active = HashSet::new();
        self.tree_count(xml.root_element(), 0.0, max_depth, &mut sizes, &mut active)
    }

    fn tree_count(
        &self,
        node: Node<'a, 'input>,
        depth: f64,
        max_depth: f64,
        sizes: &mut HashMap<NodeId, Count>,
        active: &mut HashSet<NodeId>,
    ) -> Count {
        if depth > max_depth {
            return Count::infinite();
        }
        if let Some(&count) = sizes.get(&node.id()) {
            return count;
        }
        if !active.insert(node.id()) {
            return Count::default();
        }
        let mut count = Count::leaf(1.0, 0.0, 0.0, false);
        for child in node.children().filter(Node::is_element) {
            let child_count = self.tree_count(child, depth + 1.0, max_depth, sizes, active);
            count.add(child_count, 1.0);
        }
        if let Some(target) = self.use_target(node) {
            let target_count = self.tree_count(target, depth + 2.0, max_depth, sizes, active);
            count.add(target_count, 2.0);
        }
        active.remove(&node.id());
        sizes.insert(node.id(), count);
        count
    }

    /// The target of a `<use>` element.
    fn use_target(&self, node: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
        if !node.has_tag_name((SVG_NAMESPACE, "use")) {
            return None;
        }
        let id = svgtypes::IRI::from_str(href(node)?).ok()?.0;
        self.use_ids.get(id).copied()
    }

    /// The elements that `node` refers to, except through `<use>` and
    /// marker properties (marker instances count per vertex, see
    /// [`Counter::add_markers`]).
    fn references(&self, node: Node<'a, 'input>) -> Vec<(Node<'a, 'input>, Link)> {
        let mut ids: Vec<&'a str> = Vec::new();
        for attribute in node.attributes() {
            if LINK_PROPERTIES.contains(&attribute.name()) {
                ids.extend(url_ids(attribute.value()));
            } else if attribute.name() == "style" {
                for declaration in simplecss::DeclarationTokenizer::from(attribute.value()) {
                    if LINK_PROPERTIES.contains(&declaration.name) {
                        ids.extend(url_ids(declaration.value));
                    }
                }
            }
        }
        if let Some(css_ids) = self.css.references.get(&node.id()) {
            ids.extend(css_ids);
        }
        let mut targets: Vec<(Node<'a, 'input>, Link)> = ids
            .into_iter()
            .filter_map(|id| self.link_ids.get(id).copied())
            .filter(|target| LINK_TARGETS.contains(&target.tag_name().name()))
            .map(|target| (target, Link::Url))
            .collect();
        let follows_href = is_svg(node) && HREF_ELEMENTS.contains(&node.tag_name().name());
        if follows_href
            && let Some(id) = href(node).and_then(|h| svgtypes::IRI::from_str(h).ok())
            && let Some(&target) = self.link_ids.get(id.0)
        {
            targets.push((target, Link::Href));
        }
        targets
    }

    /// True for a reference target that usvg converts once and shares: a
    /// clip path, mask, pattern or gradient whose units do not depend on
    /// the element that refers to it. Patterns and gradients inherit their
    /// units through `href`, as in usvg.
    fn is_shared(&self, target: Node<'a, 'input>) -> bool {
        let user_space = |name: &str, default_user_space: bool| {
            self.inherited(target, name)
                .map_or(default_user_space, |units| units == "userSpaceOnUse")
        };
        match target.tag_name().name() {
            "clipPath" => user_space("clipPathUnits", true),
            "mask" => user_space("maskUnits", false) && user_space("maskContentUnits", true),
            "pattern" => {
                user_space("patternUnits", false) && user_space("patternContentUnits", true)
            }
            "linearGradient" | "radialGradient" => user_space("gradientUnits", false),
            _ => false,
        }
    }

    /// The attribute `name` of `node` or of the elements it links to with
    /// `href` (patterns and gradients inherit attributes this way).
    fn inherited(&self, node: Node<'a, 'input>, name: &str) -> Option<&'a str> {
        let mut current = node;
        // usvg stops at loops; a bounded walk is enough here.
        for _ in 0..64 {
            if let Some(value) = current.attribute(name) {
                return Some(value);
            }
            let id = svgtypes::IRI::from_str(href(current)?).ok()?.0;
            current = self.link_ids.get(id).copied()?;
        }
        // A longer chain: assume the content depends on the element.
        Some("objectBoundingBox")
    }

    /// The `url()` links of an element that usvg checks for recursion: to
    /// clip paths, masks, filters and patterns.
    fn checked_links(&self, node: Node<'a, 'input>) -> Vec<Node<'a, 'input>> {
        self.references(node)
            .into_iter()
            .filter(|(target, link)| {
                *link == Link::Url && LINK_CHECKED.contains(&target.tag_name().name())
            })
            .map(|(target, _)| target)
            .collect()
    }

    /// For every node, by id: the number of its descendants, and whether
    /// it or a descendant has checked links.
    fn subtree_facts(&self, nodes: &[Node<'a, 'input>]) -> (Vec<f64>, Vec<bool>) {
        let index = |n: Node<'_, '_>| n.id().get_usize();
        let mut sizes = vec![0.0_f64; nodes.len()];
        let mut linking = vec![false; nodes.len()];
        // Children come after their parent in `nodes`.
        for &node in nodes.iter().rev() {
            let size = sizes.get(index(node)).copied().unwrap_or(0.0);
            let links = linking.get(index(node)).copied().unwrap_or(false)
                || (node.is_element() && !self.checked_links(node).is_empty());
            if let Some(slot) = linking.get_mut(index(node)) {
                *slot = links;
            }
            let Some(parent) = node.parent() else {
                continue;
            };
            if let Some(slot) = sizes.get_mut(index(parent)) {
                *slot += size + 1.0;
            }
            if let Some(slot) = linking.get_mut(index(parent)) {
                *slot |= links;
            }
        }
        (sizes, linking)
    }

    /// The elements that usvg's search for recursive links visits
    /// (`fix_recursive_links`): for each clip path, mask, filter and
    /// pattern, its descendants and the content of every element they link
    /// to; times the number of searches (one per link that may be
    /// recursive, plus one).
    fn link_scan(&self, xml: &'a Document<'input>) -> f64 {
        let nodes: Vec<Node<'a, 'input>> = xml.descendants().collect();
        let index = |n: Node<'_, '_>| n.id().get_usize();
        let (sizes, linking) = self.subtree_facts(&nodes);
        let size = |n: Node<'_, '_>| sizes.get(index(n)).copied().unwrap_or(0.0);
        // The number of checked elements around each node (itself
        // included): each of them scans the node.
        let mut checked = vec![0.0_f64; nodes.len()];
        let mut scan = 0.0;
        let mut recursive = 0.0;
        for &node in &nodes {
            let own = f64::from(u8::from(
                is_svg(node) && LINK_CHECKED.contains(&node.tag_name().name()),
            ));
            let above = node
                .parent()
                .and_then(|p| checked.get(index(p)).copied())
                .unwrap_or(0.0);
            let depth = above + own;
            if let Some(slot) = checked.get_mut(index(node)) {
                *slot = depth;
            }
            scan += own * size(node);
            if depth == 0.0 || !node.is_element() {
                continue;
            }
            for target in self.checked_links(node) {
                // A link can be recursive if it links back to an ancestor or
                // its target links on.
                let may_recurse = linking.get(index(target)).copied().unwrap_or(true)
                    || node.ancestors().any(|a| a == target);
                recursive += f64::from(u8::from(may_recurse));
                scan += depth * (1.0 + size(target));
            }
        }
        scan * (recursive + 1.0)
    }

    /// The number of path segments of a shape.
    fn shape_segments(&self, node: Node<'_, '_>) -> f64 {
        if !is_svg(node) {
            return 0.0;
        }
        let radius = |names: &[&str]| -> f64 {
            names
                .iter()
                .filter_map(|name| node.attribute(*name))
                .map(|value| self.radius(value))
                .fold(0.0, f64::max)
        };
        match node.tag_name().name() {
            "path" => node.attribute("d").map_or(0.0, path_segments),
            "polyline" | "polygon" => node.attribute("points").map_or(0.0, |p| {
                svgtypes::PointsParser::from(p).count() as f64 + 1.0
            }),
            "line" => 1.0,
            // Four quarter arcs, the lines between them, the move and the
            // close.
            "rect" => arc_segments(radius(&["rx", "ry"])) + 8.0,
            "circle" => arc_segments(radius(&["r"])) + 6.0,
            "ellipse" => arc_segments(radius(&["rx", "ry"])) + 6.0,
            _ => 0.0,
        }
    }

    /// An upper bound of a radius in user units: percentages of the largest
    /// viewport, `em` and `ex` of the initial font size (or unbounded if the
    /// document sets font sizes).
    fn radius(&self, value: &str) -> f64 {
        let Ok(length) = value.parse::<svgtypes::Length>() else {
            return 0.0;
        };
        match length.unit {
            svgtypes::LengthUnit::Percent => length.number.abs() / 100.0 * self.viewport * 1.5,
            svgtypes::LengthUnit::Em | svgtypes::LengthUnit::Ex if self.sets_font_size => {
                f64::INFINITY
            }
            _ => length_px(value).map_or(0.0, f64::abs),
        }
    }
}

/// The size of an element's render tree.
#[derive(Clone, Copy, Debug, Default)]
struct Count {
    elements: f64,
    segments: f64,
    /// Vertices that can get a marker.
    vertices: f64,
    /// The effective nesting depth (see [`Limits::depth`]).
    depth: f64,
    /// True if an element sets a marker property.
    sets_markers: bool,
}

impl Count {
    fn leaf(elements: f64, segments: f64, vertices: f64, sets_markers: bool) -> Self {
        Count {
            elements,
            segments,
            vertices,
            depth: 1.0,
            sets_markers,
        }
    }

    /// A count that exceeds every limit.
    fn infinite() -> Self {
        Count {
            depth: f64::INFINITY,
            ..Count::default()
        }
    }

    /// Adds a copy of `other`, nested `depth` levels below this element.
    fn add(&mut self, other: Count, depth: f64) {
        self.elements += other.elements;
        self.segments += other.segments;
        self.vertices += other.vertices;
        self.depth = self.depth.max(depth + other.depth);
        self.sets_markers |= other.sets_markers;
    }
}

/// How the counter reached an element.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    /// As a child, or through `<use>`: usvg's tree, whose depth usvg limits.
    Tree,
    /// Through a `url()` reference.
    Url,
    /// Through the `href` of a gradient, pattern, filter or `feImage`.
    Href,
}

struct Counter<'c, 'a, 'input> {
    analysis: &'c Analysis<'a, 'input>,
    counts: HashMap<NodeId, Count>,
    /// The elements being counted and how each was reached, to find
    /// cycles.
    path: Vec<(NodeId, Edge)>,
    active: HashSet<NodeId>,
    /// Targets that usvg converts once and shares, and their total.
    shared: HashSet<NodeId>,
    shared_total: Count,
    max_depth: f64,
}

impl<'a, 'input> Counter<'_, 'a, 'input> {
    /// The count of `node`, reached through `edge` at the effective depth
    /// `depth`. The recursion stops beyond the depth limit, so it is
    /// bounded too.
    fn count(&mut self, node: Node<'a, 'input>, depth: f64, edge: Edge) -> Count {
        if depth > self.max_depth {
            return Count::infinite();
        }
        if let Some(&count) = self.counts.get(&node.id()) {
            return count;
        }
        if self.active.contains(&node.id()) {
            return self.cycle(node, edge);
        }
        self.active.insert(node.id());
        self.path.push((node.id(), edge));
        let count = self.count_new(node, depth);
        self.path.pop();
        self.active.remove(&node.id());
        self.counts.insert(node.id(), count);
        count
    }

    /// The count of a cycle that `edge` closes at the active `node`. usvg
    /// stops cycles of `<use>` elements (at its depth limit) and of `href`
    /// links alone; it follows every other cycle forever, so those are
    /// rejected.
    fn cycle(&self, node: Node<'a, 'input>, edge: Edge) -> Count {
        let start = self.path.iter().rposition(|(id, _)| *id == node.id());
        let edges = start
            .and_then(|i| self.path.get(i + 1..))
            .unwrap_or_default()
            .iter()
            .map(|(_, e)| *e)
            .chain([edge]);
        let mut kinds = (false, false, false);
        for e in edges {
            match e {
                Edge::Tree => kinds.0 = true,
                Edge::Url => kinds.1 = true,
                Edge::Href => kinds.2 = true,
            }
        }
        match kinds {
            (_, false, false) | (false, false, true) => Count::default(),
            _ => Count::infinite(),
        }
    }

    /// Counts an element that has no count yet.
    fn count_new(&mut self, node: Node<'a, 'input>, depth: f64) -> Count {
        let analysis = self.analysis;
        let segments = analysis.shape_segments(node);
        let vertices = if !analysis.markers.is_empty() && is_markable(node) {
            segments + 2.0
        } else {
            0.0
        };
        let elements = 1.0 + image_cost(node);
        let mut count = Count::leaf(elements, segments, vertices, sets_markers(node));
        for child in node.children().filter(Node::is_element) {
            let child_count = self.count(child, depth + 1.0, Edge::Tree);
            count.add(child_count, 1.0);
        }
        if let Some(target) = analysis.use_target(node) {
            let target_count = self.count(target, depth + 2.0, Edge::Tree);
            count.add(target_count, 2.0);
        }
        for (target, link) in analysis.references(node) {
            let edge = match link {
                Link::Url => Edge::Url,
                Link::Href => Edge::Href,
            };
            let target_count = self.count(target, depth + LINK_DEPTH, edge);
            if analysis.is_shared(target) {
                // usvg converts a shared target once, but walks its tree for
                // every use afterwards (paint servers, bounding boxes): its
                // elements count per use, its segments and markers once.
                if self.shared.insert(target.id()) {
                    self.shared_total.add(target_count, 0.0);
                }
                let walked = Count {
                    segments: 0.0,
                    vertices: 0.0,
                    ..target_count
                };
                count.add(walked, LINK_DEPTH);
            } else {
                count.add(target_count, LINK_DEPTH);
            }
        }
        count
    }

    /// Adds the marker instances to `total`: a chain of up to one instance
    /// of each marker per vertex.
    fn add_markers(&mut self, total: &mut Count) {
        let markers = self.analysis.markers.clone();
        if markers.is_empty() {
            return;
        }
        let mut largest = Count::default();
        let mut nested = self.analysis.css.mentions_markers;
        for &marker in &markers {
            let count = self.count(marker, 0.0, Edge::Tree);
            largest.elements = largest.elements.max(count.elements);
            largest.segments = largest.segments.max(count.segments);
            largest.vertices = largest.vertices.max(count.vertices);
            largest.depth = largest.depth.max(count.depth);
            nested |= count.sets_markers || marker.ancestors().any(sets_markers);
        }
        let mut per_vertex = 1.0;
        if nested {
            let mut level = 1.0;
            for _ in 1..markers.len() {
                level *= largest.vertices;
                per_vertex += level;
            }
        }
        let instances = total.vertices * per_vertex;
        total.elements += instances * largest.elements;
        total.segments += instances * largest.segments;
        // Nested markers recurse in usvg.
        let levels = if nested { markers.len() as f64 } else { 1.0 };
        total.depth = total.depth.max(levels * (LINK_DEPTH + largest.depth));
    }
}

/// True for elements in the SVG namespace.
fn is_svg(node: Node<'_, '_>) -> bool {
    node.tag_name().namespace() == Some(SVG_NAMESPACE)
}

/// The `href` of an element: plain `href` before `xlink:href`, as in usvg.
fn href<'a>(node: Node<'a, '_>) -> Option<&'a str> {
    const XLINK: &str = "http://www.w3.org/1999/xlink";
    node.attributes()
        .find(|a| a.name() == "href" && a.namespace().is_none())
        .or_else(|| {
            node.attributes()
                .find(|a| a.name() == "href" && a.namespace() == Some(XLINK))
        })
        .map(|a| a.value())
}

/// The ids in the `url(#id)` references of a property value, parsed as
/// svgtypes does (`parse_func_iri`): an unquoted id ends at a space or `)`,
/// a quoted one at the quote.
pub(super) fn url_ids(text: &str) -> impl Iterator<Item = &str> {
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r');
    text.split("url(").skip(1).filter_map(move |after| {
        let after = after.trim_start_matches(is_space);
        let (quote, after) = match after.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), after[1..].trim_start_matches(is_space)),
            _ => (None, after),
        };
        let id = after.strip_prefix('#')?;
        let id = match quote {
            Some(q) => id.split(q).next().unwrap_or(id).trim_end(),
            None => id.split([' ', ')']).next().unwrap_or(id),
        };
        (!id.is_empty()).then_some(id)
    })
}

/// The width or height of an `<svg>` element, or its view box, whichever
/// is largest, in user units.
fn svg_extent(node: Node<'_, '_>) -> f32 {
    let mut extent = 0.0_f64;
    for name in ["width", "height"] {
        if let Some(px) = node.attribute(name).and_then(length_px) {
            extent = extent.max(px.abs());
        }
    }
    if let Some(v) = node
        .attribute("viewBox")
        .and_then(|v| v.parse::<svgtypes::ViewBox>().ok())
    {
        extent = extent.max(v.w.abs()).max(v.h.abs());
    }
    extent as f32
}

/// True for elements that can have markers.
fn is_markable(node: Node<'_, '_>) -> bool {
    is_svg(node) && ["path", "line", "polyline", "polygon"].contains(&node.tag_name().name())
}

/// True if the element sets a marker property, as an attribute or in its
/// `style` attribute.
fn sets_markers(node: Node<'_, '_>) -> bool {
    node.attributes().any(|a| {
        matches!(
            a.name(),
            "marker" | "marker-start" | "marker-mid" | "marker-end"
        ) || (a.name() == "style" && a.value().contains("marker"))
    })
}

/// The extra cost of an `<image>` element: its `data:` URL is decoded.
fn image_cost(node: Node<'_, '_>) -> f64 {
    if !node.has_tag_name((SVG_NAMESPACE, "image")) {
        return 0.0;
    }
    href(node).map_or(0.0, |h| h.len() as f64 / IMAGE_BYTES_PER_ELEMENT)
}

/// The number of segments of path data after the conversion of arcs to
/// cubic curves.
fn path_segments(data: &str) -> f64 {
    let mut segments = 0.0;
    let (mut x, mut y) = (0.0_f64, 0.0_f64);
    let (mut start_x, mut start_y) = (0.0, 0.0);
    for segment in svgtypes::PathParser::from(data) {
        let Ok(segment) = segment else {
            break;
        };
        let (abs, end) = match segment {
            PathSegment::MoveTo { abs, x, y }
            | PathSegment::LineTo { abs, x, y }
            | PathSegment::CurveTo { abs, x, y, .. }
            | PathSegment::SmoothCurveTo { abs, x, y, .. }
            | PathSegment::Quadratic { abs, x, y, .. }
            | PathSegment::SmoothQuadratic { abs, x, y }
            | PathSegment::EllipticalArc { abs, x, y, .. } => (abs, (x, y)),
            PathSegment::HorizontalLineTo { abs, x: to } => (abs, (to, if abs { y } else { 0.0 })),
            PathSegment::VerticalLineTo { abs, y: to } => (abs, (if abs { x } else { 0.0 }, to)),
            PathSegment::ClosePath { .. } => (true, (start_x, start_y)),
        };
        let (end_x, end_y) = if abs { end } else { (x + end.0, y + end.1) };
        segments += match segment {
            PathSegment::EllipticalArc { rx, ry, .. } if rx != 0.0 && ry != 0.0 => {
                let chord = (end_x - x).hypot(end_y - y);
                arc_segments(rx.abs().max(ry.abs()).max(chord))
            }
            _ => 1.0,
        };
        if let PathSegment::MoveTo { .. } = segment {
            (start_x, start_y) = (end_x, end_y);
        }
        (x, y) = (end_x, end_y);
    }
    segments
}

/// An upper bound on the number of cubic curves that kurbo makes for arcs
/// of up to a full turn with the radius `radius` (`Arc::append_iter` with
/// usvg's tolerance of 0.1), plus one per quarter turn for rounding.
fn arc_segments(radius: f64) -> f64 {
    (1.1163 * radius / 0.1).powf(1.0 / 6.0).max(4.0).ceil() + 4.0
}

#[cfg(test)]
mod tests;
