//! Focus from scripts: `focus()`, `blur()`, `document.activeElement` and
//! `document.hasFocus()` (HTML §6.6, "Focus",
//! <https://html.spec.whatwg.org/multipage/interaction.html#focus>).
//!
//! The focused element is the engine's (the element states, shared with
//! the pointer and the keyboard); whether an element can be focused is the
//! engine's rule ([`crate::EngineHost::can_focus`]). The events follow
//! Chromium 148 (measured in `tools/probes/web-input-handlers.json`):
//!
//! - Moving the focus from `old` to `new`: the focused element is none
//!   (`activeElement` is the body); `blur` at `old` with `relatedTarget`
//!   `new`, then `focusout` (`relatedTarget` `new`); then the focused
//!   element is `new`; `focus` at `new` (`relatedTarget` `old`), then
//!   `focusin`.
//! - A listener that moves the focus itself ends the change that is in
//!   progress: after `blur`, only `focusout` (with `relatedTarget` null)
//!   follows; after `focus`, no `focusin`. A `new` that is no longer
//!   focusable after `blur` gets nothing.
//! - `blur()` of the focused element: `blur` and `focusout` with
//!   `relatedTarget` null. The focus events are trusted `FocusEvent`s that
//!   do not bubble (`blur`, `focus`) or bubble (`focusin`, `focusout`), are
//!   composed, and have the window as `view`.
//! - The window has the focus (`hasFocus()` is true for the document of
//!   the window, false for the others) and no `focus` or `blur` event
//!   goes to the window when the focused element changes.
//!
//! Departures: elements with `contenteditable` are not focusable in swb
//! (`crates/engine/src/focus.rs`); `focus()` scrolls the
//! element into view when the task ends, not at once.

use swb_dom::NodeId;
use swb_js::{Runtime, VmResult};

use super::activation::{lock_focus, unlock_focus};
use crate::bindings::{self as b, Cx};
use crate::events::{DispatchTarget, EventInterface, TrustedEvent, fire_trusted};
use crate::generated::types::FocusOptions;
use crate::state::{dom, engine, engine_mut};

/// The focused element of the window's document.
fn focused(rt: &Runtime) -> VmResult<Option<NodeId>> {
    Ok(engine(rt)?.focused_element())
}

/// A trusted focus event at `target`.
fn fire(
    rt: &mut Runtime,
    kind: &str,
    target: NodeId,
    related_target: Option<NodeId>,
    bubbles: bool,
) -> VmResult<()> {
    let mut event = TrustedEvent::new(kind);
    event.bubbles = bubbles;
    event.composed = true;
    event.interface = EventInterface::Focus { related_target };
    fire_trusted(rt, DispatchTarget::Node(target), &event)?;
    Ok(())
}

/// True if `node` is an element of the window's document that a script
/// can focus now.
fn focusable(rt: &mut Runtime, node: NodeId) -> VmResult<bool> {
    let document = dom(rt)?;
    if document.element(node).is_none() || document.node_document(node) != NodeId::DOCUMENT {
        return Ok(false);
    }
    Ok(engine_mut(rt)?.can_focus(node))
}

/// The focusing steps for `new`, or the unfocusing steps for `None`
/// (the sequence above).
fn move_focus(rt: &mut Runtime, new: Option<NodeId>, visible: bool) -> VmResult<()> {
    let old = focused(rt)?;
    if old == new {
        return Ok(());
    }
    if let Some(old) = old {
        engine_mut(rt)?.set_focus(None, false);
        fire(rt, "blur", old, new, false)?;
        if focused(rt)?.is_some() {
            // A listener moved the focus: the change ends.
            return fire(rt, "focusout", old, None, true);
        }
        fire(rt, "focusout", old, new, true)?;
        if let Some(new) = new
            && !focusable(rt, new)?
        {
            return Ok(());
        }
    }
    if let Some(new) = new {
        engine_mut(rt)?.set_focus(Some(new), visible);
        fire(rt, "focus", new, old, false)?;
        if focused(rt)? != Some(new) {
            return Ok(());
        }
        fire(rt, "focusin", new, old, true)?;
    }
    Ok(())
}

/// HTML "focus()": the focusing steps of `node`, unless it is locked for
/// focus. `prevent_scroll` false asks the page to scroll the element into
/// view when the task ends.
pub(crate) fn focus_with(
    rt: &mut Runtime,
    node: NodeId,
    visible: bool,
    prevent_scroll: bool,
) -> VmResult<()> {
    if !lock_focus(rt, node)? {
        return Ok(());
    }
    let result = (|| {
        if !focusable(rt, node)? {
            return Ok(());
        }
        move_focus(rt, Some(node), visible)?;
        if focused(rt)? == Some(node) && !prevent_scroll {
            engine_mut(rt)?.request_scroll_into_view(node);
        }
        Ok(())
    })();
    unlock_focus(rt, node)?;
    result
}

/// [`focus_with`] for the steps of other algorithms (labels, the first
/// invalid control): focus-visible, scrolling allowed.
pub(crate) fn focus_node(rt: &mut Runtime, node: NodeId, visible: bool) -> VmResult<()> {
    focus_with(rt, node, visible, false)
}

/// `focus(options)` of `HTMLElement` and `SVGElement`. A script's focus
/// matches `:focus-visible` unless `focusVisible: false`.
pub(crate) fn focus(cx: &mut Cx<'_>, node: NodeId, options: &FocusOptions) -> b::Result<()> {
    let visible = options.focus_visible.unwrap_or(true);
    Ok(focus_with(cx.rt, node, visible, options.prevent_scroll)?)
}

/// `blur()`: the unfocusing steps if `node` is the focused element.
pub(crate) fn blur(cx: &mut Cx<'_>, node: NodeId) -> b::Result<()> {
    if focused(cx.rt)? == Some(node) {
        move_focus(cx.rt, None, false)?;
    }
    Ok(())
}

/// HTML "focus fixup rule" after a change of the DOM: a focused element
/// that is no longer in the document, or is disabled, loses the focus. As
/// in Chromium, the `blur` and `focusout` events follow at once, with the
/// `relatedTarget` null.
pub(crate) fn fixup(rt: &mut Runtime) -> VmResult<()> {
    let Some(node) = focused(rt)? else {
        return Ok(());
    };
    let document = dom(rt)?;
    if document.is_connected(node) && !swb_style::is_actually_disabled(document, node) {
        return Ok(());
    }
    move_focus(rt, None, false)
}

/// Before `node` (or, without `inclusive`, the children of `node`) leave
/// the document, or move to another place (the DOM "remove" steps run
/// for a node that an insertion takes from its parent): a focused element inside loses the focus while it is
/// still in the document, so that the `blur` and `focusout` events reach
/// the listeners above it (measured in Chromium 148).
pub(crate) fn before_removal(rt: &mut Runtime, node: NodeId, inclusive: bool) -> VmResult<()> {
    let Some(focus) = focused(rt)? else {
        return Ok(());
    };
    let inside = {
        let document = dom(rt)?;
        (inclusive && focus == node) || document.ancestors(focus).any(|a| a == node)
    };
    if inside {
        move_focus(rt, None, false)?;
    }
    Ok(())
}

/// The `hidden` attribute of `element` was set: the element itself loses
/// the focus if it has it. A focused element below `element` keeps it
/// (Chromium 148, also for `hidden=until-found`; `display: none` from a
/// style does not do this until the next rendering update).
pub(crate) fn hidden_set(rt: &mut Runtime, element: NodeId) -> VmResult<()> {
    if focused(rt)? == Some(element) {
        move_focus(rt, None, false)?;
    }
    Ok(())
}

/// `document.activeElement`: the focused element of the window's
/// document, else its body (null without one). Other documents
/// have no focused element.
pub(crate) fn active_element(cx: &mut Cx<'_>, document: NodeId) -> b::Result<Option<NodeId>> {
    if document == NodeId::DOCUMENT
        && let Some(node) = focused(cx.rt)?
        && dom(cx.rt)?.is_connected(node)
    {
        return Ok(Some(node));
    }
    let dom = dom(cx.rt)?;
    let root = dom
        .element_children(document)
        .find(|&e| dom.is_html_element(e, &swb_dom::local_name!("html")));
    let body = root.and_then(|html| {
        dom.element_children(html).find(|&c| {
            dom.is_html_element(c, &swb_dom::local_name!("body"))
                || dom.is_html_element(c, &swb_dom::local_name!("frameset"))
        })
    });
    Ok(body)
}

/// `document.hasFocus()`: true for the document of the window.
pub(crate) fn has_focus(document: NodeId) -> bool {
    document == NodeId::DOCUMENT
}
