//! The DOM tree: an arena of nodes addressed by [`NodeId`].
//!
//! Nodes are stored in a `Vec` owned by [`Document`]. Each node links to its
//! parent, its first and last child, and its previous and next sibling. A
//! removed node stays in the arena (detached); IDs are never reused, so a
//! `NodeId` stays valid for the lifetime of the document.

use std::fmt;

use html5ever::{LocalName, QualName, local_name, ns};

/// Index of a node in its [`Document`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// The document node. It is always the first node in the arena.
    pub const DOCUMENT: NodeId = NodeId(0);

    /// The index of this node in the arena.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Creates an ID from a raw index, for example one received from the
    /// automation API. Use [`Document::get`] to check that it is valid.
    pub fn from_index(index: usize) -> Option<NodeId> {
        u32::try_from(index).ok().map(NodeId)
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// The HTML document's quirks mode, set by the parser from the doctype.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum QuirksMode {
    /// Standards mode.
    #[default]
    NoQuirks,
    /// Limited-quirks ("almost standards") mode.
    LimitedQuirks,
    /// Quirks mode.
    Quirks,
}

/// One attribute of an element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    /// The qualified name. For HTML attributes the namespace is empty.
    pub name: QualName,
    /// The value.
    pub value: String,
}

/// Data of an element node.
#[derive(Clone, Debug)]
pub struct ElementData {
    /// The element's qualified name.
    pub name: QualName,
    attrs: Vec<Attribute>,
    /// For `<template>` elements: the document fragment with the contents.
    pub template_contents: Option<NodeId>,
    /// For form-associated elements: the form that the parser associated
    /// with the element (the form element pointer), also when the element
    /// is not a descendant of the form (`<table><form><tr><td><input>`).
    /// <https://html.spec.whatwg.org/multipage/parsing.html#create-an-element-for-the-token>
    pub parser_form: Option<NodeId>,
}

impl ElementData {
    /// Creates element data.
    pub fn new(name: QualName, attrs: Vec<Attribute>) -> Self {
        ElementData {
            name,
            attrs,
            template_contents: None,
            parser_form: None,
        }
    }

    /// The local name, for example `div`.
    pub fn local_name(&self) -> &LocalName {
        &self.name.local
    }

    /// True if this is an element in the HTML namespace.
    pub fn is_html(&self) -> bool {
        self.name.ns == ns!(html)
    }

    /// True if this is an HTML element with the given local name.
    pub fn is_html_named(&self, local: &LocalName) -> bool {
        self.is_html() && self.name.local == *local
    }

    /// All attributes, in source order.
    pub fn attributes(&self) -> &[Attribute] {
        &self.attrs
    }

    /// The value of the attribute with this local name and no namespace.
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name.ns == ns!() && &*a.name.local == local)
            .map(|a| a.value.as_str())
    }

    /// The `href` of an SVG element: the attribute `href`, or else
    /// `xlink:href` (SVG 2 §6.1: `href` wins).
    pub fn svg_href(&self) -> Option<&str> {
        self.attr("href").or_else(|| {
            self.attrs
                .iter()
                .find(|a| a.name.ns == ns!(xlink) && &*a.name.local == "href")
                .map(|a| a.value.as_str())
        })
    }

    /// True if the attribute exists (with no namespace).
    pub fn has_attr(&self, local: &str) -> bool {
        self.attr(local).is_some()
    }

    /// Sets an attribute (no namespace), replacing an existing value.
    pub fn set_attr(&mut self, local: &str, value: &str) {
        if let Some(attr) = self
            .attrs
            .iter_mut()
            .find(|a| a.name.ns == ns!() && &*a.name.local == local)
        {
            value.clone_into(&mut attr.value);
        } else {
            self.attrs.push(Attribute {
                name: QualName::new(None, ns!(), LocalName::from(local)),
                value: value.to_owned(),
            });
        }
    }

    /// Adds an attribute unless one with the same qualified name exists.
    pub(crate) fn add_attr_if_missing(&mut self, attr: Attribute) {
        if !self.attrs.iter().any(|a| a.name == attr.name) {
            self.attrs.push(attr);
        }
    }

    /// Removes an attribute (no namespace). Returns true if it existed.
    pub fn remove_attr(&mut self, local: &str) -> bool {
        let before = self.attrs.len();
        self.attrs
            .retain(|a| !(a.name.ns == ns!() && &*a.name.local == local));
        self.attrs.len() != before
    }

    /// The `id` attribute, if present and not empty.
    pub fn id(&self) -> Option<&str> {
        self.attr("id").filter(|id| !id.is_empty())
    }

    /// The tokens of the `class` attribute.
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.attr("class")
            .unwrap_or("")
            .split(is_html_whitespace)
            .filter(|c| !c.is_empty())
    }

    /// True if the `class` attribute contains `name` (case-sensitive).
    pub fn has_class(&self, name: &str) -> bool {
        self.classes().any(|c| c == name)
    }
}

/// ASCII whitespace as defined by the HTML standard.
pub fn is_html_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0C' | '\r')
}

/// The kind-specific data of a node.
#[derive(Clone, Debug)]
pub enum NodeData {
    /// The document node (the root of the tree).
    Document,
    /// A document fragment, used for `<template>` contents.
    DocumentFragment,
    /// A doctype.
    Doctype {
        /// The doctype name, for example `html`.
        name: String,
        /// The public identifier.
        public_id: String,
        /// The system identifier.
        system_id: String,
    },
    /// An element.
    Element(ElementData),
    /// A text node.
    Text(String),
    /// A comment.
    Comment(String),
    /// A processing instruction.
    ProcessingInstruction {
        /// The target.
        target: String,
        /// The data.
        data: String,
    },
}

/// A node: its tree links and its data.
#[derive(Clone, Debug)]
pub struct Node {
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    last_child: Option<NodeId>,
    prev_sibling: Option<NodeId>,
    next_sibling: Option<NodeId>,
    /// The node's data.
    pub data: NodeData,
}

impl Node {
    fn new(data: NodeData) -> Self {
        Node {
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            data,
        }
    }

    /// The parent node.
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// The first child.
    pub fn first_child(&self) -> Option<NodeId> {
        self.first_child
    }

    /// The last child.
    pub fn last_child(&self) -> Option<NodeId> {
        self.last_child
    }

    /// The previous sibling.
    pub fn prev_sibling(&self) -> Option<NodeId> {
        self.prev_sibling
    }

    /// The next sibling.
    pub fn next_sibling(&self) -> Option<NodeId> {
        self.next_sibling
    }

    /// The element data, if this is an element.
    pub fn as_element(&self) -> Option<&ElementData> {
        match &self.data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    /// The text, if this is a text node.
    pub fn as_text(&self) -> Option<&str> {
        match &self.data {
            NodeData::Text(t) => Some(t),
            _ => None,
        }
    }

    /// True if this is an element.
    pub fn is_element(&self) -> bool {
        matches!(self.data, NodeData::Element(_))
    }
}

/// An HTML document.
#[derive(Clone, Debug)]
pub struct Document {
    nodes: Vec<Node>,
    /// The quirks mode, set by the parser.
    pub quirks_mode: QuirksMode,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Creates an empty document that contains only the document node.
    pub fn new() -> Self {
        Document {
            nodes: vec![Node::new(NodeData::Document)],
            quirks_mode: QuirksMode::NoQuirks,
        }
    }

    /// The number of nodes in the arena, including detached nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Always false: a document contains at least the document node.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The node with this ID.
    ///
    /// # Panics
    ///
    /// Panics if the ID does not belong to this document. IDs come from this
    /// document's own methods, so this is an internal invariant.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// The node with this ID, or `None` if the ID is out of range.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    /// The element data of a node, if it is an element.
    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        self.node(id).as_element()
    }

    /// Mutable element data of a node, if it is an element.
    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut ElementData> {
        match &mut self.node_mut(id).data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    /// The parent of a node.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    /// The parent of a node if the parent is an element.
    pub fn parent_element(&self, id: NodeId) -> Option<NodeId> {
        self.parent(id).filter(|&p| self.node(p).is_element())
    }

    /// Iterates over the children of a node.
    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            doc: self,
            next: self.node(id).first_child,
        }
    }

    /// Iterates over the element children of a node.
    pub fn element_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id).filter(|&c| self.node(c).is_element())
    }

    /// Iterates over the ancestors of a node, nearest first. Does not include
    /// the node itself.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.parent(id), |&n| self.parent(n))
    }

    /// Iterates over a node and all its descendants in tree order.
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants {
            doc: self,
            root: id,
            next: Some(id),
        }
    }

    /// The previous sibling that is an element.
    pub fn prev_sibling_element(&self, id: NodeId) -> Option<NodeId> {
        std::iter::successors(self.node(id).prev_sibling, |&n| self.node(n).prev_sibling)
            .find(|&n| self.node(n).is_element())
    }

    /// The next sibling that is an element.
    pub fn next_sibling_element(&self, id: NodeId) -> Option<NodeId> {
        std::iter::successors(self.node(id).next_sibling, |&n| self.node(n).next_sibling)
            .find(|&n| self.node(n).is_element())
    }

    /// The first child that is an element.
    pub fn first_element_child(&self, id: NodeId) -> Option<NodeId> {
        self.element_children(id).next()
    }

    /// The root element (`<html>`), if any.
    pub fn document_element(&self) -> Option<NodeId> {
        self.first_element_child(NodeId::DOCUMENT)
    }

    /// The `<head>` element: the first `head` child of the root element.
    pub fn head(&self) -> Option<NodeId> {
        let html = self.document_element()?;
        self.element_children(html)
            .find(|&c| self.is_html_element(c, &local_name!("head")))
    }

    /// The `<body>` element: the first `body` (or `frameset`) child of the
    /// root element.
    pub fn body(&self) -> Option<NodeId> {
        let html = self.document_element()?;
        self.element_children(html).find(|&c| {
            self.is_html_element(c, &local_name!("body"))
                || self.is_html_element(c, &local_name!("frameset"))
        })
    }

    /// True if the node is an HTML element with this local name.
    pub fn is_html_element(&self, id: NodeId, local: &LocalName) -> bool {
        self.element(id).is_some_and(|e| e.is_html_named(local))
    }

    /// The concatenated text of all descendant text nodes.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        for n in self.descendants(id) {
            if let Some(t) = self.node(n).as_text() {
                out.push_str(t);
            }
        }
        out
    }

    /// The first element in tree order (under `root`) that satisfies `pred`.
    pub fn find_element(
        &self,
        root: NodeId,
        mut pred: impl FnMut(&ElementData) -> bool,
    ) -> Option<NodeId> {
        self.descendants(root)
            .find(|&n| self.element(n).is_some_and(&mut pred))
    }

    /// The first element with this `id` attribute.
    pub fn element_by_id(&self, id: &str) -> Option<NodeId> {
        self.find_element(NodeId::DOCUMENT, |e| e.id() == Some(id))
    }

    /// The first element with each `id` attribute value, in one pass: the
    /// index for many lookups (`element_by_id` scans the tree).
    pub fn element_ids(&self) -> std::collections::HashMap<&str, NodeId> {
        let mut ids = std::collections::HashMap::new();
        for node in self.descendants(NodeId::DOCUMENT) {
            if let Some(id) = self.element(node).and_then(ElementData::id) {
                ids.entry(id).or_insert(node);
            }
        }
        ids
    }

    // ----- Mutation -----

    /// Creates a detached node and returns its ID.
    pub fn create_node(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("fewer than 2^32 nodes"));
        self.nodes.push(Node::new(data));
        id
    }

    /// Creates a detached element.
    pub fn create_element(&mut self, name: QualName, attrs: Vec<Attribute>) -> NodeId {
        self.create_node(NodeData::Element(ElementData::new(name, attrs)))
    }

    /// Creates a detached text node.
    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.create_node(NodeData::Text(text.to_owned()))
    }

    /// Appends `child` as the last child of `parent`. The child is first
    /// removed from its current parent.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        let last = self.node(parent).last_child;
        {
            let c = self.node_mut(child);
            c.parent = Some(parent);
            c.prev_sibling = last;
        }
        match last {
            Some(last) => self.node_mut(last).next_sibling = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
        self.node_mut(parent).last_child = Some(child);
    }

    /// Inserts `child` before `sibling`. The child is first removed from its
    /// current parent. Does nothing if `sibling` has no parent.
    pub fn insert_before(&mut self, sibling: NodeId, child: NodeId) {
        let Some(parent) = self.node(sibling).parent else {
            return;
        };
        self.detach(child);
        let prev = self.node(sibling).prev_sibling;
        {
            let c = self.node_mut(child);
            c.parent = Some(parent);
            c.prev_sibling = prev;
            c.next_sibling = Some(sibling);
        }
        self.node_mut(sibling).prev_sibling = Some(child);
        match prev {
            Some(prev) => self.node_mut(prev).next_sibling = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
    }

    /// Removes a node from its parent. The node and its subtree stay in the
    /// arena, detached.
    pub fn detach(&mut self, id: NodeId) {
        let (parent, prev, next) = {
            let n = self.node(id);
            (n.parent, n.prev_sibling, n.next_sibling)
        };
        let Some(parent) = parent else {
            return;
        };
        match prev {
            Some(prev) => self.node_mut(prev).next_sibling = next,
            None => self.node_mut(parent).first_child = next,
        }
        match next {
            Some(next) => self.node_mut(next).prev_sibling = prev,
            None => self.node_mut(parent).last_child = prev,
        }
        let n = self.node_mut(id);
        n.parent = None;
        n.prev_sibling = None;
        n.next_sibling = None;
    }

    /// Appends text to `parent`: merges with the last child if it is a text
    /// node, otherwise creates a new text node.
    pub fn append_text(&mut self, parent: NodeId, text: &str) {
        if let Some(last) = self.node(parent).last_child
            && let NodeData::Text(existing) = &mut self.node_mut(last).data
        {
            existing.push_str(text);
            return;
        }
        let t = self.create_text(text);
        self.append_child(parent, t);
    }

    /// Inserts text before `sibling`: merges with the previous sibling if it
    /// is a text node, otherwise creates a new text node.
    pub fn insert_text_before(&mut self, sibling: NodeId, text: &str) {
        if let Some(prev) = self.node(sibling).prev_sibling
            && let NodeData::Text(existing) = &mut self.node_mut(prev).data
        {
            existing.push_str(text);
            return;
        }
        let t = self.create_text(text);
        self.insert_before(sibling, t);
    }

    /// Moves all children of `from` to the end of `to`'s children.
    pub fn reparent_children(&mut self, from: NodeId, to: NodeId) {
        while let Some(child) = self.node(from).first_child {
            self.append_child(to, child);
        }
    }
}

/// Iterator over the children of a node. See [`Document::children`].
pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc.node(current).next_sibling;
        Some(current)
    }
}

/// Pre-order iterator over a subtree. See [`Document::descendants`].
pub struct Descendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        let node = self.doc.node(current);
        self.next = if let Some(child) = node.first_child {
            Some(child)
        } else {
            // Go up until a node with a next sibling is found, but never
            // above the root.
            let mut n = current;
            loop {
                if n == self.root {
                    break None;
                }
                let node = self.doc.node(n);
                if let Some(sibling) = node.next_sibling {
                    break Some(sibling);
                }
                match node.parent {
                    Some(p) => n = p,
                    None => break None,
                }
            }
        };
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(local: &str) -> QualName {
        QualName::new(None, ns!(html), LocalName::from(local))
    }

    #[test]
    fn append_and_traverse() {
        let mut doc = Document::new();
        let root = doc.create_element(html("html"), vec![]);
        doc.append_child(NodeId::DOCUMENT, root);
        let a = doc.create_element(html("a"), vec![]);
        let b = doc.create_element(html("b"), vec![]);
        doc.append_child(root, a);
        doc.append_child(root, b);
        doc.append_text(a, "x");
        doc.append_text(a, "y");

        assert_eq!(doc.children(root).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(doc.text_content(root), "xy");
        assert_eq!(doc.children(a).count(), 1, "adjacent text is merged");
        let all: Vec<_> = doc.descendants(NodeId::DOCUMENT).collect();
        assert_eq!(all.len(), 5);
        assert_eq!(
            doc.ancestors(a).collect::<Vec<_>>(),
            vec![root, NodeId::DOCUMENT]
        );
        assert_eq!(doc.next_sibling_element(a), Some(b));
        assert_eq!(doc.prev_sibling_element(b), Some(a));
    }

    #[test]
    fn descendants_stay_within_subtree() {
        let mut doc = Document::new();
        let root = doc.create_element(html("html"), vec![]);
        doc.append_child(NodeId::DOCUMENT, root);
        let a = doc.create_element(html("a"), vec![]);
        let b = doc.create_element(html("b"), vec![]);
        doc.append_child(root, a);
        doc.append_child(root, b);
        let c = doc.create_element(html("c"), vec![]);
        doc.append_child(a, c);
        assert_eq!(doc.descendants(a).collect::<Vec<_>>(), vec![a, c]);
    }

    #[test]
    fn insert_before_and_detach() {
        let mut doc = Document::new();
        let p = doc.create_element(html("p"), vec![]);
        doc.append_child(NodeId::DOCUMENT, p);
        let a = doc.create_text("a");
        let c = doc.create_text("c");
        doc.append_child(p, a);
        doc.append_child(p, c);
        let b = doc.create_element(html("b"), vec![]);
        doc.insert_before(c, b);
        assert_eq!(doc.children(p).collect::<Vec<_>>(), vec![a, b, c]);
        doc.detach(b);
        assert_eq!(doc.children(p).collect::<Vec<_>>(), vec![a, c]);
        assert_eq!(doc.parent(b), None);
        doc.detach(a);
        doc.detach(c);
        assert_eq!(doc.node(p).first_child(), None);
        assert_eq!(doc.node(p).last_child(), None);
    }

    #[test]
    fn attributes_and_classes() {
        let mut e = ElementData::new(html("div"), vec![]);
        e.set_attr("class", " a\tb  c ");
        e.set_attr("id", "main");
        assert!(e.has_class("b"));
        assert!(!e.has_class("d"));
        assert_eq!(e.classes().collect::<Vec<_>>(), vec!["a", "b", "c"]);
        assert_eq!(e.id(), Some("main"));
        e.set_attr("id", "");
        assert_eq!(e.id(), None);
        assert!(e.remove_attr("id"));
        assert!(!e.has_attr("id"));
    }
}
