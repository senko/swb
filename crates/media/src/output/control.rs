//! The control of the audio output (ADR 0047 section 8): when the device is
//! opened, started, stopped, closed, and what happens when it does not work.
//! The real-time work is in `mix.rs`; nothing here runs on the device's
//! real-time thread.
//!
//! **States** ([`Phase`]):
//!
//! - `Closed`: no device. [`Output::request`] (an element that plays with
//!   audio) opens one.
//! - `Connecting`: the device is open, and the stream has not reported
//!   `Streaming`. The elements play on the silent output meanwhile.
//! - `Running`: the device streams; elements join the mixer
//!   ([`Output::attach`]).
//! - `Parked`: the device is open but stopped (`set_active(false)`: no
//!   callback runs, no CPU), because no source is mixed (the device runs on
//!   for 0.5 s after the last source left, so a loop's wrap or a seek does
//!   not restart it). It closes 30 s after it stopped.
//! - `Waking`: stopped, and asked to run again, until it streams.
//! - `Failed`: the connect failed (no server: at once), the stream did not
//!   stream within 2 s (no device, no link), or it failed while playing.
//!   One `warn!` for the process. Every element is on the silent output;
//!   [`Output::request`] tries again 30 s after the failure.
//!
//! Time comes from the injected [`MonoClock`]; [`Output::tick`] does what
//! the time and the device's state ask. A timer thread calls it at the
//! deadlines (and when the device reports a state), so no element has to
//! poll; a test builds the output without it ([`Output::manual`]) and calls
//! `tick` itself.
//!
//! **Locks.** The control state is under one mutex, which the real-time
//! thread never takes; the device's callbacks (`Sink::state`) take none (they
//! store an atomic and wake the timer thread), so a thread may hold the
//! mutex while it calls into the device (`set_active`, the drop).

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::Thread;
use std::time::Duration;

use super::source::Source;
use super::{Backend, Device, DeviceState, MAX_SOURCES, Mix, MonoClock, Reading, Sink, Waker};
use crate::ring::SampleRing;

/// The device's node latency (frames at 48 kHz; ADR 0047 section 8).
#[cfg(feature = "pipewire")]
pub(super) const LATENCY_FRAMES: u32 = 1024;
/// The wait for `Streaming` after a connect or a restart.
const CONNECT_TIMEOUT_NS: u64 = 2_000_000_000;
/// The wait before another connect after a failure.
const RETRY_AFTER_NS: u64 = 30_000_000_000;
/// The time a stopped device stays open.
const CLOSE_AFTER_NS: u64 = 30_000_000_000;
/// How long a device that streams for a request waits for its source before
/// it stops.
const KEEP_AFTER_REQUEST_NS: u64 = 100_000_000;
/// How long a device that streams keeps running after its last source left
/// (a loop's wrap and a seek replace the source within moments; a pause
/// costs half a second of callbacks).
const KEEP_AFTER_LEAVE_NS: u64 = 500_000_000;
/// A fading source that the callback has not faded by now is freed anyway
/// (no callback runs).
const FADE_TIMEOUT_NS: u64 = 100_000_000;
/// How often the owner looks for faded sources.
const REAP_POLL_NS: u64 = 20_000_000;
/// The most wakers that wait for a state change.
const MAX_WAKERS: usize = 256;

/// The states of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No device.
    Closed,
    /// The device is open and does not stream yet.
    Connecting,
    /// The device streams; sources can join.
    Running,
    /// The device is open and stopped.
    Parked,
    /// The device was asked to run again.
    Waking,
    /// The device failed; the elements use the silent output.
    Failed,
}

/// The state with its times (nanoseconds of the output's clock).
#[derive(Debug, Clone, Copy)]
enum State {
    Closed,
    Connecting { since: u64 },
    Running,
    Parked { since: u64 },
    Waking { since: u64 },
    Failed { retry_at: u64 },
}

impl State {
    fn phase(self) -> Phase {
        match self {
            State::Closed => Phase::Closed,
            State::Connecting { .. } => Phase::Connecting,
            State::Running => Phase::Running,
            State::Parked { .. } => Phase::Parked,
            State::Waking { .. } => Phase::Waking,
            State::Failed { .. } => Phase::Failed,
        }
    }
}

/// A source that was told to fade out and waits for the callback to finish
/// the fade.
#[derive(Debug, Clone, Copy)]
struct Retired {
    index: usize,
    epoch: u64,
    since: u64,
}

struct Ctl {
    device: Option<Box<dyn Device>>,
    state: State,
    /// The number of the device session (each connect and each failure
    /// starts a new one): a source of an older session is gone.
    session: u64,
    /// Sources that are mixed (not fading).
    attached: usize,
    /// When the device started to stream (the output's clock) and the frames
    /// that the mixer had pulled by then: for the log of a run.
    ran_since: Option<(u64, u64)>,
    /// A running device without sources keeps running until this time (the
    /// last request plus [`KEEP_AFTER_REQUEST_NS`], or the last leave plus
    /// [`KEEP_AFTER_LEAVE_NS`]): the element that asked joins as soon as its
    /// page looks at it, a loop joins again after its wrap, and a device
    /// that stopped before that would start again.
    keep_until: u64,
    retired: Vec<Retired>,
    wakers: Vec<Waker>,
    connects: u64,
    failures: u64,
    requests: u64,
    last_error: Option<String>,
}

/// The clock data of the sources at the failure of a device.
#[derive(Default)]
pub(super) struct Finals {
    /// The output's clock when the device failed.
    pub(super) at_ns: u64,
    /// (slot, attachment, last reading) of each source that was mixed.
    pub(super) readings: Vec<(usize, u64, Reading)>,
}

pub(super) struct Inner {
    pub(super) mix: Mix,
    /// The sources' clocks at the last failure of a device.
    pub(super) finals: Mutex<Finals>,
    backend: Box<dyn Backend>,
    pub(super) clock: Arc<dyn MonoClock>,
    /// The last state that the device reported (a `DeviceState`).
    device_state: AtomicU8,
    /// The session of `Ctl`, for the sources' `is_live` without the lock.
    pub(super) session: AtomicU64,
    ctl: Mutex<Ctl>,
    timer: OnceLock<Option<Thread>>,
    use_timer_thread: bool,
    shutdown: AtomicBool,
}

/// One `warn!` for the process.
static WARNED: AtomicBool = AtomicBool::new(false);

fn encode(state: DeviceState) -> u8 {
    match state {
        DeviceState::Unconnected => 0,
        DeviceState::Connecting => 1,
        DeviceState::Paused => 2,
        DeviceState::Streaming => 3,
        DeviceState::Error => 4,
    }
}

fn decode(value: u8) -> DeviceState {
    match value {
        0 => DeviceState::Unconnected,
        1 => DeviceState::Connecting,
        2 => DeviceState::Paused,
        3 => DeviceState::Streaming,
        _ => DeviceState::Error,
    }
}

impl Sink for Inner {
    fn fill(&self, out: &mut [f32], delay_frames: i64) {
        self.mix.fill(out, delay_frames);
    }

    fn state(&self, state: DeviceState) {
        self.device_state.store(encode(state), Ordering::Release);
        self.poke();
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Ctl> {
        self.ctl.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wakes the timer thread, so that it looks at the state now.
    fn poke(&self) {
        if let Some(Some(thread)) = self.timer.get() {
            thread.unpark();
        }
    }

    /// Does what the time and the device's state ask; returns the clock
    /// time of the next deadline, if there is one.
    pub(super) fn tick(&self) -> Option<u64> {
        let now = self.clock.now_ns();
        let (deadline, wakers) = {
            let mut ctl = self.lock();
            self.tick_locked(&mut ctl, now)
        };
        for waker in wakers {
            waker();
        }
        deadline
    }

    fn tick_locked(&self, ctl: &mut Ctl, now: u64) -> (Option<u64>, Vec<Waker>) {
        let device = decode(self.device_state.load(Ordering::Acquire));
        let mut changed = false;
        match ctl.state {
            State::Connecting { since } | State::Waking { since } => {
                let connecting = matches!(ctl.state, State::Connecting { .. });
                if device == DeviceState::Streaming {
                    log::debug!(
                        "audio output: streaming {} ms after the {}",
                        now.saturating_sub(since) / 1_000_000,
                        if connecting { "connect" } else { "restart" }
                    );
                    ctl.state = State::Running;
                    ctl.ran_since = Some((now, self.mix.frames()));
                    changed = true;
                } else if device == DeviceState::Error
                    || (!connecting && device == DeviceState::Unconnected)
                {
                    self.fail(ctl, now, "the stream failed");
                    changed = true;
                } else if now.saturating_sub(since) >= CONNECT_TIMEOUT_NS {
                    self.fail(
                        ctl,
                        now,
                        "the stream did not start within 2 s (no device or link)",
                    );
                    changed = true;
                }
            }
            State::Running => match device {
                DeviceState::Error | DeviceState::Unconnected => {
                    self.fail(ctl, now, "the stream failed while playing");
                    changed = true;
                }
                // The graph stopped to call the stream (its link went
                // away): the sources cannot be mixed until it streams
                // again.
                DeviceState::Paused => {
                    ctl.state = State::Waking { since: now };
                    changed = true;
                }
                _ => {}
            },
            State::Parked { .. } => {
                if matches!(device, DeviceState::Error | DeviceState::Unconnected) {
                    self.fail(ctl, now, "the stream failed while stopped");
                    changed = true;
                }
            }
            State::Closed | State::Failed { .. } => {}
        }
        self.reap(ctl, now);
        if matches!(ctl.state, State::Running)
            && ctl.attached == 0
            && ctl.retired.is_empty()
            && now >= ctl.keep_until
        {
            if let Some(device) = ctl.device.as_mut() {
                device.set_active(false);
            }
            self.log_run(ctl, now);
            ctl.state = State::Parked { since: now };
        }
        if let State::Parked { since } = ctl.state
            && now.saturating_sub(since) >= CLOSE_AFTER_NS
        {
            ctl.device = None;
            ctl.state = State::Closed;
        }
        let wakers = if changed {
            std::mem::take(&mut ctl.wakers)
        } else {
            Vec::new()
        };
        (Inner::next_deadline(ctl, now), wakers)
    }

    /// Frees the sources whose fade ended, or that no callback fades.
    fn reap(&self, ctl: &mut Ctl, now: u64) {
        let mix = &self.mix;
        ctl.retired.retain(|r| {
            let done =
                mix.is_faded(r.index, r.epoch) || now.saturating_sub(r.since) >= FADE_TIMEOUT_NS;
            if done {
                mix.free(r.index, r.epoch);
            }
            !done
        });
    }

    fn next_deadline(ctl: &Ctl, now: u64) -> Option<u64> {
        let state_deadline = match ctl.state {
            State::Connecting { since } | State::Waking { since } => {
                Some(since.saturating_add(CONNECT_TIMEOUT_NS))
            }
            State::Parked { since } => Some(since.saturating_add(CLOSE_AFTER_NS)),
            State::Running | State::Closed | State::Failed { .. } => None,
        };
        let reap = (!ctl.retired.is_empty()).then(|| now.saturating_add(REAP_POLL_NS));
        // A running device without sources stops when the grace ends.
        let park =
            (matches!(ctl.state, State::Running) && ctl.attached == 0 && ctl.retired.is_empty())
                .then_some(ctl.keep_until);
        [state_deadline, reap, park].into_iter().flatten().min()
    }

    /// Logs what the device did since it ran (the measurements of M14
    /// feature 8: the frames it took per second, its delay and its cycle),
    /// at the debug level.
    fn log_run(&self, ctl: &mut Ctl, now: u64) {
        let Some((since, frames)) = ctl.ran_since.take() else {
            return;
        };
        let ms = now.saturating_sub(since) / 1_000_000;
        let taken = self.mix.frames().saturating_sub(frames);
        let (delay, cycle) = self.mix.last_cycle();
        log::debug!(
            "audio output: ran {ms} ms, {taken} frames ({} a second), delay {delay} frames, \
             cycle {cycle} frames",
            taken * 1000 / ms.max(1)
        );
    }

    /// The device failed or did not stream: it closes, every source is
    /// gone (the elements continue on the silent output), one `warn!` for
    /// the process, and the next try is 30 s away.
    fn fail(&self, ctl: &mut Ctl, now: u64, why: &str) {
        // The drop stops the real-time thread: no callback runs after it.
        ctl.device = None;
        self.log_run(ctl, now);
        // The last look at each clock: the sources go on from it in real
        // time ([`super::Source::clock_us`]), so that an element's time does
        // not stand still between the failure and its next step.
        *self.finals.lock().unwrap_or_else(PoisonError::into_inner) = Finals {
            at_ns: now,
            readings: self.mix.readings(),
        };
        self.mix.clear();
        ctl.retired.clear();
        ctl.attached = 0;
        ctl.session += 1;
        self.session.store(ctl.session, Ordering::Release);
        ctl.failures += 1;
        ctl.last_error = Some(why.to_owned());
        ctl.state = State::Failed {
            retry_at: now.saturating_add(RETRY_AFTER_NS),
        };
        if WARNED.swap(true, Ordering::Relaxed) {
            log::debug!("audio output: {why}");
        } else {
            log::warn!("audio output: {why}; media plays on the silent output");
        }
    }
}

/// The process-wide audio output: the mixer and the device behind it.
pub struct Output {
    inner: Arc<Inner>,
}

/// A look at the output's state (tests and measurements).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputStats {
    /// The state.
    pub phase: Phase,
    /// Sources that are mixed.
    pub attached: usize,
    /// Sources that fade out.
    pub retired: usize,
    /// Slots of the table in use.
    pub in_use: usize,
    /// Devices opened so far.
    pub connects: u64,
    /// Failures so far.
    pub failures: u64,
    /// Calls of [`Output::request`] so far.
    pub requests: u64,
    /// Cycles of the callback so far (all devices).
    pub cycles: u64,
    /// The last failure's reason.
    pub last_error: Option<String>,
}

impl Output {
    /// An output over `backend`, with a timer thread (started at the first
    /// [`Output::request`]).
    pub fn new(backend: Box<dyn Backend>, clock: Arc<dyn MonoClock>) -> Arc<Output> {
        Output::build(backend, clock, true)
    }

    /// An output without a timer thread: the owner calls [`Output::tick`]
    /// (tests with a [`super::ManualClock`]).
    pub fn manual(backend: Box<dyn Backend>, clock: Arc<dyn MonoClock>) -> Arc<Output> {
        Output::build(backend, clock, false)
    }

    /// The output over PipeWire (the GUI), on the system clock.
    #[cfg(feature = "pipewire")]
    pub fn pipewire() -> Arc<Output> {
        Output::new(Box::new(super::pw::PipeWire), Arc::new(super::SystemClock))
    }

    fn build(backend: Box<dyn Backend>, clock: Arc<dyn MonoClock>, thread: bool) -> Arc<Output> {
        Arc::new(Output {
            inner: Arc::new(Inner {
                mix: Mix::new(Arc::clone(&clock)),
                finals: Mutex::new(Finals::default()),
                backend,
                clock,
                device_state: AtomicU8::new(encode(DeviceState::Unconnected)),
                session: AtomicU64::new(0),
                ctl: Mutex::new(Ctl {
                    device: None,
                    state: State::Closed,
                    session: 0,
                    attached: 0,
                    ran_since: None,
                    keep_until: 0,
                    retired: Vec::new(),
                    wakers: Vec::new(),
                    connects: 0,
                    failures: 0,
                    requests: 0,
                    last_error: None,
                }),
                timer: OnceLock::new(),
                use_timer_thread: thread,
                shutdown: AtomicBool::new(false),
            }),
        })
    }

    /// The clock of the output.
    pub fn clock(&self) -> &Arc<dyn MonoClock> {
        &self.inner.clock
    }

    /// Does what the time and the device's state ask now (see the module
    /// comment); returns the clock time of the next deadline.
    pub fn tick(&self) -> Option<u64> {
        self.inner.tick()
    }

    /// The state now (after a [`Output::tick`]).
    pub fn phase(&self) -> Phase {
        self.inner.tick();
        self.inner.lock().state.phase()
    }

    /// Whether sources can join now: the device streams.
    pub fn is_ready(&self) -> bool {
        self.phase() == Phase::Running
    }

    /// Whether the mixer's table has no free slot (the 33rd element cannot
    /// join until one is freed; a fading source holds its slot until the
    /// callback has faded it).
    pub fn is_full(&self) -> bool {
        self.inner.mix.in_use() >= MAX_SOURCES
    }

    /// A look at the state and the counters.
    pub fn stats(&self) -> OutputStats {
        let ctl = self.inner.lock();
        OutputStats {
            phase: ctl.state.phase(),
            attached: ctl.attached,
            retired: ctl.retired.len(),
            in_use: self.inner.mix.in_use(),
            connects: ctl.connects,
            failures: ctl.failures,
            requests: ctl.requests,
            cycles: self.inner.mix.cycles(),
            last_error: ctl.last_error.clone(),
        }
    }

    /// The mixer, for the device and for tests.
    pub fn mix(&self) -> &Mix {
        &self.inner.mix
    }

    /// An element plays with audio: opens the device when there is none,
    /// or starts a stopped one. After a failure it opens one again only
    /// when `retry` says that the element's `play()` asks (ADR 0047 section
    /// 8: the next `play()` after 30 s), and the failure is 30 s ago. It
    /// returns at once; `waker` is called when the state changes to running
    /// or failed (the page then looks at its elements again).
    pub fn request(&self, waker: Option<&Waker>, retry: bool) {
        let inner = &self.inner;
        let now = inner.clock.now_ns();
        {
            let mut ctl = inner.lock();
            ctl.keep_until = now.saturating_add(KEEP_AFTER_REQUEST_NS);
            ctl.requests += 1;
            match ctl.state {
                State::Closed => self.connect(&mut ctl, now),
                State::Failed { retry_at } if retry && now >= retry_at => {
                    self.connect(&mut ctl, now);
                }
                State::Parked { .. } => {
                    if let Some(device) = ctl.device.as_mut() {
                        device.set_active(true);
                    }
                    ctl.state = State::Waking { since: now };
                }
                State::Connecting { .. }
                | State::Waking { .. }
                | State::Running
                | State::Failed { .. } => {}
            }
            let waiting = matches!(ctl.state, State::Connecting { .. } | State::Waking { .. });
            if let Some(waker) = waker
                && waiting
                && ctl.wakers.len() < MAX_WAKERS
                && !ctl.wakers.iter().any(|w| Arc::ptr_eq(w, waker))
            {
                ctl.wakers.push(Arc::clone(waker));
            }
        }
        // A device that streams at once (a fake one) runs now.
        inner.tick();
        self.start_timer();
        inner.poke();
    }

    /// Opens the device (the control lock is held).
    fn connect(&self, ctl: &mut Ctl, now: u64) {
        let inner = &self.inner;
        ctl.session += 1;
        inner.session.store(ctl.session, Ordering::Release);
        inner
            .device_state
            .store(encode(DeviceState::Connecting), Ordering::Release);
        let sink: Arc<dyn Sink> = Arc::clone(inner) as Arc<dyn Sink>;
        let started = std::time::Instant::now();
        match inner.backend.connect(sink) {
            Ok(device) => {
                log::debug!("audio output: device opened in {:?}", started.elapsed());
                ctl.device = Some(device);
                ctl.connects += 1;
                ctl.state = State::Connecting { since: now };
            }
            Err(why) => inner.fail(ctl, now, &format!("cannot connect: {why}")),
        }
    }

    /// Starts the timer thread, once.
    fn start_timer(&self) {
        let inner = &self.inner;
        if !inner.use_timer_thread {
            return;
        }
        inner.timer.get_or_init(|| {
            let weak = Arc::downgrade(inner);
            std::thread::Builder::new()
                .name("swb-audio-timer".to_owned())
                .spawn(move || timer_loop(&weak))
                .map(|handle| handle.thread().clone())
                .map_err(|e| log::warn!("audio output: no timer thread: {e}"))
                .ok()
        });
    }

    /// Puts the ring of an element in the mixer. `lead` aligns it with the
    /// element's clock ([`super::lead_frames`]): silence first when positive,
    /// frames skipped when negative. `None` when the device does not stream
    /// (the caller keeps the silent output) or when the table is full (the
    /// 33rd source).
    pub fn attach(self: &Arc<Self>, ring: Arc<SampleRing>, lead: i64) -> Option<Source> {
        self.inner.tick();
        let (index, epoch, session) = {
            let mut ctl = self.inner.lock();
            if !matches!(ctl.state, State::Running) {
                return None;
            }
            if lead < 0 {
                ring.skip(lead.unsigned_abs());
            }
            let (index, epoch) = self
                .inner
                .mix
                .attach(ring, u64::try_from(lead).unwrap_or(0))?;
            ctl.attached += 1;
            (index, epoch, ctl.session)
        };
        Some(Source::new(Arc::clone(self), index, epoch, session))
    }

    /// A source leaves (its `Drop`): it fades out, and the owner frees its
    /// slot when the callback has faded it (at a later tick).
    pub(super) fn retire(&self, index: usize, epoch: u64, session: u64) {
        let inner = &self.inner;
        {
            let mut ctl = inner.lock();
            if ctl.session != session {
                return;
            }
            if inner.mix.begin_fade(index, epoch) {
                ctl.attached = ctl.attached.saturating_sub(1);
                let now = inner.clock.now_ns();
                ctl.retired.push(Retired {
                    index,
                    epoch,
                    since: now,
                });
                // The stream goes on for a while: a loop's wrap or a seek
                // drops its source and attaches a new one within moments,
                // and a device that stopped and started again would add
                // 20 to 40 ms and a delay of silence to every wrap.
                ctl.keep_until = ctl.keep_until.max(now.saturating_add(KEEP_AFTER_LEAVE_NS));
            }
        }
        inner.poke();
    }

    pub(super) fn inner(&self) -> &Inner {
        &self.inner
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let inner = &self.inner;
        inner.shutdown.store(true, Ordering::Release);
        let device = inner.lock().device.take();
        // The drop stops the real-time thread, without the control lock.
        drop(device);
        inner.mix.clear();
        inner.poke();
    }
}

/// The timer thread: ticks at the deadlines and when the device reports.
fn timer_loop(weak: &std::sync::Weak<Inner>) {
    loop {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        if inner.shutdown.load(Ordering::Acquire) {
            return;
        }
        let deadline = inner.tick();
        let now = inner.clock.now_ns();
        drop(inner);
        match deadline {
            Some(at) => std::thread::park_timeout(Duration::from_nanos(
                at.saturating_sub(now).max(1_000_000),
            )),
            None => std::thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::super::ManualClock;
    use super::super::fake::FakeDevice;
    use super::*;

    const SECOND_MS: u64 = 1000;

    struct Rig {
        output: Arc<Output>,
        device: FakeDevice,
        clock: Arc<ManualClock>,
    }

    fn rig() -> Rig {
        let device = FakeDevice::new();
        let clock = Arc::new(ManualClock::default());
        // The clock does not start at 0: a deadline at 0 would hide errors.
        clock.advance_ms(5);
        let output = Output::manual(
            Box::new(device.clone()),
            Arc::clone(&clock) as Arc<dyn MonoClock>,
        );
        Rig {
            output,
            device,
            clock,
        }
    }

    fn ring() -> Arc<SampleRing> {
        let ring = Arc::new(SampleRing::new(48_000));
        ring.push(&vec![0.1; 48_000 * 2]);
        ring
    }

    impl Rig {
        /// A running output with one source.
        fn playing(&self) -> Source {
            self.output.request(None, true);
            assert!(self.output.is_ready());
            self.output.attach(ring(), 0).expect("a source joins")
        }

        /// Pulls cycles until the callback faded what fades, and lets the
        /// grace of the last request and of the last leave pass; then ticks.
        fn settle_fades(&self) {
            for _ in 0..4 {
                self.device.pull(512, 512);
            }
            self.clock.advance_ms(KEEP_AFTER_LEAVE_NS / 1_000_000);
            self.output.tick();
        }
    }

    #[test]
    fn nothing_connects_until_an_element_asks() {
        let r = rig();
        assert_eq!(r.output.phase(), Phase::Closed);
        assert_eq!(r.device.connects(), 0);
        assert!(r.output.attach(ring(), 0).is_none(), "not running");
        r.output.request(None, true);
        assert_eq!(r.device.connects(), 1);
        assert_eq!(r.output.phase(), Phase::Running);
        // A second request does not open a second device.
        r.output.request(None, true);
        assert_eq!(r.device.connects(), 1);
    }

    #[test]
    fn the_device_stops_when_no_source_plays_and_closes_after_30_seconds() {
        let r = rig();
        let source = r.playing();
        assert!(r.device.is_active());
        assert_eq!(r.output.stats().attached, 1);
        drop(source);
        // The source fades out on the next cycles; the device keeps running
        // until then.
        assert_eq!(r.output.stats().retired, 1);
        r.output.tick();
        assert!(r.device.is_active());
        r.settle_fades();
        let stats = r.output.stats();
        assert_eq!(
            (stats.phase, stats.in_use, stats.retired),
            (Phase::Parked, 0, 0)
        );
        // No callback runs while it is stopped.
        assert!(!r.device.is_active());
        assert!(r.device.pull(512, 0).is_none());
        // It stays open for 30 s.
        r.clock.advance_ms(29 * SECOND_MS);
        r.output.tick();
        assert!(r.device.is_open());
        r.clock.advance_ms(SECOND_MS);
        r.output.tick();
        assert!(!r.device.is_open());
        assert_eq!(r.device.closes(), 1);
        assert_eq!(r.output.phase(), Phase::Closed);
    }

    #[test]
    fn a_new_source_wakes_a_stopped_device() {
        let r = rig();
        drop(r.playing());
        r.settle_fades();
        assert_eq!(r.output.phase(), Phase::Parked);
        let woken = Arc::new(AtomicUsize::new(0));
        let waker: Waker = {
            let woken = Arc::clone(&woken);
            Arc::new(move || {
                woken.fetch_add(1, Ordering::Relaxed);
            })
        };
        r.device.auto_stream(false);
        r.output.request(Some(&waker), true);
        assert!(r.device.is_active());
        assert_eq!(r.output.phase(), Phase::Waking);
        assert!(r.output.attach(ring(), 0).is_none(), "not streaming yet");
        r.device.report(DeviceState::Streaming);
        assert_eq!(r.output.phase(), Phase::Running);
        assert_eq!(woken.load(Ordering::Relaxed), 1);
        assert!(r.output.attach(ring(), 0).is_some());
        // The same device: one connect.
        assert_eq!(r.device.connects(), 1);
    }

    #[test]
    fn no_server_falls_back_at_once_and_tries_again_after_30_seconds() {
        let r = rig();
        r.device.refuse(Some("Host is down (-112)"));
        r.output.request(None, true);
        let stats = r.output.stats();
        assert_eq!((stats.phase, stats.failures), (Phase::Failed, 1));
        assert!(
            stats
                .last_error
                .as_deref()
                .is_some_and(|e| e.contains("Host is down"))
        );
        assert!(r.output.attach(ring(), 0).is_none());
        // The elements ask again at each play(): no connect before 30 s.
        r.clock.advance_ms(29 * SECOND_MS);
        r.output.request(None, true);
        assert_eq!(r.output.stats().failures, 1);
        // The server is there at the next play() after 30 s: not for a
        // request that does not come from a play().
        r.device.refuse(None);
        r.clock.advance_ms(SECOND_MS);
        r.output.request(None, false);
        assert_eq!(r.output.phase(), Phase::Failed);
        r.output.request(None, true);
        assert_eq!(r.output.phase(), Phase::Running);
        assert_eq!(r.device.connects(), 1);
    }

    #[test]
    fn no_stream_within_two_seconds_falls_back() {
        let r = rig();
        r.device.auto_stream(false);
        let woken = Arc::new(AtomicUsize::new(0));
        let waker: Waker = {
            let woken = Arc::clone(&woken);
            Arc::new(move || {
                woken.fetch_add(1, Ordering::Relaxed);
            })
        };
        r.output.request(Some(&waker), true);
        assert_eq!(r.output.phase(), Phase::Connecting);
        // The timer's deadline is the 2 s.
        let deadline = r.output.tick().expect("a deadline");
        assert_eq!(deadline, r.clock.now_ns() + 2_000_000_000);
        r.device.report(DeviceState::Paused);
        r.clock.advance_ms(1999);
        assert_eq!(r.output.phase(), Phase::Connecting);
        assert_eq!(woken.load(Ordering::Relaxed), 0);
        r.clock.advance_ms(1);
        assert_eq!(r.output.phase(), Phase::Failed);
        assert_eq!(woken.load(Ordering::Relaxed), 1);
        assert!(!r.device.is_open(), "the device closed");
        // A later play() after 30 s tries again.
        r.clock.advance_ms(30 * SECOND_MS);
        r.output.request(None, true);
        assert_eq!(r.device.connects(), 2);
        assert_eq!(r.output.phase(), Phase::Connecting);
    }

    #[test]
    fn an_error_while_playing_drops_every_source_and_frees_the_rings() {
        let r = rig();
        r.output.request(None, true);
        let ring_a = ring();
        let ring_b = ring();
        let a = r.output.attach(Arc::clone(&ring_a), 0).expect("a");
        let b = r.output.attach(Arc::clone(&ring_b), 0).expect("b");
        r.device.pull(512, 512);
        assert!(a.is_live() && b.is_live());
        let before = a.clock_us();
        r.device.report(DeviceState::Error);
        assert_eq!(r.output.phase(), Phase::Failed);
        assert!(!a.is_live() && !b.is_live());
        assert_eq!(r.output.stats().in_use, 0);
        assert_eq!(Arc::strong_count(&ring_a), 1);
        assert_eq!(Arc::strong_count(&ring_b), 1);
        // The clock of a dead source stands at its last value.
        assert_eq!(a.clock_us(), before);
        drop(a);
        drop(b);
        assert_eq!(r.output.stats().attached, 0);
        assert_eq!(r.output.stats().retired, 0);
        assert!(!r.device.is_open());
    }

    #[test]
    fn a_stream_that_pauses_while_running_waits_for_streaming_again() {
        let r = rig();
        let _source = r.playing();
        r.device.report(DeviceState::Paused);
        assert_eq!(r.output.phase(), Phase::Waking);
        assert!(r.output.attach(ring(), 0).is_none());
        r.device.report(DeviceState::Streaming);
        assert_eq!(r.output.phase(), Phase::Running);
        // If it does not come back, the same 2 s rule applies.
        r.device.report(DeviceState::Paused);
        assert_eq!(r.output.phase(), Phase::Waking);
        r.clock.advance_ms(2 * SECOND_MS);
        assert_eq!(r.output.phase(), Phase::Failed);
    }

    #[test]
    fn a_source_that_cannot_fade_is_freed_after_a_timeout() {
        let r = rig();
        let source = r.playing();
        drop(source);
        // No callback runs (the device is stuck): 100 ms later the slot is
        // freed anyway.
        r.clock.advance_ms(99);
        r.output.tick();
        assert_eq!(r.output.stats().in_use, 1);
        r.clock.advance_ms(1);
        r.output.tick();
        assert_eq!(r.output.stats().in_use, 0);
        // The stream runs on for the hold after a leave.
        assert_eq!(r.output.phase(), Phase::Running);
        r.clock.advance_ms(KEEP_AFTER_LEAVE_NS / 1_000_000);
        assert_eq!(r.output.phase(), Phase::Parked);
    }

    /// A loop's wrap or a seek: the source leaves and another joins within
    /// moments; the stream does not stop and start (each restart is 20 to
    /// 40 ms plus a fade-in and a delay of silence). Without the hold the
    /// device parks as soon as the fade ends.
    #[test]
    fn a_source_that_leaves_and_joins_again_does_not_restart_the_stream() {
        let r = rig();
        drop(r.playing());
        // The fade ends, 300 ms pass (the wrap's seek and decode).
        for _ in 0..4 {
            r.device.pull(512, 512);
        }
        r.clock.advance_ms(300);
        r.output.tick();
        assert_eq!(r.output.phase(), Phase::Running);
        assert!(r.device.is_active());
        // The element asks and joins again: the same running stream.
        r.output.request(None, true);
        let again = r.output.attach(ring(), 0);
        assert!(again.is_some());
        assert_eq!(r.device.connects(), 1);
        assert_eq!(r.output.stats().attached, 1);
        // The restarts that the hold saves: the stream never stopped.
        assert_eq!(r.output.stats().phase, Phase::Running);
    }

    #[test]
    fn the_33rd_source_gets_no_slot() {
        let r = rig();
        r.output.request(None, true);
        let sources: Vec<_> = (0..32)
            .map(|_| r.output.attach(ring(), 0).expect("a slot"))
            .collect();
        assert!(r.output.attach(ring(), 0).is_none());
        drop(sources);
        r.settle_fades();
        assert_eq!(r.output.stats().in_use, 0);
        assert!(r.output.attach(ring(), 0).is_none(), "parked: not running");
    }

    #[test]
    fn dropping_the_output_closes_the_device() {
        let r = rig();
        let source = r.playing();
        let Rig { output, device, .. } = r;
        drop(source);
        drop(output);
        assert!(!device.is_open());
        assert_eq!(device.closes(), 1);
    }

    #[test]
    fn the_timer_thread_ticks_by_itself() {
        // The real clock and the thread: a device that never streams fails
        // within about 2 s without any caller.
        let device = FakeDevice::new();
        device.auto_stream(false);
        let output = Output::new(
            Box::new(device.clone()),
            Arc::new(super::super::SystemClock),
        );
        let woken = Arc::new(AtomicUsize::new(0));
        let waker: Waker = {
            let woken = Arc::clone(&woken);
            Arc::new(move || {
                woken.fetch_add(1, Ordering::Relaxed);
            })
        };
        output.request(Some(&waker), true);
        device.report(DeviceState::Streaming);
        let started = std::time::Instant::now();
        while woken.load(Ordering::Relaxed) == 0 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the thread did not tick"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(output.stats().phase, Phase::Running);
    }
}
