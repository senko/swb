//! The SVG element interfaces of feature 6c2 with the runtime of a
//! document: the cases of the Chromium probe
//! `tools/probes/web-svg-elements.json` (prototype chains and constructor
//! names of the SVG elements made by `createElementNS`, the parser and
//! `innerHTML`, `className` as `SVGAnimatedString`, `classList`,
//! `dataset`, `tabIndex`, `ownerSVGElement`, `viewportElement` and the
//! few members of the elements), each with and without the GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "svg_elements/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why). None: the probe only asks for what swb has.
const SWB_VALUES: &[Departure] = &[];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The `SVGAnimatedString` of `className` is one object per element, it
/// survives a collection, and it follows the attribute.
#[test]
fn class_name_object_survives_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(
                "<!DOCTYPE html><body><svg id=s class=a><g id=g></g></svg>",
                stress,
            );
            let set = "{ const s = document.getElementById('s'); window.c = s.className; s.className.mark = 1; [typeof c.baseVal] }";
            assert_eq!(answer(&mut rt, set), expected(r#"["string"]"#));
            rt.collect();
            let check = "{ const s = document.getElementById('s'); s.setAttribute('class', 'b c'); [c === s.className, c.mark, c.baseVal, s.classList.length] }";
            assert_eq!(
                answer(&mut rt, check),
                expected(r#"[true, 1, "b c", 2]"#),
                "stress {stress}"
            );
        }
    });
}

/// A loop of writes to `className.baseVal` on many elements is linear.
#[test]
fn class_name_writes_in_a_loop() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><svg id=s></svg>", false);
        let source = "{ const s = document.getElementById('s'); const N = 'http://www.w3.org/2000/svg'; for (let i = 0; i < 20000; i++) { const e = document.createElementNS(N, 'rect'); e.className.baseVal = 'c' + i; s.appendChild(e); } [s.childNodes.length, s.lastChild.className.baseVal, document.querySelectorAll('.c19999').length] }";
        assert_eq!(answer(&mut rt, source), expected(r#"[20000, "c19999", 1]"#));
    });
}
