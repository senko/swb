//! The DOM interfaces (DOM Standard, <https://dom.spec.whatwg.org/>): the
//! read-only tree accessors of `Node`, `Element`, `CharacterData` and the
//! mixins `ParentNode` and `NonDocumentTypeChildNode`, `CharacterData`'s
//! `data`, `DocumentType` and `ProcessingInstruction` (feature 4c2).
//! `Document` is in `document.rs`, `NodeList` in `node_list.rs`.
//!
//! Each function reads the lent DOM ([`crate::state::dom`]); a function
//! that returns a node returns its `NodeId`, and the glue wraps it.

mod document;
mod node_list;

use swb_dom::{DomError, NodeData, NodeId, ns};
use swb_js::Runtime;

use crate::bindings::{self as b, Cx, Exception};
use crate::generated as g;
use crate::generated::types::GetRootNodeOptions;
use crate::state::{Slot, WebState, dom, dom_mut, engine_mut, same_object};

/// The result of a change function as a binding result: the DOM errors
/// become `DOMException`s, the limits end the task (ADR 0028 section 16).
pub(crate) fn dom_result<T>(rt: &mut Runtime, result: Result<T, DomError>) -> b::Result<T> {
    result.map_err(|error| match error {
        DomError::HierarchyRequest => Exception::dom(
            "HierarchyRequestError",
            "The operation would yield an incorrect node tree.",
        ),
        DomError::NotFound => Exception::dom(
            "NotFoundError",
            "The node to be removed is not a child of this node.",
        ),
        DomError::InvalidNodeType => {
            Exception::dom("InvalidNodeTypeError", "The node is of the wrong type.")
        }
        DomError::NotSupported => {
            Exception::dom("NotSupportedError", "The node cannot be adopted.")
        }
        DomError::IndexSize => Exception::dom("IndexSizeError", "The offset is out of range."),
        DomError::NodeLimit(_) => rt.terminate("the DOM node limit").into(),
        DomError::ByteLimit(_) => rt.terminate("the DOM byte limit").into(),
        DomError::WrongDocument => Exception::type_error("the document of another parser"),
    })
}

/// The number of UTF-16 code units of `text` (the length of a DOM string).
fn utf16_len(text: &str) -> u32 {
    let units: usize = text.chars().map(char::len_utf16).sum();
    u32::try_from(units).unwrap_or(u32::MAX)
}

/// DOM `nodeName`: the HTML-uppercased qualified name of an element (all
/// documents of swb are HTML documents), `#text` and so on for the other
/// kinds. <https://dom.spec.whatwg.org/#dom-node-nodename>
fn node_name(data: &NodeData) -> String {
    match data {
        NodeData::Element(_) => tag_name(data),
        NodeData::Text(_) => "#text".to_owned(),
        NodeData::Comment(_) => "#comment".to_owned(),
        NodeData::ProcessingInstruction { target, .. } => target.clone(),
        NodeData::Doctype { name, .. } => name.clone(),
        NodeData::Document => "#document".to_owned(),
        NodeData::DocumentFragment => "#document-fragment".to_owned(),
    }
}

/// Element `tagName`: the qualified name, in ASCII uppercase for an
/// element in the HTML namespace.
/// <https://dom.spec.whatwg.org/#dom-element-tagname>
fn tag_name(data: &NodeData) -> String {
    let NodeData::Element(e) = data else {
        return String::new();
    };
    let mut name = match &e.name.prefix {
        Some(prefix) => format!("{prefix}:{}", e.name.local),
        None => e.name.local.to_string(),
    };
    if e.name.ns == ns!(html) {
        name.make_ascii_uppercase();
    }
    name
}

impl g::node::NodeImpl for WebState {
    fn node_type(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u16> {
        Ok(match dom(cx.rt)?.node(this).data {
            NodeData::Element(_) => 1,
            NodeData::Text(_) => 3,
            NodeData::ProcessingInstruction { .. } => 7,
            NodeData::Comment(_) => 8,
            NodeData::Document => 9,
            NodeData::Doctype { .. } => 10,
            NodeData::DocumentFragment => 11,
        })
    }

    fn node_name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(node_name(&dom(cx.rt)?.node(this).data))
    }

    fn base_uri(cx: &mut Cx<'_>, _this: NodeId) -> b::Result<String> {
        Ok(engine_mut(cx.rt)?.base_url())
    }

    fn is_connected(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(dom(cx.rt)?.is_connected(this))
    }

    fn owner_document(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        let dom = dom(cx.rt)?;
        Ok(match dom.node(this).data {
            NodeData::Document => None,
            _ => Some(dom.node_document(this)),
        })
    }

    fn get_root_node(
        cx: &mut Cx<'_>,
        this: NodeId,
        _options: GetRootNodeOptions,
    ) -> b::Result<NodeId> {
        // Without shadow trees, `composed` changes nothing.
        Ok(dom(cx.rt)?.root(this))
    }

    fn parent_node(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(dom(cx.rt)?.parent(this))
    }

    fn parent_element(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(dom(cx.rt)?.parent_element(this))
    }

    fn has_child_nodes(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(dom(cx.rt)?.node(this).first_child().is_some())
    }

    fn child_nodes(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        let realm = cx.realm();
        Ok(same_object(cx.rt, this, Slot::ChildNodes, |rt| {
            node_list::new_child_list(rt, realm, this)
        })?)
    }

    fn first_child(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(dom(cx.rt)?.node(this).first_child())
    }

    fn last_child(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(dom(cx.rt)?.node(this).last_child())
    }

    fn previous_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(dom(cx.rt)?.node(this).prev_sibling())
    }

    fn next_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(dom(cx.rt)?.node(this).next_sibling())
    }

    fn is_same_node(_cx: &mut Cx<'_>, this: NodeId, other: Option<NodeId>) -> b::Result<bool> {
        Ok(other == Some(this))
    }

    /// <https://dom.spec.whatwg.org/#dom-node-contains>: `other` is an
    /// inclusive descendant of this node.
    fn contains(cx: &mut Cx<'_>, this: NodeId, other: Option<NodeId>) -> b::Result<bool> {
        let Some(other) = other else {
            return Ok(false);
        };
        let dom = dom(cx.rt)?;
        Ok(other == this || dom.ancestors(other).any(|a| a == this))
    }
}

/// `ParentNode.firstElementChild`.
fn first_element_child(cx: &Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
    Ok(dom(cx.rt)?.element_children(this).next())
}

/// `ParentNode.lastElementChild`.
fn last_element_child(cx: &Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
    Ok(dom(cx.rt)?.element_children(this).last())
}

/// `ParentNode.childElementCount`.
fn child_element_count(cx: &Cx<'_>, this: NodeId) -> b::Result<u32> {
    let count = dom(cx.rt)?.element_children(this).count();
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

/// `NonDocumentTypeChildNode.previousElementSibling`.
fn previous_element_sibling(cx: &Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
    Ok(dom(cx.rt)?.prev_sibling_element(this))
}

/// `NonDocumentTypeChildNode.nextElementSibling`.
fn next_element_sibling(cx: &Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
    Ok(dom(cx.rt)?.next_sibling_element(this))
}

impl g::element::ElementImpl for WebState {
    fn namespace_uri(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<String>> {
        let dom = dom(cx.rt)?;
        let ns = dom.element(this).map(|e| e.name.ns.to_string());
        Ok(ns.filter(|ns| !ns.is_empty()))
    }

    fn prefix(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<String>> {
        let dom = dom(cx.rt)?;
        Ok(dom
            .element(this)
            .and_then(|e| e.name.prefix.as_ref())
            .map(ToString::to_string))
    }

    fn local_name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let dom = dom(cx.rt)?;
        Ok(dom
            .element(this)
            .map(|e| e.name.local.to_string())
            .unwrap_or_default())
    }

    fn tag_name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(tag_name(&dom(cx.rt)?.node(this).data))
    }

    fn first_element_child(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        first_element_child(cx, this)
    }

    fn last_element_child(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        last_element_child(cx, this)
    }

    fn child_element_count(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        child_element_count(cx, this)
    }

    fn previous_element_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        previous_element_sibling(cx, this)
    }

    fn next_element_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        next_element_sibling(cx, this)
    }
}

impl g::document_fragment::DocumentFragmentImpl for WebState {
    fn first_element_child(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        first_element_child(cx, this)
    }

    fn last_element_child(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        last_element_child(cx, this)
    }

    fn child_element_count(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        child_element_count(cx, this)
    }
}

impl g::character_data::CharacterDataImpl for WebState {
    fn data(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(dom(cx.rt)?.char_data(this).unwrap_or_default().to_owned())
    }

    fn set_data(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        let result = dom_mut(cx.rt)?.set_data(this, &value);
        dom_result(cx.rt, result)
    }

    fn length(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        Ok(utf16_len(dom(cx.rt)?.char_data(this).unwrap_or_default()))
    }

    fn previous_element_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        previous_element_sibling(cx, this)
    }

    fn next_element_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        next_element_sibling(cx, this)
    }
}

impl g::document_type::DocumentTypeImpl for WebState {
    fn name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match &dom(cx.rt)?.node(this).data {
            NodeData::Doctype { name, .. } => name.clone(),
            _ => String::new(),
        })
    }

    fn public_id(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match &dom(cx.rt)?.node(this).data {
            NodeData::Doctype { public_id, .. } => public_id.clone(),
            _ => String::new(),
        })
    }

    fn system_id(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match &dom(cx.rt)?.node(this).data {
            NodeData::Doctype { system_id, .. } => system_id.clone(),
            _ => String::new(),
        })
    }
}

impl g::processing_instruction::ProcessingInstructionImpl for WebState {
    fn target(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match &dom(cx.rt)?.node(this).data {
            NodeData::ProcessingInstruction { target, .. } => target.clone(),
            _ => String::new(),
        })
    }
}

impl g::text::TextImpl for WebState {}

impl g::comment::CommentImpl for WebState {}

impl g::html_element::HTMLElementImpl for WebState {}

impl g::html_head_element::HTMLHeadElementImpl for WebState {}
