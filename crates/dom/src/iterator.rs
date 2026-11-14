//! The positions of `NodeIterator` objects and the pre-removing steps (DOM
//! section 6.1, <https://dom.spec.whatwg.org/#interface-nodeiterator>;
//! ADR 0028 section 17).
//!
//! A [`Document`] keeps one small record per live iterator: its root, its
//! reference node, its pointer flag, and the candidate of a traversal that
//! runs. The records hold node ids and no script objects, so they live
//! with the arena while a script runtime borrows it. Nothing is installed
//! until the first iterator of an arena: a removal in an arena without
//! iterators costs one comparison (`NodeIterators::is_empty`).
//!
//! The change functions call the pre-removing steps before a node leaves its
//! parent (`Document::unlink_with`). They update every iterator whose reference
//! (or candidate) lies in the removed subtree, as the specification says, with
//! the departures that Chromium shows (measured, `tools/probes/web-node-iterator.json`):
//!
//! - a removed node that contains the root changes nothing;
//! - a pointer before a removed node that has children, with no node after
//!   the removed node inside the root, stays where it is;
//! - `textContent` and `innerHTML` run the steps for all removed children
//!   first, while the tree is whole ([`Document::replace_all_batched`]);
//! - the parser's moves (adoption agency, foster parenting) run no steps;
//! - a traversal keeps a candidate that the steps update like the reference,
//!   and the accepted candidate becomes the reference, so a filter that removes
//!   nodes moves the candidate with the tree.
//!
//! A node of an iterator can lie in another arena: a node that script moved to
//! another document keeps its place as the reference or the candidate of the
//! iterators that point at it ([`Spot::Away`]; the host names the arena by a
//! tag). The steps of the arena that holds the entry never touch such a
//! pointer: the removed node and the root are then in different documents.
//!
//! An iterator is found from the removed subtree through an index of the
//! nodes that iterators point at, so the cost of a removal follows the size
//! of the subtree when many iterators exist, and the smaller of the subtree and
//! the pointers' depth when few exist. The steps count their work
//! ([`ArenaWork`]) and the host charges it to the time limit of the task.
//! The entries of iterators that the collector freed go on a list that the
//! arenas share ([`crate::DeadIds`]); a removal drops them first.

use std::collections::{BTreeSet, HashMap};

use crate::subtree_search::{self, SCAN_BELOW};
use crate::tree::{Document, Node, NodeId};
use crate::work::ArenaWork;

/// A node of an iterator: in the arena that holds the entry, or in another
/// arena (the host's tag of it and the node).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// A node of this arena.
    Here(NodeId),
    /// A node of the arena with this tag.
    Away(u32, NodeId),
}

/// The position of an iterator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IteratorPosition {
    /// The root.
    pub root: NodeId,
    /// The reference node.
    pub reference: Spot,
    /// The pointer before reference flag.
    pub pointer_before: bool,
    /// The node that a running traversal has reached, and its flag.
    pub candidate: Option<(Spot, bool)>,
}

#[derive(Clone, Copy, Debug)]
struct State {
    root: NodeId,
    reference: Spot,
    before: bool,
    /// The node that a running traversal has reached, and its flag.
    candidate: Option<(Spot, bool)>,
}

impl State {
    /// The nodes of this arena that pointers are at.
    fn pointers(&self) -> [Option<NodeId>; 2] {
        let here = |spot: Spot| match spot {
            Spot::Here(node) => Some(node),
            Spot::Away(..) => None,
        };
        [here(self.reference), self.candidate.and_then(|c| here(c.0))]
    }

    fn position(&self) -> IteratorPosition {
        IteratorPosition {
            root: self.root,
            reference: self.reference,
            pointer_before: self.before,
            candidate: self.candidate,
        }
    }
}

/// Which pointer of an iterator a removal moves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pointer {
    Reference,
    Candidate,
}

/// The iterators of an arena.
#[derive(Debug, Default)]
pub(crate) struct NodeIterators {
    states: HashMap<u32, State>,
    /// `(node, id)` for each node of this arena that a pointer of iterator `id` is at.
    by_pointer: BTreeSet<(NodeId, u32)>,
    /// `(root, id)`.
    by_root: BTreeSet<(NodeId, u32)>,
    /// True while the parser moves nodes: it runs no steps.
    quiet: bool,
    work: Option<ArenaWork>,
    /// How much of the list of dead iterators this arena has dropped: its
    /// epoch and length.
    dead_seen: (u32, usize),
}

/// What one removal computes once for all the iterators it moves.
#[derive(Default)]
struct Cache {
    /// Whether the removed node is below a root.
    below: HashMap<NodeId, bool>,
    /// The first node after the removed node inside a root.
    next: HashMap<NodeId, Option<NodeId>>,
    /// Where a pointer after the removed node goes.
    fallback: Option<NodeId>,
}

impl NodeIterators {
    pub(crate) fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    fn charge(&self, units: usize) {
        if let Some(work) = &self.work {
            work.charge(units);
        }
    }

    /// Records the state of iterator `id` and keeps the indexes.
    fn place(&mut self, id: u32, new: State) {
        if let Some(old) = self.states.get(&id).copied() {
            for node in old.pointers().into_iter().flatten() {
                if !new.pointers().contains(&Some(node)) {
                    self.by_pointer.remove(&(node, id));
                }
            }
        } else {
            self.by_root.insert((new.root, id));
        }
        for node in new.pointers().into_iter().flatten() {
            self.by_pointer.insert((node, id));
        }
        self.states.insert(id, new);
    }

    fn forget(&mut self, id: u32) -> Option<State> {
        let state = self.states.remove(&id)?;
        for node in state.pointers().into_iter().flatten() {
            self.by_pointer.remove(&(node, id));
        }
        self.by_root.remove(&(state.root, id));
        Some(state)
    }

    /// Drops the entries of the iterators on the list of dead ones that this
    /// arena has not read yet.
    fn drop_dead(&mut self) {
        let Some(work) = &self.work else {
            return;
        };
        for id in work.dead.fresh(&mut self.dead_seen) {
            self.forget(id);
        }
    }

    /// The pointers in the subtree of `node`.
    fn find(&self, doc: &Document, node: NodeId, steps: &mut usize) -> Vec<(u32, Pointer)> {
        let few = self.states.len() <= SCAN_BELOW;
        let pointers = self.states.iter().flat_map(|(&id, state)| {
            let here = |spot: Spot| match spot {
                Spot::Here(n) => Some(n),
                Spot::Away(..) => None,
            };
            [
                here(state.reference).map(|n| ((id, Pointer::Reference), n)),
                state
                    .candidate
                    .and_then(|c| here(c.0))
                    .map(|n| ((id, Pointer::Candidate), n)),
            ]
            .into_iter()
            .flatten()
        });
        subtree_search::pointers_in_subtree(
            doc,
            node,
            few,
            pointers,
            |descendant, hits, steps| {
                for &(_, id) in self
                    .by_pointer
                    .range((descendant, 0)..=(descendant, u32::MAX))
                {
                    *steps += 1;
                    let Some(state) = self.states.get(&id) else {
                        continue;
                    };
                    if state.reference == Spot::Here(descendant) {
                        hits.push((id, Pointer::Reference));
                    }
                    if state
                        .candidate
                        .is_some_and(|c| c.0 == Spot::Here(descendant))
                    {
                        hits.push((id, Pointer::Candidate));
                    }
                }
            },
            steps,
        )
    }

    /// The pre-removing steps for the removal of `node`, which is still in
    /// its place.
    fn before_removal(&mut self, doc: &Document, node: NodeId) {
        self.drop_dead();
        if self.states.is_empty() {
            return;
        }
        let mut steps = 1;
        let hits = self.find(doc, node, &mut steps);
        let mut cache = Cache::default();
        let owner = doc.node_document(node);
        for (id, which) in hits {
            let Some(state) = self.states.get(&id).copied() else {
                continue;
            };
            // Step 1: an iterator in another document stays, and so does one whose root
            // is not a proper ancestor of the removed node: the root itself, and (measured)
            // an ancestor of the root.
            if doc.node_document(state.root) != owner {
                continue;
            }
            let below = *cache
                .below
                .entry(state.root)
                .or_insert_with(|| is_below(doc, node, state.root, &mut steps));
            if !below {
                continue;
            }
            let before = match which {
                Pointer::Reference => state.before,
                Pointer::Candidate => match state.candidate {
                    Some((_, before)) => before,
                    None => continue,
                },
            };
            let Some(moved) = moved_pointer(doc, node, state.root, before, &mut cache, &mut steps)
            else {
                continue;
            };
            let mut new = state;
            match which {
                Pointer::Reference => (new.reference, new.before) = (Spot::Here(moved.0), moved.1),
                Pointer::Candidate => new.candidate = Some((Spot::Here(moved.0), moved.1)),
            }
            self.place(id, new);
        }
        self.charge(steps);
    }
}

/// True if `root` is a proper ancestor of `node`.
fn is_below(doc: &Document, node: NodeId, root: NodeId, steps: &mut usize) -> bool {
    let mut current = doc.get(node).and_then(Node::parent);
    while let Some(n) = current {
        *steps += 1;
        if n == root {
            return true;
        }
        current = doc.get(n).and_then(Node::parent);
    }
    false
}

/// The last node of the subtree of `node` in tree order.
fn last_inclusive_descendant(doc: &Document, node: NodeId, steps: &mut usize) -> NodeId {
    let mut current = node;
    while let Some(last) = doc.get(current).and_then(Node::last_child) {
        *steps += 1;
        current = last;
    }
    current
}

/// Steps 2 and 3 of the pre-removing steps for a pointer that was inside the
/// subtree of `node`: the new node and flag, or `None` if the pointer stays.
fn moved_pointer(
    doc: &Document,
    node: NodeId,
    root: NodeId,
    before: bool,
    cache: &mut Cache,
    steps: &mut usize,
) -> Option<(NodeId, bool)> {
    let removed = doc.get(node)?;
    if before {
        let next = *cache
            .next
            .entry(root)
            .or_insert_with(|| following_inside(doc, node, root, steps));
        if let Some(next) = next {
            return Some((next, true));
        }
        // No node after the removed node inside the root. The specification
        // sets the flag to false and goes on; Chromium leaves the pointer
        // alone when the removed node has children (measured on every tree
        // of up to five nodes).
        if removed.first_child().is_some() {
            return None;
        }
    }
    if cache.fallback.is_none() {
        cache.fallback = Some(match removed.prev_sibling() {
            Some(previous) => last_inclusive_descendant(doc, previous, steps),
            None => removed.parent()?,
        });
    }
    cache.fallback.map(|target| (target, false))
}

/// The first node after the subtree of `node` that is inside the subtree of
/// `root`.
fn following_inside(
    doc: &Document,
    node: NodeId,
    root: NodeId,
    steps: &mut usize,
) -> Option<NodeId> {
    let mut current = node;
    loop {
        *steps += 1;
        if current == root {
            return None;
        }
        let n = doc.get(current)?;
        if let Some(sibling) = n.next_sibling() {
            return Some(sibling);
        }
        current = n.parent()?;
    }
}

impl Document {
    /// Gives the arena the counters through which it reports the work of the
    /// pre-removing steps, and the list of dead iterators.
    pub fn set_iterator_work(&mut self, work: ArenaWork) {
        let same_list = self
            .iterators
            .work
            .as_ref()
            .is_some_and(|old| old.same_list(&work));
        if !same_list {
            self.iterators.dead_seen = work.dead.start();
        }
        self.iterators.work = Some(work);
    }

    /// Adds iterator `id` with the root `root`: the reference is the root, the
    /// pointer is before it.
    pub fn add_node_iterator(&mut self, id: u32, root: NodeId) {
        self.iterators.place(
            id,
            State {
                root,
                reference: Spot::Here(root),
                before: true,
                candidate: None,
            },
        );
    }

    /// Removes iterator `id`.
    pub fn remove_node_iterator(&mut self, id: u32) {
        self.iterators.forget(id);
    }

    /// The position of iterator `id`.
    pub fn node_iterator(&self, id: u32) -> Option<IteratorPosition> {
        self.iterators.states.get(&id).map(State::position)
    }

    /// The number of live iterators.
    #[cfg(test)]
    pub(crate) fn node_iterator_count(&self) -> usize {
        self.iterators.states.len()
    }

    /// The nodes in other arenas that iterator `id` points at (the reference
    /// and the candidate), as the tag and the node.
    pub fn away_pointers(&self, id: u32) -> [Option<(u32, NodeId)>; 2] {
        let away = |spot: Spot| match spot {
            Spot::Away(tag, node) => Some((tag, node)),
            Spot::Here(_) => None,
        };
        match self.iterators.states.get(&id) {
            Some(state) => [
                away(state.reference),
                state.candidate.and_then(|c| away(c.0)),
            ],
            None => [None, None],
        }
    }

    /// Starts a traversal of iterator `id`: the candidate is the reference.
    /// False if the arena has no such iterator.
    pub fn node_iterator_begin(&mut self, id: u32) -> bool {
        let Some(mut state) = self.iterators.states.get(&id).copied() else {
            return false;
        };
        state.candidate = Some((state.reference, state.before));
        self.iterators.place(id, state);
        true
    }

    /// Puts the candidate of the traversal of iterator `id` at `spot`.
    pub fn node_iterator_set_candidate(&mut self, id: u32, spot: Spot, before: bool) {
        let Some(mut state) = self.iterators.states.get(&id).copied() else {
            return;
        };
        state.candidate = Some((spot, before));
        self.iterators.place(id, state);
    }

    /// One step of "traverse" (DOM section 6.1) for a candidate in this arena:
    /// moves the candidate to the next (or previous) node of the iterator
    /// collection and returns it for the filter. `None` at the end of the
    /// collection, when no traversal runs, and when the candidate is in another
    /// arena (the host steps there with [`Document::iterator_step`]). A
    /// candidate outside the root's tree moves inside its own tree.
    #[cfg(test)]
    pub(crate) fn node_iterator_advance(&mut self, id: u32, forward: bool) -> Option<NodeId> {
        let state = self.iterators.states.get(&id).copied()?;
        let (Spot::Here(node), before) = state.candidate? else {
            return None;
        };
        let (picked, flag) = self.iterator_step(Some(state.root), node, before, forward)?;
        let mut new = state;
        new.candidate = Some((Spot::Here(picked), flag));
        self.iterators.place(id, new);
        self.iterators.charge(1);
        Some(picked)
    }

    /// The first node of "traverse" from `node` with the flag `before`: the node
    /// for the filter and the flag after the step. Inside the tree of `root`;
    /// without a root (a node in an arena that does not hold the iterator), in
    /// the tree of `node`.
    pub fn iterator_step(
        &self,
        root: Option<NodeId>,
        node: NodeId,
        before: bool,
        forward: bool,
    ) -> Option<(NodeId, bool)> {
        if forward {
            if before {
                Some((node, false))
            } else {
                Some((self.iterator_following(node, root)?, false))
            }
        } else if before {
            Some((self.iterator_preceding(node, root)?, true))
        } else {
            Some((node, true))
        }
    }

    /// The node after `node` in tree order, not above `root`.
    fn iterator_following(&self, node: NodeId, root: Option<NodeId>) -> Option<NodeId> {
        if let Some(child) = self.get(node)?.first_child() {
            return Some(child);
        }
        let mut steps = 0usize;
        let mut up = node;
        let found = loop {
            steps += 1;
            if Some(up) == root {
                break None;
            }
            let Some(n) = self.get(up) else {
                break None;
            };
            if let Some(sibling) = n.next_sibling() {
                break Some(sibling);
            }
            match n.parent() {
                Some(parent) => up = parent,
                None => break None,
            }
        };
        self.iterators.charge(steps);
        found
    }

    /// The node before `node` in the collection of the iterator with `root`.
    fn iterator_preceding(&self, node: NodeId, root: Option<NodeId>) -> Option<NodeId> {
        if Some(node) == root {
            return None;
        }
        let n = self.get(node)?;
        match n.prev_sibling() {
            Some(previous) => {
                let mut steps = 0;
                let last = last_inclusive_descendant(self, previous, &mut steps);
                self.iterators.charge(steps);
                Some(last)
            }
            None => n.parent(),
        }
    }

    /// The filter accepted the candidate: it becomes the reference.
    pub fn node_iterator_accept(&mut self, id: u32) {
        let Some(mut state) = self.iterators.states.get(&id).copied() else {
            return;
        };
        if let Some((spot, before)) = state.candidate.take() {
            state.reference = spot;
            state.before = before;
        }
        self.iterators.place(id, state);
    }

    /// The traversal ended without a result (the end of the collection, an
    /// exception): the reference stays.
    pub fn node_iterator_end(&mut self, id: u32) {
        let Some(mut state) = self.iterators.states.get(&id).copied() else {
            return;
        };
        if state.candidate.take().is_some() {
            self.iterators.place(id, state);
        }
    }

    /// Charges `units` of work to the host's counters.
    pub fn charge_iterator_work(&self, units: usize) {
        self.iterators.charge(units);
    }

    /// Takes the iterators whose root is one of the old nodes of `pairs` (old
    /// id, id in another arena) out of this arena, for
    /// [`Document::put_node_iterators`] of that arena. A pointer in the moved
    /// subtree gets the new id; a pointer of this arena outside it stays where
    /// it is, as a node of this arena (`tag`). A running traversal goes on.
    pub fn take_node_iterators(
        &mut self,
        pairs: &[(NodeId, NodeId)],
        tag: u32,
    ) -> Vec<(u32, IteratorPosition)> {
        if self.iterators.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<(u32, NodeId)> = Vec::new();
        for &(old, new) in pairs {
            for &(_, id) in self.iterators.by_root.range((old, 0)..=(old, u32::MAX)) {
                found.push((id, new));
            }
        }
        if found.is_empty() {
            return Vec::new();
        }
        let map: HashMap<NodeId, NodeId> = pairs.iter().copied().collect();
        let carry = |spot: Spot| match spot {
            Spot::Here(node) => match map.get(&node) {
                Some(&new) => Spot::Here(new),
                None => Spot::Away(tag, node),
            },
            away @ Spot::Away(..) => away,
        };
        let mut moved = Vec::with_capacity(found.len());
        for (id, root) in found {
            let Some(state) = self.iterators.forget(id) else {
                continue;
            };
            moved.push((
                id,
                IteratorPosition {
                    root,
                    reference: carry(state.reference),
                    pointer_before: state.before,
                    candidate: state.candidate.map(|(spot, before)| (carry(spot), before)),
                },
            ));
        }
        moved
    }

    /// Puts iterators that [`Document::take_node_iterators`] took from another
    /// arena into this one, which has the tag `tag`.
    pub fn put_node_iterators(&mut self, moved: Vec<(u32, IteratorPosition)>, tag: u32) {
        let here = |spot: Spot| match spot {
            Spot::Away(t, node) if t == tag => Spot::Here(node),
            other => other,
        };
        for (id, position) in moved {
            self.iterators.place(
                id,
                State {
                    root: position.root,
                    reference: here(position.reference),
                    before: position.pointer_before,
                    candidate: position
                        .candidate
                        .map(|(spot, before)| (here(spot), before)),
                },
            );
        }
    }

    /// The pointers of this arena's iterators at the old nodes of `pairs` (old
    /// id, id in the arena with the tag `target`) go there, as nodes of that
    /// arena. The ids of the iterators that changed.
    pub fn send_node_pointers(&mut self, pairs: &[(NodeId, NodeId)], target: u32) -> Vec<u32> {
        if self.iterators.is_empty() {
            return Vec::new();
        }
        let mut changed = Vec::new();
        for &(old, new) in pairs {
            let ids: Vec<u32> = self
                .iterators
                .by_pointer
                .range((old, 0)..=(old, u32::MAX))
                .map(|&(_, id)| id)
                .collect();
            for id in ids {
                let Some(mut state) = self.iterators.states.get(&id).copied() else {
                    continue;
                };
                let away = Spot::Away(target, new);
                if state.reference == Spot::Here(old) {
                    state.reference = away;
                }
                if let Some((spot, before)) = state.candidate
                    && spot == Spot::Here(old)
                {
                    state.candidate = Some((away, before));
                }
                self.iterators.place(id, state);
                changed.push(id);
            }
        }
        changed
    }

    /// The pointers of iterator `id` at the node `from` of the arena with the
    /// tag `tag` go to `to`.
    pub fn receive_node_pointer(&mut self, id: u32, tag: u32, from: NodeId, to: Spot) {
        let Some(mut state) = self.iterators.states.get(&id).copied() else {
            return;
        };
        let old = Spot::Away(tag, from);
        if state.reference == old {
            state.reference = to;
        }
        if let Some((spot, before)) = state.candidate
            && spot == old
        {
            state.candidate = Some((to, before));
        }
        self.iterators.place(id, state);
    }

    /// The pre-removing steps for `node`, which still has its parent.
    #[inline]
    pub(crate) fn run_removal_steps(&mut self, node: NodeId) {
        if self.iterators.is_empty() || self.iterators.quiet {
            return;
        }
        let mut iterators = std::mem::take(&mut self.iterators);
        iterators.before_removal(self, node);
        self.iterators = iterators;
    }

    /// Runs `change` with the pre-removing steps off: the parser's moves run
    /// none (measured).
    pub(crate) fn parser_moves<T>(&mut self, change: impl FnOnce(&mut Document) -> T) -> T {
        let before = std::mem::replace(&mut self.iterators.quiet, true);
        let result = change(self);
        self.iterators.quiet = before;
        result
    }
}

#[cfg(test)]
mod tests;
