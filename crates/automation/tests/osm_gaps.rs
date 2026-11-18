//! The small gaps of M12 feature 2 (openstreetmap.org): `ImageData`, `Attr`
//! as a `Node`, the `Worker` stand-in, `HTMLCanvasElement.getContext`, the
//! GIF without a color table, and the scroll of a text field after a
//! script sets its value.
//!
//! The cases of the Chromium probe `tools/probes/web-osm-gaps.json` run here
//! with the same input steps (`swb_automation::run_steps`), and the values of
//! their `js` queries are the ones that Chromium 148 gave
//! (`tests/data/web-osm-gaps.json`: `just probe --json
//! tools/probes/web-osm-gaps.json`, equal runs; the data file also holds the
//! pixels of the `pixels` queries, which this test does not run: `swb
//! probe --with-swb` compares them). A case with `resources` loads from
//! `http://127.0.0.1:8000/` (the probe's server), the other cases from a
//! `file:` URL, as the probe does; a fake fetcher serves the page, so no test
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
use swb_net::{Fetcher, Headers, NetError, Request, Response};

/// The values where swb departs from Chromium on purpose or not yet: (case,
/// query index, the member of the value that differs (empty: the whole
/// value), swb's value as JSON, why).
const SWB_VALUES: &[(&str, usize, &str, &str, &str)] = &[
    (
        "image-data",
        0,
        "bad",
        r#""TypeError: Failed to construct 'ImageData': parameter 1 is not of type 'Uint8ClampedArray'.""#,
        "swb picks the constructor by the type of the first argument's class (any object takes the data overload); Chromium takes the numeric one for a Uint8Array and converts the object to a number",
    ),
    (
        "canvas-2d-context",
        0,
        "",
        r#"{"size": [300, 150], "fn": "function", "names": "constructor,getContext,height,width", "ctx": null, "same": true, "canvas": null, "none": null, "noarg": "TypeError: Failed to execute 'getContext' on 'HTMLCanvasElement': 1 argument required, but only 0 present."}"#,
        "owner decision of M12: swb has no Canvas 2D, getContext returns null for every context type; the prototype has only getContext (no captureStream, toBlob, toDataURL, transferControlToOffscreen)",
    ),
    (
        "offscreen-canvas-and-image-bitmap",
        0,
        "",
        r#"{"off": "undefined", "cib": "undefined", "bitmap": "undefined"}"#,
        "OffscreenCanvas, createImageBitmap and ImageBitmap are not in swb (backlog; only the worker code of MapLibre uses them)",
    ),
    (
        "webgl-detection",
        0,
        "",
        r#"{"webgl": "undefined", "webgl2": "undefined", "fn": "function", "context": null, "lost": "undefined"}"#,
        "owner decision of M12: no WebGL; getContext returns null for webgl2, webgl and experimental-webgl, so a page takes its path for a browser without WebGL (Chromium with --disable-3d-apis does the same)",
    ),
    (
        "worker-stand-in",
        0,
        "shared",
        r#""undefined""#,
        "SharedWorker is not in swb: Craigslist takes its path for browsers without it, as in Chromium's replay",
    ),
    (
        "input-scroll-after-script-value",
        1,
        "",
        "[[0, 92], [0, 92, 92], [0], 0]",
        "swb keeps no scrollLeft for a text field (it reads 0) and focus() does not scroll the text of a field that has a value; the layout scrolls the text to the caret after the focus() (the probe's pixels)",
    ),
    (
        "input-scroll-typed-then-set",
        0,
        "",
        r#"[["a", 0, 36, 36, false], ["b", 0, 36, 36, true]]"#,
        "swb keeps no scrollLeft for a text field (it reads 0); the offset itself follows Chromium (crates/engine/tests/forms.rs, a_script_value_does_not_scroll_the_text)",
    ),
    (
        "input-scroll-typed-then-set",
        1,
        "",
        r#"[["a", 0, 92], ["b", 0, 47]]"#,
        "swb keeps no scrollLeft for a text field (it reads 0)",
    ),
    (
        "input-scroll-typed-then-set",
        2,
        "",
        "[0, 0]",
        "swb keeps no scrollLeft for a text field (it reads 0)",
    ),
];

/// The expected value of a query: Chromium's, with the departures of
/// [`SWB_VALUES`] for this case and index.
fn expected_value(chromium: &Value, name: &str, index: usize) -> Value {
    let mut want = chromium.clone();
    for &(case, at, member, json, _why) in SWB_VALUES {
        if case != name || at != index {
            continue;
        }
        let swb: Value = serde_json::from_str(json).unwrap();
        if member.is_empty() {
            want = swb;
        } else {
            want[member] = swb;
        }
    }
    want
}

/// A fetcher that serves one page at `url`, the `data:` URLs with a base64
/// payload (`data:image/gif;base64,...`), and a 404 for the rest.
struct Site {
    url: &'static str,
    html: String,
}

impl Fetcher for Site {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let url = request.url.as_str();
        if url == self.url {
            return Ok(html_response(request, 200, self.html.as_bytes().to_vec()));
        }
        let data = url
            .strip_prefix("data:")
            .and_then(|rest| rest.split_once(";base64,"));
        let Some((mime, payload)) = data else {
            return Ok(html_response(request, 404, b"not found".to_vec()));
        };
        let body = data_encoding::BASE64.decode(payload.as_bytes()).unwrap();
        let headers: Headers = [("content-type", mime)].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status: 200,
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
            &std::fs::read_to_string(repo_root().join("tools/probes/web-osm-gaps.json")).unwrap(),
        )
        .unwrap();
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-osm-gaps.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let cases = probe["cases"].as_array().unwrap();
        assert_eq!(cases.len(), expected.as_object().unwrap().len());
        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let url = if case.get("resources").is_some() {
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
                let Some(source) = query["js"].as_str() else {
                    continue;
                };
                let want = expected_value(&expected[name][index], name, index);
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
