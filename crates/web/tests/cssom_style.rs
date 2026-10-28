//! `element.style` and the `CSS` namespace of feature 7a with the runtime
//! of a document: the cases of the Chromium probe
//! `tools/probes/web-cssom-style.json` (the `style` attribute and its
//! block, the property names, feature tests, the objects and their
//! descriptors, priorities, `cssText`, shorthands, values, the quirks
//! mode, `CSS.supports` and `CSS.escape`), each with and without the GC
//! stress mode. The serialization of each property's values is tested in
//! `swb_style` (`cssom/tests.rs`).

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "cssom_style/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "feature-tests",
        1,
        r#""transition=false animation=false all=false webkitAppearance=false filter=false boxShadow=false willChange=false zoom=false touchAction=false borderImage=false""#,
        "swb does not parse these properties, so feature tests see that",
    ),
    (
        "css-namespace",
        0,
        r#""true,false,true,true,true,false,true,true,true,false,true,false,false,true,true,true,false,false,false,false,false,true,true,false,true,true,true,true,true""#,
        "`font-tech()` in `@supports` is false in swb's parser (Chromium knows the font technologies)",
    ),
    (
        "css-namespace",
        2,
        r#""object [object CSS] escape,supports 1 1 supports undefined {\"value\":{},\"writable\":true,\"enumerable\":false,\"configurable\":true} {\"writable\":true,\"enumerable\":true,\"configurable\":true} true""#,
        "swb's `CSS` has only `supports` and `escape` (Chromium also `highlights`, `paintWorklet`, the unit functions)",
    ),
    (
        "review-7a1",
        4,
        r#"["", "text-decoration: line-through 2px;"]"#,
        "swb has no `text-decoration-thickness`, so a `text-decoration` with a thickness is kept as written",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Reads in a loop parse the attribute once; a write by `setAttribute` is
/// seen by the next read.
#[test]
fn style_reads_follow_the_attribute() {
    on_big_stack(|| {
        let mut rt = load(
            "<!DOCTYPE html><body><div id=d style='width: 1px'></div>",
            false,
        );
        let source = "{ const d = document.getElementById('d'); let n = 0; for (let i = 0; i < 20000; i++) { if (d.style.width === '1px') n++; } d.setAttribute('style', 'width: 2px'); [n, d.style.width, d.style.length] }";
        assert_eq!(answer(&mut rt, source), expected(r#"[20000, "2px", 1]"#));
        let writes = "{ const d = document.getElementById('d'); for (let i = 0; i < 20000; i++) d.style.left = i + 'px'; [d.style.left, d.getAttribute('style')] }";
        assert_eq!(
            answer(&mut rt, writes),
            expected(r#"["19999px", "width: 2px; left: 19999px;"]"#)
        );
    });
}

/// The `CSSStyleDeclaration` of an element survives a collection with its
/// expandos.
#[test]
fn style_object_survives_collections() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><p id=p></p>", true);
        let set = "{ const s = document.getElementById('p').style; s.mark = 7; s.color = 'red'; [s.color] }";
        assert_eq!(answer(&mut rt, set), expected(r#"["red"]"#));
        rt.collect();
        let check = "{ const s = document.getElementById('p').style; [s.mark, s.color, s === document.getElementById('p').style] }";
        assert_eq!(answer(&mut rt, check), expected(r#"[7, "red", true]"#));
    });
}

/// The blocks of elements that are not connected go when many blocks
/// exist; their values come back from the attribute, and the block of a
/// connected element keeps its order (an important declaration first).
#[test]
fn blocks_of_detached_elements_go() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=d></div>", false);
        let source = "{ const d = document.getElementById('d'); d.style.setProperty('color', 'red', 'important'); d.style.width = '1px'; const all = []; for (let i = 0; i < 10000; i++) { const e = document.createElement('p'); e.style.left = i + 'px'; all.push(e); } [d.style.cssText, all[0].style.left, all[9999].style.left] }";
        assert_eq!(
            answer(&mut rt, source),
            expected(r#"["color: red !important; width: 1px;", "0px", "9999px"]"#)
        );
    });
}
