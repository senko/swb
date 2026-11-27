//! The update step of a [`Pipeline`] ([`Pipeline::update`]): what the owner
//! does at each clock time: handles the reports of the threads, ends a seek,
//! shows the frame at the media time, lets the silent output consume the
//! samples, and detects underruns and the end. A part of `impl Pipeline`,
//! moved out of `mod.rs` unchanged.

use std::sync::Arc;

use super::shared::{self, State, ThreadEvent};
use super::{
    AudioOutput, CurrentFrame, Metadata, Phase, Pipeline, RESUME_AHEAD_US, Report, Seeking,
    TIMEUPDATE_INTERVAL_US, audio_covers, end_reached, seek_ready, watchdog,
};
use crate::ring::us_to_frames;

impl Pipeline {
    /// Ends `update`: while the pipeline has pending work, the threads
    /// notify the owner when they have made progress, and the no-progress
    /// limit applies (also in real time, where the owner may never ask
    /// [`Pipeline::has_pending_work`]).
    ///
    /// While the pipeline plays with a frame of the current epoch queued, it
    /// is "pending" only because less than [`READY_AHEAD_US`] is decoded
    /// ahead (the queue holds 6 frames, about 200 ms, so each frame taken
    /// makes it so), and the next push of the video thread changes nothing
    /// that the owner waits for: its next due time is the queued frame. The
    /// notify is not armed then (M14 feature 7b, review: it woke the GUI
    /// once more per frame). A virtual-time owner that waits for the data
    /// asks [`Pipeline::has_pending_work`], which arms the notify itself; an
    /// owner in real time does not wait for the pipeline.
    ///
    /// The rule needs a running clock and no seek: then the queued frame is
    /// a due time of [`Pipeline::next_due`]. During a seek while playing the
    /// clock stops, so no due time comes; the frame at the target can be
    /// queued while the seek still waits for its audio, and only the notify
    /// tells a real-time owner that the seek can end (review of the fixes
    /// of 7b: `SeekDone` never came for `mp4-h264-aac-late-audio.mp4`).
    fn end_update(&self, mut st: std::sync::MutexGuard<'_, State>) {
        let pending = self.pending(&st);
        let queued_frame = self.playing
            && self.seeking.is_none()
            && self.clock.is_running()
            && st.frames.iter().any(|f| f.epoch == st.epoch);
        if pending && !queued_frame {
            st.wants_notify = true;
        }
        let watch = self.note_needs(&mut st, pending);
        drop(st);
        if let Some(due) = watch {
            watchdog::watch(&self.shared, due);
        }
    }

    /// Advances to clock time `now`: handles what the threads reported,
    /// ends a seek whose data is there, makes the frame at the media time
    /// current (dropping the frames passed over), lets the silent output
    /// consume the samples up to it, detects underruns and the end, and
    /// returns the reports.
    pub fn update(&mut self, now: i64) -> Vec<Report> {
        let mut out = std::mem::take(&mut self.reports);
        let shared = Arc::clone(&self.shared);
        let mut st = shared.lock();
        while let Some(event) = st.events.pop_front() {
            match event {
                ThreadEvent::Opened(opened) => {
                    let (info, timeline) = *opened;
                    if self.seeking.is_none() {
                        self.position = timeline.start_us;
                        self.clock.set(timeline.start_us);
                    }
                    let metadata = Metadata { info, timeline };
                    self.metadata = Some(metadata.clone());
                    out.push(Report::Metadata(Box::new(metadata)));
                }
                ThreadEvent::Failed(error) => {
                    if self.phase == Phase::Running {
                        self.phase = Phase::Failed;
                        self.playing = false;
                        self.clock.stop(now);
                        out.push(Report::Error(error));
                    }
                }
            }
        }
        if self.phase != Phase::Running || self.metadata.is_none() {
            self.end_update(st);
            return out;
        }
        // A pipeline stalled on its owner goes on with what it has: the
        // data that is missing lies further on in the file.
        if let Some(s) = self.seeking {
            if !seek_ready(&st, s) && !st.stalled_on_owner() {
                self.end_update(st);
                return out;
            }
            self.finish_seek(&mut st, s, &mut out);
        }
        if !self.first_frame_reported {
            if !self.first_data_ready(&st) && !st.stalled_on_owner() {
                self.end_update(st);
                return out;
            }
            if self.current.is_none()
                && let Some(f) = st.frames.pop_front()
            {
                self.show(f);
            }
            self.first_frame_reported = true;
            out.push(Report::FirstFrame);
        }
        drop(st);
        self.sync_clock(now);
        let mut st = shared.lock();
        // Taking frames notifies the video thread (`FrameQueue`).
        let consumed = self.advance(now, &mut st, &mut out);
        if st.buffered_dirty {
            st.buffered_dirty = false;
            out.push(Report::Buffered(st.buffered.clone()));
        }
        st.media_now = self.position;
        st.playing = self.playing;
        if self.underrun {
            st.wants_notify = true;
        }
        self.arm_frame_notify(&mut st);
        self.wake_for_external_room(&st);
        self.end_update(st);
        if consumed {
            // The silent output made room in the ring.
            shared.demux_cv.notify_all();
        }
        out
    }

    /// Starts the clock when playback can run, from the current position.
    pub(super) fn sync_clock(&mut self, now: i64) {
        let can_run = self.playing
            && self.phase == Phase::Running
            && self.metadata.is_some()
            && self.seeking.is_none()
            && !self.underrun
            && !self.ended;
        if can_run && !self.clock.is_running() {
            self.clock.set(self.position);
            self.clock.start(now);
            if self.next_timeupdate.is_none() {
                self.next_timeupdate = Some(self.position.saturating_add(TIMEUPDATE_INTERVAL_US));
            }
        }
    }

    /// Ends the seek `s`: its frame becomes current, the clock is at the
    /// target.
    fn finish_seek(&mut self, st: &mut State, s: Seeking, out: &mut Vec<Report>) {
        if let Some(i) = st
            .frames
            .iter()
            .position(|f| f.epoch == s.epoch && f.at_target)
        {
            for f in st.frames.take_front(i) {
                self.shared.recycle(f);
            }
            if let Some(f) = st.frames.pop_front() {
                self.show(f);
            }
        }
        let seekable = self.metadata.as_ref().is_none_or(|m| m.info.seekable);
        let position = if seekable {
            s.target_us
        } else {
            self.metadata.as_ref().map_or(0, |m| m.timeline.start_us)
        };
        self.seeking = None;
        self.position = position;
        self.clock.set(position);
        self.ended = s.ended;
        self.underrun = false;
        self.audio_gap = false;
        self.next_timeupdate = None;
        if s.report {
            out.push(Report::SeekDone { media_us: position });
        }
    }

    /// Makes `frame` current.
    fn show(&mut self, frame: shared::QueuedFrame) {
        self.generation += 1;
        self.quality.total_frames += 1;
        if let Some(image) = &frame.image {
            // A reduced copy is not the pool's: it goes.
            let reduced = self.reduced_from.take().is_some();
            if let Some(old) = self.last_image.replace(Arc::clone(image))
                && !reduced
            {
                self.shared.pool.give(old);
            }
        }
        self.current = Some(CurrentFrame {
            pts_us: frame.pts_us,
            duration_us: frame.duration_us,
            image: frame.image,
            generation: self.generation,
        });
    }

    /// Moves the media time to clock time `now`; returns whether samples
    /// were taken from the ring.
    pub(super) fn advance(&mut self, now: i64, st: &mut State, out: &mut Vec<Report>) -> bool {
        let end = self.end_time(st);
        let mut m = self.clock.media_time(now).min(end);
        // Stalled on the owner: the clock runs on with what is there (an
        // audio gap is silence), never an underrun.
        let stalled = st.stalled_on_owner();
        if stalled && st.audio_on && !audio_covers(st, m) {
            self.audio_gap = true;
        } else if self.audio_gap && audio_covers(st, m) {
            self.audio_gap = false;
        }
        if self.clock.is_running() {
            let covered = self.covered(st);
            // With the audio clock a starved ring is the underrun itself:
            // the clock does not pass the data, it stops at it, so `m` is
            // where the sound stopped (also when the owner looks late).
            let starved = self.audio_starved(st);
            if (starved || m > covered) && !stalled {
                if !starved {
                    m = covered.max(self.position);
                }
                self.clock.set(m);
                self.underrun = true;
                out.push(Report::Underrun { media_us: m });
            }
        } else if self.underrun && self.playing && (stalled || self.can_resume(st, m)) {
            self.underrun = false;
            self.clock.start(now);
            out.push(Report::Resumed { media_us: m });
        }
        self.take_frames(st, m);
        let consumed = self.consume_samples(st, m);
        self.position = m;
        if self.playing && m >= end && end_reached(st, end) {
            self.position = end;
            self.clock.set(end);
            self.playing = false;
            self.ended = true;
            self.next_timeupdate = None;
            out.push(Report::Ended { media_us: end });
        } else if self.clock.is_running()
            && m < end
            && let Some(t) = self.next_timeupdate
            && m >= t
        {
            out.push(Report::TimeUpdate { media_us: m });
            self.next_timeupdate = Some(m.saturating_add(TIMEUPDATE_INTERVAL_US));
        }
        consumed
    }

    /// Makes the frame with the greatest time at or before `m` current;
    /// the frames passed over are dropped.
    fn take_frames(&mut self, st: &mut State, m: i64) {
        while let Some(f) = st.frames.front() {
            if f.epoch == st.epoch && f.pts_us > m {
                break;
            }
            let Some(f) = st.frames.pop_front() else {
                break;
            };
            if f.epoch != st.epoch {
                self.shared.recycle(f);
                continue;
            }
            let next_due = st
                .frames
                .front()
                .is_some_and(|n| n.epoch == st.epoch && n.pts_us <= m);
            if next_due {
                self.quality.total_frames += 1;
                // A video that is not painted takes its frames in batches
                // ([`Pipeline::next_wake`]): the ones passed over are not
                // drops that anyone could see.
                if st.painted {
                    self.quality.dropped_frames += 1;
                }
                self.shared.recycle(f);
            } else {
                self.show(f);
            }
        }
    }

    /// Whether playback can go on after an underrun at `m`: the audio has
    /// `RESUME_AHEAD_US` decoded (or ended, or fills the ring); without
    /// audio, or in an audio gap, a frame is queued.
    fn can_resume(&self, st: &State, m: i64) -> bool {
        if st.audio_on && !self.audio_gap {
            return st.audio_drained == Some(st.epoch)
                || (st.demux_epoch == st.epoch
                    && (st.ring_full()
                        || st
                            .audio_end_us
                            .is_some_and(|e| e >= m.saturating_add(RESUME_AHEAD_US))));
        }
        !st.video_on
            || st.video_drained == Some(st.epoch)
            || st.frames.iter().any(|f| f.epoch == st.epoch)
    }

    /// The silent output: consumes the samples up to the media time `m`
    /// (not with an external consumer).
    fn consume_samples(&mut self, st: &State, m: i64) -> bool {
        if self.output != AudioOutput::Silent || !st.audio_on || st.demux_epoch != st.epoch {
            return false;
        }
        let (Some((base_pos, base_us)), Some(ring)) = (st.ring_base, &st.ring) else {
            return false;
        };
        let target = base_pos.saturating_add(us_to_frames(m.saturating_sub(base_us)));
        let read = ring.read_position();
        if target <= read {
            return false;
        }
        let n = target - read;
        match &mut self.capture {
            Some(buf) => {
                let mut chunk = vec![0.0f32; 2 * 1024];
                let mut left = n;
                while left > 0 {
                    let want = usize::try_from(left.min(1024)).unwrap_or(1024);
                    let got = ring.pop(&mut chunk[..want * 2]);
                    if got == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..got * 2]);
                    left -= got as u64;
                }
            }
            None => {
                ring.skip(n);
            }
        }
        true
    }
}
