//! The DOM changes of feature 6a with the runtime of a document: the cases
//! of the Chromium probe `tools/probes/web-dom-mutation.json` (the results
//! and the exceptions of `insertBefore`, `appendChild`, `replaceChild`,
//! `removeChild`, the `ParentNode` and `ChildNode` methods, `cloneNode`,
//! `normalize`, `compareDocumentPosition`, `isEqualNode`, `textContent`,
//! the attribute methods, `Attr`, `NamedNodeMap`, `classList`, `dataset`,
//! the node factories and `getElementById`, and scripts that remove or
//! move script elements during the parse), each with and without the GC
//! stress mode; and checks that need more than one page.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "dom_mutation/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "m090",
        0,
        r#"",,,false,true""#,
        "Attr extends EventTarget, not Node (attributes are not nodes of the arena, ADR 0028 section 3)",
    ),
    (
        "m451",
        0,
        r#""1|q:c=z@urn:q""#,
        "setAttribute('Q:C') finds the attribute with the qualified name q:c in any namespace and changes it (DOM standard); Chromium adds a second one",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Attr` objects, lists and maps keep their identity and expandos across
/// collections, and an `Attr` of a removed attribute keeps its value.
#[test]
fn views_survive_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(
                "<!DOCTYPE html><body><div id=a class='x y' data-k=v>t</div>",
                stress,
            );
            let set = "{ const a = document.getElementById('a'); const n = a.getAttributeNode('class'); n.mark = 1; a.attributes.mark = 2; a.classList.mark = 3; a.dataset.mark = '4'; a.children.mark = 5; window.gone = a.getAttributeNode('data-k'); a.removeAttribute('data-k'); [1]; }";
            assert_eq!(answer(&mut rt, set), expected("[1]"));
            rt.collect();
            let check = "{ const a = document.getElementById('a'); [a.getAttributeNode('class').mark, a.attributes.mark, a.classList.mark, a.dataset.mark, a.children.mark, a.attributes[1] === a.getAttributeNode('class'), window.gone.value, window.gone.ownerElement, a.attributes.length]; }";
            assert_eq!(
                answer(&mut rt, check),
                expected(r#"[1, 2, 3, "4", 5, true, "v", null, 3]"#),
                "stress {stress}"
            );
        }
    });
}

/// A loop over a collection of 3,000 children and the live lists after
/// changes (the caches rebuild once per DOM generation).
#[test]
fn live_lists_follow_the_dom() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><ul id=u></ul>", false);
        let source = "{ const u = document.getElementById('u'); const c = u.children, n = u.childNodes; const log = []; for (let i = 0; i < 3000; i++) { u.appendChild(document.createElement('li')); } log.push(c.length, n.length); u.firstChild.remove(); log.push(c.length, n.length); u.prepend('t'); log.push(c.length, n.length, n[0].nodeType); log; }";
        assert_eq!(
            answer(&mut rt, source),
            expected("[3000, 3000, 2999, 2999, 2999, 3000, 3]")
        );
    });
}

/// The parsed tokens of a `classList` are kept per DOM generation: every
/// change of the attribute (by the list, `className`, `setAttribute`, an
/// `Attr`) shows in the next read.
#[test]
fn token_lists_follow_the_attribute() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body class='x'>", false);
        let source = "{ const e = document.body, l = e.classList, r = []; e.className = 'a b a'; r.push(l.length, l[1], l.contains('b')); e.className = 'c'; r.push(l.length, l[0], l.contains('b'), l.item(1)); l.add('d'); r.push(l.length, l.item(1)); e.setAttribute('class', ' p  q r '); r.push(l.length, l[2]); e.getAttributeNode('class').value = 'z'; r.push(l.length, l[0]); e.removeAttribute('class'); r.push(l.length); r; }";
        assert_eq!(
            answer(&mut rt, source),
            expected(r#"[2, "b", true, 1, "c", false, null, 2, "d", 3, "r", 1, "z", 0]"#)
        );
    });
}

/// Scripts in a detached subtree wait for the connection; the search for
/// them and the test of the connection advance in turns, and the scripts
/// of one insertion run in tree order.
#[test]
fn scripts_run_when_their_subtree_is_connected() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body>", false);
        let source = "{ window.log = []; const mk = (t) => { const s = document.createElement('script'); s.textContent = 'log.push(\"' + t + '\")'; return s; }; const d = document.createElement('div'); let w = d; for (let i = 0; i < 50; i++) { const x = document.createElement('div'); w.appendChild(x); w = x; } w.appendChild(mk('deep')); const f = document.createDocumentFragment(); f.appendChild(mk('f1')); f.appendChild(mk('f2')); d.appendChild(mk('d0')); log.push('detached'); document.body.appendChild(d); log.push('mid'); document.body.appendChild(f); log; }";
        assert_eq!(
            answer(&mut rt, source),
            expected(r#"["detached", "deep", "d0", "mid", "f1", "f2"]"#)
        );
    });
}

/// The limits end the task: past the node limit `createElement` terminates.
#[test]
fn the_node_limit_ends_the_task() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body>", false);
        let value = rt.evaluate(
            "for (;;) document.body.appendChild(document.createElement('i'));",
            swb_web::JsonForm::Strict,
        );
        assert!(
            matches!(&value, swb_web::Evaluation::Error(m) if m.contains("node limit")),
            "{value:?}"
        );
    });
}
