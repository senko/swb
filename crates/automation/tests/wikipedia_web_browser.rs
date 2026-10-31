//! M8 feature 12 for the Wikipedia article "Web browser" with JavaScript on:
//! the local fixture `fixtures/js/wikipedia-web-browser` (git-ignored; set
//! `SWB_JS_FIXTURES` to the directory `fixtures/js` of another checkout), in
//! one load with all steps in order, with the page clock fixed at the
//! `captured` time of the fixture, as the tools fix it. The test skips when
//! the fixture is missing.
//!
//! The steps are in `data/wikipedia-web-browser-steps.json`, 48 groups for
//! the features of the target (roadmap, M8 target "Wikipedia"): the
//! Appearance radios (color theme, text size, page width) with the root
//! classes and the client preference cookie; pin and unpin of the table
//! of contents, Appearance, Tools and Main menu, also from their
//! dropdowns; the collapse of the table of contents by click, `Enter` and
//! `Space`; a collapsible table and a navbox by click, `Enter` and `Space`;
//! the scroll to 2,000 px and a wheel turn, with the sticky containers;
//! the hover on a wiki link, which shows the error popup (the fixture has
//! no REST summary for it, in swb as in Chromium); the search box focus,
//! typing and `Escape`; a click on table-of-contents links (the anchor
//! scroll with `scroll-margin-top`), then back and forward with `hashchange`
//! and `popstate`.
//!
//! After the load and after each group the test reads one JSON record of
//! the page (`data/wikipedia-web-browser.js`: root and body classes, the
//! cookie, the pinned and unpinned containers with their rectangles, the
//! dropdown checkboxes, the table of contents state, the collapsible
//! elements, the sticky header and containers, the popups, the hash, the
//! scroll position, the `hashchange` and `popstate` log, the focus, the
//! search box classes, the theme colors and the script errors) and compares
//! it with Chromium's record of the same steps. Rectangles are compared
//! with a tolerance of 3 px (swb lays some boxes out 1 to 3 px off, see
//! [`TOLERANCE`]); everything else, also the scroll position, exactly.
//!
//! Since feature 12b the records have the `VisualEditor` root classes
//! (`ve-available`), its accesskey link in the body, the total length of
//! the CSS in the `style` elements of `head` (not their number: it depends
//! on the frames between the module loads), the absence of the SVG
//! fallback of Popups and the computed `clip-path` of the popup container.
//! Left out of the records, because feature 12c changes them: the pixels,
//! and what `filter` and gradients draw (the dark theme). The rectangles
//! below the references are recorded only while the references fit one
//! column (swb has no multi-column layout: with the table of contents or
//! the Appearance panel unpinned, the content is wide enough for two
//! columns and everything below the references is 271 px higher in
//! Chromium). Not tested because the logged-out fixture does not have
//! them, as in Chromium: the
//! sticky header (shown to logged-in users only), reference previews, a
//! successful link preview and the search suggestions (their code is not in
//! the fixture).
//!
//! Where the Chromium values come from: Chromium 148.0.7778.96 (headless
//! shell, Playwright) replayed the same fixture with the same page clock,
//! ran the steps and evaluated the setup script and `window.__s()` after
//! the load and after each group (`swbtools replay wikipedia-web-browser
//! --steps crates/automation/tests/data/wikipedia-web-browser-steps.json
//! --setup crates/automation/tests/data/wikipedia-web-browser.js --record
//! 'window.__s()' --out
//! crates/automation/tests/data/wikipedia-web-browser-chromium.json`, a
//! 300 ms pause after each step); the 49 records are in
//! `data/wikipedia-web-browser-chromium.json`.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{console_errors, fixture, load_at_capture, on_page_thread, step_groups, value};
use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::Page;

/// The record of the page (`window.__s()`).
fn record(page: &mut Page) -> Value {
    let text = value(page, "window.__s()");
    serde_json::from_str(text.as_str().unwrap()).unwrap()
}

/// The keys of the record whose values hold rectangles (compared with
/// [`TOLERANCE`]).
const RECTANGLES: &[&str] = &[
    "pin", "pinbtn", "ddc", "coll", "sticky", "pv", "spc", "content",
];

/// The largest difference in px between two rectangle values. swb lays
/// some boxes out 1 px off (fractional layout units); with the small text
/// size, the collapsible tables 5,700 px down the page are 3 px off
/// (backlog in `docs/roadmap.md`).
const TOLERANCE: i64 = 3;

/// The whole numbers in `text`, and `text` with each number replaced by
/// `#`. A `-` is a sign only after a character that is not a letter or
/// digit.
fn numbers(text: &str) -> (String, Vec<i64>) {
    let mut skeleton = String::new();
    let mut found = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let sign = chars[i] == '-'
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
            && (i == 0 || !chars[i - 1].is_alphanumeric());
        if sign || chars[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while chars.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
            let digits: String = chars[start..i].iter().collect();
            found.push(digits.parse().unwrap_or(0));
            skeleton.push('#');
        } else {
            skeleton.push(chars[i]);
            i += 1;
        }
    }
    (skeleton, found)
}

/// True if the values are equal, with the numbers of strings within
/// [`TOLERANCE`] when `rectangles` is set.
fn same(got: &Value, expected: &Value, rectangles: bool) -> bool {
    match (got, expected) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y, rectangles))
        }
        (Value::String(a), Value::String(b)) if rectangles => {
            let ((skeleton_a, na), (skeleton_b, nb)) = (numbers(a), numbers(b));
            skeleton_a == skeleton_b
                && na.len() == nb.len()
                && na.iter().zip(&nb).all(|(x, y)| (x - y).abs() <= TOLERANCE)
        }
        _ => got == expected,
    }
}

/// The keys of `got` and `expected` that differ, as `key: got != expected`.
fn differences(name: &str, got: &Value, expected: &Value) -> Vec<String> {
    let (Some(got), Some(expected)) = (got.as_object(), expected.as_object()) else {
        panic!("{name}: a record is an object");
    };
    let mut keys: Vec<&String> = got.keys().chain(expected.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| match (got.get(*k), expected.get(*k)) {
            (Some(a), Some(b)) => !same(a, b, RECTANGLES.contains(&k.as_str())),
            _ => true,
        })
        .map(|k| {
            format!(
                "{name}: {k}: swb {:?} != Chromium {:?}",
                got.get(k),
                expected.get(k)
            )
        })
        .collect()
}

#[test]
fn numbers_in_rectangle_texts() {
    assert_eq!(
        numbers("vector-toc in vector-toc-pinned-container 48,-136,176,360 block"),
        (
            "vector-toc in vector-toc-pinned-container #,#,#,# block".to_owned(),
            vec![48, -136, 176, 360]
        )
    );
    assert!(same(&"a 1,2".into(), &"a 4,0".into(), true));
    assert!(!same(&"a 1,2".into(), &"a 5,2".into(), true));
    assert!(!same(&"a 1,2".into(), &"b 1,2".into(), true));
    assert!(!same(&"a 1,2".into(), &"a 1,3".into(), false));
}

#[test]
fn wikipedia_web_browser_works_with_javascript_on() {
    let Some(dir) = fixture("wikipedia-web-browser") else {
        eprintln!("skipped: fixtures/js/wikipedia-web-browser is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/wikipedia-web-browser-chromium.json")).unwrap();
        let mut page = load_at_capture(&dir, Duration::from_millis(1500));
        assert!(page.has_script_runtime());
        // Chromium has no script error at load.
        assert_eq!(console_errors(&page), Vec::<String>::new());
        assert_eq!(value(&mut page, "document.readyState"), "complete");
        let setup = include_str!("data/wikipedia-web-browser.js");
        assert_eq!(value(&mut page, setup), 1);

        let mut problems = differences("load", &record(&mut page), &expected[0]);
        let groups = step_groups(include_str!("data/wikipedia-web-browser-steps.json"));
        assert_eq!(groups.len() + 1, expected.len());
        for (index, (name, steps)) in groups.iter().enumerate() {
            run_steps(&mut page, steps).unwrap();
            let got = record(&mut page);
            problems.extend(differences(name, &got, &expected[index + 1]));
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        // No script error after the steps either (the records' `e` list
        // the uncaught ones; this is the console).
        assert_eq!(console_errors(&page), Vec::<String>::new());
    });
}
