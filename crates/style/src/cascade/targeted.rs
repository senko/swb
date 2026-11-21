//! The restyle after a change of the element states (ADR 0041 section 2)
//! and after DOM changes (ADR 0042): only the elements that the analysis
//! marks, the descendants of its roots that have one of the roots' subject
//! keys, and the children of elements whose style changed are matched and
//! cascaded again; the other styles stay, and the styles of nodes that left
//! the document go.
//!
//! [`restyle_targeted`] walks the flat tree from the document element like
//! `Styler::run`, with the same per-element step, but enters only the
//! elements on the way to work: so the ancestor Bloom filter, the tree
//! scopes, the query container stack and the root font size are those of
//! a full walk. Nothing is written to the style map until the walk ends;
//! a case that this pass cannot handle exactly ([`FullReason`]) leaves the
//! map as it was, and the caller restyles the whole document.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use swb_css::MediaEnvironment;
use swb_dom::{Document, FlatScopes, NodeId};
use url::Url;

use super::Styler;
use super::use_instances::is_svg_use;
use crate::ComputedStyle;
use crate::container::ContainerSizes;
use crate::dom_changes::{DomChanges, plan_dom};
use crate::element::{DomElement, ElementStates, StateSet};
use crate::invalidation::{KeySet, Plan, Planner, TARGETED_STATES, flipped};
use crate::style_map::{PseudoKind, PseudoStyles, StyleMap, image_urls_of};
use crate::stylist::Stylist;
use crate::values::{Content, ContentItem, Display};

/// Why a state change needs a restyle of the whole document (ADR 0041
/// section 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FullReason {
    /// A state other than hover, active, focus, focus-visible and
    /// focus-within changed, or the dimension sources of images changed.
    OtherStates,
    /// A changed element has a dependency that the analysis does not
    /// handle (`:nth-child(of S)`, some `:has()` shapes).
    Everything,
    /// The root element's font size changed (`rem`).
    RootFontSize,
    /// A restyled element read a query container, or a query container
    /// changed.
    Containers,
    /// Counter values may change while pseudo-elements hold counter text,
    /// or the counter text reached its limit.
    Counters,
    /// Too many DOM changes, or the marking would do more work than a full
    /// restyle (ADR 0042 section 5).
    TooLarge,
    /// The document element or the `body` element was inserted or removed
    /// (ADR 0042 section 5).
    Structure,
}

/// The inputs of a restyle that stay the same between the old and the new
/// styles: they must be those of the style map that is brought up to date
/// (the container sizes may be those of a later layout whose log held).
#[derive(Clone, Copy)]
pub struct RestyleInputs<'a> {
    /// The document.
    pub doc: &'a Document,
    /// The stylist.
    pub stylist: &'a Stylist,
    /// The media environment.
    pub env: &'a MediaEnvironment,
    /// The base URL of the author styles.
    pub base_url: &'a Url,
    /// The sizes of the query containers.
    pub sizes: &'a ContainerSizes,
}

/// The result of [`restyle_states`] and [`restyle_targeted`].
#[derive(Clone, Debug, PartialEq)]
pub enum Restyle {
    /// The map is up to date for the new states.
    Done {
        /// True if a style, an ordinal or a `use` instance changed.
        changed: bool,
        /// The elements that were matched and cascaded again.
        restyled: usize,
        /// The image URLs of the new styles (the others were seen before).
        image_urls: Vec<Arc<str>>,
    },
    /// The map is unchanged; restyle the whole document.
    Full(FullReason),
}

/// Brings `map`, the styles of `doc` for the states `old`, up to date for
/// the states `new`, restyling only the elements that the change can affect
/// (ADR 0041). The other inputs (the DOM, the stylist, the environment,
/// the container sizes) must be those of `map` (the container sizes may be
/// those of a later layout whose log held). On [`Restyle::Full`], `map` is
/// unchanged.
pub fn restyle_states(
    inputs: &RestyleInputs<'_>,
    old: &ElementStates,
    new: &ElementStates,
    map: &mut StyleMap,
) -> Restyle {
    restyle_targeted(inputs, old, new, &DomChanges::default(), map)
}

/// Brings `map`, the styles of `doc` before the DOM changes `dom` and for
/// the states `old`, up to date for the current DOM and the states `new`,
/// restyling only the elements that the changes can affect (ADR 0041, ADR
/// 0042). The other inputs as for [`restyle_states`]. On [`Restyle::Full`],
/// `map` is unchanged.
pub fn restyle_targeted(
    inputs: &RestyleInputs<'_>,
    old: &ElementStates,
    new: &ElementStates,
    dom: &DomChanges,
    map: &mut StyleMap,
) -> Restyle {
    let RestyleInputs {
        doc,
        stylist,
        env,
        base_url,
        sizes,
    } = *inputs;
    let unchanged = Restyle::Done {
        changed: false,
        restyled: 0,
        image_urls: Vec::new(),
    };
    let changed = old.changes(new) & stylist.state_dependencies();
    let same_sources = Arc::ptr_eq(&old.dimension_sources, &new.dimension_sources)
        || old.dimension_sources == new.dimension_sources;
    if !same_sources || !(changed - TARGETED_STATES).is_empty() {
        return Restyle::Full(FullReason::OtherStates);
    }
    if let Some(reason) = dom.full_reason() {
        return Restyle::Full(reason);
    }
    if changed.is_empty() && dom.is_empty() {
        return unchanged;
    }
    // The sizes must be ones that the log holds: after the bound of the
    // container passes (ADR 0038 section 4) a query result can be out of
    // date, and only a full restyle evaluates it again with `sizes`.
    if !map.container_log().holds(sizes) {
        return Restyle::Full(FullReason::Containers);
    }
    if !dom.is_empty() && dom.too_many_added(doc, map.styled_count()) {
        return Restyle::Full(FullReason::TooLarge);
    }
    let index = stylist.invalidation_index();
    // The marking may do at most about the work of a full restyle: a unit
    // of marking (a key check, a lookup) costs far less than the cascade of
    // an element.
    let budget = doc.len().saturating_mul(16).saturating_add(10_000);
    let mut planner = Planner::new(doc, index, stylist.quirks(), budget);
    if !changed.is_empty() {
        planner.flip_states(&flipped(doc, old, new, changed));
    }
    if !dom.is_empty() && !planner.done() {
        plan_dom(&mut planner, dom, old, new, map.has_use_elements());
    }
    let plan = planner.finish();
    if plan.everything {
        return Restyle::Full(FullReason::Everything);
    }
    if plan.too_large {
        return Restyle::Full(FullReason::TooLarge);
    }
    if plan.is_empty() && dom.is_empty() {
        return unchanged;
    }
    // The tables of a full state set cost a pass over the document; the
    // lazy set answers the few elements of a targeted restyle without it.
    let states = StateSet::lazy(doc, new);
    let mut styler = Styler::new(doc, &states, stylist, env, base_url, true, sizes, None);
    let changes = styler
        .run_targeted(map, &plan)
        .and_then(|changes| styler.collect_stale(map, dom, changes));
    match changes {
        Ok(changes) => styler.finish_targeted(&changes, &plan, map, sizes),
        Err(reason) => Restyle::Full(reason),
    }
}

/// An element that the walk visits.
struct Visit {
    node: NodeId,
    /// The parent's style (kept or new).
    parent: Arc<ComputedStyle>,
    /// The display of the parent box, now and in the old styles.
    layout_parent: Option<Display>,
    old_layout_parent: Option<Display>,
    depth: usize,
    scope: NodeId,
    /// True if the parent's style or the box it passes on changed: the
    /// element is restyled.
    parent_changed: bool,
    /// True inside an inserted subtree (ADR 0042): a moved element with an
    /// old style counts as appearing for the counter pass.
    inserted: bool,
    /// The subject keys of the roots above the element.
    active: Option<Rc<KeySet>>,
}

/// What the walk computed, applied at the end.
#[derive(Default)]
struct Changes {
    /// The new styles of the elements whose style changed.
    elements: Vec<(NodeId, Arc<ComputedStyle>)>,
    /// Elements that became `display: none`: their descendants lose their
    /// styles.
    cleared: Vec<NodeId>,
    /// The restyled elements whose pseudo-elements are replaced by those in
    /// `pseudos`.
    pseudo_nodes: Vec<NodeId>,
    pseudos: StyleMap,
    /// A counter input changed (ADR 0041 section 2).
    counter_inputs: bool,
    /// A new pseudo-element style has `counter()` or `counters()`.
    counter_content: bool,
    /// The restyled elements and those that lost their styles.
    touched: Vec<NodeId>,
    /// The restyled elements (they have a style after the pass).
    styled: NodeSet,
    /// Nodes that left the document or are no longer rendered after DOM
    /// changes: their styles go (ADR 0042 section 3, "Removed styles").
    stale: Vec<NodeId>,
    /// A descendant reach of the walk ended at a `display: none` element:
    /// elements below it have no style in the map, but the instance trees
    /// of `use` elements that copy them may change.
    hidden_reach: bool,
    restyled: usize,
}

/// True if a style that appears or goes away (a new or removed element or
/// pseudo-element) changes what the counter pass computes: a list item or
/// counter properties. Another element without a box of its own for
/// counters changes no counter value (ADR 0042 section 4).
fn is_counter_input(style: &ComputedStyle) -> bool {
    style.display == Display::ListItem
        || !style.counter_reset.is_empty()
        || !style.counter_increment.is_empty()
        || !style.counter_set.is_empty()
}

/// The values of a style that the counter pass reads (ADR 0041 section 2):
/// whether it has a box, its display as far as counters care, and the
/// counter properties.
fn counter_inputs_differ(a: Option<&ComputedStyle>, b: Option<&ComputedStyle>) -> bool {
    let kind = |d: Display| match d {
        Display::None => 0,
        Display::Contents => 1,
        Display::ListItem => 2,
        _ => 3,
    };
    match (a, b) {
        (None, None) => false,
        (Some(a), Some(b)) => {
            kind(a.display) != kind(b.display)
                || a.counter_reset != b.counter_reset
                || a.counter_increment != b.counter_increment
                || a.counter_set != b.counter_set
        }
        (Some(s), None) | (None, Some(s)) => is_counter_input(s),
    }
}

/// True if the style's `content` has `counter()` or `counters()`.
fn has_counters(style: &ComputedStyle) -> bool {
    matches!(&style.content, Content::Items(items)
        if items.iter().any(|i| matches!(i, ContentItem::Counter { .. })))
}

/// Notes whether the new pseudo-elements of `node` change a counter input
/// or hold counters.
fn note_pseudo_counters(map: &StyleMap, node: NodeId, inserted: bool, changes: &mut Changes) {
    let (olds, news) = (
        map.pseudo_styles_of(node),
        changes.pseudos.pseudo_styles_of(node),
    );
    if olds.is_none() && news.is_none() {
        return;
    }
    for kind in [PseudoKind::Before, PseudoKind::After, PseudoKind::Marker] {
        let old = olds.and_then(|p| p[kind as usize].as_ref());
        let new = news.and_then(|p| p[kind as usize].as_ref());
        changes.counter_inputs |= counter_inputs_differ(old.map(|s| &**s), new.map(|s| &**s));
        // A moved element's pseudo-elements move with it.
        changes.counter_inputs |=
            inserted && old.into_iter().chain(new).any(|s| is_counter_input(s));
        changes.counter_content |= new.is_some_and(|s| has_counters(s));
    }
}

/// A set of nodes, as bits by node index: the walk inserts every restyled
/// element, so a hash set would cost a hash per element.
#[derive(Debug, Default)]
struct NodeSet(Vec<u64>);

impl NodeSet {
    fn insert(&mut self, node: NodeId) {
        let (word, bit) = (node.index() / 64, node.index() % 64);
        if word >= self.0.len() {
            self.0.resize(word + 1, 0);
        }
        if let Some(w) = self.0.get_mut(word) {
            *w |= 1 << bit;
        }
    }

    fn contains(&self, node: NodeId) -> bool {
        let (word, bit) = (node.index() / 64, node.index() % 64);
        self.0.get(word).is_some_and(|w| w & (1 << bit) != 0)
    }
}

/// True if both styles are the same object or equal.
fn same(a: &Arc<ComputedStyle>, b: &Arc<ComputedStyle>) -> bool {
    Arc::ptr_eq(a, b) || **a == **b
}

/// The most keys of an active key set. A walk below many nested roots with
/// different keys would copy a growing set at each root (quadratic in the
/// depth); a larger set becomes "any element", which restyles more
/// elements, never fewer.
const MAX_ACTIVE_KEYS: usize = 256;

/// The active key set below `node`: `active` with the keys of `node` if it
/// is a root (a new set only if the root adds keys).
fn enter_root(active: Option<Rc<KeySet>>, root: Option<&KeySet>) -> Option<Rc<KeySet>> {
    let Some(root) = root else {
        return active;
    };
    match active {
        Some(a) if a.contains_all(root) => Some(a),
        Some(a) if a.len() + root.len() > MAX_ACTIVE_KEYS => Some(Rc::new(KeySet::any())),
        Some(a) => {
            let mut merged = (*a).clone();
            merged.merge(root);
            Some(Rc::new(merged))
        }
        None => Some(Rc::new(root.clone())),
    }
}

/// What the walk does below a visited element: the style that its children
/// inherit, the boxes it passes on (now and before), and whether its
/// children are restyled.
struct Descend {
    style: Arc<ComputedStyle>,
    layout_parent: Option<Display>,
    old_layout_parent: Option<Display>,
    children_changed: bool,
    active: Option<Rc<KeySet>>,
    /// True if the element was pushed on the query container stack.
    pushed: bool,
    /// True inside an inserted subtree.
    inserted: bool,
}

impl Styler<'_> {
    /// The pruned walk (ADR 0041 section 2). Fails with the reason of a
    /// late fallback.
    fn run_targeted(&mut self, map: &StyleMap, plan: &Plan) -> Result<Changes, FullReason> {
        let mut changes = Changes {
            pseudos: StyleMap::with_capacity(0),
            ..Changes::default()
        };
        let Some(root) = self.doc.document_element() else {
            return Ok(changes);
        };
        // The walk does not read the `style` attributes of the elements it
        // skips; this only turns on the tracking of container unit reads.
        self.attribute_units = true;
        let quirks = self.cx.quirks;
        let shadow = self.doc.has_shadow_state();
        let mut stack = vec![Visit {
            node: root,
            parent: Arc::clone(&self.cx.initial),
            layout_parent: None,
            old_layout_parent: None,
            depth: 0,
            scope: NodeId::DOCUMENT,
            parent_changed: false,
            inserted: false,
            active: None,
        }];
        let mut path: Vec<NodeId> = Vec::new();
        let mut children = Vec::new();
        let mut scopes = FlatScopes::default();
        while let Some(visit) = stack.pop() {
            while path.len() > visit.depth {
                if let Some(ancestor) = path.pop() {
                    self.containers.pop_if(ancestor);
                    if let (Some(filter), Some(e)) = (&mut self.filter, self.doc.element(ancestor))
                    {
                        filter.pop_element(e, quirks);
                    }
                }
            }
            let node = visit.node;
            let Some(el) = DomElement::new(self.doc, node, self.states) else {
                continue;
            };
            let Some(descend) = self.visit_targeted(&el, &visit, root, map, plan, &mut changes)?
            else {
                continue;
            };
            let all = descend.children_changed || descend.active.is_some();
            children.clear();
            children.extend(self.doc.flat_element_children(node).filter(|c| {
                all || plan.below.contains(c)
                    || plan.marked.contains(c)
                    || plan.roots.contains_key(c)
            }));
            if !children.is_empty() {
                if let Some(filter) = &mut self.filter {
                    filter.push_element(el.data(), quirks);
                }
                path.push(node);
            } else if descend.pushed {
                self.containers.pop_if(node);
            }
            let child_scope = if shadow {
                scopes.of_children(self.doc, node, visit.scope)
            } else {
                visit.scope
            };
            for &child in children.iter().rev() {
                stack.push(Visit {
                    node: child,
                    parent: Arc::clone(&descend.style),
                    layout_parent: descend.layout_parent,
                    old_layout_parent: descend.old_layout_parent,
                    depth: visit.depth + 1,
                    scope: child_scope,
                    parent_changed: descend.children_changed,
                    inserted: descend.inserted,
                    active: descend.active.clone(),
                });
            }
        }
        Ok(changes)
    }

    /// Visits one element of the walk: restyles it if it needs it (with its
    /// pseudo-elements) and says what to do below it; `None` if the walk does
    /// not go below it (no style, or `display: none`).
    fn visit_targeted(
        &mut self,
        el: &DomElement<'_>,
        visit: &Visit,
        root: NodeId,
        map: &StyleMap,
        plan: &Plan,
        changes: &mut Changes,
    ) -> Result<Option<Descend>, FullReason> {
        let node = el.node_id();
        let is_root = node == root;
        self.scope = visit.scope;
        let active = enter_root(visit.active.clone(), plan.roots.get(&node));
        let inserted = visit.inserted || plan.inserted.contains(&node);
        let old = map.get(node).cloned();
        let restyle = visit.parent_changed
            || plan.marked.contains(&node)
            || active
                .as_ref()
                .is_some_and(|a| a.matches(el.data(), self.cx.quirks));
        let (style, changed) = if restyle {
            self.restyle_one(el, visit, is_root, old.as_ref(), map, changes)
                .map(|(style, changed)| {
                    // A moved element changes the counters where it left
                    // and where it arrived.
                    changes.counter_inputs |= inserted
                        && (is_counter_input(&style)
                            || old.as_deref().is_some_and(is_counter_input));
                    (style, changed)
                })?
        } else {
            let Some(old) = old.clone() else {
                // No style: not rendered, as in a full restyle.
                return Ok(None);
            };
            (old, false)
        };
        if is_root {
            self.cx.root_font_size = style.font_size;
            self.cx.root_line_height = style.line_height_px();
            self.containers.set_root_font_size(style.font_size);
        }
        if style.display == Display::None {
            changes.hidden_reach |= active.is_some();
            if restyle {
                changes.pseudo_nodes.push(node);
                if old.as_ref().is_some_and(|o| o.display != Display::None) {
                    self.clear_subtree(map, node, changes)?;
                }
            }
            return Ok(None);
        }
        let pushed = self.containers.push(node, &style);
        let layout_parent = if style.display == Display::Contents {
            visit.layout_parent
        } else {
            Some(style.display)
        };
        let old_layout_parent = match &old {
            Some(o) if o.display == Display::Contents => visit.old_layout_parent,
            Some(o) => Some(o.display),
            None => None,
        };
        if restyle {
            self.style_pseudo_elements(el, &style, layout_parent, &mut changes.pseudos);
            if self.containers.take_read() {
                return Err(FullReason::Containers);
            }
            changes.pseudo_nodes.push(node);
            note_pseudo_counters(map, node, inserted, changes);
        }
        Ok(Some(Descend {
            children_changed: restyle && (changed || layout_parent != old_layout_parent),
            style,
            layout_parent,
            old_layout_parent,
            active,
            pushed,
            inserted,
        }))
    }

    /// Matches and cascades one element again. Returns its style (the old
    /// object if equal) and whether it changed; fails on a late fallback.
    fn restyle_one(
        &mut self,
        el: &DomElement<'_>,
        visit: &Visit,
        is_root: bool,
        old: Option<&Arc<ComputedStyle>>,
        map: &StyleMap,
        changes: &mut Changes,
    ) -> Result<(Arc<ComputedStyle>, bool), FullReason> {
        let node = el.node_id();
        if map.is_container_reader(node) {
            return Err(FullReason::Containers);
        }
        self.containers.take_read();
        // The custom-property pool gets the element's old map, so that an
        // unchanged map keeps its object (ADR 0007): only these, not the maps
        // of the whole document.
        if let Some(custom) = old.and_then(|o| o.custom_properties.as_ref()) {
            self.cx.custom_pool.seed(custom);
        }
        let new = self.style_element(el, &visit.parent, visit.layout_parent, is_root);
        if self.containers.take_read() {
            return Err(FullReason::Containers);
        }
        changes.restyled += 1;
        changes.touched.push(node);
        changes.styled.insert(node);
        let Some(old) = old else {
            if new.is_query_container() {
                return Err(FullReason::Containers);
            }
            changes.counter_inputs |= is_counter_input(&new);
            changes.elements.push((node, Arc::clone(&new)));
            return Ok((new, true));
        };
        if same(old, &new) {
            return Ok((Arc::clone(old), false));
        }
        // `rem` and `rlh` of every element depend on the root's font size and
        // line height.
        if is_root
            && (old.font_size != new.font_size || old.line_height_px() != new.line_height_px())
        {
            return Err(FullReason::RootFontSize);
        }
        if old.is_query_container() || new.is_query_container() {
            return Err(FullReason::Containers);
        }
        changes.counter_inputs |= counter_inputs_differ(Some(old), Some(&new));
        changes.elements.push((node, Arc::clone(&new)));
        Ok((new, true))
    }

    /// Adds the nodes whose styles go after the DOM changes `dom` (ADR 0042
    /// section 3, "Removed styles"): in the current subtrees of the inserted
    /// and removed roots, each node with a style that the walk did not
    /// restyle. Such a node left the document or is below an element
    /// without a box. A restyled root's subtree was walked; a node that an
    /// earlier root's walk visited is skipped with its subtree. A cleared
    /// node that read a query container makes the pass fail.
    fn collect_stale(
        &self,
        map: &StyleMap,
        dom: &DomChanges,
        mut changes: Changes,
    ) -> Result<Changes, FullReason> {
        let doc = self.doc;
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut stack = Vec::new();
        for &root in dom.roots() {
            if doc.get(root).is_none() || changes.styled.contains(root) {
                continue;
            }
            stack.push(root);
            while let Some(n) = stack.pop() {
                if !visited.insert(n) {
                    continue;
                }
                stack.extend(doc.children(n));
                if let Some(shadow) = doc.shadow_root(n) {
                    stack.push(shadow);
                }
                if changes.styled.contains(n) {
                    continue;
                }
                let Some(old) = map.get(n) else {
                    continue;
                };
                if map.is_container_reader(n) {
                    return Err(FullReason::Containers);
                }
                changes.counter_inputs |= is_counter_input(old)
                    || [PseudoKind::Before, PseudoKind::After, PseudoKind::Marker]
                        .into_iter()
                        .filter_map(|kind| map.pseudo(n, kind))
                        .any(|s| is_counter_input(s));
                changes.touched.push(n);
                changes.stale.push(n);
            }
        }
        Ok(changes)
    }

    /// Notes the descendants of `node`, which became `display: none`, as
    /// losing their styles. A descendant that read a query container makes
    /// the pass fail.
    fn clear_subtree(
        &self,
        map: &StyleMap,
        node: NodeId,
        changes: &mut Changes,
    ) -> Result<(), FullReason> {
        changes.cleared.push(node);
        let mut stack: Vec<NodeId> = self.doc.flat_element_children(node).collect();
        while let Some(n) = stack.pop() {
            if map.get(n).is_none() {
                continue;
            }
            if map.is_container_reader(n) {
                return Err(FullReason::Containers);
            }
            changes.touched.push(n);
            stack.extend(self.doc.flat_element_children(n));
        }
        Ok(())
    }

    /// Applies the changes of the walk to `map`, runs the counter pass and
    /// the instances of `use` elements again when they need it, and compares
    /// (ADR 0041 sections 2 and 3).
    fn finish_targeted(
        &mut self,
        changes: &Changes,
        plan: &Plan,
        map: &mut StyleMap,
        sizes: &ContainerSizes,
    ) -> Restyle {
        let (counter_text, counter_limit) = map.counter_flags();
        let counter_inputs = changes.counter_inputs || plan.counter_inputs;
        let resolve = counter_inputs || changes.counter_content;
        if resolve && (counter_limit || (counter_inputs && counter_text)) {
            return Restyle::Full(FullReason::Counters);
        }
        let mut changed = !changes.elements.is_empty()
            || !changes.cleared.is_empty()
            || !changes.stale.is_empty();
        map.fit(self.doc.len());
        for &node in &changes.stale {
            map.clear_element(node);
        }
        let mut image_urls = image_urls_of(changes.elements.iter().map(|(_, s)| s));
        for (node, style) in &changes.elements {
            map.set(*node, Arc::clone(style));
        }
        for &node in &changes.cleared {
            let mut stack: Vec<NodeId> = self.doc.flat_element_children(node).collect();
            while let Some(n) = stack.pop() {
                if map.get(n).is_none() {
                    continue;
                }
                map.clear_element(n);
                stack.extend(self.doc.flat_element_children(n));
            }
        }
        // The pseudo-elements: the old ones, to compare after the counter
        // pass (which writes counter text into the new ones).
        let old_pseudos: Vec<(NodeId, PseudoStyles)> = changes
            .pseudo_nodes
            .iter()
            .map(|&node| (node, map.pseudos_of(node)))
            .collect();
        for &node in &changes.pseudo_nodes {
            map.replace_pseudos(node, changes.pseudos.pseudos_of(node));
        }
        let before = resolve.then(|| map.ordinals().clone());
        // The ordinals of the default summaries that the walk set, before
        // the counter pass (which overwrites those of list items).
        for (&node, &value) in changes.pseudos.ordinals() {
            map.set_list_item_ordinal(node, value);
        }
        if let Some(before) = before {
            map.reset_ordinals();
            crate::counters::resolve(self.doc, map);
            changed |= *map.ordinals() != before;
        }
        for (node, old) in old_pseudos {
            let mut new = map.pseudos_of(node);
            let mut kept = false;
            for (old, new) in old.iter().zip(new.iter_mut()) {
                match (old, &*new) {
                    (Some(a), Some(b)) if same(a, b) => {
                        // Keep the old object.
                        *new = Some(Arc::clone(a));
                        kept = true;
                    }
                    (None, None) => {}
                    _ => {
                        changed = true;
                        if let Some(b) = &*new {
                            image_urls.extend(image_urls_of(std::iter::once(b)));
                        }
                    }
                }
            }
            if kept {
                map.replace_pseudos(node, new);
            }
        }
        if plan.uses || self.uses_outdated(changes, map) {
            let (instances, root_uses) = map.take_instances();
            self.expand_uses(map);
            changed |= !map.same_instances(&instances, &root_uses);
        }
        refresh_container_sizes(map, sizes);
        if changed {
            map.recompute_hidden_contents();
        }
        image_urls.dedup();
        Restyle::Done {
            changed,
            restyled: changes.restyled,
            image_urls,
        }
    }

    /// True if the instance trees of `use` elements may differ from those
    /// that a full restyle makes: a `use` element was restyled or lost its
    /// style (also one that had no style, and so no instance, before), an
    /// element that an instance copies was restyled, or a descendant reach
    /// went into a `display: none` subtree, whose elements the walk does not
    /// restyle but an instance may copy.
    fn uses_outdated(&self, changes: &Changes, map: &StyleMap) -> bool {
        if changes
            .touched
            .iter()
            .any(|&n| self.doc.element(n).is_some_and(is_svg_use))
        {
            return true;
        }
        if !map.has_instances() {
            return false;
        }
        if changes.hidden_reach {
            return true;
        }
        let nodes = map.instance_nodes();
        changes.touched.iter().any(|n| nodes.contains(n))
    }
}

/// Sets the size of each container of the log to its current size: what a
/// full restyle records now.
fn refresh_container_sizes(map: &mut StyleMap, sizes: &ContainerSizes) {
    if map.container_log().is_empty() {
        return;
    }
    let current: HashMap<NodeId, Option<(f32, f32)>> = map
        .container_log()
        .containers()
        .map(|node| {
            let size = map
                .get(node)
                .filter(|s| s.size_containment_applies())
                .and_then(|_| sizes.get(node));
            (node, size)
        })
        .collect();
    map.container_log_mut()
        .refresh_sizes(|node| current.get(&node).copied().flatten());
}
