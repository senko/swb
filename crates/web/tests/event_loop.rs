//! The event loop of feature 5b (HTML §8.1.7, §8.7, §8.8; ADR 0028
//! section 7): the cases of the Chromium probe
//! `tools/probes/web-event-loop.json` (the order of tasks, microtasks and
//! timers, the nesting clamp, timer arguments and ids, `error` and
//! `unhandledrejection` events, the checkpoint after callbacks, the clock),
//! each with and without the GC stress mode, in virtual time; the console
//! lines of canceled and not canceled reports; the limits (the budget of a
//! document, the heap limit, the timer limit); finalization cleanups.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "event_loop/cases.rs"]
mod cases;
mod common;

use std::time::{Duration, Instant};

use common::{Departure, answer, expected, load_in, on_big_stack, probe_failures_in};
use swb_web::{ClockMode, ConsoleLevel, ScriptRuntime};

/// The values where swb departs from Chromium on purpose.
const SWB_VALUES: &[Departure] = &[
    (
        "errors",
        0,
        r#"[["Uncaught TypeError: inline", "TypeError", true, 1, 37]]"#,
        "lines and columns count from the start of the inline script, not of the document (until the script processing of feature 5c)",
    ),
    (
        "errors",
        6,
        r#"[["Uncaught SyntaxError: reported", "SyntaxError", false, 1, 42], ["Uncaught 5", "number", false, 0, 0]]"#,
        "reportError of a value that is not an error object has no position in swb (Chromium: the call)",
    ),
];

#[test]
fn probe_cases() {
    let failures = probe_failures_in(cases::CASES, SWB_VALUES, ClockMode::Virtual);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The error lines of the console.
fn errors(rt: &ScriptRuntime) -> Vec<String> {
    rt.console_messages()
        .into_iter()
        .filter(|m| m.level == ConsoleLevel::Error)
        .map(|m| m.text)
        .collect()
}

/// Runs the event loop in virtual time until no task waits (at most
/// `steps` tasks).
fn settle(rt: &mut ScriptRuntime, steps: usize) {
    let mut ran = 0;
    while ran < steps {
        let n = rt.run_due_tasks(Duration::from_secs(5));
        ran += n;
        match rt.next_task_time() {
            Some(time) if n == 0 => rt.clock().advance_to(time),
            Some(_) => {}
            None => break,
        }
    }
}

#[test]
fn reports_go_to_the_console_unless_a_listener_cancels_them() {
    on_big_stack(|| {
        for stress in [false, true] {
            let page = "<!DOCTYPE html><script>window.addEventListener('error', (e) => { if (e.message.includes('quiet')) e.preventDefault(); }); window.addEventListener('unhandledrejection', (e) => { if (String(e.reason).includes('quiet')) e.preventDefault(); });</script>\
                <script>throw new Error('loud');</script><script>throw new Error('quiet');</script>\
                <script>Promise.reject('loud rejection'); Promise.reject('quiet rejection'); setTimeout(() => null.x, 5); setTimeout(() => { throw 'quiet timer'; }, 5);</script>";
            let mut rt = load_in(page, stress, ClockMode::Virtual);
            settle(&mut rt, 10);
            assert_eq!(
                errors(&rt),
                [
                    "Uncaught Error: loud",
                    "Uncaught (in promise) loud rejection",
                    "Uncaught TypeError: Cannot read properties of null (reading 'x')",
                ],
                "stress {stress}"
            );
        }
    });
}

#[test]
fn an_error_in_an_error_listener_goes_to_the_console_only() {
    on_big_stack(|| {
        let page = "<!DOCTYPE html><script>window.n = 0; window.addEventListener('error', () => { n++; throw new Error('in the listener'); });</script><script>throw new Error('first');</script>";
        let mut rt = load_in(page, false, ClockMode::Virtual);
        assert_eq!(answer(&mut rt, "n"), expected("1"));
        assert_eq!(
            errors(&rt),
            ["Uncaught Error: in the listener", "Uncaught Error: first"]
        );
    });
}

#[test]
fn compile_errors_fire_error_events_with_a_syntax_error() {
    on_big_stack(|| {
        let page = "<!DOCTYPE html><script>window.E = []; window.addEventListener('error', (e) => E.push([e.message.startsWith('Uncaught SyntaxError'), e.error instanceof SyntaxError, e.filename, e.lineno, e.colno]));</script><script>\n  let x = ;</script>";
        let mut rt = load_in(page, false, ClockMode::Virtual);
        assert_eq!(
            answer(&mut rt, "E"),
            expected(r#"[[true, true, "file:///probe/page.html", 2, 11]]"#)
        );
    });
}

#[test]
fn interval_zero_and_timer_chains_stay_responsive() {
    on_big_stack(|| {
        let page = "<!DOCTYPE html><script>window.n = 0; setInterval(() => n++, 0); window.c = 0; (function f() { c++; setTimeout(f, 0); })();</script>";
        let mut rt = load_in(page, false, ClockMode::Virtual);
        // One turn runs the due tasks within its budget and returns.
        let started = Instant::now();
        rt.run_due_tasks(Duration::from_millis(20));
        assert!(started.elapsed() < Duration::from_secs(1));
        // 100 ms of virtual time: both clamp to 4 ms after level 5.
        for _ in 0..200 {
            if rt.run_due_tasks(Duration::from_secs(1)) == 0 {
                let next = rt.next_task_time().unwrap();
                if next > 100.0 {
                    break;
                }
                rt.clock().advance_to(next);
            }
        }
        assert_eq!(
            answer(&mut rt, "[n > 20 && n < 40, c > 20 && c < 40]"),
            expected("[true, true]")
        );
        assert_eq!(rt.active_timers(), 2);
    });
}

#[test]
fn an_endless_timer_and_microtask_chain_end_at_the_time_limit() {
    on_big_stack(|| {
        let page = "<!DOCTYPE html><script>setTimeout(() => { for (;;) {} }, 1); setTimeout(() => { window.after = 1; }, 2); setTimeout(() => { (function f() { queueMicrotask(f); })(); }, 3); setTimeout(() => { window.last = 1; }, 4);</script>";
        let mut rt = load_in(page, false, ClockMode::Virtual);
        rt.set_time_limit(Duration::from_millis(100));
        settle(&mut rt, 10);
        assert!(!rt.stopped());
        assert_eq!(
            answer(&mut rt, "[window.after, window.last]"),
            expected("[1, 1]")
        );
        let errors = errors(&rt);
        assert_eq!(
            errors.iter().filter(|e| e.contains("time limit")).count(),
            2,
            "{errors:?}"
        );
    });
}

#[test]
fn long_tasks_use_the_budget_of_the_document() {
    on_big_stack(|| {
        let page = "<!DOCTYPE html><script>window.ran = 0; for (let i = 0; i < 20; i++) setTimeout(() => { ran++; const t = Date.now(); const end = performance.now() + 1e9; let k = 0; while (k < 2e7) k++; }, i);</script>";
        let mut rt = load_in(page, false, ClockMode::Virtual);
        // Each task takes well over 50 ms; the budget stops the document
        // after a few, before the time limit acts.
        rt.set_script_budget(Duration::from_millis(150));
        rt.set_time_limit(Duration::from_secs(10));
        settle(&mut rt, 30);
        assert!(rt.stopped());
        assert!(!rt.failed());
        assert_eq!(rt.next_task_time(), None);
    });
}

#[test]
fn the_heap_limit_marks_the_runtime_as_failed() {
    on_big_stack(|| {
        let mut rt = load_in("<!DOCTYPE html>", false, ClockMode::Virtual);
        rt.set_heap_limit(8 << 20);
        let result = rt.run_classic_script(
            "const a = []; for (;;) a.push({ x: [1, 2, 3] });",
            "file:///hog.js",
        );
        assert!(result.is_err());
        assert!(rt.stopped());
        assert!(rt.failed());
    });
}

#[test]
fn the_timer_limit_ends_the_task() {
    on_big_stack(|| {
        let mut rt = load_in("<!DOCTYPE html>", false, ClockMode::Virtual);
        let result = rt.run_classic_script(
            "for (;;) setTimeout(() => {}, 1e9, 1, 2, 3);",
            "file:///timers.js",
        );
        assert!(result.is_err());
        assert!(rt.active_timers() <= (1 << 20) / 4);
        // String handlers count their bytes (32 MiB).
        let mut rt = load_in("<!DOCTYPE html>", false, ClockMode::Virtual);
        let result = rt.run_classic_script(
            "const s = ' '.repeat(4 << 20); for (;;) setTimeout(s, 1e9);",
            "file:///code.js",
        );
        assert!(result.is_err());
        assert_eq!(rt.active_timers(), 8);
        // Cleared timers do not count.
        let mut rt = load_in("<!DOCTYPE html>", false, ClockMode::Virtual);
        rt.run_classic_script(
            "for (let i = 0; i < 300000; i++) clearTimeout(setTimeout(() => {}, 1e9));",
            "file:///cleared.js",
        )
        .unwrap();
        rt.run_classic_script(
            "const s = ' '.repeat(4 << 20); for (let i = 0; i < 100; i++) clearTimeout(setTimeout(s, 1e9));",
            "file:///cleared-code.js",
        )
        .unwrap();
    });
}

#[test]
fn finalization_cleanups_run_as_tasks() {
    on_big_stack(|| {
        let page = "<!DOCTYPE html><script>window.held = []; window.registry = new FinalizationRegistry((h) => held.push(h)); (function () { for (let i = 0; i < 3; i++) registry.register({}, 'v' + i); })();</script>";
        let mut rt = load_in(page, false, ClockMode::Virtual);
        rt.collect();
        settle(&mut rt, 10);
        assert_eq!(
            answer(&mut rt, "held.sort().join()"),
            expected(r#""v0,v1,v2""#)
        );
    });
}

#[test]
fn evaluations_wait_for_timers_in_virtual_time() {
    on_big_stack(|| {
        let mut rt = load_in("<!DOCTYPE html>", false, ClockMode::Virtual);
        let started = Instant::now();
        assert_eq!(
            answer(
                &mut rt,
                "new Promise((r) => setTimeout(() => r([performance.now() >= 10000, Date.now() - performance.timeOrigin >= 9999]), 10000))"
            ),
            expected("[true, true]")
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        // A promise that never settles is an error once no task is left.
        assert_eq!(
            answer(&mut rt, "new Promise(() => {})"),
            expected(r#""error: Error: the promise did not settle in time""#)
        );
    });
}

#[test]
fn the_clock_starts_at_a_fixed_time() {
    on_big_stack(|| {
        let clock = swb_web::PageClock::new(ClockMode::Virtual, Some(1_700_000_000_000.0));
        let mut rt = ScriptRuntime::new(clock.clone()).unwrap();
        clock.advance_to(1234.5);
        let date = rt.evaluate("[Date.now(), new Date().getTime(), performance.timeOrigin, performance.now(), new Event('x').timeStamp]", swb_web::JsonForm::Strict);
        assert_eq!(
            date,
            swb_web::Evaluation::Value(
                "[1700000001234,1700000001234,1700000000000,1234.5,1234.5]".to_owned()
            )
        );
    });
}
