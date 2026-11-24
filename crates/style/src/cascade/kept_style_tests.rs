//! Tests of the style that an element keeps for the after-change styles of
//! its children (ADR 0044 section 20, `DocumentAnimations::transition_parent`):
//! the style sharing, the cleanup, the frames of headless runs, and the
//! sharing of elements whose transitions cannot follow an inherited value.

use std::sync::Arc;

use super::test_util::node;
use super::transition_tests::Fixture;

/// A parent whose color transitions, with element children.
const PARENT: &str = "#p { color: black; transition: color 1s linear } #p.on { color: white } ";

#[test]
fn siblings_that_share_a_style_keep_the_same_style_for_their_children() {
    let mut f = Fixture::new(
        "<div id=p><div class=c id=c1><span class=g id=g1></span></div>\
         <div class=c id=c2><span class=g id=g2></span></div></div>",
        &format!("{PARENT} .g {{ transition: color 1s linear }}"),
    );
    f.class("p", "on");
    f.tick_event(0.0);
    f.tick_event(100.0);
    assert!(Arc::ptr_eq(f.style("c1"), f.style("c2")), "shared");
    let (k1, k2) = (f.kept("c1").expect("kept"), f.kept("c2").expect("kept"));
    assert!(Arc::ptr_eq(&k1, &k2));
    // The kept style has the parent's value without its transition.
    assert_eq!(k1.color, f.kept("p").expect("kept").color);
    assert_ne!(k1.color, f.style("c1").color);
    // Each grandchild runs one transition, and keeps it.
    let (g1, g2) = (f.transition_ids("g1"), f.transition_ids("g2"));
    assert_eq!((g1.len(), g2.len()), (1, 1));
    for time in [116.0, 133.0, 150.0, 300.0] {
        f.tick_event(time);
    }
    assert_eq!(f.transition_ids("g1"), g1);
    assert_eq!(f.transition_ids("g2"), g2);
    let cancels = f
        .events()
        .iter()
        .filter(|e| e.0 == "transitioncancel")
        .count();
    assert_eq!(cancels, 0);
}

#[test]
fn a_shared_style_drops_a_stale_kept_style() {
    let mut f = Fixture::new(
        "<div class='p a' id=p1><span class=c id=c1></span></div>\
         <div class='p a' id=p2><span class=c id=c2></span></div>",
        "@keyframes k { from { color: blue } to { color: lime } } \
         .a { color: red; animation: k 10s linear } .c { transition: color 1s linear }",
    );
    f.tick_event(0.0);
    f.tick_event(100.0);
    assert!(f.kept("p1").is_some() && f.kept("p2").is_some());
    for id in ["p1", "p2"] {
        let n = node(&f.doc, id);
        f.doc.set_attr(n, "class", "p").expect("class");
    }
    f.pass(true);
    assert!(Arc::ptr_eq(f.style("p1"), f.style("p2")), "shared");
    assert!(f.kept("p1").is_none() && f.kept("p2").is_none());
}

#[test]
fn kept_styles_go_with_their_elements() {
    let mut f = Fixture::new(
        "<div id=p><div id=c></div></div><div id=q><div id=d></div></div>",
        "#p, #q { color: black; transition: color 1s linear } \
         #p.on, #q.on { color: white } #q.none { display: none } \
         #c, #d { transition: color 1s linear }",
    );
    f.class("p", "on");
    f.class("q", "on");
    assert!(f.kept("p").is_some() && f.kept("q").is_some());
    let (p, q) = (node(&f.doc, "p"), node(&f.doc, "q"));
    f.doc.remove(p);
    f.doc.set_attr(q, "class", "on none").expect("class");
    f.pass(true);
    let animations = f.animations.borrow();
    assert!(animations.transition_parent(p).is_none());
    assert!(animations.transition_parent(q).is_none());
    assert!(!animations.keeps_transition_parents());
}

#[test]
fn records_of_inherited_values_get_frames_until_the_value_changes() {
    // A slow transition: its 8-bit value changes first at about 100 ms.
    let mut f = Fixture::new(
        "<div id=p><div id=c></div></div>",
        "#p { color: black; transition: color 50s linear } #p.on { color: white } \
         #c { transition: color 1s linear }",
    );
    f.class("p", "on");
    for time in [0.0, 16.0, 50.0] {
        f.tick_event(time);
        f.events();
        // The value is still the start value: the frames go on.
        let next = f.animations.borrow().next_time(false).expect("a time");
        assert!(next < time + 20.0, "{time}: {next}");
    }
    f.tick_event(400.0);
    f.events();
    // Changed: no more frames; the next time is the end of the child's
    // transition (the full pass of `class` started it).
    let next = f.animations.borrow().next_time(false).expect("a time");
    assert!((next - 1000.0).abs() < 1e-6, "{next}");
}

#[test]
fn animations_of_owners_without_children_get_no_extra_frames() {
    let css = "@keyframes k { from { color: black } to { color: white } } \
               #p { animation: k 1s linear } .t { transition: color 1s linear }";
    // With an element child, the frames until the value changes.
    let mut f = Fixture::new("<div id=p><div class=t></div></div>", css);
    f.tick(0.0);
    f.events();
    let next = f.animations.borrow().next_time(false).expect("a time");
    assert!(next < 20.0, "{next}");
    // Without, only the end of the animation.
    let mut f = Fixture::new("<div id=p></div><div class=t></div>", css);
    f.tick(0.0);
    f.events();
    let next = f.animations.borrow().next_time(false).expect("a time");
    assert!((next - 1000.0).abs() < 1e-6, "{next}");
}

#[test]
fn transitions_of_other_properties_keep_the_style_sharing() {
    let mut f = Fixture::new(
        "<div id=p><div class=c id=c1></div><div class=c id=c2></div></div><div class=t></div>",
        &format!("{PARENT} .c {{ transition: opacity 1s }} .t {{ transition: color 1s }}"),
    );
    f.class("p", "on");
    f.tick_event(0.0);
    f.tick_event(100.0);
    assert!(f.kept("p").is_some());
    assert!(Arc::ptr_eq(f.style("c1"), f.style("c2")), "shared");
    assert!(f.transitions("c1").is_empty() && f.transitions("c2").is_empty());
}

#[test]
fn transitions_of_inherited_values_do_not_share_under_a_kept_style() {
    let mut f = Fixture::new(
        "<div id=p><div class=c id=c1></div><div class=c id=c2></div></div>",
        &format!("{PARENT} .c {{ transition: color 1s linear }}"),
    );
    f.class("p", "on");
    f.tick_event(0.0);
    f.tick_event(100.0);
    assert!(!Arc::ptr_eq(f.style("c1"), f.style("c2")));
    assert_eq!(f.transitions("c1").len(), 1);
    assert_eq!(f.transitions("c2").len(), 1);
}

#[test]
fn a_shared_style_is_taken_only_without_a_change_of_a_listed_value() {
    let mut f = Fixture::new(
        "<div class=c id=c1></div><div class='c x' id=c2></div><div class='c y' id=c3></div>",
        ".c { opacity: 1; transition: opacity 1s linear } .c.x { opacity: 0 } \
         .c.y { color: red }",
    );
    for id in ["c2", "c3"] {
        let n = node(&f.doc, id);
        f.doc.set_attr(n, "class", "c").expect("class");
    }
    f.pass(true);
    // `c2` changed its opacity: its own style and a transition.
    assert_eq!(f.transitions("c2").len(), 1);
    assert!(!Arc::ptr_eq(f.style("c1"), f.style("c2")));
    // `c3` changed a value that its list does not name: shared.
    assert_eq!(f.transitions("c3"), []);
    assert!(Arc::ptr_eq(f.style("c1"), f.style("c3")));
}

#[test]
fn a_running_transition_is_cancelled_when_its_property_leaves_the_list() {
    let mut f = Fixture::new(
        "<div id=a></div>",
        "#a { opacity: 1; transition: opacity 1s linear } #a.on { opacity: 0 } \
         #a.on.n { transition-property: color }",
    );
    f.class("a", "on");
    f.tick(0.0);
    f.tick(100.0);
    assert_eq!(f.transitions("a").len(), 1);
    f.events();
    f.class("a", "on n");
    assert_eq!(f.transitions("a"), []);
    assert_eq!(f.style("a").opacity, 0.0);
    f.tick(116.0);
    let kinds: Vec<&str> = f.events().iter().map(|e| e.0).collect();
    assert_eq!(kinds, ["transitioncancel"]);
}

#[test]
fn transitions_that_went_after_their_end_keep_their_end_time() {
    let mut f = Fixture::new(
        "<div id=a></div><div id=b></div>",
        "#a, #b { opacity: 1; transition: opacity 100ms linear } \
         #a.on, #b.on { opacity: 0 } #b.none { display: none }",
    );
    f.animations.borrow_mut().track_released();
    f.class("a", "on");
    f.class("b", "on");
    let (a, b) = (f.transition_ids("a"), f.transition_ids("b"));
    f.tick(0.0);
    f.tick(50.0);
    f.class("b", "on none");
    f.tick(200.0);
    f.tick(216.0);
    let mut animations = f.animations.borrow_mut();
    let released = animations.take_released();
    assert!(released.contains(&a[0]) && released.contains(&b[0]));
    let ended = animations.take_ended();
    // The finished one ends at its end time; the cancelled one has none.
    assert_eq!(ended.get(&a[0]), Some(&100.0));
    assert_eq!(ended.get(&b[0]), None);
}

/// A child that takes a non-inherited value with `inherit` follows a
/// parent whose value transitions: the parent keeps its style without the
/// transition (any value counts, not only the inherited ones), and the
/// child runs its own transition towards the parent's end value, once (the
/// delta review of feature 5, probe `r-inherit-kw`).
#[test]
fn a_child_that_inherits_a_non_inherited_value_runs_its_own_transition() {
    let mut f = Fixture::new(
        "<div id=p><div id=c></div></div>",
        "#p { background-color: black; transition: background-color 1s linear } \
         #p.on { background-color: white } \
         #c { background-color: inherit; transition: background-color 1s linear }",
    );
    assert!(f.style("c").inherits_reset);
    f.class("p", "on");
    for time in [0.0, 100.0, 116.0, 133.0, 300.0] {
        f.tick_event(time);
    }
    let kept = f
        .kept("p")
        .expect("the parent keeps its style without the transition");
    assert_ne!(kept.background_color, f.style("p").background_color);
    assert_eq!(
        f.transition_ids("c").len(),
        1,
        "the child runs its own transition"
    );
    let cancels = f
        .events()
        .iter()
        .filter(|e| e.0 == "transitioncancel")
        .count();
    assert_eq!(cancels, 0);
}
