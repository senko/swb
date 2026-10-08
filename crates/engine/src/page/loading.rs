//! Loading: network completions, the document of a navigation (or an
//! error page), and its subresources (stylesheets and images; web fonts
//! are in `fonts.rs`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use encoding_rs::Encoding;
use swb_dom::{Document, NodeId, is_html_whitespace, local_name};
use swb_layout::Point;
use swb_net::{Destination, Request, Response, Url, escape_html};

use super::scroll::indicated;
use super::{LoadState, Page, about_blank, is_loadable};
use crate::forms::Forms;
use crate::image_source::{self, SelectedImage};
use crate::resources::{ImageState, Images, Pending, Requests, SheetSlot};
use crate::scrollers::Scrollers;
use crate::selection::TreeOrder;
use crate::web_fonts::WebFonts;

impl Page {
    /// Processes completed network requests without blocking. Returns true
    /// if anything changed that needs a repaint.
    pub fn process_network(&mut self) -> bool {
        let mut changed = false;
        while let Some(completion) = self.loader.try_recv() {
            changed |= self.handle_completion(completion);
        }
        changed
    }

    /// True if the page and all its subresources are loaded. Layout can
    /// discover more resources (background images), so the page is laid
    /// out first.
    pub fn is_fully_loaded(&mut self) -> bool {
        if self.is_loading() {
            return false;
        }
        self.update_layout();
        !self.is_loading()
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
            Pending::Font { url } => self.font_file_arrived(&url, completion.result),
            Pending::Image { url } => {
                let state = ImageState::from_fetch(&url, completion.result);
                self.images.by_url.insert(url.clone(), state);
                self.images.load_ended(&url);
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
        if response.redirected {
            self.set_redirected();
        }
        let content_type = response.content_type();
        let essence = content_type
            .as_ref()
            .map_or("text/html", |c| c.essence.as_str());
        let mut images = Images::default();
        let mut encoding = encoding_rs::UTF_8;
        let document = if essence == "text/html" || essence == "application/xhtml+xml" {
            let charset = content_type.as_ref().and_then(|c| c.charset.as_deref());
            let started = Instant::now();
            let (document, name) = swb_dom::parse_html_bytes(&response.body, charset);
            self.timings.parse = started.elapsed();
            encoding = Encoding::for_label(name.as_bytes()).unwrap_or(encoding_rs::UTF_8);
            document
        } else if essence.starts_with("text/") || essence == "application/json" {
            let text = String::from_utf8_lossy(&response.body);
            swb_dom::parse_html(&format!("<pre>{}</pre>", escape_html(&text)))
        } else if essence.starts_with("image/") {
            // The image is already here; the generated <img> uses it
            // without fetching it again.
            images.by_url.insert(
                response.url.clone(),
                ImageState::decode(&response.url, &response),
            );
            swb_dom::parse_html(&format!(
                "<body style=\"margin:0\"><img src=\"{}\">",
                escape_html(response.url.as_str())
            ))
        } else {
            self.show_error(&format!("Cannot display content of type {essence}"));
            return;
        };
        self.set_document(document, response.url, images, encoding);
    }

    fn show_error(&mut self, message: &str) {
        log::warn!("{message}");
        self.show_message("Cannot load page", message);
    }

    /// Commits the pending navigation with a page that shows a heading and
    /// a message, and sets the error to the message (the load failed).
    pub(super) fn show_message(&mut self, heading: &str, message: &str) {
        self.error = Some(message.to_owned());
        let html = format!(
            "<!DOCTYPE html><title>Error</title><body style=\"font-family:sans-serif;margin:2em\"><h1>{}</h1><p>{}</p>",
            escape_html(heading),
            escape_html(message)
        );
        let url = self.url.clone().unwrap_or_else(about_blank);
        self.set_document(
            swb_dom::parse_html(&html),
            url,
            Images::default(),
            encoding_rs::UTF_8,
        );
        self.state = LoadState::Failed;
    }

    /// Commits the pending navigation with `document` in `encoding`.
    /// `images` contains images that are already loaded.
    fn set_document(
        &mut self,
        document: Document,
        url: Url,
        images: Images,
        encoding: &'static Encoding,
    ) {
        self.commit_navigation(&url);
        // Requests that are still pending belong to the previous document
        // (for example images that a restyle of it started).
        self.loader.cancel_all();
        self.requests = Requests::default();
        self.base_url = Some(base_url(&document, &url));
        self.set_document_url(url);
        self.title = document_title(&document);
        self.tree_order = TreeOrder::new(&document);
        self.forms = Forms::new(&document);
        self.encoding = encoding;
        self.document = Some(document);
        self.sheets.clear();
        self.images = images;
        self.web_fonts = WebFonts::default();
        self.fonts.reset_web_fonts();
        self.scroll = Point::default();
        self.scrollers = Scrollers::default();
        // The pointer stays where it is; the next mouse movement updates the
        // hover state for the new document.
        self.input = self.input.for_new_document();
        // Set without a restyle: the styles of the new document are not
        // computed yet.
        self.input.states.target = self.document.as_ref().and_then(|doc| {
            let fragment = self.document_url.as_ref()?.fragment()?;
            indicated(doc, fragment)
        });
        if let Some(doc) = &self.document {
            self.input.states.controls = Arc::new(self.forms.element_states(doc));
        }
        self.start_subresources();
        self.invalidate_style();
        self.state = LoadState::LoadingResources;
        self.update_load_state();
    }

    /// Finds stylesheets, video posters and images in the document and
    /// starts loading them.
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
                    if !element.attr("rel").is_some_and(is_stylesheet_rel) {
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
                // The poster of a video loads like an image. The media
                // resource itself is never fetched (swb plays no media).
                // The sources of images are selected after this walk.
                local_name!("video") => {
                    if let Some(url) = element
                        .attr("poster")
                        .map(|s| s.trim_matches(is_html_whitespace))
                        .filter(|s| !s.is_empty())
                        .and_then(|src| base.join(src).ok())
                    {
                        image_loads.push(url.clone());
                        self.images
                            .by_node
                            .insert(node, SelectedImage { url, density: 1.0 });
                    }
                }
                _ => {}
            }
        }
        self.start_stylesheets(sheet_loads);
        for url in image_loads {
            self.start_image(url);
        }
        self.select_image_sources();
    }

    /// Starts the requests of the external stylesheets `loads` (slot,
    /// URL); a stylesheet that the document may not load fails.
    fn start_stylesheets(&mut self, loads: Vec<(usize, Url)>) {
        for (slot, url) in loads {
            if !self.may_load_subresource(&url) {
                if let Some(sheet) = self.sheets.get_mut(slot) {
                    sheet.failed = true;
                }
                continue;
            }
            let request = Request::get(url, Destination::Style);
            let id = self
                .loader
                .start(request.with_initiator(self.document_origin()));
            self.requests
                .pending
                .insert(id, Pending::Stylesheet { slot });
        }
    }

    /// Selects the source of each image for the current viewport and scale
    /// (`crate::image_source`) and starts loading the new ones: when the
    /// subresources of a document start to load, and at the next style
    /// computation after a viewport or scale change
    /// (`Images::sources_outdated`), so that a series of resize events
    /// selects once. An image whose source changes keeps its current image
    /// until the new one has loaded (`Images::reselect`); the dimension
    /// sources change at once.
    pub(super) fn select_image_sources(&mut self) {
        self.images.sources_outdated = false;
        let (Some(doc), Some(base)) = (&self.document, self.base_url.clone()) else {
            return;
        };
        // `stop` and navigations cancel requests but leave their images
        // `Loading`: forget those, so that a new selection requests them.
        let in_flight: HashSet<&Url> = self
            .requests
            .pending
            .values()
            .filter_map(|p| match p {
                Pending::Image { url } => Some(url),
                _ => None,
            })
            .collect();
        self.images.forget_cancelled(&in_flight);
        let env = self.media_environment();
        let mut dimension_sources = HashMap::new();
        let mut loads = Vec::new();
        let mut auto_nodes = Vec::new();
        for (node, selection) in image_source::select_images(doc, &base, &env) {
            if let Some(source) = selection.dimension_source {
                dimension_sources.insert(node, source);
            }
            // The width of the box decides: selected after layout.
            if selection.auto {
                auto_nodes.push(node);
                continue;
            }
            // Without a candidate, the image stays as it is (the
            // specification returns early).
            if let Some(image) = selection.image {
                let url = image.url.clone();
                if self.images.reselect(node, image) {
                    loads.push(url);
                }
            }
        }
        self.input.states.dimension_sources = Arc::new(dimension_sources);
        let kept: HashSet<NodeId> = auto_nodes.iter().copied().collect();
        self.images.auto.retain(|node, _| kept.contains(node));
        self.images.auto_nodes = auto_nodes;
        for url in loads {
            self.start_image(url);
        }
    }

    /// Starts loading the image at `url`, unless it is known already or
    /// the document may not load it (then it fails).
    pub(super) fn start_image(&mut self, url: Url) {
        if self.images.by_url.contains_key(&url) {
            return;
        }
        if !self.may_load_subresource(&url) {
            self.images.by_url.insert(url.clone(), ImageState::Failed);
            self.images.load_ended(&url);
            return;
        }
        log::debug!("image request: {url}");
        self.images.by_url.insert(url.clone(), ImageState::Loading);
        let request = Request::get(url.clone(), Destination::Image);
        let id = self
            .loader
            .start(request.with_initiator(self.document_origin()));
        self.requests.pending.insert(id, Pending::Image { url });
    }

    fn update_load_state(&mut self) {
        if self.state == LoadState::LoadingResources && self.requests.is_empty() {
            self.state = LoadState::Complete;
            log::info!("loaded {}", self.url.as_ref().map_or("", Url::as_str));
        }
    }

    /// True if every stylesheet arrived or failed: rendering can start.
    pub(super) fn stylesheets_settled(&self) -> bool {
        self.sheets.iter().all(SheetSlot::is_settled)
    }

    /// True if the current document may load `url`: only `file:` documents
    /// may load `file:` URLs, as in Chromium ("Not allowed to load local
    /// resource"). Addresses typed by the user are not restricted.
    pub(super) fn may_load_subresource(&self, url: &Url) -> bool {
        is_loadable(url)
            && (url.scheme() != "file"
                || self
                    .document_url
                    .as_ref()
                    .is_some_and(|document| document.scheme() == "file"))
    }
}

fn decode_css(bytes: &[u8], charset: Option<&str>) -> String {
    // CSS Syntax §3.2 (<https://www.w3.org/TR/css-syntax-3/#input-byte-stream>):
    // BOM, then the protocol charset, then @charset, then UTF-8. The BOM
    // (handled by `decode`), the protocol charset and UTF-8 are supported.
    let encoding = charset
        .and_then(|c| Encoding::for_label(c.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

/// The document base URL: the `href` of the first `<base>` element that
/// has one, resolved against the document URL
/// (<https://html.spec.whatwg.org/multipage/urls-and-fetching.html#document-base-url>).
fn base_url(doc: &Document, url: &Url) -> Url {
    doc.find_element(NodeId::DOCUMENT, |e| {
        e.is_html_named(&local_name!("base")) && e.has_attr("href")
    })
    .and_then(|base| doc.element(base)?.attr("href"))
    .and_then(|href| url.join(href.trim()).ok())
    .unwrap_or_else(|| url.clone())
}

/// The document title: the text of the `<title>` element, with ASCII
/// whitespace stripped and collapsed
/// (<https://html.spec.whatwg.org/multipage/dom.html#document.title>).
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

/// True for a `rel` value that makes a `link` a primary stylesheet:
/// `stylesheet` without `alternate`.
/// <https://html.spec.whatwg.org/multipage/links.html#link-type-stylesheet>
fn is_stylesheet_rel(rel: &str) -> bool {
    let has = |word: &str| {
        rel.split_ascii_whitespace()
            .any(|r| r.eq_ignore_ascii_case(word))
    };
    has("stylesheet") && !has("alternate")
}
