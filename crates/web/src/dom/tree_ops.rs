//! Tree changes for scripts (DOM §4.2.3 "Mutation algorithms",
//! <https://dom.spec.whatwg.org/#mutation-algorithms>, and the `ParentNode`
//! and `ChildNode` mixins §4.2.4 and §4.2.8): `insertBefore`,
//! `appendChild`, `replaceChild`, `removeChild`, `append`, `prepend`,
//! `replaceChildren`, `before`, `after`, `replaceWith`, `remove`.
//!
//! The change functions of `swb_dom` do the work and the checks; this
//! module runs the checks first to throw Chromium's exceptions (names and
//! messages were measured in `tools/probes/web-dom-mutation.json`), and
//! calls the steps of script elements after each insertion
//! (`crate::scripts::after_insertion`, HTML §4.12.1.1).
//!
//! An argument can be a node of another document of the runtime (`NodeRef`,
//! `adopt.rs`): the checks read both arenas, and only then the node moves here
//! (ADR 0031 "Done (feature 12b2)").

use std::collections::HashSet;

use swb_dom::{Document, NodeData, NodeId};

use super::{adopt, dom_result, node_name};
use crate::bindings::{self as b, Cx, Exception, NodeRef};
use crate::generated::types::NodeOrDomString;
use crate::state::{self, dom, dom_mut};

/// A `HierarchyRequestError`. The glue adds Chromium's prefix, `Failed to
/// execute 'NAME' on 'INTERFACE': `, to the message of every exception.
pub(crate) fn hierarchy(message: &str) -> Exception {
    Exception::dom("HierarchyRequestError", message.to_owned())
}

/// A `NotFoundError`.
pub(crate) fn not_found(message: &str) -> Exception {
    Exception::dom("NotFoundError", message.to_owned())
}

/// The type message for a node of `node_dom` that the parent cannot take.
fn type_message(dom: &Document, node_dom: &Document, node: NodeId, parent: NodeId) -> String {
    format!(
        "Nodes of type '{}' may not be inserted inside nodes of type '{}'.",
        node_name(&node_dom.node(node).data),
        node_name(&dom.node(parent).data)
    )
}

/// DOM "ensure pre-insertion validity" (and the checks of "replace" with
/// `replace`), throwing Chromium's exceptions. The order of the checks is
/// Chromium's: the kind of the parent, then (for a document parent) the
/// document rules or (otherwise) the ancestor check, the reference child,
/// and the kinds of node that the parent takes.
/// <https://dom.spec.whatwg.org/#concept-node-ensure-pre-insertion-validity>
pub(super) fn ensure_valid(
    dom: &Document,
    parent: NodeId,
    node: NodeId,
    child: Option<NodeId>,
    replace: bool,
) -> Result<(), Exception> {
    ensure_valid_in(dom, parent, dom, node, Reference::of(child), replace)
}

/// The reference child of an insertion.
#[derive(Clone, Copy)]
enum Reference {
    /// None: at the end.
    None,
    /// A node of the parent's document.
    Node(NodeId),
    /// A node of another document, which is not a child of the parent.
    OtherDocument,
}

impl Reference {
    fn of(child: Option<NodeId>) -> Reference {
        child.map_or(Reference::None, Reference::Node)
    }

    fn id(self) -> Option<NodeId> {
        match self {
            Reference::Node(node) => Some(node),
            _ => None,
        }
    }
}

/// [`ensure_valid`] of `node` of the arena `node_dom`, which can be another one
/// than the parent's: a node of another arena is no ancestor of the parent.
fn ensure_valid_in(
    dom: &Document,
    parent: NodeId,
    node_dom: &Document,
    node: NodeId,
    child: Reference,
    replace: bool,
) -> Result<(), Exception> {
    let parent_is_document = match dom.node(parent).data {
        NodeData::Document => true,
        NodeData::DocumentFragment | NodeData::Element(_) => false,
        _ => return Err(hierarchy("This node type does not support this method.")),
    };
    let node_is_document = matches!(node_dom.node(node).data, NodeData::Document);
    if !(parent_is_document && node_is_document)
        && std::ptr::eq(dom, node_dom)
        && dom.is_host_including_inclusive_ancestor(node, parent)
    {
        return Err(hierarchy("The new child element contains the parent."));
    }
    let not_a_child = match child {
        Reference::None => false,
        Reference::Node(child) => dom.parent(child) != Some(parent),
        Reference::OtherDocument => true,
    };
    if not_a_child {
        return Err(not_found(if replace {
            "The node to be replaced is not a child of this node."
        } else {
            "The node before which the new node is to be inserted is not a child of this node."
        }));
    }
    let type_error = || hierarchy(&type_message(dom, node_dom, node, parent));
    match &node_dom.node(node).data {
        NodeData::Document => return Err(type_error()),
        NodeData::Doctype { .. } if !parent_is_document => return Err(type_error()),
        NodeData::Text(_) if parent_is_document => return Err(type_error()),
        _ => {}
    }
    if parent_is_document {
        check_document_child(dom, parent, node_dom, node, child.id(), replace)?;
    }
    Ok(())
}

/// The rules for a child of a document: one element, one doctype, the
/// doctype before the element, no text.
fn check_document_child(
    dom: &Document,
    parent: NodeId,
    node_dom: &Document,
    node: NodeId,
    child: Option<NodeId>,
    replace: bool,
) -> Result<(), Exception> {
    let is_element = |n: NodeId| dom.node(n).is_element();
    let is_doctype = |n: NodeId| matches!(dom.node(n).data, NodeData::Doctype { .. });
    // The children that stay.
    let staying = |n: &NodeId| !(replace && Some(*n) == child);
    let has_element = dom.children(parent).filter(staying).any(is_element);
    let has_doctype = dom.children(parent).filter(staying).any(is_doctype);
    let following = |c: NodeId| {
        std::iter::successors(dom.node(c).next_sibling(), |&n| dom.node(n).next_sibling())
    };
    let preceding = |c: NodeId| {
        std::iter::successors(dom.node(c).prev_sibling(), |&n| dom.node(n).prev_sibling())
    };
    let before_doctype =
        child.is_some_and(|c| (!replace && is_doctype(c)) || following(c).any(is_doctype));
    let element_rule = || -> Result<(), Exception> {
        if before_doctype {
            return Err(hierarchy("Can't insert an element before a doctype."));
        }
        if has_element {
            return Err(hierarchy("Only one element on document allowed."));
        }
        Ok(())
    };
    match &node_dom.node(node).data {
        NodeData::Element(_) => element_rule(),
        NodeData::DocumentFragment => {
            let mut elements = 0;
            for c in node_dom.children(node) {
                if matches!(node_dom.node(c).data, NodeData::Text(_)) {
                    return Err(hierarchy(&type_message(dom, node_dom, c, parent)));
                }
                elements += usize::from(node_dom.node(c).is_element());
            }
            if elements > 1 {
                return Err(hierarchy("Only one element on document allowed."));
            }
            if elements == 1 {
                element_rule()?;
            }
            Ok(())
        }
        NodeData::Doctype { .. } => {
            let element_before = child.is_some_and(|c| preceding(c).any(is_element));
            if element_before || (child.is_none() && has_element) {
                return Err(hierarchy("Can't insert a doctype after the root element."));
            }
            if has_doctype {
                return Err(hierarchy("Only one doctype on document allowed."));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The nodes that an insertion of `node` adds: the node, or the children
/// of a fragment (read before the insertion empties it).
fn inserted(dom: &Document, node: NodeId) -> Vec<NodeId> {
    match dom.node(node).data {
        NodeData::DocumentFragment => dom.children(node).collect(),
        _ => vec![node],
    }
}

/// An insertion takes `node` from its old parent first (DOM "adopt" and
/// "remove"): a focused element in `node` loses the focus, as in a removal.
fn before_move(cx: &mut Cx<'_>, node: NodeId) -> b::Result<()> {
    if dom(cx.rt)?.parent(node).is_some() {
        crate::html::focus::before_removal(cx.rt, node, true)?;
    }
    Ok(())
}

/// DOM "pre-insert" `node` into `parent` before `child`, then the steps
/// of the script elements that the insertion connected.
/// <https://dom.spec.whatwg.org/#concept-node-pre-insert>
pub(crate) fn pre_insert(
    cx: &mut Cx<'_>,
    parent: NodeId,
    node: NodeId,
    child: Option<NodeId>,
) -> b::Result<()> {
    ensure_valid(dom(cx.rt)?, parent, node, child, false)?;
    before_move(cx, node)?;
    let added = inserted(dom(cx.rt)?, node);
    let result = dom_mut(cx.rt)?.insert_before(parent, node, child);
    dom_result(cx.rt, result)?;
    Ok(crate::scripts::after_insertion(cx.rt, parent, &added)?)
}

/// The reference child of a native's arguments.
fn reference(rt: &swb_js::Runtime, child: Option<NodeRef>) -> Reference {
    match child {
        None => Reference::None,
        Some(child) => child
            .here(rt)
            .map_or(Reference::OtherDocument, Reference::Node),
    }
}

/// The checks for a node of another document, in both arenas.
fn ensure_valid_across(
    rt: &mut swb_js::Runtime,
    parent: NodeId,
    node: NodeRef,
    child: Reference,
    replace: bool,
) -> b::Result<()> {
    state::ensure_lent(rt, node.document)?;
    let other = state::dom_of(rt, node.document)?;
    ensure_valid_in(dom(rt)?, parent, other, node.node, child, replace)
}

/// [`pre_insert`] of native arguments, which can belong to other documents.
pub(crate) fn pre_insert_ref(
    cx: &mut Cx<'_>,
    parent: NodeId,
    node: NodeRef,
    child: Option<NodeRef>,
) -> b::Result<()> {
    let child = reference(cx.rt, child);
    if let Some(here) = node.here(cx.rt) {
        if matches!(child, Reference::OtherDocument) {
            ensure_valid_in(dom(cx.rt)?, parent, dom(cx.rt)?, here, child, false)?;
        }
        return pre_insert(cx, parent, here, child.id());
    }
    ensure_valid_across(cx.rt, parent, node, child, false)?;
    let here = adopt::bring_here(cx.rt, node, parent)?;
    pre_insert(cx, parent, here, child.id())
}

/// [`pre_insert`] of a node argument, which can belong to another document,
/// before a child that the native found itself.
pub(crate) fn pre_insert_node(
    cx: &mut Cx<'_>,
    parent: NodeId,
    node: NodeRef,
    child: Option<NodeId>,
) -> b::Result<()> {
    let converted = match node.here(cx.rt) {
        Some(here) => Converted::Here(here),
        None => Converted::Other(node),
    };
    insert_converted(cx, parent, converted, child)
}

/// [`replace`] of native arguments, which can belong to other documents.
pub(crate) fn replace_ref(
    cx: &mut Cx<'_>,
    parent: NodeId,
    node: NodeRef,
    child: NodeRef,
) -> b::Result<NodeId> {
    let reference = reference(cx.rt, Some(child));
    let Reference::Node(old) = reference else {
        // The checks name what comes before the reference child.
        if let Some(here) = node.here(cx.rt) {
            ensure_valid_in(dom(cx.rt)?, parent, dom(cx.rt)?, here, reference, true)?;
        } else {
            ensure_valid_across(cx.rt, parent, node, reference, true)?;
        }
        return Err(not_found(
            "The node to be replaced is not a child of this node.",
        ));
    };
    if let Some(here) = node.here(cx.rt) {
        replace(cx, parent, here, old)?;
        return Ok(old);
    }
    ensure_valid_across(cx.rt, parent, node, reference, true)?;
    let here = adopt::bring_here(cx.rt, node, parent)?;
    replace(cx, parent, here, old)?;
    Ok(old)
}

/// DOM "replace" `child` of `parent` with `node`.
/// <https://dom.spec.whatwg.org/#concept-node-replace>
pub(crate) fn replace(
    cx: &mut Cx<'_>,
    parent: NodeId,
    node: NodeId,
    child: NodeId,
) -> b::Result<()> {
    ensure_valid(dom(cx.rt)?, parent, node, Some(child), true)?;
    crate::html::focus::before_removal(cx.rt, child, true)?;
    before_move(cx, node)?;
    let added = inserted(dom(cx.rt)?, node);
    let result = dom_mut(cx.rt)?.replace_child(parent, node, child);
    dom_result(cx.rt, result)?;
    Ok(crate::scripts::after_insertion(cx.rt, parent, &added)?)
}

/// DOM "replace all" `parent`'s children with `node` (`replaceChildren`).
pub(crate) fn replace_all(cx: &mut Cx<'_>, parent: NodeId, node: NodeId) -> b::Result<()> {
    ensure_valid(dom(cx.rt)?, parent, node, None, false)?;
    crate::html::focus::before_removal(cx.rt, parent, false)?;
    before_move(cx, node)?;
    let added = inserted(dom(cx.rt)?, node);
    let result = dom_mut(cx.rt)?.replace_all(parent, Some(node));
    dom_result(cx.rt, result)?;
    Ok(crate::scripts::after_insertion(cx.rt, parent, &added)?)
}

/// DOM "pre-remove": removes `child` from `parent`.
/// <https://dom.spec.whatwg.org/#concept-node-pre-remove>
pub(crate) fn pre_remove(cx: &mut Cx<'_>, parent: NodeId, child: NodeId) -> b::Result<()> {
    if dom(cx.rt)?.parent(child) != Some(parent) {
        return Err(not_found(
            "The node to be removed is not a child of this node.",
        ));
    }
    crate::html::focus::before_removal(cx.rt, child, true)?;
    let result = dom_mut(cx.rt)?.remove_child(parent, child);
    dom_result(cx.rt, result)
}

/// A node of "convert nodes into a node".
#[derive(Clone, Copy)]
enum Converted {
    /// A node of this document.
    Here(NodeId),
    /// A node of another document, alone among the arguments: the checks of the
    /// insertion come before its move.
    Other(NodeRef),
}

/// "Convert nodes into a node": the strings become text nodes of
/// `document`; a single node is the result; otherwise a new fragment gets
/// the nodes (and so takes them out of their parents).
/// <https://dom.spec.whatwg.org/#converting-nodes-into-a-node>
fn convert_nodes(
    cx: &mut Cx<'_>,
    document: NodeId,
    nodes: Vec<NodeOrDomString>,
) -> b::Result<Converted> {
    if let [NodeOrDomString::Node(only)] = nodes.as_slice() {
        return Ok(match only.here(cx.rt) {
            Some(here) => Converted::Here(here),
            None => Converted::Other(*only),
        });
    }
    let mut items = Vec::with_capacity(nodes.len());
    for item in nodes {
        items.push(match item {
            NodeOrDomString::Node(n) => Err(n),
            NodeOrDomString::DomString(text) => {
                let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::Text(text));
                Ok(dom_result(cx.rt, result)?)
            }
        });
    }
    if let [Ok(only)] = items.as_slice() {
        return Ok(Converted::Here(*only));
    }
    let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::DocumentFragment);
    let fragment = dom_result(cx.rt, result)?;
    for item in items {
        let node = match item {
            Ok(text) => text,
            Err(node) => {
                let node = node.refreshed(cx.rt).unwrap_or(node);
                if let Some(here) = node.here(cx.rt) {
                    here
                } else {
                    ensure_valid_across(cx.rt, fragment, node, Reference::None, false)?;
                    adopt::bring_here(cx.rt, node, fragment)?
                }
            }
        };
        ensure_valid(dom(cx.rt)?, fragment, node, None, false)?;
        let result = dom_mut(cx.rt)?.append_child(fragment, node);
        dom_result(cx.rt, result)?;
    }
    Ok(Converted::Here(fragment))
}

/// [`pre_insert`] of the result of [`convert_nodes`].
fn insert_converted(
    cx: &mut Cx<'_>,
    parent: NodeId,
    node: Converted,
    child: Option<NodeId>,
) -> b::Result<()> {
    match node {
        Converted::Here(here) => pre_insert(cx, parent, here, child),
        Converted::Other(node) => {
            ensure_valid_across(cx.rt, parent, node, Reference::of(child), false)?;
            let here = adopt::bring_here(cx.rt, node, parent)?;
            pre_insert(cx, parent, here, child)
        }
    }
}

/// The set of the nodes among `nodes` that belong to the current document.
fn node_set(rt: &swb_js::Runtime, nodes: &[NodeOrDomString]) -> HashSet<NodeId> {
    nodes
        .iter()
        .filter_map(|n| match n {
            NodeOrDomString::Node(n) => n.here(rt),
            NodeOrDomString::DomString(_) => None,
        })
        .collect()
}

/// `ParentNode.prepend`.
pub(super) fn prepend(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<NodeOrDomString>) -> b::Result<()> {
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    let first = dom(cx.rt)?.node(this).first_child();
    insert_converted(cx, this, node, first)
}

/// `ParentNode.append`.
pub(super) fn append(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<NodeOrDomString>) -> b::Result<()> {
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    insert_converted(cx, this, node, None)
}

/// `ParentNode.replaceChildren`.
pub(super) fn replace_children(
    cx: &mut Cx<'_>,
    this: NodeId,
    nodes: Vec<NodeOrDomString>,
) -> b::Result<()> {
    let document = dom(cx.rt)?.node_document(this);
    let node = match convert_nodes(cx, document, nodes)? {
        Converted::Here(here) => here,
        Converted::Other(node) => {
            ensure_valid_across(cx.rt, this, node, Reference::None, false)?;
            adopt::bring_here(cx.rt, node, this)?
        }
    };
    replace_all(cx, this, node)
}

/// `ChildNode.before`.
pub(super) fn before(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<NodeOrDomString>) -> b::Result<()> {
    let Some(parent) = dom(cx.rt)?.parent(this) else {
        return Ok(());
    };
    let skip = node_set(cx.rt, &nodes);
    let mut viable = dom(cx.rt)?.node(this).prev_sibling();
    while let Some(p) = viable.filter(|p| skip.contains(p)) {
        viable = dom(cx.rt)?.node(p).prev_sibling();
    }
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    let dom = dom(cx.rt)?;
    let reference = match viable {
        Some(p) => dom.node(p).next_sibling(),
        None => dom.node(parent).first_child(),
    };
    insert_converted(cx, parent, node, reference)
}

/// `ChildNode.after`.
pub(super) fn after(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<NodeOrDomString>) -> b::Result<()> {
    let Some(parent) = dom(cx.rt)?.parent(this) else {
        return Ok(());
    };
    let reference = next_not_in(cx, this, &node_set(cx.rt, &nodes))?;
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    insert_converted(cx, parent, node, reference)
}

/// `ChildNode.replaceWith`.
pub(super) fn replace_with(
    cx: &mut Cx<'_>,
    this: NodeId,
    nodes: Vec<NodeOrDomString>,
) -> b::Result<()> {
    let Some(parent) = dom(cx.rt)?.parent(this) else {
        return Ok(());
    };
    let reference = next_not_in(cx, this, &node_set(cx.rt, &nodes))?;
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    if dom(cx.rt)?.parent(this) != Some(parent) {
        return insert_converted(cx, parent, node, reference);
    }
    let node = match node {
        Converted::Here(here) => here,
        Converted::Other(node) => {
            ensure_valid_across(cx.rt, parent, node, Reference::Node(this), true)?;
            adopt::bring_here(cx.rt, node, parent)?
        }
    };
    replace(cx, parent, node, this)
}

/// `ChildNode.remove`.
pub(crate) fn remove(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
    if dom(cx.rt)?.parent(this).is_some() {
        crate::html::focus::before_removal(cx.rt, this, true)?;
        dom_mut(cx.rt)?.remove(this);
        crate::html::focus::fixup(cx.rt)?;
    }
    Ok(())
}

/// The first following sibling of `node` that is not in `skip`.
fn next_not_in(cx: &mut Cx<'_>, node: NodeId, skip: &HashSet<NodeId>) -> b::Result<Option<NodeId>> {
    let dom = dom(cx.rt)?;
    let mut next = dom.node(node).next_sibling();
    while let Some(n) = next.filter(|n| skip.contains(n)) {
        next = dom.node(n).next_sibling();
    }
    Ok(next)
}
