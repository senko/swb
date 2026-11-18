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
use common::{html_response, on_page_thread, repo_root, value};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{FontContext, Page, PageConfig, Size, Url};
use swb_net::{Fetcher, NetError, Request, Response};

/// The values where swb departs from Chromium on purpose or not yet: (case,
/// query index, swb's value as JSON, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[];

/// A fetcher that serves one page at `url` and a 404 for the rest.
struct Site {
    url: &'static str,
    html: String,
}

impl Fetcher for Site {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if request.url.as_str() == self.url {
            return Ok(html_response(request, 200, self.html.as_bytes().to_vec()));
        }
        Ok(html_response(request, 404, b"not found".to_vec()))
    }
}

/// A page with JavaScript on (800x600, test fonts) that shows `html` at
/// `url`, loaded.
fn open(url: &'static str, html: &str) -> Page {
    let config = PageConfig {
        fetcher: Arc::new(Site {
            url,
            html: html.to_owned(),
        }),
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
    page.navigate(Url::parse(url).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// Runs the cases of the probe file against Chromium's values.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let probe: Value = serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/events-after-layout.json"))
                .unwrap(),
        )
        .unwrap();
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/events-after-layout.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let cases = probe["cases"].as_array().unwrap();
        assert_eq!(cases.len(), expected.as_object().unwrap().len());
        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let mut page = open("file:///case.html", case["html"].as_str().unwrap());
            let steps = case["steps"].as_array().cloned().unwrap_or_default();
            if let Err(error) = run_steps(&mut page, &steps) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
                let Some(source) = query["js"].as_str() else {
                    continue;
                };
                let mut want = expected[name][index].clone();
                for &(case, at, swb, _why) in SWB_VALUES {
                    if case == name && at == index {
                        want = serde_json::from_str(swb).unwrap();
                    }
                }
                let got = value(&mut page, source);
                if got != want {
                    failures.push(format!(
                        "{name} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
