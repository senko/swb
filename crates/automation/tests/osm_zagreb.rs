//! M12 feature 4, the exit test of the OpenStreetMap target: the first view
//! of `openstreetmap.org` (Zagreb, Ban Jelačić Square) and 60 groups of user
//! steps, with JavaScript on, against Chromium's replay of the same fixture.
//! The local fixture is `fixtures/js/osm-zagreb` (git-ignored; set
//! `SWB_JS_FIXTURES` to the directory `fixtures/js` of another checkout; the
//! test skips when it is missing). The page loads at 1280 x 800 with the page
//! clock fixed at the `captured` time of the fixture and runs for 1.5 s of page
//! time, as `github_swb.rs` does it; the groups then run in order in one
//! page, each step followed by 300 ms of page time (the pause of `swbtools
//! replay`).
//!
//! The reference is Chromium without WebGL (`--disable-3d-apis`; owner
//! decision of 2026-11-18). OSM's vector map (the Transport Map) needs WebGL:
//! its click fails in that Chromium and in swb alike (`getContext` returns
//! null), the layer panel then keeps its list but loads no tiles, and the
//! thumbnails of the panel stay empty. In Chromium the click also breaks the
//! tile layer: the map has no tiles from the group `layer transport map` on
//! (`tiles.loaded` is 0 in the records 15 to 60). So there are two tests with
//! the same checks and their own load: [`osm_zagreb_matches_chromium_without_webgl`]
//! runs all 60 groups; [`osm_zagreb_without_the_transport_map_matches_chromium`]
//! runs the same steps without that group (59 groups), so that the map keeps
//! its tiles for the later groups (the data files `osm-zagreb-nt-*`).
//!
//! What the test checks per sequence (M12, "What works means", items 1 to
//! 12), after the load and after each group:
//!
//! 1. The record `window.__o()` (`data/osm-zagreb.js`; the URL and hash, the
//!    view, the loaded tiles, the base layer and overlays, the panes with
//!    their titles and texts, the counts of rows, the outlined objects, the
//!    marker count, the share inputs, the welcome box, the scale, the
//!    attribution and the number of elements): every field equals Chromium's
//!    (`data/osm-zagreb-chromium.json`), with two kinds of numbers compared
//!    within a tolerance: the rectangles of the visible elements of the
//!    header, the map controls, the layer, share and legend panel and the
//!    sidebar pages (`rects`, within [`RECT_TOLERANCE`]), and the numbers of
//!    the `d` attribute of each outline path (within [`OUTLINE_TOLERANCE`]).
//! 2. The pixels (`data/osm-zagreb-pixel-points.json` and `-pixels.json`):
//!    per group about 40 points that were chosen from Chromium's screenshots
//!    alone (`osm_zagreb_records.py points`: the fixed parts of the page, the
//!    inside of the rectangles of the record, flat points next to a color
//!    edge in the map and the sidebar, and a coarse grid; each point is one
//!    color in a 7 x 7 square in all three Chromium runs; none lies in a
//!    thumbnail of the layer list, which needs WebGL; the exception is the
//!    "ink" points: the core pixel of a glyph such as the "(i)" icon, which
//!    does not depend on what is behind it). A channel may differ by
//!    [`PIXEL_TOLERANCE`].
//! 3. The URLs that the fixture lacks, per group, equal Chromium's
//!    (`data/osm-zagreb-missing.json`). The Matomo URLs (`matomo.php?...`)
//!    have the time and a random number in their query: the test and the data
//!    keep their path only.
//! 4. The console: every message at the level `error` or `warn` of swb's page
//!    appears as often as in Chromium's (`data/osm-zagreb-console.json`: the
//!    console errors and warnings and the uncaught exceptions of Chromium's
//!    replay, with the smallest and the largest number of times in its three
//!    runs); a message that Chromium has and swb lacks fails too. The "Failed
//!    to load resource" lines are Chromium's network log, not page messages.
//!
//! The data come from `tools/osm_zagreb_records.py` (`steps`, `records`,
//! `points`; the module comment of the script has the commands): Chromium
//! 148.0.7778.96 (headless shell, Playwright) replayed the fixture three
//! times per sequence; the three records and the missing URLs are equal in all
//! runs.
//!
//! The test collects the problems of all checks and asserts once at the end.
//! Departures (swb's differences from Chromium that the test accepts) are in
//! `data/osm-zagreb-departures.json` and `-nt-departures.json`, each with its
//! cause: a backlog item of `docs/roadmap.md`, or a bug that part 4c of M12
//! feature 4 fixes. A departure that no longer applies fails the test: remove
//! it (see [`Departures`]).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{
    PIXEL_TOLERANCE, Recording, fixture, load_at_capture_with, on_page_thread, step_groups, value,
};
use serde_json::{Value, json};
use swb_automation::run_steps;
use swb_engine::{ConsoleLevel, Page};
use swb_net::ReplayFetcher;

/// The largest difference, in CSS px, of a coordinate or a size in the
/// `rects` of a record. Chromium and swb agree to the rounding of the record
/// (0.01 px; the largest difference of two numbers that agree is below
/// 0.05 px, measured over the 61 records of each sequence); a layout that
/// differs does so by 3 px or more (a wrapped line, a shifted row). So 0.1
/// px passes the last-layout-unit differences (1/64 px) and nothing else.
const RECT_TOLERANCE: f64 = 0.1;

/// The largest difference of a number in the `d` attribute of an outline path
/// (px). Leaflet rounds the coordinates to whole px, so 1 is one step of
/// rounding.
const OUTLINE_TOLERANCE: f64 = 1.0;

/// The pause after each step, in ms of page time (`swbtools replay`).
const PAUSE_MS: u64 = 300;

/// The URL of the page's analytics, whose query has the time and a random
/// number.
const MATOMO: &str = "https://matomo.openstreetmap.org/matomo.php";

/// Departure of the console: Chromium's `webglcontextcreationerror` event is
/// a `WebGLContextEvent` with a `statusMessage`, which `MapLibre` puts into the
/// text of its error; swb's event is a plain `Event` (backlog of M12 feature
/// 2: no `WebGLContextEvent`). The text of Chromium's exception has this part
/// more.
const STATUS_MESSAGE: &str =
    ",\"statusMessage\":\"disabled by enterprise policy or commandline switch\"";

/// Departure of the console: Matomo's `_pk_ses` cookie has an expiry that the
/// page computes from `Date.now()`. Chromium's reference reports that it
/// could not set it (the page's time is fixed at the capture time, and
/// Chromium's cookie store compares with the system time, so the cookie
/// arrived expired). swb's cookie jar takes the page clock's time (ADR 0012,
/// "The page clock"), so the cookie is set and the message may be missing.
const COOKIE_ERROR: &str = "There was an error setting cookie";

/// The data files of one sequence of step groups.
struct Sequence {
    /// The number of groups after the load.
    groups: usize,
    steps: &'static str,
    chromium: &'static str,
    missing: &'static str,
    console: &'static str,
    pixel_points: &'static str,
    pixels: &'static str,
    departures: &'static str,
}

const FULL: Sequence = Sequence {
    groups: 60,
    steps: include_str!("data/osm-zagreb-steps.json"),
    chromium: include_str!("data/osm-zagreb-chromium.json"),
    missing: include_str!("data/osm-zagreb-missing.json"),
    console: include_str!("data/osm-zagreb-console.json"),
    pixel_points: include_str!("data/osm-zagreb-pixel-points.json"),
    pixels: include_str!("data/osm-zagreb-pixels.json"),
    departures: include_str!("data/osm-zagreb-departures.json"),
};

const NO_TRANSPORT: Sequence = Sequence {
    groups: 59,
    steps: include_str!("data/osm-zagreb-nt-steps.json"),
    chromium: include_str!("data/osm-zagreb-nt-chromium.json"),
    missing: include_str!("data/osm-zagreb-nt-missing.json"),
    console: include_str!("data/osm-zagreb-nt-console.json"),
    pixel_points: include_str!("data/osm-zagreb-nt-pixel-points.json"),
    pixels: include_str!("data/osm-zagreb-nt-pixels.json"),
    departures: include_str!("data/osm-zagreb-nt-departures.json"),
};

/// The differences from Chromium that a sequence accepts, as the file of the
/// sequence lists them: a list of `{"cause": TEXT, "groups": [NAMES],
/// "fields": [PREFIXES], "points": [NAMES]}`. A difference of a record is
/// named `group: FIELD` (a field of the record such as `sidebar`,
/// `rects.panel[.overlay-layers label #2]` or `outlines.d[0]`); an entry
/// accepts it if the group is in `groups` (an empty list: every group) and the
/// name starts with one of the `fields`. A pixel point is accepted if its
/// name is in `points`.
///
/// Each use is counted: a departure whose group or point no longer differs
/// is a problem, so the list stays exact.
struct Departures {
    list: Vec<Departure>,
    used: BTreeSet<(usize, String)>,
}

struct Departure {
    cause: String,
    groups: Vec<String>,
    fields: Vec<String>,
    points: Vec<String>,
}

impl Departures {
    fn load(text: &str) -> Self {
        let entries: Vec<Value> = serde_json::from_str(text).unwrap();
        let strings = |entry: &Value, key: &str| -> Vec<String> {
            entry[key]
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|s| s.as_str().unwrap().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        let list = entries
            .iter()
            .map(|entry| Departure {
                cause: entry["cause"].as_str().unwrap().to_owned(),
                groups: strings(entry, "groups"),
                fields: strings(entry, "fields"),
                points: strings(entry, "points"),
            })
            .collect();
        Departures {
            list,
            used: BTreeSet::new(),
        }
    }

    /// True if a departure accepts the difference `field` of `group`.
    fn accepts_field(&mut self, group: &str, field: &str) -> bool {
        let found = self.list.iter().position(|d| {
            (d.groups.is_empty() || d.groups.iter().any(|g| g == group))
                && d.fields.iter().any(|prefix| field.starts_with(prefix))
        });
        if let Some(at) = found {
            self.used.insert((at, group.to_owned()));
        }
        found.is_some()
    }

    /// True if a departure accepts the pixel point `name`.
    fn accepts_point(&mut self, name: &str) -> bool {
        let found = self
            .list
            .iter()
            .position(|d| d.points.iter().any(|p| p == name));
        if let Some(at) = found {
            self.used.insert((at, name.to_owned()));
        }
        found.is_some()
    }

    /// The departures (or their groups and points) that no difference used.
    fn unused(&self) -> Vec<String> {
        let mut found = Vec::new();
        for (at, d) in self.list.iter().enumerate() {
            let seen = |key: &String| self.used.contains(&(at, key.clone()));
            if !d.fields.is_empty() {
                let stale: Vec<&String> = if d.groups.is_empty() {
                    let any = self.used.iter().any(|(i, _)| *i == at);
                    if any {
                        vec![]
                    } else {
                        d.fields.iter().collect()
                    }
                } else {
                    d.groups.iter().filter(|g| !seen(g)).collect()
                };
                if !stale.is_empty() {
                    found.push(format!(
                        "departure no longer needed ({}): {stale:?} of fields {:?}",
                        d.cause, d.fields
                    ));
                }
            }
            let stale: Vec<&String> = d.points.iter().filter(|p| !seen(p)).collect();
            if !stale.is_empty() {
                found.push(format!(
                    "departure no longer needed ({}): points {stale:?}",
                    d.cause
                ));
            }
        }
        found
    }
}

/// The steps of a group with the pause after each step, as `swbtools replay`
/// and `osm_zagreb_records.py swb` run them.
fn paced(steps: &[Value]) -> Vec<Value> {
    steps
        .iter()
        .flat_map(|step| [step.clone(), json!({ "wait": PAUSE_MS })])
        .collect()
}

/// The record `window.__o()` of the page.
fn record(page: &mut Page) -> Value {
    let text = value(page, "window.__o()");
    serde_json::from_str(text.as_str().unwrap()).unwrap()
}

/// A URL with a Matomo query cut off.
fn masked(url: &str) -> String {
    if url.starts_with(MATOMO) {
        MATOMO.to_owned()
    } else {
        url.to_owned()
    }
}

/// The text of a page message as Chromium's replay reports it: for an
/// uncaught exception the message without swb's prefix (`a promise:
/// Uncaught (in promise) TypeError: `) and without the name of an error
/// class.
fn page_message(text: &str) -> String {
    let Some((_, rest)) = text.split_once("Uncaught ") else {
        return text.to_owned();
    };
    let rest = rest.strip_prefix("(in promise) ").unwrap_or(rest);
    match rest.split_once(": ") {
        Some((name, message)) if name.ends_with("Error") && !name.contains(' ') => {
            message.to_owned()
        }
        _ => rest.to_owned(),
    }
}

/// A difference: the field it names and its text.
type Difference = (String, String);

/// The fields of a record that the test compares exactly: all but the
/// rectangles and the outline paths (the numbers of `d`).
fn exact_part(record: &Value) -> Value {
    let mut record = record.clone();
    let fields = record.as_object_mut().unwrap();
    fields.remove("rects");
    if let Some(outlines) = fields.get_mut("outlines").and_then(Value::as_object_mut) {
        outlines.remove("d");
    }
    record
}

/// The differences between two records in the fields of [`exact_part`].
fn field_differences(group: &str, got: &Value, want: &Value) -> Vec<Difference> {
    let (got, want) = (exact_part(got), exact_part(want));
    let keys: BTreeSet<&String> = got
        .as_object()
        .unwrap()
        .keys()
        .chain(want.as_object().unwrap().keys())
        .collect();
    let mut found = Vec::new();
    for key in keys {
        let (a, b) = (json!({ key: got.get(key) }), json!({ key: want.get(key) }));
        let messages = common::differences(group, &a, &b, &[], 0);
        found.extend(messages.into_iter().map(|m| (key.clone(), m)));
    }
    found
}

/// The rectangles of a record's area as `(selector, index) -> [x, y, w, h]`.
fn rect_entries(record: &Value, area: &str) -> BTreeMap<(String, u64), [f64; 4]> {
    record["rects"][area]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            let at = |i: usize| entry[i].as_f64().unwrap();
            (
                (
                    entry[0].as_str().unwrap().to_owned(),
                    entry[1].as_u64().unwrap(),
                ),
                [at(2), at(3), at(4), at(5)],
            )
        })
        .collect()
}

/// The differences in the rectangles of two records (see [`RECT_TOLERANCE`]).
fn rect_differences(group: &str, got: &Value, want: &Value) -> Vec<Difference> {
    let areas: BTreeSet<&String> = got["rects"]
        .as_object()
        .into_iter()
        .chain(want["rects"].as_object())
        .flat_map(|areas| areas.keys())
        .collect();
    let mut found = Vec::new();
    for area in areas {
        let (a, b) = (rect_entries(got, area), rect_entries(want, area));
        let keys: BTreeSet<&(String, u64)> = a.keys().chain(b.keys()).collect();
        for key in keys {
            let field = format!("rects.{area}[{} #{}]", key.0, key.1);
            let text = match (a.get(key), b.get(key)) {
                (Some(x), Some(y))
                    if x.iter()
                        .zip(y)
                        .all(|(p, q)| (p - q).abs() <= RECT_TOLERANCE) =>
                {
                    continue;
                }
                (Some(x), Some(y)) => format!("swb {x:?} != Chromium {y:?}"),
                (Some(x), None) => format!("only in swb {x:?}"),
                (None, y) => format!("only in Chromium {y:?}"),
            };
            found.push((field.clone(), format!("{group}: {field}: {text}")));
        }
    }
    found
}

/// The numbers of a path `d` and what is left of it (the commands and the
/// separators), as `(skeleton, numbers)`.
fn path_numbers(d: &str) -> (String, Vec<f64>) {
    let mut skeleton = String::new();
    let mut numbers = Vec::new();
    let chars: Vec<char> = d.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let starts = chars[i].is_ascii_digit()
            || (matches!(chars[i], '-' | '.')
                && chars
                    .get(i + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == '.'));
        if starts {
            let from = i;
            i += 1;
            while chars
                .get(i)
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                i += 1;
            }
            let text: String = chars[from..i].iter().collect();
            numbers.push(text.parse().unwrap_or(f64::NAN));
            skeleton.push('#');
        } else {
            skeleton.push(chars[i]);
            i += 1;
        }
    }
    (skeleton, numbers)
}

/// The differences in the outline paths (`outlines.d`) of two records: the
/// commands are equal, the numbers differ by [`OUTLINE_TOLERANCE`] at most.
fn outline_differences(group: &str, got: &Value, want: &Value) -> Vec<Difference> {
    let paths = |record: &Value| -> Vec<String> {
        record["outlines"]["d"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|d| d.as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let (a, b) = (paths(got), paths(want));
    if a.len() != b.len() {
        let field = "outlines.d".to_owned();
        let text = format!(
            "{group}: {field}: swb has {} paths, Chromium {}",
            a.len(),
            b.len()
        );
        return vec![(field, text)];
    }
    let mut found = Vec::new();
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        let ((skeleton_x, nx), (skeleton_y, ny)) = (path_numbers(x), path_numbers(y));
        let same = skeleton_x == skeleton_y
            && nx.len() == ny.len()
            && nx
                .iter()
                .zip(&ny)
                .all(|(p, q)| (p - q).abs() <= OUTLINE_TOLERANCE);
        if !same {
            let field = format!("outlines.d[{i}]");
            let text = format!("{group}: {field}: swb {x:?} != Chromium {y:?}");
            found.push((field, text));
        }
    }
    found
}

/// The differences of a group's record from Chromium's that no departure
/// accepts.
fn record_problems(
    group: &str,
    got: &Value,
    want: &Value,
    departures: &mut Departures,
) -> Vec<String> {
    let mut all = field_differences(group, got, want);
    all.extend(rect_differences(group, got, want));
    all.extend(outline_differences(group, got, want));
    all.into_iter()
        .filter(|(field, _)| !departures.accepts_field(group, field))
        .map(|(_, message)| message)
        .collect()
}

/// The problems of the console: a message that swb's page has more or
/// fewer times than the range of Chromium's runs.
fn console_problems(page: &Page, chromium: &str) -> Vec<String> {
    let entries: Vec<Value> = serde_json::from_str(chromium).unwrap();
    let mut want: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for entry in &entries {
        let text = entry["text"].as_str().unwrap().replace(STATUS_MESSAGE, "");
        if text.starts_with("Failed to load resource") {
            continue;
        }
        let range = want.entry(text).or_default();
        range.0 += entry["min"].as_u64().unwrap();
        range.1 += entry["max"].as_u64().unwrap();
    }
    let mut got: BTreeMap<String, u64> = BTreeMap::new();
    for message in page.console_messages() {
        if matches!(message.level, ConsoleLevel::Error | ConsoleLevel::Warn) {
            *got.entry(page_message(&message.text)).or_default() += 1;
        }
    }
    let texts: BTreeSet<&String> = got.keys().chain(want.keys()).collect();
    texts
        .into_iter()
        .filter_map(|text| {
            let n = got.get(text).copied().unwrap_or(0);
            let (mut low, high) = want.get(text).copied().unwrap_or((0, 0));
            if text.starts_with(COOKIE_ERROR) {
                low = 0;
            }
            if (low..=high).contains(&n) {
                return None;
            }
            let who = if n == 0 {
                "swb lacks it"
            } else {
                "count differs"
            };
            Some(format!(
                "console ({who}): swb {n} times, Chromium {low} to {high} times: {text}"
            ))
        })
        .collect()
}

/// The URLs that the fixture lacked since `seen` requests, masked.
fn missing_urls(fetcher: &Recording, seen: usize) -> Vec<String> {
    fetcher
        .missing_since(seen)
        .iter()
        .map(|url| masked(url))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The problem of a group with the URLs that the fixture lacked since the
/// record before: they must be the URLs that it lacked in Chromium's run.
fn missing_problems(
    label: &str,
    fetcher: &Recording,
    seen: &mut usize,
    want: &Value,
) -> Option<String> {
    let found = missing_urls(fetcher, *seen);
    *seen = fetcher.missing_count();
    let want_urls: Vec<String> = serde_json::from_value(want["missing"].clone()).unwrap();
    assert_eq!(want["label"], label);
    (found != want_urls).then(|| {
        format!("{label}: URLs that the fixture lacks: swb {found:?} != Chromium {want_urls:?}")
    })
}

/// The pixel differences of a group, without those that a departure
/// accepts.
fn pixel_problems(
    label: &str,
    page: &mut Page,
    points: &[common::Point],
    pixels: &Value,
    departures: &mut Departures,
) -> Vec<String> {
    common::pixel_mismatches(label, page, points, pixels, PIXEL_TOLERANCE)
        .into_iter()
        .filter(|(name, _)| !departures.accepts_point(name))
        .map(|(_, message)| message)
        .collect()
}

fn check(sequence: &Sequence, dir: &std::path::Path) {
    let started = Instant::now();
    let expected: Vec<Value> = serde_json::from_str(sequence.chromium).unwrap();
    let missing: Vec<Value> = serde_json::from_str(sequence.missing).unwrap();
    let groups = step_groups(sequence.steps);
    assert_eq!(groups.len(), sequence.groups);
    assert_eq!(
        expected.len(),
        sequence.groups + 1,
        "the load and the groups"
    );
    assert_eq!(missing.len(), sequence.groups + 1);
    let (pixel_groups, points) = common::pixel_points(sequence.pixel_points);
    let pixels: Value = serde_json::from_str(sequence.pixels).unwrap();
    let mut departures = Departures::load(sequence.departures);

    let fetcher = Arc::new(Recording::new(ReplayFetcher::load(dir).unwrap()));
    let mut page = load_at_capture_with(dir, Duration::from_millis(1500), fetcher.clone());
    assert!(page.has_script_runtime());
    page.evaluate(
        include_str!("data/osm-zagreb.js"),
        swb_engine::JsonForm::Strict,
    );

    let mut problems = Vec::new();
    let mut sampled = 0;
    let mut seen_missing = 0;
    let labels = std::iter::once("load".to_owned())
        .chain(groups.iter().map(|(name, _)| name.clone()))
        .collect::<Vec<_>>();
    for (number, label) in labels.iter().enumerate() {
        if number > 0 {
            run_steps(&mut page, &paced(&groups[number - 1].1))
                .unwrap_or_else(|e| panic!("group {label}: {e}"));
        }
        problems.extend(record_problems(
            label,
            &record(&mut page),
            &expected[number],
            &mut departures,
        ));
        problems.extend(missing_problems(
            label,
            &fetcher,
            &mut seen_missing,
            &missing[number],
        ));
        if let Some(at) = pixel_groups.iter().position(|g| g == label) {
            let pixels = &pixels[label.as_str()];
            problems.extend(pixel_problems(
                label,
                &mut page,
                &points[at],
                pixels,
                &mut departures,
            ));
            sampled += 1;
        }
    }
    if sampled != pixel_groups.len() {
        problems.push("a group of the pixel check is missing".to_owned());
    }
    problems.extend(console_problems(&page, sequence.console));
    problems.extend(departures.unused());
    eprintln!("osm_zagreb: {:.1} s", started.elapsed().as_secs_f64());
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// Runs a sequence in a page thread; skips if the fixture is missing.
fn run(sequence: &'static Sequence) {
    let Some(dir) = fixture("osm-zagreb") else {
        eprintln!("skipped: fixtures/js/osm-zagreb is missing");
        return;
    };
    on_page_thread(move || check(sequence, &dir));
}

#[test]
fn osm_zagreb_matches_chromium_without_webgl() {
    run(&FULL);
}

#[test]
fn osm_zagreb_without_the_transport_map_matches_chromium() {
    run(&NO_TRANSPORT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_numbers_split_a_path_into_commands_and_numbers() {
        let (skeleton, numbers) = path_numbers("M10 -20.5L3,4.25z");
        assert_eq!(skeleton, "M# #L#,#z");
        assert_eq!(numbers, vec![10.0, -20.5, 3.0, 4.25]);
    }

    #[test]
    fn outline_paths_compare_within_one_px() {
        let record = |d: &str| json!({ "outlines": { "d": [d] } });
        let same = |a: &str, b: &str| outline_differences("g", &record(a), &record(b)).is_empty();
        assert!(same("M10 20L30 41", "M11 19L30 40"));
        assert!(!same("M10 20L30 41", "M12 20L30 41"));
        assert!(!same("M10 20L30 41", "M10 20L30 41L1 1"));
        assert!(!same("M10 20", "M10 20z"));
    }

    #[test]
    fn rectangles_compare_within_the_tolerance_and_name_missing_ones() {
        let record = |rects: Value| json!({ "rects": { "panel": rects } });
        let a = record(json!([["label", 0, 10, 20, 30, 40], ["p", 0, 0, 0, 5, 5]]));
        let b = record(json!([
            ["label", 0, 10.5, 20, 30, 41.5],
            ["q", 0, 0, 0, 5, 5]
        ]));
        let found = rect_differences("g", &a, &b);
        let fields: Vec<&str> = found.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(
            fields,
            [
                "rects.panel[label #0]",
                "rects.panel[p #0]",
                "rects.panel[q #0]"
            ]
        );
    }

    #[test]
    fn departures_match_by_group_and_prefix_and_report_stale_ones() {
        let text = r#"[
            {"cause": "A", "groups": ["g1", "g2"], "fields": ["rects.panel[x"]},
            {"cause": "B", "points": ["001-a", "001-b"]}
        ]"#;
        let mut d = Departures::load(text);
        assert!(d.accepts_field("g1", "rects.panel[x #0]"));
        assert!(!d.accepts_field("g3", "rects.panel[x #0]"));
        assert!(!d.accepts_field("g1", "rects.panel[y #0]"));
        assert!(d.accepts_point("001-a"));
        assert!(!d.accepts_point("001-c"));
        let stale = d.unused();
        assert_eq!(stale.len(), 2, "{stale:?}");
        assert!(stale[0].contains("g2") && stale[1].contains("001-b"));
        assert!(d.accepts_field("g2", "rects.panel[x #1]"));
        assert!(d.accepts_point("001-b"));
        let rest = d.unused();
        assert!(rest.is_empty(), "{rest:?}");
    }
}
