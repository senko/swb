//! Form controls: their state (value, checkedness, selected options), the
//! parts of the HTML forms model that the page needs (form owners, radio
//! button groups, labels, reset, validity), and what style needs from
//! them. What layout needs is in `view.rs`, submission in `submit.rs`, the
//! encodings of form data in `encode.rs`.
//!
//! The state is not in the DOM: the `value`, `checked` and `selected`
//! attributes are only the defaults. [`Forms`] holds the state of every
//! control of a document, keyed by element. swb runs no scripts, so the
//! DOM does not change after parsing: form owners and the other
//! attribute-derived facts are computed once.
//!
//! <https://html.spec.whatwg.org/multipage/forms.html>. Deliberate
//! simplifications: date, time, color and range inputs are text fields;
//! file inputs are buttons without a file; list boxes (`<select multiple>`
//! or `size` > 1) look like drop-downs; image buttons show their `alt`
//! text instead of the image.

mod encode;
mod select;
mod submit;
mod values;
mod view;

use std::collections::{HashMap, HashSet};

use swb_css::ElementState;
use swb_dom::{Document, ElementData, NodeId, local_name, parse_non_negative_u32};
use swb_layout::Point;
use swb_style::DisabledElements;

use crate::edit::TextEdit;

pub(crate) use select::{option_label, option_value};
pub(crate) use submit::{FormContext, Submitter, blocking_fields, default_button, form_submission};
pub(crate) use view::{LayoutControls, display_offset, value_offset};

use select::{has_placeholder_label_option, select_options};
use values::sanitize;

/// The `type` of an `input` element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputType {
    Text,
    Search,
    Tel,
    Url,
    Email,
    Password,
    Number,
    Hidden,
    Checkbox,
    Radio,
    Submit,
    Reset,
    Button,
    Image,
    File,
    /// Shown as a text field.
    Range,
    /// Shown as a text field.
    Color,
    /// A date or time type, shown as a text field.
    Date(DateType),
}

/// The date and time types of `input`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DateType {
    Date,
    Month,
    Week,
    Time,
    DateTimeLocal,
}

impl InputType {
    /// The type of an `input` element. A missing or unknown `type` is
    /// `text`. The value is not trimmed (`type=" checkbox"` is a text
    /// field), as in the specification and Chromium.
    pub(crate) fn of(e: &ElementData) -> InputType {
        let Some(t) = e.attr("type") else {
            return InputType::Text;
        };
        match t.to_ascii_lowercase().as_str() {
            "search" => InputType::Search,
            "tel" => InputType::Tel,
            "url" => InputType::Url,
            "email" => InputType::Email,
            "password" => InputType::Password,
            "number" => InputType::Number,
            "hidden" => InputType::Hidden,
            "checkbox" => InputType::Checkbox,
            "radio" => InputType::Radio,
            "submit" => InputType::Submit,
            "reset" => InputType::Reset,
            "button" => InputType::Button,
            "image" => InputType::Image,
            "file" => InputType::File,
            "range" => InputType::Range,
            "color" => InputType::Color,
            "date" => InputType::Date(DateType::Date),
            "month" => InputType::Date(DateType::Month),
            "week" => InputType::Date(DateType::Week),
            "time" => InputType::Date(DateType::Time),
            "datetime-local" => InputType::Date(DateType::DateTimeLocal),
            _ => InputType::Text,
        }
    }

    /// True for the types that swb shows as a text field.
    pub(crate) fn is_text(self) -> bool {
        matches!(
            self,
            InputType::Text
                | InputType::Search
                | InputType::Tel
                | InputType::Url
                | InputType::Email
                | InputType::Password
                | InputType::Number
                | InputType::Range
                | InputType::Color
                | InputType::Date(_)
        )
    }

    /// True for the types whose fields block implicit submission when a
    /// form has more than one of them.
    /// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#field-that-blocks-implicit-submission>
    pub(crate) fn blocks_implicit_submission(self) -> bool {
        self.is_text() && !matches!(self, InputType::Range | InputType::Color)
    }

    /// True for the types to which `maxlength` applies (and `size`, which
    /// sets the width).
    /// <https://html.spec.whatwg.org/multipage/input.html#do-not-apply>
    pub(crate) fn has_size(self) -> bool {
        matches!(
            self,
            InputType::Text
                | InputType::Search
                | InputType::Url
                | InputType::Tel
                | InputType::Email
                | InputType::Password
        )
    }
}

/// The `type` of a `<button>` element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonType {
    Submit,
    Reset,
    Button,
}

/// What kind of form control an element is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlType {
    Input(InputType),
    TextArea,
    Select,
    Button(ButtonType),
}

impl ControlType {
    /// The control type of an element, if it is a form control.
    pub(crate) fn of(e: &ElementData) -> Option<ControlType> {
        if !e.is_html() {
            return None;
        }
        Some(match &**e.local_name() {
            "input" => ControlType::Input(InputType::of(e)),
            "textarea" => ControlType::TextArea,
            "select" => ControlType::Select,
            "button" => ControlType::Button(
                match e.attr("type").map(str::to_ascii_lowercase).as_deref() {
                    Some("reset") => ButtonType::Reset,
                    Some("button") => ButtonType::Button,
                    _ => ButtonType::Submit,
                },
            ),
            _ => return None,
        })
    }

    /// True for text fields and text areas.
    pub(crate) fn is_text(self) -> bool {
        match self {
            ControlType::Input(t) => t.is_text(),
            ControlType::TextArea => true,
            _ => false,
        }
    }

    /// True for submit buttons: `input` of type submit or image, and
    /// `<button>` of type submit.
    pub(crate) fn is_submit_button(self) -> bool {
        matches!(
            self,
            ControlType::Input(InputType::Submit | InputType::Image)
                | ControlType::Button(ButtonType::Submit)
        )
    }

    /// True for buttons of any kind.
    pub(crate) fn is_button(self) -> bool {
        matches!(
            self,
            ControlType::Input(
                InputType::Submit | InputType::Reset | InputType::Button | InputType::Image
            ) | ControlType::Button(_)
        )
    }

    /// True for checkboxes and radio buttons.
    pub(crate) fn is_checkable(self) -> bool {
        matches!(
            self,
            ControlType::Input(InputType::Checkbox | InputType::Radio)
        )
    }

    /// True for the controls to which `readonly` applies.
    /// <https://html.spec.whatwg.org/multipage/input.html#do-not-apply>
    fn has_readonly(self) -> bool {
        match self {
            ControlType::Input(t) => {
                t.has_size() || matches!(t, InputType::Number | InputType::Date(_))
            }
            other => other == ControlType::TextArea,
        }
    }

    /// True for the controls to which `required` applies.
    fn has_required(self) -> bool {
        match self {
            ControlType::Input(t) => {
                self.has_readonly()
                    || matches!(t, InputType::Checkbox | InputType::Radio | InputType::File)
            }
            other => matches!(other, ControlType::Select | ControlType::TextArea),
        }
    }
}

/// The selectedness of one option of a select.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OptionState {
    pub(crate) node: NodeId,
    pub(crate) selected: bool,
}

/// The state of one form control.
#[derive(Clone, Debug)]
pub(crate) struct ControlState {
    pub(crate) ty: ControlType,
    /// The value of text fields and text areas, with the caret and the
    /// selection. Other controls do not use it.
    pub(crate) edit: TextEdit,
    pub(crate) checked: bool,
    /// The scroll offset of the text, from the last layout.
    pub(crate) scroll: Point,
    /// For a select: its options in tree order.
    pub(crate) options: Vec<OptionState>,
    /// The form owner.
    pub(crate) owner: Option<NodeId>,
    /// True if the control is disabled.
    pub(crate) disabled: bool,
    /// True if the control is barred from constraint validation
    /// (disabled, read-only, hidden, in a `datalist`, or a button that
    /// does not submit).
    barred: bool,
}

impl ControlState {
    /// True for a text field or a text area that can be edited: not
    /// disabled and without a `readonly` attribute. `e` is the control's
    /// element.
    pub(crate) fn is_editable(&self, e: &ElementData) -> bool {
        self.ty.is_text() && !self.disabled && !e.has_attr("readonly")
    }
}

/// The states of all form controls of a document.
#[derive(Debug, Default)]
pub(crate) struct Forms {
    controls: HashMap<NodeId, ControlState>,
    /// The select of each option.
    option_select: HashMap<NodeId, NodeId>,
}

impl Forms {
    /// The controls of `doc` with their default state.
    pub(crate) fn new(doc: &Document) -> Forms {
        let ids = doc.element_ids();
        let disabled_elements = DisabledElements::new(doc);
        let mut forms = Forms::default();
        let mut radios = Vec::new();
        for node in doc.descendants(NodeId::DOCUMENT) {
            let Some(e) = doc.element(node) else {
                continue;
            };
            let Some(ty) = ControlType::of(e) else {
                continue;
            };
            let disabled = disabled_elements.contains(node);
            let barred = disabled
                || (ty.is_button() && !ty.is_submit_button())
                || ty == ControlType::Input(InputType::Hidden)
                || (ty.has_readonly() && e.has_attr("readonly"))
                || in_datalist(doc, node);
            let state = ControlState {
                owner: form_owner(doc, node, &ids),
                disabled,
                barred,
                ..default_state(doc, node, ty, &disabled_elements)
            };
            if ty == ControlType::Input(InputType::Radio) && state.checked {
                radios.push(node);
            }
            for option in &state.options {
                forms.option_select.insert(option.node, node);
            }
            forms.controls.insert(node, state);
        }
        forms.keep_last_checked(doc, &radios);
        forms
    }

    /// Unchecks every radio button in `checked` (checked radio buttons in
    /// tree order) except the last one of each group: setting a radio
    /// button's checkedness unchecks the others of its group.
    fn keep_last_checked(&mut self, doc: &Document, checked: &[NodeId]) {
        let mut last: HashMap<(Option<NodeId>, &str), NodeId> = HashMap::new();
        for &node in checked {
            if let Some(key) = self.radio_key(doc, node) {
                last.insert(key, node);
            }
        }
        let unchecked: Vec<NodeId> = checked
            .iter()
            .copied()
            .filter(|&n| {
                self.radio_key(doc, n)
                    .is_some_and(|key| last.get(&key) != Some(&n))
            })
            .collect();
        for node in unchecked {
            if let Some(state) = self.get_mut(node) {
                state.checked = false;
            }
        }
    }

    /// The state of control `node`.
    pub(crate) fn get(&self, node: NodeId) -> Option<&ControlState> {
        self.controls.get(&node)
    }

    /// The state of control `node`, for changing it.
    pub(crate) fn get_mut(&mut self, node: NodeId) -> Option<&mut ControlState> {
        self.controls.get_mut(&node)
    }

    /// The type of control `node`.
    pub(crate) fn control_type(&self, node: NodeId) -> Option<ControlType> {
        self.get(node).map(|s| s.ty)
    }

    /// True if `node` is a text field or a text area.
    pub(crate) fn is_text_control(&self, node: NodeId) -> bool {
        self.control_type(node).is_some_and(ControlType::is_text)
    }

    /// The form owner of control `node`.
    pub(crate) fn owner(&self, node: NodeId) -> Option<NodeId> {
        self.get(node)?.owner
    }

    /// True if control `node` is disabled.
    pub(crate) fn is_disabled(&self, node: NodeId) -> bool {
        self.get(node).is_some_and(|s| s.disabled)
    }

    /// The value of a control as the `value` IDL attribute returns it
    /// (<https://html.spec.whatwg.org/multipage/input.html#dom-input-value>),
    /// the value of the first selected option of a select, or the value of
    /// an option.
    pub(crate) fn value(&self, doc: &Document, node: NodeId) -> Option<String> {
        if self.option_select.contains_key(&node) {
            return Some(option_value(doc, node));
        }
        let state = self.get(node)?;
        let e = doc.element(node)?;
        Some(match state.ty {
            ControlType::Input(
                t @ (InputType::Number | InputType::Range | InputType::Color | InputType::Date(_)),
            ) => sanitize(e, t, state.edit.text()),
            ControlType::Input(InputType::Checkbox | InputType::Radio) => {
                e.attr("value").unwrap_or("on").to_owned()
            }
            ControlType::Input(InputType::File) => String::new(),
            ControlType::Input(t) if !t.is_text() => e.attr("value").unwrap_or("").to_owned(),
            ControlType::Button(_) => e.attr("value").unwrap_or("").to_owned(),
            ControlType::Select => state
                .options
                .iter()
                .find(|o| o.selected)
                .map(|o| option_value(doc, o.node))
                .unwrap_or_default(),
            _ => state.edit.text().to_owned(),
        })
    }

    /// The checkedness of a checkbox or radio button, or the selectedness
    /// of an option.
    pub(crate) fn checked(&self, node: NodeId) -> Option<bool> {
        if let Some(select) = self.option_select.get(&node) {
            return self
                .get(*select)?
                .options
                .iter()
                .find(|o| o.node == node)
                .map(|o| o.selected);
        }
        let state = self.get(node)?;
        state.ty.is_checkable().then_some(state.checked)
    }

    /// Sets the checkedness of a checkbox or radio button (as the user
    /// does). Checking a radio button unchecks the others of its group.
    /// Returns true if anything changed.
    pub(crate) fn set_checked(&mut self, doc: &Document, node: NodeId, checked: bool) -> bool {
        let Some(state) = self.get_mut(node) else {
            return false;
        };
        let changed = state.checked != checked;
        state.checked = checked;
        let is_radio = state.ty == ControlType::Input(InputType::Radio);
        if !(checked && is_radio) {
            return changed;
        }
        let mut changed = changed;
        for other in self.radio_group(doc, node) {
            if other != node
                && let Some(state) = self.get_mut(other)
            {
                changed |= state.checked;
                state.checked = false;
            }
        }
        changed
    }

    /// Selects option `index` of a drop-down select (as the user does).
    /// Returns true if the selection changed.
    pub(crate) fn select_option(&mut self, select: NodeId, index: usize) -> bool {
        let Some(state) = self.get_mut(select) else {
            return false;
        };
        if state.options.get(index).is_none_or(|o| o.selected) {
            return false;
        }
        for (i, option) in state.options.iter_mut().enumerate() {
            option.selected = i == index;
        }
        true
    }

    /// Resets the controls whose form owner is `form` to their defaults.
    /// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#concept-form-reset>
    pub(crate) fn reset(&mut self, doc: &Document, form: NodeId) {
        let nodes: Vec<NodeId> = doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| self.owner(n) == Some(form))
            .collect();
        let disabled_elements = DisabledElements::new(doc);
        let mut checked_radios = Vec::new();
        for node in nodes {
            let Some(old) = self.get(node) else {
                continue;
            };
            let mut state = ControlState {
                scroll: old.scroll,
                owner: old.owner,
                disabled: old.disabled,
                barred: old.barred,
                ..default_state(doc, node, old.ty, &disabled_elements)
            };
            // After a reset, Chromium (measured) types at the end of a text
            // field and at the start of a text area.
            if state.ty.is_text() && state.ty != ControlType::TextArea {
                state.edit.text_end(false);
            }
            if state.ty == ControlType::Input(InputType::Radio) && state.checked {
                checked_radios.push(node);
            }
            self.controls.insert(node, state);
        }
        self.keep_last_checked(doc, &checked_radios);
    }

    /// The states that style needs: [`swb_style::CONTROL_STATES`] of every
    /// control and option. Controls that are barred from constraint
    /// validation match neither `:valid` nor `:invalid`.
    pub(crate) fn element_states(&self, doc: &Document) -> HashMap<NodeId, ElementState> {
        let mut out = HashMap::with_capacity(self.controls.len());
        let groups = self.radio_groups(doc);
        for (&node, state) in &self.controls {
            let Some(e) = doc.element(node) else {
                continue;
            };
            let mut flags = ElementState::empty();
            if state.checked && state.ty.is_checkable() {
                flags |= ElementState::CHECKED;
            }
            for option in &state.options {
                let selected = if option.selected {
                    ElementState::CHECKED
                } else {
                    ElementState::empty()
                };
                out.insert(option.node, selected);
            }
            if state.ty.is_text()
                && state.edit.text().is_empty()
                && e.attr("placeholder").is_some_and(|p| !p.is_empty())
            {
                flags |= ElementState::PLACEHOLDER_SHOWN;
            }
            if !state.barred {
                flags |= if self.is_missing(doc, node, &groups) {
                    ElementState::INVALID
                } else {
                    ElementState::VALID
                };
            }
            out.insert(node, flags);
        }
        out
    }

    /// The controls whose form owner is `form` and that fail constraint
    /// validation, in tree order. swb checks only the `required`
    /// constraint.
    /// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#statically-validate-the-constraints>
    pub(crate) fn invalid_controls(&self, doc: &Document, form: NodeId) -> Vec<NodeId> {
        let groups = self.radio_groups(doc);
        doc.descendants(NodeId::DOCUMENT)
            .filter(|&n| {
                self.owner(n) == Some(form)
                    && self.get(n).is_some_and(|s| !s.barred)
                    && self.is_missing(doc, n, &groups)
            })
            .collect()
    }

    /// The radio button groups that have a checked radio button, and the
    /// groups that have a `required` radio button.
    fn radio_groups<'a>(&self, doc: &'a Document) -> RadioGroups<'a> {
        let mut groups = RadioGroups::default();
        for (&node, state) in &self.controls {
            if state.ty != ControlType::Input(InputType::Radio) {
                continue;
            }
            let Some(key) = self.radio_key(doc, node) else {
                continue;
            };
            if state.checked {
                groups.checked.insert(key);
            }
            if doc.element(node).is_some_and(|e| e.has_attr("required")) {
                groups.required.insert(key);
            }
        }
        groups
    }

    /// True if a `required` control has no value
    /// (<https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#suffering-from-being-missing>).
    /// A radio button is missing if a button of its group is `required`
    /// and none is checked. `required` applies to the types that the
    /// specification lists.
    fn is_missing(&self, doc: &Document, node: NodeId, groups: &RadioGroups<'_>) -> bool {
        let (Some(state), Some(e)) = (self.get(node), doc.element(node)) else {
            return false;
        };
        if state.ty == ControlType::Input(InputType::Radio) {
            return match self.radio_key(doc, node) {
                Some(key) => groups.required.contains(&key) && !groups.checked.contains(&key),
                None => e.has_attr("required") && !state.checked,
            };
        }
        if !e.has_attr("required") || !state.ty.has_required() {
            return false;
        }
        match state.ty {
            ControlType::Input(InputType::Checkbox) => !state.checked,
            ty if ty.is_text() => state.edit.text().is_empty(),
            ControlType::Select => {
                let selected = state.options.iter().position(|o| o.selected);
                selected.is_none()
                    || (selected == Some(0) && has_placeholder_label_option(doc, node, state))
            }
            _ => false,
        }
    }

    /// The key of the radio button group of radio button `node`: its form
    /// owner and its name. `None` without a name (the button is alone in
    /// its group).
    fn radio_key<'a>(&self, doc: &'a Document, node: NodeId) -> Option<(Option<NodeId>, &'a str)> {
        let name = doc
            .element(node)
            .and_then(|e| e.attr("name"))
            .filter(|n| !n.is_empty())?;
        Some((self.owner(node), name))
    }

    /// The radio button group of radio button `node`: the radio buttons
    /// with the same form owner and the same non-empty name, including
    /// `node`, in tree order.
    /// <https://html.spec.whatwg.org/multipage/input.html#radio-button-group>
    pub(crate) fn radio_group(&self, doc: &Document, node: NodeId) -> Vec<NodeId> {
        let Some(key) = self.radio_key(doc, node) else {
            return vec![node];
        };
        doc.descendants(NodeId::DOCUMENT)
            .filter(|&n| {
                self.control_type(n) == Some(ControlType::Input(InputType::Radio))
                    && self.radio_key(doc, n) == Some(key)
            })
            .collect()
    }
}

/// The radio button groups with a checked button and with a `required`
/// button.
#[derive(Default)]
struct RadioGroups<'a> {
    checked: HashSet<(Option<NodeId>, &'a str)>,
    required: HashSet<(Option<NodeId>, &'a str)>,
}

/// The state of a control from its attributes and content. The form
/// owner and the disabled and barred flags are not set.
fn default_state(
    doc: &Document,
    node: NodeId,
    ty: ControlType,
    disabled: &DisabledElements,
) -> ControlState {
    let e = doc.element(node);
    let mut edit = if ty == ControlType::TextArea {
        TextEdit::multiline()
    } else {
        TextEdit::new()
    };
    let default_value = match (ty, e) {
        // The child text content; the parser already dropped a leading
        // newline. Line breaks are normalized to LF (the API value).
        (ControlType::TextArea, _) => normalize_newlines(&doc.text_content(node)),
        (ControlType::Input(t), Some(e)) => sanitize(e, t, e.attr("value").unwrap_or("")),
        _ => String::new(),
    };
    edit.replace_text(&default_value);
    // The caret starts at the start: in Chromium (measured), text typed
    // after a label or a script focused the field goes before its value.
    edit.text_start(false);
    let has_max_len = match ty {
        ControlType::Input(t) => t.has_size(),
        other => other == ControlType::TextArea,
    };
    if has_max_len {
        let max_len = e
            .and_then(|e| e.attr("maxlength"))
            .and_then(parse_non_negative_u32)
            .map(|v| v as usize);
        edit.set_max_len(max_len);
    }
    ControlState {
        ty,
        edit,
        checked: e.is_some_and(|e| e.has_attr("checked")),
        scroll: Point::default(),
        options: if ty == ControlType::Select {
            select_options(doc, node, disabled)
        } else {
            Vec::new()
        },
        owner: None,
        disabled: false,
        barred: false,
    }
}

/// Replaces every CR LF pair and every other CR with LF.
/// <https://infra.spec.whatwg.org/#normalize-newlines>
fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// True for elements whose `form` attribute names their form owner.
/// <https://html.spec.whatwg.org/multipage/forms.html#category-listed>
fn is_listed(e: &ElementData) -> bool {
    e.is_html()
        && matches!(
            &**e.local_name(),
            "button" | "fieldset" | "input" | "object" | "output" | "select" | "textarea"
        )
}

/// The form owner of a form-associated element: the form that its `form`
/// attribute names (looked up in `ids`), else the form that the parser
/// associated with it, else its nearest `form` ancestor.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#reset-the-form-owner>
fn form_owner(doc: &Document, node: NodeId, ids: &HashMap<&str, NodeId>) -> Option<NodeId> {
    let e = doc.element(node)?;
    let is_form = |n: NodeId| doc.is_html_element(n, &local_name!("form"));
    if is_listed(e)
        && let Some(id) = e.attr("form")
    {
        return ids.get(id).copied().filter(|&f| is_form(f));
    }
    if let Some(form) = e.parser_form.filter(|&f| is_form(f) && in_document(doc, f)) {
        return Some(form);
    }
    doc.ancestors(node).find(|&n| is_form(n))
}

/// True if `node` has a `datalist` ancestor: such a control is barred
/// from constraint validation and is not submitted.
fn in_datalist(doc: &Document, node: NodeId) -> bool {
    doc.ancestors(node)
        .any(|a| doc.is_html_element(a, &local_name!("datalist")))
}

/// True if `node` is in the document tree (not detached, not in a
/// template).
fn in_document(doc: &Document, node: NodeId) -> bool {
    doc.ancestors(node).last() == Some(NodeId::DOCUMENT)
}

/// The control that a `label` element labels: the element its `for`
/// attribute names, else its first labelable descendant.
/// <https://html.spec.whatwg.org/multipage/forms.html#labeled-control>
pub(crate) fn labeled_control(doc: &Document, label: NodeId) -> Option<NodeId> {
    let e = doc.element(label)?;
    let labelable = |n: NodeId| {
        doc.element(n).is_some_and(|e| {
            e.is_html()
                && match &**e.local_name() {
                    "input" => InputType::of(e) != InputType::Hidden,
                    "button" | "meter" | "output" | "progress" | "select" | "textarea" => true,
                    _ => false,
                }
        })
    };
    if let Some(id) = e.attr("for") {
        return doc.element_by_id(id).filter(|&n| labelable(n));
    }
    doc.descendants(label).skip(1).find(|&n| labelable(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_dom::parse_html;

    fn id(doc: &Document, id: &str) -> NodeId {
        doc.element_by_id(id).expect("element exists")
    }

    #[test]
    fn default_values_are_sanitized() {
        let doc = parse_html(
            "<input id=t value='a&#10;b'><input id=u type=url value=' x '>\
             <input id=n type=number value=abc><input id=n2 type=number value='-1.5e3'>\
             <input id=c type=checkbox><textarea id=ta>\nline&#13;&#10;two</textarea>\
             <input id=r type=range><input id=r2 type=range min=10 max=20 value=x>\
             <input id=col type=color value=#ABCDEF><input id=col2 type=color value=red>",
        );
        let forms = Forms::new(&doc);
        let value = |name: &str| forms.value(&doc, id(&doc, name)).expect("a control");
        assert_eq!(value("t"), "ab");
        assert_eq!(value("u"), "x");
        assert_eq!(value("n"), "");
        assert_eq!(value("n2"), "-1.5e3");
        assert_eq!(value("c"), "on");
        assert_eq!(value("ta"), "line\ntwo");
        assert_eq!(value("r"), "50");
        assert_eq!(value("r2"), "15");
        assert_eq!(value("col"), "#abcdef");
        assert_eq!(value("col2"), "#000000");
    }

    #[test]
    fn the_last_checked_radio_button_of_a_group_wins() {
        let doc = parse_html(
            "<form><input id=a type=radio name=r checked><input id=b type=radio name=r checked>\
             <input id=c type=radio name=other checked></form>\
             <input id=d type=radio name=r checked>",
        );
        let mut forms = Forms::new(&doc);
        let checked = |forms: &Forms, name: &str| forms.checked(id(&doc, name)) == Some(true);
        assert!(!checked(&forms, "a"));
        assert!(checked(&forms, "b"));
        assert!(checked(&forms, "c"));
        // Another form owner: another group.
        assert!(checked(&forms, "d"));
        forms.set_checked(&doc, id(&doc, "a"), true);
        assert!(checked(&forms, "a") && !checked(&forms, "b") && checked(&forms, "d"));
    }

    #[test]
    fn select_options_and_values() {
        let doc = parse_html(
            "<select id=s><option disabled>x</option><option id=o2 value=v2> Two  words </option>\
             <optgroup label=g><option id=o3 label=L3>3</option></optgroup></select>\
             <select id=m><option selected>a</option><option selected>b</option></select>",
        );
        let mut forms = Forms::new(&doc);
        let s = id(&doc, "s");
        // The first option that is not disabled is selected.
        assert_eq!(forms.value(&doc, s).as_deref(), Some("v2"));
        assert_eq!(forms.checked(id(&doc, "o2")), Some(true));
        let control = forms.layout_control(&doc, s, None).expect("a control");
        assert_eq!(control.text, "Two words");
        assert_eq!(control.options, ["x", "Two words", "    L3"]);
        assert!(forms.select_option(s, 2));
        assert!(!forms.select_option(s, 99));
        assert_eq!(forms.value(&doc, s).as_deref(), Some("3"));
        // Two selected options in a drop-down: the last one stays.
        assert_eq!(forms.value(&doc, id(&doc, "m")).as_deref(), Some("b"));
    }

    #[test]
    fn form_owners() {
        let doc = parse_html(
            "<form id=f1><input id=a><input id=b form=f2></form><form id=f2></form>\
             <input id=c form=nothing><table><form id=f3><tr><td><input id=d></td></tr></form></table>",
        );
        let forms = Forms::new(&doc);
        let owner = |name: &str| forms.owner(id(&doc, name));
        assert_eq!(owner("a"), Some(id(&doc, "f1")));
        assert_eq!(owner("b"), Some(id(&doc, "f2")));
        assert_eq!(owner("c"), None);
        assert_eq!(owner("d"), Some(id(&doc, "f3")));
    }

    #[test]
    fn reset_restores_defaults() {
        let doc = parse_html(
            "<form id=f><input id=t value=x><input id=c type=checkbox checked>\
             <select id=s><option>a</option><option selected>b</option></select></form>",
        );
        let mut forms = Forms::new(&doc);
        let (t, c, s) = (id(&doc, "t"), id(&doc, "c"), id(&doc, "s"));
        forms.get_mut(t).expect("t").edit.insert("yz");
        forms.set_checked(&doc, c, false);
        forms.select_option(s, 0);
        forms.reset(&doc, id(&doc, "f"));
        assert_eq!(forms.value(&doc, t).as_deref(), Some("x"));
        assert_eq!(forms.checked(c), Some(true));
        assert_eq!(forms.value(&doc, s).as_deref(), Some("b"));
        assert_eq!(forms.owner(t), Some(id(&doc, "f")));
    }

    #[test]
    fn style_states_follow_the_state() {
        let doc = parse_html(
            "<input id=p placeholder=x><input id=r required value=v><input id=c type=checkbox>\
             <input id=ro readonly required><input id=sb type=submit><input id=b type=button>",
        );
        let mut forms = Forms::new(&doc);
        let states = forms.element_states(&doc);
        let (p, r, c) = (id(&doc, "p"), id(&doc, "r"), id(&doc, "c"));
        assert!(states[&p].contains(ElementState::PLACEHOLDER_SHOWN));
        assert!(states[&r].contains(ElementState::VALID));
        assert!(!states[&c].contains(ElementState::CHECKED));
        // Barred from validation: neither valid nor invalid.
        let neither = ElementState::VALID | ElementState::INVALID;
        assert!(!states[&id(&doc, "ro")].intersects(neither));
        assert!(!states[&id(&doc, "b")].intersects(neither));
        assert!(states[&id(&doc, "sb")].contains(ElementState::VALID));
        forms.get_mut(p).expect("p").edit.insert("a");
        forms.get_mut(r).expect("r").edit.select_all();
        forms.get_mut(r).expect("r").edit.delete_selection();
        forms.set_checked(&doc, c, true);
        let states = forms.element_states(&doc);
        assert!(!states[&p].contains(ElementState::PLACEHOLDER_SHOWN));
        assert!(states[&r].contains(ElementState::INVALID));
        assert!(states[&c].contains(ElementState::CHECKED));
    }

    #[test]
    fn required_selects_and_the_placeholder_label_option() {
        let doc = parse_html(
            "<form id=f><select id=a required><option value=''>Pick</option><option>x</option></select>\
             <select id=b required><option>first</option><option value=''>empty</option></select>\
             <select id=c required><optgroup><option value=''>in group</option></optgroup></select></form>",
        );
        let mut forms = Forms::new(&doc);
        let f = id(&doc, "f");
        assert_eq!(forms.invalid_controls(&doc, f), [id(&doc, "a")]);
        forms.select_option(id(&doc, "b"), 1);
        // An empty value that is not the placeholder label option is fine.
        assert_eq!(forms.invalid_controls(&doc, f), [id(&doc, "a")]);
        forms.select_option(id(&doc, "a"), 1);
        assert_eq!(forms.invalid_controls(&doc, f), Vec::<NodeId>::new());
    }

    #[test]
    fn required_text_areas_dates_and_radio_groups() {
        let doc = parse_html(
            "<form id=f><textarea id=t required></textarea><input id=d type=date required>\
             <input id=r1 type=radio name=g><input id=r2 type=radio name=g required>\
             <input id=c type=color required></form>",
        );
        let mut forms = Forms::new(&doc);
        let f = id(&doc, "f");
        let ids = |ids: &[&str]| ids.iter().map(|i| id(&doc, i)).collect::<Vec<_>>();
        // `required` on one button makes the whole group required.
        assert_eq!(
            forms.invalid_controls(&doc, f),
            ids(&["t", "d", "r1", "r2"])
        );
        forms.set_checked(&doc, id(&doc, "r1"), true);
        assert_eq!(forms.invalid_controls(&doc, f), ids(&["t", "d"]));
    }

    #[test]
    fn labels() {
        let doc = parse_html(
            "<label id=l1 for=b>x</label><label id=l2><input type=hidden><span><input id=a></span></label>\
             <input id=b>",
        );
        assert_eq!(labeled_control(&doc, id(&doc, "l1")), Some(id(&doc, "b")));
        assert_eq!(labeled_control(&doc, id(&doc, "l2")), Some(id(&doc, "a")));
    }

    #[test]
    fn implicit_submission_types() {
        assert!(InputType::Date(DateType::Time).blocks_implicit_submission());
        assert!(!InputType::Range.blocks_implicit_submission());
        assert!(!InputType::Color.blocks_implicit_submission());
        assert!(!InputType::Checkbox.blocks_implicit_submission());
    }
}
