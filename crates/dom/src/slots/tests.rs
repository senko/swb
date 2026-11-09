// The variables have the names of the elements' ids in the probe cases.
#![allow(clippy::many_single_char_names)]

use super::*;
use crate::shadow::ShadowRootInit;
use crate::shadow::tests::{element, open};
use crate::tree::{ShadowRootMode, SlotAssignment};

fn id(doc: &Document, name: &str) -> NodeId {
    doc.element_by_id(name).expect("element")
}

fn slot(doc: &mut Document, name: Option<&str>) -> NodeId {
    let s = element(doc, "slot");
    if let Some(name) = name {
        doc.set_attr(s, "name", name).expect("name");
    }
    s
}

/// The host of the probe case `slot-assignment-named` with a root that has
/// the slots `d` (default), `s1` and `e` (`nope2`).
fn named_host() -> (Document, NodeId, NodeId, [NodeId; 3]) {
    let mut doc = crate::parse_html(
        "<div id=h><span id=a slot=s1>A</span><span id=b>B</span> t <span id=c slot=nope>C</span><!--c--></div>",
    );
    let h = id(&doc, "h");
    let root = doc.attach_shadow(h, open(), false).expect("root");
    let d = slot(&mut doc, None);
    let s1 = slot(&mut doc, Some("s1"));
    let e = slot(&mut doc, Some("nope2"));
    for s in [d, s1, e] {
        doc.append_child(root, s).expect("append");
    }
    (doc, h, root, [d, s1, e])
}

#[test]
fn named_slots_take_the_matching_children() {
    let (mut doc, h, _root, [d, s1, e]) = named_host();
    let (a, b, c) = (id(&doc, "a"), id(&doc, "b"), id(&doc, "c"));
    let text = doc.children(h).nth(2).expect("text");
    assert_eq!(doc.assigned_nodes(d), &[b, text]);
    assert_eq!(doc.assigned_nodes(s1), &[a]);
    assert_eq!(doc.assigned_nodes(e), []);
    assert_eq!(doc.assigned_slot(a), Some(s1));
    assert_eq!(doc.assigned_slot(c), None);
    assert_eq!(doc.find_slot(text, true), Some(d));
    assert!(doc.element(s1).is_some_and(ElementData::has_assigned_nodes));
    assert!(!doc.element(e).is_some_and(ElementData::has_assigned_nodes));
    // Changing `slot` reassigns at once.
    let _ = doc.take_slot_signals();
    doc.set_attr(c, "slot", "s1").expect("slot");
    assert_eq!(doc.assigned_nodes(s1), &[a, c]);
    assert_eq!(doc.take_slot_signals(), vec![s1]);
    doc.remove_attr(c, "slot");
    assert_eq!(doc.assigned_nodes(d), &[b, text, c]);
    assert_eq!(doc.assigned_nodes(s1), &[a]);
    assert_eq!(doc.take_slot_signals(), vec![s1, d]);
    assert_eq!(doc.assigned_slot(c), Some(d));
    // The fallback of a slot without assigned nodes is its children, in the
    // flattened list only.
    let fb = element(&mut doc, "i");
    doc.append_child(e, fb).expect("append");
    assert_eq!(doc.assigned_nodes(e), []);
    assert_eq!(doc.flattened_assigned_nodes(e), vec![fb]);
    assert_eq!(
        doc.take_slot_signals(),
        vec![e],
        "a fallback change signals"
    );
}

/// A slot of the same name earlier in tree order takes the nodes; renaming
/// gives them back (measured `s1b` and `zz`).
#[test]
fn the_first_slot_of_a_name_wins() {
    let (mut doc, _h, root, [_d, s1, _e]) = named_host();
    let a = id(&doc, "a");
    let s1b = slot(&mut doc, Some("s1"));
    let _ = doc.take_slot_signals();
    let first = doc.node(root).first_child();
    doc.insert_before(root, s1b, first).expect("prepend");
    assert_eq!(doc.assigned_nodes(s1b), &[a]);
    assert_eq!(doc.assigned_nodes(s1), []);
    assert_eq!(doc.take_slot_signals(), vec![s1b, s1], "tree order");
    doc.remove(s1b);
    assert_eq!(doc.assigned_nodes(s1), &[a]);
    assert_eq!(doc.assigned_nodes(s1b), []);
    assert_eq!(doc.assigned_slot(a), Some(s1));
    // Chromium signals the removed slot first (measured, `slotchange-order`).
    assert_eq!(doc.take_slot_signals(), vec![s1b, s1]);
    doc.set_attr(s1, "name", "zz").expect("name");
    assert_eq!(doc.assigned_nodes(s1), []);
    assert_eq!(doc.assigned_slot(a), None);
    doc.set_attr(s1, "name", "s1").expect("name");
    assert_eq!(doc.assigned_nodes(s1), &[a]);
    // The same value changes nothing.
    let _ = doc.take_slot_signals();
    doc.set_attr(s1, "name", "s1").expect("name");
    assert!(!doc.has_slot_signals());
}

#[test]
fn signals_follow_the_assigned_lists() {
    let mut doc = crate::parse_html("<div id=h><span id=a>A</span></div>");
    let h = id(&doc, "h");
    let root = doc.attach_shadow(h, open(), false).expect("root");
    let s = slot(&mut doc, None);
    doc.append_child(root, s).expect("append");
    assert_eq!(doc.take_slot_signals(), vec![s]);
    // Two insertions: one signal.
    for _ in 0..2 {
        let i = element(&mut doc, "i");
        doc.append_child(h, i).expect("append");
    }
    assert_eq!(doc.take_slot_signals(), vec![s]);
    assert_eq!(doc.assigned_nodes(s).len(), 3);
    // A comment is no slottable; an empty text node is.
    let comment = doc
        .create_node(NodeData::Comment("c".to_owned()))
        .expect("comment");
    doc.append_child(h, comment).expect("append");
    assert!(!doc.has_slot_signals());
    let empty = doc.create_text("").expect("text");
    doc.append_child(h, empty).expect("append");
    assert_eq!(doc.take_slot_signals(), vec![s]);
    // A removal from the host.
    let first = doc.node(h).first_child().expect("child");
    doc.remove(first);
    assert_eq!(doc.take_slot_signals(), vec![s]);
    assert_eq!(doc.assigned_slot(first), None);
    // Slots of a detached tree signal too.
    let d = element(&mut doc, "div");
    let i = element(&mut doc, "i");
    doc.append_child(d, i).expect("append");
    let r = doc.attach_shadow(d, open(), false).expect("root");
    let s2 = slot(&mut doc, None);
    doc.append_child(r, s2).expect("append");
    assert_eq!(doc.take_slot_signals(), vec![s2]);
    // A slot outside a shadow tree has no assigned nodes and no flattened
    // list.
    let outside = slot(&mut doc, None);
    let t = doc.create_text("fb").expect("text");
    doc.append_child(outside, t).expect("append");
    let body = doc.body().expect("body");
    doc.append_child(body, outside).expect("append");
    assert_eq!(doc.flattened_assigned_nodes(outside), Vec::<NodeId>::new());
    assert!(!doc.has_slot_signals());
}

/// The probe case `slot-assignment-manual` (Chromium 148).
#[test]
fn manual_assignment() {
    let mut doc = crate::parse_html(
        "<div id=h><span id=a slot=s1>A</span><span id=b>B</span>t</div><span id=o>O</span>",
    );
    let (h, a, b, o) = (id(&doc, "h"), id(&doc, "a"), id(&doc, "b"), id(&doc, "o"));
    let manual = ShadowRootInit {
        slot_assignment: SlotAssignment::Manual,
        ..open()
    };
    let root = doc.attach_shadow(h, manual, false).expect("root");
    let s1 = slot(&mut doc, Some("s1"));
    let s2 = slot(&mut doc, None);
    doc.append_child(root, s1).expect("append");
    doc.append_child(root, s2).expect("append");
    assert_eq!(
        (doc.assigned_nodes(s1), doc.assigned_nodes(s2)),
        (&[][..], &[][..])
    );
    doc.assign_slot_manually(s1, &[b, a]);
    assert_eq!(doc.assigned_nodes(s1), &[b, a]);
    assert_eq!(doc.find_slot(a, true), Some(s1));
    doc.assign_slot_manually(s2, &[a]);
    assert_eq!(doc.assigned_nodes(s1), &[b]);
    assert_eq!(doc.assigned_nodes(s2), &[a]);
    doc.assign_slot_manually(s1, &[o]);
    assert_eq!(doc.assigned_nodes(s1), []);
    doc.append_child(h, o).expect("append");
    assert_eq!(doc.assigned_nodes(s1), &[o]);
    doc.assign_slot_manually(s1, &[]);
    assert_eq!(doc.assigned_nodes(s1), []);
    // An unchanged list does not signal.
    doc.assign_slot_manually(s1, &[b]);
    let _ = doc.take_slot_signals();
    doc.assign_slot_manually(s1, &[b, b]);
    assert!(!doc.has_slot_signals());
    // A removed slot keeps its manual list and gets it back.
    doc.remove(s1);
    assert_eq!(doc.assigned_slot(b), None);
    assert_eq!(doc.assigned_nodes(s1), []);
    doc.append_child(root, s1).expect("append");
    assert_eq!(doc.assigned_nodes(s1), &[b]);
}

/// A nested slot's flattened list has the outer host's children (measured).
#[test]
fn flattened_lists_of_nested_slots() {
    let mut doc = crate::parse_html("<div id=o><b id=x1>x</b></div>");
    let (o, x1) = (id(&doc, "o"), id(&doc, "x1"));
    let r1 = doc.attach_shadow(o, open(), false).expect("root");
    let ih = element(&mut doc, "div");
    let os = slot(&mut doc, None);
    doc.append_child(ih, os).expect("append");
    doc.append_child(r1, ih).expect("append");
    let r2 = doc.attach_shadow(ih, open(), false).expect("root");
    let is = slot(&mut doc, None);
    let fb = doc.create_text("fb").expect("text");
    doc.append_child(is, fb).expect("append");
    doc.append_child(r2, is).expect("append");
    assert_eq!(doc.assigned_nodes(is), &[os]);
    assert_eq!(doc.flattened_assigned_nodes(is), vec![x1]);
    assert_eq!(doc.assigned_nodes(os), &[x1]);
    assert_eq!(doc.assigned_slot(os), Some(is));
    // A closed root hides its slot from `assignedSlot`.
    let d = crate::parse_html("<div id=d><i>q</i></div>");
    let mut d = d;
    let host = id(&d, "d");
    let closed = ShadowRootInit {
        mode: ShadowRootMode::Closed,
        ..open()
    };
    let r = d.attach_shadow(host, closed, false).expect("root");
    let s = slot(&mut d, None);
    d.append_child(r, s).expect("append");
    let q = d.node(host).first_child().expect("child");
    assert_eq!(d.find_slot(q, true), None);
    assert_eq!(d.find_slot(q, false), Some(s));
    assert_eq!(d.assigned_nodes(s), &[q]);
}

/// The steps count their work, so that a hostile page reaches the time limit
/// (ADR 0036 section 12).
#[test]
fn assignment_counts_its_work() {
    let mut doc = crate::parse_html("<div id=h></div>");
    let h = id(&doc, "h");
    let root = doc.attach_shadow(h, open(), false).expect("root");
    let s = slot(&mut doc, None);
    doc.append_child(root, s).expect("append");
    for _ in 0..3000 {
        let i = element(&mut doc, "i");
        doc.append_child(h, i).expect("append");
    }
    assert!(!doc.take_shadow_work(usize::MAX));
    assert!(doc.take_shadow_work(1000));
    assert!(!doc.take_shadow_work(1000), "the count starts again");
}

/// A slot prepended to a root costs a few steps, however many slots the
/// root has (the siblings are not scanned).
#[test]
fn prepended_slots_cost_linear_work() {
    let mut doc = crate::parse_html("<div id=h></div>");
    let h = id(&doc, "h");
    let root = doc.attach_shadow(h, open(), false).expect("root");
    let mut last = None;
    for _ in 0..20_000 {
        let s = slot(&mut doc, None);
        let first = doc.node(root).first_child();
        doc.insert_before(root, s, first).expect("prepend");
        last = Some(s);
    }
    assert!(
        !doc.take_shadow_work(500_000),
        "the work of the prepends is linear"
    );
    assert_eq!(doc.first_slot(root, ""), last);
    // A slot in the middle goes to its place.
    let first = doc.node(root).first_child().expect("slot");
    let middle = slot(&mut doc, None);
    let second = doc.node(first).next_sibling;
    doc.insert_before(root, middle, second).expect("insert");
    assert_eq!(doc.first_slot(root, ""), Some(first));
    doc.remove(first);
    assert_eq!(doc.first_slot(root, ""), Some(middle));
}

/// Building a large detached tree bottom-up walks a bounded part of the
/// subtree per insertion, also when the arena has a shadow root elsewhere.
#[test]
fn a_detached_chain_is_not_walked_for_slots() {
    let mut doc = crate::parse_html("<div id=h></div>");
    let h = id(&doc, "h");
    let root = doc.attach_shadow(h, open(), false).expect("root");
    let mut top = element(&mut doc, "div");
    for _ in 0..10_000 {
        let outer = element(&mut doc, "div");
        doc.append_child(outer, top).expect("append");
        top = outer;
    }
    assert!(
        !doc.take_shadow_work(500_000),
        "the insertions walk a bounded part of the subtree"
    );
    // A slot in a small subtree joins the root of its new parent.
    let inner = element(&mut doc, "div");
    let s = slot(&mut doc, None);
    doc.append_child(inner, s).expect("append");
    let wrapper = element(&mut doc, "div");
    doc.append_child(wrapper, inner).expect("append");
    assert_eq!(doc.first_slot(root, ""), None);
    doc.append_child(root, wrapper).expect("append");
    assert_eq!(doc.first_slot(root, ""), Some(s));
    // So does a slot in a subtree larger than the bounded walk.
    let mut big = element(&mut doc, "div");
    for _ in 0..100 {
        let outer = element(&mut doc, "div");
        doc.append_child(outer, big).expect("append");
        big = outer;
    }
    let deep = slot(&mut doc, Some("deep"));
    doc.append_child(big, deep).expect("append");
    let big_top = doc.root(big);
    doc.append_child(root, big_top).expect("append");
    assert_eq!(doc.first_slot(root, "deep"), Some(deep));
    doc.remove(big_top);
    assert_eq!(doc.first_slot(root, "deep"), None);
}

/// Removing assigned children from either end of a long list costs a few
/// steps each.
#[test]
fn removing_assigned_nodes_at_the_ends_is_cheap() {
    for from_end in [true, false] {
        let mut doc = crate::parse_html("<div id=h></div>");
        let h = id(&doc, "h");
        let root = doc.attach_shadow(h, open(), false).expect("root");
        let s = slot(&mut doc, None);
        doc.append_child(root, s).expect("append");
        for _ in 0..20_000 {
            let i = element(&mut doc, "i");
            doc.append_child(h, i).expect("append");
        }
        assert_eq!(doc.assigned_nodes(s).len(), 20_000);
        let _ = doc.take_shadow_work(0);
        while let Some(child) = if from_end {
            doc.node(h).last_child()
        } else {
            doc.node(h).first_child()
        } {
            doc.remove_child(h, child).expect("remove");
        }
        assert_eq!(doc.assigned_nodes(s), []);
        assert!(
            !doc.take_shadow_work(500_000),
            "removal from the end: {from_end}"
        );
    }
}

/// `assignedSlot` (`find_slot` with `open`) hides the slot of a closed root;
/// the assignment itself is the same.
#[test]
fn assigned_slot_hides_the_slot_of_a_closed_root() {
    for (mode, visible) in [
        (ShadowRootMode::Open, true),
        (ShadowRootMode::Closed, false),
    ] {
        let mut doc = crate::parse_html("<div id=h><i id=a></i></div>");
        let h = id(&doc, "h");
        let a = id(&doc, "a");
        let init = ShadowRootInit { mode, ..open() };
        let root = doc.attach_shadow(h, init, false).expect("root");
        let s = slot(&mut doc, None);
        doc.append_child(root, s).expect("append");
        assert_eq!(doc.find_slot(a, false), Some(s));
        assert_eq!(doc.assigned_slot(a), Some(s));
        assert_eq!(doc.find_slot(a, true), visible.then_some(s), "{mode:?}");
    }
}

/// Signal slots that were taken and not fired go back before the new ones.
#[test]
fn restored_signal_slots_come_first() {
    let (mut doc, h, _root, [d, s1, e]) = named_host();
    let _ = doc.take_slot_signals();
    let extra = element(&mut doc, "b");
    doc.append_child(h, extra).expect("append");
    assert_eq!(doc.take_slot_signals(), vec![d]);
    doc.restore_slot_signals(&[d, s1]);
    doc.set_attr(extra, "slot", "nope2").expect("slot");
    assert_eq!(doc.take_slot_signals(), vec![d, s1, e]);
    assert!(!doc.has_slot_signals());
}

/// A transplant drops the manual and assigned entries of the nodes that
/// leave, and keeps those of the nodes that stay.
#[test]
fn a_transplant_drops_manual_entries_of_moved_nodes() {
    let mut doc =
        crate::parse_html("<div id=h><i id=a></i><i id=b></i></div><div id=m><u id=u></u></div>");
    let h = id(&doc, "h");
    let init = ShadowRootInit {
        slot_assignment: SlotAssignment::Manual,
        ..open()
    };
    let root = doc.attach_shadow(h, init, false).expect("root");
    let s = slot(&mut doc, None);
    doc.append_child(root, s).expect("append");
    let (a, b) = (id(&doc, "a"), id(&doc, "b"));
    doc.assign_slot_manually(s, &[a, b]);
    assert_eq!(doc.assigned_nodes(s), &[a, b]);
    // An unrelated subtree leaves: nothing changes.
    let m = id(&doc, "m");
    doc.remove(m);
    drop(doc.take_subtree(m));
    assert_eq!(doc.assigned_nodes(s), &[a, b]);
    // `a` leaves: its entries go, those of `b` stay.
    doc.remove(a);
    drop(doc.take_subtree(a));
    let state = doc.shadow.as_ref().expect("state");
    assert_eq!(state.manual.get(&s), Some(&vec![b]));
    assert!(!state.manual_slot.contains_key(&a));
    assert!(!state.assigned_slot.contains_key(&a));
    assert_eq!(doc.assigned_nodes(s), &[b]);
    // The host leaves: the entries of the slot go with it.
    doc.remove(h);
    drop(doc.take_subtree(h));
    let state = doc.shadow.as_ref().expect("state");
    assert!(state.manual.is_empty() && state.manual_slot.is_empty());
    assert!(state.assigned.is_empty() && state.assigned_slot.is_empty());
}
