//! The id index: `id` values of connected elements (ADR 0028 section 3).
//!
//! The index maps each `id` value to the elements that have it and are
//! connected (in the tree of a document node). The change functions keep
//! it up to date: an insertion into a connected tree adds the ids of the
//! inserted subtree, a removal from it removes them, and a change of an
//! `id` attribute of a connected element moves the element. A lookup
//! picks, among the elements of one id in the asked document, the first in
//! tree order. With a few elements of one id, only those elements are
//! compared; with more, one walk in tree order finds the first, because a
//! comparison walks siblings and many comparisons would be quadratic.
//! <https://dom.spec.whatwg.org/#dom-nonelementparentnode-getelementbyid>
//!
//! The index starts at the first lookup, with one walk of the connected
//! trees; until then the change functions skip it. A parse without
//! scripts therefore pays nothing for it, and the engine's few lookups
//! (the URL fragment, `label for`) cost one walk, as before the index.

use std::collections::{HashMap, HashSet};

use crate::tree::{Document, ElementData, NodeData, NodeId};

/// The most elements of one id that a lookup compares pairwise; above it,
/// the lookup walks the tree.
const MAX_PAIRWISE: usize = 8;

/// See the module documentation.
#[derive(Clone, Debug, Default)]
pub(crate) struct IdIndex {
    /// False until the first lookup.
    pub(crate) active: bool,
    map: HashMap<String, Vec<NodeId>>,
    /// Results of walks for ids with many elements, valid for the DOM
    /// generation `walked_at`: a loop of lookups does not walk again.
    walked: HashMap<(NodeId, String), Option<NodeId>>,
    walked_at: u64,
}

impl IdIndex {
    pub(crate) fn add(&mut self, id: &str, element: NodeId) {
        if let Some(elements) = self.map.get_mut(id) {
            elements.push(element);
        } else {
            self.map.insert(id.to_owned(), vec![element]);
        }
    }

    pub(crate) fn remove(&mut self, id: &str, element: NodeId) {
        if let Some(elements) = self.map.get_mut(id) {
            if let Some(i) = elements.iter().position(|&e| e == element) {
                elements.swap_remove(i);
            }
            if elements.is_empty() {
                self.map.remove(id);
            }
        }
    }

    /// Removes many pairs at once: one pass over the elements of each id,
    /// so that the removal of a subtree with many elements of one id stays
    /// linear.
    fn remove_all(&mut self, pairs: &[(String, NodeId)]) {
        if let [(id, element)] = pairs {
            self.remove(id, *element);
            return;
        }
        let mut by_id: HashMap<&str, HashSet<NodeId>> = HashMap::new();
        for (id, element) in pairs {
            by_id.entry(id.as_str()).or_default().insert(*element);
        }
        for (id, removed) in by_id {
            if let Some(elements) = self.map.get_mut(id) {
                elements.retain(|e| !removed.contains(e));
                if elements.is_empty() {
                    self.map.remove(id);
                }
            }
        }
    }

    fn get(&self, id: &str) -> &[NodeId] {
        self.map.get(id).map_or(&[], Vec::as_slice)
    }
}

impl Document {
    /// The first element in tree order whose `id` is `id` in the tree of
    /// `root`, a document node of the arena. An empty `id` finds nothing.
    /// Elements in detached subtrees and in template contents are not in
    /// the index.
    /// <https://dom.spec.whatwg.org/#dom-nonelementparentnode-getelementbyid>
    pub fn get_element_by_id(&self, root: NodeId, id: &str) -> Option<NodeId> {
        if !self.ids.borrow().active {
            let index = self.build_id_index();
            *self.ids.borrow_mut() = index;
        }
        let ids = self.ids.borrow();
        let elements = ids.get(id);
        if elements.len() > MAX_PAIRWISE {
            drop(ids);
            return self.walk_for_id(root, id);
        }
        let mut first: Option<NodeId> = None;
        for &element in elements {
            if self.root(element) != root {
                continue;
            }
            if first.is_none_or(|f| self.precedes(element, f)) {
                first = Some(element);
            }
        }
        first
    }

    /// The number of elements whose `id` is `id` in the tree of `root`,
    /// counted up to `limit` (a lookup that only asks whether there are
    /// several stops early).
    pub fn count_elements_by_id(&self, root: NodeId, id: &str, limit: usize) -> usize {
        if id.is_empty() {
            return 0;
        }
        if !self.ids.borrow().active {
            let index = self.build_id_index();
            *self.ids.borrow_mut() = index;
        }
        let ids = self.ids.borrow();
        let mut count = 0;
        for &element in ids.get(id) {
            if self.root(element) == root {
                count += 1;
                if count >= limit {
                    break;
                }
            }
        }
        count
    }

    /// The first element of `id` in tree order, by a walk of the tree of
    /// `root`; the result is kept until the next change.
    fn walk_for_id(&self, root: NodeId, id: &str) -> Option<NodeId> {
        {
            let mut ids = self.ids.borrow_mut();
            if ids.walked_at != self.generation {
                ids.walked.clear();
                ids.walked_at = self.generation;
            } else if let Some(found) = ids.walked.get(&(root, id.to_owned())) {
                return *found;
            }
        }
        let found = self
            .descendants(root)
            .find(|&n| self.element(n).and_then(ElementData::id) == Some(id));
        self.ids
            .borrow_mut()
            .walked
            .insert((root, id.to_owned()), found);
        found
    }

    /// The index of all connected elements: a walk of the tree of each
    /// document node.
    fn build_id_index(&self) -> IdIndex {
        let mut index = IdIndex {
            active: true,
            ..IdIndex::default()
        };
        let documents = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n.data, NodeData::Document))
            .filter_map(|(i, _)| NodeId::from_index(i));
        for document in documents {
            for n in self.descendants(document) {
                if let Some(id) = self.element(n).and_then(ElementData::id) {
                    index.add(id, n);
                }
            }
        }
        index
    }

    /// True if the index is active and `node` or a descendant may have an
    /// id: a quick test that lets the change functions skip the index for
    /// a leaf without one.
    #[inline]
    pub(crate) fn may_have_ids(&mut self, node: NodeId) -> bool {
        if !self.ids.get_mut().active {
            return false;
        }
        let n = self.node(node);
        n.first_child.is_some() || n.as_element().is_some_and(|e| e.id().is_some())
    }

    /// Adds the ids of `node` and its descendants to the index. `node`
    /// must be connected.
    pub(crate) fn index_ids(&mut self, node: NodeId) {
        let mut found = Vec::new();
        for n in self.descendants(node) {
            if let Some(id) = self.element(n).and_then(ElementData::id) {
                found.push((id.to_owned(), n));
            }
        }
        let ids = self.ids.get_mut();
        for (id, n) in found {
            ids.add(&id, n);
        }
    }

    /// Removes the ids of `node` and its descendants from the index.
    pub(crate) fn unindex_ids(&mut self, node: NodeId) {
        let found: Vec<(String, NodeId)> = self
            .descendants(node)
            .filter_map(|n| {
                self.element(n)
                    .and_then(ElementData::id)
                    .map(|id| (id.to_owned(), n))
            })
            .collect();
        if !found.is_empty() {
            self.ids.get_mut().remove_all(&found);
        }
    }

    /// Moves a connected element in the id index from `old` to `new`.
    pub(crate) fn reindex_id(&mut self, element: NodeId, old: Option<&str>, new: Option<&str>) {
        if !self.ids.get_mut().active || !self.is_connected(element) {
            return;
        }
        let ids = self.ids.get_mut();
        if let Some(old) = old.filter(|v| !v.is_empty()) {
            ids.remove(old, element);
        }
        if let Some(new) = new.filter(|v| !v.is_empty()) {
            ids.add(new, element);
        }
    }
}
