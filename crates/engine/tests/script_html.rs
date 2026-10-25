//! Form controls from scripts (M8 feature 6c): the `value`, `checked`,
//! `selected` and `selectedIndex` IDL attributes read and write the
//! engine's form state, and a write reaches the rendering and the
//! selectors `:checked`, `:valid` and `:placeholder-shown`. The cases of
//! the Chromium probe `tools/probes/web-html-elements.json` that start
//! with `forms-` run here, in a page with JavaScript on; the tests below
//! check what a probe cannot see (the display list, the states of the
//! engine, hostile sizes).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Size};
use swb_layout::FragmentRef;
use swb_net::NetworkFetcher;
use swb_style::Rgba;

mod common;
use common::{Site, finish_loading, new_page, node};

/// A probe case: its name, its page and its queries with Chromium's
/// values.
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

#[path = "script_html/cases.rs"]
mod cases;

/// The cases of `tools/probes/web-image-option.json` that need the form
/// state (the selectedness of options made by `new Option`).
#[path = "script_html/option_cases.rs"]
mod option_cases;

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        "forms-input-01",
        0,
        r##""\"a\",\"\",\"\",\"on\",\"h\",\"\",\"#000000\",\"\"""##,
        "a color name in the value of a color input is not converted to hex",
    ),
    (
        "forms-select-01",
        8,
        r#""<option>two</option><option>three</option><option value=\"1\">one</option><option>four</option>,0,<option>two</option><option value=\"1\">one</option><option>four</option>,<option value=\"1\">one</option><option>four</option>,2""#,
        "a select that grows between two reads keeps no memory of the option that was selected after the first insertion",
    ),
    (
        "forms-form-01",
        10,
        r#""number,object,number,function""#,
        "a control named like a built-in member of the form does not hide it (no [LegacyOverrideBuiltIns]: it would make every property read build the list of controls)",
    ),
    (
        "forms-fix-01",
        5,
        r#""false,0,true,2,false""#,
        "an option that leaves a select whose state no read has built yet is not selected (Chromium keeps the implicit selection of the first option); after a read it is",
    ),
    (
        "forms-fix-01",
        30,
        r#""true,false""#,
        "a radio button that a script checked while it was in the document, removed and inserted again does not win over a button that a script checked in between (the order of the insertion is not known)",
    ),
    (
        "forms-fix-01",
        38,
        r#""5,7,8""#,
        "a range input keeps the value that a script wrote; Chromium clamps it again when `max` changes",
    ),
    (
        "forms-form-01",
        11,
        r#""string,String,""#,
        "a control named like a built-in member of the form does not hide it (see query 10)",
    ),
    (
        "forms-form-01",
        1,
        r#""a2,sel,a2,a2,sel,,,,,fs,ou,bt,ob,nest,ext""#,
        "an img of a form is not a named property of the form",
    ),
];

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A page with JavaScript on that shows `html`.
fn open(site: &Site, html: &str) -> Page {
    let url = site.page("page.html", html);
    let mut page = new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(600.0, 400.0));
    page.set_javascript(true);
    page.navigate(url);
    finish_loading(&mut page, Duration::from_secs(30));
    page
}

/// Runs a script as a task and lays the page out.
fn run(page: &mut Page, source: &str) -> String {
    let value = match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{source}: {other:?}"),
    };
    page.update_layout();
    value
}

/// The probe cases of the form controls.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let mut failures = Vec::new();
        for case in cases::CASES.iter().chain(option_cases::CASES) {
            let site = Site::new(&format!("script-html-{}", case.name));
            let mut page = open(&site, case.html);
            for (index, &(query, chromium)) in case.queries.iter().enumerate() {
                let want = SWB_VALUES
                    .iter()
                    .find(|v| v.0 == case.name && v.1 == index)
                    .map_or(chromium, |v| v.2);
                let got = match page.evaluate(query, JsonForm::Strict) {
                    Some(Evaluation::Value(text)) => text,
                    other => format!("{other:?}"),
                };
                let want_value: serde_json::Value = serde_json::from_str(want).unwrap();
                let got_value: serde_json::Value = serde_json::from_str(&got)
                    .unwrap_or_else(|_| serde_json::Value::String(got.clone()));
                if got_value != want_value {
                    failures.push(format!(
                        "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                        case.name
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

const BODY: &str = "<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>";

/// The text of all text fragments of the page, in tree order.
fn shown_text(page: &mut Page) -> String {
    page.update_layout();
    let mut shown = String::new();
    page.fragments().unwrap().walk(|f, _| {
        if let FragmentRef::Text(t) = f {
            shown.push_str(&t.text);
            shown.push('|');
        }
    });
    shown
}

/// A script write of `value` reaches the engine's state, the display list
/// and the selectors.
#[test]
fn a_written_value_is_painted_and_restyles() {
    on_page_thread(|| {
        let site = Site::new("script-html-value");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<style>input:placeholder-shown {{ color: red }} input:valid {{ outline: 1px solid blue }}</style><input id=t placeholder=hint><input id=r required><textarea id=a></textarea>"
            ),
        );
        let t = node(&page, "t");
        let red = Rgba::rgb(255, 0, 0);
        assert_eq!(page.styles().unwrap().get(t).unwrap().color, red);
        run(&mut page, "document.getElementById('t').value = 'typed'");
        assert_eq!(page.control_value(t).as_deref(), Some("typed"));
        assert_ne!(page.styles().unwrap().get(t).unwrap().color, red);
        assert!(shown_text(&mut page).contains("typed"));
        run(
            &mut page,
            "document.getElementById('a').value = 'line1\\nline2'",
        );
        let a = node(&page, "a");
        assert_eq!(page.control_value(a).as_deref(), Some("line1\nline2"));
        let text = shown_text(&mut page);
        assert!(text.contains("line1") && text.contains("line2"), "{text}");
        // `:valid` follows a required field.
        let answer = run(
            &mut page,
            "const r = document.getElementById('r'); const a = document.querySelectorAll('input:valid').length; r.value = 'x'; [a, document.querySelectorAll('input:valid').length]",
        );
        assert_eq!(answer, "[1,2]");
    });
}

/// A script write of `checked` and `selectedIndex` changes the controls,
/// the rendering of `:checked` rules and `querySelectorAll(':checked')`.
#[test]
fn written_checkedness_and_selection_restyle() {
    on_page_thread(|| {
        let site = Site::new("script-html-checked");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<style>:checked {{ color: red }}</style><input id=c type=checkbox><input id=ra type=radio name=g checked><input id=rb type=radio name=g><select id=s><option id=o1>one</option><option id=o2>two</option></select>"
            ),
        );
        let red = Rgba::rgb(255, 0, 0);
        let color =
            |page: &Page, id: &str| page.styles().unwrap().get(node(page, id)).unwrap().color;
        assert_ne!(color(&page, "c"), red);
        assert_eq!(color(&page, "ra"), red);
        assert_eq!(
            run(&mut page, "document.querySelectorAll(':checked').length"),
            "2"
        );
        run(&mut page, "document.getElementById('c').checked = true");
        assert_eq!(color(&page, "c"), red);
        assert_eq!(
            run(&mut page, "document.querySelectorAll(':checked').length"),
            "3"
        );
        // The newly checked radio button wins.
        run(&mut page, "document.getElementById('rb').checked = true");
        assert_eq!(color(&page, "rb"), red);
        assert_ne!(color(&page, "ra"), red);
        assert_eq!(page.control_checked(node(&page, "ra")), Some(false));
        assert_eq!(
            run(
                &mut page,
                "document.querySelectorAll('option:checked').length"
            ),
            "1"
        );
        run(&mut page, "document.getElementById('s').selectedIndex = 1");
        assert_eq!(color(&page, "o2"), red);
        assert_ne!(color(&page, "o1"), red);
        assert_eq!(page.control_value(node(&page, "s")).as_deref(), Some("two"));
        assert_eq!(
            run(&mut page, "document.querySelector('option:checked').id"),
            "\"o2\""
        );
        // A change of the DOM keeps what the script wrote.
        run(
            &mut page,
            "document.body.appendChild(document.createElement('p'))",
        );
        assert_eq!(
            run(&mut page, "document.getElementById('c').checked"),
            "true"
        );
        assert_eq!(
            run(&mut page, "document.getElementById('s').selectedIndex"),
            "1"
        );
        // `defaultChecked` and the `checked` attribute do not change a dirty control.
        run(
            &mut page,
            "document.getElementById('c').removeAttribute('checked'); document.getElementById('c').setAttribute('checked', '')",
        );
        run(&mut page, "document.getElementById('c').checked = false");
        run(
            &mut page,
            "document.getElementById('c').setAttribute('checked', '')",
        );
        assert_eq!(
            run(&mut page, "document.getElementById('c').checked"),
            "false"
        );
    });
}

/// The value of a control that a script removes and inserts again, and of
/// a control that a script makes, is kept.
#[test]
fn values_survive_moves_and_new_controls() {
    on_page_thread(|| {
        let site = Site::new("script-html-moves");
        let mut page = open(&site, &format!("{BODY}<div id=d><input id=i></div>"));
        run(
            &mut page,
            "const i = document.getElementById('i'); i.value = 'kept'; i.remove(); document.body.appendChild(document.createElement('b')); document.body.appendChild(document.createElement('b')); document.getElementById('d').appendChild(i);",
        );
        assert_eq!(
            page.control_value(node(&page, "i")).as_deref(),
            Some("kept")
        );
        run(
            &mut page,
            "const n = document.createElement('input'); n.id = 'n'; n.value = 'fresh'; document.getElementById('d').appendChild(n);",
        );
        assert_eq!(
            page.control_value(node(&page, "n")).as_deref(),
            Some("fresh")
        );
        assert!(shown_text(&mut page).contains("fresh"));
    });
}

/// A loop over many controls and options stays fast: reads and writes of
/// values cost O(1) after a change of the DOM.
#[test]
fn many_controls_stay_linear() {
    on_page_thread(|| {
        let site = Site::new("script-html-many");
        let mut page = open(
            &site,
            &format!("{BODY}<form id=f></form><select id=s></select>"),
        );
        let started = std::time::Instant::now();
        let answer = run(
            &mut page,
            "const f = document.getElementById('f'); const s = document.getElementById('s'); for (let i = 0; i < 20000; i++) { const e = document.createElement('input'); e.value = 'v' + i; f.appendChild(e); if (e.value !== 'v' + i) throw new Error('value'); const o = document.createElement('option'); o.value = i; s.add(o); } [f.length, s.length, f.elements[19999].value]",
        );
        assert_eq!(answer, "[20000,20000,\"v19999\"]");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
    });
}

/// The values that scripts write to controls have a budget of 32 MiB: the
/// task that goes past it ends, the page keeps the values written so far
/// and later tasks run (ADR 0028 section 16).
#[test]
fn the_values_of_controls_have_a_budget() {
    on_page_thread(|| {
        let site = Site::new("script-html-budget");
        let mut page = open(&site, &format!("{BODY}<div id=d style=display:none></div>"));
        let source = "const big = 'a'.repeat(1 << 20); const d = document.getElementById('d'); for (let i = 0; i < 40; i++) { const t = document.createElement('textarea'); d.appendChild(t); t.value = big; } 'not reached'";
        match page.evaluate(source, JsonForm::Strict) {
            Some(Evaluation::Error(message)) => {
                assert!(message.contains("terminated"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        let kept = run(
            &mut page,
            "document.getElementById('d').children.length + ',' + document.getElementById('d').children[0].value.length",
        );
        assert_eq!(kept, "\"33,1048576\"");
        // A replaced value gives its bytes back.
        let again = run(
            &mut page,
            "const d2 = document.getElementById('d'); for (let i = 0; i < 32; i++) d2.children[i].value = ''; const t2 = d2.children[0]; t2.value = 'b'.repeat(1 << 20); t2.value.length",
        );
        assert_eq!(again, "1048576");
    });
}

/// A select that leaves the document keeps what a script selected,
/// whether or not a read rebuilt the form state in between; so does an
/// option that leaves its select. A clone of a control gets the value
/// and the checkedness of the original.
#[test]
fn detached_selects_and_clones_keep_their_state() {
    on_page_thread(|| {
        let site = Site::new("script-html-detached");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<select id=s><option>a<option>b<option>c</select><select id=o><option>x<option>y</select><input id=i value=v><input id=c type=checkbox>"
            ),
        );
        let answer = run(
            &mut page,
            "const s = document.getElementById('s'); const o = document.getElementById('o'); s.selectedIndex = 2; o.selectedIndex = 1; s.remove(); const r = [s.selectedIndex, s.options[2].selected]; document.body.appendChild(s); r.push(s.value); const i = document.getElementById('i'); i.value = 'w'; const c = document.getElementById('c'); c.checked = true; const i2 = i.cloneNode(), c2 = c.cloneNode(); r.push(i2.value, c2.checked, document.importNode(i, true).value); r",
        );
        assert_eq!(answer, "[2,true,\"c\",\"w\",true,\"w\"]");
    });
}
