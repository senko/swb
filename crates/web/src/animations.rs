//! CSS animations for scripts (Web Animations 1 §6,
//! <https://drafts.csswg.org/web-animations-1/#programming-interface>; CSS
//! Animations 2 §6.1; ADR 0044 section 7, M13 features 3b and 3c):
//! `getAnimations()` of documents and elements, `CSSAnimation` (`play()`,
//! `pause()`, `cancel()`, `finish()`, `currentTime`, `startTime`,
//! `playState`, `ready`, `finished`, `timeline`, `animationName`,
//! `effect`), `KeyframeEffect` (`target`, `getKeyframes()`, `getTiming()`,
//! `getComputedTiming()`), `document.timeline` and `AnimationEvent`.
//!
//! The promises. A wrapper keeps its current `ready` and `finished`
//! promises, made at the first read. A pending one puts the wrapper on
//! the waiting list ([`AnimationObjects`]); [`settle_promises`] settles
//! them from the state of the records: at the step "update animations and
//! send events" of the rendering update (before its microtask checkpoint)
//! and after each command of a script. `ready` resolves when no play or
//! pause is pending, `finished` when the play state is `finished`; an
//! animation whose record went (a cancel by a style change) and `cancel()`
//! reject both with an `AbortError` marked as handled, and the next read
//! makes new ones (Web Animations §4.4.14 and §4.4.15).
//!
//! The engine keeps the animation records (`swb_style::animation`); the
//! bindings keep one wrapper per live record ([`AnimationObjects`], traced
//! strongly), so `getAnimations()` gives the same object for the same
//! record. A wrapper is an event target whose data holds the record's id
//! and its `[SameObject]` effect. The ids of released records come from
//! the engine ([`crate::EngineHost::released_animations`]); their entries
//! go, and a wrapper that a script still holds answers as an idle
//! animation that keeps its `animationName` and `effect.target`.

use std::collections::{BTreeMap, HashMap};

use swb_dom::NodeId;
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value, VmError, VmResult};

use crate::bindings::{self as b, Cx, Platform};
use crate::events::TargetData;
use crate::generated as g;
use crate::generated::types::{ComputedEffectTiming, EffectTiming, GetAnimationsOptions};
use crate::state::{WebState, engine_mut, state_mut};
use crate::{AnimationCommand, AnimationInfo};

/// The wrappers of the live animations that scripts have seen, by id.
#[derive(Default)]
pub(crate) struct AnimationObjects {
    wrappers: HashMap<u64, Gc<Object>>,
    /// The wrappers with a pending `ready` or `finished` promise, by id
    /// (also after their record went). Promises settle in the order of the
    /// ids, which is the order of creation, so a run is deterministic.
    waiting: BTreeMap<u64, Gc<Object>>,
    /// The state epoch of the engine's records at the last settling of the
    /// waiting promises ([`swb_style::animation::DocumentAnimations::state_epoch`]):
    /// while it is the current one, no play state or pending flag changed
    /// and the promises have nothing to settle.
    settled_epoch: Option<u64>,
    /// `document.timeline` (`[SameObject]`).
    timeline: Option<Gc<Object>>,
}

impl AnimationObjects {
    /// Reports the wrappers to the collector.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for object in self.wrappers.values().chain(self.waiting.values()) {
            tracer.object(*object);
        }
        if let Some(timeline) = self.timeline {
            tracer.object(timeline);
        }
    }

    /// True if a promise of an animation waits and the records changed
    /// since the last settling (`epoch` is the engine's state epoch).
    pub(crate) fn promises_due(&self, epoch: u64) -> bool {
        !self.waiting.is_empty() && self.settled_epoch != Some(epoch)
    }
}

/// The part of an `Animation`'s event target data: the record and the
/// `[SameObject]` effect.
#[derive(Debug)]
pub(crate) struct AnimationData {
    id: u64,
    effect: Option<Gc<Object>>,
    /// The current ready promise, once read (never a rejected one).
    ready: Option<Gc<Object>>,
    /// The current finished promise, once read (never a rejected one).
    finished: Option<Gc<Object>>,
    /// What stays true after the record went: the name and the target.
    origin: Origin,
}

/// The name and the target of an animation, kept by its wrapper: a wrapper
/// whose record went still answers `animationName` and `effect.target`
/// (measured in Chromium 148, probe `remove-element`).
#[derive(Clone, Debug)]
struct Origin {
    name: String,
    target: NodeId,
    pseudo: Option<&'static str>,
}

impl AnimationData {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for object in [self.effect, self.ready, self.finished]
            .into_iter()
            .flatten()
        {
            tracer.object(object);
        }
    }
}

/// The host data of a `KeyframeEffect`: the record of its animation.
struct EffectData {
    id: u64,
    origin: Origin,
}

impl GenericData for EffectData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// The handle of a generic host object.
fn generic(data: HostData) -> b::Result<Gc<swb_js::Generic>> {
    match data {
        HostData::Generic(handle) => Ok(handle),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// Drops the wrappers of the records that went.
fn sweep(cx: &mut Cx<'_>) -> b::Result<()> {
    let released = engine_mut(cx.rt)?.released_animations();
    if !released.is_empty() {
        let objects = &mut state_mut(cx.rt)?.doc.animations;
        for id in released {
            objects.wrappers.remove(&id);
        }
    }
    Ok(())
}

/// The wrappers of `ids`, made for the ids that have none.
fn wrappers(cx: &mut Cx<'_>, ids: &[u64]) -> b::Result<Vec<Gc<Object>>> {
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        if let Some(object) = state_mut(cx.rt)?.doc.animations.wrappers.get(&id) {
            out.push(*object);
            continue;
        }
        let origin = match engine_mut(cx.rt)?.animation(id) {
            Some(info) => Origin {
                name: info.name,
                target: info.target,
                pseudo: info.pseudo,
            },
            None => Origin {
                name: String::new(),
                target: NodeId::DOCUMENT,
                pseudo: None,
            },
        };
        let mut data = TargetData::default();
        data.animation = Some(Box::new(AnimationData {
            id,
            effect: None,
            ready: None,
            finished: None,
            origin,
        }));
        let data = cx.rt.alloc_host_data(data)?;
        let realm = cx.realm();
        let object = cx.rt.new_host_object(realm, g::CSS_ANIMATION, data)?;
        state_mut(cx.rt)?.doc.animations.wrappers.insert(id, object);
        out.push(object);
    }
    Ok(out)
}

/// `getAnimations()` of the document (`element` `None`) or an element.
fn get_animations(
    cx: &mut Cx<'_>,
    element: Option<NodeId>,
    subtree: bool,
) -> b::Result<Vec<Gc<Object>>> {
    let ids = engine_mut(cx.rt)?.animations(element, subtree);
    sweep(cx)?;
    wrappers(cx, &ids)
}

/// `document.getAnimations()`.
pub(crate) fn document_animations(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Vec<Gc<Object>>> {
    if this != NodeId::DOCUMENT {
        return Ok(Vec::new());
    }
    get_animations(cx, None, false)
}

/// `element.getAnimations(options)`.
pub(crate) fn element_animations(
    cx: &mut Cx<'_>,
    this: NodeId,
    options: &GetAnimationsOptions,
) -> b::Result<Vec<Gc<Object>>> {
    get_animations(cx, Some(this), options.subtree)
}

/// The id of the animation `this`.
fn animation_id(cx: &Cx<'_>, this: Platform) -> b::Result<u64> {
    let handle = generic(this.data)?;
    cx.rt
        .heap()
        .generic::<TargetData>(handle)
        .map_err(VmError::from)?
        .animation
        .as_ref()
        .map(|a| a.id)
        .ok_or_else(|| VmError::type_error("Illegal invocation").into())
}

/// The state of the animation `this`; `None` once its record went.
fn info(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<AnimationInfo>> {
    let id = animation_id(cx, this)?;
    Ok(engine_mut(cx.rt)?.animation(id))
}

/// The id of the animation of the effect `this`.
fn effect_id(cx: &Cx<'_>, this: Platform) -> b::Result<u64> {
    Ok(effect_data(cx, this)?.0)
}

/// The id and the origin of the animation of the effect `this`.
fn effect_data(cx: &Cx<'_>, this: Platform) -> b::Result<(u64, Origin)> {
    let handle = generic(this.data)?;
    let data = cx
        .rt
        .heap()
        .generic::<EffectData>(handle)
        .map_err(VmError::from)?;
    Ok((data.id, data.origin.clone()))
}

/// The origin of the animation `this`.
fn animation_origin(cx: &Cx<'_>, this: Platform) -> b::Result<Origin> {
    let handle = generic(this.data)?;
    cx.rt
        .heap()
        .generic::<TargetData>(handle)
        .map_err(VmError::from)?
        .animation
        .as_ref()
        .map(|a| a.origin.clone())
        .ok_or_else(|| VmError::type_error("Illegal invocation").into())
}

// ----- The ready and finished promises -----

/// Which promise of an animation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Ready,
    Finished,
}

/// The handle of the event target data of the animation wrapper `object`.
fn handle_of(rt: &Runtime, object: Gc<Object>) -> VmResult<Gc<swb_js::Generic>> {
    match rt.host_object(object.into()) {
        Some((_, HostData::Generic(handle))) => Ok(handle),
        _ => Err(VmError::type_error("Illegal invocation")),
    }
}

/// The id and the current promise `which` of the animation wrapper
/// `object`.
fn promise_of(
    rt: &Runtime,
    object: Gc<Object>,
    which: Which,
) -> VmResult<(u64, Option<Gc<Object>>)> {
    let handle = handle_of(rt, object)?;
    let data = rt.heap().generic::<TargetData>(handle)?;
    let animation = data
        .animation
        .as_ref()
        .ok_or(VmError::type_error("Illegal invocation"))?;
    let promise = match which {
        Which::Ready => animation.ready,
        Which::Finished => animation.finished,
    };
    Ok((animation.id, promise))
}

/// Replaces the current promise `which` of the animation wrapper `object`.
fn set_promise(
    rt: &mut Runtime,
    object: Gc<Object>,
    which: Which,
    promise: Option<Gc<Object>>,
) -> VmResult<()> {
    let handle = handle_of(rt, object)?;
    if let Some(animation) = rt
        .heap_mut()
        .generic_mut::<TargetData>(handle)?
        .animation
        .as_mut()
    {
        match which {
            Which::Ready => animation.ready = promise,
            Which::Finished => animation.finished = promise,
        }
    }
    Ok(())
}

/// True if `promise` is pending.
fn is_pending(rt: &Runtime, promise: Gc<Object>) -> bool {
    matches!(
        rt.promise_settlement(promise.into()),
        Some((swb_js::PromiseStatus::Pending, _))
    )
}

/// The state that settles the promises: `Some((play state, pending))`, or
/// `None` when the record went.
type PromiseState = Option<(&'static str, bool)>;

/// True if the promise `which` of an animation in `state` is settled
/// (resolved): `ready` when no play or pause is pending and the animation
/// is not idle (measured in Chromium 148, `ready-promise` of
/// `css-animations-3c.json`: the `ready` of a cancelled animation stays
/// pending), `finished` when the animation is finished.
fn resolved_in(which: Which, state: PromiseState) -> bool {
    match (which, state) {
        (Which::Ready, Some((play_state, pending))) => !pending && play_state != "idle",
        (Which::Finished, Some((play_state, _))) => play_state == "finished",
        (_, None) => false,
    }
}

/// Rejects `promise` with an `AbortError` and marks it as handled ("cancel
/// an animation"): no `unhandledrejection` follows.
fn reject_aborted(rt: &mut Runtime, promise: Gc<Object>) -> VmResult<()> {
    let realm = crate::state::doc_realm(rt)?;
    let error = b::dom_exception::create(
        rt,
        realm,
        g::DOM_EXCEPTION,
        "AbortError",
        "The user aborted a request.",
    )?;
    rt.mark_promise_handled(promise);
    rt.reject_pending_promise(promise, error.into())
}

/// Settles the pending promises of the animation wrapper `object` from
/// `state`; a record that went (`None`) was cancelled, which rejects them
/// and drops them (the next read makes new ones). Returns true if a
/// promise still waits (and the animation is not idle: an idle animation's
/// `finished` waits for a `play()`, which adds it to the list again).
fn settle_wrapper(rt: &mut Runtime, object: Gc<Object>, state: PromiseState) -> VmResult<bool> {
    let mut waits = false;
    for which in [Which::Ready, Which::Finished] {
        let (_, promise) = promise_of(rt, object, which)?;
        let Some(promise) = promise.filter(|&p| is_pending(rt, p)) else {
            continue;
        };
        if state.is_none() {
            reject_aborted(rt, promise)?;
            set_promise(rt, object, which, None)?;
        } else if resolved_in(which, state) {
            rt.resolve_pending_promise(promise, object.into())?;
        } else {
            waits = true;
        }
    }
    Ok(waits && state.is_some_and(|(play_state, _)| play_state != "idle"))
}

/// Settles the promises of the animation wrapper `object` (id `id`) now
/// and keeps it on the waiting list while one waits.
fn refresh(rt: &mut Runtime, object: Gc<Object>, id: u64) -> VmResult<()> {
    let state = engine_mut(rt)?
        .animation_states(&[id])
        .first()
        .copied()
        .flatten();
    let waits = settle_wrapper(rt, object, state)?;
    let objects = &mut state_mut(rt)?.doc.animations;
    if waits {
        objects.waiting.insert(id, object);
    } else {
        objects.waiting.remove(&id);
    }
    Ok(())
}

/// Settles the `ready` and `finished` promises of the animations that
/// wait (the step "update animations and send events", before its
/// microtask checkpoint).
pub(crate) fn settle_promises(rt: &mut Runtime, epoch: u64) -> VmResult<()> {
    state_mut(rt)?.doc.animations.settled_epoch = Some(epoch);
    let waiting: Vec<(u64, Gc<Object>)> = state_mut(rt)?
        .doc
        .animations
        .waiting
        .iter()
        .map(|(id, object)| (*id, *object))
        .collect();
    if waiting.is_empty() {
        return Ok(());
    }
    let ids: Vec<u64> = waiting.iter().map(|(id, _)| *id).collect();
    let states = engine_mut(rt)?.animation_states(&ids);
    let mut done = Vec::new();
    for ((id, object), state) in waiting.into_iter().zip(states) {
        if !settle_wrapper(rt, object, state)? {
            done.push(id);
        }
    }
    let objects = &mut state_mut(rt)?.doc.animations;
    for id in done {
        objects.waiting.remove(&id);
    }
    Ok(())
}

/// The current promise `which` of the animation `this`: the stored one
/// while it is pending, or resolved and still right for the state; else a
/// new one, resolved at once if the state says so.
fn current_promise(cx: &mut Cx<'_>, this: Platform, which: Which) -> b::Result<Gc<Object>> {
    let object = this.object;
    let (id, stored) = promise_of(cx.rt, object, which)?;
    let state = engine_mut(cx.rt)?
        .animation_states(&[id])
        .first()
        .copied()
        .flatten();
    if let Some(promise) = stored
        && (is_pending(cx.rt, promise) || resolved_in(which, state))
    {
        return Ok(promise);
    }
    let realm = cx.realm();
    let promise = cx.rt.new_pending_promise(realm)?;
    set_promise(cx.rt, object, which, Some(promise))?;
    if resolved_in(which, state) {
        cx.rt.resolve_pending_promise(promise, object.into())?;
    } else if state.is_some_and(|(play_state, _)| play_state != "idle") {
        state_mut(cx.rt)?.doc.animations.waiting.insert(id, object);
    }
    Ok(promise)
}

/// After a command of a script: settles the promises of `this` that the
/// command settled (`finish()`, a seek or a new start time end a pending
/// task) and keeps the wrapper waiting while one waits.
fn after_command(cx: &mut Cx<'_>, this: Platform, id: u64) -> b::Result<()> {
    refresh(cx.rt, this.object, id)?;
    Ok(())
}

/// `cancel()`: the record goes idle; a pending `ready` and the `finished`
/// promise are rejected with an `AbortError`, marked as handled and
/// dropped. The next read makes new ones, both pending while the animation
/// is idle (Chromium 148; Web Animations would resolve the new `ready`).
fn cancel_promises(cx: &mut Cx<'_>, this: Platform, id: u64) -> b::Result<()> {
    for which in [Which::Ready, Which::Finished] {
        let (_, promise) = promise_of(cx.rt, this.object, which)?;
        if let Some(promise) = promise {
            if is_pending(cx.rt, promise) {
                reject_aborted(cx.rt, promise)?;
            }
            set_promise(cx.rt, this.object, which, None)?;
        }
    }
    state_mut(cx.rt)?.doc.animations.waiting.remove(&id);
    Ok(())
}

/// `document.timeline`: one `DocumentTimeline` per window.
pub(crate) fn document_timeline(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    if let Some(timeline) = state_mut(cx.rt)?.doc.animations.timeline {
        return Ok(timeline);
    }
    let realm = cx.realm();
    let timeline = cx
        .rt
        .new_host_object(realm, g::DOCUMENT_TIMELINE, HostData::Word(0))?;
    state_mut(cx.rt)?.doc.animations.timeline = Some(timeline);
    Ok(timeline)
}

/// What a start time of the document timeline gains on the scale of
/// `document.timeline.currentTime`: the scripts' timeline time minus the
/// timeline's own time. The scripts' time is coarsened as
/// `performance.now()` is, so a start time moves with it and
/// `currentTime` stays `document.timeline.currentTime - startTime`
/// (Chromium 148, `timeline-time` of `css-animations-3c.json`).
fn timeline_shift(cx: &mut Cx<'_>) -> b::Result<f64> {
    let Some((time, zero)) = engine_mut(cx.rt)?.animation_timeline() else {
        return Ok(0.0);
    };
    Ok(crate::state::state(cx.rt)?.doc.clock.at(zero + time) - time)
}

/// A time for an IDL `double?`: `None` stays. A hold time (`held`: a
/// seeked or paused animation) is read unchanged (Chromium reads back
/// `2500.0004` as written); a running animation's time is rounded to
/// 1e-9 ms, which removes the float noise of `timeline - start time` but
/// keeps a time that a script set (Chromium reads back `2500.0004` from a
/// running animation too; ADR 0044 section 15).
fn time_value(time: Option<f64>, held: bool) -> Option<f64> {
    time.map(|t| {
        if held {
            t
        } else {
            swb_style::animation::round_noise(t) + 0.0
        }
    })
}

impl g::animation::AnimationImpl for WebState {
    fn id(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn effect(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        let handle = generic(this.data)?;
        let (id, effect, origin) = {
            let data = cx
                .rt
                .heap()
                .generic::<TargetData>(handle)
                .map_err(VmError::from)?;
            let animation = data
                .animation
                .as_ref()
                .ok_or_else(|| VmError::type_error("Illegal invocation"))?;
            (animation.id, animation.effect, animation.origin.clone())
        };
        if let Some(effect) = effect {
            return Ok(Some(effect));
        }
        let data = cx.rt.alloc_host_data(EffectData { id, origin })?;
        let realm = cx.realm();
        let effect = cx.rt.new_host_object(realm, g::KEYFRAME_EFFECT, data)?;
        if let Some(animation) = cx
            .rt
            .heap_mut()
            .generic_mut::<TargetData>(handle)
            .map_err(VmError::from)?
            .animation
            .as_mut()
        {
            animation.effect = Some(effect);
        }
        Ok(Some(effect))
    }

    fn timeline(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(Some(document_timeline(cx)?))
    }

    /// The start time on the scale of `document.timeline.currentTime`.
    fn start_time(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<f64>> {
        let start = info(cx, this)?.and_then(|i| i.start_time);
        let shift = timeline_shift(cx)?;
        Ok(time_value(start.map(|s| s + shift), false))
    }

    fn set_start_time(cx: &mut Cx<'_>, this: Platform, value: Option<f64>) -> b::Result<()> {
        let id = animation_id(cx, this)?;
        let shift = timeline_shift(cx)?;
        let start = value.map(|v| v - shift);
        engine_mut(cx.rt)?.animation_command(id, AnimationCommand::SetStartTime(start));
        after_command(cx, this, id)
    }

    fn current_time(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<f64>> {
        Ok(info(cx, this)?.and_then(|i| time_value(i.current_time, i.held)))
    }

    /// Setting `null` while the timeline is active throws a `TypeError`
    /// ("set the current time", step 1).
    fn set_current_time(cx: &mut Cx<'_>, this: Platform, value: Option<f64>) -> b::Result<()> {
        let id = animation_id(cx, this)?;
        let Some(time) = value else {
            return Err(VmError::type_error(
                "Failed to set the 'currentTime' property on 'Animation': currentTime may not be changed from resolved to unresolved",
            )
            .into());
        };
        engine_mut(cx.rt)?.animation_command(id, AnimationCommand::SetCurrentTime(time));
        after_command(cx, this, id)
    }

    fn play_state(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(info(cx, this)?.map_or("idle", |i| i.play_state).to_owned())
    }

    fn replace_state(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("active".to_owned())
    }

    fn pending(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(info(cx, this)?.is_some_and(|i| i.pending))
    }

    fn ready(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        current_promise(cx, this, Which::Ready)
    }

    fn finished(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        current_promise(cx, this, Which::Finished)
    }

    fn cancel(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = animation_id(cx, this)?;
        engine_mut(cx.rt)?.animation_command(id, AnimationCommand::Cancel);
        cancel_promises(cx, this, id)
    }

    /// An animation whose end is infinite throws `InvalidStateError` (Web
    /// Animations "finish an animation", step 2).
    fn finish(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = animation_id(cx, this)?;
        if !engine_mut(cx.rt)?.animation_command(id, AnimationCommand::Finish) {
            return Err(b::Exception::dom(
                "InvalidStateError",
                "Cannot finish Animation with an infinite target effect end.",
            ));
        }
        after_command(cx, this, id)
    }

    fn play(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = animation_id(cx, this)?;
        engine_mut(cx.rt)?.animation_command(id, AnimationCommand::Play);
        after_command(cx, this, id)
    }

    fn pause(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = animation_id(cx, this)?;
        engine_mut(cx.rt)?.animation_command(id, AnimationCommand::Pause);
        after_command(cx, this, id)
    }
}

impl g::css_transition::CSSTransitionImpl for WebState {
    /// No `CSSTransition` exists before feature 5.
    fn transition_property(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }
}

impl g::animation_timeline::AnimationTimelineImpl for WebState {
    /// The time of the document timeline as `performance.now()` gives it
    /// for the same page time (so it equals the timestamp of the animation
    /// frame callbacks of the update, as in Chromium 148:
    /// `timeline-time` of `css-animations-3c.json`).
    fn current_time(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<f64>> {
        let Some((time, zero)) = engine_mut(cx.rt)?.animation_timeline() else {
            return Ok(None);
        };
        Ok(Some(crate::state::state(cx.rt)?.doc.clock.at(zero + time)))
    }
}

impl g::css_animation::CSSAnimationImpl for WebState {
    fn animation_name(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match info(cx, this)? {
            Some(info) => Ok(info.name),
            None => Ok(animation_origin(cx, this)?.name),
        }
    }
}

/// The text of a fill mode, a direction.
fn fill_text(fill: swb_style::animation::FillMode) -> &'static str {
    use swb_style::animation::FillMode as F;
    match fill {
        F::None => "none",
        F::Forwards => "forwards",
        F::Backwards => "backwards",
        F::Both => "both",
    }
}

fn direction_text(direction: swb_style::animation::PlaybackDirection) -> &'static str {
    use swb_style::animation::PlaybackDirection as D;
    match direction {
        D::Normal => "normal",
        D::Reverse => "reverse",
        D::Alternate => "alternate",
        D::AlternateReverse => "alternate-reverse",
    }
}

/// `getTiming()` of an animation's state (of a released one: the default
/// timing).
fn effect_timing(info: Option<&AnimationInfo>) -> EffectTiming {
    let default = swb_style::animation::Timing::default();
    let t = info.map_or(&default, |i| &i.timing);
    EffectTiming {
        delay: t.delay,
        direction: direction_text(t.direction).to_owned(),
        duration: t.duration,
        easing: t.easing.computed_text(),
        end_delay: t.end_delay,
        fill: fill_text(t.fill).to_owned(),
        iteration_start: t.iteration_start,
        iterations: t.iterations,
    }
}

impl g::animation_effect::AnimationEffectImpl for WebState {
    fn get_timing(cx: &mut Cx<'_>, this: Platform) -> b::Result<EffectTiming> {
        let id = effect_id(cx, this)?;
        let info = engine_mut(cx.rt)?.animation(id);
        Ok(effect_timing(info.as_ref()))
    }

    fn get_computed_timing(cx: &mut Cx<'_>, this: Platform) -> b::Result<ComputedEffectTiming> {
        let id = effect_id(cx, this)?;
        let info = engine_mut(cx.rt)?.animation(id);
        let timing = effect_timing(info.as_ref());
        let default = swb_style::animation::Timing::default();
        let model = info.as_ref().map_or(&default, |i| &i.timing);
        let computed = info.as_ref().map(|i| i.computed);
        Ok(ComputedEffectTiming {
            delay: timing.delay,
            direction: timing.direction,
            duration: timing.duration,
            easing: timing.easing,
            end_delay: timing.end_delay,
            fill: timing.fill,
            iteration_start: timing.iteration_start,
            iterations: timing.iterations,
            active_duration: model.active_duration(),
            current_iteration: computed.and_then(|c| c.current_iteration),
            end_time: model.end_time(),
            local_time: info
                .as_ref()
                .and_then(|i| time_value(i.current_time, i.held)),
            progress: computed.and_then(|c| c.progress),
        })
    }
}

impl g::keyframe_effect::KeyframeEffectImpl for WebState {
    fn target(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        let (_, origin) = effect_data(cx, this)?;
        Ok(Some(origin.target))
    }

    fn pseudo_element(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        let (_, origin) = effect_data(cx, this)?;
        Ok(origin.pseudo.map(str::to_owned))
    }

    fn composite(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("replace".to_owned())
    }

    fn get_keyframes(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        let id = effect_id(cx, this)?;
        let frames = engine_mut(cx.rt)?.animation_keyframes(id);
        let realm = cx.realm();
        let mut out = Vec::with_capacity(frames.len());
        for frame in frames {
            let object = cx.rt.new_plain_object(realm)?;
            b::to_js::dictionary_member(cx.rt, object, "offset", Value::number(frame.offset))?;
            let easing = b::to_js::string(cx.rt, &frame.easing)?;
            b::to_js::dictionary_member(cx.rt, object, "easing", easing)?;
            let composite = b::to_js::string(cx.rt, frame.composite)?;
            b::to_js::dictionary_member(cx.rt, object, "composite", composite)?;
            for (name, value) in &frame.values {
                let value = b::to_js::string(cx.rt, value)?;
                b::to_js::dictionary_member(cx.rt, object, name, value)?;
            }
            b::to_js::dictionary_member(
                cx.rt,
                object,
                "computedOffset",
                Value::number(frame.offset),
            )?;
            b::keep(cx.rt, Value::Object(object));
            out.push(object);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::time_value;

    #[test]
    fn a_time_that_a_script_set_reads_back_unchanged() {
        // Chromium 148 reads back `2500.0004` after `a.currentTime =
        // 2500.0004`, from a paused and from a running animation (the
        // review's probe `current-time-values`). A running animation's time
        // is `timeline - start time`, with float noise.
        let timeline = 61.0 * 1000.0 / 60.0;
        let start = timeline - 2500.0004;
        assert_eq!(time_value(Some(timeline - start), false), Some(2500.0004));
        assert_eq!(time_value(Some(2500.0004), true), Some(2500.0004));
        assert_eq!(
            time_value(Some(123.456_789_012_345), true),
            Some(123.456_789_012_345)
        );
        assert_eq!(time_value(Some(1e300), false), Some(1e300));
        assert_eq!(
            time_value(Some(timeline - (timeline - 5.0)), false),
            Some(5.0)
        );
        assert_eq!(
            time_value(Some(-0.0), false).map(f64::is_sign_negative),
            Some(false)
        );
        assert_eq!(time_value(None, false), None);
    }
}
