//! What scripts need from the engine (ADR 0028 section 1): the trait
//! [`EngineHost`], which the engine implements for its document state.
//!
//! `web` does not depend on `engine`. The engine lends its boxed document
//! state to the runtime for each task ([`crate::ScriptRuntime::lend`]) as a
//! `Box<dyn EngineHost>` and takes it back afterwards
//! ([`crate::ScriptRuntime::take_back`]); [`EngineHost::into_any`] turns the
//! box back into the engine's type. The style and layout queries of scripts
//! (`getComputedStyle`, feature 7) are here too.

use std::any::Any;
use std::sync::Arc;

use swb_dom::{Document, NodeId, ParseStep, TextPosition};
use swb_style::cssom::resolved::PseudoElement;
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

/// The document of a task, as the engine lends it to the bindings.
pub trait EngineHost: Any {
    /// The user agent string (`navigator.userAgent`, ADR 0025): the one
    /// that requests send.
    fn user_agent(&self) -> &'static str {
        "Mozilla/5.0 (X11; Linux x86_64) swb"
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

    // ----- Geometry and scrolling (CSSOM View, feature 7b) -----
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

    // ----- Rendered text and media (feature 7b2) -----

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

    // ----- Location and the session history (feature 8a1) -----

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
    /// `hashchange`.
    fn request_navigation(&mut self, url: &str, replace: bool) -> bool {
        let _ = (url, replace);
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
}
