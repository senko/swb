//! `MutationObserver` and `MutationRecord` (M8 feature 9a, ADR 0028 section
//! 13): the cases of the Chromium probe `tools/probes/web-mutation-observer.json`
//! (the interface, the options and their errors, the record lists of the
//! DOM operations, form controls, transient registered observers, delivery
//! and its order against promises, records from the parser), each with and
//! without the GC stress mode, in virtual time; and tests of the lifetime of
//! observers and of the bounds.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "mutation_observer/cases.rs"]
mod cases;
mod common;

use common::{Departure, answer, load_in, on_big_stack, probe_failures_in};
use swb_web::ClockMode;

/// The values where swb departs from Chromium on purpose.
const SWB_VALUES: &[Departure] = &[
    (
        "mo-structure",
        36,
        r#""TypeError: Failed to execute 'observe' on 'MutationObserver': parameter 1 is not of type 'Node'.""#,
        "Attr is not a Node in swb (it extends EventTarget; ADR 0028 section 3), so it cannot be observed",
    ),
    (
        "mo-operations",
        8,
        r#"["attributes ; span#s1 ;  ;  ; null ; null ; style ;  ; ", "attributes ; span#s1 ;  ;  ; null ; null ; style ;  ; color: red;", "attributes ; span#s1 ;  ;  ; null ; null ; style ;  ; color: blue;", "attributes ; span#s1 ;  ;  ; null ; null ; style ;  ; color: green;"]"#,
        "Chromium synchronizes the style attribute lazily, so the old value of a change after a CSSOM write is the stale text (its last record names `color: blue;` where the attribute held `color: green;`); swb writes the attribute at each CSSOM write",
    ),
    (
        "mo-operations",
        15,
        r#"["childList ; div#a ;  ; span#s1 ; p#p1 ; null ;  ;  ; ", "childList ; div#a ; p#p1 ; p#p1 ; null ; null ;  ;  ; "]"#,
        "`node.replaceWith(node)` is one replace in swb (one record with the node added and removed); Chromium removes and inserts it in two records",
    ),
    (
        "mo-style",
        0,
        "[]",
        "`x` is a CSS property in Chromium (the SVG geometry property), so `style.setProperty('x', '1')` changes the declaration block and the `style` attribute; swb has no `x` property and changes nothing, so it makes no record (the records of valid properties are equal)",
    ),
];

#[test]
fn probe_cases() {
    let failures = probe_failures_in(cases::CASES, SWB_VALUES, ClockMode::Virtual);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const PAGE: &str = "<!DOCTYPE html><html><body><div id=a><p id=p1>x</p></div></body></html>";

fn eval(rt: &mut swb_web::ScriptRuntime, expression: &str) -> serde_json::Value {
    answer(rt, expression)
}

/// An observer that script no longer references keeps working while it has
/// registrations (the node's list holds it): its callback runs after a
/// collection.
#[test]
fn an_observed_observer_survives_a_collection() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load_in(PAGE, stress, ClockMode::Virtual);
            eval(
                &mut rt,
                "(function () { window.hits = 0; new MutationObserver(function () { window.hits++; }).observe(a, {childList: true}); })(), 1",
            );
            rt.collect();
            rt.collect();
            eval(&mut rt, "a.appendChild(document.createElement('i')), 1");
            rt.run_due_tasks(std::time::Duration::from_secs(1));
            assert_eq!(eval(&mut rt, "window.hits"), 1, "stress {stress}");
            eval(&mut rt, "a.appendChild(document.createElement('i')), 1");
            rt.collect();
            rt.run_due_tasks(std::time::Duration::from_secs(1));
            assert_eq!(eval(&mut rt, "window.hits"), 2, "stress {stress}");
        }
    });
}

/// An observer that was disconnected, or never observed, is collected when
/// script drops it.
#[test]
fn a_disconnected_observer_is_collected() {
    on_big_stack(|| {
        let mut rt = load_in(PAGE, false, ClockMode::Virtual);
        eval(
            &mut rt,
            "(function () { const o = new MutationObserver(function () {}); o.observe(a, {childList: true}); o.disconnect(); const p = new MutationObserver(function () {}); window.w = [new WeakRef(o), new WeakRef(p)]; })(), 1",
        );
        rt.collect();
        assert_eq!(
            eval(&mut rt, "window.w.map((r) => r.deref() === undefined)"),
            serde_json::json!([true, true])
        );
        // An observer with a registration stays.
        eval(
            &mut rt,
            "(function () { const o = new MutationObserver(function () {}); o.observe(a, {childList: true}); window.v = new WeakRef(o); })(), 1",
        );
        rt.collect();
        assert_eq!(eval(&mut rt, "window.v.deref() === undefined"), false);
    });
}

/// A callback that changes the tree again gets its next records in the same
/// checkpoint, again and again: the time limit of the task ends it.
#[test]
fn a_callback_that_mutates_without_end_is_stopped() {
    on_big_stack(|| {
        let mut rt = load_in(PAGE, false, ClockMode::Virtual);
        rt.set_time_limit(std::time::Duration::from_millis(300));
        eval(
            &mut rt,
            "(function () { new MutationObserver(function () { a.appendChild(document.createElement('i')); }).observe(a, {childList: true}); a.appendChild(document.createElement('b')); })(), 1",
        );
        rt.run_due_tasks(std::time::Duration::from_secs(5));
        // The page lives on: the evaluation works.
        assert_eq!(eval(&mut rt, "1 + 1"), 2);
    });
}

/// A termination in the callback of one observer does not lose the other
/// observers of the notify set: their records stay queued and the next
/// delivery gives them (DOM "notify mutation observers" takes the records
/// of each observer at its turn).
#[test]
fn a_termination_keeps_the_later_observers_notified() {
    on_big_stack(|| {
        let mut rt = load_in(PAGE, false, ClockMode::Virtual);
        rt.set_time_limit(std::time::Duration::from_millis(200));
        eval(
            &mut rt,
            "(function () { window.hits = 0; window.loops = 0; \
             new MutationObserver(function () { if (loops++ === 0) { for (;;) {} } }).observe(a, {childList: true}); \
             new MutationObserver(function (r) { window.hits += r.length; }).observe(a, {childList: true}); \
             a.appendChild(document.createElement('b')); })(), 1",
        );
        rt.run_due_tasks(std::time::Duration::from_secs(1));
        // The first callback was stopped; the second observer has its record.
        eval(&mut rt, "a.getAttribute('x'), 1");
        rt.run_due_tasks(std::time::Duration::from_secs(1));
        assert_eq!(eval(&mut rt, "window.loops"), 1);
        assert_eq!(eval(&mut rt, "window.hits"), 1);
    });
}

/// 100,000 children removed one by one under a subtree observer take
/// linear time (the transient registered observers are a set).
#[test]
fn removals_under_a_subtree_observer_are_linear() {
    on_big_stack(|| {
        let mut rt = load_in(PAGE, false, ClockMode::Virtual);
        rt.set_time_limit(std::time::Duration::from_secs(5));
        let started = std::time::Instant::now();
        eval(
            &mut rt,
            "(function () { const r = document.createElement('div'); document.body.appendChild(r); \
             for (let i = 0; i < 100000; i++) r.appendChild(document.createElement('i')); \
             new MutationObserver(function () {}).observe(r, {childList: true, subtree: true}); \
             while (r.firstChild) r.removeChild(r.firstChild); \
             window.n = 100000; })(), 1",
        );
        assert_eq!(eval(&mut rt, "window.n"), 100_000);
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    });
}
