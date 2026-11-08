//! What the script runtime needs of the document for its script elements
//! (ADR 0028 section 8, M8 feature 5c, M9 feature 7) and its module map
//! (ADR 0034 sections 8 and 10): the fetches of classic scripts and modules
//! and their results, and the style sheets that block scripts.
//!
//! A module fetch of `import()` does not delay the `load` event: it does not
//! count in [`DocumentState::is_loading`], only in the page's activity.
//!
//! A fetch is a `GET` with the destination `script` from the document's
//! origin. A classic script is fetched in the mode `no-cors`; a module
//! script in the mode `cors` with the credentials mode `same-origin`, or
//! `include` for `crossorigin=use-credentials` (HTML "fetch a module
//! script graph"), so that a response from another origin needs
//! `Access-Control-Allow-Origin`. At most [`MAX_SCRIPT_FETCHES`] run at
//! once; the next ones wait in a queue (ADR 0028 section 16) and still
//! delay the load event. A
//! response with an error status, a type that a script may not have
//! (`image/*`, `audio/*`, `video/*`, `text/csv`), or a type that is not
//! JavaScript under `X-Content-Type-Options: nosniff` is a network error
//! (Fetch "should response to request be blocked due to its MIME type?"
//! and "... due to nosniff?"). The text is decoded with the BOM, else the
//! response's charset, else the document's encoding (HTML "fetch a classic
//! script"; the `charset` attribute is not used). A module script must have
//! a JavaScript type, also without `nosniff`, and its text is decoded with
//! the BOM, else the response's charset, else UTF-8 (measured in Chromium,
//! `tools/probes/web-module-scripts.json`, case `encoding-page`; HTML says
//! UTF-8 only).

use encoding_rs::Encoding;
use swb_dom::NodeId;
use swb_net::{CredentialsMode, Destination, NetError, Request, RequestMode, Response, Url};
use swb_web::{ModuleFetch, ScriptFetchMode};

use super::DocumentState;
use crate::resources::{MAX_SCRIPT_FETCHES, Pending, ScriptTarget};

impl DocumentState {
    /// Starts the fetch of the script at `url` for the script element
    /// `node`, or queues it. False if the document may not load it.
    pub(crate) fn fetch_script(&mut self, node: NodeId, url: &str, mode: ScriptFetchMode) -> bool {
        self.fetch_script_for(ScriptTarget::Element(node), url, mode)
    }

    /// Starts the fetch of a module of the runtime's module map (`id`), or
    /// queues it. False if the document may not load it.
    pub(crate) fn fetch_module(&mut self, id: u64, url: &str, options: ModuleFetch) -> bool {
        let target = ScriptTarget::Module {
            id,
            delays_load: options.delays_load,
        };
        let mode = ScriptFetchMode::Module {
            credentials: options.credentials,
        };
        self.fetch_script_for(target, url, mode)
    }

    /// Makes the module fetch `id`, in flight or queued, delay the `load`
    /// event (a script element joined the fetch of an `import()`).
    pub(crate) fn delay_load_for_module(&mut self, id: u64) {
        let fetches = &mut self.script_fetches;
        for pending in self.requests.pending.values_mut() {
            if let Pending::Script {
                target:
                    ScriptTarget::Module {
                        id: fetch,
                        delays_load,
                    },
                ..
            } = pending
                && *fetch == id
            {
                if !*delays_load {
                    *delays_load = true;
                    fetches.quiet_in_flight = fetches.quiet_in_flight.saturating_sub(1);
                }
                return;
            }
        }
        for (target, _, _) in &mut fetches.queued {
            if let ScriptTarget::Module {
                id: fetch,
                delays_load,
            } = target
                && *fetch == id
            {
                if !*delays_load {
                    *delays_load = true;
                    fetches.quiet_queued = fetches.quiet_queued.saturating_sub(1);
                }
                return;
            }
        }
    }

    fn fetch_script_for(&mut self, target: ScriptTarget, url: &str, mode: ScriptFetchMode) -> bool {
        if self.retired {
            return false;
        }
        let Ok(url) = Url::parse(url) else {
            return false;
        };
        if !self.may_load(&url) {
            log::warn!("not allowed to load the script {url}");
            return false;
        }
        if self.script_fetches.in_flight >= MAX_SCRIPT_FETCHES {
            if target.is_quiet() {
                self.script_fetches.quiet_queued += 1;
            }
            self.script_fetches.queued.push_back((target, url, mode));
        } else {
            self.start_script_fetch(target, url, mode);
        }
        true
    }

    fn start_script_fetch(&mut self, target: ScriptTarget, url: Url, mode: ScriptFetchMode) {
        #[cfg(test)]
        self.load_log.push(format!("script {url}"));
        let mut request = Request::get(url, Destination::Script);
        if let ScriptFetchMode::Module { credentials } = mode {
            request.mode = RequestMode::Cors;
            request.credentials = if credentials {
                CredentialsMode::Include
            } else {
                CredentialsMode::SameOrigin
            };
        }
        let id = self.start_subresource(request);
        self.requests
            .pending
            .insert(id, Pending::Script { target, mode });
        self.script_fetches.in_flight += 1;
        if target.is_quiet() {
            self.script_fetches.quiet_in_flight += 1;
        }
    }

    /// The response of a script fetch: the decoded script, or `None` for a
    /// network error. The next queued fetch starts.
    pub(super) fn script_arrived(
        &mut self,
        target: ScriptTarget,
        mode: ScriptFetchMode,
        result: Result<Response, NetError>,
    ) {
        let fetches = &mut self.script_fetches;
        fetches.in_flight = fetches.in_flight.saturating_sub(1);
        if target.is_quiet() {
            fetches.quiet_in_flight = fetches.quiet_in_flight.saturating_sub(1);
        }
        let script = match result {
            Ok(response) => self.script_of(&response, mode),
            Err(error) => {
                log::warn!("script failed: {error}");
                None
            }
        };
        self.script_fetches.arrived.push((target, script));
        if let Some((next, url, next_mode)) = self.script_fetches.queued.pop_front() {
            if next.is_quiet() {
                let fetches = &mut self.script_fetches;
                fetches.quiet_queued = fetches.quiet_queued.saturating_sub(1);
            }
            self.start_script_fetch(next, url, next_mode);
        }
    }

    /// The script of a response, or `None` if it is a network error for a
    /// script.
    fn script_of(
        &self,
        response: &Response,
        mode: ScriptFetchMode,
    ) -> Option<swb_web::FetchedScript> {
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
        let module = matches!(mode, ScriptFetchMode::Module { .. });
        // A module needs a JavaScript type (HTML "fetch a single module
        // script": a response with another MIME type essence is a network
        // error), with or without `nosniff`.
        if blocked || ((nosniff || module) && !swb_web::is_javascript_mime_type(essence)) {
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
            .unwrap_or(if module {
                encoding_rs::UTF_8
            } else {
                self.encoding
            });
        let (text, _, _) = encoding.decode(&response.body);
        Some(swb_web::FetchedScript {
            source: text.into_owned(),
            url: response.url.to_string(),
        })
    }

    /// The scripts that arrived since the last call, for the runtime, in
    /// order of arrival.
    pub(crate) fn take_arrived_scripts(
        &mut self,
    ) -> Vec<(ScriptTarget, Option<swb_web::FetchedScript>)> {
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
