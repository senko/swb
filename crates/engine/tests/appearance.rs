//! The `appearance` property (M12 feature 3 review fixes): every case of
//! the Chromium probe `tools/probes/appearance.json`, with the answers
//! that Chromium 148 gave (`tests/data/appearance.json`:
//! `uv run --project tools swbtools probe --json tools/probes/appearance.json`).
//!
//! The initial value is `none` and the user-agent style sheet gives `auto`
//! to the controls; the compat keywords compute to themselves; the keyword
//! set is Chromium's (`CSS.supports`); `-webkit-appearance` is an alias in
//! the CSSOM and `-moz-appearance` is not a property; `getComputedStyle`
//! lists `appearance`.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Size, Url};

use common::{OnePage, answer, differences, new_page, on_page_thread, repo_root};

#[test]
fn appearance_matches_chromium() {
    on_page_thread(|| {
        let probe: Value = serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/appearance.json")).unwrap(),
        )
        .unwrap();
        let expected: Value = serde_json::from_str(include_str!("data/appearance.json")).unwrap();
        let cases = probe["cases"].as_array().unwrap();
        let answers = expected["cases"].as_array().unwrap();
        assert_eq!(cases.len(), answers.len());
        let mut failures = Vec::new();
        for (case, results) in cases.iter().zip(answers) {
            let name = case["name"].as_str().unwrap();
            assert_eq!(case["name"], results["name"]);
            let html = case["html"].as_str().unwrap();
            let mut page = new_page(
                Arc::new(OnePage::with_data_urls(html)),
                2,
                Size::new(800.0, 600.0),
            );
            page.set_javascript(true);
            page.navigate(Url::parse(common::CASE_PAGE).unwrap());
            assert!(page.wait_until_loaded(Duration::from_secs(20)));
            for (row, result) in results["results"].as_array().unwrap().iter().enumerate() {
                let query = result["label"].as_str().unwrap();
                let got = answer(&mut page, query);
                if got != result["value"] {
                    failures.push(format!(
                        "{name} #{row}: {}\n{}",
                        &query[..query.len().min(120)],
                        differences(&result["value"], &got)
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
