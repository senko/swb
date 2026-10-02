//! Layout tests: every `tests/layout/*.html` is loaded in swb and its
//! element boxes are compared with the Chromium boxes stored next to it
//! (`*.boxes.json`). See docs/testing.md.
//!
//! Tests listed in `tests/layout/known-failures.txt` may fail; the test
//! reports them but does not fail. A listed test that passes makes the test
//! fail, so the list stays current.

mod common;

use std::sync::Arc;

use swb_engine::{Size, Url};
use swb_net::NetworkFetcher;

const TOLERANCE: f32 = 1.0;

#[test]
fn layout_tests_match_chromium() {
    let dir = common::repo_root().join("tests/layout");
    let known_failures: Vec<String> = std::fs::read_to_string(dir.join("known-failures.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().next().unwrap_or("").to_owned())
        .collect();

    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/layout exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "html"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no layout tests found");

    let mut failures = Vec::new();
    for html in entries {
        let name = html.file_stem().unwrap().to_string_lossy().into_owned();
        let reference = common::read_dump(&html.with_extension("boxes.json"));
        let url = Url::from_file_path(html.canonicalize().unwrap()).unwrap();
        let actual = common::load_boxes(
            Arc::new(NetworkFetcher::new()),
            url,
            Size::new(800.0, 600.0),
        );
        let result = common::compare(&reference, &actual, TOLERANCE);
        let passed = result.geometry >= 1.0;
        let expected_to_fail = known_failures.contains(&name);
        match (passed, expected_to_fail) {
            (true, true) => {
                failures.push(format!("{name}: passes; remove it from known-failures.txt"));
            }
            (false, false) => failures.push(format!(
                "{name}: geometry {:.4}\n    {}",
                result.geometry,
                result.mismatches.join("\n    ")
            )),
            (false, true) => eprintln!("{name}: known failure (geometry {:.4})", result.geometry),
            (true, false) => {}
        }
    }
    assert!(
        failures.is_empty(),
        "layout tests failed:\n{}",
        failures.join("\n")
    );
}
