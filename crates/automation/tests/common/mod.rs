//! Helpers of the integration tests on the local JavaScript fixtures
//! (`fixtures/js/<name>`, git-ignored, ADR 0028 section 17). Set
//! `SWB_JS_FIXTURES` to the directory `fixtures/js` of another checkout;
//! a test skips when its fixture is missing.

// Each test file uses some of the helpers.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use swb_engine::{
    ClockMode, ConsoleLevel, Evaluation, FontContext, JsonForm, Page, PageClock, PageConfig, Size,
    Url,
};
use swb_net::{CookieJar, Fetcher, NetError, ReplayFetcher, Request, Response};

/// A fetcher that records the URLs of its requests, in order, and answers
/// from the fixture.
pub(crate) struct Recording {
    inner: ReplayFetcher,
    urls: Mutex<Vec<String>>,
}

impl Recording {
    /// A recording fetcher around `inner`.
    pub(crate) fn new(inner: ReplayFetcher) -> Self {
        Recording {
            inner,
            urls: Mutex::new(Vec::new()),
        }
    }

    /// The URLs requested so far, in order.
    pub(crate) fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }
}

impl Fetcher for Recording {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        self.urls.lock().unwrap().push(request.url.to_string());
        self.inner.fetch(request)
    }

    fn cookie_jar(&self) -> Option<&CookieJar> {
        self.inner.cookie_jar()
    }
}

/// The repository root.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The directory of the fixture `name`, or `None` (the test then skips).
pub(crate) fn fixture(name: &str) -> Option<PathBuf> {
    let root = std::env::var_os("SWB_JS_FIXTURES").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/js"),
        PathBuf::from,
    );
    let dir = root.join(name);
    dir.join("fixture.json").is_file().then_some(dir)
}

/// Runs `f` on a thread with the stack of the page thread (16 MiB).
pub(crate) fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// The fixture's `fixture.json`.
pub(crate) fn fixture_meta(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).unwrap()).unwrap()
}

/// A page of 1280x800 with the test fonts that replays the fixture in
/// `dir`, with JavaScript on or off, and the URL of the fixture. The
/// caller navigates.
pub(crate) fn replay_page(dir: &Path, javascript: bool) -> (Page, Url) {
    replay_page_with(dir, javascript, Arc::new(ReplayFetcher::load(dir).unwrap()))
}

/// [`replay_page`] with `fetcher` (a wrapper of the fixture's
/// [`ReplayFetcher`], for a test that records the requests).
pub(crate) fn replay_page_with(
    dir: &Path,
    javascript: bool,
    fetcher: Arc<dyn Fetcher>,
) -> (Page, Url) {
    let url = Url::parse(fixture_meta(dir)["url"].as_str().unwrap()).unwrap();
    let config = PageConfig {
        fetcher,
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(1280.0, 800.0),
        1.0,
    );
    page.set_javascript(javascript);
    (page, url)
}

/// The value of the expression `source` in the page, as JSON.
pub(crate) fn value(page: &mut Page, source: &str) -> Value {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{source}: {other:?}"),
    }
}

/// The fixture in `dir` with JavaScript on, loaded as `swb --headless --js
/// --test-fonts --now CAPTURED --virtual-time --settle-ms SETTLE_MS` loads
/// it (1280x800): the page clock fixed at the `captured` time of the
/// fixture, as the tools fix it for Chromium, and the event loop run for
/// `settle` of page time after the `load` event.
pub(crate) fn load_at_capture(dir: &Path, settle: Duration) -> Page {
    load_at_capture_with(dir, settle, Arc::new(ReplayFetcher::load(dir).unwrap()))
}

/// [`load_at_capture`] with `fetcher` (see [`replay_page_with`]).
pub(crate) fn load_at_capture_with(
    dir: &Path,
    settle: Duration,
    fetcher: Arc<dyn Fetcher>,
) -> Page {
    let (mut page, url) = replay_page_with(dir, true, fetcher);
    let captured: jiff::Timestamp = fixture_meta(dir)["captured"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    page.set_clock(PageClock::new(
        ClockMode::Virtual,
        Some(captured.as_millisecond() as f64),
    ));
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(60)));
    page.run_event_loop(settle, Duration::from_secs(60));
    page
}

/// The texts of the console messages at `error` level.
pub(crate) fn console_errors(page: &Page) -> Vec<String> {
    page.console_messages()
        .into_iter()
        .filter(|m| m.level == ConsoleLevel::Error)
        .map(|m| m.text)
        .collect()
}

/// The whole numbers in `text`, and `text` with each number replaced by
/// `#`. A `-` is a sign only after a character that is not a letter or
/// digit.
pub(crate) fn numbers(text: &str) -> (String, Vec<i64>) {
    let mut skeleton = String::new();
    let mut found = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let sign = chars[i] == '-'
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
            && (i == 0 || !chars[i - 1].is_alphanumeric());
        if sign || chars[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while chars.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
            let digits: String = chars[start..i].iter().collect();
            found.push(digits.parse().unwrap_or(0));
            skeleton.push('#');
        } else {
            skeleton.push(chars[i]);
            i += 1;
        }
    }
    (skeleton, found)
}

/// True if the values are equal, with the numbers of strings within
/// `tolerance` when `rectangles` is set (the strings are texts such as
/// `"48,-136,176,360 block"`).
pub(crate) fn same(got: &Value, expected: &Value, rectangles: bool, tolerance: i64) -> bool {
    match (got, expected) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| same(x, y, rectangles, tolerance))
        }
        (Value::String(a), Value::String(b)) if rectangles => {
            let ((skeleton_a, na), (skeleton_b, nb)) = (numbers(a), numbers(b));
            skeleton_a == skeleton_b
                && na.len() == nb.len()
                && na.iter().zip(&nb).all(|(x, y)| (x - y).abs() <= tolerance)
        }
        _ => got == expected,
    }
}

/// The keys of the record `got` and of `expected` that differ, as `name:
/// key: swb .. != Chromium ..`. The values of the keys in `rectangle_keys`
/// compare within `tolerance` px.
pub(crate) fn differences(
    name: &str,
    got: &Value,
    expected: &Value,
    rectangle_keys: &[&str],
    tolerance: i64,
) -> Vec<String> {
    let (Some(got), Some(expected)) = (got.as_object(), expected.as_object()) else {
        panic!("{name}: a record is an object");
    };
    let mut keys: Vec<&String> = got.keys().chain(expected.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| match (got.get(*k), expected.get(*k)) {
            (Some(a), Some(b)) => !same(a, b, rectangle_keys.contains(&k.as_str()), tolerance),
            _ => true,
        })
        .map(|k| {
            format!(
                "{name}: {k}: swb {:?} != Chromium {:?}",
                got.get(k),
                expected.get(k)
            )
        })
        .collect()
}

/// A point of the pixel check: name, x and y in the viewport.
pub(crate) type Point = (String, u32, u32);

/// The groups after which a test reads pixels, and for each group the points
/// that it checks, from the text of a points file (`swbtools replay
/// --pixels`). A group that `group_points` names checks the points whose
/// name starts with one of the listed prefixes; every other group checks the
/// points that no prefix claims.
pub(crate) fn pixel_points(text: &str) -> (Vec<String>, Vec<Vec<Point>>) {
    let data: Value = serde_json::from_str(text).unwrap();
    let groups: Vec<String> = data["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g.as_str().unwrap().to_owned())
        .collect();
    let prefixes_of = |group: &str| -> Vec<String> {
        data["group_points"][group]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|p| p.as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let claimed: Vec<String> = groups.iter().flat_map(|g| prefixes_of(g)).collect();
    let points: Vec<Point> = data["points"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, p)| {
            (
                name.clone(),
                p[0].as_u64().unwrap() as u32,
                p[1].as_u64().unwrap() as u32,
            )
        })
        .collect();
    let starts = |name: &str, prefixes: &[String]| prefixes.iter().any(|p| name.starts_with(p));
    let per_group = groups
        .iter()
        .map(|g| {
            let own = prefixes_of(g);
            let selected: Vec<Point> = points
                .iter()
                .filter(|(name, ..)| {
                    if own.is_empty() {
                        !starts(name, &claimed)
                    } else {
                        starts(name, &own)
                    }
                })
                .cloned()
                .collect();
            // A misspelt prefix in `group_points` would check nothing.
            assert!(!selected.is_empty(), "no pixel points for group {g}");
            selected
        })
        .collect();
    (groups, per_group)
}

/// The points whose color in a screenshot of the viewport differs from
/// `expected` (a map of point names to `[r, g, b]`) by more than `tolerance`
/// per channel, as `group: point (x, y): swb [..] != Chromium [..]`.
pub(crate) fn pixel_differences(
    group: &str,
    page: &mut Page,
    points: &[Point],
    expected: &Value,
    tolerance: i32,
) -> Vec<String> {
    let shot = page.screenshot(false).unwrap();
    points
        .iter()
        .filter_map(|(name, x, y)| {
            let want: Vec<i32> = expected[name.as_str()]
                .as_array()
                .unwrap_or_else(|| panic!("{group}: no Chromium color for {name}"))
                .iter()
                .map(|c| c.as_i64().unwrap() as i32)
                .collect();
            let px = shot.pixel(*x, *y).unwrap();
            let got = [px.red(), px.green(), px.blue()].map(i32::from);
            let off = got
                .iter()
                .zip(&want)
                .any(|(g, w)| (g - w).abs() > tolerance);
            off.then(|| format!("{group}: {name} ({x}, {y}): swb {got:?} != Chromium {want:?}"))
        })
        .collect()
}

/// The step groups of a steps file (the file that `swbtools replay --steps`
/// and the tests share): a list of `{"name": ..., "steps": [...]}`.
pub(crate) fn step_groups(text: &str) -> Vec<(String, Vec<Value>)> {
    let groups: Vec<Value> = serde_json::from_str(text).unwrap();
    groups
        .into_iter()
        .map(|g| {
            let name = g["name"].as_str().unwrap().to_owned();
            (name, g["steps"].as_array().unwrap().clone())
        })
        .collect()
}
