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
//! script and `window.__s()` after the load and after each group; the 17
//! records are in `data/craigslist-posting-chromium.json` (three runs of
//! the tool gave equal files). M9 feature 14b added the groups "favorite",
//! "unfavorite" and "favorite again" at the start, and "reload" at the end
//! (the reload drops `window.__s`, so the group evaluates the setup script
//! again), and the record keys `fav`, `favStore` and `favFrames`: the
//! favorites work through the site's hidden storage frame. `swbtools
//! --no-shared-worker --no-intl replay craigslist-posting --steps
//! crates/automation/tests/data/craigslist-posting-steps.json --setup
//! crates/automation/tests/data/craigslist-posting.js --record
//! 'window.__s()' --out FILE` makes them again from the same steps file
//! (docs/testing.md, "Automation tests"); the one difference from the
//! committed file is the click point of the share menu's selector step in
//! the `ev` key, which the test skips anyway. The "reply" group waits
//! twice: the failed POST of the reply request takes real time, so the
//! second wait (virtual) runs the event loop again after it. The record
//! keys are compared
//! as written, with one exception (swb differs, backlog in
//! `docs/roadmap.md`): `ev` (the mouse events) is compared only after the
//! double click and the context menu, because the click event after a drag
//! is not fired in swb, and the click point of a selector step is
//! fractional in swb.

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

/// The step groups after the load, with the name of the record that each
/// one ends with (the order of `data/craigslist-posting-chromium.json`
/// after the first record). A group is the steps, then the wait in page
/// time that the zoom animation (250 ms timer) and the failed requests need.
/// The file is the one that `swbtools replay` runs in Chromium.
fn groups() -> Vec<(String, Vec<Value>)> {
    step_groups(include_str!("data/craigslist-posting-steps.json"))
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
        let mut page = load_at_capture(&dir, Duration::from_millis(1500));
        assert!(page.has_script_runtime());
        // Chromium has no script error at load (three warnings of Leaflet
        // and jQuery Migrate only).
        assert_eq!(console_errors(&page), Vec::<String>::new());
        assert_eq!(value(&mut page, "document.readyState"), "complete");
        let setup = include_str!("data/craigslist-posting.js");
        assert_eq!(value(&mut page, setup), 1);

        let start = record(&mut page);
        let at_start = favorites_state(&start);
        assert!(
            at_start.contains(&"favorites off|none|0 favorites".to_owned()),
            "{at_start:?}"
        );
        assert!(
            at_start.contains(&"fave|block|favorite".to_owned()),
            "{at_start:?}"
        );
        let mut problems = differences("load", &start, &expected[0], &[]);
        let groups = groups();
        assert_eq!(groups.len() + 1, expected.len());
        for (index, (name, steps)) in groups.iter().enumerate() {
            if name == "reload" {
                // The reload starts a new page with its own console.
                check_script_errors(&page);
            }
            run_steps(&mut page, steps).unwrap();
            // The mouse events: see the module comment.
            let skip: &[&str] = if matches!(name.as_str(), "double click" | "context menu") {
                &[]
            } else {
                &["ev"]
            };
            let got = record(&mut page);
            check_favorites(name, &favorites_state(&got));
            problems.extend(differences(name, &got, &expected[index + 1], skip));
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        // After the reload the page is clean: the record's `e` is compared above.
        assert_eq!(console_errors(&page), Vec::<String>::new());
    });
}

/// The script errors after the steps are those of Chromium: the page's own
/// `console.error` for the missing `bfp3-concat.min.js`, and the unhandled
/// rejections that the record's `e` lists (the share and favorite requests that the
/// fixture lacks, `writeText` without the clipboard permission). Chromium's other
/// errors are failed requests.
fn check_script_errors(page: &Page) {
    let errors = console_errors(page);
    let own = errors
        .iter()
        .filter(|e| e.contains("svs-boot-injectJs-error"))
        .count();
    assert_eq!(own, 1, "{errors:?}");
    assert!(
        errors
            .iter()
            .filter(|e| !e.contains("svs-boot-injectJs-error"))
            .all(|e| e.starts_with("Uncaught (in promise)")),
        "{errors:?}"
    );
}

/// The states of the favorites in a record: the header item, the two buttons and the
/// stored favorites.
fn favorites_state(record: &Value) -> Vec<String> {
    let list = |key: &str| -> Vec<String> {
        record[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };
    let mut out = list("fav");
    out.extend(
        list("favStore")
            .into_iter()
            .filter(|k| k.starts_with("faves")),
    );
    out
}

/// The favorites work through the site's hidden storage frame: a same-origin `iframe`
/// whose script reads `document.referrer` for the page's origin, reads `localStorage`
/// and sends it to the page with `postMessage`, and stores what the page sends back.
/// Chromium's records for the groups "favorite", "unfavorite", "favorite again" and
/// "reload" are compared in the test; this names the states, so that a record that was
/// wrong in Chromium too would show.
fn check_favorites(group: &str, state: &[String]) {
    let has = |text: &str| state.iter().any(|s| s == text);
    let faved = |on: bool| {
        assert_eq!(
            has("favorites|inline-block|1 favorites"),
            on,
            "{group}: {state:?}"
        );
        assert_eq!(has("fave|none|favorite"), on, "{group}: {state:?}");
        assert_eq!(has("unfave|block|favorite"), on, "{group}: {state:?}");
        assert_eq!(has("fave|block|favorite"), !on, "{group}: {state:?}");
        assert_eq!(
            has("favorites off|none|0 favorites"),
            !on,
            "{group}: {state:?}"
        );
    };
    match group {
        "favorite" | "favorite again" | "reload" => {
            faved(true);
            assert!(
                state.iter().any(|k| k.starts_with("faves_1={\"")),
                "{group}: {state:?}"
            );
        }
        "unfavorite" => {
            faved(false);
            assert!(has("faves_1={}"), "{group}: {state:?}");
        }
        _ => {}
    }
}
