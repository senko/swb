//! Nested interactive elements and the activation of a click (M9, the BBC
//! side menu; ADR 0028 "Nested activation"): the cases of the Chromium probe
//! `tools/probes/web-nested-activation.json` run here with the same input
//! steps (`swb_automation::run_steps`), and the value of each query is the
//! one that Chromium 148 gave (`tests/data/web-nested-activation.json`:
//! `uv run --project tools swbtools probe --json
//! tools/probes/web-nested-activation.json`, three equal runs; the file maps a
//! case to the values of its queries).
//!
//! The cases put one interactive element in another (a `button` in a link, a
//! link in a link, a form in a link, a label, a `summary`, a link in a
//! `button`, ...) and click the inner one for real, with `click()`, with
//! `dispatchEvent` and with the keyboard. Each query lists the `click`
//! listeners that ran (`c@ID`, `!` if the event was already canceled), the
//! `submit`, `reset`, `change` and `toggle` events, the history length, the
//! query and fragment of the URL after the navigation, and the state of the
//! checkboxes and `details` elements.
//!
//! Chromium's pages have one history entry more than swb's (the page of the
//! probe starts from `about:blank`), so the test lowers the value of `h` by 1.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{on_page_thread, repo_root, value};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{FontContext, Page, PageConfig, Size, Url};
use swb_net::NetworkFetcher;

/// A directory with test pages, removed at the end of the test.
struct Site(PathBuf);

impl Site {
    fn new(tag: &str) -> Site {
        let dir = std::env::temp_dir().join(format!(
            "swb-nested-activation-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Site(dir)
    }

    fn page(&self, name: &str, html: &str) -> Url {
        let path = self.0.join(name);
        std::fs::write(&path, html).unwrap();
        Url::from_file_path(path).unwrap()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A page with JavaScript on at `url` (800x600, test fonts), loaded.
fn open(url: Url) -> Page {
    let config = PageConfig {
        fetcher: Arc::new(NetworkFetcher::new()),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(800.0, 600.0),
        1.0,
    );
    page.set_javascript(true);
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// The probe file and Chromium's values.
fn load_cases() -> (Value, Value) {
    let probe: Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("tools/probes/web-nested-activation.json"))
            .unwrap(),
    )
    .unwrap();
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-nested-activation.json"),
        )
        .unwrap(),
    )
    .unwrap();
    (probe, expected)
}

/// Chromium's value with the history length of a page that starts from
/// `about:blank` lowered to swb's.
fn lowered(expected: &str) -> String {
    expected
        .split(' ')
        .map(|part| match part.strip_prefix("h=") {
            Some(n) => format!("h={}", n.parse::<u32>().unwrap() - 1),
            None => part.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The value without the `toggle` events of `details` (swb fires none: the
/// element's events are not implemented).
fn without_toggle(value: &str) -> String {
    let (log, rest) = value.split_once(" h=").unwrap_or((value, ""));
    let log: Vec<&str> = log
        .split('|')
        .filter(|e| !e.starts_with("toggle@"))
        .collect();
    format!("{} h={rest}", log.join("|"))
}

/// The cases where swb differs from Chromium on purpose or until a later
/// feature, with the value that swb gives (the test checks it too, so a fix
/// shows) and the reason.
const DEPARTURES: &[(&str, &str, &str)] = &[
    (
        "a-input-image-in-form-dispatch",
        "c@i|c@f|c@o|submit@f/i h=2 loc=?q=1&im.x=0&im.y=0#F",
        "the point of a `click` that `dispatchEvent` makes is (0, 0) in the viewport, so Chromium \
         gives negative coordinates relative to the image button; swb clamps them to 0",
    ),
    (
        "a-details-default-summary-real",
        "c@i|c@o h=2 loc=#L i.open=true",
        "a `details` element without a `summary`: the click in its default summary toggles it \
         in Chromium and follows no link; swb toggles it and also follows the link around \
         (the dispatch of the script path does not know the point of the click)",
    ),
    (
        "area-real",
        "c@i h=1 loc=",
        "the `click` event of a click on an image map goes to the `img` in swb, to the `area` in \
         Chromium, and the link of the `area` is not followed",
    ),
    (
        "a-button-revalidate-form-removed",
        "c@t|c@i|c@o h=1 loc=",
        REVALIDATE,
    ),
    (
        "a-button-revalidate-form-attr-removed",
        "c@t|c@i|c@o h=1 loc=",
        REVALIDATE,
    ),
    (
        "a-button-revalidate-type-button",
        "c@t|c@i|c@o|c@f h=1 loc=",
        REVALIDATE,
    ),
    (
        "a-button-revalidate-disabled",
        "c@t|c@i|c@o|c@f h=1 loc=",
        REVALIDATE,
    ),
    (
        "a-button-revalidate-type-button-click",
        "c@i|c@o|c@f h=1 loc=",
        REVALIDATE,
    ),
];

/// The reason of the rows `a-button-revalidate-*`: a `click` listener changes
/// the button so that it does not act (no form owner, `type=button`, disabled).
/// Chromium checks the button again after the listeners and follows the link
/// around it; swb fixes the activation target before the listeners, so it
/// submits nothing and follows nothing. The same change in a `mousedown`
/// listener matches (`a-button-revalidate-type-button-mousedown`).
const REVALIDATE: &str = "a `click` listener makes the button in the link not act; Chromium then follows \
     the link, swb fixed the target before the listeners and does nothing (ADR 0028 \
     \"Nested activation\")";

/// Runs the cases of the probe file against Chromium's values.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let (probe, expected) = load_cases();
        let cases = probe["cases"].as_array().unwrap();
        assert_eq!(cases.len(), expected.as_object().unwrap().len());
        let site = Site::new("scripts");
        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let mut page = open(site.page("page.html", case["html"].as_str().unwrap()));
            // The probe waits 50 ms at the end for Chromium's navigations; the
            // test waits for the load to end instead.
            let steps: Vec<Value> = case["steps"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|step| step.get("wait").is_none())
                .cloned()
                .collect();
            if let Err(error) = run_steps(&mut page, &steps) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            page.wait_until_loaded(Duration::from_secs(5));
            page.run_event_loop(Duration::from_millis(1), Duration::from_secs(5));
            let want = without_toggle(&lowered(expected[name][0].as_str().unwrap()));
            let got = without_toggle(value(&mut page, "R()").as_str().unwrap());
            let problem = match DEPARTURES.iter().find(|(case, _, _)| case == &name) {
                Some((_, swb, _)) => (got != *swb)
                    .then(|| format!("{name} (departure)\n  swb was: {swb}\n  actual:  {got}")),
                None => {
                    (got != want).then(|| format!("{name}\n  expected: {want}\n  actual:   {got}"))
                }
            };
            failures.extend(problem);
        }
        assert!(
            failures.is_empty(),
            "{} failures\n{}",
            failures.len(),
            failures.join("\n")
        );
    });
}

/// The page of `url` with JavaScript off (800x600, test fonts), loaded.
fn open_without_scripts(url: Url) -> Page {
    let config = PageConfig {
        fetcher: Arc::new(NetworkFetcher::new()),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(800.0, 600.0),
        1.0,
    );
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// The query and fragment of the page's URL, as `location.search +
/// location.hash`.
fn location_tail(page: &Page) -> String {
    let url = page.url().unwrap();
    format!(
        "{}{}",
        url.query().map(|q| format!("?{q}")).unwrap_or_default(),
        url.fragment().map(|f| format!("#{f}")).unwrap_or_default()
    )
}

/// Runs one step: `focus` through the engine (it is a script in the steps of
/// the probe), the others with `run_steps`.
fn run_step(page: &mut Page, step: &Value) -> Result<(), String> {
    if let Some(selector) = step.get("focus").and_then(Value::as_str) {
        let node = page
            .query_selector_all(selector)
            .and_then(|nodes| nodes.first().copied())
            .ok_or("no element to focus")?;
        page.focus(Some(node), true);
        return Ok(());
    }
    run_steps(page, std::slice::from_ref(step))
}

/// The state of `key` (`ID` for a checkbox, `ID.open` for a `details`
/// element) in the page now.
fn state_of(page: &Page, key: &str) -> Option<bool> {
    let (id, open) = match key.strip_suffix(".open") {
        Some(id) => (id, true),
        None => (key, false),
    };
    let node = page
        .query_selector_all(&format!("#{id}"))
        .and_then(|nodes| nodes.first().copied())?;
    if open {
        page.document()?.element(node).map(|e| e.has_attr("open"))
    } else {
        page.control_checked(node)
    }
}

/// The cases of the probe file that need no script (`markup_only`: the pages
/// are markup, the steps are clicks and keys) with JavaScript off: the engine
/// path, which does not use the dispatch of the script runtime, follows
/// the same rule. The values are Chromium's from the run with scripts (the
/// activation does not depend on them): the URL after the click, and the
/// state of the checkboxes and `details` elements if the page did not load a
/// new document.
#[test]
fn probe_cases_without_scripts() {
    on_page_thread(|| {
        let (probe, expected) = load_cases();
        let site = Site::new("markup");
        let mut failures = Vec::new();
        let mut run = 0;
        for case in probe["cases"].as_array().unwrap() {
            if case.get("markup_only") != Some(&Value::Bool(true)) {
                continue;
            }
            run += 1;
            let name = case["name"].as_str().unwrap();
            // swb has no hit testing of image maps: a click on an `area` does
            // not reach the area (see DEPARTURES for the click events).
            if name == "area-real" {
                continue;
            }
            let html = case["html"].as_str().unwrap();
            let mut page = open_without_scripts(site.page("page.html", html));
            let steps = case["steps"].as_array().unwrap();
            if let Some(error) = steps.iter().find_map(|s| run_step(&mut page, s).err()) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            page.wait_until_loaded(Duration::from_secs(5));
            let want = expected[name][0].as_str().unwrap();
            let (_, tail) = want.split_once(" loc=").unwrap();
            let mut parts = tail.split(' ');
            let want_loc = parts.next().unwrap();
            let mut problems = Vec::new();
            if location_tail(&page) != want_loc {
                problems.push(format!(
                    "loc {} instead of {want_loc}",
                    location_tail(&page)
                ));
            }
            // A new document (a submitted form) has the states of the markup.
            if !want_loc.starts_with('?') {
                for state in parts {
                    let (key, on) = state.split_once('=').unwrap();
                    let got = state_of(&page, key);
                    if got != Some(on == "true") {
                        problems.push(format!("{key} is {got:?} instead of {on}"));
                    }
                }
            }
            if !problems.is_empty() {
                failures.push(format!(
                    "{name}: {}\n  Chromium: {want}",
                    problems.join(", ")
                ));
            }
        }
        assert!(run > 60, "{run} cases without scripts");
        assert!(
            failures.is_empty(),
            "{} failures\n{}",
            failures.len(),
            failures.join("\n")
        );
    });
}
