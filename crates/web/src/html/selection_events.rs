//! The events of the selection of text controls: `select` and
//! `selectionchange` (HTML §4.10.5.5 "Common input element APIs", and
//! the Selection API,
//! <https://w3c.github.io/selection-api/#selectionchange-event>).
//!
//! Measured in Chromium 148 (`tools/probes/web-forms-selection.json`,
//! `sel-events`, `sel-user` and the cases that follow them):
//!
//! - Both are plain `Event`s fired at the control; both bubble (measured:
//!   `select` too, although HTML says it does not), neither can be
//!   canceled.
//! - A script's change of the selection (`select()`, `setSelectionRange()`,
//!   the setters of `selectionStart`, `selectionEnd` and
//!   `selectionDirection`) queues `selectionchange` and `select` for a task
//!   after the script, in that order, if the selection (start, end or
//!   direction) changed; a call that changes nothing queues nothing. Two
//!   calls in a task queue one `selectionchange` and two `select` events.
//!   `setRangeText()` queues `selectionchange` always and `select` if the
//!   selection changed; a write of `value` queues `selectionchange` if the
//!   caret moved. A control that is not in the document gets none.
//! - A text control that gets the focus while another control (or none)
//!   owns the selection of the document gets `selectionchange` (the
//!   focus moves the selection into it); `blur()` does not give up the
//!   ownership, so a focus right after it queues nothing.
//! - The user's selection fires `select` at once, when the new selection
//!   is not empty (Shift and arrow keys, Ctrl+A, a double or triple click,
//!   a drag), and queues `selectionchange`. Typing, deleting and moving the
//!   caret queue `selectionchange` only.
//! - The events wait in one queue and one task delivers them; a change
//!   that is already waiting for the same control adds no second
//!   `selectionchange`.
//!
//! Departure: the order of the two events for `select()` on an unfocused
//! control varies in Chromium (it was `select`, `selectionchange` once
//! when another control had the focus and the selection had been set
//! before); swb always fires `selectionchange` first for a script.

use std::collections::HashSet;

use swb_dom::NodeId;
use swb_js::{Runtime, ScriptError, VmResult};

use crate::event_loop::{Task, queue_task};
use crate::events::{DispatchTarget, TrustedEvent, fire_trusted};
use crate::host::SelectionKey;
use crate::state::{dom, engine_mut, state, state_mut};

/// The most events that wait for the task: a loop of calls cannot grow
/// the queue without end.
const MAX_PENDING: usize = 4096;

/// The events that wait, and who owns the selection of the document.
#[derive(Debug, Default)]
pub(crate) struct SelectionEvents {
    /// The events in the order of queuing: the control and whether it is
    /// `select` (else `selectionchange`).
    pending: Vec<(NodeId, bool)>,
    /// The controls with a `selectionchange` in `pending`.
    changes: HashSet<NodeId>,
    /// True while the task is queued.
    queued: bool,
    /// The text control whose selection is the selection of the document.
    owner: Option<NodeId>,
}

/// Queues one event for the control.
fn push(rt: &mut Runtime, node: NodeId, select: bool) -> VmResult<()> {
    let connected = dom(rt)?.is_connected(node);
    let events = &mut state_mut(rt)?.selection_events;
    if !connected || events.pending.len() >= MAX_PENDING {
        return Ok(());
    }
    if !select && !events.changes.insert(node) {
        return Ok(());
    }
    events.pending.push((node, select));
    if !events.queued {
        events.queued = true;
        queue_task(rt, Task::SelectionEvents)?;
    }
    Ok(())
}

/// The key of the selection of the control, if it has one. With `sync`
/// the state of the control is made current first.
pub(crate) fn key(rt: &mut Runtime, node: NodeId, sync: bool) -> VmResult<Option<SelectionKey>> {
    Ok(engine_mut(rt)?.control_selection_key(node, sync))
}

/// A script changed the selection of the control (or its value):
/// `before` is the key from before the call. `select` says whether the
/// call is one that fires `select` when the selection changes; `always`
/// that `selectionchange` follows although the selection stayed
/// (`setRangeText`).
pub(crate) fn script_changed(
    rt: &mut Runtime,
    node: NodeId,
    before: Option<SelectionKey>,
    select: bool,
    always: bool,
) -> VmResult<()> {
    // A value write does not make the state current (`select` and
    // `always` are false then): without a state before, there is nothing
    // to compare.
    let sync = select || always;
    if before.is_none() && !sync {
        return Ok(());
    }
    let after = key(rt, node, sync)?;
    if after.is_none() {
        return Ok(());
    }
    let changed = before != after;
    if changed || always {
        push(rt, node, false)?;
    }
    if changed && select {
        push(rt, node, true)?;
    }
    Ok(())
}

/// The text control `node` got the focus: if the selection of the document
/// was somewhere else, it moves into the control (a `selectionchange`).
pub(crate) fn focused(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    if key(rt, node, true)?.is_none() {
        return Ok(());
    }
    let events = &mut state_mut(rt)?.selection_events;
    if events.owner == Some(node) {
        return Ok(());
    }
    events.owner = Some(node);
    push(rt, node, false)
}

/// The user's selection in the focused text control `node` changed:
/// `select` fires now if `select`; `selectionchange` follows in a task.
pub(crate) fn user_changed(rt: &mut Runtime, node: NodeId, select: bool) -> VmResult<()> {
    // The events exist for the types with the selection API only.
    if key(rt, node, false)?.is_none() {
        return Ok(());
    }
    state_mut(rt)?.selection_events.owner = Some(node);
    if select {
        let mut event = TrustedEvent::new("select");
        event.bubbles = true;
        fire_trusted(rt, DispatchTarget::Node(node), &event)?;
    }
    push(rt, node, false)
}

/// The task: fires the waiting events in order. Controls that left the
/// document since get none.
pub(crate) fn deliver(rt: &mut Runtime) -> Result<(), ScriptError> {
    let pending = {
        let events = &mut state_mut(rt)
            .map_err(crate::runtime::task_error)?
            .selection_events;
        events.queued = false;
        events.changes.clear();
        std::mem::take(&mut events.pending)
    };
    for (node, select) in pending {
        let connected = state(rt)
            .and_then(|_| dom(rt))
            .map_err(crate::runtime::task_error)?
            .is_connected(node);
        if !connected {
            continue;
        }
        let mut event = TrustedEvent::new(if select { "select" } else { "selectionchange" });
        event.bubbles = true;
        fire_trusted(rt, DispatchTarget::Node(node), &event).map_err(crate::runtime::task_error)?;
    }
    Ok(())
}
