//! The change functions: the only code that changes the arena after a node
//! is created (ADR 0028 section 3).
//!
//! They follow the DOM standard's algorithms ("insert", "remove",
//! "replace", "replace all", "adopt", "change an attribute", "replace
//! data": <https://dom.spec.whatwg.org/>), without live ranges, shadow
//! trees and custom elements. Each primitive change bumps the generation,
//! appends a journal entry and calls the mutation sink (`journal.rs`), and
//! keeps the id index (`id_index.rs`) and the byte count of the limits up
//! to date.
//!
//! The public functions check the DOM's preconditions and the limits and
//! return a [`DomError`]. The parser uses the crate-private functions,
//! which skip the checks: html5ever builds valid trees, and the parser
//! stops by itself at the limits (`parser.rs`).
//!
//! Offsets of text data are byte offsets into the UTF-8 data; the bindings
//! convert the UTF-16 offsets of the DOM API.

use html5ever::{LocalName, QualName, local_name, ns};

use crate::journal::Change;
use crate::tree::{Attribute, Document, ElementData, NodeData, NodeId};

/// The errors of the change functions. The first ones are the DOM
/// exceptions of the same names; the limits end the running script task
/// (ADR 0028 section 16).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DomError {
    /// The change would make an invalid tree (a cycle, a text node in a
    /// document, a second root element).
    #[error("HierarchyRequestError: the change would make an invalid tree")]
    HierarchyRequest,
    /// The reference node is not a child of the parent.
    #[error("NotFoundError: the node is not a child of this parent")]
    NotFound,
    /// The node is of the wrong kind for the operation (for example
    /// attributes of a text node, the data of an element).
    #[error("InvalidNodeTypeError: the node is of the wrong kind")]
    InvalidNodeType,
    /// A document node cannot be adopted.
    #[error("NotSupportedError: a document cannot be adopted")]
    NotSupported,
    /// An offset is past the end of the data or not at a character boundary.
    #[error("IndexSizeError: the offset is out of range")]
    IndexSize,
    /// The arena holds its maximum number of nodes.
    #[error("the document reached its limit of {0} nodes")]
    NodeLimit(usize),
    /// The arena holds its maximum number of text and attribute bytes.
    #[error("the document reached its limit of {0} bytes of text and attributes")]
    ByteLimit(usize),
    /// The parser resumed with a document other than its own.
    #[error("the parser resumed with another document")]
    WrongDocument,
}

/// The bytes that the data of a node adds to the byte count of the limits.
pub(crate) fn data_bytes(data: &NodeData) -> usize {
    match data {
        NodeData::Document | NodeData::DocumentFragment => 0,
        NodeData::Doctype {
            name,
            public_id,
            system_id,
        } => name.len() + public_id.len() + system_id.len(),
        NodeData::Element(e) => element_bytes(e),
        NodeData::Text(t) | NodeData::Comment(t) => t.len(),
        NodeData::ProcessingInstruction { target, data } => target.len() + data.len(),
    }
}

/// The bytes of an element: its name and its attributes.
pub(crate) fn element_bytes(e: &ElementData) -> usize {
    name_bytes(&e.name) + e.attrs.iter().map(attribute_bytes).sum::<usize>()
}

/// The bytes of an attribute: its name, its value and the size of its
/// record, so that many short attributes also reach the limit.
fn attribute_bytes(attr: &Attribute) -> usize {
    attribute_size(&attr.name, &attr.value)
}

fn attribute_size(name: &QualName, value: &str) -> usize {
    name_bytes(name) + value.len() + size_of::<Attribute>()
}

/// The bytes of a qualified name. Names are atoms, but a script can make
/// them as long as it wants.
pub(crate) fn name_bytes(name: &QualName) -> usize {
    name.prefix.as_ref().map_or(0, |p| p.len()) + name.ns.len() + name.local.len()
}

/// True for the attribute `id` without a namespace.
fn is_id(name: &QualName) -> bool {
    name.ns == ns!() && name.local == local_name!("id")
}

/// How [`Document::check_insertion`] treats the parent's children.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Validity {
    /// "Ensure pre-insertion validity" before `child`.
    PreInsert,
    /// The checks of "replace": `child` is replaced.
    Replace,
}

// ----- Creation -----

impl Document {
    /// Checks that `nodes` more nodes and `bytes` more bytes fit the limits.
    pub(crate) fn check_room(&self, nodes: usize, bytes: usize) -> Result<(), DomError> {
        if self.nodes.len() + nodes > self.limits.max_nodes {
            return Err(DomError::NodeLimit(self.limits.max_nodes));
        }
        if self.text_bytes + bytes > self.limits.max_text_bytes {
            return Err(DomError::ByteLimit(self.limits.max_text_bytes));
        }
        Ok(())
    }

    /// Creates a detached node whose node document is `owner` (a document
    /// node; a new document node is its own), without the limit check. An
    /// HTML `template` element gets its template contents.
    pub(crate) fn push_node(&mut self, data: NodeData, owner: NodeId) -> NodeId {
        let id = NodeId::from_index(self.nodes.len())
            .expect("the node limits keep the arena far below 2^32 nodes");
        let is_document = matches!(data, NodeData::Document);
        let is_template =
            matches!(&data, NodeData::Element(e) if e.is_html_named(&local_name!("template")));
        self.text_bytes += data_bytes(&data);
        let owner = if is_document { id } else { owner };
        self.nodes.push(crate::tree::Node::new(data, owner));
        if is_template {
            let contents_owner = self.template_contents_owner(owner);
            let contents = self.push_node(NodeData::DocumentFragment, contents_owner);
            if let Some(e) = self.element_mut(id) {
                e.template_contents = Some(contents);
            }
            self.template_hosts.insert(contents, id);
        }
        id
    }

    /// The template contents owner document of the document `doc`: an
    /// inert document that is created at its first use; an inert
    /// document is its own.
    /// <https://html.spec.whatwg.org/multipage/scripting.html#appropriate-template-contents-owner-document>
    pub(crate) fn template_contents_owner(&mut self, doc: NodeId) -> NodeId {
        if let Some(&inert) = self.template_owners.get(&doc) {
            return inert;
        }
        let inert = self.push_node(NodeData::Document, doc);
        self.template_owners.insert(doc, inert);
        self.template_owners.insert(inert, inert);
        inert
    }

    /// Creates a detached node in the document of [`NodeId::DOCUMENT`].
    pub fn create_node(&mut self, data: NodeData) -> Result<NodeId, DomError> {
        self.create_node_in(NodeId::DOCUMENT, data)
    }

    /// Creates a detached node whose node document is the node document of
    /// `document` (a document node, or any node of the wanted document).
    /// A [`NodeData::Document`] creates a new document of the arena.
    pub fn create_node_in(&mut self, document: NodeId, data: NodeData) -> Result<NodeId, DomError> {
        let template =
            matches!(&data, NodeData::Element(e) if e.is_html_named(&local_name!("template")));
        // A template needs its contents and maybe the inert document.
        self.check_room(if template { 3 } else { 1 }, data_bytes(&data))?;
        let owner = self.node_document(document);
        Ok(self.push_node(data, owner))
    }

    /// Gives back a document fragment that the caller made with
    /// [`parse_fragment`](crate::parse_fragment) and no script can reach,
    /// so that the next fragment parse reuses it instead of making a new
    /// node. Does nothing unless the fragment is detached and empty.
    pub fn recycle_fragment(&mut self, fragment: NodeId) {
        let node = self.node(fragment);
        if matches!(node.data, NodeData::DocumentFragment)
            && node.parent.is_none()
            && node.first_child.is_none()
            && !self.template_hosts.contains_key(&fragment)
        {
            self.scratch_fragment = Some(fragment);
        }
    }

    /// The recycled fragment whose node document is `owner`, if any.
    pub(crate) fn take_scratch_fragment(&mut self, owner: NodeId) -> Option<NodeId> {
        let fragment = self.scratch_fragment.take()?;
        (self.node_document(fragment) == owner).then_some(fragment)
    }

    /// A detached `body` element of `document`, for the context of a
    /// fragment parse that has no element to use (`insertAdjacentHTML` on
    /// a child of a document). The same element serves all such parses of
    /// a document; no script can reach it.
    ///
    /// # Errors
    ///
    /// [`DomError::NodeLimit`] if the arena is full.
    pub fn scratch_body(&mut self, document: NodeId) -> Result<NodeId, DomError> {
        let owner = self.node_document(document);
        if let Some(body) = self.scratch_body
            && self.node_document(body) == owner
        {
            return Ok(body);
        }
        let name = QualName::new(None, ns!(html), local_name!("body"));
        let body =
            self.create_node_in(owner, NodeData::Element(ElementData::new(name, Vec::new())))?;
        self.scratch_body = Some(body);
        Ok(body)
    }

    /// Creates a detached element in the document of [`NodeId::DOCUMENT`].
    pub fn create_element(
        &mut self,
        name: QualName,
        attrs: Vec<Attribute>,
    ) -> Result<NodeId, DomError> {
        self.create_node(NodeData::Element(ElementData::new(name, attrs)))
    }

    /// Creates a detached text node in the document of [`NodeId::DOCUMENT`].
    pub fn create_text(&mut self, text: &str) -> Result<NodeId, DomError> {
        self.create_node(NodeData::Text(text.to_owned()))
    }

    /// Creates a new empty document node in the arena
    /// (`createHTMLDocument`, `DOMParser`). It is its own node document.
    pub fn create_document(&mut self) -> Result<NodeId, DomError> {
        self.create_node(NodeData::Document)
    }
}

// ----- Tree links -----

impl Document {
    /// Links the detached `node` into `parent`'s children before `before`
    /// (a child of `parent`) or at the end, and adds its ids to the index
    /// if it became connected. No record.
    #[inline]
    pub(crate) fn link(&mut self, parent: NodeId, node: NodeId, before: Option<NodeId>) {
        self.link_raw(parent, node, before);
        if self.may_have_ids(node) && self.is_connected(parent) {
            self.index_ids(node);
        }
    }

    /// [`link`](Self::link) without the id index: for a `parent` that is
    /// not connected.
    #[inline]
    pub(crate) fn link_raw(&mut self, parent: NodeId, node: NodeId, before: Option<NodeId>) {
        let prev = match before {
            Some(b) => self.node(b).prev_sibling,
            None => self.node(parent).last_child,
        };
        {
            let n = self.node_mut(node);
            n.parent = Some(parent);
            n.prev_sibling = prev;
            n.next_sibling = before;
        }
        match prev {
            Some(p) => self.node_mut(p).next_sibling = Some(node),
            None => self.node_mut(parent).first_child = Some(node),
        }
        match before {
            Some(b) => self.node_mut(b).prev_sibling = Some(node),
            None => self.node_mut(parent).last_child = Some(node),
        }
        let is_element = self.node(node).is_element();
        let p = self.node_mut(parent);
        p.child_count += 1;
        p.element_child_count += u32::from(is_element);
    }

    /// Unlinks `node` from its parent and removes its ids from the index if
    /// it was connected. No record. Does nothing for a node without parent.
    pub(crate) fn unlink(&mut self, node: NodeId) {
        let (parent, prev, next) = {
            let n = self.node(node);
            (n.parent, n.prev_sibling, n.next_sibling)
        };
        let Some(parent) = parent else {
            return;
        };
        if self.may_have_ids(node) && self.is_connected(parent) {
            self.unindex_ids(node);
        }
        match prev {
            Some(p) => self.node_mut(p).next_sibling = next,
            None => self.node_mut(parent).first_child = next,
        }
        match next {
            Some(n) => self.node_mut(n).prev_sibling = prev,
            None => self.node_mut(parent).last_child = prev,
        }
        let n = self.node_mut(node);
        n.parent = None;
        n.prev_sibling = None;
        n.next_sibling = None;
        let is_element = n.is_element();
        let p = self.node_mut(parent);
        p.child_count -= 1;
        p.element_child_count -= u32::from(is_element);
    }

    /// Reports a change: bumps the generation, appends a journal entry and
    /// calls the mutation sink with the document in its new state.
    #[inline]
    pub(crate) fn record(&mut self, change: &Change<'_>) {
        self.generation += 1;
        if self.journaling {
            self.journal.record(change);
        } else {
            self.journal.skip();
        }
        if let Some(mut sink) = self.mutation_sink.take() {
            sink.change(self, change);
            self.mutation_sink = Some(sink);
        }
    }

    /// The children of a node, collected.
    fn child_list(&self, node: NodeId) -> Vec<NodeId> {
        self.children(node).collect()
    }
}

// ----- The DOM's tree algorithms -----

impl Document {
    /// DOM "insert" (without the checks): inserts `node` (or the children
    /// of a fragment `node`) into `parent` before `before` or at the end.
    /// <https://dom.spec.whatwg.org/#concept-node-insert>
    pub(crate) fn insert(
        &mut self,
        node: NodeId,
        parent: NodeId,
        before: Option<NodeId>,
        suppress_observers: bool,
    ) {
        if !matches!(self.node(node).data, NodeData::DocumentFragment) {
            // One node: no list to collect (the parser's case). As in step 6
            // of "insert", the previous sibling is read before the adopt
            // step removes `node`: a move within the same parent can name
            // `node` itself in the record.
            let prev_sibling = match before {
                Some(b) => self.node(b).prev_sibling,
                None => self.node(parent).last_child,
            };
            self.adopt_into(node, self.node_document(parent));
            self.link(parent, node, before);
            if !suppress_observers {
                self.record(&Change::Children {
                    parent,
                    added: std::slice::from_ref(&node),
                    removed: &[],
                    prev_sibling,
                    next_sibling: before,
                });
            }
            return;
        }
        let nodes = self.child_list(node);
        if nodes.is_empty() {
            return;
        }
        for &n in &nodes {
            self.unlink(n);
        }
        self.record(&Change::Children {
            parent: node,
            added: &[],
            removed: &nodes,
            prev_sibling: None,
            next_sibling: None,
        });
        let prev_sibling = match before {
            Some(b) => self.node(b).prev_sibling,
            None => self.node(parent).last_child,
        };
        let document = self.node_document(parent);
        for &n in &nodes {
            self.adopt_into(n, document);
            self.link(parent, n, before);
        }
        if !suppress_observers {
            self.record(&Change::Children {
                parent,
                added: &nodes,
                removed: &[],
                prev_sibling,
                next_sibling: before,
            });
        }
    }

    /// DOM "remove" (without the checks). Does nothing for a node without
    /// parent.
    /// <https://dom.spec.whatwg.org/#concept-node-remove>
    pub(crate) fn remove_node(&mut self, node: NodeId, suppress_observers: bool) {
        let (parent, prev_sibling, next_sibling) = {
            let n = self.node(node);
            (n.parent, n.prev_sibling, n.next_sibling)
        };
        let Some(parent) = parent else {
            return;
        };
        self.unlink(node);
        if !suppress_observers {
            self.record(&Change::Children {
                parent,
                added: &[],
                removed: &[node],
                prev_sibling,
                next_sibling,
            });
        }
    }

    /// DOM "adopt" (without the check for documents): removes `node` from
    /// its parent and makes `document` the node document of its subtree;
    /// template contents go to `document`'s template contents owner.
    /// <https://dom.spec.whatwg.org/#concept-node-adopt>
    #[inline]
    pub(crate) fn adopt_into(&mut self, node: NodeId, document: NodeId) {
        if self.node(node).parent.is_some() {
            self.remove_node(node, false);
        }
        if self.node(node).owner != document {
            self.set_node_document(node, document);
        }
    }

    /// Makes `document` the node document of the subtree of `node`, and
    /// its template contents owner that of the template contents in it.
    fn set_node_document(&mut self, node: NodeId, document: NodeId) {
        let mut stack = vec![(node, document)];
        while let Some((root, document)) = stack.pop() {
            let subtree: Vec<NodeId> = self.descendants(root).collect();
            for n in subtree {
                self.node_mut(n).owner = document;
                if let Some(contents) = self.element(n).and_then(|e| e.template_contents) {
                    let inert = self.template_contents_owner(document);
                    stack.push((contents, inert));
                }
            }
        }
    }

    /// DOM "replace all" (without the checks).
    /// <https://dom.spec.whatwg.org/#concept-node-replace-all>
    fn replace_all_unchecked(&mut self, parent: NodeId, node: Option<NodeId>) {
        let removed = self.child_list(parent);
        let added = match node {
            Some(n) if matches!(self.node(n).data, NodeData::DocumentFragment) => {
                self.child_list(n)
            }
            Some(n) => vec![n],
            None => Vec::new(),
        };
        for &child in &removed {
            self.remove_node(child, true);
            self.reset_form_owners(child);
        }
        if let Some(n) = node {
            let moved = self.moved_roots(n);
            self.reset_moved(&moved);
            self.insert(n, parent, None, true);
        }
        if !added.is_empty() || !removed.is_empty() {
            self.record(&Change::Children {
                parent,
                added: &added,
                removed: &removed,
                prev_sibling: None,
                next_sibling: None,
            });
        }
    }

    /// DOM "pre-insert": inserts `node` into `parent` before `child` (at the
    /// end if `None`). A fragment inserts its children. `node` is first
    /// removed from its parent.
    /// <https://dom.spec.whatwg.org/#concept-node-pre-insert>
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        node: NodeId,
        child: Option<NodeId>,
    ) -> Result<(), DomError> {
        self.check_insertion(parent, node, child, Validity::PreInsert)?;
        let before = if child == Some(node) {
            self.node(node).next_sibling
        } else {
            child
        };
        let moved = self.moved_roots(node);
        self.insert(node, parent, before, false);
        self.reset_moved(&moved);
        Ok(())
    }

    /// DOM `appendChild`: inserts `node` at the end of `parent`'s children.
    pub fn append_child(&mut self, parent: NodeId, node: NodeId) -> Result<(), DomError> {
        self.insert_before(parent, node, None)
    }

    /// DOM "remove": removes `node` from its parent (`ChildNode.remove`).
    /// Does nothing for a node without parent.
    pub fn remove(&mut self, node: NodeId) {
        self.remove_node(node, false);
        self.reset_form_owners(node);
    }

    /// DOM `removeChild`: removes `child` from `parent`.
    /// <https://dom.spec.whatwg.org/#concept-node-pre-remove>
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        if self.node(child).parent != Some(parent) {
            return Err(DomError::NotFound);
        }
        self.remove_node(child, false);
        self.reset_form_owners(child);
        Ok(())
    }

    /// DOM "replace": replaces `child` of `parent` with `node` (or the
    /// children of a fragment `node`), with one record.
    /// <https://dom.spec.whatwg.org/#concept-node-replace>
    pub fn replace_child(
        &mut self,
        parent: NodeId,
        node: NodeId,
        child: NodeId,
    ) -> Result<(), DomError> {
        self.check_insertion(parent, node, Some(child), Validity::Replace)?;
        let mut reference = self.node(child).next_sibling;
        if reference == Some(node) {
            reference = self.node(node).next_sibling;
        }
        let prev_sibling = self.node(child).prev_sibling;
        self.remove_node(child, true);
        let nodes = if matches!(self.node(node).data, NodeData::DocumentFragment) {
            self.child_list(node)
        } else {
            vec![node]
        };
        self.reset_form_owners(child);
        let moved = self.moved_roots(node);
        self.reset_moved(&moved);
        self.insert(node, parent, reference, true);
        self.record(&Change::Children {
            parent,
            added: &nodes,
            removed: &[child],
            prev_sibling,
            next_sibling: reference,
        });
        Ok(())
    }

    /// DOM "replace all": replaces all children of `parent` with `node`
    /// (or the children of a fragment `node`, or nothing), with one record
    /// (`textContent`, `innerHTML`, `replaceChildren`). The check is the one
    /// of `replaceChildren`, "ensure pre-insertion validity" before null
    /// with the current children: a document with an element child refuses
    /// a second element even though the first would go.
    /// <https://dom.spec.whatwg.org/#dom-parentnode-replacechildren>
    pub fn replace_all(&mut self, parent: NodeId, node: Option<NodeId>) -> Result<(), DomError> {
        if let Some(node) = node {
            self.check_insertion(parent, node, None, Validity::PreInsert)?;
        } else if !self.can_have_children(parent) {
            return Err(DomError::HierarchyRequest);
        }
        self.replace_all_unchecked(parent, node);
        Ok(())
    }

    /// DOM `adoptNode`: removes `node` from its parent and moves its
    /// subtree into the document `document` (any node of that document).
    /// <https://dom.spec.whatwg.org/#dom-document-adoptnode>
    pub fn adopt(&mut self, node: NodeId, document: NodeId) -> Result<(), DomError> {
        if matches!(self.node(node).data, NodeData::Document) {
            return Err(DomError::NotSupported);
        }
        let document = self.node_document(document);
        let had_parent = self.node(node).parent.is_some();
        self.adopt_into(node, document);
        if had_parent {
            self.reset_form_owners(node);
        }
        Ok(())
    }

    /// The nodes that an insertion of `node` moves from another place: the
    /// node, or the children of a fragment, that have a parent.
    ///
    /// Invariant for the parser form pointers ([`ElementData::parser_form`]):
    /// the parser sets them only through its own crate-private functions,
    /// and every removal by the public functions clears the pointers of the
    /// removed subtree (`reset_form_owners`). A subtree whose root has no
    /// parent therefore holds none (the parser never leaves one detached,
    /// the fragment parser and `clone_node` clear them), and an insertion
    /// of such a root needs no walk: building a chain bottom-up stays
    /// linear. The children of a fragment all have a parent (the fragment),
    /// so they are walked: the contents of a `template` may hold pointers.
    fn moved_roots(&self, node: NodeId) -> Vec<NodeId> {
        if !self.parser_forms {
            return Vec::new();
        }
        if matches!(self.node(node).data, NodeData::DocumentFragment) {
            self.child_list(node)
        } else if self.node(node).parent.is_some() {
            vec![node]
        } else {
            Vec::new()
        }
    }

    /// Resets the form owners of the subtrees of `roots`.
    fn reset_moved(&mut self, roots: &[NodeId]) {
        for &root in roots {
            self.reset_form_owners(root);
        }
    }

    fn can_have_children(&self, node: NodeId) -> bool {
        matches!(
            self.node(node).data,
            NodeData::Document | NodeData::DocumentFragment | NodeData::Element(_)
        )
    }

    /// True if `node` is a host-including inclusive ancestor of `of`: an
    /// inclusive ancestor, also across template contents to their
    /// template element.
    /// <https://dom.spec.whatwg.org/#concept-tree-host-including-inclusive-ancestor>
    pub fn is_host_including_inclusive_ancestor(&self, node: NodeId, of: NodeId) -> bool {
        // A node without children and template contents is an ancestor of
        // nothing: the common case (a new leaf) needs no walk, so that
        // building a deep tree is not quadratic.
        let n = self.node(node);
        if n.first_child.is_none() && n.as_element().is_none_or(|e| e.template_contents.is_none()) {
            return node == of;
        }
        let mut current = Some(of);
        while let Some(n) = current {
            if n == node {
                return true;
            }
            current = self
                .node(n)
                .parent
                .or_else(|| self.template_hosts.get(&n).copied());
        }
        false
    }

    /// The checks of "ensure pre-insertion validity" and of "replace"
    /// (<https://dom.spec.whatwg.org/#concept-node-ensure-pre-insertion-validity>).
    fn check_insertion(
        &self,
        parent: NodeId,
        node: NodeId,
        child: Option<NodeId>,
        validity: Validity,
    ) -> Result<(), DomError> {
        if !self.can_have_children(parent)
            || self.is_host_including_inclusive_ancestor(node, parent)
        {
            return Err(DomError::HierarchyRequest);
        }
        if let Some(child) = child
            && self.node(child).parent != Some(parent)
        {
            return Err(DomError::NotFound);
        }
        let parent_is_document = matches!(self.node(parent).data, NodeData::Document);
        match &self.node(node).data {
            NodeData::Document => return Err(DomError::HierarchyRequest),
            NodeData::Text(_) if parent_is_document => return Err(DomError::HierarchyRequest),
            NodeData::Doctype { .. } if !parent_is_document => {
                return Err(DomError::HierarchyRequest);
            }
            _ => {}
        }
        if parent_is_document {
            self.check_document_child(parent, node, child, validity)?;
        }
        Ok(())
    }

    /// The checks for a child of a document: one element, one doctype, the
    /// doctype before the element, no text.
    fn check_document_child(
        &self,
        parent: NodeId,
        node: NodeId,
        child: Option<NodeId>,
        validity: Validity,
    ) -> Result<(), DomError> {
        let is_element = |n: NodeId| self.node(n).is_element();
        let is_doctype = |n: NodeId| matches!(self.node(n).data, NodeData::Doctype { .. });
        // The parent's children that stay: with "replace", all but `child`.
        let staying = |n: &NodeId| match validity {
            Validity::PreInsert => true,
            Validity::Replace => Some(*n) != child,
        };
        let has_element = self.children(parent).filter(staying).any(is_element);
        let has_doctype = self.children(parent).filter(staying).any(is_doctype);
        let following = |c: NodeId| {
            std::iter::successors(self.node(c).next_sibling, |&n| self.node(n).next_sibling)
        };
        let preceding = |c: NodeId| {
            std::iter::successors(self.node(c).prev_sibling, |&n| self.node(n).prev_sibling)
        };
        let doctype_after_child = child.is_some_and(|c| following(c).any(is_doctype));
        let child_is_doctype = validity == Validity::PreInsert && child.is_some_and(is_doctype);
        let bad = match &self.node(node).data {
            NodeData::DocumentFragment => {
                let elements = self.element_children(node).count();
                let has_text = self
                    .children(node)
                    .any(|c| matches!(self.node(c).data, NodeData::Text(_)));
                elements > 1
                    || has_text
                    || (elements == 1 && (has_element || child_is_doctype || doctype_after_child))
            }
            NodeData::Element(_) => has_element || child_is_doctype || doctype_after_child,
            NodeData::Doctype { .. } => {
                let element_before_child = child.is_some_and(|c| preceding(c).any(is_element));
                has_doctype
                    || element_before_child
                    || (validity != Validity::Replace && child.is_none() && has_element)
            }
            _ => false,
        };
        if bad {
            Err(DomError::HierarchyRequest)
        } else {
            Ok(())
        }
    }
}

// ----- Attributes -----

impl Document {
    /// DOM "set an attribute value" / "change an attribute": sets the
    /// attribute `name` (namespace and local name) of `element` to `value`.
    /// <https://dom.spec.whatwg.org/#concept-element-attributes-set-value>
    pub fn set_attribute(
        &mut self,
        element: NodeId,
        name: &QualName,
        value: &str,
    ) -> Result<(), DomError> {
        let e = self.element(element).ok_or(DomError::InvalidNodeType)?;
        let index = e.attr_index(name);
        // A change replaces the value; a new attribute adds its name and
        // record too.
        let (old_bytes, new_bytes) = match index.and_then(|i| e.attrs.get(i)) {
            Some(a) => (a.value.len(), value.len()),
            None => (0, attribute_size(name, value)),
        };
        self.check_room(0, new_bytes.saturating_sub(old_bytes))?;
        let e = self
            .element_mut(element)
            .expect("the node is an element (checked above)");
        let old_value = if let Some(attr) = index.and_then(|i| e.attrs.get_mut(i)) {
            Some(std::mem::replace(&mut attr.value, value.to_owned()))
        } else {
            e.attrs.push(Attribute {
                name: name.clone(),
                value: value.to_owned(),
            });
            None
        };
        self.text_bytes = (self.text_bytes + new_bytes).saturating_sub(old_bytes);
        if is_id(name) {
            self.reindex_id(element, old_value.as_deref(), Some(value));
        }
        self.record(&Change::Attribute {
            element,
            name,
            old_value: old_value.as_deref(),
        });
        Ok(())
    }

    /// [`set_attribute`](Self::set_attribute) for an attribute without a
    /// namespace.
    pub fn set_attr(&mut self, element: NodeId, local: &str, value: &str) -> Result<(), DomError> {
        self.set_attribute(
            element,
            &QualName::new(None, ns!(), LocalName::from(local)),
            value,
        )
    }

    /// DOM "remove an attribute by namespace and local name". Returns the
    /// old value, or `None` if the attribute did not exist (or `element`
    /// is not an element).
    /// <https://dom.spec.whatwg.org/#concept-element-attributes-remove-by-namespace>
    pub fn remove_attribute(&mut self, element: NodeId, name: &QualName) -> Option<String> {
        let e = self.element_mut(element)?;
        let index = e.attr_index(name)?;
        let attr = e.attrs.remove(index);
        self.text_bytes = self.text_bytes.saturating_sub(attribute_bytes(&attr));
        if is_id(&attr.name) {
            self.reindex_id(element, Some(&attr.value), None);
        }
        self.record(&Change::Attribute {
            element,
            name: &attr.name,
            old_value: Some(&attr.value),
        });
        Some(attr.value)
    }

    /// [`remove_attribute`](Self::remove_attribute) for an attribute
    /// without a namespace. Returns true if it existed.
    pub fn remove_attr(&mut self, element: NodeId, local: &str) -> bool {
        let name = QualName::new(None, ns!(), LocalName::from(local));
        self.remove_attribute(element, &name).is_some()
    }

    /// Adds each attribute that `element` does not have (the parser, for
    /// a second `<html>` or `<body>` tag). No limit check.
    pub(crate) fn add_attrs_if_missing(&mut self, element: NodeId, attrs: Vec<Attribute>) {
        for attr in attrs {
            let Some(e) = self.element_mut(element) else {
                return;
            };
            if e.attr_index(&attr.name).is_some() {
                continue;
            }
            let name = attr.name.clone();
            self.text_bytes += attribute_bytes(&attr);
            if is_id(&name) {
                self.reindex_id(element, None, Some(&attr.value));
            }
            if let Some(e) = self.element_mut(element) {
                e.attrs.push(attr);
            }
            self.record(&Change::Attribute {
                element,
                name: &name,
                old_value: None,
            });
        }
    }
}

// ----- Text data -----

impl Document {
    /// The data of a text, comment or processing instruction node.
    pub fn char_data(&self, node: NodeId) -> Option<&str> {
        match &self.node(node).data {
            NodeData::Text(t) | NodeData::Comment(t) => Some(t),
            NodeData::ProcessingInstruction { data, .. } => Some(data),
            _ => None,
        }
    }

    fn char_data_mut(&mut self, node: NodeId) -> Option<&mut String> {
        match &mut self.node_mut(node).data {
            NodeData::Text(t) | NodeData::Comment(t) => Some(t),
            NodeData::ProcessingInstruction { data, .. } => Some(data),
            _ => None,
        }
    }

    /// DOM "replace data": replaces `count` bytes of the data of `node`
    /// from `offset` with `data`. `count` is clamped to the end of the
    /// data; both ends must be character boundaries.
    /// <https://dom.spec.whatwg.org/#concept-cd-replace>
    pub fn replace_data(
        &mut self,
        node: NodeId,
        offset: usize,
        count: usize,
        data: &str,
    ) -> Result<(), DomError> {
        let current = self.char_data(node).ok_or(DomError::InvalidNodeType)?;
        let len = current.len();
        if offset > len {
            return Err(DomError::IndexSize);
        }
        let end = offset + count.min(len - offset);
        if !current.is_char_boundary(offset) || !current.is_char_boundary(end) {
            return Err(DomError::IndexSize);
        }
        self.check_room(0, data.len().saturating_sub(end - offset))?;
        if offset == len {
            self.append_data_unchecked(node, data);
            return Ok(());
        }
        let old = self.mutation_sink.is_some().then(|| current.to_owned());
        if let Some(text) = self.char_data_mut(node) {
            text.replace_range(offset..end, data);
        }
        self.text_bytes = (self.text_bytes + data.len()).saturating_sub(end - offset);
        self.record(&Change::Text {
            node,
            old_value: old.as_deref(),
        });
        Ok(())
    }

    /// Replaces all data of `node` (`Text.data`, `nodeValue`).
    pub fn set_data(&mut self, node: NodeId, data: &str) -> Result<(), DomError> {
        self.replace_data(node, 0, usize::MAX, data)
    }

    /// Appends to the data of `node` (`appendData`).
    pub fn append_data(&mut self, node: NodeId, data: &str) -> Result<(), DomError> {
        let len = self.char_data(node).ok_or(DomError::InvalidNodeType)?.len();
        self.replace_data(node, len, 0, data)
    }

    /// Appends to the data of `node` without the limit check. The old
    /// value that the sink gets is a prefix of the new data, so that text
    /// appended in many steps is not copied each time.
    #[inline]
    fn append_data_unchecked(&mut self, node: NodeId, data: &str) {
        let Some(text) = self.char_data_mut(node) else {
            return;
        };
        let old_len = text.len();
        text.push_str(data);
        self.text_bytes += data.len();
        self.generation += 1;
        if self.journaling {
            self.journal.record(&Change::Text {
                node,
                old_value: None,
            });
        } else {
            self.journal.skip();
        }
        if let Some(mut sink) = self.mutation_sink.take() {
            let old_value = self.char_data(node).and_then(|t| t.get(..old_len));
            sink.change(self, &Change::Text { node, old_value });
            self.mutation_sink = Some(sink);
        }
    }

    /// The parser's text insertion: appends `text` to the last child of
    /// `parent` if it is a text node, otherwise inserts a new text node.
    /// <https://html.spec.whatwg.org/multipage/parsing.html#insert-a-character>
    pub(crate) fn parser_append_text(&mut self, parent: NodeId, text: &str) {
        if let Some(last) = self.node(parent).last_child
            && matches!(self.node(last).data, NodeData::Text(_))
        {
            self.append_data_unchecked(last, text);
            return;
        }
        let owner = self.node_document(parent);
        let t = self.push_node(NodeData::Text(text.to_owned()), owner);
        self.insert(t, parent, None, false);
    }

    /// The parser's text insertion before `sibling`: appends to the
    /// previous sibling if it is a text node, otherwise inserts a new text
    /// node. Does nothing if `sibling` has no parent.
    pub(crate) fn parser_insert_text_before(&mut self, sibling: NodeId, text: &str) {
        let Some(parent) = self.node(sibling).parent else {
            return;
        };
        if let Some(prev) = self.node(sibling).prev_sibling
            && matches!(self.node(prev).data, NodeData::Text(_))
        {
            self.append_data_unchecked(prev, text);
            return;
        }
        let owner = self.node_document(parent);
        let t = self.push_node(NodeData::Text(text.to_owned()), owner);
        self.insert(t, parent, Some(sibling), false);
    }
}

#[cfg(test)]
mod tests;
