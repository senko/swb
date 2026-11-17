//! The DOM changes since the styles were computed, and what they restyle
//! (ADR 0042 sections 2 and 3).
//!
//! The engine records each journal entry of a document with styles in a
//! [`DomChanges`] set (attributes with their first old `class` and `id`,
//! the parents whose children changed, the inserted and removed subtrees,
//! the parents of changed text, top layer changes) until the next style
//! computation. [`plan_dom`] turns the set into marks of the
//! [`Planner`]; the targeted restyle (`cascade/targeted.rs`) walks the
//! plan and clears the styles of the nodes that left the document or are
//! no longer rendered.

use std::collections::{HashMap, HashSet};

use swb_dom::{Change, Document, ElementData, LocalName, NodeId, local_name};

use crate::cascade::targeted::FullReason;
use crate::element::ElementStates;
use crate::invalidation::{
    At, CHAIN_STATES, DOM_STATES, ElementKeys, InvalidationIndex, Key, KeySet, Planner,
};

/// More connected entries than this since the styles: a full restyle
/// (ADR 0042 section 5; the click steps of the fixtures have at most 493).
const MAX_ENTRIES: usize = 10_000;

/// More inserted or removed subtree roots (also outside the document) than
/// this: a full restyle.
const MAX_ROOTS: usize = 100_000;

/// More nodes than this in the removed subtrees that the drains walk for
/// their keys: a full restyle (a script that removes a large subtree again
/// and again).
const MAX_WALKED: usize = 200_000;

/// The attributes of one element that changed.
#[derive(Clone, Debug, Default)]
struct ChangedAttributes {
    /// The local names, each once.
    names: Vec<LocalName>,
    /// The `class` and `id` values when the styles were computed (the
    /// first old value), if they changed.
    old_class: Option<OldValue>,
    old_id: Option<OldValue>,
}

/// The value of an attribute before the first change since the styles
/// (`None`: it had none).
#[derive(Clone, Debug)]
struct OldValue(Option<String>);

/// The DOM changes of a document since its styles were computed (ADR 0042
/// section 3).
#[derive(Clone, Debug)]
pub struct DomChanges {
    full: Option<FullReason>,
    attributes: HashMap<NodeId, ChangedAttributes>,
    /// The parents whose children changed (connected at the drain).
    parents: HashSet<NodeId>,
    /// The inserted subtree roots (connected at the drain).
    added: Vec<NodeId>,
    /// The parents of removed subtrees (connected at the drain).
    removal_parents: HashSet<NodeId>,
    /// The keys of the removed elements, also the `class` and `id` that
    /// they had when the styles were computed (before attribute changes in
    /// an earlier drain or outside the document).
    removed_keys: ElementKeys,
    /// Every inserted or removed node, also outside the document: the
    /// roots of the walk that clears the styles of nodes that left.
    roots: Vec<NodeId>,
    root_set: HashSet<NodeId>,
    /// The parents of changed text.
    text_parents: HashSet<NodeId>,
    /// The elements that entered or left the top layer.
    top_layer: HashSet<NodeId>,
    /// The connected entries.
    entries: usize,
    /// The nodes of removed subtrees walked for their keys.
    walked: usize,
    /// True while every change is an attribute change of a name that only
    /// the style system reads (ADR 0042 section 6).
    layout_free: bool,
}

impl Default for DomChanges {
    fn default() -> Self {
        DomChanges {
            full: None,
            attributes: HashMap::new(),
            parents: HashSet::new(),
            added: Vec::new(),
            removal_parents: HashSet::new(),
            removed_keys: ElementKeys::default(),
            roots: Vec::new(),
            root_set: HashSet::new(),
            text_parents: HashSet::new(),
            top_layer: HashSet::new(),
            entries: 0,
            walked: 0,
            layout_free: true,
        }
    }
}

/// True for an attribute that only the style system reads, so that its
/// change keeps the layout when no style changed (ADR 0042 section 6). `id`
/// is not one: layout resolves SVG references (`url(#id)`) through it.
fn layout_free_attribute(name: &swb_dom::QualName) -> bool {
    if name.ns != swb_dom::ns!() {
        return false;
    }
    let local = &*name.local;
    matches!(local, "class" | "style" | "title" | "role" | "tabindex")
        || local.starts_with("data-")
        || local.starts_with("aria-")
}

/// True if `node` is in the document of `doc` (shadow-including).
fn connected(doc: &Document, node: NodeId) -> bool {
    doc.get(node).is_some() && doc.shadow_including_root(node) == NodeId::DOCUMENT
}

impl DomChanges {
    /// True if nothing changed.
    pub fn is_empty(&self) -> bool {
        self.full.is_none()
            && self.attributes.is_empty()
            && self.parents.is_empty()
            && self.roots.is_empty()
            && self.text_parents.is_empty()
            && self.top_layer.is_empty()
    }

    /// The reason for a full restyle, if one is needed: [`FullReason::TooLarge`]
    /// or [`FullReason::Structure`].
    pub(crate) fn full_reason(&self) -> Option<FullReason> {
        self.full
    }

    /// True if every change is an attribute change that layout does not
    /// read (ADR 0042 section 6).
    pub fn layout_free(&self) -> bool {
        self.layout_free
    }

    /// Forgets all changes (after a style computation).
    pub fn clear(&mut self) {
        *self = DomChanges::default();
    }

    /// Notes that a full restyle is needed; the lists are dropped.
    fn set_full(&mut self, reason: FullReason) {
        *self = DomChanges {
            full: Some(reason),
            layout_free: false,
            ..DomChanges::default()
        };
    }

    /// Records one journal entry of `doc` (in the document's state after
    /// the drain's changes). `connected` says whether the entry's target
    /// (the parent, the element or the text node) is in the document.
    pub fn record(&mut self, doc: &Document, change: &Change<'_>, connected: bool) {
        if self.full.is_some() {
            return;
        }
        match *change {
            Change::Children {
                parent,
                added,
                removed,
                ..
            } => self.record_children(doc, parent, added, removed, connected),
            Change::Attribute {
                element,
                name,
                old_value,
            } => self.record_attribute(element, name, old_value, connected),
            Change::Text { node, .. } => {
                if !connected {
                    return;
                }
                self.entries += 1;
                self.layout_free = false;
                if let Some(parent) = doc.parent(node) {
                    self.text_parents.insert(parent);
                }
            }
            Change::TopLayer { element } => {
                if !connected {
                    return;
                }
                self.entries += 1;
                self.layout_free = false;
                self.top_layer.insert(element);
            }
        }
        if self.entries > MAX_ENTRIES {
            self.set_full(FullReason::TooLarge);
        }
    }

    /// Records a change of the children of `parent`.
    fn record_children(
        &mut self,
        doc: &Document,
        parent: NodeId,
        added: &[NodeId],
        removed: &[NodeId],
        connected: bool,
    ) {
        for &node in added.iter().chain(removed) {
            if self.root_set.insert(node) {
                self.roots.push(node);
            }
        }
        if self.roots.len() > MAX_ROOTS {
            self.set_full(FullReason::TooLarge);
            return;
        }
        if !connected {
            return;
        }
        self.entries += 1;
        self.layout_free = false;
        let structural = |n: NodeId| {
            doc.element(n).is_some_and(|e| {
                e.is_html_named(&local_name!("body")) || e.is_html_named(&local_name!("frameset"))
            })
        };
        if parent == NodeId::DOCUMENT
            || (doc.document_element() == Some(parent)
                && added.iter().chain(removed).any(|&n| structural(n)))
        {
            self.set_full(FullReason::Structure);
            return;
        }
        self.parents.insert(parent);
        self.added
            .extend(added.iter().filter(|&&n| doc.element(n).is_some()));
        if removed.is_empty() {
            return;
        }
        self.removal_parents.insert(parent);
        for &root in removed {
            for n in doc.descendants(root) {
                self.walked += 1;
                if let Some(data) = doc.element(n) {
                    self.removed_keys.add_element(data);
                }
                // The styles still have the `class` and `id` from before an
                // attribute change of an earlier drain.
                if let Some(changed) = self.attributes.get(&n) {
                    add_old_keys(&mut self.removed_keys, changed);
                }
            }
        }
        if self.walked > MAX_WALKED {
            self.set_full(FullReason::TooLarge);
        }
    }

    /// Records a change of the attribute `name` of `element`.
    fn record_attribute(
        &mut self,
        element: NodeId,
        name: &swb_dom::QualName,
        old_value: Option<&str>,
        connected: bool,
    ) {
        let is_class = name.ns == swb_dom::ns!() && name.local == local_name!("class");
        let is_id = name.ns == swb_dom::ns!() && name.local == local_name!("id");
        if !connected {
            // The keys that a removed element had before this change, for
            // the lookups of its removal.
            if let Some(old) = old_value {
                if is_class {
                    old.split_ascii_whitespace()
                        .for_each(|c| self.removed_keys.add_class(c));
                } else if is_id {
                    self.removed_keys.add_id(old);
                }
            }
            return;
        }
        self.entries += 1;
        if !layout_free_attribute(name) {
            self.layout_free = false;
        }
        let entry = self.attributes.entry(element).or_default();
        if !entry.names.contains(&name.local) {
            entry.names.push(name.local.clone());
        }
        if is_class && entry.old_class.is_none() {
            entry.old_class = Some(OldValue(old_value.map(str::to_owned)));
        }
        if is_id && entry.old_id.is_none() {
            entry.old_id = Some(OldValue(old_value.map(str::to_owned)));
        }
    }

    /// The inserted and removed roots, for the walk that clears the styles
    /// of nodes that left the document or are not rendered.
    pub(crate) fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    /// True if more elements were inserted than half of `styled` (a full
    /// restyle is cheaper; ADR 0042 section 5, item 6). Counts at most up
    /// to the limit.
    pub(crate) fn too_many_added(&self, doc: &Document, styled: usize) -> bool {
        let limit = styled / 2;
        let mut count = 0usize;
        let mut seen = HashSet::new();
        for &root in &self.added {
            if !seen.insert(root) || !connected(doc, root) {
                continue;
            }
            for n in doc.descendants(root) {
                if doc.element(n).is_some() {
                    count += 1;
                    if count > limit.max(64) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Adds the `class` tokens and the `id` that `changed` holds from the time
/// of the styles to `keys`.
fn add_old_keys(keys: &mut ElementKeys, changed: &ChangedAttributes) {
    if let Some(OldValue(Some(class))) = &changed.old_class {
        class
            .split_ascii_whitespace()
            .for_each(|c| keys.add_class(c));
    }
    if let Some(OldValue(Some(id))) = &changed.old_id {
        keys.add_id(id);
    }
}

/// The `class` tokens that are in only one of `old` and `new`: the class
/// keys that a change from `old` to `new` adds or removes.
fn changed_tokens<'a>(old: Option<&'a str>, new: Option<&'a str>) -> Vec<&'a str> {
    let a: HashSet<&str> = old.unwrap_or("").split_ascii_whitespace().collect();
    let b: HashSet<&str> = new.unwrap_or("").split_ascii_whitespace().collect();
    a.symmetric_difference(&b).copied().collect()
}

/// The rules for attributes that the cascade reads on an element other than
/// the one it styles (ADR 0042 section 2, item 4; the search is in the
/// implementation notes): what a change of `name` on the element `data`
/// restyles beyond the element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadByOthers {
    /// The links of the document (`body` `link`: the link colour hint).
    Links,
    /// The cells of the table (`table` `cellpadding` and `border`).
    Cells,
    /// The states of the whole subtree (`fieldset` and `optgroup`
    /// `disabled`, `contenteditable`).
    SubtreeStates,
    /// The ordinals of list items (`ol` `start` and `reversed`, `li`
    /// `value`).
    Counters,
}

/// The attributes of the ADR's table `ATTRIBUTES_READ_BY_OTHERS`.
fn read_by_others(data: &ElementData, name: &str) -> Option<ReadByOthers> {
    if name == "contenteditable" {
        return Some(ReadByOthers::SubtreeStates);
    }
    if !data.is_html() {
        return None;
    }
    match (&**data.local_name(), name) {
        ("body", "link") => Some(ReadByOthers::Links),
        ("table", "cellpadding" | "border") => Some(ReadByOthers::Cells),
        ("fieldset" | "optgroup", "disabled") => Some(ReadByOthers::SubtreeStates),
        ("ol", "start" | "reversed") | ("li", "value") => Some(ReadByOthers::Counters),
        _ => None,
    }
}

/// A key set of HTML element names.
fn names(list: &[&str], quirks: bool) -> KeySet {
    let mut set = KeySet::default();
    for name in list {
        set.add(&Key::html_name(name), quirks);
    }
    set
}

/// True if a change below `node` changes the states of its subtree: a
/// `fieldset` with `disabled` (its first `legend` decides), or an `optgroup`
/// with `disabled`.
fn disables_subtree(data: &ElementData) -> bool {
    (data.is_html_named(&local_name!("fieldset")) || data.is_html_named(&local_name!("optgroup")))
        && data.has_attr("disabled")
}

/// Marks what the DOM changes `changes` restyle (ADR 0042 section 2).
/// `old` are the states of the styles, `new` the current ones; `uses` says
/// whether the document has `use` elements.
pub(crate) fn plan_dom(
    planner: &mut Planner<'_>,
    changes: &DomChanges,
    old: &ElementStates,
    new: &ElementStates,
    uses: bool,
) {
    let mut marking = Marking {
        ids: std::mem::take(&mut planner.scratch),
        subtree_states: None,
    };
    marking.attributes(planner, changes, new, uses);
    marking.top_layer(planner, changes);
    marking.parents(planner, changes);
    if uses && !changes.parents.is_empty() {
        planner.plan.uses = true;
    }
    marking.inserted(planner, changes);
    marking.removed(planner, changes);
    if chains_moved(planner.doc, changes, old) {
        marking.moved_chains(planner, changes, old, new);
    }
    planner.scratch = marking.ids;
}

/// The state of [`plan_dom`]: a list of entries for the lookups, and the
/// entries of the states of a subtree (made at the first use).
struct Marking {
    ids: Vec<u32>,
    subtree_states: Option<Vec<u32>>,
}

impl Marking {
    /// The states of `node`'s whole subtree flip (section 2, item 4).
    fn subtree(&mut self, planner: &mut Planner<'_>, node: NodeId) {
        let index = planner.index;
        let ids = self.subtree_states.get_or_insert_with(|| {
            let mut all = Vec::new();
            index.all_states(DOM_STATES, &mut all);
            all
        });
        planner.apply(node, ids, At::Subtree);
    }

    /// The entries that `lookup` appends, applied at `node`.
    fn lookup(
        &mut self,
        planner: &mut Planner<'_>,
        node: NodeId,
        at: At,
        lookup: impl FnOnce(&InvalidationIndex, &mut Vec<u32>),
    ) {
        self.ids.clear();
        lookup(planner.index, &mut self.ids);
        planner.apply(node, &self.ids, at);
    }

    /// Attribute changes (section 2).
    fn attributes(
        &mut self,
        planner: &mut Planner<'_>,
        changes: &DomChanges,
        new: &ElementStates,
        uses: bool,
    ) {
        let (doc, quirks) = (planner.doc, planner.quirks);
        for (&element, attributes) in &changes.attributes {
            if planner.done() {
                break;
            }
            let Some(data) = doc.element(element).filter(|_| connected(doc, element)) else {
                continue;
            };
            // The cascade reads attributes of the element outside selectors.
            planner.mark_one(element);
            if let Some(OldValue(old)) = &attributes.old_class {
                for token in changed_tokens(old.as_deref(), data.attr("class")) {
                    self.lookup(planner, element, At::Element, |i, ids| {
                        i.class(token, quirks, ids);
                    });
                }
            }
            if let Some(OldValue(old)) = &attributes.old_id {
                for id in [old.as_deref(), data.attr("id")].into_iter().flatten() {
                    self.lookup(planner, element, At::Element, |i, ids| {
                        i.id(id, quirks, ids);
                    });
                }
                planner.plan.uses |= uses;
            }
            for name in &attributes.names {
                self.lookup(planner, element, At::Element, |i, ids| {
                    i.attribute(data, name, quirks, ids);
                });
                self.read_by_others(planner, element, data, name);
                if &**name == "href" {
                    planner.plan.uses |= uses;
                }
            }
            self.lookup(planner, element, At::Element, |i, ids| {
                i.states_of(data, DOM_STATES, quirks, ids);
            });
            // An image whose dimension attributes come from this `source`.
            for (&img, &source) in new.dimension_sources.iter() {
                if source == element {
                    planner.mark_one(img);
                }
            }
        }
    }

    /// What a change of `name` on `element` restyles on other elements.
    fn read_by_others(
        &mut self,
        planner: &mut Planner<'_>,
        element: NodeId,
        data: &ElementData,
        name: &str,
    ) {
        let quirks = planner.quirks;
        match read_by_others(data, name) {
            Some(ReadByOthers::Links) => {
                if let Some(root) = planner.doc.document_element() {
                    planner.root(root, &names(&["a", "area"], quirks));
                }
            }
            Some(ReadByOthers::Cells) => planner.root(element, &names(&["td", "th"], quirks)),
            Some(ReadByOthers::SubtreeStates) => self.subtree(planner, element),
            Some(ReadByOthers::Counters) => planner.plan.counter_inputs = true,
            None => {}
        }
    }

    /// Top layer changes: the element, its `:popover-open` and `:modal`, and
    /// the root (the blocking dialog makes the document inert).
    fn top_layer(&mut self, planner: &mut Planner<'_>, changes: &DomChanges) {
        if changes.top_layer.is_empty() {
            return;
        }
        let (doc, quirks) = (planner.doc, planner.quirks);
        if let Some(root) = doc.document_element() {
            planner.mark_one(root);
        }
        for &element in &changes.top_layer {
            let Some(data) = doc.element(element).filter(|_| connected(doc, element)) else {
                continue;
            };
            planner.mark_one(element);
            self.lookup(planner, element, At::Element, |i, ids| {
                i.states_of(
                    data,
                    swb_css::ElementState::POPOVER_OPEN | swb_css::ElementState::MODAL,
                    quirks,
                    ids,
                );
            });
        }
    }

    /// Parents whose children or text changed: the parent itself (its
    /// pseudo-elements can depend on its children: the default summary of a
    /// `details`), `:empty`, the value of a `textarea`, the first legend of
    /// a disabled fieldset, and the index of every child.
    fn parents(&mut self, planner: &mut Planner<'_>, changes: &DomChanges) {
        let (doc, quirks) = (planner.doc, planner.quirks);
        for &parent in changes.parents.iter().chain(&changes.text_parents) {
            if planner.done() {
                return;
            }
            let Some(data) = doc.element(parent).filter(|_| connected(doc, parent)) else {
                continue;
            };
            planner.mark_one(parent);
            self.lookup(planner, parent, At::Element, |i, ids| {
                i.empty(data, quirks, ids);
            });
            if data.is_html_named(&local_name!("textarea")) {
                self.lookup(planner, parent, At::Element, |i, ids| {
                    i.states_of(data, DOM_STATES, quirks, ids);
                });
            }
            if disables_subtree(data) {
                self.subtree(planner, parent);
            }
        }
        if !planner.index.has_structural() {
            return;
        }
        for &parent in &changes.parents {
            if !connected(doc, parent) {
                continue;
            }
            let mut child = doc.first_element_child(parent);
            while let Some(c) = child {
                if planner.done() {
                    return;
                }
                if let Some(data) = doc.element(c) {
                    self.lookup(planner, c, At::Element, |i, ids| {
                        i.structural(data, quirks, ids);
                    });
                }
                planner.count(1);
                child = doc.next_sibling_element(c);
            }
        }
    }

    /// Inserted subtrees: styled in full; their following siblings and the
    /// `:has()` anchors above them (section 2, items 1, 5 and 6).
    fn inserted(&mut self, planner: &mut Planner<'_>, changes: &DomChanges) {
        let (doc, quirks) = (planner.doc, planner.quirks);
        let mut seen = HashSet::new();
        for &root in &changes.added {
            if planner.done() {
                return;
            }
            if !seen.insert(root) || !connected(doc, root) {
                continue;
            }
            let Some(data) = doc.element(root) else {
                continue;
            };
            planner.root(root, &KeySet::any());
            planner.mark_one(root);
            planner.plan.inserted.insert(root);
            if !planner.index.has_presence() {
                continue;
            }
            self.lookup(planner, root, At::Element, |i, ids| {
                i.presence(data, quirks, ids);
            });
            if planner.index.has_presence_anchors() {
                let mut keys = ElementKeys::default();
                for n in doc.descendants(root).skip(1) {
                    if let Some(d) = doc.element(n) {
                        keys.add_element(d);
                    }
                }
                if !keys.is_empty() {
                    self.lookup(planner, root, At::Element, |i, ids| {
                        i.presence_of_keys(&keys, quirks, ids);
                    });
                }
            }
        }
    }

    /// Removed subtrees: the children of their parents (the removed position
    /// is gone) and the anchors from the parents up.
    fn removed(&mut self, planner: &mut Planner<'_>, changes: &DomChanges) {
        if !planner.index.has_presence() || changes.removal_parents.is_empty() {
            return;
        }
        let doc = planner.doc;
        self.ids.clear();
        planner
            .index
            .presence_of_keys(&changes.removed_keys, planner.quirks, &mut self.ids);
        if self.ids.is_empty() {
            return;
        }
        for &parent in &changes.removal_parents {
            if planner.done() {
                return;
            }
            if connected(doc, parent) && doc.element(parent).is_some() {
                planner.apply(parent, &self.ids, At::RemovedChild);
            }
        }
    }

    /// Moved state chains (ADR 0042 implementation notes): a node of the old
    /// hover, active or focus chain was inserted or removed, so the old
    /// chain is not the current one. The elements that left it are the
    /// removal parents and their ancestors; the current chains of those
    /// elements changed too (an ancestor was inserted above them).
    fn moved_chains(
        &mut self,
        planner: &mut Planner<'_>,
        changes: &DomChanges,
        old: &ElementStates,
        new: &ElementStates,
    ) {
        let (doc, quirks) = (planner.doc, planner.quirks);
        let mut visited = HashSet::new();
        let current = [
            old.hover, old.active, old.focus, new.hover, new.active, new.focus,
        ];
        let starts: Vec<NodeId> = changes
            .removal_parents
            .iter()
            .copied()
            .chain(current.into_iter().flatten())
            .filter(|&n| doc.get(n).is_some())
            .collect();
        for start in starts {
            let ups: Vec<NodeId> = std::iter::once(start)
                .chain(doc.flat_ancestors(start))
                .chain(doc.ancestors(start))
                .collect();
            for n in ups {
                if planner.done() {
                    return;
                }
                if !visited.insert(n) {
                    continue;
                }
                let Some(data) = doc.element(n).filter(|_| connected(doc, n)) else {
                    continue;
                };
                self.lookup(planner, n, At::Element, |i, ids| {
                    i.states_of(data, CHAIN_STATES, quirks, ids);
                });
            }
        }
    }
}

/// True if an inserted or removed root is an inclusive ancestor (now) of
/// the hovered, active or focused element of the old states: by the last
/// change that moved an inclusive ancestor of it, one of the roots holds it
/// now.
fn chains_moved(doc: &Document, changes: &DomChanges, old: &ElementStates) -> bool {
    if changes.root_set.is_empty() {
        return false;
    }
    [old.hover, old.active, old.focus]
        .into_iter()
        .flatten()
        .filter(|&n| doc.get(n).is_some())
        .any(|n| {
            let mut current = Some(n);
            while let Some(c) = current {
                if changes.root_set.contains(&c) {
                    return true;
                }
                current = doc.shadow_including_parent(c);
            }
            false
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_free_names() {
        let name = |n: &str| swb_dom::QualName::new(None, swb_dom::ns!(), LocalName::from(n));
        for n in [
            "class",
            "style",
            "data-x",
            "aria-expanded",
            "role",
            "title",
            "tabindex",
        ] {
            assert!(layout_free_attribute(&name(n)), "{n}");
        }
        for n in ["id", "src", "type", "value", "width", "hidden", "href", "d"] {
            assert!(!layout_free_attribute(&name(n)), "{n}");
        }
    }

    #[test]
    fn changed_class_tokens() {
        let mut tokens = changed_tokens(Some("a b c"), Some("b c d"));
        tokens.sort_unstable();
        assert_eq!(tokens, ["a", "d"]);
        assert_eq!(
            changed_tokens(Some("a  b"), Some("b a")),
            Vec::<&str>::new()
        );
    }
}
