//! The tree sink: html5ever's tree builder writes into our [`Document`]
//! through the [`TreeSink`] trait, with the change functions of
//! `mutation.rs`, so that the parser's changes are journaled and reach the
//! mutation sink.
//!
//! The sink holds the document only while the parser runs a step; between
//! steps the caller has it (`HtmlParser::resume`).
//!
//! Synchronous custom element construction (ADR 0035 section 8): for an
//! HTML element whose local name the window's registry defines
//! ([`Document::is_defined_name`]), `create_element` makes a placeholder
//! element without attributes, keeps the token's attributes, and asks the
//! token sink (`token_sink.rs`) to pause after the token; the insertion of
//! the placeholder that follows in the same token is recorded, not done.
//! At the pause the script runtime constructs the element, and
//! `HtmlParser::finish_custom_element` gives it to the sink, which maps the
//! placeholder to it (html5ever keeps the placeholder in its stack of open
//! elements; every method resolves its handles through the alias table, a
//! test of a `Cell` while it is empty) and performs the recorded insertion
//! with it. In template contents (whose document has no registry) the
//! placeholder becomes an ordinary element.
//!
//! A script can adopt an element that is open in the parser into another
//! document. The arena keeps the id as an empty detached comment
//! (`transplant.rs`), so the handle stays valid but names no element. The
//! sink drops what the parser would put into such a handle (Chromium keeps
//! parsing into the element in the other document; the arenas are
//! separate, ADR 0035 4c notes), and it gives the tree builder a fixed name
//! for it (`is_gone`).

use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::LazyLock;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::BufferQueue;
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode as HtmlQuirksMode, TreeSink};
use html5ever::{QualName, local_name, ns};

use super::MAX_TREE_DEPTH;
use super::token_sink::PAUSE_CUSTOM;
use crate::tree::{Attribute, Document, ElementData, NodeData, NodeId, QuirksMode};

/// The tree sink. html5ever's trait takes `&self`, so the document is in a
/// `RefCell`. Every method except `elem_name` releases its borrow before it
/// returns. `elem_name` returns a shared borrow; html5ever drops it before
/// it calls a method that mutates the tree.
pub(super) struct Sink {
    pub(super) doc: RefCell<Document>,
    /// Elements that are `MathML` `annotation-xml` integration points.
    integration_points: RefCell<HashSet<NodeId>>,
    /// The parser's input, shared with the parser: at a limit, the sink
    /// empties it, so that the tokenizer stops after the current token.
    input: Rc<BufferQueue>,
    /// True once the document reached a limit of its arena.
    pub(super) stopped: Cell<bool>,
    /// The pause cell (`token_sink.rs`), shared with the token sink and,
    /// while the document parser runs, with the document.
    pub(super) pause: Rc<Cell<u8>>,
    /// The element that waits for its construction.
    pending: RefCell<Option<Pending>>,
    /// Placeholder to constructed element.
    aliases: RefCell<HashMap<NodeId, NodeId>>,
    /// True once `aliases` has an entry.
    has_aliases: Cell<bool>,
}

/// A custom element that waits for its construction: the placeholder that
/// the tree builder holds, the token's attributes and the insertion that the
/// tree builder asked for.
struct Pending {
    placeholder: NodeId,
    attrs: Vec<Attribute>,
    insertion: Option<Insertion>,
}

/// A recorded insertion, with the parent found when it was recorded (HTML
/// finds the adjusted insertion location before it creates the element).
#[derive(Clone, Copy, Debug)]
enum Insertion {
    /// At the end of the parent.
    Append(NodeId),
    /// Into the parent, before the sibling.
    Before(NodeId, NodeId),
}

/// The name that the tree builder gets for a handle whose element left the
/// arena.
static GONE_NAME: LazyLock<QualName> =
    LazyLock::new(|| QualName::new(None, ns!(html), local_name!("div")));

impl Sink {
    pub(super) fn new(doc: Document, input: Rc<BufferQueue>) -> Sink {
        Sink {
            doc: RefCell::new(doc),
            integration_points: RefCell::default(),
            input,
            stopped: Cell::new(false),
            pause: Rc::new(Cell::new(0)),
            pending: RefCell::default(),
            aliases: RefCell::default(),
            has_aliases: Cell::new(false),
        }
    }

    /// The element that `handle` stands for: the constructed element of a
    /// placeholder, else `handle`.
    #[inline]
    fn resolve(&self, handle: NodeId) -> NodeId {
        if !self.has_aliases.get() {
            return handle;
        }
        self.aliases
            .borrow()
            .get(&handle)
            .copied()
            .unwrap_or(handle)
    }

    /// The placeholder of the element that waits for its construction, if
    /// the tree builder asked for its insertion (the parser pauses then).
    pub(super) fn pending_inserted(&self) -> Option<NodeId> {
        self.pending
            .borrow()
            .as_ref()
            .filter(|p| p.insertion.is_some())
            .map(|p| p.placeholder)
    }

    /// The token's attributes of the element that waits for its
    /// construction.
    pub(super) fn pending_attributes(&self) -> Vec<Attribute> {
        self.pending
            .borrow()
            .as_ref()
            .map(|p| p.attrs.clone())
            .unwrap_or_default()
    }

    /// Creates the placeholder of a custom element (the rare path of
    /// `create_element`).
    #[cold]
    fn create_pending(&self, doc: &mut Document, name: QualName, attrs: Vec<Attribute>) -> NodeId {
        let data = NodeData::Element(ElementData::new(name, Vec::new()));
        let placeholder = doc.push_node(data, NodeId::DOCUMENT);
        *self.pending.borrow_mut() = Some(Pending {
            placeholder,
            attrs,
            insertion: None,
        });
        self.pause.set(self.pause.get() | PAUSE_CUSTOM);
        placeholder
    }

    /// Records the insertion of `node` if it is the placeholder of the
    /// element that waits for its construction; true if it did. Into
    /// template contents (no registry), the placeholder becomes an ordinary
    /// element and the caller inserts it.
    #[inline]
    fn record(&self, doc: &mut Document, node: NodeId, insertion: Insertion) -> bool {
        self.pause.get() & PAUSE_CUSTOM != 0 && self.record_pending(doc, node, insertion)
    }

    /// [`record`](Self::record) while an element waits (out of line).
    #[cold]
    fn record_pending(&self, doc: &mut Document, node: NodeId, insertion: Insertion) -> bool {
        let mut pending = self.pending.borrow_mut();
        let Some(p) = pending
            .as_mut()
            .filter(|p| p.placeholder == node && p.insertion.is_none())
        else {
            return false;
        };
        let parent = match insertion {
            Insertion::Append(parent) | Insertion::Before(parent, _) => parent,
        };
        if doc.node_document(parent) == NodeId::DOCUMENT {
            p.insertion = Some(insertion);
            return true;
        }
        let attrs = std::mem::take(&mut p.attrs);
        *pending = None;
        self.pause.set(self.pause.get() & !PAUSE_CUSTOM);
        doc.add_attrs_if_missing(node, attrs);
        false
    }

    /// The element that waits for its construction is not constructed: the
    /// placeholder gets the token's attributes and the recorded insertion
    /// (the token sink calls it when the token cannot end with a pause; the
    /// parser when it stopped at a limit).
    pub(super) fn abandon_pending(&self) {
        let placeholder = self.pending.borrow().as_ref().map(|p| p.placeholder);
        if let Some(placeholder) = placeholder {
            self.finish_pending(&mut self.doc.borrow_mut(), placeholder, None);
        }
    }

    /// Ends the construction of the element of `placeholder` with `element`
    /// (`None`: no construction; the placeholder gets the token's
    /// attributes): the placeholder stands for the element from now on, and
    /// the recorded insertion is performed with it. HTML "insert an element
    /// at the adjusted insertion location" steps 3 to 5 (the caller pushes
    /// and pops the element queue). An insertion that is no longer possible
    /// (a script made the element an ancestor of the parent) is dropped.
    /// <https://html.spec.whatwg.org/multipage/parsing.html#insert-an-element-at-the-adjusted-insertion-location>
    pub(super) fn finish_pending(
        &self,
        doc: &mut Document,
        placeholder: NodeId,
        element: Option<NodeId>,
    ) {
        let Some(pending) = self
            .pending
            .borrow_mut()
            .take_if(|p| p.placeholder == placeholder)
        else {
            return;
        };
        self.pause.set(self.pause.get() & !PAUSE_CUSTOM);
        let node = match element {
            Some(element) if element != placeholder => {
                self.aliases.borrow_mut().insert(placeholder, element);
                self.has_aliases.set(true);
                element
            }
            _ => {
                doc.add_attrs_if_missing(placeholder, pending.attrs);
                placeholder
            }
        };
        let (parent, before) = match pending.insertion {
            None => return,
            Some(Insertion::Append(parent)) => (parent, None),
            // The constructor may have moved the sibling.
            Some(Insertion::Before(parent, sibling)) => (
                parent,
                (doc.parent(sibling) == Some(parent)).then_some(sibling),
            ),
        };
        if let Err(error) = doc.parser_moves(|doc| doc.insert_before(parent, node, before)) {
            log::debug!("the parser did not insert a custom element: {error:?}");
        }
        self.check_limits(doc);
    }

    /// The tree builder appends `node` to a handle that left the arena: the
    /// insertion is dropped. If `node` is the placeholder that waits, its
    /// construction ends without an insertion.
    #[cold]
    fn drop_insertion(&self, doc: &mut Document, node: NodeId) {
        if self.pause.get() & PAUSE_CUSTOM == 0 {
            return;
        }
        let Some(p) = self.pending.borrow_mut().take_if(|p| p.placeholder == node) else {
            return;
        };
        self.pause.set(self.pause.get() & !PAUSE_CUSTOM);
        doc.add_attrs_if_missing(node, p.attrs);
    }

    /// Stops the parse when the document reached a limit: the rest of the
    /// input is dropped, so no more nodes come. The token at hand may
    /// still create a few nodes.
    fn check_limits(&self, doc: &Document) {
        if self.stopped.get() || !doc.at_limit() {
            return;
        }
        let limits = doc.limits();
        log::warn!(
            "HTML parser stopped: the document reached its limit of {} nodes or {} bytes of text and attributes",
            limits.max_nodes,
            limits.max_text_bytes
        );
        self.stopped.set(true);
        while self.input.pop_front().is_some() {}
    }
}

/// The attributes of a token as the tree stores them. The HTML parser gives
/// the `xmlns` attribute of a foreign element an empty prefix; the standard
/// says null (HTML "adjust foreign attributes": `xmlns`, prefix null, local
/// name `xmlns`, XMLNS namespace). An empty prefix would make the qualified
/// name `:xmlns`.
fn convert_attrs(attrs: Vec<html5ever::Attribute>) -> Vec<Attribute> {
    attrs
        .into_iter()
        .map(|mut a| {
            if a.name.prefix.as_ref().is_some_and(|p| p.is_empty()) {
                a.name.prefix = None;
            }
            Attribute {
                name: a.name,
                value: a.value.to_string(),
            }
        })
        .collect()
}

/// True if the element of a handle left the arena (a script adopted it into
/// another document): the arena keeps its id as a detached comment, and the
/// parser never uses a comment as a parent.
#[inline]
fn is_gone(doc: &Document, handle: NodeId) -> bool {
    doc.subtrees_taken && matches!(doc.node(handle).data, NodeData::Comment(_))
}

/// `parent`, or the ancestor of `parent` at depth `MAX_TREE_DEPTH - 1` if a
/// child of `parent` would be deeper than [`MAX_TREE_DEPTH`].
fn depth_limited_parent(doc: &Document, parent: NodeId) -> NodeId {
    let depth = doc.ancestors(parent).count();
    if depth < MAX_TREE_DEPTH {
        return parent;
    }
    doc.ancestors(parent)
        .nth(depth - MAX_TREE_DEPTH)
        .unwrap_or(parent)
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Document;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Document {
        self.doc.into_inner()
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        log::trace!("HTML parse error: {msg}");
    }

    fn get_document(&self) -> NodeId {
        NodeId::DOCUMENT
    }

    /// The name of an element. A placeholder has the name of the element
    /// that took its place (the checks of a constructed element and the
    /// failed fallback keep the local name; the parser's HTML elements have
    /// no prefix), so the handle is not resolved: the tree builder asks for
    /// names at nearly every token.
    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.doc.borrow(), |doc| match doc.element(*target) {
            Some(element) => &element.name,
            None => &GONE_NAME,
        })
    }

    /// Creates an element; an HTML `template` gets its contents fragment
    /// from `push_node`. An element with a defined name gets a placeholder
    /// (see the module documentation).
    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        flags: ElementFlags,
    ) -> NodeId {
        let mut doc = self.doc.borrow_mut();
        // The set first: it is empty (one test) while nothing is defined.
        if doc.is_defined_name(&name.local)
            && name.ns == ns!(html)
            && self.pending.borrow().is_none()
        {
            let id = self.create_pending(&mut doc, name, convert_attrs(attrs));
            self.check_limits(&doc);
            return id;
        }
        let data = NodeData::Element(ElementData::new(name, convert_attrs(attrs)));
        let id = doc.push_node(data, NodeId::DOCUMENT);
        if flags.mathml_annotation_xml_integration_point {
            self.integration_points.borrow_mut().insert(id);
        }
        self.check_limits(&doc);
        id
    }

    /// HTML "already started" of a script that the parser will not run
    /// (fragments, the end of the input inside the script).
    fn mark_script_already_started(&self, node: &NodeId) {
        if let Some(element) = self.doc.borrow_mut().element_mut(self.resolve(*node)) {
            element.script_already_started = true;
        }
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        let mut doc = self.doc.borrow_mut();
        let id = doc.push_node(NodeData::Comment(text.to_string()), NodeId::DOCUMENT);
        self.check_limits(&doc);
        id
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> NodeId {
        let mut doc = self.doc.borrow_mut();
        let id = doc.push_node(
            NodeData::ProcessingInstruction {
                target: target.to_string(),
                data: data.to_string(),
            },
            NodeId::DOCUMENT,
        );
        self.check_limits(&doc);
        id
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        let mut doc = self.doc.borrow_mut();
        let parent = depth_limited_parent(&doc, self.resolve(*parent));
        let gone = is_gone(&doc, parent);
        match child {
            NodeOrText::AppendNode(node) => {
                let node = self.resolve(node);
                if !gone && self.record(&mut doc, node, Insertion::Append(parent)) {
                    return;
                }
                if gone {
                    self.drop_insertion(&mut doc, node);
                } else {
                    doc.parser_moves(|doc| doc.insert(node, parent, None, false));
                }
            }
            NodeOrText::AppendText(text) => {
                if !gone {
                    doc.parser_append_text(parent, &text);
                }
            }
        }
        self.check_limits(&doc);
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.doc.borrow().parent(self.resolve(*element)).is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let mut doc = self.doc.borrow_mut();
        let id = doc.push_node(
            NodeData::Doctype {
                name: name.to_string(),
                public_id: public_id.to_string(),
                system_id: system_id.to_string(),
            },
            NodeId::DOCUMENT,
        );
        doc.insert(id, NodeId::DOCUMENT, None, false);
        self.check_limits(&doc);
    }

    /// The contents of a template element. For a handle that is not a
    /// template (it left the arena), a detached fragment that nothing
    /// reads: what the parser puts into it is dropped.
    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        let mut doc = self.doc.borrow_mut();
        let contents = doc
            .element(self.resolve(*target))
            .and_then(|e| e.template_contents);
        contents.unwrap_or_else(|| doc.push_node(NodeData::DocumentFragment, NodeId::DOCUMENT))
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: HtmlQuirksMode) {
        self.doc.borrow_mut().quirks_mode = match mode {
            HtmlQuirksMode::Quirks => QuirksMode::Quirks,
            HtmlQuirksMode::LimitedQuirks => QuirksMode::LimitedQuirks,
            HtmlQuirksMode::NoQuirks => QuirksMode::NoQuirks,
        };
    }

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let mut doc = self.doc.borrow_mut();
        let sibling = self.resolve(*sibling);
        match new_node {
            NodeOrText::AppendNode(node) => {
                let node = self.resolve(node);
                if let Some(parent) = doc.parent(sibling)
                    && !self.record(&mut doc, node, Insertion::Before(parent, sibling))
                {
                    doc.parser_moves(|doc| doc.insert(node, parent, Some(sibling), false));
                }
            }
            NodeOrText::AppendText(text) => doc.parser_insert_text_before(sibling, &text),
        }
        self.check_limits(&doc);
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<html5ever::Attribute>) {
        let mut doc = self.doc.borrow_mut();
        doc.add_attrs_if_missing(self.resolve(*target), convert_attrs(attrs));
        self.check_limits(&doc);
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.doc
            .borrow_mut()
            .parser_moves(|doc| doc.remove_node(self.resolve(*target), false));
    }

    /// Moves the children one by one, as the adoption agency algorithm
    /// appends them (each move is a removal and an insertion).
    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut doc = self.doc.borrow_mut();
        let (node, new_parent) = (self.resolve(*node), self.resolve(*new_parent));
        if is_gone(&doc, new_parent) {
            return;
        }
        while let Some(child) = doc.node(node).first_child() {
            doc.parser_moves(|doc| doc.insert(child, new_parent, None, false));
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeId) -> bool {
        self.integration_points.borrow().contains(handle)
    }

    /// Records the form element pointer for a form-associated element, if
    /// the form and the element's intended parent are in the same tree
    /// (the document or one template's contents).
    fn associate_with_form(
        &self,
        target: &NodeId,
        form: &NodeId,
        nodes: (&NodeId, Option<&NodeId>),
    ) {
        let mut doc = self.doc.borrow_mut();
        let (target, form) = (self.resolve(*target), self.resolve(*form));
        if doc.element(form).is_none() || doc.root(form) != doc.root(self.resolve(*nodes.0)) {
            return;
        }
        if let Some(element) = doc.element_mut(target) {
            element.parser_form = Some(form);
            doc.parser_forms = true;
        }
    }
}
