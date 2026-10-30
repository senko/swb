//! Loading: network completions, and the document of a navigation (or an
//! error page). The subresources of a document are in
//! `crate::document`.
//!
//! With JavaScript off, a document is parsed to the end at once
//! (`parse_html*`, no journal); the document state then walks it once. With
//! JavaScript on, the parser runs in steps (`HtmlParser`), which stop at
//! each script end tag; the document state processes the journal after
//! each step, so the loads of the elements start as they are inserted.
//! The script runtime drives the steps and runs the scripts at their place
//! (`script.rs`, feature 5c).
//!
//! A document with a script runtime is loaded when its `load` event has
//! fired ([`Page::is_fully_loaded`]); requests that scripts start after it
//! do not count. Without a runtime, it is loaded when no request runs
//! after a layout.

use std::rc::Rc;
use std::time::{Duration, Instant};

use encoding_rs::Encoding;
use swb_dom::{Document, HtmlParser};
use swb_net::{Response, Url, escape_html};

use super::{LoadState, Page, about_blank};
use crate::document::{DocumentInit, DocumentState, indicated};
use crate::resources::{ImageState, Images, Pending};

/// A parsed or new document and its parser.
struct Parsed {
    dom: Document,
    parser: Option<HtmlParser>,
    encoding: &'static Encoding,
    /// The content type of the document.
    content_type: String,
    /// The time of a parse to the end (a parse in steps measures itself).
    parse: Duration,
}

impl Parsed {
    /// A generated document (text, an image, an error message) of
    /// `content_type`, parsed to the end, in UTF-8.
    fn complete(dom: Document, content_type: &str) -> Parsed {
        Parsed {
            dom,
            parser: None,
            encoding: encoding_rs::UTF_8,
            content_type: content_type.to_owned(),
            parse: Duration::ZERO,
        }
    }
}

impl Page {
    /// Processes completed network requests without blocking. Returns true
    /// if anything changed that needs a repaint.
    pub fn process_network(&mut self) -> bool {
        let mut changed = false;
        // While a script runtime holds the state, the completions wait in
        // the loader.
        while !self.lent
            && let Some(completion) = self.loader.try_recv()
        {
            changed |= self.handle_completion(completion);
        }
        changed
    }

    /// True if the page and all its subresources are loaded. Layout can
    /// discover more resources (background images), so the page is laid
    /// out first. With a script runtime: the `load` event has fired (it
    /// waited for the same resources; requests that scripts start later do
    /// not count).
    pub fn is_fully_loaded(&mut self) -> bool {
        if self.state == LoadState::LoadingDocument {
            return false;
        }
        if let Some(script) = &self.script {
            return script.document_loaded();
        }
        if self.is_loading() {
            return false;
        }
        self.update_layout();
        !self.is_loading()
    }

    /// True if the load that [`Page::wait_until_loaded`] waits for is over,
    /// without a layout.
    fn load_done(&self) -> bool {
        if self.state == LoadState::LoadingDocument {
            return false;
        }
        match &self.script {
            Some(script) => script.document_loaded(),
            None => !self.is_loading(),
        }
    }

    /// Blocks until the page and all subresources are loaded, or until
    /// `timeout` passes. Returns true if loading completed.
    pub fn wait_until_loaded(&mut self, timeout: Duration) -> bool {
        if self.lent {
            // The completions wait for the state; waiting here would not end.
            return false;
        }
        // A timeout too large to represent means "no deadline".
        let deadline = Instant::now().checked_add(timeout);
        while !self.load_done() {
            let now = Instant::now();
            let mut remaining = match deadline {
                Some(d) if now >= d => return false,
                Some(d) => d - now,
                None => Duration::from_secs(3600),
            };
            // Timers of the document run while it loads (JavaScript on).
            if let Some(next) = self.next_task_instant() {
                remaining = remaining.min(next.saturating_duration_since(now));
            }
            if let Some(completion) = self.loader.recv_timeout(remaining) {
                self.handle_completion(completion);
            }
            self.run_due_tasks();
            // Layout can discover more resources (background images); with
            // a runtime, the check of the load event lays out.
            if self.script.is_none() && !self.is_loading() {
                self.update_layout();
            }
        }
        true
    }

    /// Handles one completed request. Completions of requests that were
    /// cancelled (by a navigation or by [`Page::stop`]) are not pending and
    /// are ignored.
    pub(super) fn handle_completion(&mut self, completion: swb_net::Completion) -> bool {
        if self.document_request == Some(completion.id) {
            self.document_request = None;
            self.document_loaded(completion.result);
            return true;
        }
        let Some((doc, fonts)) = self.parts() else {
            return false;
        };
        if doc.is_api_fetch(completion.id) {
            doc.api_fetch_completed(
                completion.id,
                completion.result,
                completion.kind,
                completion.reservation,
            );
            self.deliver_api_fetches();
            return true;
        }
        let Some(pending) = doc.requests.pending.remove(&completion.id) else {
            return false;
        };
        let sheet = matches!(pending, Pending::Stylesheet { .. });
        doc.handle_completion(pending, completion.result, fonts);
        if !doc.is_loading() && self.state == LoadState::Complete {
            log::info!("loaded {}", self.url.as_ref().map_or("", Url::as_str));
        }
        // The events of images and style sheets fire before the `load`
        // event that this completion may allow.
        self.deliver_element_events();
        self.deliver_scripts(sheet);
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
        self.set_response(response.status, response.body.len() as u64);
        let content_type = response.content_type();
        let essence = content_type
            .as_ref()
            .map_or("text/html", |c| c.essence.as_str());
        let mut images = Images::default();
        let parsed = if essence == "text/html" || essence == "application/xhtml+xml" {
            let charset = content_type.as_ref().and_then(|c| c.charset.as_deref());
            let mut parsed = self.parse_document(&response.body, charset);
            essence.clone_into(&mut parsed.content_type);
            parsed
        } else if essence.starts_with("text/") || essence == "application/json" {
            let text = String::from_utf8_lossy(&response.body);
            Parsed::complete(
                swb_dom::parse_html_with(
                    &format!("<pre>{}</pre>", escape_html(&text)),
                    self.javascript,
                ),
                essence,
            )
        } else if essence.starts_with("image/") {
            // The image is already here; the generated <img> uses it
            // without fetching it again.
            let state = ImageState::decode(&response.url, &response, &images.counting);
            images.by_url.insert(response.url.clone(), state);
            Parsed::complete(
                swb_dom::parse_html_with(
                    &format!(
                        "<body style=\"margin:0\"><img src=\"{}\">",
                        escape_html(response.url.as_str())
                    ),
                    self.javascript,
                ),
                essence,
            )
        } else {
            self.show_error(&format!("Cannot display content of type {essence}"));
            return;
        };
        self.set_document(parsed, response.url, images);
    }

    /// Parses an HTML document from its bytes: to the end at once with
    /// JavaScript off, in steps with JavaScript on.
    fn parse_document(&self, bytes: &[u8], charset: Option<&str>) -> Parsed {
        if self.javascript {
            let (text, name) = swb_dom::decode_html_bytes(bytes, charset);
            let mut dom = Document::new();
            let parser = HtmlParser::new(&mut dom, &text, true);
            return Parsed {
                dom,
                parser: Some(parser),
                encoding: encoding_for(name),
                content_type: "text/html".to_owned(),
                parse: Duration::ZERO,
            };
        }
        let started = Instant::now();
        let (dom, name) = swb_dom::parse_html_bytes_with(bytes, charset, false);
        Parsed {
            dom,
            parser: None,
            encoding: encoding_for(name),
            content_type: "text/html".to_owned(),
            parse: started.elapsed(),
        }
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
        let parsed = Parsed::complete(
            swb_dom::parse_html_with(&html, self.javascript),
            "text/html",
        );
        self.set_document(parsed, url, Images::default());
        self.state = LoadState::Failed;
    }

    /// Commits the pending navigation with the document `parsed`. `images`
    /// contains images that are already loaded.
    fn set_document(&mut self, parsed: Parsed, url: Url, images: Images) {
        let navigation = self.navigation_info();
        let scroll_target = self.commit_navigation(&url);
        // The old document sees `pagehide` and `unload` first.
        self.unload_document();
        // Its `fetch()` calls end with it: a request that still runs gives
        // its place to the requests of the new document.
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.abandon_api_fetches();
        }
        // Requests that are still pending belong to the previous document
        // (for example images that a restyle of it started).
        self.loader.cancel_all();
        self.document_request = None;
        // The old document goes first, with its script runtime: its web
        // fonts leave the font context.
        self.script = None;
        self.doc = None;
        self.lent = false;
        self.fonts.reset_web_fonts();
        self.url = Some(url.clone());
        // The pointer stays where it is; the next mouse movement updates the
        // hover state for the new document.
        self.input = self.input.for_new_document();
        self.state = LoadState::Complete;
        let mut doc = DocumentState::new(DocumentInit {
            dom: parsed.dom,
            parser: parsed.parser,
            url,
            encoding: parsed.encoding,
            content_type: parsed.content_type,
            images,
            loader: Rc::clone(&self.loader),
            storage: Rc::clone(&self.storage),
            view: self.view,
            scroll_target,
            parse: parsed.parse,
        });
        // Set without a restyle: the styles of the new document are not
        // computed yet.
        doc.states.target = doc
            .url()
            .fragment()
            .and_then(|fragment| indicated(doc.dom(), fragment));
        doc.history = self.history_info();
        doc.navigation = navigation;
        if !doc.is_loading() {
            log::info!("loaded {}", self.url.as_ref().map_or("", Url::as_str));
        }
        self.doc = Some(Box::new(doc));
        self.start_scripts();
    }
}

/// The encoding of an encoding name from the decoder.
fn encoding_for(name: &str) -> &'static Encoding {
    Encoding::for_label(name.as_bytes()).unwrap_or(encoding_rs::UTF_8)
}
