//! The hover state and the boundary events after the content under a
//! still pointer changes (M12 feature 3a; Chromium's fake mouse move after
//! a layout change, a script and a scroll).
//!
//! The cases of the Chromium probe `tools/probes/events-after-layout.json`
//! run here with the same input steps (`swb_automation::run_steps`), and the
//! values of their `js` queries are the ones that Chromium 148 gave
//! (`tests/data/events-after-layout.json`: `just probe --json
//! tools/probes/events-after-layout.json`, three equal runs, mapped to the
//! values of the `js` queries). A fake fetcher serves the page, so no test
//! touches the network. [`SWB_VALUES`] lists the queries where swb departs
//! from Chromium on purpose or not yet.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{OnePage, on_page_thread, open_script_page, run_probe_file};

use std::sync::Arc;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

/// The values where swb departs from Chromium on purpose or not yet: (case,
/// query index, swb's value as JSON, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[];

/// The page of a probe case.
fn open(case: &Value) -> Page {
    let url = "file:///case.html";
    let fetcher = OnePage {
        url,
        html: case["html"].as_str().unwrap().to_owned(),
    };
    open_script_page(
        Arc::new(fetcher),
        Url::parse(url).unwrap(),
        Size::new(800.0, 600.0),
    )
}

/// Runs the cases of the probe file against Chromium's values.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let failures = run_probe_file("events-after-layout", open, |name, index, chromium| {
            SWB_VALUES
                .iter()
                .find(|v| v.0 == name && v.1 == index)
                .map_or_else(|| chromium.clone(), |v| serde_json::from_str(v.2).unwrap())
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
