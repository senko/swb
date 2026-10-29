//! `structuredClone()` and the serialization behind `history.state` (M8
//! feature 8a1, ADR 0028 section 13): the cases of the Chromium probe
//! `tools/probes/web-structured-clone.json` (primitives, wrapper objects,
//! objects, arrays with holes, `Date`, `RegExp`, `Map`, `Set`, buffers,
//! typed arrays, `DataView`, errors, `DOMException`, transfer, the errors
//! and their messages, deep and wide graphs), each with and without the GC
//! stress mode; and checks that need more than one page.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "structured_clone/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why). None yet.
const SWB_VALUES: &[Departure] = &[];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A long array clones, and a graph deeper than the limit (1,000,000 levels)
/// fails with the `RangeError` without a stack overflow; the walks are
/// iterative. (The probe's `limits` case is not in `cases.rs`: the GC stress
/// mode takes minutes on graphs of 100,000 objects.)
#[test]
fn deep_and_wide_graphs_clone_iteratively() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body>", false);
        let wide = "{ const a = []; for (let i = 0; i < 300000; i++) a.push({i, s: 'x' + i, d: [i]}); const c = structuredClone(a); [c.length, c[299999].s, c[5].d[0], c !== a]; }";
        assert_eq!(
            answer(&mut rt, wide),
            expected(r#"[300000, "x299999", 5, true]"#)
        );
        let deep = "{ let o = {}; const root = o; for (let i = 0; i < 1500; i++) { o.n = [{}]; o = o.n[0]; } const c = structuredClone(root); let d = 0, p = c; while (p.n) { p = p.n[0]; d++; } d; }";
        assert_eq!(answer(&mut rt, deep), expected("1500"));
        let too_deep = "{ let o = {}; const root = o; for (let i = 0; i < 1000000; i++) { o.n = {}; o = o.n; } try { structuredClone(root); 'cloned'; } catch (e) { e.name + ':' + e.message; } }";
        assert_eq!(
            answer(&mut rt, too_deep),
            expected(
                r#""RangeError:Failed to execute 'structuredClone' on 'Window': Maximum call stack size exceeded""#
            )
        );
    });
}

/// A graph of many shared references and cycles keeps its shape, and the
/// clone does not refer to the original.
#[test]
fn shared_references_and_cycles_keep_their_shape() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body>", true);
        let source = "{ const shared = {k: 1}; const ring = []; for (let i = 0; i < 50; i++) ring.push({shared, next: null, i}); for (let i = 0; i < 50; i++) ring[i].next = ring[(i + 1) % 50]; const m = new Map([[shared, ring]]); const c = structuredClone({ring, m, shared}); const k = [...c.m.keys()][0]; [c.ring[49].next === c.ring[0], c.ring.every((r) => r.shared === c.shared), k === c.shared, c.m.get(k) === c.ring, c.shared !== shared, c.ring[7].next.i]; }";
        assert_eq!(
            answer(&mut rt, source),
            expected("[true, true, true, true, true, 8]")
        );
    });
}
