//! The arrival and the commit of frame documents (ADR 0031 sections 2, 3 and
//! 11): the parse of `srcdoc`, the admission of a document by the budgets for
//! frame documents, the commit (the origin, the sandbox flags, the referrer,
//! the realm of the document) and the start of its parse. `frames.rs` has the
//! frame tree, `frame_load.rs` the load state.

use std::rc::Rc;

use swb_dom::QuirksMode;
use swb_net::{NetError, Origin, Response, Url, escape_html};
use swb_web::ScriptRuntime;

use super::Page;
use super::loading::Parsed;
use crate::document::DocumentState;
use crate::frames::registry::FrameDocument;
use crate::frames::{
    Charge, FrameId, FrameScript, MAX_NODES, MAX_SOURCE_BYTES, PendingNavigation, Sandbox,
};
use crate::resources::Images;

/// What a committed frame document has of its surroundings ([`Page::surroundings_of_commit`]).
struct Surroundings {
    sandboxed: bool,
    storage_site: Option<Rc<str>>,
    storage_partition: swb_web::StoragePartition,
    referrer: String,
}

impl Page {
    /// Parses the `srcdoc` attribute of frame `id`'s container, within the
    /// budget for frame documents (an error document otherwise).
    pub(super) fn parse_srcdoc(&mut self, id: FrameId, origin: &Origin) -> (Parsed, Charge) {
        let markup = self.srcdoc_of(id).map(str::to_owned);
        let markup = markup.as_deref().unwrap_or_default();
        if !self.admit_bytes(id, markup.len()) {
            return (self.over_budget_document(), Charge::default());
        }
        // The parse stops one node past the room that the budget has left.
        let max_nodes = self.frames.nodes_left(id).saturating_add(1);
        if self.frame_runs_scripts(id, origin) && may_have_script(markup.as_bytes(), None) {
            let mut dom = swb_dom::Document::new();
            dom.set_max_nodes(max_nodes);
            let parser = swb_dom::HtmlParser::new(&mut dom, markup, true);
            let mut parsed = Parsed::complete(dom, "text/html");
            parsed.parser = Some(parser);
            return (
                parsed,
                crate::frames::registry::scripted_charge(markup.len()),
            );
        }
        let mut dom = swb_dom::parse_html_limited(markup, self.javascript, max_nodes);
        // An iframe srcdoc document is never in quirks mode (HTML "the
        // initial insertion mode").
        dom.quirks_mode = QuirksMode::NoQuirks;
        let parsed = Parsed::complete(dom, "text/html");
        self.admit_parsed(id, parsed, markup.len(), max_nodes)
    }

    /// The `srcdoc` attribute of frame `id`'s container.
    fn srcdoc_of(&self, id: FrameId) -> Option<&str> {
        let frame = self.frames.get(id)?;
        let doc = self.doc_of(frame.parent?)?;
        doc.dom().element(frame.container?)?.attr("srcdoc")
    }

    /// The sandboxing flags of frame `id`'s container now.
    pub(super) fn sandbox_flags_now(&self, id: FrameId) -> Sandbox {
        let frame = self.frames.get(id);
        let parent = frame.and_then(|f| f.parent);
        let element = frame
            .and_then(|f| Some((f.parent?, f.container?)))
            .and_then(|(p, c)| self.doc_of(p)?.dom().element(c));
        match parent {
            Some(parent) => self.container_sandbox(parent, element.and_then(|e| e.attr("sandbox"))),
            None => Sandbox::default(),
        }
    }

    /// The document of frame `id`'s navigation is here: it has the flags of
    /// the navigation.
    pub(super) fn take_pending_sandbox(&mut self, id: FrameId) -> Sandbox {
        let Some(frame) = self.frames.get_mut(id) else {
            return Sandbox::default();
        };
        frame.sandbox = frame.pending_sandbox;
        frame.sandbox
    }

    /// The document of frame `id`'s navigation arrived (or failed).
    pub(super) fn frame_document_arrived(
        &mut self,
        id: FrameId,
        result: Result<Response, NetError>,
    ) {
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        let url = match frame.pending.take() {
            Some(PendingNavigation::Request { url, .. }) => url,
            other => {
                frame.pending = other;
                return;
            }
        };
        let sandbox = self.take_pending_sandbox(id);
        let opaque = |o: Origin| {
            if sandbox.opaque_origin() {
                Origin::new_opaque()
            } else {
                o
            }
        };
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                log::warn!("{id}: cannot load {url}: {error}");
                let parsed =
                    error_document(&format!("Cannot load {url}: {error}"), self.javascript);
                self.commit_frame_document(
                    id,
                    parsed,
                    url,
                    Origin::new_opaque(),
                    Images::default(),
                    Charge::default(),
                );
                return;
            }
        };
        if !response.is_success() {
            log::debug!("{id}: {}: HTTP {}", response.url, response.status);
        }
        let origin = opaque(response.url.origin());
        let size = response.body.len();
        if !self.admit_bytes(id, size) {
            let parsed = self.over_budget_document();
            let none = Charge::default();
            self.commit_frame_document(id, parsed, response.url, origin, Images::default(), none);
            return;
        }
        // The parse stops one node past the room that the budget has left,
        // so that a document that does not fit costs little.
        let max_nodes = self.frames.nodes_left(id).saturating_add(1);
        let charset = response.content_type().and_then(|c| c.charset);
        let steps = self.frame_runs_scripts(id, &origin)
            && may_have_script(&response.body, charset.as_deref());
        match self.parse_response(&response, steps, max_nodes) {
            Ok((parsed, images)) if parsed.parser.is_some() => {
                let charge = crate::frames::registry::scripted_charge(size);
                self.commit_frame_document(id, parsed, response.url, origin, images, charge);
            }
            Ok((parsed, images)) => {
                let (parsed, charge) = self.admit_parsed(id, parsed, size, max_nodes);
                // An over-budget document has no images either.
                let images = if charge == Charge::default() {
                    Images::default()
                } else {
                    images
                };
                self.commit_frame_document(id, parsed, response.url, origin, images, charge);
            }
            Err(essence) => {
                let parsed = error_document(
                    &format!("Cannot display content of type {essence}"),
                    self.javascript,
                );
                let none = Charge::default();
                self.commit_frame_document(
                    id,
                    parsed,
                    response.url,
                    origin,
                    Images::default(),
                    none,
                );
            }
        }
    }

    /// True if a document of `len` bytes of source fits the page's budget
    /// for frame documents when it replaces frame `id`'s (ADR 0031 section
    /// 11). One warning per top-level document otherwise.
    fn admit_bytes(&mut self, id: FrameId, len: usize) -> bool {
        self.frames.fits_bytes(id, len) || self.warn_over_budget()
    }

    /// Admits the parsed document of `bytes` of source if its nodes fit the
    /// budget, else gives an error document. A parse that reached
    /// `max_nodes` stopped early, and the page must not show a part of the
    /// document, even if the part would fit. Returns the document and what it
    /// costs.
    fn admit_parsed(
        &mut self,
        id: FrameId,
        parsed: Parsed,
        bytes: usize,
        max_nodes: usize,
    ) -> (Parsed, Charge) {
        let nodes = parsed.dom.len();
        if nodes < max_nodes && self.frames.fits_nodes(id, nodes) {
            return (parsed, Charge { bytes, nodes });
        }
        self.frames.budget.note_rejection();
        self.warn_over_budget();
        (self.over_budget_document(), Charge::default())
    }

    /// Warns once per top-level document that a frame document did not fit
    /// the budget. Returns false.
    fn warn_over_budget(&mut self) -> bool {
        if !std::mem::replace(&mut self.frames.budget.warned, true) {
            log::warn!(
                "the frame documents of the page have {} MiB of source or {} million nodes, the limit; later ones show an error",
                MAX_SOURCE_BYTES >> 20,
                MAX_NODES >> 20
            );
        }
        false
    }

    /// The document of a frame that did not fit the budget.
    fn over_budget_document(&self) -> Parsed {
        error_document("Too many frame documents on this page", self.javascript)
    }

    /// True if a document of origin `origin` in frame `id` may run scripts: the
    /// top-level document has scripting on, its sandbox has `allow-scripts`, and
    /// its origin has a runtime or can get one.
    fn frame_runs_scripts(&mut self, id: FrameId, origin: &Origin) -> bool {
        self.javascript
            && self.agents.enabled
            && self
                .frames
                .get(id)
                .is_some_and(|f| f.sandbox.scripts_allowed())
            && self.may_host_scripts(origin)
    }

    /// Replaces the document of frame `id` with a new one: the old one
    /// unloads and goes with its frames and requests. `charge` is what the
    /// document costs the page's budget for frame documents.
    pub(super) fn commit_frame_document(
        &mut self,
        id: FrameId,
        parsed: Parsed,
        url: Url,
        origin: Origin,
        images: Images,
        charge: Charge,
    ) {
        self.unload_for_commit(id);
        let Some(frame) = self.frames.get(id) else {
            return;
        };
        let Some(parent) = frame.parent else {
            return;
        };
        let viewport = frame.viewport;
        for child in frame.children.clone() {
            self.destroy_frame(child);
        }
        let (fallback_base, creator_url) = self.creator_of_commit(parent, &url, &origin);
        let history = self
            .doc
            .as_deref()
            .map(|d| d.history.clone())
            .unwrap_or_default();
        let Surroundings {
            sandboxed,
            storage_site,
            storage_partition,
            referrer,
        } = self.surroundings_of_commit(id, parent, &url, &origin, creator_url.as_ref());
        let mut doc = self.maker.make(FrameDocument {
            dom: parsed.dom,
            parser: parsed.parser,
            encoding: parsed.encoding,
            content_type: parsed.content_type,
            parse: parsed.parse,
            url,
            origin: origin.clone(),
            site_for_cookies: Some(self.frames.site_for_cookies(parent, Some(&origin))),
            view: self.frame_settings().view(viewport),
            images,
            fallback_base,
            creator_url,
            history,
            node_room: Rc::clone(&self.frames.node_room),
            sandboxed_origin: sandboxed,
            storage_partition,
            referrer,
        });
        doc.frame_id = id;
        let refresh = doc
            .declarative_refresh()
            .map(|r| (self.clock.now() + r.delay_s * 1000.0, r.url));
        let client = doc.client;
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        let mut old = frame.doc.replace(doc);
        let old_key = frame.script.take();
        let origin_stays = frame.origin.as_ref() == Some(&origin);
        let reuse = frame.reuse_window && origin_stays;
        frame.reuse_window = false;
        if !origin_stays {
            frame.activation = None;
        }
        frame.origin = Some(origin);
        frame.storage_site = storage_site;
        // The new document has its own pointer and keyboard state; the
        // focus stays with the frame (measured: a navigated frame that
        // held the focus keeps it, with no focused element in it).
        self.frame_inputs.remove(&id);
        if self
            .input
            .press_frame
            .as_ref()
            .is_some_and(|p| p.frame == id)
        {
            self.input.press_frame = None;
        }
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        frame.load_fired = false;
        frame.refresh = refresh;
        frame.canvas = None;
        let old_client = old.as_ref().map(|d| d.client);
        if let Some(old) = old_client {
            self.loader.cancel_client(old);
        }
        if let Some(old) = old.as_deref_mut() {
            old.release_own_fonts(&mut self.fonts);
        }
        self.frames.set_client(id, old_client, client);
        self.frames.set_charge(id, charge);
        let kept = self.retire_old_document(old, old_key, reuse);
        log::debug!(
            "{id}: committed {}",
            self.doc_of(id).map_or("", |d| d.url().as_str())
        );
        self.start_frame_document(id, kept);
        self.update_frames(id);
    }

    /// The old document of frame `id` unloads before a commit replaces it. The `storage`
    /// events that wait for it come first: Chromium's come before a pending navigation
    /// commits (measured, case `storage-event-retire`).
    fn unload_for_commit(&mut self, id: FrameId) {
        if self.frames.get(id).is_some_and(|f| f.script.is_some()) {
            self.run_task_in(id, |script| {
                script.flush_storage_events();
                script.unload_document();
            });
        }
    }

    /// The creator of an `about:` document that a child of `parent` commits at `url` with
    /// `origin`: the fallback base URL, and the creator's URL of the cookies and storage
    /// (when the origin is the parent's).
    fn creator_of_commit(
        &self,
        parent: FrameId,
        url: &Url,
        origin: &Origin,
    ) -> (Option<Url>, Option<Url>) {
        let creator = (url.scheme() == "about")
            .then(|| self.doc_of(parent))
            .flatten();
        let fallback_base = creator.map(|d| d.base_url().clone());
        let same_origin = self
            .frames
            .get(parent)
            .is_some_and(|p| p.origin.as_ref() == Some(origin));
        let creator_url = creator
            .filter(|_| same_origin)
            .map(|d| d.cookie_url().clone());
        (fallback_base, creator_url)
    }

    /// What the document that frame `id` commits at `url` has of its surroundings: whether
    /// its sandbox gives it an opaque origin, its site and storage partition, and its
    /// referrer.
    fn surroundings_of_commit(
        &mut self,
        id: FrameId,
        parent: FrameId,
        url: &Url,
        origin: &Origin,
        creator_url: Option<&Url>,
    ) -> Surroundings {
        let sandboxed = self
            .frames
            .get(id)
            .is_some_and(|f| f.sandbox.opaque_origin());
        let storage_site =
            crate::frames::storage_site_of(origin, creator_url.unwrap_or(url), sandboxed);
        let storage_partition = self
            .frames
            .storage_partition(parent, storage_site.as_deref());
        let referrer = self.referrer_of_commit(id, parent, url);
        Surroundings {
            sandboxed,
            storage_site,
            storage_partition,
            referrer,
        }
    }

    /// `document.referrer` of the document that frame `id` commits at `url`
    /// ([`crate::referrer`]): the source is the document that started the navigation, else
    /// the parent.
    fn referrer_of_commit(&mut self, id: FrameId, parent: FrameId, url: &Url) -> String {
        let started_by = self
            .frames
            .get_mut(id)
            .and_then(|f| f.pending_source.take());
        let Some(source) =
            started_by.or_else(|| self.doc_of(parent).map(|d| d.cookie_url().clone()))
        else {
            return String::new();
        };
        let target = match url.as_str() {
            "about:srcdoc" => crate::referrer::Target::Srcdoc,
            _ if url.scheme() == "about" => crate::referrer::Target::Blank,
            _ => crate::referrer::Target::Url(url),
        };
        crate::referrer::referrer(&source, target)
    }

    /// The document that a commit replaced: with a realm it stays in the
    /// runtime (ADR 0031 section 5). With `reuse`, the new document takes over
    /// its `Window` (HTML "initial about:blank"; measured, case
    /// `scripted-initial-reuse`); returns its key.
    fn retire_old_document(
        &mut self,
        old: Option<Box<DocumentState>>,
        script: Option<FrameScript>,
        reuse: bool,
    ) -> Option<FrameScript> {
        let script = script?;
        let agent = script.agent();
        let kept = if reuse {
            let key = script.key_in(agent)?;
            self.runtime_of_mut(agent)?
                .reuse_window(key)
                .map(|kept| FrameScript::new(agent, kept))
        } else {
            None
        };
        let nodes = old.as_ref().map_or(0, |d| d.node_count());
        self.retire_in_agent(script, old, nodes);
        kept
    }

    /// A new document of frame `id` starts. With a script to run it gets a
    /// realm in the runtime of its origin, which is made now if it has none, and
    /// the runtime drives its parse. Each runtime that has seen the frame's proxy
    /// holds it for a script, so the proxy needs a new target: a realm in the
    /// runtime of the document's origin, none in the others (ADR 0031 section 5).
    /// Else the document needs no realm yet.
    fn start_frame_document(&mut self, id: FrameId, kept: Option<FrameScript>) {
        let number = id.number();
        if !self.agents.enabled {
            self.finish_frame_parse(id);
            return;
        }
        let parsing = self.doc_of(id).is_some_and(DocumentState::is_parsing);
        let origin = self.frames.get(id).and_then(|f| f.origin.clone());
        let home = origin.as_ref().and_then(|o| self.agent_of_origin(o));
        self.clear_frame_targets_except(id, home);
        let known = home
            .and_then(|agent| self.runtime_of(agent))
            .is_some_and(|runtime| runtime.knows_frame(number));
        let script = match (kept, home) {
            (Some(kept), _) => Some(kept),
            (None, Some(agent)) if parsing || known => self
                .with_registry(agent, |s| s.add_frame_document(number))
                .flatten()
                .map(|key| FrameScript::new(agent, key)),
            (None, None) if parsing => origin.and_then(|origin| self.make_agent(id, origin)),
            (None, _) => None,
        };
        let scripts_off = !self
            .frames
            .get(id)
            .is_some_and(|f| f.sandbox.scripts_allowed());
        if let Some(frame) = self.frames.get_mut(id) {
            frame.script = script;
        }
        if let Some((runtime, key)) = self.frame_runtime_mut(id) {
            runtime.set_scripts_off(key, scripts_off);
        }
        if script.is_none() {
            self.finish_frame_parse(id);
            if known && let Some(runtime) = home.and_then(|agent| self.runtime_of_mut(agent)) {
                runtime.clear_frame_target(number);
            }
        } else if parsing {
            self.run_task_in(id, ScriptRuntime::start_document);
        } else if let Some((runtime, key)) = self.frame_runtime_mut(id) {
            // Its `load` is to come: the window that a script holds gets it
            // (measured, case `scripted-initial-reuse`).
            runtime.adopt_complete_document(key, true);
        }
    }

    /// A parse in steps of frame `id`'s document goes to its end without
    /// scripts; the document is charged for its nodes.
    fn finish_frame_parse(&mut self, id: FrameId) {
        let Some(doc) = self.doc_of_mut(id) else {
            return;
        };
        if !doc.is_parsing() {
            return;
        }
        doc.finish_parse_without_scripts();
        self.update_frame_charge(id);
        self.update_frames(id);
    }

    /// Charges the nodes that frame `id`'s document has now (a document that
    /// a script or a parse in steps grew).
    pub(super) fn update_frame_charge(&mut self, id: FrameId) {
        let Some(frame) = self.frames.get(id) else {
            return;
        };
        let Some(doc) = frame.doc.as_deref() else {
            return;
        };
        let charge = Charge {
            bytes: frame.charge.bytes,
            nodes: doc.node_count(),
        };
        if charge != frame.charge {
            self.frames.set_charge(id, charge);
        }
    }
}

/// True if the source of a document can have a script to run: a `script`
/// element, or an attribute `on...` with a value. A false positive costs a realm.
/// A document counts as a positive where the bytes do not tell: a byte order
/// mark or a `charset` (of the HTTP header) that names an encoding that is not
/// compatible with ASCII, and an escape byte, which can hide markup in
/// ISO-2022-JP (a `meta` can name that encoding; measured).
fn may_have_script(bytes: &[u8], charset: Option<&str>) -> bool {
    let wide = charset
        .and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes()))
        .is_some_and(|encoding| !encoding.is_ascii_compatible());
    if wide
        || bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
        || bytes.contains(&0x1B)
    {
        return true;
    }
    let lower = |b: &u8| b.to_ascii_lowercase();
    bytes
        .windows(7)
        .any(|w| w[0] == b'<' && w[1..].iter().map(lower).eq(b"script".iter().copied()))
        // A link or a form that runs a script when the user activates it.
        || bytes
            .windows(11)
            .any(|w| w.eq_ignore_ascii_case(b"javascript:"))
        || bytes.iter().enumerate().any(|(index, &byte)| {
            if byte != b'=' {
                return false;
            }
            let head = bytes[..index].trim_ascii_end();
            let letters = head
                .iter()
                .rev()
                .take_while(|b| b.is_ascii_alphabetic())
                .count();
            letters >= 3 && {
                let name = &head[head.len() - letters..];
                name[..2].eq_ignore_ascii_case(b"on")
            }
        })
}

/// The URL of an `about:srcdoc` document.
pub(super) fn srcdoc_url() -> Url {
    Url::parse("about:srcdoc").expect("about:srcdoc is a valid URL")
}

/// A frame document that shows a load error.
fn error_document(message: &str, scripting: bool) -> Parsed {
    let html = format!(
        "<!DOCTYPE html><title>Error</title><body style=\"font-family:sans-serif\"><p>{}</p>",
        escape_html(message)
    );
    Parsed::complete(swb_dom::parse_html_with(&html, scripting), "text/html")
}

#[cfg(test)]
mod tests {
    use super::may_have_script;

    #[test]
    fn the_source_scan_finds_each_form_of_a_script() {
        for source in [
            "<script>",
            "<SCRIPT src=a>",
            "<svg><script>",
            "<body onload=a>",
            "<body\nONLOAD\n=\na>",
            "<img/onerror=a>",
        ] {
            assert!(may_have_script(source.as_bytes(), None), "{source}");
        }
        for source in ["", "<p>on</p>", "<a href=b>", "<a on=b>", "<scrip>"] {
            assert!(!may_have_script(source.as_bytes(), None), "{source}");
        }
    }

    #[test]
    fn the_source_scan_cannot_read_a_wide_encoding() {
        let wide: Vec<u8> = "<script>".bytes().flat_map(|b| [b, 0]).collect();
        assert!(!may_have_script(&wide, None));
        assert!(!may_have_script(&wide, Some("utf-8")));
        assert!(may_have_script(&wide, Some("utf-16le")));
        assert!(may_have_script(&[0xFF, 0xFE, b'x', 0], None));
        assert!(may_have_script(&[0xFE, 0xFF, 0, b'x'], None));
        assert!(may_have_script(b"<scr\x1b(Bipt>", None));
    }
}
