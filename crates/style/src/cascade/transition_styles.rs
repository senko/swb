//! The animation glue of the cascade: the inputs of a style change event
//! ([`TransitionInputs`], [`ChangeSource`]), the sweep of the animation
//! records after a pass ([`sweep_animations`]), and the computation of the
//! style of an element with animations and transitions
//! ([`compute_with_animations`], ADR 0044 sections 3 and 20).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use swb_dom::{ElementData, NodeId};

use super::{
    CascadeContext, ContainerBasis, ImportantPhase, Winners, animated, compute, drop_font_cycles,
};
use crate::ComputedStyle;
use crate::animation::transition_properties::TransitionStyle;
use crate::animation::{AnimationOwner, DocumentAnimations};
use crate::properties::LonghandId;
use crate::style_map::StyleMap;

/// The inputs of a style change event (CSS Transitions 1 §3, ADR 0044
/// sections 3 and 20): a style pass with them starts, reverses and cancels
/// the CSS transitions of the elements it styles, from their before-change
/// styles (their styles in the map that the pass starts from). A pass
/// without them (a check) only reads the transitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct TransitionInputs<'a> {
    /// Styles of single elements that queries of scripts computed after
    /// the map that the pass starts from: they are the newer before-change
    /// styles of those elements.
    pub queried: Option<&'a HashMap<NodeId, Arc<ComputedStyle>>>,
    /// True for a targeted pass for the animation ticks alone: it is a
    /// style change event only for the elements whose parent's style
    /// changed in the pass, and for the pseudo-elements of an element
    /// whose style changed (an inherited value can have changed; measured
    /// in Chromium 148: such a child starts its own transition one frame
    /// after its parent's). A full pass ignores it.
    pub inherited_only: bool,
}

/// Where a style pass finds the before-change styles of the elements it
/// styles.
pub(super) struct ChangeSource<'a> {
    /// The styles before the event: the previous map of a full restyle.
    pub(super) previous: Option<&'a StyleMap>,
    /// Newer styles of single elements (see [`TransitionInputs::queried`]).
    pub(super) queried: Option<&'a HashMap<NodeId, Arc<ComputedStyle>>>,
    /// The old style of the element that a targeted walk restyles now.
    pub(super) element: Option<Arc<ComputedStyle>>,
    /// The old styles of its pseudo-elements.
    pub(super) pseudos: Option<crate::style_map::PseudoStyles>,
    /// False in a pass of [`TransitionInputs::inherited_only`]: then only
    /// the owners with `parent_changed` have a style change event.
    pub(super) all: bool,
    /// In a targeted pass: the parent of the element that the walk styles
    /// now changed its style (for its pseudo-elements: the element or its
    /// parent did).
    pub(super) parent_changed: bool,
}

impl ChangeSource<'_> {
    /// The before-change style of `owner`.
    pub(super) fn before(&self, owner: AnimationOwner) -> Option<Arc<ComputedStyle>> {
        match owner.pseudo {
            None => self
                .queried
                .and_then(|q| q.get(&owner.node))
                .or(self.element.as_ref())
                .or_else(|| self.previous.and_then(|m| m.get(owner.node)))
                .cloned(),
            Some(kind) => self
                .pseudos
                .as_ref()
                .and_then(|p| p.get(kind as usize).cloned().flatten())
                .or_else(|| {
                    self.previous
                        .and_then(|m| m.pseudo(owner.node, kind))
                        .cloned()
                }),
        }
    }
}

/// Cancels the animation records whose owner has no style in `map` or no
/// animation name (it left the document, an ancestor is `display: none`, a
/// pseudo-element lost its box): CSS Animations 1 §3. (An element that is
/// `display: none` itself keeps only animations of `display`, which the
/// cascade decides.)
pub fn sweep_animations(map: &StyleMap, animations: &RefCell<DocumentAnimations>) {
    let Ok(mut animations) = animations.try_borrow_mut() else {
        return;
    };
    if animations.is_empty() {
        return;
    }
    let style_of = |owner: AnimationOwner| match owner.pseudo {
        None => map.get(owner.node),
        Some(kind) => map.pseudo(owner.node, kind),
    };
    animations.retain_owners(|owner| {
        style_of(owner).is_some_and(|s| crate::animation::AnimationStyle::has_names(s))
    });
    animations.retain_transitions(|owner| style_of(owner).cloned());
}

/// The end of [`cascade`]: the font cycles, the style from the winners,
/// and with an animation target the style with its animated values and
/// running transitions; it also keeps the style that the after-change
/// styles of the target's children inherit from (ADR 0044 section 20).
#[allow(clippy::too_many_arguments)]
pub(super) fn compute_with_animations(
    cx: &CascadeContext<'_>,
    mut winners: Winners<'_>,
    important: &ImportantPhase<'_, '_>,
    parent: &ComputedStyle,
    element: Option<&ElementData>,
    is_root: bool,
    units: ContainerBasis,
    target: Option<&animated::AnimationTarget<'_>>,
) -> ComputedStyle {
    drop_font_cycles(cx, &mut winners, is_root);
    let run = Recompute {
        cx,
        winners: &winners,
        element,
        is_root,
        units,
    };
    let base = run.style(parent, None);
    let Some(target) = target else {
        return base;
    };
    // The parent's style without the animations and transitions of it and
    // its ancestors, if that differs from its style.
    let parent_base = target
        .parent
        .and_then(|node| animated::transition_parent(target, node));
    let parent_base = parent_base.as_deref();
    if !animated::may_animate(target, &base) {
        let keeps = animated::keeps_parent_style(target);
        let every = animated::keeps_every_value(target);
        let kept = parent_base
            .filter(|_| keeps)
            .and_then(|p| unanimated_parent(run.style(p, None), &base, every));
        set_kept_style(target, kept);
        return base;
    }
    let input = animated::input(cx, target, &base, important);
    let keeps = animated::keeps_parent_style(target);
    let styles = TransitionStyles::compute(&run, target, parent, parent_base, base, input, keeps);
    let effects =
        animated::transition_effects(target, styles.after_change(), styles.input.as_ref());
    let TransitionStyles {
        after,
        unanimated,
        input,
        ..
    } = styles;
    let every = animated::keeps_every_value(target);
    if effects.is_empty() {
        let kept = unanimated
            .filter(|_| keeps)
            .and_then(|u| unanimated_parent(u, &after, every));
        set_kept_style(target, kept);
        return after;
    }
    let after = Arc::new(after);
    let mut input =
        input.unwrap_or_else(|| animated::AnimationInput::transitions_only(Arc::clone(&after)));
    input.add_transitions(effects);
    let style = run.style(parent, Some(&input));
    let kept = match unanimated {
        _ if !keeps => None,
        Some(unanimated) => unanimated_parent(unanimated, &style, every),
        None => kept_values_differ(&after, &style, every).then_some(after),
    };
    set_kept_style(target, kept);
    style
}

/// The inputs of [`compute`] for one element, to compute its style from
/// other parent styles and animation inputs.
struct Recompute<'r, 'w> {
    cx: &'r CascadeContext<'r>,
    winners: &'r Winners<'w>,
    element: Option<&'r ElementData>,
    is_root: bool,
    units: ContainerBasis,
}

impl Recompute<'_, '_> {
    /// The element's style as the child of `parent`, with the animated
    /// values of `input`.
    fn style(
        &self,
        parent: &ComputedStyle,
        input: Option<&animated::AnimationInput>,
    ) -> ComputedStyle {
        compute(
            self.cx,
            self.winners,
            parent,
            self.element,
            self.is_root,
            self.units,
            input,
        )
    }
}

/// The styles of an element with animations or transitions, before its
/// transitions apply.
struct TransitionStyles {
    /// The style with the values of its CSS animations: its style if no
    /// transition runs.
    after: ComputedStyle,
    /// Its style without animations and transitions, as a child of the
    /// parent's kept style (`None`: `after`, which has none). Computed only
    /// when the element keeps a style for its children or its transitions
    /// can follow an inherited value.
    unanimated: Option<ComputedStyle>,
    /// The after-change style of its transitions when it is not `after`
    /// or `unanimated`: with the animated values, as a child of the parent's
    /// kept style.
    change: Option<ComputedStyle>,
    /// True if `unanimated` is the after-change style (no animations).
    change_is_unanimated: bool,
    /// The animation input of its CSS animations.
    input: Option<animated::AnimationInput>,
}

impl TransitionStyles {
    /// The styles of the target of `run` with the base style `base` (no
    /// animations), the parent's style `parent` and its kept style
    /// `parent_base`. `keeps`: the element keeps a style for its children.
    fn compute(
        run: &Recompute<'_, '_>,
        target: &animated::AnimationTarget<'_>,
        parent: &ComputedStyle,
        parent_base: Option<&ComputedStyle>,
        base: ComputedStyle,
        input: Option<animated::AnimationInput>,
        keeps: bool,
    ) -> TransitionStyles {
        // A list that cannot name an inherited value does not need the
        // after-change style of the parent's kept style (ADR 0044 section
        // 20).
        let follows = parent_base.filter(|_| TransitionStyle::may_transition_inherited(&base));
        let own_base = parent_base
            .filter(|_| keeps || follows.is_some())
            .map(|p| run.style(p, None));
        let change = match (follows, &input) {
            (Some(parent_base), Some(input)) if target.change.is_some() => {
                Some(run.style(parent_base, Some(input)))
            }
            _ => None,
        };
        match &input {
            Some(i) => TransitionStyles {
                after: run.style(parent, Some(i)),
                unanimated: own_base.or(Some(base)),
                change,
                change_is_unanimated: false,
                input,
            },
            // Without animations the after-change style is the style as a
            // child of the parent's kept style, if the list follows it.
            None => TransitionStyles {
                after: base,
                change: None,
                change_is_unanimated: follows.is_some() && own_base.is_some(),
                unanimated: own_base,
                input,
            },
        }
    }

    /// The after-change style of the element's transitions.
    fn after_change(&self) -> &ComputedStyle {
        let unanimated = self
            .unanimated
            .as_ref()
            .filter(|_| self.change_is_unanimated);
        self.change.as_ref().or(unanimated).unwrap_or(&self.after)
    }
}

/// Keeps `kept` as the style that the after-change styles of the children
/// of the element of `target` inherit from (`None`: its style).
fn set_kept_style(target: &animated::AnimationTarget<'_>, kept: Option<Arc<ComputedStyle>>) {
    if target.owner.pseudo.is_none() {
        animated::set_transition_parent(target, kept);
    }
}

/// The style that the after-change styles of the children of an element
/// with the style `style` inherit from, if it is not `style`: its style
/// without animations and transitions, `unanimated`, when that differs
/// ([`kept_values_differ`]).
fn unanimated_parent(
    unanimated: ComputedStyle,
    style: &ComputedStyle,
    every: bool,
) -> Option<Arc<ComputedStyle>> {
    kept_values_differ(&unanimated, style, every).then(|| Arc::new(unanimated))
}

/// True if `a` and `b` differ in a value that children take: an inherited
/// value, or with `every` any value (a child can take a non-inherited
/// value with `inherit`, `background-color: inherit`).
fn kept_values_differ(a: &ComputedStyle, b: &ComputedStyle, every: bool) -> bool {
    if every {
        return a != b;
    }
    a.custom_properties != b.custom_properties
        || LonghandId::ALL
            .iter()
            .any(|&id| id.is_inherited() && !id.same_value(a, b))
}
