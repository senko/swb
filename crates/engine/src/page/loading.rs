//! Loading: network completions, the document of a navigation (or an
//! error page), and its subresources (stylesheets and images).

use std::time::{Duration, Instant};

use swb_dom::{Document, NodeId, local_name};
use swb_layout::Point;
use swb_net::{Destination, Request, Response, Url};

use super::scroll::indicated;
use super::{LoadState, Page, about_blank, is_loadable};
use crate::resources::{ImageState, Images, Pending, Requests, SheetSlot};
use crate::selection::TreeOrder;

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
            let started = Instant::now();
            let document = swb_dom::parse_html_bytes(&response.body, charset).0;
            self.timings.parse = started.elapsed();
            document
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
        self.tree_order = TreeOrder::new(&document);
        self.document = Some(document);
        self.sheets.clear();
        self.images = images;
        self.scroll = Point::default();
        // The pointer stays where it is; the next mouse movement updates the
        // hover state for the new document.
        self.input = self.input.for_new_document();
        // Set without a restyle: the styles of the new document are not
        // computed yet.
        self.input.states.target = self.document.as_ref().and_then(|doc| {
            let fragment = self.document_url.as_ref()?.fragment()?;
            indicated(doc, fragment)
        });
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
            let request = Request::get(url, Destination::Style);
            let id = self
                .loader
                .start(request.with_initiator(self.document_origin()));
            self.requests
                .pending
                .insert(id, Pending::Stylesheet { slot });
        }
        for url in image_loads {
            self.start_image(url);
        }
    }

    pub(super) fn start_image(&mut self, url: Url) {
        if self.images.by_url.contains_key(&url) {
            return;
        }
        if !self.may_load_subresource(&url) {
            self.images.by_url.insert(url, ImageState::Failed);
            return;
        }
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
        .and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes()))
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

/// Escapes text for inclusion in generated HTML.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
