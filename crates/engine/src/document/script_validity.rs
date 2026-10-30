//! What the script runtime asks of constraint validation (M8 feature
//! 9b2): the validity states, `validationMessage` and
//! `setCustomValidity()` of a control, and whether an `invalid` event is
//! due ([`swb_web::EngineHost`]).
//!
//! A control in the document is read from the states of `forms` (made
//! current first); a control outside the document gets a state of its own
//! from its attributes and what scripts wrote to it
//! ([`crate::forms::detached_state`]), so a new `input` validates too.
//! The custom messages live in [`DocumentState::validity`], by element, so
//! they stay with the element when it leaves and enters the document.

use swb_dom::{Document, NodeId};
use swb_web::{ControlValidity, ControlWrite};

use super::DocumentState;
use crate::forms::{ControlType, Forms, ValidityCx, detached_state, select_is_attached};

impl DocumentState {
    /// Runs `f` with a set of states that has the control `node`, and the
    /// validity inputs. `None` if `node` is not a form control.
    fn with_validity_subject<R>(
        &mut self,
        node: NodeId,
        f: impl FnOnce(&Forms, &Document, &ValidityCx) -> R,
    ) -> Option<R> {
        let ty = ControlType::of(self.dom.element(node)?)?;
        self.sync_type(node, ty);
        if select_is_attached(&self.dom, node) {
            self.ensure_forms();
            if self.forms.value().typed_state(node, ty).is_some() {
                return Some(f(self.forms.value(), &self.dom, &self.validity));
            }
        } else if self.forms.value().has_state(node) {
            // The control left the document since the last rebuild: the
            // rebuild keeps what it held as seeds.
            self.ensure_forms();
        }
        let forms = Forms::single(node, detached_state(&self.dom, node, ty, &self.seeds));
        Some(f(&forms, &self.dom, &self.validity))
    }

    /// The validity states of `node`. An element that is no form control
    /// (`fieldset`, `output`) has only the custom error and is never a
    /// candidate.
    pub(crate) fn script_validity(&mut self, node: NodeId) -> ControlValidity {
        self.validity.begin_pass();
        let found =
            self.with_validity_subject(node, |forms, dom, cx| forms.validity_of(dom, node, cx));
        match found.flatten() {
            Some((v, candidate)) => ControlValidity {
                will_validate: candidate,
                value_missing: v.value_missing,
                type_mismatch: v.type_mismatch,
                pattern_mismatch: v.pattern_mismatch,
                too_long: v.too_long,
                too_short: v.too_short,
                range_underflow: v.range_underflow,
                range_overflow: v.range_overflow,
                step_mismatch: v.step_mismatch,
                bad_input: v.bad_input,
                custom_error: v.custom_error,
            },
            None => ControlValidity {
                custom_error: self.validity.custom(node).is_some(),
                ..ControlValidity::default()
            },
        }
    }

    /// `validationMessage`.
    pub(crate) fn script_validation_message(&mut self, node: NodeId) -> String {
        self.validity.begin_pass();
        self.with_validity_subject(node, |forms, dom, cx| {
            forms.validation_message(dom, node, cx)
        })
        .flatten()
        .unwrap_or_default()
    }

    /// True if `node` is a candidate for constraint validation and fails
    /// it.
    pub(crate) fn script_is_invalid(&mut self, node: NodeId) -> bool {
        self.with_validity_subject(node, |forms, dom, cx| forms.is_node_invalid(dom, node, cx))
            .unwrap_or(false)
    }

    /// `setCustomValidity(message)`. Line breaks are normalized to LF
    /// (Chromium, measured). The messages count against
    /// [`crate::forms::MAX_CUSTOM_BYTES`].
    pub(crate) fn script_set_custom_validity(
        &mut self,
        node: NodeId,
        message: &str,
    ) -> ControlWrite {
        let message = message.replace("\r\n", "\n").replace('\r', "\n");
        if !self.validity.set_custom(node, &message) {
            log::warn!(
                "the custom validity messages reached the limit of {} bytes; the task ends",
                crate::forms::MAX_CUSTOM_BYTES
            );
            return ControlWrite::OverLimit;
        }
        self.controls_changed();
        ControlWrite::Done
    }
}
