//! The collector and the weak references (M7 feature 9b, ADR 0026 section
//! 8): ephemerons (a value that references its own key, chains of 100,000
//! ephemerons in one table and across 1,000 tables in both orders of
//! insertion, keys that only another table's value reaches), the sweep of
//! dead entries, `WeakRef` and the kept set, and `FinalizationRegistry`
//! with its cleanup job. The collections are forced from Rust
//! (`Runtime::collect`), so the cases do not depend on when a safepoint
//! collects; each also runs in the GC stress mode (the long chains and
//! the large tables with 5,000 entries there: the stress mode collects at
//! every safepoint while they are built).

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{Output, on_big_stack, runtime};
use swb_js::{Runtime, ScriptError};

/// The entries of the large cases: fewer in the stress mode.
fn large(stress: bool) -> usize {
    if stress { 5_000 } else { 100_000 }
}

/// Runs `f` with a runtime of each mode (normal, stress) on a big stack.
fn both_modes(f: fn(&mut Runtime, bool)) {
    for stress in [false, true] {
        on_big_stack(move || {
            let mut rt = runtime(stress);
            f(&mut rt, stress);
            assert_eq!(rt.heap().stats().total_stale_roots, 0, "stale roots");
        });
    }
}

/// Evaluates a script and returns its result as text (`display`).
fn eval(rt: &mut Runtime, source: &str) -> String {
    match rt.eval(source) {
        Ok(value) => rt.display(value),
        Err(error) => panic!("{source}: {error}"),
    }
}

/// The lines that `print` wrote since the last call.
fn printed(rt: &mut Runtime) -> Vec<String> {
    rt.host_data_mut::<Output>().unwrap().take()
}

/// Ends the synchronous run (`ClearKeptObjects`) and collects.
fn settle(rt: &mut Runtime) {
    rt.clear_kept_objects();
    rt.collect();
}

#[test]
fn a_value_that_references_its_key_dies_with_the_key() {
    both_modes(|rt, _| {
        eval(rt, "var m = new WeakMap(); var keep = {};");
        settle(rt);
        let before = rt.heap().counts().objects;
        eval(
            rt,
            "for (let i = 0; i < 1000; i++) { const k = {}; m.set(k, { k, self: [k] }); } \
             m.set(keep, { keep });",
        );
        settle(rt);
        // Only the entry of `keep` and its value stay.
        let after = rt.heap().counts().objects;
        assert!(
            after <= before + 2,
            "objects before {before}, after {after}"
        );
        assert_eq!(eval(rt, "m.get(keep).keep === keep"), "true");
    });
}

#[test]
fn the_memory_of_dead_entries_returns() {
    both_modes(|rt, stress| {
        eval(rt, "var m = new WeakMap(); var s = new WeakSet();");
        settle(rt);
        let empty = rt.heap().stats().live_bytes;
        let n = large(stress);
        eval(
            rt,
            &format!("for (let i = 0; i < {n}; i++) {{ const k = {{}}; m.set(k, i); s.add(k); }}"),
        );
        rt.collect();
        let after = rt.heap().stats().live_bytes;
        assert!(
            after < empty + 64 * 1024,
            "live bytes {empty} before, {after} after the keys died"
        );
    });
}

/// Builds a chain of `n` ephemerons from `head` across `tables` tables:
/// entry `i` maps key `i` to key `i + 1` in table `i % tables`, inserted
/// in the given order. Only `head` is a root.
fn build_chain(rt: &mut Runtime, n: usize, tables: usize, reverse: bool) {
    let order = if reverse {
        format!("for (let i = {n} - 1; i >= 0; i--)")
    } else {
        format!("for (let i = 0; i < {n}; i++)")
    };
    eval(
        rt,
        &format!(
            "var maps = []; for (let t = 0; t < {tables}; t++) maps.push(new WeakMap()); \
             var head = {{}}; \
             (() => {{ const keys = [head]; for (let i = 0; i < {n}; i++) keys.push({{ i }}); \
             {order} maps[i % {tables}].set(keys[i], keys[i + 1]); }})();"
        ),
    );
}

/// The length of the chain that starts at `head`.
const CHAIN_LENGTH: &str = "(() => { let c = head, n = 0; for (;;) { const next = maps[n % maps.length].get(c); \
    if (next === undefined) return n; c = next; n++; } })()";

fn check_chain(rt: &mut Runtime, n: usize, tables: usize, reverse: bool) {
    build_chain(rt, n, tables, reverse);
    settle(rt);
    let pause = rt.heap().stats().last_pause;
    // Linear marking takes milliseconds; a quadratic one (a rescan of all
    // tables per link) would take hours.
    assert!(
        pause < Duration::from_secs(5),
        "a collection with a chain of {n} took {pause:?}"
    );
    assert_eq!(eval(rt, CHAIN_LENGTH), n.to_string());
    let alive = rt.heap().counts().objects;
    eval(rt, "head = null;");
    settle(rt);
    let dead = rt.heap().counts().objects;
    assert!(
        alive - dead >= n,
        "{alive} objects with the chain, {dead} without"
    );
}

#[test]
fn a_chain_in_one_table_is_marked_in_linear_time() {
    both_modes(|rt, stress| {
        check_chain(rt, large(stress), 1, false);
        check_chain(rt, large(stress), 1, true);
    });
}

#[test]
fn a_chain_across_tables_is_marked_in_linear_time() {
    both_modes(|rt, stress| {
        check_chain(rt, large(stress), 1000, false);
        check_chain(rt, large(stress), 1000, true);
    });
}

#[test]
fn a_key_reachable_only_through_another_tables_value_lives() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var m1 = new WeakMap(), m2 = new WeakMap(), a = {}; \
             (() => { const b = { b: 1 }; m1.set(a, b); m2.set(b, { c: 2 }); })(); \
             var r = new WeakRef(m1.get(a));",
        );
        settle(rt);
        assert_eq!(eval(rt, "m2.get(m1.get(a)).c"), "2");
        // `m2` alone holds nothing: when `a` dies, `b` and its value die.
        eval(rt, "a = null;");
        settle(rt);
        assert_eq!(eval(rt, "r.deref()"), "undefined");
    });
}

#[test]
fn a_dead_table_keeps_nothing() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var k = {}; var r = (() => { const m = new WeakMap(); const v = {}; m.set(k, v); \
             return new WeakRef(v); })();",
        );
        settle(rt);
        assert_eq!(eval(rt, "r.deref()"), "undefined");
    });
}

#[test]
fn a_weakref_target_lives_until_clear_kept_objects() {
    both_modes(|rt, _| {
        eval(rt, "var r = new WeakRef({ x: 1 });");
        rt.collect();
        // Made in this synchronous run: kept.
        assert_eq!(eval(rt, "r.deref().x"), "1");
        rt.collect();
        // Read again by `deref`: kept again.
        assert_eq!(eval(rt, "typeof r.deref()"), "object");
        settle(rt);
        assert_eq!(eval(rt, "r.deref()"), "undefined");
        assert_eq!(rt.heap().kept_objects(), 0);
    });
}

#[test]
fn run_jobs_clears_the_kept_objects() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var r = new WeakRef({}); var s = new WeakRef(Symbol('s')); \
             var later; Promise.resolve().then(() => { later = typeof r.deref(); });",
        );
        rt.run_jobs().unwrap();
        assert_eq!(eval(rt, "later"), "object");
        assert_eq!(rt.heap().kept_objects(), 0);
        rt.collect();
        assert_eq!(eval(rt, "[r.deref(), s.deref()].join()"), ",");
    });
}

#[test]
fn a_symbol_key_dies_with_its_last_reference() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var m = new WeakMap(); var kept = Symbol('kept'); var r; \
             (() => { const s = Symbol('gone'); const v = {}; m.set(s, v); m.set(kept, s); \
             r = new WeakRef(v); })();",
        );
        settle(rt);
        // `kept` → `s` → `v`: a chain through a symbol key.
        assert_eq!(eval(rt, "typeof r.deref()"), "object");
        eval(rt, "m.delete(kept);");
        settle(rt);
        assert_eq!(eval(rt, "r.deref()"), "undefined");
        // Well-known symbols never die.
        eval(rt, "m.set(Symbol.iterator, { w: 1 });");
        settle(rt);
        assert_eq!(eval(rt, "m.get(Symbol.iterator).w"), "1");
    });
}

#[test]
fn a_revoked_proxy_no_longer_keeps_its_target() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var p = (() => { const t = {}; globalThis.r = new WeakRef(t); \
             return Proxy.revocable(t, {}); })(); var m = new WeakMap(); m.set(p.proxy, 1);",
        );
        settle(rt);
        assert_eq!(eval(rt, "typeof r.deref()"), "object");
        eval(rt, "p.revoke();");
        settle(rt);
        assert_eq!(eval(rt, "r.deref()"), "undefined");
        assert_eq!(eval(rt, "m.get(p.proxy)"), "1");
    });
}

#[test]
fn cleanup_callbacks_run_after_the_collection_once_per_target() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var fr = new FinalizationRegistry(held => print('held ' + held)); \
             for (let i = 0; i < 5; i++) fr.register({}, i);",
        );
        settle(rt);
        // The collection only queues the job.
        assert_eq!(printed(rt), Vec::<String>::new());
        assert_eq!(rt.pending_finalization_cleanups(), 1);
        assert!(rt.run_finalization_cleanup().unwrap());
        assert_eq!(
            printed(rt),
            ["held 0", "held 1", "held 2", "held 3", "held 4"]
        );
        assert!(!rt.run_finalization_cleanup().unwrap());
        settle(rt);
        assert!(!rt.run_finalization_cleanup().unwrap());
        assert_eq!(printed(rt), Vec::<String>::new());
    });
}

#[test]
fn an_unregistered_cell_never_runs() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var fr = new FinalizationRegistry(held => print('held ' + held)); var token = {}; \
             fr.register({}, 'a', token); fr.register({}, 'b'); fr.unregister(token);",
        );
        settle(rt);
        while rt.run_finalization_cleanup().unwrap() {}
        assert_eq!(printed(rt), ["held b"]);
        // Unregistered after the target died, before the job ran.
        eval(
            rt,
            "var t2 = {}; fr.register({}, 'c', t2); fr.register({}, 'd', t2);",
        );
        settle(rt);
        assert_eq!(eval(rt, "fr.unregister(t2)"), "true");
        while rt.run_finalization_cleanup().unwrap() {}
        assert_eq!(printed(rt), Vec::<String>::new());
    });
}

#[test]
fn a_dead_token_does_not_keep_the_cell_from_running() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var fr = new FinalizationRegistry(held => print('held ' + held)); \
             var target = {}; fr.register(target, 'x', {});",
        );
        settle(rt);
        eval(rt, "target = null;");
        settle(rt);
        while rt.run_finalization_cleanup().unwrap() {}
        assert_eq!(printed(rt), ["held x"]);
    });
}

#[test]
fn a_held_value_that_references_the_target_keeps_it_while_the_registry_lives() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var fr = new FinalizationRegistry(held => print('held')); \
             var r = (() => { const t = {}; fr.register(t, { t }); return new WeakRef(t); })();",
        );
        settle(rt);
        // The registry holds the held value strongly, and it holds the
        // target: the target cannot die, so the callback never runs.
        assert_eq!(eval(rt, "typeof r.deref()"), "object");
        assert_eq!(rt.pending_finalization_cleanups(), 0);
        eval(rt, "fr = null;");
        settle(rt);
        assert_eq!(eval(rt, "r.deref()"), "undefined");
        assert!(!rt.run_finalization_cleanup().unwrap());
        assert_eq!(printed(rt), Vec::<String>::new());
    });
}

#[test]
fn a_dead_registry_runs_no_callbacks() {
    both_modes(|rt, _| {
        eval(
            rt,
            "(() => { const fr = new FinalizationRegistry(() => print('ran')); \
             for (let i = 0; i < 10; i++) fr.register({}, i); })();",
        );
        settle(rt);
        assert_eq!(rt.pending_finalization_cleanups(), 0);
        assert!(!rt.run_finalization_cleanup().unwrap());
        assert_eq!(printed(rt), Vec::<String>::new());
    });
}

#[test]
fn a_dropped_registry_leaves_the_queue() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var fr = new FinalizationRegistry(held => print('held ' + held)); \
             fr.register({}, 1);",
        );
        settle(rt);
        assert_eq!(rt.pending_finalization_cleanups(), 1);
        eval(rt, "fr = null;");
        settle(rt);
        // The queue holds its registries weakly: no callback runs.
        assert_eq!(rt.pending_finalization_cleanups(), 0);
        assert!(!rt.run_finalization_cleanup().unwrap());
        assert_eq!(printed(rt), Vec::<String>::new());
    });
}

#[test]
fn a_callback_that_throws_reports_and_the_rest_runs_later() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var fr = new FinalizationRegistry(held => { print('held ' + held); \
             if (held === 1) throw new Error('boom ' + held); }); \
             for (let i = 0; i < 3; i++) fr.register({}, i);",
        );
        settle(rt);
        let error = rt.run_finalization_cleanup().unwrap_err();
        match error {
            ScriptError::Uncaught { message, .. } => assert_eq!(message, "boom 1"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(printed(rt), ["held 0", "held 1"]);
        assert_eq!(rt.pending_finalization_cleanups(), 1);
        assert!(rt.run_finalization_cleanup().unwrap());
        assert_eq!(printed(rt), ["held 2"]);
        assert_eq!(rt.stack_depths(), (0, 0));
    });
}

#[test]
fn cleanup_callbacks_can_collect_and_register() {
    both_modes(|rt, _| {
        eval(
            rt,
            "var fr = new FinalizationRegistry(held => { \
             const junk = []; for (let i = 0; i < 1000; i++) junk.push({ i }); gc(); \
             if (held < 3) fr.register({}, held + 1); print('held ' + held); }); \
             fr.register({}, 0);",
        );
        let mut rounds = 0;
        loop {
            settle(rt);
            if !rt.run_finalization_cleanup().unwrap() {
                break;
            }
            rounds += 1;
            assert!(rounds < 10, "too many rounds");
        }
        assert_eq!(printed(rt), ["held 0", "held 1", "held 2", "held 3"]);
    });
}

#[test]
fn weakmap_values_survive_collections_inside_callbacks() {
    both_modes(|rt, _| {
        let out = eval(
            rt,
            "const m = new WeakMap(); const keys = []; \
             for (let i = 0; i < 100; i++) { const k = {}; keys.push(k); \
             m.getOrInsertComputed(k, () => { gc(); return { v: i }; }); } \
             gc(); keys.reduce((s, k) => s + m.get(k).v, 0)",
        );
        assert_eq!(out, "4950");
    });
}
