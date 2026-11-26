//! The document's side of the media elements (M14 features 6a and 6b, ADR
//! 0047 section 10): the journal drain tells the media state about `video`
//! and `audio` elements (`changes.rs`), the page steps it (`Page::pump`), and
//! scripts read it and command it (`EngineHost::media_state`,
//! `media_command`, `media_play`).
//!
//! The state is [`crate::media::Media`]; this file builds the context that
//! its algorithms need from the document (the DOM, the base URL, the media
//! environment, the loader, the origin, the user activation).

use swb_dom::NodeId;
use swb_web::{MediaCommand, MediaInfo, PlayStart};

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
            // HTML "has sticky activation" of the window.
            activated: self.user_activation.is_some(),
        };
        f(&mut self.media, &cx)
    }

    /// The page time of the media state, in milliseconds: the time of the
    /// last step, or of the last task of a script (`set_media_time`).
    fn media_now(&self) -> f64 {
        self.media.now_ms()
    }

    /// Tells the media state the page time (ms): the page does it when it
    /// lends the document to a task, so that `play()` and a read of
    /// `currentTime` in a task that runs after a jump of virtual time have
    /// the time of the task.
    pub(crate) fn set_media_time(&mut self, now_ms: f64) {
        self.media.now_us = crate::media::ms_to_us(now_ms);
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
        let muted = self
            .dom
            .element(node)
            .is_some_and(|e| e.attr("muted").is_some());
        self.media.info(node, muted)
    }

    /// `load()`: the media element load algorithm, with a load that does not
    /// wait for the user under `preload=none`.
    pub(crate) fn media_load(&mut self, node: NodeId) {
        self.process_changes();
        let now = self.media_now();
        self.with_media(now, |media, cx| media.load_algorithm(cx, node, true));
    }

    /// A command of a script (`pause()`, a seek, `volume`, `muted`, the
    /// rates; `load()` is [`DocumentState::media_load`]).
    pub(crate) fn media_run(&mut self, node: NodeId, command: MediaCommand) {
        if command == MediaCommand::Load {
            self.media_load(node);
            return;
        }
        self.process_changes();
        let now = self.media_now();
        self.with_media(now, |media, cx| match command {
            MediaCommand::Load => {}
            MediaCommand::Pause => media.pause(cx, node),
            MediaCommand::Seek(seconds) => media.seek(cx, node, seconds),
            MediaCommand::SetVolume(volume) => media.set_volume(cx, node, volume),
            MediaCommand::SetMuted(muted) => media.set_muted(cx, node, muted),
            MediaCommand::SetPlaybackRate(rate) => media.set_rate(cx, node, rate, false),
            MediaCommand::SetDefaultPlaybackRate(rate) => media.set_rate(cx, node, rate, true),
        });
    }

    /// `play()` of the media element `node`.
    pub(crate) fn media_play(&mut self, node: NodeId) -> PlayStart {
        self.process_changes();
        let now = self.media_now();
        self.with_media(now, |media, cx| media.play(cx, node))
    }

    /// One step of the media elements at page time `now_ms`; `strict` in
    /// virtual time (see [`crate::media::Media::step`]). Returns true if one
    /// did something.
    pub(crate) fn media_step(&mut self, now_ms: f64, strict: bool) -> bool {
        let queued = self.media.event_count();
        self.process_changes();
        if self.media.event_count() != queued {
            // The synchronous part of a load algorithm queued events
            // (`abort`, `emptied`) and the marker of its stable-state step:
            // they are delivered before that step runs.
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
    pub(crate) fn take_media_events(&mut self) -> Vec<swb_web::MediaDelivery> {
        self.media.take_events()
    }

    /// A media task is about to run: false if a load dropped it.
    pub(crate) fn media_task_begin(
        &mut self,
        owner: NodeId,
        epoch: u32,
        task: swb_web::MediaTask,
    ) -> bool {
        self.media.task_begin(owner, epoch, task)
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

    /// The page time (ms) of the next media due time (a frame or a
    /// `timeupdate` point of a playing pipeline, the suspend of an idle one).
    pub(crate) fn media_next_due(&self) -> Option<f64> {
        self.media.next_due()
    }
}
