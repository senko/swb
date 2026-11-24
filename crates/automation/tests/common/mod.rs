//! Helpers of the integration tests on the local JavaScript fixtures
//! (`fixtures/js/<name>`, git-ignored, ADR 0028 section 17). Set
//! `SWB_JS_FIXTURES` to the directory `fixtures/js` of another checkout;
//! a test skips when its fixture is missing.

// Each test file uses some of the helpers.
#![allow(dead_code)]

pub(crate) mod animation_values;
pub(crate) mod console;
pub(crate) mod departures;
pub(crate) mod records;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use swb_engine::{
    ClockMode, ConsoleLevel, Evaluation, FontContext, JsonForm, Page, PageClock, PageConfig, Size,
    Url,
};
use swb_net::{CookieJar, Fetcher, Headers, NetError, ReplayFetcher, Request, Response};

/// The largest difference of a color channel between swb's pixel and
/// Chromium's in the pixel points of the page tests. The points are in
/// flat areas (also on borders, icons, gradients, and anti-aliased wordmark
/// edges within the tolerance); the tolerance covers the rounding of
/// blends, not the anti-aliasing of text.
pub(crate) const PIXEL_TOLERANCE: i32 = 12;

/// The response of `status` with `body` for `request`, as `text/html`.
pub(crate) fn html_response(request: &Request, status: u16, body: Vec<u8>) -> Response {
    let headers: Headers = [("content-type", "text/html")].into_iter().collect();
    Response {
        url: request.url.clone(),
        status,
        headers,
        body,
        redirected: false,
    }
}

/// A fetcher that serves one page at `url`; other URLs get a 404.
pub(crate) struct OnePage {
    pub(crate) url: &'static str,
    pub(crate) html: String,
}

impl Fetcher for OnePage {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        Ok(if request.url.as_str() == self.url {
            html_response(request, 200, self.html.as_bytes().to_vec())
        } else {
            html_response(request, 404, b"not found".to_vec())
        })
    }
}

/// A fetcher that serves one page at `url`, the `data:` URLs with a base64
/// payload (`data:image/gif;base64,...`), and a 404 for the rest.
pub(crate) struct DataUrls(pub(crate) OnePage);

impl Fetcher for DataUrls {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let data = request
            .url
            .as_str()
            .strip_prefix("data:")
            .and_then(|rest| rest.split_once(";base64,"));
        let Some((mime, payload)) = data else {
            return self.0.fetch(request);
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

/// A page with JavaScript on and test fonts, of `size`, that loads `url`
/// from `fetcher` (30 s at most).
pub(crate) fn open_script_page(fetcher: Arc<dyn Fetcher>, url: Url, size: Size) -> Page {
    let config = PageConfig {
        fetcher,
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(config, FontContext::for_tests(), size, 1.0);
    page.set_javascript(true);
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// The expected value that skips a query in [`run_probe_file_with`]: the
/// query does not run (a test of a feature that comes later).
pub(crate) const SKIP_QUERY: &str = "\u{0}skip";

/// Runs the cases of the probe file `tools/probes/{name}.json` and compares
/// the values of their `js` queries with the file
/// `tests/data/{name}.json` (Chromium's values). `open` makes the page of a
/// case; `want` gives the expected value of a query (case, index, Chromium's
/// value), where a test applies its departures. Returns the failures.
pub(crate) fn run_probe_file(
    name: &str,
    open: impl Fn(&Value) -> Page,
    want: impl Fn(&str, usize, &Value) -> Value,
) -> Vec<String> {
    run_probe_file_with(name, open, want, |want, got| want == got)
}

/// [`run_probe_file`] with `same`, the comparison of an expected and an
/// actual value (a test that accepts small numeric differences).
pub(crate) fn run_probe_file_with(
    name: &str,
    open: impl Fn(&Value) -> Page,
    want: impl Fn(&str, usize, &Value) -> Value,
    same: impl Fn(&Value, &Value) -> bool,
) -> Vec<String> {
    let read = |path: PathBuf| -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let probe = read(repo_root().join(format!("tools/probes/{name}.json")));
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/data/{name}.json"));
    let expected = read(data);
    let cases = probe["cases"].as_array().unwrap();
    assert_eq!(cases.len(), expected.as_object().unwrap().len());
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut page = open(case);
        let steps = case["steps"].as_array().cloned().unwrap_or_default();
        if let Err(error) = swb_automation::run_steps(&mut page, &steps) {
            failures.push(format!("{name}: {error}"));
            continue;
        }
        for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
            let Some(source) = query["js"].as_str() else {
                continue;
            };
            let want = want(name, index, &expected[name][index]);
            if want.as_str() == Some(SKIP_QUERY) {
                continue;
            }
            let got = value(&mut page, source);
            if !same(&want, &got) {
                failures.push(format!(
                    "{name} #{index}: {source}
  expected: {want}
  actual:   {got}"
                ));
            }
        }
    }
    failures
}

/// A fetcher that records the URLs of its requests, in order, and answers
/// from the fixture. It also records the URLs that the fixture lacks.
pub(crate) struct Recording {
    inner: ReplayFetcher,
    urls: Mutex<Vec<String>>,
    missing: Mutex<Vec<String>>,
}

impl Recording {
    /// A recording fetcher around `inner`.
    pub(crate) fn new(inner: ReplayFetcher) -> Self {
        Recording {
            inner,
            urls: Mutex::new(Vec::new()),
            missing: Mutex::new(Vec::new()),
        }
    }

    /// The URLs requested so far, in order.
    pub(crate) fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }

    /// The number of requests so far that the fixture lacked (each repeat
    /// counts); [`Recording::missing_since`] takes it as `from`.
    pub(crate) fn missing_count(&self) -> usize {
        self.missing.lock().unwrap().len()
    }

    /// The URLs of the requests that the fixture lacked after the first
    /// `from` of them, sorted, each once (what `swbtools replay
    /// --missing-out` lists for a group).
    pub(crate) fn missing_since(&self, from: usize) -> Vec<String> {
        let mut urls = self.missing.lock().unwrap()[from..].to_vec();
        urls.sort();
        urls.dedup();
        urls
    }

    /// The URLs requested so far that the fixture lacks, sorted, each once
    /// (the set that `swbtools replay` logs as "not in fixture").
    pub(crate) fn missing(&self) -> Vec<String> {
        let mut urls = self.missing.lock().unwrap().clone();
        urls.sort();
        urls.dedup();
        urls
    }
}

impl Fetcher for Recording {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        self.urls.lock().unwrap().push(request.url.to_string());
        let result = self.inner.fetch(request);
        if matches!(result, Err(NetError::NotInFixture(_))) {
            self.missing.lock().unwrap().push(request.url.to_string());
        }
        result
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
fn fixture_meta(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).unwrap()).unwrap()
}

/// A page of 1280x800 with the test fonts that replays the fixture in
/// `dir`, with JavaScript on or off and animations off (as Chromium's
/// references), and the URL of the fixture. The caller navigates.
pub(crate) fn replay_page(dir: &Path, javascript: bool) -> (Page, Url) {
    replay_page_with(
        dir,
        javascript,
        Arc::new(ReplayFetcher::load(dir).unwrap()),
        false,
    )
}

/// [`replay_page`] with `fetcher` (a wrapper of the fixture's
/// [`ReplayFetcher`], for a test that records the requests), and with the
/// page's animations and transitions on if `animations` (the exit test of
/// videojs.org; the references of the other tests have them off).
pub(crate) fn replay_page_with(
    dir: &Path,
    javascript: bool,
    fetcher: Arc<dyn Fetcher>,
    animations: bool,
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
    // Chromium's references are taken with animations off (ADR 0044
    // section 10, docs/testing.md "Animations in the references").
    page.set_animations(animations);
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
    load_at_capture_animated(dir, settle, fetcher, false)
}

/// [`load_at_capture_with`] with the page's animations and transitions on
/// if `animations`.
pub(crate) fn load_at_capture_animated(
    dir: &Path,
    settle: Duration,
    fetcher: Arc<dyn Fetcher>,
    animations: bool,
) -> Page {
    let (mut page, url) = replay_page_with(dir, true, fetcher, animations);
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

/// `record` without the keys in `skip`.
pub(crate) fn without(record: &Value, skip: &[&str]) -> Value {
    let mut record = record.clone();
    for key in skip {
        record.as_object_mut().unwrap().remove(*key);
    }
    record
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
    pixel_mismatches(group, page, points, expected, tolerance)
        .into_iter()
        .map(|(_, message)| message)
        .collect()
}

/// [`pixel_differences`] with the name of each point that differs next to
/// its message.
pub(crate) fn pixel_mismatches(
    group: &str,
    page: &mut Page,
    points: &[Point],
    expected: &Value,
    tolerance: i32,
) -> Vec<(String, String)> {
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
            off.then(|| {
                let message =
                    format!("{group}: {name} ({x}, {y}): swb {got:?} != Chromium {want:?}");
                (name.clone(), message)
            })
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
