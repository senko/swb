//! The live range steps of `normalize` (DOM "normalize", steps 6.1 to 6.4, and
//! the pre-remove steps of the nodes that it removes; ADR 0039 section 6).
//!
//! `normalize` collects the steps per parent while it changes the tree, in the
//! indices that the children had before the call ("old indices"), and runs them
//! at the end: one pass over the boundary points at each parent, whatever the
//! number of runs and empty text nodes. The index of each node comes from a
//! cursor per parent that only moves forward. The work of one call is linear in
//! the nodes and the boundary points. The ranges are not read during the call,
//! so the order of the steps cannot be seen.
//! <https://dom.spec.whatwg.org/#dom-node-normalize>

use std::collections::HashMap;

use super::{Boundary, index_counted};
use crate::tree::{Document, NodeId};

/// The live range steps of one `normalize` call.
#[derive(Debug, Default)]
pub(crate) struct NormalizeSteps {
    parents: HashMap<NodeId, ParentChanges>,
}

/// What `normalize` did to the children of one parent, in old indices.
#[derive(Debug, Default)]
struct ParentChanges {
    /// A child in the tree as it is now and its index: the next index is
    /// counted forward from it (`None`: from the first child).
    cursor: Option<(NodeId, u32)>,
    /// The old indices of the children removed so far (empty text nodes and
    /// merged ones), in increasing order: `normalize` goes forward.
    removed: Vec<u32>,
    /// One entry per run of merged nodes: the text node that they merged into,
    /// the old index of the first merged node, and for each merged node the
    /// bytes of the data of the text node before it merged.
    runs: Vec<(NodeId, u32, Vec<u32>)>,
}

impl ParentChanges {
    /// The index of `child` of `parent` in the tree as it is now, counted
    /// forward from the cursor; the cursor moves to `child`.
    fn index_of(&mut self, doc: &Document, parent: NodeId, child: NodeId, work: &mut usize) -> u32 {
        let (mut current, mut index) = match self.cursor {
            Some((node, index)) => (doc.node(node).next_sibling, index.saturating_add(1)),
            None => (doc.node(parent).first_child, 0),
        };
        while let Some(n) = current {
            *work += 1;
            if n == child {
                self.cursor = Some((child, index));
                return index;
            }
            index = index.saturating_add(1);
            current = doc.node(n).next_sibling;
        }
        // Not after the cursor (not expected: the walk goes forward).
        let index = index_counted(doc, child, work);
        self.cursor = Some((child, index));
        index
    }

    /// The old index of `child`, whose index is `index` now: the removed
    /// children are all before it.
    fn old_index(&self, index: u32) -> u32 {
        index.saturating_add(len_u32(self.removed.len()))
    }

    /// Where the boundary point `(parent, offset)` ends, with `offset` an old
    /// index.
    fn map(&self, parent: NodeId, offset: u32) -> Boundary {
        let run = self.runs.partition_point(|&(_, first, _)| first <= offset);
        if let Some((text, first, lengths)) = run.checked_sub(1).and_then(|i| self.runs.get(i))
            && let Some(&length) = lengths.get(offset.saturating_sub(*first) as usize)
        {
            // Steps 6.3 and 6.4: the point is before a merged node.
            return Boundary::new(*text, length);
        }
        // Steps 4 and 5 of the pre-remove steps: one less for each child
        // removed before the point.
        let before = self.removed.partition_point(|&r| r < offset);
        Boundary::new(parent, offset.saturating_sub(len_u32(before)))
    }
}

impl Document {
    /// The collector of the live range steps of `normalize`, or `None` when the
    /// arena has no live range (the steps are then skipped).
    pub(crate) fn normalize_steps(&mut self) -> Option<NormalizeSteps> {
        let ranges = self.ranges.as_mut()?;
        ranges.drop_dead();
        Some(NormalizeSteps::default())
    }

    /// The steps of the removal of the empty text node `text`, which still has
    /// its parent; the removal then skips the range steps. A boundary point in
    /// `text` goes to the parent at the old index of `text` (steps 2 and 3 of
    /// the pre-remove steps; the pass at the end maps the old index).
    pub(crate) fn normalize_remove_empty(&mut self, steps: &mut NormalizeSteps, text: NodeId) {
        let Some(parent) = self.parent(text) else {
            return;
        };
        let mut work = 1;
        let changes = steps.parents.entry(parent).or_default();
        let index = changes.index_of(self, parent, text, &mut work);
        let old = changes.old_index(index);
        changes.removed.push(old);
        // `text` leaves: count from the node before it next time.
        changes.cursor = self
            .node(text)
            .prev_sibling
            .map(|p| (p, index.saturating_sub(1)));
        self.move_points(text, |_| Boundary::new(parent, old), &mut work);
        self.charge_range_work(work);
    }

    /// The steps of a run: the text nodes `merged`, each with the bytes of the
    /// data of `text` before it merged, merged into `text` and left the tree
    /// without the range steps. Steps 6.1 and 6.2 (the boundary points in the
    /// merged nodes) run now; steps 6.3 and 6.4 and the removals' steps at the
    /// parent run at the end.
    pub(crate) fn normalize_merged(
        &mut self,
        steps: &mut NormalizeSteps,
        text: NodeId,
        merged: &[(NodeId, usize)],
    ) {
        if merged.is_empty() {
            return;
        }
        let Some(parent) = self.parent(text) else {
            return;
        };
        let mut work = merged.len();
        for &(node, length) in merged {
            let length = len_u32(length);
            self.move_points(
                node,
                |offset| Boundary::new(text, offset.saturating_add(length)),
                &mut work,
            );
        }
        let changes = steps.parents.entry(parent).or_default();
        let index = changes.index_of(self, parent, text, &mut work);
        let first = changes.old_index(index).saturating_add(1);
        changes
            .removed
            .extend((0..len_u32(merged.len())).map(|j| first.saturating_add(j)));
        let lengths = merged.iter().map(|&(_, length)| len_u32(length)).collect();
        changes.runs.push((text, first, lengths));
        self.charge_range_work(work);
    }

    /// Runs the collected steps at the parents: one pass over the boundary
    /// points at each parent whose children changed.
    pub(crate) fn finish_normalize(&mut self, steps: NormalizeSteps) {
        let Some(mut ranges) = self.ranges.take() else {
            return;
        };
        let mut work = 0;
        for (parent, changes) in steps.parents {
            if changes.removed.is_empty() {
                continue;
            }
            for id in ranges.ids_at(parent) {
                work += 1;
                let Some(mut range) = ranges.entries.get(&id).copied() else {
                    continue;
                };
                for b in [&mut range.start, &mut range.end] {
                    if b.node == parent {
                        *b = changes.map(parent, b.offset);
                    }
                }
                ranges.place(id, range);
            }
        }
        ranges.charge(work);
        self.ranges = Some(ranges);
    }

    /// Moves every boundary point at `node` to `to(offset)`.
    fn move_points(&mut self, node: NodeId, to: impl Fn(u32) -> Boundary, work: &mut usize) {
        let Some(ranges) = &mut self.ranges else {
            return;
        };
        for id in ranges.ids_at(node) {
            *work += 1;
            let Some(mut range) = ranges.entries.get(&id).copied() else {
                continue;
            };
            for b in [&mut range.start, &mut range.end] {
                if b.node == node {
                    *b = to(b.offset);
                }
            }
            ranges.place(id, range);
        }
    }
}

fn len_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
