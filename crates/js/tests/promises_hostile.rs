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

use common::{on_big_stack, run_in, runtime, runtime_with_tests};
use swb_js::{HeapConfig, Runtime, RuntimeConfig, ScriptError, Termination};

/// Runs `source` and its jobs with a deadline; the result of the drain
/// (or of the script) and the time it took.
fn run_with_deadline(rt: &mut Runtime, source: &str, deadline: Duration) -> (String, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + deadline));
    let out = run_in(rt, source);
    rt.set_deadline(None);
    (out, start.elapsed())
}

fn assert_clean(rt: &Runtime) {
    assert_eq!(rt.stack_depths(), (0, 0));
    assert_eq!(rt.heap().scope_depth(), 0);
    assert_eq!(rt.heap().no_gc_depth(), 0);
    assert_eq!(rt.heap().stats().total_stale_roots, 0);
}

fn runtime_with_limit(mib: usize) -> Runtime {
    runtime_with_tests(RuntimeConfig {
        heap: HeapConfig {
            limit: mib << 20,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    })
}

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
            let mut rt = runtime_with_limit(64);
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
fn growing_queues_end_at_the_heap_limit() {
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
            let mut rt = runtime_with_limit(32);
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
