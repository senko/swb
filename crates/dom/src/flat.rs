//! The flat tree (CSS Scoping 1 §3.3; ADR 0036 section 4 and the 5b notes).
//!
//! Style, layout, counters and hit testing walk one tree: the children of a
//! shadow host are its shadow root's children, the children of a slot in a
//! shadow tree are its assigned nodes (or its own children when it has
//! none: the fallback content), and a light child of a host that no slot
//! takes is not in the tree. A slot that is itself assigned to a slot of a
//! nested host stays in the tree, as a child of that slot (§3.3 builds the
//! tree with "find slottables", not with the flattened lists; measured in
//! Chromium 148, case `nested-slot-inherit`: the light child inherits from
//! the outer slot).
//!
//! No tree is cached: the walks read the `flags` byte of each element
//! and look up the shadow state only at hosts and slots. In an arena without
//! shadow state every function takes one branch more than its DOM
//! counterpart. The walks are iterative.
//! <https://drafts.csswg.org/css-scoping/#flattening>

use std::collections::HashMap;

use crate::tree::{Children, Document, NodeData, NodeId};

/// The tree scopes of a walk down the flat tree. A tree scope is the root
/// of a node's tree: the document, a shadow root, or the root of a
/// detached tree. A walk that knows the scope of a node gets the scope of
/// its flat children from [`FlatScopes::of_children`], without a walk up.
#[derive(Default)]
pub struct FlatScopes {
    /// The scope of each host that the walk entered.
    of_host: HashMap<NodeId, NodeId>,
}

impl FlatScopes {
    /// The tree scope of the flat children of `node`, whose own scope is
    /// `scope`: the shadow root for a host, the scope of the host for the
    /// assigned nodes of a slot, else `scope`. Call it for the nodes of a
    /// path of the flat tree from the top down: a host comes before the
    /// slots of its shadow tree, so their host's scope is known. For a slot
    /// whose host was not given, the scope comes from a walk up from the
    /// host.
    pub fn of_children(&mut self, doc: &Document, node: NodeId, scope: NodeId) -> NodeId {
        if !doc.has_shadow_state() {
            return scope;
        }
        let Some(element) = doc.element(node) else {
            return scope;
        };
        if element.is_shadow_host()
            && let Some(root) = doc.shadow_root(node)
        {
            self.of_host.insert(node, scope);
            return root;
        }
        if element.has_assigned_nodes()
            && let Some(host) = doc.shadow_host(scope)
        {
            return self
                .of_host
                .get(&host)
                .copied()
                .unwrap_or_else(|| doc.root(host));
        }
        scope
    }
}

/// The children of a node in the flat tree. See [`Document::flat_children`].
pub enum FlatChildren<'a> {
    /// The DOM children of a node (or of a host's shadow root).
    Dom(Children<'a>),
    /// The assigned nodes of a slot.
    Listed(std::slice::Iter<'a, NodeId>),
}

impl Iterator for FlatChildren<'_> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        match self {
            FlatChildren::Dom(children) => children.next(),
            FlatChildren::Listed(nodes) => nodes.next().copied(),
        }
    }
}

impl Document {
    /// The children of `node` in the flat tree: the children of its shadow
    /// root for a host, the assigned nodes for a slot that has some, else
    /// the DOM children.
    #[inline]
    pub fn flat_children(&self, node: NodeId) -> FlatChildren<'_> {
        if self.shadow.is_none() {
            return FlatChildren::Dom(self.children(node));
        }
        let Some(element) = self.element(node) else {
            return FlatChildren::Dom(self.children(node));
        };
        if element.is_shadow_host()
            && let Some(root) = self.shadow_root(node)
        {
            return FlatChildren::Dom(self.children(root));
        }
        if element.has_assigned_nodes() {
            return FlatChildren::Listed(self.assigned_nodes(node).iter());
        }
        FlatChildren::Dom(self.children(node))
    }

    /// The element children of `node` in the flat tree.
    #[inline]
    pub fn flat_element_children(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.flat_children(node)
            .filter(|&c| self.node(c).is_element())
    }

    /// The parent of `node` in the flat tree: the assigned slot of a child
    /// of a host, the host of a child of a shadow root, else the DOM parent.
    /// `None` for a node without a parent, for a child of a host that no
    /// slot takes, and for a child of a slot that has assigned nodes (its
    /// fallback content is not used). The answer looks one step up only: a
    /// node below one of those has its DOM parent as its flat parent, which
    /// is what `getComputedStyle` inherits from (measured, cases
    /// `unslotted-computed-style` and `unused-fallback-computed-style`); use
    /// [`Document::in_flat_tree`] for the whole chain.
    #[inline]
    pub fn flat_parent(&self, node: NodeId) -> Option<NodeId> {
        let parent = self.parent(node)?;
        if self.shadow.is_none() {
            return Some(parent);
        }
        match &self.node(parent).data {
            NodeData::Element(e) if e.is_shadow_host() => self.assigned_slot(node),
            NodeData::Element(e) if e.has_assigned_nodes() => None,
            NodeData::ShadowRoot(data) => Some(data.host),
            _ => Some(parent),
        }
    }

    /// The flat parent of `node` if it is an element.
    #[inline]
    pub fn flat_parent_element(&self, node: NodeId) -> Option<NodeId> {
        self.flat_parent(node)
            .filter(|&p| self.node(p).is_element())
    }

    /// The flat ancestors of `node`, nearest first, without `node`.
    pub fn flat_ancestors(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.flat_parent(node), |&n| self.flat_parent(n))
    }

    /// True if `node` is in the flat tree of its document: its flat
    /// ancestors reach a document node. False for a shadow root, for a node
    /// of a detached tree, and for a node below a light child that no slot
    /// takes or below unused fallback content. Costs the flat depth.
    pub fn in_flat_tree(&self, node: NodeId) -> bool {
        let top = self.flat_ancestors(node).last().unwrap_or(node);
        matches!(self.node(top).data, NodeData::Document)
            && (top != node || matches!(self.node(node).data, NodeData::Document))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::parse_html;
    use crate::shadow::ShadowRootInit;
    use crate::tree::{ShadowRootMode, SlotAssignment};

    fn init(slot_assignment: SlotAssignment) -> ShadowRootInit {
        ShadowRootInit {
            mode: ShadowRootMode::Open,
            slot_assignment,
            delegates_focus: false,
            clonable: false,
            serializable: false,
        }
    }

    fn by_id(doc: &Document, id: &str) -> NodeId {
        doc.element_by_id(id).unwrap()
    }

    fn element(doc: &mut Document, local: &str, attrs: &[(&str, &str)]) -> NodeId {
        let e = crate::shadow::tests::element(doc, local);
        for (k, v) in attrs {
            doc.set_attr(e, k, v).unwrap();
        }
        e
    }

    fn text(doc: &mut Document, data: &str) -> NodeId {
        doc.create_text(data).unwrap()
    }

    /// Attaches an open root to `host` and appends `children` to it.
    fn shadow(doc: &mut Document, host: NodeId, children: &[NodeId]) -> NodeId {
        let root = doc
            .attach_shadow(host, init(SlotAssignment::Named), false)
            .unwrap();
        for &c in children {
            doc.append_child(root, c).unwrap();
        }
        root
    }

    fn flat(doc: &Document, node: NodeId) -> Vec<NodeId> {
        doc.flat_children(node).collect()
    }

    #[test]
    fn without_shadow_state_the_flat_tree_is_the_dom() {
        let doc = parse_html("<div id=a><b id=b>x</b>y</div>");
        assert!(!doc.has_shadow_state());
        let a = by_id(&doc, "a");
        let b = by_id(&doc, "b");
        assert!(matches!(doc.flat_children(a), FlatChildren::Dom(_)));
        assert_eq!(flat(&doc, a), doc.children(a).collect::<Vec<_>>());
        assert_eq!(doc.flat_parent(b), Some(a));
        assert_eq!(doc.flat_parent_element(b), Some(a));
        assert_eq!(
            doc.flat_ancestors(b).collect::<Vec<_>>(),
            doc.ancestors(b).collect::<Vec<_>>()
        );
        assert!(doc.in_flat_tree(b));
        assert!(doc.in_flat_tree(NodeId::DOCUMENT));
    }

    #[test]
    fn a_host_shows_its_shadow_children_and_not_its_light_children() {
        let mut doc = parse_html("<div id=h><i id=l>light</i></div>");
        let h = by_id(&doc, "h");
        let l = by_id(&doc, "l");
        let p = element(&mut doc, "p", &[]);
        let t = text(&mut doc, "shadow");
        let root = shadow(&mut doc, h, &[p, t]);
        assert_eq!(flat(&doc, h), vec![p, t]);
        assert_eq!(doc.flat_element_children(h).collect::<Vec<_>>(), vec![p]);
        assert_eq!(doc.flat_parent(p), Some(h));
        assert_eq!(doc.flat_parent(t), Some(h));
        // The root is not in the flat tree; its children's flat parent is
        // the host.
        assert!(!doc.in_flat_tree(root));
        assert!(doc.in_flat_tree(p));
        // A light child without a slot: no flat parent, not in the tree.
        assert_eq!(doc.flat_parent(l), None);
        assert!(!doc.in_flat_tree(l));
        let below = doc.children(l).next().unwrap();
        assert_eq!(doc.flat_parent(below), Some(l));
        assert!(!doc.in_flat_tree(below));
    }

    #[test]
    fn slots_show_their_assigned_nodes_in_slot_order() {
        let mut doc =
            parse_html("<div id=h><span id=x slot=b>x</span><span id=y slot=a>y</span>t</div>");
        let host = by_id(&doc, "h");
        let (span_x, span_y) = (by_id(&doc, "x"), by_id(&doc, "y"));
        let text = doc.children(host).last().unwrap();
        let slot_a = element(&mut doc, "slot", &[("name", "a")]);
        let slot_b = element(&mut doc, "slot", &[("name", "b")]);
        let default = element(&mut doc, "slot", &[]);
        shadow(&mut doc, host, &[slot_a, slot_b, default]);
        assert_eq!(flat(&doc, host), vec![slot_a, slot_b, default]);
        assert_eq!(flat(&doc, slot_a), vec![span_y]);
        assert_eq!(flat(&doc, slot_b), vec![span_x]);
        assert_eq!(flat(&doc, default), vec![text]);
        assert_eq!(doc.flat_parent(span_x), Some(slot_b));
        assert_eq!(doc.flat_parent(text), Some(default));
        assert_eq!(
            doc.flat_ancestors(span_x).take(3).collect::<Vec<_>>(),
            vec![slot_b, host, doc.parent(host).unwrap()]
        );
        assert!(doc.in_flat_tree(span_x));
    }

    #[test]
    fn a_slot_without_assigned_nodes_shows_its_fallback() {
        let mut doc = parse_html("<div id=h></div><div id=g><b id=l>l</b></div>");
        let (h, g, l) = (by_id(&doc, "h"), by_id(&doc, "g"), by_id(&doc, "l"));
        let s1 = element(&mut doc, "slot", &[]);
        let f1 = text(&mut doc, "fallback");
        doc.append_child(s1, f1).unwrap();
        shadow(&mut doc, h, &[s1]);
        assert_eq!(flat(&doc, s1), vec![f1]);
        assert_eq!(doc.flat_parent(f1), Some(s1));
        assert!(doc.in_flat_tree(f1));
        // With assigned nodes the fallback is not used.
        let s2 = element(&mut doc, "slot", &[]);
        let f2 = element(&mut doc, "i", &[]);
        doc.append_child(s2, f2).unwrap();
        shadow(&mut doc, g, &[s2]);
        assert_eq!(flat(&doc, s2), vec![l]);
        assert_eq!(doc.flat_parent(f2), None);
        assert!(!doc.in_flat_tree(f2));
        // A slot outside a shadow tree is an ordinary element.
        let s3 = element(&mut doc, "slot", &[]);
        let f3 = text(&mut doc, "z");
        doc.append_child(s3, f3).unwrap();
        doc.append_child(l, s3).unwrap();
        assert_eq!(flat(&doc, s3), vec![f3]);
        assert_eq!(doc.flat_parent(f3), Some(s3));
    }

    #[test]
    fn a_slot_passed_on_to_a_nested_host_stays_in_the_flat_tree() {
        // <div h><b x/></div>; h's root: <div ih><slot os/></div>;
        // ih's root: <i><slot is/></i>.
        let mut doc = parse_html("<div id=h><b id=x>pass</b></div>");
        let (h, x) = (by_id(&doc, "h"), by_id(&doc, "x"));
        let ih = element(&mut doc, "div", &[]);
        let os = element(&mut doc, "slot", &[]);
        doc.append_child(ih, os).unwrap();
        shadow(&mut doc, h, &[ih]);
        let i = element(&mut doc, "i", &[]);
        let is = element(&mut doc, "slot", &[]);
        doc.append_child(i, is).unwrap();
        shadow(&mut doc, ih, &[i]);
        assert_eq!(flat(&doc, h), vec![ih]);
        assert_eq!(flat(&doc, ih), vec![i]);
        assert_eq!(flat(&doc, is), vec![os]);
        assert_eq!(flat(&doc, os), vec![x]);
        assert_eq!(doc.flat_parent(x), Some(os));
        assert_eq!(doc.flat_parent(os), Some(is));
        assert_eq!(
            doc.flat_ancestors(x).take(6).collect::<Vec<_>>(),
            vec![os, is, i, ih, h, doc.parent(h).unwrap()]
        );
        assert!(doc.in_flat_tree(x));
    }

    #[test]
    fn manual_slots_follow_assign() {
        let mut doc = parse_html("<div id=h><b id=a>a</b><b id=b>b</b></div>");
        let (h, a, b) = (by_id(&doc, "h"), by_id(&doc, "a"), by_id(&doc, "b"));
        let s = element(&mut doc, "slot", &[]);
        let root = doc
            .attach_shadow(h, init(SlotAssignment::Manual), false)
            .unwrap();
        doc.append_child(root, s).unwrap();
        assert_eq!(flat(&doc, s), Vec::<NodeId>::new());
        assert_eq!(doc.flat_parent(a), None);
        doc.assign_slot_manually(s, &[b, a]);
        assert_eq!(flat(&doc, s), vec![b, a]);
        assert_eq!(doc.flat_parent(a), Some(s));
        doc.assign_slot_manually(s, &[]);
        assert_eq!(doc.flat_parent(a), None);
    }

    #[test]
    fn a_new_assignment_of_a_connected_slot_marks_the_flat_tree_changed() {
        let mut doc = parse_html("<div id=h><b id=a>a</b></div>");
        let (host, light) = (by_id(&doc, "h"), by_id(&doc, "a"));
        let root = doc
            .attach_shadow(host, init(SlotAssignment::Manual), false)
            .unwrap();
        let slot = element(&mut doc, "slot", &[]);
        doc.append_child(root, slot).unwrap();
        assert!(doc.take_shadow_changed(), "the attach");
        doc.assign_slot_manually(slot, &[light]);
        assert!(doc.take_shadow_changed(), "assign()");
        doc.assign_slot_manually(slot, &[light]);
        assert!(!doc.take_shadow_changed(), "the same list");
        // A detached host: nothing to restyle.
        let mut detached = parse_html("");
        let div = element(&mut detached, "div", &[]);
        let child = element(&mut detached, "b", &[]);
        detached.append_child(div, child).unwrap();
        let root = detached
            .attach_shadow(div, init(SlotAssignment::Manual), false)
            .unwrap();
        let slot = element(&mut detached, "slot", &[]);
        detached.append_child(root, slot).unwrap();
        detached.assign_slot_manually(slot, &[child]);
        assert!(!detached.take_shadow_changed());
    }

    #[test]
    fn scopes_of_flat_children() {
        // <div h><b x/></div>; h's root: <div ih><slot os/></div>;
        // ih's root: <slot is/>. A walk from the top: x is in the document
        // tree, although its flat parent os is in h's root.
        let mut doc = parse_html("<div id=h><b id=x>pass</b></div>");
        let (h, x) = (by_id(&doc, "h"), by_id(&doc, "x"));
        let ih = element(&mut doc, "div", &[]);
        let os = element(&mut doc, "slot", &[]);
        doc.append_child(ih, os).unwrap();
        let outer = shadow(&mut doc, h, &[ih]);
        let is = element(&mut doc, "slot", &[]);
        let inner = shadow(&mut doc, ih, &[is]);
        let mut scopes = FlatScopes::default();
        let d = NodeId::DOCUMENT;
        assert_eq!(scopes.of_children(&doc, doc.parent(h).unwrap(), d), d);
        assert_eq!(scopes.of_children(&doc, h, d), outer);
        assert_eq!(scopes.of_children(&doc, ih, outer), inner);
        assert_eq!(scopes.of_children(&doc, is, inner), outer, "os");
        assert_eq!(scopes.of_children(&doc, os, outer), d, "x");
        assert_eq!(scopes.of_children(&doc, x, d), d);
        // A slot whose host the walk did not enter: a walk up.
        let mut fresh = FlatScopes::default();
        assert_eq!(fresh.of_children(&doc, os, outer), d);
        // Without shadow state: the same scope.
        let plain = parse_html("<div id=a></div>");
        let a = by_id(&plain, "a");
        assert_eq!(FlatScopes::default().of_children(&plain, a, d), d);
    }

    #[test]
    fn deep_nesting_needs_no_recursion() {
        let mut doc = parse_html("<div id=h></div>");
        let mut host = by_id(&doc, "h");
        for _ in 0..20_000 {
            let next = element(&mut doc, "div", &[]);
            shadow(&mut doc, host, &[next]);
            host = next;
        }
        assert_eq!(doc.flat_ancestors(host).count(), 20_000 + 3);
        assert!(doc.in_flat_tree(host));
    }
}
