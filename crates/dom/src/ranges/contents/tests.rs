//! Tests of the contents operations. The values are the DOM standard's;
//! `crates/web/tests/range.rs` checks the same operations against Chromium's
//! values (`tools/probes/web-range.json`, `contents-*`).

use super::*;
use crate::{RangePiece, inner_html, parse_html};
use html5ever::{LocalName, QualName, ns};

fn html(local: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(local))
}

const TREE: &str = "<div id=r>ab<p id=p>cd<i id=i>ef</i>gh</p>ij<s id=s>kl</s></div>";

/// A document with `body` holding `html`.
fn page(html: &str) -> Document {
    parse_html(&format!("<!DOCTYPE html><body>{html}"))
}

fn by_id(doc: &Document, id: &str) -> NodeId {
    doc.element_by_id(id).expect("the element")
}

fn first(doc: &Document, node: NodeId) -> NodeId {
    doc.children(node).next().expect("a child")
}

fn at(node: NodeId, offset: u32) -> Boundary {
    Boundary::new(node, offset)
}

fn range(start: Boundary, end: Boundary) -> LiveRange {
    LiveRange { start, end }
}

/// The range from `cd` at 1 to `kl` at 1 in [`TREE`].
fn across(doc: &Document) -> LiveRange {
    let cd = first(doc, by_id(doc, "p"));
    let kl = first(doc, by_id(doc, "s"));
    range(at(cd, 1), at(kl, 1))
}

/// Adds a collapsed live range at each boundary point of the subtree of
/// `root` (ids from 100) and returns their ids.
fn every_point(doc: &mut Document, root: NodeId) -> Vec<u32> {
    let nodes: Vec<NodeId> = doc.descendants(root).collect();
    let mut ids = Vec::new();
    for node in nodes {
        for offset in 0..=doc.node_length(node) {
            let id = 100 + u32::try_from(ids.len()).expect("few");
            doc.set_live_range(id, LiveRange::collapsed_at(at(node, offset)));
            ids.push(id);
        }
    }
    ids
}

#[test]
fn clone_copies_partly_contained_nodes_at_both_ends() {
    let mut doc = page(TREE);
    let r = by_id(&doc, "r");
    let before = inner_html(&doc, r);
    let contents = doc.clone_contents(across(&doc)).unwrap();
    assert_eq!(
        inner_html(&doc, contents.fragment),
        "<p id=\"p\">d<i id=\"i\">ef</i>gh</p>ij<s id=\"s\">k</s>"
    );
    assert_eq!(inner_html(&doc, r), before, "the tree stays");
    assert_eq!(doc.parent(contents.fragment), None);
}

#[test]
fn extract_moves_the_contents_and_collapses_the_range() {
    let mut doc = page(TREE);
    let r = by_id(&doc, "r");
    let i = by_id(&doc, "i");
    doc.set_live_range(1, across(&doc));
    let contents = doc.extract_contents(1).unwrap();
    assert_eq!(
        inner_html(&doc, contents.fragment),
        "<p id=\"p\">d<i id=\"i\">ef</i>gh</p>ij<s id=\"s\">k</s>"
    );
    assert_eq!(inner_html(&doc, r), "ab<p id=\"p\">c</p><s id=\"s\">l</s>");
    // After the child of the common ancestor that holds the start.
    assert_eq!(doc.live_range(1), Some(LiveRange::collapsed_at(at(r, 2))));
    // The moved `i` is the original, the `p` and `s` in the fragment are copies.
    assert_eq!(doc.root(i), contents.fragment);
    assert_eq!(contents.copies.len(), 4, "p, its text, s, its text");
}

#[test]
fn delete_removes_the_contents_and_collapses_the_range() {
    let mut doc = page(TREE);
    let r = by_id(&doc, "r");
    doc.set_live_range(1, across(&doc));
    doc.delete_contents(1).unwrap();
    assert_eq!(inner_html(&doc, r), "ab<p id=\"p\">c</p><s id=\"s\">l</s>");
    assert_eq!(doc.live_range(1), Some(LiveRange::collapsed_at(at(r, 2))));
}

#[test]
fn a_range_in_one_text_node_takes_its_data() {
    let mut doc = page(TREE);
    let ab = first(&doc, by_id(&doc, "r"));
    let part = range(at(ab, 0), at(ab, 1));
    let copy = doc.clone_contents(part).unwrap();
    assert_eq!(inner_html(&doc, copy.fragment), "a");
    doc.set_live_range(1, part);
    let moved = doc.extract_contents(1).unwrap();
    assert_eq!(inner_html(&doc, moved.fragment), "a");
    assert_eq!(doc.char_data(ab), Some("b"));
    // The standard does not set the range: the data change collapsed it.
    assert_eq!(doc.live_range(1), Some(LiveRange::collapsed_at(at(ab, 0))));
    doc.set_live_range(2, range(at(ab, 0), at(ab, 1)));
    doc.delete_contents(2).unwrap();
    assert_eq!(doc.char_data(ab), Some(""));
}

#[test]
fn comments_and_processing_instructions_are_copied_in_part() {
    let mut doc = page("<div id=r>a<!--comment-->b</div>");
    let r = by_id(&doc, "r");
    let pi = doc
        .create_node(NodeData::ProcessingInstruction {
            target: "t".to_owned(),
            data: "data".to_owned(),
        })
        .unwrap();
    doc.append_child(r, pi).unwrap();
    let comment = doc.children(r).nth(1).unwrap();
    let part = range(at(comment, 2), at(pi, 2));
    let copy = doc.clone_contents(part).unwrap();
    assert_eq!(inner_html(&doc, copy.fragment), "<!--mment-->b<?t da>");
    doc.set_live_range(1, part);
    doc.delete_contents(1).unwrap();
    assert_eq!(inner_html(&doc, r), "a<!--co--><?t ta>");
    assert_eq!(doc.live_range(1), Some(LiveRange::collapsed_at(at(r, 2))));
    // A range inside one comment.
    let inside = range(at(comment, 0), at(comment, 1));
    let copy = doc.clone_contents(inside).unwrap();
    assert_eq!(inner_html(&doc, copy.fragment), "<!--c-->");
}

#[test]
fn a_doctype_in_the_range_stops_clone_and_extract_but_not_delete() {
    let mut doc = page("x");
    let html_element = doc.document_element().unwrap();
    let whole = range(at(NodeId::DOCUMENT, 0), at(html_element, 1));
    assert!(doc.range_contains_doctype(whole));
    assert_eq!(
        doc.clone_contents(whole).unwrap_err(),
        DomError::HierarchyRequest
    );
    doc.set_live_range(1, whole);
    assert_eq!(
        doc.extract_contents(1).unwrap_err(),
        DomError::HierarchyRequest
    );
    assert_eq!(doc.live_range(1), Some(whole), "nothing changed");
    doc.delete_contents(1).unwrap();
    assert_eq!(doc.child_count(NodeId::DOCUMENT), 1, "the doctype went");
    assert_eq!(
        doc.live_range(1),
        Some(LiveRange::collapsed_at(at(NodeId::DOCUMENT, 0)))
    );
    let inside = range(at(html_element, 0), at(html_element, 1));
    assert!(!doc.range_contains_doctype(inside));
}

/// The live range steps of the batched runs equal those of single removals:
/// the same deletion done with `remove` and `replace_data`, node by node in
/// Chromium's order, moves every boundary point to the same place.
#[test]
fn delete_moves_live_ranges_as_single_removals_do() {
    let markup = "<div id=r>ab<p id=p>cd<i id=i>ef<b>x</b></i>gh</p>ij<s id=s>kl<u id=u><b>y</b>mn</u>op</s></div>";
    let mut batched = page(markup);
    let mut single = page(markup);
    let r = by_id(&batched, "r");
    let ids = every_point(&mut batched, r);
    let same_ids = every_point(&mut single, r);
    assert_eq!(ids, same_ids);
    let ef = first(&batched, by_id(&batched, "i"));
    let u = by_id(&batched, "u");
    let mn = batched.children(u).nth(1).unwrap();
    batched.set_live_range(1, range(at(ef, 1), at(mn, 1)));
    batched.delete_contents(1).unwrap();
    // By hand: the start's side, the end's side, the middle.
    single.replace_data(ef, 1, 1, "").unwrap();
    let b = single.children(by_id(&single, "i")).nth(1).unwrap();
    single.remove(b);
    let gh = single.children(by_id(&single, "p")).nth(2).unwrap();
    single.remove(gh);
    single.replace_data(mn, 0, 1, "").unwrap();
    let y = first(&single, u);
    single.remove(y);
    let kl = first(&single, by_id(&single, "s"));
    single.remove(kl);
    let ij = single.children(r).nth(2).unwrap();
    single.remove(ij);
    assert_eq!(inner_html(&batched, r), inner_html(&single, r));
    for id in ids {
        assert_eq!(batched.live_range(id), single.live_range(id), "range {id}");
    }
}

#[test]
fn extract_moves_live_ranges_as_single_removals_do() {
    let mut batched = page(TREE);
    let mut single = page(TREE);
    let r = by_id(&batched, "r");
    let ids = every_point(&mut batched, r);
    every_point(&mut single, r);
    let all = range(at(r, 1), at(r, 4));
    batched.set_live_range(1, all);
    let contents = batched.extract_contents(1).unwrap();
    assert_eq!(
        inner_html(&batched, contents.fragment),
        "<p id=\"p\">cd<i id=\"i\">ef</i>gh</p>ij<s id=\"s\">kl</s>"
    );
    for _ in 0..3 {
        let child = single.children(r).nth(1).unwrap();
        single.remove(child);
    }
    for id in ids {
        assert_eq!(batched.live_range(id), single.live_range(id), "range {id}");
    }
    assert_eq!(
        batched.live_range(1),
        Some(LiveRange::collapsed_at(at(r, 1)))
    );
}

#[test]
fn insert_node_splits_text_and_grows_a_collapsed_range() {
    let mut doc = page(TREE);
    let para = by_id(&doc, "p");
    let cd = first(&doc, para);
    doc.set_live_range(1, LiveRange::collapsed_at(at(cd, 1)));
    let text_n = doc.create_text("N").unwrap();
    assert_eq!(doc.insert_node(1, text_n).unwrap(), para);
    assert_eq!(inner_html(&doc, para), "cNd<i id=\"i\">ef</i>gh");
    assert_eq!(doc.live_range(1), Some(range(at(cd, 1), at(para, 2))));
    // A fragment at an element offset: the range grows over its children.
    let fragment = doc.create_node(NodeData::DocumentFragment).unwrap();
    for t in ["x", "y"] {
        let text = doc.create_text(t).unwrap();
        doc.append_child(fragment, text).unwrap();
    }
    doc.set_live_range(2, LiveRange::collapsed_at(at(para, 0)));
    doc.insert_node(2, fragment).unwrap();
    assert_eq!(doc.live_range(2), Some(range(at(para, 0), at(para, 2))));
    // A node that is the child at the offset moves after itself: no change.
    let italic = by_id(&doc, "i");
    let index = doc.index_in_parent(italic);
    doc.set_live_range(3, LiveRange::collapsed_at(at(para, index)));
    doc.insert_node(3, italic).unwrap();
    assert_eq!(doc.index_in_parent(italic), index);
    assert_eq!(
        doc.live_range(3),
        Some(range(at(para, index), at(para, index + 1)))
    );
    // A range that is not collapsed keeps its end.
    let strike = by_id(&doc, "s");
    doc.set_live_range(4, range(at(strike, 0), at(strike, 1)));
    let underline = doc.create_element(html("u"), vec![]).unwrap();
    doc.insert_node(4, underline).unwrap();
    assert_eq!(doc.live_range(4), Some(range(at(strike, 0), at(strike, 2))));
}

#[test]
fn insert_node_refuses_comments_detached_text_and_the_start_node() {
    let mut doc = page("<div id=r>a<!--c--></div>");
    let r = by_id(&doc, "r");
    let comment = doc.children(r).nth(1).unwrap();
    let n = doc.create_text("N").unwrap();
    doc.set_live_range(1, LiveRange::collapsed_at(at(comment, 1)));
    assert_eq!(doc.insert_node(1, n), Err(DomError::HierarchyRequest));
    let lone = doc.create_text("lone").unwrap();
    doc.set_live_range(2, LiveRange::collapsed_at(at(lone, 1)));
    assert_eq!(doc.insert_node(2, n), Err(DomError::HierarchyRequest));
    assert_eq!(doc.char_data(lone), Some("lone"), "no split");
    let a = first(&doc, r);
    doc.set_live_range(3, LiveRange::collapsed_at(at(a, 0)));
    assert_eq!(doc.insert_node(3, a), Err(DomError::HierarchyRequest));
    // The node contains the parent.
    assert_eq!(doc.insert_node(3, r), Err(DomError::HierarchyRequest));
    assert_eq!(
        doc.char_data(a),
        Some("a"),
        "no split before a failed check"
    );
}

#[test]
fn surround_checks_partly_selected_nodes() {
    let doc = page(TREE);
    let r = by_id(&doc, "r");
    let ab = first(&doc, r);
    let ij = doc.children(r).nth(2).unwrap();
    assert!(!doc.range_partly_selects_non_text(range(at(ab, 1), at(ij, 1))));
    assert!(doc.range_partly_selects_non_text(across(&doc)));
    assert!(!doc.range_partly_selects_non_text(range(at(r, 0), at(r, 2))));
    let p = by_id(&doc, "p");
    assert!(doc.range_contains_node(range(at(ab, 1), at(ij, 1)), p));
    assert!(!doc.range_contains_node(across(&doc), p));
}

/// A range at the bottom of a chain of 100,000 elements, and one over a
/// subtree of 100,000 nodes: no recursion, and each call stays linear.
#[test]
fn deep_and_wide_ranges() {
    let mut doc = page("<div id=r></div>");
    let r = by_id(&doc, "r");
    let mut parent = r;
    for _ in 0..100_000 {
        let child = doc.create_element(html("b"), vec![]).unwrap();
        doc.append_child(parent, child).unwrap();
        parent = child;
    }
    let text = doc.create_text("deep").unwrap();
    doc.append_child(parent, text).unwrap();
    let tail = doc.create_text("tail").unwrap();
    doc.append_child(r, tail).unwrap();
    let deep = range(at(text, 1), at(tail, 2));
    let copy = doc.clone_contents(deep).unwrap();
    assert_eq!(doc.descendants(copy.fragment).count(), 100_003);
    doc.set_live_range(1, deep);
    let moved = doc.extract_contents(1).unwrap();
    assert_eq!(doc.char_data(text), Some("d"));
    assert_eq!(doc.char_data(tail), Some("il"));
    assert_eq!(doc.descendants(moved.fragment).count(), 100_003);
    assert_eq!(doc.live_range(1), Some(LiveRange::collapsed_at(at(r, 1))));
    // 100,000 children with ranges in them.
    let wide = doc.create_element(html("div"), vec![]).unwrap();
    doc.append_child(r, wide).unwrap();
    for i in 0..100_000u32 {
        let t = doc.create_text("w").unwrap();
        doc.append_child(wide, t).unwrap();
        if i % 10 == 0 {
            doc.set_live_range(10 + i, LiveRange::collapsed_at(at(t, 1)));
        }
    }
    doc.set_live_range(2, range(at(wide, 0), at(wide, 100_000)));
    doc.delete_contents(2).unwrap();
    assert_eq!(doc.child_count(wide), 0);
    assert_eq!(
        doc.live_range(10 + 99_990),
        Some(LiveRange::collapsed_at(at(wide, 0)))
    );
}

#[test]
fn the_selection_reports_range_after_a_contents_operation_moved_it() {
    let mut doc = page(TREE);
    let r = by_id(&doc, "r");
    let ab = first(&doc, r);
    let ij = doc.children(r).nth(2).unwrap();
    doc.set_live_range(1, range(at(ab, 1), at(ij, 1)));
    assert!(doc.set_selection_entry(1));
    // A backward selection through the arena's own entry.
    doc.set_selection(Some(range(at(ab, 1), at(ij, 1))), true, true);
    let id = doc.selection().unwrap().id;
    assert!(doc.selection().unwrap().backward);
    doc.delete_contents(id).unwrap();
    let s = doc.selection().unwrap();
    assert!(s.range.is_collapsed());
    assert!(s.stale_type && !s.backward);
    // An unrelated data change keeps it; a change of the selection ends it.
    let kl = first(&doc, by_id(&doc, "s"));
    doc.replace_data(kl, 0, 1, "K").unwrap();
    assert!(doc.selection().unwrap().stale_type);
    doc.set_live_range(id, range(at(r, 0), at(r, 1)));
    assert!(!doc.selection().unwrap().stale_type);
    // A script's change of a backward selection's range makes it forward.
    doc.set_selection(Some(range(at(r, 0), at(r, 2))), true, true);
    let id = doc.selection().unwrap().id;
    doc.set_live_range(id, range(at(r, 0), at(r, 1)));
    assert!(!doc.selection().unwrap().backward);
}

#[test]
fn geometry_pieces_are_top_elements_and_selected_text() {
    let doc = page(TREE);
    let r = by_id(&doc, "r");
    let (p, i, s) = (by_id(&doc, "p"), by_id(&doc, "i"), by_id(&doc, "s"));
    let ab = first(&doc, r);
    let cd = first(&doc, p);
    let ef = first(&doc, i);
    let gh = doc.children(p).nth(2).unwrap();
    let ij = doc.children(r).nth(2).unwrap();
    let kl = first(&doc, s);
    let text = |node, start, end| RangePiece::Text { node, start, end };
    // The children of `r`: `p` and `s` count with their boxes, their text
    // nodes with their parts; `i` does not (its parent counts).
    assert_eq!(
        doc.range_pieces(range(at(r, 0), at(r, 4))),
        vec![
            text(ab, 0, 2),
            RangePiece::Element(p),
            text(cd, 0, 2),
            text(ef, 0, 2),
            text(gh, 0, 2),
            text(ij, 0, 2),
            RangePiece::Element(s),
            text(kl, 0, 2),
        ]
    );
    // Partly contained elements do not count; their contained children do.
    assert_eq!(
        doc.range_pieces(across(&doc)),
        vec![
            text(cd, 1, 2),
            RangePiece::Element(i),
            text(ef, 0, 2),
            text(gh, 0, 2),
            text(ij, 0, 2),
            text(kl, 0, 1),
        ]
    );
    // A collapsed range in a text node has an empty part; at an element
    // offset it has none.
    assert_eq!(
        doc.range_pieces(LiveRange::collapsed_at(at(ab, 1))),
        vec![text(ab, 1, 1)]
    );
    assert_eq!(
        doc.range_pieces(LiveRange::collapsed_at(at(r, 1))),
        Vec::new()
    );
}
