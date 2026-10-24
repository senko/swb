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
//! and their completions, image sources, web fonts), `pipeline.rs` (style,
//! layout, display list, raster), `queries.rs` (geometry, scrolling, hit
//! testing, rendered text).

mod changes;
mod derived;
mod loads;
mod pipeline;
mod queries;
#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use encoding_rs::Encoding;
use swb_dom::{Document, HtmlParser, NodeId, ParseStep};
use swb_layout::{FragmentTree, Point, Size};
use swb_net::{Loader, Origin, Url};
use swb_paint::DisplayList;
use swb_style::{ElementStates, StyleMap, Stylist};

use crate::forms::Forms;
use crate::page::StageTimings;
use crate::resources::{Images, Requests, Sheets};
use crate::scrollers::Scrollers;
use crate::selection::{Selection, TreeOrder};
use crate::web_fonts::WebFonts;

pub(crate) use derived::Derived;
pub(crate) use pipeline::Rendering;
pub use queries::ElementScroll;
pub(crate) use queries::{indicated, is_rendered_in};

/// What the document knows of its viewport: the page sets it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    /// The viewport size in CSS px.
    pub(crate) viewport: Size,
    /// The device pixel ratio.
    pub(crate) scale: f32,
    /// True to draw overlay scroll indicators (the GUI).
    pub(crate) indicators: bool,
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
    /// The viewport.
    pub(crate) view: View,
    /// Where to scroll after the first layout.
    pub(crate) scroll_target: Option<ScrollTarget>,
    /// The time of the parse of a complete DOM (a parse in steps measures
    /// itself).
    pub(crate) parse: std::time::Duration,
}

/// The state of one document. See the module documentation.
pub struct DocumentState {
    pub(crate) dom: Document,
    /// The parser while a parse in steps goes on.
    parser: Option<HtmlParser>,
    pub(crate) loader: Rc<Loader>,
    /// The running requests of the document's subresources.
    pub(crate) requests: Requests,
    /// The document URL.
    pub(crate) url: Url,
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
    /// The tree order of the nodes, for the selection; built when needed.
    tree_order: Derived<TreeOrder>,
    /// The author stylesheets, in tree order of their owner elements.
    pub(crate) sheets: Sheets,
    /// The number of the last stylesheet load (each load has its own, so
    /// that a stale response is ignored).
    pub(crate) sheet_serial: u64,
    pub(crate) images: Images,
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
    /// The text selection of the page.
    pub(crate) selection: Option<Selection>,
    pub(crate) stylist: Option<Stylist>,
    pub(crate) styles: Option<StyleMap>,
    pub(crate) fragments: Option<FragmentTree>,
    pub(crate) display_list: Option<DisplayList>,
    pub(crate) timings: StageTimings,
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
    /// complete DOM is walked once (its journal is incomplete), a parse in
    /// steps runs to the end, and each step's changes are processed (no
    /// script runs yet). The subresources start to load.
    pub(crate) fn new(init: DocumentInit) -> DocumentState {
        let url = init.url;
        let mut doc = DocumentState {
            dom: init.dom,
            parser: init.parser,
            loader: init.loader,
            requests: Requests::default(),
            base_url: Derived::new(url.clone()),
            url,
            encoding: init.encoding,
            content_type: init.content_type,
            title: Derived::new(String::new()),
            forms: Derived::new(Forms::default()),
            tree_order: Derived::new(TreeOrder::default()),
            sheets: Sheets::default(),
            sheet_serial: 0,
            images: init.images,
            web_fonts: WebFonts::default(),
            view: init.view,
            scroll: Point::default(),
            pending_scroll: init.scroll_target,
            scrollers: Scrollers::default(),
            states: ElementStates::default(),
            selection: None,
            stylist: None,
            styles: None,
            fragments: None,
            display_list: None,
            timings: StageTimings {
                parse: init.parse,
                ..StageTimings::default()
            },
            drained: None,
            #[cfg(test)]
            load_log: Vec::new(),
            #[cfg(test)]
            walks: 0,
        };
        doc.run_parser();
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

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl DocumentState {
    /// Runs the parser to the end, if a parse in steps goes on, and
    /// processes the changes after each step. Feature 5c runs the scripts
    /// at the pauses.
    fn run_parser(&mut self) {
        let Some(mut parser) = self.parser.take() else {
            return;
        };
        let mut parse = std::time::Duration::ZERO;
        loop {
            let started = Instant::now();
            let step = parser.resume(&mut self.dom);
            parse += started.elapsed();
            self.process_changes();
            match step {
                Ok(ParseStep::Script(_)) => {}
                Ok(ParseStep::Finished) => break,
                Err(e) => {
                    log::warn!("HTML parser: {e}");
                    break;
                }
            }
        }
        self.timings.parse = parse;
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
        let generation = self.dom.generation();
        if self.forms.is_current(generation) {
            return;
        }
        let dom = &self.dom;
        self.forms
            .rebuild(generation, |old| Forms::rebuild(old, dom));
        self.states.controls = Arc::new(self.forms.value().element_states(dom));
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

    /// True while the parse or subresource loads go on.
    pub fn is_loading(&self) -> bool {
        self.parser.is_some() || !self.requests.is_empty()
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
        let restyle = view.viewport != self.view.viewport || view.scale != self.view.scale;
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
        self.url.origin()
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
