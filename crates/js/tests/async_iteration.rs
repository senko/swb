//! Async iteration (M7 feature 7d): async generators and their queue of
//! requests, `for await`, `yield*` in async generators, the Async-from-Sync
//! Iterator and `Array.fromAsync`. The measured Chromium cases
//! (`tools/probes/js-async-iteration.json`: the shape, the job order of the
//! requests, the closing of iterators, the messages and the async frames of
//! stack traces), generators that allocate and collect while they wait,
//! generators that become garbage, the embedding API and terminations.
//! Each case except the heavy ones also runs in the GC stress mode.

mod common;

use std::time::{Duration, Instant};

use common::{check_cases, on_big_stack, run, run_in, runtime};
use swb_js::{PromiseStatus, ScriptError, Termination, Value};

/// The snippets of `tools/probes/js-async-iteration.json` with Chromium
/// 148's results and job order (`data/async_iteration_cases.expected`; the
/// script and the expected output are made by
/// `data/gen-async-iteration-cases.js` from the output of `just probe`),
/// normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/async_iteration_cases.js"),
        include_str!("data/async_iteration_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with long loops, deep `yield*` chains and long
/// queues, without the stress mode.
#[test]
fn chromium_cases_heavy() {
    check_chromium(
        include_str!("data/async_iteration_cases_heavy.js"),
        include_str!("data/async_iteration_cases_heavy.expected"),
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

/// Registers, `arguments`, cells, `finally` state, queues of requests,
/// loop records, Async-from-Sync Iterators and the state of
/// `Array.fromAsync`, held only by suspended frames and reactions while
/// the code allocates and collects (the expected output is that of
/// Node.js 22; `just jsdiff` compares it).
#[test]
fn suspended_generators_survive_collections() {
    check_cases(&[(
        include_str!("data/async_iteration_gc.js"),
        include_str!("data/async_iteration_gc.expected").trim_end_matches('\n'),
    )]);
}

/// Async generators that no one can reach are garbage: suspended at the
/// start, at a `yield` with waiting requests, or at an `await` of a
/// promise that no one can settle.
#[test]
fn unreachable_async_generators_are_collected() {
    on_big_stack(|| {
        // The stress mode collects at every allocation: fewer generators.
        for (stress, count) in [(false, 3000), (true, 300)] {
            let mut rt = runtime(stress);
            rt.collect();
            let before = rt.heap().stats().live_bytes;
            rt.eval(&format!(
                "async function* g(k) {{ var big = new Array(100).fill(k); yield big; \
                   await new Promise(() => {{}}); yield big; }} \
                 for (var i = 0; i < {count}; i++) {{ \
                   g(i); \
                   var a = g(i); a.next(); a.next(); a.next(); \
                   var b = g(i); b.next(); \
                 }} \
                 (async () => {{ for await (var x of g(0)) ; }})(); \
                 Array.fromAsync({{ length: 1, 0: new Promise(() => {{}}) }}); \
                 a = b = undefined"
            ))
            .unwrap();
            rt.run_jobs().unwrap();
            rt.collect();
            let after = rt.heap().stats().live_bytes;
            // 3,000 frames with an array of 100 values each would be more
            // than 2 MiB (300: more than 200 KiB).
            assert!(
                after < before + (128 << 10),
                "before {before}, after {after}"
            );
            // A generator that waits for a promise in a global stays, with
            // its queue.
            rt.eval(
                "var resolveIt; var kept = new Promise(r => { resolveIt = r; }); \
                 var w = (async function* () { var big = new Array(1000).fill(1); \
                   yield (await kept) + big.length; })(); \
                 w.next().then(r => print('first', r.value)); \
                 w.next().then(r => print('second', r.done)); w = undefined",
            )
            .unwrap();
            rt.run_jobs().unwrap();
            rt.collect();
            assert_eq!(
                run_in(&mut rt, "resolveIt('late '); undefined"),
                "first late 1000\nsecond true"
            );
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}

/// The embedding API: `next` of an async generator called from the host
/// gives the request's promise; the body runs until its first `await` or
/// `yield` during the call.
#[test]
fn the_host_steps_an_async_generator() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let generator = rt
            .eval(
                "(async function* () { print('start'); var x = yield 1; print('got', x); \
                  await null; return x * 2; })()",
            )
            .unwrap();
        let next = rt
            .eval("Object.getPrototypeOf(async function* () {}).prototype.next")
            .unwrap();
        let first = rt.call(next, generator, &[]).unwrap();
        assert_eq!(run_in(&mut rt, ""), "start");
        let second = rt.call(next, generator, &[Value::Int(21)]).unwrap();
        assert_eq!(
            rt.promise_settlement(second).unwrap().0,
            PromiseStatus::Pending
        );
        // `run_in` also runs the jobs.
        assert_eq!(run_in(&mut rt, ""), "got 21");
        assert_eq!(
            rt.promise_settlement(first).unwrap().0,
            PromiseStatus::Fulfilled
        );
        assert_eq!(
            rt.promise_settlement(second).unwrap().0,
            PromiseStatus::Fulfilled
        );
        assert_eq!(rt.stack_depths(), (0, 0));
    });
}

/// A termination inside an async generator or a `for await` loop ends
/// the script or the drain as a termination: the waiting promises stay
/// pending, no handler and no `finally` block runs. A generator that was
/// suspended during the termination resumes in a later task; one whose
/// frame the termination removed is completed.
#[test]
fn a_termination_is_never_a_rejection() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "var caught = []; \
             async function* endless() { try { for (;;) yield 1; } finally { caught.push('finally'); } } \
             var looping = (async () => { for await (var x of endless()) ; })(); \
             looping.catch(() => caught.push('looping')); \
             var resolveLater; \
             var waiting = (async function* () { var v = await new Promise(r => { resolveLater = r; }); \
               yield v; })(); \
             waiting.next().then(r => print('resumed', r.value)); undefined",
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
        // An endless body that runs in the call of `next`: the frame is
        // removed and the generator is completed.
        let start = Instant::now();
        rt.set_deadline(Some(start + Duration::from_millis(100)));
        let result = rt.eval(
            "var busy = (async function* () { for (;;) {} })(); \
             var busyRequest = busy.next(); busyRequest.catch(() => caught.push('busy')); undefined",
        );
        assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
        rt.set_deadline(None);
        assert_eq!(rt.stack_depths(), (0, 0));
        assert_eq!(
            run_in(
                &mut rt,
                "busy.next().then(r => print('after', r.done)); undefined"
            ),
            "after true"
        );
        // The suspended generator resumes; no handler ran.
        assert_eq!(
            run_in(&mut rt, "resolveLater('late'); undefined"),
            "resumed late"
        );
        assert_eq!(run_in(&mut rt, "caught.join()"), "=> ");
    });
}

/// `Array.fromAsync` closes the iterator once when the mapping function
/// throws, also when `return` throws (Chromium 148 calls `return` again
/// in every job and never settles the promise; the specification rejects
/// with the mapping function's error).
#[test]
fn from_async_closes_once_when_return_throws() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        assert_eq!(
            run_in(
                &mut rt,
                "var calls = 0; var it = { [Symbol.asyncIterator]() { return { \
                   next() { return { value: 1, done: false }; }, \
                   return() { calls++; throw new Error('return'); } }; } }; \
                 Array.fromAsync(it, () => { throw new Error('map'); }) \
                   .catch(e => print(e.message, calls)); undefined"
            ),
            "map 1"
        );
    });
}
