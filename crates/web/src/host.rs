//! What scripts need from the engine (ADR 0028 section 1): the trait
//! [`EngineHost`], which the engine implements for its document state.
//!
//! `web` does not depend on `engine`. The engine lends its boxed document
//! state to the runtime for each task ([`crate::ScriptRuntime::lend`]) as a
//! `Box<dyn EngineHost>` and takes it back afterwards
//! ([`crate::ScriptRuntime::take_back`]); [`EngineHost::into_any`] turns the
//! box back into the engine's type. Features 7 and later add the style and
//! layout queries here.

use std::any::Any;

use swb_dom::{Document, NodeId, ParseStep, TextPosition};
use swb_style::ElementStates;

/// The result of a script write to the value of a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlWrite {
    /// The value is stored.
    Done,
    /// The element is not a text field or text area.
    NotControl,
    /// The values that scripts wrote reached the budget of the document;
    /// the caller ends the task.
    OverLimit,
}

/// The document of a task, as the engine lends it to the bindings.
pub trait EngineHost: Any {
    /// The DOM of the document.
    fn dom(&self) -> &Document;

    /// The DOM, for changes through the change functions of `swb_dom`. The
    /// engine processes the changes when the state comes back (the journal
    /// drain of ADR 0028 section 3).
    fn dom_mut(&mut self) -> &mut Document;

    /// The document URL, serialized (`document.URL`).
    fn url(&self) -> String;

    /// The document base URL, serialized (`baseURI`). It reflects the
    /// `base` elements of the DOM as it is now, also after changes of this
    /// task.
    fn base_url(&mut self) -> String;

    /// `value` parsed as a URL relative to the document base URL and
    /// serialized, or `None` if it does not parse (`[ReflectURL]`).
    fn parse_url(&mut self, value: &str) -> Option<String>;

    /// The name of the document's character encoding (`characterSet`,
    /// for example `UTF-8` or `windows-1252`).
    fn character_set(&self) -> &'static str;

    /// The URL of the document that linked to this one
    /// (`document.referrer`), serialized, or empty.
    fn referrer(&self) -> String {
        String::new()
    }

    /// The content type of the document (`text/html`, `text/plain`,
    /// `image/png`).
    fn content_type(&self) -> &str;

    /// One step of the document's parser (feature 5c): it parses until the
    /// end tag of a `script` element (with the position of the script's
    /// text in the source) or to the end of the input, and processes the
    /// changes. `None` if the document has no parser (it is complete; a
    /// [`ParseStep::Finished`] drops the parser).
    fn parse_step(&mut self) -> Option<(ParseStep, TextPosition)> {
        None
    }

    /// True if the document has a style sheet that is blocking scripts
    /// (HTML §4.2.4): a style sheet of the parser, for the current media,
    /// that has not loaded yet.
    fn style_sheets_block_scripts(&mut self) -> bool {
        false
    }

    /// Starts the fetch of the classic script at `url` (absolute, from
    /// [`EngineHost::parse_url`]) for the script element `node`; the
    /// engine hands the result to [`crate::ScriptRuntime::script_fetched`].
    /// False if the document may not load the URL (the fetch fails at
    /// once).
    fn fetch_script(&mut self, node: NodeId, url: &str) -> bool {
        let _ = (node, url);
        false
    }

    /// The states that selectors see (`:hover`, `:focus`, ...), for
    /// `matches`, `closest` and `querySelector*`. With `controls`, the
    /// states of the form controls (`:checked`, `:valid`, ...) are
    /// current for the DOM as it is now; the caller asks for them only
    /// when a selector reads them. The default has no state.
    fn element_states(&mut self, controls: bool) -> ElementStates {
        let _ = controls;
        ElementStates::default()
    }

    /// `value` parsed as a URL relative to the document URL (not the base
    /// URL) and serialized (`base.href`). The default uses
    /// [`EngineHost::parse_url`].
    fn parse_url_against_document_url(&mut self, value: &str) -> Option<String> {
        self.parse_url(value)
    }

    /// The `value` of a text field or text area as the page sees it (the
    /// value the user typed or a script wrote, else the default from the
    /// attributes); `None` for the other elements. The state of form
    /// controls is the engine's (feature 6c).
    fn control_value(&mut self, node: NodeId) -> Option<String> {
        let _ = node;
        None
    }

    /// Sets the value of a text field or text area (the dirty value flag
    /// becomes true). [`ControlWrite::NotControl`] for the other elements;
    /// [`ControlWrite::OverLimit`] if the values that scripts wrote reached
    /// the budget of the document (ADR 0028 section 16), and then the value
    /// is not stored.
    fn set_control_value(&mut self, node: NodeId, value: &str) -> ControlWrite {
        let _ = (node, value);
        ControlWrite::NotControl
    }

    /// The `indeterminate` property of a checkbox changed (the
    /// `:indeterminate` pseudo-class follows it).
    fn set_control_indeterminate(&mut self, node: NodeId, value: bool) {
        let _ = (node, value);
    }

    /// The cloning steps of `input` and `textarea`: copies the value and
    /// the checkedness of `from` to its copy `to` if `from` has the dirty
    /// value flag or the dirty checkedness flag.
    fn copy_control_state(&mut self, from: NodeId, to: NodeId) -> ControlWrite {
        let _ = (from, to);
        ControlWrite::Done
    }

    /// The checkedness of a checkbox or radio button; `None` for the
    /// other elements.
    fn control_checked(&mut self, node: NodeId) -> Option<bool> {
        let _ = node;
        None
    }

    /// Sets the checkedness of a checkbox or radio button (the dirty
    /// checkedness flag becomes true; checking a radio button unchecks the
    /// others of its group). False for the other elements.
    fn set_control_checked(&mut self, node: NodeId, checked: bool) -> bool {
        let _ = (node, checked);
        false
    }

    /// `select.selectedIndex`: the index of the first selected option in
    /// the list of options, or -1.
    fn selected_index(&mut self, select: NodeId) -> i32 {
        let _ = select;
        -1
    }

    /// `select.selectedIndex = index`.
    fn set_selected_index(&mut self, select: NodeId, index: i32) {
        let _ = (select, index);
    }

    /// `option.selected`.
    fn option_selected(&mut self, option: NodeId) -> bool {
        let _ = option;
        false
    }

    /// `option.selected = selected`.
    fn set_option_selected(&mut self, option: NodeId, selected: bool) {
        let _ = (option, selected);
    }

    /// The selectedness of a new option (`new Option()`) without the
    /// dirtiness flag: it stays until the `selected` attribute is added or
    /// removed, or a script writes `option.selected`.
    fn set_option_initial_selected(&mut self, option: NodeId, selected: bool) {
        let _ = (option, selected);
    }

    /// The `selected` attribute of `option` was added or removed.
    fn option_selected_attribute_changed(&mut self, option: NodeId) {
        let _ = option;
    }

    /// The scroll offset of the viewport in CSS px (the page coordinates
    /// of mouse events add it).
    fn scroll_offset(&self) -> (f64, f64) {
        (0.0, 0.0)
    }

    /// The focused element of the document, if any (the node in the
    /// engine's element states; `document.activeElement` falls back to the
    /// body).
    fn focused_element(&self) -> Option<NodeId> {
        None
    }

    /// True if a script can focus `node`: it is a focusable area (HTML
    /// "focusable area": a link, a control that is not disabled, an element
    /// with `tabindex`) and it is rendered (its style and its ancestors'
    /// are not `display: none` and it is not `visibility: hidden`; no
    /// layout is needed).
    fn can_focus(&mut self, node: NodeId) -> bool {
        let _ = node;
        false
    }

    /// Moves the focus to `node` or removes it. `visible` sets
    /// `:focus-visible`. The engine does not restyle at once: a loop of
    /// `focus()` calls stays linear.
    fn set_focus(&mut self, node: Option<NodeId>, visible: bool) {
        let _ = (node, visible);
    }

    /// True if the user edited the value of the text control `node` since
    /// it got the focus or since its last `change` event, and the value
    /// differs from the one then (the user's edit was committed: `change`
    /// fires). Clears the state of the control either way.
    fn take_pending_change(&mut self, node: NodeId) -> bool {
        let _ = node;
        false
    }

    /// Asks the page to scroll `node` into view when the task ends (the
    /// scroll of `focus()` without `preventScroll`; scrolling needs the
    /// layout, which a script task does not have).
    fn request_scroll_into_view(&mut self, node: NodeId) {
        let _ = node;
    }

    /// Asks the page to follow the hyperlink of the `a` or `area` element
    /// `node` when the task ends (HTML "follow the hyperlink"). False if
    /// the element has no `href` that resolves. A link to a fragment of the
    /// document scrolls; one to another document navigates.
    fn request_hyperlink(&mut self, node: NodeId) -> bool {
        let _ = node;
        false
    }

    /// The controls of `form` that fail constraint validation, in tree
    /// order.
    fn invalid_controls(&mut self, form: NodeId) -> Vec<NodeId> {
        let _ = form;
        Vec::new()
    }

    /// Asks the page to submit `form` when the task ends (HTML "submit":
    /// the planned navigation), with the submit button `submitter` or
    /// without. False if the submission does not navigate (an action that
    /// cannot load, a `dialog` form). `coordinate` is the point of the
    /// click relative to the submitter, for an image button (0, 0 for
    /// `click()`).
    fn request_form_submission(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        coordinate: (i32, i32),
    ) -> bool {
        let _ = (form, submitter, coordinate);
        false
    }

    /// The reset algorithm of `form`: its controls return to their default
    /// values.
    fn reset_form(&mut self, form: NodeId) {
        let _ = form;
    }

    /// The checked radio button of the group of `radio`, if any.
    fn checked_radio_in_group(&mut self, radio: NodeId) -> Option<NodeId> {
        let _ = radio;
        None
    }

    /// The box as `Any`, so that the engine gets its own type back.
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}
