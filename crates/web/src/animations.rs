//! CSS animations for scripts (Web Animations 1 §6,
//! <https://drafts.csswg.org/web-animations-1/#programming-interface>; CSS
//! Animations 2 §6.1; ADR 0044 section 7, M13 feature 3b): `getAnimations()`
//! of documents and elements, `CSSAnimation` (`play()`, `pause()`,
//! `currentTime`, `playState`, `animationName`, `effect`) and
//! `KeyframeEffect` (`target`, `getKeyframes()`, `getTiming()`,
//! `getComputedTiming()`).
//!
//! The engine keeps the animation records (`swb_style::animation`); the
//! bindings keep one wrapper per live record ([`AnimationObjects`], traced
//! strongly), so `getAnimations()` gives the same object for the same
//! record. A wrapper is an event target whose data holds the record's id
//! and its `[SameObject]` effect. The ids of released records come from
//! the engine ([`crate::EngineHost::released_animations`]); their entries
//! go, and a wrapper that a script still holds answers as an idle
//! animation that keeps its `animationName` and `effect.target`.

use std::collections::HashMap;

use swb_dom::NodeId;
use swb_js::{Gc, GenericData, HostData, Object, Tracer, Value, VmError};

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
}

impl AnimationObjects {
    /// Reports the wrappers to the collector.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for object in self.wrappers.values() {
            tracer.object(*object);
        }
    }
}

/// The part of an `Animation`'s event target data: the record and the
/// `[SameObject]` effect.
#[derive(Debug)]
pub(crate) struct AnimationData {
    id: u64,
    effect: Option<Gc<Object>>,
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
        if let Some(effect) = self.effect {
            tracer.object(effect);
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

    fn start_time(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<f64>> {
        Ok(time_value(
            info(cx, this)?.and_then(|i| i.start_time),
            false,
        ))
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
        Ok(())
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

    fn play(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = animation_id(cx, this)?;
        engine_mut(cx.rt)?.animation_command(id, AnimationCommand::Play);
        Ok(())
    }

    fn pause(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = animation_id(cx, this)?;
        engine_mut(cx.rt)?.animation_command(id, AnimationCommand::Pause);
        Ok(())
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
