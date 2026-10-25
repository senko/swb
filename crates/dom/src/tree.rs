//! The DOM tree: an arena of nodes addressed by [`NodeId`].
//!
//! Nodes are stored in a `Vec` owned by [`Document`]. Each node links to its
//! parent, its first and last child, and its previous and next sibling. A
//! removed node stays in the arena (detached); IDs are never reused, so a
//! `NodeId` stays valid for the lifetime of the document.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU64, Ordering};

use html5ever::{LocalName, QualName, local_name, ns};

use crate::id_index::IdIndex;
use crate::journal::{Journal, MutationSink};

/// Index of a node in its [`Document`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
///
/// It holds the index plus one, so that `Option<NodeId>` takes four bytes:
/// each node has five such links.
pub struct NodeId(NonZeroU32);

impl NodeId {
    /// The document node. It is always the first node in the arena.
    pub const DOCUMENT: NodeId = NodeId(NonZeroU32::MIN);

    /// The index of this node in the arena.
    pub fn index(self) -> usize {
        self.0.get() as usize - 1
    }

    /// Creates an ID from a raw index, for example one received from the
    /// automation API. Use [`Document::get`] to check that it is valid.
    pub fn from_index(index: usize) -> Option<NodeId> {
        let raw = u32::try_from(index.checked_add(1)?).ok()?;
        NonZeroU32::new(raw).map(NodeId)
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.index())
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
    pub(crate) attrs: Vec<Attribute>,
    /// For `<template>` elements: the document fragment with the contents.
    pub template_contents: Option<NodeId>,
    /// For form-associated elements: the form that the parser associated
    /// with the element (the form element pointer), also when the element
    /// is not a descendant of the form (`<table><form><tr><td><input>`).
    /// <https://html.spec.whatwg.org/multipage/parsing.html#create-an-element-for-the-token>
    pub parser_form: Option<NodeId>,
    /// For `script` elements: the parser set the HTML "already started"
    /// flag (a script of a fragment, or one that the end of the input cut
    /// off), so the script never runs. The bindings keep the rest of the
    /// state of a script element (feature 5c of M8).
    pub script_already_started: bool,
}

impl ElementData {
    /// Creates element data.
    pub fn new(name: QualName, attrs: Vec<Attribute>) -> Self {
        ElementData {
            name,
            attrs,
            template_contents: None,
            parser_form: None,
            script_already_started: false,
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

    /// The attribute whose qualified name (`prefix:local`, or `local`
    /// without a prefix) is `name`, case-sensitive.
    /// <https://dom.spec.whatwg.org/#concept-element-attributes-get-by-name>
    pub fn attr_by_qualified_name(&self, name: &str) -> Option<&Attribute> {
        self.attrs.iter().find(|a| match &a.name.prefix {
            None => &*a.name.local == name,
            Some(prefix) => name
                .strip_prefix(&**prefix)
                .and_then(|rest| rest.strip_prefix(':'))
                .is_some_and(|local| local == &*a.name.local),
        })
    }

    /// The attribute with this namespace (empty for none) and local name.
    /// <https://dom.spec.whatwg.org/#concept-element-attributes-get-by-namespace>
    pub fn attr_by_namespace(&self, namespace: &str, local: &str) -> Option<&Attribute> {
        self.attrs
            .iter()
            .find(|a| &*a.name.ns == namespace && &*a.name.local == local)
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

    /// The index of the attribute with this namespace and local name.
    pub(crate) fn attr_index(&self, name: &QualName) -> Option<usize> {
        self.attrs
            .iter()
            .position(|a| a.name.ns == name.ns && a.name.local == name.local)
    }

    /// The `id` attribute, if present and not empty.
    pub fn id(&self) -> Option<&str> {
        // Atom comparison: cheaper than `attr("id")`, and the change
        // functions ask for it at each insertion.
        self.attrs
            .iter()
            .find(|a| a.name.ns == ns!() && a.name.local == local_name!("id"))
            .map(|a| a.value.as_str())
            .filter(|id| !id.is_empty())
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

/// A node: its tree links, its node document and its data.
#[derive(Clone, Debug)]
pub struct Node {
    pub(crate) parent: Option<NodeId>,
    pub(crate) first_child: Option<NodeId>,
    pub(crate) last_child: Option<NodeId>,
    pub(crate) prev_sibling: Option<NodeId>,
    pub(crate) next_sibling: Option<NodeId>,
    /// The number of children, and of element children. The link and
    /// unlink primitives keep them, so that a script can read the length
    /// of `childNodes` and `children` without a walk.
    pub(crate) child_count: u32,
    pub(crate) element_child_count: u32,
    /// The node document: a document node of the arena. A document node is
    /// its own node document.
    pub(crate) owner: NodeId,
    /// The node's data.
    pub data: NodeData,
}

impl Node {
    pub(crate) fn new(data: NodeData, owner: NodeId) -> Self {
        Node {
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            child_count: 0,
            element_child_count: 0,
            owner,
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

/// The limits of an arena (ADR 0028 section 16). The change functions
/// return [`DomError::NodeLimit`](crate::DomError::NodeLimit) or
/// [`DomError::ByteLimit`](crate::DomError::ByteLimit) instead of going
/// past them; the parser stops at them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most nodes that the arena may hold (all nodes ever created,
    /// since nodes are never freed).
    pub max_nodes: usize,
    /// The most bytes of character data (text, comments, processing
    /// instructions, doctypes), names and attributes that the arena may
    /// hold. An attribute counts its name, its value and the size of its
    /// record; an element counts its name.
    pub max_text_bytes: usize,
}

/// The default node limit: 1,048,576 nodes per arena.
pub const MAX_NODES: usize = 1 << 20;

/// The default limit of text and attribute bytes: 256 MiB per arena.
pub const MAX_TEXT_BYTES: usize = 256 << 20;

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_nodes: MAX_NODES,
            max_text_bytes: MAX_TEXT_BYTES,
        }
    }
}

/// The source of arena identities ([`Document::arena_id`]).
static NEXT_ARENA_ID: AtomicU64 = AtomicU64::new(1);

/// An arena of nodes: the HTML document of [`NodeId::DOCUMENT`] and the
/// other documents and fragments that scripts create (ADR 0028 section 3).
///
/// All changes go through the change functions (`mutation.rs`), which keep
/// the journal, the generation, the id index and the limits.
pub struct Document {
    pub(crate) nodes: Vec<Node>,
    /// The quirks mode of the document of [`NodeId::DOCUMENT`], set by the
    /// parser.
    pub quirks_mode: QuirksMode,
    /// The scripting flag of the document: true if the document was
    /// created with JavaScript on (ADR 0028 section 15). The parser reads it
    /// as its scripting flag; media queries, the serializer, media
    /// controls and `<canvas>` follow it. Set by the parser; it does not
    /// change during the life of the document.
    scripting: bool,
    /// The changes since the last drain.
    pub(crate) journal: Journal,
    /// False while changes are not recorded (the journal then becomes
    /// incomplete).
    pub(crate) journaling: bool,
    /// Grows with every change.
    pub(crate) generation: u64,
    /// Like `generation`, but only changes of the child lists bump it.
    pub(crate) structure_generation: u64,
    /// `id` values of connected elements; built at the first lookup, which
    /// takes `&self`.
    pub(crate) ids: RefCell<IdIndex>,
    /// The bytes of character data, names and attributes in the arena
    /// (see [`Limits::max_text_bytes`]).
    pub(crate) text_bytes: usize,
    pub(crate) limits: Limits,
    /// True once the parser gave an element a form element pointer
    /// ([`ElementData::parser_form`]); the change functions then clear the
    /// pointers of moved subtrees (HTML "reset the form owner").
    pub(crate) parser_forms: bool,
    pub(crate) mutation_sink: Option<Box<dyn MutationSink>>,
    /// The template contents owner document of each document node that
    /// has one (created when the document gets its first template). An
    /// inert owner document maps to itself.
    pub(crate) template_owners: HashMap<NodeId, NodeId>,
    /// Pairs of a template contents fragment and its template element
    /// (the fragment's host, for the host-including ancestor check).
    pub(crate) template_hosts: HashMap<NodeId, NodeId>,
    /// The quirks mode of the other documents that are not no-quirks
    /// (`DOMParser` documents without a doctype).
    other_quirks: HashMap<NodeId, QuirksMode>,
    /// An empty detached fragment that the markup setters give back for the
    /// next fragment parse, and the stand-in `body` context of
    /// `insertAdjacentHTML`: nodes are never freed, so a loop of `innerHTML`
    /// writes reuses them.
    pub(crate) scratch_fragment: Option<NodeId>,
    pub(crate) scratch_body: Option<NodeId>,
    arena_id: u64,
}

impl fmt::Debug for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Document")
            .field("nodes", &self.nodes.len())
            .field("quirks_mode", &self.quirks_mode)
            .field("scripting", &self.scripting)
            .field("generation", &self.generation)
            .field("structure_generation", &self.structure_generation)
            .finish_non_exhaustive()
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Creates an empty document that contains only the document node,
    /// with the default limits.
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    /// Creates an empty document with these limits.
    pub fn with_limits(limits: Limits) -> Self {
        Document {
            nodes: vec![Node::new(NodeData::Document, NodeId::DOCUMENT)],
            quirks_mode: QuirksMode::NoQuirks,
            scripting: false,
            journal: Journal::default(),
            journaling: true,
            generation: 0,
            structure_generation: 0,
            ids: RefCell::default(),
            text_bytes: 0,
            limits,
            parser_forms: false,
            mutation_sink: None,
            template_owners: HashMap::new(),
            template_hosts: HashMap::new(),
            other_quirks: HashMap::new(),
            scratch_fragment: None,
            scratch_body: None,
            arena_id: NEXT_ARENA_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// A number that identifies this arena among all arenas of the process.
    /// The parser checks that it resumes with the arena that it started
    /// with.
    pub fn arena_id(&self) -> u64 {
        self.arena_id
    }

    /// The limits of the arena.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The bytes of character data, names and attributes in the arena
    /// (see [`Limits::max_text_bytes`]).
    pub fn text_bytes(&self) -> usize {
        self.text_bytes
    }

    /// True if the arena has reached one of its limits.
    pub fn at_limit(&self) -> bool {
        self.nodes.len() >= self.limits.max_nodes || self.text_bytes >= self.limits.max_text_bytes
    }

    /// The generation: a counter that grows with every change. Derived
    /// data records the generation it was built for.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The structure generation: a counter that grows with every change
    /// of a child list (insertion, removal, replacement), and not with
    /// changes of attributes or text. Data that depends only on the
    /// sibling links, such as `children`, records this one.
    pub fn structure_generation(&self) -> u64 {
        self.structure_generation
    }

    /// The changes since the last [`take_journal`](Self::take_journal).
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Drains the journal: returns the changes so far and starts an empty
    /// journal.
    pub fn take_journal(&mut self) -> Journal {
        std::mem::take(&mut self.journal)
    }

    /// Turns the recording of changes on or off (on for a new document).
    /// While it is off, the journal only notes that changes are missing
    /// ([`Journal::incomplete`]); the mutation sink still gets every change.
    pub fn set_journaling(&mut self, on: bool) {
        self.journaling = on;
    }

    /// Installs the sink that receives each change as it happens, and
    /// returns the previous one. `None` removes it.
    pub fn set_mutation_sink(
        &mut self,
        sink: Option<Box<dyn MutationSink>>,
    ) -> Option<Box<dyn MutationSink>> {
        std::mem::replace(&mut self.mutation_sink, sink)
    }

    /// The installed mutation sink.
    pub fn mutation_sink_mut(&mut self) -> Option<&mut (dyn MutationSink + 'static)> {
        self.mutation_sink.as_deref_mut()
    }

    /// True if scripting is enabled for the document (the document was
    /// created with JavaScript on).
    /// <https://html.spec.whatwg.org/multipage/webappapis.html#concept-n-script>
    pub fn scripting(&self) -> bool {
        self.scripting
    }

    /// True if scripting is enabled for `node` (HTML: its node document
    /// has a browsing context and scripting is on). Only the document of
    /// [`NodeId::DOCUMENT`] has one; the documents that scripts make
    /// (`DOMParser`, `createHTMLDocument`, the inert documents of template
    /// contents) do not. The fragment parser reads it for a `noscript`
    /// element and the serializer for the text of one.
    pub fn scripting_for(&self, node: NodeId) -> bool {
        self.scripting && self.node_document(node) == NodeId::DOCUMENT
    }

    /// The quirks mode of the document node `document`.
    pub fn quirks_mode_of(&self, document: NodeId) -> QuirksMode {
        if document == NodeId::DOCUMENT {
            self.quirks_mode
        } else {
            self.other_quirks
                .get(&document)
                .copied()
                .unwrap_or(QuirksMode::NoQuirks)
        }
    }

    /// Sets the quirks mode of a document other than the one of
    /// [`NodeId::DOCUMENT`] (the parser sets that one).
    pub(crate) fn set_other_quirks_mode(&mut self, document: NodeId, mode: QuirksMode) {
        if mode == QuirksMode::NoQuirks {
            self.other_quirks.remove(&document);
        } else {
            self.other_quirks.insert(document, mode);
        }
    }

    /// Sets the scripting flag. Only the parser (and tests that build a
    /// document by hand) call it, before the document is used.
    pub fn set_scripting(&mut self, enabled: bool) {
        self.scripting = enabled;
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

    pub(crate) fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    /// The element data of a node, if it is an element.
    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        self.node(id).as_element()
    }

    /// Mutable element data of a node, if it is an element. Not public:
    /// changes of attributes go through the change functions.
    pub(crate) fn element_mut(&mut self, id: NodeId) -> Option<&mut ElementData> {
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

    /// The number of children of a node (kept, no walk).
    pub fn child_count(&self, id: NodeId) -> usize {
        self.node(id).child_count as usize
    }

    /// The number of element children of a node (kept, no walk).
    pub fn element_child_count(&self, id: NodeId) -> usize {
        self.node(id).element_child_count as usize
    }

    /// The node that follows `current` in tree order inside the subtree of
    /// `root` (`None` after the last descendant of `root`).
    pub fn next_in_tree_order(&self, current: NodeId, root: NodeId) -> Option<NodeId> {
        self.successor(current, root)
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

    /// The summary of `details`: its first `summary` child element. `None`
    /// if `details` is not an HTML `details` element or has no summary
    /// (it then shows a default one).
    /// <https://html.spec.whatwg.org/multipage/interactive-elements.html#the-details-element>
    pub fn details_summary(&self, details: NodeId) -> Option<NodeId> {
        if !self.is_html_element(details, &local_name!("details")) {
            return None;
        }
        self.element_children(details)
            .find(|&c| self.is_html_element(c, &local_name!("summary")))
    }

    /// The `details` element of which `summary` is the summary: its
    /// parent, if `summary` is the first `summary` child of an HTML
    /// `details` element.
    pub fn details_of_summary(&self, summary: NodeId) -> Option<NodeId> {
        let parent = self.parent(summary)?;
        (self.details_summary(parent) == Some(summary)).then_some(parent)
    }

    /// True if `node` is in the content of a closed `details` element: a
    /// descendant of a `details` without the `open` attribute that is
    /// not (in) its summary. The content of a closed `details` is laid
    /// out but not rendered.
    pub fn in_closed_details_content(&self, node: NodeId) -> bool {
        let mut child = node;
        while let Some(parent) = self.parent(child) {
            if let Some(details) = self.element(parent)
                && details.is_html_named(&local_name!("details"))
                && !details.has_attr("open")
                // Only a `summary` can be the summary (the check avoids
                // a search among the children of `details` otherwise).
                && !(self.is_html_element(child, &local_name!("summary"))
                    && self.details_summary(parent) == Some(child))
            {
                return true;
            }
            child = parent;
        }
        false
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

    /// The concatenated data of the text node descendants of `id`, except
    /// those inside a descendant `script` element of the HTML or SVG
    /// namespace (the "child text content" of an `option`).
    /// <https://html.spec.whatwg.org/multipage/form-elements.html#concept-option-label>
    pub fn text_content_without_scripts(&self, id: NodeId) -> String {
        let mut out = String::new();
        let mut stack: Vec<NodeId> = self.children(id).collect();
        stack.reverse();
        while let Some(n) = stack.pop() {
            if let Some(t) = self.node(n).as_text() {
                out.push_str(t);
                continue;
            }
            let script = self.element(n).is_some_and(|e| {
                &**e.local_name() == "script" && (e.is_html() || e.name.ns == ns!(svg))
            });
            if !script {
                let start = stack.len();
                stack.extend(self.children(n));
                stack[start..].reverse();
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

    /// The first element in tree order with this `id` in the document of
    /// [`NodeId::DOCUMENT`] (from the id index).
    pub fn element_by_id(&self, id: &str) -> Option<NodeId> {
        self.get_element_by_id(NodeId::DOCUMENT, id)
    }

    /// The first element with each `id` attribute value, in one pass: the
    /// index for many lookups (`element_by_id` scans the tree).
    pub fn element_ids(&self) -> HashMap<&str, NodeId> {
        let mut ids = HashMap::new();
        for node in self.descendants(NodeId::DOCUMENT) {
            if let Some(id) = self.element(node).and_then(ElementData::id) {
                ids.entry(id).or_insert(node);
            }
        }
        ids
    }

    /// The node document of a node: the document node that the node
    /// belongs to (a document node is its own node document).
    /// <https://dom.spec.whatwg.org/#concept-node-document>
    pub fn node_document(&self, id: NodeId) -> NodeId {
        self.node(id).owner
    }

    /// The root of the tree that contains `id`: its last ancestor, or the
    /// node itself.
    pub fn root(&self, id: NodeId) -> NodeId {
        self.ancestors(id).last().unwrap_or(id)
    }

    /// True if the root of the tree that contains `id` is a document node
    /// (DOM "connected"; swb has no shadow trees).
    pub fn is_connected(&self, id: NodeId) -> bool {
        matches!(self.node(self.root(id)).data, NodeData::Document)
    }

    /// True if `a` comes before `b` in tree order. Both must be in the same
    /// tree; otherwise the result is false.
    /// <https://dom.spec.whatwg.org/#concept-tree-preceding>
    pub fn precedes(&self, a: NodeId, b: NodeId) -> bool {
        if a == b {
            return false;
        }
        // The inclusive ancestors of each, root first.
        let path = |n: NodeId| {
            let mut path: Vec<NodeId> = std::iter::once(n).chain(self.ancestors(n)).collect();
            path.reverse();
            path
        };
        let (pa, pb) = (path(a), path(b));
        if pa.first() != pb.first() {
            return false;
        }
        let common = pa.iter().zip(&pb).take_while(|(x, y)| x == y).count();
        match (pa.get(common), pb.get(common)) {
            // `a` is an ancestor of `b`.
            (None, Some(_)) => true,
            // Siblings under the last common ancestor: is `ca` before `cb`?
            (Some(&ca), Some(&cb)) => {
                std::iter::successors(self.node(ca).next_sibling, |&n| self.node(n).next_sibling)
                    .any(|n| n == cb)
            }
            // `b` is an ancestor of `a`, or `a == b`.
            (_, None) => false,
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

impl Document {
    /// The tree-order successor of `current` that is not above `root`.
    fn successor(&self, current: NodeId, root: NodeId) -> Option<NodeId> {
        if let Some(child) = self.node(current).first_child {
            return Some(child);
        }
        // Go up until a node with a next sibling is found, but never
        // above the root.
        let mut n = current;
        loop {
            if n == root {
                return None;
            }
            let node = self.node(n);
            if let Some(sibling) = node.next_sibling {
                return Some(sibling);
            }
            n = node.parent?;
        }
    }
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc.successor(current, self.root);
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
    fn child_counts_follow_the_tree_changes() {
        let mut doc = crate::parse_html("<div id=d>a<p id=p1></p>b<p id=p2></p></div>");
        let d = doc.element_by_id("d").expect("d");
        let p1 = doc.element_by_id("p1").expect("p1");
        assert_eq!((doc.child_count(d), doc.element_child_count(d)), (4, 2));
        doc.remove(p1);
        assert_eq!((doc.child_count(d), doc.element_child_count(d)), (3, 1));
        let text = doc.create_text("x").expect("text");
        doc.append_child(d, text).expect("append");
        doc.append_child(d, p1).expect("append");
        assert_eq!((doc.child_count(d), doc.element_child_count(d)), (5, 2));
        assert_eq!(doc.children(d).count(), doc.child_count(d));
        let root = doc.document_element().expect("root");
        let expected: Vec<NodeId> = doc.descendants(root).collect();
        let mut walked = vec![root];
        while let Some(n) = doc.next_in_tree_order(*walked.last().expect("one"), root) {
            walked.push(n);
        }
        assert_eq!(walked, expected);
    }

    #[test]
    fn details_summaries() {
        let doc = crate::parse_html(
            "<details id=a><p id=p>x</p><summary id=s1>1</summary><summary id=s2>2</summary>\
             <div id=d><span id=inner></span></div></details>\
             <details id=b open><summary id=s3>3</summary><i id=i>x</i></details>\
             <summary id=lone>x</summary>",
        );
        let id = |name: &str| doc.element_by_id(name).expect("element");
        assert_eq!(doc.details_summary(id("a")), Some(id("s1")));
        assert_eq!(doc.details_summary(id("p")), None);
        assert_eq!(doc.details_of_summary(id("s1")), Some(id("a")));
        assert_eq!(doc.details_of_summary(id("s2")), None);
        assert_eq!(doc.details_of_summary(id("lone")), None);
        let hidden = |name: &str| doc.in_closed_details_content(id(name));
        // The summaries other than the first and all other children are in
        // the content; the first summary is not.
        assert!(hidden("p") && hidden("s2") && hidden("inner") && hidden("d"));
        assert!(!hidden("s1") && !hidden("a") && !hidden("lone"));
        assert!(!hidden("s3") && !hidden("i"));
    }

    #[test]
    fn traversal() {
        let doc = crate::parse_html("<html><body><a id=a><c id=c></c>xy</a><b id=b></b>");
        let id = |name: &str| doc.element_by_id(name).expect("element");
        let (a, b, c) = (id("a"), id("b"), id("c"));
        let body = doc.body().expect("body");
        assert_eq!(doc.children(body).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(doc.text_content(body), "xy");
        assert_eq!(doc.descendants(a).count(), 3, "a, c and the text");
        assert_eq!(doc.ancestors(a).last(), Some(NodeId::DOCUMENT));
        assert_eq!(doc.next_sibling_element(a), Some(b));
        assert_eq!(doc.prev_sibling_element(b), Some(a));
        assert!(doc.precedes(a, c) && doc.precedes(c, b) && doc.precedes(a, b));
        assert!(!doc.precedes(b, a) && !doc.precedes(c, a) && !doc.precedes(a, a));
        assert!(doc.is_connected(c));
        assert_eq!(doc.root(c), NodeId::DOCUMENT);
    }

    #[test]
    fn attributes_and_classes() {
        let attr = |name: &str, value: &str| Attribute {
            name: QualName::new(None, ns!(), LocalName::from(name)),
            value: value.to_owned(),
        };
        let e = ElementData::new(
            html("div"),
            vec![attr("class", " a\tb  c "), attr("id", "main")],
        );
        assert!(e.has_class("b"));
        assert!(!e.has_class("d"));
        assert_eq!(e.classes().collect::<Vec<_>>(), vec!["a", "b", "c"]);
        assert_eq!(e.id(), Some("main"));
        let e = ElementData::new(html("div"), vec![attr("id", "")]);
        assert_eq!(e.id(), None);
        assert!(e.has_attr("id"));
    }
}
