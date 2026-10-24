//! The tree sink: html5ever's tree builder writes into our [`Document`]
//! through the [`TreeSink`] trait, with the change functions of
//! `mutation.rs`, so that the parser's changes are journaled and reach the
//! mutation sink.
//!
//! The sink holds the document only while the parser runs a step; between
//! steps the caller has it (`HtmlParser::resume`).

use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use html5ever::QualName;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::BufferQueue;
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode as HtmlQuirksMode, TreeSink};

use super::MAX_TREE_DEPTH;
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
}

impl Sink {
    pub(super) fn new(doc: Document, input: Rc<BufferQueue>) -> Sink {
        Sink {
            doc: RefCell::new(doc),
            integration_points: RefCell::default(),
            input,
            stopped: Cell::new(false),
        }
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

fn convert_attrs(attrs: Vec<html5ever::Attribute>) -> Vec<Attribute> {
    attrs
        .into_iter()
        .map(|a| Attribute {
            name: a.name,
            value: a.value.to_string(),
        })
        .collect()
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

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.doc.borrow(), |doc| {
            &doc.element(*target)
                .expect("html5ever asks for names of elements only")
                .name
        })
    }

    /// Creates an element; an HTML `template` gets its contents fragment
    /// from `push_node`.
    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        flags: ElementFlags,
    ) -> NodeId {
        let mut doc = self.doc.borrow_mut();
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
        if let Some(element) = self.doc.borrow_mut().element_mut(*node) {
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
        let parent = depth_limited_parent(&doc, *parent);
        match child {
            NodeOrText::AppendNode(node) => doc.insert(node, parent, None, false),
            NodeOrText::AppendText(text) => doc.parser_append_text(parent, &text),
        }
        self.check_limits(&doc);
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.doc.borrow().parent(*element).is_some();
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

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        self.doc
            .borrow()
            .element(*target)
            .and_then(|e| e.template_contents)
            .expect("html5ever asks for contents of template elements only")
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
        match new_node {
            NodeOrText::AppendNode(node) => {
                if let Some(parent) = doc.parent(*sibling) {
                    doc.insert(node, parent, Some(*sibling), false);
                }
            }
            NodeOrText::AppendText(text) => doc.parser_insert_text_before(*sibling, &text),
        }
        self.check_limits(&doc);
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<html5ever::Attribute>) {
        let mut doc = self.doc.borrow_mut();
        doc.add_attrs_if_missing(*target, convert_attrs(attrs));
        self.check_limits(&doc);
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.doc.borrow_mut().remove_node(*target, false);
    }

    /// Moves the children one by one, as the adoption agency algorithm
    /// appends them (each move is a removal and an insertion).
    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut doc = self.doc.borrow_mut();
        while let Some(child) = doc.node(*node).first_child() {
            doc.insert(child, *new_parent, None, false);
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
        if doc.root(*form) != doc.root(*nodes.0) {
            return;
        }
        if let Some(element) = doc.element_mut(*target) {
            element.parser_form = Some(*form);
        }
    }
}
