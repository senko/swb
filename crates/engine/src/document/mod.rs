//! The state of one document (ADR 0028 section 2): the DOM and its parser,
//! the stylesheets and the stylist, the styles, the layout and the display
//! list, the subresources (images, web fonts), the form controls, the
//! scroll offsets, the selection and the element states.
//!
//! [`crate::Page`] owns it in a box. A script runtime will hold the box for
//! the length of a task (feature 4c), so `DocumentState` is `'static` and
//! does not borrow from the page: it shares the page's loader through an
//! `Rc`, keeps a copy of the viewport, and the methods that lay out or
//! paint take the font context as an argument.
//!
//! Changes of the DOM go through the change functions of `swb_dom`, which
//! record them in the document's journal. [`DocumentState::process_changes`]
//! drains the journal (`changes.rs`): it starts the loads of inserted
//! elements, drops the sheets of removed `style` and `link` elements, and
//! marks the style or the stylist dirty. Style and layout run on demand,
//! when a frame or a query needs them (`pipeline.rs`). Data derived from the
//! DOM records the DOM generation it was built for (`derived.rs`).
//!
//! Files: `changes.rs` (the journal drain and resource discovery),
//! `derived.rs` (generation-keyed caches), `loads.rs` (subresource loads
//! and their completions, image sources, web fonts), `scripts.rs` (the
//! fetches of the scripts of script elements, the style sheets that block
//! scripts), `pipeline.rs` (style, layout, display list, raster),
//! `queries.rs` (geometry, scrolling, hit testing, rendered text),
//! `resolved.rs` (`getComputedStyle` and the layout flush of a script
//! task), `cssom.rs` (the style sheets of scripts: `document.styleSheets`,
//! `sheet`), `frames.rs` (the `iframe` changes for the page's frame tree,
//! the frame viewports, the declarative refresh, the requests of
//! subresources with the document's loader client).

mod api_fetches;
mod changes;
mod cssom;
mod derived;
mod element_events;
pub(crate) mod frames;
mod geometry;
mod history_api;
mod inner_text;
mod lazy;
mod loads;
mod pipeline;
mod queries;
mod resolved;
mod script_actions;
mod script_forms;
mod script_validity;
mod scripts;
#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use encoding_rs::Encoding;
use swb_dom::{Document, HtmlParser, NodeId, ParseStep, TextPosition, WrittenStep};
use swb_layout::{FragmentTree, Point, Size};
use swb_net::{ByteBudget, ClientId, Loader, Origin, Url};
use swb_paint::DisplayList;
use swb_style::{ElementStates, StyleMap, Stylist};

use api_fetches::ApiFetches;

use crate::forms::Forms;
use crate::page::{FlushCounters, StageTimings};
use crate::resources::{Images, Requests, ScriptFetches, Sheets};
use crate::scrollers::Scrollers;
use crate::selection::{Selection, TreeOrder};
use crate::web_fonts::WebFonts;

pub(crate) use derived::Derived;
pub(crate) use history_api::{HistoryInfo, StateChange};
pub(crate) use pipeline::Rendering;
pub use queries::ElementScroll;
pub(crate) use queries::{indicated, rendered_test};
pub(crate) use script_actions::{MAX_PENDING_TRAVERSALS, ScriptNavigation, ScriptRequests};

/// What the document knows of its viewport: the page sets it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    /// The viewport size in CSS px.
    pub(crate) viewport: Size,
    /// The device pixel ratio.
    pub(crate) scale: f32,
    /// True to draw overlay scroll indicators (the GUI).
    pub(crate) indicators: bool,
    /// `prefers-color-scheme` (light unless an embedder sets it).
    pub(crate) color_scheme: swb_css::ColorScheme,
}

/// Where to scroll after each layout until the document is loaded.
#[derive(Clone, Debug)]
pub(crate) enum ScrollTarget {
    Fragment(String),
    Position(Point),
}

/// What a new document starts with.
pub(crate) struct DocumentInit {
    /// The DOM: complete (parsed without pauses), or new and empty with
    /// `parser` set.
    pub(crate) dom: Document,
    /// The parser of a parse in steps (JavaScript on).
    pub(crate) parser: Option<HtmlParser>,
    /// The document URL.
    pub(crate) url: Url,
    /// The character encoding (forms submit in it).
    pub(crate) encoding: &'static Encoding,
    /// The content type (`text/html`; `text/plain` and `image/png` for the
    /// generated documents of other types).
    pub(crate) content_type: String,
    /// Images that are already loaded (an image document).
    pub(crate) images: Images,
    /// The page's loader.
    pub(crate) loader: Rc<Loader>,
    /// The page's Web Storage.
    pub(crate) storage: swb_web::SharedStorage,
    /// The viewport.
    pub(crate) view: View,
    /// Where to scroll after the first layout.
    pub(crate) scroll_target: Option<ScrollTarget>,
    /// The time of the parse of a complete DOM (a parse in steps measures
    /// itself).
    pub(crate) parse: std::time::Duration,
    /// The loader client of the document (ADR 0031 section 3).
    pub(crate) client: ClientId,
    /// The origin when it is not the URL's: the parent's for `about:blank`
    /// and `about:srcdoc` frame documents, an opaque one in a sandbox.
    pub(crate) origin: Option<Origin>,
    /// The site for cookies of a frame document's requests; `None` for the
    /// top-level document.
    pub(crate) site_for_cookies: Option<Origin>,
    /// True for a frame document (not the top-level document).
    pub(crate) frame: bool,
    /// The fallback base URL of an `about:blank` or `about:srcdoc` frame
    /// document: the base URL of its creator (HTML "fallback base URL").
    /// `None`: the document URL.
    pub(crate) fallback_base: Option<Url>,
    /// The URL of the cookies and the storage of an `about:blank` or
    /// `about:srcdoc` frame document with its creator's origin: its creator's
    /// (measured, case `scripted-about-frame-state`). `None`: the document URL.
    pub(crate) creator_url: Option<Url>,
    /// The page's budget for the style sheets of frame documents; `None`
    /// for the top-level document.
    pub(crate) style_budget: Option<Arc<ByteBudget>>,
    /// The page's budget for the response bodies that the workers read for
    /// frame documents and the page has not taken yet; `None` for the
    /// top-level document.
    pub(crate) body_budget: Option<Arc<ByteBudget>>,
    /// The page's budget for the fonts of frame documents; `None` for the
    /// top-level document.
    pub(crate) font_budget: Option<Arc<ByteBudget>>,
    /// The page's font context while a task runs.
    pub(crate) fonts: Rc<crate::frames::registry::LentFonts>,
    /// The nodes that the budget for frame documents has room for; `None`
    /// for the top-level document.
    pub(crate) node_room: Option<Rc<std::cell::Cell<usize>>>,
}

/// The state of one document. See the module documentation.
pub struct DocumentState {
    pub(crate) dom: Document,
    /// The parser while a parse in steps goes on.
    pub(crate) parser: Option<HtmlParser>,
    pub(crate) loader: Rc<Loader>,
    /// The loader client of the document: its requests carry it, and the
    /// page hands their completions to this document.
    pub(crate) client: ClientId,
    /// The origin of the document if it is not the origin of its URL
    /// ([`DocumentInit::origin`]).
    origin_override: Option<Origin>,
    /// [`DocumentInit::creator_url`].
    creator_url: Option<Url>,
    /// The site for cookies of the document's requests (frames).
    pub(crate) site_for_cookies: Option<Origin>,
    /// True for a frame document. Its `@font-face` faces are its own
    /// ([`DocumentState::with_own_fonts`]).
    pub(crate) frame: bool,
    /// True for a frame document whose sandbox has no `allow-same-origin`: its
    /// origin is opaque because of the sandbox, not because of its URL (`file:`,
    /// `data:`).
    pub(crate) sandboxed_origin: bool,
    /// The partition of the document's storage key (`FirstParty` for the top-level
    /// document).
    pub(crate) storage_partition: swb_web::StoragePartition,
    /// `document.referrer`: empty for the top-level document.
    pub(crate) referrer: String,
    /// The frame of the document ([`crate::FrameId::TOP`] for the top-level
    /// document).
    pub(crate) frame_id: crate::frames::FrameId,
    /// True when the document left its frame and stays only for the scripts
    /// that hold its nodes (ADR 0031 section 5): it starts nothing and has no
    /// layout.
    pub(crate) retired: bool,
    /// See [`DocumentInit::node_room`].
    node_room: Option<Rc<std::cell::Cell<usize>>>,
    /// The content box of each `iframe` element at a layout serial.
    frame_box_cache: Option<(u64, std::collections::HashMap<NodeId, Size>)>,
    /// The `iframe` elements that the drain saw change, for the page.
    pub(crate) frame_changes: frames::FrameChanges,
    /// The page's Web Storage (`localStorage`, `sessionStorage`).
    storage: swb_web::SharedStorage,
    /// The running requests of the document's subresources.
    pub(crate) requests: Requests,
    /// The fetches of the scripts of script elements: the queue beyond the
    /// limit and the arrived scripts that wait for the runtime.
    pub(crate) script_fetches: ScriptFetches,
    /// The `fetch()` calls of scripts (they do not delay the load event).
    pub(crate) api_fetches: ApiFetches,
    /// The `load` and `error` events of images and style sheets that wait
    /// for the script runtime.
    pub(crate) element_events: element_events::ElementEvents,
    /// True while the journal drain processes a step of the parser: the
    /// style sheets that it finds block scripts.
    parser_drain: bool,
    /// The document URL.
    pub(crate) url: Url,
    /// The URL that a `base` element and the base URL resolve against: the
    /// document URL, or the creator's base URL for `about:` documents.
    pub(crate) base_fallback: Url,
    /// See [`DocumentInit::style_budget`].
    pub(crate) style_budget: Option<Arc<ByteBudget>>,
    /// See [`DocumentInit::body_budget`].
    pub(crate) body_budget: Option<Arc<ByteBudget>>,
    pub(crate) encoding: &'static Encoding,
    /// The content type of the response (`document.contentType`).
    content_type: String,
    /// The document base URL; [`DocumentState::process_changes`] keeps it
    /// current.
    pub(crate) base_url: Derived<Url>,
    /// The document title; kept current as the base URL.
    title: Derived<String>,
    /// The state of the form controls; rebuilt by generation
    /// ([`DocumentState::refresh_forms`]).
    pub(crate) forms: Derived<Forms>,
    /// What scripts wrote to controls that have no state in `forms`.
    pub(crate) seeds: crate::forms::Seeds,
    /// The custom validity messages of the controls and the cache of
    /// `pattern` matches (constraint validation).
    pub(crate) validity: crate::forms::ValidityCx,
    /// True if a script changed the state of a control: the element
    /// states of the controls (`:checked`, `:valid`, ...) are outdated.
    pub(crate) controls_stale: bool,
    /// The bytes of the values that scripts wrote to text fields and text
    /// areas (in the states and in the seeds); a budget of the document.
    pub(crate) control_bytes: usize,
    /// The inputs whose `indeterminate` property a script set to true.
    pub(crate) indeterminate: std::collections::HashSet<NodeId>,
    /// The tree order of the nodes, for the selection; built when needed.
    tree_order: Derived<TreeOrder>,
    /// The author stylesheets, in tree order of their owner elements.
    pub(crate) sheets: Sheets,
    /// The estimated size of the style data that scripts made (the sheets of
    /// their `style` elements, their rules); bounded (ADR 0028 section 16).
    pub(crate) style_memory: swb_style::cssom::sheet::StyleMemory,
    /// The number of the last stylesheet load (each load has its own, so
    /// that a stale response is ignored).
    pub(crate) sheet_serial: u64,
    pub(crate) images: Images,
    /// The images whose load waits for the viewport (`loading=lazy`).
    pub(crate) lazy: lazy::LazyImages,
    /// The web fonts of the document (`@font-face`) and their files.
    pub(crate) web_fonts: WebFonts,
    pub(crate) view: View,
    /// The scroll position of the viewport in CSS px.
    pub(crate) scroll: Point,
    pub(crate) pending_scroll: Option<ScrollTarget>,
    /// The scroll containers and their scroll offsets.
    pub(crate) scrollers: Scrollers,
    /// The element states that selectors see (hover, focus, form control
    /// states, ...).
    pub(crate) states: ElementStates,
    /// True while the last input that moved the focus or pressed a key was
    /// the keyboard, or there was none: a script's `focus()` then matches
    /// `:focus-visible`. A mouse press that moves the focus to an element
    /// clears it, a key sets it (Chromium 148, measured in
    /// `tools/probes/web-small-gaps.json`, cases `fv-*`).
    pub(crate) keyboard_modality: bool,
    /// The `iframe` element that holds the focus through its frame
    /// (`document.activeElement`); the page sets it (ADR 0031 section 8).
    pub(crate) frame_focus: Option<NodeId>,
    /// True while the window does not have the focus: Tab left the page,
    /// or the frame that held the focus is gone; the page keeps it
    /// (`document.hasFocus()`).
    pub(crate) window_blurred: bool,
    /// The text selection of the page.
    pub(crate) selection: Option<Selection>,
    pub(crate) stylist: Option<Stylist>,
    pub(crate) styles: Option<StyleMap>,
    /// True if a script changed the focus and the styles that depend on
    /// `:focus` are outdated: the next style flush computes them again.
    pub(crate) styles_stale: bool,
    /// What scripts asked for during the task (navigation, scrolling).
    pub(crate) script_requests: ScriptRequests,
    /// The session history as scripts see it; the page keeps it current.
    pub(crate) history: HistoryInfo,
    /// How the document was reached and what the response was, for the
    /// navigation entry of the performance timeline (feature 8a2).
    pub(crate) navigation: swb_web::NavigationInfo,
    /// The text control that the user edited since its last `change` event
    /// (or since it got the focus), with its value before the first edit.
    /// `change` fires when the control commits, if the value differs.
    pub(crate) commit_baseline: Option<(NodeId, String)>,
    pub(crate) fragments: Option<FragmentTree>,
    /// Counts the layouts: a cache of data derived from the fragments is
    /// valid while this stays the same.
    pub(crate) layout_serial: u64,
    pub(crate) display_list: Option<DisplayList>,
    /// [`swb_paint::transform_ends`] of `display_list`, for a frame document
    /// (the rasterizer of its parent uses it; the top-level document's list
    /// gets it in each rasterization).
    pub(crate) list_ends: Vec<usize>,
    /// The page's supported color schemes (the `color-scheme` meta element)
    /// at a DOM generation, for a frame document.
    page_color_schemes: Option<(u64, Option<Arc<str>>)>,
    pub(crate) timings: StageTimings,
    /// The style and layout runs of the document.
    pub(crate) flushes: FlushCounters,
    /// True while a query of a script flushes style or layout (the runs
    /// count as forced).
    pub(crate) in_query: bool,
    /// The styles of single elements and their ancestors, computed for
    /// queries while `styles` is `None` (ADR 0028 section 9); dropped with
    /// the styles and at each change of the element states.
    pub(crate) element_styles: std::collections::HashMap<NodeId, Arc<swb_style::ComputedStyle>>,
    /// The styles of pseudo-elements that `getComputedStyle` asked for
    /// (also those without a box); dropped with `element_styles`.
    pub(crate) pseudo_styles:
        std::collections::HashMap<(NodeId, swb_style::PseudoKind), Arc<swb_style::ComputedStyle>>,
    /// The used boxes of the layout with the serial, built at the first
    /// query of a resolved value that needs them (`resolved.rs`).
    pub(crate) used_boxes: Option<(u64, resolved::BoxIndex)>,
    /// The boxes of the elements of the layout with the serial, for the
    /// geometry of scripts (`geometry.rs`), built at the first query.
    pub(crate) geometry: Option<(u64, geometry::GeometryIndex)>,
    /// The client rectangles of the elements at a scroll state, built at
    /// the first query (`geometry.rs`).
    pub(crate) client_rects: Option<geometry::RectIndex>,
    /// `getBBox` and `getTotalLength` of the elements of one outer `<svg>`
    /// in the layout with the serial, built at the first query
    /// (`geometry.rs`).
    pub(crate) svg_geometry: Option<(u64, NodeId, geometry::SvgGeometry)>,
    /// The elements (the document for the viewport) that scrolled since
    /// the page last fired `scroll` events, in the order they scrolled.
    pub(crate) scrolled: Vec<NodeId>,
    /// The page's font context while a script task runs (ADR 0028 section 2):
    /// the layout flushes of queries use it. All documents of the page share
    /// this cell.
    pub(crate) lent_fonts: Rc<crate::frames::registry::LentFonts>,
    /// The DOM generation at the last drain of the journal.
    drained: Option<u64>,
    /// The loads that the document started, for the tests of resource
    /// discovery.
    #[cfg(test)]
    pub(crate) load_log: Vec<String>,
    /// The number of walks of the whole document (incomplete journals).
    #[cfg(test)]
    pub(crate) walks: usize,
}

impl DocumentState {
    /// Creates the state of a new document and processes its DOM: a
    /// complete DOM is walked once (its journal is incomplete). A parse in
    /// steps has not started: the script runtime drives it
    /// ([`swb_web::EngineHost::parse_step`]), or, without one, the page
    /// runs it to the end ([`DocumentState::run_parser`]). The
    /// subresources start to load.
    pub(crate) fn new(init: DocumentInit) -> DocumentState {
        let url = init.url;
        let base_fallback = init.fallback_base.unwrap_or_else(|| url.clone());
        let mut doc = DocumentState {
            dom: init.dom,
            parser: init.parser,
            loader: init.loader,
            client: init.client,
            origin_override: init.origin,
            creator_url: init.creator_url,
            site_for_cookies: init.site_for_cookies,
            frame: init.frame,
            sandboxed_origin: false,
            storage_partition: swb_web::StoragePartition::FirstParty,
            referrer: String::new(),
            frame_id: crate::frames::FrameId::TOP,
            retired: false,
            node_room: init.node_room,
            frame_box_cache: None,
            frame_changes: frames::FrameChanges::default(),
            storage: init.storage,
            requests: Requests::default(),
            script_fetches: ScriptFetches::default(),
            api_fetches: ApiFetches::default(),
            element_events: element_events::ElementEvents::default(),
            parser_drain: false,
            base_url: Derived::new(base_fallback.clone()),
            base_fallback,
            style_budget: init.style_budget,
            body_budget: init.body_budget,
            url,
            encoding: init.encoding,
            content_type: init.content_type,
            title: Derived::new(String::new()),
            forms: Derived::new(Forms::default()),
            seeds: crate::forms::Seeds::default(),
            validity: crate::forms::ValidityCx::default(),
            controls_stale: false,
            control_bytes: 0,
            indeterminate: std::collections::HashSet::new(),
            tree_order: Derived::new(TreeOrder::default()),
            sheets: Sheets::default(),
            style_memory: swb_style::cssom::sheet::StyleMemory::default(),
            sheet_serial: 0,
            images: init.images,
            lazy: lazy::LazyImages::default(),
            web_fonts: WebFonts::with_budget(init.font_budget),
            view: init.view,
            scroll: Point::default(),
            pending_scroll: init.scroll_target,
            scrollers: Scrollers::default(),
            states: ElementStates::default(),
            keyboard_modality: true,
            frame_focus: None,
            window_blurred: false,
            selection: None,
            stylist: None,
            styles: None,
            styles_stale: false,
            script_requests: ScriptRequests::default(),
            history: HistoryInfo::default(),
            navigation: swb_web::NavigationInfo::default(),
            commit_baseline: None,
            fragments: None,
            layout_serial: 0,
            display_list: None,
            list_ends: Vec::new(),
            page_color_schemes: None,
            timings: StageTimings {
                parse: init.parse,
                ..StageTimings::default()
            },
            flushes: FlushCounters::default(),
            in_query: false,
            element_styles: std::collections::HashMap::new(),
            pseudo_styles: std::collections::HashMap::new(),
            used_boxes: None,
            geometry: None,
            client_rects: None,
            svg_geometry: None,
            scrolled: Vec::new(),
            lent_fonts: init.fonts,
            drained: None,
            #[cfg(test)]
            load_log: Vec::new(),
            #[cfg(test)]
            walks: 0,
        };
        doc.sync();
        doc
    }
}

/// What the bindings read from the document (ADR 0028 section 2): the box
/// goes to the script runtime for a task.
impl swb_web::EngineHost for DocumentState {
    fn dom(&self) -> &Document {
        &self.dom
    }

    fn dom_mut(&mut self) -> &mut Document {
        &mut self.dom
    }

    fn url(&self) -> String {
        self.url.to_string()
    }

    fn origin_text(&self) -> String {
        self.origin_string()
    }

    fn base_url(&mut self) -> String {
        // A script may have changed a `base` element in this task.
        self.process_changes();
        self.base_url.value().to_string()
    }

    fn parse_url(&mut self, value: &str) -> Option<String> {
        self.process_changes();
        let url = self.base_url.value().join(value).ok()?;
        Some(url.to_string())
    }

    fn character_set(&self) -> &'static str {
        self.encoding.name()
    }

    fn content_type(&self) -> &str {
        &self.content_type
    }

    fn navigation_info(&self) -> swb_web::NavigationInfo {
        self.navigation
    }

    /// `document.cookie`: the jar of the page's fetcher, without the
    /// `HttpOnly` cookies. A page without a jar (a test fetcher) has none.
    fn cookie_string(&self) -> String {
        let Some(jar) = self.loader.cookie_jar() else {
            return String::new();
        };
        match &self.site_for_cookies {
            Some(site) => jar.script_cookie_string_in(self.cookie_url(), site),
            None => jar.script_cookie_string(self.cookie_url()),
        }
    }

    fn set_cookie(&mut self, line: &str) {
        let Some(jar) = self.loader.cookie_jar() else {
            return;
        };
        match &self.site_for_cookies {
            Some(site) => jar.set_script_cookie_in(self.cookie_url(), line, site),
            None => jar.set_script_cookie(self.cookie_url(), line),
        }
    }

    fn sandboxed_origin(&self) -> bool {
        self.sandboxed_origin
    }

    fn storage_url(&self) -> String {
        self.cookie_url().to_string()
    }

    fn storage_partition(&self) -> swb_web::StoragePartition {
        self.storage_partition.clone()
    }

    fn referrer(&self) -> String {
        self.referrer.clone()
    }

    fn storage(&self) -> Option<swb_web::SharedStorage> {
        Some(Rc::clone(&self.storage))
    }

    fn parse_step(&mut self) -> Option<(ParseStep, Option<TextPosition>)> {
        self.parse_step()
    }

    fn parser(&mut self) -> Option<&mut HtmlParser> {
        self.parser.as_mut()
    }

    fn parse_written_step(&mut self) -> Option<WrittenStep> {
        self.parse_written_step()
    }

    fn style_sheets_block_scripts(&mut self) -> bool {
        self.style_sheets_block_scripts()
    }

    fn fetch_script(&mut self, node: NodeId, url: &str, mode: swb_web::ScriptFetchMode) -> bool {
        self.fetch_script(node, url, mode)
    }

    fn fetch_module(&mut self, id: u64, url: &str, options: swb_web::ModuleFetch) -> bool {
        self.fetch_module(id, url, options)
    }

    fn delay_load_for_module(&mut self, id: u64) {
        self.delay_load_for_module(id);
    }

    fn start_api_fetch(&mut self, request: swb_net::Request) -> Option<u64> {
        self.start_api_fetch(request)
    }

    fn cancel_api_fetch(&mut self, id: u64) {
        self.cancel_api_fetch(id);
    }

    fn element_states(&mut self, controls: bool) -> ElementStates {
        if controls {
            self.process_changes();
            self.refresh_forms();
        }
        self.states.clone()
    }

    fn control_value(&mut self, node: NodeId) -> Option<String> {
        self.script_control_value(node)
    }

    fn control_validity(&mut self, node: NodeId) -> swb_web::ControlValidity {
        self.script_validity(node)
    }

    fn control_validation_message(&mut self, node: NodeId) -> String {
        self.script_validation_message(node)
    }

    fn set_custom_validity(&mut self, node: NodeId, message: &str) -> swb_web::ControlWrite {
        self.script_set_custom_validity(node, message)
    }

    fn control_is_invalid(&mut self, node: NodeId) -> bool {
        self.script_is_invalid(node)
    }

    fn form_validation_reported(&mut self, form: NodeId) {
        if self.forms.value_mut().note_failed_submission(form) {
            self.controls_stale = true;
        }
    }

    fn set_control_value(&mut self, node: NodeId, value: &str) -> swb_web::ControlWrite {
        self.script_set_control_value(node, value)
    }

    fn set_control_indeterminate(&mut self, node: NodeId, value: bool) {
        self.script_set_indeterminate(node, value);
    }

    fn copy_control_state(&mut self, from: NodeId, to: NodeId) -> swb_web::ControlWrite {
        self.script_copy_control_state(from, to)
    }

    fn take_control_snapshots(&mut self, root: NodeId) -> Vec<(NodeId, swb_web::ControlSnapshot)> {
        self.script_control_snapshots(root, true)
    }

    fn copy_control_snapshots(
        &mut self,
        root: NodeId,
        deep: bool,
    ) -> Vec<(NodeId, swb_web::ControlSnapshot)> {
        self.script_control_snapshots(root, deep)
    }

    fn put_control_snapshot(
        &mut self,
        node: NodeId,
        snapshot: swb_web::ControlSnapshot,
    ) -> swb_web::ControlWrite {
        self.script_put_control_snapshot(node, &snapshot)
    }

    fn control_selection(&mut self, node: NodeId) -> Option<swb_web::SelectionRange> {
        self.script_selection(node)
    }

    fn control_selection_key(&mut self, node: NodeId, sync: bool) -> Option<swb_web::SelectionKey> {
        self.script_selection_key(node, sync)
    }

    fn set_control_selection(
        &mut self,
        node: NodeId,
        start: u32,
        end: u32,
        backward: bool,
    ) -> bool {
        self.script_set_selection(node, start, end, backward)
    }

    fn select_control_text(&mut self, node: NodeId) -> bool {
        self.script_select_text(node)
    }

    fn set_control_range_text(
        &mut self,
        node: NodeId,
        replacement: &str,
        start: u32,
        end: u32,
        mode: swb_web::RangeTextMode,
    ) -> swb_web::ControlWrite {
        self.script_set_range_text(node, replacement, start, end, mode)
    }

    fn input_type_changed(&mut self, node: NodeId, old_value: Option<String>) {
        self.script_input_type_changed(node, old_value);
    }

    fn control_checked(&mut self, node: NodeId) -> Option<bool> {
        self.script_control_checked(node)
    }

    fn set_control_checked(&mut self, node: NodeId, checked: bool) -> bool {
        self.script_set_control_checked(node, checked)
    }

    fn selected_index(&mut self, select: NodeId) -> i32 {
        self.script_selected_index(select)
    }

    fn set_selected_index(&mut self, select: NodeId, index: i32) {
        self.script_set_selected_index(select, index);
    }

    fn option_selected(&mut self, option: NodeId) -> bool {
        self.script_option_selected(option)
    }

    fn set_option_selected(&mut self, option: NodeId, selected: bool) {
        self.script_set_option_selected(option, selected);
    }

    fn set_option_initial_selected(&mut self, option: NodeId, selected: bool) {
        self.script_set_option_initial_selected(option, selected);
    }

    fn option_selected_attribute_changed(&mut self, option: NodeId) {
        self.script_option_selected_attribute_changed(option);
    }

    fn parse_url_against_document_url(&mut self, value: &str) -> Option<String> {
        self.url.join(value).ok().map(|u| u.to_string())
    }

    fn scroll_offset(&self) -> (f64, f64) {
        self.script_scroll_offset()
    }

    fn focused_element(&self) -> Option<NodeId> {
        self.states.focus
    }

    fn frame_focus_element(&self) -> Option<NodeId> {
        self.frame_focus
    }

    fn window_has_focus(&self) -> bool {
        !self.window_blurred && !self.retired
    }

    fn can_focus(&mut self, node: NodeId) -> bool {
        self.script_can_focus(node)
    }

    fn set_focus(&mut self, node: Option<NodeId>, visible: bool) {
        self.script_set_focus(node, visible);
    }

    fn keyboard_modality(&self) -> bool {
        self.keyboard_modality
    }

    fn set_keyboard_modality(&mut self, keyboard: bool) {
        self.keyboard_modality = keyboard;
    }

    fn take_pending_change(&mut self, node: NodeId) -> bool {
        self.script_take_pending_change(node)
    }

    fn request_scroll_into_view(&mut self, node: NodeId) {
        self.script_request_scroll(node);
    }

    fn request_hyperlink(&mut self, node: NodeId) -> bool {
        self.script_request_hyperlink(node)
    }

    fn invalid_controls(&mut self, form: NodeId) -> Vec<NodeId> {
        self.script_invalid_controls(form)
    }

    fn request_form_submission(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        coordinate: (i32, i32),
        entries: Option<Vec<swb_web::FormDataEntry>>,
    ) -> bool {
        self.script_request_form_submission(form, submitter, coordinate, entries)
    }

    fn image_info(&mut self, node: NodeId) -> swb_web::ImageInfo {
        DocumentState::image_info(self, node)
    }

    fn form_entries(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        coordinate: (i32, i32),
    ) -> Vec<swb_web::FormDataEntry> {
        self.script_form_entries(form, submitter, coordinate)
    }

    fn reset_form(&mut self, form: NodeId) {
        self.script_reset_form(form);
    }

    fn checked_radio_in_group(&mut self, radio: NodeId) -> Option<NodeId> {
        self.script_checked_radio(radio)
    }

    fn computed_style(
        &mut self,
        element: NodeId,
        pseudo: Option<swb_style::cssom::resolved::PseudoElement>,
    ) -> Option<Arc<swb_style::ComputedStyle>> {
        if self.retired {
            return None;
        }
        self.query_computed_style(element, pseudo)
    }

    fn style_sheet_count(&mut self) -> usize {
        self.script_sheet_count()
    }

    fn style_sheet(&mut self, index: usize) -> Option<swb_style::cssom::sheet::SheetRef> {
        self.script_sheet(index)
    }

    fn style_sheet_of(&mut self, owner: NodeId) -> Option<swb_style::cssom::sheet::SheetRef> {
        self.script_sheet_of(owner)
    }

    fn settle_style_sheets(&mut self) {
        self.process_changes();
    }

    fn style_sheets_changed(&mut self) {
        self.flushes.sheet_changes = self.flushes.sheet_changes.saturating_add(1);
        self.invalidate_style();
    }

    fn resolved_value(
        &mut self,
        element: NodeId,
        pseudo: Option<swb_style::cssom::resolved::PseudoElement>,
        property: &str,
    ) -> String {
        if self.retired {
            return String::new();
        }
        self.resolved_value(element, pseudo, property)
    }

    fn client_rects(&mut self, element: NodeId) -> Vec<swb_web::DomRect> {
        self.script_client_rects(element)
    }

    fn offset_metrics(&mut self, element: NodeId) -> swb_web::OffsetMetrics {
        self.script_offset_metrics(element)
    }

    fn client_metrics(&mut self, element: NodeId) -> [i32; 4] {
        self.script_client_metrics(element)
    }

    fn scroll_metrics(&mut self, element: NodeId) -> swb_web::ScrollMetrics {
        self.script_scroll_metrics(element)
    }

    fn scroll_element(&mut self, element: NodeId, request: swb_web::ScrollRequest) {
        self.script_scroll_element(element, request);
    }

    fn scroll_position(&mut self) -> (f64, f64) {
        self.script_scroll_position()
    }

    fn scroll_viewport(&mut self, request: swb_web::ScrollRequest) {
        self.script_scroll_viewport(request);
    }

    fn scroll_into_view(
        &mut self,
        element: NodeId,
        block: swb_web::ScrollAlign,
        inline: swb_web::ScrollAlign,
    ) {
        self.script_scroll_into_view(element, block, inline);
    }

    fn scrolling_element(&mut self) -> Option<NodeId> {
        self.script_scrolling_element()
    }

    fn viewport(&self) -> swb_web::Viewport {
        if self.retired {
            // Measured, case `scripted-retired`.
            return swb_web::Viewport {
                width: 0.0,
                height: 0.0,
                scale: f64::from(self.view.scale),
            };
        }
        swb_web::Viewport {
            width: f64::from(self.view.viewport.width),
            height: f64::from(self.view.viewport.height),
            scale: f64::from(self.view.scale),
        }
    }

    fn elements_from_point(&mut self, x: f64, y: f64, all: bool) -> Vec<NodeId> {
        self.script_elements_from_point(x, y, all)
    }

    fn content_size(&mut self, element: NodeId) -> Option<(f64, f64)> {
        self.script_content_size(element)
    }

    fn observed_box(&mut self, element: NodeId) -> Option<swb_web::ObservedBox> {
        self.script_observed_box(element)
    }

    fn event_origins(&mut self, target: NodeId) -> Option<swb_web::EventOrigins> {
        self.script_event_origins(target)
    }

    fn inner_text(&mut self, element: NodeId) -> Option<String> {
        self.script_inner_text(element)
    }

    fn svg_geometry(&mut self, element: NodeId) -> Option<(swb_web::DomRect, f64)> {
        self.script_svg_geometry(element)
    }

    fn media_environment(&self) -> swb_css::MediaEnvironment {
        let mut environment = DocumentState::media_environment(self);
        if self.retired {
            environment.viewport_width = 0.0;
            environment.viewport_height = 0.0;
        }
        environment
    }

    fn history_length(&self) -> usize {
        self.history.length.max(1)
    }

    fn history_state(&self) -> Option<Rc<swb_web::Serialized>> {
        self.history.state.clone()
    }

    fn scroll_restoration_manual(&self) -> bool {
        self.history.manual_scroll
    }

    fn set_scroll_restoration(&mut self, manual: bool) {
        self.history.manual_scroll = manual;
    }

    fn push_state(&mut self, state: Rc<swb_web::Serialized>, url: Option<&str>, replace: bool) {
        self.script_push_state(state, url, replace);
    }

    fn request_navigation(&mut self, url: &str, replace: bool, caller: u32) -> bool {
        if self.retired {
            return false;
        }
        self.script_request_navigation(url, replace, caller)
    }

    fn request_reload(&mut self) {
        if !self.retired {
            self.script_requests.navigation = Some(ScriptNavigation::Reload);
        }
    }

    fn request_traversal(&mut self, delta: i32) {
        // Chromium drops a call whose target is out of range of the history
        // as it is at the call (`back()` after `forward()` is not undone).
        let target = i64::try_from(self.history.index).unwrap_or(i64::MAX) + i64::from(delta);
        let list = &mut self.script_requests.traversals;
        if list.len() >= MAX_PENDING_TRAVERSALS || (delta == 0 && list.contains(&0)) {
            return;
        }
        if delta == 0 || (0..i64::try_from(self.history.length).unwrap_or(0)).contains(&target) {
            list.push(delta);
        }
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }

    fn take_inserted_frames(&mut self) -> Vec<NodeId> {
        if self.frame_changes.rescan {
            return frames::iframes_in(&self.dom);
        }
        std::mem::take(&mut self.frame_changes.inserted)
    }

    fn retire(&mut self) {
        self.retire_document();
    }
}

impl DocumentState {
    /// Runs the parser to the end without scripts, if a parse in steps
    /// goes on: the document has no script runtime (or lost it).
    pub(crate) fn run_parser(&mut self) {
        while let Some((ParseStep::Script(_), _)) = self.parse_step() {}
        self.sync();
    }

    /// The document stays as it is and runs no scripts: the rest of its parse
    /// goes to the end at once.
    pub(crate) fn finish_parse_without_scripts(&mut self) {
        if self.is_parsing() {
            self.run_parser();
        }
    }

    /// See [`swb_web::EngineHost::retire`].
    fn retire_document(&mut self) {
        self.retired = true;
        self.parser = None;
        self.loader.cancel_client(self.client);
        self.requests = Requests::default();
        self.script_fetches = ScriptFetches::default();
        self.api_fetches = ApiFetches::default();
        self.element_events = element_events::ElementEvents::default();
        self.release_fonts_in_task();
        self.stylist = None;
        self.styles = None;
        self.fragments = None;
        self.display_list = None;
        self.list_ends = Vec::new();
        self.used_boxes = None;
        self.geometry = None;
        self.client_rects = None;
        self.svg_geometry = None;
        self.element_styles.clear();
        self.pseudo_styles.clear();
        self.images = Images::default();
        self.lazy = lazy::LazyImages::default();
        self.frame_changes = frames::FrameChanges::default();
        self.script_requests = ScriptRequests::default();
        self.scrolled = Vec::new();
    }

    /// Unloads the document's web font faces, while the page's font context
    /// is lent to a task. Outside a task the page does it with its context.
    pub(crate) fn release_fonts_in_task(&mut self) {
        let fonts = Rc::clone(&self.lent_fonts);
        if let Ok(mut fonts) = fonts.try_borrow_mut()
            && let Some(fonts) = fonts.as_mut()
        {
            self.release_own_fonts(fonts);
        }
    }

    /// The content box of the `iframe` element `node` in the current layout.
    pub(crate) fn frame_box(&mut self, node: NodeId) -> Option<Size> {
        let serial = self.layout_serial;
        if self
            .frame_box_cache
            .as_ref()
            .is_none_or(|(at, _)| *at != serial)
        {
            self.frame_box_cache = Some((serial, self.frame_boxes()));
        }
        self.frame_box_cache.as_ref()?.1.get(&node).copied()
    }

    /// The source of a retired document costs nothing more: the nodes that it
    /// holds, for the page's budget.
    pub(crate) fn node_count(&self) -> usize {
        self.dom.len()
    }

    /// One step of the parser (the script runtime calls it through
    /// [`swb_web::EngineHost::parse_step`]): the changes of the scripts
    /// before it are processed, then the parser runs to the next `script`
    /// end tag or to the end, and the changes of the step are processed
    /// (the style sheets that it finds block scripts). The parser goes away
    /// at the end. `None` without a parser.
    pub(crate) fn parse_step(&mut self) -> Option<(ParseStep, Option<TextPosition>)> {
        let mut parser = self.parser.take()?;
        self.process_changes();
        // A frame document's parser stops at the room that the page's budget
        // has left (ADR 0031 section 11).
        let nodes_before = self.dom.len();
        if let Some(room) = &self.node_room {
            self.dom
                .set_max_nodes(nodes_before.saturating_add(room.get()));
        }
        let started = Instant::now();
        let step = parser.resume(&mut self.dom);
        self.timings.parse += started.elapsed();
        if let Some(room) = &self.node_room {
            let added = self.dom.len().saturating_sub(nodes_before);
            room.set(room.get().saturating_sub(added));
            // An iframe srcdoc document is never in quirks mode (HTML "the
            // initial insertion mode").
            if self.url.as_str() == "about:srcdoc" {
                self.dom.quirks_mode = swb_dom::QuirksMode::NoQuirks;
            }
        }
        self.parser_drain = true;
        self.process_changes();
        self.parser_drain = false;
        match step {
            Ok(ParseStep::Script(node)) => {
                let position = (!parser.script_written()).then(|| parser.script_position());
                self.parser = Some(parser);
                Some((ParseStep::Script(node), position))
            }
            Ok(ParseStep::Finished) => Some((ParseStep::Finished, None)),
            Err(e) => {
                log::warn!("HTML parser: {e}");
                Some((ParseStep::Finished, None))
            }
        }
    }

    /// The text that `document.write` put into the parser, read up to the
    /// insertion point or the next `script` end tag (the script runtime
    /// calls it through [`swb_web::EngineHost::parse_written_step`]). The
    /// changes of the scripts before it are processed first, and those of
    /// the step after it (the style sheets that it finds block scripts, as
    /// in [`DocumentState::parse_step`]).
    pub(crate) fn parse_written_step(&mut self) -> Option<WrittenStep> {
        let mut parser = self.parser.take()?;
        self.process_changes();
        let started = Instant::now();
        let step = parser.resume_written(&mut self.dom);
        self.timings.parse += started.elapsed();
        self.parser_drain = true;
        self.process_changes();
        self.parser_drain = false;
        self.parser = Some(parser);
        match step {
            Ok(step) => Some(step),
            Err(e) => {
                log::warn!("HTML parser: {e}");
                None
            }
        }
    }

    /// True if the parser waits or runs (a parse in steps).
    pub(crate) fn is_parsing(&self) -> bool {
        self.parser.is_some()
    }

    // ----- The DOM and its changes -----

    /// The document.
    pub fn dom(&self) -> &Document {
        &self.dom
    }

    /// The document, for changes through the change functions of
    /// `swb_dom`. The changes take effect at the next
    /// [`process_changes`](Self::process_changes): the engine calls it after
    /// each task, and every flush (style, layout, a query) calls it first.
    pub fn dom_mut(&mut self) -> &mut Document {
        &mut self.dom
    }

    /// Changes the document with `change`, then processes the changes
    /// (loads, invalidation, derived data).
    pub fn change_dom<R>(&mut self, change: impl FnOnce(&mut Document) -> R) -> R {
        let result = change(&mut self.dom);
        self.sync();
        result
    }

    /// Processes the journal and brings the derived data up to date: the
    /// drain point after a task, a DOM change by the engine, or a parse.
    pub fn sync(&mut self) {
        self.process_changes();
        self.refresh_forms();
    }

    /// The state of the form controls, rebuilt if the DOM changed since
    /// they were built. A rebuild keeps the values that the user changed
    /// (the dirty value and checkedness flags).
    pub(crate) fn refresh_forms(&mut self) {
        if !self.ensure_forms() && self.controls_stale {
            // A script changed a control without a change of the DOM.
            self.controls_stale = false;
            let states = Arc::new(self.forms.value().element_states(
                &self.dom,
                &self.indeterminate,
                &self.validity,
            ));
            self.update_states(|s| s.controls = states);
            self.invalidate_layout();
        }
    }

    /// Rebuilds the state of the form controls if the DOM changed since
    /// they were built, and leaves the element states of script writes for
    /// [`DocumentState::refresh_forms`] (a loop of writes then does not
    /// compute them for each write). True if it rebuilt.
    pub(crate) fn ensure_forms(&mut self) -> bool {
        let generation = self.dom.generation();
        if self.forms.is_current(generation) {
            return false;
        }
        let dom = &self.dom;
        let seeds = &mut self.seeds;
        self.forms.rebuild(generation, |old| {
            let mut forms = Forms::rebuild(old, dom, seeds);
            forms.merge_seeds(old, dom, seeds);
            forms
        });
        self.controls_stale = false;
        self.control_bytes = self.forms.value().dirty_value_bytes() + self.seeds.value_bytes();
        self.states.controls = Arc::new(self.forms.value().element_states(
            dom,
            &self.indeterminate,
            &self.validity,
        ));
        true
    }

    /// The tree order of the document's nodes, rebuilt if the DOM changed.
    pub(crate) fn tree_order(&mut self) -> &TreeOrder {
        let dom = &self.dom;
        self.tree_order
            .get(dom.generation(), || TreeOrder::new(dom))
    }

    // ----- Accessors -----

    /// The document URL.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The document base URL.
    pub fn base_url(&self) -> &Url {
        self.base_url.value()
    }

    /// The document title (HTML `document.title`).
    pub fn title(&self) -> &str {
        self.title.value()
    }

    /// The computed styles, if they are up to date.
    pub fn styles(&self) -> Option<&StyleMap> {
        self.styles.as_ref()
    }

    /// The fragment tree, if the layout is up to date.
    pub fn fragments(&self) -> Option<&FragmentTree> {
        self.fragments.as_ref()
    }

    /// The scroll position of the viewport in CSS px.
    pub fn scroll_position(&self) -> Point {
        self.scroll
    }

    /// True while the parse or subresource loads go on (not the module
    /// fetches of `import()`, which do not delay the `load` event).
    pub fn is_loading(&self) -> bool {
        self.parser.is_some()
            || self.requests.pending.len() > self.script_fetches.quiet_in_flight
            || !self.script_fetches.is_empty()
    }

    /// True if the document was parsed with scripting on.
    pub fn scripting(&self) -> bool {
        self.dom.scripting()
    }

    /// The content type of the document.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// Sets the viewport and the scale. Image sources and media queries
    /// may change, so the styles are computed again.
    pub(crate) fn set_view(&mut self, view: View) {
        if view == self.view {
            return;
        }
        let restyle = view.viewport != self.view.viewport
            || view.scale != self.view.scale
            || view.color_scheme != self.view.color_scheme;
        self.view = view;
        if restyle {
            self.images.sources_outdated = true;
            self.invalidate_style();
        } else {
            self.display_list = None;
        }
    }

    /// The origin of the document: the initiator of its subresource
    /// requests and of the navigations it starts.
    pub(crate) fn origin(&self) -> Origin {
        match &self.origin_override {
            Some(origin) => origin.clone(),
            None => self.url.origin(),
        }
    }

    /// The URL of the document's cookies and storage (see
    /// [`DocumentInit::creator_url`]).
    pub(crate) fn cookie_url(&self) -> &Url {
        self.creator_url.as_ref().unwrap_or(&self.url)
    }

    /// The serialized origin of the document (`"null"` for an opaque one).
    pub fn origin_string(&self) -> String {
        self.origin().ascii_serialization()
    }

    /// The element under the pointer (`:hover`); the `iframe` for a
    /// document whose frame the pointer is in.
    pub fn hovered_element(&self) -> Option<NodeId> {
        self.states.hover
    }

    /// The element that is being pressed (`:active`).
    pub fn pressed_element(&self) -> Option<NodeId> {
        self.states.active
    }

    /// The focused element of the document (`:focus`).
    pub fn focus_element(&self) -> Option<NodeId> {
        self.states.focus
    }

    /// The `iframe` element that holds the page's focus through its frame
    /// (`document.activeElement` while no element of the document is
    /// focused; ADR 0031 section 8).
    pub fn focused_frame_element(&self) -> Option<NodeId> {
        self.frame_focus
    }

    /// True if the document may load `url`: only `file:` documents may
    /// load `file:` URLs, as in Chromium ("Not allowed to load local
    /// resource"). Addresses typed by the user are not restricted.
    pub(crate) fn may_load(&self, url: &Url) -> bool {
        crate::page::is_loadable(url) && (url.scheme() != "file" || self.url.scheme() == "file")
    }

    // ----- Element states -----

    /// Changes the element states with `change`. Restyles if a selector
    /// depends on a state that changed. Returns the old focus and whether
    /// a state changed that selectors depend on.
    pub(crate) fn update_states(
        &mut self,
        change: impl FnOnce(&mut ElementStates),
    ) -> (bool, Option<NodeId>) {
        let mut states = self.states.clone();
        change(&mut states);
        let changed = self.states.changes(&states);
        let old_focus = self.states.focus;
        self.states = states;
        if changed.is_empty() {
            return (false, old_focus);
        }
        self.clear_element_styles();
        let affected = self
            .stylist
            .as_ref()
            .is_none_or(|s| s.state_dependencies().intersects(changed));
        if affected {
            self.restyle();
        }
        (affected, old_focus)
    }

    /// Replaces the selection. Returns true if it changed.
    pub(crate) fn set_selection(&mut self, selection: Option<Selection>) -> bool {
        if selection == self.selection {
            return false;
        }
        let visible_before = self.visible_selection().is_some();
        self.selection = selection;
        if visible_before || self.visible_selection().is_some() {
            // The highlight is part of the display list.
            self.display_list = None;
        }
        true
    }

    /// The selection, if there is one and it is not empty.
    pub(crate) fn visible_selection(&self) -> Option<Selection> {
        self.selection.filter(|s| !s.is_collapsed())
    }
}
