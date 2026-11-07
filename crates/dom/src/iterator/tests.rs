//! Tests of the iterator positions and the pre-removing steps. The values of
//! the rule are Chromium's (`tools/probes/web-node-iterator.json`, cases
//! `remove-*`); `crates/web/tests/node_iterator.rs` runs them against every tree
//! of up to five nodes.

use std::cell::Cell;
use std::rc::Rc;

use super::*;
use crate::tree::NodeData;
use crate::{LocalName, QualName, ns};

fn element(doc: &mut Document) -> NodeId {
    let name = QualName::new(None, ns!(html), LocalName::from("p"));
    doc.create_element(name, vec![]).expect("room")
}

/// A tree from a word of parentheses (`(()())`): the root first, then the nodes in
/// tree order.
fn tree(doc: &mut Document, word: &str) -> Vec<NodeId> {
    let root = element(doc);
    let mut nodes = vec![root];
    let mut stack = vec![root];
    for c in word.chars() {
        if c == '(' {
            let node = element(doc);
            doc.append_child(*stack.last().unwrap(), node).unwrap();
            stack.push(node);
            nodes.push(node);
        } else {
            stack.pop();
        }
    }
    nodes
}

/// The index of a node of `nodes` and the flag, for comparing.
fn at(doc: &Document, nodes: &[NodeId], id: u32) -> (usize, bool) {
    let p = doc.node_iterator(id).expect("an iterator");
    (
        nodes
            .iter()
            .position(|&n| Spot::Here(n) == p.reference)
            .expect("a node"),
        p.pointer_before,
    )
}

/// Puts iterator `id` (made at the root) at node `index`: after it, or before.
fn place(doc: &mut Document, nodes: &[NodeId], id: u32, index: usize, before: bool) {
    walk_to(doc, id, nodes[index]);
    if before {
        step(doc, id, false);
    }
}

fn walk_to(doc: &mut Document, id: u32, target: NodeId) {
    while let Some(node) = step(doc, id, true) {
        if node == target {
            return;
        }
    }
}

/// One accepted step: the node, or `None` at the end.
fn step(doc: &mut Document, id: u32, forward: bool) -> Option<NodeId> {
    assert!(doc.node_iterator_begin(id));
    let node = doc.node_iterator_advance(id, forward);
    if node.is_some() {
        doc.node_iterator_accept(id);
    } else {
        doc.node_iterator_end(id);
    }
    node
}

#[test]
fn an_iterator_starts_before_its_root_and_walks_the_tree_in_order() {
    let mut doc = Document::new();
    let nodes = tree(&mut doc, "(()())()");
    doc.add_node_iterator(1, nodes[0]);
    assert_eq!(at(&doc, &nodes, 1), (0, true));
    let mut seen = Vec::new();
    while let Some(node) = step(&mut doc, 1, true) {
        seen.push(nodes.iter().position(|&n| n == node).unwrap());
    }
    assert_eq!(seen, [0, 1, 2, 3, 4]);
    assert_eq!(at(&doc, &nodes, 1), (4, false));
    let mut back = Vec::new();
    while let Some(node) = step(&mut doc, 1, false) {
        back.push(nodes.iter().position(|&n| n == node).unwrap());
    }
    assert_eq!(back, [4, 3, 2, 1, 0]);
    assert_eq!(at(&doc, &nodes, 1), (0, true));
}

#[test]
fn a_traversal_that_ends_or_does_not_accept_leaves_the_reference() {
    let mut doc = Document::new();
    let nodes = tree(&mut doc, "()()");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 1, false);
    assert!(doc.node_iterator_begin(1));
    assert_eq!(doc.node_iterator_advance(1, true), Some(nodes[2]));
    // The filter threw: the reference stays.
    doc.node_iterator_end(1);
    assert_eq!(at(&doc, &nodes, 1), (1, false));
    // No traversal runs: nothing to advance.
    assert_eq!(doc.node_iterator_advance(1, true), None);
    assert!(!doc.node_iterator_begin(99));
}

#[test]
fn a_pointer_after_a_removed_node_goes_to_the_node_before_it() {
    let mut doc = Document::new();
    // r > n1 > n2, n3; n4
    let nodes = tree(&mut doc, "(()())()");
    doc.add_node_iterator(1, nodes[0]);
    // After n3, n3 is removed: the previous sibling n2 (a leaf).
    place(&mut doc, &nodes, 1, 3, false);
    doc.remove(nodes[3]);
    assert_eq!(at(&doc, &nodes, 1), (2, false));
    // After n2, n1 is removed: no previous sibling, so the parent.
    doc.remove(nodes[1]);
    assert_eq!(at(&doc, &nodes, 1), (0, false));
}

#[test]
fn the_node_before_a_removed_node_is_the_last_descendant_of_its_previous_sibling() {
    let mut doc = Document::new();
    // r > n1 > n2 > n3; n4
    let nodes = tree(&mut doc, "((()))()");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 4, false);
    doc.remove(nodes[4]);
    assert_eq!(at(&doc, &nodes, 1), (3, false));
}

#[test]
fn a_pointer_before_a_removed_node_goes_to_the_next_node_outside_it() {
    let mut doc = Document::new();
    // r > n1 > n2; n3
    let nodes = tree(&mut doc, "(())()");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 2, true);
    doc.remove(nodes[1]);
    assert_eq!(at(&doc, &nodes, 1), (3, true));
}

#[test]
fn the_next_node_is_searched_inside_the_root_only() {
    let mut doc = Document::new();
    // r > n1 > n2; n3 (n3 follows the iterator's root n1 but is outside it)
    let nodes = tree(&mut doc, "(())()");
    doc.add_node_iterator(1, nodes[1]);
    walk_to(&mut doc, 1, nodes[2]);
    step(&mut doc, 1, false);
    assert_eq!(at(&doc, &nodes, 1), (2, true));
    doc.remove(nodes[2]);
    // No node after n2 inside n1: the flag is false and the reference the parent.
    assert_eq!(at(&doc, &nodes, 1), (1, false));
}

#[test]
fn a_pointer_before_a_removed_node_with_children_and_no_next_node_stays() {
    let mut doc = Document::new();
    // r > n1 > n2 (n1 has children, nothing follows it)
    let nodes = tree(&mut doc, "(())");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 2, true);
    doc.remove(nodes[1]);
    assert_eq!(at(&doc, &nodes, 1), (2, true));
    // A leaf at the end: the specification's result.
    let mut doc = Document::new();
    let nodes = tree(&mut doc, "()()");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 2, true);
    doc.remove(nodes[2]);
    assert_eq!(at(&doc, &nodes, 1), (1, false));
}

#[test]
fn the_root_and_its_ancestors_change_nothing() {
    let mut doc = Document::new();
    let holder = element(&mut doc);
    let nodes = tree(&mut doc, "()");
    doc.append_child(holder, nodes[0]).unwrap();
    doc.append_child(NodeId::DOCUMENT, holder).unwrap();
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 1, false);
    doc.remove(nodes[0]);
    assert_eq!(at(&doc, &nodes, 1), (1, false));
    doc.append_child(holder, nodes[0]).unwrap();
    doc.remove(holder);
    assert_eq!(at(&doc, &nodes, 1), (1, false));
}

#[test]
fn a_node_that_moves_inside_the_root_runs_the_steps_for_its_place_before() {
    let mut doc = Document::new();
    // r > n1, n2
    let nodes = tree(&mut doc, "()()");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 1, false);
    doc.append_child(nodes[0], nodes[1]).unwrap();
    // n1 left its place: the reference is the parent; it is inserted again at the end.
    assert_eq!(at(&doc, &nodes, 1), (0, false));
}

#[test]
fn the_candidate_follows_the_tree_and_becomes_the_reference() {
    let mut doc = Document::new();
    let nodes = tree(&mut doc, "()()");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 0, false);
    assert!(doc.node_iterator_begin(1));
    assert_eq!(doc.node_iterator_advance(1, true), Some(nodes[1]));
    // The filter removes the node it looks at: the candidate becomes its parent.
    doc.remove(nodes[1]);
    doc.node_iterator_accept(1);
    assert_eq!(at(&doc, &nodes, 1), (0, false));
}

#[test]
fn a_reference_in_a_detached_subtree_walks_inside_it() {
    let mut doc = Document::new();
    // r > n1 > n2, n3
    let nodes = tree(&mut doc, "(()())");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 2, false);
    doc.remove(nodes[1]);
    // The pointer moved to the root; the detached node 1 is not walked from there.
    assert_eq!(at(&doc, &nodes, 1), (0, false));
    assert_eq!(step(&mut doc, 1, true), None);
}

#[test]
fn the_children_of_a_text_content_change_leave_together() {
    // r > n1, n2, n3 with the pointer after n3 (batched) or after n2 (not).
    for batched in [false, true] {
        let mut doc = Document::new();
        let nodes = tree(&mut doc, "()()()");
        doc.add_node_iterator(1, nodes[0]);
        place(&mut doc, &nodes, 1, 2, false);
        if batched {
            doc.replace_all_batched(nodes[0], None).unwrap();
        } else {
            doc.replace_all(nodes[0], None).unwrap();
        }
        // Batched: n2's previous sibling n1 is still there. Else n1 is gone.
        assert_eq!(
            at(&doc, &nodes, 1),
            (usize::from(batched), false),
            "{batched}"
        );
    }
}

#[test]
fn the_children_of_a_fragment_leave_together() {
    let mut doc = Document::new();
    let fragment = doc.create_node(NodeData::DocumentFragment).unwrap();
    let a = element(&mut doc);
    let b = element(&mut doc);
    doc.append_child(fragment, a).unwrap();
    doc.append_child(fragment, b).unwrap();
    doc.add_node_iterator(1, fragment);
    walk_to(&mut doc, 1, b);
    let host = element(&mut doc);
    doc.append_child(host, fragment).unwrap();
    // The previous sibling of b was still a when b left.
    assert_eq!(doc.node_iterator(1).unwrap().reference, Spot::Here(a));
}

#[test]
fn the_parser_runs_no_steps() {
    let mut doc = Document::new();
    let nodes = tree(&mut doc, "()()");
    doc.add_node_iterator(1, nodes[0]);
    place(&mut doc, &nodes, 1, 1, false);
    doc.parser_moves(|doc| doc.append_child(nodes[0], nodes[1]).unwrap());
    assert_eq!(at(&doc, &nodes, 1), (1, false));
    // The flag is off again: the node is last now, and leaves again before n2.
    doc.append_child(nodes[0], nodes[1]).unwrap();
    assert_eq!(at(&doc, &nodes, 1), (2, false));
}

/// A word of parentheses for all the trees of `n` nodes.
fn words(n: usize) -> Vec<String> {
    fn go(out: &mut Vec<String>, word: String, open: usize, close: usize, n: usize) {
        if word.len() == 2 * n {
            out.push(word);
            return;
        }
        if open < n {
            go(out, format!("{word}("), open + 1, close, n);
        }
        if close < open {
            go(out, format!("{word})"), open, close + 1, n);
        }
    }
    let mut out = Vec::new();
    go(&mut out, String::new(), 0, 0, n);
    out
}

/// The way through the index (more than `SCAN_BELOW` iterators) gives what the scan
/// of the pointers gives: every tree of four nodes, every removal, every position.
#[test]
fn many_iterators_are_found_through_the_index() {
    for word in words(4) {
        for x in 1..=4 {
            for p in 0..=4 {
                for before in [false, true] {
                    let mut results = Vec::new();
                    for padding in [0, 20] {
                        let mut doc = Document::new();
                        let nodes = tree(&mut doc, &word);
                        for id in 0..padding {
                            doc.add_node_iterator(100 + id, nodes[0]);
                        }
                        doc.add_node_iterator(1, nodes[0]);
                        place(&mut doc, &nodes, 1, p, before);
                        // The padding iterators also move; they must not disturb.
                        doc.remove(nodes[x]);
                        results.push(at(&doc, &nodes, 1));
                    }
                    assert_eq!(results[0], results[1], "{word} x{x} p{p} {before}");
                }
            }
        }
    }
}

/// The pointers deeper than the first climb take the walk where the climb and the
/// subtree take turns; it gives what the index gives.
#[test]
fn deep_pointers_are_found_as_through_the_index() {
    let word = format!("{}{}", "(".repeat(150), ")".repeat(150));
    for x in [1, 10, 70, 100, 149] {
        for p in [0, 5, 80, 100, 150] {
            for before in [false, true] {
                let mut results = Vec::new();
                for padding in [0, 20] {
                    let mut doc = Document::new();
                    let nodes = tree(&mut doc, &word);
                    for id in 0..padding {
                        doc.add_node_iterator(100 + id, nodes[0]);
                    }
                    doc.add_node_iterator(1, nodes[0]);
                    place(&mut doc, &nodes, 1, p, before);
                    doc.remove(nodes[x]);
                    results.push(at(&doc, &nodes, 1));
                }
                assert_eq!(results[0], results[1], "x{x} p{p} {before}");
            }
        }
    }
}

#[test]
fn iterators_move_with_their_root_to_another_arena() {
    let mut a = Document::new();
    let nodes = tree(&mut a, "(()())");
    a.add_node_iterator(1, nodes[1]);
    place(&mut a, &nodes, 1, 3, false);
    // The root `n1` and its children go to another arena (here: by id pairs).
    let mut b = Document::new();
    let copy = tree(&mut b, "(()())");
    let pairs = [
        (nodes[1], copy[1]),
        (nodes[2], copy[2]),
        (nodes[3], copy[3]),
    ];
    let moved = a.take_node_iterators(&pairs, 7);
    assert_eq!(moved.len(), 1);
    assert_eq!(a.node_iterator_count(), 0);
    b.put_node_iterators(moved, 8);
    let p = b.node_iterator(1).unwrap();
    assert_eq!(
        (p.root, p.reference, p.pointer_before),
        (copy[1], Spot::Here(copy[3]), false)
    );
    // The new arena runs the steps for it.
    b.remove(copy[3]);
    assert_eq!(b.node_iterator(1).unwrap().reference, Spot::Here(copy[2]));
    // A pointer outside the moved subtree stays where it is, now in another arena.
    let mut a = Document::new();
    let nodes = tree(&mut a, "(()())");
    a.add_node_iterator(1, nodes[1]);
    place(&mut a, &nodes, 1, 3, false);
    assert!(a.node_iterator_begin(1));
    let moved = a.take_node_iterators(&[(nodes[1], copy[1])], 7);
    assert_eq!(moved[0].1.reference, Spot::Away(7, nodes[3]));
    assert_eq!(moved[0].1.candidate, Some((Spot::Away(7, nodes[3]), false)));
    // An Away pointer at the tag of the receiving arena is a node there.
    b.put_node_iterators(
        vec![(
            2,
            IteratorPosition {
                root: copy[1],
                reference: Spot::Away(8, copy[2]),
                pointer_before: true,
                candidate: None,
            },
        )],
        8,
    );
    assert_eq!(b.node_iterator(2).unwrap().reference, Spot::Here(copy[2]));
}

#[test]
fn a_pointer_that_leaves_for_another_arena_is_not_moved_by_removals_and_comes_back() {
    let mut a = Document::new();
    let nodes = tree(&mut a, "(()())");
    a.add_node_iterator(1, nodes[0]);
    place(&mut a, &nodes, 1, 2, false);
    let mut b = Document::new();
    let copy = tree(&mut b, "(()())");
    // `n2` goes to the arena with the tag 5; the root stays.
    assert_eq!(a.send_node_pointers(&[(nodes[2], copy[2])], 5), [1]);
    let p = a.node_iterator(1).unwrap();
    assert_eq!(p.reference, Spot::Away(5, copy[2]));
    assert_eq!(a.node_iterator_aways(1), [Some((5, copy[2])), None]);
    // Removals in this arena do not change it.
    a.remove(nodes[1]);
    assert_eq!(
        a.node_iterator(1).unwrap().reference,
        Spot::Away(5, copy[2])
    );
    // It comes back as another node.
    a.receive_node_pointer(1, 5, copy[2], Spot::Here(nodes[3]));
    assert_eq!(a.node_iterator(1).unwrap().reference, Spot::Here(nodes[3]));
    assert_eq!(a.node_iterator_aways(1), [None, None]);
}

#[test]
fn a_candidate_in_another_arena_is_not_advanced_here() {
    let mut a = Document::new();
    let nodes = tree(&mut a, "(()())");
    a.add_node_iterator(1, nodes[0]);
    assert!(a.node_iterator_begin(1));
    a.node_iterator_set_candidate(1, Spot::Away(5, nodes[1]), false);
    assert_eq!(a.node_iterator_advance(1, true), None);
    assert_eq!(
        a.node_iterator(1).unwrap().candidate,
        Some((Spot::Away(5, nodes[1]), false))
    );
    // Without a root the step goes through the whole tree of the node.
    assert_eq!(
        a.iterator_step(None, nodes[2], false, true),
        Some((nodes[3], false))
    );
    assert_eq!(a.iterator_step(None, nodes[3], false, true), None);
    assert_eq!(
        a.iterator_step(None, nodes[1], true, false),
        Some((nodes[0], true))
    );
    assert_eq!(a.iterator_step(Some(nodes[1]), nodes[1], true, false), None);
}

#[test]
fn work_is_counted_and_asks_the_host_to_look() {
    let steps = Rc::new(Cell::new(0));
    let request = Rc::new(Cell::new(false));
    let mut doc = Document::new();
    let dead = Rc::new(DeadIterators::default());
    doc.set_iterator_work(IteratorWork::new(
        Rc::clone(&steps),
        Rc::clone(&request),
        dead,
    ));
    // A chain of nodes: a removal at the top walks the subtree or the ancestors.
    let root = element(&mut doc);
    let mut parent = root;
    let mut deepest = root;
    for _ in 0..5000 {
        let node = element(&mut doc);
        doc.append_child(parent, node).unwrap();
        parent = node;
        deepest = node;
    }
    doc.add_node_iterator(1, root);
    walk_to(&mut doc, 1, deepest);
    steps.set(0);
    request.set(false);
    let top = doc.node(root).first_child().unwrap();
    doc.remove(top);
    assert!(steps.get() >= WORK_PER_CHECK, "{}", steps.get());
    assert!(request.get());
    assert_eq!(doc.node_iterator(1).unwrap().reference, Spot::Here(root));
}

#[test]
fn removing_the_top_of_a_very_deep_chain_with_iterators_inside_does_not_recurse() {
    let mut doc = Document::new();
    let root = element(&mut doc);
    let mut parent = root;
    let mut chain = vec![root];
    for _ in 0..200_000 {
        let node = element(&mut doc);
        doc.append_child(parent, node).unwrap();
        parent = node;
        chain.push(node);
    }
    for id in 0..20 {
        doc.add_node_iterator(id, root);
    }
    walk_to(&mut doc, 3, chain[chain.len() - 1]);
    walk_to(&mut doc, 4, chain[chain.len() / 2]);
    doc.remove(chain[1]);
    assert_eq!(doc.node_iterator(3).unwrap().reference, Spot::Here(root));
    assert_eq!(doc.node_iterator(4).unwrap().reference, Spot::Here(root));
    assert_eq!(doc.node_iterator(5).unwrap().reference, Spot::Here(root));
}

#[test]
fn a_removal_far_from_the_pointers_does_not_walk_their_ancestors() {
    let steps = Rc::new(Cell::new(0));
    let mut doc = Document::new();
    doc.set_iterator_work(IteratorWork::new(
        Rc::clone(&steps),
        Rc::new(Cell::new(false)),
        Rc::new(DeadIterators::default()),
    ));
    let root = element(&mut doc);
    let mut parent = root;
    let mut deepest = root;
    for _ in 0..50_000 {
        let node = element(&mut doc);
        doc.append_child(parent, node).unwrap();
        parent = node;
        deepest = node;
    }
    doc.add_node_iterator(1, root);
    walk_to(&mut doc, 1, deepest);
    steps.set(0);
    let other = element(&mut doc);
    for _ in 0..100 {
        doc.append_child(root, other).unwrap();
        doc.remove(other);
    }
    // A bounded climb and the removed node: a few dozen units, not 50,000.
    assert!(steps.get() <= 100 * 80, "{}", steps.get());
    assert_eq!(doc.node_iterator(1).unwrap().reference, Spot::Here(deepest));
    // A pointer below the removed node is still found.
    let top = doc.node(root).first_child().unwrap();
    doc.remove(top);
    assert_eq!(doc.node_iterator(1).unwrap().reference, Spot::Here(root));
}

#[test]
fn the_entries_of_dead_iterators_go_at_the_next_removal() {
    let dead = Rc::new(DeadIterators::default());
    let mut doc = Document::new();
    doc.set_iterator_work(IteratorWork::new(
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(false)),
        Rc::clone(&dead),
    ));
    let nodes = tree(&mut doc, "()()()");
    for id in 1..=30 {
        doc.add_node_iterator(id, nodes[0]);
    }
    assert_eq!(doc.node_iterator_count(), 30);
    // The ids of other arenas on the list do no harm.
    for id in 1..=25 {
        dead.push(id);
    }
    dead.push(1000);
    doc.remove(nodes[3]);
    assert_eq!(doc.node_iterator_count(), 5);
    dead.push(26);
    doc.remove(nodes[2]);
    assert_eq!(doc.node_iterator_count(), 4);
    // The host clears the list; the arena starts at the new list.
    assert_eq!(dead.take().len(), 27);
    dead.push(27);
    doc.remove(nodes[1]);
    assert_eq!(doc.node_iterator_count(), 3);
}
