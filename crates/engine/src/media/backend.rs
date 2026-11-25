//! What the media element needs from the media crates, behind one interface
//! that has the same shape with and without the Cargo feature `media`
//! (ADR 0047 section 3): a [`Fetch`] starts a load of one resource and
//! tells what happens to it as [`Note`]s, one at a time.
//!
//! With the feature, a [`Fetch`] is a `swb_media::pipeline::Pipeline` over
//! the loader's media byte source (`Loader::open_media`, `SourceOpener::net`).
//! Without it, nothing is fetched: the load fails with code 4 as a failed
//! request does (`MEDIA_ELEMENT_ERROR: Format error`), one step after it
//! started, so that `loadstart` comes first.

use swb_net::{Destination, Loader, Request};

/// What the media of one page share: the places of open pipelines (the
/// process's 16, ADR 0047 section 6, unless the page has its own) and
/// whether media elements load. One per [`crate::Page`], handed to each of
/// its documents.
#[derive(Clone)]
pub(crate) struct MediaContext {
    /// The places of open pipelines: [`swb_media::pipeline::PipelineSlots::global`]
    /// unless [`MediaContext::isolate`] gave the page its own.
    #[cfg(feature = "media")]
    slots: std::sync::Arc<std::sync::Mutex<swb_media::pipeline::PipelineSlots>>,
    /// False: media elements do not start loads ([`crate::Page::set_media`]).
    enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl MediaContext {
    /// A new context: the process's places of open pipelines; media
    /// elements load.
    pub(crate) fn new() -> MediaContext {
        MediaContext {
            #[cfg(feature = "media")]
            slots: std::sync::Arc::new(std::sync::Mutex::new(
                swb_media::pipeline::PipelineSlots::global(),
            )),
            enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    /// Whether media elements start loads.
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Turns the loads of media elements on or off.
    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Gives the page places of its own (16), for the pipelines that start
    /// from now on ([`crate::Page::isolate_media`]); the last thread of a
    /// run that ended waits `handover_delay` before it hands its place on
    /// (a test hook).
    #[cfg_attr(not(feature = "media"), allow(clippy::unused_self))]
    pub(crate) fn isolate(&self, handover_delay: std::time::Duration) {
        #[cfg(feature = "media")]
        {
            let slots =
                swb_media::pipeline::PipelineSlots::new(swb_media::pipeline::MAX_OPEN_PIPELINES);
            slots.set_handover_delay(handover_delay);
            *self
                .slots
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = slots;
        }
        #[cfg(not(feature = "media"))]
        let _ = handover_delay;
    }

    /// The places that a new pipeline uses.
    #[cfg(feature = "media")]
    fn slots(&self) -> swb_media::pipeline::PipelineSlots {
        self.slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Default for MediaContext {
    fn default() -> MediaContext {
        MediaContext::new()
    }
}

/// How much to read ahead of the playback position (`preload`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preload {
    /// `none` (after a load that was asked for) and `metadata`: 3 s.
    Metadata,
    /// `auto`: 15 s.
    Auto,
}

/// What a fetch tells the element, one note at a time. Without the feature
/// `media` only a failure comes.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(feature = "media"), allow(dead_code))]
pub(crate) enum Note {
    /// The container opened: `loadedmetadata`.
    Metadata(Meta),
    /// The first data at the position is decoded: `loadeddata`.
    FirstData,
    /// The load failed before the metadata, or the pipeline failed later
    /// (the element decides which it is).
    Failed(Failure),
}

/// What the demuxer found, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Meta {
    /// `duration`: infinity when the file does not tell.
    pub(crate) duration: f64,
    /// The media time where playback starts (`currentTime` after the load).
    pub(crate) start: f64,
    /// `videoWidth` and `videoHeight`, when the file has a video track.
    pub(crate) size: Option<(u32, u32)>,
    /// Whether `seekable` is the whole timeline.
    pub(crate) seekable: bool,
}

/// A failed load or a failed pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    /// `MediaError.code`.
    pub(crate) code: u16,
    /// `MediaError.message`.
    pub(crate) message: String,
    /// True when bytes arrived (the request itself succeeded): `progress`
    /// and `suspend` come before the `error`.
    pub(crate) bytes_arrived: bool,
}

/// The message of a failed request or a bad URL (Chromium's).
pub(crate) const FORMAT_ERROR: &str = "MEDIA_ELEMENT_ERROR: Format error";

#[cfg(feature = "media")]
pub(crate) use real::Fetch;
#[cfg(not(feature = "media"))]
pub(crate) use stub::Fetch;

/// The request for the media resource at `url`: the destination, the
/// credentials and the CORS mode of the element's `crossorigin` attribute
/// (HTML "potentially CORS-enabled fetch": without the attribute the
/// request is `no-cors` with credentials `include`; `anonymous` is `cors`
/// with `same-origin` credentials; `use-credentials` `cors` with `include`).
pub(crate) fn request_for(
    url: swb_net::Url,
    video: bool,
    crossorigin: Option<&str>,
    initiator: Option<swb_net::Origin>,
    site_for_cookies: Option<swb_net::Origin>,
) -> Request {
    use swb_net::{CredentialsMode, RequestMode};
    let destination = if video {
        Destination::Video
    } else {
        Destination::Audio
    };
    let mut request = Request::get(url, destination)
        .with_initiator(initiator)
        .with_site_for_cookies(site_for_cookies);
    if let Some(value) = crossorigin {
        request.mode = RequestMode::Cors;
        request.credentials = if value.eq_ignore_ascii_case("use-credentials") {
            CredentialsMode::Include
        } else {
            CredentialsMode::SameOrigin
        };
    }
    request
}

#[cfg(not(feature = "media"))]
#[allow(clippy::unused_self)] // The same interface as the real fetch.
mod stub {
    //! Without the feature: nothing is fetched.

    use super::{FORMAT_ERROR, Failure, Loader, MediaContext, Note, Preload, Request};

    /// A load that fails at once.
    pub(crate) struct Fetch {
        failed: Option<Failure>,
    }

    impl Fetch {
        /// Starts the load of `request`: it fails with code 4, without a
        /// request.
        pub(crate) fn start(
            _ctx: &MediaContext,
            _loader: &Loader,
            _request: Request,
            _video: bool,
            _preload: Preload,
        ) -> Fetch {
            Fetch {
                failed: Some(Failure {
                    code: 4,
                    message: FORMAT_ERROR.to_owned(),
                    bytes_arrived: false,
                }),
            }
        }

        /// Lets the fetch look at its pipeline (`now_us`, the page clock).
        pub(crate) fn poll(&mut self, _now_us: i64) {}

        /// The next note.
        pub(crate) fn next_note(&mut self) -> Option<Note> {
            self.failed.take().map(Note::Failed)
        }

        /// Whether a note waits.
        pub(crate) fn has_notes(&self) -> bool {
            self.failed.is_some()
        }

        /// Whether the next note is the first data.
        pub(crate) fn front_is_first_data(&self) -> bool {
            false
        }

        /// Whether the next note is the metadata and the byte source has
        /// not read enough for it yet.
        pub(crate) fn net_unsettled(&self) -> bool {
            false
        }

        /// Whether the byte source stopped (the cache holds what the
        /// read-ahead asks for, or all of the resource).
        pub(crate) fn net_idle(&self) -> bool {
            true
        }

        /// Whether the pipeline still opens (it has pending work).
        pub(crate) fn pending(&self) -> bool {
            false
        }

        /// Whether the data is enough to play through (it needs data
        /// otherwise).
        pub(crate) fn enough(&self) -> bool {
            true
        }

        /// `buffered` for a duration of `duration`, in seconds.
        pub(crate) fn buffered(&self, _duration: f64) -> Vec<(f64, f64)> {
            Vec::new()
        }

        /// The playback position in seconds.
        pub(crate) fn position(&self) -> f64 {
            0.0
        }

        /// Whether the pipeline runs and can suspend.
        pub(crate) fn can_suspend(&self) -> bool {
            false
        }

        /// Closes the pipeline's threads and keeps its state.
        pub(crate) fn suspend(&mut self, _now_us: i64) {}

        /// The pipeline is suspended.
        pub(crate) fn is_suspended(&self) -> bool {
            false
        }

        /// Tells the byte source what to read ahead.
        pub(crate) fn set_preload(&mut self, _preload: Preload) {}
    }
}

#[cfg(feature = "media")]
mod real {
    //! With the feature: a pipeline over the loader's media byte source.

    use std::collections::VecDeque;

    use swb_media::MediaError;
    use swb_media::pipeline::{Pipeline, PipelineOptions, Report, SourceOpener};
    use swb_net::{MediaHandle, MediaOptions, ReadAhead};

    use super::{FORMAT_ERROR, Failure, Loader, MediaContext, Meta, Note, Preload, Request};

    /// The metadata waits until the byte source has read this much (or all
    /// of a smaller resource): a small file is read whole before the first
    /// event, so the events do not depend on how fast the threads are.
    const SETTLE_BYTES: usize = 1 << 20;

    /// A pipeline and the byte source under it.
    pub(crate) struct Fetch {
        pipeline: Pipeline,
        handle: MediaHandle,
        video: bool,
        /// Reports that `poll` took and the element has not read.
        notes: VecDeque<Note>,
        /// The metadata was reported.
        metadata_seen: bool,
        /// The first data was reported (or made up after a late failure).
        first_data: bool,
        /// A failure that came after the metadata: the element keeps it for
        /// the playback (feature 6b) and does not report it at load, as
        /// Chromium loads a file whose damage lies later and fails when it
        /// plays there. The load goes on as if the first data had come, so
        /// that the events do not depend on when the threads met the damage.
        late: Option<Failure>,
        suspended: bool,
    }

    fn read_ahead(preload: Preload) -> ReadAhead {
        match preload {
            Preload::Metadata => ReadAhead::METADATA,
            Preload::Auto => ReadAhead::AUTO,
        }
    }

    impl Fetch {
        /// Opens the pipeline over the resource of `request`.
        pub(crate) fn start(
            ctx: &MediaContext,
            loader: &Loader,
            request: Request,
            video: bool,
            preload: Preload,
        ) -> Fetch {
            let options = MediaOptions {
                read_ahead: read_ahead(preload),
                ..MediaOptions::default()
            };
            let resource = loader.open_media_with(request, options);
            let handle = resource.handle();
            // The element and its page wake up when the byte source makes
            // progress and when the pipeline has something to report.
            handle.set_notify(loader.waker());
            drop(resource);
            let mut options = PipelineOptions::new(loader.waker());
            options.video = video;
            options.slots = ctx.slots();
            // No frame is painted before feature 7.
            options.painted = false;
            let pipeline = Pipeline::open(SourceOpener::net(handle.clone()), options);
            Fetch {
                pipeline,
                handle,
                video,
                notes: VecDeque::new(),
                metadata_seen: false,
                first_data: false,
                late: None,
                suspended: false,
            }
        }

        /// Takes the reports of the pipeline at page time `now_us`.
        pub(crate) fn poll(&mut self, now_us: i64) {
            if self.suspended {
                return;
            }
            for report in self.pipeline.update(now_us) {
                match report {
                    Report::Metadata(m) => {
                        self.metadata_seen = true;
                        let timeline = m.timeline;
                        self.handle
                            .set_duration(timeline.duration_us.map(|d| d as f64 / 1e6));
                        self.notes.push_back(Note::Metadata(Meta {
                            duration: timeline
                                .duration_us
                                .map_or(f64::INFINITY, |d| d as f64 / 1e6),
                            start: timeline.start_us as f64 / 1e6,
                            size: m.natural_size().filter(|&(w, h)| w > 0 && h > 0),
                            seekable: m.info.seekable,
                        }));
                    }
                    Report::FirstFrame => {
                        self.first_data = true;
                        self.notes.push_back(Note::FirstData);
                    }
                    Report::Error(error) => {
                        let failure = self.failure(&error);
                        if self.metadata_seen {
                            self.late = Some(failure);
                            if !self.first_data {
                                self.first_data = true;
                                self.notes.push_back(Note::FirstData);
                            }
                        } else {
                            self.notes.push_back(Note::Failed(failure));
                        }
                    }
                    // Buffered ranges are read live; the other reports
                    // belong to playback (feature 6b).
                    _ => {}
                }
            }
        }

        /// The failure that `error` is, in the element's terms (ADR 0047
        /// section 10: Chromium's messages).
        fn failure(&self, error: &MediaError) -> Failure {
            let bytes_arrived = self.handle.error().is_none();
            let (code, message) = if bytes_arrived {
                match error {
                    MediaError::NotMedia(_) => (
                        4,
                        "PipelineStatus::DEMUXER_ERROR_COULD_NOT_OPEN: FFmpegDemuxer: open context failed"
                            .to_owned(),
                    ),
                    MediaError::Unsupported(_) | MediaError::NotBuilt => (
                        4,
                        "PipelineStatus::DEMUXER_ERROR_NO_SUPPORTED_STREAMS: FFmpegDemuxer: no supported streams"
                            .to_owned(),
                    ),
                    MediaError::Decode(_) | MediaError::Seek(_) => (
                        3,
                        format!(
                            "PipelineStatus::PIPELINE_ERROR_DECODE: {} decode error!",
                            if self.video { "video" } else { "audio" }
                        ),
                    ),
                    MediaError::Source(_) => (
                        2,
                        "PipelineStatus::PIPELINE_ERROR_READ: FFmpegDemuxer: data source error"
                            .to_owned(),
                    ),
                }
            } else {
                // The request failed (a status, CORS, a refused
                // connection): Chromium's message for every such load.
                (4, FORMAT_ERROR.to_owned())
            };
            Failure {
                code,
                message,
                bytes_arrived,
            }
        }

        /// The next note, in the order of the reports.
        pub(crate) fn next_note(&mut self) -> Option<Note> {
            self.notes.pop_front()
        }

        /// Whether a note waits.
        pub(crate) fn has_notes(&self) -> bool {
            !self.notes.is_empty()
        }

        /// Whether the next note is the first data.
        pub(crate) fn front_is_first_data(&self) -> bool {
            matches!(self.notes.front(), Some(Note::FirstData))
        }

        /// Whether the next note is the metadata and the byte source has not
        /// read enough for it ([`SETTLE_BYTES`], or all of the resource).
        pub(crate) fn net_unsettled(&self) -> bool {
            if !matches!(self.notes.front(), Some(Note::Metadata(_))) {
                return false;
            }
            let stats = self.handle.stats();
            !(stats.idle || stats.cached_bytes >= SETTLE_BYTES)
        }

        /// Whether the byte source stopped fetching.
        pub(crate) fn net_idle(&self) -> bool {
            self.handle.stats().idle
        }

        /// Whether the pipeline has not decoded what the owner needs next
        /// (it opens), or waits for a place that is being handed on (the
        /// threads of a run that ended are ending): virtual time waits.
        pub(crate) fn pending(&self) -> bool {
            // `awaits_handover` is read first. While the pipeline waits for
            // a place, `has_pending_work` is false. If the handover ended
            // between the two reads (`has_pending_work` first), both would
            // be false although the pipeline now has the place and work to
            // do. Read in this order, a handover that ends after the first
            // read is seen by the second.
            !self.suspended
                && (self.pipeline.awaits_handover()
                    || self.pipeline.has_pending_work()
                    || self.pipeline.has_unread_reports())
        }

        /// Whether the decoded data is enough to play through; a failure
        /// after the first data counts as enough (the error comes with the
        /// playback).
        pub(crate) fn enough(&self) -> bool {
            self.suspended || self.late.is_some() || self.pipeline.has_enough_data()
        }

        /// `buffered`, in seconds, for a duration of `duration`: the whole
        /// timeline once the byte source holds all of the resource (Chromium,
        /// measured: a truncated or damaged file that arrived whole is
        /// buffered to its duration), else the ranges that the demuxer read,
        /// clamped to the duration.
        pub(crate) fn buffered(&self, duration: f64) -> Vec<(f64, f64)> {
            let stats = self.handle.stats();
            if stats.idle
                && stats
                    .size
                    .is_some_and(|size| stats.cached_bytes as u64 >= size)
                && duration.is_finite()
                && duration > 0.0
            {
                return vec![(0.0, duration)];
            }
            self.pipeline
                .buffered()
                .into_iter()
                .map(|(a, b)| (a as f64 / 1e6, (b as f64 / 1e6).min(duration)))
                .filter(|(a, b)| a < b)
                .collect()
        }

        /// The playback position in seconds.
        pub(crate) fn position(&self) -> f64 {
            self.pipeline.position() as f64 / 1e6
        }

        /// Whether the pipeline runs: it can suspend.
        pub(crate) fn can_suspend(&self) -> bool {
            !self.suspended && !self.pipeline.is_failed() && !self.pipeline.is_waiting()
        }

        /// Closes the threads of the pipeline and gives its place to the
        /// next one; the metadata, the position and the byte cache stay.
        pub(crate) fn suspend(&mut self, now_us: i64) {
            self.pipeline.suspend(now_us);
            self.suspended = true;
        }

        /// The pipeline is suspended.
        pub(crate) fn is_suspended(&self) -> bool {
            self.suspended
        }

        /// Tells the byte source what to read ahead.
        pub(crate) fn set_preload(&mut self, preload: Preload) {
            self.handle.set_read_ahead(read_ahead(preload));
        }
    }
}

#[cfg(all(test, feature = "media"))]
mod tests {
    use super::MediaContext;
    use swb_media::pipeline::PipelineSlots;

    /// ADR 0047 section 6: the pipelines of all pages share the process's
    /// 16 places; a page that is isolated (tests) has its own.
    #[test]
    fn pages_share_the_process_places_unless_isolated() {
        let a = MediaContext::new();
        let b = MediaContext::new();
        assert!(a.slots() == PipelineSlots::global());
        assert!(b.slots() == a.slots());
        let shared = a.clone();
        a.isolate(std::time::Duration::ZERO);
        assert!(a.slots() != PipelineSlots::global());
        // The documents of the page (clones of its context) follow.
        assert!(shared.slots() == a.slots());
        assert!(b.slots() == PipelineSlots::global());
    }
}
