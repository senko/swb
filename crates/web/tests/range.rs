//! `AbstractRange`, `StaticRange`, `Range` and `Document.createRange` with the
//! runtime of one document (M10 sub-feature 9b-1, ADR 0039): every query of the
//! Chromium probe `tools/probes/web-range.json` that 9b-1 covers (the objects
//! and their descriptors, boundary points and errors, the comparisons,
//! `toString`, and the live updates for every kind of DOM change), each with and
//! without the GC stress mode; and the checks that a probe cannot make: the
//! collector, many ranges, deep trees.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "range/cases.rs"]
mod cases;

mod common;

use common::{Case, Departure, answer, expected, load, on_big_stack};

/// The values where swb departs from Chromium on purpose: (case, query index,
/// swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "api",
        1,
        r#"["length,name,prototype,START_TO_START,START_TO_END,END_TO_END,END_TO_START","commonAncestorContainer,START_TO_START,START_TO_END,END_TO_END,END_TO_START,cloneContents,cloneRange,collapse,compareBoundaryPoints,comparePoint,createContextualFragment,deleteContents,detach,extractContents,getBoundingClientRect,getClientRects,insertNode,intersectsNode,isPointInRange,selectNode,selectNodeContents,setEnd,setEndAfter,setEndBefore,setStart,setStartAfter,setStartBefore,surroundContents,toString,constructor","length,name,prototype","startContainer,startOffset,endContainer,endOffset,collapsed,constructor","length,name,prototype","constructor"]"#,
        "Chromium's non-standard Range.prototype.expand is absent (ADR 0039 section 5)",
    ),
    (
        "static-range",
        1,
        r#"["t:abcd,99-t:abcd,0 false","p#p,7-#document,2 false","InvalidNodeTypeError: Failed to construct 'StaticRange': Neither startContainer nor endContainer can be a DocumentType or Attribute node.","InvalidNodeTypeError: Failed to construct 'StaticRange': Neither startContainer nor endContainer can be a DocumentType or Attribute node.","TypeError: Failed to construct 'StaticRange': Failed to read the 'startContainer' property from 'StaticRangeInit': Failed to convert value to 'Node'.","t:abcd,4294967295-t:abcd,0 false","t:abcd,0-t:abcd,1 false","t:abcd,0-b,0 false","TypeError: Failed to construct 'StaticRange': Failed to read the 'startContainer' property from 'StaticRangeInit': Failed to convert value to 'Node'."]"#,
        "an Attr is no node in swb (its attributes are not nodes of the arena, ADR 0028 \
         section 3), so an Attr container fails the conversion to Node with a TypeError; \
         Chromium refuses it with InvalidNodeTypeError",
    ),
    (
        "live-more",
        6,
        r#"["t:😀a😀,0-t:😀a😀,5 \"😀a😀\" 5","t:😀a😀,2-t:😀a😀,5 \"a😀\" 3","t:😀a😀,2-t:😀a😀,5 \"a😀\" 3","t:😀a😀,3-t:😀a😀,5 \"😀\" 2","t:😀a😀,5-t:😀a😀,5 \"\" 0","t:😀a😀,5-t:😀a😀,5 \"\" 0","t:x😀a😀,3-t:x😀a😀,6","t:x😀😀,3-t:x😀😀,5","\"😀\"","t:x😀,3-t:x😀,3","\"x😀\""]"#,
        "the DOM stores UTF-8 and cannot hold a lone surrogate (ADR 0028 section 3): a \
         UTF-16 offset inside a surrogate pair rounds up to the end of the character, in \
         ranges as in CharacterData (ADR 0039 section 1); Chromium keeps the offset, and \
         toString and deleteData split the pair",
    ),
];

/// The cases and queries that run elsewhere: (case, query index or `None` for
/// all, why). The queries still run, so that none of them panics.
const ELSEWHERE: &[(&str, Option<usize>, &str)] = &[
    ("geometry", None, NEEDS_LAYOUT),
    ("geometry-more", None, NEEDS_LAYOUT),
    ("geometry-review", None, NEEDS_LAYOUT),
    ("geometry-frames", None, NEEDS_FRAMES),
    ("range-frames", None, NEEDS_FRAMES),
];

const NEEDS_LAYOUT: &str = "needs layout: crates/engine/tests/frames/range.rs runs it";
const NEEDS_FRAMES: &str = "needs frames: crates/engine/tests/frames/range.rs runs it";

/// The cases that the GC stress mode makes too slow for the time limit of one
/// evaluation (the collector runs at every allocation): they run without it.
const NO_STRESS: &[&str] = &[];

fn elsewhere(case: &str, index: usize) -> bool {
    ELSEWHERE
        .iter()
        .any(|l| l.0 == case && l.1.is_none_or(|i| i == index))
}

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
        if elsewhere(case.name, index) {
            continue;
        }
        if got != expected(want) {
            failures.push(format!(
                "{} #{index} (stress {stress}): {query}\n  expected: {want}\n  actual:   {got}",
                case.name
            ));
        }
    }
    failures
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

/// A range keeps its boundary points across collections with no other reference
/// to their nodes: the start is in a detached subtree that only the range names,
/// and the wrapper of the container is the same object after each collection.
#[test]
fn a_range_keeps_its_nodes_across_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load("<!DOCTYPE html><body><p>x</p>", stress);
            let make = "{ window.r = (() => { const d = document.createElement('div'); d.innerHTML = '<b>ab</b>cd'; d.expando = 7; const r = new Range(); r.setStart(d.firstChild.firstChild, 1); r.setEnd(d, 2); return r; })(); [1]; }";
            assert_eq!(answer(&mut rt, make), expected("[1]"));
            for _ in 0..3 {
                rt.collect();
            }
            let read = "{ const r = window.r; [r.endContainer.expando, String(r), r.startOffset, r.commonAncestorContainer === r.endContainer]; }";
            assert_eq!(
                answer(&mut rt, read),
                expected(r#"[7, "bcd", 1, true]"#),
                "stress {stress}"
            );
            // The tree changes after the collections; the range follows.
            let change = "{ const r = window.r; r.endContainer.firstChild.remove(); [r.startContainer.localName, r.startOffset, r.endOffset]; }";
            assert_eq!(answer(&mut rt, change), expected(r#"["div", 0, 1]"#));
        }
    });
}

/// A million ranges made and dropped in a loop, then 100,000 live ranges and a
/// loop of changes: the entries of the dropped ranges go, and the changes stay
/// cheap (no limit on live ranges, as in Chromium).
#[test]
fn many_ranges_made_and_dropped_and_many_live() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=a>x</div>", false);
        let made = "{ const a = document.getElementById('a'); for (let i = 0; i < 1000000; i++) { const r = document.createRange(); r.setStart(a.firstChild, 1); } [1]; }";
        assert_eq!(answer(&mut rt, made), expected("[1]"));
        rt.collect();
        let live = "{ const a = document.getElementById('a'); window.keep = []; for (let i = 0; i < 100000; i++) { const r = new Range(); r.setStart(a, 1); window.keep.push(r); } for (let i = 0; i < 1000; i++) { const t = document.createTextNode('y'); document.body.appendChild(t); t.remove(); } a.prepend('q'); [window.keep[99999].startOffset, window.keep[0].startOffset]; }";
        assert_eq!(answer(&mut rt, live), expected("[2, 2]"));
    });
}

/// A range at the bottom of a chain of 100,000 levels: the comparisons, the
/// common ancestor, `toString`, and the removal of the top of the chain do not
/// recurse.
#[test]
fn a_range_in_a_deep_chain() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=a></div>", false);
        let source = "{ const root = document.getElementById('a'); let cur = root; for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); cur.appendChild(d); cur = d; } cur.append('deep'); const r = new Range(); r.setStart(cur.firstChild, 1); r.setEnd(root, 1); const s = new Range(); s.setStart(root, 0); const out = [r.commonAncestorContainer === root, String(r), r.compareBoundaryPoints(Range.START_TO_START, s), r.comparePoint(root, 0), r.intersectsNode(cur)]; root.firstChild.remove(); out.push(r.startContainer === root, r.startOffset, r.collapsed); out; }";
        assert_eq!(
            answer(&mut rt, source),
            expected(r#"[true, "eep", 1, -1, true, true, 0, true]"#)
        );
    });
}
