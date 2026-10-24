//! Markup and `insertAdjacent*` (DOM Parsing and Serialization,
//! <https://w3c.github.io/DOM-Parsing/>, and DOM §4.9): `innerHTML`,
//! `outerHTML`, `insertAdjacentHTML`, `insertAdjacentElement` and
//! `insertAdjacentText`.
//!
//! The getters serialize with [`swb_dom::outer_html`] and
//! [`swb_dom::inner_html`] (the HTML fragment serialization algorithm,
//! <https://html.spec.whatwg.org/multipage/parsing.html#serialising-html-fragments>).
//! The setters parse with the fragment parsing algorithm
//! ([`swb_dom::parse_fragment`], iterative like the serializer) into a
//! fragment and insert it with the ordinary insertion steps, so the scripts
//! of the markup are "already started" and do not run, and the engine finds
//! the style sheets and images of the new nodes in the journal. The
//! exceptions and their messages are Chromium's (measured in
//! `tools/probes/web-dom-query.json`).

use swb_dom::{NodeData, NodeId, local_name};

use super::{dom_result, node_name, tree_ops};
use crate::bindings::{self as b, Cx, Exception};
use crate::state::{dom, dom_mut};

/// A position of `insertAdjacent*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    BeforeBegin,
    AfterBegin,
    BeforeEnd,
    AfterEnd,
}

/// The position named by `text` (ASCII case-insensitive), or Chromium's
/// `SyntaxError`.
fn position(text: &str) -> Result<Position, Exception> {
    Ok(match text.to_ascii_lowercase().as_str() {
        "beforebegin" => Position::BeforeBegin,
        "afterbegin" => Position::AfterBegin,
        "beforeend" => Position::BeforeEnd,
        "afterend" => Position::AfterEnd,
        _ => {
            return Err(Exception::dom(
                "SyntaxError",
                format!(
                    "The value provided ('{text}') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'."
                ),
            ));
        }
    })
}

/// A `NoModificationAllowedError`.
fn no_modification(message: String) -> Exception {
    Exception::dom("NoModificationAllowedError", message)
}

/// `Element.innerHTML` (getter): the serialized children (of the template
/// contents for a `template`).
pub(super) fn inner_html(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(swb_dom::inner_html(dom(cx.rt)?, this))
}

/// `Element.outerHTML` (getter).
pub(super) fn outer_html(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(swb_dom::outer_html(dom(cx.rt)?, this))
}

/// The fragment that the markup makes in the context of the element
/// `context`.
fn parse_in(cx: &mut Cx<'_>, context: NodeId, markup: &str) -> b::Result<NodeId> {
    let result = swb_dom::parse_fragment(dom_mut(cx.rt)?, context, markup);
    dom_result(cx.rt, result)
}

/// `Element.innerHTML` (setter): "replace all" with the parsed fragment,
/// within the template contents for a `template`.
/// <https://w3c.github.io/DOM-Parsing/#dom-innerhtml-innerhtml>
pub(super) fn set_inner_html(cx: &mut Cx<'_>, this: NodeId, markup: &str) -> b::Result<()> {
    let fragment = parse_in(cx, this, markup)?;
    let target = dom(cx.rt)?
        .element(this)
        .and_then(|e| e.template_contents)
        .unwrap_or(this);
    let result = tree_ops::replace_all(cx, target, fragment);
    recycle(cx, fragment)?;
    result
}

/// Gives the parsed fragment back to the DOM for the next parse (no
/// script can reach it).
fn recycle(cx: &mut Cx<'_>, fragment: NodeId) -> b::Result<()> {
    dom_mut(cx.rt)?.recycle_fragment(fragment);
    Ok(())
}

/// `Element.outerHTML` (setter): replaces the element with the parsed
/// fragment, parsed in the context of the parent.
/// <https://w3c.github.io/DOM-Parsing/#dom-element-outerhtml>
pub(super) fn set_outer_html(cx: &mut Cx<'_>, this: NodeId, markup: &str) -> b::Result<()> {
    let dom_ref = dom(cx.rt)?;
    let Some(parent) = dom_ref.parent(this) else {
        return Err(no_modification(
            "This element has no parent node.".to_owned(),
        ));
    };
    if !dom_ref.node(parent).is_element() {
        return Err(no_modification(format!(
            "This element's parent is of type '{}', which is not an element node.",
            node_name(&dom_ref.node(parent).data)
        )));
    }
    let fragment = parse_in(cx, parent, markup)?;
    let dom_ref = dom(cx.rt)?;
    let (before, after) = (
        dom_ref.node(this).prev_sibling(),
        dom_ref.node(this).next_sibling(),
    );
    let added: Vec<NodeId> = dom_ref.children(fragment).collect();
    let result = tree_ops::replace(cx, parent, fragment, this);
    recycle(cx, fragment)?;
    result?;
    merge_around(cx, before, after, &added)
}

/// True if `node` is a text node.
fn is_text(cx: &Cx<'_>, node: NodeId) -> b::Result<bool> {
    Ok(matches!(dom(cx.rt)?.node(node).data, NodeData::Text(_)))
}

/// Appends the text node `absorbed` to the text node `keep` and removes it.
fn merge_text(cx: &mut Cx<'_>, keep: NodeId, absorbed: NodeId) -> b::Result<()> {
    let data = dom(cx.rt)?
        .char_data(absorbed)
        .unwrap_or_default()
        .to_owned();
    let result = dom_mut(cx.rt)?.append_data(keep, &data);
    dom_result(cx.rt, result)?;
    dom_mut(cx.rt)?.remove(absorbed);
    Ok(crate::scripts::data_changed(cx.rt, keep)?)
}

/// After `outerHTML = ...` replaced a node that had the siblings `before`
/// and `after` with the nodes `added`: Chromium joins a text node at the
/// end of the new nodes (or `before`, if there are none) with the text
/// node `after`, then the text node `before` with the first new node
/// (measured: `a<span></span>b` with `outerHTML = 'X'` has one text node
/// `aXb`, the node `a`; the other `insertAdjacent*` and `innerHTML` leave
/// adjacent text nodes alone).
fn merge_around(
    cx: &mut Cx<'_>,
    before: Option<NodeId>,
    after: Option<NodeId>,
    added: &[NodeId],
) -> b::Result<()> {
    if let Some(after) = after
        && is_text(cx, after)?
        && let Some(last) = added.last().copied().or(before)
        && is_text(cx, last)?
    {
        merge_text(cx, last, after)?;
    }
    if let Some(before) = before
        && is_text(cx, before)?
        && let Some(first) = added.first().copied()
        && is_text(cx, first)?
    {
        merge_text(cx, before, first)?;
    }
    Ok(())
}

/// The place for a node at `position` relative to `this`: the parent and
/// the child to insert before; `None` if the position needs a parent and
/// `this` has none.
fn place(
    cx: &Cx<'_>,
    this: NodeId,
    position: Position,
) -> b::Result<Option<(NodeId, Option<NodeId>)>> {
    let dom = dom(cx.rt)?;
    Ok(match position {
        Position::BeforeBegin => dom.parent(this).map(|p| (p, Some(this))),
        Position::AfterEnd => dom.parent(this).map(|p| (p, dom.node(this).next_sibling())),
        Position::AfterBegin => Some((this, dom.node(this).first_child())),
        Position::BeforeEnd => Some((this, None)),
    })
}

/// `Element.insertAdjacentHTML`.
/// <https://w3c.github.io/DOM-Parsing/#dom-element-insertadjacenthtml>
pub(super) fn insert_adjacent_html(
    cx: &mut Cx<'_>,
    this: NodeId,
    position_text: &str,
    markup: &str,
) -> b::Result<()> {
    let position = position(position_text)?;
    let Some((parent, before)) = place(cx, this, position)? else {
        return Err(no_modification("The element has no parent.".to_owned()));
    };
    // The parent of `beforeBegin` and `afterEnd` must be an element, and a
    // document or `html` element context parses as `body`.
    let dom_ref = dom(cx.rt)?;
    let context = match position {
        Position::BeforeBegin | Position::AfterEnd => {
            if matches!(dom_ref.node(parent).data, NodeData::Document) {
                return Err(no_modification("The element has no parent.".to_owned()));
            }
            parent
        }
        Position::AfterBegin | Position::BeforeEnd => this,
    };
    let is_html_root = dom_ref.is_html_element(context, &local_name!("html"));
    let context = if !dom_ref.node(context).is_element() || is_html_root {
        let document = dom_ref.node_document(this);
        let result = dom_mut(cx.rt)?.scratch_body(document);
        dom_result(cx.rt, result)?
    } else {
        context
    };
    let fragment = parse_in(cx, context, markup)?;
    let result = tree_ops::pre_insert(cx, parent, fragment, before);
    recycle(cx, fragment)?;
    result.map(drop)
}

/// `Element.insertAdjacentElement`: the element, or `None` if the position
/// needs a parent and there is none.
/// <https://dom.spec.whatwg.org/#insert-adjacent>
pub(super) fn insert_adjacent_element(
    cx: &mut Cx<'_>,
    this: NodeId,
    position_text: &str,
    element: NodeId,
) -> b::Result<Option<NodeId>> {
    let position = position(position_text)?;
    let Some((parent, before)) = place(cx, this, position)? else {
        return Ok(None);
    };
    tree_ops::pre_insert(cx, parent, element, before)?;
    Ok(Some(element))
}

/// `Element.insertAdjacentText`.
pub(super) fn insert_adjacent_text(
    cx: &mut Cx<'_>,
    this: NodeId,
    position_text: &str,
    data: String,
) -> b::Result<()> {
    let position = position(position_text)?;
    let Some((parent, before)) = place(cx, this, position)? else {
        return Ok(());
    };
    let document = dom(cx.rt)?.node_document(this);
    let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::Text(data));
    let text = dom_result(cx.rt, result)?;
    tree_ops::pre_insert(cx, parent, text, before)
}
