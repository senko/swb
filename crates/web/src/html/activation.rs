//! Activation behavior, `click()` and the form methods (HTML §4.10.21.3
//! "Form submission", §4.10.21.4 "Resetting a form", the activation
//! behaviors of `a`, `input`, `button` and `label`; DOM §2.9
//! "dispatch", steps for the activation target):
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>.
//!
//! A `click` event that is a `MouseEvent` (also one that a script
//! dispatches) has an *activation target*: the event's target, or, if the
//! event bubbles, its nearest ancestor with an activation behavior. A
//! checkbox or a radio button changes its state *before* the event (the
//! legacy-pre-activation behavior) and returns to the old state if a
//! listener cancels the event; if not, `input` and `change` follow. A
//! link, a submit button, a reset button and a label act after the
//! dispatch.
//!
//! What is the engine's goes through [`crate::EngineHost`]: the
//! checkedness of controls, the invalid controls of a form, the reset of
//! its controls, and the requests that the page acts on when the task ends
//! (a navigation, a scroll).
//!
//! Measured in Chromium 148 (`tools/probes/web-input-handlers.json`):
//! `dispatchEvent` of a `click` `MouseEvent` activates a disabled checkbox;
//! only `click()` stops at disabled controls; a radio button that is
//! already checked fires no `input` and `change`; a disconnected control
//! fires none either; the `submit` event is a trusted `SubmitEvent` that
//! bubbles, can be canceled and is not composed; `input` is an `Event`
//! (bubbles, composed), `change` an `Event` (bubbles).

use std::collections::HashSet;

use swb_dom::{Document, NodeId, local_name, ns};
use swb_js::{Runtime, VmResult};

use super::{form_owner, labeled_control};
use crate::bindings::{self as b, Cx, Host};
use crate::events::{
    DispatchTarget, EventInterface, TrustedEvent, dispatch, fire_trusted, handlers, new_click_event,
};
use crate::state::{WebState, dom, engine_mut, state, state_mut};

/// The elements that are in the middle of an operation that must not
/// start again (the flags of the HTML algorithms).
#[derive(Debug, Default)]
pub(crate) struct Busy {
    /// "Locked for focus".
    focus: HashSet<NodeId>,
    /// "Click in progress flag".
    click: HashSet<NodeId>,
    /// "Firing submission events".
    submit: HashSet<NodeId>,
    /// "Locked for reset".
    reset: HashSet<NodeId>,
    /// A label that is dispatching the click at its control.
    label: HashSet<NodeId>,
}

/// Which activation behavior an element has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Link,
    Checkbox,
    Radio,
    Submit,
    Reset,
    Label,
}

/// The state that the legacy-pre-activation behavior saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Saved {
    None,
    Checkbox {
        checked: bool,
        indeterminate: bool,
    },
    Radio {
        was_checked: bool,
        previous: Option<NodeId>,
    },
}

/// The activation target of a `click` event.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Activation {
    node: NodeId,
    /// The target of the event (the activation target or a descendant).
    origin: NodeId,
    kind: Kind,
    saved: Saved,
}

/// The activation behavior of an element.
fn kind_of(dom: &Document, node: NodeId) -> Option<Kind> {
    let e = dom.element(node)?;
    if e.name.ns == ns!(svg) {
        let link = *e.local_name() == local_name!("a") && e.svg_href().is_some();
        return link.then_some(Kind::Link);
    }
    if e.name.ns != ns!(html) {
        return None;
    }
    let ty = || e.attr("type").map(str::to_ascii_lowercase);
    match &**e.local_name() {
        "a" | "area" => e.has_attr("href").then_some(Kind::Link),
        "label" => Some(Kind::Label),
        "input" => match ty().as_deref() {
            Some("checkbox") => Some(Kind::Checkbox),
            Some("radio") => Some(Kind::Radio),
            Some("submit" | "image") => Some(Kind::Submit),
            Some("reset") => Some(Kind::Reset),
            _ => None,
        },
        "button" => match ty().as_deref() {
            Some("reset") => Some(Kind::Reset),
            Some("button") => None,
            _ => Some(Kind::Submit),
        },
        _ => None,
    }
}

/// The activation target for a click on `node`: the node, or with
/// `bubbles` the nearest ancestor with an activation behavior.
pub(crate) fn find_target(
    rt: &Runtime,
    node: NodeId,
    bubbles: bool,
) -> VmResult<Option<Activation>> {
    let node_target = node;
    let dom = dom(rt)?;
    let found = std::iter::once(node)
        .chain(
            dom.ancestors(node)
                .take(if bubbles { usize::MAX } else { 0 }),
        )
        .find_map(|n| Some((n, kind_of(dom, n)?)));
    Ok(found.map(|(node, kind)| Activation {
        node,
        origin: node_target,
        kind,
        saved: Saved::None,
    }))
}

/// The legacy-pre-activation behavior (HTML, `input` of the type
/// `checkbox` and `radio`): the state changes before the event.
pub(crate) fn pre_activation(rt: &mut Runtime, mut a: Activation) -> VmResult<Activation> {
    match a.kind {
        Kind::Checkbox => {
            let checked = engine_mut(rt)?.control_checked(a.node).unwrap_or(false);
            let indeterminate = state(rt)?.indeterminate.contains(&a.node);
            a.saved = Saved::Checkbox {
                checked,
                indeterminate,
            };
            set_checked(rt, a.node, !checked)?;
            set_indeterminate(rt, a.node, false)?;
        }
        Kind::Radio => {
            let was_checked = engine_mut(rt)?.control_checked(a.node).unwrap_or(false);
            let previous = engine_mut(rt)?
                .checked_radio_in_group(a.node)
                .filter(|&n| n != a.node);
            a.saved = Saved::Radio {
                was_checked,
                previous,
            };
            set_checked(rt, a.node, true)?;
        }
        _ => {}
    }
    Ok(a)
}

fn set_checked(rt: &mut Runtime, node: NodeId, checked: bool) -> VmResult<()> {
    engine_mut(rt)?.set_control_checked(node, checked);
    Ok(())
}

fn set_indeterminate(rt: &mut Runtime, node: NodeId, value: bool) -> VmResult<()> {
    let changed = if value {
        state_mut(rt)?.indeterminate.insert(node)
    } else {
        state_mut(rt)?.indeterminate.remove(&node)
    };
    if changed {
        engine_mut(rt)?.set_control_indeterminate(node, value);
    }
    Ok(())
}

/// DOM "dispatch" steps 5.15 and 5.16: the activation behavior if the
/// event was not canceled, else the legacy-canceled-activation behavior.
pub(crate) fn after_dispatch(rt: &mut Runtime, a: Activation, canceled: bool) -> VmResult<()> {
    if canceled {
        return canceled_activation(rt, a);
    }
    // A listener may have changed the element since.
    if kind_of(dom(rt)?, a.node) != Some(a.kind) {
        return Ok(());
    }
    match a.kind {
        Kind::Checkbox => fire_input_and_change(rt, a.node),
        Kind::Radio => match a.saved {
            Saved::Radio { was_checked, .. } if was_checked => Ok(()),
            _ => fire_input_and_change(rt, a.node),
        },
        Kind::Submit => activate_submit(rt, a.node),
        Kind::Reset => activate_reset(rt, a.node),
        Kind::Link => {
            engine_mut(rt)?.request_hyperlink(a.node);
            Ok(())
        }
        Kind::Label => activate_label(rt, a.node, a.origin),
    }
}

/// The legacy-canceled-activation behavior: the old state returns.
fn canceled_activation(rt: &mut Runtime, a: Activation) -> VmResult<()> {
    match a.saved {
        Saved::Checkbox {
            checked,
            indeterminate,
        } => {
            set_checked(rt, a.node, checked)?;
            set_indeterminate(rt, a.node, indeterminate)
        }
        Saved::Radio { previous, .. } => match previous {
            Some(previous) => set_checked(rt, previous, true),
            None => set_checked(rt, a.node, false),
        },
        Saved::None => Ok(()),
    }
}

/// `input` (bubbles, composed) and `change` (bubbles) at a connected
/// control.
fn fire_input_and_change(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    if !dom(rt)?.is_connected(node) {
        return Ok(());
    }
    for (kind, composed) in [("input", true), ("change", false)] {
        let mut event = TrustedEvent::new(kind);
        event.bubbles = true;
        event.composed = composed;
        fire_trusted(rt, DispatchTarget::Node(node), &event)?;
    }
    Ok(())
}

fn is_disabled(rt: &Runtime, node: NodeId) -> VmResult<bool> {
    Ok(swb_style::is_actually_disabled(dom(rt)?, node))
}

/// The activation behavior of a submit button: submits its form.
fn activate_submit(rt: &mut Runtime, button: NodeId) -> VmResult<()> {
    let Some(form) = form_owner(dom(rt)?, button) else {
        return Ok(());
    };
    if is_disabled(rt, button)? {
        return Ok(());
    }
    submit(rt, form, Some(button), false)
}

/// The activation behavior of a reset button: resets its form.
fn activate_reset(rt: &mut Runtime, button: NodeId) -> VmResult<()> {
    let Some(form) = form_owner(dom(rt)?, button) else {
        return Ok(());
    };
    if is_disabled(rt, button)? {
        return Ok(());
    }
    reset(rt, form)
}

/// The activation behavior of a label (HTML, `label`): it focuses the
/// labeled control and clicks it (Chromium's simulated click). It does
/// nothing if the event came from the control or from inside it, and a
/// label that is already clicking its control does not start again, so
/// that no chain of labels, `for` attributes and nesting can recurse.
fn activate_label(rt: &mut Runtime, label: NodeId, origin: NodeId) -> VmResult<()> {
    let document = dom(rt)?;
    let Some(control) = labeled_control(document, label) else {
        return Ok(());
    };
    if origin == control || document.ancestors(origin).any(|a| a == control) {
        return Ok(());
    }
    if is_disabled(rt, control)? || !state_mut(rt)?.busy.label.insert(label) {
        return Ok(());
    }
    let result =
        super::focus::focus_node(rt, control, true).and_then(|()| dispatch_click(rt, control));
    state_mut(rt)?.busy.label.remove(&label);
    result
}

/// Dispatches a synthetic `click` at `node`.
fn dispatch_click(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    let scope = rt.heap_mut().open_scope();
    let result = (|| {
        let target = WebState::wrap_node(rt, 0, node)?;
        let event = new_click_event(rt)?;
        dispatch(rt, 0, target, event, None).map(|_| ())
    })();
    if let Err(error) = rt.heap_mut().close_scope(scope) {
        log::warn!("the handle scope of a click was not the last one: {error:?}");
    }
    result
}

/// `HTMLElement.click()` (HTML §6.2.1, "dom-click"): a disabled form control
/// does nothing; a click that is in progress on the element is not
/// repeated; the event is a synthetic `click` that is not trusted.
pub(crate) fn click(cx: &mut Cx<'_>, node: NodeId) -> b::Result<()> {
    if is_disabled(cx.rt, node)? {
        return Ok(());
    }
    if !state_mut(cx.rt)?.busy.click.insert(node) {
        return Ok(());
    }
    let result =
        handlers::with_context(cx.rt, "Failed to execute 'click' on 'HTMLElement'", |rt| {
            dispatch_click(rt, node)
        });
    state_mut(cx.rt)?.busy.click.remove(&node);
    Ok(result?)
}

// ----- Forms -----

/// True if the form `form` has the `novalidate` attribute, or the
/// submitter has `formnovalidate`.
fn skips_validation(dom: &Document, form: NodeId, submitter: Option<NodeId>) -> bool {
    dom.element(form).is_some_and(|e| e.has_attr("novalidate"))
        || submitter
            .and_then(|n| dom.element(n))
            .is_some_and(|e| e.has_attr("formnovalidate"))
}

/// HTML "submit": validates (unless `from_method`: `form.submit()`),
/// fires `submit` (also unless `from_method`) and, if nothing canceled it,
/// asks the engine to navigate.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>
pub(crate) fn submit(
    rt: &mut Runtime,
    form: NodeId,
    submitter: Option<NodeId>,
    from_method: bool,
) -> VmResult<()> {
    let document = dom(rt)?;
    if !document.is_connected(form) || document.node_document(form) != NodeId::DOCUMENT {
        return Ok(());
    }
    if !state_mut(rt)?.busy.submit.insert(form) {
        return Ok(());
    }
    let result = submit_steps(rt, form, submitter, from_method);
    state_mut(rt)?.busy.submit.remove(&form);
    result
}

fn submit_steps(
    rt: &mut Runtime,
    form: NodeId,
    submitter: Option<NodeId>,
    from_method: bool,
) -> VmResult<()> {
    if !from_method {
        if !skips_validation(dom(rt)?, form, submitter) && !validate(rt, form)? {
            return Ok(());
        }
        let mut event = TrustedEvent::new("submit");
        event.bubbles = true;
        event.cancelable = true;
        event.interface = EventInterface::Submit { submitter };
        let proceed = fire_trusted(rt, DispatchTarget::Node(form), &event)?;
        // A listener may have taken the form out of the document.
        if !proceed || !dom(rt)?.is_connected(form) {
            return Ok(());
        }
    }
    engine_mut(rt)?.request_form_submission(form, submitter);
    Ok(())
}

/// HTML "interactively validate the constraints": an `invalid` event at
/// each invalid control; the first one gets the focus unless a listener
/// canceled its event. False if the form is invalid.
fn validate(rt: &mut Runtime, form: NodeId) -> VmResult<bool> {
    let invalid = engine_mut(rt)?.invalid_controls(form);
    if invalid.is_empty() {
        return Ok(true);
    }
    let mut first_canceled = false;
    for (index, &node) in invalid.iter().enumerate() {
        let mut event = TrustedEvent::new("invalid");
        event.cancelable = true;
        let canceled = !fire_trusted(rt, DispatchTarget::Node(node), &event)?;
        if index == 0 {
            first_canceled = canceled;
        }
    }
    if !first_canceled && let Some(&first) = invalid.first() {
        super::focus::focus_node(rt, first, true)?;
    }
    Ok(false)
}

/// HTML "reset": `reset` at the form (bubbles, can be canceled), then the
/// controls return to their defaults. A reset that runs inside the `reset`
/// event of the same form does nothing ("locked for reset").
pub(crate) fn reset(rt: &mut Runtime, form: NodeId) -> VmResult<()> {
    if !state_mut(rt)?.busy.reset.insert(form) {
        return Ok(());
    }
    let mut event = TrustedEvent::new("reset");
    event.bubbles = true;
    event.cancelable = true;
    let result = fire_trusted(rt, DispatchTarget::Node(form), &event);
    state_mut(rt)?.busy.reset.remove(&form);
    if result? {
        engine_mut(rt)?.reset_form(form);
    }
    Ok(())
}

/// `form.requestSubmit(submitter)` (HTML §4.10.21.3): the submitter must
/// be a submit button owned by the form.
pub(crate) fn request_submit(
    cx: &mut Cx<'_>,
    form: NodeId,
    submitter: Option<NodeId>,
) -> b::Result<()> {
    if let Some(node) = submitter {
        let document = dom(cx.rt)?;
        if kind_of(document, node) != Some(Kind::Submit) {
            return Err(b::Exception::type_error(
                "The specified element is not a submit button.",
            ));
        }
        if form_owner(document, node) != Some(form) {
            return Err(b::Exception::dom(
                "NotFoundError",
                "The specified element is not owned by this form element.",
            ));
        }
    }
    Ok(submit(cx.rt, form, submitter, false)?)
}

/// `form.submit()`: no validation and no `submit` event.
pub(crate) fn submit_method(cx: &mut Cx<'_>, form: NodeId) -> b::Result<()> {
    Ok(submit(cx.rt, form, None, true)?)
}

/// `form.reset()`.
pub(crate) fn reset_method(cx: &mut Cx<'_>, form: NodeId) -> b::Result<()> {
    Ok(reset(cx.rt, form)?)
}

/// Used by `focus.rs`: the busy set for "locked for focus".
pub(crate) fn lock_focus(rt: &mut Runtime, node: NodeId) -> VmResult<bool> {
    Ok(state_mut(rt)?.busy.focus.insert(node))
}

/// Used by `focus.rs`: leaves "locked for focus".
pub(crate) fn unlock_focus(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    state_mut(rt)?.busy.focus.remove(&node);
    Ok(())
}
