//! Tests of the live range steps and the boundary point algorithms. The
//! values are the DOM standard's; `crates/web/tests/range.rs` checks the same
//! steps against Chromium's values (`tools/probes/web-range.json`, `live-*`).

use std::cell::Cell;
use std::rc::Rc;

use super::*;
use crate::{ArenaWork, DeadIds, parse_html};
use html5ever::{LocalName, QualName, ns};

fn html(local: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(local))
}

/// A document with `body` holding `html`, and the body.
fn page(html: &str) -> (Document, NodeId) {
    let doc = parse_html(&format!("<!DOCTYPE html><body>{html}"));
    let body = doc.body().expect("a body");
    (doc, body)
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

/// Adds a range per boundary point of the subtree of `root` (collapsed), and
/// returns the ids with their points.
fn every_point(doc: &mut Document, root: NodeId) -> Vec<(u32, Boundary)> {
    let nodes: Vec<NodeId> = doc.descendants(root).collect();
    let mut out = Vec::new();
    for node in nodes {
        for offset in 0..=doc.node_length(node) {
            let id = u32::try_from(out.len()).expect("few") + 1;
            let point = at(node, offset);
            doc.set_live_range(id, LiveRange::collapsed_at(point));
            out.push((id, point));
        }
    }
    out
}

#[test]
fn an_arena_without_ranges_installs_nothing() {
    let (mut doc, body) = page("<p id=p>ab</p>");
    let p = by_id(&doc, "p");
    doc.remove(p);
    doc.append_child(body, p).unwrap();
    let text = first(&doc, p);
    doc.replace_data(text, 0, 1, "x").unwrap();
    doc.split_text(text, 1).unwrap();
    doc.normalize(body).unwrap();
    assert!(doc.ranges.is_none());
}

#[test]
fn insertions_shift_offsets_after_the_child() {
    let (mut doc, body) = page("<p id=p>a<b>b</b>c</p>");
    let p = by_id(&doc, "p");
    let b = doc.children(p).nth(1).unwrap();
    for (id, offset) in [(1, 0), (2, 1), (3, 2), (4, 3)] {
        doc.set_live_range(id, LiveRange::collapsed_at(at(p, offset)));
    }
    let x = doc.create_text("x").unwrap();
    doc.insert_before(p, x, Some(b)).unwrap();
    let offsets: Vec<u32> = (1..=4)
        .map(|id| doc.live_range(id).unwrap().start.offset)
        .collect();
    assert_eq!(offsets, [0, 1, 3, 4]);
    // An append changes nothing.
    let y = doc.create_text("y").unwrap();
    doc.append_child(p, y).unwrap();
    assert_eq!(doc.live_range(4).unwrap().start.offset, 4);
    let _ = body;
}

#[test]
fn a_fragment_inserts_its_children_one_by_one_with_the_same_result() {
    let (mut doc, _) = page("<p id=p>a<b>b</b></p>");
    let p = by_id(&doc, "p");
    let b = doc.children(p).nth(1).unwrap();
    doc.set_live_range(1, range(at(p, 1), at(p, 2)));
    let fragment = doc.create_node(NodeData::DocumentFragment).unwrap();
    for t in ["x", "y", "z"] {
        let n = doc.create_text(t).unwrap();
        doc.append_child(fragment, n).unwrap();
    }
    doc.set_live_range(2, range(at(fragment, 1), at(fragment, 3)));
    doc.insert_before(p, fragment, Some(b)).unwrap();
    assert_eq!(doc.live_range(1), Some(range(at(p, 1), at(p, 5))));
    assert_eq!(
        doc.live_range(2),
        Some(range(at(fragment, 0), at(fragment, 0)))
    );
}

#[test]
fn removals_move_points_out_of_the_subtree() {
    let (mut doc, body) = page("<div id=r>a<p id=p>bc<i>d</i>e</p><s>f</s></div>");
    let r = by_id(&doc, "r");
    let p = by_id(&doc, "p");
    let points = every_point(&mut doc, r);
    doc.remove(p);
    for (id, before) in points {
        let after = doc.live_range(id).unwrap().start;
        let inside = before.node == p || doc.ancestors(before.node).any(|a| a == p);
        let expected = if inside {
            at(r, 1)
        } else if before.node == r && before.offset > 1 {
            at(r, before.offset - 1)
        } else {
            before
        };
        assert_eq!(after, expected, "{before:?}");
    }
    let _ = body;
}

#[test]
fn many_ranges_and_few_ranges_find_the_same_points() {
    for count in [3u32, 40] {
        let (mut doc, _) = page("<div id=r><p id=p><i id=i>x</i></p>y</div>");
        let (r, i) = (by_id(&doc, "r"), by_id(&doc, "i"));
        let x = first(&doc, i);
        for id in 1..=count {
            // Even ids in the removed subtree, odd ids after it in the parent.
            let point = if id % 2 == 0 { at(x, 1) } else { at(r, 2) };
            doc.set_live_range(id, LiveRange::collapsed_at(point));
        }
        doc.remove(by_id(&doc, "p"));
        for id in 1..=count {
            let expected = if id % 2 == 0 { at(r, 0) } else { at(r, 1) };
            assert_eq!(doc.live_range(id).unwrap().start, expected, "{count} {id}");
        }
    }
}

#[test]
fn replace_data_moves_points_in_and_after_the_replaced_part() {
    let (mut doc, _) = page("<p id=p>abcdef</p>");
    let t = first(&doc, by_id(&doc, "p"));
    for offset in 0..=6 {
        doc.set_live_range(offset + 1, LiveRange::collapsed_at(at(t, offset)));
    }
    // Bytes 1..3 ("bc") become "XYZW".
    doc.replace_data(t, 1, 2, "XYZW").unwrap();
    let offsets: Vec<u32> = (1..=7)
        .map(|id| doc.live_range(id).unwrap().start.offset)
        .collect();
    assert_eq!(offsets, [0, 1, 1, 1, 6, 7, 8]);
    // An append moves nothing.
    doc.append_data(t, "!").unwrap();
    assert_eq!(doc.live_range(7).unwrap().start.offset, 8);
}

#[test]
fn split_text_moves_points_to_the_new_node() {
    let (mut doc, _) = page("<p id=p>abc<b>x</b></p>");
    let p = by_id(&doc, "p");
    let t = first(&doc, p);
    let ranges = [
        at(t, 0),
        at(t, 1),
        at(t, 2),
        at(t, 3),
        at(p, 0),
        at(p, 1),
        at(p, 2),
    ];
    for (i, b) in ranges.iter().enumerate() {
        doc.set_live_range(u32::try_from(i).unwrap() + 1, LiveRange::collapsed_at(*b));
    }
    let new = doc.split_text(t, 1).unwrap();
    let got: Vec<Boundary> = (1..=7)
        .map(|id| doc.live_range(id).unwrap().start)
        .collect();
    assert_eq!(
        got,
        [
            at(t, 0),
            at(t, 1),
            at(new, 1),
            at(new, 2),
            at(p, 0),
            at(p, 2),
            at(p, 3)
        ]
    );
    // Without a parent, the truncation clamps.
    let lone = doc.create_text("abcd").unwrap();
    doc.set_live_range(9, LiveRange::collapsed_at(at(lone, 3)));
    doc.split_text(lone, 1).unwrap();
    assert_eq!(doc.live_range(9).unwrap().start, at(lone, 1));
}

#[test]
fn normalize_moves_points_into_the_merged_node() {
    let (mut doc, _) = page("<div id=r></div>");
    let r = by_id(&doc, "r");
    let mut texts = Vec::new();
    for data in ["ab", "", "cd", "ef"] {
        let t = doc.create_text(data).unwrap();
        doc.append_child(r, t).unwrap();
        texts.push(t);
    }
    let points = [
        at(r, 1),
        at(r, 2),
        at(r, 3),
        at(r, 4),
        at(texts[2], 1),
        at(texts[3], 2),
    ];
    for (i, b) in points.iter().enumerate() {
        doc.set_live_range(u32::try_from(i).unwrap() + 1, LiveRange::collapsed_at(*b));
    }
    doc.normalize(r).unwrap();
    let a = texts[0];
    let got: Vec<Boundary> = (1..=6)
        .map(|id| doc.live_range(id).unwrap().start)
        .collect();
    assert_eq!(
        got,
        [at(a, 2), at(a, 2), at(a, 4), at(r, 1), at(a, 3), at(a, 6)]
    );
    assert_eq!(doc.char_data(a), Some("abcdef"));
}

#[test]
fn normalize_stopped_by_the_text_limit_leaves_the_unmerged_points() {
    let build = |doc: &mut Document| {
        let r = doc.create_element(html("r"), vec![]).unwrap();
        let texts: Vec<NodeId> = ["ab", "cd", "ef"]
            .iter()
            .map(|d| {
                let t = doc.create_text(d).unwrap();
                doc.append_child(r, t).unwrap();
                t
            })
            .collect();
        (r, texts)
    };
    let mut probe = Document::new();
    build(&mut probe);
    // Room for the data of "cd" to join "ab", not for the data of "ef".
    let mut doc = Document::with_limits(crate::Limits {
        max_nodes: 20,
        max_text_bytes: probe.text_bytes() + 2,
    });
    let (r, texts) = build(&mut doc);
    let points = [
        at(r, 1),
        at(r, 2),
        at(r, 3),
        at(texts[1], 1),
        at(texts[2], 1),
    ];
    for (i, b) in points.iter().enumerate() {
        doc.set_live_range(u32::try_from(i).unwrap() + 1, LiveRange::collapsed_at(*b));
    }
    // "cd" merges (9 bytes); "ef" does not fit.
    assert!(doc.normalize(r).is_err());
    let a = texts[0];
    assert_eq!(doc.char_data(a), Some("abcd"));
    let got: Vec<Boundary> = (1..=5)
        .map(|id| doc.live_range(id).unwrap().start)
        .collect();
    assert_eq!(
        got,
        [at(a, 2), at(r, 1), at(r, 2), at(a, 3), at(texts[2], 1)]
    );
}

#[test]
fn normalize_merges_several_runs_with_one_pass_each() {
    let (mut doc, _) = page("<div id=r></div>");
    let r = by_id(&doc, "r");
    let mut kids = Vec::new();
    for data in ["ab", "cd", "ef", "<i>", "gh", "ij", "<i>", "kl"] {
        let n = if data == "<i>" {
            doc.create_element(html("i"), vec![]).unwrap()
        } else {
            doc.create_text(data).unwrap()
        };
        doc.append_child(r, n).unwrap();
        kids.push(n);
    }
    // Every offset at `r`, a point in each text node.
    for (i, offset) in (0..=8).enumerate() {
        doc.set_live_range(i as u32 + 1, LiveRange::collapsed_at(at(r, offset)));
    }
    doc.set_live_range(20, LiveRange::collapsed_at(at(kids[2], 1)));
    doc.set_live_range(21, LiveRange::collapsed_at(at(kids[5], 2)));
    doc.normalize(r).unwrap();
    let (a, g) = (kids[0], kids[4]);
    let got: Vec<Boundary> = (1..=9)
        .map(|id| doc.live_range(id).unwrap().start)
        .collect();
    assert_eq!(
        got,
        [
            at(r, 0),
            at(a, 2),
            at(a, 4),
            at(r, 1),
            at(r, 2),
            at(g, 2),
            at(r, 3),
            at(r, 4),
            at(r, 5),
        ]
    );
    assert_eq!(doc.live_range(20).unwrap().start, at(a, 5));
    assert_eq!(doc.live_range(21).unwrap().start, at(g, 4));
}

#[test]
fn normalize_with_empty_texts_and_a_nested_parent_moves_points_as_node_by_node() {
    // r: "" "ab" "" "cd" <i>"" "x" "y"</i> "" "" <b> "ef" ""
    let (mut doc, _) = page("<div id=r></div>");
    let r = by_id(&doc, "r");
    let mut kids = Vec::new();
    for data in ["", "ab", "", "cd", "<i>", "", "", "<b>", "ef", ""] {
        let n = match data {
            "<i>" | "<b>" => doc.create_element(html(&data[1..2]), vec![]).unwrap(),
            _ => doc.create_text(data).unwrap(),
        };
        doc.append_child(r, n).unwrap();
        kids.push(n);
    }
    let em = kids[4];
    let mut inner = Vec::new();
    for data in ["", "x", "y"] {
        let t = doc.create_text(data).unwrap();
        doc.append_child(em, t).unwrap();
        inner.push(t);
    }
    // Every offset at `r` (ids 1 to 11) and at `i` (ids 12 to 15), and points
    // in removed nodes (ids 16 to 22).
    let mut points: Vec<Boundary> = (0..=10).map(|o| at(r, o)).collect();
    points.extend((0..=3).map(|o| at(em, o)));
    points.extend([
        at(kids[0], 0),
        at(kids[2], 0),
        at(kids[3], 1),
        at(kids[5], 0),
        at(kids[9], 0),
        at(inner[0], 0),
        at(inner[2], 1),
    ]);
    for (n, b) in points.iter().enumerate() {
        doc.set_live_range(u32::try_from(n).unwrap() + 1, LiveRange::collapsed_at(*b));
    }
    doc.normalize(r).unwrap();
    let (ab, ef, xy) = (kids[1], kids[8], inner[1]);
    assert_eq!(doc.children(r).collect::<Vec<_>>(), [ab, em, kids[7], ef]);
    let got: Vec<Boundary> = (1..=22)
        .map(|id| doc.live_range(id).unwrap().start)
        .collect();
    // The DOM standard's steps, node by node in tree order.
    let want = [
        at(r, 0),
        at(r, 0),
        at(ab, 2),
        at(ab, 2),
        at(r, 1),
        at(r, 2),
        at(r, 2),
        at(r, 2),
        at(r, 3),
        at(ef, 2),
        at(r, 4),
        at(em, 0),
        at(em, 0),
        at(xy, 1),
        at(em, 1),
        at(r, 0),
        at(ab, 2),
        at(ab, 3),
        at(r, 2),
        at(ef, 2),
        at(em, 0),
        at(xy, 2),
    ];
    assert_eq!(got, want);
}

#[test]
fn replacing_all_children_moves_every_point_to_the_parent_start() {
    let (mut doc, _) = page("<div id=r><i>a</i>b<i><b>c</b></i></div>");
    let r = by_id(&doc, "r");
    let kids: Vec<NodeId> = doc.children(r).collect();
    let deep = doc.children(kids[2]).next().unwrap();
    let points = [at(r, 0), at(r, 2), at(r, 3), at(kids[1], 1), at(deep, 1)];
    for (i, b) in points.iter().enumerate() {
        doc.set_live_range(i as u32 + 1, LiveRange::collapsed_at(*b));
    }
    doc.replace_all(r, None).unwrap();
    for id in 1..=5 {
        assert_eq!(
            doc.live_range(id).unwrap(),
            LiveRange::collapsed_at(at(r, 0))
        );
    }
    // A fragment with boundary points at it.
    let frag = doc.create_node(NodeData::DocumentFragment).unwrap();
    for _ in 0..3 {
        let n = doc.create_text("x").unwrap();
        doc.append_child(frag, n).unwrap();
    }
    doc.set_live_range(6, range(at(frag, 1), at(frag, 3)));
    doc.append_child(r, frag).unwrap();
    assert_eq!(doc.live_range(6).unwrap(), range(at(frag, 0), at(frag, 0)));
}

#[test]
fn ranges_go_with_a_root_to_another_arena() {
    let (mut doc, _) = page("<p id=p>x</p>");
    let p = by_id(&doc, "p");
    let t = first(&doc, p);
    doc.set_live_range(1, range(at(t, 0), at(p, 1)));
    doc.set_live_range(2, LiveRange::collapsed_at(at(doc.body().unwrap(), 0)));
    let pairs = [
        (p, NodeId::from_index(70).unwrap()),
        (t, NodeId::from_index(71).unwrap()),
    ];
    let moved = doc.take_live_ranges(&pairs);
    assert_eq!(moved.len(), 1);
    assert_eq!(doc.live_range(1), None);
    assert!(doc.live_range(2).is_some());
    let (mut other, _) = page("");
    other.put_live_ranges(moved);
    assert_eq!(
        other.live_range(1),
        Some(range(at(pairs[1].1, 0), at(pairs[0].1, 1)))
    );
}

#[test]
fn dead_ranges_go_and_work_is_counted() {
    let (mut doc, _) = page("<p id=p>x</p>");
    let p = by_id(&doc, "p");
    let steps = Rc::new(Cell::new(0));
    let request = Rc::new(Cell::new(false));
    let dead = Rc::new(DeadIds::default());
    doc.set_range_work(ArenaWork::new(Rc::clone(&steps), request, Rc::clone(&dead)));
    doc.set_live_range(1, LiveRange::collapsed_at(at(p, 1)));
    doc.set_live_range(2, LiveRange::collapsed_at(at(p, 0)));
    dead.push(1);
    assert_eq!(doc.live_range_count(), 1);
    let before = steps.get();
    doc.remove(first(&doc, p));
    assert!(steps.get() > before);
}

#[test]
fn boundary_order_and_common_ancestor() {
    let (doc, body) = page("<p id=p>ab<i id=i>c</i>d</p>");
    let (p, i) = (by_id(&doc, "p"), by_id(&doc, "i"));
    let c = first(&doc, i);
    assert_eq!(doc.compare_boundaries(at(p, 1), at(c, 0)), Ordering::Less);
    assert_eq!(
        doc.compare_boundaries(at(p, 2), at(c, 1)),
        Ordering::Greater
    );
    assert_eq!(doc.compare_boundaries(at(c, 1), at(p, 2)), Ordering::Less);
    assert_eq!(
        doc.compare_boundaries(at(body, 0), at(c, 0)),
        Ordering::Less
    );
    let ab = first(&doc, p);
    assert_eq!(doc.compare_boundaries(at(ab, 2), at(c, 0)), Ordering::Less);
    assert_eq!(doc.range_common_ancestor(range(at(ab, 1), at(c, 1))), p);
    assert_eq!(doc.range_common_ancestor(range(at(c, 0), at(c, 1))), c);
    assert!(doc.range_intersects(range(at(ab, 1), at(p, 2)), i));
    assert!(!doc.range_intersects(range(at(ab, 1), at(p, 1)), i));
    assert_eq!(doc.range_text(range(at(ab, 1), at(c, 1))), "bc");
    assert_eq!(doc.range_text(range(at(p, 0), at(p, 3))), "abcd");
    assert_eq!(doc.range_text(range(at(body, 0), at(i, 0))), "ab");
}

#[test]
fn removal_in_a_deep_chain_far_from_the_ranges_is_cheap() {
    let (mut doc, body) = page("");
    let mut parent = body;
    for _ in 0..200_000 {
        let name = html("b");
        let e = doc.create_element(name, vec![]).unwrap();
        doc.append_child(parent, e).unwrap();
        parent = e;
    }
    let steps = Rc::new(Cell::new(0));
    doc.set_range_work(ArenaWork::new(
        Rc::clone(&steps),
        Rc::new(Cell::new(false)),
        Rc::new(DeadIds::default()),
    ));
    doc.set_live_range(1, LiveRange::collapsed_at(at(parent, 0)));
    // A leaf beside the chain, added and removed many times.
    for _ in 0..1000 {
        let leaf = doc.create_text("x").unwrap();
        doc.append_child(body, leaf).unwrap();
        doc.remove(leaf);
    }
    assert!(steps.get() < 1_000_000, "{}", steps.get());
    // Removing the top of the chain moves the range.
    let top = first(&doc, body);
    doc.remove(top);
    assert_eq!(doc.live_range(1).unwrap().start, at(body, 0));
}
