//! The `WindowProxy` of a top-level window against Chromium (M9 feature 12a,
//! ADR 0031 section 5): every query of the case `windowproxy-top` of
//! `tools/probes/web-frames.json` with Chromium's values.

use super::{case_html, open, results};
use crate::common::{evaluate_json as evaluate, json_differences, on_page_thread};

/// The rows where swb departs on purpose: (query, row of the array, why).
/// Each is a difference in what the window has, not in how the proxy
/// forwards.
const DEPARTURES: &[(usize, usize, &str)] = &[(
    6,
    17,
    "eval of a function whose global property is not configurable: swb has the TypeError of \
         ECMA-262 §19.2.1.3, V8 a SyntaxError; not a matter of the proxy",
)];

#[test]
fn the_top_level_window_matches_chromium() {
    on_page_thread(|| {
        let (mut page, _sim) = open(&case_html("windowproxy-top"), &[], true);
        let mut failures = Vec::new();
        for (query, result) in results("windowproxy-top").iter().enumerate() {
            let source = result["label"].as_str().unwrap();
            let swb = evaluate(&mut page, source);
            let mut found = Vec::new();
            json_differences(
                &format!("query {query}"),
                &swb,
                &result["value"],
                &mut found,
            );
            found.retain(|(path, _)| {
                !DEPARTURES
                    .iter()
                    .any(|&(q, row, _)| q == query && *path == format!("query {q}[{row}]"))
            });
            failures.extend(
                found
                    .into_iter()
                    .map(|(path, text)| format!("{path}: {text}")),
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
