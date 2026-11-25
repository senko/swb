//! A test owner of a media pipeline: a wake-up that the pipeline notifies,
//! and a driver that plays in virtual time (the clock jumps to the next due
//! time only when the pipeline has no pending work, ADR 0028 section 7).

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use swb_media::pipeline::{Pipeline, PipelineOptions, PipelineSlots, Report, SourceOpener};

use super::fixture;

/// The owner's wake-up: counts notifications and lets a test wait for one.
#[derive(Default)]
pub(crate) struct Waker {
    count: Mutex<u64>,
    cv: Condvar,
}

impl Waker {
    pub(crate) fn notify(&self) {
        *self.count.lock().expect("lock") += 1;
        self.cv.notify_all();
    }

    /// The notifications so far.
    pub(crate) fn count(&self) -> u64 {
        *self.count.lock().expect("lock")
    }

    /// Waits until the count passes `seen`, at most `timeout`.
    pub(crate) fn wait_past(&self, seen: u64, timeout: Duration) -> u64 {
        let guard = self.count.lock().expect("lock");
        let (guard, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |c| *c <= seen)
            .expect("lock");
        *guard
    }
}

/// Options with a wake-up and a bound of their own (tests run in parallel).
pub(crate) fn options(waker: &Arc<Waker>) -> PipelineOptions {
    let w = Arc::clone(waker);
    let mut o = PipelineOptions::new(Arc::new(move || w.notify()));
    o.slots = PipelineSlots::new(16);
    o.capture_audio = true;
    o
}

/// Opens a test file of `fixtures/media/`.
pub(crate) fn open_fixture(name: &str, waker: &Arc<Waker>) -> Pipeline {
    Pipeline::open(SourceOpener::file(fixture(name)), options(waker))
}

/// The longest a test waits for the pipeline's threads (the longest
/// legitimate wait is about 2 s, under load).
pub(crate) const HANG: Duration = Duration::from_secs(10);

/// Waits until the pipeline has no pending work; panics after `HANG`.
pub(crate) fn settle(p: &Pipeline, waker: &Waker) {
    let started = Instant::now();
    loop {
        let seen = waker.count();
        if !p.has_pending_work() {
            return;
        }
        assert!(started.elapsed() < HANG, "the pipeline does not settle");
        waker.wait_past(seen, Duration::from_secs(1));
    }
}

/// Settles and updates at `now` until nothing is pending at `now`;
/// collects the reports.
pub(crate) fn settle_update(p: &mut Pipeline, waker: &Waker, now: i64, out: &mut Vec<Report>) {
    loop {
        settle(p, waker);
        out.extend(p.update(now));
        if !p.has_pending_work() {
            return;
        }
    }
}

/// What a playback in virtual time shows.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Played {
    /// At each due time: the clock time, the media time and the time of the
    /// current frame.
    pub(crate) steps: Vec<(i64, i64, Option<i64>)>,
    /// The media times of the `timeupdate` points.
    pub(crate) timeupdates: Vec<i64>,
    /// The media time of the end.
    pub(crate) ended: Option<i64>,
    /// Every report, in order.
    pub(crate) reports: Vec<Report>,
    /// The clock time at the end of the run.
    pub(crate) now: i64,
}

/// Plays `p` in virtual time from clock time `now` until it ends, fails,
/// or the media time reaches `until`.
pub(crate) fn play_virtual(p: &mut Pipeline, waker: &Waker, now: i64, until: i64) -> Played {
    let mut played = Played {
        now,
        ..Played::default()
    };
    let mut reports = Vec::new();
    settle_update(p, waker, played.now, &mut reports);
    p.play(played.now);
    loop {
        settle_update(p, waker, played.now, &mut reports);
        if p.is_ended() || p.is_failed() || p.position() >= until {
            break;
        }
        let Some(due) = p.next_due() else {
            if p.is_waiting() {
                // No place yet: not pending work, but nothing to play.
                let seen = waker.count();
                if p.is_waiting() {
                    waker.wait_past(seen, Duration::from_millis(20));
                }
                continue;
            }
            break;
        };
        played.now = played.now.max(due);
        reports.extend(p.update(played.now));
        let step = (
            played.now,
            p.position(),
            p.current_frame().map(|f| f.pts_us),
        );
        // A due time that stays due without pending work is a spin.
        let repeats = played
            .steps
            .iter()
            .rev()
            .take_while(|s| **s == step)
            .count();
        assert!(repeats < 3, "the driver spins at {step:?}");
        played.steps.push(step);
    }
    for r in &reports {
        match r {
            Report::TimeUpdate { media_us } => played.timeupdates.push(*media_us),
            Report::Ended { media_us } => played.ended = Some(*media_us),
            _ => {}
        }
    }
    played.reports = reports;
    played
}
