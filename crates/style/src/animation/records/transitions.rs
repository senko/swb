//! CSS transitions in the records of a document (CSS Transitions 1 §3,
//! <https://drafts.csswg.org/css-transitions-1/#starting>; CSS Transitions
//! 2; ADR 0044 sections 1, 3 and 20, M13 feature 5).
//!
//! A transition is an [`AnimationRecord`] with a [`TransitionState`]: its
//! property, its start and end values, the reversing-adjusted start value,
//! the reversing shortening factor and its transition generation. Its
//! effect has the transition's delay and duration, one iteration, the fill
//! mode `backwards` and the `transition-timing-function` as its easing (as
//! Chromium 148 shows it in `getComputedTiming()`). The values are the
//! computed values of the one property ([`ComputedSlot`]), not whole
//! styles.
//!
//! [`DocumentAnimations::sync_transitions`] runs the rules of CSS
//! Transitions 1 §3 for one element in a style change event: the
//! before-change style (the element's style before the event, with its
//! running transitions at their values) against the after-change style
//! (the new cascade with the animated values of the CSS animations and
//! without the transitions, inheriting from the parent's style without
//! animations and transitions). [`DocumentAnimations::transition_effects`]
//! gives the cascade the values of the running transitions for the second
//! compute.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use swb_css::{ColorScheme, ComponentValue};

use super::{
    AnimationId, AnimationOwner, AnimationPlayState, AnimationRecord, CompletedSet,
    DocumentAnimations,
};
use crate::ComputedStyle;
use crate::animation::easing::Easing;
use crate::animation::interpolate::{
    custom_is_interpolable, interpolate_custom, interpolate_longhand, is_interpolable,
};
use crate::animation::timing::{FillMode, Phase, Timing};
use crate::animation::transition_properties::{
    TransitionEntry, TransitionName, TransitionStyle, is_transitionable_longhand,
};
use crate::animation::visibility::{
    is_composited_property, is_inherited_property, reach_of_property,
};
use crate::properties::set::LonghandSet;
use crate::properties::{ComputedSlot, LonghandId};
use crate::used_color_scheme;
use crate::values::Display;

/// The property of a transition.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TransitionTarget {
    /// A longhand.
    Longhand(LonghandId),
    /// A custom property (by its name).
    Custom(Arc<str>),
}

impl TransitionTarget {
    /// The name of the property (`transitionProperty`, `propertyName`).
    pub(crate) fn name(&self) -> &str {
        match self {
            TransitionTarget::Longhand(id) => id.name(),
            TransitionTarget::Custom(name) => name,
        }
    }

    /// The transition of the property in `list`.
    fn entry<'a>(&self, list: &'a TransitionStyle) -> Option<TransitionEntry<'a>> {
        match self {
            TransitionTarget::Longhand(id) => list.matching(Some(*id), None),
            TransitionTarget::Custom(name) => list.matching(None, Some(name)),
        }
    }
}

/// A value of a transition: the computed value of a longhand alone, or the
/// computed tokens of a custom property (`None`: the guaranteed-invalid
/// value).
#[derive(Clone, Debug)]
pub(crate) enum TransitionValue {
    /// The computed value of the longhand.
    Longhand(ComputedSlot),
    /// The value of the custom property.
    Custom(Option<Arc<[ComponentValue]>>),
}

impl TransitionValue {
    /// The tokens of a custom property value.
    pub(crate) fn tokens(&self) -> Option<&Arc<[ComponentValue]>> {
        match self {
            TransitionValue::Custom(tokens) => tokens.as_ref(),
            TransitionValue::Longhand(_) => None,
        }
    }
}

/// What a CSS transition keeps besides its timing.
#[derive(Clone, Debug)]
pub(crate) struct TransitionState {
    /// The property.
    pub(crate) target: TransitionTarget,
    /// The start value.
    pub(crate) start: TransitionValue,
    /// The end value.
    pub(crate) end: TransitionValue,
    /// The reversing-adjusted start value.
    pub(crate) reversing_start: TransitionValue,
    /// The reversing shortening factor (0 to 1).
    pub(crate) reversing_factor: f64,
    /// The transition generation: transitions that one style change event
    /// of an element starts share one (composite order).
    pub(crate) generation: u64,
}

/// The value of a running transition for the cascade.
pub(crate) struct TransitionEffect {
    /// The property.
    pub(crate) target: TransitionTarget,
    /// The start value.
    pub(crate) from: TransitionValue,
    /// The end value.
    pub(crate) to: TransitionValue,
    /// The progress through the transition's easing.
    pub(crate) progress: f64,
}

thread_local! {
    /// Styles that hold the values of a transition for the functions on
    /// whole styles (interpolation, comparison): only the fields of the
    /// property at hand are read, so the others keep what earlier calls
    /// left in them.
    static SCRATCH: RefCell<Option<Box<[ComputedStyle; 3]>>> = const { RefCell::new(None) };
}

/// Runs `f` with three scratch styles (fresh ones if a caller up the stack
/// holds them).
fn with_scratch<R>(f: impl FnOnce(&mut [ComputedStyle; 3]) -> R) -> R {
    SCRATCH.with(|cell| {
        if let Ok(mut slot) = cell.try_borrow_mut() {
            let styles = slot.get_or_insert_with(|| {
                let initial = (*ComputedStyle::initial()).clone();
                Box::new([initial.clone(), initial.clone(), initial])
            });
            f(styles)
        } else {
            let initial = (*ComputedStyle::initial()).clone();
            let mut styles = [initial.clone(), initial.clone(), initial];
            f(&mut styles)
        }
    })
}

/// Interpolates the longhand `id` from `from` to `to` at `progress` into
/// `style` (the values of a running transition in the cascade; `style`
/// gives `currentcolor` its color).
pub(crate) fn interpolate_slots(
    id: LonghandId,
    from: &ComputedSlot,
    to: &ComputedSlot,
    progress: f64,
    style: &mut ComputedStyle,
) {
    with_scratch(|[a, b, _]| {
        id.put_value(from, a);
        id.put_value(to, b);
        interpolate_longhand(id, a, b, progress, style);
    });
}

/// The value of `target` in `style`.
fn value_of(target: &TransitionTarget, style: &ComputedStyle) -> TransitionValue {
    match target {
        TransitionTarget::Longhand(id) => TransitionValue::Longhand(id.value_of(style)),
        TransitionTarget::Custom(name) => TransitionValue::Custom(custom_of(style, name).cloned()),
    }
}

/// The value of the custom property `name` in `style`.
fn custom_of<'a>(style: &'a ComputedStyle, name: &str) -> Option<&'a Arc<[ComponentValue]>> {
    style
        .custom_properties
        .as_ref()
        .and_then(|map| map.get(name))
}

/// True if `a` and `b` are the same value of `target`.
fn same(target: &TransitionTarget, a: &TransitionValue, b: &TransitionValue) -> bool {
    match (target, a, b) {
        (
            TransitionTarget::Longhand(id),
            TransitionValue::Longhand(a),
            TransitionValue::Longhand(b),
        ) => {
            a.ptr_eq(b)
                || with_scratch(|[x, y, _]| {
                    id.put_value(a, x);
                    id.put_value(b, y);
                    id.same_value(x, y)
                })
        }
        (_, TransitionValue::Custom(a), TransitionValue::Custom(b)) => a == b,
        _ => false,
    }
}

/// True if a transition of `target` can run from `a` to `b` (CSS
/// Transitions 1 §3: the values are transitionable; CSS Transitions 2 §3:
/// with `allow-discrete`, discrete pairs too). `style` is the after-change
/// style (the registrations of custom properties, the color for
/// `currentcolor`).
fn transitionable(
    target: &TransitionTarget,
    a: &TransitionValue,
    b: &TransitionValue,
    allow_discrete: bool,
    style: &ComputedStyle,
) -> bool {
    match (target, a, b) {
        (
            TransitionTarget::Longhand(id),
            TransitionValue::Longhand(a),
            TransitionValue::Longhand(b),
        ) => {
            is_transitionable_longhand(*id)
                && (allow_discrete
                    || with_scratch(|[x, y, _]| {
                        id.put_value(a, x);
                        id.put_value(b, y);
                        is_interpolable(*id, x, y)
                    }))
        }
        (
            TransitionTarget::Custom(name),
            TransitionValue::Custom(a),
            TransitionValue::Custom(b),
        ) => {
            allow_discrete
                || style.custom_properties.as_ref().is_some_and(|map| {
                    custom_is_interpolable(map, name, a.as_ref(), b.as_ref(), style.color)
                })
        }
        _ => false,
    }
}

/// The value of `target` between `from` and `to` at `progress`, for an
/// element with the after-change style `base`.
fn interpolated(
    target: &TransitionTarget,
    from: &TransitionValue,
    to: &TransitionValue,
    progress: f64,
    base: &ComputedStyle,
) -> TransitionValue {
    match (target, from, to) {
        (
            TransitionTarget::Longhand(id),
            TransitionValue::Longhand(a),
            TransitionValue::Longhand(b),
        ) => with_scratch(|[x, y, out]| {
            id.put_value(a, x);
            id.put_value(b, y);
            // `currentcolor` mixes with the element's color.
            out.color = base.color;
            out.color_exact = base.color_exact;
            interpolate_longhand(*id, x, y, progress, out);
            TransitionValue::Longhand(id.value_of(out))
        }),
        (
            TransitionTarget::Custom(name),
            TransitionValue::Custom(a),
            TransitionValue::Custom(b),
        ) => {
            let value = base.custom_properties.as_ref().and_then(|map| {
                interpolate_custom(map, name, a.as_ref(), b.as_ref(), base.color, progress)
            });
            TransitionValue::Custom(value)
        }
        _ => from.clone(),
    }
}

/// The transitionable longhands in the order of `LonghandId::ALL`: the
/// candidates of `all`.
static TRANSITIONABLE: LazyLock<Vec<LonghandId>> = LazyLock::new(|| {
    LonghandId::ALL
        .iter()
        .copied()
        .filter(|&id| is_transitionable_longhand(id))
        .collect()
});

/// The candidate longhands of a style change event whose values differ
/// between `before` and `after`, or for which `also` is true: the
/// longhands that the items of `transition-property` name (all
/// transitionable longhands for `all`), without repeats, in the order of
/// the list (of `LonghandId::ALL` for `all`).
fn changed_longhands(
    list: &TransitionStyle,
    before: &ComputedStyle,
    after: &ComputedStyle,
    also: &dyn Fn(LonghandId) -> bool,
) -> Vec<LonghandId> {
    let differs = |id: LonghandId| !id.same_value(before, after) || also(id);
    let all = list.properties.contains(&TransitionName::All);
    let mut out: Vec<LonghandId> = Vec::new();
    if all {
        out.extend(TRANSITIONABLE.iter().copied().filter(|&id| differs(id)));
        if list.properties.len() == 1 {
            return out;
        }
    }
    // The longhands of the named properties that `all` did not cover.
    let mut seen = LonghandSet::new();
    let mut mark = |id: LonghandId| seen.insert(id);
    if all {
        for &id in TRANSITIONABLE.iter() {
            mark(id);
        }
    }
    for name in list.properties.iter() {
        let TransitionName::Name(name) = name else {
            continue;
        };
        if name.starts_with("--") {
            continue;
        }
        for &id in crate::properties::longhands_of(name) {
            if is_transitionable_longhand(id) && mark(id) && differs(id) {
                out.push(id);
            }
        }
    }
    out
}

/// The candidate custom properties of a style change event whose values
/// differ between `before` and `after`, or that have a completed
/// transition: the ones that the items of `transition-property` name,
/// without repeats. None when the two maps are equal and no custom
/// property has a completed transition (the common case costs one
/// comparison).
fn changed_customs(
    list: &TransitionStyle,
    before: &ComputedStyle,
    after: &ComputedStyle,
    completed: &CompletedSet,
) -> Vec<Arc<str>> {
    let completed_custom = |name: &str| {
        completed
            .iter()
            .any(|(t, _)| matches!(t, TransitionTarget::Custom(c) if c.as_ref() == name))
    };
    if before.custom_properties == after.custom_properties
        && !completed
            .iter()
            .any(|(t, _)| matches!(t, TransitionTarget::Custom(_)))
    {
        return Vec::new();
    }
    let mut out: Vec<Arc<str>> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for name in list.properties.iter() {
        let TransitionName::Name(name) = name else {
            continue;
        };
        if name.starts_with("--")
            && (custom_of(before, name) != custom_of(after, name) || completed_custom(name))
            && seen.insert(name)
        {
            out.push(Arc::clone(name));
        }
    }
    out
}

/// The timing of a transition's effect: its delay and duration, one
/// iteration, the fill mode `backwards` and the timing function as the
/// effect's easing.
fn transition_timing(delay: f64, duration: f64, easing: &Easing) -> Timing {
    Timing {
        delay,
        duration,
        iterations: 1.0,
        fill: FillMode::Backwards,
        easing: easing.clone(),
        ..Timing::default()
    }
}

/// A transition to start: its property, values and timing.
struct NewTransition {
    target: TransitionTarget,
    start: TransitionValue,
    end: TransitionValue,
    reversing_start: TransitionValue,
    reversing_factor: f64,
    timing: Timing,
}

/// What the style of an element says to the transitions of a property:
/// the parts of the rules of CSS Transitions 1 §3 that depend on the
/// after-change style.
struct Change<'a> {
    after: &'a ComputedStyle,
    list: &'a TransitionStyle,
    /// The owner has element children (they can inherit the values).
    children: bool,
}

impl DocumentAnimations {
    /// Runs the rules of CSS Transitions 1 §3 for `owner` in a style change
    /// event: cancels the running transitions whose property no longer has
    /// a matching item of `transition-property` (rule 3) or whose end value
    /// changed to a value they cannot reach (rule 4), reverses or restarts
    /// the others whose end value changed (rule 4), and starts transitions
    /// for the properties whose value differs between `before` and
    /// `after` (rule 1). `before` is `None` for an element without a
    /// before-change style (inserted, coming out of `display: none`, the
    /// first styles of the document): it starts no transition.
    /// `animated` says which longhands and custom properties a CSS
    /// animation of the element drives now: their changes come from the
    /// animation and start no transition. An element whose after-change
    /// `display` is `none` and that keeps no `display` transition has no
    /// transitions: they are cancelled (an unresolved cancel time, ADR
    /// 0044 section 6).
    pub(crate) fn sync_transitions(
        &mut self,
        owner: AnimationOwner,
        before: Option<&Arc<ComputedStyle>>,
        after: &ComputedStyle,
        children: bool,
        prefers: ColorScheme,
        animated: &dyn Fn(&TransitionTarget) -> bool,
    ) {
        let list = TransitionStyle::of(after);
        let change = Change {
            after,
            list,
            children,
        };
        let mut completed = self.completed.remove(&owner).unwrap_or_default();
        // A second run of the event for the same before-change style (the
        // cascade of an element with container units runs twice; a style
        // pass can follow a restyle of the animations with no style change
        // in between) sees the set of completed transitions that the first
        // one used. The set keeps the style: its address cannot come back
        // for another style while the set is kept.
        if let Some((kept, saved)) = self.repeated.remove(&owner)
            && before.is_some_and(|b| Arc::ptr_eq(b, &kept))
        {
            for (target, value) in saved {
                completed.retain(|(t, _)| *t != target);
                completed.push((target, value));
            }
        }
        let mut handled: HashSet<TransitionTarget> = HashSet::new();
        let mut generation = None;
        if self.transitions.contains_key(&owner) {
            let running = self.transition_ids_of(owner).to_vec();
            for id in running {
                if let Some(target) = self.update_running(owner, id, &change, &mut generation) {
                    handled.insert(target);
                }
            }
            // The transitions that this event found completed: their end
            // values replace older ones of the same property.
            for (target, value) in self.completed.remove(&owner).unwrap_or_default() {
                completed.retain(|(old, _)| *old != target);
                completed.push((target, value));
            }
        }
        if let Some(before) = before.filter(|b| b.display != Display::None)
            && TransitionStyle::may_transition(after)
        {
            // A change of the used color scheme changes `color` for the
            // transitions even where its value is equal (measured in
            // Chromium 148, probe rows `scheme-*` of `css-videojs-6.json`:
            // `color-scheme: light only` to `dark` starts a transition of
            // `color` from white to white, and of no other property;
            // `light` to `light dark` changes no used scheme and starts
            // none). The page's own `color-scheme` meta is not read here.
            let scheme_changed = used_color_scheme(before.color_scheme.as_deref(), None, prefers)
                != used_color_scheme(after.color_scheme.as_deref(), None, prefers);
            // A property with a completed transition is a candidate even
            // where the before-change style holds the value of the
            // after-change style: the completed end value is its value
            // before the change (`finish()`, then the old class again:
            // Chromium starts a transition back, probe rows
            // `*-then-restore` of `css-videojs-6.json`).
            let also = |id: LonghandId| {
                (scheme_changed && id == LonghandId::Color)
                    || completed
                        .iter()
                        .any(|(t, _)| *t == TransitionTarget::Longhand(id))
            };
            let longhands = changed_longhands(list, before, after, &also);
            let customs = changed_customs(list, before, after, &completed);
            let targets = longhands
                .into_iter()
                .map(TransitionTarget::Longhand)
                .chain(customs.into_iter().map(TransitionTarget::Custom));
            for target in targets {
                if handled.contains(&target) || animated(&target) {
                    continue;
                }
                let Some(entry) = target.entry(list) else {
                    continue;
                };
                if entry.combined_duration() <= 0.0 {
                    continue;
                }
                // A transition of the property that completed since the
                // before-change style: its end value is the value before
                // the change (rules 1 and 2).
                let start = match completed.iter().find(|(t, _)| *t == target) {
                    Some((_, value)) => value.clone(),
                    None => value_of(&target, before),
                };
                let end = value_of(&target, after);
                let forced =
                    scheme_changed && target == TransitionTarget::Longhand(LonghandId::Color);
                if (!forced && same(&target, &start, &end))
                    || !transitionable(&target, &start, &end, entry.allow_discrete, after)
                {
                    continue;
                }
                let new = NewTransition {
                    target,
                    reversing_start: start.clone(),
                    start,
                    end,
                    reversing_factor: 1.0,
                    timing: transition_timing(entry.delay, entry.duration, entry.easing),
                };
                self.start_transition(owner, new, &mut generation, children);
            }
        }
        if let Some(before) = before
            && !completed.is_empty()
        {
            self.repeated.insert(owner, (Arc::clone(before), completed));
        }
        if after.display == Display::None {
            let keeps = self.transition_ids_of(owner).iter().any(|id| {
                self.records
                    .get(id)
                    .and_then(|r| r.transition.as_ref())
                    .is_some_and(|t| t.target == TransitionTarget::Longhand(LonghandId::Display))
            });
            if !keeps {
                self.cancel_transitions(owner, None);
            }
        }
    }

    /// Rules 3 and 4 for the running transition `id` of `owner`: returns
    /// its property if the transition was handled (kept, replaced or
    /// cancelled because its property changed), `None` if it went for
    /// another reason.
    fn update_running(
        &mut self,
        owner: AnimationOwner,
        id: AnimationId,
        change: &Change<'_>,
        generation: &mut Option<u64>,
    ) -> Option<TransitionTarget> {
        let time = self.time;
        let record = self.records.get(&id)?;
        let state = record.transition.as_deref()?;
        let target = state.target.clone();
        let timing = record.computed_timing(time);
        if matches!(timing.phase, Phase::After | Phase::Idle) {
            // Completed (a script seeked it to its end and paused it, or
            // finished it): not a running transition; it goes without an
            // event. Its end value is the before-change value of the
            // property (a stale animated value in the before-change style
            // must not start a transition), and an older completed
            // transition of the property does not count.
            let end = state.end.clone();
            let list = self.completed.entry(owner).or_default();
            list.retain(|(t, _)| *t != target);
            list.push((target, end));
            self.drop_completed(owner, id);
            return None;
        }
        let Some(entry) = target.entry(change.list) else {
            // Rule 3: no matching item of `transition-property`.
            self.cancel_transition(owner, id, Some(time));
            return Some(target);
        };
        let after = value_of(&target, change.after);
        if same(&target, &state.end, &after) {
            return Some(target);
        }
        // Rule 4: the end value changed.
        let progress = timing.progress.unwrap_or(0.0);
        let current = interpolated(&target, &state.start, &state.end, progress, change.after);
        if same(&target, &current, &after)
            || entry.combined_duration() <= 0.0
            || !transitionable(
                &target,
                &current,
                &after,
                entry.allow_discrete,
                change.after,
            )
        {
            self.cancel_transition(owner, id, Some(time));
            return Some(target);
        }
        let new = if same(&target, &state.reversing_start, &after) {
            // Reversing: the shortening factor shortens the duration (and a
            // negative delay) by how far the old transition got.
            let old = state.reversing_factor;
            let factor = (progress * old + (1.0 - old)).abs().clamp(0.0, 1.0);
            let delay = if entry.delay >= 0.0 {
                entry.delay
            } else {
                factor * entry.delay
            };
            NewTransition {
                target: target.clone(),
                start: current,
                end: after,
                reversing_start: state.end.clone(),
                reversing_factor: factor,
                timing: transition_timing(delay, entry.duration * factor, entry.easing),
            }
        } else {
            NewTransition {
                target: target.clone(),
                reversing_start: current.clone(),
                start: current,
                end: after,
                reversing_factor: 1.0,
                timing: transition_timing(entry.delay, entry.duration, entry.easing),
            }
        };
        self.cancel_transition(owner, id, Some(time));
        self.start_transition(owner, new, generation, change.children);
        Some(target)
    }

    /// Starts a transition of `owner` (a pending play, resolved at the
    /// next update), unless the page has [`super::MAX_RECORDS`].
    fn start_transition(
        &mut self,
        owner: AnimationOwner,
        new: NewTransition,
        generation: &mut Option<u64>,
        children: bool,
    ) {
        let Some(id) = self.reserve_id() else {
            return;
        };
        let generation = *generation.get_or_insert_with(|| {
            self.generation += 1;
            self.generation
        });
        let name = new.target.name().to_owned();
        let mut record = AnimationRecord::new(id, owner, Arc::from(name.as_str()), 0, 0);
        record.step_period = match &new.timing.easing {
            Easing::Steps { count, .. } if *count > 0 => {
                Some(new.timing.duration / f64::from(*count))
            }
            _ => None,
        };
        record.timing = new.timing;
        record.reach = reach_of_property(&name);
        record.composited = is_composited_property(&name);
        record.inherited_due = children && is_inherited_property(&name);
        record.transition = Some(Box::new(TransitionState {
            target: new.target,
            start: new.start,
            end: new.end,
            reversing_start: new.reversing_start,
            reversing_factor: new.reversing_factor,
            generation,
        }));
        record.play(self.time);
        self.records.insert(id, record);
        // Composite order: by generation (the new one is the latest), then
        // by property name in code point order.
        let list = self.transitions.entry(owner).or_default();
        let records = &self.records;
        let key = |id: &AnimationId| {
            records
                .get(id)
                .map(|r| (r.transition_generation(), Arc::clone(&r.name)))
        };
        let new_key = key(&id);
        let at = list.partition_point(|other| key(other) <= new_key);
        list.insert(at, id);
    }

    /// Cancels the transition `id` of `owner` at the timeline time `time`
    /// (`None`: unresolved).
    fn cancel_transition(&mut self, owner: AnimationOwner, id: AnimationId, time: Option<f64>) {
        if let Some(list) = self.transitions.get_mut(&owner) {
            list.retain(|&other| other != id);
            if list.is_empty() {
                self.transitions.remove(&owner);
            }
        }
        self.release(id, time);
    }

    /// Drops the completed transition `id` of `owner` without an event.
    fn drop_completed(&mut self, owner: AnimationOwner, id: AnimationId) {
        if let Some(list) = self.transitions.get_mut(&owner) {
            list.retain(|&other| other != id);
            if list.is_empty() {
                self.transitions.remove(&owner);
            }
        }
        if let Some(record) = self.records.get_mut(&id) {
            record.last_phase = Some(Phase::After);
        }
        self.release(id, None);
    }

    /// Cancels every running transition of `owner`.
    fn cancel_transitions(&mut self, owner: AnimationOwner, time: Option<f64>) {
        if let Some(ids) = self.transitions.remove(&owner) {
            for id in ids {
                self.release(id, time);
            }
        }
    }

    /// The transition `id` of `owner` reached its end at an update: it
    /// leaves the running transitions; its record is released at the next
    /// update, after the bindings settled its `finished` promise.
    pub(super) fn complete_transition(&mut self, id: AnimationId, owner: AnimationOwner) {
        if let Some(list) = self.transitions.get_mut(&owner) {
            list.retain(|&other| other != id);
            if list.is_empty() {
                self.transitions.remove(&owner);
            }
        }
        self.marked.insert(owner.node);
        self.finished.push(id);
        if let Some(state) = self.records.get(&id).and_then(|r| r.transition.as_deref()) {
            let entry = (state.target.clone(), state.end.clone());
            let list = self.completed.entry(owner).or_default();
            list.retain(|(t, _)| *t != entry.0);
            list.push(entry);
        }
    }

    /// The values of the running transitions of `owner` that have a value
    /// now, for the second compute of its cascade (in composite order: a
    /// later transition of a property wins).
    pub(crate) fn transition_effects(&self, owner: AnimationOwner) -> Vec<TransitionEffect> {
        let time = self.time;
        self.transition_ids_of(owner)
            .iter()
            .filter_map(|id| {
                let record = self.records.get(id)?;
                let state = record.transition.as_deref()?;
                if record.play_state(time) == AnimationPlayState::Idle {
                    return None;
                }
                let progress = record.computed_timing(time).progress?;
                Some(TransitionEffect {
                    target: state.target.clone(),
                    from: state.start.clone(),
                    to: state.end.clone(),
                    progress,
                })
            })
            .collect()
    }

    /// Starts the transitions that wait for their first update at the
    /// current timeline time: the transitions that the style step of a
    /// rendering update created start in that frame (their start time is
    /// the frame's time, as Chromium's transitions of a frame's style
    /// recalculation; a transition that a script's read of the styles
    /// created waits for the next update, `pending` until then). Their
    /// events come at the next update.
    pub fn start_pending_transitions(&mut self) {
        let time = self.time;
        let mut started = false;
        for ids in self.transitions.values() {
            for id in ids {
                if let Some(record) = self.records.get_mut(id)
                    && record.last_phase.is_none()
                    && record.pending()
                {
                    record.resolve_pending(time);
                    started = true;
                }
            }
        }
        if started {
            self.invalidate();
        }
    }

    /// True if `owner` has running transitions.
    pub(crate) fn has_transitions(&self, owner: AnimationOwner) -> bool {
        self.transitions.contains_key(&owner)
    }

    /// True if `owner` has completed transitions since its last style
    /// change event.
    pub(crate) fn has_completed(&self, owner: AnimationOwner) -> bool {
        !self.completed.is_empty() && self.completed.contains_key(&owner)
    }

    /// Ends the set of completed transitions of `owner` at a style change
    /// event that has nothing else to do (its styles did not change).
    pub(crate) fn forget_completed(&mut self, owner: AnimationOwner) {
        if !self.completed.is_empty() {
            self.completed.remove(&owner);
        }
        if !self.repeated.is_empty() {
            self.repeated.remove(&owner);
        }
    }

    /// The number of elements whose last style change event kept a set of
    /// completed transitions for a second run.
    #[cfg(test)]
    pub(crate) fn repeated_sets(&self) -> usize {
        self.repeated.len()
    }

    /// Cancels the transitions whose owner has no style now (it left the
    /// document, or it or an ancestor is `display: none`: an unresolved
    /// cancel time) or whose property no longer has a matching item of
    /// `transition-property` (its style came from the style sharing cache
    /// without the rules of a style change event). `style_of` gives the
    /// owner's style.
    pub fn retain_transitions(
        &mut self,
        mut style_of: impl FnMut(AnimationOwner) -> Option<Arc<ComputedStyle>>,
    ) {
        if !self.transition_parents.is_empty() {
            // The elements that lost their style (left the document, below
            // `display: none`).
            self.transition_parents.retain(|&node, _| {
                style_of(AnimationOwner { node, pseudo: None })
                    .is_some_and(|s| s.display != Display::None)
            });
        }
        if !self.completed.is_empty() {
            let gone: Vec<AnimationOwner> = self
                .completed
                .keys()
                .copied()
                .filter(|&o| style_of(o).is_none_or(|s| s.display == Display::None))
                .collect();
            for owner in gone {
                self.completed.remove(&owner);
            }
        }
        if !self.repeated.is_empty() {
            // The sets kept for a second run of an event: they hold a style
            // each, and an element that left keeps none.
            self.repeated
                .retain(|&o, _| style_of(o).is_some_and(|s| s.display != Display::None));
        }
        if self.transitions.is_empty() {
            return;
        }
        let mut owners: Vec<AnimationOwner> = self.transitions.keys().copied().collect();
        // The order of creation: the cancel events are the same in every
        // run.
        owners.sort_by_key(|o| self.transitions.get(o).and_then(|ids| ids.first().copied()));
        let time = self.time;
        for owner in owners {
            let Some(style) = style_of(owner).filter(|s| s.display != Display::None) else {
                self.cancel_transitions(owner, None);
                continue;
            };
            let list = TransitionStyle::of(&style);
            let gone: Vec<AnimationId> = self
                .transition_ids_of(owner)
                .iter()
                .copied()
                .filter(|id| {
                    self.records
                        .get(id)
                        .and_then(|r| r.transition.as_deref())
                        .is_none_or(|t| t.target.entry(list).is_none())
                })
                .collect();
            for id in gone {
                self.cancel_transition(owner, id, Some(time));
            }
        }
    }

    /// A new record id for a record that the caller inserts, unless the
    /// page (or the document without a room) has [`super::MAX_RECORDS`]
    /// records (one warning per document).
    pub(super) fn reserve_id(&mut self) -> Option<AnimationId> {
        let room = match &mut self.room {
            Some(room) => room.take(),
            None => self.records.len() < super::MAX_RECORDS,
        };
        if !room {
            if !self.limit_warned {
                log::warn!(
                    "animations: the page has {} animations and transitions; no more are created",
                    super::MAX_RECORDS
                );
                self.limit_warned = true;
            }
            return None;
        }
        self.invalidate();
        self.next_id += 1;
        Some(self.next_id)
    }
}
