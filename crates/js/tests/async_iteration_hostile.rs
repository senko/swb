//! Hostile cases for async iteration (M7 feature 7d): a chain of 100,000
//! async generators that delegate with `yield*`, `for await` over a million
//! values, a queue of a million requests on one generator, `Array.fromAsync`
//! over a million values, endless async generators and loops, and queues
//! that grow without end. A case ends with a result, an error or a
//! termination at the deadline or the heap limit; none may overflow the
//! Rust stack, hold memory outside the heap accounting in proportion to
//! the script's data, or run past the deadline.

mod common;

use std::time::Duration;

use common::{
    assert_clean, on_big_stack, run_in, run_with_deadline, runtime, runtime_with_heap_limit,
};

/// Each level's `next` resumes the next level as a frame of the same
/// interpreter loop: the frame limit ends the chain with a `RangeError`
/// that rejects the innermost request, and every level rejects in turn
/// from the jobs (Chromium 148 also rejects with the `RangeError`).
#[test]
fn a_chain_of_100000_yield_star_levels() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let (out, _) = run_with_deadline(
            &mut rt,
            "async function* chain(n) { if (n > 0) yield* chain(n - 1); else yield 'bottom'; } \
             chain(100000).next().then(r => print('value', r.value), \
               e => print(e.constructor.name, e.message)); undefined",
            Duration::from_secs(120),
        );
        assert_eq!(out, "RangeError Maximum call stack size exceeded");
        assert_clean(&rt);
        // A chain within the frame limit works; values and returns pass
        // through every level.
        let (out, _) = run_with_deadline(
            &mut rt,
            "async function* deep(n) { if (n > 0) return yield* deep(n - 1); \
               yield 'a'; yield 'b'; return 'end'; } \
             (async () => { var it = deep(4000), out = []; \
               for (var r = await it.next(); !r.done; r = await it.next()) out.push(r.value); \
               print(out.join(), r.value); })(); undefined",
            Duration::from_secs(120),
        );
        assert_eq!(out, "a,b end");
        assert_clean(&rt);
    });
}

#[test]
fn for_await_over_a_million_values() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        for source in [
            "async function* many(n) { for (var i = 0; i < n; i++) yield i; } \
             (async () => { var s = 0; for await (var v of many(1000000)) s += v; print(s); })(); \
             undefined",
            "(async () => { var a = new Array(1000000).fill(1); var s = 0; \
               for await (var v of a) s += v; print(s * 499999.5); })(); undefined",
            "var it = { [Symbol.asyncIterator]() { var i = 0; return { \
               next() { return Promise.resolve({ value: i, done: i++ >= 1000000 }); } }; } }; \
             (async () => { var s = 0; for await (var v of it) s += v; print(s); })(); undefined",
        ] {
            let (out, _) = run_with_deadline(&mut rt, source, Duration::from_secs(300));
            assert_eq!(out, "499999500000", "{source}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn a_queue_of_a_million_requests() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let (out, _) = run_with_deadline(
            &mut rt,
            "var g = (async function* () { yield 1; await null; yield 2; })(); var ps = []; \
             for (var i = 0; i < 1000000; i++) ps.push(g.next()); \
             Promise.all(ps).then(r => print(r.length, r[0].value, r[1].value, r[2].done, \
               r[999999].done)); undefined",
            Duration::from_secs(300),
        );
        assert_eq!(out, "1000000 1 2 true true");
        assert_clean(&rt);
        // Requests of all kinds while the generator waits.
        let (out, _) = run_with_deadline(
            &mut rt,
            "var h = (async function* () { try { yield await null; } finally { print('fin'); } })(); \
             var ps = []; for (var i = 0; i < 90000; i++) \
               ps.push(i % 3 === 0 ? h.next() : i % 3 === 1 ? h.return(i) : h.throw(i)); \
             Promise.allSettled(ps).then(rs => print( \
               rs.filter(r => r.status === 'fulfilled').length, \
               rs.filter(r => r.status === 'rejected').length)); undefined",
            Duration::from_secs(300),
        );
        assert_eq!(out, "fin\n60000 30000");
        assert_clean(&rt);
    });
}

#[test]
fn from_async_over_a_million_values() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        for (source, expected) in [
            (
                "Array.fromAsync({ length: 1000000 }).then(a => print(a.length)); undefined",
                "1000000",
            ),
            (
                "Array.fromAsync(new Array(1000000).fill(2), (x, i) => x + i) \
                   .then(a => print(a.length, a[999999])); undefined",
                "1000000 1000001",
            ),
            (
                "async function* many(n) { for (var i = 0; i < n; i++) yield i; } \
                 Array.fromAsync(many(1000000)).then(a => print(a.length, a[999999])); undefined",
                "1000000 999999",
            ),
        ] {
            let (out, _) = run_with_deadline(&mut rt, source, Duration::from_secs(300));
            assert_eq!(out, expected, "{source}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn endless_async_iteration_ends_at_the_deadline() {
    on_big_stack(|| {
        let cases = [
            "async function* g() { for (;;) yield 1; } (async () => { for await (var x of g()); })();",
            "async function* g() { for (;;) { await null; yield 1; } } \
             (async () => { for await (var x of g()); })();",
            "(async () => { for await (var x of { [Symbol.iterator]() { return { \
               next() { return { value: 1, done: false }; } }; } }); })();",
            "async function* g() { for (;;) yield 1; } Array.fromAsync(g());",
            "async function* g() { for (;;) yield* [1, 2]; } (async () => { for await (var x of g()); })();",
            "var it = (async function* () { for (;;) yield 1; })(); function f() { it.next().then(f); } f();",
            "(async () => { for (;;) { for await (var x of []); } })();",
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
fn growing_async_generator_queues_end_at_the_heap_limit() {
    on_big_stack(|| {
        let cases = [
            // Requests on a generator that waits forever.
            "var g = (async function* () { await new Promise(() => {}); })(); g.next(); \
             for (;;) g.next();",
            // Requests on a generator that runs, in one script.
            "var g = (async function* () { for (;;) yield 1; })(); for (;;) g.next();",
            // Return requests that wait for promises that never settle.
            "var g = (async function* () {})(); for (;;) g.return(new Promise(() => {}));",
            // A generator whose frame grows across yields.
            "(async () => { for await (var x of (async function* () { var a = []; \
               for (;;) { a.push(new Array(100)); yield a.length; } })()); })();",
            // Values that `Array.fromAsync` collects without end.
            "async function* g() { for (;;) yield new Array(10); } Array.fromAsync(g());",
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
