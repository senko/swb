//! The animation input of the cascade (ADR 0044 section 3, M13 feature
//! 3b): the animated values of an element's CSS animations enter the
//! compute step of its cascade as computed values.
//!
//! [`input`] takes the style computed from the winning declarations (the
//! *base style*, whose values are the underlying values of the animations),
//! brings the element's animation records up to date with its
//! `animation-*` lists ([`DocumentAnimations::sync`]), and finds, for each
//! animated property, the effect that wins in composite order: with the
//! composite mode `replace`, the last animation in `animation-name` with
//! keyframes for the property and a value at its current time. A property
//! whose winner is an `!important` author or user-agent declaration is not
//! animated (CSS Cascade 5 §6.1: animations sit between important and
//! normal author declarations). `compute()` then runs again with the input:
//!
//! - the animated custom properties replace their winners before `var()` is
//!   substituted ([`custom_values`]), so a `width: var(--len)` follows an
//!   animated `--len`;
//! - each animated longhand takes its value at its place in the order
//!   ([`animate_longhand`]): its two keyframes are computed with the
//!   element's `ComputeContext` (so `em`, `var()` and `currentcolor` resolve
//!   as for the element), an implicit 0% or 100% keyframe takes the base
//!   style's value, and the two values are interpolated with the
//!   computed-value rules of the property.
//!
//! CSS transitions (M13 feature 5): in a style change event
//! ([`AnimationTarget::change`]), the style with the animated values is
//! the after-change style; [`transition_effects`] runs the rules of CSS
//! Transitions 1 §3 against the owner's before-change style
//! ([`DocumentAnimations::sync_transitions`]) and gives the values of the
//! running transitions, which win over the animations of their properties
//! (CSS Cascade 5 §6.1: transitions are the highest origin). `compute()`
//! then runs once more with both.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use swb_dom::NodeId;

use super::{
    CascadeContext, ImportantPhase, LONGHAND_WORDS, LonghandSet, ResolvedVariables,
    apply_declaration,
};
use crate::ComputedStyle;
use crate::animation::interpolate::interpolate_longhand;
use crate::animation::keyframes::{AnimatedProperty, Keyframe};
use crate::animation::records::{
    TransitionEffect, TransitionTarget, TransitionValue, interpolate_slots,
};
use crate::animation::transition_properties::TransitionStyle;
use crate::animation::{AnimationOwner, AnimationStyle, DocumentAnimations, Easing, KeyframeSet};
use crate::custom::{CustomPool, CustomProperties, Registered, compute_custom_properties};
use crate::properties::compute::ComputeContext;
use crate::properties::{CssWideKeyword, CustomDeclaration, LonghandId, PropertyDeclaration};
use crate::values::{Content, Display};

/// The element or pseudo-element whose animations a cascade applies.
pub(crate) struct AnimationTarget<'t> {
    /// The document's animations.
    pub(crate) animations: &'t RefCell<DocumentAnimations>,
    /// The owner of the animations.
    pub(crate) owner: AnimationOwner,
    /// The evaluated `@media` conditions of the stylist (for the
    /// `@keyframes` rules).
    pub(crate) media: &'t [bool],
    /// True if the cascade creates, updates and cancels the owner's
    /// records; false if it only reads them.
    pub(crate) sync: bool,
    /// The style change event of this cascade, if it is one: then the
    /// owner's transitions are started, reversed and cancelled; else (an
    /// animation tick, the style of a query) they are only read.
    pub(crate) change: Option<StyleChange>,
    /// True if the owner is an element with element children, or with
    /// pseudo-elements that can have transitions (a rule for `::before`,
    /// `::after`, `::marker` or `::placeholder` sets a `transition-*`
    /// longhand): styles that inherit from its style.
    pub(crate) children: bool,
    /// The element whose style the owner inherits from (`None`: the root).
    pub(crate) parent: Option<NodeId>,
}

/// The style that the after-change styles of the children of `node`
/// inherit from, if it is not `node`'s style (ADR 0044 section 20).
pub(crate) fn transition_parent(
    target: &AnimationTarget<'_>,
    node: NodeId,
) -> Option<Arc<ComputedStyle>> {
    target
        .animations
        .try_borrow()
        .ok()
        .and_then(|a| a.transition_parent(node))
}

/// True if the owner of `target` keeps the style that the after-change
/// styles of its children inherit from (ADR 0044 section 20): it has
/// children ([`AnimationTarget::children`]) and a style of the document can
/// start a transition of an inherited value.
pub(crate) fn keeps_parent_style(target: &AnimationTarget<'_>) -> bool {
    target.owner.pseudo.is_none()
        && target.children
        && target
            .animations
            .try_borrow()
            .is_ok_and(|a| a.inherited_transitions_seen())
}

/// True if the kept style of the owner of `target` counts every value, not
/// only the inherited ones: a style of the document that can start a
/// transition takes a non-inherited value with `inherit`.
pub(crate) fn keeps_every_value(target: &AnimationTarget<'_>) -> bool {
    target
        .animations
        .try_borrow()
        .is_ok_and(|a| a.reset_inherited_seen())
}

/// Keeps `style` as [`transition_parent`] of the owner of `target` (`None`:
/// its own style is it).
pub(crate) fn set_transition_parent(
    target: &AnimationTarget<'_>,
    style: Option<Arc<ComputedStyle>>,
) {
    if let Ok(mut animations) = target.animations.try_borrow_mut() {
        animations.set_transition_parent(target.owner.node, style);
    }
}

/// A style change event of one element or pseudo-element (CSS Transitions
/// 1 §3, ADR 0044 section 3).
pub(crate) struct StyleChange {
    /// The before-change style: the owner's style before the event; `None`
    /// for an element that had none (inserted, coming out of `display:
    /// none`).
    pub(crate) before: Option<Arc<ComputedStyle>>,
    /// True for the document element (its fix-ups).
    pub(crate) is_root: bool,
    /// The fix-ups that the owner's style goes through.
    pub(crate) fixup: Fixup,
}

/// Whether the style of an element or pseudo-element goes through the
/// fix-ups of [`super::apply_fixups`] (blockification, the border widths of
/// `border-style: none`): the after-change style of its transitions must
/// too.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Fixup {
    /// No fix-ups (`::marker`, `::placeholder`).
    None,
    /// The fix-ups, with the display of the parent box.
    Parent(Option<Display>),
}

/// One animation that has a value at its current time.
pub(crate) struct Effect {
    /// The keyframes of its `@keyframes` rule.
    set: Arc<KeyframeSet>,
    /// The iteration progress.
    progress: f64,
    /// The before flag of the easing.
    before: bool,
    /// The `animation-timing-function`: the default keyframe easing.
    easing: Easing,
}

/// The animated properties of an element and the effect of each.
pub(crate) struct AnimationInput {
    /// The style without the animations: the underlying values.
    base: Arc<ComputedStyle>,
    effects: Vec<Effect>,
    /// The animated longhands: (longhand, effect, property index in the
    /// effect's keyframe set), sorted by longhand.
    longhands: Vec<(LonghandId, usize, usize)>,
    /// The animated longhands as a set.
    pub(crate) set: LonghandSet,
    /// The animated custom properties: (name, effect, property index).
    custom: Vec<(Arc<str>, usize, usize)>,
    /// The values of the running transitions: they win over the
    /// animations.
    transitions: Vec<TransitionEffect>,
    /// The transitioning longhands: (longhand, index in `transitions`),
    /// sorted by longhand.
    transition_longhands: Vec<(LonghandId, usize)>,
}

impl AnimationInput {
    /// An input with no animation: only the transitions that
    /// [`AnimationInput::add_transitions`] adds, over the after-change
    /// style `base`.
    pub(crate) fn transitions_only(base: Arc<ComputedStyle>) -> AnimationInput {
        AnimationInput {
            base,
            effects: Vec::new(),
            longhands: Vec::new(),
            set: [0; LONGHAND_WORDS],
            custom: Vec::new(),
            transitions: Vec::new(),
            transition_longhands: Vec::new(),
        }
    }

    /// Adds the values of the running transitions (a later one of a
    /// property wins).
    pub(crate) fn add_transitions(&mut self, effects: Vec<TransitionEffect>) {
        let mut longhands: HashMap<LonghandId, usize> = HashMap::new();
        for (i, effect) in effects.iter().enumerate() {
            if let TransitionTarget::Longhand(id) = effect.target {
                longhands.insert(id, i);
                let index = id as usize;
                self.set[index / 64] |= 1 << (index % 64);
            }
        }
        let mut longhands: Vec<(LonghandId, usize)> = longhands.into_iter().collect();
        longhands.sort_by_key(|&(id, _)| id as usize);
        self.transition_longhands = longhands;
        self.transitions = effects;
    }

    /// True if a CSS animation drives the longhand `id` now.
    fn animates(&self, id: LonghandId) -> bool {
        self.longhands
            .binary_search_by_key(&(id as usize), |&(l, _, _)| l as usize)
            .is_ok()
    }

    /// The transition of the longhand `id`, if one runs.
    fn transition(&self, id: LonghandId) -> Option<&TransitionEffect> {
        let at = self
            .transition_longhands
            .binary_search_by_key(&(id as usize), |&(l, _)| l as usize)
            .ok()?;
        let &(_, effect) = self.transition_longhands.get(at)?;
        self.transitions.get(effect)
    }

    /// The effect and the property index of the animated longhand `id`.
    fn longhand(&self, id: LonghandId) -> Option<(&Effect, usize)> {
        let index = id as usize;
        if self.set[index / 64] & (1 << (index % 64)) == 0 {
            return None;
        }
        let at = self
            .longhands
            .binary_search_by_key(&index, |&(l, _, _)| l as usize)
            .ok()?;
        let &(_, effect, property) = self.longhands.get(at)?;
        Some((self.effects.get(effect)?, property))
    }
}

/// True if a pseudo-element with the style `style` has a box (and so
/// animations): `::before` and `::after` need a `content`.
fn pseudo_exists(owner: AnimationOwner, style: &ComputedStyle) -> bool {
    use crate::style_map::PseudoKind;
    match owner.pseudo {
        None => true,
        Some(PseudoKind::Before | PseudoKind::After) => {
            style.display != Display::None
                && !matches!(style.content, Content::Normal | Content::None)
        }
        Some(_) => style.display != Display::None,
    }
}

/// True if `target` with the base style `base` can have animations or
/// transitions: it has an animation name, a transition to start (in a
/// style change event) or records (the common case, none, costs one look
/// at the style, and at the index only in a document with animations).
pub(crate) fn may_animate(target: &AnimationTarget<'_>, base: &ComputedStyle) -> bool {
    if target.sync && AnimationStyle::has_names(base) {
        return true;
    }
    if TransitionStyle::may_transition(base) {
        if let Ok(mut animations) = target.animations.try_borrow_mut() {
            animations.see_transitions(
                || TransitionStyle::may_transition_inherited(base),
                base.inherits_reset,
            );
        }
        if target.change.is_some() {
            return true;
        }
    }
    target.animations.try_borrow().is_ok_and(|animations| {
        !animations.is_empty()
            && (!animations.ids_of(target.owner).is_empty()
                || animations.has_transitions(target.owner))
    })
}

/// The transitions of `target` with the after-change style `after`: in a
/// style change event, the rules of CSS Transitions 1 §3 run first
/// ([`DocumentAnimations::sync_transitions`]); then the values of its
/// running transitions now. `input` holds the CSS animations of the
/// element: the properties they drive start no transition. A
/// pseudo-element without a box has no transitions. An element without
/// running transitions whose after-change style equals its before-change
/// style has nothing to do: it costs one comparison and no allocation.
pub(crate) fn transition_effects(
    target: &AnimationTarget<'_>,
    after: &ComputedStyle,
    input: Option<&AnimationInput>,
) -> Vec<TransitionEffect> {
    let Ok(mut animations) = target.animations.try_borrow_mut() else {
        return Vec::new();
    };
    let running = animations.has_transitions(target.owner);
    if let Some(change) = &target.change
        && pseudo_exists(target.owner, after)
        && (change.before.is_some() || running)
    {
        // The before-change style went through the fix-ups of the element
        // (blockification, the border widths of `border-style: none`); the
        // after-change style must too, or their changes would show where
        // there are none.
        let fixed = match change.fixup {
            Fixup::Parent(layout_parent) if change.before.is_some() => {
                let mut style = after.clone();
                super::apply_fixups(&mut style, change.is_root, layout_parent);
                Some(style)
            }
            _ => None,
        };
        let after = fixed.as_ref().unwrap_or(after);
        if !running
            && change
                .before
                .as_deref()
                .is_some_and(|before| before == after)
        {
            animations.forget_completed(target.owner);
            return Vec::new();
        }
        let animated = |t: &TransitionTarget| match t {
            TransitionTarget::Longhand(id) => input.is_some_and(|i| i.animates(*id)),
            TransitionTarget::Custom(name) => {
                input.is_some_and(|i| i.custom.iter().any(|(n, _, _)| n == name))
            }
        };
        animations.sync_transitions(
            target.owner,
            change.before.as_deref(),
            after,
            target.children,
            &animated,
        );
    }
    if !animations.has_transitions(target.owner) {
        return Vec::new();
    }
    animations.transition_effects(target.owner)
}

/// The effects of the animations of `target` that have a value now, in
/// composite order: with `target.sync`, after its records were brought up
/// to date with the base style `base`.
fn effects(
    cx: &CascadeContext<'_>,
    target: &AnimationTarget<'_>,
    base: &ComputedStyle,
) -> Vec<Effect> {
    let stylist = cx.stylist;
    let mut lookup = |name: &str| stylist.keyframes(name, target.media, &cx.layer_ranks);
    let Ok(mut animations) = target.animations.try_borrow_mut() else {
        return Vec::new();
    };
    let list: Vec<_> = if target.sync {
        let exists = pseudo_exists(target.owner, base);
        let list = animations.sync(target.owner, base, exists, &mut lookup);
        if !target.children {
            animations.note_no_dependents(target.owner);
        }
        list
    } else {
        let ids = animations.ids_of(target.owner).to_vec();
        ids.into_iter()
            .filter_map(|id| {
                let record = animations.record(id)?;
                Some((id, lookup(&record.name)?))
            })
            .collect()
    };
    let time = animations.time();
    list.into_iter()
        .filter_map(|(id, set)| {
            let record = animations.record(id)?;
            let timing = record.computed_timing(time);
            Some(Effect {
                set,
                progress: timing.progress?,
                before: record.timing.keyframe_before_flag(&timing),
                easing: record.keyframe_easing.clone(),
            })
        })
        .collect()
}

/// The animation input of `target` for its base style `base`; `None` if no
/// animation has a value for a property now.
pub(crate) fn input(
    cx: &CascadeContext<'_>,
    target: &AnimationTarget<'_>,
    base: &ComputedStyle,
    important: &ImportantPhase<'_, '_>,
) -> Option<AnimationInput> {
    let effects = effects(cx, target, base);
    if effects.is_empty() {
        return None;
    }
    let important = important.collect();
    let mut longhands: HashMap<LonghandId, (usize, usize)> = HashMap::new();
    let mut custom: Vec<(Arc<str>, usize, usize)> = Vec::new();
    for (e, effect) in effects.iter().enumerate() {
        for (p, list) in effect.set.properties.iter().enumerate() {
            match &list.property {
                AnimatedProperty::Longhand(id) => {
                    let index = *id as usize;
                    if important.longhands[index / 64] & (1 << (index % 64)) == 0 {
                        longhands.insert(*id, (e, p));
                    }
                }
                AnimatedProperty::Custom(name) => {
                    if important.custom.iter().any(|n| *n == &**name) {
                        continue;
                    }
                    match custom.iter_mut().find(|(n, _, _)| n == name) {
                        Some(entry) => *entry = (Arc::clone(name), e, p),
                        None => custom.push((Arc::clone(name), e, p)),
                    }
                }
            }
        }
    }
    if longhands.is_empty() && custom.is_empty() {
        return None;
    }
    let mut longhands: Vec<(LonghandId, usize, usize)> = longhands
        .into_iter()
        .map(|(id, (e, p))| (id, e, p))
        .collect();
    longhands.sort_by_key(|&(id, _, _)| id as usize);
    let mut set = [0u64; LONGHAND_WORDS];
    for &(id, _, _) in &longhands {
        let index = id as usize;
        set[index / 64] |= 1 << (index % 64);
    }
    Some(AnimationInput {
        base: Arc::new(base.clone()),
        effects,
        longhands,
        set,
        custom,
        transitions: Vec::new(),
        transition_longhands: Vec::new(),
    })
}

/// The animated values of the custom properties of `input`, as
/// declarations that replace their winners: each keyframe value is
/// computed against the base style's custom properties (`var()`
/// substituted, a registered property computed by its syntax), then the
/// two are interpolated ([`crate::animation::interpolate_custom`], with the
/// base style's `color` for `currentcolor`).
pub(crate) fn custom_values(
    input: &AnimationInput,
    registered: Option<Registered<'_>>,
) -> Vec<(Arc<str>, CustomDeclaration)> {
    if input.custom.is_empty()
        && !input
            .transitions
            .iter()
            .any(|t| matches!(t.target, TransitionTarget::Custom(_)))
    {
        return Vec::new();
    }
    let base = input.base.custom_properties.as_ref();
    let blank;
    let map: &CustomProperties = if let Some(map) = base {
        map
    } else {
        blank = CustomProperties::new(registered.map(|r| r.registry));
        &blank
    };
    // The keyframe values of registered properties take the element's font
    // size and line height (`em`, `lh`).
    let lengths = registered.map(|r| {
        let mut lengths = *r.lengths;
        lengths.font_size = input.base.font_size;
        lengths.line_height = input.base.line_height_px();
        lengths
    });
    let registered = match (registered, lengths.as_ref()) {
        (Some(r), Some(lengths)) => Some(Registered { lengths, ..r }),
        _ => None,
    };
    let pool = CustomPool::default();
    let mut out = Vec::with_capacity(input.custom.len());
    for (name, e, p) in &input.custom {
        let Some(effect) = input.effects.get(*e) else {
            continue;
        };
        let Some(interval) =
            effect
                .set
                .interval(*p, effect.progress, &effect.easing, effect.before)
        else {
            continue;
        };
        let value_of = |k: Option<&Keyframe>| match k {
            Some(Keyframe {
                declaration: PropertyDeclaration::Custom(_, declaration),
                ..
            }) => compute_custom_properties(base, &[(name, declaration)], registered, &pool)
                .and_then(|m| m.get(name).cloned()),
            Some(_) => None,
            None => map.get(name).cloned(),
        };
        let from = value_of(interval.from);
        let to = value_of(interval.to);
        let value = crate::animation::interpolate_custom(
            map,
            name,
            from.as_ref(),
            to.as_ref(),
            input.base.color,
            interval.progress,
        );
        let declaration = match value {
            Some(tokens) => CustomDeclaration::Value(tokens),
            None => CustomDeclaration::CssWide(CssWideKeyword::Initial),
        };
        out.push((Arc::clone(name), declaration));
    }
    // The transitions of custom properties win over their animations.
    for effect in &input.transitions {
        let TransitionTarget::Custom(name) = &effect.target else {
            continue;
        };
        let value = crate::animation::interpolate_custom(
            map,
            name,
            effect.from.tokens(),
            effect.to.tokens(),
            input.base.color,
            effect.progress,
        );
        let declaration = match value {
            Some(tokens) => CustomDeclaration::Value(tokens),
            None => CustomDeclaration::CssWide(CssWideKeyword::Initial),
        };
        out.retain(|(n, _)| n != name);
        out.push((Arc::clone(name), declaration));
    }
    out
}

/// The winning custom declarations with the animated values of `values`
/// in place of their winners (and added where a property has none).
pub(crate) fn with_custom_values<'a>(
    winners: &'a [(&'a Arc<str>, &'a CustomDeclaration)],
    values: &'a [(Arc<str>, CustomDeclaration)],
) -> Cow<'a, [(&'a Arc<str>, &'a CustomDeclaration)]> {
    if values.is_empty() {
        return Cow::Borrowed(winners);
    }
    let mut list: Vec<(&Arc<str>, &CustomDeclaration)> = winners
        .iter()
        .copied()
        .filter(|(name, _)| !values.iter().any(|(n, _)| n == *name))
        .collect();
    list.extend(values.iter().map(|(name, value)| (name, value)));
    Cow::Owned(list)
}

/// Sets the animated value of the longhand `id` in `style`, if `input`
/// animates it: the two keyframes around the effect's progress, computed
/// with the element's context `ccx` (the implicit 0% and 100% keyframes
/// take the base style's value), interpolated at the eased progress.
pub(crate) fn animate_longhand(
    cx: &CascadeContext<'_>,
    input: &AnimationInput,
    id: LonghandId,
    ccx: &ComputeContext<'_>,
    style: &mut ComputedStyle,
    resolved: &mut ResolvedVariables,
) {
    if let Some(transition) = input.transition(id) {
        if let (TransitionValue::Longhand(from), TransitionValue::Longhand(to)) =
            (&transition.from, &transition.to)
        {
            interpolate_slots(id, from, to, transition.progress, style);
        }
        return;
    }
    let Some((effect, property)) = input.longhand(id) else {
        return;
    };
    let Some(interval) =
        effect
            .set
            .interval(property, effect.progress, &effect.easing, effect.before)
    else {
        return;
    };
    let mut value_of = |k: Option<&Keyframe>| {
        let mut s = style.clone();
        match k {
            Some(k) => apply_declaration(id, &k.declaration, ccx, &mut s, &cx.initial, resolved),
            None => id.copy_value(&input.base, &mut s),
        }
        s
    };
    let from = value_of(interval.from);
    let same = match (interval.from, interval.to) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        (None, None) => interval.progress == 0.0,
        _ => false,
    };
    let to = if same {
        from.clone()
    } else {
        value_of(interval.to)
    };
    interpolate_longhand(id, &from, &to, interval.progress, style);
}

/// The `animation-*` and `transition-*` longhands.
static ANIMATION_LONGHANDS: LazyLock<Vec<LonghandId>> = LazyLock::new(|| {
    LonghandId::ALL
        .iter()
        .copied()
        .filter(|id| {
            let name = id.name();
            name.starts_with("animation-") || name.starts_with("transition-")
        })
        .collect()
});

/// `--animations off` (ADR 0044 section 10): the `animation-*` and
/// `transition-*` longhands compute to their initial values, as if
/// `animation: none !important; transition: none !important` applied to
/// every element and pseudo-element at the highest author precedence.
pub(crate) fn reset_animation_properties(style: &mut ComputedStyle, initial: &ComputedStyle) {
    // Both groups are initial while their shared records are absent.
    if style.animations.is_none() && style.transitions.is_none() {
        return;
    }
    for &id in ANIMATION_LONGHANDS.iter() {
        id.copy_value(initial, style);
    }
}
