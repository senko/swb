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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_dom::NodeId;

use super::easing::Easing;
use super::keyframes::KeyframeSet;
use super::properties::{AnimationEntry, AnimationStyle, PlayState};
use super::timing::{ComputedTiming, Phase, Timing};
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
    /// The phase at the last update (for the events of feature 3c).
    pub last_phase: Option<Phase>,
    /// The current iteration at the last update (for the events of feature
    /// 3c).
    pub last_iteration: Option<f64>,
    /// The current time at the last update.
    last_time: Option<f64>,
    /// The length of one step in ms when every keyframe interval eases with
    /// `steps()` over the whole iteration (`None`: continuous).
    step_period: Option<f64>,
    /// The keyframe set and easing that `step_period` was computed for.
    step_key: (usize, Easing, f64),
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
            last_phase: None,
            last_iteration: None,
            last_time: None,
            step_period: None,
            step_key: (0, Easing::Linear, f64::NAN),
        }
    }

    /// The current time at the timeline time `timeline`; `None` when idle.
    /// <https://drafts.csswg.org/web-animations-1/#the-current-time-of-an-animation>
    pub fn current_time(&self, timeline: f64) -> Option<f64> {
        self.hold_time
            .or_else(|| self.start_time.map(|start| timeline - start))
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
        let current = timeline - start;
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
    records: HashMap<AnimationId, AnimationRecord>,
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
    /// The page time that the next read of the styles catches the timeline
    /// up to (set by the page, applied by [`DocumentAnimations::settle`]).
    catch_up_to: Option<f64>,
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
            let current = record.current_time(time);
            let timing = record.timing.compute(current);
            let phase = Some(timing.phase);
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
            for (slot, frames) in next.iter_mut().zip([false, true]) {
                if let Some(t) = record.next_time(time, frames) {
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
        let next = if let Some(Cached::Known(next)) = cache.get(slot).copied() {
            next
        } else {
            let next = self
                .records
                .values()
                .filter_map(|r| r.next_time(self.time, frames))
                .reduce(f64::min);
            if let Some(entry) = cache.get_mut(slot) {
                *entry = Cached::Known(next);
            }
            self.next_cache.set(cache);
            next
        };
        next.map(|t| t + self.origin)
    }

    /// Drops the cached next time: a record changed.
    fn invalidate(&self) {
        self.next_cache.set([Cached::Stale; 2]);
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
        let gone: Vec<AnimationOwner> = self.owners.keys().copied().filter(|&o| !keep(o)).collect();
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
        if self.records.remove(&id).is_some() {
            self.invalidate();
            if self.track_released {
                self.released.push(id);
            }
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
        if fresh {
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
