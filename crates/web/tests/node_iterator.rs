//! `NodeIterator`, `NodeFilter` and `Document.createNodeIterator` with the runtime
//! of one document (M9 feature 15a, ADR 0028 section 17): every query of the
//! Chromium probe `tools/probes/web-node-iterator.json` (the objects and their
//! descriptors, the arguments, the traversal over each node type and mask, the
//! filters, the pre-removing steps for every way a node leaves a tree on a fixed
//! tree, on every tree of up to five nodes, and in fragments, documents and
//! templates, the parser's moves, `DOMPurify`'s loops), each with and without the GC
//! stress mode; and the checks that a probe cannot make: the collector, the
//! registry's life, bounds and time. The cases with frames are in
//! `crates/engine/tests/frames/node_iterator.rs`.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "node_iterator/cases.rs"]
mod cases;

mod common;

use common::{Case, Departure, answer, expected, load, on_big_stack};

/// The values where swb departs from Chromium on purpose: (case, query index,
/// swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "args",
        2,
        r#""TypeError: Failed to execute 'createNodeIterator' on 'Document': parameter 1 is not of type 'Node'.""#,
        "an Attr is no Node in swb (ADR 0028 section 3), so it cannot be a root",
    ),
    (
        "xml",
        0,
        "error: NotSupportedError: Failed to execute 'parseFromString' on 'DOMParser': swb does not parse XML documents.",
        "swb has no XML documents (ADR 0028 section 3), so no processing instruction or CDATA section node can be made by parsing",
    ),
    (
        "xml",
        1,
        "error: NotSupportedError: Failed to execute 'parseFromString' on 'DOMParser': swb does not parse XML documents.",
        "as above",
    ),
    (
        "xml",
        2,
        r#"["TypeError: document.createProcessingInstruction is not a function", "TypeError: document.createCDATASection is not a function", "TypeError: document.createProcessingInstruction is not a function"]"#,
        "swb has no createProcessingInstruction and no createCDATASection (roadmap backlog); the SHOW_PROCESSING_INSTRUCTION and SHOW_CDATA_SECTION bits are tested with the node types that exist",
    ),
    (
        "filter",
        1,
        r#""RangeError: vo""#,
        "an exception that script code throws while the result of acceptNode is converted passes unchanged; Chromium adds the prefix \"Failed to execute 'acceptNode' on 'NodeFilter': \" to the message",
    ),
    (
        "filter",
        6,
        r#"["TypeError: Cannot perform 'get' on a proxy that has been revoked", "true", "div#r<"]"#,
        "an exception of the engine while acceptNode is read passes unchanged; Chromium adds the prefix \"Failed to execute 'nextNode' on 'NodeIterator': \"",
    ),
];

/// The cases that the GC stress mode makes too slow for the time limit of one
/// evaluation (the collector runs at every allocation): they run without it.
const NO_STRESS: &[&str] = &["remove-trees-5"];

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

/// The object keeps its root, reference and filter across collections with no other
/// reference to them: the root is a detached subtree that only the iterator names,
/// the filter an object that only the iterator holds, and a reference that moved
/// when the node left. The wrapper of the root is the same object after each
/// collection and keeps its expando.
#[test]
fn an_iterator_keeps_what_it_names_across_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load("<!DOCTYPE html><body><p>x</p>", stress);
            let make = "{ window.it = (() => { const d = document.createElement('div'); d.innerHTML = '<p id=a>t<i id=a1></i></p><p id=b></p>'; d.expando = 7; return document.createNodeIterator(d, 1, {acceptNode(n) { return n.id === 'b' ? 3 : 1; }}); })(); window.seen = [window.it.root === window.it.root]; [1]; }";
            assert_eq!(answer(&mut rt, make), expected("[1]"));
            for _ in 0..3 {
                rt.collect();
            }
            let walk = "{ const it = window.it; const o = []; let n; while ((n = it.nextNode())) o.push(n.localName + (n.id ? '#' + n.id : '')); o.push(it.root.expando, it.root === it.root, it.root.firstChild.id, it.filter.acceptNode.length); o; }";
            assert_eq!(
                answer(&mut rt, walk),
                expected(r#"["div", "p#a", "i#a1", 7, true, "a", 1]"#),
                "stress {stress}"
            );
            // The reference leaves the tree; the new one survives collections.
            let moved = "{ const it = window.it; it.previousNode(); const gone = it.root.querySelector('#a1'); gone.remove(); [it.referenceNode.id, it.pointerBeforeReferenceNode]; }";
            assert_eq!(answer(&mut rt, moved), expected(r#"["b", true]"#));
            for _ in 0..3 {
                rt.collect();
            }
            let after =
                "{ const it = window.it; [it.referenceNode.id, it.root.querySelector('#a1')]; }";
            assert_eq!(answer(&mut rt, after), expected(r#"["b", null]"#));
        }
    });
}

/// Chromium 148's values for the page of the next test (three equal runs of
/// `swbtools probe`): the last value is `null`. After a filter caught the error of
/// its own nested call, Chromium's walk ends at once; the specification's steps
/// give `i#a1`, the node after `p#a`. The same page with a loop of six more
/// `nextNode` calls crashes Chromium's renderer ("Page crashed" in three runs), so
/// Chromium's state after a caught nested call does not follow the standard.
const NESTED_CATCH_CHROMIUM: &str = r#"["div#r", "InvalidStateError: Failed to execute 'nextNode' on 'NodeIterator': Filter function can't be recursive", "InvalidStateError: Failed to execute 'previousNode' on 'NodeIterator': Filter function can't be recursive", "undefined", "p#a", "null"]"#;

/// A filter that calls `nextNode`, `previousNode` and `detach` of its own iterator and
/// catches the `InvalidStateError` changes nothing of the walk that runs (the
/// specification's steps). swb departs from Chromium in the last value
/// ([`NESTED_CATCH_CHROMIUM`]).
#[test]
fn a_filter_that_catches_the_error_of_its_own_nested_call_leaves_the_walk_alone() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(
                "<!DOCTYPE html><body><div id=r><p id=a>t1<i id=a1></i></p><p id=b></p></div>",
                stress,
            );
            let source = "{ const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } }; const N = n => n === null ? 'null' : n.localName + (n.id ? '#' + n.id : ''); const r = document.getElementById('r'); const o = []; let it, mode = 0; it = document.createNodeIterator(r, 1, n => { if (mode === 1) { mode = 0; o.push(E(() => N(it.nextNode()))); o.push(E(() => N(it.previousNode()))); o.push(E(() => String(it.detach()))); } return 1; }); o.push(N(it.nextNode())); mode = 1; o.push(N(it.nextNode())); o.push(N(it.nextNode())); o; }";
            let swb = r#"["div#r", "InvalidStateError: Failed to execute 'nextNode' on 'NodeIterator': Filter function can't be recursive", "InvalidStateError: Failed to execute 'previousNode' on 'NodeIterator': Filter function can't be recursive", "undefined", "p#a", "i#a1"]"#;
            assert_eq!(answer(&mut rt, source), expected(swb), "stress {stress}");
            // The two lists differ in the last value only.
            assert_eq!(
                swb.replace(r#""i#a1"]"#, r#""null"]"#),
                NESTED_CATCH_CHROMIUM
            );
        }
    });
}

/// 1,000,000 iterators made and dropped in a loop all work, as in Chromium: the
/// entries of dead objects go at the next `createNodeIterator`, so the arena does not
/// keep them, and a removal after the loop is not slowed down by them.
#[test]
fn a_million_iterators_made_and_dropped_all_work() {
    on_big_stack(|| {
        let mut rt = load(
            "<!DOCTYPE html><body><div id=a><p id=b></p><p id=c></p></div>",
            false,
        );
        let source = "{ let ok = 0; const a = document.getElementById('a'); for (let i = 0; i < 1000000; i++) { const it = document.createNodeIterator(a); if (it.nextNode() === a) ok++; } ok; }";
        assert_eq!(answer(&mut rt, source), expected("1000000"));
        for _ in 0..2 {
            rt.collect();
        }
        // One more call reaps the rest; the arena holds the one new entry.
        let tail = "{ const it = document.createNodeIterator(document.getElementById('a')); it.nextNode(); it.nextNode(); document.getElementById('b').remove(); [it.referenceNode.id, it.pointerBeforeReferenceNode]; }";
        assert_eq!(answer(&mut rt, tail), expected(r#"["a", false]"#));
    });
}

/// 100,000 live iterators and a loop of removals: every removal finds the
/// iterators that point into the removed subtree through the index, so the loop
/// costs the subtrees and not the product.
#[test]
fn removals_with_100000_live_iterators_cost_the_subtrees() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=a></div>", false);
        // Each iterator points at the child of its own root; removing the children
        // moves each iterator to its root.
        let distinct = "{ const a = document.getElementById('a'); const live = []; const children = []; for (let i = 0; i < 100000; i++) { const p = document.createElement('p'); p.appendChild(document.createElement('i')); a.appendChild(p); const it = document.createNodeIterator(p); it.nextNode(); it.nextNode(); live.push(it); children.push(p.firstChild); } let before = 0; for (const it of live) { if (it.referenceNode.localName === 'i') before++; } for (const c of children) { c.remove(); } let after = 0; for (const it of live) { if (it.referenceNode === it.root && !it.pointerBeforeReferenceNode) after++; } window.live = live; [before, after]; }";
        assert_eq!(answer(&mut rt, distinct), expected("[100000, 100000]"));
        // 100,000 iterators at one node move together, and the removals of the other
        // nodes between them are not slowed down.
        let shared = "{ const a = document.getElementById('a'); const kids = Array.from(a.children); const live = []; for (let i = 0; i < 100000; i++) { const it = document.createNodeIterator(a); it.nextNode(); it.nextNode(); live.push(it); } for (const k of kids.slice().reverse()) { k.remove(); } let atRoot = 0; for (const it of live) { if (it.referenceNode === a && !it.pointerBeforeReferenceNode) atRoot++; } [atRoot, a.children.length]; }";
        assert_eq!(answer(&mut rt, shared), expected("[100000, 0]"));
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
