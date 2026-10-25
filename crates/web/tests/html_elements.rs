//! The HTML element interfaces of feature 6c with the runtime of a
//! document: the cases of the Chromium probe
//! `tools/probes/web-html-elements.json` (prototype chains, constructors,
//! the reflected attributes of every element interface, the members of
//! `HTMLElement`, URL decomposition, token lists, tables and the element
//! lists), each with and without the GC stress mode. The form controls
//! (`forms-` cases) run in `crates/engine/tests/script_html.rs`, where the
//! engine's form state exists.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "html_elements/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "fix-html-01",
        0,
        "\"http://h.test:5/p,http://h.test:5/p,http://h:5/p,http://h:5/p,http://h.test:5/p,http://h:5/p,http://h:5/p,http://h.test:5/p,http://h2.test:5/p\"",
        "a space in the host: Chromium writes it as %20, swb follows the standard (a forbidden host code point: the setter does nothing)",
    ),
    (
        "fix-html-01",
        1,
        "\"http://h2.test:99/p,http://a:5/p,http://h:5/p,http://h:5/p,http://h2.test:5/p,http://h2.test:5/p,http://h.test:5/p,http://h:5/p,http://[::1]:9/p,http://h.test:5/p\"",
        "a space in the host (see query 0)",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `getElementsByTagName`-style lists and the element lists share one cache
/// per DOM generation: a loop over `table.rows` with changes in between
/// stays linear and sees the changes.
#[test]
fn table_rows_follow_the_dom() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><table id=t></table>", false);
        let source = "{ const t = document.getElementById('t'); for (let i = 0; i < 2000; i++) t.insertRow(); const rows = t.rows; const n = rows.length; t.deleteRow(0); [n, rows.length, t.tBodies.length, t.rows[1999 - 1].rowIndex]; }";
        assert_eq!(answer(&mut rt, source), expected("[2000, 1999, 1, 1998]"));
    });
}

/// `form.elements` of a form with many controls builds once per DOM
/// generation and finds names through an index.
#[test]
fn form_elements_with_many_controls() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><form id=f></form>", false);
        let source = "{ const f = document.getElementById('f'); for (let i = 0; i < 5000; i++) { const e = document.createElement('input'); e.name = 'n' + i; f.appendChild(e); } let found = 0; for (let i = 0; i < 5000; i++) if (f.elements['n' + i] === f[i]) found++; [f.length, found, f.elements.namedItem('n4999') === f.elements[4999]]; }";
        assert_eq!(answer(&mut rt, source), expected("[5000, 5000, true]"));
    });
}

/// A `select` with many options: `add` and `remove` in a loop, `length`.
#[test]
fn select_options_with_many_options() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><select id=s></select>", false);
        let source = "{ const s = document.getElementById('s'); for (let i = 0; i < 5000; i++) { const o = document.createElement('option'); o.value = i; s.add(o); } const n = s.length; for (let i = 0; i < 2500; i++) s.remove(0); [n, s.length, s.options[0].value, s.options.length]; }";
        assert_eq!(
            answer(&mut rt, source),
            expected(r#"[5000, 2500, "2500", 2500]"#)
        );
    });
}
