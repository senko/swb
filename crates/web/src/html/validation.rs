//! Constraint validation for scripts (HTML §4.10.20.3, "Constraint
//! validation"): `willValidate`, `validity` (`ValidityState`),
//! `validationMessage`, `checkValidity()`, `reportValidity()` and
//! `setCustomValidity()` of the form controls, `fieldset`, `output` and
//! `object`, the two methods of `form`, and `labels`.
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#the-constraint-validation-api>
//!
//! The validity states and the texts are the engine's
//! ([`crate::EngineHost::control_validity`]); this module fires the
//! `invalid` events and moves the focus. Measured in Chromium 148
//! (`tools/probes/web-forms-validation.json`): `invalid` does not bubble
//! and can be canceled; `checkValidity()` of a form fires it at each
//! control that is still invalid when its turn comes (a listener can fix a
//! later control); `reportValidity()`, `requestSubmit()` and a click on a
//! submit button fire the events at all invalid controls first and then
//! focus (and scroll to) the first control whose event nobody canceled.
//! swb shows no bubble with the message: the message is only readable
//! through `validationMessage`.

use swb_dom::NodeId;
use swb_js::{Gc, Object, Runtime, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::dom::{ElementList, lists};
use crate::events::{DispatchTarget, TrustedEvent, fire_trusted};
use crate::generated as g;
use crate::state::{Slot, WebState, dom, engine_mut, same_object};

/// The members of the constraint validation API that every interface of
/// a listed element has in the same form. The macro expands inside the
/// `impl` of the generated trait of the interface.
macro_rules! constraint_validation_members {
    () => {
        fn will_validate(
            cx: &mut $crate::bindings::Cx<'_>,
            this: swb_dom::NodeId,
        ) -> $crate::bindings::Result<bool> {
            $crate::html::validation::will_validate(cx, this)
        }

        fn validity(
            cx: &mut $crate::bindings::Cx<'_>,
            this: swb_dom::NodeId,
        ) -> $crate::bindings::Result<swb_js::Gc<swb_js::Object>> {
            $crate::html::validation::validity(cx, this)
        }

        fn validation_message(
            cx: &mut $crate::bindings::Cx<'_>,
            this: swb_dom::NodeId,
        ) -> $crate::bindings::Result<String> {
            $crate::html::validation::validation_message(cx, this)
        }

        fn check_validity(
            cx: &mut $crate::bindings::Cx<'_>,
            this: swb_dom::NodeId,
        ) -> $crate::bindings::Result<bool> {
            $crate::html::validation::check_validity(cx, this)
        }

        fn report_validity(
            cx: &mut $crate::bindings::Cx<'_>,
            this: swb_dom::NodeId,
        ) -> $crate::bindings::Result<bool> {
            $crate::html::validation::report_validity(cx, this)
        }

        fn set_custom_validity(
            cx: &mut $crate::bindings::Cx<'_>,
            this: swb_dom::NodeId,
            error: String,
        ) -> $crate::bindings::Result<()> {
            $crate::html::validation::set_custom_validity(cx, this, &error)
        }
    };
}
pub(crate) use constraint_validation_members;

/// `willValidate`.
pub(crate) fn will_validate(cx: &mut Cx<'_>, node: NodeId) -> b::Result<bool> {
    Ok(engine_mut(cx.rt)?.control_validity(node).will_validate)
}

/// `validity`: the one `ValidityState` of the element.
pub(crate) fn validity(cx: &mut Cx<'_>, node: NodeId) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    Ok(same_object(cx.rt, node, Slot::Validity, |rt| {
        rt.new_host_object(realm, g::VALIDITY_STATE, b::node_data(node))
    })?)
}

/// `validationMessage`.
pub(crate) fn validation_message(cx: &mut Cx<'_>, node: NodeId) -> b::Result<String> {
    let message = engine_mut(cx.rt)?.control_validation_message(node);
    cx.rt.check_time()?;
    Ok(message)
}

/// `setCustomValidity(error)`. The task ends if the messages reached the
/// budget of the document (ADR 0028 section 16).
pub(crate) fn set_custom_validity(cx: &mut Cx<'_>, node: NodeId, message: &str) -> b::Result<()> {
    let result = engine_mut(cx.rt)?.set_custom_validity(node, message);
    super::over_limit(cx, result)
}

/// Fires `invalid` at `node` (it does not bubble, it can be canceled).
/// True if a listener canceled it.
fn fire_invalid(rt: &mut Runtime, node: NodeId) -> VmResult<bool> {
    let mut event = TrustedEvent::new("invalid");
    event.cancelable = true;
    Ok(!fire_trusted(rt, DispatchTarget::Node(node), &event)?)
}

/// `checkValidity()` of a control: true if it is not a candidate or is
/// valid; else the `invalid` event, and false.
pub(crate) fn check_validity(cx: &mut Cx<'_>, node: NodeId) -> b::Result<bool> {
    if !engine_mut(cx.rt)?.control_is_invalid(node) {
        return Ok(true);
    }
    fire_invalid(cx.rt, node)?;
    cx.rt.check_time()?;
    Ok(false)
}

/// `reportValidity()` of a control: as `checkValidity()`; and if no
/// listener canceled the event, the control gets the focus (swb shows no
/// bubble; Chromium also scrolls it into view, so does `focus`).
pub(crate) fn report_validity(cx: &mut Cx<'_>, node: NodeId) -> b::Result<bool> {
    if !engine_mut(cx.rt)?.control_is_invalid(node) {
        return Ok(true);
    }
    if !fire_invalid(cx.rt, node)? {
        super::focus::focus_node(cx.rt, node, true)?;
    }
    cx.rt.check_time()?;
    Ok(false)
}

/// `form.checkValidity()`: HTML "statically validate the constraints":
/// the `invalid` event at each control that fails, in tree order. A
/// control that a listener fixed before its turn gets none (measured).
pub(crate) fn check_form(cx: &mut Cx<'_>, form: NodeId) -> b::Result<bool> {
    let invalid = engine_mut(cx.rt)?.invalid_controls(form);
    for &node in &invalid {
        if engine_mut(cx.rt)?.control_is_invalid(node) {
            fire_invalid(cx.rt, node)?;
        }
        cx.rt.check_time()?;
    }
    Ok(invalid.is_empty())
}

/// HTML "interactively validate the constraints" (`form.reportValidity()`,
/// `requestSubmit()`, a click on a submit button, implicit submission): an
/// `invalid` event at each control that fails, then the focus for the
/// first control whose event nobody canceled (measured). False if the form
/// is invalid.
pub(crate) fn report_form(rt: &mut Runtime, form: NodeId) -> VmResult<bool> {
    let invalid = engine_mut(rt)?.invalid_controls(form);
    if invalid.is_empty() {
        return Ok(true);
    }
    engine_mut(rt)?.form_validation_reported(form);
    let mut first = None;
    for &node in &invalid {
        if engine_mut(rt)?.control_is_invalid(node) {
            let canceled = fire_invalid(rt, node)?;
            if !canceled && first.is_none() {
                first = Some(node);
            }
        }
        rt.check_time()?;
    }
    if let Some(node) = first {
        super::focus::focus_node(rt, node, true)?;
    }
    Ok(false)
}

/// `form.reportValidity()`.
pub(crate) fn report_form_method(cx: &mut Cx<'_>, form: NodeId) -> b::Result<bool> {
    Ok(report_form(cx.rt, form)?)
}

/// `labels` of the labelable element `node`: the one live `NodeList` of
/// the labels whose labeled control it is. `None` for an `input` of the
/// type `hidden`, which is not labelable.
/// <https://html.spec.whatwg.org/multipage/forms.html#dom-lfe-labels>
pub(crate) fn labels(cx: &mut Cx<'_>, node: NodeId) -> b::Result<Option<Gc<Object>>> {
    let hidden = dom(cx.rt)?.element(node).is_some_and(|e| {
        &**e.local_name() == "input"
            && e.attr("type")
                .is_some_and(|t| t.eq_ignore_ascii_case("hidden"))
    });
    if hidden {
        return Ok(None);
    }
    Ok(Some(lists::element_list(cx, node, ElementList::Labels)?))
}

/// The flags of the element that a `ValidityState` object belongs to.
fn flags(cx: &mut Cx<'_>, this: Platform) -> b::Result<crate::ControlValidity> {
    let node = b::node_of(this.data).ok_or_else(|| Exception::type_error("Illegal invocation"))?;
    Ok(engine_mut(cx.rt)?.control_validity(node))
}

impl g::validity_state::ValidityStateImpl for WebState {
    fn value_missing(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.value_missing)
    }

    fn type_mismatch(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.type_mismatch)
    }

    fn pattern_mismatch(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.pattern_mismatch)
    }

    fn too_long(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.too_long)
    }

    fn too_short(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.too_short)
    }

    fn range_underflow(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.range_underflow)
    }

    fn range_overflow(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.range_overflow)
    }

    fn step_mismatch(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.step_mismatch)
    }

    fn bad_input(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.bad_input)
    }

    fn custom_error(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.custom_error)
    }

    fn valid(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(flags(cx, this)?.is_valid())
    }
}

impl g::html_meter_element::HTMLMeterElementImpl for WebState {
    fn labels(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::Labels)
    }
}

impl g::html_progress_element::HTMLProgressElementImpl for WebState {
    fn labels(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::Labels)
    }
}
