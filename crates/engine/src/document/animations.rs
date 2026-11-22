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

use std::collections::{HashMap, HashSet};

use swb_dom::{Document, NodeId};
use swb_layout::FragmentRef;
use swb_style::animation::Reach;
use swb_style::animation::{AnimationOwner, AnimationRecord, PendingAnimationEvent};
use swb_style::{PseudoKind, StyleMap};
use swb_web::{
    AnimationCommand, AnimationInfo, DispatchTarget, EventInterface, KeyframeInfo, TrustedEvent,
};

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

/// The nearest ancestor of `node` (in the flat tree) that clips it in both
/// axes (`overflow` other than `visible`, or paint containment), for an
/// animation of [`Reach::Clip`]. `None` (the animation can paint anywhere)
/// when there is none, or when `node` or an ancestor below it is
/// absolutely positioned or fixed (its containing block can be outside the
/// clip).
fn clip_ancestor(dom: &Document, styles: &StyleMap, node: NodeId) -> Option<NodeId> {
    let escapes = |n: NodeId| {
        styles.get(n).is_some_and(|s| {
            matches!(
                s.position,
                swb_style::Position::Absolute | swb_style::Position::Fixed
            )
        })
    };
    if escapes(node) {
        return None;
    }
    let mut current = dom.flat_parent(node);
    while let Some(n) = current {
        let style = styles.get(n)?;
        if style.clips_x() && style.clips_y() {
            return Some(n);
        }
        if escapes(n) {
            return None;
        }
        current = dom.flat_parent(n);
    }
    None
}

/// The nearest inclusive ancestor of `node` (in the flat tree) in
/// `owners`, with the answers kept in `memo` for every node on the way.
fn nearest_owner(
    dom: &Document,
    owners: &HashSet<NodeId>,
    memo: &mut HashMap<NodeId, Option<NodeId>>,
    node: NodeId,
) -> Option<NodeId> {
    let mut path = Vec::new();
    let mut current = Some(node);
    let found = loop {
        let Some(n) = current else {
            break None;
        };
        if let Some(known) = memo.get(&n) {
            break *known;
        }
        if owners.contains(&n) {
            break Some(n);
        }
        path.push(n);
        current = dom.flat_parent(n);
    };
    for n in path {
        memo.insert(n, found);
    }
    found
}

/// Whether the nearest flat-tree ancestor of `node` (not `node` itself)
/// that has painted `bounds` meets `viewport`; false when no ancestor has
/// any. `memo` keeps the answer of every node on the way (for a node: its
/// nearest inclusive ancestor with bounds), so many owners under one
/// ancestor cost one walk.
fn ancestor_shows(
    dom: &Document,
    bounds: &HashMap<NodeId, swb_layout::Rect>,
    viewport: &swb_layout::Rect,
    memo: &mut HashMap<NodeId, bool>,
    node: NodeId,
) -> bool {
    let mut path = Vec::new();
    let mut current = dom.flat_parent(node);
    let shows = loop {
        let Some(a) = current else {
            break false;
        };
        if let Some(&known) = memo.get(&a) {
            break known;
        }
        path.push(a);
        if let Some(rect) = bounds.get(&a) {
            break rect.intersection(viewport).is_some();
        }
        current = dom.flat_parent(a);
    };
    for a in path {
        memo.insert(a, shows);
    }
    shows
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

    /// Takes the document's pending animation event queue for the step
    /// "update animations and send events" (Web Animations 1 §4.3.1; ADR
    /// 0044 section 6), stable-sorted by scheduled event time (unresolved
    /// first), then by composite order (the owners in tree order, an
    /// element's pseudo-elements after it, then the position in
    /// `animation-name`), as trusted `AnimationEvent`s at the owning
    /// elements (`bubbles` and `cancelable` true, measured in Chromium
    /// 148).
    pub(crate) fn take_animation_events(&mut self) -> Vec<(DispatchTarget, TrustedEvent)> {
        let mut events = self.animations.get_mut().take_events();
        // An owner that the arena freed gets nothing (a removed element
        // that still exists gets its `animationcancel`).
        events.retain(|e| self.dom.get(e.owner.node).is_some());
        if events.is_empty() {
            return Vec::new();
        }
        let order = self.tree_order();
        let key = |e: &PendingAnimationEvent| {
            (
                e.scheduled.is_some(),
                e.scheduled.unwrap_or(0.0),
                order.rank(e.owner.node),
                pseudo_rank(e.owner.pseudo),
                e.index,
            )
        };
        let mut keyed: Vec<_> = events.drain(..).map(|e| (key(&e), e)).collect();
        keyed.sort_by(|(a, _), (b, _)| {
            a.0.cmp(&b.0)
                .then(a.1.total_cmp(&b.1))
                .then(a.2.cmp(&b.2))
                .then(a.3.cmp(&b.3))
                .then(a.4.cmp(&b.4))
        });
        keyed
            .into_iter()
            .map(|(_, e)| {
                let mut event = TrustedEvent::new(e.kind.as_str());
                event.bubbles = true;
                event.cancelable = true;
                event.interface = EventInterface::Animation {
                    animation_name: e.name.to_string(),
                    elapsed_time: e.elapsed,
                    pseudo_element: pseudo_text(e.owner.pseudo).unwrap_or("").to_owned(),
                };
                (DispatchTarget::Node(e.owner.node), event)
            })
            .collect()
    }

    /// True if a style computation can start or cancel animations: the
    /// document has `@keyframes` rules (in its current stylist) and work
    /// for the style pass (styles dropped by DOM changes, stale styles, or
    /// DOM changes not yet taken). The rendering update then computes the
    /// styles, as a browser's frame does, so that the animations that a
    /// script starts by a style change run without a read of the styles.
    pub(crate) fn animation_styles_due(&self) -> bool {
        self.animations_enabled
            && !self.retired
            && self
                .stylist
                .as_ref()
                .is_some_and(swb_style::Stylist::has_keyframes)
            && (self.styles.is_none()
                || self.styles_stale
                || self.drained != Some(self.dom.generation())
                || !self.dom.journal().is_empty())
            && self.stylesheets_settled()
    }

    /// The visibility test of the frame loop (ADR 0044 section 8): after a
    /// display list, a scroll or a resize, finds the owners of running
    /// animations that can change visible pixels, by the [`Reach`] of
    /// their animations: the painted bounds of the owner's subtree, or of
    /// the nearest ancestor that clips it, intersected with the viewport
    /// (`swb_paint::animation_bounds`); the others need painted frames
    /// only at their event times. An owner without a box and without painted
    /// descendants (a child of an `<svg>`, an empty `display: contents`
    /// element) counts with the box of its nearest ancestor that has one; a
    /// `display: contents` owner has those of its children.
    pub(crate) fn update_animation_visibility(&mut self) {
        let key = (self.list_serial, self.scroll, self.view.viewport);
        if self.animation_visibility == Some(key) || self.animations.get_mut().is_empty() {
            return;
        }
        self.animation_visibility = Some(key);
        let requests = self.animations.get_mut().bounds_requests();
        if let Some(visible) = self.visible_owners(&requests) {
            let checked = requests.iter().map(|&(node, _)| node).collect();
            self.animations.get_mut().set_visible(checked, visible);
        }
    }

    /// The owners of `requests` whose animations can change visible pixels;
    /// `None` (all) without a layout or styles.
    fn visible_owners(&self, requests: &[(NodeId, Reach)]) -> Option<HashSet<NodeId>> {
        let (Some(fragments), Some(styles)) = (&self.fragments, &self.styles) else {
            return None;
        };
        let mut visible = HashSet::new();
        let mut subtree = HashSet::new();
        let mut clip_of: HashMap<NodeId, NodeId> = HashMap::new();
        for &(node, reach) in requests {
            match reach {
                Reach::Subtree => {
                    subtree.insert(node);
                }
                Reach::Clip => match clip_ancestor(&self.dom, styles, node) {
                    Some(clip) => {
                        clip_of.insert(node, clip);
                    }
                    None => {
                        visible.insert(node);
                    }
                },
                Reach::Everywhere => {
                    visible.insert(node);
                }
            }
        }
        if subtree.is_empty() && clip_of.is_empty() {
            return Some(visible);
        }
        let clips: HashSet<NodeId> = clip_of.values().copied().collect();
        let mut owner_of: HashMap<NodeId, Option<NodeId>> = HashMap::new();
        let dom = &self.dom;
        let bounds = swb_paint::animation_bounds(
            fragments,
            self.scrollers.offsets(),
            self.scroll,
            &mut |fragment, out| {
                let node = match fragment {
                    FragmentRef::Box(b) => b.node,
                    FragmentRef::Text(t) => Some(t.node),
                };
                let Some(node) = node else {
                    return;
                };
                if clips.contains(&node) {
                    out.push((node, false));
                }
                if !subtree.is_empty()
                    && let Some(owner) = nearest_owner(dom, &subtree, &mut owner_of, node)
                {
                    out.push((owner, true));
                }
            },
        );
        let viewport = swb_layout::Rect::new(
            self.scroll.x,
            self.scroll.y,
            self.view.viewport.width,
            self.view.viewport.height,
        );
        let shows = |key: NodeId| {
            bounds
                .get(&key)
                .is_some_and(|r| r.intersection(&viewport).is_some())
        };
        visible.extend(subtree.iter().copied().filter(|&n| shows(n)));
        visible.extend(clip_of.iter().filter(|&(_, &c)| shows(c)).map(|(&n, _)| n));
        // Owners without a box of their own that paint through an
        // ancestor's box (the descendants of an `<svg>`).
        let missing: Vec<NodeId> = subtree
            .iter()
            .copied()
            .filter(|n| !bounds.contains_key(n))
            .collect();
        if !missing.is_empty() {
            let through = self.painted_through_ancestor(&missing, &viewport);
            visible.extend(through);
        }
        Some(visible)
    }

    /// The owners of `missing` (no fragment of their own and no
    /// descendant's) that paint through the box of their nearest flat-tree
    /// ancestor that has one, and whose ancestor's painted bounds meet
    /// `viewport` (the children of an `<svg>` are painted by the svg box;
    /// the fragment tree has no boxes for them). An owner with no ancestor
    /// box (`display: none`) is not visible.
    fn painted_through_ancestor(
        &self,
        missing: &[NodeId],
        viewport: &swb_layout::Rect,
    ) -> Vec<NodeId> {
        let Some(fragments) = &self.fragments else {
            return missing.to_vec();
        };
        // Every flat-tree ancestor of the missing owners, each once: a walk
        // up stops at a node that an earlier walk added (its ancestors are
        // in the set already), so the cost is the size of the union, not
        // owners x depth.
        let mut candidates: HashSet<NodeId> = HashSet::new();
        for &n in missing {
            let mut current = self.dom.flat_parent(n);
            while let Some(a) = current {
                if !candidates.insert(a) {
                    break;
                }
                current = self.dom.flat_parent(a);
            }
        }
        let bounds = swb_paint::animation_bounds(
            fragments,
            self.scrollers.offsets(),
            self.scroll,
            &mut |fragment, out| {
                let node = match fragment {
                    FragmentRef::Box(b) => b.node,
                    FragmentRef::Text(t) => Some(t.node),
                };
                if let Some(node) = node
                    && candidates.contains(&node)
                {
                    out.push((node, true));
                }
            },
        );
        let mut memo = HashMap::new();
        missing
            .iter()
            .copied()
            .filter(|&n| ancestor_shows(&self.dom, &bounds, viewport, &mut memo, n))
            .collect()
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

    /// A command of a script for the animation `id`. False if it was
    /// refused (`finish()` of an animation whose end is infinite).
    pub(crate) fn animation_command(&mut self, id: u64, command: AnimationCommand) -> bool {
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
            AnimationCommand::SetStartTime(time) => {
                if time.is_none_or(f64::is_finite) {
                    animations.set_start_time(id, time);
                }
            }
            AnimationCommand::Cancel => animations.cancel(id),
            AnimationCommand::Finish => return animations.finish(id),
        }
        true
    }

    /// The play state and the pending flag of each animation of `ids`
    /// (`None` once gone).
    pub(crate) fn animation_states(&self, ids: &[u64]) -> Vec<Option<(&'static str, bool)>> {
        self.settle_animations();
        let animations = self.animations.borrow();
        let time = animations.time();
        ids.iter()
            .map(|&id| {
                let record = animations.record(id)?;
                Some((record.play_state(time).as_str(), record.pending()))
            })
            .collect()
    }

    /// The current time of the document timeline (ms from its zero: the
    /// time of the last update, or of the frame grid at or before now when
    /// a task asked for a catch-up that has not run) and the page time of
    /// its zero.
    pub(crate) fn timeline_time(&self) -> (f64, f64) {
        self.settle_animations();
        let animations = self.animations.borrow();
        (animations.current_timeline_time(), animations.origin())
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
