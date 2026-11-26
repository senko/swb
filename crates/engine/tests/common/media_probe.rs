//! The probe rows of the media element that play (`tools/probes/
//! media-playback.json` and `media-autoplay.json`, M14 feature 6b): a case
//! runs in the engine in virtual time against a simulated case server
//! (`media_site.rs`), and its answers are compared with Chromium 148's.
//!
//! **The comparison is fuzzy in time.** Chromium plays in real time: the
//! values that depend on the speed of the machine (`currentTime` while
//! playing, the end of a `played` range) differ by a few tens of
//! milliseconds from run to run, and by up to 0.25 s from swb's, which plays
//! in virtual time without delays. The probe rounds the values to 0.25 s
//! where it can (`ct` while playing, `played`); the rows that it cannot round
//! (a paused `ct` after `pause()`) are compared within [`TOLERANCE`]. Only
//! times have the tolerance: the numbers under the keys of [`TIME_KEYS`], and
//! the numbers of a text that are not `rs=`, `ns=` or the value of an error
//! message; `volume`, `readyState`, `networkState`, counts and rates are
//! equal. Two texts are equal when they have the same words and the numbers
//! agree so, and `timeupdate*` (a run of events that the probe merged) is
//! `timeupdate`.

// Test helpers unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{ClockMode, Evaluation, JsonForm, Key, Modifiers, Page, PageClock, Size, Url};
use swb_net::Fetcher;

use super::media_site::{MediaSite, RangeMode};
use super::{json_differences, new_page, repo_root};

/// How far two numbers of a row may be apart: a step of the probe's
/// rounding.
pub(crate) const TOLERANCE: f64 = 0.26;

/// The page time that passes between the load and the first step or query
/// (the media of the page load meanwhile: virtual time waits for them).
pub(crate) const FIRST_QUERY_MS: u64 = 1;

/// The folder of a probe case on the probe's server: `case-N`, N counted
/// from 1 in the probe file (the URLs of the answers contain it).
pub(crate) fn folder_of(probe: &str, name: &str) -> String {
    let text = std::fs::read_to_string(repo_root().join(probe)).unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    let index = file["cases"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c["name"].as_str() == Some(name))
        .unwrap();
    format!("case-{}", index + 1)
}

/// The simulated server of a probe case.
pub(crate) fn site_of(probe: &str, case: &Value) -> MediaSite {
    let folder = folder_of(probe, case["name"].as_str().unwrap());
    let mut site = MediaSite::new(&folder, case["html"].as_str().unwrap());
    for (served, path) in case["files"].as_object().into_iter().flatten() {
        site.add_file(served, path.as_str().unwrap());
    }
    for (served, resource) in case["resources"].as_object().into_iter().flatten() {
        site.add_resource(served, resource);
    }
    site.mode = match &case["ranges"] {
        Value::Bool(false) => RangeMode::Ignore,
        Value::String(s) if s == "ignore" => RangeMode::ClaimButIgnore,
        _ => RangeMode::Honor,
    };
    site
}

/// The page of a case, loaded in virtual time with JavaScript on.
pub(crate) fn load_case(probe: &str, case: &Value) -> (Page, Arc<MediaSite>) {
    let name = case["name"].as_str().unwrap();
    let site = Arc::new(site_of(probe, case));
    let mut page = new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        2,
        Size::new(800.0, 600.0),
    );
    page.set_javascript(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    // The probe's own evaluations give the page activation, and the case
    // script at load runs before any of them: the page gives it by the
    // queries and steps below, not by itself (`navigator.userActivation`).
    page.set_evaluation_activation(true);
    let folder = folder_of(probe, name);
    page.navigate(Url::parse(&MediaSite::page_url(&folder)).unwrap());
    assert!(
        page.wait_until_loaded(Duration::from_secs(30)),
        "{name}: load"
    );
    (page, site)
}

/// Runs the input of the steps of a case (`click` at a point, `hover`, `key`,
/// `wait` in milliseconds of page time, `eval`).
pub(crate) fn run_steps(page: &mut Page, case: &Value) {
    for step in case["steps"].as_array().into_iter().flatten() {
        let (kind, argument) = step.as_object().unwrap().iter().next().unwrap();
        let point = |argument: &Value| {
            let p = argument.as_array().unwrap();
            (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32)
        };
        match kind.as_str() {
            "click" => {
                let (x, y) = point(argument);
                page.click(x, y);
            }
            "hover" => {
                let (x, y) = point(argument);
                page.mouse_move(x, y);
            }
            "key" => {
                let key = Key::Character(argument.as_str().unwrap().to_owned());
                page.key_down(&key, Modifiers::NONE);
                page.key_up(&key, Modifiers::NONE);
            }
            "wait" => {
                let ms = argument.as_u64().unwrap();
                page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(60));
            }
            "eval" => {
                evaluate(page, argument.as_str().unwrap());
            }
            other => panic!("the step {other} is not supported"),
        }
        // The events that the input queued.
        page.run_event_loop(Duration::from_millis(1), Duration::from_secs(60));
    }
}

/// The value of a query, as the probe's row has it (an error is the string
/// `error: ...`).
pub(crate) fn evaluate(page: &mut Page, js: &str) -> Value {
    match page.evaluate_with_timeout(js, JsonForm::Strict, Duration::from_secs(60)) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        Some(Evaluation::Error(text)) => Value::String(format!("error: {text}")),
        None => Value::String("no runtime".to_owned()),
    }
}

/// The keys of the answers whose numbers are times (seconds), so that they
/// differ by [`TOLERANCE`]: every other number (`readyState`, `networkState`,
/// counts, `volume`, rates) must be equal. A number in an array has the key
/// of the array.
const TIME_KEYS: &[&str] = &[
    "ct",
    "dur",
    "sync",
    "sync99",
    "syncNeg",
    "before",
    "setBefore",
    "atMetadata",
    "first",
    "last",
    "errAt",
];

/// Whether the numbers at `path` (`#0.before[1]`) are times: the last key of
/// the path is in [`TIME_KEYS`].
fn is_time_path(path: &str) -> bool {
    let key = path.rsplit('.').next().unwrap_or(path);
    let key = key.split('[').next().unwrap_or(key);
    TIME_KEYS.contains(&key)
}

/// A number of a text and whether it is exact: not a time (`rs=4`, `ns=1`,
/// and the value in `(1.5)` of an error message).
type Number = (f64, bool);

/// The words and numbers of a text: the text with each number replaced by
/// `#`, and the numbers. `timeupdate*` is `timeupdate`.
fn split_numbers(text: &str) -> (String, Vec<Number>) {
    let text = text.replace("timeupdate*", "timeupdate");
    let mut skeleton = String::new();
    let mut numbers = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let number: String = chars[start..i].iter().collect();
            match number.trim_end_matches('.').parse::<f64>() {
                Ok(n) => {
                    let before: String = skeleton.chars().rev().take(3).collect();
                    let exact = before.starts_with("=sr")
                        || before.starts_with("=sn")
                        || before.starts_with('(')
                        || before.starts_with("-(");
                    skeleton.push('#');
                    numbers.push((n, exact));
                }
                Err(_) => skeleton.push_str(&number),
            }
        } else {
            skeleton.push(chars[i]);
            i += 1;
        }
    }
    (skeleton, numbers)
}

/// True if two texts have the same words, the times within [`TOLERANCE`]
/// and the other numbers equal.
pub(crate) fn same_text(swb: &str, chromium: &str) -> bool {
    let (a, x) = split_numbers(swb);
    let (b, y) = split_numbers(chromium);
    a == b
        && x.len() == y.len()
        && x.iter().zip(&y).all(|(p, q)| {
            let limit = if p.1 || q.1 { 0.0 } else { TOLERANCE };
            (p.0 - q.0).abs() <= limit
        })
}

/// The differences of two answers (`path`, `swb ..., Chromium ...`): the
/// shape of the values and the texts are compared as above; numbers within
/// [`TOLERANCE`].
pub(crate) fn fuzzy_differences(
    path: &str,
    swb: &Value,
    chromium: &Value,
    out: &mut Vec<(String, String)>,
) {
    match (swb, chromium) {
        (Value::String(a), Value::String(b)) => {
            if !same_text(a, b) {
                out.push((path.to_owned(), format!("swb {swb}, Chromium {chromium}")));
            }
        }
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            let limit = if is_time_path(path) { TOLERANCE } else { 0.0 };
            if (a - b).abs() > limit {
                out.push((path.to_owned(), format!("swb {swb}, Chromium {chromium}")));
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                fuzzy_differences(&format!("{path}[{index}]"), x, y, out);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for key in b.keys().chain(a.keys().filter(|k| !b.contains_key(*k))) {
                let path = format!("{path}.{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => fuzzy_differences(&path, x, y, out),
                    (x, y) => {
                        let shown = |v: Option<&Value>| {
                            v.map_or_else(|| "(missing)".to_owned(), ToString::to_string)
                        };
                        out.push((path, format!("swb {}, Chromium {}", shown(x), shown(y))));
                    }
                }
            }
        }
        _ => json_differences(path, swb, chromium, out),
    }
}

/// The differences of one case: the steps run, then each `js` query with
/// its row of Chromium's answer. The page loads and its media load
/// (`FIRST_QUERY_MS` of page time), and the scripts of the case run, before
/// the probe's first evaluation, which waits for the fonts and gives the page
/// a user activation: the case's script at load sees none.
pub(crate) fn run_case(
    probe: &str,
    case: &'static Value,
    answer: &'static Value,
) -> Vec<(String, String)> {
    let (mut page, _site) = load_case(probe, case);
    page.run_event_loop(
        Duration::from_millis(FIRST_QUERY_MS),
        Duration::from_secs(60),
    );
    page.note_user_activation();
    run_steps(&mut page, case);
    let mut out = Vec::new();
    for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
        let Some(js) = query["js"].as_str() else {
            continue;
        };
        let want = &answer["results"][index]["value"];
        let got = evaluate(&mut page, js);
        fuzzy_differences(&format!("#{index}"), &got, want, &mut out);
    }
    out
}

/// The answers of a probe file with the `NaN` of a number made `null`.
pub(crate) fn answers(text: &'static str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<Vec<(usize, &'static str)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    let mut cache = cache.lock().unwrap();
    let key = text.as_ptr() as usize;
    if let Some((_, done)) = cache.iter().find(|(k, _)| *k == key) {
        return done;
    }
    let done: &'static str = Box::leak(super::media_site::chromium_json(text).into_boxed_str());
    cache.push((key, done));
    done
}
