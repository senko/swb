//! Forms from scripts (M8 feature 9b): the cases of the Chromium probes
//! `tools/probes/web-forms-script.json` (the dirty flags, controls that
//! scripts insert, move and remove, the events of typing and clicks, the
//! form methods), `web-forms-selection.json` (the selection members of
//! `input` and `textarea` and their `select` and `selectionchange`
//! events) and `web-forms-validation.json` (constraint validation: the
//! validity states, the English messages, the `invalid` events, `:valid`
//! and `:invalid`, `labels`) run here with the same input steps
//! (`swb_automation::run_steps`), and the values of their queries are the
//! ones that Chromium 148 gave (`tests/data/web-forms-script.json`: `just
//! probe --json` of the files, three equal runs each). Queries that wait
//! for tasks (`(async () => ...)`) run too: the event loop runs until the
//! promise settles.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{Evaluation, FontContext, JsonForm, Page, PageConfig, Size, Url};
use swb_net::NetworkFetcher;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A directory with test pages, removed at the end of the test.
struct Site(PathBuf);

impl Site {
    fn new(name: &str) -> Site {
        let dir =
            std::env::temp_dir().join(format!("swb-forms-script-{}-{name}", std::process::id()));
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

/// A page with JavaScript on that shows `html` (800x600, test fonts).
fn open(site: &Site, html: &str) -> Page {
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
    page.navigate(site.page("page.html", html));
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// The JSON value of an expression.
fn value(page: &mut Page, source: &str) -> Value {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{source}: {other:?}"),
    }
}

/// The values where swb departs from Chromium on purpose: (case, index of
/// the query, swb's value, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        "sel-set-range",
        4,
        r#""4 \"1,3,forward\" \"1,1,forward\" \"0,1,forward\" \"0,4,forward\" \"3,3,forward\"""#,
        "a selection offset inside a surrogate pair moves back to the start of the pair (the engine's selection is at character boundaries); Chromium keeps the offset",
    ),
    (
        "form-methods",
        7,
        r#""\"ok\" \"function\" false""#,
        "a control named like a member of the form does not hide it (no [LegacyOverrideBuiltIns]: every property read of a form, also `appendChild`, would build the list of its controls; measured in feature 9b: 20,000 `appendChild` calls on a growing form hit the time limit)",
    ),
    (
        "form-methods",
        8,
        r#""\"function\" \"HTMLFormControlsCollection\" 4 \"string\" 4""#,
        "as query 7 (`reset`, `elements`, `length` and `action` stay the members of the form)",
    ),
];

/// Runs the cases of the probe files against Chromium's values.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-forms-script.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let mut failures = Vec::new();
        let mut seen = 0;
        for file in [
            "web-forms-script",
            "web-forms-selection",
            "web-forms-validation",
        ] {
            let probe: Value = serde_json::from_str(
                &std::fs::read_to_string(repo_root().join(format!("tools/probes/{file}.json")))
                    .unwrap(),
            )
            .unwrap();
            for case in probe["cases"].as_array().unwrap() {
                let name = case["name"].as_str().unwrap();
                seen += 1;
                let site = Site::new(name);
                let mut page = open(&site, case["html"].as_str().unwrap());
                if let Some(steps) = case["steps"].as_array()
                    && let Err(error) = run_steps(&mut page, steps)
                {
                    failures.push(format!("{name}: {error}"));
                    continue;
                }
                for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
                    let source = query["js"].as_str().unwrap();
                    let want: Value = SWB_VALUES
                        .iter()
                        .find(|v| v.0 == name && v.1 == index)
                        .map_or_else(
                            || expected[name][index].clone(),
                            |v| serde_json::from_str(v.2).unwrap(),
                        );
                    let got = value(&mut page, source);
                    if got != want {
                        failures.push(format!(
                            "{name} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
                        ));
                    }
                }
            }
        }
        assert_eq!(seen, expected.as_object().unwrap().len());
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
