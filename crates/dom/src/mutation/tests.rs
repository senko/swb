//! Tests of the change functions, the journal, the mutation sink and the
//! id index.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::journal::MutationSink;
use crate::parse_html;
use crate::tree::Limits;

fn html(local: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(local))
}

fn element(doc: &mut Document, local: &str) -> NodeId {
    doc.create_element(html(local), vec![]).expect("room")
}

fn element_with_id(doc: &mut Document, local: &str, id: &str) -> NodeId {
    let attrs = vec![Attribute {
        name: QualName::new(None, ns!(), local_name!("id")),
        value: id.to_owned(),
    }];
    doc.create_element(html(local), attrs).expect("room")
}

fn children(doc: &Document, node: NodeId) -> Vec<NodeId> {
    doc.children(node).collect()
}

/// A record as a mutation observer would see it, with the state that the
/// sink can read from the document at that moment.
#[derive(Debug, PartialEq, Eq)]
enum Record {
    Children {
        parent: NodeId,
        added: Vec<NodeId>,
        removed: Vec<NodeId>,
        prev: Option<NodeId>,
        next: Option<NodeId>,
        /// The inclusive ancestors of the parent at the time (where the
        /// observers of a removed node's transient registrations are).
        parent_path: Vec<NodeId>,
        /// True if each removed node was detached at the time.
        removed_detached: bool,
    },
    Attribute {
        element: NodeId,
        name: String,
        old: Option<String>,
    },
    Text {
        node: NodeId,
        old: Option<String>,
        new: String,
    },
}

#[derive(Default)]
struct Recorder(Rc<RefCell<Vec<Record>>>);

impl MutationSink for Recorder {
    fn change(&mut self, doc: &Document, change: &Change<'_>) {
        let record = match *change {
            Change::Children {
                parent,
                added,
                removed,
                prev_sibling,
                next_sibling,
            } => Record::Children {
                parent,
                added: added.to_vec(),
                removed: removed.to_vec(),
                prev: prev_sibling,
                next: next_sibling,
                parent_path: std::iter::once(parent)
                    .chain(doc.ancestors(parent))
                    .collect(),
                removed_detached: removed.iter().all(|&n| doc.parent(n).is_none()),
            },
            Change::Attribute {
                element,
                name,
                old_value,
            } => Record::Attribute {
                element,
                name: name.local.to_string(),
                old: old_value.map(str::to_owned),
            },
            Change::Text { node, old_value } => Record::Text {
                node,
                old: old_value.map(str::to_owned),
                new: doc.char_data(node).unwrap_or_default().to_owned(),
            },
        };
        self.0.borrow_mut().push(record);
    }
}

/// A document with `<html><body>` and a recording sink.
fn recorded() -> (Document, NodeId, Rc<RefCell<Vec<Record>>>) {
    let mut doc = parse_html("<body>");
    let body = doc.body().expect("body");
    let recorder = Recorder::default();
    let records = Rc::clone(&recorder.0);
    doc.set_mutation_sink(Some(Box::new(recorder)));
    doc.take_journal();
    (doc, body, records)
}

#[test]
fn insertion_and_removal_are_recorded() {
    let (mut doc, body, records) = recorded();
    let html_element = doc.document_element().unwrap();
    let a = element(&mut doc, "a");
    let b = element(&mut doc, "b");
    let c = element(&mut doc, "c");
    let generation = doc.generation();
    doc.append_child(body, a).unwrap();
    doc.append_child(body, c).unwrap();
    doc.insert_before(body, b, Some(c)).unwrap();
    assert_eq!(children(&doc, body), [a, b, c]);
    assert_eq!(doc.generation(), generation + 3);
    doc.remove(b);
    assert_eq!(children(&doc, body), [a, c]);
    let path = vec![body, html_element, NodeId::DOCUMENT];
    let added = |node, prev, next| Record::Children {
        parent: body,
        added: vec![node],
        removed: vec![],
        prev,
        next,
        parent_path: path.clone(),
        removed_detached: true,
    };
    assert_eq!(
        *records.borrow(),
        [
            added(a, None, None),
            added(c, Some(a), None),
            added(b, Some(a), Some(c)),
            Record::Children {
                parent: body,
                added: vec![],
                removed: vec![b],
                prev: Some(a),
                next: Some(c),
                parent_path: path.clone(),
                removed_detached: true,
            },
        ]
    );
    // The journal has the same changes.
    let journal = doc.take_journal();
    assert_eq!(journal.len(), 4);
    assert!(doc.journal().is_empty());
    // Removing a node without parent changes nothing.
    doc.remove(b);
    assert_eq!(records.borrow().len(), 4);
}

#[test]
fn moving_a_node_removes_it_first() {
    let (mut doc, body, records) = recorded();
    let a = element(&mut doc, "a");
    let b = element(&mut doc, "b");
    doc.append_child(body, a).unwrap();
    doc.append_child(a, b).unwrap();
    records.borrow_mut().clear();
    doc.append_child(body, b).unwrap();
    assert_eq!(children(&doc, body), [a, b]);
    let records = records.borrow();
    assert!(matches!(
        &records[..],
        [
            Record::Children { parent: p1, removed: r, .. },
            Record::Children { parent: p2, added: ad, prev: Some(prev), .. },
        ] if *p1 == a && r == &[b] && *p2 == body && ad == &[b] && *prev == a
    ));
}

#[test]
fn a_fragment_inserts_its_children_with_one_record() {
    let (mut doc, body, records) = recorded();
    let fragment = doc.create_node(NodeData::DocumentFragment).expect("room");
    let (a, b) = (element(&mut doc, "a"), element(&mut doc, "b"));
    doc.append_child(fragment, a).unwrap();
    doc.append_child(fragment, b).unwrap();
    let existing = element(&mut doc, "x");
    doc.append_child(body, existing).unwrap();
    records.borrow_mut().clear();
    doc.insert_before(body, fragment, Some(existing)).unwrap();
    assert_eq!(children(&doc, body), [a, b, existing]);
    assert_eq!(doc.children(fragment).count(), 0);
    let records = records.borrow();
    assert_eq!(records.len(), 2);
    assert!(
        matches!(&records[0], Record::Children { parent, removed, .. }
        if *parent == fragment && removed == &[a, b])
    );
    assert!(
        matches!(&records[1], Record::Children { parent, added, prev: None, next, .. }
        if *parent == body && added == &[a, b] && *next == Some(existing))
    );
}

#[test]
fn replace_and_replace_all_make_one_record() {
    let (mut doc, body, records) = recorded();
    let (a, b, c) = (
        element(&mut doc, "a"),
        element(&mut doc, "b"),
        element(&mut doc, "c"),
    );
    doc.append_child(body, a).unwrap();
    doc.append_child(body, b).unwrap();
    records.borrow_mut().clear();
    doc.replace_child(body, c, a).unwrap();
    assert_eq!(children(&doc, body), [c, b]);
    assert!(
        matches!(&records.borrow()[..], [Record::Children { added, removed, prev: None, next, .. }]
        if added == &[c] && removed == &[a] && *next == Some(b))
    );
    records.borrow_mut().clear();
    let text = doc.create_text("t").unwrap();
    doc.replace_all(body, Some(text)).unwrap();
    assert_eq!(children(&doc, body), [text]);
    assert!(
        matches!(&records.borrow()[..], [Record::Children { added, removed, .. }]
        if added == &[text] && removed == &[c, b])
    );
    records.borrow_mut().clear();
    doc.replace_all(body, None).unwrap();
    assert_eq!(doc.children(body).count(), 0);
    assert_eq!(records.borrow().len(), 1);
    // Nothing to replace: no record.
    doc.replace_all(body, None).unwrap();
    assert_eq!(records.borrow().len(), 1);
}

#[test]
fn invalid_trees_are_refused() {
    let mut doc = parse_html("<!DOCTYPE html><body><div id=d><p id=p></p></div>");
    let id = |doc: &Document, name: &str| doc.element_by_id(name).unwrap();
    let (div, p) = (id(&doc, "d"), id(&doc, "p"));
    let body = doc.body().unwrap();
    let html_element = doc.document_element().unwrap();
    let generation = doc.generation();
    // A node into itself or its descendant.
    assert_eq!(doc.append_child(div, div), Err(DomError::HierarchyRequest));
    assert_eq!(doc.append_child(p, div), Err(DomError::HierarchyRequest));
    // The reference node is not a child.
    assert_eq!(
        doc.insert_before(div, body, Some(p)),
        Err(DomError::HierarchyRequest)
    );
    let other = element(&mut doc, "i");
    assert_eq!(
        doc.insert_before(body, other, Some(p)),
        Err(DomError::NotFound)
    );
    assert_eq!(doc.remove_child(body, p), Err(DomError::NotFound));
    // Text into a document, a second root element, a document as a child.
    let text = doc.create_text("x").unwrap();
    assert_eq!(
        doc.append_child(NodeId::DOCUMENT, text),
        Err(DomError::HierarchyRequest)
    );
    assert_eq!(
        doc.append_child(NodeId::DOCUMENT, other),
        Err(DomError::HierarchyRequest)
    );
    let second = doc.create_document().unwrap();
    assert_eq!(
        doc.append_child(body, second),
        Err(DomError::HierarchyRequest)
    );
    // Children of a text node.
    assert_eq!(
        doc.append_child(text, other),
        Err(DomError::HierarchyRequest)
    );
    // A doctype after the root element.
    let doctype = doc
        .create_node(NodeData::Doctype {
            name: "html".into(),
            public_id: String::new(),
            system_id: String::new(),
        })
        .unwrap();
    assert_eq!(
        doc.append_child(NodeId::DOCUMENT, doctype),
        Err(DomError::HierarchyRequest)
    );
    assert_eq!(
        doc.append_child(body, doctype),
        Err(DomError::HierarchyRequest)
    );
    // A template into its own contents (across the host).
    let template = element(&mut doc, "template");
    let contents = doc.element(template).unwrap().template_contents.unwrap();
    assert_eq!(
        doc.append_child(contents, template),
        Err(DomError::HierarchyRequest)
    );
    // None of these changed anything.
    assert_eq!(doc.generation(), generation);
    // Replacing the root element with another element is allowed.
    doc.replace_child(NodeId::DOCUMENT, other, html_element)
        .unwrap();
    assert_eq!(doc.document_element(), Some(other));
    // `replaceChildren` checks against the current children: a document
    // with a root element refuses another one, but not nothing.
    assert_eq!(
        doc.replace_all(NodeId::DOCUMENT, Some(html_element)),
        Err(DomError::HierarchyRequest)
    );
    doc.replace_all(NodeId::DOCUMENT, None).unwrap();
    doc.replace_all(NodeId::DOCUMENT, Some(html_element))
        .unwrap();
    assert_eq!(doc.document_element(), Some(html_element));
}

#[test]
fn documents_and_adoption() {
    let mut doc = parse_html("<body>");
    let body = doc.body().unwrap();
    let second = doc.create_document().unwrap();
    assert_eq!(doc.node_document(second), second);
    let root = doc
        .create_node_in(
            second,
            NodeData::Element(ElementData::new(html("html"), vec![])),
        )
        .unwrap();
    assert_eq!(doc.node_document(root), second);
    doc.append_child(second, root).unwrap();
    let a = element_with_id(&mut doc, "a", "x");
    let template = element(&mut doc, "template");
    doc.append_child(a, template).unwrap();
    // Insertion into the other document adopts the subtree.
    doc.append_child(root, a).unwrap();
    assert_eq!(doc.node_document(a), second);
    assert_eq!(doc.node_document(template), second);
    let contents = doc.element(template).unwrap().template_contents.unwrap();
    let inert = doc.node_document(contents);
    assert_ne!(inert, second);
    assert_ne!(inert, NodeId::DOCUMENT);
    // Each document has its own ids.
    assert_eq!(doc.get_element_by_id(second, "x"), Some(a));
    assert_eq!(doc.element_by_id("x"), None);
    // adoptNode removes the node from its parent.
    doc.adopt(a, NodeId::DOCUMENT).unwrap();
    assert_eq!(doc.parent(a), None);
    assert_eq!(doc.node_document(a), NodeId::DOCUMENT);
    assert_eq!(doc.get_element_by_id(second, "x"), None);
    assert_eq!(
        doc.adopt(second, NodeId::DOCUMENT),
        Err(DomError::NotSupported)
    );
    doc.append_child(body, a).unwrap();
    assert_eq!(doc.element_by_id("x"), Some(a));
}

#[test]
fn attribute_changes_are_recorded() {
    let (mut doc, body, records) = recorded();
    doc.set_attr(body, "class", "a").unwrap();
    doc.set_attr(body, "class", "b").unwrap();
    assert!(doc.remove_attr(body, "class"));
    assert!(!doc.remove_attr(body, "class"));
    let xlink = QualName::new(None, ns!(xlink), LocalName::from("href"));
    doc.set_attribute(body, &xlink, "#x").unwrap();
    // The same local name in another namespace is another attribute.
    doc.set_attr(body, "href", "y").unwrap();
    let e = doc.element(body).unwrap();
    assert_eq!(e.attr("href"), Some("y"));
    assert_eq!(e.svg_href(), Some("y"));
    assert_eq!(e.attributes().len(), 2);
    let attr = |name: &str, old: Option<&str>| Record::Attribute {
        element: body,
        name: name.to_owned(),
        old: old.map(str::to_owned),
    };
    assert_eq!(
        *records.borrow(),
        [
            attr("class", None),
            attr("class", Some("a")),
            attr("class", Some("b")),
            attr("href", None),
            attr("href", None),
        ]
    );
    let text = doc.create_text("t").unwrap();
    assert_eq!(doc.set_attr(text, "a", "b"), Err(DomError::InvalidNodeType));
}

#[test]
fn text_changes_give_the_sink_the_old_value() {
    let (mut doc, body, records) = recorded();
    let text = doc.create_text("hello").unwrap();
    doc.append_child(body, text).unwrap();
    records.borrow_mut().clear();
    doc.append_data(text, " world").unwrap();
    doc.replace_data(text, 0, 5, "goodbye").unwrap();
    doc.set_data(text, "é").unwrap();
    assert_eq!(doc.replace_data(text, 1, 0, "x"), Err(DomError::IndexSize));
    assert_eq!(doc.replace_data(text, 3, 0, "x"), Err(DomError::IndexSize));
    doc.replace_data(text, 2, 100, "!").unwrap();
    let change = |old: &str, new: &str| Record::Text {
        node: text,
        old: Some(old.to_owned()),
        new: new.to_owned(),
    };
    assert_eq!(
        *records.borrow(),
        [
            change("hello", "hello world"),
            change("hello world", "goodbye world"),
            change("goodbye world", "é"),
            change("é", "é!"),
        ]
    );
    // The journal does not keep the old text.
    let journal = doc.take_journal();
    let texts: Vec<_> = journal
        .iter()
        .filter(|c| matches!(c, Change::Text { .. }))
        .collect();
    assert_eq!(
        texts,
        [Change::Text {
            node: text,
            old_value: None
        }; 4]
    );
    assert_eq!(doc.set_data(body, "x"), Err(DomError::InvalidNodeType));
}

#[test]
fn the_id_index_follows_changes() {
    let mut doc = parse_html("<body><p id=a>1</p><div id=d><p id=a>2</p></div><p id=b></p>");
    let body = doc.body().unwrap();
    let first = doc.element_by_id("a").unwrap();
    assert_eq!(doc.text_content(first), "1");
    let div = doc.element_by_id("d").unwrap();
    let second = doc.children(div).next().unwrap();
    // Duplicates: the first in tree order wins, also after a move.
    doc.append_child(body, first).unwrap();
    assert_eq!(doc.element_by_id("a"), Some(second));
    doc.insert_before(body, first, Some(div)).unwrap();
    assert_eq!(doc.element_by_id("a"), Some(first));
    // A removed subtree leaves the index; re-insertion brings it back.
    doc.remove(div);
    assert_eq!(doc.element_by_id("d"), None);
    assert_eq!(doc.element_by_id("a"), Some(first));
    doc.remove(first);
    assert_eq!(doc.element_by_id("a"), None);
    doc.append_child(body, div).unwrap();
    assert_eq!(doc.element_by_id("a"), Some(second));
    assert_eq!(doc.element_by_id("d"), Some(div));
    // Attribute changes.
    doc.set_attr(second, "id", "c").unwrap();
    assert_eq!(doc.element_by_id("a"), None);
    assert_eq!(doc.element_by_id("c"), Some(second));
    doc.set_attr(second, "id", "").unwrap();
    assert_eq!(doc.element_by_id("c"), None);
    assert_eq!(doc.element_by_id(""), None);
    doc.set_attr(second, "id", "b").unwrap();
    let b = doc.get_element_by_id(NodeId::DOCUMENT, "b").unwrap();
    assert_ne!(b, second, "the earlier #b wins");
    assert!(doc.remove_attr(b, "id"));
    assert_eq!(doc.element_by_id("b"), Some(second));
    // A detached element with an id is not in the index.
    let detached = element_with_id(&mut doc, "i", "z");
    assert_eq!(doc.element_by_id("z"), None);
    doc.set_attr(detached, "id", "zz").unwrap();
    assert_eq!(doc.element_by_id("zz"), None);
    doc.replace_all(body, Some(detached)).unwrap();
    assert_eq!(doc.element_by_id("zz"), Some(detached));
    assert_eq!(doc.element_by_id("b"), None);
}

#[test]
fn many_duplicate_ids_are_removed_in_one_pass() {
    let html = format!("<body><div id=d>{}</div>", "<i id=x></i>".repeat(20_000));
    let mut doc = parse_html(&html);
    let div = doc.element_by_id("d").unwrap();
    assert!(doc.element_by_id("x").is_some());
    doc.remove(div);
    assert_eq!(doc.element_by_id("x"), None);
}

#[test]
fn a_lookup_among_many_duplicate_ids_walks_the_tree() {
    // 30,000 siblings of one id: pairwise comparisons in tree order would
    // take seconds.
    let html = format!("<body>{}<p id=x></p>", "<i id=x></i>".repeat(30_000));
    let mut doc = parse_html(&html);
    let body = doc.body().unwrap();
    let first = doc.children(body).next().unwrap();
    let p = doc.children(body).last().unwrap();
    assert_eq!(doc.element_by_id("x"), Some(first));
    doc.insert_before(body, p, Some(first)).unwrap();
    assert_eq!(doc.element_by_id("x"), Some(p));
}

#[test]
fn a_loop_of_lookups_among_many_duplicate_ids_walks_once() {
    // 50,000 other siblings before the first of 20,000 elements of one id:
    // a walk per call (50,000 steps) would take 1e9 steps for the loop.
    let html = format!(
        "<body>{}{}",
        "<b></b>".repeat(50_000),
        "<i id=x></i>".repeat(20_000)
    );
    let mut doc = parse_html(&html);
    let body = doc.body().unwrap();
    let first = doc.element_by_id("x").unwrap();
    for _ in 0..20_000 {
        assert_eq!(doc.element_by_id("x"), Some(first));
    }
    // A change drops the result.
    doc.remove(first);
    assert_ne!(doc.element_by_id("x"), Some(first));
    let new = element_with_id(&mut doc, "p", "x");
    let head = doc.children(body).next();
    doc.insert_before(body, new, head).unwrap();
    assert_eq!(doc.element_by_id("x"), Some(new));
}

#[test]
fn the_change_functions_stop_at_the_limits() {
    let mut doc = Document::with_limits(Limits {
        max_nodes: 4,
        max_text_bytes: 100,
    });
    // An element counts its name.
    let a = element(&mut doc, "a");
    let rest = 100 - doc.text_bytes();
    doc.create_text(&"x".repeat(rest)).unwrap();
    assert_eq!(doc.create_text("x"), Err(DomError::ByteLimit(100)));
    assert_eq!(doc.set_attr(a, "id", "x"), Err(DomError::ByteLimit(100)));
    doc.create_text("").unwrap();
    assert_eq!(doc.len(), 4);
    assert_eq!(doc.create_text(""), Err(DomError::NodeLimit(4)));
    // A template needs room for its contents.
    let mut doc = Document::with_limits(Limits {
        max_nodes: 3,
        max_text_bytes: 10,
    });
    assert_eq!(
        doc.create_element(html("template"), vec![]),
        Err(DomError::NodeLimit(3))
    );
    // Replacing text frees its bytes.
    let mut doc = Document::with_limits(Limits {
        max_nodes: 10,
        max_text_bytes: 10,
    });
    let t = doc.create_text("0123456789").unwrap();
    doc.set_data(t, "").unwrap();
    assert_eq!(doc.text_bytes(), 0);
    doc.set_data(t, "abcdefghij").unwrap();
    assert_eq!(doc.append_data(t, "k"), Err(DomError::ByteLimit(10)));
    // Attribute names and records count, so empty attributes with new
    // names also reach the limit; a removal frees all of it.
    let mut doc = Document::with_limits(Limits {
        max_nodes: 10,
        max_text_bytes: 1000,
    });
    let a = element(&mut doc, "a");
    let before = doc.text_bytes();
    doc.set_attr(a, "n", "").unwrap();
    assert!(doc.text_bytes() > before + 1);
    assert!(doc.remove_attr(a, "n"));
    assert_eq!(doc.text_bytes(), before);
    let added = (0..).take_while(|i| doc.set_attr(a, &format!("n{i}"), "").is_ok());
    assert!(added.count() < 20);
    assert_eq!(doc.set_attr(a, "m", ""), Err(DomError::ByteLimit(1000)));
}

#[test]
fn the_id_index_starts_at_the_first_lookup() {
    let mut doc = parse_html("<body><p id=a></p><div id=d><i id=b></i></div>");
    let body = doc.body().unwrap();
    // Changes before the first lookup: the index is built from the tree as
    // it is then.
    let a = doc.children(body).next().unwrap();
    doc.set_attr(a, "id", "c").unwrap();
    let div = doc.children(body).nth(1).unwrap();
    doc.remove(div);
    let detached = element_with_id(&mut doc, "i", "e");
    assert_eq!(doc.element_by_id("a"), None);
    assert_eq!(doc.element_by_id("c"), Some(a));
    assert_eq!(doc.element_by_id("b"), None, "in a removed subtree");
    assert_eq!(doc.element_by_id("e"), None, "detached");
    // After it, the changes keep the index up to date.
    doc.append_child(body, div).unwrap();
    doc.append_child(div, detached).unwrap();
    assert!(doc.element_by_id("b").is_some());
    assert_eq!(doc.element_by_id("e"), Some(detached));
}

#[test]
fn journaling_can_be_off() {
    let (mut doc, body, records) = recorded();
    doc.set_journaling(false);
    let a = element(&mut doc, "a");
    doc.append_child(body, a).unwrap();
    // The sink still gets the change; the journal only notes the gap.
    assert_eq!(records.borrow().len(), 1);
    assert!(doc.journal().incomplete());
    assert_eq!(doc.journal().len(), 0);
    assert!(!doc.journal().is_empty());
    doc.set_journaling(true);
    doc.remove(a);
    assert_eq!(doc.journal().len(), 0, "an incomplete journal stays empty");
    let drained = doc.take_journal();
    assert!(drained.incomplete());
    doc.append_child(body, a).unwrap();
    assert!(!doc.journal().incomplete());
    assert_eq!(doc.journal().len(), 1);
}

#[test]
fn the_sink_can_be_replaced_and_reached() {
    let (mut doc, _body, records) = recorded();
    let sink = doc.mutation_sink_mut().expect("installed");
    let any: &mut dyn std::any::Any = sink;
    assert!(any.downcast_mut::<Recorder>().is_some());
    let old = doc.set_mutation_sink(None);
    assert!(old.is_some());
    let a = element(&mut doc, "a");
    doc.set_attr(a, "x", "y").unwrap();
    assert_eq!(records.borrow().len(), 0, "no sink, no calls");
}
