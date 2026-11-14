//! `TreeWalker`, `Document.createTreeWalker` and `ariaNotify` with the runtime of
//! one document (M10 feature 9a, ADR 0028 section 17): every query of the Chromium
//! probe `tools/probes/web-tree-walker.json` (the objects and their descriptors,
//! the arguments, the seven methods on a fixed tree from every start node with
//! every rule of the filter, the masks, filter values and objects, re-entry,
//! mutations, roots and nodes that move to other documents, shadow trees and
//! frames, `ariaNotify`), each with and without the GC stress mode; and the
//! checks that a probe cannot make: the collector, bounds and time.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "tree_walker/cases.rs"]
mod cases;

mod common;

use common::{Case, Departure, answer, expected, load, on_big_stack};

/// The values where swb departs from Chromium on purpose: (case, query index,
/// swb's value, why).
const SWB_VALUES: &[Departure] = &[];

/// The cases that the GC stress mode makes too slow for the time limit of one
/// evaluation (the collector runs at every allocation): they run without it.
const NO_STRESS: &[&str] = &[];

/// The differences of one case from Chromium's values, in a new runtime.
fn check(case: &Case, stress: bool) -> Vec<String> {
    let mut failures = Vec::new();
    let mut rt = load(case.html, stress);
    for (index, &(query, chromium)) in case.queries.iter().enumerate() {
        let want = SWB_VALUES
            .iter()
            .find(|v| v.0 == case.name && v.1 == index)
            .map_or(chromium, |v| v.2);
        let got = answer(&mut rt, query);
        if got != expected(want) {
            failures.push(format!(
                "{} #{index} (stress {stress}): {query}\n  expected: {want}\n  actual:   {got}",
                case.name
            ));
        }
    }
    failures
}

/// The object keeps its root, current node and filter across collections with no other
/// reference to them: the root is a detached subtree that only the walker names, the
/// filter an object that only the walker holds. The wrapper of the root is the same
/// object after each collection and keeps its expando.
#[test]
fn a_walker_keeps_what_it_names_across_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load("<!DOCTYPE html><body><p>x</p>", stress);
            let make = "{ window.w = (() => { const d = document.createElement('div'); d.innerHTML = '<p id=a>t<i id=a1></i></p><p id=b></p>'; d.expando = 7; return document.createTreeWalker(d, 1, {acceptNode(n) { return n.id === 'b' ? 3 : 1; }}); })(); window.seen = [window.w.root === window.w.root]; [1]; }";
            assert_eq!(answer(&mut rt, make), expected("[1]"));
            for _ in 0..3 {
                rt.collect();
            }
            let walk = "{ const w = window.w; const o = []; let n; while ((n = w.nextNode())) o.push(n.localName + (n.id ? '#' + n.id : '')); o.push(w.root.expando, w.root === w.root, w.root.firstChild.id, w.filter.acceptNode.length, w.currentNode.id); o; }";
            assert_eq!(
                answer(&mut rt, walk),
                expected(r#"["p#a", "i#a1", 7, true, "a", 1, "a1"]"#),
                "stress {stress}"
            );
            // The current node leaves the tree; the walker keeps it and the node
            // survives collections.
            let moved = "{ const w = window.w; const gone = w.currentNode; gone.remove(); [w.currentNode === gone, w.currentNode.isConnected, w.root.querySelector('#a1')]; }";
            assert_eq!(answer(&mut rt, moved), expected("[true, false, null]"));
            for _ in 0..3 {
                rt.collect();
            }
            let after =
                "{ const w = window.w; [w.currentNode.id, w.parentNode(), w.currentNode.id]; }";
            assert_eq!(answer(&mut rt, after), expected(r#"["a1", null, "a1"]"#));
        }
    });
}

/// A walk with no filter over a chain of 100,000 nested elements is native and
/// does not recurse: down, up, to the parents and to the first children.
#[test]
fn a_deep_chain_is_walked_in_every_direction() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=a></div>", false);
        let source = "{ const root = document.getElementById('a'); let cur = root; for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); cur.appendChild(d); cur = d; } const w = document.createTreeWalker(root, 1); let down = 0; while (w.nextNode()) down++; let up = 0; while (w.previousNode()) up++; w.currentNode = cur; let parents = 0; while (w.parentNode()) parents++; let kids = 0; while (w.firstChild()) kids++; [down, up, parents, kids]; }";
        assert_eq!(
            answer(&mut rt, source),
            expected("[100000, 100000, 100000, 100000]")
        );
    });
}

/// A walk whose filter skips everything, from a current node outside the root's tree
/// (the standard filters the same node for ever; Chromium returns `null`), ends
/// for every method.
#[test]
fn a_current_node_outside_the_root_ends_every_walk() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=a></div>", false);
        let source = "{ const root = document.getElementById('a'); for (let i = 0; i < 1000; i++) root.appendChild(document.createElement('i')); const out = document.createElement('p'); out.appendChild(document.createElement('b')); const w = document.createTreeWalker(root, 1, n => 3); w.currentNode = out.firstChild; ['nextNode', 'previousNode', 'nextSibling', 'previousSibling', 'firstChild', 'lastChild', 'parentNode'].map(m => { const r = w[m](); return r === null ? 'null' : r.localName; }); }";
        assert_eq!(
            answer(&mut rt, source),
            expected(r#"["null", "null", "null", "null", "null", "null", "null"]"#)
        );
    });
}

/// After its filter throws, a walker works again (the "active" flag is unset also
/// when the filter throws), and a nested call that fails inside the filter keeps the
/// outer call's state. Chromium 148's values (the 9a review's case `rv-throw`):
/// `["RangeError: x", "a", "Error: y", "Error: y"]`.
#[test]
fn a_walker_works_again_after_its_filter_throws() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(
                "<!DOCTYPE html><body><div id=r><p id=a>t</p><p id=b>u</p></div>",
                stress,
            );
            let source = "{ const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } }; const r = document.getElementById('r'); let boom = true; const w = document.createTreeWalker(r, 1, n => { if (boom) throw new RangeError('x'); return 1; }); const a = E(() => w.nextNode()); boom = false; const b = E(() => w.nextNode() && w.currentNode.id); let w3; w3 = document.createTreeWalker(r, 1, n => { try { w3.nextNode(); } catch (e) {} throw new Error('y'); }); const c1 = E(() => w3.nextNode()); const c2 = E(() => w3.nextNode()); [a, b, c1, c2]; }";
            assert_eq!(
                answer(&mut rt, source),
                expected(r#"["RangeError: x", "a", "Error: y", "Error: y"]"#)
            );
        }
    });
}

/// A walk over many nodes with a mask that shows none of them is native work with
/// no filter call, and ends.
#[test]
fn a_walk_over_many_nodes_with_a_mask_that_skips_all_ends() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=a></div>", false);
        let source = "{ const root = document.getElementById('a'); for (let i = 0; i < 300000; i++) root.appendChild(document.createElement('i')); const w = document.createTreeWalker(root, 4); [w.nextNode(), w.firstChild(), w.lastChild(), (w.currentNode = root.lastChild, w.previousNode()), w.nextSibling(), w.previousSibling()].map(String); }";
        assert_eq!(
            answer(&mut rt, source),
            expected(r#"["null", "null", "null", "null", "null", "null"]"#)
        );
    });
}

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = on_big_stack(|| {
        let mut failures = Vec::new();
        for case in cases::CASES {
            for stress in [false, true] {
                if stress && NO_STRESS.contains(&case.name) {
                    continue;
                }
                failures.extend(check(case, stress));
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
