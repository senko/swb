//! The queries and the markup of feature 6b with the runtime of a document:
//! the cases of the Chromium probe `tools/probes/web-dom-query.json` (the
//! selector methods and their exceptions, the live collections, the
//! serialization, `innerHTML`, `outerHTML` and `insertAdjacent*`, the
//! parse contexts, `template.content`, `DOMParser`, `document.implementation`
//! and `window[name]` with several elements), each with and without the GC
//! stress mode; and checks that need more than one page.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "dom_query/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "collections-01",
        5,
        "false",
        "Chromium keeps one collection per node and arguments while it lives; swb makes a new one at each call (a cache keyed by the arguments would hold the lists alive)",
    ),
    (
        "parse-context-01",
        8,
        "\"\\\"a\u{fffd}b\\\"\"",
        "a lone surrogate becomes U+FFFD in the DOM (ADR 0028 section 3, strings)",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A loop over a live `getElementsByTagName('*')` list builds the list once
/// while the DOM does not change, and again after each change.
#[test]
fn live_lists_follow_the_dom() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><ul id=u></ul>", false);
        let source = "{ const u = document.getElementById('u'); const all = document.getElementsByTagName('li'), cls = document.getElementsByClassName('x'), named = document.getElementsByName('n'); const log = []; for (let i = 0; i < 3000; i++) { const li = u.appendChild(document.createElement('li')); li.className = 'x'; li.setAttribute('name', 'n'); } log.push(all.length, cls.length, named.length); u.firstChild.remove(); log.push(all.length, cls.length, named.length); u.firstChild.className = ''; log.push(cls.length); log; }";
        assert_eq!(
            answer(&mut rt, source),
            expected("[3000, 3000, 3000, 2999, 2999, 2999, 2998]")
        );
    });
}

/// A selector that matches the elements of a tree, with `:scope`, the
/// static list and the cache of parsed selectors.
#[test]
fn queries_over_a_large_tree() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=r></div>", false);
        let build = "{ const r = document.getElementById('r'); for (let i = 0; i < 2000; i++) { const d = r.appendChild(document.createElement('div')); d.className = 'c' + (i % 3); d.appendChild(document.createElement('span')); } [r.querySelectorAll('*').length, r.querySelectorAll('.c1 > span').length, r.querySelectorAll(':scope > div').length, r.querySelector('div:nth-child(1500)').className, document.querySelectorAll('div div div').length]; }";
        assert_eq!(
            answer(&mut rt, build),
            expected(r#"[4000, 667, 2000, "c2", 0]"#)
        );
        let repeated = "{ let n = 0; for (let i = 0; i < 3000; i++) if (document.body.children[0].matches('#r.nope, #r')) n++; n; }";
        assert_eq!(answer(&mut rt, repeated), expected("3000"));
    });
}

/// `innerHTML` of a tree 100,000 levels deep reads and writes without a
/// stack overflow.
#[test]
fn a_deep_tree_serializes() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=r></div>", false);
        let build = "{ const r = document.getElementById('r'); let p = r; for (let i = 0; i < 100000; i++) p = p.appendChild(document.createElement('i')); const html = r.innerHTML; const copy = document.createElement('div'); copy.innerHTML = html.slice(0, 3000); [html.length, copy.getElementsByTagName('i').length > 0]; }";
        assert_eq!(answer(&mut rt, build), expected("[700000, true]"));
    });
}
