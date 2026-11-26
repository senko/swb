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
        self.sync_media_images(None);
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
        self.sync_media_images(None);
    }

    /// `play()` of the media element `node`.
    pub(crate) fn media_play(&mut self, node: NodeId) -> PlayStart {
        self.process_changes();
        let now = self.media_now();
        let start = self.with_media(now, |media, cx| media.play(cx, node));
        self.sync_media_images(None);
        start
    }

    /// One step of the media elements at page time `now_ms`; `strict` in
    /// virtual time (see [`crate::media::Media::step`]). Returns true if one
    /// did something. The images of the videos follow the step, so that the
    /// events that the step queued see the layout that they imply (a
    /// `loadedmetadata` listener reads the new size).
    pub(crate) fn media_step(&mut self, now_ms: f64, strict: bool) -> bool {
        let progressed = self.media_step_units(now_ms, strict);
        self.sync_media_images(None);
        progressed
    }

    /// The units of [`DocumentState::media_step`].
    fn media_step_units(&mut self, now_ms: f64, strict: bool) -> bool {
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

    /// Brings the images of the `<video>` elements up to date: the natural
    /// size of the stream, the poster or the frame that shows, and the
    /// current frame (ADR 0047 section 9). With `poll_at_us` the pipelines
    /// first take the frames that are due at that page time (a paint). A new
    /// frame bumps the content generation of the video's image, and the
    /// display list stays; a change of the natural size (or of what shows,
    /// when the sizes differ) makes the layout outdated. Without
    /// `poll_at_us` only the elements that a unit of a step or a script
    /// touched are looked at (a page with 16,000 videos does not look at all
    /// of them after each unit); a paint looks at all.
    pub(crate) fn sync_media_images(&mut self, poll_at_us: Option<i64>) {
        if self.media.is_empty() && self.images.no_videos() {
            return;
        }
        if let Some(now) = poll_at_us {
            self.media.poll_frames(now);
        }
        let touched = self.media.take_touched();
        let all = self.media.take_resync_all() || poll_at_us.is_some();
        let mut layout = false;
        if all {
            let mut live = std::collections::HashSet::new();
            for (node, view) in self.media.video_views() {
                layout |= self.set_video_image(node, view);
                live.insert(node);
            }
            layout |= self.images.retain_videos(|node| live.contains(&node));
        } else {
            for node in touched {
                layout |= match self.media.video_view(node) {
                    Some(view) => self.set_video_image(node, view),
                    None => self.images.forget_video(node),
                };
            }
        }
        if layout {
            self.invalidate_layout();
        }
    }

    /// Gives `Images` what the video `node` shows. True if the layout is
    /// outdated.
    fn set_video_image(&mut self, node: NodeId, view: crate::media::VideoView) -> bool {
        let poster = self
            .dom
            .element(node)
            .and_then(|e| e.attr("poster"))
            .is_some_and(|p| !p.trim_matches(swb_dom::is_html_whitespace).is_empty());
        // The poster shows while the flag is set, and also with the flag
        // clear while the current load has no frame yet (`play()` before the
        // first frame, a decode failure; measured in Chromium).
        let poster_mode = poster && (view.show_poster || view.frame.is_none());
        self.images
            .set_video(node, view.natural, poster_mode, view.frame)
    }

    /// The `poster` attribute of the `<video>` `node` changed (or it got a
    /// poster): its images may have changed.
    pub(super) fn media_poster_changed(&mut self, node: NodeId) {
        self.media.touch(node);
    }

    /// Tells the media elements which videos have a box after a layout: a
    /// video without one (`display: none`, not in the document) converts no
    /// frames, and one that gets a box again shows the frame at its position
    /// (ADR 0047 section 6, "Hidden elements").
    pub(super) fn sync_media_painted(&mut self) {
        if self.media.is_empty() {
            return;
        }
        let Some(tree) = &self.fragments else {
            return;
        };
        let mut boxes = std::collections::HashSet::new();
        tree.walk(|fragment, _| {
            if let swb_layout::FragmentRef::Box(b) = fragment
                && let swb_layout::BoxContent::Media(m) = &b.content
            {
                boxes.insert(m.node);
            }
        });
        self.media.set_painted(&boxes);
    }

    /// True if a media element has a state in this document.
    pub(crate) fn has_media(&self) -> bool {
        !self.media.is_empty()
    }

    /// The frames that the pipeline of the video `node` converted (tests).
    pub(crate) fn media_frames_converted(&self, node: NodeId) -> Option<u64> {
        self.media.frames_converted(node)
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
