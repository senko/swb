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
use common::{DataUrls, OnePage, on_page_thread, open_script_page, run_probe_file};

use std::sync::Arc;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

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
fn expected_value(name: &str, index: usize, chromium: &Value) -> Value {
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

/// The page of a probe case.
fn open(case: &Value) -> Page {
    let url = if case.get("resources").is_some() {
        "http://127.0.0.1:8000/case.html"
    } else {
        "file:///case.html"
    };
    let fetcher = DataUrls(OnePage {
        url,
        html: case["html"].as_str().unwrap().to_owned(),
    });
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
        let failures = run_probe_file("web-osm-gaps", open, expected_value);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
