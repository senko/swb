//! Light dismiss (HTML "light dismiss open popovers",
//! <https://html.spec.whatwg.org/multipage/popover.html#light-dismiss-open-popovers>;
//! ADR 0037 section 4): a press and a release on the same "topmost clicked
//! popover" (or outside every popover) close the auto and hint popovers
//! above it. "Light dismiss open dialogs" (sub-feature 6d) runs at the same
//! hooks: a click outside the topmost open dialog with `closedby=any` (on
//! its backdrop, for a modal one) requests its close.
//!
//! The engine calls [`light_dismiss_down`] when a pointer button goes down
//! (before the listeners of `pointerdown` run, with or without scripts)
//! and [`light_dismiss_up`] when it goes up (before the listeners of
//! `pointerup`). Every button counts. A listener that cancels the press
//! changes nothing (measured, `light-dismiss-prevented-pointerdown`). The
//! callers pass the target of the event, the element under the pointer.
//!
//! The walk to the "nearest inclusive target popover" needs the popover
//! that an invoker names; the caller knows how (the script layer's
//! `popoverTargetElement` table, or the id), so it passes the lookup.

use html5ever::local_name;

use super::watchers::request_close;
use super::{
    ClosedBy, Document, NodeId, PopoverError, PopoverState, TopLayerHost, WatcherKind,
    hide_all_popovers_until,
};

impl Document {
    /// True if an auto or hint popover shows: a press and a release may
    /// light dismiss it. The engine asks before it walks the path of a
    /// click.
    pub fn light_dismiss_active(&self) -> bool {
        self.top_layer
            .as_deref()
            .is_some_and(|s| !s.auto.is_empty() || !s.hint.is_empty())
    }
}

/// HTML "nearest inclusive open popover": the first showing auto or hint
/// popover among `node` and its flat tree ancestors (a manual popover does
/// not count; measured, `light-dismiss-manual`).
fn nearest_open_popover(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(n) = current {
        if doc.is_popover_showing(n) && is_light_dismissed(doc, n) {
            return Some(n);
        }
        current = doc.flat_parent(n);
    }
    None
}

/// True if `popover` is an auto or hint popover (not manual).
fn is_light_dismissed(doc: &Document, popover: NodeId) -> bool {
    matches!(
        doc.popover_state(popover),
        Some(PopoverState::Auto | PopoverState::Hint)
    )
}

/// HTML "nearest inclusive target popover": the target of the first invoker
/// among `node` and its flat tree ancestors whose target is a showing auto
/// or hint popover. `target_of` gives the popover that an element invokes.
/// The action of the invoker does not matter (measured,
/// `light-dismiss-invoker-show` and `-hide`).
fn nearest_target_popover(
    doc: &Document,
    node: NodeId,
    target_of: &mut impl FnMut(NodeId) -> Option<NodeId>,
) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(n) = current {
        if let Some(popover) = target_of(n)
            && doc.is_popover_showing(popover)
            && is_light_dismissed(doc, popover)
        {
            return Some(popover);
        }
        current = doc.flat_parent(n);
    }
    None
}

/// HTML "topmost clicked popover": the higher in the showing popover lists
/// of the nearest inclusive open popover and the nearest inclusive target
/// popover of `node`.
pub fn topmost_clicked_popover(
    doc: &Document,
    node: NodeId,
    mut target_of: impl FnMut(NodeId) -> Option<NodeId>,
) -> Option<NodeId> {
    let clicked = nearest_open_popover(doc, node);
    let target = nearest_target_popover(doc, node, &mut target_of);
    match (clicked, target) {
        (Some(c), Some(t)) => {
            let state = doc.top_layer()?;
            Some(if state.position(t) > state.position(c) {
                t
            } else {
                c
            })
        }
        (one, other) => one.or(other),
    }
}

/// The press: stores the topmost clicked popover of `node` as the "popover
/// pointerdown target" if a popover shows, else clears it. One test when
/// nothing shows.
pub fn light_dismiss_down(
    doc: &mut Document,
    node: NodeId,
    target_of: impl FnMut(NodeId) -> Option<NodeId>,
) {
    let showing = doc
        .top_layer()
        .is_some_and(|s| !s.auto.is_empty() || !s.hint.is_empty());
    if !showing {
        // A popover that this press opens (a `pointerdown` listener) is not
        // the target: the release closes it (measured,
        // `light-dismiss-open-on-down`).
        doc.set_popover_pointerdown_target(None);
        return;
    }
    let target = topmost_clicked_popover(doc, node, target_of);
    doc.set_popover_pointerdown_target(target);
}

/// The release: if the topmost clicked popover of `node` is the one that
/// the press stored (both none: outside every popover), hides the popovers
/// above it with events and without focus changes. Clears the stored target.
///
/// # Errors
///
/// A stop of the host.
pub fn light_dismiss_up<H: TopLayerHost + ?Sized>(
    host: &mut H,
    node: NodeId,
    target_of: impl FnMut(NodeId) -> Option<NodeId>,
) -> Result<(), PopoverError> {
    let doc = host.doc();
    let down = doc.popover_pointerdown_target();
    doc.set_popover_pointerdown_target(None);
    let showing = doc
        .top_layer()
        .is_some_and(|s| !s.auto.is_empty() || !s.hint.is_empty());
    if !showing {
        return Ok(());
    }
    let up = topmost_clicked_popover(doc, node, target_of);
    if up != down {
        return Ok(());
    }
    hide_all_popovers_until(host, up, true)
}

// ----- Dialogs -----

/// HTML "nearest clicked dialog": `None` for a press on an open modal
/// dialog outside its border box (on its backdrop: `outside_modal`, which the
/// caller computes from the box), else the first open dialog among `node`
/// and its flat tree ancestors.
/// <https://html.spec.whatwg.org/multipage/interactive-elements.html#nearest-clicked-dialog>
fn nearest_clicked_dialog(doc: &Document, node: NodeId, outside_modal: bool) -> Option<NodeId> {
    let is_open_dialog = |n: NodeId| {
        doc.element(n)
            .is_some_and(|e| e.is_html_named(&local_name!("dialog")) && e.has_attr("open"))
    };
    if outside_modal
        && is_open_dialog(node)
        && doc.element(node).is_some_and(crate::ElementData::is_modal)
    {
        return None;
    }
    let mut current = Some(node);
    while let Some(n) = current {
        if is_open_dialog(n) {
            return Some(n);
        }
        current = doc.flat_parent(n);
    }
    None
}

/// HTML "light dismiss open dialogs" at `pointerdown`: notes the nearest
/// clicked dialog of the press on `node` as the "dialog pointerdown target".
/// Nothing happens while the open dialogs list is empty.
/// <https://html.spec.whatwg.org/multipage/interactive-elements.html#light-dismiss-open-dialogs>
pub fn light_dismiss_dialogs_down(doc: &mut Document, node: NodeId, outside_modal: bool) {
    if doc.open_dialogs().is_empty() {
        return;
    }
    let target = nearest_clicked_dialog(doc, node, outside_modal);
    if let Some(state) = doc.top_layer.as_deref_mut() {
        state.dialog_pointerdown_target = target;
    }
}

/// HTML "light dismiss open dialogs" at `pointerup`: if the press and the
/// release have the same nearest clicked dialog and it is not the topmost
/// open dialog, a topmost dialog with `closedby=any` gets a close request
/// (`cancel` can prevent it, measured: `inert-closedby-any`).
///
/// # Errors
///
/// A stop of the host.
pub fn light_dismiss_dialogs_up<H: TopLayerHost + ?Sized>(
    host: &mut H,
    node: NodeId,
    outside_modal: bool,
) -> Result<(), PopoverError> {
    let doc = host.doc();
    let Some(&topmost) = doc.open_dialogs().last() else {
        return Ok(());
    };
    let clicked = nearest_clicked_dialog(doc, node, outside_modal);
    let down = doc
        .top_layer
        .as_deref_mut()
        .and_then(|s| s.dialog_pointerdown_target.take());
    if clicked != down || clicked == Some(topmost) || ClosedBy::of(doc, topmost) != ClosedBy::Any {
        return Ok(());
    }
    request_close(host, topmost, WatcherKind::Dialog, false).map(|_| ())
}
