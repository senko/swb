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

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use super::{
    CascadeContext, ImportantPhase, LONGHAND_WORDS, LonghandSet, ResolvedVariables,
    apply_declaration,
};
use crate::ComputedStyle;
use crate::animation::interpolate::interpolate_longhand;
use crate::animation::keyframes::{AnimatedProperty, Keyframe};
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
    base: ComputedStyle,
    effects: Vec<Effect>,
    /// The animated longhands: (longhand, effect, property index in the
    /// effect's keyframe set), sorted by longhand.
    longhands: Vec<(LonghandId, usize, usize)>,
    /// The animated longhands as a set.
    pub(crate) set: LonghandSet,
    /// The animated custom properties: (name, effect, property index).
    custom: Vec<(Arc<str>, usize, usize)>,
}

impl AnimationInput {
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

/// True if `target` with the base style `base` can have animations: it has
/// an animation name or records (the common case, neither, costs one look
/// at the style, and at the index only in a document with animations).
pub(crate) fn may_animate(target: &AnimationTarget<'_>, base: &ComputedStyle) -> bool {
    if target.sync && AnimationStyle::has_names(base) {
        return true;
    }
    target.animations.try_borrow().is_ok_and(|animations| {
        !animations.is_empty() && !animations.ids_of(target.owner).is_empty()
    })
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
        animations.sync(target.owner, base, exists, &mut lookup)
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
        base: base.clone(),
        effects,
        longhands,
        set,
        custom,
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
    if input.custom.is_empty() {
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
    if style.animations.is_none() && style.inert.is_none() {
        return;
    }
    for &id in ANIMATION_LONGHANDS.iter() {
        id.copy_value(initial, style);
    }
}
