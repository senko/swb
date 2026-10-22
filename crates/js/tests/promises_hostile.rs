//! Hostile cases for promises and the job queue (M7 feature 7b): chains of
//! a million `then` calls, promises resolved with promises 100,000 levels
//! deep, `Promise.all` and `any` over a million elements, endless job
//! loops, queues and rejection lists that grow without end, deep thenable
//! chains and recursive executors. A case ends with a result, an error or
//! a termination at the deadline or the heap limit; none may overflow the
//! Rust stack, hold memory outside the heap accounting in proportion to
//! the script's data, or run past the deadline.

mod common;

use std::time::{Duration, Instant};

use common::{
    assert_clean, on_big_stack, run_in, run_with_deadline, runtime, runtime_with_heap_limit,
};
use swb_js::{ScriptError, Termination};

#[test]
fn a_million_then_calls_run_in_order() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let (out, _) = run_with_deadline(
            &mut rt,
            "function inc(v) { return v + 1; } var p = Promise.resolve(0); \
             for (var i = 0; i < 1000000; i++) p = p.then(inc); \
             p.then(v => print(v)); undefined",
            Duration::from_secs(120),
        );
        assert_eq!(out, "1000000");
        assert_clean(&rt);
        // The same chain with `catch` and `finally` links, from the end:
        // nothing recurses when the first promise settles last.
        let (out, _) = run_with_deadline(
            &mut rt,
            "var r; var first = new Promise(x => { r = x; }); var p = first; \
             for (var i = 0; i < 100000; i++) p = i % 2 ? p.catch(e => e) : p.finally(() => {}); \
             p.then(v => print('end', v)); r('v'); undefined",
            Duration::from_secs(120),
        );
        assert_eq!(out, "end v");
        assert_clean(&rt);
    });
}

#[test]
fn promises_resolved_with_promises_100000_levels_deep() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let (out, _) = run_with_deadline(
            &mut rt,
            "var p = Promise.resolve('deep'); \
             for (var i = 0; i < 100000; i++) { const q = p; p = new Promise(r => r(q)); } \
             p.then(v => print(v)); undefined",
            Duration::from_secs(120),
        );
        assert_eq!(out, "deep");
        // Thenables that resolve with the next thenable, 100,000 levels.
        let (out, _) = run_with_deadline(
            &mut rt,
            "function t(n) { return { then(r) { r(n ? t(n - 1) : 'bottom'); } }; } \
             Promise.resolve(t(100000)).then(v => print(v)); undefined",
            Duration::from_secs(120),
        );
        assert_eq!(out, "bottom");
        // A handler that returns the previous promise of a chain.
        let (out, _) = run_with_deadline(
            &mut rt,
            "var p = Promise.resolve(1); \
             for (var i = 0; i < 100000; i++) { const q = p; p = Promise.resolve().then(() => q); } \
             p.then(v => print(v)); undefined",
            Duration::from_secs(120),
        );
        assert_eq!(out, "1");
        assert_clean(&rt);
    });
}

#[test]
fn combinators_over_a_million_elements() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            (
                "Promise.all(Array.from({ length: 1000000 }, (_, i) => i)).then(v => print(v.length, v[999999])); undefined",
                "1000000 999999",
            ),
            (
                "Promise.any(Array.from({ length: 1000000 }, (_, i) => Promise.reject(i))).catch(e => print(e.errors.length)); undefined",
                "1000000",
            ),
            (
                "Promise.allSettled(Array.from({ length: 1000000 }, (_, i) => i)).then(v => print(v.length)); undefined",
                "1000000",
            ),
            (
                "Promise.race(Array.from({ length: 1000000 }, (_, i) => i)).then(v => print(v)); undefined",
                "0",
            ),
        ];
        for (source, expected) in cases {
            let (out, _) = run_with_deadline(&mut rt, source, Duration::from_secs(120));
            assert_eq!(out, expected, "{source}");
            assert_clean(&rt);
            rt.collect();
        }
    });
}

#[test]
fn combinators_end_at_the_heap_limit() {
    on_big_stack(|| {
        for source in [
            "Promise.all(Array.from({ length: 1000000 }, (_, i) => i)); undefined",
            "Promise.any(Array.from({ length: 1000000 }, (_, i) => Promise.reject(i))); undefined",
        ] {
            let mut rt = runtime_with_heap_limit(64, false);
            let (out, _) = run_with_deadline(&mut rt, source, Duration::from_secs(120));
            assert!(out.contains("heap limit"), "{source}: {out}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn endless_job_loops_end_at_the_deadline() {
    on_big_stack(|| {
        let cases = [
            "function f() { Promise.resolve().then(f); } f();",
            "function g() { new Promise(r => r()).then(g); } g();",
            // A thenable job that resolves with a new thenable each time.
            "function t() { return { then(r) { r(t()); } }; } Promise.resolve(t());",
            "var p = Promise.resolve(); function h() { p.finally(h); } h();",
        ];
        for source in cases {
            let mut rt = runtime(false);
            let (out, elapsed) = run_with_deadline(&mut rt, source, Duration::from_millis(100));
            assert!(out.contains("time limit"), "{source}: {out}");
            assert!(elapsed < Duration::from_secs(2), "{source}: {elapsed:?}");
            assert_clean(&rt);
            // The runtime stays usable; the queued jobs are dropped.
            rt.clear_jobs();
            assert_eq!(run_in(&mut rt, "1 + 1"), "=> 2");
        }
    });
}

#[test]
fn growing_promise_queues_end_at_the_heap_limit() {
    on_big_stack(|| {
        let cases = [
            // The script fills the queue.
            "for (;;) Promise.resolve().then(() => {});",
            "(function () { var p = Promise.resolve(); for (;;) p.then(); })();",
            // Each job enqueues two.
            "function f() { Promise.resolve().then(f); Promise.resolve().then(f); } f();",
            // The reactions of one pending promise (a local, so that the
            // memory is garbage after the termination).
            "(function () { var p = new Promise(() => {}); for (;;) p.then(() => {}); })();",
            // Rejections without a handler, which the tracker keeps.
            "for (;;) Promise.reject(new Error('x'));",
        ];
        for source in cases {
            let mut rt = runtime_with_heap_limit(32, false);
            let (out, elapsed) = run_with_deadline(&mut rt, source, Duration::from_secs(60));
            assert!(out.contains("heap limit"), "{source}: {out}");
            assert!(elapsed < Duration::from_secs(30), "{source}: {elapsed:?}");
            assert_clean(&rt);
            rt.clear_jobs();
            let _ = rt.take_unhandled_rejections();
            rt.collect();
            assert_eq!(run_in(&mut rt, "1 + 1"), "=> 2");
        }
    });
}

#[test]
fn recursive_executors_end_with_a_range_error() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Each executor runs through re-entry (Rust recursion with the
        // recursion budget): the innermost levels reject with the
        // RangeError, the others resolve with them.
        let (out, _) = run_with_deadline(
            &mut rt,
            "function f(n) { return new Promise(r => r(n && f(n - 1))); } \
             f(100000).then(v => print('value', v), e => print(e.constructor.name, e.message)); undefined",
            Duration::from_secs(60),
        );
        assert_eq!(out, "RangeError Maximum call stack size exceeded");
        // A `then` that calls `then` of a native promise in its executor.
        let (out, _) = run_with_deadline(
            &mut rt,
            "function g(n) { return n ? new Promise((r, j) => g(n - 1).then(r, j)) : Promise.resolve('g'); } \
             g(100000).then(v => print(v), e => print(e.constructor.name)); undefined",
            Duration::from_secs(60),
        );
        assert_eq!(out, "RangeError");
        assert_clean(&rt);
    });
}

#[test]
fn a_termination_in_a_job_skips_catch_handlers() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let start = Instant::now();
        rt.set_deadline(Some(start + Duration::from_millis(100)));
        rt.eval(
            "var caught = 0; \
             Promise.resolve().then(() => { for (;;) {} }).catch(() => { caught++; }).finally(() => { caught++; });",
        )
        .unwrap();
        let error = rt.run_jobs().unwrap_err();
        assert_eq!(error, ScriptError::Terminated(Termination::TimeLimit));
        rt.set_deadline(None);
        rt.run_jobs().unwrap();
        assert_eq!(run_in(&mut rt, "caught"), "=> 0");
        assert_clean(&rt);
    });
}

// --- Async functions (M7 feature 7c) ---

#[test]
fn async_recursion_100000_deep_ends_with_a_range_error() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Each call runs synchronously until its first `await`, so the
        // recursion costs frames: the frame limit ends it with the
        // RangeError, which rejects the innermost promise; every level
        // above awaits the rejection (Chromium 148 rejects too).
        let (out, _) = run_with_deadline(
            &mut rt,
            "var depth = 0; async function f(n) { depth++; if (n) await f(n - 1); return n; } \
             f(100000).then(v => print('value', v), e => print(e.constructor.name, e.message, depth > 1000)); \
             undefined",
            Duration::from_secs(60),
        );
        assert_eq!(out, "RangeError Maximum call stack size exceeded true");
        assert_clean(&rt);
        // With an `await` before the recursive call, each level starts
        // from a job: no frame recursion, the result arrives.
        let (out, _) = run_with_deadline(
            &mut rt,
            "async function g(n) { await null; return n ? 1 + await g(n - 1) : 0; } \
             g(100000).then(v => print(v)); undefined",
            Duration::from_secs(60),
        );
        assert_eq!(out, "100000");
        assert_clean(&rt);
    });
}

#[test]
fn a_chain_of_100000_async_functions_awaiting_each_other() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        for (source, expected) in [
            // The first promise settles last: the settlement passes
            // through 100,000 suspended functions, one job each.
            (
                "var r; var first = new Promise(x => { r = x; }); var p = first; \
                 for (let i = 0; i < 100000; i++) { const q = p; p = (async () => (await q) + 1)(); } \
                 r(0); p.then(v => print(v)); undefined",
                "100000",
            ),
            // Each function returns the promise of the previous one (a
            // thenable job per level) from a `try` with a `finally`.
            (
                "var p = Promise.resolve(0); \
                 for (let i = 0; i < 100000; i++) { const q = p; p = (async () => { try { return q; } finally { } })(); } \
                 p.then(v => print('returned', v)); undefined",
                "returned 0",
            ),
        ] {
            let (out, _) = run_with_deadline(&mut rt, source, Duration::from_secs(60));
            assert_eq!(out, expected, "{source}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn endless_await_loops_end_at_the_deadline() {
    on_big_stack(|| {
        let cases = [
            "(async () => { for (;;) await 0; })();",
            "(async () => { for (;;) await { then(r) { r(); } }; })();",
            "async function f() { await null; f(); } f();",
            "(async () => { for (;;) { try { await Promise.reject(1); } catch (e) {} } })();",
        ];
        for source in cases {
            let mut rt = runtime(false);
            let (out, elapsed) = run_with_deadline(&mut rt, source, Duration::from_millis(100));
            assert!(out.contains("time limit"), "{source}: {out}");
            assert!(elapsed < Duration::from_secs(2), "{source}: {elapsed:?}");
            assert_clean(&rt);
            rt.clear_jobs();
            assert_eq!(run_in(&mut rt, "1 + 1"), "=> 2");
        }
    });
}

#[test]
fn growing_sets_of_suspended_functions_end_at_the_heap_limit() {
    on_big_stack(|| {
        let cases = [
            // Suspended functions that wait for promises in a global array.
            "var keep = []; for (;;) { const p = new Promise(() => {}); keep.push(p); \
             (async () => { var a = [1, 2, 3]; await p; })(); }",
            // One function whose frame grows across awaits.
            "(async () => { var a = []; for (;;) { a.push(new Array(100)); await null; } })();",
            // Awaits that start more async calls than they finish.
            "async function f() { await null; f(); f(); } f();",
        ];
        for source in cases {
            let mut rt = runtime_with_heap_limit(32, false);
            let (out, elapsed) = run_with_deadline(&mut rt, source, Duration::from_secs(60));
            assert!(out.contains("heap limit"), "{source}: {out}");
            assert!(elapsed < Duration::from_secs(30), "{source}: {elapsed:?}");
            assert_clean(&rt);
            rt.clear_jobs();
            rt.collect();
            assert_eq!(run_in(&mut rt, "1 + 1"), "=> 2");
        }
    });
}
