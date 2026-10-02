//! A page: one document in one viewport, with its loading state, rendering
//! pipeline, scroll position, input handling and session history.
//!
//! Navigation: [`Page::navigate`], [`Page::reload`] and history traversal
//! start a *pending navigation*. Starting one cancels all requests that
//! are still running. The old document stays visible until the new one
//! arrives; then the navigation commits: the history is updated and the
//! new document replaces the old one. A navigation that changes only the
//! fragment of the current document does not load anything; it scrolls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use swb_css::{MediaEnvironment, MediaQueryList};
use swb_dom::{Document, NodeId, local_name};
use swb_layout::{FragmentRef, FragmentTree, LayoutInput, Point, Size};
use swb_net::{Destination, Fetcher, Loader, Request, Response, Url};
use swb_paint::{DisplayList, Pixmap, RasterParams};
use swb_style::{ElementStates, StyleMap, Stylist};
use swb_text::FontContext;

use crate::history::History;
use crate::hit_test::{self, HitResult};
use crate::resources::{ImageState, Images, Pending, Requests, SheetSlot};

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
    states: ElementStates,
    hovered_link: Option<Url>,
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
            states: ElementStates::default(),
            hovered_link: None,
        }
    }

    // ----- Navigation -----

    /// Navigates to `url`. The new history entry is added when the document
    /// arrives.
    pub fn navigate(&mut self, url: Url) {
        self.start_navigation(url, HistoryHandling::Push, None);
    }

    /// Reloads the current page and keeps the scroll position. While a
    /// navigation is pending, restarts that navigation instead.
    pub fn reload(&mut self) {
        if let Some(pending) = self.pending
            && let Some(url) = self.url.clone()
        {
            self.start_navigation(url, pending.handling, pending.restore_scroll);
            return;
        }
        if let Some(entry) = self.history.current() {
            let url = entry.url.clone();
            self.start_navigation(url, HistoryHandling::Replace, Some(self.scroll));
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
            self.set_document_url(entry.url);
            self.scroll_to(entry.scroll);
            return true;
        }
        self.start_navigation(
            entry.url,
            HistoryHandling::Traverse(target),
            Some(entry.scroll),
        );
        true
    }

    /// Starts loading a document. Cancels all requests that are still
    /// running: those of a previous pending navigation and those of the
    /// current document's subresources.
    fn start_navigation(
        &mut self,
        url: Url,
        handling: HistoryHandling,
        restore_scroll: Option<Point>,
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
            .start(Request::get(url.clone(), Destination::Document));
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

    /// The font context.
    pub fn fonts(&mut self) -> &mut FontContext {
        &mut self.fonts
    }

    /// The viewport size in CSS px.
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// The URL of the link under the mouse pointer.
    pub fn hovered_link(&self) -> Option<&Url> {
        self.hovered_link.as_ref()
    }

    /// The scroll position in CSS px.
    pub fn scroll_position(&self) -> Point {
        self.scroll
    }

    // ----- Network -----

    /// Processes completed network requests without blocking. Returns true
    /// if anything changed that needs a repaint.
    pub fn process_network(&mut self) -> bool {
        let mut changed = false;
        while let Some(completion) = self.loader.try_recv() {
            changed |= self.handle_completion(completion);
        }
        changed
    }

    /// Blocks until the page and all subresources are loaded, or until
    /// `timeout` passes. Returns true if loading completed.
    pub fn wait_until_loaded(&mut self, timeout: Duration) -> bool {
        // A timeout too large to represent means "no deadline".
        let deadline = Instant::now().checked_add(timeout);
        while self.is_loading() {
            let now = Instant::now();
            let remaining = match deadline {
                Some(d) if now >= d => return false,
                Some(d) => d - now,
                None => Duration::from_secs(3600),
            };
            if let Some(completion) = self.loader.recv_timeout(remaining) {
                self.handle_completion(completion);
            }
            // Layout can discover more resources (background images).
            if !self.is_loading() {
                self.update_layout();
            }
        }
        true
    }

    /// Handles one completed request. Completions of requests that were
    /// cancelled (by a navigation or by [`Page::stop`]) are not pending and
    /// are ignored.
    fn handle_completion(&mut self, completion: swb_net::Completion) -> bool {
        let Some(pending) = self.requests.pending.remove(&completion.id) else {
            return false;
        };
        match pending {
            Pending::Document => self.document_loaded(completion.result),
            Pending::Stylesheet { slot } => {
                if let Some(sheet) = self.sheets.get_mut(slot) {
                    match completion.result {
                        Ok(response) if response.is_success() => {
                            let charset = response.content_type().and_then(|c| c.charset);
                            sheet.css = Some(decode_css(&response.body, charset.as_deref()));
                            sheet.base_url = response.url;
                        }
                        Ok(response) => {
                            log::warn!(
                                "stylesheet {} failed: HTTP {}",
                                response.url,
                                response.status
                            );
                            sheet.failed = true;
                        }
                        Err(e) => {
                            log::warn!("stylesheet failed: {e}");
                            sheet.failed = true;
                        }
                    }
                }
                self.invalidate_style();
            }
            Pending::Image { url } => {
                let state = ImageState::from_fetch(&url, completion.result);
                self.images.by_url.insert(url, state);
                self.invalidate_layout();
            }
        }
        self.update_load_state();
        true
    }

    fn document_loaded(&mut self, result: Result<Response, swb_net::NetError>) {
        let response = match result {
            Ok(r) => r,
            Err(e) => {
                self.show_error(&format!("Cannot load page: {e}"));
                return;
            }
        };
        if !response.is_success() {
            log::warn!("{}: HTTP {}", response.url, response.status);
        }
        let content_type = response.content_type();
        let essence = content_type
            .as_ref()
            .map_or("text/html", |c| c.essence.as_str());
        let mut images = Images::default();
        let document = if essence == "text/html" || essence == "application/xhtml+xml" {
            let charset = content_type.as_ref().and_then(|c| c.charset.as_deref());
            swb_dom::parse_html_bytes(&response.body, charset).0
        } else if essence.starts_with("text/") || essence == "application/json" {
            let text = String::from_utf8_lossy(&response.body);
            swb_dom::parse_html(&format!("<pre>{}</pre>", escape_html(&text)))
        } else if essence.starts_with("image/") {
            // The image is already here; the generated <img> uses it
            // without fetching it again.
            images.by_url.insert(
                response.url.clone(),
                ImageState::decode(&response.url, &response.body),
            );
            swb_dom::parse_html(&format!(
                "<body style=\"margin:0\"><img src=\"{}\">",
                escape_html(response.url.as_str())
            ))
        } else {
            self.show_error(&format!("Cannot display content of type {essence}"));
            return;
        };
        self.set_document(document, response.url, images);
    }

    fn show_error(&mut self, message: &str) {
        log::warn!("{message}");
        self.error = Some(message.to_owned());
        let html = format!(
            "<!DOCTYPE html><title>Error</title><body style=\"font-family:sans-serif;margin:2em\"><h1>Cannot load page</h1><p>{}</p>",
            escape_html(message)
        );
        let url = self.url.clone().unwrap_or_else(about_blank);
        self.set_document(swb_dom::parse_html(&html), url, Images::default());
        self.state = LoadState::Failed;
    }

    /// Commits the pending navigation with `document`. `images` contains
    /// images that are already loaded.
    fn set_document(&mut self, document: Document, url: Url, images: Images) {
        self.commit_navigation(&url);
        // Requests that are still pending belong to the previous document
        // (for example images that a restyle of it started).
        self.loader.cancel_all();
        self.requests = Requests::default();
        self.base_url = Some(base_url(&document, &url));
        self.set_document_url(url);
        self.title = document_title(&document);
        self.document = Some(document);
        self.sheets.clear();
        self.images = images;
        self.scroll = Point::default();
        self.states = ElementStates::default();
        self.hovered_link = None;
        self.start_subresources();
        self.invalidate_style();
        self.state = LoadState::LoadingResources;
        self.update_load_state();
    }

    /// Finds stylesheets and images in the document and starts loading them.
    fn start_subresources(&mut self) {
        let (Some(doc), Some(base)) = (&self.document, self.base_url.clone()) else {
            return;
        };
        let mut sheet_loads = Vec::new();
        let mut image_loads = Vec::new();
        for node in doc.descendants(NodeId::DOCUMENT) {
            let Some(element) = doc.element(node) else {
                continue;
            };
            if !element.is_html() {
                continue;
            }
            match *element.local_name() {
                local_name!("style") => {
                    self.sheets.push(SheetSlot {
                        css: Some(doc.text_content(node)),
                        base_url: base.clone(),
                        media: element.attr("media").map(str::to_owned),
                        failed: false,
                    });
                }
                local_name!("link") => {
                    let is_stylesheet = element.attr("rel").is_some_and(|rel| {
                        rel.split_ascii_whitespace()
                            .any(|r| r.eq_ignore_ascii_case("stylesheet"))
                            && !rel
                                .split_ascii_whitespace()
                                .any(|r| r.eq_ignore_ascii_case("alternate"))
                    });
                    if !is_stylesheet {
                        continue;
                    }
                    let Some(url) = element.attr("href").and_then(|h| base.join(h.trim()).ok())
                    else {
                        continue;
                    };
                    let slot = self.sheets.len();
                    self.sheets.push(SheetSlot {
                        css: None,
                        base_url: url.clone(),
                        media: element.attr("media").map(str::to_owned),
                        failed: false,
                    });
                    sheet_loads.push((slot, url));
                }
                local_name!("img") => {
                    if let Some(url) = element
                        .attr("src")
                        .filter(|s| !s.trim().is_empty())
                        .and_then(|src| base.join(src.trim()).ok())
                    {
                        self.images.by_node.insert(node, url.clone());
                        image_loads.push(url);
                    }
                }
                _ => {}
            }
        }
        for (slot, url) in sheet_loads {
            if !self.may_load_subresource(&url) {
                if let Some(sheet) = self.sheets.get_mut(slot) {
                    sheet.failed = true;
                }
                continue;
            }
            let id = self.loader.start(Request::get(url, Destination::Style));
            self.requests
                .pending
                .insert(id, Pending::Stylesheet { slot });
        }
        for url in image_loads {
            self.start_image(url);
        }
    }

    fn start_image(&mut self, url: Url) {
        if self.images.by_url.contains_key(&url) {
            return;
        }
        if !self.may_load_subresource(&url) {
            self.images.by_url.insert(url, ImageState::Failed);
            return;
        }
        self.images.by_url.insert(url.clone(), ImageState::Loading);
        let id = self
            .loader
            .start(Request::get(url.clone(), Destination::Image));
        self.requests.pending.insert(id, Pending::Image { url });
    }

    fn update_load_state(&mut self) {
        if self.state == LoadState::LoadingResources && self.requests.is_empty() {
            self.state = LoadState::Complete;
            log::info!("loaded {}", self.url.as_ref().map_or("", Url::as_str));
        }
    }

    fn stylesheets_settled(&self) -> bool {
        self.sheets.iter().all(SheetSlot::is_settled)
    }

    // ----- Pipeline -----

    fn invalidate_style(&mut self) {
        self.stylist = None;
        self.styles = None;
        self.invalidate_layout();
    }

    fn invalidate_layout(&mut self) {
        self.fragments = None;
        self.display_list = None;
    }

    fn media_environment(&self) -> MediaEnvironment {
        MediaEnvironment {
            viewport_width: self.viewport.width,
            viewport_height: self.viewport.height,
            device_pixel_ratio: self.scale,
            ..MediaEnvironment::default()
        }
    }

    /// Brings styles and layout up to date. Rendering waits for
    /// stylesheets: until they are loaded, there is no layout. After the
    /// first layout of a document, scrolls to the fragment or to the
    /// restored position.
    pub fn update_layout(&mut self) {
        if self.document.is_none() || !self.stylesheets_settled() {
            return;
        }
        if self.styles.is_none() {
            self.compute_styles();
        }
        if self.fragments.is_none()
            && let (Some(doc), Some(styles)) = (&self.document, &self.styles)
        {
            let input = LayoutInput {
                document: doc,
                styles,
                viewport: self.viewport,
                replaced: &self.images,
            };
            let started = Instant::now();
            let fragments = swb_layout::layout(&input, &mut self.fonts);
            log::debug!("layout: {:?}", started.elapsed());
            self.fragments = Some(fragments);
            // The scroll target is applied after every layout until the
            // page is loaded, because images that arrive later move it.
            // Scrolling by the user cancels it.
            match self.pending_scroll.clone() {
                Some(ScrollTarget::Fragment(fragment)) => {
                    if let Some(position) = self.fragment_position(&fragment) {
                        self.scroll = position;
                    }
                }
                Some(ScrollTarget::Position(position)) => self.scroll = position,
                None => {}
            }
            self.clamp_scroll();
            if !self.is_loading() {
                self.pending_scroll = None;
            }
        }
    }

    /// Brings the display list up to date.
    fn update_display_list(&mut self) {
        self.update_layout();
        if self.display_list.is_none()
            && let Some(fragments) = &self.fragments
        {
            self.display_list = Some(swb_paint::build_display_list(fragments, &self.images));
        }
    }

    fn compute_styles(&mut self) {
        let Some(doc) = &self.document else {
            return;
        };
        let env = self.media_environment();
        if self.stylist.is_none() {
            let mut stylist = Stylist::new(doc.quirks_mode);
            for sheet in &self.sheets {
                let Some(css) = &sheet.css else {
                    continue;
                };
                if let Some(media) = &sheet.media
                    && !MediaQueryList::parse_str(media).matches(&env)
                {
                    continue;
                }
                stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &sheet.base_url);
            }
            self.stylist = Some(stylist);
        }
        let Some(stylist) = &self.stylist else {
            return;
        };
        let started = Instant::now();
        let base = self.base_url.clone().unwrap_or_else(about_blank);
        let styles = swb_style::compute_styles(doc, stylist, &env, &self.states, &base);
        log::debug!("style: {:?}", started.elapsed());
        let image_urls = styles.image_urls();
        self.styles = Some(styles);
        for url in image_urls {
            if let Ok(url) = Url::parse(&url) {
                self.start_image(url);
            }
        }
        if !self.requests.is_empty() && self.state == LoadState::Complete {
            self.state = LoadState::LoadingResources;
        }
    }

    /// Renders the viewport into `target` (device pixels). The target size
    /// should be the viewport size times the scale factor.
    pub fn render(&mut self, target: &mut Pixmap) {
        swb_paint::fill(target, swb_style::Rgba::WHITE);
        self.update_display_list();
        if let Some(list) = &self.display_list {
            let params = RasterParams {
                scroll: self.scroll,
                scale: self.scale,
            };
            swb_paint::rasterize(list, target, params, &mut self.fonts, &self.images);
        }
    }

    /// Changes the viewport size (CSS px) and scale factor.
    pub fn set_viewport(&mut self, viewport: Size, scale: f32) {
        if viewport == self.viewport && (scale - self.scale).abs() < f32::EPSILON {
            return;
        }
        self.viewport = viewport;
        self.scale = scale;
        // Media queries may change, so styles are recomputed.
        self.invalidate_style();
    }

    // ----- Scrolling -----

    /// The size of the scrollable content in CSS px.
    pub fn content_size(&mut self) -> Size {
        self.update_layout();
        self.fragments
            .as_ref()
            .map_or(self.viewport, |f| f.scroll_size)
    }

    /// Scrolls by a delta in CSS px.
    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        self.scroll_to(Point::new(self.scroll.x + dx, self.scroll.y + dy));
    }

    /// Scrolls to a position in CSS px, clamped to the content. Replaces a
    /// scroll to the fragment that waits for the first layout.
    pub fn scroll_to(&mut self, p: Point) {
        self.update_layout();
        self.pending_scroll = None;
        self.scroll = p;
        self.clamp_scroll();
    }

    /// Clamps the scroll position to the content. Without a layout the
    /// content size is not known; the position is then clamped after the
    /// next layout.
    fn clamp_scroll(&mut self) {
        self.scroll.x = self.scroll.x.max(0.0);
        self.scroll.y = self.scroll.y.max(0.0);
        let Some(fragments) = &self.fragments else {
            return;
        };
        let size = fragments.scroll_size;
        let max_x = (size.width - self.viewport.width).max(0.0);
        let max_y = (size.height - self.viewport.height).max(0.0);
        self.scroll.x = self.scroll.x.min(max_x);
        self.scroll.y = self.scroll.y.min(max_y);
    }

    // ----- Input -----

    /// Hit-tests a point in viewport coordinates (CSS px).
    pub fn hit_test(&mut self, x: f32, y: f32) -> Option<HitResult> {
        self.update_display_list();
        let doc = self.document.as_ref()?;
        let list = self.display_list.as_ref()?;
        let point = Point::new(x + self.scroll.x, y + self.scroll.y);
        hit_test::hit_test(doc, list, point, self.base_url.as_ref())
    }

    /// Handles mouse movement over the page. Returns true if a repaint is
    /// needed.
    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let link = self.hit_test(x, y).and_then(|h| h.link);
        let changed = link != self.hovered_link;
        self.hovered_link = link;
        changed
    }

    /// Handles the mouse pointer leaving the page area. Returns true if a
    /// repaint is needed.
    pub fn mouse_leave(&mut self) -> bool {
        self.hovered_link.take().is_some()
    }

    /// Handles a primary-button click. Follows links. Returns true if a
    /// navigation started or the page scrolled to a fragment.
    pub fn click(&mut self, x: f32, y: f32) -> bool {
        match self.hit_test(x, y).and_then(|hit| hit.link) {
            Some(link) => self.follow_link(link),
            None => false,
        }
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
            self.set_document_url(link);
            self.scroll_to_fragment(&fragment);
            return true;
        }
        self.navigate(link);
        true
    }

    /// Scrolls to the element that the fragment indicates (HTML "scroll to
    /// the fragment"): the element with that ID or an `<a>` with that name,
    /// or the top of the document for an empty fragment or `top`. Without a
    /// layout yet, scrolls after the first layout.
    pub fn scroll_to_fragment(&mut self, fragment: &str) {
        self.update_layout();
        if self.fragments.is_none() {
            self.pending_scroll = Some(ScrollTarget::Fragment(fragment.to_owned()));
            return;
        }
        if let Some(position) = self.fragment_position(fragment) {
            self.scroll_to(position);
        }
    }

    /// The scroll position for a fragment, if it indicates something.
    /// <https://html.spec.whatwg.org/multipage/browsing-the-web.html#the-indicated-part-of-the-document>
    fn fragment_position(&self, fragment: &str) -> Option<Point> {
        let doc = self.document.as_ref()?;
        let tree = self.fragments.as_ref()?;
        let decoded = percent_decode(fragment);
        let target = indicated_element(doc, fragment).or_else(|| indicated_element(doc, &decoded));
        match target {
            Some(target) => node_position(doc, tree, target).map(|p| Point::new(0.0, p.y)),
            None if decoded.is_empty() || decoded.eq_ignore_ascii_case("top") => {
                Some(Point::default())
            }
            None => None,
        }
    }
}

/// The element with the ID `name`, or the first `<a>` with that name.
fn indicated_element(doc: &Document, name: &str) -> Option<NodeId> {
    if name.is_empty() {
        return None;
    }
    doc.element_by_id(name).or_else(|| {
        doc.find_element(NodeId::DOCUMENT, |e| {
            e.is_html_named(&local_name!("a")) && e.attr("name") == Some(name)
        })
    })
}

/// The position of the first fragment of `node`. A node without fragments
/// (for example an empty `<a name>`) uses the first fragment that follows
/// it in tree order.
fn node_position(doc: &Document, tree: &FragmentTree, node: NodeId) -> Option<Point> {
    let mut first: HashMap<NodeId, Point> = HashMap::new();
    tree.walk(|fragment, origin| {
        let (owner, rect) = match fragment {
            FragmentRef::Box(b) if b.pseudo.is_none() => match b.node {
                Some(owner) => (owner, b.border_rect.translate(origin)),
                None => return,
            },
            FragmentRef::Text(t) => (t.node, t.rect.translate(origin)),
            FragmentRef::Box(_) => return,
        };
        first.entry(owner).or_insert(rect.origin());
    });
    doc.descendants(NodeId::DOCUMENT)
        .skip_while(|&n| n != node)
        .find_map(|n| first.get(&n).copied())
}

impl Page {
    /// True if the current document may load `url`: only `file:` documents
    /// may load `file:` URLs, as in Chromium ("Not allowed to load local
    /// resource"). Addresses typed by the user are not restricted.
    fn may_load_subresource(&self, url: &Url) -> bool {
        is_loadable(url)
            && (url.scheme() != "file"
                || self
                    .document_url
                    .as_ref()
                    .is_some_and(|document| document.scheme() == "file"))
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

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2)))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: Option<&u8>) -> Option<u8> {
    let b = *b?;
    (b as char).to_digit(16).map(|d| d as u8)
}

fn decode_css(bytes: &[u8], charset: Option<&str>) -> String {
    // CSS Syntax §3.2: BOM, then the protocol charset, then @charset,
    // then UTF-8. The BOM (handled by `decode`), the protocol charset and
    // UTF-8 are supported.
    let encoding = charset
        .and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

fn base_url(doc: &Document, url: &Url) -> Url {
    doc.find_element(NodeId::DOCUMENT, |e| {
        e.is_html_named(&local_name!("base")) && e.has_attr("href")
    })
    .and_then(|base| doc.element(base)?.attr("href"))
    .and_then(|href| url.join(href.trim()).ok())
    .unwrap_or_else(|| url.clone())
}

fn document_title(doc: &Document) -> String {
    let title = doc
        .head()
        .and_then(|head| {
            doc.element_children(head)
                .find(|&c| doc.is_html_element(c, &local_name!("title")))
        })
        .or_else(|| doc.find_element(NodeId::DOCUMENT, |e| e.is_html_named(&local_name!("title"))));
    title.map_or_else(String::new, |t| {
        doc.text_content(t)
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// Escapes text for inclusion in generated HTML.
pub(crate) fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%C4%8D"), "č");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%4"), "%4");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn loadable_schemes() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(is_loadable(&u("https://a.test/")));
        assert!(is_loadable(&u("data:text/html,x")));
        assert!(!is_loadable(&u("javascript:void(0)")));
        assert!(!is_loadable(&u("mailto:a@b.test")));
    }
}
