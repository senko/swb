//! Form controls in a page: editing text fields and text areas, the keys
//! of focused controls, the activation of buttons, checkboxes, radio
//! buttons and labels, and form submission (ADR 0013).

use std::sync::Arc;

use swb_dom::{Document, NodeId, local_name};
use swb_layout::{FragmentRef, Point};
use swb_style::{PseudoKind, is_actually_disabled};

use super::{HistoryHandling, Page};
use crate::edit::TextEdit;
use crate::forms::{
    ButtonType, ControlType, FormContext, Forms, InputType, Submitter, blocking_fields,
    default_button, form_submission, labeled_control, option_label, value_offset,
};
use crate::hit_test::link_around;
use crate::input::{Key, Modifiers};
use crate::selection;

impl Page {
    // ----- Public API -----

    /// True if the focused element is a text field or a text area that
    /// can be edited (not read-only or disabled).
    pub fn has_editable_focus(&self) -> bool {
        self.editable_focus().is_some()
    }

    /// Inserts text at the caret of the focused text control (typed or
    /// composed text), replacing its selection. Line breaks become spaces
    /// in text fields. Returns false if no editable control has the focus.
    /// With scripts, `beforeinput` and `input` (`inputType` `insertText`)
    /// surround the edit.
    pub fn insert_text(&mut self, text: &str) -> bool {
        self.insert_text_as(text, "insertText")
    }

    /// Inserts pasted text (`inputType` `insertFromPaste`); otherwise as
    /// [`Page::insert_text`].
    pub fn paste_text(&mut self, text: &str) -> bool {
        self.insert_text_as(text, "insertFromPaste")
    }

    fn insert_text_as(&mut self, text: &str, input_type: &str) -> bool {
        let Some(node) = self.editable_focus() else {
            return false;
        };
        let Some(state) = self.forms_mut().and_then(|f| f.get_mut(node)) else {
            return false;
        };
        let text = if state.ty == ControlType::Input(InputType::Number) {
            number_characters(text)
        } else {
            text.to_owned()
        };
        let scripted = self.script_input();
        if scripted {
            if !self.fire_input(true, node, input_type, Some(&text)) {
                return true;
            }
            if self.editable_focus() != Some(node) {
                return true;
            }
            if let Some(doc) = self.doc.as_deref_mut() {
                doc.note_user_edit(node);
            }
        }
        let Some(state) = self.forms_mut().and_then(|f| f.get_mut(node)) else {
            return false;
        };
        if state.edit.insert(&text) {
            state.dirty = true;
            state.user_edited = true;
            self.control_changed();
            if scripted {
                self.fire_input(false, node, input_type, Some(&text));
            }
        }
        true
    }

    /// Cuts the selection of the focused editable text control: returns
    /// the selected text and deletes it. Empty without a selection (and
    /// for password fields, which cannot be copied). With scripts,
    /// `beforeinput` and `input` (`deleteByCut`) surround the deletion.
    pub fn cut_selection(&mut self) -> String {
        let text = self
            .doc
            .as_ref()
            .and_then(|d| d.control_selected_text())
            .unwrap_or_default();
        if text.is_empty() {
            return text;
        }
        let scripted = self.script_input();
        if let Some(node) = self.editable_focus() {
            if scripted {
                if !self.fire_input(true, node, "deleteByCut", None) {
                    return String::new();
                }
                if let Some(doc) = self.doc.as_deref_mut() {
                    doc.note_user_edit(node);
                }
            }
            if let Some(state) = self.forms_mut().and_then(|f| f.get_mut(node))
                && state.edit.delete_selection()
            {
                state.dirty = true;
                state.user_edited = true;
                self.control_changed();
                if scripted {
                    self.fire_input(false, node, "deleteByCut", None);
                }
            }
        }
        text
    }

    /// The value of a form control (as the `value` IDL attribute), of the
    /// first selected option of a select, or of an option. `None` for
    /// other elements.
    pub fn control_value(&self, node: NodeId) -> Option<String> {
        self.forms()?.value(self.document()?, node)
    }

    /// The checkedness of a checkbox or radio button, or the selectedness
    /// of an option. `None` for other elements.
    pub fn control_checked(&self, node: NodeId) -> Option<bool> {
        self.forms()?.checked(node)
    }

    // ----- Selection and focus -----

    /// The form controls of the current document.
    fn forms(&self) -> Option<&Forms> {
        Some(self.doc.as_ref()?.forms.value())
    }

    /// The form controls of the current document, for user input.
    fn forms_mut(&mut self) -> Option<&mut Forms> {
        Some(self.doc.as_deref_mut()?.forms.value_mut())
    }

    /// The document and its form controls, for user input.
    fn dom_and_forms(&mut self) -> Option<(&Document, &mut Forms)> {
        let doc = self.doc.as_deref_mut()?;
        Some((&doc.dom, doc.forms.value_mut()))
    }

    /// True if `node` is a text field or a text area.
    pub(super) fn is_text_control(&self, node: NodeId) -> bool {
        self.forms().is_some_and(|f| f.is_text_control(node))
    }

    /// The type of control `node`.
    fn control_type(&self, node: NodeId) -> Option<ControlType> {
        self.forms()?.control_type(node)
    }

    /// True if control `node` is disabled.
    fn is_disabled(&self, node: NodeId) -> bool {
        self.forms().is_some_and(|f| f.is_disabled(node))
    }

    /// The focused text control, if it can be edited.
    fn editable_focus(&self) -> Option<NodeId> {
        let node = self.focused_element()?;
        let e = self.document()?.element(node)?;
        self.forms()?.get(node)?.is_editable(e).then_some(node)
    }

    /// Called when the focus moved from `old` to the current focus. A text
    /// field that gets the focus starts with its text scrolled to the
    /// start (as an unfocused field shows it). Returns true if the layout
    /// changes (the caret moved between controls).
    pub(super) fn focus_changed(&mut self, old: Option<NodeId>) -> bool {
        let Some(doc) = self.doc.as_deref_mut() else {
            return false;
        };
        let new = doc.states.focus;
        let forms = doc.forms.value_mut();
        if let Some(old) = old.filter(|&o| Some(o) != new)
            && forms.note_blur(old)
        {
            doc.controls_stale = true;
        }
        let forms = doc.forms.value_mut();
        if let Some(state) = new.and_then(|n| forms.get_mut(n))
            && state.ty != ControlType::TextArea
        {
            state.scroll = Point::default();
        }
        let involves_control = [old, new]
            .into_iter()
            .flatten()
            .any(|n| forms.get(n).is_some());
        if involves_control {
            doc.invalidate_layout();
        }
        involves_control
    }

    /// Drops the layout of the current document.
    fn invalidate_layout(&mut self) {
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.invalidate_layout();
        }
    }

    /// Keyboard focus selects the whole text of a text field (not of a
    /// text area), as in Chromium.
    pub(super) fn select_field_text(&mut self, node: NodeId) {
        if let Some(state) = self.forms_mut().and_then(|f| f.get_mut(node))
            && let ControlType::Input(t) = state.ty
            && t.is_text()
        {
            state.edit.select_all();
            self.invalidate_layout();
        }
    }

    // ----- Pointer -----

    /// The offset in the value of text control `node` at a point in
    /// viewport coordinates.
    pub(super) fn control_offset_at(&mut self, node: NodeId, point: Point) -> usize {
        self.update_layout();
        let Some(doc) = self.doc.as_deref() else {
            return 0;
        };
        let document_point = Point::new(point.x + doc.scroll.x, point.y + doc.scroll.y);
        let shown = doc
            .fragments
            .as_ref()
            .and_then(|tree| {
                selection::control_offset_at(tree, doc.scroll_offsets(), node, document_point)
            })
            .unwrap_or(0) as usize;
        let Some(state) = doc.forms.value().get(node) else {
            return 0;
        };
        let password = state.ty == ControlType::Input(InputType::Password);
        value_offset(state.edit.text(), shown, password)
    }

    /// A press in text control `node` at `offset` in its value: places the
    /// caret, extends the selection with Shift, selects a word with a
    /// double click (all of a password) and everything with a triple
    /// click.
    pub(super) fn press_in_control(
        &mut self,
        node: NodeId,
        offset: usize,
        shift: bool,
        clicks: u32,
    ) {
        let before = self.selection_key(node);
        let Some(state) = self.forms_mut().and_then(|f| f.get_mut(node)) else {
            return;
        };
        let password = state.ty == ControlType::Input(InputType::Password);
        match clicks {
            2 if !password => state.edit.select_word_at(offset),
            n if n >= 2 => state.edit.select_all(),
            _ => state.edit.move_to(offset, shift),
        }
        self.invalidate_layout();
        self.selection_changed(node, before, false, false);
    }

    /// A drag in text control `node`: extends its selection to the point.
    pub(super) fn drag_in_control(&mut self, node: NodeId, point: Point) -> bool {
        let offset = self.control_offset_at(node, point);
        let before = self.selection_key(node);
        let Some(state) = self.forms_mut().and_then(|f| f.get_mut(node)) else {
            return false;
        };
        if state.edit.cursor() == offset {
            return false;
        }
        state.edit.move_to(offset, true);
        self.invalidate_layout();
        self.selection_changed(node, before, false, false);
        true
    }

    /// The element whose activation behavior a click on `node` (at
    /// `point`, in viewport coordinates) runs: the nearest inclusive
    /// ancestor that is a link, a button, a checkbox, a radio button, a
    /// label, or the summary of a `details` element (also its default
    /// summary, which has no element: the target is then the `details`
    /// element). A label or summary does not count if the click is on
    /// other interactive content inside it (a text field, a select): as in
    /// Chromium's `HTMLLabelElement`, the label then does nothing. A
    /// disabled control gets no click events, so nothing above it
    /// activates (as in Chromium).
    pub(super) fn activation_target(&self, node: NodeId, point: Point) -> Option<NodeId> {
        let doc = self.document()?;
        let mut in_interactive = false;
        for n in std::iter::once(node).chain(doc.ancestors(node)) {
            let Some(e) = doc.element(n).filter(|e| e.is_html()) else {
                continue;
            };
            match &**e.local_name() {
                "a" | "area" if e.has_attr("href") => return Some(n),
                "label" if !in_interactive => return Some(n),
                "summary" if !in_interactive && doc.details_of_summary(n).is_some() => {
                    return Some(n);
                }
                "details"
                    if !in_interactive
                        && doc.details_summary(n).is_none()
                        && self.in_default_summary(n, point) =>
                {
                    return Some(n);
                }
                _ => {}
            }
            match self.control_type(n) {
                Some(_) if self.is_disabled(n) => return None,
                Some(t) if t.is_button() || t.is_checkable() => return Some(n),
                Some(t) => in_interactive |= t != ControlType::Input(InputType::Hidden),
                None => in_interactive |= is_interactive(e),
            }
        }
        None
    }

    /// Runs the activation behavior of `target` for a click at `point`
    /// (viewport coordinates): follows a link, activates a control or the
    /// control of a label. As in Chromium, a button without an activation
    /// behavior (type `button`, no form owner) and a label without a
    /// control let the click go to a link around them; a label whose
    /// control is disabled handles the click and does nothing. Returns true
    /// if something happened.
    pub(super) fn activate(&mut self, target: NodeId, point: Point) -> bool {
        let Some(doc) = self.document() else {
            return false;
        };
        let Some(e) = doc.element(target) else {
            return false;
        };
        if e.is_html_named(&local_name!("summary")) {
            return doc
                .details_of_summary(target)
                .is_some_and(|details| self.toggle_details(details));
        }
        if e.is_html_named(&local_name!("details")) {
            return self.toggle_details(target);
        }
        if e.is_html_named(&local_name!("label")) {
            if let Some(control) = labeled_control(doc, target) {
                return self.activate_label(control);
            }
        } else if self.control_type(target).is_some() {
            if self.is_disabled(target) {
                // A disabled control gets no click events at all.
                return false;
            }
            if self.has_activation_behavior(target) {
                let scroll = self.scroll_position();
                let coordinate = self.element_box(target).map(|r| {
                    let x = point.x + scroll.x - r.x;
                    let y = point.y + scroll.y - r.y;
                    (x.floor().max(0.0) as i32, y.floor().max(0.0) as i32)
                });
                return self.activate_control(target, coordinate);
            }
        }
        self.follow_link_around(target)
    }

    /// True if `point` (viewport coordinates) is in the default summary of
    /// `details`, which has no summary element.
    fn in_default_summary(&self, details: NodeId, point: Point) -> bool {
        let Some(doc) = self.doc.as_deref() else {
            return false;
        };
        let Some(tree) = &doc.fragments else {
            return false;
        };
        let point = Point::new(point.x + doc.scroll.x, point.y + doc.scroll.y);
        let mut inside = false;
        tree.walk_painted(
            doc.scroll_offsets(),
            doc.scroll,
            |fragment, rect, matrix| {
                if let FragmentRef::Box(b) = fragment
                    && b.node == Some(details)
                    && b.pseudo == Some(PseudoKind::DetailsSummary)
                {
                    inside |= matrix.map_rect(&rect).contains(point);
                }
            },
        );
        inside
    }

    /// Toggles the `open` attribute of `details` and lays out the page
    /// again (the activation behavior of its summary). The focus leaves
    /// content that is no longer rendered. Returns true.
    /// <https://html.spec.whatwg.org/multipage/interactive-elements.html#the-summary-element>
    pub(super) fn toggle_details(&mut self, details: NodeId) -> bool {
        if self.document().and_then(|d| d.element(details)).is_none() {
            return false;
        }
        self.change_dom(|doc| {
            if !doc.remove_attr(details, "open") && doc.set_attr(details, "open", "").is_err() {
                log::warn!("cannot open the details element: the document is at its limit");
            }
        });
        if let (Some(doc), Some(focus)) = (self.document(), self.focused_element())
            && doc.in_closed_details_content(focus)
        {
            self.update_states(|s| s.focus = None);
        }
        // A selection that starts or ends in the hidden content goes away.
        let selection = self.doc.as_ref().and_then(|d| d.selection);
        if let (Some(doc), Some(selection)) = (self.document(), selection)
            && (doc.in_closed_details_content(selection.anchor.node)
                || doc.in_closed_details_content(selection.focus.node))
        {
            self.clear_selection();
        }
        true
    }

    /// Follows the nearest link that contains `node` (or is `node`).
    /// Returns true if a navigation started or the page scrolled.
    fn follow_link_around(&mut self, node: NodeId) -> bool {
        let Some(doc) = self.doc.as_deref() else {
            return false;
        };
        let link = link_around(doc.dom(), node, doc.base_url());
        link.is_some_and(|(_, link)| self.follow_link(link))
    }

    /// True if a click on control `node` does something: checkboxes and
    /// radio buttons, and submit and reset buttons with a form owner.
    fn has_activation_behavior(&self, node: NodeId) -> bool {
        let Some(forms) = self.forms() else {
            return false;
        };
        let Some(ty) = forms.control_type(node) else {
            return false;
        };
        let resets = matches!(
            ty,
            ControlType::Input(InputType::Reset) | ControlType::Button(ButtonType::Reset)
        );
        ty.is_checkable() || ((ty.is_submit_button() || resets) && forms.owner(node).is_some())
    }

    /// The activation of a label: focuses its control and clicks it. The
    /// click runs the activation behavior of the control (a button, a
    /// checkbox, a radio button); for a control without one, it goes on to
    /// a link around the control (as in Chromium, where the simulated click
    /// bubbles up from the control). A disabled control gets no click.
    fn activate_label(&mut self, control: NodeId) -> bool {
        if self.is_disabled(control) {
            return false;
        }
        let focused = self.focus(Some(control), false);
        if self.has_activation_behavior(control) {
            return self.activate_control(control, None) || focused;
        }
        self.follow_link_around(control) || focused
    }

    /// The activation behavior of a control: toggles a checkbox, checks a
    /// radio button, submits or resets the form. `coordinate` is the
    /// point of a click relative to the control (for image buttons).
    /// Returns true if something happened.
    fn activate_control(&mut self, node: NodeId, coordinate: Option<(i32, i32)>) -> bool {
        let Some(ty) = self.control_type(node) else {
            return false;
        };
        if self.is_disabled(node) {
            return false;
        }
        let owner = self.forms().and_then(|f| f.owner(node));
        match ty {
            ControlType::Input(InputType::Checkbox | InputType::Radio) => {
                let Some((doc, forms)) = self.dom_and_forms() else {
                    return false;
                };
                let checked = ty == ControlType::Input(InputType::Radio)
                    || !forms.checked(node).unwrap_or(false);
                let changed = forms.set_checked(doc, node, checked);
                if changed {
                    self.control_changed();
                }
                changed
            }
            t if t.is_submit_button() => owner
                .is_some_and(|form| self.submit_form(form, Submitter::Button { node, coordinate })),
            ControlType::Input(InputType::Reset) | ControlType::Button(ButtonType::Reset) => {
                let (Some(form), Some((doc, forms))) = (owner, self.dom_and_forms()) else {
                    return false;
                };
                let _ = forms.reset(doc, form);
                self.control_changed();
                true
            }
            _ => false,
        }
    }

    // ----- Keyboard -----

    /// Handles a key press for the focused form control. `None` if the
    /// focus is not on a control or the control does not use the key (Tab,
    /// Escape, the page scrolling keys); the page handles it then.
    pub(super) fn control_key(&mut self, key: &Key, modifiers: Modifiers) -> Option<bool> {
        let node = self.focused_element()?;
        let ty = self.control_type(node)?;
        if modifiers.alt {
            return None;
        }
        match ty {
            t if t.is_text() => return self.text_key(node, t, key, modifiers),
            ControlType::Select => return self.select_key(node, key, modifiers),
            ControlType::Input(InputType::Radio)
                if matches!(
                    key,
                    Key::ArrowDown | Key::ArrowRight | Key::ArrowUp | Key::ArrowLeft
                ) =>
            {
                let backward = matches!(key, Key::ArrowUp | Key::ArrowLeft);
                return Some(self.next_radio(node, backward));
            }
            // Space clicks checkboxes, radio buttons and buttons; Enter
            // clicks buttons and submits the form of a checkbox or radio
            // button.
            // With scripts, Space clicks when the key goes up (key_up).
            _ if key.is_char(' ') && (ty.is_checkable() || ty.is_button()) => {
                if self.script_input() {
                    self.input.space_down = Some(node);
                } else {
                    self.activate_control(node, None);
                }
            }
            _ if key == &Key::Enter && ty.is_checkable() => {
                self.implicit_submission(node);
            }
            _ if key == &Key::Enter && ty.is_button() => {
                if self.script_input() {
                    self.simulate_click(node);
                } else {
                    self.activate_control(node, None);
                }
            }
            _ => return None,
        }
        Some(true)
    }

    /// A key in a text field or text area: Enter submits a text field
    /// (implicit submission); the other keys edit (see [`edit_key`]).
    fn text_key(
        &mut self,
        node: NodeId,
        ty: ControlType,
        key: &Key,
        modifiers: Modifiers,
    ) -> Option<bool> {
        let scripted = self.script_input();
        let editable = self.editable_focus() == Some(node);
        if key == &Key::Enter && ty != ControlType::TextArea {
            // Chromium fires `beforeinput` for the line break of a single
            // line field; a listener can cancel the submission with it.
            if scripted && editable && !self.fire_input(true, node, "insertLineBreak", None) {
                return Some(true);
            }
            self.implicit_submission(node);
            return Some(true);
        }
        let field = FieldKind {
            multiline: ty == ControlType::TextArea,
            number: ty == ControlType::Input(InputType::Number),
            password: ty == ControlType::Input(InputType::Password),
            editable,
        };
        // The edit that the key makes: its `inputType` and `data`.
        let plan = if scripted {
            input_plan(field, key, modifiers)
        } else {
            None
        };
        if let Some((input_type, data)) = &plan {
            if !self.fire_input(true, node, input_type, data.as_deref()) {
                return Some(true);
            }
            // Listeners may have moved the focus or changed the page.
            if self.editable_focus() != Some(node) {
                return Some(true);
            }
            if let Some(doc) = self.doc.as_deref_mut() {
                doc.note_user_edit(node);
            }
        }
        let before = self.selection_key(node);
        let state = self.forms_mut()?.get_mut(node)?;
        let changed = edit_key(&mut state.edit, field, key, modifiers)?;
        if changed {
            state.dirty = true;
            state.user_edited = true;
            self.control_changed();
            if let Some((input_type, data)) = &plan {
                self.fire_input(false, node, input_type, data.as_deref());
            }
        } else {
            self.invalidate_layout();
        }
        // Ctrl+A fires `select` again although the selection stays.
        let select_all = (modifiers.ctrl || modifiers.meta) && key.is_char('a');
        self.selection_changed(node, before, select_all, changed);
        Some(true)
    }

    /// The selection of text control `node` as a key (byte offsets and the
    /// direction), for the selection events.
    fn selection_key(&self, node: NodeId) -> Option<(usize, usize, bool)> {
        let state = self.forms()?.get(node)?;
        let selection = state.edit.selection();
        Some((selection.start, selection.end, state.edit.is_backward()))
    }

    /// The user changed the selection of text control `node` (it was
    /// `before`): with scripts, `select` fires when the new selection is
    /// not empty and the text did not change, and `selectionchange` is
    /// queued. `force` counts an action that selects although the
    /// selection stays (Ctrl+A).
    fn selection_changed(
        &mut self,
        node: NodeId,
        before: Option<(usize, usize, bool)>,
        force: bool,
        text_changed: bool,
    ) {
        if !self.script_input() {
            return;
        }
        let after = self.selection_key(node);
        if after.is_none() || (before == after && !force && !text_changed) {
            return;
        }
        let select = !text_changed && after.is_some_and(|(start, end, _)| start != end);
        self.run_task(|s| s.user_selection(node, select));
    }

    /// A key in a focused drop-down select: the arrows, Home and End
    /// select another option; a character selects the next option whose
    /// label starts with it.
    fn select_key(&mut self, node: NodeId, key: &Key, modifiers: Modifiers) -> Option<bool> {
        if modifiers.ctrl || modifiers.meta {
            return None;
        }
        let doc = self.document()?;
        let state = self.forms()?.get(node)?;
        let enabled: Vec<bool> = state
            .options
            .iter()
            .map(|o| !is_actually_disabled(doc, o.node))
            .collect();
        let current = state.options.iter().position(|o| o.selected);
        let len = enabled.len();
        let is_enabled = |i: &usize| enabled.get(*i).copied().unwrap_or(false);
        let target = match key {
            Key::ArrowDown | Key::ArrowRight => {
                let start = current.map_or(0, |c| c + 1);
                (start..len).find(is_enabled)
            }
            Key::ArrowUp | Key::ArrowLeft => {
                let end = current.unwrap_or(len);
                (0..end).rev().find(is_enabled)
            }
            Key::Home => (0..len).find(is_enabled),
            Key::End => (0..len).rev().find(is_enabled),
            Key::Enter => None,
            Key::Character(c) if c == " " => None,
            Key::Character(c) => {
                // Type-ahead: the next option whose label starts with `c`.
                let c = c.to_lowercase();
                let start = current.map_or(0, |c| c + 1);
                (0..len).map(|k| (start + k) % len).find(|&i| {
                    is_enabled(&i)
                        && state.options.get(i).is_some_and(|o| {
                            option_label(doc, o.node).to_lowercase().starts_with(&c)
                        })
                })
            }
            _ => return None,
        };
        if let Some(index) = target
            && self
                .forms_mut()
                .is_some_and(|f| f.select_option(node, index))
        {
            self.control_changed();
            if self.script_input() {
                // `input` and `change` follow at once (measured).
                self.run_task(|s| s.select_changed(node));
            }
        }
        Some(true)
    }

    /// Checks and focuses the next (or previous) radio button of the group
    /// of `node` that is not disabled, wrapping around.
    fn next_radio(&mut self, node: NodeId, backward: bool) -> bool {
        let (Some(doc), Some(forms)) = (self.document(), self.forms()) else {
            return false;
        };
        let group: Vec<NodeId> = forms
            .radio_group(doc, node)
            .into_iter()
            .filter(|&n| n == node || !forms.is_disabled(n))
            .collect();
        let Some(index) = group.iter().position(|&n| n == node) else {
            return false;
        };
        let len = group.len();
        let next = if backward {
            group[(index + len - 1) % len]
        } else {
            group[(index + 1) % len]
        };
        if next != node {
            self.focus(Some(next), true);
        }
        if self.script_input() {
            // The arrow clicks the radio button as a key does.
            self.simulate_click(next);
        } else {
            self.activate_control(next, None);
        }
        true
    }

    // ----- Submission -----

    /// Implicit submission (Enter in a field): activates the form's
    /// default button, or submits a form without a submit button that has
    /// at most one field that blocks implicit submission. Returns true if
    /// a submission started.
    /// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#implicit-submission>
    fn implicit_submission(&mut self, control: NodeId) -> bool {
        let scripted = self.script_input();
        if scripted {
            // The line break commits the edit (`change`) before the
            // submission (measured).
            self.run_task(|s| s.commit_change(control));
        }
        let (Some(doc), Some(forms)) = (self.document(), self.forms()) else {
            return false;
        };
        let Some(form) = forms.owner(control) else {
            return false;
        };
        let button = default_button(doc, forms, form);
        let blocking = blocking_fields(doc, forms, form);
        match button {
            Some(button) if scripted => self.simulate_click(button),
            Some(button) => self.activate_control(button, None),
            None if blocking > 1 => false,
            None if scripted => {
                self.run_task(|s| s.implicit_submit(form));
                true
            }
            None => self.submit_form(form, Submitter::Form),
        }
    }

    /// Submits `form`: validates it (unless `novalidate`), builds the
    /// request and navigates. Returns true if a navigation started.
    fn submit_form(&mut self, form: NodeId, submitter: Submitter) -> bool {
        let Some(state) = self.doc.as_deref() else {
            return false;
        };
        let (doc, forms) = (state.dom(), state.forms.value());
        let novalidate = doc.element(form).is_some_and(|e| e.has_attr("novalidate"))
            || submitter
                .node()
                .and_then(|n| doc.element(n))
                .is_some_and(|e| e.has_attr("formnovalidate"));
        if !novalidate
            && let Some(&first) = forms.invalid_controls(doc, form, &state.validity).first()
        {
            log::info!("form not submitted: a control fails constraint validation");
            if let Some(doc) = self.doc.as_deref_mut()
                && doc.forms.value_mut().note_failed_submission(form)
            {
                doc.controls_stale = true;
            }
            self.focus(Some(first), true);
            return false;
        }
        let cx = FormContext {
            doc,
            forms,
            base_url: state.base_url(),
            document_url: state.url(),
            encoding: state.encoding,
        };
        let Some(submission) = form_submission(&cx, form, submitter) else {
            return false;
        };
        if !self.may_load_subresource(&submission.url) {
            log::warn!("not allowed to submit a form to {}", submission.url);
            return false;
        }
        let method = if submission.post.is_some() {
            "POST"
        } else {
            "GET"
        };
        log::info!("submitting a form: {method} {}", submission.url);
        let initiator = self.document_origin();
        self.start_navigation(
            submission.url,
            submission.post,
            HistoryHandling::Push,
            None,
            initiator,
        );
        true
    }

    /// Updates the style states and the layout after a control's value,
    /// checkedness or selected option changed.
    fn control_changed(&mut self) {
        if let Some(doc) = self.doc.as_deref() {
            let states = Arc::new(doc.forms.value().element_states(
                doc.dom(),
                &doc.indeterminate,
                &doc.validity,
            ));
            self.update_states(|s| s.controls = states);
        }
        self.invalidate_layout();
    }
}

/// What kind of text control a key edits.
#[derive(Clone, Copy)]
struct FieldKind {
    multiline: bool,
    number: bool,
    password: bool,
    editable: bool,
}

impl FieldKind {
    /// Moves to the previous word; in a password field, to the start (the
    /// words of a password are not revealed).
    fn word_left(self, edit: &mut TextEdit, select: bool) {
        if self.password {
            edit.text_start(select);
        } else {
            edit.word_left(select);
        }
    }

    /// Moves to the next word; in a password field, to the end.
    fn word_right(self, edit: &mut TextEdit, select: bool) {
        if self.password {
            edit.text_end(select);
        } else {
            edit.word_right(select);
        }
    }
}

/// Applies an editing key to the text of a field: characters type,
/// Backspace and Delete delete (with Ctrl, a word), the arrows, Home and
/// End move the caret (see [`move_caret`]), Ctrl+A selects all, Enter adds
/// a line in a text area. A field that cannot be edited only moves the
/// caret. Returns whether the text changed, or `None` if the field does
/// not use the key.
fn edit_key(
    edit: &mut TextEdit,
    field: FieldKind,
    key: &Key,
    modifiers: Modifiers,
) -> Option<bool> {
    let ctrl = modifiers.ctrl || modifiers.meta;
    match key {
        Key::Character(c) if !ctrl => {
            if field.editable {
                let text = if field.number {
                    number_characters(c)
                } else {
                    c.clone()
                };
                return Some(edit.insert(&text));
            }
        }
        key if ctrl && key.is_char('a') => edit.select_all(),
        Key::Backspace | Key::Delete if field.editable => {
            return Some(delete(edit, field, key == &Key::Backspace, ctrl));
        }
        Key::Enter if field.multiline && field.editable => return Some(edit.insert("\n")),
        // A field that cannot be edited uses these keys without a change.
        Key::Backspace | Key::Delete | Key::Enter => {}
        key => {
            if !move_caret(edit, field, key, ctrl, modifiers.shift) {
                return None;
            }
        }
    }
    Some(false)
}

/// The `inputType` and the `data` of the `beforeinput` and `input` events
/// of the edit that a key makes in a text field (UI Events, Input Events 2,
/// "inputType"), as Chromium fires them (measured): a character inserts
/// text, Backspace and Delete delete (a word with Ctrl), Enter in a text
/// area breaks the line. `None` for keys that edit nothing and for fields
/// that cannot be edited.
fn input_plan(
    field: FieldKind,
    key: &Key,
    modifiers: Modifiers,
) -> Option<(&'static str, Option<String>)> {
    if !field.editable {
        return None;
    }
    let ctrl = modifiers.ctrl || modifiers.meta;
    match key {
        Key::Character(c) if !ctrl => {
            let text = if field.number {
                number_characters(c)
            } else {
                c.clone()
            };
            (!text.is_empty()).then_some(("insertText", Some(text)))
        }
        Key::Backspace if ctrl => Some(("deleteWordBackward", None)),
        Key::Backspace => Some(("deleteContentBackward", None)),
        Key::Delete if ctrl => Some(("deleteWordForward", None)),
        Key::Delete => Some(("deleteContentForward", None)),
        Key::Enter if field.multiline => Some(("insertLineBreak", None)),
        _ => None,
    }
}

/// Backspace (`backspace` is true) or Delete: deletes the selection, else
/// the previous or next character (with Ctrl, word). Returns whether the
/// text changed.
fn delete(edit: &mut TextEdit, field: FieldKind, backspace: bool, ctrl: bool) -> bool {
    if ctrl && edit.selection().is_empty() {
        if backspace {
            field.word_left(edit, true);
        } else {
            field.word_right(edit, true);
        }
    }
    if backspace {
        edit.backspace()
    } else {
        edit.delete()
    }
}

/// Moves the caret for the arrows, Home and End: with Shift, the selection
/// grows; with Ctrl, the left and right arrows move by words and Home and
/// End go to the ends of the text. Up and down move by lines in a text
/// area and to the ends in a text field. Returns false for other keys.
fn move_caret(edit: &mut TextEdit, field: FieldKind, key: &Key, ctrl: bool, shift: bool) -> bool {
    match key {
        Key::ArrowLeft if ctrl => field.word_left(edit, shift),
        Key::ArrowLeft => edit.left(shift),
        Key::ArrowRight if ctrl => field.word_right(edit, shift),
        Key::ArrowRight => edit.right(shift),
        Key::Home if ctrl => edit.text_start(shift),
        Key::Home => edit.home(shift),
        Key::End if ctrl => edit.text_end(shift),
        Key::End => edit.end(shift),
        Key::ArrowUp if field.multiline => edit.line_up(shift),
        Key::ArrowUp => edit.text_start(shift),
        Key::ArrowDown if field.multiline => edit.line_down(shift),
        Key::ArrowDown => edit.text_end(shift),
        _ => return false,
    }
    true
}

/// True for interactive content other than links and form controls: a
/// click on it inside a label does not activate the label's control.
/// `details` (and so its summary), `embed`, `iframe`, media with controls
/// and images with `usemap` are interactive. Deviation: the specification
/// no longer lists `object` with `usemap`; Chromium (measured) still
/// treats it as interactive, and so does swb.
/// <https://html.spec.whatwg.org/multipage/dom.html#interactive-content>
fn is_interactive(e: &swb_dom::ElementData) -> bool {
    match &**e.local_name() {
        "details" | "embed" | "iframe" => true,
        "audio" | "video" => e.has_attr("controls"),
        "img" | "object" => e.has_attr("usemap"),
        _ => false,
    }
}

/// The characters of `text` that a number field accepts (as Chromium):
/// digits, signs, the decimal point and exponents.
fn number_characters(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD: FieldKind = FieldKind {
        multiline: false,
        number: false,
        password: false,
        editable: true,
    };

    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };

    #[test]
    fn password_word_steps_go_to_the_ends() {
        let mut edit = TextEdit::new();
        edit.insert("ab cd");
        let password = FieldKind {
            password: true,
            ..FIELD
        };
        assert_eq!(
            edit_key(&mut edit, password, &Key::Backspace, CTRL),
            Some(true)
        );
        assert_eq!(edit.text(), "");
        edit.insert("ab cd");
        edit_key(&mut edit, FIELD, &Key::Backspace, CTRL);
        assert_eq!(edit.text(), "ab ");
    }

    #[test]
    fn read_only_fields_move_but_do_not_change() {
        let mut edit = TextEdit::new();
        edit.insert("abc");
        let read_only = FieldKind {
            editable: false,
            ..FIELD
        };
        let typed = Key::Character("x".to_owned());
        assert_eq!(
            edit_key(&mut edit, read_only, &typed, Modifiers::NONE),
            Some(false)
        );
        assert_eq!(
            edit_key(&mut edit, read_only, &Key::Backspace, Modifiers::NONE),
            Some(false)
        );
        edit_key(&mut edit, read_only, &Key::Home, Modifiers::NONE);
        assert_eq!((edit.text(), edit.cursor()), ("abc", 0));
        assert_eq!(edit_key(&mut edit, FIELD, &Key::Tab, Modifiers::NONE), None);
        let number = FieldKind {
            number: true,
            ..FIELD
        };
        edit_key(
            &mut edit,
            number,
            &Key::Character("x".to_owned()),
            Modifiers::NONE,
        );
        assert_eq!(edit.text(), "abc");
    }
}
