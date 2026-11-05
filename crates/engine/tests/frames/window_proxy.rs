//! The `WindowProxy` of a top-level window against Chromium (M9 feature 12a,
//! ADR 0031 section 5): every query of the case `windowproxy-top` of
//! `tools/probes/web-frames.json` with Chromium's values.

use serde_json::Value as Json;
use swb_engine::{Evaluation, JsonForm};

use super::{case_html, open, results};
use crate::common::on_page_thread;

/// The rows where swb departs on purpose: (query, row of the array, why).
/// Each is a difference in what the window has, not in how the proxy
/// forwards.
const DEPARTURES: &[(usize, usize, &str)] = &[(
    6,
    17,
    "eval of a function whose global property is not configurable: swb has the TypeError of \
         ECMA-262 §19.2.1.3, V8 a SyntaxError; not a matter of the proxy",
)];

/// The differences of two values, by path.
fn differences(path: &str, swb: &Json, chromium: &Json, out: &mut Vec<String>) {
    match (swb, chromium) {
        (Json::Array(a), Json::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                differences(&format!("{path}[{index}]"), x, y, out);
            }
        }
        _ if swb == chromium => {}
        _ => out.push(format!("{path}: swb {swb}, Chromium {chromium}")),
    }
}

/// The value of an object in any key order, as JSON.
fn evaluate(page: &mut swb_engine::Page, source: &str) -> Json {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap_or(Json::String(text)),
        Some(Evaluation::Error(text)) => Json::String(format!("error: {text}")),
        None => Json::String("no evaluation".to_owned()),
    }
}

#[test]
fn the_top_level_window_matches_chromium() {
    on_page_thread(|| {
        let (mut page, _sim) = open(&case_html("windowproxy-top"), &[], true);
        let mut failures = Vec::new();
        for (query, result) in results("windowproxy-top").iter().enumerate() {
            let source = result["label"].as_str().unwrap();
            let swb = evaluate(&mut page, source);
            let mut found = Vec::new();
            differences(
                &format!("query {query}"),
                &swb,
                &result["value"],
                &mut found,
            );
            found.retain(|text| {
                !DEPARTURES.iter().any(|&(q, row, _)| {
                    q == query && text.starts_with(&format!("query {q}[{row}]:"))
                })
            });
            failures.extend(found);
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
