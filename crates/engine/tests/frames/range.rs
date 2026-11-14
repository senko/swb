//! `Range` across the documents of a runtime (M10 sub-feature 9b-1, ADR 0039):
//! the queries of the case `range-frames` of `tools/probes/web-range.json` with
//! Chromium's values (`tests/data/web-range-frames.json`: three equal runs of
//! `swbtools probe --json --case range-frames`): the realm of a range, a range of
//! one document set to the nodes of a frame's document (the entry moves to the
//! frame's arena and stays live there), a range in a detached tree adopted by a
//! frame's document (it goes with the tree, live in the new arena, and back),
//! and a range in a frame whose node leaves the tree.

use super::open;
use crate::common::{evaluate_json as evaluate, on_page_thread};
use serde_json::Value as Json;

const CASE: &str = "range-frames";

fn probe() -> Json {
    serde_json::from_str(include_str!("../../../../tools/probes/web-range.json")).unwrap()
}

fn chromium() -> Json {
    serde_json::from_str(include_str!("../data/web-range-frames.json")).unwrap()
}

fn find<'a>(list: &'a Json, name: &str) -> &'a Json {
    list["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no case {name}"))
}

#[test]
fn ranges_across_frames_match_chromium() {
    on_page_thread(|| {
        let probe = probe();
        let html = find(&probe, CASE)["html"].as_str().unwrap().to_owned();
        let data = chromium();
        let results = find(&data, CASE)["results"].as_array().unwrap().clone();
        let (mut page, _sim) = open(&html, &[], true);
        let mut failures = Vec::new();
        for (index, result) in results.iter().enumerate() {
            let source = result["label"].as_str().unwrap();
            let got = evaluate(&mut page, source);
            if got != result["value"] {
                failures.push(format!(
                    "{CASE} #{index}: {source}\n  expected: {}\n  actual:   {got}",
                    result["value"]
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
