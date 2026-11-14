//! The search for the live positions in a subtree that leaves its parent:
//! which `NodeIterator` pointers (`iterator.rs`) and which boundary points of
//! live ranges (`ranges.rs`) are at a node of the removed subtree (ADR 0028
//! "Feature 15a", ADR 0039 section 1).
//!
//! With few positions, each climbs toward the root for a few steps first, which
//! settles it in a shallow tree; a position that has not settled then takes
//! part in a walk where its climb and the walk through the subtree take turns,
//! and the first that answers decides, so a removal far from the positions
//! costs the size of its subtree, not their depth. With many positions, the
//! subtree is walked and each node is looked up in the registry's index.

use crate::tree::{Document, Node, NodeId};

/// Up to this many positions, a removal looks for them by walking from the
/// positions and through the removed subtree in turns; with more, it walks the
/// removed subtree and looks each node up in the index.
pub(crate) const SCAN_BELOW: usize = 8;

/// The steps that a position climbs toward the root before its climb takes
/// turns with the walk through the removed subtree.
const CLIMB_FIRST: usize = 64;

/// A position whose climb toward the root takes turns with the walk through
/// the removed subtree: its key, its node and where the climb is.
struct Climb<K> {
    key: K,
    start: NodeId,
    at: NodeId,
}

/// The keys of the positions in the subtree of `node`. With `few`, `positions`
/// lists every position (its key and the node it is at) and the climbs answer;
/// otherwise the subtree is walked and `at` adds the keys of the positions at
/// each node. `steps` counts the work.
pub(crate) fn pointers_in_subtree<K: Copy>(
    doc: &Document,
    node: NodeId,
    few: bool,
    positions: impl Iterator<Item = (K, NodeId)>,
    mut at: impl FnMut(NodeId, &mut Vec<K>, &mut usize),
    steps: &mut usize,
) -> Vec<K> {
    let mut hits = Vec::new();
    if !few {
        for descendant in doc.descendants(node) {
            *steps += 1;
            at(descendant, &mut hits, steps);
        }
        return hits;
    }
    let mut open: Vec<Climb<K>> = Vec::new();
    for (key, start) in positions {
        let mut current = Some(start);
        let mut budget = CLIMB_FIRST;
        while let Some(n) = current {
            *steps += 1;
            if n == node {
                hits.push(key);
                break;
            }
            if budget == 0 {
                open.push(Climb { key, start, at: n });
                break;
            }
            budget -= 1;
            current = doc.get(n).and_then(Node::parent);
        }
    }
    let mut subtree = doc.descendants(node);
    let mut subtree_done = false;
    while !open.is_empty() && !subtree_done {
        *steps += 1;
        match subtree.next() {
            Some(descendant) => open.retain(|c| {
                let found = c.start == descendant;
                if found {
                    hits.push(c.key);
                }
                !found
            }),
            None => subtree_done = true,
        }
        open.retain_mut(|c| {
            *steps += 1;
            if c.at == node {
                hits.push(c.key);
                return false;
            }
            match doc.get(c.at).and_then(Node::parent) {
                Some(parent) => {
                    c.at = parent;
                    true
                }
                None => false,
            }
        });
    }
    hits
}
