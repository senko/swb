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
//!   goes to the window when the focused element changes. The engine tells
//!   when the user's Tab leaves the page after its last element and when
//!   the next Tab enters it again; `blur` and `focus` then go to the window
//!   ([`window_focus`]).
//! - A text control with an edit of the user that was not committed fires
//!   `change` before `blur` when it loses the focus (measured in
//!   `tools/probes/web-input-real.json`).
//!
//! `:focus-visible` (HTML "focus-visible" heuristics, Chromium 148 measured
//! in `tools/probes/web-small-gaps.json`, cases `fv-*`): the engine keeps a
//! modality flag. A key sets it ("keyboard"); a mouse press (or a label's
//! activation) that moves the focus to an element clears it ("mouse"); other
//! input (hover, wheel, a press on nothing focusable, a press on the element
//! that has the focus, a press whose `mousedown` was canceled) leaves it. A
//! script's `focus()` matches `:focus-visible` if the flag is set, which it
//! is before any input; `focus({focusVisible})` overrides. A text field
//! matches whenever it has the focus (`swb_style`).
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
pub(crate) fn focused(rt: &Runtime) -> VmResult<Option<NodeId>> {
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
    move_focus_to(rt, new, visible, true)
}

/// [`move_focus`]; `recheck`: after `focusout`, `new` must still be
/// focusable (false for a dialog that takes the focus itself).
fn move_focus_to(
    rt: &mut Runtime,
    new: Option<NodeId>,
    visible: bool,
    recheck: bool,
) -> VmResult<()> {
    let old = focused(rt)?;
    if old == new {
        return Ok(());
    }
    if let Some(old) = old {
        commit_change(rt, old)?;
        // A listener of `change` can move the focus; the change ends then.
        if focused(rt)? != Some(old) {
            return Ok(());
        }
        engine_mut(rt)?.set_focus(None, false);
        fire(rt, "blur", old, new, false)?;
        if focused(rt)?.is_some() {
            // A listener moved the focus: the change ends.
            return fire(rt, "focusout", old, None, true);
        }
        fire(rt, "focusout", old, new, true)?;
        if let Some(new) = new
            && recheck
            && !focusable(rt, new)?
        {
            return Ok(());
        }
    }
    if let Some(new) = new {
        engine_mut(rt)?.set_focus(Some(new), visible);
        super::selection_events::focused(rt, new)?;
        fire(rt, "focus", new, old, false)?;
        if focused(rt)? != Some(new) {
            return Ok(());
        }
        fire(rt, "focusin", new, old, true)?;
    }
    Ok(())
}

/// Fires `change` (bubbles) at the control `node` if the user edited its
/// value since the last `change` or since it got the focus, and the value
/// differs from the one then. The engine keeps the state.
pub(crate) fn commit_change(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    if !engine_mut(rt)?.take_pending_change(node) {
        return Ok(());
    }
    let mut event = TrustedEvent::new("change");
    event.bubbles = true;
    fire_trusted(rt, DispatchTarget::Node(node), &event)?;
    Ok(())
}

/// The user's focus change: `new` gets the focus (`None` removes it) with
/// the events of the focusing steps. The page scrolls the element itself.
pub(crate) fn user_focus(rt: &mut Runtime, new: Option<NodeId>, visible: bool) -> VmResult<()> {
    match new {
        Some(node) => focus_with(rt, node, visible, true),
        None => move_focus(rt, None, false),
    }
}

/// `blur` or `focus` at the window (a `FocusEvent` without a related
/// target, composed, not bubbling): the user's Tab left the page or
/// entered it again.
pub(crate) fn window_focus(rt: &mut Runtime, gained: bool) -> VmResult<()> {
    let mut event = TrustedEvent::new(if gained { "focus" } else { "blur" });
    event.composed = true;
    event.interface = EventInterface::Focus {
        related_target: None,
    };
    fire_trusted(rt, DispatchTarget::Window, &event)?;
    Ok(())
}

/// The page's focus leaves the current document for another document of the
/// runtime: its focused element and then its window get `blur`.
pub(crate) fn focus_leaves_document(rt: &mut Runtime) -> VmResult<()> {
    move_focus(rt, None, false)?;
    window_focus(rt, false)
}

/// `window.focus()` (HTML "run the focusing steps" for the window's document):
/// the page's focus comes to this window; the focused element of its document
/// stays as it is.
pub(crate) fn focus_window(rt: &mut Runtime) -> VmResult<()> {
    crate::frames::focus_enters_current(rt)
}

/// HTML "focus()": the focusing steps of `node`, unless it is locked for
/// focus. With `prevent_scroll` false, an element that gets the focus is
/// scrolled into view at once (a layout flush); one that has it already
/// is not (the focusing steps return early; Chromium 148 does not scroll).
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
        // HTML "focusing steps": a host that delegates focus gives the
        // focus to its focus delegate, also if it has a `tabindex`; one
        // without a delegate takes none (measured, case `focus-shadow`). If
        // the focused element is a shadow-including descendant of the host,
        // the focus stays there, with no events (HTML "focusing steps" and
        // "focus delegate"; measured, case `focus-inside-delegating`).
        let node = if dom(rt)?.delegates_focus(node) {
            if let Some(current) = focused(rt)?
                && dom(rt)?
                    .shadow_including_ancestors(current)
                    .any(|a| a == node)
            {
                return Ok(());
            }
            match engine_mut(rt)?.focus_delegate(node) {
                Some(delegate) => delegate,
                None => return Ok(()),
            }
        } else {
            node
        };
        if !focusable(rt, node)? {
            return Ok(());
        }
        if crate::frames::focus_enters_frame_of(rt, node)? {
            // The window of the frame has the focus. The element matches `:focus`
            // and gets its events (measured), unless it has the focus already.
            if focused(rt)? == Some(node) {
                return Ok(());
            }
            engine_mut(rt)?.set_focus(Some(node), visible);
            fire(rt, "focus", node, None, false)?;
            return fire(rt, "focusin", node, None, true);
        }
        crate::frames::focus_enters_current(rt)?;
        let before = focused(rt)?;
        move_focus(rt, Some(node), visible)?;
        if before != Some(node) && focused(rt)? == Some(node) && !prevent_scroll {
            engine_mut(rt)?.request_scroll_into_view(node);
            rt.check_time()?;
        }
        Ok(())
    })();
    unlock_focus(rt, node)?;
    result
}

/// The focusing steps that the popover algorithms run (the popover focusing
/// steps, the restoration of the focus; ADR 0037 section 5): `node` gets the
/// focus with its events if it is a focusable area that is rendered, as for
/// `focus()` with the engine's modality; the viewport does not scroll.
/// Returns whether `node` could take the focus.
pub(crate) fn focus_popover_target(rt: &mut Runtime, node: NodeId) -> VmResult<bool> {
    if !focusable(rt, node)? {
        return Ok(false);
    }
    let visible = engine(rt)?.keyboard_modality();
    focus_with(rt, node, visible, true)?;
    Ok(true)
}

/// The last step of the dialog focusing steps (ADR 0037 section 5): the
/// dialog itself gets the focus with its events, although it is no focusable
/// area (measured, `dialog-focus` #3); the viewport does not scroll. Returns
/// whether it has the focus afterwards.
pub(crate) fn focus_dialog(rt: &mut Runtime, dialog: NodeId) -> VmResult<bool> {
    let document = dom(rt)?;
    if !document.is_connected(dialog) || document.node_document(dialog) != NodeId::DOCUMENT {
        return Ok(false);
    }
    if !lock_focus(rt, dialog)? {
        return Ok(false);
    }
    let result = (|| {
        crate::frames::focus_enters_current(rt)?;
        if !engine_mut(rt)?.is_rendered_for_focus(dialog) {
            // Chromium (measured, `r-hidden-focus`): a dialog that is not
            // rendered cannot take the focus, and the element that had it
            // loses it (`blur` and `focusout` with no related target).
            move_focus(rt, None, false)?;
            return Ok(false);
        }
        let visible = engine(rt)?.keyboard_modality();
        move_focus_to(rt, Some(dialog), visible, false)?;
        Ok(focused(rt)? == Some(dialog))
    })();
    unlock_focus(rt, dialog)?;
    result
}

/// [`focus_with`] for the steps of other algorithms (labels, the first
/// invalid control): focus-visible, scrolling allowed.
pub(crate) fn focus_node(rt: &mut Runtime, node: NodeId, visible: bool) -> VmResult<()> {
    focus_with(rt, node, visible, false)
}

/// `focus(options)` of `HTMLElement` and `SVGElement`. The `focusVisible`
/// option decides whether the element matches `:focus-visible`; without it
/// the element matches if the last input was the keyboard or there was
/// none (the engine's modality), as in Chromium: after a click, a script's
/// `focus()` of another element does not match (text fields match in any
/// case, in the style system).
pub(crate) fn focus(cx: &mut Cx<'_>, node: NodeId, options: &FocusOptions) -> b::Result<()> {
    let visible = match options.focus_visible {
        Some(visible) => visible,
        None => engine(cx.rt)?.keyboard_modality(),
    };
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
    crate::frames::before_removal(rt, node, inclusive)?;
    let Some(focus) = focused(rt)? else {
        return Ok(());
    };
    let inside = {
        let document = dom(rt)?;
        (inclusive && focus == node)
            || document
                .shadow_including_ancestors(focus)
                .any(|a| a == node)
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
/// document (the host of a shadow tree that holds it), else its body (null
/// without one). Other documents have no focused element.
pub(crate) fn active_element(cx: &mut Cx<'_>, document: NodeId) -> b::Result<Option<NodeId>> {
    if document == NodeId::DOCUMENT
        && let Some(node) = focused(cx.rt)?.or(engine(cx.rt)?.frame_focus_element())
        && dom(cx.rt)?.is_connected(node)
    {
        // `DocumentOrShadowRoot.activeElement`: retargeted against the
        // document (ADR 0036 section 8).
        return Ok(Some(dom(cx.rt)?.retarget(node, document)));
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

/// `document.hasFocus()`: true for the document of the window while the
/// window has the focus (the engine tells: Tab left the page, or the
/// focused frame was removed).
pub(crate) fn has_focus(cx: &mut Cx<'_>, document: NodeId) -> b::Result<bool> {
    if document != NodeId::DOCUMENT {
        return Ok(false);
    }
    if crate::state::state(cx.rt)?.doc.frame != 0 {
        return Ok(crate::frames::holds_focus(cx.rt)?);
    }
    Ok(engine(cx.rt)?.window_has_focus())
}
