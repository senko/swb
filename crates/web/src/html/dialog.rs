//! `HTMLDialogElement` for scripts (HTML §4.11.4,
//! <https://html.spec.whatwg.org/multipage/interactive-elements.html#the-dialog-element>;
//! ADR 0037 sections 6, 9 and 10): `show()`, `showModal()`, `close()`,
//! `requestClose()`, `returnValue`, `closedBy`, the dialog `toggle` and
//! `close` tasks, and the form method `dialog`.
//!
//! The algorithms are `swb_dom`'s (`top_layer/dialog.rs`); the popover
//! module's [`WebHost`] is their host: it fires `beforetoggle` and `cancel`,
//! sets the `open` attribute as a script would, and queues the tasks here.

use swb_dom::{ClosedBy, NodeId, QualName, ns};
use swb_js::{Runtime, VmResult};

use super::popover::WebHost;
use crate::bindings::{self as b, Cx};
use crate::event_loop::Task;
use crate::events::{DispatchTarget, EventInterface, TrustedEvent, fire_trusted};
use crate::generated as g;
use crate::state::{WebState, dom, dom_mut, state_mut};

impl g::html_dialog_element::HTMLDialogElementImpl for WebState {
    fn return_value(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(dom(cx.rt)?.dialog_return_value(this).to_owned())
    }

    fn set_return_value(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        dom_mut(cx.rt)?.set_dialog_return_value(this, &value);
        Ok(())
    }

    /// The computed closed-by state (HTML: the getter returns it, so a
    /// missing or invalid attribute gives `closerequest` for a modal dialog
    /// and `none` otherwise; measured, `popover-reflection` #5).
    fn closed_by(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(ClosedBy::of(dom(cx.rt)?, this).as_str().to_owned())
    }

    fn set_closed_by(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        super::set_attribute(cx, this, "closedby", &value)
    }

    fn show(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        let mut host = WebHost::new(cx.rt);
        let result = swb_dom::show_dialog(&mut host, this);
        host.finish(result)
    }

    fn show_modal(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        let mut host = WebHost::new(cx.rt);
        let result = swb_dom::show_modal(&mut host, this, None);
        host.finish(result)
    }

    fn close(cx: &mut Cx<'_>, this: NodeId, return_value: Option<String>) -> b::Result<()> {
        let mut host = WebHost::new(cx.rt);
        let result = swb_dom::close_dialog(&mut host, this, return_value);
        host.finish(result)
    }

    fn request_close(cx: &mut Cx<'_>, this: NodeId, return_value: Option<String>) -> b::Result<()> {
        let mut host = WebHost::new(cx.rt);
        let result = swb_dom::request_close_dialog(&mut host, this, return_value);
        host.finish(result)
    }
}

/// Sets or removes the `open` attribute of `dialog` for the dialog
/// algorithms, as a script's change (mutation records, custom element
/// reactions, `Attr` objects).
pub(crate) fn set_open(rt: &mut Runtime, dialog: NodeId, open: bool) -> VmResult<()> {
    let name = QualName::new(None, ns!(), "open".into());
    if open {
        match crate::dom::set_attribute(rt, dialog, &name, "") {
            Err(b::Exception::Js(error)) => Err(error),
            // An attribute without a namespace on an element: no DOM error.
            _ => Ok(()),
        }
    } else {
        crate::dom::remove_attribute(rt, dialog, &name).map(|_| ())
    }
}

/// Queues the dialog `toggle` task of `dialog` at the end and removes the
/// one that waits (HTML "queue a dialog toggle event task").
pub(crate) fn queue_toggle_task(rt: &mut Runtime, dialog: NodeId, serial: u64) -> VmResult<()> {
    let now = crate::event_loop::page_now(rt);
    let state = state_mut(rt)?;
    if let Some(old) = state.doc.dialog_toggle_tasks.remove(&dialog) {
        state.event_loop.tasks.remove(old);
    }
    let key = state.event_loop.tasks.push(
        now,
        Task::DialogToggle {
            node: dialog,
            serial,
        },
    );
    state.doc.dialog_toggle_tasks.insert(dialog, key);
    Ok(())
}

/// Queues the task that fires `close` at `dialog`.
pub(crate) fn queue_close_task(rt: &mut Runtime, dialog: NodeId) -> VmResult<()> {
    let now = crate::event_loop::page_now(rt);
    state_mut(rt)?
        .event_loop
        .tasks
        .push(now, Task::DialogClose(dialog));
    Ok(())
}

/// The dialog `toggle` task of `dialog`: fires `toggle` with the coalesced
/// states, unless a later change replaced the task.
pub(crate) fn toggle_task(rt: &mut Runtime, dialog: NodeId, serial: u64) -> VmResult<()> {
    state_mut(rt)?.doc.dialog_toggle_tasks.remove(&dialog);
    let Some(toggle) = dom_mut(rt)?.take_dialog_toggle(dialog, serial) else {
        return Ok(());
    };
    // A node that went to another arena is an empty comment here.
    if dom(rt)?.element(dialog).is_none() {
        return Ok(());
    }
    let mut event = TrustedEvent::new("toggle");
    event.interface = EventInterface::Toggle {
        old: toggle.old,
        new: toggle.new,
        source: toggle
            .source
            .filter(|&s| dom(rt).is_ok_and(|d| d.element(s).is_some())),
        foreign: None,
    };
    fire_trusted(rt, DispatchTarget::Node(dialog), &event).map(|_| ())
}

/// The `close` task of `dialog`: an `Event` that does not bubble and cannot
/// be canceled.
pub(crate) fn close_task(rt: &mut Runtime, dialog: NodeId) -> VmResult<()> {
    if dom(rt)?.element(dialog).is_none() {
        return Ok(());
    }
    fire_trusted(
        rt,
        DispatchTarget::Node(dialog),
        &TrustedEvent::new("close"),
    )
    .map(|_| ())
}

/// The form method `dialog` (HTML "form submission algorithm"): if the
/// method of `form` with `submitter` is `dialog`, the form's nearest
/// ancestor dialog closes with the submitter's value (or the coordinate of
/// an image button) and the submission ends. Returns false if the method is
/// not `dialog`.
pub(crate) fn submit_to_dialog(
    rt: &mut Runtime,
    form: NodeId,
    submitter: Option<NodeId>,
    coordinate: Option<(i32, i32)>,
) -> VmResult<bool> {
    let Some(target) = swb_dom::dialog_form_target(dom(rt)?, form, submitter, coordinate) else {
        return Ok(false);
    };
    if let Some((dialog, result)) = target {
        let mut host = WebHost::new(rt);
        let outcome = swb_dom::close_dialog(&mut host, dialog, result);
        if let Err(b::Exception::Js(error)) = host.finish(outcome) {
            return Err(error);
        }
    }
    Ok(true)
}
