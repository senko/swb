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
//! A navigation can send a request with a body (a form submitted with
//! `POST`). Its history entry keeps the body: a reload sends it again, but
//! going back or forward to the entry shows a page that asks for a reload
//! instead of sending it silently (ADR 0013).
//!
//! This file has the page state, navigation and the accessors. The other
//! parts of `Page` are in `loading.rs` (network completions, documents and
//! subresources), `pipeline.rs` (style, layout, display list, raster),
//! `scroll.rs` (scrolling of the viewport and of scroll containers, and
//! fragment targets), `input.rs` (pointer,
//! keyboard, focus, selection) and `forms.rs` (form controls: editing,
//! activation, submission).

mod forms;
mod input;
mod loading;
mod pipeline;
mod scroll;

use std::sync::Arc;
use std::time::Duration;

use encoding_rs::Encoding;
use swb_dom::{Document, NodeId};
use swb_layout::{FragmentTree, Point, Size};
use swb_net::{CookieJar, Destination, Fetcher, Loader, Method, Origin, Request, Url};
use swb_paint::DisplayList;
use swb_style::{StyleMap, Stylist};
use swb_text::FontContext;

use crate::forms::Forms;
use crate::history::{Commit, Entry, History, PostData};
use crate::resources::{Images, Pending, Requests, SheetSlot};
use crate::scrollers::Scrollers;
use crate::selection::TreeOrder;

pub use pipeline::{
    MAX_SCALE, MAX_SCREENSHOT_PIXELS, MAX_VIEWPORT_SIDE, ScreenshotError, ViewportError,
    check_scale, check_viewport_size, device_size,
};
pub use scroll::ElementScroll;

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
#[derive(Clone, Debug)]
struct PendingNavigation {
    handling: HistoryHandling,
    /// The scroll position to restore after the first layout (reload and
    /// history traversal). Without one, the page scrolls to the fragment
    /// of the document URL.
    restore_scroll: Option<Point>,
    /// The URL of the request.
    url: Url,
    /// The body of a `POST` request; the history entry keeps it.
    post: Option<PostData>,
    /// The origin of the document that started the navigation; the
    /// history entry keeps it.
    initiator: Option<Origin>,
    /// True if the document arrived after a redirect.
    redirected: bool,
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
    /// The character encoding of the document (forms submit in it).
    encoding: &'static Encoding,
    /// The number of the current document: it grows with every committed
    /// document. History entries of the same document can be traversed
    /// without loading.
    document_number: u64,
    /// The state of the document's form controls.
    forms: Forms,
    title: String,
    sheets: Vec<SheetSlot>,
    images: Images,

    viewport: Size,
    scale: f32,
    scroll: Point,
    pending_scroll: Option<ScrollTarget>,
    /// The scroll containers of the document and their scroll offsets.
    scrollers: Scrollers,
    /// True to draw overlay scroll indicators (the GUI).
    scroll_indicators: bool,

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
            encoding: encoding_rs::UTF_8,
            document_number: 0,
            forms: Forms::default(),
            title: String::new(),
            sheets: Vec::new(),
            images: Images::default(),
            viewport,
            scale,
            scroll: Point::default(),
            pending_scroll: None,
            scrollers: Scrollers::default(),
            scroll_indicators: false,
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
        self.start_navigation(url, None, HistoryHandling::Push, None, None);
    }

    /// Navigates with a request: a `GET`, or a `POST` with a body (form
    /// submission). The request's initiator decides the `SameSite` context
    /// (ADR 0012). Other headers of the request are not kept in the
    /// history; a reload sends the URL and the body again.
    pub fn navigate_with(&mut self, request: Request) {
        let post = post_data(&request);
        let initiator = request.initiator.clone();
        self.start_navigation(request.url, post, HistoryHandling::Push, None, initiator);
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
            // The entry shows the same document: it keeps the POST body and
            // the initiator of the document's entry, because a reload of it
            // requests that document again.
            let current = self.history.current().map(|e| &e.commit);
            self.history.push(Commit {
                url: link.clone(),
                post: current.and_then(|c| c.post.clone()),
                initiator: current.and_then(|c| c.initiator.clone()),
                document: self.document_number,
            });
            let target = self.set_target(Some(&fragment));
            self.set_document_url(link);
            self.scroll_to_fragment(&fragment);
            self.focus_fragment_target(target);
            return true;
        }
        let initiator = self.document_origin();
        self.start_navigation(link, None, HistoryHandling::Push, None, initiator);
        true
    }

    /// Reloads the current page and keeps the scroll position. While a
    /// navigation is pending, restarts that navigation instead; a pending
    /// `POST` is dropped and the current entry is reloaded (as Chromium
    /// does), so that the reload does not send that form again. Limits: if
    /// the current entry is itself a `POST` result, the reload sends its
    /// body again; without a committed entry, a reload does nothing and
    /// the pending `POST` goes on. A reload requests the entry again with
    /// its `POST` body and its initiator (ADR 0012, ADR 0013).
    pub fn reload(&mut self) {
        if let Some(pending) = self.pending.clone()
            && pending.post.is_none()
        {
            self.start_navigation(
                pending.url,
                pending.post,
                pending.handling,
                pending.restore_scroll,
                pending.initiator,
            );
            return;
        }
        if let Some(entry) = self.history.current().cloned() {
            self.start_navigation(
                entry.commit.url,
                entry.commit.post,
                HistoryHandling::Replace,
                Some(self.scroll),
                entry.commit.initiator,
            );
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

    /// Goes to the history entry of a `POST` result without sending the
    /// request again: shows a page that asks for a reload (as Chromium's
    /// form resubmission page). The entry keeps its body, so a reload
    /// sends it.
    fn traverse_to_post(&mut self, target: usize, commit: Commit) {
        self.loader.cancel_all();
        self.requests = Requests::default();
        self.pending = Some(PendingNavigation {
            handling: HistoryHandling::Traverse(target),
            restore_scroll: None,
            url: commit.url.clone(),
            post: commit.post,
            initiator: commit.initiator,
            redirected: false,
        });
        self.url = Some(commit.url);
        self.show_message(
            "Confirm form resubmission",
            "This page was the result of a form submission. \
             Reload the page to send the form data again.",
        );
    }

    fn traverse(&mut self, delta: isize) -> bool {
        let Some(target) = self.history_index().checked_add_signed(delta) else {
            return false;
        };
        let Some(Entry { commit, scroll }) = self.history.get(target).cloned() else {
            return false;
        };
        let same_document = self.pending.is_none()
            && self.document.is_some()
            && commit.document == self.document_number;
        if !same_document && commit.post.is_some() {
            self.traverse_to_post(target, commit);
            return true;
        }
        if same_document {
            // The entry shows the current document (its URL differs only
            // in the fragment): restore its scroll position without
            // loading.
            self.save_scroll();
            self.history.go_to(target);
            let fragment_target = self.set_target(commit.url.fragment());
            self.set_document_url(commit.url);
            self.scroll_to(scroll);
            self.focus_fragment_target(fragment_target);
            return true;
        }
        self.start_navigation(
            commit.url,
            None,
            HistoryHandling::Traverse(target),
            Some(scroll),
            commit.initiator,
        );
        true
    }

    /// Starts loading a document, with a `POST` request if `post` is
    /// given. Cancels all requests that are still running: those of a
    /// previous pending navigation and those of the current document's
    /// subresources.
    ///
    /// `initiator` is the origin of the document that started the
    /// navigation (a link or a form), or `None` if the user started it
    /// (address bar). Cookies use it for `SameSite`, and a `POST` sends it
    /// as `Origin` (ADR 0012). The history entry keeps it, so that a reload
    /// and back and forward use it again.
    fn start_navigation(
        &mut self,
        url: Url,
        post: Option<PostData>,
        handling: HistoryHandling,
        restore_scroll: Option<Point>,
        initiator: Option<Origin>,
    ) {
        self.loader.cancel_all();
        self.requests = Requests::default();
        self.pending = Some(PendingNavigation {
            handling,
            restore_scroll,
            url: url.clone(),
            post: post.clone(),
            initiator: initiator.clone(),
            redirected: false,
        });
        self.state = LoadState::LoadingDocument;
        self.error = None;
        let id = self
            .loader
            .start(document_request(url.clone(), post).with_initiator(initiator));
        self.requests.pending.insert(id, Pending::Document);
        log::info!("navigating to {url}");
        // Keep showing the old page until the new document arrives, but
        // show the new URL in the address bar.
        self.url = Some(url);
    }

    /// Updates the session history for the document that just arrived,
    /// and decides where to scroll after the first layout.
    ///
    /// A `POST` that redirects is followed by a `GET`
    /// ("POST/redirect/GET"), so its history entry keeps no body and a
    /// reload does not send the form again. (A 307 or 308 redirect keeps
    /// the method; its entry has no body either, a deliberate
    /// simplification.) It still gets a new history entry when it
    /// redirects to the URL of the current entry. Deviation: the HTML
    /// standard turns every navigation to the current URL into a
    /// replacement; Chromium (measured) adds an entry for a `POST`, and so
    /// does swb.
    fn commit_navigation(&mut self, url: &Url) {
        self.save_scroll();
        let pending = self.pending.take();
        self.document_number += 1;
        let redirected_post = pending
            .as_ref()
            .is_some_and(|p| p.redirected && p.post.is_some());
        let commit = Commit {
            url: url.clone(),
            post: pending
                .as_ref()
                .filter(|p| !p.redirected)
                .and_then(|p| p.post.clone()),
            initiator: pending.as_ref().and_then(|p| p.initiator.clone()),
            document: self.document_number,
        };
        match pending.as_ref().map(|p| p.handling) {
            Some(HistoryHandling::Push) if redirected_post => self.history.push_new(commit),
            Some(HistoryHandling::Push) | None => self.history.push(commit),
            Some(HistoryHandling::Replace) => self.history.replace_current(commit),
            Some(HistoryHandling::Traverse(index)) => {
                self.history.go_to(index);
                self.history.replace_current(commit);
            }
        }
        self.pending_scroll = match pending.and_then(|p| p.restore_scroll) {
            Some(position) => Some(ScrollTarget::Position(position)),
            None => url.fragment().map(|f| ScrollTarget::Fragment(f.to_owned())),
        };
    }

    /// Records that the document of the pending navigation arrived after a
    /// redirect (see [`Page::commit_navigation`]).
    pub(super) fn set_redirected(&mut self) {
        if let Some(pending) = &mut self.pending {
            pending.redirected = true;
        }
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
    /// (CSS px), with the scroll offsets of scroll containers applied, if
    /// it has a box.
    pub fn element_box(&mut self, node: NodeId) -> Option<swb_layout::Rect> {
        self.update_layout();
        self.fragments
            .as_ref()?
            .element_boxes_scrolled(self.scrollers.offsets())
            .get(&node)
            .copied()
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

/// The request for a document: a `GET`, or a `POST` with `post` as the
/// body.
fn document_request(url: Url, post: Option<PostData>) -> Request {
    match post {
        Some(post) => Request::post(
            url,
            post.body.to_vec(),
            &post.content_type,
            Destination::Document,
        ),
        None => Request::get(url, Destination::Document),
    }
}

/// The body of a `POST` request with its content type.
fn post_data(request: &Request) -> Option<PostData> {
    (request.method == Method::Post).then(|| PostData {
        body: request.body.as_deref().unwrap_or_default().into(),
        content_type: request
            .headers
            .get("content-type")
            .unwrap_or("application/x-www-form-urlencoded")
            .to_owned(),
    })
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
