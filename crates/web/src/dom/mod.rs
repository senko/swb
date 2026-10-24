//! The DOM interfaces (DOM Standard, <https://dom.spec.whatwg.org/>) for
//! scripts: `Node` with the tree changes (`tree_ops.rs`), `Element`
//! (`element.rs`) with its attributes (`attrs.rs`, `token_list.rs`,
//! `string_map.rs`), `CharacterData`, `Text`, `Comment`, `DocumentType`
//! and `ProcessingInstruction` (`character_data.rs`), `Document`
//! (`document.rs`), the live lists `NodeList` and `HTMLCollection`
//! (`lists.rs`), the selector queries (`queries.rs`), markup and
//! `insertAdjacent*` (`markup.rs`), and `DOMImplementation`, `DOMParser`
//! and `template.content` (`implementation.rs`). Features 4c2 (the
//! read-only accessors), 5c (`appendChild` for scripts that insert
//! scripts), 6a (the rest) and 6b (queries, markup, more documents).
//!
//! Each function reads the lent DOM ([`crate::state::dom`]); a function
//! that returns a node returns its `NodeId`, and the glue wraps it.

#[macro_use]
mod macros;
mod attrs;
mod character_data;
mod document;
mod element;
pub(crate) mod element_lists;
mod implementation;
pub(crate) mod lists;
mod markup;
mod queries;
pub(crate) mod string_map;
pub(crate) mod token_list;
pub(crate) mod tree_ops;

use swb_dom::{DomError, NodeData, NodeId, ns};
use swb_js::Runtime;

pub(crate) use attrs::{remove_attribute, set_attribute};
pub(crate) use element_lists::ElementList;
pub(crate) use lists::DocCollection;

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
pub(super) fn node_name(data: &NodeData) -> String {
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

/// DOM "string replace all": replaces the children of `parent` with a
/// text node of `value` (none for the empty string); the steps of a
/// `script` parent follow (HTML "children changed").
/// <https://dom.spec.whatwg.org/#string-replace-all>
pub(crate) fn string_replace_all(cx: &mut Cx<'_>, parent: NodeId, value: &str) -> b::Result<()> {
    // Chromium keeps the node (and makes no change) when the only child is
    // a Text node that already has this data (measured in
    // `tools/probes/web-dom-mutation.json`; every other case makes a new
    // node). The children changed steps of a `script` parent still run.
    let same = {
        let dom = dom(cx.rt)?;
        let mut children = dom.children(parent);
        match (children.next(), children.next()) {
            (Some(only), None) => matches!(&dom.node(only).data, NodeData::Text(t) if t == value),
            _ => false,
        }
    };
    if same && !value.is_empty() {
        return Ok(crate::scripts::after_insertion(cx.rt, parent, &[])?);
    }
    let node = if value.is_empty() {
        None
    } else {
        let document = dom(cx.rt)?.node_document(parent);
        let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::Text(value.to_owned()));
        Some(dom_result(cx.rt, result)?)
    };
    let result = dom_mut(cx.rt)?.replace_all(parent, node);
    dom_result(cx.rt, result)?;
    let added: Vec<NodeId> = node.into_iter().collect();
    Ok(crate::scripts::after_insertion(cx.rt, parent, &added)?)
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
            lists::new_list(rt, realm, this, lists::ListKind::ChildNodes)
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

    /// <https://dom.spec.whatwg.org/#dom-node-nodevalue>: the data of a
    /// character data node, else null.
    fn node_value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<String>> {
        Ok(dom(cx.rt)?.char_data(this).map(str::to_owned))
    }

    fn set_node_value(cx: &mut Cx<'_>, this: NodeId, value: Option<String>) -> b::Result<()> {
        if dom(cx.rt)?.char_data(this).is_none() {
            return Ok(());
        }
        character_data::set_data(cx, this, &value.unwrap_or_default())
    }

    /// <https://dom.spec.whatwg.org/#dom-node-textcontent>
    fn text_content(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<String>> {
        let dom = dom(cx.rt)?;
        Ok(match dom.node(this).data {
            NodeData::Document | NodeData::Doctype { .. } => None,
            NodeData::Element(_) | NodeData::DocumentFragment => Some(dom.text_content(this)),
            _ => dom.char_data(this).map(str::to_owned),
        })
    }

    fn set_text_content(cx: &mut Cx<'_>, this: NodeId, value: Option<String>) -> b::Result<()> {
        let value = value.unwrap_or_default();
        match dom(cx.rt)?.node(this).data {
            NodeData::Element(_) | NodeData::DocumentFragment => {
                string_replace_all(cx, this, &value)
            }
            NodeData::Document | NodeData::Doctype { .. } => Ok(()),
            _ => character_data::set_data(cx, this, &value),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-node-normalize>
    fn normalize(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        let result = dom_mut(cx.rt)?.normalize(this);
        dom_result(cx.rt, result)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-clonenode>
    fn clone_node(cx: &mut Cx<'_>, this: NodeId, subtree: bool) -> b::Result<NodeId> {
        clone(cx, this, this, subtree)
    }

    fn is_equal_node(cx: &mut Cx<'_>, this: NodeId, other: Option<NodeId>) -> b::Result<bool> {
        Ok(other.is_some_and(|other| dom(cx.rt).is_ok_and(|d| d.is_equal_node(this, other))))
    }

    fn is_same_node(_cx: &mut Cx<'_>, this: NodeId, other: Option<NodeId>) -> b::Result<bool> {
        Ok(other == Some(this))
    }

    /// <https://dom.spec.whatwg.org/#dom-node-comparedocumentposition>
    fn compare_document_position(cx: &mut Cx<'_>, this: NodeId, other: NodeId) -> b::Result<u16> {
        Ok(dom(cx.rt)?.compare_position(this, other))
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

    /// <https://dom.spec.whatwg.org/#dom-node-lookupprefix>
    fn lookup_prefix(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
    ) -> b::Result<Option<String>> {
        let Some(namespace) = namespace.filter(|n| !n.is_empty()) else {
            return Ok(None);
        };
        let dom = dom(cx.rt)?;
        Ok(match context_element(dom, this) {
            Some(element) => locate_prefix(dom, element, &namespace),
            None => None,
        })
    }

    /// <https://dom.spec.whatwg.org/#dom-node-lookupnamespaceuri>
    fn lookup_namespace_uri(
        cx: &mut Cx<'_>,
        this: NodeId,
        prefix: Option<String>,
    ) -> b::Result<Option<String>> {
        let prefix = prefix.filter(|p| !p.is_empty());
        let dom = dom(cx.rt)?;
        Ok(locate_namespace(dom, this, prefix.as_deref()))
    }

    /// <https://dom.spec.whatwg.org/#dom-node-isdefaultnamespace>
    fn is_default_namespace(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
    ) -> b::Result<bool> {
        let namespace = namespace.filter(|n| !n.is_empty());
        let dom = dom(cx.rt)?;
        Ok(locate_namespace(dom, this, None) == namespace)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-insertbefore>
    fn insert_before(
        cx: &mut Cx<'_>,
        this: NodeId,
        node: NodeId,
        child: Option<NodeId>,
    ) -> b::Result<NodeId> {
        tree_ops::pre_insert(cx, this, node, child)?;
        Ok(node)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-appendchild>: DOM "append",
    /// then the steps of inserted script elements (they run now, or start
    /// their fetch).
    fn append_child(cx: &mut Cx<'_>, this: NodeId, node: NodeId) -> b::Result<NodeId> {
        tree_ops::pre_insert(cx, this, node, None)?;
        Ok(node)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-replacechild>
    fn replace_child(
        cx: &mut Cx<'_>,
        this: NodeId,
        node: NodeId,
        child: NodeId,
    ) -> b::Result<NodeId> {
        tree_ops::replace(cx, this, node, child)?;
        Ok(child)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-removechild>
    fn remove_child(cx: &mut Cx<'_>, this: NodeId, child: NodeId) -> b::Result<NodeId> {
        tree_ops::pre_remove(cx, this, child)?;
        Ok(child)
    }
}

const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

/// The element that DOM "locate a namespace prefix" starts at: the node
/// itself for an element, the document element for a document, the parent
/// element for the other nodes, none for a doctype and a fragment.
fn context_element(dom: &swb_dom::Document, node: NodeId) -> Option<NodeId> {
    match dom.node(node).data {
        NodeData::Element(_) => Some(node),
        NodeData::Document => dom.element_children(node).next(),
        NodeData::Doctype { .. } | NodeData::DocumentFragment => None,
        _ => dom.parent_element(node),
    }
}

/// DOM "locate a namespace prefix" for `element`.
/// <https://dom.spec.whatwg.org/#locate-a-namespace-prefix>
fn locate_prefix(dom: &swb_dom::Document, element: NodeId, namespace: &str) -> Option<String> {
    let mut current = Some(element);
    while let Some(el) = current {
        let data = dom.element(el)?;
        if &*data.name.ns == namespace
            && let Some(prefix) = &data.name.prefix
        {
            return Some(prefix.to_string());
        }
        let declared = data
            .attributes()
            .iter()
            .find(|a| a.name.prefix.as_deref() == Some("xmlns") && a.value == namespace);
        if let Some(attr) = declared {
            return Some(attr.name.local.to_string());
        }
        current = dom.parent_element(el);
    }
    None
}

/// DOM "locate a namespace" for `node` and `prefix` (none for the default
/// namespace).
/// <https://dom.spec.whatwg.org/#locate-a-namespace>
fn locate_namespace(dom: &swb_dom::Document, node: NodeId, prefix: Option<&str>) -> Option<String> {
    let mut current = context_element(dom, node);
    while let Some(el) = current {
        let data = dom.element(el)?;
        if !data.name.ns.is_empty() && data.name.prefix.as_deref() == prefix {
            return Some(data.name.ns.to_string());
        }
        let declared = data.attributes().iter().find(|a| {
            &*a.name.ns == XMLNS_NS
                && match prefix {
                    Some(p) => a.name.prefix.as_deref() == Some("xmlns") && &*a.name.local == p,
                    None => a.name.prefix.is_none() && &*a.name.local == "xmlns",
                }
        });
        if let Some(attr) = declared {
            return Some(attr.value.clone()).filter(|v| !v.is_empty());
        }
        current = dom.parent_element(el);
    }
    None
}

/// DOM "clone a node" with the node document `document`, then the cloning
/// steps of script elements: the copy of a script that has started has
/// started too (HTML §4.12.1.1).
/// <https://dom.spec.whatwg.org/#concept-node-clone>
pub(super) fn clone(
    cx: &mut Cx<'_>,
    node: NodeId,
    document: NodeId,
    deep: bool,
) -> b::Result<NodeId> {
    let result = dom_mut(cx.rt)?.clone_node(node, document, deep);
    let cloned = dom_result(cx.rt, result)?;
    for (original, copy) in cloned.scripts {
        if crate::scripts::has_started(cx.rt, original)? {
            dom_mut(cx.rt)?.mark_script_started(copy);
        }
    }
    // The cloning steps of `input` and `textarea` (HTML §4.10.5, §4.10.11):
    // the copy gets the value and the checkedness with their dirty flags.
    for (original, copy) in cloned.controls {
        if engine_mut(cx.rt)?.copy_control_state(original, copy) == crate::ControlWrite::OverLimit {
            return Err(cx.rt.terminate("the budget of form control values").into());
        }
    }
    Ok(cloned.root)
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

/// `ParentNode.children`: the one `HTMLCollection` of the node.
fn children_collection(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
    let realm = cx.realm();
    Ok(same_object(cx.rt, this, Slot::Children, |rt| {
        lists::new_list(rt, realm, this, lists::ListKind::Children)
    })?)
}

impl g::document_fragment::DocumentFragmentImpl for WebState {
    parent_node_methods!("DocumentFragment");

    /// <https://dom.spec.whatwg.org/#dom-nonelementparentnode-getelementbyid>:
    /// the first descendant element with the id (a fragment is not
    /// connected, so it has no index).
    fn get_element_by_id(
        cx: &mut Cx<'_>,
        this: NodeId,
        element_id: String,
    ) -> b::Result<Option<NodeId>> {
        if element_id.is_empty() {
            return Ok(None);
        }
        let dom = dom(cx.rt)?;
        Ok(dom.descendants(this).find(|&n| {
            dom.element(n).and_then(swb_dom::ElementData::id) == Some(element_id.as_str())
        }))
    }

    /// `new DocumentFragment()`: a fragment of the window's document.
    fn constructor(cx: &mut Cx<'_>) -> b::Result<swb_js::Gc<swb_js::Object>> {
        let result = dom_mut(cx.rt)?.create_node(NodeData::DocumentFragment);
        let node = dom_result(cx.rt, result)?;
        let realm = cx.realm();
        Ok(<WebState as b::Host>::wrap_node(cx.rt, realm, node)?)
    }
}
