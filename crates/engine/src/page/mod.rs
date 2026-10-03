//! A page: one document in one viewport, with its loading state, rendering
//! pipeline, scroll position, input handling and session history.
//!
//! Navigation: [`Page::navigate`], [`Page::reload`] and history traversal
//! start a *pending navigation*. Starting one cancels all requests that
//! are still running. The old document stays visible until the new one
//! arrives; then the navigation commits: the history is updated and the
//! new document replaces the old one. A navigation that changes only the
//! fragment of the current document does not load anything; it scrolls.
//!
//! This file has the page state, navigation and the accessors. The other
//! parts of `Page` are in `loading.rs` (network completions, documents and
//! subresources), `pipeline.rs` (style, layout, display list, raster),
//! `scroll.rs` (scrolling and fragment targets) and `input.rs` (pointer,
//! keyboard, focus, selection).

mod input;
mod loading;
mod pipeline;
mod scroll;

use std::sync::Arc;
use std::time::Duration;

use swb_dom::{Document, NodeId};
use swb_layout::{FragmentTree, Point, Size};
use swb_net::{CookieJar, Destination, Fetcher, Loader, Origin, Request, Url};
use swb_paint::DisplayList;
use swb_style::{StyleMap, Stylist};
use swb_text::FontContext;

use crate::history::History;
use crate::resources::{Images, Pending, Requests, SheetSlot};
use crate::selection::TreeOrder;

pub use pipeline::{
    MAX_SCALE, MAX_SCREENSHOT_PIXELS, MAX_VIEWPORT_SIDE, ScreenshotError, ViewportError,
    check_scale, check_viewport_size, device_size,
};

/// The loading state of a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// Nothing loaded yet.
    Idle,
    /// The document is loading.
    LoadingDocument,
    /// The document is parsed; subresources are loading.
    LoadingResources,
    /// Everything is loaded (or loading was stopped).
    Complete,
    /// The navigation failed.
    Failed,
}

/// The duration of each pipeline stage, the last time it ran.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StageTimings {
    /// Parsing the HTML document.
    pub parse: Duration,
    /// Parsing the stylesheets and building the rule index.
    pub stylesheets: Duration,
    /// The cascade: the computed styles of all elements.
    pub style: Duration,
    /// Box tree construction and layout.
    pub layout: Duration,
    /// Building the display list.
    pub display_list: Duration,
    /// Rasterizing the viewport.
    pub raster: Duration,
}

/// Configuration of a page.
pub struct PageConfig {
    /// Fetches resources.
    pub fetcher: Arc<dyn Fetcher>,
    /// Called from a network thread when a response arrives. The GUI uses it
    /// to wake its event loop.
    pub notify: Arc<dyn Fn() + Send + Sync>,
    /// Number of network worker threads.
    pub network_threads: usize,
}

impl PageConfig {
    /// The number of network worker threads that the browser uses.
    pub const DEFAULT_NETWORK_THREADS: usize = 6;
}

/// How a navigation changes the session history when it commits.
#[derive(Clone, Copy, Debug)]
enum HistoryHandling {
    /// Add an entry (or replace the current one if the URL is the same).
    Push,
    /// Replace the current entry (reload).
    Replace,
    /// Make the entry at this index current (back and forward).
    Traverse(usize),
}

/// A navigation whose document is loading.
#[derive(Clone, Copy, Debug)]
struct PendingNavigation {
    handling: HistoryHandling,
    /// The scroll position to restore after the first layout (reload and
    /// history traversal). Without one, the page scrolls to the fragment
    /// of the document URL.
    restore_scroll: Option<Point>,
}

/// Where to scroll after each layout until the page is loaded.
#[derive(Clone, Debug)]
enum ScrollTarget {
    Fragment(String),
    Position(Point),
}

/// A page.
pub struct Page {
    loader: Loader,
    fonts: FontContext,
    history: History,
    pending: Option<PendingNavigation>,
    requests: Requests,
    state: LoadState,
    error: Option<String>,

    /// The URL shown to the user: the pending navigation's URL while it
    /// loads, otherwise the document URL.
    url: Option<Url>,
    document_url: Option<Url>,
    base_url: Option<Url>,
    document: Option<Document>,
    title: String,
    sheets: Vec<SheetSlot>,
    images: Images,

    viewport: Size,
    scale: f32,
    scroll: Point,
    pending_scroll: Option<ScrollTarget>,

    stylist: Option<Stylist>,
    styles: Option<StyleMap>,
    fragments: Option<FragmentTree>,
    display_list: Option<DisplayList>,
    /// The tree order of the document's nodes, for the selection.
    tree_order: TreeOrder,
    input: input::InputState,
    timings: StageTimings,
}

impl Page {
    /// Creates an empty page.
    pub fn new(config: PageConfig, fonts: FontContext, viewport: Size, scale: f32) -> Self {
        Page {
            loader: Loader::new(config.fetcher, config.network_threads, config.notify),
            fonts,
            history: History::default(),
            pending: None,
            requests: Requests::default(),
            state: LoadState::Idle,
            error: None,
            url: None,
            document_url: None,
            base_url: None,
            document: None,
            title: String::new(),
            sheets: Vec::new(),
            images: Images::default(),
            viewport,
            scale,
            scroll: Point::default(),
            pending_scroll: None,
            stylist: None,
            styles: None,
            fragments: None,
            display_list: None,
            tree_order: TreeOrder::default(),
            input: input::InputState::default(),
            timings: StageTimings::default(),
        }
    }

    // ----- Navigation -----

    /// Navigates to `url`, as if the user typed it (the request has no
    /// initiator). The new history entry is added when the document
    /// arrives.
    pub fn navigate(&mut self, url: Url) {
        self.start_navigation(url, HistoryHandling::Push, None, None);
    }

    /// Follows a link. A link to a fragment of the current document scrolls
    /// to it instead of loading. Links with a scheme that cannot be loaded
    /// (`javascript:`, `mailto:` and others) are ignored. Returns true if a
    /// navigation started or the page scrolled.
    pub fn follow_link(&mut self, link: Url) -> bool {
        if !is_loadable(&link) {
            log::info!("ignoring a link with the scheme {}:", link.scheme());
            return false;
        }
        if !self.may_load_subresource(&link) {
            log::warn!("not allowed to load local resource {link}");
            return false;
        }
        if let Some(fragment) = link.fragment()
            && self.pending.is_none()
            && self.is_same_document(&link)
        {
            let fragment = fragment.to_owned();
            self.save_scroll();
            self.history.push(link.clone());
            let target = self.set_target(Some(&fragment));
            self.set_document_url(link);
            self.scroll_to_fragment(&fragment);
            self.focus_fragment_target(target);
            return true;
        }
        let initiator = self.document_origin();
        self.start_navigation(link, HistoryHandling::Push, None, initiator);
        true
    }

    /// Reloads the current page and keeps the scroll position. While a
    /// navigation is pending, restarts that navigation instead.
    pub fn reload(&mut self) {
        if let Some(pending) = self.pending
            && let Some(url) = self.url.clone()
        {
            self.start_navigation(url, pending.handling, pending.restore_scroll, None);
            return;
        }
        if let Some(entry) = self.history.current() {
            let url = entry.url.clone();
            self.start_navigation(url, HistoryHandling::Replace, Some(self.scroll), None);
        }
    }

    /// Stops loading: cancels the pending navigation and the subresource
    /// requests. The current document stays; stylesheets that did not
    /// arrive are skipped.
    pub fn stop(&mut self) {
        if !self.is_loading() {
            return;
        }
        self.loader.cancel_all();
        self.requests = Requests::default();
        self.pending = None;
        self.url.clone_from(&self.document_url);
        let mut sheets_dropped = false;
        for sheet in &mut self.sheets {
            if !sheet.is_settled() {
                sheet.failed = true;
                sheets_dropped = true;
            }
        }
        if sheets_dropped {
            self.invalidate_style();
        }
        self.state = if self.document.is_some() {
            LoadState::Complete
        } else {
            LoadState::Idle
        };
    }

    /// Goes back in the session history. Returns false if there is no
    /// previous entry.
    pub fn go_back(&mut self) -> bool {
        self.traverse(-1)
    }

    /// Goes forward in the session history. Returns false if there is no
    /// next entry.
    pub fn go_forward(&mut self) -> bool {
        self.traverse(1)
    }

    /// True if there is a previous history entry.
    pub fn can_go_back(&self) -> bool {
        self.history_index() > 0
    }

    /// True if there is a next history entry.
    pub fn can_go_forward(&self) -> bool {
        self.history_index() + 1 < self.history.len()
    }

    /// The history index that back and forward start from: the target of a
    /// pending traversal, otherwise the current entry.
    fn history_index(&self) -> usize {
        match self.pending {
            Some(PendingNavigation {
                handling: HistoryHandling::Traverse(index),
                ..
            }) => index,
            _ => self.history.index(),
        }
    }

    fn traverse(&mut self, delta: isize) -> bool {
        let Some(target) = self.history_index().checked_add_signed(delta) else {
            return false;
        };
        let Some(entry) = self.history.get(target).cloned() else {
            return false;
        };
        if self.pending.is_none() && self.is_same_document(&entry.url) {
            // Only the fragment differs: restore the entry's scroll
            // position without loading.
            self.save_scroll();
            self.history.go_to(target);
            let fragment_target = self.set_target(entry.url.fragment());
            self.set_document_url(entry.url);
            self.scroll_to(entry.scroll);
            self.focus_fragment_target(fragment_target);
            return true;
        }
        self.start_navigation(
            entry.url,
            HistoryHandling::Traverse(target),
            Some(entry.scroll),
            None,
        );
        true
    }

    /// Starts loading a document. Cancels all requests that are still
    /// running: those of a previous pending navigation and those of the
    /// current document's subresources.
    ///
    /// `initiator` is the origin of the document that started the
    /// navigation (a link), or `None` if the user started it (address bar,
    /// reload, back and forward). Cookies use it for `SameSite`
    /// (ADR 0012).
    fn start_navigation(
        &mut self,
        url: Url,
        handling: HistoryHandling,
        restore_scroll: Option<Point>,
        initiator: Option<Origin>,
    ) {
        self.loader.cancel_all();
        self.requests = Requests::default();
        self.pending = Some(PendingNavigation {
            handling,
            restore_scroll,
        });
        self.state = LoadState::LoadingDocument;
        self.error = None;
        let id = self
            .loader
            .start(Request::get(url.clone(), Destination::Document).with_initiator(initiator));
        self.requests.pending.insert(id, Pending::Document);
        log::info!("navigating to {url}");
        // Keep showing the old page until the new document arrives, but
        // show the new URL in the address bar.
        self.url = Some(url);
    }

    /// Updates the session history for the document that just arrived,
    /// and decides where to scroll after the first layout.
    fn commit_navigation(&mut self, url: &Url) {
        self.save_scroll();
        let pending = self.pending.take();
        match pending.map(|p| p.handling) {
            Some(HistoryHandling::Push) | None => self.history.push(url.clone()),
            Some(HistoryHandling::Replace) => self.history.replace_current(url.clone()),
            Some(HistoryHandling::Traverse(index)) => {
                self.history.go_to(index);
                self.history.replace_current(url.clone());
            }
        }
        self.pending_scroll = match pending.and_then(|p| p.restore_scroll) {
            Some(position) => Some(ScrollTarget::Position(position)),
            None => url.fragment().map(|f| ScrollTarget::Fragment(f.to_owned())),
        };
    }

    /// Stores the scroll position in the current history entry.
    fn save_scroll(&mut self) {
        let scroll = self.scroll;
        if let Some(entry) = self.history.current_mut() {
            entry.scroll = scroll;
        }
    }

    /// True if `url` is the URL of the current document, ignoring the
    /// fragment.
    fn is_same_document(&self, url: &Url) -> bool {
        self.document.is_some()
            && self
                .document_url
                .as_ref()
                .is_some_and(|current| strip_fragment(current) == strip_fragment(url))
    }

    fn set_document_url(&mut self, url: Url) {
        self.url = Some(url.clone());
        self.document_url = Some(url);
    }

    /// The origin of the current document: the initiator of its
    /// subresource requests and of the navigations it starts.
    fn document_origin(&self) -> Option<Origin> {
        self.document_url.as_ref().map(Url::origin)
    }

    // ----- Accessors -----

    /// The URL to show to the user: the URL being loaded, or the document
    /// URL.
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }

    /// The document title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The loading state.
    pub fn load_state(&self) -> LoadState {
        self.state
    }

    /// The error of a failed navigation.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The cookie jar of the page's fetcher, if it has one (a fetcher that
    /// replays a fixture has none).
    pub fn cookie_jar(&self) -> Option<&CookieJar> {
        self.loader.cookie_jar()
    }

    /// True while the document or subresources are loading.
    pub fn is_loading(&self) -> bool {
        matches!(
            self.state,
            LoadState::LoadingDocument | LoadState::LoadingResources
        )
    }

    /// The document, if one is loaded.
    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }

    /// The computed styles (after [`Page::update_layout`]).
    pub fn styles(&self) -> Option<&StyleMap> {
        self.styles.as_ref()
    }

    /// The fragment tree (after [`Page::update_layout`]).
    pub fn fragments(&self) -> Option<&FragmentTree> {
        self.fragments.as_ref()
    }

    /// The elements that match a selector list, in tree order (as
    /// `querySelectorAll`). `None` if the selector list is invalid or there
    /// is no document.
    pub fn query_selector_all(&self, selectors: &str) -> Option<Vec<NodeId>> {
        swb_style::query_selector_all(self.document.as_ref()?, selectors, &self.input.states)
    }

    /// The union of the border boxes of an element in document coordinates
    /// (CSS px), if it has a box.
    pub fn element_box(&mut self, node: NodeId) -> Option<swb_layout::Rect> {
        self.update_layout();
        self.fragments.as_ref()?.element_boxes().get(&node).copied()
    }

    /// The font context.
    pub fn fonts(&mut self) -> &mut FontContext {
        &mut self.fonts
    }

    /// The viewport size in CSS px.
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// The device pixel ratio.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The scroll position in CSS px.
    pub fn scroll_position(&self) -> Point {
        self.scroll
    }
}

/// True for URL schemes that the browser can load.
fn is_loadable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "file" | "data" | "about")
}

fn about_blank() -> Url {
    Url::parse("about:blank").expect("about:blank is a valid URL")
}

fn strip_fragment(url: &Url) -> Url {
    let mut u = url.clone();
    u.set_fragment(None);
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loadable_schemes() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(is_loadable(&u("https://a.test/")));
        assert!(is_loadable(&u("data:text/html,x")));
        assert!(!is_loadable(&u("javascript:void(0)")));
        assert!(!is_loadable(&u("mailto:a@b.test")));
    }
}
