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
use swb_layout::{Point, Rect, Size};
use swb_web::{MediaCommand, MediaInfo, PlayStart};

use super::DocumentState;
use crate::media::Cx;

/// How far outside the viewport a video still converts frames, in viewports
/// on each side (ADR 0047 "Implementation notes (M14 feature 7b)"): a
/// video that scrolls into view has converted frames by then. After a
/// painted flag turns on, the first converted frame is current after the
/// frames that the pipeline queued while it was not painted (at most 6, so
/// 240 ms at 25 fps); a margin of one viewport covers a scroll of that
/// length at up to 3,000 px/s in a 720 px viewport, and a page-down.
const VIDEO_MARGIN: f32 = 1.0;

/// What the painted flag of the videos depends on (see
/// [`DocumentState::update_media_visibility`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct VisibilityKey {
    geometry: MediaGeometry,
    shown: bool,
}

/// What decides whether a box of a document lies in the window of the
/// videos ([`video_window`]): the layout, the display list (element
/// scrolls), the viewport scroll offset and size, and the viewport rule.
/// The page keys its look at the frame containers by the geometry of the
/// parent document (`Page::update_frames_shown`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MediaGeometry {
    layout: u64,
    list: u64,
    scroll: Point,
    viewport: Size,
    rule: bool,
}

/// The rectangle around the viewport (at `scroll`) in which a video is
/// painted: the viewport grown by [`VIDEO_MARGIN`].
fn video_window(scroll: Point, viewport: Size) -> Rect {
    let (mx, my) = (
        viewport.width * VIDEO_MARGIN,
        viewport.height * VIDEO_MARGIN,
    );
    Rect::new(
        scroll.x - mx,
        scroll.y - my,
        viewport.width + 2.0 * mx,
        viewport.height + 2.0 * my,
    )
}

impl DocumentState {
    /// Runs `f` on the media state with the context of the document at page
    /// time `now_ms`.
    pub(super) fn with_media<R>(
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

    /// The painted flag of the videos (ADR 0047 section 6, "Hidden
    /// elements", and the notes of feature 7b): tells the media elements
    /// which videos are painted. A video that is not converts no frames; one
    /// that is again shows the frame at its position. A video is painted when
    /// its box is painted (not `display: none`, not in the document, not in
    /// the skipped contents of `content-visibility` or a closed `details`),
    /// its `visibility` is `visible`, and the document is shown (the window
    /// is not hidden, and a frame document lies in its parent's viewport);
    /// with the viewport rule (the page paints frames: the GUI), also when
    /// the box lies within [`VIDEO_MARGIN`] viewports of the document's
    /// viewport. Headless pages draw whole pages (a full-page screenshot
    /// shows the videos below the viewport), so they have no viewport rule.
    /// It runs after a layout and a display list, when the scroll offset
    /// or the viewport changed, and when the page changes the inputs
    /// (`key`); a paint where nothing changed does nothing.
    pub(crate) fn update_media_visibility(&mut self) {
        if self.media.is_empty() {
            return;
        }
        let Some(tree) = &self.fragments else {
            return;
        };
        let shown = self.media_shown && !self.media.ctx.window_hidden();
        let key = VisibilityKey {
            geometry: self.media_geometry(),
            shown,
        };
        if self.media_visibility == Some(key) {
            return;
        }
        self.media_visibility = Some(key);
        let mut painted = std::collections::HashSet::new();
        if shown {
            self.walk_boxes_in_window(tree, |b| {
                if let swb_layout::BoxContent::Media(m) = &b.content {
                    painted.insert(m.node);
                }
            });
        }
        self.media.set_painted(&painted);
    }

    /// The inputs of the window test of the boxes of this document.
    pub(crate) fn media_geometry(&self) -> MediaGeometry {
        MediaGeometry {
            layout: self.layout_serial,
            list: self.list_serial,
            scroll: self.scroll,
            viewport: self.view.viewport,
            rule: self.media.ctx.viewport_rule(),
        }
    }

    /// Calls `found` for each box that paint visits, whose `visibility` is
    /// `visible`, and that lies in the window of the videos (with the
    /// viewport rule; else wherever it is). One walk of the whole tree.
    fn walk_boxes_in_window(
        &self,
        tree: &swb_layout::FragmentTree,
        mut found: impl FnMut(&swb_layout::BoxFragment),
    ) {
        self.media_window_walks
            .set(self.media_window_walks.get() + 1);
        let rule = self.media.ctx.viewport_rule();
        let window = video_window(self.scroll, self.view.viewport);
        tree.walk_painted(
            self.scrollers.offsets(),
            self.scroll,
            |fragment, rect, matrix| {
                if let swb_layout::FragmentRef::Box(b) = fragment
                    && b.style.visibility == swb_style::Visibility::Visible
                    && (!rule || matrix.map_rect(&rect).intersection(&window).is_some())
                {
                    found(b);
                }
            },
        );
    }

    /// How many walks of the fragment tree for the window test this document
    /// ran (tests: a render without a change runs none).
    pub(crate) fn media_window_walks(&self) -> u64 {
        self.media_window_walks.get()
    }

    /// Whether the parent shows the document ([`DocumentState::set_media_shown`]).
    pub(crate) fn media_is_shown(&self) -> bool {
        self.media_shown
    }

    /// Tells the document whether its parent shows it (a frame document;
    /// see [`DocumentState::update_media_visibility`]). True at the start.
    pub(crate) fn set_media_shown(&mut self, shown: bool) {
        self.media_shown = shown;
        self.update_media_visibility();
    }

    /// True if the box of the element `node` is painted and lies within the
    /// margin of the viewport ([`VIDEO_MARGIN`]) when the page applies the
    /// viewport rule (else only that it is painted): the frame of a frame
    /// document shows if this holds for its container. False without a
    /// layout.
    pub(crate) fn element_in_media_window(&self, node: NodeId) -> bool {
        let Some(tree) = &self.fragments else {
            return false;
        };
        let mut found = false;
        self.walk_boxes_in_window(tree, |b| found |= b.node == Some(node));
        found
    }

    /// True if a painted video has a converted frame due at page time
    /// `now_us` that it does not show yet ([`crate::media::Media::frame_due`]).
    pub(crate) fn media_frame_due(&self, now_us: i64) -> bool {
        !self.media.is_empty() && self.media.frame_due(now_us)
    }

    /// The counters of the video pipelines of the document.
    pub(crate) fn media_counters(&self) -> crate::media::MediaCounters {
        self.media.counters()
    }

    /// The synchronization of the playing, painted videos of the document at
    /// page time `now_us` (microseconds).
    pub(crate) fn media_sync_samples(&self, now_us: i64) -> Vec<crate::media::MediaSync> {
        self.media.sync_samples(now_us)
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
    /// A real-time page passes `coarse`: a video that is not painted has no
    /// frame times. `now_us` is the page time now (microseconds).
    pub(crate) fn media_next_due(&self, coarse: bool, now_us: i64) -> Option<f64> {
        self.media.next_due(coarse, now_us)
    }
}
