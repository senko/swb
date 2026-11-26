//! The mixer (ADR 0047 section 8): the real-time half of the audio output.
//! [`Mix::fill`] runs on the device's real-time thread and sums the sample
//! rings of the playing elements into the device buffer.
//!
//! **The callback never blocks, allocates, frees or logs, and makes no
//! blocking system call** (it reads the clock, which is a vDSO call, not a
//! system call that can block). It works on a fixed table of [`MAX_SOURCES`] slots and on atomics;
//! its only lock is a `try_lock` of one slot's mutex, and it gives up the
//! slot for that cycle when the lock is taken.
//!
//! **The slot protocol** (the proof that the callback never frees):
//!
//! 1. A slot is `FREE`, `ACTIVE` (mixed), `FADING` (mixed down to silence)
//!    or `FADED` (silent, waits for the owner). Its data (the element's
//!    ring, the ramp state) is `Option<SlotData>` in a `Mutex`.
//! 2. The callback holds a slot's lock only while it mixes that slot, and
//!    only borrows the data: it never clones or drops the `Arc` of the
//!    ring. It never takes the lock with `lock()`: a `try_lock` that fails
//!    skips the slot for this cycle (the source plays silence, its clock
//!    stops for the cycle: counted in [`Mix::skipped`]).
//! 3. The owner takes the lock only with `try_lock` in a loop with
//!    `yield_now`, never with `lock()`: so the mutex never has a waiter, its
//!    unlock in the callback is a store, not a `futex` wake-up (a system
//!    call).
//!    The state is read a second time under the lock, and that reading
//!    decides (a stalled callback sees the slot of a new source, not the
//!    fade of the old one).
//! 4. The owner puts data in a `FREE` slot (under the lock) and then sets
//!    `ACTIVE` with release ordering: the callback, which loads the state
//!    with acquire ordering, sees the data. The owner takes data out of a
//!    slot only under the lock, and drops it after it released the lock, on
//!    its own thread. When the owner has the lock the callback holds no
//!    reference to the data, and it cannot get one without the lock, so the
//!    drop of the last `Arc` of a ring is never on the callback's thread.
//! 5. The owner ends a source in two steps: `FADING` (the callback ramps
//!    the gain to 0 in [`RAMP_FRAMES`] frames, then sets `FADED`; no click),
//!    then, when it sees `FADED` (or after a timeout, when no callback
//!    runs), it takes the data out and sets `FREE`.
//!
//! **The audio clock's data.** Per slot, the frames that the callback mixed
//! for the source (`taken`: frames of its ring, and the frames of silence
//! that align it, not the frames of an empty ring), and per cycle the time
//! of the callback and the device's delay. A sequence counter makes the
//! values one consistent snapshot for the owner ([`Mix::reading`]).

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU32, AtomicU64, Ordering, fence};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use super::MonoClock;
use crate::ring::SampleRing;

/// The most sources that the mixer mixes (elements that play with audio).
pub const MAX_SOURCES: usize = 32;
/// A change of the gain from 0 to 1 takes this many frames (10 ms at
/// 48 kHz); every other change takes at most as long.
pub const RAMP_FRAMES: u32 = 480;
/// The frames that the callback mixes at a time (stack space: 8 KiB).
const CHUNK: usize = 1024;

const FREE: u8 = 0;
const ACTIVE: u8 = 1;
const FADING: u8 = 2;
const FADED: u8 = 3;

/// What a slot holds while it is in use.
struct SlotData {
    ring: Arc<SampleRing>,
    /// The gain now (the ramp's state); 0 at the start, so a source starts
    /// with a fade-in.
    gain: f32,
    /// Frames of silence before the first frame of the ring (the source is
    /// aligned with its clock: [`super::lead_frames`]).
    pre_silence: u64,
}

struct Slot {
    state: AtomicU8,
    /// The attachment's number (the owner sets it before `ACTIVE`).
    epoch: AtomicU64,
    /// Frames mixed for the source since it joined.
    taken: AtomicU64,
    /// The last cycle found the ring empty.
    starved: AtomicBool,
    data: Mutex<Option<SlotData>>,
}

/// One consistent look at the clock data of a source ([`Mix::reading`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Frames mixed for the source since it joined.
    pub taken: u64,
    /// The delay between the last cycle's frames and the device (frames).
    pub delay_frames: i64,
    /// The time of the last cycle's callback (`MonoClock` nanoseconds).
    pub snap_ns: u64,
    /// The frames of the last cycle.
    pub cycle_frames: u32,
    /// The last cycle found the ring empty (the clock stops).
    pub starved: bool,
    /// Cycles run so far (0: none yet).
    pub cycles: u64,
}

/// The mixer: a fixed table of sources and the cycle's snapshot.
pub struct Mix {
    slots: Box<[Slot]>,
    clock: Arc<dyn MonoClock>,
    /// The attachment numbers (the owner's).
    next_epoch: AtomicU64,
    /// The sequence counter of the snapshot: odd while the callback writes.
    seq: AtomicU64,
    snap_ns: AtomicU64,
    snap_delay: AtomicI64,
    snap_frames: AtomicU32,
    cycles: AtomicU64,
    /// Frames of all cycles so far.
    frames: AtomicU64,
    skipped: AtomicU64,
}

/// The lock of a slot for the owner: `try_lock` in a loop (see the module
/// comment, point 3).
fn lock_owner(slot: &Slot) -> MutexGuard<'_, Option<SlotData>> {
    loop {
        match slot.data.try_lock() {
            Ok(guard) => return guard,
            Err(TryLockError::Poisoned(poisoned)) => return poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => std::thread::yield_now(),
        }
    }
}

impl Mix {
    /// An empty mixer; `clock` stamps the cycles.
    pub fn new(clock: Arc<dyn MonoClock>) -> Mix {
        let slots = (0..MAX_SOURCES)
            .map(|_| Slot {
                state: AtomicU8::new(FREE),
                epoch: AtomicU64::new(0),
                taken: AtomicU64::new(0),
                starved: AtomicBool::new(false),
                data: Mutex::new(None),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Mix {
            slots,
            clock,
            next_epoch: AtomicU64::new(1),
            seq: AtomicU64::new(0),
            snap_ns: AtomicU64::new(0),
            snap_delay: AtomicI64::new(0),
            snap_frames: AtomicU32::new(0),
            cycles: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
        }
    }

    /// Cycles of the callback so far.
    pub fn cycles(&self) -> u64 {
        self.cycles.load(Ordering::Relaxed)
    }

    /// Frames of all cycles so far.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// The delay and the frames of the last cycle (the device's numbers;
    /// the callback's snapshot, read without the sequence check: for logs).
    pub fn last_cycle(&self) -> (i64, u32) {
        (
            self.snap_delay.load(Ordering::Relaxed),
            self.snap_frames.load(Ordering::Relaxed),
        )
    }

    /// Slot visits that a held lock made the callback skip (tests).
    pub fn skipped(&self) -> u64 {
        self.skipped.load(Ordering::Relaxed)
    }

    /// The sources in the table, in any state but free.
    pub fn in_use(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.state.load(Ordering::Acquire) != FREE)
            .count()
    }

    // ----- The real-time side -----

    /// One cycle: fills `out` (interleaved stereo) with the sum of the
    /// sources, each with its gain, clamped to [-1, 1]. `delay_frames` is
    /// the delay between these frames and the device. Real-time safe (see
    /// the module comment).
    pub fn fill(&self, out: &mut [f32], delay_frames: i64) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let now = self.clock.now_ns();
        // Only this thread writes the counter.
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        // One scratch buffer for the cycle (zeroed once, not for each slot).
        let mut scratch = [0.0f32; CHUNK * 2];
        for slot in &self.slots {
            self.mix_slot(slot, out, frames, &mut scratch);
        }
        for s in out.iter_mut() {
            *s = if s.is_nan() { 0.0 } else { s.clamp(-1.0, 1.0) };
        }
        self.snap_ns.store(now, Ordering::Relaxed);
        self.snap_delay.store(delay_frames, Ordering::Relaxed);
        self.snap_frames
            .store(u32::try_from(frames).unwrap_or(u32::MAX), Ordering::Relaxed);
        self.cycles.fetch_add(1, Ordering::Relaxed);
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.seq.store(seq + 2, Ordering::Release);
    }

    fn mix_slot(&self, slot: &Slot, out: &mut [f32], frames: usize, scratch: &mut [f32]) {
        let seen = slot.state.load(Ordering::Acquire);
        self.mix_slot_seen(slot, seen, out, frames, scratch);
    }

    /// Mixes one slot. `seen` is the state that the callback read before it
    /// tried the lock: only a first look (the slot may be mixed at all). The
    /// state that decides is read again under the lock: a callback that
    /// stalled between the two (the owner can free and attach the slot again
    /// meanwhile) must not apply the fade of the old source to the new one.
    /// Under the lock the state and the data agree: the owner sets `FADED`
    /// before it takes the data out and `ACTIVE` after it put the data in.
    fn mix_slot_seen(
        &self,
        slot: &Slot,
        seen: u8,
        out: &mut [f32],
        frames: usize,
        scratch: &mut [f32],
    ) {
        if seen != ACTIVE && seen != FADING {
            return;
        }
        let Ok(mut guard) = slot.data.try_lock() else {
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let state = slot.state.load(Ordering::Acquire);
        if state != ACTIVE && state != FADING {
            return;
        }
        let Some(data) = guard.as_mut() else {
            return;
        };
        let fading = state == FADING;
        let target = if fading {
            0.0
        } else {
            data.ring.gain().clamp(0.0, 1.0)
        };
        let mut taken = slot.taken.load(Ordering::Relaxed);
        let mut gain = data.gain;
        let mut starved = false;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(CHUNK);
            let pre = usize::try_from(data.pre_silence.min(n as u64)).unwrap_or(0);
            data.pre_silence -= pre as u64;
            let want = n - pre;
            let popped = data.ring.pop_unscaled(&mut scratch[..want * 2]);
            let dst = &mut out[done * 2..(done + pre + popped) * 2];
            ramp_add(dst, &scratch[..popped * 2], pre, &mut gain, target);
            taken += (pre + popped) as u64;
            done += n;
            if popped < want {
                starved = true;
                break;
            }
        }
        if fading && (starved || gain <= 0.0) {
            gain = 0.0;
            slot.state.store(FADED, Ordering::Release);
        }
        data.gain = gain;
        slot.taken.store(taken, Ordering::Relaxed);
        slot.starved.store(starved, Ordering::Relaxed);
    }

    // ----- The owner's side -----

    /// A consistent look at the clock data of the source of `index`, if it
    /// is still the attachment `epoch`. `None` also when 8 reads in a row
    /// found the callback writing a snapshot (the owner keeps its last
    /// reading and asks again).
    pub fn reading(&self, index: usize, epoch: u64) -> Option<Reading> {
        let slot = self.slots.get(index)?;
        for _ in 0..8 {
            let seq = self.seq.load(Ordering::Acquire);
            if seq % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let reading = Reading {
                taken: slot.taken.load(Ordering::Relaxed),
                delay_frames: self.snap_delay.load(Ordering::Relaxed),
                snap_ns: self.snap_ns.load(Ordering::Relaxed),
                cycle_frames: self.snap_frames.load(Ordering::Relaxed),
                starved: slot.starved.load(Ordering::Relaxed),
                cycles: self.cycles.load(Ordering::Relaxed),
            };
            let slot_epoch = slot.epoch.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == seq {
                return (slot_epoch == epoch && slot.state.load(Ordering::Acquire) != FREE)
                    .then_some(reading);
            }
        }
        None
    }

    /// The readings of every source in the table: (slot, attachment, reading).
    /// For the owner, when the device failed: the last look at each clock.
    pub fn readings(&self) -> Vec<(usize, u64, Reading)> {
        (0..self.slots.len())
            .filter_map(|index| {
                let epoch = self.slots.get(index)?.epoch.load(Ordering::Relaxed);
                Some((index, epoch, self.reading(index, epoch)?))
            })
            .collect()
    }

    /// Puts a source in a free slot: its `ring` (the callback is its one
    /// consumer from now on), with `pre_silence` frames of silence before
    /// the first frame. Returns the slot and the attachment's number; `None`
    /// when the table is full (the 33rd source). The owner serializes the
    /// calls that change the table.
    pub fn attach(&self, ring: Arc<SampleRing>, pre_silence: u64) -> Option<(usize, u64)> {
        let (index, slot) = self
            .slots
            .iter()
            .enumerate()
            .find(|(_, s)| s.state.load(Ordering::Acquire) == FREE)?;
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        {
            let mut guard = lock_owner(slot);
            *guard = Some(SlotData {
                ring,
                gain: 0.0,
                pre_silence,
            });
        }
        slot.epoch.store(epoch, Ordering::Relaxed);
        slot.taken.store(0, Ordering::Relaxed);
        slot.starved.store(false, Ordering::Relaxed);
        slot.state.store(ACTIVE, Ordering::Release);
        Some((index, epoch))
    }

    /// Starts to fade the source out: the callback ramps it to silence in
    /// [`RAMP_FRAMES`] frames and then sets it faded. False when the slot is
    /// not that attachment, or not active.
    pub fn begin_fade(&self, index: usize, epoch: u64) -> bool {
        let Some(slot) = self.slots.get(index) else {
            return false;
        };
        slot.epoch.load(Ordering::Relaxed) == epoch
            && slot
                .state
                .compare_exchange(ACTIVE, FADING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    /// Whether the source of the slot is fading and the callback has
    /// finished the fade (`FADED`).
    pub fn is_faded(&self, index: usize, epoch: u64) -> bool {
        self.slots.get(index).is_some_and(|slot| {
            slot.epoch.load(Ordering::Relaxed) == epoch
                && slot.state.load(Ordering::Acquire) == FADED
        })
    }

    /// Frees the slot of a source that is fading or faded (or any source,
    /// when no callback runs): takes its data out and drops it here, on the
    /// owner's thread. False when the slot is not that attachment.
    pub fn free(&self, index: usize, epoch: u64) -> bool {
        let Some(slot) = self.slots.get(index) else {
            return false;
        };
        if slot.epoch.load(Ordering::Relaxed) != epoch || slot.state.load(Ordering::Acquire) == FREE
        {
            return false;
        }
        // Not mixed any more (a callback that already took the lock does
        // its work before the owner gets it).
        slot.state.store(FADED, Ordering::Release);
        let data = lock_owner(slot).take();
        slot.state.store(FREE, Ordering::Release);
        drop(data);
        true
    }

    /// Frees every slot. Only when no callback can run (the device is gone).
    pub fn clear(&self) {
        for (index, slot) in self.slots.iter().enumerate() {
            let epoch = slot.epoch.load(Ordering::Relaxed);
            self.free(index, epoch);
        }
    }
}

/// Adds `src` (interleaved stereo frames; `pre` frames of silence come
/// first) to `dst` with the gain `gain` moving toward `target` by
/// 1 / [`RAMP_FRAMES`] per frame. `dst` holds `pre + src.len() / 2` frames.
fn ramp_add(dst: &mut [f32], src: &[f32], pre: usize, gain: &mut f32, target: f32) {
    let step = 1.0 / RAMP_FRAMES as f32;
    for _ in 0..pre {
        *gain = approach(*gain, target, step);
    }
    let body = dst
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .skip(pre)
        .zip(src.as_chunks::<2>().0);
    if (*gain - target).abs() < f32::EPSILON {
        *gain = target;
        for (d, s) in body {
            d[0] += s[0] * target;
            d[1] += s[1] * target;
        }
        return;
    }
    for (d, s) in body {
        *gain = approach(*gain, target, step);
        d[0] += s[0] * *gain;
        d[1] += s[1] * *gain;
    }
}

/// `gain` one step closer to `target`.
fn approach(gain: f32, target: f32, step: f32) -> f32 {
    if gain < target {
        (gain + step).min(target)
    } else {
        (gain - step).max(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::ManualClock;

    fn mix() -> Mix {
        Mix::new(Arc::new(ManualClock::default()))
    }

    /// A ring that holds `frames` frames of `value` on both channels.
    fn ring_of(value: f32, frames: usize) -> Arc<SampleRing> {
        let ring = Arc::new(SampleRing::new(frames.max(1)));
        ring.push(&vec![value; frames * 2]);
        ring
    }

    /// A mixer cycle of `frames` frames.
    fn cycle(mix: &Mix, frames: usize) -> Vec<f32> {
        let mut out = vec![9.0; frames * 2];
        mix.fill(&mut out, 0);
        out
    }

    /// Fades a source in and out of the way: attaches it, runs the ramp-up.
    fn warmed(mix: &Mix, ring: &Arc<SampleRing>) -> (usize, u64) {
        let slot = mix.attach(Arc::clone(ring), 0).expect("a free slot");
        cycle(mix, RAMP_FRAMES as usize);
        slot
    }

    #[test]
    fn sources_sum_and_clamp() {
        let mix = mix();
        let a = ring_of(0.25, 4096);
        let b = ring_of(0.5, 4096);
        warmed(&mix, &a);
        warmed(&mix, &b);
        let out = cycle(&mix, 100);
        assert!(
            out.iter().all(|&s| (s - 0.75).abs() < 1e-6),
            "{:?}",
            &out[..4]
        );
        let loud = ring_of(0.9, 4096);
        warmed(&mix, &loud);
        let out = cycle(&mix, 100);
        assert!(
            out.iter().all(|&s| (s - 1.0).abs() < 1e-6),
            "clamped: {:?}",
            &out[..4]
        );
        let neg = ring_of(-0.9, 4096);
        let neg2 = ring_of(-0.9, 4096);
        warmed(&mix, &neg);
        warmed(&mix, &neg2);
        // 0.25 + 0.5 + 0.9 - 0.9 - 0.9 = -0.15
        let out = cycle(&mix, 10);
        assert!((out[0] + 0.15).abs() < 1e-5, "{}", out[0]);
    }

    #[test]
    fn non_finite_samples_play_as_silence_or_full_scale() {
        let mix = mix();
        let ring = Arc::new(SampleRing::new(2048));
        // After the ramp (gain 1), so the non-finite samples are not
        // multiplied by a gain of 0 (which would hide them for +-inf only).
        ring.push(&vec![0.0; RAMP_FRAMES as usize * 2]);
        mix.attach(Arc::clone(&ring), 0).expect("slot");
        cycle(&mix, RAMP_FRAMES as usize);
        ring.push(&[
            f32::NAN,
            f32::NAN,
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            0.5,
            0.5,
        ]);
        let out = cycle(&mix, 4);
        let want = [0.0, 0.0, 1.0, 1.0, -1.0, -1.0, 0.5, 0.5];
        assert!(
            out.iter().zip(want).all(|(s, w)| (s - w).abs() < 1e-3),
            "NaN plays as 0 and infinity as full scale: {out:?}"
        );
    }

    /// A callback that stalled between its first look at a slot and its lock
    /// (`seen`: FADING) while the owner freed the slot and attached another
    /// source to it must treat the slot as the new source's: not fade it.
    #[test]
    fn a_stalled_callback_does_not_fade_the_source_that_took_the_slot() {
        let mix = mix();
        let old = ring_of(0.5, 4096);
        let (index, epoch) = warmed(&mix, &old);
        assert!(mix.begin_fade(index, epoch));
        assert!(mix.free(index, epoch));
        let new = ring_of(0.5, 4096);
        let (new_index, new_epoch) = mix.attach(Arc::clone(&new), 0).expect("slot");
        assert_eq!(new_index, index, "the same slot");
        let slot = &mix.slots[index];
        let mut out = vec![0.0f32; 2 * RAMP_FRAMES as usize];
        let mut scratch = [0.0f32; CHUNK * 2];
        // The stale look: FADING (what the callback saw before the stall).
        mix.mix_slot_seen(slot, FADING, &mut out, RAMP_FRAMES as usize, &mut scratch);
        assert_eq!(slot.state.load(Ordering::Acquire), ACTIVE, "not faded");
        assert!(out[2 * (RAMP_FRAMES as usize - 1)] > 0.49, "{}", out[958]);
        assert!(mix.begin_fade(new_index, new_epoch), "it can still fade");
    }

    #[test]
    fn the_gain_ramps_in_ten_milliseconds_and_follows_changes() {
        let mix = mix();
        let ring = ring_of(1.0, 8192);
        mix.attach(Arc::clone(&ring), 0).expect("slot");
        let out = cycle(&mix, 960);
        // 0 to 1 in 480 frames, linear.
        assert!(out[0] <= 1.0 / RAMP_FRAMES as f32 + 1e-6);
        assert!(
            (out[2 * 239] - 240.0 / 480.0).abs() < 1e-3,
            "{}",
            out[2 * 239]
        );
        assert!((out[2 * 479] - 1.0).abs() < 1e-6);
        assert!(out[2 * 500..].iter().all(|&s| (s - 1.0).abs() < 1e-6));
        // volume 0.5: down in 240 frames.
        ring.set_gain(0.5);
        let out = cycle(&mix, 480);
        assert!((out[0] - (1.0 - 1.0 / 480.0)).abs() < 1e-4);
        assert!((out[2 * 239] - 0.5).abs() < 1e-3);
        assert!(out[2 * 240..].iter().all(|&s| (s - 0.5).abs() < 1e-6));
        // muted: to 0, at the same slope.
        ring.set_gain(0.0);
        let out = cycle(&mix, 480);
        assert!((out[2 * 119] - 0.25).abs() < 1e-3);
        assert!(out[2 * 240..].iter().all(|&s| s.abs() < 1e-6));
        // A muted source is still mixed: its frames count.
        let taken = mix.reading(0, 1).expect("reading").taken;
        assert_eq!(taken, 960 + 480 + 480);
    }

    #[test]
    fn frames_are_counted_for_each_source() {
        let mix = mix();
        let a = ring_of(0.1, 5000);
        let b = ring_of(0.1, 300);
        let (ia, ea) = mix.attach(Arc::clone(&a), 0).expect("slot");
        let (ib, eb) = mix.attach(Arc::clone(&b), 0).expect("slot");
        cycle(&mix, 512);
        let ra = mix.reading(ia, ea).expect("a");
        let rb = mix.reading(ib, eb).expect("b");
        assert_eq!((ra.taken, ra.starved), (512, false));
        // `b` ran dry: it counts the frames of its ring only.
        assert_eq!((rb.taken, rb.starved), (300, true));
        assert_eq!(rb.cycle_frames, 512);
        assert_eq!(ra.cycles, 1);
        // A stale attachment number reads nothing.
        assert_eq!(mix.reading(ia, ea + 100), None);
        assert_eq!(mix.reading(99, 1), None);
    }

    #[test]
    fn silence_before_the_first_frame_counts_and_a_dry_ring_does_not() {
        let mix = mix();
        let ring = ring_of(1.0, 4096);
        let (slot, epoch) = mix.attach(Arc::clone(&ring), 200).expect("slot");
        let out = cycle(&mix, 300);
        // 200 frames of silence (the ramp runs through them), then the ring.
        assert!(out[..400].iter().all(|&s| s == 0.0));
        assert!(out[400] > 0.0);
        assert_eq!(mix.reading(slot, epoch).expect("reading").taken, 300);
        assert_eq!(ring.read_position(), 100);
        // The ring is empty: silence and no count.
        let dry = Arc::new(SampleRing::new(16));
        let (dry_slot, dry_epoch) = mix.attach(Arc::clone(&dry), 0).expect("slot");
        cycle(&mix, 64);
        let reading = mix.reading(dry_slot, dry_epoch).expect("reading");
        assert_eq!((reading.taken, reading.starved), (0, true));
    }

    #[test]
    fn thirty_two_sources_fit_and_the_33rd_does_not() {
        let mix = mix();
        let rings: Vec<_> = (0..MAX_SOURCES).map(|_| ring_of(0.01, 1024)).collect();
        let slots: Vec<_> = rings
            .iter()
            .map(|r| mix.attach(Arc::clone(r), 0).expect("a slot"))
            .collect();
        assert_eq!(mix.in_use(), MAX_SOURCES);
        assert!(mix.attach(ring_of(0.01, 16), 0).is_none());
        // A freed slot is used again, with a new attachment number.
        let (i, e) = slots[5];
        assert!(mix.begin_fade(i, e));
        cycle(&mix, 960);
        assert!(mix.is_faded(i, e));
        assert!(mix.free(i, e));
        let (j, f) = mix.attach(ring_of(0.01, 16), 0).expect("the freed slot");
        assert_eq!(j, i);
        assert_ne!(f, e);
        // The old handle reads nothing and frees nothing.
        assert_eq!(mix.reading(i, e), None);
        assert!(!mix.free(i, e));
    }

    #[test]
    fn a_fade_ends_in_silence_and_the_ring_is_freed_by_the_owner() {
        let mix = mix();
        let ring = ring_of(1.0, 8192);
        let (i, e) = warmed(&mix, &ring);
        assert_eq!(Arc::strong_count(&ring), 2);
        assert!(mix.begin_fade(i, e));
        // Not twice.
        assert!(!mix.begin_fade(i, e));
        let out = cycle(&mix, 600);
        assert!((out[0] - (1.0 - 1.0 / 480.0)).abs() < 1e-4);
        assert!(out[2 * 480..].iter().all(|&s| s == 0.0));
        assert!(mix.is_faded(i, e));
        // The callback still holds its reference: it frees nothing.
        assert_eq!(Arc::strong_count(&ring), 2);
        let read = ring.read_position();
        cycle(&mix, 100);
        assert_eq!(ring.read_position(), read, "a faded source takes nothing");
        assert!(mix.free(i, e));
        assert_eq!(Arc::strong_count(&ring), 1);
        assert_eq!(mix.in_use(), 0);
    }

    #[test]
    fn a_fade_of_an_empty_ring_ends_at_once() {
        let mix = mix();
        let ring = ring_of(1.0, 64);
        let (i, e) = warmed(&mix, &ring);
        cycle(&mix, 64);
        assert!(mix.begin_fade(i, e));
        cycle(&mix, 16);
        assert!(mix.is_faded(i, e));
    }

    /// A thread that runs the callback in a loop while the owner adds and
    /// removes sources: no panic, no lost frame count, no leak.
    #[test]
    fn sources_come_and_go_while_a_thread_runs_the_callback() {
        let mix = Arc::new(mix());
        let stop = Arc::new(AtomicBool::new(false));
        let runner = {
            let (mix, stop) = (Arc::clone(&mix), Arc::clone(&stop));
            std::thread::spawn(move || {
                let mut out = vec![0.0f32; 512 * 2];
                while !stop.load(Ordering::Relaxed) {
                    mix.fill(&mut out, 512);
                    assert!(out.iter().all(|s| s.abs() <= 1.0));
                    std::thread::yield_now();
                }
            })
        };
        let mut rings = Vec::new();
        for round in 0..400 {
            let ring = ring_of(0.1, 4096);
            if let Some((i, e)) = mix.attach(Arc::clone(&ring), (round % 7) as u64) {
                if round % 3 == 0 {
                    std::thread::yield_now();
                }
                mix.begin_fade(i, e);
                let started = std::time::Instant::now();
                while !mix.is_faded(i, e) && started.elapsed().as_millis() < 200 {
                    std::thread::yield_now();
                }
                assert!(mix.free(i, e));
            }
            rings.push(Arc::downgrade(&ring));
            drop(ring);
        }
        stop.store(true, Ordering::Relaxed);
        runner.join().expect("the callback thread ends");
        assert_eq!(mix.in_use(), 0);
        assert!(mix.cycles() > 0);
        // Every ring was freed, none by the callback's thread (it never
        // held the last reference: `free` ran on this thread).
        assert!(rings.iter().all(|w| w.upgrade().is_none()));
    }

    #[test]
    fn a_held_slot_lock_skips_the_slot_and_never_blocks() {
        let mix = mix();
        let ring = ring_of(0.5, 4096);
        let (i, e) = warmed(&mix, &ring);
        let before = mix.reading(i, e).expect("reading").taken;
        let guard = mix.slots[i].data.lock().expect("lock");
        let out = cycle(&mix, 128);
        drop(guard);
        assert!(out.iter().all(|&s| s == 0.0));
        assert_eq!(mix.skipped(), 1);
        assert_eq!(mix.reading(i, e).expect("reading").taken, before);
    }

    #[test]
    fn clear_frees_everything() {
        let mix = mix();
        let rings: Vec<_> = (0..4).map(|_| ring_of(0.1, 64)).collect();
        for r in &rings {
            mix.attach(Arc::clone(r), 0).expect("slot");
        }
        mix.clear();
        assert_eq!(mix.in_use(), 0);
        assert!(rings.iter().all(|r| Arc::strong_count(r) == 1));
    }
}
