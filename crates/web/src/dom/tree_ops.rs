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

use std::collections::HashSet;

use swb_dom::{Document, NodeData, NodeId};

use super::{dom_result, node_name};
use crate::bindings::{self as b, Cx, Exception};
use crate::generated::types::NodeOrDomString;
use crate::state::{dom, dom_mut};

/// A `HierarchyRequestError`. The glue adds Chromium's prefix, `Failed to
/// execute 'NAME' on 'INTERFACE': `, to the message of every exception.
pub(super) fn hierarchy(message: &str) -> Exception {
    Exception::dom("HierarchyRequestError", message.to_owned())
}

/// A `NotFoundError`.
pub(super) fn not_found(message: &str) -> Exception {
    Exception::dom("NotFoundError", message.to_owned())
}

/// The type message for a node that the parent cannot take.
fn type_message(dom: &Document, node: NodeId, parent: NodeId) -> String {
    format!(
        "Nodes of type '{}' may not be inserted inside nodes of type '{}'.",
        node_name(&dom.node(node).data),
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
    let parent_is_document = match dom.node(parent).data {
        NodeData::Document => true,
        NodeData::DocumentFragment | NodeData::Element(_) => false,
        _ => return Err(hierarchy("This node type does not support this method.")),
    };
    let node_is_document = matches!(dom.node(node).data, NodeData::Document);
    if !(parent_is_document && node_is_document)
        && dom.is_host_including_inclusive_ancestor(node, parent)
    {
        return Err(hierarchy("The new child element contains the parent."));
    }
    if let Some(child) = child
        && dom.parent(child) != Some(parent)
    {
        return Err(not_found(if replace {
            "The node to be replaced is not a child of this node."
        } else {
            "The node before which the new node is to be inserted is not a child of this node."
        }));
    }
    let type_error = || hierarchy(&type_message(dom, node, parent));
    match &dom.node(node).data {
        NodeData::Document => return Err(type_error()),
        NodeData::Doctype { .. } if !parent_is_document => return Err(type_error()),
        NodeData::Text(_) if parent_is_document => return Err(type_error()),
        _ => {}
    }
    if parent_is_document {
        check_document_child(dom, parent, node, child, replace)?;
    }
    Ok(())
}

/// The rules for a child of a document: one element, one doctype, the
/// doctype before the element, no text.
fn check_document_child(
    dom: &Document,
    parent: NodeId,
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
    match &dom.node(node).data {
        NodeData::Element(_) => element_rule(),
        NodeData::DocumentFragment => {
            let mut elements = 0;
            for c in dom.children(node) {
                if matches!(dom.node(c).data, NodeData::Text(_)) {
                    return Err(hierarchy(&type_message(dom, c, parent)));
                }
                elements += usize::from(is_element(c));
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

/// DOM "pre-insert" `node` into `parent` before `child`, then the steps
/// of the script elements that the insertion connected.
/// <https://dom.spec.whatwg.org/#concept-node-pre-insert>
pub(super) fn pre_insert(
    cx: &mut Cx<'_>,
    parent: NodeId,
    node: NodeId,
    child: Option<NodeId>,
) -> b::Result<()> {
    ensure_valid(dom(cx.rt)?, parent, node, child, false)?;
    let added = inserted(dom(cx.rt)?, node);
    let result = dom_mut(cx.rt)?.insert_before(parent, node, child);
    dom_result(cx.rt, result)?;
    Ok(crate::scripts::after_insertion(cx.rt, parent, &added)?)
}

/// DOM "replace" `child` of `parent` with `node`.
/// <https://dom.spec.whatwg.org/#concept-node-replace>
pub(super) fn replace(
    cx: &mut Cx<'_>,
    parent: NodeId,
    node: NodeId,
    child: NodeId,
) -> b::Result<()> {
    ensure_valid(dom(cx.rt)?, parent, node, Some(child), true)?;
    let added = inserted(dom(cx.rt)?, node);
    let result = dom_mut(cx.rt)?.replace_child(parent, node, child);
    dom_result(cx.rt, result)?;
    Ok(crate::scripts::after_insertion(cx.rt, parent, &added)?)
}

/// DOM "replace all" `parent`'s children with `node` (`replaceChildren`).
fn replace_all(cx: &mut Cx<'_>, parent: NodeId, node: NodeId) -> b::Result<()> {
    ensure_valid(dom(cx.rt)?, parent, node, None, false)?;
    let added = inserted(dom(cx.rt)?, node);
    let result = dom_mut(cx.rt)?.replace_all(parent, Some(node));
    dom_result(cx.rt, result)?;
    Ok(crate::scripts::after_insertion(cx.rt, parent, &added)?)
}

/// DOM "pre-remove": removes `child` from `parent`.
/// <https://dom.spec.whatwg.org/#concept-node-pre-remove>
pub(super) fn pre_remove(cx: &mut Cx<'_>, parent: NodeId, child: NodeId) -> b::Result<()> {
    if dom(cx.rt)?.parent(child) != Some(parent) {
        return Err(not_found(
            "The node to be removed is not a child of this node.",
        ));
    }
    let result = dom_mut(cx.rt)?.remove_child(parent, child);
    dom_result(cx.rt, result)
}

/// "Convert nodes into a node": the strings become text nodes of
/// `document`; a single node is the result; otherwise a new fragment gets
/// the nodes (and so takes them out of their parents).
/// <https://dom.spec.whatwg.org/#converting-nodes-into-a-node>
fn convert_nodes(
    cx: &mut Cx<'_>,
    document: NodeId,
    nodes: Vec<NodeOrDomString>,
) -> b::Result<NodeId> {
    let mut items = Vec::with_capacity(nodes.len());
    for item in nodes {
        items.push(match item {
            NodeOrDomString::Node(n) => n,
            NodeOrDomString::DomString(text) => {
                let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::Text(text));
                dom_result(cx.rt, result)?
            }
        });
    }
    if let [only] = items.as_slice() {
        return Ok(*only);
    }
    let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::DocumentFragment);
    let fragment = dom_result(cx.rt, result)?;
    for node in items {
        ensure_valid(dom(cx.rt)?, fragment, node, None, false)?;
        let result = dom_mut(cx.rt)?.append_child(fragment, node);
        dom_result(cx.rt, result)?;
    }
    Ok(fragment)
}

/// The set of the nodes among `nodes`.
fn node_set(nodes: &[NodeOrDomString]) -> HashSet<NodeId> {
    nodes
        .iter()
        .filter_map(|n| match n {
            NodeOrDomString::Node(n) => Some(*n),
            NodeOrDomString::DomString(_) => None,
        })
        .collect()
}

/// `ParentNode.prepend`.
pub(super) fn prepend(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<NodeOrDomString>) -> b::Result<()> {
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    let first = dom(cx.rt)?.node(this).first_child();
    pre_insert(cx, this, node, first)
}

/// `ParentNode.append`.
pub(super) fn append(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<NodeOrDomString>) -> b::Result<()> {
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    pre_insert(cx, this, node, None)
}

/// `ParentNode.replaceChildren`.
pub(super) fn replace_children(
    cx: &mut Cx<'_>,
    this: NodeId,
    nodes: Vec<NodeOrDomString>,
) -> b::Result<()> {
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    replace_all(cx, this, node)
}

/// `ChildNode.before`.
pub(super) fn before(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<NodeOrDomString>) -> b::Result<()> {
    let Some(parent) = dom(cx.rt)?.parent(this) else {
        return Ok(());
    };
    let skip = node_set(&nodes);
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
    pre_insert(cx, parent, node, reference)
}

/// `ChildNode.after`.
pub(super) fn after(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<NodeOrDomString>) -> b::Result<()> {
    let Some(parent) = dom(cx.rt)?.parent(this) else {
        return Ok(());
    };
    let reference = next_not_in(cx, this, &node_set(&nodes))?;
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    pre_insert(cx, parent, node, reference)
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
    let reference = next_not_in(cx, this, &node_set(&nodes))?;
    let document = dom(cx.rt)?.node_document(this);
    let node = convert_nodes(cx, document, nodes)?;
    if dom(cx.rt)?.parent(this) == Some(parent) {
        replace(cx, parent, node, this)
    } else {
        pre_insert(cx, parent, node, reference)
    }
}

/// `ChildNode.remove`.
pub(super) fn remove(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
    if dom(cx.rt)?.parent(this).is_some() {
        dom_mut(cx.rt)?.remove(this);
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
