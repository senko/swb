//! Tests of CSS transitions in the cascade (CSS Transitions 1 §3, ADR 0044
//! section 3, M13 feature 5): the rules that start, reverse and cancel
//! transitions in a style change event, the values of running transitions,
//! the discrete cases, the events and the composite order. The probe rows
//! of `tools/probes/css-transitions.json` check the same against Chromium
//! (`crates/automation/tests/css_transitions.rs`).

use std::cell::RefCell;
use std::sync::Arc;

use swb_css::MediaEnvironment;
use swb_dom::{Document, parse_html};

use super::targeted::{Restyle, RestyleInputs, restyle_animated};
use super::test_util::{base, node, stylist};
use super::{TransitionInputs, compute_styles_animated};
use crate::ComputedStyle;
use crate::animation::{AnimationEventKind, AnimationId, AnimationOwner, DocumentAnimations};
use crate::container::ContainerSizes;
use crate::dom_changes::DomChanges;
use crate::element::ElementStates;
use crate::style_map::StyleMap;
use crate::stylist::Stylist;
use crate::values::Display;

/// A document with `css`, its stylist and animations, styled once (the
/// first styles: no style change event).
pub(super) struct Fixture {
    pub(super) doc: Document,
    stylist: Stylist,
    pub(super) animations: RefCell<DocumentAnimations>,
    map: StyleMap,
}

impl Fixture {
    pub(super) fn new(html: &str, css: &str) -> Self {
        let doc = parse_html(html);
        let stylist = stylist(&doc, css);
        let mut fixture = Fixture {
            doc,
            stylist,
            animations: RefCell::new(DocumentAnimations::new(0.0)),
            map: StyleMap::with_capacity(0),
        };
        fixture.pass(false);
        fixture
    }

    /// A full style pass from the current map; a style change event if
    /// `event`.
    pub(super) fn pass(&mut self, event: bool) {
        let previous = std::mem::replace(&mut self.map, StyleMap::with_capacity(0));
        self.map = compute_styles_animated(
            &self.doc,
            &self.stylist,
            &MediaEnvironment::default(),
            &ElementStates::default(),
            &base(),
            &ContainerSizes::new(),
            Some(&previous),
            &self.animations,
            event.then(TransitionInputs::default),
        );
    }

    /// Sets the `class` of `#id`, then runs a style change event.
    pub(super) fn class(&mut self, id: &str, class: &str) {
        let n = node(&self.doc, id);
        self.doc.set_attr(n, "class", class).expect("class");
        self.pass(true);
    }

    /// The update of a rendering update at `time`, then the restyle of the
    /// marked owners (no style change event).
    pub(super) fn tick(&mut self, time: f64) {
        self.restyle_tick(time, None);
    }

    /// [`Fixture::tick`] as the engine runs it once a style can start a
    /// transition: a style change event for the elements whose parent's
    /// style changed (`TransitionInputs::inherited_only`).
    pub(super) fn tick_event(&mut self, time: f64) {
        let inputs = TransitionInputs {
            queried: None,
            inherited_only: true,
        };
        self.restyle_tick(time, Some(inputs));
    }

    fn restyle_tick(&mut self, time: f64, transitions: Option<TransitionInputs<'_>>) {
        self.animations.borrow_mut().update(time);
        let marked = self.animations.borrow_mut().take_marked();
        let env = MediaEnvironment::default();
        let sizes = ContainerSizes::new();
        let inputs = RestyleInputs {
            doc: &self.doc,
            stylist: &self.stylist,
            env: &env,
            base_url: &base(),
            sizes: &sizes,
        };
        let states = ElementStates::default();
        let outcome = restyle_animated(
            &inputs,
            &states,
            &states,
            &DomChanges::default(),
            &marked,
            Some(&self.animations),
            transitions,
            &mut self.map,
        );
        assert!(matches!(outcome, Restyle::Done { .. }), "{outcome:?}");
    }

    pub(super) fn style(&self, id: &str) -> &Arc<ComputedStyle> {
        self.map.get(node(&self.doc, id)).expect("a style")
    }

    fn opacity(&self, id: &str) -> f32 {
        self.style(id).opacity
    }

    /// The style that `#id` keeps for the after-change styles of its
    /// children, if any.
    pub(super) fn kept(&self, id: &str) -> Option<Arc<ComputedStyle>> {
        self.animations
            .borrow()
            .transition_parent(node(&self.doc, id))
    }

    /// The ids of the running transitions of `#id`.
    pub(super) fn transition_ids(&self, id: &str) -> Vec<AnimationId> {
        let owner = AnimationOwner {
            node: node(&self.doc, id),
            pseudo: None,
        };
        self.animations.borrow().transition_ids_of(owner).to_vec()
    }

    /// The running transitions of `#id`: (property, delay, duration).
    pub(super) fn transitions(&self, id: &str) -> Vec<(String, f64, f64)> {
        let animations = self.animations.borrow();
        let owner = AnimationOwner {
            node: node(&self.doc, id),
            pseudo: None,
        };
        animations
            .transition_ids_of(owner)
            .iter()
            .filter_map(|&id| animations.record(id))
            .map(|r| (r.name.to_string(), r.timing.delay, r.timing.duration))
            .collect()
    }

    /// The queued events: (type, property, scheduled time).
    pub(super) fn events(&self) -> Vec<(&'static str, String, Option<f64>)> {
        self.animations
            .borrow_mut()
            .take_events()
            .into_iter()
            .map(|e| (e.kind.as_str(), e.name.to_string(), e.scheduled))
            .collect()
    }
}

const BOX: &str = "<div id=a></div>";

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn a_change_starts_a_transition_from_the_before_change_value() {
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: opacity 1s linear } #a.on { opacity: 0 }",
    );
    assert!(
        f.transitions("a").is_empty(),
        "the first styles start nothing"
    );
    f.class("a", "on");
    assert_eq!(f.transitions("a"), [("opacity".to_owned(), 0.0, 1000.0)]);
    // At the start (a pending play): the start value.
    assert_eq!(f.opacity("a"), 1.0);
    f.tick(100.0);
    f.tick(600.0);
    assert!(close(f.opacity("a"), 0.5), "{}", f.opacity("a"));
    // The end: the transition is done, the value is the after-change one.
    f.tick(1100.0);
    assert_eq!(f.opacity("a"), 0.0);
    assert_eq!(f.transitions("a"), []);
    let kinds: Vec<&str> = f.events().iter().map(|e| e.0).collect();
    assert_eq!(kinds, ["transitionrun", "transitionstart", "transitionend"]);
}

#[test]
fn no_transition_without_a_before_change_style_or_a_duration() {
    // `display: none` before the change: no before-change style.
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: opacity 1s; display: none } #a.on { opacity: 0; display: block }",
    );
    f.class("a", "on");
    assert_eq!(f.transitions("a"), []);
    // A combined duration of 0 (a negative delay that eats the duration).
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: opacity 1s -1s } #a.on { opacity: 0 }",
    );
    f.class("a", "on");
    assert_eq!(f.transitions("a"), []);
    assert_eq!(f.opacity("a"), 0.0);
    // A change that no item of `transition-property` names.
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: width 1s } #a.on { opacity: 0 }",
    );
    f.class("a", "on");
    assert_eq!(f.transitions("a"), []);
}

#[test]
fn reversing_shortens_the_duration_by_the_progress() {
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 0; transition: opacity 1000ms linear } #a.on { opacity: 1 }",
    );
    f.class("a", "on");
    f.tick(0.0);
    f.tick(400.0);
    assert!(close(f.opacity("a"), 0.4));
    // Back to the reversing-adjusted start value: a reversal from the
    // current value, 0.4 × 1000 ms (CSS Transitions 1 §3.1).
    f.class("a", "");
    assert_eq!(f.transitions("a"), [("opacity".to_owned(), 0.0, 400.0)]);
    assert!(close(f.opacity("a"), 0.4));
    f.tick(400.0);
    f.tick(600.0);
    assert!(close(f.opacity("a"), 0.2), "{}", f.opacity("a"));
    // Forth again, from 0.2: the factor is |0.5 × 0.4 + 1 − 0.4| = 0.8.
    f.class("a", "on");
    assert_eq!(f.transitions("a"), [("opacity".to_owned(), 0.0, 800.0)]);
}

#[test]
fn a_new_end_value_restarts_from_the_current_value() {
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: .2; transition: opacity 1000ms linear } #a.on { opacity: 1 } #a.other { opacity: .9 }",
    );
    f.class("a", "on");
    f.tick(0.0);
    f.tick(500.0);
    assert!(close(f.opacity("a"), 0.6));
    f.class("a", "other");
    // Not the reversing-adjusted start value: a new transition of the full
    // duration from the current value.
    assert_eq!(f.transitions("a"), [("opacity".to_owned(), 0.0, 1000.0)]);
    f.tick(500.0);
    f.tick(1000.0);
    assert!(close(f.opacity("a"), 0.75), "{}", f.opacity("a"));
}

#[test]
fn transitions_are_cancelled_by_their_properties_and_display_none() {
    let css = "#a { opacity: 1; transition: opacity 1s linear } #a.on { opacity: 0 } \
               #a.on.none { transition: none } #a.on.gone { display: none } \
               #a.on.other { transition: width 1s }";
    for (class, unresolved) in [("on none", false), ("on gone", true), ("on other", false)] {
        let mut f = Fixture::new(BOX, css);
        f.class("a", "on");
        f.tick(0.0);
        f.tick(100.0);
        f.events();
        f.class("a", class);
        assert_eq!(f.transitions("a"), [], "{class}");
        let events = f.events();
        assert_eq!(events.len(), 1, "{class}: {events:?}");
        assert_eq!(events[0].0, "transitioncancel");
        // A cancel by `display: none` has an unresolved time (it sorts
        // first, ADR 0044 section 6).
        assert_eq!(events[0].2.is_none(), unresolved, "{class}");
    }
}

#[test]
fn discrete_properties_need_allow_discrete() {
    let css = "#a { display: block; opacity: 1 } #a.off { display: none; opacity: 0 } \
               #a.d { transition: opacity 1s linear, display 1s allow-discrete } \
               #a.n { transition: opacity 1s linear, display 1s }";
    // Without `allow-discrete`, `display: none` comes at once and cancels.
    let mut f = Fixture::new("<div id=a class=n></div>", css);
    f.class("a", "n off");
    assert_eq!(f.transitions("a"), []);
    assert_eq!(f.style("a").display, Display::None);
    // With it, `display` stays `block` for the whole interval, and the
    // opacity transitions.
    let mut f = Fixture::new("<div id=a class=d></div>", css);
    f.class("a", "d off");
    let names: Vec<String> = f.transitions("a").into_iter().map(|t| t.0).collect();
    assert_eq!(names, ["display", "opacity"]);
    f.tick(0.0);
    f.tick(999.0);
    assert_eq!(f.style("a").display, Display::Block);
    f.tick(1000.0);
    assert_eq!(f.style("a").display, Display::None);
}

#[test]
fn unregistered_custom_properties_are_discrete() {
    let css = "#a { --x: 10px; transition: --x 1s linear } #a.on { --x: 30px } \
               #a.d { transition: --x 1s linear allow-discrete }";
    let mut f = Fixture::new(BOX, css);
    f.class("a", "on");
    assert_eq!(f.transitions("a"), []);
    let mut f = Fixture::new("<div id=a class=d></div>", css);
    f.class("a", "d on");
    assert_eq!(f.transitions("a").len(), 1);
    let value = |f: &Fixture| {
        f.style("a")
            .custom_properties
            .as_ref()
            .and_then(|m| m.get("--x"))
            .map(|v| swb_css::serialize_component_values(v))
    };
    f.tick(0.0);
    f.tick(400.0);
    assert_eq!(value(&f).as_deref(), Some("10px"));
    f.tick(600.0);
    assert_eq!(value(&f).as_deref(), Some("30px"));
}

#[test]
fn values_that_an_animation_drives_start_no_transition() {
    let css = "#a { opacity: 1; transition: opacity 1s } #a.on { animation: k 1s }
               @keyframes k { to { opacity: 0 } }";
    let mut f = Fixture::new(BOX, css);
    f.class("a", "on");
    assert_eq!(f.transitions("a"), []);
}

#[test]
fn composite_order_is_generation_then_property_name() {
    let css = "#a { opacity: 1; left: 0; color: red; position: relative; transition: all 1s } \
               #a.one { opacity: 0; left: 10px } #a.one.two { color: blue }";
    let mut f = Fixture::new(BOX, css);
    f.class("a", "one");
    f.class("a", "one two");
    let names: Vec<String> = f.transitions("a").into_iter().map(|t| t.0).collect();
    // `left` and `opacity` of the first event, then `color`.
    assert_eq!(names, ["left", "opacity", "color"]);
}

#[test]
fn events_have_the_interval_times_and_the_run_comes_at_the_start() {
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: opacity 80ms linear 40ms } #a.on { opacity: 0 }",
    );
    f.class("a", "on");
    f.tick(100.0);
    assert_eq!(
        f.events(),
        [("transitionrun", "opacity".to_owned(), Some(100.0))]
    );
    f.tick(150.0);
    assert_eq!(
        f.events(),
        [("transitionstart", "opacity".to_owned(), Some(140.0))]
    );
    f.tick(300.0);
    let events = f.animations.borrow_mut().take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, AnimationEventKind::TransitionEnd);
    assert!((events[0].elapsed - 0.08).abs() < 1e-12);
    assert_eq!(events[0].scheduled, Some(220.0));
}

#[test]
fn a_completed_transition_ends_at_its_end_value() {
    // A transition that ended after the last restyle: the before-change
    // value is its end value, not the value of its last tick.
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: opacity 100ms linear } #a.on { opacity: 0 }",
    );
    f.class("a", "on");
    f.tick(0.0);
    f.tick(50.0);
    assert!(close(f.opacity("a"), 0.5));
    // The update at the end, without a restyle after it.
    f.animations.borrow_mut().update(200.0);
    f.class("a", "on x");
    assert_eq!(f.transitions("a"), []);
    assert_eq!(f.opacity("a"), 0.0);
}

#[test]
fn a_transition_that_a_script_finished_ends_at_its_end_value() {
    // Probe `finish-then-style-change` (css-videojs-6.json): `finish()` of a
    // paused transition, then a style change before any restyle. The
    // before-change style still holds the old animated value (1), but the
    // finished transition's end value (0) is the one that counts: no new
    // transition from 1 to 0.
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: opacity 1s linear } #a.on { opacity: 0 }",
    );
    f.class("a", "on");
    let ids = f.transition_ids("a");
    assert_eq!(ids.len(), 1);
    {
        let mut animations = f.animations.borrow_mut();
        animations.pause(ids[0]);
        animations.set_current_time(ids[0], 950.0);
        assert!(animations.finish(ids[0]));
    }
    f.class("a", "on x");
    assert_eq!(f.transitions("a"), []);
    assert_eq!(f.opacity("a"), 0.0);
}

#[test]
fn a_finished_transition_starts_one_back_when_the_old_value_returns() {
    // Probe rows `finish-then-restore` and `seek-end-then-restore`
    // (css-videojs-6.json): the before-change style still holds the start
    // value (1), which equals the after-change value, but the finished
    // transition's end value (0) is the value before the change: Chromium
    // starts a transition from 0 to 1.
    for seek in [false, true] {
        let mut f = Fixture::new(
            BOX,
            "#a { opacity: 1; transition: opacity 1s linear } #a.on { opacity: 0 }",
        );
        f.class("a", "on");
        let ids = f.transition_ids("a");
        assert_eq!(ids.len(), 1);
        {
            let mut animations = f.animations.borrow_mut();
            if seek {
                animations.pause(ids[0]);
                animations.set_current_time(ids[0], 1000.0);
            } else {
                assert!(animations.finish(ids[0]));
            }
        }
        f.class("a", "");
        assert_eq!(
            f.transitions("a"),
            [("opacity".to_owned(), 0.0, 1000.0)],
            "seek {seek}"
        );
        assert_eq!(f.opacity("a"), 0.0, "seek {seek}");
    }
}

#[test]
fn the_sets_kept_for_a_second_run_go_with_their_element() {
    // `DocumentAnimations::repeated` holds a style per element: an element
    // that leaves the document keeps none.
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: opacity 1s linear } #a.on { opacity: 0 } \
         #a.x { margin-left: 10px }",
    );
    f.class("a", "on");
    f.tick(0.0);
    f.tick(2000.0);
    f.class("a", "on x");
    assert_eq!(f.animations.borrow().repeated_sets(), 1);
    f.animations.borrow_mut().retain_transitions(|_| None);
    assert_eq!(f.animations.borrow().repeated_sets(), 0);
}

#[test]
fn a_completed_transition_does_not_outlive_a_style_change_without_transitions() {
    // The page's overscroll spring: `transition: none` with a value that
    // changes (a pull), then `transition: opacity 600ms` with the old value
    // (the release), twice. The completed transition of the first release
    // ends at the pull, a style change event of an element whose
    // after-change style has no transitions; if its end value stayed, the
    // second release would see no change from it.
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1 } #a.pull { transition: none; opacity: 0.5 } \
         #a.rel { transition: opacity 600ms linear; opacity: 1 }",
    );
    for round in 0..2 {
        let base = f64::from(round) * 2000.0;
        f.tick(base + 0.0);
        f.class("a", "pull");
        f.tick(base + 16.0);
        f.tick(base + 200.0);
        f.class("a", "rel");
        assert_eq!(f.transitions("a").len(), 1, "round {round}");
        f.tick(base + 216.0);
        f.tick(base + 1000.0);
        assert_eq!(f.transitions("a"), [], "round {round}");
        f.tick(base + 1100.0);
    }
}

#[test]
fn a_change_of_the_used_color_scheme_starts_a_color_transition_of_equal_colors() {
    // Probe rows `color-scheme-*` of css-videojs-6.json: `light only` to
    // `dark` starts a transition of `color` (white to white) and of no
    // other property; `light` to `light dark` changes no used scheme.
    let css = "#a { color: #fff; background-color: #fff; transition: all 1s linear; \
               color-scheme: light only } #a.dark { color-scheme: dark } \
               #b { color: #fff; transition: all 1s linear; color-scheme: light } \
               #b.both { color-scheme: light dark }";
    let mut f = Fixture::new("<div id=a></div><div id=b></div>", css);
    f.class("a", "dark");
    assert_eq!(f.transitions("a"), [("color".to_owned(), 0.0, 1000.0)]);
    f.class("b", "both");
    assert_eq!(f.transitions("b"), []);
}

#[test]
fn a_repeat_of_a_style_change_event_sees_the_same_completed_transitions() {
    // The cascade of an element that uses container units runs twice in one
    // style change event, with the same before-change style. A transition
    // that a script finished between the styles is dropped by the first run;
    // the second must still start nothing from the stale animated value of
    // the before-change style.
    let mut f = Fixture::new(
        BOX,
        "#a { opacity: 1; transition: opacity 1s linear } #a.on { opacity: 0 }",
    );
    f.class("a", "on");
    let ids = f.transition_ids("a");
    f.tick(0.0);
    f.tick(100.0);
    {
        let mut animations = f.animations.borrow_mut();
        animations.pause(ids[0]);
        animations.set_current_time(ids[0], 950.0);
    }
    f.tick(116.0);
    let before = Arc::clone(f.style("a"));
    assert!(close(before.opacity, 0.05), "{}", before.opacity);
    f.animations.borrow_mut().finish(ids[0]);
    let mut after = (**f.style("a")).clone();
    after.opacity = 0.0;
    let owner = AnimationOwner {
        node: node(&f.doc, "a"),
        pseudo: None,
    };
    for _ in 0..2 {
        f.animations.borrow_mut().sync_transitions(
            owner,
            Some(&before),
            &after,
            false,
            swb_css::ColorScheme::Light,
            &|_| false,
        );
        assert_eq!(f.transitions("a"), []);
    }
}

#[test]
fn shorthands_in_transition_property_name_their_longhands() {
    let css = "#a { margin: 0; transition: margin 1s } #a.on { margin: 0 10px }";
    let mut f = Fixture::new(BOX, css);
    f.class("a", "on");
    let names: Vec<String> = f.transitions("a").into_iter().map(|t| t.0).collect();
    assert_eq!(names, ["margin-left", "margin-right"]);
}

/// Values other than the initial ones for most longhands.
const RICH: &str = "#a { color: rgb(10 20 30); color-scheme: dark; font-family: serif; \
    font-size: 21px; font-weight: 700; font-style: italic; font-stretch: 75%; \
    font-variant-caps: small-caps; font-variant-ligatures: none; \
    font-variant-numeric: tabular-nums; -webkit-tap-highlight-color: red; \
    text-size-adjust: 50%; -webkit-font-smoothing: antialiased; line-break: strict; \
    interpolate-size: allow-keywords; font-variation-settings: 'wght' 300; \
    font-feature-settings: 'liga' 0; line-height: 1.7; text-align: right; \
    text-indent: 7px; text-transform: uppercase; white-space-collapse: preserve; \
    letter-spacing: 2px; word-spacing: 3px; word-break: break-all; \
    overflow-wrap: anywhere; hyphens: auto; tab-size: 3; visibility: hidden; \
    list-style-type: square; list-style-position: inside; \
    list-style-image: url(a.png); cursor: pointer; border-collapse: collapse; \
    border-spacing: 3px 4px; caption-side: bottom; empty-cells: hide; \
    pointer-events: none; fill: red; fill-rule: evenodd; clip-rule: evenodd; \
    shape-rendering: crispEdges; fill-opacity: 0.5; stroke: blue; stroke-width: 3px; \
    stroke-linecap: round; stroke-linejoin: round; stroke-miterlimit: 7; \
    stroke-dasharray: 2 3; stroke-dashoffset: 4px; stroke-opacity: 0.3; \
    text-wrap-mode: nowrap; text-wrap-style: balance; -webkit-text-stroke-width: 2px; \
    -webkit-text-stroke-color: green; -webkit-text-fill-color: teal; \
    text-shadow: 1px 2px 3px red; display: flex; position: relative; float: left; \
    clear: both; top: 1px; right: 2px; bottom: 3px; left: 4px; z-index: 5; \
    transform: rotate(10deg); transform-origin: 1px 2px; clip: rect(1px 2px 3px 4px); \
    clip-path: circle(40%); width: 50px; height: 60px; min-width: 5px; \
    min-height: 6px; max-width: 500px; max-height: 600px; box-sizing: border-box; \
    aspect-ratio: 2 / 1; margin: 1px 2px 3px 4px; padding: 5px 6px 7px 8px; \
    border-width: 1px 2px 3px 4px; border-style: solid dashed dotted double; \
    border-color: red green blue yellow; border-radius: 1px 2px 3px 4px; \
    outline: 3px solid red; outline-offset: 2px; background-color: silver; \
    background-image: linear-gradient(red, blue); background-position: 10px 20px; \
    background-size: 30px 40px; background-repeat: no-repeat; \
    background-origin: content-box; background-clip: padding-box; \
    background-attachment: fixed; mask-image: linear-gradient(red, transparent); \
    mask-mode: luminance; mask-position: 3px 4px; mask-size: 5px 6px; \
    mask-repeat: round; mask-origin: padding-box; mask-clip: content-box; \
    mask-composite: exclude; overflow: hidden scroll; text-overflow: ellipsis; \
    opacity: 0.4; vertical-align: 3px; text-decoration: underline wavy red; \
    flex-direction: column; flex-wrap: wrap; flex-grow: 2; flex-shrink: 3; \
    flex-basis: 40px; order: 3; justify-content: center; align-items: center; \
    align-self: end; align-content: space-between; row-gap: 3px; column-gap: 4px; \
    column-width: 50px; column-count: 3; justify-items: center; justify-self: end; \
    grid-template-columns: 10px 1fr; grid-template-rows: 20px auto; \
    grid-template-areas: 'a b'; grid-auto-columns: 30px; grid-auto-rows: 40px; \
    grid-auto-flow: column dense; grid-row: 1 / 3; grid-column: 2 / 4; \
    table-layout: fixed; content: 'x'; counter-reset: c 2; counter-increment: c 3; \
    counter-set: c 4; contain: paint; container-type: inline-size; appearance: none; \
    field-sizing: content; content-visibility: auto; container-name: box; \
    contain-intrinsic-size: 10px 20px; object-fit: cover; mix-blend-mode: multiply; \
    resize: both; scrollbar-width: thin; object-position: 1px 2px; user-select: none; \
    filter: blur(2px); backdrop-filter: blur(3px); box-shadow: 1px 2px 3px red; \
    translate: 3px 4px; rotate: 30deg; scale: 2; isolation: isolate; \
    overflow-anchor: none; overscroll-behavior: contain; -webkit-line-clamp: 2; \
    -webkit-box-orient: vertical; corner-shape: bevel; touch-action: none; \
    perspective: 100px; perspective-origin: 3px 4px; scroll-margin: 1px 2px 3px 4px; \
    scroll-padding: 5px 6px 7px 8px; }";

#[test]
fn transition_values_round_trip_and_interpolate_as_whole_styles() {
    use crate::animation::interpolate::interpolate_longhand;
    use crate::animation::records::interpolate_slots;
    use crate::animation::transition_properties::is_transitionable_longhand;
    use crate::properties::LonghandId;

    let initial = ComputedStyle::initial();
    let f = Fixture::new(BOX, RICH);
    let rich = f.style("a");
    let ids: Vec<LonghandId> = LonghandId::ALL
        .iter()
        .copied()
        .filter(|&id| is_transitionable_longhand(id))
        .collect();
    let differ = ids
        .iter()
        .filter(|id| !id.same_value(&initial, rich))
        .count();
    assert!(differ >= 150, "{differ} of {} longhands differ", ids.len());
    for (a, b) in [(&**rich, &*initial), (&*initial, &**rich)] {
        for &id in &ids {
            // `put_value` of `value_of` gives the value back.
            let mut style = b.clone();
            id.put_value(&id.value_of(a), &mut style);
            assert!(id.same_value(a, &style), "{}", id.name());
            // The scratch styles hold what earlier calls left: the result
            // is that of the whole styles all the same.
            for progress in [0.0, 0.3, 1.0] {
                let mut whole = b.clone();
                interpolate_longhand(id, a, b, progress, &mut whole);
                let mut slots = b.clone();
                interpolate_slots(id, &id.value_of(a), &id.value_of(b), progress, &mut slots);
                assert!(id.same_value(&whole, &slots), "{} {progress}", id.name());
            }
        }
    }
}
