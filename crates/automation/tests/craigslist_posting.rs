//! Exit of M8 feature 11 for the Craigslist posting page: the local fixture
//! `fixtures/js/craigslist-posting` (git-ignored; set `SWB_JS_FIXTURES` to
//! the directory `fixtures/js` of another checkout) with JavaScript on, in
//! one load with all steps in order. The page runs on its legacy path
//! (the tools delete `SharedWorker` and `Intl` in Chromium, ADR 0028) with
//! the page clock fixed at the `captured` time of the fixture, as the
//! tools fix it (the site's manifest name and the relative dates depend on
//! it). The test skips when the fixture is missing.
//!
//! After the load and after each step group the test reads one JSON record
//! of the page (`data/craigslist-posting.js`: the relative date text,
//! jQuery and Leaflet versions, `screen.orientation`, `matchMedia`, the
//! slides, the map panes, tile containers and tile counts, the circle's
//! SVG, the mouse events of the map, the reply panel, the share menu, the
//! print events, the errors and unhandled rejections) and compares it with
//! Chromium's record of the same steps.
//!
//! Where the Chromium values come from: Chromium 148.0.7778.96 (headless
//! shell, Playwright) replayed the same fixture with the same page clock,
//! `SharedWorker` and `Intl` deleted, ran the steps of [`groups`] (with
//! the wheel at 120; the mouse moves as single moves, a 300 ms pause
//! after each step, then the group's waits) and evaluated the setup
//! script and `window.__s()` after the load and after each group; the 13
//! records are in `data/craigslist-posting-chromium.json`. The helper
//! that made them is not in the repository (backlog: a `swbtools`
//! command for Chromium replays with steps). The record keys are compared
//! as written, with these exceptions (swb differs, backlog in
//! `docs/roadmap.md`):
//! - `ev` (the mouse events) only after the double click and the context
//!   menu: the click event after a drag is not fired in swb, and the click
//!   point of a selector step is fractional in swb;
//! - the wheel steps send 240 where Chromium got 120: Leaflet divides the
//!   wheel delta by 2 unless `navigator.userAgent` contains "chrome" and
//!   swb's user agent does not (measured: with 240 swb zooms by the same
//!   two levels).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::{fixture, fixture_meta, on_page_thread, replay_page, value};
use serde_json::{Value, json};
use swb_automation::run_steps;
use swb_engine::{ClockMode, ConsoleLevel, Page, PageClock};

/// The wheel delta of the wheel steps. Chromium's records come from 120;
/// see the module comment.
const WHEEL: i64 = 240;

/// The posting, loaded as `swb --headless --js --test-fonts --now CAPTURED
/// --virtual-time --settle-ms 1500` loads it (1280x800).
fn load(dir: &Path) -> Page {
    let (mut page, url) = replay_page(dir, true);
    let captured: jiff::Timestamp = fixture_meta(dir)["captured"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    page.set_clock(PageClock::new(
        ClockMode::Virtual,
        Some(captured.as_millisecond() as f64),
    ));
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(60)));
    page.run_event_loop(Duration::from_millis(1500), Duration::from_secs(60));
    page
}

/// The record of the page (`window.__s()`).
fn record(page: &mut Page) -> Value {
    let text = value(page, "window.__s()");
    serde_json::from_str(text.as_str().unwrap()).unwrap()
}

/// The texts of the console messages at `error` level.
fn console_errors(page: &Page) -> Vec<String> {
    page.console_messages()
        .into_iter()
        .filter(|m| m.level == ConsoleLevel::Error)
        .map(|m| m.text)
        .collect()
}

/// The step groups after the load, with the name of the record that each
/// one ends with (the order of `data/craigslist-posting-chromium.json`
/// after the first record). A group is the steps, then the wait in page
/// time that the zoom animation (250 ms timer) and the failed requests need.
fn groups() -> Vec<(&'static str, Value)> {
    let map = [971, 297];
    vec![
        (
            "thumbnail 2",
            json!([{"click": "a.thumb[title=\"2\"]"}, {"wait": 800}]),
        ),
        (
            "zoom in",
            json!([{"click": ".leaflet-control-zoom-in"}, {"wait": 800}]),
        ),
        (
            "zoom out",
            json!([{"click": ".leaflet-control-zoom-out"}, {"wait": 800}]),
        ),
        (
            "drag",
            json!([
                {"hover": map}, {"mousedown": "left"}, {"hover": [900, 250]},
                {"hover": [850, 230]}, {"mouseup": "left"}, {"wait": 800}
            ]),
        ),
        ("double click", json!([{"dblclick": map}, {"wait": 800}])),
        ("context menu", json!([{"rightclick": map}, {"wait": 800}])),
        (
            "wheel in",
            json!([{"hover": map}, {"wheel": [0, -WHEEL]}, {"wait": 800}]),
        ),
        ("wheel out", json!([{"wheel": [0, WHEEL]}, {"wait": 800}])),
        // The failed POST of the reply request is real time, so the second
        // wait (virtual) only runs the event loop again after it.
        (
            "reply",
            json!([{"click": ".reply-button"}, {"wait": 1500}, {"wait": 1500}]),
        ),
        ("print", json!([{"click": "#printme"}, {"wait": 2000}])),
        (
            "share",
            json!([{"click": "#sharebutton-container a"}, {"wait": 500}]),
        ),
        (
            "copy URL",
            json!([{"click": ".share-menu li:nth-child(7) a"}, {"wait": 800}]),
        ),
    ]
}

/// The keys of `got` and `expected` that differ, as `key: got != expected`.
fn differences(name: &str, got: &Value, expected: &Value, skip: &[&str]) -> Vec<String> {
    let (Some(got), Some(expected)) = (got.as_object(), expected.as_object()) else {
        panic!("{name}: a record is an object");
    };
    let mut keys: Vec<&String> = got.keys().chain(expected.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| !skip.contains(&k.as_str()) && got.get(*k) != expected.get(*k))
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
fn craigslist_posting_works_with_javascript_on() {
    let Some(dir) = fixture("craigslist-posting") else {
        eprintln!("skipped: fixtures/js/craigslist-posting is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/craigslist-posting-chromium.json")).unwrap();
        let mut page = load(&dir);
        assert!(page.has_script_runtime());
        // Chromium has no script error at load (three warnings of Leaflet
        // and jQuery Migrate only).
        assert_eq!(console_errors(&page), Vec::<String>::new());
        assert_eq!(value(&mut page, "document.readyState"), "complete");
        let setup = include_str!("data/craigslist-posting.js");
        assert_eq!(value(&mut page, setup), 1);

        let mut problems = differences("load", &record(&mut page), &expected[0], &[]);
        let groups = groups();
        assert_eq!(groups.len() + 1, expected.len());
        for (index, (name, steps)) in groups.iter().enumerate() {
            run_steps(&mut page, steps.as_array().unwrap()).unwrap();
            // The mouse events: see the module comment.
            let skip: &[&str] = if matches!(*name, "double click" | "context menu") {
                &[]
            } else {
                &["ev"]
            };
            let got = record(&mut page);
            problems.extend(differences(name, &got, &expected[index + 1], skip));
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));

        // The script errors after the steps are those of Chromium: the
        // page's own `console.error` for the missing `bfp3-concat.min.js`,
        // and the three unhandled rejections that the record's `e` lists
        // (the share requests that the fixture lacks, `writeText` without
        // the clipboard permission). Chromium's other errors are failed
        // requests.
        let errors = console_errors(&page);
        assert_eq!(errors.len(), 4, "{errors:?}");
        assert!(errors[0].contains("svs-boot-injectJs-error"), "{errors:?}");
        assert!(
            errors[1..]
                .iter()
                .all(|e| e.starts_with("Uncaught (in promise)"))
        );
    });
}
