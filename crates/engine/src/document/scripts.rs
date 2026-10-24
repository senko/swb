//! What the script runtime needs of the document for its script elements
//! (ADR 0028 section 8, M8 feature 5c): the fetches of classic scripts and
//! their results, and the style sheets that block scripts.
//!
//! A fetch is a `GET` with the destination `script` from the document's
//! origin. At most [`MAX_SCRIPT_FETCHES`] run at once; the next ones wait
//! in a queue (ADR 0028 section 16) and still delay the load event. A
//! response with an error status, a type that a script may not have
//! (`image/*`, `audio/*`, `video/*`, `text/csv`), or a type that is not
//! JavaScript under `X-Content-Type-Options: nosniff` is a network error
//! (Fetch "should response to request be blocked due to its MIME type?"
//! and "... due to nosniff?"). The text is decoded with the BOM, else the
//! response's charset, else the document's encoding (HTML "fetch a classic
//! script"; the `charset` attribute is not used).

use encoding_rs::Encoding;
use swb_dom::NodeId;
use swb_net::{Destination, NetError, Request, Response, Url};

use super::DocumentState;
use crate::resources::{MAX_SCRIPT_FETCHES, Pending};

impl DocumentState {
    /// Starts the fetch of the script at `url` for the script element
    /// `node`, or queues it. False if the document may not load it.
    pub(crate) fn fetch_script(&mut self, node: NodeId, url: &str) -> bool {
        let Ok(url) = Url::parse(url) else {
            return false;
        };
        if !self.may_load(&url) {
            log::warn!("not allowed to load the script {url}");
            return false;
        }
        if self.script_fetches.in_flight >= MAX_SCRIPT_FETCHES {
            self.script_fetches.queued.push_back((node, url));
        } else {
            self.start_script_fetch(node, url);
        }
        true
    }

    fn start_script_fetch(&mut self, node: NodeId, url: Url) {
        #[cfg(test)]
        self.load_log.push(format!("script {url}"));
        let request = Request::get(url, Destination::Script).with_initiator(Some(self.origin()));
        let id = self.loader.start(request);
        self.requests.pending.insert(id, Pending::Script { node });
        self.script_fetches.in_flight += 1;
    }

    /// The response of a script fetch: the decoded script, or `None` for a
    /// network error. The next queued fetch starts.
    pub(super) fn script_arrived(&mut self, node: NodeId, result: Result<Response, NetError>) {
        self.script_fetches.in_flight = self.script_fetches.in_flight.saturating_sub(1);
        let script = match result {
            Ok(response) => self.script_of(&response),
            Err(error) => {
                log::warn!("script failed: {error}");
                None
            }
        };
        self.script_fetches.arrived.push((node, script));
        if let Some((next, url)) = self.script_fetches.queued.pop_front() {
            self.start_script_fetch(next, url);
        }
    }

    /// The script of a response, or `None` if it is a network error for a
    /// script.
    fn script_of(&self, response: &Response) -> Option<swb_web::FetchedScript> {
        if !response.is_success() {
            log::warn!("script {} failed: HTTP {}", response.url, response.status);
            return None;
        }
        let content_type = response.content_type();
        let essence = content_type.as_ref().map_or("", |c| c.essence.as_str());
        let blocked = essence.starts_with("audio/")
            || essence.starts_with("image/")
            || essence.starts_with("video/")
            || essence == "text/csv";
        let nosniff = response
            .headers
            .get("x-content-type-options")
            .and_then(|v| v.split(',').next())
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("nosniff"));
        if blocked || (nosniff && !swb_web::is_javascript_mime_type(essence)) {
            log::warn!(
                "script {} refused: its MIME type ('{essence}') is not executable",
                response.url
            );
            return None;
        }
        let encoding = content_type
            .as_ref()
            .and_then(|c| c.charset.as_deref())
            .and_then(|label| Encoding::for_label(label.as_bytes()))
            .unwrap_or(self.encoding);
        let (text, _, _) = encoding.decode(&response.body);
        Some(swb_web::FetchedScript {
            source: text.into_owned(),
            url: response.url.to_string(),
        })
    }

    /// The scripts that arrived since the last call, for the runtime.
    pub(crate) fn take_arrived_scripts(&mut self) -> Vec<(NodeId, Option<swb_web::FetchedScript>)> {
        std::mem::take(&mut self.script_fetches.arrived)
    }

    /// Drops the fetches of scripts (the document's scripts stopped): the
    /// queue and the results that wait are discarded, and the responses of
    /// the requests in flight are ignored.
    pub(crate) fn abandon_scripts(&mut self) {
        self.script_fetches = crate::resources::ScriptFetches::default();
        self.requests
            .pending
            .retain(|_, pending| !matches!(pending, Pending::Script { .. }));
    }

    /// True if a style sheet blocks scripts: the parser inserted its
    /// `link`, it has not arrived or failed, and its media match.
    pub(crate) fn style_sheets_block_scripts(&mut self) -> bool {
        // The sheets of the elements that scripts inserted come first.
        self.process_changes();
        let env = self.media_environment();
        self.sheets.iter().any(|sheet| {
            sheet.blocks_scripts
                && !sheet.is_settled()
                && sheet
                    .media
                    .as_deref()
                    .is_none_or(|m| swb_css::MediaQueryList::parse_str(m).matches(&env))
        })
    }
}
