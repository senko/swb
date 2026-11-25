//! The document's side of the media elements (M14 feature 6a, ADR 0047
//! section 10): the journal drain tells the media state about `video` and
//! `audio` elements (`changes.rs`), the page steps it (`Page::pump`), and
//! scripts read it (`EngineHost::media_state`).
//!
//! The state is [`crate::media::Media`]; this file builds the context that
//! its algorithms need from the document (the DOM, the base URL, the media
//! environment, the loader, the origin).

use swb_dom::NodeId;
use swb_web::MediaInfo;

use super::DocumentState;
use crate::media::Cx;

impl DocumentState {
    /// Runs `f` on the media state with the context of the document at page
    /// time `now_ms`.
    fn with_media<R>(
        &mut self,
        now_ms: f64,
        f: impl FnOnce(&mut crate::media::Media, &Cx<'_>) -> R,
    ) -> R {
        let initiator = Some(self.origin());
        let site_for_cookies = self.site_for_cookies.clone();
        let env = self.media_environment();
        let file_document = self.url.scheme() == "file";
        let cx = Cx {
            dom: &self.dom,
            base: self.base_url.value(),
            env,
            loader: &self.loader,
            initiator,
            site_for_cookies,
            file_document,
            now_ms,
        };
        f(&mut self.media, &cx)
    }

    /// The page time of the last media step, in milliseconds.
    fn media_now(&self) -> f64 {
        self.media.now_us as f64 / 1000.0
    }

    /// The `src` of the media element `node` was set or changed.
    pub(super) fn media_src_changed(&mut self, node: NodeId) {
        let now = self.media_now();
        self.with_media(now, |media, cx| media.load_algorithm(cx, node, false));
    }

    /// The media element `node` was found in the document.
    pub(super) fn media_discovered(&mut self, node: NodeId) {
        let now = self.media_now();
        self.with_media(now, |media, cx| media.discovered(cx, node));
    }

    /// A `source` was inserted into the media element `parent`.
    pub(super) fn media_source_inserted(&mut self, parent: NodeId) {
        let now = self.media_now();
        self.with_media(now, |media, cx| media.source_inserted(cx, parent));
    }

    /// The `preload` attribute of the media element `node` changed.
    pub(super) fn media_preload_changed(&mut self, node: NodeId) {
        let now = self.media_now();
        self.with_media(now, |media, cx| media.preload_changed(cx, node));
    }

    /// The children `removed` of the media element `parent` were removed;
    /// `before` was the child before them.
    pub(super) fn media_children_removed(
        &mut self,
        parent: NodeId,
        removed: &[NodeId],
        before: Option<NodeId>,
    ) {
        self.media.children_removed(parent, removed, before);
    }

    /// The media element `node` left the document.
    pub(super) fn media_removed(&mut self, node: NodeId) {
        self.media.removed(node);
    }

    /// `media_state` of the bindings: what the media element `node` reports.
    pub(crate) fn media_info(&mut self, node: NodeId) -> MediaInfo {
        self.process_changes();
        self.media.info(node)
    }

    /// `load()`: the media element load algorithm, with a load that does not
    /// wait for the user under `preload=none`.
    pub(crate) fn media_load(&mut self, node: NodeId) {
        self.process_changes();
        let now = self.media_now();
        self.with_media(now, |media, cx| media.load_algorithm(cx, node, true));
    }

    /// One step of the media elements at page time `now_ms`; `strict` in
    /// virtual time (see [`crate::media::Media::step`]). Returns true if one
    /// did something.
    pub(crate) fn media_step(&mut self, now_ms: f64, strict: bool) -> bool {
        let queued = self.media.event_count();
        self.process_changes();
        if self.media.event_count() != queued {
            // The synchronous part of a load algorithm queued events
            // (`abort`, `emptied`): they are delivered before the stable
            // state part runs.
            return true;
        }
        if self.media.is_empty() {
            // The pump runs for every input: a page without media pays
            // nothing here.
            return false;
        }
        self.with_media(now_ms, |media, cx| media.step(cx, strict))
    }

    /// Takes the media events for the script runtime.
    pub(crate) fn take_media_events(&mut self) -> Vec<(NodeId, &'static str)> {
        self.media.take_events()
    }

    /// True if media events wait for delivery.
    pub(crate) fn has_media_events(&self) -> bool {
        self.media.has_events()
    }

    /// True if virtual time must wait for the media of the document.
    pub(crate) fn media_pending(&self) -> bool {
        self.media.pending()
    }

    /// The number of media elements with an open pipeline.
    pub(crate) fn media_open_fetches(&self) -> usize {
        self.media.open_fetches()
    }

    /// The page time (ms) of the next media due time (an idle pipeline that
    /// suspends).
    pub(crate) fn media_next_due(&self) -> Option<f64> {
        self.media.next_due()
    }
}
