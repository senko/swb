//! The animation records of a document (ADR 0044 sections 1, 2 and 8; M13
//! feature 3b): one [`DocumentAnimations`] per document holds the document
//! timeline's time, the CSS animations in a table by [`AnimationId`] and an
//! index from each owning element (and pseudo-element) to its animations in
//! composite order.
//!
//! The cascade creates, updates and cancels the records of an element from
//! its computed `animation-name` list when it styles the element
//! ([`DocumentAnimations::sync`]); the engine advances the timeline at the
//! rendering update ([`DocumentAnimations::update`]), which marks the
//! owners whose animated values can have changed, and asks when the
//! records need the next update ([`DocumentAnimations::next_time`]).
//!
//! The play states follow Web Animations 1 §4.4
//! (<https://drafts.csswg.org/web-animations-1/#animations>) with a playback
//! rate of 1: an animation has a start time or a hold time; `play()` and
//! `pause()` are pending until the next update, which resolves them at the
//! timeline's time (the ready time). A pending pause takes its hold time at
//! once (scripts read the same current time until the update anyway).
//!
//! CSS Animations 1 §3 (<https://drafts.csswg.org/css-animations-1/#animations>):
//! an animation lives while its name is in the element's `animation-name`;
//! a name that no `@keyframes` rule defines makes no animation; an element
//! (or an ancestor) with `display: none` has none. A name that stays keeps
//! its record (and its identity for scripts) when the other `animation-*`
//! values change; a name that appears twice makes two animations, matched by
//! their order among the entries of that name.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use swb_dom::NodeId;

use super::easing::Easing;
use super::events::{
    AnimationEventKind, PendingAnimationEvent, PhaseChange, cancel_elapsed, event_time, events_of,
    scheduled_time,
};
use super::keyframes::KeyframeSet;
use super::properties::{AnimationEntry, AnimationStyle, PlayState};
use super::timing::{ComputedTiming, Phase, Timing};
use super::visibility::{Reach, reach_of};
use crate::ComputedStyle;
use crate::properties::LonghandId;
use crate::style_map::PseudoKind;
use crate::values::Display;

/// The id of an animation record: grows per document, never reused.
pub type AnimationId = u64;

/// The most live animation records of one document (ADR 0044 section 11):
/// beyond, no record is created (one warning per document).
pub const MAX_RECORDS: usize = 100_000;

/// The length of a frame in ms (60 Hz): the scheduling treats steps shorter
/// than this as continuous.
const FRAME_MS: f64 = 1000.0 / 60.0;

/// `time` rounded to 1e-9 ms: removes the float noise of a difference of
/// two times, keeps every time that a script can write.
pub fn round_noise(time: f64) -> f64 {
    let rounded = (time * 1e9).round() / 1e9;
    if rounded.is_finite() { rounded } else { time }
}

/// The element (or pseudo-element) that owns an animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimationOwner {
    /// The element (the originating element of a pseudo-element).
    pub node: NodeId,
    /// The pseudo-element (`::before`, `::after`, `::marker`), if any.
    pub pseudo: Option<PseudoKind>,
}

/// The play state of an animation
/// (<https://drafts.csswg.org/web-animations-1/#play-states>).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationPlayState {
    /// No current time.
    Idle,
    /// Playing.
    Running,
    /// Paused (or a pause is pending).
    Paused,
    /// The current time is at or past the end of the effect.
    Finished,
}

impl AnimationPlayState {
    /// The IDL string.
    pub fn as_str(self) -> &'static str {
        match self {
            AnimationPlayState::Idle => "idle",
            AnimationPlayState::Running => "running",
            AnimationPlayState::Paused => "paused",
            AnimationPlayState::Finished => "finished",
        }
    }
}

/// A pending play or pause task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Play,
    Pause,
}

/// One CSS animation.
#[derive(Clone, Debug)]
pub struct AnimationRecord {
    /// The id.
    pub id: AnimationId,
    /// The owning element.
    pub owner: AnimationOwner,
    /// The animation name (`animationName`).
    pub name: Arc<str>,
    /// The position of the name in `animation-name`.
    pub index: usize,
    /// The number of entries with the same name before this one.
    occurrence: usize,
    /// The timing of the effect.
    pub timing: Timing,
    /// The `animation-timing-function` of the entry: the default keyframe
    /// easing.
    pub keyframe_easing: Easing,
    start_time: Option<f64>,
    hold_time: Option<f64>,
    pending: Option<Pending>,
    /// `animation-play-state: paused` at the last sync.
    css_paused: bool,
    /// True after `play()` or `pause()` by a script: `animation-play-state`
    /// no longer plays or pauses the animation (CSS Animations 2 §2).
    script_override: bool,
    /// True after `cancel()` by a script: the animation stays idle while its
    /// name stays in `animation-name` (CSS Animations 2 §2), whatever the
    /// other `animation-*` values do, until a script plays it.
    cancelled: bool,
    /// The phase at the last update (for the events).
    pub last_phase: Option<Phase>,
    /// The current iteration at the last update (for the events).
    pub last_iteration: Option<f64>,
    /// The current time at the last update.
    last_time: Option<f64>,
    /// The length of one step in ms when every keyframe interval eases with
    /// `steps()` over the whole iteration (`None`: continuous).
    step_period: Option<f64>,
    /// The keyframe set and easing that `step_period` was computed for.
    step_key: (usize, Easing, f64),
    /// The painted area that the animation can change (the visibility
    /// test).
    reach: Reach,
    /// True if the keyframes animate `transform`, `translate`, `rotate`,
    /// `scale` or `opacity`: while the animation runs, paint composites the
    /// owner's group (ADR 0044 section 19).
    composited: bool,
    /// The play state and the pending flag at the last update (for
    /// [`DocumentAnimations::state_epoch`]).
    observed: Option<(AnimationPlayState, bool)>,
}

impl AnimationRecord {
    fn new(
        id: AnimationId,
        owner: AnimationOwner,
        name: Arc<str>,
        index: usize,
        occurrence: usize,
    ) -> Self {
        AnimationRecord {
            id,
            owner,
            name,
            index,
            occurrence,
            timing: Timing::default(),
            keyframe_easing: Easing::Ease,
            start_time: None,
            hold_time: None,
            pending: None,
            css_paused: false,
            script_override: false,
            cancelled: false,
            last_phase: None,
            last_iteration: None,
            last_time: None,
            step_period: None,
            step_key: (0, Easing::Linear, f64::NAN),
            reach: Reach::default(),
            composited: false,
            observed: None,
        }
    }

    /// The current time at the timeline time `timeline`; `None` when idle.
    /// A running animation's time is rounded to 1e-9 ms
    /// ([`round_noise`]): `timeline - start` has float noise, and an update
    /// at the scheduled end of an iteration must see the boundary reached
    /// (349.795982 − 149.795982 is 199.99999999999997).
    /// <https://drafts.csswg.org/web-animations-1/#the-current-time-of-an-animation>
    pub fn current_time(&self, timeline: f64) -> Option<f64> {
        self.hold_time
            .or_else(|| self.start_time.map(|start| round_noise(timeline - start)))
    }

    /// True if the current time is a hold time (paused, or not started).
    pub fn is_held(&self) -> bool {
        self.hold_time.is_some()
    }

    /// The start time, if resolved.
    pub fn start_time(&self) -> Option<f64> {
        self.start_time
    }

    /// True if a play or pause is pending (`pending`).
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The play state at the timeline time `timeline`.
    pub fn play_state(&self, timeline: f64) -> AnimationPlayState {
        let current = self.current_time(timeline);
        if current.is_none() && self.pending.is_none() {
            return AnimationPlayState::Idle;
        }
        if self.pending == Some(Pending::Pause)
            || (self.start_time.is_none() && self.pending != Some(Pending::Play))
        {
            return AnimationPlayState::Paused;
        }
        if current.is_some_and(|c| c >= self.timing.end_time()) {
            return AnimationPlayState::Finished;
        }
        AnimationPlayState::Running
    }

    /// The timing of the effect at the timeline time `timeline`.
    pub fn computed_timing(&self, timeline: f64) -> ComputedTiming {
        self.timing.compute(self.current_time(timeline))
    }

    /// True if the animation is relevant (`getAnimations()` lists it):
    /// its effect is in the before or the active phase, or in effect (a
    /// fill). Measured in Chromium 148 (`fill-modes`): a paused animation
    /// past its end without a `forwards` fill is not listed.
    /// <https://drafts.csswg.org/web-animations-1/#relevant-animations>
    pub fn is_relevant(&self, timeline: f64) -> bool {
        if self.play_state(timeline) == AnimationPlayState::Idle {
            return false;
        }
        let timing = self.computed_timing(timeline);
        matches!(timing.phase, Phase::Before | Phase::Active) || timing.active_time.is_some()
    }

    /// Plays: a paused animation resumes from its current time, an idle or
    /// finished one starts again at 0 (Web Animations "play an animation"
    /// with auto-rewind).
    fn play(&mut self, timeline: f64) {
        self.cancelled = false;
        let current = self.current_time(timeline);
        let end = self.timing.end_time();
        if current.is_none_or(|c| c < 0.0 || c >= end) {
            self.hold_time = Some(0.0);
            self.start_time = None;
        } else if self.start_time.is_some() && self.pending != Some(Pending::Pause) {
            // Running already.
            self.pending = None;
            return;
        } else {
            self.hold_time = current;
            self.start_time = None;
        }
        self.pending = Some(Pending::Play);
    }

    /// Pauses at the current time (0 if idle).
    fn pause(&mut self, timeline: f64) {
        if self.pending == Some(Pending::Pause)
            || (self.start_time.is_none() && self.hold_time.is_some() && self.pending.is_none())
        {
            return;
        }
        // The hold time loses the float noise of `timeline - start_time`
        // (about 1e-12 ms): scripts read a held time unchanged.
        self.hold_time = Some(round_noise(self.current_time(timeline).unwrap_or(0.0)));
        self.start_time = None;
        self.pending = Some(Pending::Pause);
    }

    /// Sets the current time ("set the current time"): a paused
    /// animation holds it, a running one moves its start time. A pending
    /// pause completes. `None` is ignored (a current time cannot be
    /// unresolved by a script while the timeline is active).
    fn set_current_time(&mut self, time: f64, timeline: f64) {
        if self.hold_time.is_some() || self.start_time.is_none() {
            self.hold_time = Some(time);
        } else {
            self.start_time = Some(timeline - time);
        }
        if self.pending == Some(Pending::Pause) {
            self.start_time = None;
            self.pending = None;
        }
    }

    /// Cancels ("cancel an animation"): no current time, no pending task.
    /// Returns the `animationcancel` event's elapsed time (ms) when the
    /// animation was in its before or active phase at the last update
    /// (CSS Animations 2 §5.1: idle from a phase other than idle or after).
    fn cancel(&mut self, timeline: f64) -> Option<f64> {
        let elapsed = matches!(self.last_phase, Some(Phase::Before | Phase::Active))
            .then(|| cancel_elapsed(&self.timing, self.current_time(timeline)));
        self.hold_time = None;
        self.start_time = None;
        self.pending = None;
        self.last_phase = Some(Phase::Idle);
        self.last_iteration = None;
        self.last_time = None;
        elapsed
    }

    /// Finishes ("finish an animation" with a playback rate of 1): the
    /// current time jumps to the end, the animation runs (a pending play or
    /// pause is done). False, with no change, when the end is infinite
    /// (`InvalidStateError`).
    fn finish(&mut self, timeline: f64) -> bool {
        let end = self.timing.end_time();
        if !end.is_finite() {
            return false;
        }
        self.cancelled = false;
        self.hold_time = None;
        self.start_time = Some(timeline - end);
        self.pending = None;
        true
    }

    /// Sets the start time ("set the start time of an animation"): a
    /// resolved start time runs the animation from it, an unresolved one
    /// holds the current time. A pending play or pause is done.
    fn set_start_time(&mut self, start: Option<f64>, timeline: f64) {
        let previous = self.current_time(timeline);
        self.start_time = start;
        self.hold_time = if start.is_some() { None } else { previous };
        self.pending = None;
        if start.is_some() {
            self.cancelled = false;
        }
    }

    /// Resolves a pending play or pause at the ready time `timeline`.
    fn resolve_pending(&mut self, timeline: f64) {
        match self.pending.take() {
            Some(Pending::Play) => {
                if let Some(hold) = self.hold_time.take() {
                    self.start_time = Some(timeline - hold);
                }
            }
            Some(Pending::Pause) | None => {}
        }
    }

    /// The next timeline time at which the output or the events of the
    /// animation can change (ADR 0044 section 8), at the timeline time
    /// `timeline`: with `frames`, the next frame while a continuous easing
    /// runs and the next step boundary for `steps()`; without (headless:
    /// nothing paints between steps), only the event times (the end of the
    /// delay, of each iteration and of the active phase). `None` when
    /// idle, paused or finished.
    fn next_time(&self, timeline: f64, frames: bool) -> Option<f64> {
        if self.pending.is_some() {
            // The ready time: the next update.
            return Some(timeline);
        }
        let start = self.start_time?;
        if self.hold_time.is_some() {
            return None;
        }
        let current = round_noise(timeline - start);
        let timing = &self.timing;
        let delay = timing.delay;
        let active_end = delay + timing.active_duration();
        if current < delay {
            return Some(start + delay);
        }
        if current >= active_end || timing.duration <= 0.0 {
            return None;
        }
        let iteration = ((current - delay) / timing.duration).floor();
        let boundary = (delay + (iteration + 1.0) * timing.duration).min(active_end);
        let event = start + boundary;
        if event <= timeline {
            // Float precision: the time is too large for the boundary to be
            // after it (a script set a huge `currentTime`). Nothing
            // distinguishes the next time from now; a painted frame still
            // shows the animation, a headless run has nothing to wait for.
            return frames.then_some(timeline + FRAME_MS);
        }
        if !frames {
            return Some(event);
        }
        let frame = match self.step_period {
            Some(period) if period >= FRAME_MS => {
                let k = ((current - delay) / period + 1e-9).floor() + 1.0;
                start + (delay + k * period).min(active_end)
            }
            _ => timeline + FRAME_MS,
        };
        Some(frame.min(event))
    }

    /// The length of a step in ms when the easing of every keyframe of
    /// `set` is `steps()` and the keyframes are only at 0% and 100%; `None`
    /// otherwise (continuous for the scheduling).
    fn step_period_of(set: &KeyframeSet, easing: &Easing, duration: f64) -> Option<f64> {
        let mut steps = None;
        for list in &set.properties {
            for keyframe in &list.keyframes {
                if keyframe.offset != 0.0 && keyframe.offset != 1.0 {
                    return None;
                }
                let e = keyframe.easing.as_ref().unwrap_or(easing);
                let Easing::Steps { count: n, .. } = e else {
                    if keyframe.offset == 1.0 {
                        // The easing of the last keyframe applies to no
                        // interval.
                        continue;
                    }
                    return None;
                };
                match steps {
                    Some(s) if s != *n => return None,
                    _ => steps = Some(*n),
                }
            }
        }
        let n = match steps {
            Some(n) => n,
            None => match easing {
                Easing::Steps { count, .. } => *count,
                _ => return None,
            },
        };
        (n > 0).then(|| duration / f64::from(n))
    }
}

/// The `animationcancel` event of `record` (elapsed time in ms), scheduled
/// at the timeline time `time`.
fn cancel_event(record: &AnimationRecord, elapsed: f64, time: f64) -> PendingAnimationEvent {
    PendingAnimationEvent {
        kind: AnimationEventKind::Cancel,
        id: record.id,
        owner: record.owner,
        name: Arc::clone(&record.name),
        index: record.index,
        elapsed: elapsed / 1000.0,
        scheduled: Some(event_time(time)),
    }
}

/// The result of the visibility test (see
/// [`DocumentAnimations::set_visible`]).
#[derive(Debug, PartialEq)]
struct Visibility {
    checked: HashSet<NodeId>,
    visible: HashSet<NodeId>,
}

impl Visibility {
    /// True if the animations of `node` can change visible pixels.
    fn shows(&self, node: NodeId) -> bool {
        !self.checked.contains(&node) || self.visible.contains(&node)
    }
}

/// A cached minimum of next times.
#[derive(Clone, Copy, Debug, Default)]
enum Cached {
    /// To compute.
    #[default]
    Stale,
    /// The minimum over the records (`None`: no record needs an update).
    Known(Option<f64>),
}

/// The animations of one document.
#[derive(Debug, Default)]
pub struct DocumentAnimations {
    /// The current time of the document timeline in ms from the time
    /// origin.
    time: f64,
    /// The page time of the document's time origin.
    origin: f64,
    /// The page time of the last update (`None` before the first).
    last_update: Option<f64>,
    next_id: AnimationId,
    records: BTreeMap<AnimationId, AnimationRecord>,
    /// The records of each owner, in composite order (by position in
    /// `animation-name`).
    owners: HashMap<AnimationOwner, Vec<AnimationId>>,
    /// The owners whose animated values can have changed since their last
    /// style computation.
    marked: HashSet<NodeId>,
    /// The ids of the records that went, for the bindings' wrapper map;
    /// kept only while `track_released` is set.
    released: Vec<AnimationId>,
    /// True once the bindings made a wrapper: only then do they need the
    /// ids of the records that went (a page without scripts, or whose
    /// scripts never list animations, would grow the list without bound).
    track_released: bool,
    /// The cached minimum of the records' next times (timeline times,
    /// without the origin), for `frames` false and true; stale: to compute.
    next_cache: Cell<[Cached; 2]>,
    /// Counts the changes of the records' play state or pending flag, and
    /// of their existence (see [`DocumentAnimations::state_epoch`]).
    state_epoch: Cell<u64>,
    /// The page time that the next read of the styles catches the timeline
    /// up to (set by the page, applied by [`DocumentAnimations::settle`]).
    catch_up_to: Option<f64>,
    /// The latest grid time a task noted (see
    /// [`DocumentAnimations::note_grid_time`]).
    grid_time: Option<f64>,
    /// The pending animation event queue (ADR 0044 section 6): the events
    /// that the next rendering update dispatches.
    events: Vec<PendingAnimationEvent>,
    /// True when a record went while the bindings track them: the next
    /// rendering update settles the promises of its wrapper.
    wake: bool,
    /// The visibility test of section 8: the owners that the engine checked
    /// and those of them whose animations can change visible pixels; `None`
    /// until the engine first told. An owner that was not checked (its
    /// animation started to run since) counts as visible.
    visible: Option<Visibility>,
    /// True once the record limit was reported.
    limit_warned: bool,
    /// The number of updates (for tests of the scheduling).
    updates: u64,
}

impl DocumentAnimations {
    /// The animations of a document whose time origin is the page time
    /// `origin`.
    pub fn new(origin: f64) -> Self {
        DocumentAnimations {
            origin,
            ..DocumentAnimations::default()
        }
    }

    /// True if the document has no animation.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The number of live animations.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// The current time of the document timeline (ms from the time origin).
    pub fn time(&self) -> f64 {
        self.time
    }

    /// The current time of the document timeline for scripts
    /// (`document.timeline.currentTime`): the time of the last update, or
    /// the requested catch-up time if later (a document without animations
    /// runs no update).
    pub fn current_timeline_time(&self) -> f64 {
        [self.catch_up_to, self.grid_time]
            .into_iter()
            .flatten()
            .fold(self.time, |time, to| time.max(to - self.origin))
    }

    /// Notes the page time of the frame grid at or before now, for
    /// [`DocumentAnimations::current_timeline_time`] of a document without
    /// animations (no catch-up runs then).
    pub fn note_grid_time(&mut self, page_time: f64) {
        self.grid_time = Some(self.grid_time.map_or(page_time, |t| t.max(page_time)));
    }

    /// Sets the timeline to the page time `page_time` without an update (a
    /// document without animations: nothing else changes).
    pub fn advance(&mut self, page_time: f64) {
        self.time = self.time.max(page_time - self.origin);
    }

    /// The page time of the time origin.
    pub fn origin(&self) -> f64 {
        self.origin
    }

    /// The page time of the last update, if any.
    pub fn last_update(&self) -> Option<f64> {
        self.last_update
    }

    /// The number of updates so far.
    pub fn updates(&self) -> u64 {
        self.updates
    }

    /// The record `id`.
    pub fn record(&self, id: AnimationId) -> Option<&AnimationRecord> {
        self.records.get(&id)
    }

    /// The records of `owner` in composite order.
    pub fn ids_of(&self, owner: AnimationOwner) -> &[AnimationId] {
        self.owners.get(&owner).map_or(&[], Vec::as_slice)
    }

    /// The owners that have animations, in no order.
    pub fn owners(&self) -> impl Iterator<Item = AnimationOwner> + '_ {
        self.owners.keys().copied()
    }

    /// The update of the rendering update (ADR 0044 section 2): sets the
    /// timeline's time to the page time `page_time` minus the time origin,
    /// resolves the pending plays and pauses, and marks the owners whose
    /// output can have changed. Returns true if one was marked.
    pub fn update(&mut self, page_time: f64) -> bool {
        self.last_update = Some(page_time);
        self.updates += 1;
        let time = (page_time - self.origin).max(self.time);
        self.time = time;
        let mut marked = false;
        let mut next = [None::<f64>; 2];
        for record in self.records.values_mut() {
            record.resolve_pending(time);
            let state = (record.play_state(time), record.pending());
            if record.observed != Some(state) {
                record.observed = Some(state);
                self.state_epoch.set(self.state_epoch.get() + 1);
            }
            let current = record.current_time(time);
            let timing = record.timing.compute(current);
            let phase = Some(timing.phase);
            let change = PhaseChange {
                before: (
                    record.last_phase.unwrap_or(Phase::Idle),
                    record.last_iteration,
                ),
                after: (timing.phase, timing.current_iteration),
            };
            // The timeline time of local time 0, to place the events: the
            // start time of a running animation as it is (`time - current`
            // would carry the rounding of the current time, and the start
            // and end of a finished animation could swap), else the time
            // minus the hold time.
            let zero = match (record.hold_time, record.start_time) {
                (Some(hold), _) => time - hold,
                (None, Some(start)) => start,
                (None, None) => time,
            };
            for (kind, elapsed) in events_of(&record.timing, change) {
                self.events.push(PendingAnimationEvent {
                    kind,
                    id: record.id,
                    owner: record.owner,
                    name: Arc::clone(&record.name),
                    index: record.index,
                    elapsed: elapsed / 1000.0,
                    scheduled: Some(scheduled_time(&record.timing, zero, elapsed)),
                });
            }
            let changed = phase != record.last_phase
                || timing.current_iteration != record.last_iteration
                || (timing.phase == Phase::Active && current != record.last_time);
            record.last_phase = phase;
            record.last_iteration = timing.current_iteration;
            record.last_time = current;
            if changed {
                self.marked.insert(record.owner.node);
                marked = true;
            }
            let shown = self
                .visible
                .as_ref()
                .is_none_or(|v| v.shows(record.owner.node));
            for (slot, frames) in next.iter_mut().zip([false, true]) {
                if let Some(t) = record.next_time(time, frames && shown) {
                    *slot = Some(slot.map_or(t, |m| m.min(t)));
                }
            }
        }
        self.next_cache
            .set([Cached::Known(next[0]), Cached::Known(next[1])]);
        marked
    }

    /// Asks that the next [`DocumentAnimations::settle`] advances the
    /// timeline to the page time `page_time` (a time on the frame grid at
    /// or before now). Costs nothing until something reads the styles.
    pub fn request_catch_up(&mut self, page_time: f64) {
        self.catch_up_to = Some(self.catch_up_to.map_or(page_time, |t| t.max(page_time)));
    }

    /// True if a requested catch-up has not run (and there is an animation
    /// that it could change).
    pub fn unsettled(&self) -> bool {
        !self.records.is_empty()
            && self
                .catch_up_to
                .is_some_and(|to| self.last_update.is_none_or(|last| last < to))
    }

    /// Runs the requested catch-up: advances the timeline to the requested
    /// page time if the last update is older. Call it before something
    /// reads the styles or the state of the animations.
    pub fn settle(&mut self) {
        if self.unsettled()
            && let Some(to) = self.catch_up_to
        {
            self.update(to);
        }
    }

    /// The next page time at which an animation needs an update (see
    /// [`AnimationRecord::next_time`]); `None` if none does. The minimum
    /// over the records is cached until a record or the time changes, so
    /// that a scheduler that asks at every task pays once.
    pub fn next_time(&self, frames: bool) -> Option<f64> {
        let slot = usize::from(frames);
        let mut cache = self.next_cache.get();
        if !self.events.is_empty() || self.wake {
            // The events wait for the next rendering update.
            return Some(self.time + self.origin);
        }
        let next = if let Some(Cached::Known(next)) = cache.get(slot).copied() {
            next
        } else {
            let next = self
                .records
                .values()
                .filter_map(|r| r.next_time(self.time, frames && self.shown(r.owner.node)))
                .reduce(f64::min);
            if let Some(entry) = cache.get_mut(slot) {
                *entry = Cached::Known(next);
            }
            self.next_cache.set(cache);
            next
        };
        next.map(|t| t + self.origin)
    }

    /// True if the animations of `node` can change visible pixels.
    fn shown(&self, node: NodeId) -> bool {
        self.visible.as_ref().is_none_or(|v| v.shows(node))
    }

    /// The owners of the animations that run (play state `running` at the
    /// current timeline time: also in their delay, not paused, finished or
    /// idle) and animate `transform`, `translate`, `rotate`, `scale` or
    /// `opacity`: paint composites their groups (ADR 0044 section 19). A
    /// running transition will join them (feature 5).
    pub fn composited_owners(&self) -> HashSet<AnimationOwner> {
        self.records
            .values()
            .filter(|r| r.composited && r.play_state(self.time) == AnimationPlayState::Running)
            .map(|r| r.owner)
            .collect()
    }

    /// The owners of the animations that run (or wait for a pending play),
    /// each once with the widest [`Reach`] of its animations: the engine
    /// finds their painted bounds for [`DocumentAnimations::set_visible`].
    pub fn bounds_requests(&self) -> Vec<(NodeId, Reach)> {
        let mut out: HashMap<NodeId, Reach> = HashMap::new();
        for record in self.records.values() {
            let runs = record.pending == Some(Pending::Play)
                || (record.start_time.is_some() && record.hold_time.is_none());
            if runs {
                let reach = out.entry(record.owner.node).or_default();
                *reach = (*reach).max(record.reach);
            }
        }
        let mut list: Vec<(NodeId, Reach)> = out.into_iter().collect();
        list.sort_unstable();
        list
    }

    /// Sets the result of the visibility test (after a display list, a
    /// scroll or a resize): of the `checked` owners (those of
    /// [`DocumentAnimations::bounds_requests`]), the `visible` ones can
    /// change visible pixels; the others need no painted frames, only
    /// their event times. Owners that were not checked count as visible.
    pub fn set_visible(&mut self, checked: HashSet<NodeId>, visible: HashSet<NodeId>) {
        let new = Some(Visibility { checked, visible });
        if self.visible != new {
            self.visible = new;
            self.next_cache.set([Cached::Stale; 2]);
        }
    }

    /// A record changed (or appeared, or went): drops the cached next time
    /// and counts the change in [`DocumentAnimations::state_epoch`].
    fn invalidate(&self) {
        self.next_cache.set([Cached::Stale; 2]);
        self.state_epoch.set(self.state_epoch.get() + 1);
    }

    /// A number that changes when a record's play state or pending flag
    /// changes, or a record appears or goes (by an update, a command of a
    /// script or a style change). The bindings settle the `ready` and
    /// `finished` promises only when it differs from the value of their last
    /// settling, so an update that changed no animation costs nothing for
    /// them (ADR 0044 section 7).
    pub fn state_epoch(&self) -> u64 {
        self.state_epoch.get()
    }

    /// Makes the bindings' wrapper bookkeeping start: from now on the ids
    /// of the records that go are kept for [`DocumentAnimations::take_released`].
    pub fn track_released(&mut self) {
        self.track_released = true;
    }

    /// True if an owner waits for a restyle.
    pub fn has_marked(&self) -> bool {
        !self.marked.is_empty()
    }

    /// True if an owner that waits for a restyle can change visible pixels
    /// (the visibility test): a paint needs the restyle. The marks of
    /// invisible owners wait for the next style flush (a script's read, a
    /// screenshot, the render after a scroll).
    pub fn has_shown_marks(&self) -> bool {
        self.marked.iter().any(|&node| self.shown(node))
    }

    /// Takes the owners that wait for a restyle.
    pub fn take_marked(&mut self) -> Vec<NodeId> {
        let mut nodes: Vec<NodeId> = self.marked.drain().collect();
        nodes.sort_unstable();
        nodes
    }

    /// Takes the ids of the records that went since the last call.
    pub fn take_released(&mut self) -> Vec<AnimationId> {
        std::mem::take(&mut self.released)
    }

    /// Cancels the records of the owners for which `keep` is false (their
    /// element left the document, lost its style or its names).
    pub fn retain_owners(&mut self, mut keep: impl FnMut(AnimationOwner) -> bool) {
        let mut gone: Vec<AnimationOwner> =
            self.owners.keys().copied().filter(|&o| !keep(o)).collect();
        // The order of creation, not of the hash map: the cancel events and
        // the released ids are the same in every run.
        gone.sort_by_key(|o| self.owners.get(o).and_then(|ids| ids.first().copied()));
        for owner in gone {
            if let Some(ids) = self.owners.remove(&owner) {
                for id in ids {
                    self.release(id);
                }
            }
        }
    }

    /// Cancels every animation (a new stylist made the document's
    /// animations off, or the document goes).
    pub fn clear(&mut self) {
        self.retain_owners(|_| false);
    }

    fn release(&mut self, id: AnimationId) {
        let time = self.time;
        if let Some(mut record) = self.records.remove(&id) {
            self.invalidate();
            if let Some(elapsed) = record.cancel(time) {
                self.events.push(cancel_event(&record, elapsed, time));
            }
            if self.track_released {
                self.released.push(id);
                self.wake = true;
            }
        }
    }

    /// Takes the pending animation event queue (in the order the events
    /// were queued) for the step "update animations and send events".
    pub fn take_events(&mut self) -> Vec<PendingAnimationEvent> {
        self.wake = false;
        std::mem::take(&mut self.events)
    }

    /// True if events wait for the next rendering update.
    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// `cancel()` by a script: the animation goes idle and stays so while
    /// its name stays in `animation-name`; an `animationcancel` event is
    /// queued if it was in its before or active phase.
    pub fn cancel(&mut self, id: AnimationId) {
        let time = self.time;
        let Some(record) = self.records.get_mut(&id) else {
            return;
        };
        if record.play_state(time) == AnimationPlayState::Idle {
            return;
        }
        record.script_override = true;
        record.cancelled = true;
        if let Some(elapsed) = record.cancel(time) {
            self.events.push(cancel_event(record, elapsed, time));
        }
        self.marked.insert(record.owner.node);
        self.invalidate();
    }

    /// `finish()` by a script. False, with no change, when the end of the
    /// effect is infinite (the caller throws `InvalidStateError`).
    pub fn finish(&mut self, id: AnimationId) -> bool {
        let time = self.time;
        let Some(record) = self.records.get_mut(&id) else {
            return true;
        };
        if !record.finish(time) {
            return false;
        }
        record.script_override = true;
        self.marked.insert(record.owner.node);
        self.invalidate();
        true
    }

    /// Sets `startTime` (ms, `None` for null) by a script.
    pub fn set_start_time(&mut self, id: AnimationId, start: Option<f64>) {
        let time = self.time;
        if let Some(record) = self.records.get_mut(&id) {
            record.script_override = true;
            record.set_start_time(start, time);
            self.marked.insert(record.owner.node);
            self.invalidate();
        }
    }

    /// `pause()` by a script.
    pub fn pause(&mut self, id: AnimationId) {
        let time = self.time;
        if let Some(record) = self.records.get_mut(&id) {
            record.script_override = true;
            record.pause(time);
            self.marked.insert(record.owner.node);
            self.invalidate();
        }
    }

    /// `play()` by a script.
    pub fn play(&mut self, id: AnimationId) {
        let time = self.time;
        if let Some(record) = self.records.get_mut(&id) {
            record.script_override = true;
            record.play(time);
            self.marked.insert(record.owner.node);
            self.invalidate();
        }
    }

    /// Sets `currentTime` (ms) by a script.
    pub fn set_current_time(&mut self, id: AnimationId, current: f64) {
        let time = self.time;
        if let Some(record) = self.records.get_mut(&id) {
            record.set_current_time(current, time);
            self.marked.insert(record.owner.node);
            self.invalidate();
        }
    }

    /// Brings the records of `owner` up to date with its computed
    /// `animation-*` lists in `style` (CSS Animations 1 §3): one record per
    /// name that `lookup` finds a keyframe set for, in the order of the
    /// names, the existing record of a name kept. `exists` is false for a
    /// pseudo-element without a box (it has no animations). Returns the
    /// records with their keyframe sets, in composite order. An element
    /// with `display: none` has only the animations that animate `display`
    /// (measured in Chromium 148, `interp-lengths-numbers`: `ds`).
    pub(crate) fn sync(
        &mut self,
        owner: AnimationOwner,
        style: &ComputedStyle,
        exists: bool,
        lookup: &mut dyn FnMut(&str) -> Option<Arc<KeyframeSet>>,
    ) -> Vec<(AnimationId, Arc<KeyframeSet>)> {
        let old = self.owners.remove(&owner).unwrap_or_default();
        let mut out: Vec<(AnimationId, Arc<KeyframeSet>)> = Vec::new();
        if exists && AnimationStyle::has_names(style) {
            let hidden = style.display == Display::None;
            let list = AnimationStyle::of(style);
            let mut previous = self.identities(&old);
            let mut seen: HashMap<&str, usize> = HashMap::new();
            for i in 0..list.len() {
                let Some(entry) = list.entry(i) else {
                    continue;
                };
                let Some(name) = entry.name else {
                    continue;
                };
                let occurrence = {
                    let n = seen.entry(name).or_insert(0);
                    *n += 1;
                    *n - 1
                };
                let Some(set) = lookup(name) else {
                    continue;
                };
                if hidden && !set.animates(LonghandId::Display) {
                    continue;
                }
                let existing = previous.remove(&(Arc::from(name), occurrence));
                let id = match existing {
                    Some(id) => id,
                    None => match self.create(owner, name, i, occurrence) {
                        Some(id) => id,
                        None => continue,
                    },
                };
                self.update_record(id, i, &entry, &set, existing.is_none());
                out.push((id, set));
            }
        }
        let kept: HashSet<AnimationId> = out.iter().map(|(id, _)| *id).collect();
        for id in old {
            if !kept.contains(&id) {
                self.release(id);
            }
        }
        if !out.is_empty() {
            self.owners
                .insert(owner, out.iter().map(|(id, _)| *id).collect());
        }
        out
    }

    /// The identity of each record in `ids`: (name, occurrence among the
    /// entries of that name) to id, built once per sync.
    fn identities(&self, ids: &[AnimationId]) -> HashMap<(Arc<str>, usize), AnimationId> {
        ids.iter()
            .filter_map(|id| {
                let r = self.records.get(id)?;
                Some(((Arc::clone(&r.name), r.occurrence), *id))
            })
            .collect()
    }

    /// Applies the entry `i` of the animation lists to the record `id`: the
    /// timing, the easing and the play state (a fresh record starts playing,
    /// or paused; an existing one follows `animation-play-state` unless a
    /// script took over), and the step period of its keyframes.
    fn update_record(
        &mut self,
        id: AnimationId,
        i: usize,
        entry: &AnimationEntry<'_>,
        set: &Arc<KeyframeSet>,
        fresh: bool,
    ) {
        let time = self.time;
        let Some(record) = self.records.get_mut(&id) else {
            return;
        };
        let mut changed = fresh
            || record.timing != entry.timing
            || record.keyframe_easing != *entry.keyframe_easing;
        record.index = i;
        record.timing = entry.timing.clone();
        record.keyframe_easing = entry.keyframe_easing.clone();
        let paused = entry.play_state == PlayState::Paused;
        if record.cancelled {
            // Cancelled by a script: `animation-play-state` does not play
            // it again (CSS Animations 2 §2).
        } else if fresh {
            if paused {
                record.hold_time = Some(0.0);
                record.pending = Some(Pending::Pause);
            } else {
                record.play(time);
            }
        } else if !record.script_override && paused != record.css_paused {
            if paused {
                record.pause(time);
            } else {
                record.play(time);
            }
            self.marked.insert(record.owner.node);
            changed = true;
        }
        record.css_paused = paused;
        let key = (
            Arc::as_ptr(set) as usize,
            record.keyframe_easing.clone(),
            record.timing.duration,
        );
        if key != record.step_key {
            record.step_period = AnimationRecord::step_period_of(
                set,
                &record.keyframe_easing,
                record.timing.duration,
            );
            record.step_key = key;
            record.reach = reach_of(set);
            record.composited = super::visibility::is_composited(set);
            changed = true;
        }
        if changed {
            self.invalidate();
        }
    }

    /// A new record, unless the document has [`MAX_RECORDS`].
    fn create(
        &mut self,
        owner: AnimationOwner,
        name: &str,
        index: usize,
        occurrence: usize,
    ) -> Option<AnimationId> {
        if self.records.len() >= MAX_RECORDS {
            if !self.limit_warned {
                log::warn!(
                    "animations: the document has {MAX_RECORDS} animations; no more are created"
                );
                self.limit_warned = true;
            }
            return None;
        }
        self.invalidate();
        self.next_id += 1;
        let id = self.next_id;
        self.records.insert(
            id,
            AnimationRecord::new(id, owner, Arc::from(name), index, occurrence),
        );
        Some(id)
    }
}

#[cfg(test)]
mod tests;
