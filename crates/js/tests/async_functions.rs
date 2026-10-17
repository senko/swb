//! Async functions and `await` (M7 feature 7c): the measured Chromium
//! cases (`tools/probes/js-async.json`: the shape, the `AsyncFunction`
//! constructor, the messages, the order of the jobs and the async frames
//! of stack traces), async functions that allocate and collect while they
//! wait, frames that become garbage, the embedding API and terminations.
//! Each case except the heavy ones also runs in the GC stress mode.

mod common;

use std::time::{Duration, Instant};

use common::{check_cases, check_script_cases, on_big_stack, run, run_in, runtime};
use swb_js::{PromiseStatus, ScriptError, Termination, Value};

/// The snippets of `tools/probes/js-async.json` with Chromium 148's
/// results and job order (`data/async_cases.expected`; the script and the
/// expected output are made by `data/gen-async-cases.js` from the output
/// of `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/async_cases.js"),
        include_str!("data/async_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with deep recursion and long chains of awaits,
/// without the stress mode.
#[test]
fn chromium_cases_heavy() {
    check_chromium(
        include_str!("data/async_cases_heavy.js"),
        include_str!("data/async_cases_heavy.expected"),
        &[false],
    );
}

fn check_chromium(script: &str, expected: &str, modes: &[bool]) {
    for &stress in modes {
        let actual = run(script, stress);
        let failures: Vec<String> = actual
            .lines()
            .zip(expected.lines())
            .filter(|(actual, expected)| actual != expected)
            .map(|(actual, expected)| format!("expected: {expected}\nactual:   {actual}"))
            .collect();
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "number of lines (stress: {stress})\n{actual}"
        );
        assert!(
            failures.is_empty(),
            "stress: {stress}\n{}",
            failures.join("\n")
        );
    }
}

/// Registers, `arguments`, cells, `finally` state, iterators and values
/// held only by suspended frames while the functions allocate and
/// collect (the expected output is that of Node.js 22; `just jsdiff`
/// compares it).
#[test]
fn suspended_frames_survive_collections() {
    check_cases(&[(
        include_str!("data/async_gc.js"),
        include_str!("data/async_gc.expected").trim_end_matches('\n'),
    )]);
}

/// The full text of `stack` with the async frames, in a named script
/// (the positions of the `await`s), as Chromium 148 shows it.
#[test]
fn stack_traces_show_the_awaiting_functions() {
    check_script_cases(&[
        (
            "async function inner() {\n  await null;\n  throw new Error('deep');\n}\n\
             async function middle() {\n  return await inner();\n}\n\
             async function outer() {\n  try { await middle(); } catch (e) { print(e.stack); }\n}\n\
             outer(); undefined",
            "Error: deep\n    at inner (t.js:3:9)\n    at async middle (t.js:6:10)\n    \
             at async outer (t.js:9:9)",
        ),
        // Before the first `await`, the callers are ordinary frames.
        (
            "async function f() { return new Error('sync').stack; }\nf().then(print); undefined",
            "Error: sync\n    at f (t.js:1:29)\n    at t.js:2:1",
        ),
    ]);
}

/// A suspended async function that waits for a promise no one can settle
/// is garbage once nothing refers to that promise: its frame, registers
/// and result promise are collected.
#[test]
fn unreachable_suspended_functions_are_collected() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            rt.collect();
            let before = rt.heap().stats().live_bytes;
            rt.eval(
                "for (var i = 0; i < 10000; i++) \
                   (async function (k) { var big = new Array(100).fill(k); \
                     await new Promise(() => {}); return big; })(i); \
                 undefined",
            )
            .unwrap();
            rt.run_jobs().unwrap();
            rt.collect();
            let after = rt.heap().stats().live_bytes;
            // 10,000 frames with an array of 100 values each would be more
            // than 8 MiB.
            assert!(
                after < before + (256 << 10),
                "before {before}, after {after}"
            );
            // A function that waits for a promise in a global stays.
            rt.eval(
                "var resolveIt; var kept = new Promise(r => { resolveIt = r; }); \
                 (async () => { var big = new Array(1000).fill(1); var v = await kept; \
                   print('resumed', v, big.length); })(); undefined",
            )
            .unwrap();
            rt.collect();
            assert_eq!(
                run_in(&mut rt, "resolveIt('late'); undefined"),
                "resumed late 1000"
            );
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}

/// The embedding API: calling an async function from the host gives its
/// promise at the first `await`; the rest runs in the jobs.
#[test]
fn the_host_calls_an_async_function() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let f = rt
            .eval("(async function (a) { print('start', a); await null; print('end'); return a * 2; })")
            .unwrap();
        let p = rt.call(f, Value::Undefined, &[Value::Int(21)]).unwrap();
        assert_eq!(rt.promise_settlement(p).unwrap().0, PromiseStatus::Pending);
        assert_eq!(rt.pending_jobs(), 1);
        rt.run_jobs().unwrap();
        assert_eq!(
            rt.promise_settlement(p).unwrap(),
            (PromiseStatus::Fulfilled, Value::Int(42))
        );
        assert_eq!(run_in(&mut rt, ""), "start 21\nend");
        assert_eq!(rt.stack_depths(), (0, 0));
    });
}

/// A termination inside an async function (in its synchronous part or in
/// a job after an `await`) ends the script or the drain as a termination;
/// the result promise stays pending and no handler runs. A function that
/// was suspended during the termination resumes in a later task.
#[test]
fn a_termination_is_never_a_rejection() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // An endless loop of awaits ends at the deadline.
        rt.eval(
            "var caught = []; \
             var looping = (async () => { for (;;) await 0; })(); \
             looping.catch(() => caught.push('looping')); \
             var resolveLater; \
             var waiting = (async () => { var v = await new Promise(r => { resolveLater = r; }); \
               print('resumed', v); })(); undefined",
        )
        .unwrap();
        let start = Instant::now();
        rt.set_deadline(Some(start + Duration::from_millis(100)));
        let error = rt.run_jobs().unwrap_err();
        assert_eq!(error, ScriptError::Terminated(Termination::TimeLimit));
        assert!(start.elapsed() < Duration::from_secs(2));
        rt.set_deadline(None);
        assert_eq!(rt.stack_depths(), (0, 0));
        assert_eq!(rt.heap().scope_depth(), 0);
        rt.clear_jobs();
        let looping = rt.eval("looping").unwrap();
        assert_eq!(
            rt.promise_settlement(looping).unwrap().0,
            PromiseStatus::Pending
        );
        // A loop in the body after an `await`, and in the synchronous part.
        for source in [
            "var late = (async () => { await null; for (;;) {} })(); \
             late.catch(() => caught.push('late')); undefined",
            "var early = (async () => { for (;;) {} })(); undefined",
        ] {
            let start = Instant::now();
            rt.set_deadline(Some(start + Duration::from_millis(100)));
            let result = rt.eval(source).and_then(|_| rt.run_jobs());
            assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
            rt.set_deadline(None);
            assert_eq!(rt.stack_depths(), (0, 0));
            rt.clear_jobs();
        }
        let late = rt.eval("late").unwrap();
        assert_eq!(
            rt.promise_settlement(late).unwrap().0,
            PromiseStatus::Pending
        );
        // The suspended function resumes in a later task.
        assert_eq!(
            run_in(&mut rt, "resolveLater(5); caught.join()"),
            "resumed 5\n=> "
        );
    });
}

/// `run_jobs` drains the awaits of many async functions in the order of
/// the specification (the same as the Chromium cases, here through the
/// embedding API with a collection between two tasks).
#[test]
fn awaits_continue_across_tasks() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            rt.eval(
                "var log = []; async function a() { for (var i = 0; i < 3; i++) { log.push('a' + i); await null; } } \
                 async function b() { for (var i = 0; i < 3; i++) { log.push('b' + i); await null; } } \
                 a(); b(); undefined",
            )
            .unwrap();
            rt.collect();
            rt.run_jobs().unwrap();
            assert_eq!(run_in(&mut rt, "log.join()"), "=> a0,b0,a1,b1,a2,b2");
        }
    });
}
