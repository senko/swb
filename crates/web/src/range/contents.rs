//! The contents operations of `Range` (`cloneContents`, `extractContents`,
//! `deleteContents`, `insertNode`, `surroundContents`; DOM §5.5),
//! `createContextualFragment` (HTML "dynamic markup insertion") and the
//! deletion of `Selection.deleteFromDocument` (M10 sub-feature 9b-3, ADR 0039
//! section 2).
//!
//! The DOM does the work in the arena of the range
//! (`swb_dom::Document::extract_contents` and the others, `ranges/contents.rs`
//! of `swb_dom`), whose document is the current one for the call. This module
//! adds Chromium's exceptions (measured, case `contents-errors` of
//! `tools/probes/web-range.json`), what a removal and an insertion do outside
//! the DOM (the frames and the focus in the removed contents, the steps of
//! inserted scripts), and the cloning steps and upgrade reactions of the
//! copies.

use swb_dom::{LiveRange, NodeData, NodeId, local_name};
use swb_js::{Gc, Object, Runtime};

use super::{arena_or_error, current, illegal, no_parent_error, store};
use crate::bindings::{self as b, Cx, Exception, NodeRef, Platform};
use crate::custom_elements::upgrade;
use crate::dom::{cloning_steps, dom_result, node_name, tree_ops};
use crate::node_iterator::wrapper_in;
use crate::state::{self, DocKey, dom, dom_mut};

/// Runs `body` with the document of the range `this` as the current one;
/// `body` gets the range's id, the document and the boundary points.
fn in_range_document<T>(
    cx: &mut Cx<'_>,
    this: Platform,
    body: impl FnOnce(&mut Cx<'_>, u32, DocKey, LiveRange) -> b::Result<T>,
) -> b::Result<T> {
    let (id, key, range) = current(cx.rt, this)?;
    let entered = state::switch_if_other(cx.rt, key)?;
    let result = body(cx, id, key, range);
    state::leave(cx.rt, entered);
    result
}

/// The name of the doctype that `range` contains, if any (only a child of a
/// document can be one).
fn doctype_in(dom: &swb_dom::Document, range: LiveRange) -> Option<String> {
    if !dom.range_contains_doctype(range) {
        return None;
    }
    let root = dom.root(range.start.node);
    dom.children(root).find_map(|n| match &dom.node(n).data {
        NodeData::Doctype { name, .. } => Some(name.clone()),
        _ => None,
    })
}

/// `Range.cloneContents`.
pub(super) fn clone_contents(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
    in_range_document(cx, this, |cx, _, key, range| {
        // Chromium appends the copy of the doctype to the fragment, which
        // refuses it (measured).
        if let Some(name) = doctype_in(dom(cx.rt)?, range) {
            return Err(tree_ops::hierarchy(&format!(
                "Nodes of type '{name}' may not be inserted inside nodes of type '#document-fragment'."
            )));
        }
        let result = dom_mut(cx.rt)?.clone_contents(range);
        let contents = dom_result(cx.rt, result)?;
        upgrade::enqueue_copies(cx.rt, contents.fragment)?;
        cloning_steps(cx.rt, &contents.scripts, &contents.controls)?;
        cx.rt.check_time()?;
        Ok(wrapper_in(cx.rt, key, contents.fragment)?)
    })
}

/// `Range.extractContents`.
pub(super) fn extract_contents(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
    in_range_document(cx, this, |cx, id, key, range| {
        let fragment = extract(cx.rt, id, range)?;
        Ok(wrapper_in(cx.rt, key, fragment)?)
    })
}

/// DOM "extract" of live range `id` (at `range`) of the current document:
/// the new fragment.
fn extract(rt: &mut Runtime, id: u32, range: LiveRange) -> b::Result<NodeId> {
    if dom(rt)?.range_contains_doctype(range) {
        return Err(tree_ops::hierarchy("The Range contains a doctype node."));
    }
    crate::html::focus::before_contents_removal(rt, range)?;
    let result = dom_mut(rt)?.extract_contents(id);
    let contents = dom_result(rt, result)?;
    for &copy in &contents.copies {
        upgrade::try_upgrade(rt, copy)?;
    }
    cloning_steps(rt, &contents.scripts, &contents.controls)?;
    rt.check_time()?;
    Ok(contents.fragment)
}

/// `Range.deleteContents`.
pub(super) fn delete_contents(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
    in_range_document(cx, this, |cx, id, _, range| delete(cx.rt, id, range))
}

/// `deleteContents` of live range `id` (at `range`) of the current document
/// (also `Selection.deleteFromDocument`).
pub(crate) fn delete(rt: &mut Runtime, id: u32, range: LiveRange) -> b::Result<()> {
    crate::html::focus::before_contents_removal(rt, range)?;
    let result = dom_mut(rt)?.delete_contents(id);
    dom_result(rt, result)?;
    Ok(rt.check_time()?)
}

/// `Range.insertNode`.
pub(super) fn insert_node(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<()> {
    in_range_document(cx, this, |cx, id, _, range| {
        insert_at(cx.rt, id, range, node).map(drop)
    })
}

/// The kind of a range's start node, for the first checks of `insertNode`.
enum StartKind {
    /// A comment or a processing instruction.
    NoChildren,
    /// A text node without a parent.
    LoneText,
    /// A text node with a parent (it is split).
    Text,
    /// An element, a document or a fragment.
    Other,
}

/// `insertNode` of `node` at the start of live range `id` (at `range`) of
/// the current document, with Chromium's checks and messages in its order.
/// Returns the node's id here.
fn insert_at(rt: &mut Runtime, id: u32, range: LiveRange, node: NodeRef) -> b::Result<NodeId> {
    let start = range.start;
    let here = node.here(rt);
    let kind = {
        let dom = dom(rt)?;
        match &dom.node(start.node).data {
            NodeData::Comment(_) | NodeData::ProcessingInstruction { .. } => StartKind::NoChildren,
            NodeData::Text(_) if dom.parent(start.node).is_none() => StartKind::LoneText,
            NodeData::Text(_) => StartKind::Text,
            _ => StartKind::Other,
        }
    };
    match kind {
        StartKind::NoChildren => return Err(tree_ops::type_error(rt, start.node, node)?),
        StartKind::LoneText => {
            return Err(tree_ops::hierarchy(
                "This operation would split a text node, but there's no parent into which to insert.",
            ));
        }
        _ if here == Some(start.node) => {
            return Err(tree_ops::hierarchy(
                "Unable to insert a node into a Range starting from the node itself.",
            ));
        }
        _ => {}
    }
    let (reference, parent) = {
        let dom = dom(rt)?;
        let reference = match kind {
            StartKind::Text => Some(start.node),
            _ => dom.children(start.node).nth(start.offset as usize),
        };
        let parent = match reference {
            Some(r) => dom.parent(r).ok_or_else(no_parent_error)?,
            None => start.node,
        };
        (reference, parent)
    };
    tree_ops::check_insertion(rt, parent, node, reference)?;
    // A node of another document comes here after its checks.
    let node = match here {
        Some(here) => here,
        None => crate::dom::adopt::bring_here(rt, node, parent)?,
    };
    if dom(rt)?.parent(node).is_some() {
        crate::html::focus::before_removal(rt, node, true)?;
    }
    let added: Vec<NodeId> = {
        let dom = dom(rt)?;
        if dom.node(node).data.is_fragment() {
            dom.children(node).collect()
        } else {
            vec![node]
        }
    };
    let result = dom_mut(rt)?.insert_node(id, node);
    let parent = dom_result(rt, result)?;
    crate::scripts::after_insertion(rt, parent, &added)?;
    Ok(node)
}

/// `Range.surroundContents`: extract, empty the new parent, insert it, append
/// the contents to it, select it (DOM's steps, each with its exceptions).
pub(super) fn surround_contents(
    cx: &mut Cx<'_>,
    this: Platform,
    new_parent: NodeRef,
) -> b::Result<()> {
    in_range_document(cx, this, |cx, id, key, range| {
        if dom(cx.rt)?.range_partly_selects_non_text(range) {
            return Err(Exception::dom(
                "InvalidStateError",
                "The Range has partially selected a non-Text node.",
            ));
        }
        let parent_dom = arena_or_error(cx.rt, new_parent.document)?;
        let data = &parent_dom.node(new_parent.node).data;
        if matches!(
            data,
            NodeData::Document
                | NodeData::Doctype { .. }
                | NodeData::DocumentFragment
                | NodeData::ShadowRoot(_)
        ) {
            return Err(Exception::dom(
                "InvalidNodeTypeError",
                format!("The node provided is of type '{}'.", node_name(data)),
            ));
        }
        let fragment = extract(cx.rt, id, range)?;
        let new_parent = new_parent.refreshed(cx.rt).unwrap_or(new_parent);
        empty(cx.rt, new_parent)?;
        let new_parent = new_parent.refreshed(cx.rt).unwrap_or(new_parent);
        let range = arena_or_error(cx.rt, key)?
            .live_range(id)
            .ok_or_else(illegal)?;
        let placed = insert_at(cx.rt, id, range, new_parent)?;
        tree_ops::pre_insert(cx, placed, fragment, None)?;
        let dom = dom(cx.rt)?;
        let parent = dom.parent(placed).ok_or_else(no_parent_error)?;
        let index = dom.index_in_parent(placed);
        let selected = LiveRange {
            start: swb_dom::Boundary::new(parent, index),
            end: swb_dom::Boundary::new(parent, index + 1),
        };
        store(cx.rt, id, key, selected)
    })
}

/// DOM "replace all" with null within `node` (of any document), if it has
/// children.
fn empty(rt: &mut Runtime, node: NodeRef) -> b::Result<()> {
    if arena_or_error(rt, node.document)?
        .node(node.node)
        .first_child()
        .is_none()
    {
        return Ok(());
    }
    let entered = state::switch_if_other(rt, node.document)?;
    let result = empty_here(rt, node.node);
    state::leave(rt, entered);
    result
}

fn empty_here(rt: &mut Runtime, node: NodeId) -> b::Result<()> {
    crate::html::focus::before_removal(rt, node, false)?;
    let result = dom_mut(rt)?.replace_all(node, None);
    dom_result(rt, result)
}

/// `Range.createContextualFragment` (HTML): the markup parsed with the
/// element at the range's start as the context (the parent element of a text
/// node or comment; `body` without one, or for the `html` element), with
/// the scripts of the fragment not "already started", so that they run when
/// the fragment is inserted into a document (measured, cases
/// `contextual-fragment` and `contextual-fragment-more`).
/// <https://html.spec.whatwg.org/multipage/dynamic-markup-insertion.html#dom-range-createcontextualfragment>
pub(super) fn create_contextual_fragment(
    cx: &mut Cx<'_>,
    this: Platform,
    markup: &str,
) -> b::Result<Gc<Object>> {
    in_range_document(cx, this, |cx, _, key, range| {
        let node = range.start.node;
        let (element, document) = {
            let dom = dom(cx.rt)?;
            let element = match &dom.node(node).data {
                NodeData::Element(_) => Some(node),
                NodeData::Text(_) | NodeData::Comment(_) => dom.parent_element(node),
                _ => None,
            }
            .filter(|&e| !dom.is_html_element(e, &local_name!("html")));
            (element, dom.node_document(node))
        };
        let context = if let Some(element) = element {
            element
        } else {
            let result = dom_mut(cx.rt)?.scratch_body(document);
            dom_result(cx.rt, result)?
        };
        let fragment = crate::dom::markup::parse_in(cx, context, markup)?;
        dom_mut(cx.rt)?.unmark_scripts_started(fragment);
        cx.rt.check_time()?;
        Ok(wrapper_in(cx.rt, key, fragment)?)
    })
}
