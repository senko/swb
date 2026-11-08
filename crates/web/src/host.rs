//! What scripts need from the engine (ADR 0028 section 1): the trait
//! [`EngineHost`], which the engine implements for its document state.
//!
//! `web` does not depend on `engine`. The engine lends its boxed document
//! state to the runtime for each task ([`crate::ScriptRuntime::lend`]) as a
//! `Box<dyn EngineHost>` and takes it back afterwards
//! ([`crate::ScriptRuntime::take_back`]); [`EngineHost::into_any`] turns the
//! box back into the engine's type. The style and layout queries of scripts
//! (`getComputedStyle`) are here too.

use std::any::Any;
use std::sync::Arc;

use swb_dom::{Document, HtmlParser, NodeId, ParseStep, TextPosition, WrittenStep};
use swb_style::cssom::resolved::PseudoElement;
use swb_style::cssom::sheet::SheetRef;
use swb_style::{ComputedStyle, ElementStates};

use crate::storage::SharedStorage;

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

/// The selection of a text control as a key: byte offsets of the start and
/// the end, and whether it is backward ([`EngineHost::control_selection_key`]).
pub type SelectionKey = (usize, usize, bool);

/// The validity states of a form control (`ValidityState`) and whether it
/// is a candidate for constraint validation (`willValidate`).
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#validity-states>
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
pub struct ControlValidity {
    /// `willValidate`.
    pub will_validate: bool,
    /// `valueMissing`.
    pub value_missing: bool,
    /// `typeMismatch`.
    pub type_mismatch: bool,
    /// `patternMismatch`.
    pub pattern_mismatch: bool,
    /// `tooLong`.
    pub too_long: bool,
    /// `tooShort`.
    pub too_short: bool,
    /// `rangeUnderflow`.
    pub range_underflow: bool,
    /// `rangeOverflow`.
    pub range_overflow: bool,
    /// `stepMismatch`.
    pub step_mismatch: bool,
    /// `badInput`.
    pub bad_input: bool,
    /// `customError`.
    pub custom_error: bool,
}

impl ControlValidity {
    /// `valid`: no state is set.
    pub fn is_valid(&self) -> bool {
        !(self.value_missing
            || self.type_mismatch
            || self.pattern_mismatch
            || self.too_long
            || self.too_short
            || self.range_underflow
            || self.range_overflow
            || self.step_mismatch
            || self.bad_input
            || self.custom_error)
    }
}

/// What a control or an option holds besides its attributes, on its way to
/// another document of the runtime (ADR 0031 "Done (feature 12b2)").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlSnapshot {
    /// The value, if the dirty value flag is set.
    pub value: Option<String>,
    /// The selection in the value (UTF-16 offsets, backward).
    pub selection: Option<(u32, u32, bool)>,
    /// The checkedness, if the dirty checkedness flag is set.
    pub checked: Option<bool>,
    /// The selectedness of an option and its dirtiness flag.
    pub selected: Option<(bool, bool)>,
    /// The message of `setCustomValidity`.
    pub custom_validity: Option<String>,
}

/// The selection of a text field or text area, in UTF-16 code units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    /// The start of the selection.
    pub start: u32,
    /// The end of the selection.
    pub end: u32,
    /// True if the selection direction is "backward".
    pub backward: bool,
}

/// The `selectionMode` of `setRangeText`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeTextMode {
    /// Select the new text.
    Select,
    /// Put the caret before the new text.
    Start,
    /// Put the caret after the new text.
    End,
    /// Keep the selection where it was, adjusted to the change.
    Preserve,
}

/// A rectangle in CSS px, as `DOMRect` has it (CSSOM View geometry).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DomRect {
    /// The left edge.
    pub x: f64,
    /// The top edge.
    pub y: f64,
    /// The width.
    pub width: f64,
    /// The height.
    pub height: f64,
}

/// `offsetParent` and the `offset*` values of an element (CSSOM View §7,
/// <https://drafts.csswg.org/cssom-view/#extensions-to-the-htmlelement-interface>),
/// in whole CSS px as Chromium rounds them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OffsetMetrics {
    /// `offsetParent`.
    pub parent: Option<NodeId>,
    /// `offsetLeft`.
    pub left: i32,
    /// `offsetTop`.
    pub top: i32,
    /// `offsetWidth`.
    pub width: i32,
    /// `offsetHeight`.
    pub height: i32,
}

/// The `scroll*` values of an element (CSSOM View §6).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollMetrics {
    /// `scrollLeft`.
    pub left: f64,
    /// `scrollTop`.
    pub top: f64,
    /// `scrollWidth`.
    pub width: i32,
    /// `scrollHeight`.
    pub height: i32,
}

/// The alignment of `scrollIntoView` on one axis (`ScrollLogicalPosition`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAlign {
    /// `"start"`.
    Start,
    /// `"center"`.
    Center,
    /// `"end"`.
    End,
    /// `"nearest"`.
    Nearest,
}

/// A script scroll: each coordinate is a position or, with `by`, a delta;
/// `None` keeps the axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollRequest {
    /// The horizontal position or delta.
    pub x: Option<f64>,
    /// The vertical position or delta.
    pub y: Option<f64>,
    /// True for `scrollBy`: the values are deltas.
    pub by: bool,
}

/// The viewport of the document (no flush needed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// The width in CSS px.
    pub width: f64,
    /// The height in CSS px.
    pub height: f64,
    /// Device pixels per CSS px.
    pub scale: f64,
}

/// The `position` of an element, as the observers need it to find the
/// containing block chain (CSS Positioned Layout).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObservedPosition {
    /// `static`.
    #[default]
    Static,
    /// `relative` or `sticky`.
    Relative,
    /// `absolute`.
    Absolute,
    /// `fixed`.
    Fixed,
}

/// The layout box of an element for the intersection and resize observers
/// (feature 9a): the first box fragment as layout placed it (no transforms),
/// in CSS px, with the sizes in layout units (multiples of 1/64 px).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ObservedBox {
    /// The width and height of the border box.
    pub border_size: (f64, f64),
    /// The width and height of the content box.
    pub content_size: (f64, f64),
    /// The padding on the left and on the top (`contentRect.x` and `.y`).
    pub padding_origin: (f64, f64),
    /// The border widths: top, right, bottom, left.
    pub border_widths: [f64; 4],
    /// True if the box is a part of an inline box: it has no size.
    pub inline: bool,
    /// The `position`.
    pub position: ObservedPosition,
    /// True if the element has a transform (it is the containing block of
    /// absolutely and fixed positioned boxes).
    pub transformed: bool,
    /// True if `overflow` clips the content on either axis.
    pub clips: bool,
}

/// The points that a dispatched mouse event is relative to (`offsetX` and
/// `layerX`), in document coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventOrigins {
    /// The padding edge of the target's first box, with the scroll offsets
    /// of its scroll containers (`offsetX` and `offsetY`).
    pub padding: (f64, f64),
    /// The border edge of the nearest positioned inclusive ancestor of the
    /// target (`layerX` and `layerY`); the origin if there is none.
    pub layer: (f64, f64),
}

/// How the document was reached (`performance.navigation.type`, the
/// `type` of the navigation entry; HTML "navigate", reload, traversal).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NavigationKind {
    /// A new navigation (a link, the address bar, `location`).
    #[default]
    Navigate,
    /// A reload.
    Reload,
    /// A traversal of the session history (back, forward, `go`).
    BackForward,
}

/// What the engine knows of the navigation that made the document (the
/// navigation entry of the performance timeline, feature 8a2): swb's loader
/// gives no times for the phases of the request, only the result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavigationInfo {
    /// How the document was reached.
    pub kind: NavigationKind,
    /// The HTTP status of the response (0 for a document that did not
    /// come from the network).
    pub status: u16,
    /// The size of the response body in bytes.
    pub body_bytes: u64,
}

/// An entry of a form's entry list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormDataEntry {
    /// The name.
    pub name: String,
    /// The value.
    pub value: FormDataValue,
}

/// The value of a [`FormDataEntry`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormDataValue {
    /// A string.
    Text(String),
    /// A file (swb has the empty file of a file input without a file).
    File {
        /// The file name.
        filename: String,
        /// The MIME type.
        content_type: String,
        /// The bytes.
        data: Vec<u8>,
    },
}

/// What an `img` element reports of its image (`complete`, `naturalWidth`,
/// `naturalHeight`, `currentSrc`; feature 10b).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageInfo {
    /// `complete`.
    pub complete: bool,
    /// `naturalWidth`.
    pub natural_width: u32,
    /// `naturalHeight`.
    pub natural_height: u32,
    /// `currentSrc`.
    pub current_src: String,
}

/// The document of a task, as the engine lends it to the bindings.
pub trait EngineHost: Any {
    /// The user agent string (`navigator.userAgent`, ADR 0030): the one
    /// that requests send.
    fn user_agent(&self) -> &'static str {
        swb_net::USER_AGENT
    }

    /// How the document was reached, and what the response was.
    fn navigation_info(&self) -> NavigationInfo {
        NavigationInfo::default()
    }

    /// The cookies that a script of the document sees (`document.cookie`,
    /// feature 8b): the page's jar without the `HttpOnly` cookies. The
    /// bindings call it only for a document with an `http:` or `https:`
    /// URL. The default has no jar.
    fn cookie_string(&self) -> String {
        String::new()
    }

    /// Stores the cookie of a string that a script wrote to
    /// `document.cookie`. Rejected cookies are ignored.
    fn set_cookie(&mut self, line: &str) {
        let _ = line;
    }

    /// The Web Storage of the page, shared by its documents (the engine
    /// creates one store for the page). The default has none; the
    /// runtime then uses a store of its own, which lasts as long as the
    /// runtime.
    fn storage(&self) -> Option<SharedStorage> {
        None
    }

    /// The DOM of the document.
    fn dom(&self) -> &Document;

    /// The DOM, for changes through the change functions of `swb_dom`. The
    /// engine processes the changes when the state comes back (the journal
    /// drain of ADR 0028 section 3).
    fn dom_mut(&mut self) -> &mut Document;

    /// The document URL, serialized (`document.URL`).
    fn url(&self) -> String;

    /// The serialized origin of the document (`self.origin`): not the URL's
    /// for `about:blank`, `about:srcdoc` and sandboxed documents.
    fn origin_text(&self) -> String {
        swb_net::Url::parse(&self.url()).map_or_else(
            |_| "null".to_owned(),
            |url| url.origin().ascii_serialization(),
        )
    }

    /// The URL that gives the document its cookies and storage, serialized:
    /// the creator's for `about:blank` and `about:srcdoc` documents with their
    /// creator's origin, else the document's own.
    fn storage_url(&self) -> String {
        self.url()
    }

    /// The partition of the document's storage key (ADR 0031 section 3): empty for a
    /// top-level document and for a frame whose ancestors are all of its site, else the
    /// top-level site. The default is the empty partition.
    fn storage_partition(&self) -> crate::storage::StoragePartition {
        crate::storage::StoragePartition::FirstParty
    }

    /// True if the document is a frame document whose sandbox has no
    /// `allow-same-origin`, whatever its URL.
    fn sandboxed_origin(&self) -> bool {
        false
    }

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
    /// text in the source, `None` for a script from `document.write`) or
    /// to the end of the input, and processes the changes. `None` if the
    /// document has no parser (it is complete; a [`ParseStep::Finished`]
    /// drops the parser).
    fn parse_step(&mut self) -> Option<(ParseStep, Option<TextPosition>)> {
        None
    }

    /// The document's parser, for the insertion point of `document.write`
    /// (M9 feature 2): the runtime marks the run of each script that the
    /// parser starts ([`HtmlParser::enter_script`]) and puts written text
    /// in ([`HtmlParser::write`]). `None` without a parser.
    fn parser(&mut self) -> Option<&mut HtmlParser> {
        None
    }

    /// Reads the text that `document.write` put into the parser, up to the
    /// insertion point or the end tag of a `script` element, and processes
    /// the changes ([`HtmlParser::resume_written`]). `None` without a
    /// parser.
    fn parse_written_step(&mut self) -> Option<WrittenStep> {
        None
    }

    /// True if the document has a style sheet that is blocking scripts
    /// (HTML §4.2.4): a style sheet of the parser, for the current media,
    /// that has not loaded yet.
    fn style_sheets_block_scripts(&mut self) -> bool {
        false
    }

    /// Starts the fetch of the script at `url` (absolute, from
    /// [`EngineHost::parse_url`]) for the script element `node`, as a
    /// classic script or a module script (`mode`); the engine hands the
    /// result to [`crate::ScriptRuntime::script_fetched`]. False if the
    /// document may not load the URL (the fetch fails at once).
    fn fetch_script(&mut self, node: NodeId, url: &str, mode: crate::ScriptFetchMode) -> bool {
        let _ = (node, url, mode);
        false
    }

    /// Starts the fetch of a module of the module map (ADR 0034 section 8)
    /// at `url` (absolute, without a fragment), as a module script
    /// (`cors`, the JavaScript MIME type check, UTF-8): the engine hands
    /// the result to [`crate::ScriptRuntime::module_fetched`] with `id`.
    /// False if the document may not load the URL (the fetch fails at
    /// once).
    fn fetch_module(&mut self, id: u64, url: &str, options: crate::ModuleFetch) -> bool {
        let _ = (id, url, options);
        false
    }

    /// Makes the fetch `id` of [`EngineHost::fetch_module`] delay the `load`
    /// event, if it does not (a script element joined the fetch of an
    /// `import()`; ADR 0034 section 10). Nothing if the fetch is not there.
    fn delay_load_for_module(&mut self, id: u64) {
        let _ = id;
    }

    /// Starts the fetch of a script's `fetch()` call (feature 10a): the
    /// engine gives the request its initiator (the document's origin), runs
    /// it through its loader and queue (at most 64 at once per document)
    /// and later hands the result to
    /// [`crate::ScriptRuntime::api_fetch_completed`]. Returns the id of the
    /// call, or `None` if the document may not load the URL or its queue is
    /// full (the call fails at once).
    fn start_api_fetch(&mut self, request: swb_net::Request) -> Option<u64> {
        let _ = request;
        None
    }

    /// Drops the call `id` of [`EngineHost::start_api_fetch`] (its signal
    /// aborted): a queued request is not sent, and the result of one in
    /// flight is ignored.
    fn cancel_api_fetch(&mut self, id: u64) {
        let _ = id;
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

    /// The validity states of an element (`validity`, `willValidate`):
    /// those of a form control, or only `customError` for the other
    /// elements that have the constraint validation API (`fieldset`,
    /// `output`), which are never candidates.
    fn control_validity(&mut self, node: NodeId) -> ControlValidity {
        let _ = node;
        ControlValidity::default()
    }

    /// `validationMessage`: Chromium's English text of the first state
    /// that is set; empty for an element that is not a candidate.
    fn control_validation_message(&mut self, node: NodeId) -> String {
        let _ = node;
        String::new()
    }

    /// `setCustomValidity(message)`; an empty message clears the error.
    /// [`ControlWrite::OverLimit`] if the messages reached the budget of
    /// the document (ADR 0028 section 16), and then nothing is stored.
    fn set_custom_validity(&mut self, node: NodeId, message: &str) -> ControlWrite {
        let _ = (node, message);
        ControlWrite::Done
    }

    /// A validation of `form` found an invalid control and reported it
    /// (`reportValidity()`, a submission): its controls count as interacted
    /// with, for `:user-valid` and `:user-invalid`.
    fn form_validation_reported(&mut self, form: NodeId) {
        let _ = form;
    }

    /// True if `node` is a candidate for constraint validation and fails
    /// it (an `invalid` event is due).
    fn control_is_invalid(&mut self, node: NodeId) -> bool {
        let _ = node;
        false
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

    /// Takes what scripts and the user gave the controls and options below
    /// `root` (inclusive), which leave for another document of the runtime.
    fn take_control_snapshots(&mut self, root: NodeId) -> Vec<(NodeId, ControlSnapshot)> {
        let _ = root;
        Vec::new()
    }

    /// The same as [`EngineHost::take_control_snapshots`] for copies, of `root`
    /// and with `deep` of its descendants; nothing changes here.
    fn copy_control_snapshots(
        &mut self,
        root: NodeId,
        deep: bool,
    ) -> Vec<(NodeId, ControlSnapshot)> {
        let _ = (root, deep);
        Vec::new()
    }

    /// Gives the control `node` of this document what another document's
    /// control had.
    fn put_control_snapshot(&mut self, node: NodeId, snapshot: ControlSnapshot) -> ControlWrite {
        let _ = (node, snapshot);
        ControlWrite::Done
    }

    /// The selection of a text field or text area, or `None` if the
    /// element has no selection (an input of a type that the selection
    /// API does not apply to, another element).
    fn control_selection(&mut self, node: NodeId) -> Option<SelectionRange> {
        let _ = node;
        None
    }

    /// The selection of a text field or text area as a cheap key (byte
    /// offsets of the start and the end in the value, and the direction):
    /// two keys are equal if the selection is. `None` if the element has no
    /// selection or no state. The selection events compare keys, so a write
    /// of a large value does not scan it. With `sync` the state of a
    /// control in the document is made current first (a rebuild if the
    /// DOM changed); without it a control that has no current state gives
    /// `None`, so that a loop of value writes does not rebuild the states.
    fn control_selection_key(&mut self, node: NodeId, sync: bool) -> Option<SelectionKey> {
        let _ = (node, sync);
        None
    }

    /// Sets the selection of a text field or text area (HTML "set the
    /// selection range": offsets are clamped, `start` above `end`
    /// collapses to `end`). False if the element has no selection.
    fn set_control_selection(
        &mut self,
        node: NodeId,
        start: u32,
        end: u32,
        backward: bool,
    ) -> bool {
        let _ = (node, start, end, backward);
        false
    }

    /// Selects all the text of a text field or text area. False for the
    /// other elements.
    fn select_control_text(&mut self, node: NodeId) -> bool {
        let _ = node;
        false
    }

    /// `setRangeText`: replaces the range between two offsets (UTF-16 code
    /// units, clamped) of a text field or text area, then sets the
    /// selection as `mode` says. The value counts as changed by a script.
    fn set_control_range_text(
        &mut self,
        node: NodeId,
        replacement: &str,
        start: u32,
        end: u32,
        mode: RangeTextMode,
    ) -> ControlWrite {
        let _ = (node, replacement, start, end, mode);
        ControlWrite::NotControl
    }

    /// The `type` attribute of an `input` changed; `old_value` is the
    /// value before it if the old type had the value mode `value`. The
    /// engine sanitizes the value for the new type (also for a control that
    /// has no state yet) and drops a value that a script wrote if the new
    /// type shows the `value` attribute instead.
    fn input_type_changed(&mut self, node: NodeId, old_value: Option<String>) {
        let _ = (node, old_value);
    }

    /// The checkedness of an `input`; `None` for the other elements.
    fn control_checked(&mut self, node: NodeId) -> Option<bool> {
        let _ = node;
        None
    }

    /// Sets the checkedness of an `input` (the dirty checkedness flag
    /// becomes true; checking a radio button unchecks the others of its
    /// group; an input of another type keeps it for later). False for the
    /// other elements.
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

    /// The `selected` attribute of `option` was added or removed. Called
    /// right after the change, before any other change of the DOM.
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

    /// True while the window has the focus (`document.hasFocus()`): not
    /// after Tab left the page, and not after the frame that held the focus
    /// was removed. A page whose focus is in a frame has it.
    fn window_has_focus(&self) -> bool {
        true
    }

    /// The `iframe` element of the document that holds the focus through
    /// its frame: `document.activeElement` is the element while no element
    /// of the document itself is focused. It is not the focused element
    /// for `:focus` or for the focusing steps (no `blur` event for it).
    fn frame_focus_element(&self) -> Option<NodeId> {
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

    /// True while the last input that moved the focus or pressed a key was
    /// the keyboard, or there was none. `focus()` without the `focusVisible`
    /// option matches `:focus-visible` exactly then (Chromium's heuristic).
    fn keyboard_modality(&self) -> bool {
        true
    }

    /// Sets the modality: a label's activation focuses its control as a
    /// mouse press does.
    fn set_keyboard_modality(&mut self, keyboard: bool) {
        let _ = keyboard;
    }

    /// True if the user edited the value of the text control `node` since
    /// it got the focus or since its last `change` event, and the value
    /// differs from the one then (the user's edit was committed: `change`
    /// fires). Clears the state of the control either way.
    fn take_pending_change(&mut self, node: NodeId) -> bool {
        let _ = node;
        false
    }

    /// Asks to scroll `node` into view as `focus()` without `preventScroll`
    /// does: at the next geometry query of the task or when the task ends,
    /// whichever comes first (Chromium's `scrollY` right after `focus()`
    /// shows the scroll; a loop of `focus()` calls must not lay out at each
    /// call).
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
    ///
    /// `entries` replaces the form's entry list when a `formdata` listener
    /// changed it (feature 10b).
    fn request_form_submission(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        coordinate: (i32, i32),
        entries: Option<Vec<FormDataEntry>>,
    ) -> bool {
        let _ = (form, submitter, coordinate, entries);
        false
    }

    /// `complete`, the natural size and `currentSrc` of the `img` element
    /// `node` (feature 10b). The default knows no loads: an image is
    /// complete when it has neither `src` nor `srcset`.
    fn image_info(&mut self, node: NodeId) -> ImageInfo {
        let has = |name: &str| {
            self.dom()
                .element(node)
                .and_then(|e| e.attr(name))
                .is_some_and(|v| !v.is_empty())
        };
        ImageInfo {
            complete: !has("src") && !has("srcset"),
            ..ImageInfo::default()
        }
    }

    /// The entry list of `form` as a submission by `submitter` would send
    /// it (HTML "constructing the entry list" without the `formdata`
    /// event), for `new FormData(form)` (feature 10b).
    fn form_entries(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        coordinate: (i32, i32),
    ) -> Vec<FormDataEntry> {
        let _ = (form, submitter, coordinate);
        Vec::new()
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

    /// The computed style of `element`, or of its pseudo-element `pseudo`
    /// (also one without a box), for `getComputedStyle` (CSSOM §9); `None`
    /// if the element is not in the document. The default has none.
    fn computed_style(
        &mut self,
        element: NodeId,
        pseudo: Option<PseudoElement>,
    ) -> Option<Arc<ComputedStyle>> {
        let _ = (element, pseudo);
        None
    }

    /// The resolved value of `property` of `element` (or of its
    /// pseudo-element), serialized as Chromium serializes it
    /// (`swb_style::cssom::resolved`); empty if the element has no style.
    /// A value that layout resolves flushes the layout (ADR 0028 section
    /// 9). The default is empty.
    fn resolved_value(
        &mut self,
        element: NodeId,
        pseudo: Option<PseudoElement>,
        property: &str,
    ) -> String {
        let _ = (element, pseudo, property);
        String::new()
    }

    // ----- Style sheets (CSSOM) -----
    //
    // Each of these processes the DOM changes first, so that a `style`
    // element that a script just inserted has its sheet.

    /// The number of the document's style sheets (`document.styleSheets`):
    /// the sheets of the `style` and `link` elements of the document that
    /// have one (a `link` has one once its load ended), in tree order.
    fn style_sheet_count(&mut self) -> usize {
        0
    }

    /// The style sheet at `index` of the list of
    /// [`EngineHost::style_sheet_count`].
    fn style_sheet(&mut self, index: usize) -> Option<SheetRef> {
        let _ = index;
        None
    }

    /// The style sheet of the `style` or `link` element `owner` (`sheet`):
    /// `None` if it has none (not in the document, a `link` that loads, a
    /// `style` of another type).
    fn style_sheet_of(&mut self, owner: NodeId) -> Option<SheetRef> {
        let _ = owner;
        None
    }

    /// Processes the DOM changes, so that a sheet whose `style` element a
    /// script just removed or rewrote has lost its owner (`ownerNode`).
    fn settle_style_sheets(&mut self) {}

    /// A script changed a style sheet of the document (its rules, a rule,
    /// its media or its disabled flag): the rule index is built again at the
    /// next style flush.
    fn style_sheets_changed(&mut self) {}

    // ----- Geometry and scrolling (CSSOM View) -----
    //
    // Each of these that needs layout flushes it first (a forced run in a
    // script task); the bindings check the task deadline afterwards.

    /// The client rectangles of `element` (`getClientRects`) in viewport
    /// coordinates: one per box fragment, transformed; empty if it has no
    /// box.
    fn client_rects(&mut self, element: NodeId) -> Vec<DomRect> {
        let _ = element;
        Vec::new()
    }

    /// `offsetParent` and the `offset*` values of `element`.
    fn offset_metrics(&mut self, element: NodeId) -> OffsetMetrics {
        let _ = element;
        OffsetMetrics::default()
    }

    /// `clientLeft`, `clientTop`, `clientWidth` and `clientHeight` of
    /// `element`.
    fn client_metrics(&mut self, element: NodeId) -> [i32; 4] {
        let _ = element;
        [0; 4]
    }

    /// The `scroll*` values of `element`.
    fn scroll_metrics(&mut self, element: NodeId) -> ScrollMetrics {
        let _ = element;
        ScrollMetrics::default()
    }

    /// Scrolls `element` (`scrollTop = ...`, `element.scrollTo` and
    /// `scrollBy`): a scroll container, or the viewport for the element
    /// that scrolls it; other elements do not scroll. The values are
    /// finite.
    fn scroll_element(&mut self, element: NodeId, request: ScrollRequest) {
        let _ = (element, request);
    }

    /// `scrollX` and `scrollY`: [`EngineHost::scroll_offset`] after the
    /// scroll of a `focus()` that waits (see
    /// [`EngineHost::request_scroll_into_view`]).
    fn scroll_position(&mut self) -> (f64, f64) {
        self.scroll_offset()
    }

    /// Scrolls the viewport (`window.scrollTo` and `scrollBy`). The values
    /// are finite.
    fn scroll_viewport(&mut self, request: ScrollRequest) {
        let _ = request;
    }

    /// `element.scrollIntoView` with the alignments on the block and the
    /// inline axis.
    fn scroll_into_view(&mut self, element: NodeId, block: ScrollAlign, inline: ScrollAlign) {
        let _ = (element, block, inline);
    }

    /// `document.scrollingElement`.
    fn scrolling_element(&mut self) -> Option<NodeId> {
        None
    }

    /// The viewport (`innerWidth`, `devicePixelRatio`).
    fn viewport(&self) -> Viewport {
        Viewport {
            width: 0.0,
            height: 0.0,
            scale: 1.0,
        }
    }

    /// The elements at the point (`x`, `y`) of the viewport, topmost first
    /// (`elementsFromPoint`; with `all` false at most the first,
    /// `elementFromPoint`). Empty outside the viewport.
    fn elements_from_point(&mut self, x: f64, y: f64, all: bool) -> Vec<NodeId> {
        let _ = (x, y, all);
        Vec::new()
    }

    /// The size of the content box of the first box of `element` in CSS px
    /// (`img.width` of a rendered image); `None` if it has no box.
    fn content_size(&mut self, element: NodeId) -> Option<(f64, f64)> {
        let _ = element;
        None
    }

    /// The layout box of `element` for the observers; `None` if it has no
    /// box (not rendered, detached, `display: none`) or there is no layout.
    /// Flushes style and layout.
    fn observed_box(&mut self, element: NodeId) -> Option<ObservedBox> {
        let _ = element;
        None
    }

    /// The points that a mouse event dispatched at `target` is relative to;
    /// `None` if the target has no box.
    fn event_origins(&mut self, target: NodeId) -> Option<EventOrigins> {
        let _ = target;
        None
    }

    // ----- Rendered text and media -----

    /// The rendered text of `element` (`innerText`, HTML "rendered text
    /// collection steps"), after a style flush; `None` if the element is
    /// not being rendered (the caller takes its text content).
    fn inner_text(&mut self, element: NodeId) -> Option<String> {
        let _ = element;
        None
    }

    /// `getBBox` and `getTotalLength` of an SVG element: its fill bounding
    /// box in its user space and the length of its path (0 for other
    /// elements), after a layout flush; `None` if layout drew nothing for
    /// it.
    fn svg_geometry(&mut self, element: NodeId) -> Option<(DomRect, f64)> {
        let _ = element;
        None
    }

    /// The media environment of the document (`matchMedia`; no flush).
    fn media_environment(&self) -> swb_css::MediaEnvironment {
        swb_css::MediaEnvironment::default()
    }

    // ----- Location and the session history -----

    /// The number of entries in the session history (`history.length`).
    fn history_length(&self) -> usize {
        1
    }

    /// The state of the current history entry (`history.state`), or `None`
    /// for the state `null`.
    fn history_state(&self) -> Option<std::rc::Rc<crate::Serialized>> {
        None
    }

    /// True if `history.scrollRestoration` is `"manual"`.
    fn scroll_restoration_manual(&self) -> bool {
        false
    }

    /// Sets `history.scrollRestoration`.
    fn set_scroll_restoration(&mut self, manual: bool) {
        let _ = manual;
    }

    /// `pushState()` (`replace` false) or `replaceState()`: the document's
    /// URL becomes `url` (absolute, serialized; `None` keeps it) and the
    /// current entry's state `state`, at once; the page adds or replaces
    /// the history entry when the task ends. No load, no `hashchange`.
    fn push_state(
        &mut self,
        state: std::rc::Rc<crate::Serialized>,
        url: Option<&str>,
        replace: bool,
    ) {
        let _ = (state, url, replace);
    }

    /// Asks the page to navigate to `url` (absolute, serialized) when the
    /// task ends: `location.href = url`, `assign` (`replace` false) and
    /// `replace`. The last request of a task wins. A URL that differs from
    /// the document's only in the fragment (and has one) is a navigation
    /// to a fragment: the document's URL and the history length change at
    /// once, the page adds the entry and scrolls when the task ends, and
    /// the call returns true; the caller fires `popstate` and
    /// `hashchange`. `caller` is the frame of the document whose script asked: it is the
    /// source of the new document's referrer.
    fn request_navigation(&mut self, url: &str, replace: bool, caller: u32) -> bool {
        let _ = (url, replace, caller);
        false
    }

    /// Asks the page to reload the document when the task ends.
    fn request_reload(&mut self) {}

    /// Asks the page to traverse the session history by `delta` entries
    /// when the task ends (`history.go(delta)`; 0 reloads).
    fn request_traversal(&mut self, delta: i32) {
        let _ = delta;
    }

    /// The box as `Any`, so that the engine gets its own type back.
    fn into_any(self: Box<Self>) -> Box<dyn Any>;

    /// The `iframe` elements that the changes processed so far inserted and
    /// that the bindings have not met through a node insertion (the parser's):
    /// the runtime makes their frames. The list goes.
    fn take_inserted_frames(&mut self) -> Vec<NodeId> {
        Vec::new()
    }

    /// The `link` elements with `rel=modulepreload` that the parser's last
    /// steps inserted (scripting on): the runtime starts their fetches. The
    /// list goes. Links that scripts insert go through the node insertion
    /// bindings, not through this list.
    fn take_parser_links(&mut self) -> Vec<NodeId> {
        Vec::new()
    }

    /// The document's frame was destroyed or got another document (ADR 0031
    /// section 5): its requests and its parser end, its derived data goes,
    /// and from now on it starts nothing. The nodes stay readable.
    fn retire(&mut self) {}
}

/// A frame that [`FrameHost::connect`] made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewFrame {
    /// The number of the frame.
    pub frame: u32,
    /// True if the frame keeps its initial `about:blank` document: its
    /// container's `load` fires inside the insertion (measured).
    pub loaded_at_once: bool,
}

/// A frame that [`FrameHost::disconnect`] destroyed.
pub struct RetiredFrame {
    /// The number of the frame.
    pub frame: u32,
    /// The document's key in the runtime, if it has a realm.
    pub key: Option<crate::runtime::DocumentKey>,
    /// The document if it has a realm and the registry held it; the runtime
    /// keeps it while the realm lives. `None` for a lent document.
    pub doc: Option<Box<dyn EngineHost>>,
}

/// The documents that the runtime holds, for [`FrameHost::update_viewports`].
pub trait LentDocuments {
    /// The document `key` if the runtime holds it.
    fn lent_host(&mut self, key: crate::runtime::DocumentKey) -> Option<&mut dyn EngineHost>;
}

/// The frame tree of the page as natives see it (ADR 0031 section 5). The page
/// shares its registry with the runtime; a native borrows it for one call and
/// never across a call into script. A frame has a number below 2^30 (the
/// binding state uses the next bit, `window_proxy.rs`); the top level has 0. A
/// number of a destroyed frame finds nothing.
pub trait FrameHost {
    /// True if the frame exists.
    fn is_alive(&self, frame: u32) -> bool;

    /// The parent of the frame; `None` for the top level and a stale number.
    fn parent(&self, frame: u32) -> Option<u32>;

    /// The number of child frames.
    fn child_count(&self, frame: u32) -> u32;

    /// The child frame at `index`, in the tree order of the containers.
    fn child_at(&self, frame: u32, index: u32) -> Option<u32>;

    /// The first child frame with the name `name`.
    fn child_named(&self, frame: u32, name: &str) -> Option<u32>;

    /// The child frame of the `iframe` element `container` of the document of
    /// `parent`, which has the origin of the running runtime.
    fn child_of_container(&self, parent: u32, container: NodeId) -> Option<u32>;

    /// The child frames with their containers, if the frame's document has the
    /// origin of the running runtime.
    fn child_containers(&self, frame: u32) -> Vec<(u32, NodeId)>;

    /// The `iframe` element of the frame, if its parent's document has the
    /// origin of the running runtime: a `NodeId` means nothing elsewhere.
    fn container(&self, frame: u32) -> Option<NodeId>;

    /// The name of the frame (`window.name`).
    fn name(&self, frame: u32) -> String;

    /// Sets the name of the frame.
    fn set_name(&mut self, frame: u32, name: &str);

    /// True if the document of the frame can have a realm in this runtime: it
    /// has the origin of the runtime. Each origin has a runtime of its own.
    fn in_this_agent(&self, frame: u32) -> bool;

    /// True if the document of the frame may run scripts (its sandbox).
    fn scripts_allowed(&self, frame: u32) -> bool;

    /// The key of the frame's document in the running runtime, if it has a
    /// realm there. A document of another runtime has no key here.
    fn document_key(&self, frame: u32) -> Option<crate::runtime::DocumentKey>;

    /// Records the key of the frame's document.
    fn set_document_key(&mut self, frame: u32, key: Option<crate::runtime::DocumentKey>);

    /// True if the frame's document still loads subresources (its `load` event
    /// is to come).
    fn document_loading(&self, frame: u32) -> bool;

    /// Takes the frame's document out of the registry for the rest of the
    /// task, if `key` is its key in the running runtime; the page takes it
    /// back after the task.
    fn take_document(
        &mut self,
        frame: u32,
        key: crate::runtime::DocumentKey,
    ) -> Option<Box<dyn EngineHost>>;

    /// An `iframe` element of the document of `parent` (`parent_doc`) was
    /// connected (HTML "the iframe post-connection steps"): makes the frame
    /// with its initial `about:blank` document. Its attributes navigate it
    /// after the task. `None` past the limits of frames and depth.
    fn connect(
        &mut self,
        parent: u32,
        container: NodeId,
        parent_doc: &mut dyn EngineHost,
    ) -> Option<NewFrame>;

    /// The frame and its descendants, children first.
    fn subtree(&self, frame: u32) -> Vec<u32>;

    /// Destroys the frame and its descendants (HTML "destroy a child
    /// navigable"). The page acts on the rest after the task.
    fn disconnect(&mut self, frame: u32) -> Vec<RetiredFrame>;

    /// The frame's document and its frames have loaded, and its container's
    /// `load` event fires: the frame no longer delays its parent's.
    fn frame_loaded(&mut self, frame: u32);

    /// True if the document of frame `source` may navigate frame `target`.
    fn may_navigate(&self, source: u32, target: u32) -> bool;

    /// A script writes to the `location` of frame `target`: the navigation, which
    /// starts after the task, has the sandboxing flags of this moment.
    fn note_location_flags(&mut self, target: u32, docs: &mut dyn LentDocuments);

    /// The page navigates frame `target` to `url` when the task ends, for the
    /// document of frame `source`: a write to the `Location` of a frame of
    /// another origin, whose document no task can borrow. With `replace` the
    /// navigation replaces the entry of the session history. A later request
    /// for the same frame replaces an earlier one.
    fn navigate_frame(&mut self, target: u32, url: &str, source: u32, replace: bool);

    /// The frame that the browsing context name `name` (`_parent`, `_top` or a
    /// frame's name) means for frame `source`, if `source` may navigate it. `None`
    /// for a name without a frame (a new context in other browsers) and for a
    /// frame that the rules refuse.
    fn choose_frame(&self, source: u32, name: &str) -> Option<u32>;

    /// The frame that holds the page's focus (the top level if none does).
    fn focused_frame(&self) -> u32;

    /// True if the frame holds the page's focus, or a frame inside it does.
    fn holds_focus(&self, frame: u32) -> bool;

    /// A script moved the page's focus to the frame: the documents above it are
    /// focused through their `iframe` elements, the others not.
    fn set_focused_frame(&mut self, frame: u32, docs: &mut dyn LentDocuments);

    /// The frame that held the page's focus is gone: the focus falls to its
    /// nearest ancestor, and a top level gets no focus (measured).
    fn focus_after_disconnect(&mut self, docs: &mut dyn LentDocuments);

    /// Brings the viewport of the frame up to date: the layout of the
    /// documents above it, top first, then each frame's viewport from its
    /// container's box.
    fn update_viewports(&mut self, frame: u32, docs: &mut dyn LentDocuments);
}
