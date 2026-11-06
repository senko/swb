//! The small DOM gaps of M9 feature 1: `ShadowRoot`, the qualified name of
//! a parsed `xmlns` attribute, `document.domain` and `originAgentCluster`,
//! `navigator.plugins` and `mimeTypes`, `window.postMessage()` to the own
//! window, and `:focus-visible` after a pointer click and a script `focus()`.
//!
//! The cases of the Chromium probe `tools/probes/web-small-gaps.json` run
//! here with the same input steps (`swb_automation::run_steps`), and the
//! values of their queries are the ones that Chromium 148 gave
//! (`tests/data/web-small-gaps.json`: `just probe --json
//! tools/probes/web-small-gaps.json`, three equal runs). A case whose name
//! ends in `-http` loads from `http://127.0.0.1:8000/` (the probe's
//! server), the other cases from a `file:` URL, as the probe does; a fake
//! fetcher serves the page, so no test touches the network.
//! [`SWB_VALUES`] lists the queries where swb departs from Chromium on
//! purpose.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{on_page_thread, repo_root, value};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{FontContext, Page, PageConfig, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value as JSON, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        "shadow-root-interface",
        7,
        r#""constructor""#,
        "ShadowRoot is the interface object only: no instance exists, so its members are left out (ADR 0028, M9 feature 1)",
    ),
    (
        "shadow-root-interface",
        8,
        r#"["undefined", false, null, false]"#,
        "no `attachShadow()` and no `shadowRoot`: swb has no shadow trees",
    ),
    (
        "navigator-plugins",
        11,
        r#""constructor""#,
        "Plugin is the interface object only: the plugin list is always empty, so no instance exists",
    ),
    (
        "navigator-plugins",
        12,
        r#""constructor""#,
        "MimeType is the interface object only: the list is always empty, so no instance exists",
    ),
    (
        "fv-click-script-editable",
        0,
        r#""a 0000000 1000000""#,
        "swb does not focus an element with contenteditable (crates/web/src/html/focus.rs)",
    ),
    (
        "fv-types-mouse",
        0,
        r#""text:F1 search:F1 url:F1 tel:F1 email:F1 password:F1 number:F1 date:F1 datetime-local:F1 month:F1 week:F1 time:F1 color:F0 range:F0 file:F0 checkbox:F0 radio:F0 submit:F0 button:F0 reset:F0 readonly-text:F1 text-notype:F1 textarea:F1 select:F1 summary:F0 link:F0 tabindex:F0 editable:-0 plaintext:-0 button:F0 iframe:F0""#,
        "swb does not focus an element with contenteditable",
    ),
    (
        "fv-types-keyboard",
        0,
        r#""text:F1 search:F1 url:F1 tel:F1 email:F1 password:F1 number:F1 date:F1 datetime-local:F1 month:F1 week:F1 time:F1 color:F1 range:F1 file:F1 checkbox:F1 radio:F1 submit:F1 button:F1 reset:F1 readonly-text:F1 text-notype:F1 textarea:F1 select:F1 summary:F1 link:F1 tabindex:F1 editable:-0 plaintext:-0 button:F1 iframe:F0""#,
        "swb does not focus an element with contenteditable",
    ),
];

/// Serves one page; other URLs get a 404.
struct OnePage {
    url: &'static str,
    html: String,
}

impl Fetcher for OnePage {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let (status, body) = if request.url.as_str() == self.url {
            (200, self.html.as_bytes().to_vec())
        } else {
            (404, b"not found".to_vec())
        };
        let headers: Headers = [("content-type", "text/html")].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
            redirected: false,
        })
    }
}

/// A page with JavaScript on (800x600, test fonts) that shows `html` at
/// `url`, loaded.
fn open(url: &'static str, html: &str) -> Page {
    let config = PageConfig {
        fetcher: Arc::new(OnePage {
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
            &std::fs::read_to_string(repo_root().join("tools/probes/web-small-gaps.json")).unwrap(),
        )
        .unwrap();
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-small-gaps.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let cases = probe["cases"].as_array().unwrap();
        assert_eq!(cases.len(), expected.as_object().unwrap().len());
        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let url = if name.ends_with("-http") {
                "http://127.0.0.1:8000/case.html"
            } else {
                "file:///case.html"
            };
            let mut page = open(url, case["html"].as_str().unwrap());
            let steps = case["steps"].as_array().cloned().unwrap_or_default();
            if let Err(error) = run_steps(&mut page, &steps) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
                let source = query["js"].as_str().unwrap();
                let departure = SWB_VALUES
                    .iter()
                    .find(|v| v.0 == name && v.1 == index)
                    .map(|v| serde_json::from_str::<Value>(v.2).unwrap());
                let want = departure.as_ref().unwrap_or(&expected[name][index]);
                let got = value(&mut page, source);
                if &got != want {
                    failures.push(format!(
                        "{name} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
