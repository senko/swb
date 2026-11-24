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
use common::{OnePage, on_page_thread, open_script_page, run_probe_file};

use std::sync::Arc;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value as JSON, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        "shadow-root-interface",
        7,
        r#""activeElement,adoptedStyleSheets,clonable,constructor,delegatesFocus,elementFromPoint,elementsFromPoint,getAnimations,host,innerHTML,mode,onslotchange,serializable,slotAssignment,styleSheets""#,
        "`ShadowRoot` lacks the members that ADR 0036 section 9 leaves out of M10",
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

/// The page of a probe case.
fn open(case: &Value) -> Page {
    let url = if case["name"].as_str().unwrap().ends_with("-http") {
        "http://127.0.0.1:8000/case.html"
    } else {
        "file:///case.html"
    };
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
        let failures = run_probe_file("web-small-gaps", open, |name, index, chromium| {
            SWB_VALUES
                .iter()
                .find(|v| v.0 == name && v.1 == index)
                .map_or_else(|| chromium.clone(), |v| serde_json::from_str(v.2).unwrap())
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
