//! The result of style computation for a document: one computed style per
//! element, plus styles for generated pseudo-elements.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_dom::NodeId;

use crate::ComputedStyle;
use crate::container::ContainerLog;
use crate::content::content_text;
use crate::counter_style::marker_text;
use crate::custom::{CustomPool, CustomProperties};
use crate::values::{Content, Image, ListStyleType, MaskImage};

/// A pseudo-element that generates a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoKind {
    /// `::before`
    Before,
    /// `::after`
    After,
    /// `::marker`
    Marker,
    /// `::placeholder` of a text field (`input` or `textarea` with a
    /// `placeholder` attribute). Layout uses its style for the placeholder
    /// text.
    Placeholder,
    /// The default summary of a `details` element without a `summary`
    /// child (a list item with the text "Details" and a disclosure
    /// marker). The marker has the `Marker` style of the `details`
    /// element and its list item ordinal is 0.
    DetailsSummary,
    /// `::details-content`: the box around the children of a `details`
    /// element other than its summary. Layout builds it for every
    /// `details` that has such children. Page style sheets cannot style
    /// it.
    DetailsContent,
    /// `::backdrop` of an element in the top layer (CSS Position 4 §3.2):
    /// styled only for elements with the top-layer flag, inheriting from
    /// the element; layout builds its box as a child of the initial
    /// containing block, right before the element's box (ADR 0037
    /// section 8).
    Backdrop,
}

impl PseudoKind {
    /// All kinds.
    pub const ALL: [PseudoKind; 7] = [
        PseudoKind::Before,
        PseudoKind::After,
        PseudoKind::Marker,
        PseudoKind::Placeholder,
        PseudoKind::DetailsSummary,
        PseudoKind::DetailsContent,
        PseudoKind::Backdrop,
    ];
}

/// Computed styles for the elements of a document.
#[derive(Clone, Debug, Default)]
pub struct StyleMap {
    /// Indexed by `NodeId::index()`. `None` for non-elements and for
    /// elements in `display: none` subtrees (which have no style).
    elements: Vec<Option<Arc<ComputedStyle>>>,
    pseudos: HashMap<(NodeId, PseudoKind), Arc<ComputedStyle>>,
    /// The ordinal value of each list item (see `counters.rs`).
    ordinals: HashMap<NodeId, i32>,
    /// The instance trees of SVG `use` elements (`use_instances.rs`).
    instances: Vec<UseInstance>,
    /// The instances of the `use` elements of the document tree.
    root_uses: HashMap<NodeId, InstanceId>,
    /// What the styles read of query containers (ADR 0038); empty if they
    /// read none.
    container_log: ContainerLog,
    /// True if a stored style skips its contents (set when the cascade
    /// stores it, so that layout does not scan the styles).
    hidden_contents: bool,
    /// The elements whose style or pseudo-element styles read a query
    /// container (ADR 0041 section 2).
    container_readers: HashSet<NodeId>,
    /// True if `counters::resolve` wrote the text of a counter into a
    /// pseudo-element's content (ADR 0041 section 2). It can stay true
    /// after a targeted restyle removed that content.
    counter_text: bool,
    /// True if `counters::resolve` reached the limit of counter text.
    counter_limit: bool,
}

/// The identity of the instance tree of one SVG `use` element (SVG 2 §5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InstanceId(u32);

/// The instance tree of a `use` element: the computed styles of the
/// referenced element and its descendants as the `use` element's
/// inheritance gives them (the cascade matches the original elements, so
/// only the inherited values differ), and the instances of the `use`
/// elements inside.
#[derive(Clone, Debug)]
pub(crate) struct UseInstance {
    /// The referenced element.
    pub(crate) target: NodeId,
    pub(crate) styles: HashMap<NodeId, Arc<ComputedStyle>>,
    pub(crate) uses: HashMap<NodeId, InstanceId>,
}

impl StyleMap {
    /// The style of `node` in the instance tree `scope`, or in the
    /// document tree for `None`.
    pub fn style_in(&self, scope: Option<InstanceId>, node: NodeId) -> Option<&Arc<ComputedStyle>> {
        match scope {
            None => self.get(node),
            Some(InstanceId(i)) => self.instances.get(i as usize)?.styles.get(&node),
        }
    }

    /// The instance tree of the SVG `use` element `use_node` in the tree
    /// `scope` (the document tree for `None`); `None` if it has none (an
    /// invalid reference, a cycle, or a limit).
    pub fn use_instance(&self, scope: Option<InstanceId>, use_node: NodeId) -> Option<InstanceId> {
        match scope {
            None => self.root_uses.get(&use_node).copied(),
            Some(InstanceId(i)) => self.instances.get(i as usize)?.uses.get(&use_node).copied(),
        }
    }

    /// The number of styled elements in all instance trees.
    #[cfg(test)]
    pub(crate) fn instance_element_count(&self) -> usize {
        self.instances.iter().map(|i| i.styles.len()).sum()
    }

    /// The element that instance tree `id` copies.
    pub fn instance_target(&self, id: InstanceId) -> Option<NodeId> {
        self.instances.get(id.0 as usize).map(|i| i.target)
    }

    /// Adds an instance tree and returns its identity.
    pub(crate) fn add_instance(&mut self, instance: UseInstance) -> InstanceId {
        self.instances.push(instance);
        InstanceId(u32::try_from(self.instances.len() - 1).unwrap_or(u32::MAX))
    }

    /// The instance tree being built.
    pub(crate) fn instance_mut(&mut self, id: InstanceId) -> Option<&mut UseInstance> {
        self.instances.get_mut(id.0 as usize)
    }

    /// Records the instance of `use_node` in `scope`.
    pub(crate) fn set_use_instance(
        &mut self,
        scope: Option<InstanceId>,
        use_node: NodeId,
        id: InstanceId,
    ) {
        match scope {
            None => {
                self.root_uses.insert(use_node, id);
            }
            Some(s) => {
                if let Some(instance) = self.instance_mut(s) {
                    instance.uses.insert(use_node, id);
                }
            }
        }
    }

    /// Creates an empty map with room for `node_count` nodes.
    pub(crate) fn with_capacity(node_count: usize) -> Self {
        StyleMap {
            elements: vec![None; node_count],
            pseudos: HashMap::new(),
            ordinals: HashMap::new(),
            instances: Vec::new(),
            root_uses: HashMap::new(),
            container_log: ContainerLog::default(),
            hidden_contents: false,
            container_readers: HashSet::new(),
            counter_text: false,
            counter_limit: false,
        }
    }

    /// Notes that the style of `node` or of its pseudo-elements read a
    /// query container.
    pub(crate) fn add_container_reader(&mut self, node: NodeId) {
        self.container_readers.insert(node);
    }

    /// True if the style of `node` or of its pseudo-elements read a query
    /// container.
    pub(crate) fn is_container_reader(&self, node: NodeId) -> bool {
        !self.container_readers.is_empty() && self.container_readers.contains(&node)
    }

    /// Records what `counters::resolve` did: it wrote counter text, and it
    /// reached the limit of counter text (each flag stays set once set).
    pub(crate) fn note_counters(&mut self, text: bool, limit: bool) {
        self.counter_text |= text;
        self.counter_limit |= limit;
    }

    /// See [`StyleMap::note_counters`].
    pub(crate) fn counter_flags(&self) -> (bool, bool) {
        (self.counter_text, self.counter_limit)
    }

    /// The container log, for the targeted restyle.
    pub(crate) fn container_log_mut(&mut self) -> &mut ContainerLog {
        &mut self.container_log
    }

    /// Removes the style of `node` and of its pseudo-elements.
    pub(crate) fn clear_element(&mut self, node: NodeId) {
        if let Some(slot) = self.elements.get_mut(node.index()) {
            *slot = None;
        }
        self.remove_pseudos(node);
    }

    /// Removes the pseudo-element styles of `node`.
    pub(crate) fn remove_pseudos(&mut self, node: NodeId) {
        if self.pseudos.is_empty() {
            return;
        }
        for kind in PseudoKind::ALL {
            self.pseudos.remove(&(node, kind));
        }
    }

    /// The ordinals of the list items.
    pub(crate) fn ordinals(&self) -> &HashMap<NodeId, i32> {
        &self.ordinals
    }

    /// Keeps only the ordinals that the style walk sets (the default
    /// summaries of `details`, 0), before `counters::resolve` runs again.
    pub(crate) fn reset_ordinals(&mut self) {
        let pseudos = &self.pseudos;
        self.ordinals
            .retain(|&node, _| pseudos.contains_key(&(node, PseudoKind::DetailsSummary)));
        for value in self.ordinals.values_mut() {
            *value = 0;
        }
    }

    /// True if the map has instance trees of `use` elements.
    pub(crate) fn has_instances(&self) -> bool {
        !self.instances.is_empty() || !self.root_uses.is_empty()
    }

    /// The `use` elements of the document tree and the elements that some
    /// instance tree copies.
    pub(crate) fn instance_nodes(&self) -> HashSet<NodeId> {
        let mut nodes: HashSet<NodeId> = self.root_uses.keys().copied().collect();
        for instance in &self.instances {
            nodes.extend(instance.styles.keys().copied());
            nodes.extend(instance.uses.keys().copied());
        }
        nodes
    }

    /// Removes the instance trees (before `expand_uses` runs again) and
    /// returns them.
    pub(crate) fn take_instances(&mut self) -> (Vec<UseInstance>, HashMap<NodeId, InstanceId>) {
        (
            std::mem::take(&mut self.instances),
            std::mem::take(&mut self.root_uses),
        )
    }

    /// True if the map's instance trees equal `instances` and `root_uses`.
    pub(crate) fn same_instances(
        &self,
        instances: &[UseInstance],
        root_uses: &HashMap<NodeId, InstanceId>,
    ) -> bool {
        let same = |a: &Arc<ComputedStyle>, b: &Arc<ComputedStyle>| Arc::ptr_eq(a, b) || a == b;
        self.root_uses == *root_uses
            && self.instances.len() == instances.len()
            && self.instances.iter().zip(instances).all(|(a, b)| {
                a.uses == b.uses
                    && a.styles.len() == b.styles.len()
                    && a.styles
                        .iter()
                        .all(|(k, x)| b.styles.get(k).is_some_and(|y| same(x, y)))
            })
    }

    /// Computes [`StyleMap::has_hidden_contents`] again from all styles.
    pub(crate) fn recompute_hidden_contents(&mut self) {
        self.hidden_contents = self
            .elements
            .iter()
            .flatten()
            .chain(self.pseudos.values())
            .any(|s| s.skips_contents());
    }

    /// The pseudo-element styles of `node`, by kind.
    pub(crate) fn pseudos_of(&self, node: NodeId) -> [Option<Arc<ComputedStyle>>; 7] {
        PseudoKind::ALL.map(|kind| self.pseudos.get(&(node, kind)).cloned())
    }

    /// Stores (or with `None` removes) a pseudo-element style.
    pub(crate) fn put_pseudo(
        &mut self,
        node: NodeId,
        kind: PseudoKind,
        style: Option<Arc<ComputedStyle>>,
    ) {
        match style {
            Some(style) => self.set_pseudo(node, kind, style),
            None => {
                self.pseudos.remove(&(node, kind));
            }
        }
    }

    /// What differs between two maps, for the messages of tests: the first
    /// element or pseudo-element style, ordinal, or the other parts that
    /// [`StyleMap::same_result`] compares.
    #[doc(hidden)]
    pub fn first_difference(&self, other: &StyleMap) -> Option<String> {
        let same = |a: Option<&Arc<ComputedStyle>>, b: Option<&Arc<ComputedStyle>>| match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
            (None, None) => true,
            _ => false,
        };
        let count = self.elements.len().max(other.elements.len());
        for index in 0..count {
            let a = self.elements.get(index).and_then(Option::as_ref);
            let b = other.elements.get(index).and_then(Option::as_ref);
            if !same(a, b) {
                return Some(format!(
                    "the style of node {index}: {}",
                    style_difference(a, b)
                ));
            }
        }
        for (key, a) in &self.pseudos {
            if !same(Some(a), other.pseudos.get(key)) {
                return Some(format!(
                    "the pseudo-element {key:?}: {}",
                    style_difference(Some(a), other.pseudos.get(key))
                ));
            }
        }
        if let Some(key) = other.pseudos.keys().find(|k| !self.pseudos.contains_key(k)) {
            return Some(format!("the pseudo-element {key:?} is missing"));
        }
        if self.ordinals != other.ordinals {
            return Some("the ordinals".to_owned());
        }
        if !self.same_instances(&other.instances, &other.root_uses) {
            return Some("the use instances".to_owned());
        }
        if !self.container_log.same_as(&other.container_log) {
            return Some(format!(
                "the container log: {:?} against {:?}",
                self.container_log, other.container_log
            ));
        }
        if self.hidden_contents != other.hidden_contents {
            return Some("the hidden contents flag".to_owned());
        }
        None
    }

    /// True if both maps have the same container log, `use` instances and
    /// styles ([`StyleMap::same_styles`]): for the tests that compare a
    /// targeted restyle with a full one. The hidden contents flag must be
    /// equal too.
    pub fn same_result(&self, other: &StyleMap) -> bool {
        self.same_styles(other)
            && other.same_styles(self)
            && self.container_log.same_as(&other.container_log)
            && self.hidden_contents == other.hidden_contents
    }

    /// What the styles read of query containers: the engine checks it
    /// against the sizes of the next layout (ADR 0038).
    pub fn container_log(&self) -> &ContainerLog {
        &self.container_log
    }

    /// Sets the container log of the style pass.
    pub(crate) fn set_container_log(&mut self, log: ContainerLog) {
        self.container_log = log;
    }

    /// The style of an element, if it has one.
    pub fn get(&self, id: NodeId) -> Option<&Arc<ComputedStyle>> {
        self.elements.get(id.index()).and_then(Option::as_ref)
    }

    /// The style of a pseudo-element of `id`, if it generates a box.
    pub fn pseudo(&self, id: NodeId, kind: PseudoKind) -> Option<&Arc<ComputedStyle>> {
        self.pseudos.get(&(id, kind))
    }

    /// The style of a pseudo-element of `id`, for changes.
    pub(crate) fn pseudo_mut(
        &mut self,
        id: NodeId,
        kind: PseudoKind,
    ) -> Option<&mut Arc<ComputedStyle>> {
        self.pseudos.get_mut(&(id, kind))
    }

    /// True if a style skips its contents
    /// ([`ComputedStyle::skips_contents`]): a closed `details` with a
    /// `::details-content` box, or an element with `content-visibility:
    /// hidden`.
    pub fn has_hidden_contents(&self) -> bool {
        self.hidden_contents
    }

    /// The ordinal value of list item `id` (an element with
    /// `display: list-item`): the number that its marker shows with
    /// `content: normal`.
    /// <https://html.spec.whatwg.org/multipage/grouping-content.html#ordinal-value>
    pub fn list_item_ordinal(&self, id: NodeId) -> Option<i32> {
        self.ordinals.get(&id).copied()
    }

    /// The text of the marker of list item `id`, whose style is `style`:
    /// the `content` of its `::marker` (with counters resolved), or for
    /// `content: normal` its ordinal value in its `list-style-type`. `None`
    /// if `id` is not a list item (also a replaced element or form control
    /// with `display: list-item`) or if the marker has no box:
    /// `content: none`, or `content: normal` with `list-style-type: none`
    /// and no `list-style-image`.
    pub fn list_marker_text(&self, id: NodeId, style: &ComputedStyle) -> Option<String> {
        let ordinal = self.list_item_ordinal(id)?;
        let marker = self.pseudo(id, PseudoKind::Marker)?;
        match &marker.content {
            Content::None => None,
            Content::Items(_) => content_text(marker),
            Content::Normal => {
                if style.list_style_type == ListStyleType::None && style.list_style_image.is_none()
                {
                    return None;
                }
                Some(marker_text(style.list_style_type, ordinal))
            }
        }
    }

    /// Stores the ordinal value of a list item.
    pub(crate) fn set_list_item_ordinal(&mut self, id: NodeId, value: i32) {
        self.ordinals.insert(id, value);
    }

    /// Stores the style of an element.
    pub(crate) fn set(&mut self, id: NodeId, style: Arc<ComputedStyle>) {
        let index = id.index();
        if index >= self.elements.len() {
            self.elements.resize(index + 1, None);
        }
        self.hidden_contents |= style.skips_contents();
        self.elements[index] = Some(style);
    }

    /// True if both maps have equal styles for all elements and
    /// pseudo-elements, and equal list item ordinals.
    pub fn same_styles(&self, other: &StyleMap) -> bool {
        let same = |a: &Arc<ComputedStyle>, b: &Arc<ComputedStyle>| Arc::ptr_eq(a, b) || a == b;
        self.elements.len() == other.elements.len()
            && self
                .elements
                .iter()
                .zip(&other.elements)
                .all(|pair| match pair {
                    (Some(a), Some(b)) => same(a, b),
                    (None, None) => true,
                    _ => false,
                })
            && self.pseudos.len() == other.pseudos.len()
            && self
                .pseudos
                .iter()
                .all(|(key, a)| other.pseudos.get(key).is_some_and(|b| same(a, b)))
            && self.ordinals == other.ordinals
            && self.root_uses == other.root_uses
            && self.instances.len() == other.instances.len()
            && self.instances.iter().zip(&other.instances).all(|(a, b)| {
                a.uses == b.uses
                    && a.styles.len() == b.styles.len()
                    && a.styles
                        .iter()
                        .all(|(k, x)| b.styles.get(k).is_some_and(|y| same(x, y)))
            })
    }

    /// Adds the custom-property maps of all styles to `pool`, each map
    /// once (the elements that inherit a map share its `Arc`).
    pub(crate) fn seed_custom_pool(&self, pool: &CustomPool) {
        let mut last: Option<&Arc<CustomProperties>> = None;
        let mut seen: HashSet<*const CustomProperties> = HashSet::new();
        let styles = self.elements.iter().flatten().chain(self.pseudos.values());
        for style in styles {
            let Some(map) = &style.custom_properties else {
                continue;
            };
            if last.is_some_and(|l| Arc::ptr_eq(l, map)) || !seen.insert(Arc::as_ptr(map)) {
                continue;
            }
            last = Some(map);
            pool.seed(map);
        }
    }

    /// Stores the style of a pseudo-element.
    pub(crate) fn set_pseudo(&mut self, id: NodeId, kind: PseudoKind, style: Arc<ComputedStyle>) {
        self.hidden_contents |= style.skips_contents();
        self.pseudos.insert((id, kind), style);
    }

    /// The absolute URLs of all `background-image`, `mask-image` and
    /// `list-style-image` `url()` values, without duplicates: those of
    /// element styles in document order, then those of pseudo-element
    /// styles (in no particular order).
    pub fn image_urls(&self) -> Vec<Arc<str>> {
        image_urls_of(self.elements.iter().flatten().chain(self.pseudos.values()))
    }
}

/// A short description of how two styles differ, for test messages.
fn style_difference(a: Option<&Arc<ComputedStyle>>, b: Option<&Arc<ComputedStyle>>) -> String {
    match (a, b) {
        (Some(a), Some(b)) => {
            let (a, b) = (format!("{a:?}"), format!("{b:?}"));
            let start = a
                .bytes()
                .zip(b.bytes())
                .position(|(x, y)| x != y)
                .unwrap_or(0);
            let start = a.floor_char_boundary(start.saturating_sub(80));
            let end_a = a.ceil_char_boundary((start + 240).min(a.len()));
            let end_b = b.ceil_char_boundary((start + 240).min(b.len()));
            format!(
                "...{} against ...{}",
                a.get(start..end_a).unwrap_or(""),
                b.get(start..end_b).unwrap_or("")
            )
        }
        (a, b) => format!("{:?} against {:?}", a.is_some(), b.is_some()),
    }
}

/// The absolute URLs of the `background-image`, `mask-image` and
/// `list-style-image` `url()` values of `styles`, without duplicates, in
/// order.
pub(crate) fn image_urls_of<'a>(
    styles: impl Iterator<Item = &'a Arc<ComputedStyle>>,
) -> Vec<Arc<str>> {
    let mut seen: HashSet<Arc<str>> = HashSet::new();
    let mut urls = Vec::new();
    let mut visited: HashSet<*const ComputedStyle> = HashSet::new();
    for style in styles {
        if !visited.insert(Arc::as_ptr(style)) {
            continue;
        }
        let masks = style.mask_image.iter().filter_map(|m| match m {
            MaskImage::Image(image) => Some(image),
            _ => None,
        });
        let images = style
            .background_image
            .iter()
            .flatten()
            .chain(masks)
            .chain(style.list_style_image.as_ref());
        for image in images {
            if let Image::Url(url) = image
                && seen.insert(Arc::clone(url))
            {
                urls.push(Arc::clone(url));
            }
        }
    }
    urls
}
