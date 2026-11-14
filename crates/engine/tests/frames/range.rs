//! `Range` across the documents of a runtime and with layout (M10 sub-features
//! 9b-1 and 9b-3, ADR 0039): the queries of the cases `range-frames`,
//! `geometry`, `geometry-more`, `geometry-frames` and `geometry-review` of
//! `tools/probes/web-range.json` with Chromium's values
//! (`tests/data/web-range-frames.json`: three equal runs of `swbtools probe
//! --json` of each case).
//!
//! - `range-frames`: the realm of a range, a range of one document set to the
//!   nodes of a frame's document (the entry moves to the frame's arena and stays
//!   live there), a range in a detached tree adopted by a frame's document (it
//!   goes with the tree, live in the new arena, and back), and a range in a
//!   frame whose node leaves the tree.
//! - `geometry*`: `getClientRects` and `getBoundingClientRect` of ranges over
//!   text on several lines, inline and block elements, a `br`, hidden content,
//!   scroll containers and a scrolled viewport, a transformed box, preserved
//!   white space, an inline block, and ranges of and into a frame's document;
//!   the numbers within [`GEOMETRY_TOLERANCE`].

use super::{MAIN, open, open_sized};
use crate::common::{evaluate_json as evaluate, on_page_thread};
use serde_json::Value as Json;
use swb_engine::Size;

/// The largest difference in px between a number of swb and Chromium's in the
/// geometry cases: the text of the probe is in `sans-serif`, which is a
/// bundled test font in swb and a system font in Chromium (glyph advances
/// differ by hundredths of a px).
const GEOMETRY_TOLERANCE: f64 = 0.5;

/// The probe's viewport.
const VIEWPORT: Size = Size::new(400.0, 300.0);

/// The values where swb departs from Chromium on purpose: (case, query index,
/// swb's value, why).
const DEPARTURES: &[(&str, usize, &str, &str)] = &[];

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
    const CASE: &str = "range-frames";
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

/// The parts of a value of the geometry queries: numbers and the text
/// between them.
fn parts(text: &str) -> Vec<Result<f64, String>> {
    let mut out: Vec<Result<f64, String>> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let number_len = rest
            .char_indices()
            .take_while(|&(i, c)| c.is_ascii_digit() || c == '.' || (i == 0 && c == '-'))
            .count();
        let number = rest.get(..number_len).and_then(|n| n.parse::<f64>().ok());
        if let Some(n) = number {
            out.push(Ok(n));
            rest = &rest[number_len..];
        } else {
            let c = rest.chars().next().unwrap();
            match out.last_mut() {
                Some(Err(s)) => s.push(c),
                _ => out.push(Err(c.to_string())),
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// True if `got` equals `want` with the numbers in strings within
/// [`GEOMETRY_TOLERANCE`].
fn close(got: &Json, want: &Json) -> bool {
    match (got, want) {
        (Json::Array(a), Json::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| close(x, y))
        }
        (Json::String(a), Json::String(b)) => {
            let (pa, pb) = (parts(a), parts(b));
            pa.len() == pb.len()
                && pa.iter().zip(&pb).all(|(x, y)| match (x, y) {
                    (Ok(x), Ok(y)) => (x - y).abs() <= GEOMETRY_TOLERANCE,
                    (Err(x), Err(y)) => x == y,
                    _ => false,
                })
        }
        _ => got == want,
    }
}

#[test]
fn range_geometry_matches_chromium() {
    on_page_thread(|| {
        let probe = probe();
        let data = chromium();
        let mut failures = Vec::new();
        for case in [
            "geometry",
            "geometry-more",
            "geometry-frames",
            "geometry-review",
        ] {
            let html = find(&probe, case)["html"].as_str().unwrap().to_owned();
            let results = find(&data, case)["results"].as_array().unwrap().clone();
            let top = format!("{MAIN}/case/case.html");
            let (mut page, _sim) = open_sized(&top, &html, &[], true, VIEWPORT);
            for (index, result) in results.iter().enumerate() {
                let source = result["label"].as_str().unwrap();
                let got = evaluate(&mut page, source);
                let want = DEPARTURES
                    .iter()
                    .find(|d| d.0 == case && d.1 == index)
                    .map_or_else(
                        || result["value"].clone(),
                        |d| serde_json::from_str(d.2).unwrap(),
                    );
                if !close(&got, &want) {
                    failures.push(format!(
                        "{case} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
