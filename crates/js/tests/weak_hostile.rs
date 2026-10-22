//! Hostile cases for the weak collections and references (M7 feature
//! 9b): a million entries whose keys die under a small heap limit, chains
//! of a million ephemerons (marked without recursion, on an 8 MiB stack in
//! the debug build of `cargo test`), tables, kept sets and registries that
//! grow to the heap limit, cleanup callbacks that register new targets or
//! throw, and `getOrInsertComputed` callbacks that change or re-enter the
//! map. Each case ends with a result, an error or a termination; none
//! panics or keeps memory that a dead key held.

#![allow(clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};

use common::{assert_clean, on_big_stack, runtime, runtime_with_tests};
use swb_js::{HeapConfig, Runtime, RuntimeConfig, ScriptError, Termination};

fn runtime_with_limit(limit: usize) -> Runtime {
    runtime_with_tests(RuntimeConfig {
        heap: HeapConfig {
            limit,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    })
}

#[test]
fn a_million_entries_whose_keys_die_fit_a_small_heap() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(32 << 20);
        // Alive at once, the entries and their keys would need about
        // 150 MB; the collections remove the dead ones as the loop runs.
        rt.eval(
            "var m = new WeakMap(), s = new WeakSet(); \
             for (let i = 0; i < 1000000; i++) { const k = {}; m.set(k, { i }); s.add(k); }",
        )
        .unwrap();
        rt.clear_kept_objects();
        rt.collect();
        let live = rt.heap().stats().live_bytes;
        assert!(live < 8 << 20, "{live} live bytes after the keys died");
        assert_clean(&rt);
    });
}

#[test]
fn a_chain_of_a_million_ephemerons_is_marked_without_recursion() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Inserted backwards: every entry waits for its key at first.
        rt.eval(
            "var m = new WeakMap(); var head = {}; \
             (() => { const keys = [head]; for (let i = 0; i < 1000000; i++) keys.push({}); \
             for (let i = 999999; i >= 0; i--) m.set(keys[i], keys[i + 1]); })();",
        )
        .unwrap();
        rt.clear_kept_objects();
        let start = Instant::now();
        rt.collect();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(10),
            "collection took {elapsed:?}"
        );
        let length = rt
            .eval("let c = head, n = 0; while (m.has(c)) { c = m.get(c); n++; } n")
            .unwrap();
        assert_eq!(rt.display(length), "1000000");
        rt.eval("head = null;").unwrap();
        rt.collect();
        assert!(rt.heap().counts().objects < 100_000);
        assert_clean(&rt);
    });
}

#[test]
fn a_chain_that_grows_to_the_heap_limit_terminates() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(64 << 20);
        let result = rt.eval(
            "var maps = [new WeakMap(), new WeakMap()]; var head = {}; \
             for (let c = head, i = 0; ; i++) { const next = {}; maps[i % 2].set(c, next); c = next; }",
        );
        assert_eq!(result, Err(ScriptError::Terminated(Termination::HeapLimit)));
        assert_clean(&rt);
    });
}

#[test]
fn growing_weak_objects_end_at_the_heap_limit() {
    on_big_stack(|| {
        for source in [
            // Every key stays alive through the array.
            "var keep = [], m = new WeakMap(); for (;;) { const k = {}; keep.push(k); m.set(k, 1); }",
            // The kept set holds every target until the run ends.
            "for (;;) new WeakRef({});",
            // The cells live as long as the registry.
            "var fr = new FinalizationRegistry(() => {}), t = {}; for (let i = 0; ; i++) fr.register(t, i, t);",
        ] {
            let mut rt = runtime_with_limit(48 << 20);
            let result = rt.eval(source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::HeapLimit)),
                "{source}"
            );
            assert_clean(&rt);
        }
    });
}

#[test]
fn a_callback_that_registers_new_targets_each_time() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "var calls = 0; var fr = new FinalizationRegistry(held => { calls++; \
             fr.register({ junk: new Array(100) }, held + 1); }); \
             fr.register({}, 0);",
        )
        .unwrap();
        // Each round finds the target of the last one dead: an endless
        // sequence of tasks, as with a timer that sets itself again; the
        // host decides how many run. Memory stays bounded.
        for _ in 0..2000 {
            rt.clear_kept_objects();
            rt.collect();
            assert!(rt.run_finalization_cleanup().unwrap());
        }
        let calls = rt.eval("calls").unwrap();
        assert_eq!(rt.display(calls), "2000");
        assert!(rt.heap().stats().live_bytes < 16 << 20);
        assert_clean(&rt);
    });
}

#[test]
fn the_shell_loop_of_cleanup_tasks_ends_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(true);
        rt.eval(
            "function reg() { fr.register({}, 0); } \
             var fr = new FinalizationRegistry(() => { reg(); for (let i = 0; i < 10; i++) [i]; }); \
             reg();",
        )
        .unwrap();
        rt.collect();
        // The stress mode collects inside each callback (at the backward
        // jumps of its loop), so every task queues the next one: the
        // deadline ends the loop.
        let start = Instant::now();
        rt.set_deadline(Some(start + Duration::from_millis(300)));
        let mut result = Ok(true);
        while matches!(result, Ok(true)) {
            result = rt.run_finalization_cleanup();
            if start.elapsed() > Duration::from_secs(20) {
                break;
            }
        }
        rt.set_deadline(None);
        assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
        assert_clean(&rt);
    });
}

#[test]
fn callbacks_that_throw_report_one_error_each() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "var fr = new FinalizationRegistry(held => { throw new Error('cell ' + held); }); \
             for (let i = 0; i < 10000; i++) fr.register({}, i);",
        )
        .unwrap();
        rt.clear_kept_objects();
        rt.collect();
        let start = Instant::now();
        let mut errors = 0;
        loop {
            match rt.run_finalization_cleanup() {
                Ok(true) => {}
                Ok(false) => break,
                Err(ScriptError::Uncaught { message, .. }) => {
                    assert_eq!(message, format!("cell {errors}"));
                    errors += 1;
                }
                Err(other) => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(errors, 10000);
        assert!(start.elapsed() < Duration::from_secs(10));
        assert_clean(&rt);
    });
}

#[test]
fn get_or_insert_computed_callbacks_that_change_the_map() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let value = rt
            .eval(
                "var m = new WeakMap(); var k = {}; var keep = []; \
                 m.getOrInsertComputed(k, () => { \
                   for (let i = 0; i < 100000; i++) { const o = {}; keep.push(o); m.set(o, i); } \
                   for (let i = 0; i < 100000; i += 2) m.delete(keep[i]); \
                   m.set(k, 'inner'); return 'outer'; }) + ' ' + m.get(k) + ' ' + m.get(keep[99999])",
            )
            .unwrap();
        assert_eq!(rt.display(value), "outer outer 99999");
        // Re-entry without end is a RangeError, not a stack overflow.
        let result = rt.eval(
            "var deep = new WeakMap(); function f() { return deep.getOrInsertComputed({}, f); } f()",
        );
        match result {
            Err(ScriptError::Uncaught { name, message, .. }) => {
                assert_eq!(name, "RangeError");
                assert_eq!(message, "Maximum call stack size exceeded");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_clean(&rt);
    });
}
