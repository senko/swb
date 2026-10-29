//! `innerText` and `outerText` (HTML §3.2.7, "The innerText and outerText
//! properties",
//! <https://html.spec.whatwg.org/multipage/dom.html#the-innertext-idl-attribute>;
//! M8 feature 7b2).
//!
//! The getter is the rendered text of the element, which the engine
//! computes from the styles of the document ([`crate::EngineHost::inner_text`]:
//! the rendered text collection steps); an element that is not being
//! rendered gives its descendant text content. The engine's flush can take
//! long in one native call, so the deadline of the task is checked after
//! it. The setters replace the children (or the element) with the
//! "rendered text fragment" of the value: text nodes and `br` elements.
//!
//! Measured in Chromium 148 (`tools/probes/web-inner-text.json`): `\r\n`,
//! `\n` and `\r` each give one `br`; an empty value removes all children;
//! `outerText` merges the new text with the text nodes next to it, leaves
//! one empty text node for an empty value, and throws
//! `NoModificationAllowedError` ("The element has no parent.") for an
//! element without a parent.

use swb_dom::{ElementData, NodeData, NodeId, QualName, local_name, ns};
use swb_js::Runtime;

use crate::bindings::{self as b, Cx};
use crate::dom::dom_result;
use crate::dom::tree_ops::{pre_remove, replace, replace_all};
use crate::state::{dom, dom_mut, engine_mut};

/// The `innerText` and `outerText` getters.
pub(crate) fn get(rt: &mut Runtime, element: NodeId) -> b::Result<String> {
    let text = engine_mut(rt)?.inner_text(element);
    rt.check_time()?;
    match text {
        Some(text) => Ok(text),
        // Not being rendered: the descendant text content.
        None => Ok(dom(rt)?.text_content(element)),
    }
}

/// The `innerText` setter: HTML "set the inner text steps".
pub(crate) fn set_inner(cx: &mut Cx<'_>, element: NodeId, value: &str) -> b::Result<()> {
    let fragment = rendered_text_fragment(cx, element, value)?;
    replace_all(cx, element, fragment)
}

/// The `outerText` setter: replaces the element with the rendered text
/// fragment and merges the text nodes at its edges.
pub(crate) fn set_outer(cx: &mut Cx<'_>, element: NodeId, value: &str) -> b::Result<()> {
    let (parent, previous, next) = {
        let dom = dom(cx.rt)?;
        let Some(parent) = dom.parent(element) else {
            return Err(b::Exception::dom(
                "NoModificationAllowedError",
                "The element has no parent.",
            ));
        };
        let node = dom.node(element);
        (parent, node.prev_sibling(), node.next_sibling())
    };
    let fragment = rendered_text_fragment(cx, element, value)?;
    if dom(cx.rt)?.children(fragment).next().is_none() {
        let text = new_text(cx, element, "")?;
        let result = dom_mut(cx.rt)?.append_child(fragment, text);
        dom_result(cx.rt, result)?;
    }
    replace(cx, parent, fragment, element)?;
    if let Some(next) = next {
        merge_with_previous(cx, next)?;
    }
    if let Some(previous) = previous
        && let Some(after) = dom(cx.rt)?.node(previous).next_sibling()
    {
        merge_with_previous(cx, after)?;
    }
    Ok(())
}

/// HTML "merge with the next text node" for the text node before `node`:
/// if both are text nodes, the data of `node` goes to the end of the one
/// before it and `node` is removed.
fn merge_with_previous(cx: &mut Cx<'_>, node: NodeId) -> b::Result<()> {
    let (previous, data, parent) = {
        let dom = dom(cx.rt)?;
        let Some(NodeData::Text(data)) = dom.get(node).map(|n| &n.data) else {
            return Ok(());
        };
        let Some(previous) = dom.node(node).prev_sibling() else {
            return Ok(());
        };
        if !matches!(dom.node(previous).data, NodeData::Text(_)) {
            return Ok(());
        }
        let Some(parent) = dom.parent(node) else {
            return Ok(());
        };
        (previous, data.clone(), parent)
    };
    let result = dom_mut(cx.rt)?.append_data(previous, &data);
    dom_result(cx.rt, result)?;
    pre_remove(cx, parent, node)
}

/// HTML "rendered text fragment": a fragment of the node document of
/// `element` with a text node for each run of the value between line
/// breaks (`\r\n`, `\n` or `\r`) and a `br` element for each line break.
fn rendered_text_fragment(cx: &mut Cx<'_>, element: NodeId, value: &str) -> b::Result<NodeId> {
    let document = dom(cx.rt)?.node_document(element);
    let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::DocumentFragment);
    let fragment = dom_result(cx.rt, result)?;
    let mut rest = value;
    loop {
        let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
        if end > 0 {
            let text = new_text(cx, element, &rest[..end])?;
            let result = dom_mut(cx.rt)?.append_child(fragment, text);
            dom_result(cx.rt, result)?;
        }
        let Some(after) = rest.get(end..).filter(|r| !r.is_empty()) else {
            break;
        };
        let skip = if after.starts_with("\r\n") { 2 } else { 1 };
        let name = QualName::new(None, ns!(html), local_name!("br"));
        let data = NodeData::Element(ElementData::new(name, Vec::new()));
        let result = dom_mut(cx.rt)?.create_node_in(document, data);
        let br = dom_result(cx.rt, result)?;
        let result = dom_mut(cx.rt)?.append_child(fragment, br);
        dom_result(cx.rt, result)?;
        rest = &after[skip..];
    }
    Ok(fragment)
}

/// A new text node of the node document of `element`.
fn new_text(cx: &mut Cx<'_>, element: NodeId, data: &str) -> b::Result<NodeId> {
    let document = dom(cx.rt)?.node_document(element);
    let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::Text(data.to_owned()));
    dom_result(cx.rt, result)
}
