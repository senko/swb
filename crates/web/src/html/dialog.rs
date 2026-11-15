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

use super::popover::{ToggleKind, WebHost, queue_toggle_task};
use crate::bindings::{self as b, Cx};
use crate::event_loop::Task;
use crate::events::{DispatchTarget, TrustedEvent, fire_trusted};
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

/// The dialog steps of [`WebHost`] (its `TopLayerHost` methods call them).
impl WebHost<'_> {
    pub(super) fn focus_dialog_steps(&mut self, dialog: NodeId) -> bool {
        if self.error.is_some() {
            return false;
        }
        match super::focus::focus_dialog(self.rt, dialog) {
            Ok(focused) => focused,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    pub(super) fn set_open_attribute(&mut self, dialog: NodeId, open: bool) -> bool {
        if self.error.is_some() {
            return false;
        }
        match set_open(self.rt, dialog, open) {
            Ok(()) => true,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    pub(super) fn fire_cancel_event(&mut self, dialog: NodeId, cancelable: bool) -> Option<bool> {
        if self.error.is_some() {
            return None;
        }
        let mut event = TrustedEvent::new("cancel");
        event.cancelable = cancelable;
        match fire_trusted(self.rt, DispatchTarget::Node(dialog), &event) {
            Ok(proceed) => Some(proceed),
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    pub(super) fn queue_dialog_toggle(&mut self, dialog: NodeId, serial: u64) {
        if let Err(error) = queue_toggle_task(self.rt, ToggleKind::Dialog, dialog, serial) {
            self.error.get_or_insert(error);
        }
    }

    pub(super) fn queue_close(&mut self, dialog: NodeId) {
        if let Err(error) = queue_close_task(self.rt, dialog) {
            self.error.get_or_insert(error);
        }
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

/// Queues the task that fires `close` at `dialog`.
pub(crate) fn queue_close_task(rt: &mut Runtime, dialog: NodeId) -> VmResult<()> {
    let now = crate::event_loop::page_now(rt);
    state_mut(rt)?
        .event_loop
        .tasks
        .push(now, Task::DialogClose(dialog));
    Ok(())
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
