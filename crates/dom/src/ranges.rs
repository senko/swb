//! Live ranges (DOM section 5, <https://dom.spec.whatwg.org/#ranges>; ADR 0039
//! section 1): their boundary points as plain data in the arena that holds the
//! nodes, the steps of the change functions that keep them live, and the
//! boundary point algorithms of `Range`.
//!
//! A [`Document`] keeps one entry per live range, keyed by an id that the host
//! hands out. Nothing is installed until the first range of an arena: a change
//! in an arena without ranges costs one test of an `Option`. The offset of a
//! boundary point in a text, comment or processing instruction node is a byte
//! offset in its UTF-8 data (always at a character boundary); in other nodes it
//! is a child index. The host converts byte offsets to the UTF-16 offsets of
//! the API.
//!
//! The change functions call the steps:
//!
//! - an insertion before a child ([`Document::link`]): offsets at the parent after
//!   the child's index grow (an append at the end changes nothing);
//! - a removal ([`Document::unlink_with`], also when the iterators' steps are
//!   batched): boundary points in the removed subtree go to `(parent, index)`,
//!   offsets at the parent after the index shrink; the boundary points in the
//!   subtree are found with the search that the iterators use
//!   (`subtree_search.rs`);
//! - "replace data", "split a Text node" and `normalize` (its steps are
//!   collected per parent and run at the end: `ranges/normalize.rs`).
//!
//! A range whose root moves to another arena (a detached tree adopted by a
//! document of another arena) goes with it ([`Document::take_live_ranges`]).
//! The work of the steps is counted ([`ArenaWork`]) and the host charges it to
//! the time limit. The entries of ranges whose objects the collector freed go
//! on a shared list ([`crate::DeadIds`]); the arena drops them when it next
//! looks.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use crate::subtree_search::{self, SCAN_BELOW};
use crate::tree::{Document, NodeData, NodeId};
use crate::work::ArenaWork;

mod normalize;

pub(crate) use normalize::NormalizeSteps;

/// A boundary point: a node and an offset (bytes of the data of a character
/// data node, else a child index).
/// <https://dom.spec.whatwg.org/#concept-range-bp>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Boundary {
    /// The node.
    pub node: NodeId,
    /// The offset.
    pub offset: u32,
}

impl Boundary {
    /// The boundary point `(node, offset)`.
    pub fn new(node: NodeId, offset: u32) -> Boundary {
        Boundary { node, offset }
    }
}

/// The start and the end of a range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveRange {
    /// The start.
    pub start: Boundary,
    /// The end.
    pub end: Boundary,
}

impl LiveRange {
    /// A range collapsed at `point`.
    pub fn collapsed_at(point: Boundary) -> LiveRange {
        LiveRange {
            start: point,
            end: point,
        }
    }

    /// True if the start is the end.
    pub fn is_collapsed(&self) -> bool {
        self.start == self.end
    }
}

/// Which boundary point of a range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    Start,
    End,
}

/// The live ranges of an arena.
#[derive(Debug, Default)]
pub(crate) struct LiveRanges {
    entries: HashMap<u32, LiveRange>,
    /// `(node, id, end)` for each boundary point: `end` is true for the end.
    by_node: BTreeSet<(NodeId, u32, bool)>,
    work: Option<ArenaWork>,
    /// How much of the list of dead ranges this arena has dropped.
    dead_seen: (u32, usize),
}

impl LiveRanges {
    fn charge(&self, units: usize) {
        if let Some(work) = &self.work {
            work.charge(units);
        }
    }

    /// The index entries of the boundary points at `node`.
    fn at(&self, node: NodeId) -> impl Iterator<Item = &(NodeId, u32, bool)> {
        self.by_node
            .range((node, 0, false)..=(node, u32::MAX, true))
    }

    /// True if a boundary point is at `node`.
    fn has(&self, node: NodeId) -> bool {
        self.at(node).next().is_some()
    }

    /// Records range `id` and keeps the index.
    fn place(&mut self, id: u32, range: LiveRange) {
        if let Some(old) = self.entries.get(&id).copied() {
            if old == range {
                return;
            }
            self.by_node.remove(&(old.start.node, id, false));
            self.by_node.remove(&(old.end.node, id, true));
        }
        self.by_node.insert((range.start.node, id, false));
        self.by_node.insert((range.end.node, id, true));
        self.entries.insert(id, range);
    }

    fn forget(&mut self, id: u32) -> Option<LiveRange> {
        let range = self.entries.remove(&id)?;
        self.by_node.remove(&(range.start.node, id, false));
        self.by_node.remove(&(range.end.node, id, true));
        Some(range)
    }

    /// Drops the entries of the ranges on the list of dead ones.
    fn drop_dead(&mut self) {
        let Some(work) = &self.work else {
            return;
        };
        for id in work.dead.fresh(&mut self.dead_seen) {
            self.forget(id);
        }
    }

    /// The ids with a boundary point at `node`, each once.
    fn ids_at(&self, node: NodeId) -> Vec<u32> {
        let mut ids: Vec<u32> = self.at(node).map(|&(_, id, _)| id).collect();
        ids.dedup();
        ids
    }

    /// Changes the boundary points at `node` of every range that has one there.
    /// `change` moves the offset only, so the index stays.
    fn update_at(&mut self, node: NodeId, mut change: impl FnMut(&mut u32)) {
        let mut count = 0;
        for &(_, id, end) in self
            .by_node
            .range((node, 0, false)..=(node, u32::MAX, true))
        {
            count += 1;
            if let Some(range) = self.entries.get_mut(&id) {
                change(if end {
                    &mut range.end.offset
                } else {
                    &mut range.start.offset
                });
            }
        }
        self.charge(count);
    }

    /// The steps of an insertion of `count` nodes into `parent` before `before`
    /// (an offset after the index of `before` grows by `count`).
    /// <https://dom.spec.whatwg.org/#concept-node-insert>
    fn inserting(&mut self, doc: &Document, parent: NodeId, before: NodeId, count: u32) {
        self.drop_dead();
        if !self.has(parent) {
            return;
        }
        let mut steps = 1;
        let index = index_counted(doc, before, &mut steps);
        self.charge(steps);
        self.update_at(parent, |o| {
            if *o > index {
                *o = o.saturating_add(count);
            }
        });
    }

    /// The live range pre-remove steps for `node`, which is still in its place.
    /// <https://dom.spec.whatwg.org/#live-range-pre-remove-steps>
    fn removing(&mut self, doc: &Document, node: NodeId) {
        self.drop_dead();
        let Some(parent) = doc.parent(node) else {
            return;
        };
        if self.entries.is_empty() {
            return;
        }
        let mut steps = 1;
        let few = self.entries.len() <= SCAN_BELOW;
        let positions = self.entries.iter().flat_map(|(&id, r)| {
            [
                ((id, End::Start), r.start.node),
                ((id, End::End), r.end.node),
            ]
        });
        let hits = subtree_search::pointers_in_subtree(
            doc,
            node,
            few,
            positions,
            |descendant, hits, steps| {
                for &(_, id, end) in self.at(descendant) {
                    *steps += 1;
                    hits.push((id, if end { End::End } else { End::Start }));
                }
            },
            &mut steps,
        );
        if hits.is_empty() && !self.has(parent) {
            self.charge(steps);
            return;
        }
        let index = index_counted(doc, node, &mut steps);
        self.charge(steps);
        // Steps 4 and 5 first: the boundary points that steps 2 and 3 move to
        // `(parent, index)` are not after the index.
        self.update_at(parent, |o| {
            if *o > index {
                *o -= 1;
            }
        });
        let to = Boundary::new(parent, index);
        for (id, end) in hits {
            let Some(mut range) = self.entries.get(&id).copied() else {
                continue;
            };
            match end {
                End::Start => range.start = to,
                End::End => range.end = to,
            }
            self.place(id, range);
        }
    }

    /// The live range pre-remove steps for all children of `parent`, removed
    /// one after the other from the first: every boundary point in `parent`
    /// or below it ends at `(parent, 0)` (an offset at `parent` shrinks by one
    /// for each child removed, to 0 at the least; a boundary point in a removed
    /// subtree goes to `(parent, 0)`, where the next removals leave it). One
    /// pass over the subtree or the boundary points instead of one per child.
    fn removing_all(&mut self, doc: &Document, parent: NodeId) {
        self.drop_dead();
        if self.entries.is_empty() {
            return;
        }
        let mut steps = 1;
        let few = self.entries.len() <= SCAN_BELOW;
        let positions = self.entries.iter().flat_map(|(&id, r)| {
            [
                ((id, End::Start), r.start.node),
                ((id, End::End), r.end.node),
            ]
        });
        let hits = subtree_search::pointers_in_subtree(
            doc,
            parent,
            few,
            positions,
            |descendant, hits, steps| {
                for &(_, id, end) in self.at(descendant) {
                    *steps += 1;
                    hits.push((id, if end { End::End } else { End::Start }));
                }
            },
            &mut steps,
        );
        self.charge(steps);
        let to = Boundary::new(parent, 0);
        for (id, end) in hits {
            let Some(mut range) = self.entries.get(&id).copied() else {
                continue;
            };
            match end {
                End::Start => range.start = to,
                End::End => range.end = to,
            }
            self.place(id, range);
        }
    }

    /// The live range steps of "replace data": `count` bytes from `offset` of
    /// `node` became `len` bytes.
    /// <https://dom.spec.whatwg.org/#concept-cd-replace>
    fn replacing(&mut self, node: NodeId, offset: u32, count: u32, len: u32) {
        self.drop_dead();
        if !self.has(node) {
            return;
        }
        let end = offset.saturating_add(count);
        self.update_at(node, |o| {
            if *o > offset && *o <= end {
                *o = offset;
            } else if *o > end {
                *o = (*o - count).saturating_add(len);
            }
        });
    }
}

/// The index of `node` among its siblings; `steps` counts the siblings.
fn index_counted(doc: &Document, node: NodeId, steps: &mut usize) -> u32 {
    let mut index = 0u32;
    let mut current = doc.node(node).prev_sibling;
    while let Some(n) = current {
        index += 1;
        current = doc.node(n).prev_sibling;
    }
    *steps += index as usize;
    index
}

impl Document {
    /// Gives the arena the counters through which it reports the work of the
    /// live range steps, and the list of dead ranges.
    pub fn set_range_work(&mut self, work: ArenaWork) {
        let ranges = self.ranges.get_or_insert_with(Box::default);
        let same_list = ranges.work.as_ref().is_some_and(|old| old.same_list(&work));
        if !same_list {
            ranges.dead_seen = work.dead.start();
        }
        ranges.work = Some(work);
    }

    /// Adds (or replaces) live range `id`.
    pub fn set_live_range(&mut self, id: u32, range: LiveRange) {
        self.ranges
            .get_or_insert_with(Box::default)
            .place(id, range);
    }

    /// Removes live range `id`.
    pub fn remove_live_range(&mut self, id: u32) {
        if let Some(ranges) = &mut self.ranges {
            ranges.forget(id);
        }
    }

    /// The boundary points of live range `id`.
    pub fn live_range(&self, id: u32) -> Option<LiveRange> {
        self.ranges.as_ref()?.entries.get(&id).copied()
    }

    /// The number of live ranges of the arena (after the dead ones went).
    pub fn live_range_count(&mut self) -> usize {
        match &mut self.ranges {
            Some(ranges) => {
                ranges.drop_dead();
                ranges.entries.len()
            }
            None => 0,
        }
    }

    /// Charges `units` of work to the host's counters of the live ranges.
    pub fn charge_range_work(&self, units: usize) {
        if let Some(ranges) = &self.ranges {
            ranges.charge(units);
        }
    }

    /// Takes the ranges whose start is at one of the old nodes of `pairs` (old
    /// id, id in another arena) out of this arena, with their nodes mapped to
    /// the new ids, for [`Document::put_live_ranges`] of that arena. A range
    /// with a boundary point in a moved subtree whose root had a parent was
    /// moved out of it by the removal; what is left has both boundary points in
    /// the moved tree.
    pub fn take_live_ranges(&mut self, pairs: &[(NodeId, NodeId)]) -> Vec<(u32, LiveRange)> {
        let Some(ranges) = &mut self.ranges else {
            return Vec::new();
        };
        ranges.drop_dead();
        if ranges.entries.is_empty() {
            return Vec::new();
        }
        let map: HashMap<NodeId, NodeId> = pairs.iter().copied().collect();
        let mut ids: Vec<u32> = Vec::new();
        for &(old, _) in pairs {
            ids.extend(ranges.at(old).map(|&(_, id, _)| id));
        }
        ids.sort_unstable();
        ids.dedup();
        let mut moved = Vec::new();
        for id in ids {
            let Some(range) = ranges.entries.get(&id).copied() else {
                continue;
            };
            let (Some(&start), Some(&end)) = (map.get(&range.start.node), map.get(&range.end.node))
            else {
                continue;
            };
            ranges.forget(id);
            moved.push((
                id,
                LiveRange {
                    start: Boundary::new(start, range.start.offset),
                    end: Boundary::new(end, range.end.offset),
                },
            ));
        }
        moved
    }

    /// Puts ranges that [`Document::take_live_ranges`] took from another arena
    /// into this one.
    pub fn put_live_ranges(&mut self, moved: Vec<(u32, LiveRange)>) {
        if moved.is_empty() {
            return;
        }
        let ranges = self.ranges.get_or_insert_with(Box::default);
        for (id, range) in moved {
            ranges.place(id, range);
        }
    }

    /// The steps of an insertion of `count` nodes into `parent` before `before`.
    #[inline]
    pub(crate) fn range_insert_steps(
        &mut self,
        parent: NodeId,
        before: Option<NodeId>,
        count: usize,
    ) {
        // An append changes no offset: none is greater than the child count.
        let Some(before) = before else {
            return;
        };
        let Some(mut ranges) = self.ranges.take() else {
            return;
        };
        ranges.inserting(
            self,
            parent,
            before,
            u32::try_from(count).unwrap_or(u32::MAX),
        );
        self.ranges = Some(ranges);
    }

    /// The live range pre-remove steps for `node`, which still has its parent.
    #[inline]
    pub(crate) fn range_remove_steps(&mut self, node: NodeId) {
        let Some(mut ranges) = self.ranges.take() else {
            return;
        };
        ranges.removing(self, node);
        self.ranges = Some(ranges);
    }

    /// The live range steps of "replace data" (byte offsets).
    #[inline]
    pub(crate) fn range_replace_steps(
        &mut self,
        node: NodeId,
        offset: usize,
        count: usize,
        len: usize,
    ) {
        let Some(ranges) = &mut self.ranges else {
            return;
        };
        let to_u32 = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        ranges.replacing(node, to_u32(offset), to_u32(count), to_u32(len));
    }

    /// Steps 7.2 and 7.3 of "split a Text node": the boundary points in `node`
    /// after `offset` (bytes) move to `new_node`.
    /// <https://dom.spec.whatwg.org/#concept-text-split>
    pub(crate) fn range_split_moves(&mut self, node: NodeId, new_node: NodeId, offset: u32) {
        let Some(ranges) = &mut self.ranges else {
            return;
        };
        ranges.drop_dead();
        for id in ranges.ids_at(node) {
            let Some(mut range) = ranges.entries.get(&id).copied() else {
                continue;
            };
            for b in [&mut range.start, &mut range.end] {
                if b.node == node && b.offset > offset {
                    *b = Boundary::new(new_node, b.offset - offset);
                }
            }
            ranges.place(id, range);
        }
    }

    /// Steps 7.4 and 7.5 of "split a Text node": offsets at `parent` equal to
    /// `index` grow by one.
    pub(crate) fn range_split_parent(&mut self, parent: NodeId, index: u32) {
        let Some(ranges) = &mut self.ranges else {
            return;
        };
        if !ranges.has(parent) {
            return;
        }
        ranges.update_at(parent, |o| {
            if *o == index {
                *o += 1;
            }
        });
    }

    /// The live range pre-remove steps for all children of `parent` that leave
    /// one after the other from the first (`replace all`, a fragment's
    /// children). Call before the first removal; the removals then skip the
    /// steps.
    pub(crate) fn range_remove_all_steps(&mut self, parent: NodeId) {
        let Some(mut ranges) = self.ranges.take() else {
            return;
        };
        ranges.removing_all(self, parent);
        self.ranges = Some(ranges);
    }

    // ----- The boundary point algorithms -----

    /// DOM "length" of a node: the bytes of the data of a character data node,
    /// 0 for a doctype, else the number of children.
    /// <https://dom.spec.whatwg.org/#concept-node-length>
    pub fn node_length(&self, node: NodeId) -> u32 {
        let n = self.node(node);
        let len = match &n.data {
            NodeData::Text(t) | NodeData::Comment(t) => t.len(),
            NodeData::ProcessingInstruction { data, .. } => data.len(),
            NodeData::Doctype { .. } => 0,
            _ => return n.child_count,
        };
        u32::try_from(len).unwrap_or(u32::MAX)
    }

    /// The index of `node` among its siblings.
    /// <https://dom.spec.whatwg.org/#concept-tree-index>
    pub fn index_in_parent(&self, node: NodeId) -> u32 {
        let mut steps = 0;
        let index = index_counted(self, node, &mut steps);
        self.charge_range_work(steps);
        index
    }

    /// The position of boundary point `a` relative to `b`, which must have the
    /// same root: `Less` for before, `Greater` for after.
    /// <https://dom.spec.whatwg.org/#concept-range-bp-position>
    pub fn compare_boundaries(&self, a: Boundary, b: Boundary) -> Ordering {
        if a.node == b.node {
            return a.offset.cmp(&b.offset);
        }
        // The inclusive ancestors of each, root first.
        let path = |n: NodeId| {
            let mut path: Vec<NodeId> = std::iter::once(n).chain(self.ancestors(n)).collect();
            path.reverse();
            path
        };
        let (pa, pb) = (path(a.node), path(b.node));
        self.charge_range_work(pa.len() + pb.len());
        let common = pa.iter().zip(&pb).take_while(|(x, y)| x == y).count();
        match (pa.get(common), pb.get(common)) {
            // `a.node` is an ancestor of `b.node`: the child of `a.node` on the
            // path decides.
            (None, Some(&child)) => {
                if self.index_in_parent(child) < a.offset {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (Some(&child), None) => {
                if self.index_in_parent(child) < b.offset {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (Some(&ca), Some(&cb)) => {
                if self.precedes_within(ca, cb, usize::MAX, &mut 0) == Some(true) {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            // Not reached: the nodes differ.
            (None, None) => Ordering::Equal,
        }
    }

    /// DOM "set the start or end" of `range` to `point` (checked): the new
    /// range. A point in another root, or on the wrong side of the other
    /// boundary point, collapses the range there.
    /// <https://dom.spec.whatwg.org/#concept-range-bp-set>
    pub fn range_with(&self, range: LiveRange, point: Boundary, start: bool) -> LiveRange {
        let other = if start { range.end } else { range.start };
        let other_root = self.root(other.node);
        let same_root = other_root == self.root(point.node);
        let wrong_side = same_root && {
            let order = self.compare_boundaries(point, other);
            if start {
                order == Ordering::Greater
            } else {
                order == Ordering::Less
            }
        };
        if !same_root || wrong_side {
            return LiveRange::collapsed_at(point);
        }
        if start {
            LiveRange {
                start: point,
                end: range.end,
            }
        } else {
            LiveRange {
                start: range.start,
                end: point,
            }
        }
    }

    /// The common ancestor container of `range`: the nearest inclusive ancestor
    /// of the start that is an inclusive ancestor of the end.
    /// <https://dom.spec.whatwg.org/#dom-range-commonancestorcontainer>
    pub fn range_common_ancestor(&self, range: LiveRange) -> NodeId {
        let ends: std::collections::HashSet<NodeId> = std::iter::once(range.end.node)
            .chain(self.ancestors(range.end.node))
            .collect();
        let mut steps = ends.len();
        let mut found = range.start.node;
        for n in std::iter::once(range.start.node).chain(self.ancestors(range.start.node)) {
            steps += 1;
            if ends.contains(&n) {
                found = n;
                break;
            }
        }
        self.charge_range_work(steps);
        found
    }

    /// `Range.intersectsNode` for a node in the range's root.
    /// <https://dom.spec.whatwg.org/#dom-range-intersectsnode>
    pub fn range_intersects(&self, range: LiveRange, node: NodeId) -> bool {
        let Some(parent) = self.parent(node) else {
            return true;
        };
        let offset = self.index_in_parent(node);
        self.compare_boundaries(Boundary::new(parent, offset), range.end) == Ordering::Less
            && self.compare_boundaries(Boundary::new(parent, offset + 1), range.start)
                == Ordering::Greater
    }

    /// The node after the subtree of `node` in tree order, if any.
    fn after_subtree(&self, node: NodeId) -> Option<NodeId> {
        let mut current = node;
        loop {
            let n = self.get(current)?;
            if let Some(next) = n.next_sibling() {
                return Some(next);
            }
            current = n.parent()?;
        }
    }

    /// The first node in tree order that comes after the boundary point `b`
    /// (the child at the offset, else the node after the subtree); for a
    /// character data node, the node after it.
    fn first_after(&self, b: Boundary) -> Option<NodeId> {
        if self.is_character_data(b.node) {
            return self.after_subtree(b.node);
        }
        let mut child = self.node(b.node).first_child();
        for _ in 0..b.offset {
            child = child.and_then(|c| self.node(c).next_sibling());
        }
        child.or_else(|| self.after_subtree(b.node))
    }

    fn is_character_data(&self, node: NodeId) -> bool {
        matches!(
            self.node(node).data,
            NodeData::Text(_) | NodeData::Comment(_) | NodeData::ProcessingInstruction { .. }
        )
    }

    /// The stringification of `range`: the selected data of the text nodes.
    /// <https://dom.spec.whatwg.org/#dom-range-stringifier>
    pub fn range_text(&self, range: LiveRange) -> String {
        let text_at = |node: NodeId| match &self.node(node).data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        };
        let slice = |t: &str, from: u32, to: u32| {
            let (from, to) = (from as usize, (to as usize).min(t.len()));
            t.get(from.min(to)..to).unwrap_or_default().to_owned()
        };
        if range.start.node == range.end.node && self.is_character_data(range.start.node) {
            return text_at(range.start.node)
                .map(|t| slice(t, range.start.offset, range.end.offset))
                .unwrap_or_default();
        }
        let mut out = String::new();
        if let Some(t) = text_at(range.start.node) {
            out.push_str(&slice(t, range.start.offset, u32::MAX));
        }
        // The node where the walk stops: the end's text node, or the first node
        // after the end boundary point.
        let stop = if self.is_character_data(range.end.node) {
            Some(range.end.node)
        } else {
            self.first_after(range.end)
        };
        let mut steps = 0usize;
        let mut current = self.first_after(range.start);
        while let Some(n) = current {
            if Some(n) == stop {
                break;
            }
            steps += 1;
            if let Some(t) = text_at(n) {
                out.push_str(t);
            }
            current = match self.node(n).first_child() {
                Some(child) => Some(child),
                None => self.after_subtree(n),
            };
        }
        if let Some(t) = text_at(range.end.node) {
            out.push_str(&slice(t, 0, range.end.offset));
        }
        self.charge_range_work(steps);
        out
    }
}

#[cfg(test)]
mod tests;
