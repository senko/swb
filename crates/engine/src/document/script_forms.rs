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
use swb_web::{ControlWrite, RangeTextMode, SelectionRange};

use super::DocumentState;
use crate::edit::TextEdit;
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

    /// True if the selection API of HTML applies to the control: a text
    /// area, and an input of the type `text`, `search`, `tel`, `url` or
    /// `password`.
    /// <https://html.spec.whatwg.org/multipage/input.html#concept-input-apply>
    fn has_selection_api(ty: ControlType) -> bool {
        match ty {
            ControlType::TextArea => true,
            ControlType::Input(t) => matches!(
                t,
                InputType::Text
                    | InputType::Search
                    | InputType::Tel
                    | InputType::Url
                    | InputType::Password
            ),
            _ => false,
        }
    }

    /// The editing state of a text control as it is now, as a copy: the
    /// state if the control has one (a control in the document gets one
    /// first), else the value and the selection that a script wrote, else
    /// the default value. `None` for other elements.
    fn script_edit(&mut self, node: NodeId) -> Option<TextEdit> {
        let ty = self.script_type(node).filter(|t| t.is_text())?;
        self.script_sync_text(node, ty);
        if let Some(state) = self.forms.value().typed_state(node, ty) {
            return Some(state.edit.clone());
        }
        let mut edit = if ty == ControlType::TextArea {
            TextEdit::multiline()
        } else {
            TextEdit::new()
        };
        if let Some(value) = self.seeds.value(node) {
            edit.replace_text(value);
        } else {
            edit.replace_text(&crate::forms::default_text(&self.dom, node, ty));
            edit.text_start(false);
        }
        if let Some((start, end, backward)) = self.seeds.selection(node) {
            edit.select_range_utf16(start, end, backward);
        }
        Some(edit)
    }

    /// Stores the editing state of a text control after a script changed
    /// its selection, or its value too (`value_changed`: the dirty value
    /// flag is set).
    fn script_put_edit(&mut self, node: NodeId, edit: TextEdit, value_changed: bool) {
        let Some(ty) = self.script_type(node).filter(|t| t.is_text()) else {
            return;
        };
        if let Some(state) = self.forms.value_mut().typed_state_mut(node, ty) {
            state.edit = edit;
            state.dirty |= value_changed;
        } else {
            if value_changed {
                self.seeds.set_value(node, edit.text().to_owned());
            }
            let (start, end) = edit.selection_utf16();
            self.seeds
                .set_selection(node, (start, end, edit.is_backward()));
        }
        self.controls_changed();
    }

    /// Makes the state of a text control in the document current, so that
    /// a read or write of its selection finds it (a control without a
    /// state, because it was inserted since the last build, gets one).
    fn script_sync_text(&mut self, node: NodeId, ty: ControlType) {
        self.sync_type(node, ty);
        // A control without a state, and one that follows its attributes
        // (not dirty) and whose attributes may have changed, need a
        // rebuild; a dirty one keeps its text over rebuilds.
        let stale = self
            .forms
            .value()
            .typed_state(node, ty)
            .is_none_or(|state| !state.dirty);
        if stale
            && select_is_attached(&self.dom, node)
            && !self.forms.is_current(self.dom.generation())
        {
            self.ensure_forms();
        }
    }

    /// `selectionStart`, `selectionEnd` and `selectionDirection`.
    pub(crate) fn script_selection(&mut self, node: NodeId) -> Option<SelectionRange> {
        let ty = self
            .script_type(node)
            .filter(|t| Self::has_selection_api(*t))?;
        self.script_sync_text(node, ty);
        let range = |edit: &TextEdit| {
            let (start, end) = edit.selection_utf16();
            SelectionRange {
                start: u32::try_from(start).unwrap_or(u32::MAX),
                end: u32::try_from(end).unwrap_or(u32::MAX),
                backward: edit.is_backward(),
            }
        };
        if let Some(state) = self.forms.value().typed_state(node, ty) {
            return Some(range(&state.edit));
        }
        Some(range(&self.script_edit(node)?))
    }

    /// HTML "set the selection range".
    pub(crate) fn script_set_selection(
        &mut self,
        node: NodeId,
        start: u32,
        end: u32,
        backward: bool,
    ) -> bool {
        let Some(ty) = self
            .script_type(node)
            .filter(|t| Self::has_selection_api(*t))
        else {
            return false;
        };
        self.script_sync_text(node, ty);
        if let Some(state) = self.forms.value_mut().typed_state_mut(node, ty) {
            state
                .edit
                .select_range_utf16(start as usize, end as usize, backward);
            self.controls_changed();
            return true;
        }
        let Some(mut edit) = self.script_edit(node) else {
            return false;
        };
        edit.select_range_utf16(start as usize, end as usize, backward);
        self.script_put_edit(node, edit, false);
        true
    }

    /// `select()`: selects all the text of a text control.
    pub(crate) fn script_select_text(&mut self, node: NodeId) -> bool {
        let Some(mut edit) = self.script_edit(node) else {
            return false;
        };
        edit.select_range_utf16(0, usize::MAX, false);
        self.script_put_edit(node, edit, false);
        true
    }

    /// `setRangeText`: the HTML "set range text" steps after the checks.
    /// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#dom-textarea/input-setrangetext>
    pub(crate) fn script_set_range_text(
        &mut self,
        node: NodeId,
        replacement: &str,
        start: u32,
        end: u32,
        mode: RangeTextMode,
    ) -> ControlWrite {
        let Some(ty) = self.script_type(node).filter(|t| t.is_text()) else {
            return ControlWrite::NotControl;
        };
        let Some(mut edit) = self.script_edit(node) else {
            return ControlWrite::NotControl;
        };
        let replacement = if ty == ControlType::TextArea {
            replacement.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            replacement.replace(['\n', '\r'], "")
        };
        let end = (end as usize).min(edit.len_utf16());
        let start = (start as usize).min(end);
        let (old_start, old_end) = edit.selection_utf16();
        // The bytes that the control counts now.
        let counted = match self.forms.value().typed_state(node, ty) {
            Some(state) if state.dirty => state.edit.text().len(),
            Some(_) => 0,
            None => self.seeds.value(node).map_or(0, str::len),
        };
        edit.replace_range_utf16(start, end, &replacement);
        if self.control_bytes.saturating_sub(counted) + edit.text().len() > MAX_CONTROL_BYTES {
            log::warn!(
                "the values of form controls reached the limit of {MAX_CONTROL_BYTES} bytes; the task ends"
            );
            return ControlWrite::OverLimit;
        }
        let new_length = replacement.encode_utf16().count();
        let delta = new_length as i64 - (end - start) as i64;
        let adjust = |offset: usize| -> usize {
            if offset > end {
                (offset as i64 + delta).max(0) as usize
            } else if offset > start {
                start
            } else {
                offset
            }
        };
        let (new_start, new_end) = match mode {
            RangeTextMode::Select => (start, start + new_length),
            RangeTextMode::Start => (start, start),
            RangeTextMode::End => (start + new_length, start + new_length),
            RangeTextMode::Preserve => (adjust(old_start), adjust(old_end)),
        };
        edit.select_range_utf16(new_start, new_end, false);
        self.script_put_edit(node, edit, true);
        self.control_bytes = self.forms.value().dirty_value_bytes() + self.seeds.value_bytes();
        ControlWrite::Done
    }

    /// Rebuilds the states if the control `node` has a state of another
    /// type than the one the DOM says (its `type` attribute changed since
    /// the last build): the rebuild carries the value and the checkedness
    /// over. O(1) when the types agree.
    fn sync_type(&mut self, node: NodeId, ty: ControlType) {
        if self
            .forms
            .value()
            .get(node)
            .is_some_and(|state| state.ty != ty)
        {
            self.ensure_forms();
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
        self.sync_type(node, ty);
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
        self.sync_type(node, ty);
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

    /// The `type` attribute of an input changed (HTML "input type change").
    /// A control with a state gets a rebuild, which carries its value over
    /// sanitized for the new type. For one without a state (a new input,
    /// the states not built yet) the value stays a script's seed: the
    /// sanitized old value, if the sanitization changed what the control
    /// shows. A value that a script wrote is dropped if the new type shows
    /// the `value` attribute instead.
    pub(crate) fn script_input_type_changed(&mut self, node: NodeId, old_value: Option<String>) {
        let Some(ty) = self.script_type(node) else {
            return;
        };
        self.sync_type(node, ty);
        let had_seed = self.seeds.take_value(node).is_some();
        if ty.is_text()
            && let Some(old) = old_value
            && self.forms.value().typed_state(node, ty).is_none()
            && let Some(e) = self.dom.element(node)
        {
            let kept = crate::forms::sanitize_value(e, ty, &old);
            if had_seed || kept != crate::forms::default_text(&self.dom, node, ty) {
                self.seeds.set_value(node, kept);
            }
        }
        self.control_bytes = self.forms.value().dirty_value_bytes() + self.seeds.value_bytes();
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

    /// The checkedness of an `input`. The state of a radio button in the
    /// document is current first: the checked radio buttons of a group are
    /// settled when the states are built. An input whose type is not a
    /// checkbox or radio button has the checkedness that a script wrote
    /// (HTML: every input has the dirty checkedness flag), else the
    /// `checked` attribute.
    pub(crate) fn script_control_checked(&mut self, node: NodeId) -> Option<bool> {
        let ty = self
            .script_type(node)
            .filter(|t| matches!(t, ControlType::Input(_)))?;
        self.sync_type(node, ty);
        let radio_in_document =
            ty == ControlType::Input(InputType::Radio) && select_is_attached(&self.dom, node);
        if radio_in_document {
            self.ensure_forms();
        }
        if ty.is_checkable()
            && let Some(state) = self.forms.value().typed_state(node, ty)
            && (state.dirty || radio_in_document)
        {
            return Some(state.checked);
        }
        if let Some(checked) = self.seeds.checked(node) {
            return Some(checked);
        }
        Some(self.dom.element(node)?.has_attr("checked"))
    }

    /// Sets the checkedness of an `input`; checking a radio button
    /// unchecks the others of its group (the newest wins). False for the
    /// other elements.
    pub(crate) fn script_set_control_checked(&mut self, node: NodeId, checked: bool) -> bool {
        let Some(ty) = self
            .script_type(node)
            .filter(|t| matches!(t, ControlType::Input(_)))
        else {
            return false;
        };
        self.sync_type(node, ty);
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
        if ty.is_checkable() && !seed_radio && self.forms.value().typed_state(node, ty).is_some() {
            let forms = self.forms.value_mut();
            forms.set_checked(&self.dom, node, checked);
        } else {
            // A control outside the document is "inserted" later (stamp 0);
            // so is an input that becomes a radio button.
            let stamp = if ty.is_checkable() && select_is_attached(&self.dom, node) {
                self.forms.value_mut().next_stamp()
            } else {
                0
            };
            let owner = crate::forms::current_owner(&self.dom, node);
            self.seeds.set_checked(node, checked, stamp, owner);
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
            let before = options.clone();
            result = f.take().map(|f| f(&self.dom, &mut options));
            for (old, option) in before.iter().zip(&options) {
                if old.selected != option.selected || old.dirty != option.dirty {
                    self.seeds
                        .set_option(option.node, option.selected, option.dirty);
                }
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
                option.dirty |= option.selected;
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
    /// attribute change steps of `option`), and the select then asks for a
    /// reset (a selected option deselects the others of a select without
    /// `multiple`; an option that is not selected any more may give way to
    /// the first one, the selectedness setting algorithm).
    pub(crate) fn script_option_selected_attribute_changed(&mut self, option: NodeId) {
        let present = self
            .dom
            .element(option)
            .is_some_and(|e| e.has_attr("selected"));
        let Some(select) = select_of_option(&self.dom, option) else {
            // An option outside a select: a selectedness that scripts wrote
            // without the dirtiness flag gives way to the attribute.
            self.seeds.drop_clean_selected(option);
            self.controls_changed();
            return;
        };
        let multiple = self
            .dom
            .element(select)
            .is_some_and(|e| e.has_attr("multiple"));
        // If this attribute is the only change since the forms were built,
        // the write below keeps them valid: a loop of `selected` writes then
        // does not rebuild them for each write.
        let generation = self.dom.generation();
        if generation
            .checked_sub(1)
            .is_some_and(|g| self.forms.is_current(g))
        {
            self.forms.confirm(generation);
        }
        self.write_options(select, |dom, options| {
            let Some(index) = options.iter().position(|o| o.node == option) else {
                return;
            };
            if options[index].dirty {
                return;
            }
            options[index].selected = present;
            if present && !multiple {
                for (i, o) in options.iter_mut().enumerate() {
                    if i != index {
                        o.selected = false;
                    }
                }
            }
            settle(dom, select, options, &|n| option_is_disabled(dom, n));
        });
    }

    /// `option.selected = selected`: selects the option (and deselects the
    /// others of a select without `multiple`), then the selectedness
    /// setting algorithm runs ("ask for a reset"). The option is dirty.
    pub(crate) fn script_set_option_selected(&mut self, option: NodeId, selected: bool) {
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
                    o.dirty = true;
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
