//! What the script runtime asks of the form controls (M8 feature 6c,
//! [`swb_web::EngineHost`]): the value, the checkedness and the
//! selectedness of controls, as the `value`, `checked`, `selected` and
//! `selectedIndex` IDL attributes read and write them.
//!
//! The state is that of `forms` when the control has an entry and the
//! user or a script changed it (the dirty flag). Without one it is the
//! value that a script wrote before ([`crate::forms::Seeds`]), else the
//! default from the attributes and content. A write changes the entry
//! and marks the element states of the controls outdated
//! ([`DocumentState::refresh_forms`] updates them at the next flush), or
//! it becomes a seed that the next rebuild applies. Reads and writes of
//! values and checkedness cost O(1) in the number of controls; those of
//! selects rebuild `forms` first if the DOM changed (the options of a
//! select are in `forms`).

use swb_dom::{Document, NodeId};
use swb_web::ControlWrite;

use super::DocumentState;
use crate::forms::{
    ControlType, InputType, OptionState, option_is_disabled, options_of_detached,
    select_is_attached, select_of_option, settle,
};

/// The bytes of values that scripts may write to text fields and text areas
/// in one document (a row of the limits table in ADR 0028 section 16).
pub(crate) const MAX_CONTROL_BYTES: usize = 32 << 20;

impl DocumentState {
    /// The control type of `node` as the DOM says now.
    fn script_type(&self, node: NodeId) -> Option<ControlType> {
        ControlType::of(self.dom.element(node)?)
    }

    /// The `indeterminate` property of an input changed.
    pub(crate) fn script_set_indeterminate(&mut self, node: NodeId, value: bool) {
        let changed = if value {
            self.indeterminate.insert(node)
        } else {
            self.indeterminate.remove(&node)
        };
        if changed {
            self.controls_changed();
        }
    }

    /// A script changed the state of a control.
    fn controls_changed(&mut self) {
        self.controls_stale = true;
        self.invalidate_layout();
    }

    /// The `value` of a text field or text area, or `None` for the other
    /// elements.
    pub(crate) fn script_control_value(&mut self, node: NodeId) -> Option<String> {
        let ty = self.script_type(node).filter(|t| t.is_text())?;
        let e = self.dom.element(node)?;
        if let Some(state) = self.forms.value().typed_state(node, ty)
            && state.dirty
        {
            return Some(crate::forms::state_value(e, &self.dom, state));
        }
        if let Some(value) = self.seeds.value(node) {
            return Some(crate::forms::sanitize_value(e, ty, value));
        }
        Some(crate::forms::default_text(&self.dom, node, ty))
    }

    /// Sets the value of a text field or text area. The values that
    /// scripts wrote count against [`MAX_CONTROL_BYTES`]; past it the
    /// value is not stored.
    pub(crate) fn script_set_control_value(&mut self, node: NodeId, value: &str) -> ControlWrite {
        let Some(ty) = self.script_type(node).filter(|t| t.is_text()) else {
            return ControlWrite::NotControl;
        };
        let Some(e) = self.dom.element(node) else {
            return ControlWrite::NotControl;
        };
        let value = crate::forms::sanitize_value(e, ty, value);
        // The bytes that the old value of the control held.
        let held = match self.forms.value().typed_state(node, ty) {
            Some(state) if state.dirty => state.edit.text().len(),
            Some(_) => 0,
            None => self.seeds.value(node).map_or(0, str::len),
        };
        let total = self.control_bytes.saturating_sub(held) + value.len();
        if total > MAX_CONTROL_BYTES {
            log::warn!(
                "the values of form controls reached the limit of {MAX_CONTROL_BYTES} bytes; the task ends"
            );
            return ControlWrite::OverLimit;
        }
        self.control_bytes = total;
        if let Some(state) = self.forms.value_mut().typed_state_mut(node, ty) {
            if state.edit.text() != value {
                state.edit.replace_text(&value);
            }
            state.dirty = true;
        } else {
            self.seeds.set_value(node, value);
        }
        self.controls_changed();
        ControlWrite::Done
    }

    /// The cloning steps of `input` and `textarea`: the copy `to` gets the
    /// value and the checkedness of `from` when their dirty flags are set.
    pub(crate) fn script_copy_control_state(&mut self, from: NodeId, to: NodeId) -> ControlWrite {
        let Some(ty) = self.script_type(from) else {
            return ControlWrite::Done;
        };
        if ty.is_text() {
            let dirty = self.seeds.value(from).is_some()
                || self
                    .forms
                    .value()
                    .typed_state(from, ty)
                    .is_some_and(|s| s.dirty);
            if dirty && let Some(value) = self.script_control_value(from) {
                return self.script_set_control_value(to, &value);
            }
        } else if ty.is_checkable() {
            let dirty = self.seeds.checked(from).is_some()
                || self
                    .forms
                    .value()
                    .typed_state(from, ty)
                    .is_some_and(|s| s.dirty);
            if dirty && let Some(checked) = self.script_control_checked(from) {
                self.script_set_control_checked(to, checked);
            }
        }
        ControlWrite::Done
    }

    /// The checkedness of a checkbox or radio button. The state of a radio
    /// button in the document is current first: the checked radio buttons
    /// of a group are settled when the states are built.
    pub(crate) fn script_control_checked(&mut self, node: NodeId) -> Option<bool> {
        let ty = self.script_type(node).filter(|t| t.is_checkable())?;
        let radio_in_document =
            ty == ControlType::Input(InputType::Radio) && select_is_attached(&self.dom, node);
        if radio_in_document {
            self.ensure_forms();
        }
        if let Some(state) = self.forms.value().typed_state(node, ty)
            && (state.dirty || radio_in_document)
        {
            return Some(state.checked);
        }
        if let Some(checked) = self.seeds.checked(node) {
            return Some(checked);
        }
        Some(self.dom.element(node)?.has_attr("checked"))
    }

    /// Sets the checkedness of a checkbox or radio button; checking a
    /// radio button unchecks the others of its group (the newest wins).
    /// False for the other elements.
    pub(crate) fn script_set_control_checked(&mut self, node: NodeId, checked: bool) -> bool {
        let Some(ty) = self.script_type(node).filter(|t| t.is_checkable()) else {
            return false;
        };
        // A radio button in the document that a script checks while the
        // states are outdated becomes a seed, as one outside the document:
        // the next rebuild applies it as the newest of its group. A loop
        // that appends and checks radio buttons then costs no rebuild per
        // write.
        let outdated = !self.forms.is_current(self.dom.generation());
        let seed_radio = checked
            && ty == ControlType::Input(InputType::Radio)
            && select_is_attached(&self.dom, node)
            && outdated;
        if !seed_radio && self.forms.value().typed_state(node, ty).is_some() {
            let forms = self.forms.value_mut();
            forms.set_checked(&self.dom, node, checked);
        } else {
            // A control outside the document is "inserted" later (stamp 0).
            let stamp = if select_is_attached(&self.dom, node) {
                self.forms.value_mut().next_stamp()
            } else {
                0
            };
            self.seeds.set_checked(node, checked, stamp);
        }
        self.controls_changed();
        true
    }

    /// Reads the options of a select with their selectedness, as the
    /// select has them now.
    fn read_options<R>(&mut self, select: NodeId, f: impl FnOnce(&[OptionState]) -> R) -> R {
        if select_is_attached(&self.dom, select) {
            self.ensure_forms();
            if let Some(state) = self.forms.value().typed_state(select, ControlType::Select) {
                return f(&state.options);
            }
        } else if self.forms.value().has_state(select) {
            // The select left the document since the last rebuild: the
            // rebuild keeps what it held as seeds.
            self.ensure_forms();
        }
        f(&options_of_detached(&self.dom, select, &self.seeds))
    }

    /// Changes the selectedness of the options of a select in place (a
    /// select in the document keeps them in its state; one outside the
    /// document gets seeds).
    fn write_options<R>(
        &mut self,
        select: NodeId,
        f: impl FnOnce(&Document, &mut [OptionState]) -> R,
    ) -> R {
        let mut f = Some(f);
        let mut result = None;
        if select_is_attached(&self.dom, select) {
            self.ensure_forms();
            if let Some(state) = self
                .forms
                .value_mut()
                .typed_state_mut(select, ControlType::Select)
                && let Some(f) = f.take()
            {
                state.dirty = true;
                result = Some(f(&self.dom, &mut state.options));
            }
        }
        if result.is_none() {
            if self.forms.value().has_state(select) {
                self.ensure_forms();
            }
            let mut options = options_of_detached(&self.dom, select, &self.seeds);
            result = f.take().map(|f| f(&self.dom, &mut options));
            for option in options {
                self.seeds.set_selected(option.node, option.selected);
            }
        }
        self.controls_changed();
        result.expect("one of the two branches ran the change")
    }

    /// `select.selectedIndex`: the index of the first selected option,
    /// or -1.
    pub(crate) fn script_selected_index(&mut self, select: NodeId) -> i32 {
        self.read_options(select, |options| {
            options
                .iter()
                .position(|o| o.selected)
                .and_then(|i| i32::try_from(i).ok())
                .unwrap_or(-1)
        })
    }

    /// `select.selectedIndex = index`: the option at `index` is the only
    /// selected one; no option is selected for an index out of range.
    pub(crate) fn script_set_selected_index(&mut self, select: NodeId, index: i32) {
        let wanted = usize::try_from(index).ok();
        self.write_options(select, |_, options| {
            for (i, option) in options.iter_mut().enumerate() {
                option.selected = Some(i) == wanted;
            }
        });
    }

    /// `option.selected`.
    pub(crate) fn script_option_selected(&mut self, option: NodeId) -> bool {
        if let Some(select) = select_of_option(&self.dom, option) {
            return self.read_options(select, |options| {
                options
                    .iter()
                    .find(|o| o.node == option)
                    .is_some_and(|o| o.selected)
            });
        }
        if self.forms.value().has_option(option) {
            // The option left its select since the last rebuild: the
            // rebuild keeps its selectedness as a seed.
            self.ensure_forms();
        }
        self.seeds.selected(option).unwrap_or_else(|| {
            self.dom
                .element(option)
                .is_some_and(|e| e.has_attr("selected"))
        })
    }

    /// The selectedness of a new option (`new Option()`), without the
    /// dirtiness flag: the `selected` attribute decides again when it
    /// changes.
    pub(crate) fn script_set_option_initial_selected(&mut self, option: NodeId, selected: bool) {
        self.seeds.set_selected_clean(option, selected);
        self.controls_changed();
    }

    /// The `selected` attribute of `option` was added or removed: an
    /// option that is not dirty follows the attribute (HTML, the
    /// attribute change steps of `option`).
    pub(crate) fn script_option_selected_attribute_changed(&mut self, option: NodeId) {
        let was_clean = self.seeds.drop_clean_selected(option);
        let was_applied = self.seeds.take_clean_applied(option);
        if (was_clean || was_applied) && select_of_option(&self.dom, option).is_some() {
            // The option is in a select, which keeps its state over
            // rebuilds: the selectedness follows the attribute.
            let selected = self
                .dom
                .element(option)
                .is_some_and(|e| e.has_attr("selected"));
            self.script_set_option_selected(option, selected);
            self.seeds.mark_clean_applied(option);
            return;
        }
        self.controls_changed();
    }

    /// `option.selected = selected`: selects the option (and deselects the
    /// others of a select without `multiple`), then the selectedness
    /// setting algorithm runs ("ask for a reset").
    pub(crate) fn script_set_option_selected(&mut self, option: NodeId, selected: bool) {
        self.seeds.forget_clean_applied(option);
        let Some(select) = select_of_option(&self.dom, option) else {
            self.seeds.set_selected(option, selected);
            self.controls_changed();
            return;
        };
        let multiple = self
            .dom
            .element(select)
            .is_some_and(|e| e.has_attr("multiple"));
        self.write_options(select, |dom, options| {
            for o in options.iter_mut() {
                if o.node == option {
                    o.selected = selected;
                } else if selected && !multiple {
                    o.selected = false;
                }
            }
            // One selected option is a valid state; the other cases need
            // the algorithm (a scan of the options).
            if !selected || multiple {
                settle(dom, select, options, &|n| option_is_disabled(dom, n));
            }
        });
    }
}
