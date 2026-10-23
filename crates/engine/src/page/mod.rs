//! A page: one document in one viewport, with its loading state, session
//! history and input handling.
//!
//! The page owns the state of its current document in a box
//! ([`DocumentState`], ADR 0028 section 2): the DOM, the stylesheets, the
//! styles, the layout, the subresources, the form controls and the scroll
//! offsets. The page itself holds what outlives a document: the history,
//! the pending navigation, the loader, the font context, the viewport and
//! the state of the pointer. A script runtime can take the box for a task
//! ([`Page::take_document_state`]) and give it back.
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
//! parts of `Page` are in `loading.rs` (network completions, the document
//! of a navigation), `pipeline.rs` (rendering, the viewport and its
//! accepted limits, screenshots), `scroll.rs` (scrolling by the user and
//! the scroll queries), `input.rs` (pointer, keyboard, focus, selection)
//! and `forms.rs` (form controls: editing, activation, submission).

mod forms;
mod input;
mod loading;
mod pipeline;
mod scroll;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use swb_dom::{Document, NodeId};
use swb_layout::{FragmentTree, Point, Size};
use swb_net::{CookieJar, Destination, Fetcher, Loader, Method, Origin, Request, RequestId, Url};
use swb_style::StyleMap;
use swb_text::FontContext;

use crate::document::{DocumentState, ScrollTarget, View};
use crate::history::{Commit, Entry, History, PostData};

pub use crate::document::ElementScroll;
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

/// A page.
pub struct Page {
    /// Shared with the document state, which starts the loads of its
    /// subresources.
    loader: Rc<Loader>,
    fonts: FontContext,
    history: History,
    pending: Option<PendingNavigation>,
    /// The request of the pending navigation's document.
    document_request: Option<RequestId>,
    /// The loading state; `Complete` stands for both `LoadingResources`
    /// and `Complete`, which the document's requests tell apart
    /// ([`Page::load_state`]).
    state: LoadState,
    error: Option<String>,

    /// The URL shown to the user: the pending navigation's URL while it
    /// loads, otherwise the document URL.
    url: Option<Url>,
    /// The current document; `None` before the first one, and while a
    /// script runtime holds it.
    doc: Option<Box<DocumentState>>,
    /// True between [`Page::take_document_state`] and the return of the
    /// state. Completions stay queued in the loader meanwhile (the
    /// requests map is in the state).
    lent: bool,
    /// The JavaScript setting. A new document takes the value it has when
    /// the document is created; the current document keeps its own
    /// ([`Document::scripting`]), so a change applies to the next
    /// document (ADR 0028 section 15).
    javascript: bool,
    /// The number of the current document: it grows with every committed
    /// document. History entries of the same document can be traversed
    /// without loading.
    document_number: u64,
    /// The viewport, the scale and the scroll indicator setting; the
    /// document has a copy.
    view: View,
    input: input::InputState,
}

impl Page {
    /// Creates an empty page.
    pub fn new(config: PageConfig, fonts: FontContext, viewport: Size, scale: f32) -> Self {
        Page {
            loader: Rc::new(Loader::new(
                config.fetcher,
                config.network_threads,
                config.notify,
            )),
            fonts,
            history: History::default(),
            pending: None,
            document_request: None,
            state: LoadState::Idle,
            error: None,
            url: None,
            doc: None,
            lent: false,
            javascript: false,
            document_number: 0,
            view: View {
                viewport,
                scale,
                indicators: false,
            },
            input: input::InputState::default(),
        }
    }

    // ----- JavaScript setting -----

    /// The JavaScript setting (off by default).
    pub fn javascript(&self) -> bool {
        self.javascript
    }

    /// Sets the JavaScript setting. It applies to the next document: the
    /// current document keeps the value it was created with, and a reload
    /// applies the change. No script runs yet; the setting is the
    /// document's scripting flag (the parser's scripting flag, the
    /// `scripting` media feature, media controls and `<canvas>`).
    pub fn set_javascript(&mut self, enabled: bool) {
        self.javascript = enabled;
    }

    /// True if the current document was created with JavaScript on.
    pub fn document_has_scripting(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.scripting())
    }

    // ----- The document state -----

    /// The state of the current document.
    pub fn document_state(&self) -> Option<&DocumentState> {
        self.doc.as_deref()
    }

    /// The state of the current document, for changes and queries.
    pub fn document_state_mut(&mut self) -> Option<&mut DocumentState> {
        self.doc.as_deref_mut()
    }

    /// Takes the state of the current document out of the page, for a
    /// script runtime that runs a task (ADR 0028 section 2). Until
    /// [`Page::return_document_state`], the page has no document: queries
    /// give empty values (`title()` is empty, `scroll_position()` is
    /// zero), and a navigation or reload in this time saves scroll 0 into
    /// the history entry, so a script task must end before a navigation
    /// commits. Network completions stay queued in the loader meanwhile
    /// ([`Page::process_network`] does nothing, [`Page::wait_until_loaded`]
    /// returns at once); they are handled after the return, in their
    /// order. Moving the box costs a pointer copy.
    pub fn take_document_state(&mut self) -> Option<Box<DocumentState>> {
        let state = self.doc.take();
        self.lent = state.is_some();
        state
    }

    /// Gives back the state that [`Page::take_document_state`] took and
    /// processes the DOM changes of the task. A state that is not the
    /// current document's (the page navigated meanwhile) is dropped.
    pub fn return_document_state(&mut self, mut state: Box<DocumentState>) {
        self.lent = false;
        if self.doc.is_some() {
            log::debug!("a document came back after a navigation; dropped");
            return;
        }
        state.sync();
        self.doc = Some(state);
    }

    /// Changes the DOM of the current document with `change` (through the
    /// change functions of `swb_dom`), then processes the changes: loads
    /// of inserted elements start, removed stylesheets go away, and the
    /// styles and the layout are computed again when needed. Returns
    /// `None` without a document.
    pub fn change_dom<R>(&mut self, change: impl FnOnce(&mut Document) -> R) -> Option<R> {
        let result = self.doc.as_deref_mut()?.change_dom(change);
        Some(result)
    }

    /// The document state and the font context, for the calls that lay
    /// out.
    fn parts(&mut self) -> Option<(&mut DocumentState, &mut FontContext)> {
        Some((self.doc.as_deref_mut()?, &mut self.fonts))
    }

    // ----- Navigation -----

    /// Navigates to `url`, as if the user typed it (the request has no
    /// initiator). The new history entry is added when the document
    /// arrives. A URL with a fragment that equals the document's apart
    /// from the fragment scrolls to the fragment instead of loading (HTML
    /// "navigate to a fragment", as for links), unless a load is in
    /// progress or the page shows an error.
    pub fn navigate(&mut self, url: Url) {
        if self.navigate_to_fragment(&url) {
            return;
        }
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
        if self.navigate_to_fragment(&link) {
            return true;
        }
        let initiator = self.document_origin();
        self.start_navigation(link, None, HistoryHandling::Push, None, initiator);
        true
    }

    /// If `url` is the current document with a fragment (and nothing is
    /// loading), scrolls to the fragment, adds a history entry and returns
    /// true (HTML "navigate to a fragment",
    /// <https://html.spec.whatwg.org/multipage/browsing-the-web.html#scroll-to-fragid>).
    /// An error page is not the document of its URL: a navigation loads it
    /// again.
    fn navigate_to_fragment(&mut self, url: &Url) -> bool {
        let Some(fragment) = url.fragment() else {
            return false;
        };
        if self.pending.is_some() || self.state == LoadState::Failed || !self.is_same_document(url)
        {
            return false;
        }
        let fragment = fragment.to_owned();
        self.save_scroll();
        // The entry shows the same document: it keeps the POST body and the
        // initiator of the document's entry, because a reload of it
        // requests that document again.
        let current = self.history.current().map(|e| &e.commit);
        self.history.push(Commit {
            url: url.clone(),
            post: current.and_then(|c| c.post.clone()),
            initiator: current.and_then(|c| c.initiator.clone()),
            document: self.document_number,
        });
        let target = self.set_target(Some(&fragment));
        self.set_document_url(url.clone());
        self.scroll_to_fragment(&fragment);
        self.focus_fragment_target(target);
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
            let scroll = self.scroll_position();
            self.start_navigation(
                entry.commit.url,
                entry.commit.post,
                HistoryHandling::Replace,
                Some(scroll),
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
        self.document_request = None;
        self.pending = None;
        self.url = self.doc.as_ref().map(|d| d.url().clone());
        if let Some((doc, fonts)) = self.parts() {
            doc.stop_loading(fonts);
        }
        self.state = if self.doc.is_some() {
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
        self.document_request = None;
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
        let same_document =
            self.pending.is_none() && self.doc.is_some() && commit.document == self.document_number;
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
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.requests = crate::resources::Requests::default();
        }
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
        self.document_request = Some(id);
        log::info!("navigating to {url}");
        // Keep showing the old page until the new document arrives, but
        // show the new URL in the address bar.
        self.url = Some(url);
    }

    /// Updates the session history for the document that just arrived.
    /// Returns where to scroll after the first layout.
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
    fn commit_navigation(&mut self, url: &Url) -> Option<ScrollTarget> {
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
        match pending.and_then(|p| p.restore_scroll) {
            Some(position) => Some(ScrollTarget::Position(position)),
            None => url.fragment().map(|f| ScrollTarget::Fragment(f.to_owned())),
        }
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
        let scroll = self.scroll_position();
        if let Some(entry) = self.history.current_mut() {
            entry.scroll = scroll;
        }
    }

    /// True if `url` is the URL of the current document, ignoring the
    /// fragment.
    fn is_same_document(&self, url: &Url) -> bool {
        self.doc
            .as_ref()
            .is_some_and(|d| strip_fragment(d.url()) == strip_fragment(url))
    }

    fn set_document_url(&mut self, url: Url) {
        self.url = Some(url.clone());
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.url = url;
        }
    }

    /// The origin of the current document: the initiator of its
    /// subresource requests and of the navigations it starts.
    fn document_origin(&self) -> Option<Origin> {
        self.doc.as_ref().map(|d| d.origin())
    }

    /// True if the current document may load `url` (see
    /// [`DocumentState::may_load`]). Without a document, only loadable
    /// schemes other than `file:`.
    pub(super) fn may_load_subresource(&self, url: &Url) -> bool {
        match &self.doc {
            Some(doc) => doc.may_load(url),
            None => is_loadable(url) && url.scheme() != "file",
        }
    }

    // ----- Accessors -----

    /// The URL to show to the user: the URL being loaded, or the document
    /// URL.
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }

    /// The document title.
    pub fn title(&self) -> &str {
        self.doc.as_ref().map_or("", |d| d.title())
    }

    /// The loading state.
    pub fn load_state(&self) -> LoadState {
        match self.state {
            LoadState::Complete | LoadState::LoadingResources
                if self.doc.as_ref().is_some_and(|d| d.is_loading()) =>
            {
                LoadState::LoadingResources
            }
            LoadState::LoadingResources => LoadState::Complete,
            state => state,
        }
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
            self.load_state(),
            LoadState::LoadingDocument | LoadState::LoadingResources
        )
    }

    /// The document, if one is loaded.
    pub fn document(&self) -> Option<&Document> {
        self.doc.as_ref().map(|d| d.dom())
    }

    /// The computed styles (after [`Page::update_layout`]).
    pub fn styles(&self) -> Option<&StyleMap> {
        self.doc.as_ref()?.styles()
    }

    /// The fragment tree (after [`Page::update_layout`]).
    pub fn fragments(&self) -> Option<&FragmentTree> {
        self.doc.as_ref()?.fragments()
    }

    /// The elements that match a selector list, in tree order (as
    /// `querySelectorAll`). `None` if the selector list is invalid or there
    /// is no document.
    pub fn query_selector_all(&self, selectors: &str) -> Option<Vec<NodeId>> {
        self.doc.as_ref()?.query_selector_all(selectors)
    }

    /// The union of the border boxes of an element as painted, in document
    /// coordinates (CSS px), if it has a box: with the scroll offsets of
    /// scroll containers, and with transforms and fixed and sticky
    /// positioning at the current scroll position (as
    /// `getBoundingClientRect()` plus the scroll offset).
    pub fn element_box(&mut self, node: NodeId) -> Option<swb_layout::Rect> {
        let (doc, fonts) = self.parts()?;
        doc.element_box(node, fonts)
    }

    /// The font context.
    pub fn fonts(&mut self) -> &mut FontContext {
        &mut self.fonts
    }

    /// The viewport size in CSS px.
    pub fn viewport(&self) -> Size {
        self.view.viewport
    }

    /// The device pixel ratio.
    pub fn scale(&self) -> f32 {
        self.view.scale
    }

    /// The scroll position in CSS px.
    pub fn scroll_position(&self) -> Point {
        self.doc.as_ref().map_or(Point::default(), |d| d.scroll)
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
pub(crate) fn is_loadable(url: &Url) -> bool {
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
