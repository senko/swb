//! Sub-feature 6c (ADR 0037 sections 4 to 6): close watchers and their
//! groups, light dismiss, the popover focusing steps and the restoration of
//! the focus, with the recording host of the parent tests.

use std::fmt::Write as _;

use super::*;

/// Four nested auto popovers, a manual one, two buttons and a hint.
const NEST: &str = "<div id=p1 popover>1<div id=p2 popover>2<div id=p3 popover>3<div id=p4 \
    popover>4</div></div></div></div><div id=mm popover=manual>m</div>\
    <button id=ob>o</button><button id=inv popovertarget=p1>i</button>\
    <div id=out>out</div><div id=hh popover=hint>h</div>";

/// Escape: the close request of the key path.
fn escape(h: &mut Rec) -> bool {
    process_close_watchers(h).unwrap()
}

fn counts(h: &Rec) -> (usize, usize) {
    h.doc.close_watcher_counts()
}

#[test]
fn watchers_made_without_an_activation_share_one_group() {
    let mut h = Rec::new(NEST);
    for id in ["mm", "p1", "p2"] {
        h.show(id).unwrap();
    }
    // The manual popover has no watcher.
    assert_eq!(counts(&h), (1, 2));
    let _ = h.take_log();
    assert!(escape(&mut h));
    // The newest first, with events; the manual popover stays.
    assert_eq!(
        h.take_log(),
        "bt:p2(open>closed) bt:p1(open>closed) toggle:p2(open>closed) toggle:p1(open>closed)"
    );
    assert_eq!(h.open(), "mm");
    assert_eq!(counts(&h), (0, 0));
    assert!(!escape(&mut h));
    invariants(&h.doc);
}

#[test]
fn an_activation_allows_a_new_group() {
    let mut h = Rec::new(NEST);
    h.show("p1").unwrap();
    h.doc.note_user_activation();
    h.show("p2").unwrap();
    assert_eq!(counts(&h), (2, 2));
    assert!(escape(&mut h));
    assert_eq!(h.open(), "p1");
    assert!(escape(&mut h));
    assert_eq!(h.open(), "");
}

#[test]
fn the_groups_follow_the_measured_sequences() {
    // `escape-popovers-groups-model`: an activation, two popovers, an
    // activation, two more: [p1] [p2] [p3, p4].
    let mut h = Rec::new(NEST);
    h.doc.note_user_activation();
    h.show("p1").unwrap();
    h.show("p2").unwrap();
    h.doc.note_user_activation();
    h.show("p3").unwrap();
    h.show("p4").unwrap();
    assert_eq!(counts(&h), (3, 4));
    assert!(escape(&mut h));
    assert_eq!(h.open(), "p1,p2");
    assert!(escape(&mut h));
    assert_eq!(h.open(), "p1");
    // Several activations without a watcher in between add one group only:
    // `escape-popovers-clicks-before-3`.
    let mut h = Rec::new(NEST);
    for _ in 0..4 {
        h.doc.note_user_activation();
    }
    for id in ["p1", "p2", "p3"] {
        h.show(id).unwrap();
    }
    assert_eq!(counts(&h), (2, 3));
    // Three activations with a watcher after each: three groups
    // (`escape-popovers-three-clicks`).
    let mut h = Rec::new(NEST);
    h.doc.note_user_activation();
    for id in ["p1", "p2", "p3"] {
        h.show(id).unwrap();
        h.doc.note_user_activation();
    }
    assert_eq!(counts(&h), (3, 3));
}

#[test]
fn an_activation_before_the_first_popover_counts() {
    let mut h = Rec::new(NEST);
    h.doc.note_user_activation();
    assert!(h.doc.top_layer().is_none());
    h.show("p1").unwrap();
    h.show("p2").unwrap();
    assert_eq!(counts(&h), (2, 2));
}

#[test]
fn the_history_action_activation_is_set_by_an_activation_and_consumed() {
    let mut h = Rec::new(NEST);
    assert!(!h.doc.has_history_action_activation());
    h.doc.note_user_activation();
    assert!(h.doc.has_history_action_activation());
    h.show("p1").unwrap();
    assert!(h.doc.has_history_action_activation());
    h.doc.consume_history_action_activation();
    assert!(!h.doc.has_history_action_activation());
}

#[test]
fn a_hidden_or_removed_popover_has_no_watcher() {
    let mut h = Rec::new(NEST);
    h.show("p1").unwrap();
    h.show("p2").unwrap();
    h.hide("p2").unwrap();
    assert_eq!(counts(&h), (1, 1));
    let p1 = h.id("p1");
    h.doc.remove(p1);
    assert_eq!(counts(&h), (0, 0));
    assert!(!escape(&mut h));
    // A change of the attribute to manual hides it and its watcher goes.
    let mut h = Rec::new(NEST);
    h.show("p1").unwrap();
    let p1 = h.id("p1");
    h.doc.set_attr(p1, "popover", "manual").unwrap();
    run_popover_attribute_steps(&mut h).unwrap();
    assert_eq!(counts(&h), (0, 0));
}

#[test]
fn a_request_goes_on_when_a_listener_removes_a_watcher() {
    let mut h = Rec::new(NEST);
    h.show("p1").unwrap();
    h.show("p2").unwrap();
    h.show("p3").unwrap();
    // The hide of `p3` removes `p2` in its event: its watcher is gone and the
    // request skips it.
    h.hook = Some(|h, element, new| {
        if new == ToggleState::Closed && h.name(element) == "p3" {
            let p2 = h.id("p2");
            h.doc.remove(p2);
        }
    });
    assert!(escape(&mut h));
    assert_eq!(counts(&h), (0, 0));
    assert_eq!(h.open(), "");
    invariants(&h.doc);
}

/// The popover that an invoker names, for the tests: the button `inv` names
/// `p1`.
fn target_of(h: &Rec) -> impl Fn(NodeId) -> Option<NodeId> + use<> {
    let (inv, p1) = (h.id("inv"), h.id("p1"));
    move |n| (n == inv).then_some(p1)
}

/// A press and a release on `down` and `up`: light dismiss.
fn click(h: &mut Rec, down: &str, up: &str) {
    let (d, u) = (h.id(down), h.id(up));
    let target = target_of(h);
    light_dismiss_down(&mut h.doc, d, &target);
    light_dismiss_up(h, u, &target).unwrap();
}

#[test]
fn a_click_outside_closes_the_auto_popovers_and_keeps_the_manual_one() {
    let mut h = Rec::new(NEST);
    for id in ["mm", "p1", "p2"] {
        h.show(id).unwrap();
    }
    let _ = h.take_log();
    click(&mut h, "out", "out");
    assert_eq!(
        h.take_log(),
        "bt:p2(open>closed) bt:p1(open>closed) toggle:p2(open>closed) toggle:p1(open>closed)"
    );
    assert_eq!(h.open(), "mm");
    // No popover moved the focus.
    assert_eq!(h.focus, None);
    assert!(h.doc.popover_pointerdown_target().is_none());
}

#[test]
fn a_click_inside_closes_the_popovers_above_it_only() {
    let mut h = Rec::new(NEST);
    for id in ["p1", "p2", "p3"] {
        h.show(id).unwrap();
    }
    click(&mut h, "p2", "p2");
    assert_eq!(h.open(), "p1,p2");
    // The target of the click may be a descendant of the popover.
    let p2 = h.id("p2");
    let inner = h.doc.first_element_child(p2).unwrap_or(p2);
    let target = |_| None;
    light_dismiss_down(&mut h.doc, inner, target);
    light_dismiss_up(&mut h, inner, target).unwrap();
    assert_eq!(h.open(), "p1,p2");
    // A manual popover is not a popover for the walk: a click on it closes
    // the autos.
    h.show("mm").unwrap();
    click(&mut h, "mm", "mm");
    assert_eq!(h.open(), "mm");
}

#[test]
fn a_press_and_a_release_on_different_popovers_close_nothing() {
    let mut h = Rec::new(NEST);
    for id in ["p1", "p2"] {
        h.show(id).unwrap();
    }
    click(&mut h, "out", "p2");
    assert_eq!(h.open(), "p1,p2");
    click(&mut h, "p2", "out");
    assert_eq!(h.open(), "p1,p2");
    // The stored target is gone after a release.
    assert!(h.doc.popover_pointerdown_target().is_none());
}

#[test]
fn a_popover_that_the_press_opens_is_closed_by_the_release() {
    let mut h = Rec::new(NEST);
    let (out, target) = (h.id("out"), |_| None);
    // Nothing shows at the press: no target.
    light_dismiss_down(&mut h.doc, out, target);
    h.show("p1").unwrap();
    light_dismiss_up(&mut h, out, target).unwrap();
    assert_eq!(h.open(), "");
}

#[test]
fn the_invoker_counts_for_its_showing_popover() {
    let mut h = Rec::new(NEST);
    for id in ["p1", "p2"] {
        h.show(id).unwrap();
    }
    // A click on the invoker of `p1`: `p2` closes, `p1` stays.
    click(&mut h, "inv", "inv");
    assert_eq!(h.open(), "p1");
    // The invoker of a manual popover counts for nothing.
    let mut h = Rec::new(NEST);
    h.show("p1").unwrap();
    let (inv, mm) = (h.id("inv"), h.id("mm"));
    h.show("mm").unwrap();
    let target = move |n: NodeId| (n == inv).then_some(mm);
    light_dismiss_down(&mut h.doc, inv, target);
    light_dismiss_up(&mut h, inv, target).unwrap();
    assert_eq!(h.open(), "mm");
}

#[test]
fn light_dismiss_with_hints() {
    // A click in an unrelated hint closes nothing; in an auto popover it
    // closes the hints; outside it closes the hints and then the autos.
    let mut h = Rec::new(NEST);
    h.show("p1").unwrap();
    h.show("hh").unwrap();
    assert_eq!(h.open(), "hh,p1");
    click(&mut h, "hh", "hh");
    assert_eq!(h.open(), "hh,p1");
    click(&mut h, "p1", "p1");
    assert_eq!(h.open(), "p1");
    h.show("hh").unwrap();
    let _ = h.take_log();
    click(&mut h, "out", "out");
    assert_eq!(h.open(), "");
    assert!(
        h.take_log()
            .starts_with("bt:hh(open>closed) bt:p1(open>closed)")
    );
    // With the hint list alone: a click in the hint stays, outside closes.
    h.show("hh").unwrap();
    click(&mut h, "hh", "hh");
    assert_eq!(h.open(), "hh");
    click(&mut h, "out", "out");
    assert_eq!(h.open(), "");
}

#[test]
fn hide_all_popovers_until_a_hint_leaves_the_auto_list() {
    let mut h = Rec::new(NEST);
    h.show("p1").unwrap();
    h.show("hh").unwrap();
    let hh = h.id("hh");
    hide_all_popovers_until(&mut h, Some(hh), true).unwrap();
    assert_eq!(h.open(), "hh,p1");
}

#[test]
fn a_shown_popover_focuses_its_autofocus_element() {
    let mut h = Rec::new(
        "<button id=ob>o</button><div id=pa popover><button id=pb autofocus>x</button></div>\
         <div id=pc popover autofocus tabindex=-1>c</div><div id=pd popover><input id=pi></div>",
    );
    let ob = h.id("ob");
    h.focus = Some(ob);
    h.show("pa").unwrap();
    assert_eq!(h.focus, Some(h.id("pb")));
    // The popover itself with `autofocus`.
    h.show("pc").unwrap();
    assert_eq!(h.focus, Some(h.id("pc")));
    // No `autofocus`: the focus does not move.
    h.focus = Some(ob);
    h.show("pd").unwrap();
    assert_eq!(h.focus, Some(ob));
    // An `autofocus` element that cannot take the focus: the next one.
    let mut h = Rec::new(
        "<div id=pa popover><button id=b1 autofocus>1</button><button id=b2 autofocus>2</button></div>",
    );
    h.unfocusable.push(h.id("b1"));
    h.show("pa").unwrap();
    assert_eq!(h.focus, Some(h.id("b2")));
}

#[test]
fn the_focus_goes_back_when_the_popover_that_opened_the_stack_hides() {
    let mut h = Rec::new(NEST);
    let (ob, p1i) = (h.id("ob"), h.id("p1"));
    h.focus = Some(ob);
    h.show("p1").unwrap();
    h.show("p2").unwrap();
    // The focus is inside the popovers; the outer one remembers `ob`.
    h.focus = Some(h.id("p2"));
    let _ = (p1i, h.take_log());
    h.hide("p1").unwrap();
    assert_eq!(h.focus, Some(ob));
    assert!(h.take_log().contains("focus:ob"));
}

#[test]
fn the_focus_stays_when_it_is_not_inside_or_nothing_was_recorded() {
    let mut h = Rec::new(NEST);
    let ob = h.id("ob");
    // The focus is outside the popover at the hide.
    h.focus = Some(ob);
    h.show("p1").unwrap();
    let other = h.id("inv");
    h.focus = Some(other);
    h.hide("p1").unwrap();
    assert_eq!(h.focus, Some(other));
    // A manual popover records nothing.
    h.focus = Some(ob);
    h.show("mm").unwrap();
    h.focus = Some(h.id("mm"));
    h.hide("mm").unwrap();
    assert_eq!(h.focus, Some(h.id("mm")));
    // A nested popover records nothing; the outer one does.
    h.focus = Some(ob);
    h.show("p1").unwrap();
    h.show("p2").unwrap();
    h.focus = Some(h.id("p2"));
    h.hide("p2").unwrap();
    assert_eq!(h.focus, Some(h.id("p2")));
    h.hide("p1").unwrap();
    assert_eq!(h.focus, Some(ob));
    // A hint shown while an auto popover shows records nothing.
    h.show("p1").unwrap();
    h.show("hh").unwrap();
    h.focus = Some(h.id("hh"));
    h.hide("hh").unwrap();
    assert_eq!(h.focus, Some(h.id("hh")));
    // The element that had the focus cannot take it any more.
    let mut h = Rec::new(NEST);
    let ob = h.id("ob");
    h.focus = Some(ob);
    h.show("p1").unwrap();
    h.focus = Some(h.id("p1"));
    h.unfocusable.push(ob);
    h.hide("p1").unwrap();
    assert_eq!(h.focus, Some(h.id("p1")));
}

#[test]
fn a_close_request_and_the_focus_go_together() {
    let mut h = Rec::new(NEST);
    let ob = h.id("ob");
    h.focus = Some(ob);
    h.show("p1").unwrap();
    h.focus = Some(h.id("p1"));
    assert!(escape(&mut h));
    assert_eq!(h.focus, Some(ob));
    // The removing steps and light dismiss do not move the focus.
    h.show("p1").unwrap();
    h.focus = Some(h.id("p1"));
    click(&mut h, "out", "out");
    assert_eq!(h.focus, Some(h.id("p1")));
}

#[test]
fn a_manual_popover_keeps_its_invoker_after_its_toggle_task() {
    let mut h = Rec::new(NEST);
    let (mm, inv) = (h.id("mm"), h.id("inv"));
    show_popover(&mut h, mm, true, Some(inv)).unwrap();
    let _ = h.take_log();
    assert_eq!(h.doc.popover_invoker(mm), Some(inv));
    h.hide("mm").unwrap();
    assert_eq!(h.doc.popover_invoker(mm), None);
    let _ = h.take_log();
    assert!(h.doc.top_layer().unwrap().popovers.is_empty());
}

#[test]
fn ten_thousand_watchers_close_in_linear_time() {
    // Nested popovers by `source`: one group, closed top first.
    let mut html = String::from("<button id=b>b</button>");
    for i in 0..10_000 {
        write!(html, "<div id=s{i} popover></div>").unwrap();
    }
    let mut h = Rec::new(&html);
    let b = h.id("b");
    let mut previous = b;
    for i in 0..10_000 {
        let el = h.id(&format!("s{i}"));
        show_popover(&mut h, el, true, Some(previous)).unwrap();
        previous = el;
    }
    assert_eq!(counts(&h), (1, 10_000));
    let started = std::time::Instant::now();
    assert!(escape(&mut h));
    assert_eq!(counts(&h), (0, 0));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn activations_between_two_watchers_add_one_group() {
    // `escape-popovers-groups-model`: a second activation before a watcher
    // exists adds nothing.
    let mut h = Rec::new(NEST);
    h.show("p1").unwrap();
    for _ in 0..4 {
        h.doc.note_user_activation();
    }
    h.show("p2").unwrap();
    h.show("p3").unwrap();
    assert_eq!(counts(&h), (2, 3));
}

#[test]
fn the_higher_of_the_clicked_and_the_invoked_popover_is_the_endpoint() {
    // A click on the invoker of `p1` that lies inside `p2`: `p2` is higher,
    // so `p3` closes and `p2` stays.
    let mut h = Rec::new(
        "<div id=p1 popover>1<div id=p2 popover>2<button id=inv popovertarget=p1>i</button>\
         <div id=p3 popover>3</div></div></div><div id=out>o</div>",
    );
    for id in ["p1", "p2", "p3"] {
        h.show(id).unwrap();
    }
    let (inv, p1) = (h.id("inv"), h.id("p1"));
    let target = move |n: NodeId| (n == inv).then_some(p1);
    light_dismiss_down(&mut h.doc, inv, target);
    light_dismiss_up(&mut h, inv, target).unwrap();
    assert_eq!(h.open(), "p1,p2");
}
