//! Animation frames between a network request and its completion, in
//! virtual time (M12 feature 3c, ADR 0028 section 7 "Clocks").
//!
//! The cases of the Chromium probe `tools/probes/network-frames.json` run
//! here with the same input steps (`swb_automation::run_steps`) and a
//! virtual clock. Each case counts the animation frames between the start
//! of a request and its completion (`fetch()`, `XMLHttpRequest`, an
//! inserted image, an inserted script; started in a click handler, a
//! timer, the parser and the `load` event, and in chains where each
//! completion starts the next request). `tests/data/network-frames.json`
//! has the counts of three Chromium 148 runs of each case
//! (`just probe --json tools/probes/network-frames.json`, one list for each
//! run). A request that a click handler starts has one frame between its
//! start and its completion in every run (7 to 12 ms); one that a timer, the
//! parser or the `load` event starts has none in most runs (1 to 4 ms; a
//! frame falls into the window in about 1 of 8 requests). swb's count must
//! be one that Chromium gave at that position in some run. A fake fetcher
//! serves the page and its resources, so no test touches the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{html_response, on_page_thread, repo_root, value};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{ClockMode, FontContext, Page, PageClock, PageConfig, Size, Url};
use swb_net::{Fetcher, NetError, Request, Response};

const ORIGIN: &str = "http://probe.test";

/// The results where swb may also give another count than Chromium: (case,
/// position, the extra count, why).
const DEPARTURES: &[(&str, usize, u64, &str)] = &[(
    "fetch-load",
    0,
    1,
    "a request that the `load` handler starts is not held; while the first animation \
     frame is still due at page time 0, the frame can run before the completion when the \
     loader's thread answers late (seen in two runs under load; Chromium gave 1 in one run \
     of three)",
)];

/// Serves the page of a case at `/case.html` and its resources.
struct Site {
    html: String,
    resources: Value,
}

impl Fetcher for Site {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let path = request.url.path().trim_start_matches('/');
        if path == "case.html" {
            return Ok(html_response(request, 200, self.html.as_bytes().to_vec()));
        }
        let Some(resource) = self.resources.get(path) else {
            return Ok(html_response(request, 404, b"not found".to_vec()));
        };
        let text = resource["text"].as_str().unwrap_or_default();
        let mut response = html_response(request, 200, text.as_bytes().to_vec());
        let content_type = resource["content_type"].as_str().unwrap_or(match path {
            "x.js" => "text/javascript",
            _ => "text/plain",
        });
        response.headers = [("content-type", content_type)].into_iter().collect();
        Ok(response)
    }
}

/// A page with JavaScript on and a virtual clock that shows the case,
/// loaded.
fn open(case: &Value) -> Page {
    let config = PageConfig {
        fetcher: Arc::new(Site {
            html: case["html"].as_str().unwrap().to_owned(),
            resources: case["resources"].clone(),
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
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.navigate(Url::parse(&format!("{ORIGIN}/case.html")).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// The frame counts that the case's query returned.
fn frames_of(page: &mut Page, case: &Value) -> Vec<u64> {
    let source = case["measure"][0]["js"].as_str().unwrap();
    value(page, source)
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row[1].as_u64().unwrap())
        .collect()
}

#[test]
fn frames_between_a_request_and_its_completion_match_chromium() {
    on_page_thread(|| {
        let probe: Value = serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/network-frames.json")).unwrap(),
        )
        .unwrap();
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/network-frames.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let cases = probe["cases"].as_array().unwrap();
        assert_eq!(cases.len(), expected.as_object().unwrap().len());
        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let mut page = open(case);
            let steps = case["steps"].as_array().cloned().unwrap_or_default();
            if let Err(error) = run_steps(&mut page, &steps) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            let got = frames_of(&mut page, case);
            let runs: Vec<Vec<u64>> = serde_json::from_value(expected[name].clone()).unwrap();
            for (position, &frames) in got.iter().enumerate() {
                let seen: Vec<u64> = runs
                    .iter()
                    .filter_map(|r| r.get(position))
                    .copied()
                    .collect();
                let departs = DEPARTURES
                    .iter()
                    .any(|&(c, at, extra, _)| c == name && at == position && extra == frames);
                if !seen.contains(&frames) && !departs {
                    failures.push(format!(
                        "{name} #{position}: swb {frames} frames, Chromium {seen:?}"
                    ));
                }
            }
            if got.len() != runs[0].len() {
                failures.push(format!(
                    "{name}: {} results, Chromium {}",
                    got.len(),
                    runs[0].len()
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
