//! The CSS animations of a document for scripts (ADR 0044 section 7, M13
//! feature 3b): `getAnimations()` in composite order, the state of one
//! animation, the commands of scripts (`play()`, `pause()`,
//! `currentTime`) and `getKeyframes()`.
//!
//! Composite order (CSS Animations 2 §2.2,
//! <https://drafts.csswg.org/css-animations-2/#animation-composite-order>):
//! the owning elements in tree order, each element's pseudo-elements after
//! it (`::marker`, `::before`, `::after`), and the animations of one owner
//! by their position in `animation-name`.

use swb_dom::NodeId;
use swb_style::PseudoKind;
use swb_style::animation::{AnimationOwner, AnimationRecord};
use swb_web::{AnimationCommand, AnimationInfo, KeyframeInfo};

use super::DocumentState;

/// The place of a pseudo-element after its element in composite order.
fn pseudo_rank(pseudo: Option<PseudoKind>) -> u8 {
    match pseudo {
        None => 0,
        Some(PseudoKind::Marker) => 1,
        Some(PseudoKind::Before) => 2,
        Some(PseudoKind::After) => 3,
        Some(_) => 4,
    }
}

/// The text of a pseudo-element for `KeyframeEffect.pseudoElement`.
fn pseudo_text(pseudo: Option<PseudoKind>) -> Option<&'static str> {
    match pseudo? {
        PseudoKind::Marker => Some("::marker"),
        PseudoKind::Before => Some("::before"),
        PseudoKind::After => Some("::after"),
        _ => None,
    }
}

/// The IDL attribute name of a CSS property (`background-color` is
/// `backgroundColor`, `float` is `cssFloat`; custom properties keep their
/// name). <https://drafts.csswg.org/web-animations-1/#animation-property-name-to-idl-attribute-name>
fn idl_name(property: &str) -> String {
    if property.starts_with("--") {
        return property.to_owned();
    }
    if property == "float" {
        return "cssFloat".to_owned();
    }
    let mut out = String::with_capacity(property.len());
    let mut upper = false;
    for c in property.chars() {
        if c == '-' {
            upper = !out.is_empty();
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// A keyframe of `getKeyframes()` while it is put together: the values
/// carry the order of their property.
struct Frame {
    offset: f64,
    easing: String,
    composite: &'static str,
    values: Vec<(usize, String, String)>,
}

/// Adds `value` to the frame of `offset` and `easing`, or to a new frame
/// with `composite`.
fn add_value(
    frames: &mut Vec<Frame>,
    offset: f64,
    easing: String,
    composite: &'static str,
    value: (usize, String, String),
) {
    match frames
        .iter_mut()
        .find(|f| f.offset == offset && f.easing == easing)
    {
        Some(frame) => frame.values.push(value),
        None => frames.push(Frame {
            offset,
            easing,
            composite,
            values: vec![value],
        }),
    }
}

impl DocumentState {
    /// Runs the catch-up of the animations' timeline that the page asked
    /// for (ADR 0044 section 2): before the styles or the state of the
    /// animations are read, not before each task.
    pub(crate) fn settle_animations(&self) {
        if let Ok(mut animations) = self.animations.try_borrow_mut() {
            animations.settle();
        }
    }

    /// `getAnimations()`: the relevant animations of the document, or of
    /// `element` (with `subtree`, of its inclusive descendants and their
    /// pseudo-elements), in composite order. Brings the styles up to date
    /// first (they create the animations).
    pub(crate) fn animation_list(&mut self, element: Option<NodeId>, subtree: bool) -> Vec<u64> {
        let outer = std::mem::replace(&mut self.in_query, true);
        self.update_style();
        self.in_query = outer;
        if self.animations.borrow().is_empty() {
            return Vec::new();
        }
        if let (Some(node), false) = (element, subtree) {
            // One element without its subtree: its own animations (the
            // pseudo-elements' belong to the subtree), read directly.
            let animations = self.animations.borrow();
            let time = animations.time();
            let owner = AnimationOwner { node, pseudo: None };
            return animations
                .ids_of(owner)
                .iter()
                .copied()
                .filter(|&id| animations.record(id).is_some_and(|r| r.is_relevant(time)))
                .collect();
        }
        let owners: Vec<AnimationOwner> = self.animations.borrow().owners().collect();
        let order = self.tree_order();
        let range = element.map(|e| (order.rank(e), order.end_of(e)));
        let mut keyed: Vec<(u32, u8, AnimationOwner)> = owners
            .into_iter()
            .filter_map(|owner| {
                let rank = order.rank(owner.node);
                if rank == u32::MAX {
                    return None;
                }
                let wanted = match range {
                    Some((start, end)) => rank >= start && rank < end,
                    None => true,
                };
                wanted.then_some((rank, pseudo_rank(owner.pseudo), owner))
            })
            .collect();
        keyed.sort_by_key(|&(rank, pseudo, _)| (rank, pseudo));
        let animations = self.animations.borrow();
        let time = animations.time();
        keyed
            .into_iter()
            .flat_map(|(_, _, owner)| animations.ids_of(owner).iter().copied())
            .filter(|&id| animations.record(id).is_some_and(|r| r.is_relevant(time)))
            .collect()
    }

    /// The state of the animation `id`.
    pub(crate) fn animation_info(&self, id: u64) -> Option<AnimationInfo> {
        self.settle_animations();
        let animations = self.animations.borrow();
        let time = animations.time();
        let record: &AnimationRecord = animations.record(id)?;
        Some(AnimationInfo {
            name: record.name.to_string(),
            target: record.owner.node,
            pseudo: pseudo_text(record.owner.pseudo),
            play_state: record.play_state(time).as_str(),
            pending: record.pending(),
            current_time: record.current_time(time),
            held: record.is_held(),
            start_time: record.start_time(),
            timing: record.timing.clone(),
            keyframe_easing: record.keyframe_easing.computed_text(),
            computed: record.computed_timing(time),
        })
    }

    /// A command of a script for the animation `id`.
    pub(crate) fn animation_command(&mut self, id: u64, command: AnimationCommand) {
        self.settle_animations();
        let animations = self.animations.get_mut();
        match command {
            AnimationCommand::Play => animations.play(id),
            AnimationCommand::Pause => animations.pause(id),
            AnimationCommand::SetCurrentTime(time) => {
                if time.is_finite() {
                    animations.set_current_time(id, time);
                }
            }
        }
    }

    /// `getKeyframes()` of the animation `id`: the keyframes of its
    /// `@keyframes` rule, those of one offset and easing together, with
    /// their specified values, the properties in the order of their first
    /// declaration. A property without a keyframe at 0% or 100% adds its
    /// computed value there (into the keyframe of that offset with the
    /// animation's easing, or a keyframe of its own whose `composite` is
    /// `replace`), as Chromium 148 lists them (`keyframes-conditions`).
    /// Departure: the value is the element's current one, which is the
    /// animated value for a property that the animation changes now.
    pub(crate) fn animation_keyframes(&mut self, id: u64) -> Vec<KeyframeInfo> {
        let Some((name, easing, owner)) = self
            .animations
            .borrow()
            .record(id)
            .map(|r| (r.name.clone(), r.keyframe_easing.computed_text(), r.owner))
        else {
            return Vec::new();
        };
        let env = self.media_environment();
        let Some(set) = self
            .stylist
            .as_ref()
            .and_then(|stylist| stylist.keyframes_in(&name, &env))
        else {
            return Vec::new();
        };
        let mut lists: Vec<&swb_style::animation::PropertyKeyframes> =
            set.properties.iter().collect();
        lists.sort_by_key(|l| l.order);
        let mut frames: Vec<Frame> = Vec::new();
        for list in &lists {
            for keyframe in &list.keyframes {
                let text = keyframe.easing.as_ref().map_or_else(
                    || easing.clone(),
                    swb_style::animation::Easing::computed_text,
                );
                let value = (list.order, idl_name(&list.name), keyframe.text.to_string());
                add_value(&mut frames, keyframe.offset, text, "auto", value);
            }
        }
        let pseudo = owner
            .pseudo
            .map(swb_style::cssom::resolved::PseudoElement::Styled);
        for list in &lists {
            for offset in [0.0, 1.0] {
                if list.keyframes.iter().any(|k| k.offset == offset) {
                    continue;
                }
                let computed = self.resolved_value(owner.node, pseudo, &list.name);
                let value = (list.order, idl_name(&list.name), computed);
                add_value(&mut frames, offset, easing.clone(), "replace", value);
            }
        }
        // Stable: the keyframes of one offset keep their order.
        frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        frames
            .into_iter()
            .map(|mut frame| {
                frame.values.sort_by_key(|v| v.0);
                KeyframeInfo {
                    offset: frame.offset,
                    easing: frame.easing,
                    composite: frame.composite,
                    values: frame.values.into_iter().map(|(_, n, v)| (n, v)).collect(),
                }
            })
            .collect()
    }

    /// The ids of the animations that went since the last call. The first
    /// call starts the bookkeeping: the bindings call it before they make a
    /// wrapper, and a document whose scripts never list animations keeps
    /// no ids.
    pub(crate) fn released_animations(&mut self) -> Vec<u64> {
        let animations = self.animations.get_mut();
        animations.track_released();
        animations.take_released()
    }
}

#[cfg(test)]
mod tests {
    use super::idl_name;

    #[test]
    fn idl_names_of_properties() {
        assert_eq!(idl_name("opacity"), "opacity");
        assert_eq!(idl_name("background-color"), "backgroundColor");
        assert_eq!(
            idl_name("-webkit-text-stroke-color"),
            "webkitTextStrokeColor"
        );
        assert_eq!(idl_name("float"), "cssFloat");
        assert_eq!(idl_name("--my-var"), "--my-var");
    }
}
