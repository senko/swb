//! The due times and the pending work of a [`Pipeline`]: the end of the
//! media, how far the decoded data covers it, the media times at which the
//! owner has something to do ([`Pipeline::next_due`], [`Pipeline::frame_due`])
//! and whether the threads still owe it progress ([`Pipeline::has_pending_work`]).
//! A part of `impl Pipeline`, moved out of `mod.rs` unchanged.

use std::time::Instant;

use super::shared::State;
use super::{
    AudioOutput, Phase, Pipeline, drained, end_reached, ready_for, seek_ready, waits, watchdog,
};
use crate::clock::ClockSource;

impl Pipeline {
    /// The end of the media: the duration. Data that ends before it (Opus:
    /// FFmpeg's duration counts the padding; a file that was cut: the
    /// truncated WebM and MP3 of the probe `media-states`, M14 feature 6b)
    /// plays on to the duration, as Chromium's `currentTime` is the
    /// duration at `ended` (silence, and the last frame, where the data
    /// ends). Only a duration that the file does not tell makes the end of
    /// the data the end (never before the position).
    pub(super) fn end_time(&self, st: &State) -> i64 {
        let duration = self
            .metadata
            .as_ref()
            .and_then(|m| m.timeline.duration_us)
            .unwrap_or(i64::MAX);
        if duration != i64::MAX || !drained(st) {
            return duration;
        }
        let audio = if st.audio_on && st.demux_epoch == st.epoch {
            st.audio_end_us
        } else {
            None
        };
        let video = if st.video_on { st.video_end_us } else { None };
        audio.max(video).unwrap_or(self.position).max(self.position)
    }

    /// The media time up to which the decoded data reaches (the audio when
    /// it plays and is not in a gap, else the video).
    pub(super) fn covered(&self, st: &State) -> i64 {
        if st.audio_on && !self.audio_gap {
            if st.audio_drained == Some(st.epoch) {
                return i64::MAX;
            }
            if st.demux_epoch != st.epoch {
                return self.position;
            }
            return st.audio_end_us.unwrap_or(self.position);
        }
        if st.video_on
            && st.video_drained != Some(st.epoch)
            && !st.frames.iter().any(|f| f.epoch == st.epoch)
        {
            return self
                .current
                .as_ref()
                .map_or(self.position, |c| c.pts_us.saturating_add(c.duration_us));
        }
        i64::MAX
    }

    /// Whether the mixer has taken every decoded sample and the audio has
    /// not ended: with the audio clock ([`ClockSource::Audio`]) the clock
    /// stands still then (the mixer plays silence for the element and counts
    /// no frame), so the clock never passes the end of the decoded data, and
    /// this is how an underrun shows. With the page clock the media time
    /// passes the decoded data, which `covered` compares.
    pub(super) fn audio_starved(&self, st: &State) -> bool {
        self.clock.source() == ClockSource::Audio
            && st.audio_on
            && !self.audio_gap
            && st.demux_epoch == st.epoch
            && st.audio_drained != Some(st.epoch)
            && st.ring.as_ref().is_some_and(|r| r.available() == 0)
    }

    /// Whether the data at the position is there for the first time.
    pub(super) fn first_data_ready(&self, st: &State) -> bool {
        if st.video_on {
            return self.current.is_some()
                || st.frames.iter().any(|f| f.epoch == st.epoch)
                || st.video_drained == Some(st.epoch);
        }
        st.audio_drained == Some(st.epoch)
            || (st.demux_epoch == st.epoch && st.audio_end_us.is_some())
    }

    /// The next due media time while the clock runs: the next frame, the
    /// next `timeupdate` point, the end.
    fn next_due_media(&self, st: &State) -> Option<i64> {
        self.due_media(st, true)
    }

    /// [`Pipeline::next_due_media`]; with `exact_frames` false and the
    /// video not painted, the frame term is the frame in the middle of the
    /// queue, not the first: nobody sees the frames, so the owner takes
    /// them in batches (see [`Pipeline::next_wake`]).
    fn due_media(&self, st: &State, exact_frames: bool) -> Option<i64> {
        if !self.clock.is_running() {
            return None;
        }
        let mut frames = st.frames.iter().filter(|f| f.epoch == st.epoch);
        let frame = if exact_frames || st.painted {
            frames.next()
        } else {
            let queued = frames.clone().count();
            frames.nth(queued / 2)
        };
        [
            frame.map(|f| f.pts_us),
            self.next_timeupdate,
            Some(self.end_time(st)),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// The clock time of the next due item (a frame, a `timeupdate` point,
    /// the end), while playing; `None` when nothing is due (paused, ended,
    /// seeking, in an underrun, suspended).
    pub fn next_due(&self) -> Option<i64> {
        let mut st = self.shared.lock();
        self.arm_frame_notify(&mut st);
        let t = self.next_due_media(&st)?;
        self.clock.clock_time(t)
    }

    /// The clock time at which a real-time owner needs to call
    /// [`Pipeline::update`] next. It is [`Pipeline::next_due`], except that
    /// a video that is not painted has no frame times: nobody sees its
    /// frames, so the owner takes the queue in batches, at the frame in the
    /// middle of it (a wake-up every 3 frames of the 6 queued, not every
    /// frame; the video thread refills the queue with the room that the
    /// update makes). The frames passed over are counted as shown, not as
    /// dropped ([`Pipeline::quality`]). A virtual-time owner uses
    /// `next_due`: its jumps and its results do not depend on the paint.
    pub fn next_wake(&self) -> Option<i64> {
        let mut st = self.shared.lock();
        self.arm_frame_notify(&mut st);
        let t = self.due_media(&st, false)?;
        self.clock.clock_time(t)
    }

    /// Whether [`Pipeline::update`] at clock time `now` would show a frame
    /// that has pixels and is not the one shown (M14 feature 7b: the owner's
    /// frame decision, which runs at each batch of the GUI). It converts and
    /// takes nothing; it holds the lock for a walk over at most the frame
    /// queue (6 frames), and none while the clock does not run (paused,
    /// ended, seeking, in an underrun, suspended, failed) or while the
    /// queue holds no frame that is due. A due frame that has no pixels (it
    /// was decoded while the video was not painted) changes nothing that
    /// paint shows, so it does not count: the video thread converts the
    /// frames that come after [`Pipeline::set_painted`].
    pub fn frame_due(&self, now: i64) -> bool {
        if !self.playing
            || self.phase != Phase::Running
            || self.seeking.is_some()
            || self.underrun
            || !self.clock.is_running()
        {
            return false;
        }
        let media = self.clock.media_time(now);
        let st = self.shared.lock();
        let mut shown = false;
        for frame in st.frames.iter() {
            if frame.epoch != st.epoch {
                continue;
            }
            if frame.pts_us > media {
                break;
            }
            shown = frame.image.is_some();
        }
        shown
    }

    /// While playing without a queued frame, the next due time changes when
    /// the video thread queues one: it then notifies the owner.
    pub(super) fn arm_frame_notify(&self, st: &mut State) {
        if self.playing
            && st.video_on
            && st.video_drained != Some(st.epoch)
            && !st.frames.iter().any(|f| f.epoch == st.epoch)
        {
            st.wants_notify = true;
        }
    }

    /// Whether the pipeline has not yet decoded what it needs next: the
    /// metadata and the first data while it opens, the data at the target
    /// of a seek, or, while playing, the data for the next due time. A
    /// pipeline that waits for a place, is paused, ended, suspended or
    /// failed has none, nor has one that is stalled on its owner (its
    /// threads wait for room that only the owner's progress frees). When it
    /// returns true, the pipeline calls the owner's notify once it has made
    /// progress, is stalled, or failed (also by the no-progress limit).
    pub fn has_pending_work(&self) -> bool {
        let mut st = self.shared.lock();
        self.wake_for_external_room(&st);
        let pending = self.pending(&st);
        if pending {
            st.wants_notify = true;
        }
        let watch = self.note_needs(&mut st, pending);
        drop(st);
        if let Some(due) = watch {
            watchdog::watch(&self.shared, due);
        }
        pending
    }

    /// Whether a thread reported something that [`Pipeline::update`] has not
    /// handled (the open, a failure): while it is true,
    /// [`Pipeline::has_pending_work`] is false until the owner updates. An
    /// owner that decides to wait for the pipeline asks this too, so that it
    /// does not take the moment between the report and its update for "no
    /// work" (M14 feature 6a).
    pub fn has_unread_reports(&self) -> bool {
        !self.reports.is_empty() || !self.shared.lock().events.is_empty()
    }

    /// Whether the decoded data is enough to play through
    /// (`HAVE_ENOUGH_DATA`, `canplaythrough`; M14 feature 6a, ADR 0047
    /// "Implementation notes of feature 6a"): the first data was reported,
    /// no seek runs and no underrun, and the threads have settled: every
    /// track is decoded to its end, or each thread waits for room in a
    /// queue that only the owner frees (the demux thread at its packet
    /// bound or the sample ring, the video thread at its frame cap) or for
    /// nothing more to read. So the whole resource is read (a small file),
    /// or the decoded queues are full (a large one). It depends only on what
    /// the threads decoded, so it is the same on every run once they have
    /// settled; the buffered ranges are final then too. While it returns
    /// false the owner needs data: the pipeline calls the owner's notify
    /// when a thread makes progress or all wait, and the no-progress limit
    /// applies (a source that stalls after the first frame fails the
    /// pipeline with a decode error, as for [`Pipeline::has_pending_work`]).
    pub fn has_enough_data(&self) -> bool {
        let mut st = self.shared.lock();
        let live = self.phase == Phase::Running
            && !waits(&st)
            && self.first_frame_reported
            && self.seeking.is_none()
            && !self.underrun
            && st.events.is_empty();
        let enough = live && (st.stalled_on_owner() || drained(&st));
        if live && !enough {
            st.wants_notify = true;
            let watch = self.note_needs(&mut st, true);
            drop(st);
            if let Some(due) = watch {
                watchdog::watch(&self.shared, due);
            }
        }
        enough
    }

    /// With an external consumer, which takes samples outside the lock and
    /// notifies nobody: wakes the demux thread when it waits for room that
    /// is there now.
    pub(super) fn wake_for_external_room(&self, st: &State) {
        if self.output == AudioOutput::External
            && st.demux_wait.is_some_and(|(run, _)| run == st.run)
            && !st.demux_blocked()
        {
            self.shared.demux_cv.notify_all();
        }
    }

    /// The owner needs data (`needs`): the no-progress limit applies.
    /// Returns the time of the first check when the watchdog must start to
    /// watch the pipeline.
    pub(super) fn note_needs(&self, st: &mut State, needs: bool) -> Option<Instant> {
        let needs = needs || (self.underrun && self.playing);
        if needs && !st.watch.needs_data {
            // The limit counts from here (also after a long pause).
            st.watch.since = Instant::now();
            st.watch.lost = None;
        }
        st.watch.needs_data = needs;
        if needs && !st.watch.armed {
            st.watch.armed = true;
            return Some(st.watch.since + self.shared.stall_limit);
        }
        None
    }

    /// See [`Pipeline::has_pending_work`].
    pub(super) fn pending(&self, st: &State) -> bool {
        if self.phase != Phase::Running || waits(st) || !st.events.is_empty() {
            return false;
        }
        let needs = if st.info.is_none() {
            true
        } else if self.metadata.is_none() {
            // The metadata event waits for `update`.
            false
        } else if let Some(s) = self.seeking {
            !seek_ready(st, s)
        } else if !self.first_frame_reported {
            !self.first_data_ready(st)
        } else if self.playing && !self.ended && !self.underrun {
            let end = self.end_time(st);
            self.next_due_media(st).is_some_and(|t| {
                if t >= end {
                    !end_reached(st, end)
                } else {
                    !ready_for(st, t)
                }
            })
        } else {
            false
        };
        needs && !st.stalled_on_owner()
    }
}
