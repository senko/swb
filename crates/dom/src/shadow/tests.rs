use super::*;
use crate::tree::{QuirksMode, ShadowRootMode, SlotAssignment};
use html5ever::{LocalName, QualName};

pub(crate) fn open() -> ShadowRootInit {
    ShadowRootInit {
        mode: ShadowRootMode::Open,
        slot_assignment: SlotAssignment::Named,
        delegates_focus: false,
        clonable: false,
        serializable: false,
    }
}

pub(crate) fn element(doc: &mut Document, local: &str) -> NodeId {
    let name = QualName::new(None, ns!(html), LocalName::from(local));
    doc.create_element(name, Vec::new()).expect("element")
}

fn id(doc: &Document, name: &str) -> NodeId {
    doc.element_by_id(name).expect("element")
}

/// The checks of "attach a shadow root" (the names of the probe case
/// `attach-errors`, Chromium 148).
#[test]
fn attach_checks_the_host() {
    let mut doc = Document::new();
    for name in [
        "div",
        "span",
        "article",
        "aside",
        "blockquote",
        "body",
        "footer",
        "h1",
        "h6",
        "header",
        "main",
        "nav",
        "p",
        "section",
        "x-a",
    ] {
        let e = element(&mut doc, name);
        assert!(doc.attach_shadow(e, open(), false).is_ok(), "{name}");
    }
    for name in [
        "a",
        "img",
        "input",
        "button",
        "template",
        "slot",
        "video",
        "li",
        "ul",
        "table",
        "textarea",
        "iframe",
        "font-face",
        "foo",
        "X-A",
    ] {
        let e = element(&mut doc, name);
        assert_eq!(
            doc.attach_shadow(e, open(), false),
            Err(AttachError::Unsupported),
            "{name}"
        );
    }
    let svg = doc
        .create_element(
            QualName::new(None, ns!(svg), LocalName::from("g")),
            Vec::new(),
        )
        .expect("g");
    assert_eq!(
        doc.attach_shadow(svg, open(), false),
        Err(AttachError::Unsupported)
    );
    let x = element(&mut doc, "x-b");
    assert_eq!(
        doc.attach_shadow(x, open(), true),
        Err(AttachError::Disabled)
    );
    let d = element(&mut doc, "div");
    let root = doc.attach_shadow(d, open(), false).expect("root");
    assert_eq!(
        doc.attach_shadow(d, open(), false),
        Err(AttachError::Exists)
    );
    assert_eq!(doc.shadow_root(d), Some(root));
    assert_eq!(doc.shadow_host(root), Some(d));
    assert!(doc.node(root).data.is_fragment());
    assert_eq!(doc.node(root).parent(), None);
    assert_eq!(doc.node_document(root), NodeId::DOCUMENT);
}

#[test]
fn a_full_arena_refuses_a_root() {
    let mut doc = Document::with_limits(crate::tree::Limits {
        max_nodes: 2,
        ..crate::tree::Limits::default()
    });
    let d = element(&mut doc, "div");
    assert!(matches!(
        doc.attach_shadow(d, open(), false),
        Err(AttachError::Limit(DomError::NodeLimit(2)))
    ));
    assert_eq!(doc.shadow_root(d), None);
}

/// Connectedness follows the hosts; the document's id index does not see
/// shadow nodes (measured, case `root-properties`).
#[test]
fn connected_through_hosts() {
    let mut doc = crate::parse_html("<div id=o><div id=h>light</div></div>");
    let (o, h) = (id(&doc, "o"), id(&doc, "h"));
    let _ = doc.take_shadow_changed();
    let root = doc.attach_shadow(h, open(), false).expect("root");
    assert!(doc.take_shadow_changed(), "a connected host restyles");
    let inner = element(&mut doc, "div");
    doc.set_attr(inner, "id", "in").expect("id");
    doc.append_child(root, inner).expect("append");
    assert!(doc.is_connected(inner));
    assert!(!doc.in_document_tree(inner));
    assert!(doc.in_shadow_tree(inner));
    assert_eq!(doc.root(inner), root);
    assert_eq!(doc.shadow_including_root(inner), NodeId::DOCUMENT);
    assert_eq!(doc.element_by_id("in"), None);
    doc.remove(o);
    assert!(!doc.is_connected(inner));
    assert_eq!(doc.shadow_including_root(inner), o);
    let body = doc.body().expect("body");
    doc.append_child(body, o).expect("append");
    assert!(doc.is_connected(inner));
    // A detached host: its tree is not connected, and no restyle.
    let d = element(&mut doc, "div");
    let _ = doc.take_shadow_changed();
    doc.attach_shadow(d, open(), false).expect("root");
    assert!(!doc.take_shadow_changed());
    assert!(!doc.is_connected(doc.shadow_root(d).expect("root")));
}

/// `r.appendChild(host)` and `r.appendChild(ancestor of host)` are
/// hierarchy errors (measured), also for a host without light children.
#[test]
fn a_host_is_a_host_including_ancestor_of_its_tree() {
    let mut doc = crate::parse_html("<div id=o><div id=h></div></div>");
    let (o, h) = (id(&doc, "o"), id(&doc, "h"));
    let root = doc.attach_shadow(h, open(), false).expect("root");
    assert_eq!(doc.append_child(root, h), Err(DomError::HierarchyRequest));
    assert_eq!(doc.append_child(root, o), Err(DomError::HierarchyRequest));
    let inner = element(&mut doc, "p");
    doc.append_child(root, inner).expect("append");
    assert_eq!(doc.append_child(inner, h), Err(DomError::HierarchyRequest));
}

/// DOM "shadow-including tree order": a host's tree right after the host,
/// before its children, nested trees inside (measured with `define`).
#[test]
fn shadow_including_order() {
    let mut doc = crate::parse_html("<p id=p1></p><div id=h><i id=l></i></div><p id=p2></p>");
    let h = id(&doc, "h");
    let root = doc.attach_shadow(h, open(), false).expect("root");
    let q1 = element(&mut doc, "b");
    let nh = element(&mut doc, "div");
    doc.append_child(root, q1).expect("append");
    doc.append_child(root, nh).expect("append");
    let inner_root = doc.attach_shadow(nh, open(), false).expect("root");
    let q0 = element(&mut doc, "u");
    doc.append_child(inner_root, q0).expect("append");
    let body = doc.body().expect("body");
    let order: Vec<NodeId> = doc.shadow_including_descendants(body).collect();
    let expected = vec![
        body,
        id(&doc, "p1"),
        h,
        root,
        q1,
        nh,
        inner_root,
        q0,
        id(&doc, "l"),
        id(&doc, "p2"),
    ];
    assert_eq!(order, expected);
    for pair in expected.windows(2) {
        assert!(doc.shadow_including_precedes(pair[0], pair[1]), "{pair:?}");
        assert!(!doc.shadow_including_precedes(pair[1], pair[0]), "{pair:?}");
    }
    // From a root and from a host without children.
    assert_eq!(
        doc.shadow_including_descendants(inner_root)
            .collect::<Vec<_>>(),
        vec![inner_root, q0]
    );
    assert_eq!(
        doc.shadow_including_descendants(nh).collect::<Vec<_>>(),
        vec![nh, inner_root, q0]
    );
    assert_eq!(
        doc.shadow_including_ancestors(q0).collect::<Vec<_>>()[..4],
        [inner_root, nh, root, h]
    );
}

/// 100,000 nested shadow trees: the walks are iterative.
#[test]
fn deep_nesting_needs_no_recursion() {
    let mut doc = Document::new();
    let top = element(&mut doc, "div");
    let mut host = top;
    for _ in 0..100_000 {
        let root = doc.attach_shadow(host, open(), false).expect("root");
        let next = element(&mut doc, "div");
        doc.append_child(root, next).expect("append");
        host = next;
    }
    assert_eq!(doc.shadow_including_descendants(top).count(), 200_001);
    assert_eq!(doc.shadow_including_root(host), top);
    assert!(!doc.is_connected(host));
}

/// "Clone a node": a clonable root comes along with its content, also for a
/// shallow clone; another root does not; a root itself is not clonable.
#[test]
fn clones_copy_clonable_roots() {
    let mut doc = crate::parse_html("<div id=d>t</div><div id=e>t</div>");
    let (d, e) = (id(&doc, "d"), id(&doc, "e"));
    let rd = doc.attach_shadow(d, open(), false).expect("root");
    let re = doc
        .attach_shadow(
            e,
            ShadowRootInit {
                clonable: true,
                serializable: true,
                ..open()
            },
            false,
        )
        .expect("root");
    for (root, text) in [(rd, "s"), (re, "s")] {
        let i = element(&mut doc, "i");
        let t = doc.create_text(text).expect("text");
        doc.append_child(i, t).expect("append");
        doc.append_child(root, i).expect("append");
    }
    let c1 = doc.clone_node(d, d, true).expect("clone").root;
    assert_eq!(doc.shadow_root(c1), None);
    for deep in [true, false] {
        let copy = doc.clone_node(e, e, deep).expect("clone").root;
        let root = doc.shadow_root(copy).expect("copied root");
        assert_ne!(root, re);
        let data = doc.shadow_root_data(root).expect("data");
        assert!(data.clonable && data.serializable);
        assert_eq!(data.host, copy);
        assert_eq!(crate::inner_html(&doc, root), "<i>s</i>");
        assert_eq!(doc.child_count(copy), usize::from(deep));
    }
    assert_eq!(
        doc.clone_node(re, re, true).map(|c| c.root),
        Err(DomError::NotSupported)
    );
}

/// A host adopted into another document takes its tree along; a root
/// cannot be adopted.
#[test]
fn adoption_takes_the_shadow_tree() {
    let mut doc = crate::parse_html("<div id=h></div>");
    let h = id(&doc, "h");
    let root = doc.attach_shadow(h, open(), false).expect("root");
    let inner = element(&mut doc, "b");
    doc.append_child(root, inner).expect("append");
    let other = doc.create_document().expect("document");
    doc.adopt(h, other).expect("adopt");
    assert_eq!(doc.node_document(root), other);
    assert_eq!(doc.node_document(inner), other);
    assert_eq!(
        doc.adopt(root, NodeId::DOCUMENT),
        Err(DomError::HierarchyRequest)
    );
    assert_eq!(doc.quirks_mode_of(other), QuirksMode::NoQuirks);
}

/// Appending a root moves its children (a fragment insertion) and keeps the
/// root with its host (measured).
#[test]
fn a_root_inserts_like_a_fragment() {
    let mut doc = crate::parse_html("<div id=h></div><div id=t></div>");
    let (h, t) = (id(&doc, "h"), id(&doc, "t"));
    let root = doc.attach_shadow(h, open(), false).expect("root");
    for _ in 0..2 {
        let i = element(&mut doc, "i");
        doc.append_child(root, i).expect("append");
    }
    doc.append_child(t, root).expect("append");
    assert_eq!(doc.child_count(t), 2);
    assert_eq!(doc.child_count(root), 0);
    assert_eq!(doc.shadow_root(h), Some(root));
}

/// A transplant takes the shadow roots with their hosts; the other arena
/// assigns their slots again.
#[test]
fn a_transplant_takes_the_roots() {
    let mut a = crate::parse_html("<body><div id=d><div id=h><b>x</b><i slot=n>y</i></div></div>");
    let mut b = crate::parse_html("<body>");
    let h = id(&a, "h");
    let root = a.attach_shadow(h, open(), false).expect("root");
    let slot = element(&mut a, "slot");
    let named = element(&mut a, "slot");
    a.set_attr(named, "name", "n").expect("name");
    a.append_child(root, slot).expect("append");
    a.append_child(root, named).expect("append");
    assert_eq!(a.assigned_nodes(slot).len(), 1);
    let d = id(&a, "d");
    a.remove(d);
    let size = a.subtree_size(d);
    b.check_subtree_room(size).expect("room");
    let subtree = a.take_subtree(d);
    assert_eq!(subtree.len(), 9);
    assert!(
        a.shadow
            .as_ref()
            .is_some_and(|s| s.roots.is_empty() && s.assigned.is_empty())
    );
    let placed = b.put_subtree(NodeId::DOCUMENT, subtree).expect("put");
    let new_host = placed
        .pairs
        .iter()
        .find(|&&(old, _)| old == h)
        .map(|&(_, new)| new)
        .expect("host");
    let new_root = b.shadow_root(new_host).expect("root");
    assert_eq!(b.shadow_host(new_root), Some(new_host));
    let slots: Vec<NodeId> = b.children(new_root).collect();
    let light: Vec<NodeId> = b.children(new_host).collect();
    assert_eq!(b.assigned_nodes(slots[0]), &light[..1]);
    assert_eq!(b.assigned_nodes(slots[1]), &light[1..]);
    assert_eq!(b.assigned_slot(light[1]), Some(slots[1]));
}
