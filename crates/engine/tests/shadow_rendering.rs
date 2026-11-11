//! The flat tree in rendering (M10 sub-feature 5b, ADR 0036 section 4): the
//! cases of the Chromium probe `tools/probes/shadow-dom.json`, with the
//! answers that Chromium 148 gave (`tests/data/shadow-dom.json`: `just probe
//! --json tools/probes/shadow-dom.json`, three equal runs).
//!
//! - `boxes` rows: the border box of a light element (or none: a light
//!   child that no slot takes, or below a `display: none` host) after a
//!   script attached a shadow root: hosts that show their shadow children,
//!   default and named slots, fallback content, nested slots, slots in flex
//!   containers, tables and lists through slots, counters through a slot,
//!   inheritance from a slot.
//! - `js` rows: computed styles of slotted and unslotted elements and of
//!   unused fallback content, `offsetHeight`, `innerText` of a host,
//!   counter widths, and `elementFromPoint` and `elementsFromPoint` through
//!   a host (also a host that the retargeting repeats).
//!
//! [`WAITING`] lists the cases that need a later sub-feature.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

use common::{OnePage, answer, differences, new_page, on_page_threads, repo_root};

/// The cases that wait for a later sub-feature: (case, why).
const WAITING: &[(&str, &str)] = &[
    ("declarative-js-off", "declarative shadow roots (backlog)"),
    ("declarative-js-on", "declarative shadow roots (backlog)"),
];

/// The largest difference of a box coordinate from Chromium's, in px.
const TOLERANCE: f64 = 0.5;

const PAGE: &str = common::CASE_PAGE;

/// A page that shows `html`, loaded, in a viewport of `size`.
fn open(html: &str, size: Size) -> Page {
    let mut page = new_page(Arc::new(OnePage::with_data_urls(html)), 2, size);
    page.set_javascript(true);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// The `js` row: swb's answer to the expression against Chromium's.
fn check_js(case: &str, row: usize, page: &mut Page, result: &Value) -> Vec<String> {
    let query = result["label"].as_str().unwrap();
    let want = &result["value"];
    let got = answer(page, query);
    if close(&got, want) {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row}: {}\n{}",
        &query[..query.len().min(160)],
        differences(want, &got)
    )]
}

/// True if the values are equal, numbers within 0.05 (the width of a text
/// can differ in the last bits of the font's advances).
fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            (x.as_f64().unwrap_or(f64::NAN) - y.as_f64().unwrap_or(f64::NAN)).abs() <= 0.05
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| close(a, b))
        }
        _ => a == b,
    }
}

/// The `boxes` row: the border box of the element with the index in
/// document order, or no box (`rect` is null).
fn check_box(case: &str, row: usize, page: &mut Page, result: &Value) -> Vec<String> {
    let index = result["index"].as_u64().unwrap();
    let expression = format!(
        "(() => {{ const e = document.querySelectorAll('*')[{index}]; const r = e.getBoundingClientRect(); return [e.localName, e.getClientRects().length, r.x + scrollX, r.y + scrollY, r.width, r.height]; }})()"
    );
    let got = answer(page, &expression);
    let tag_ok = got.get(0) == Some(&result["tag"]);
    let has_box = got.get(1).and_then(Value::as_u64).unwrap_or(0) > 0;
    let ok = tag_ok
        && match result["rect"].as_array() {
            None => !has_box,
            Some(want) => {
                has_box
                    && got
                        .as_array()
                        .unwrap()
                        .iter()
                        .skip(2)
                        .zip(want)
                        .all(|(a, b)| {
                            (a.as_f64().unwrap_or(f64::NAN) - number(b)).abs() <= TOLERANCE
                        })
            }
        };
    if ok {
        return Vec::new();
    }
    vec![format!(
        "{case} #{row} {}: swb {got}, Chromium {} {}",
        result["label"], result["tag"], result["rect"]
    )]
}

fn viewport_of(results: &Value) -> Size {
    let viewport = results["viewport"].as_array().unwrap();
    Size::new(number(&viewport[0]) as f32, number(&viewport[1]) as f32)
}

/// The probe file and Chromium's answers.
fn load_cases() -> (&'static [Value], &'static [Value]) {
    let probe: &'static Value = Box::leak(Box::new(
        serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/shadow-dom.json")).unwrap(),
        )
        .unwrap(),
    ));
    let expected: &'static Value = Box::leak(Box::new(
        serde_json::from_str(include_str!("data/shadow-dom.json")).unwrap(),
    ));
    let cases = probe["cases"].as_array().unwrap();
    let answers = expected["cases"].as_array().unwrap();
    assert_eq!(cases.len(), answers.len());
    for (case, answer) in cases.iter().zip(answers) {
        assert_eq!(case["name"], answer["name"]);
    }
    (cases, answers)
}

/// Runs a case with JavaScript on.
fn run_case(case: &'static Value, results: &'static Value) -> Vec<String> {
    let name = case["name"].as_str().unwrap();
    let mut page = open(case["html"].as_str().unwrap(), viewport_of(results));
    let mut failures = Vec::new();
    for (row, result) in results["results"].as_array().unwrap().iter().enumerate() {
        failures.extend(match result["kind"].as_str().unwrap() {
            "js" => check_js(name, row, &mut page, result),
            "boxes" => check_box(name, row, &mut page, result),
            other => vec![format!("{name} #{row}: unknown kind {other}")],
        });
    }
    failures
}

/// Every case but the waiting ones.
#[test]
fn shadow_rendering_matches_chromium() {
    let (cases, answers) = load_cases();
    let runs = on_page_threads(
        cases
            .iter()
            .zip(answers)
            .filter(|(case, _)| !WAITING.iter().any(|w| case["name"] == w.0))
            .map(|(case, answer)| move || run_case(case, answer)),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const LINK_PAGE: &str = "<!DOCTYPE html><body style='margin:0;font:16px/20px sans-serif'>\
    <p><a id=l href='#go'>by <span id=h>light</span></a> end</p>\
    <script>var L = []; document.addEventListener('click', function (e) { L.push(e.target.id); });\
    document.getElementById('h').attachShadow({mode: 'open'}).innerHTML = '<b id=in>shadow text</b>';\
    </script>";

/// A point in the middle of the shadow `b` of `LINK_PAGE`.
fn shadow_point(page: &mut Page) -> (f32, f32) {
    let at = answer(
        page,
        "(() => { const r = document.getElementById('h').shadowRoot.firstChild.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()",
    );
    (number(&at[0]) as f32, number(&at[1]) as f32)
}

/// A hit on shadow text: the node is the shadow text (its element, through
/// the flat tree), the link around the host contains it, the document's
/// listener sees the host as the target (the interim rule of 5b), and the
/// click follows the link.
#[test]
fn a_click_on_shadow_text_in_a_link_follows_the_link() {
    common::on_page_thread(|| {
        let mut page = open(LINK_PAGE, Size::new(400.0, 300.0));
        let (x, y) = shadow_point(&mut page);
        let hit = page.hit_test(x, y).unwrap();
        let doc = page.document().unwrap();
        let text_parent = doc.parent(hit.node).unwrap();
        assert_eq!(
            doc.element(text_parent).and_then(|e| e.id()),
            Some("in"),
            "the hit is the shadow text"
        );
        assert_eq!(hit.link_element, doc.element_by_id("l"));
        assert!(hit.link.unwrap().as_str().ends_with("#go"));
        page.click(x, y);
        assert_eq!(answer(&mut page, "L.join()"), "h");
        assert!(page.url().unwrap().as_str().ends_with("#go"));
        assert_eq!(
            answer(
                &mut page,
                "(() => { const r = document.getElementById('l').getClientRects()[0]; return document.elementFromPoint(r.x + 2, r.y + r.height / 2).id; })()"
            ),
            "l",
            "the light text of the link"
        );
    });
}

/// After the time limits stopped the scripts, the input works as with
/// JavaScript off: the engine's own activation walk starts at the host too.
#[test]
fn the_link_around_a_host_activates_without_scripts() {
    common::on_page_thread(|| {
        let mut page = open(LINK_PAGE, Size::new(400.0, 300.0));
        let (x, y) = shadow_point(&mut page);
        let runtime = page.script_runtime_mut().unwrap();
        runtime.set_time_limit(Duration::from_millis(30));
        for _ in 0..3 {
            page.evaluate("while (true);", swb_engine::JsonForm::Strict);
        }
        assert!(page.script_runtime_mut().unwrap().stopped());
        assert!(page.click(x, y));
        assert!(page.url().unwrap().as_str().ends_with("#go"));
    });
}

/// The scroll containers of a shadow element are its flat ancestors: its
/// box moves with the light scroll container around the host.
#[test]
fn shadow_boxes_move_with_their_scroll_container() {
    common::on_page_thread(|| {
        let html = "<!DOCTYPE html><body style='margin:0'><div id=sc style='overflow:auto;height:50px'>\
                    <div id=h></div><div style='height:500px'></div></div><script>\
                    document.getElementById('h').attachShadow({mode: 'open'}).innerHTML =\
                    \"<p style='margin:0;height:20px'>x</p>\";</script>";
        let mut page = open(html, Size::new(400.0, 300.0));
        let top = "document.getElementById('h').shadowRoot.firstChild.getBoundingClientRect().top";
        assert_eq!(answer(&mut page, top), 0);
        answer(&mut page, "document.getElementById('sc').scrollTop = 15");
        assert_eq!(answer(&mut page, top), -15);
        // `scrollIntoView` scrolls the light container (the scroll chain).
        answer(
            &mut page,
            "document.getElementById('h').shadowRoot.innerHTML = \"<div style='height:300px'></div><p id=in style='margin:0;height:20px'>y</p>\"",
        );
        answer(
            &mut page,
            "document.getElementById('h').shadowRoot.getElementById('in').scrollIntoView()",
        );
        assert_eq!(
            answer(&mut page, "document.getElementById('sc').scrollTop"),
            300
        );
    });
}

/// The element of light text that a slot takes is the slot (its flat
/// parent): the cursor of the slot applies.
#[test]
fn slotted_text_takes_the_cursor_of_the_slot() {
    common::on_page_thread(|| {
        let html = "<!DOCTYPE html><body style='margin:0'><div id=h>light text</div><script>\
                    document.getElementById('h').attachShadow({mode: 'open'}).innerHTML =\
                    \"<slot style='cursor:pointer'></slot>\";</script>";
        let mut page = open(html, Size::new(400.0, 300.0));
        page.mouse_move(5.0, 8.0);
        assert_eq!(page.cursor(), swb_engine::Cursor::Pointer);
    });
}

/// `HTMLSlotElement.assign()` makes no journal entry; the flat tree changes,
/// and the next layout shows the assigned node.
#[test]
fn assign_lays_out_again() {
    common::on_page_thread(|| {
        let html = "<!DOCTYPE html><body><div id=h><b id=a>A</b></div><script>\
                    var s = document.createElement('slot');\
                    document.getElementById('h').attachShadow({mode: 'open', slotAssignment: 'manual'}).appendChild(s);\
                    </script>";
        let mut page = open(html, Size::new(400.0, 300.0));
        let count = "document.getElementById('a').getClientRects().length";
        assert_eq!(answer(&mut page, count), 0);
        answer(&mut page, "s.assign(document.getElementById('a'))");
        assert_eq!(answer(&mut page, count), 1);
        answer(&mut page, "s.assign()");
        assert_eq!(answer(&mut page, count), 0);
    });
}

/// A change of a shadow tree's sheet restyles (ADR 0036 section 5,
/// "Invalidation"): `insertRule` and `deleteRule` through
/// `ShadowRoot.styleSheets`, a new text, a `<style>` inserted into and
/// removed from the root; the rules never leak into the document.
#[test]
fn shadow_sheet_changes_restyle() {
    common::on_page_thread(|| {
        let html = "<!DOCTYPE html><body><div id=h></div><span id=l>l</span><script>\
                    var r = document.getElementById('h').attachShadow({mode: 'open'});\
                    r.innerHTML = '<style>span{color:rgb(0,128,0)}</style><span id=s>s</span>';\
                    var s = r.getElementById('s');\
                    function c(e) { return getComputedStyle(e).color + ' ' + getComputedStyle(e).fontSize; }\
                    </script>";
        let mut page = open(html, Size::new(400.0, 300.0));
        assert_eq!(answer(&mut page, "c(s)"), "rgb(0, 128, 0) 16px");
        assert_eq!(answer(&mut page, "c(l)"), "rgb(0, 0, 0) 16px");
        answer(
            &mut page,
            "r.styleSheets[0].insertRule('span{font-size:30px}', 1)",
        );
        assert_eq!(answer(&mut page, "c(s)"), "rgb(0, 128, 0) 30px");
        assert_eq!(answer(&mut page, "c(l)"), "rgb(0, 0, 0) 16px");
        answer(&mut page, "r.styleSheets[0].deleteRule(0)");
        assert_eq!(answer(&mut page, "c(s)"), "rgb(0, 0, 0) 30px");
        answer(
            &mut page,
            "r.firstChild.textContent = 'span{color:rgb(0,0,255)}'",
        );
        assert_eq!(answer(&mut page, "c(s)"), "rgb(0, 0, 255) 16px");
        answer(
            &mut page,
            "var t = document.createElement('style'); t.textContent = 'span{font-size:20px}'; \
             r.appendChild(t)",
        );
        assert_eq!(answer(&mut page, "c(s)"), "rgb(0, 0, 255) 20px");
        assert_eq!(answer(&mut page, "c(l)"), "rgb(0, 0, 0) 16px");
        answer(&mut page, "t.remove()");
        assert_eq!(answer(&mut page, "c(s)"), "rgb(0, 0, 255) 16px");
        // The same through layout: the box of the shadow text.
        answer(&mut page, "r.appendChild(t)");
        let height = "s.getBoundingClientRect().height > 20";
        assert_eq!(answer(&mut page, height), true);
    });
}

/// The waiting cases name cases of the probe file.
#[test]
fn waiting_cases_exist() {
    let (cases, _) = load_cases();
    for (name, _) in WAITING {
        assert!(cases.iter().any(|c| c["name"] == *name), "{name}");
    }
}
