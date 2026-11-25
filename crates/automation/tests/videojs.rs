//! M13 feature 6, the exit test of the videojs.org target: the front page of
//! `https://videojs.org/` without playback, against Chromium's replay of the
//! same fixture, at 1280 x 800 with the page clock fixed at the `captured`
//! time. The JS-on tests use the local fixture `fixtures/js/videojs`
//! (git-ignored; set `SWB_JS_FIXTURES` to the directory `fixtures/js` of
//! another checkout; they skip when it is missing). The JS-off test uses the
//! committed `fixtures/pages/videojs` and always runs. In all of them the
//! page's CSS animations and transitions are ON: they are the subject.
//!
//! What the test checks, one test per sequence, by the items of "What works
//! means" of M13 in `docs/roadmap.md`:
//!
//! 1. **The first view** (item 1). JS on ([`values_match_chromium`], group
//!    `first view`): after the load and 1.5 s of page time the hero reveal has
//!    ended in both browsers (it ends 1965 ms after the script in Chromium, 2000
//!    ms in swb: the page's script sets `--hero-delay` from
//!    `performance.now()`). The record `window.__v()` (`data/videojs.js`) has the
//!    rectangles of the header (the logo, the navigation, the search button,
//!    the appearance button), the hero (the title with the rectangles of its
//!    lines, the sponsor line) and the player (box, poster, controls, the
//!    sliders) within [`RECT_TOLERANCE`], the texts, the state of the page and
//!    the animations that exist; and 46 pixel points chosen from Chromium's
//!    three screenshots alone (flat in a 7 x 7 square in all three runs),
//!    also inside the player. JS off ([`js_off_first_view_matches_chromium`]):
//!    `fixtures/pages/videojs` with animations on, 1.5 s after the load, the
//!    screenshot against `reference/screenshot.png` (every pixel; a channel
//!    may differ by [`PIXEL_TOLERANCE`]; the number of pixels that differ must
//!    not rise above [`JS_OFF_DIFFERENT`]) and the boxes against
//!    `reference/boxes.json` (geometry at least the score of
//!    `fixtures/scores.json`). The references and `fixture_scores.rs` run with
//!    animations off; this test shows that running animations do not change
//!    the first view. The pixel count cannot show an animation that recolors
//!    pixels that already differ (the title's), so
//!    [`js_off_animations_leave_the_first_view_alone`] checks the values: the
//!    three reveal elements have the values of a page with animations off (the
//!    reveal has no animation without the script: Chromium's `getAnimations()`
//!    of the fixture lists only the two bars, paused, and the marquee), and the
//!    two bars are at width 0 (paused at their start until `data-animate`).
//! 2. **The CSS animations at sampled times** (item 2; the `values`
//!    sequence). The page's animations that run in Chromium on this fixture are
//!    the hero reveal (three elements: the title's color, the outlined
//!    words' stroke color, the video box's opacity and transform), the two
//!    growing bars (`grow-bar`, 3.25 s and 1.01 s: they start when the
//!    `IntersectionObserver` sees them) and the marquee (30 s, endless).
//!    The spinner of the player and the shimmer of the search dialog do not
//!    run here: the spinner needs a loading player and the shimmer an open
//!    search dialog (`DocSearchModal` is not in the fixture); the probe rows
//!    `spinner` and `shimmer` of `css-animations.json` cover them. Each
//!    animation is paused and set to its delay plus a time (`window.__seek`,
//!    docs/videojs-diagnosis.md section 2; relative to the delay, because the
//!    delay of the reveal comes from `performance.now()`): the reveal at -100
//!    ms (the fill before the delay), 0, 550, 1050 ms of its 1100 ms and at its
//!    end; the bars at 0, 0.5 and 0.95 of their durations and at their ends;
//!    the marquee at 0, 15000, 29900 and 30100 ms (both sides of the end of an
//!    iteration). At each time: the computed values of the animated properties
//!    of every animation (`values`, with the tolerances of ADR 0044 section 12:
//!    `common::animation_values`), the rectangles of the animated elements
//!    (`targets`) and the pixel points of Chromium's screenshot of the same
//!    paused state.
//! 3. **The transitions at sampled times, and the event order** (item 3).
//!    **No link of the page has a transition.** All 26 `a` elements have
//!    `transition: 0s all` (Chromium, `getComputedStyle`), so the groups for
//!    the navigation links (`hover blog`, `hover docs`, `hover github`; in the
//!    `events` sequence `hover nav links without transitions`) and the Tab
//!    presses at links (`tab 1`, `tab 2`, `tab 4`) only check that swb starts
//!    none. The elements of the first view whose `:hover` or `:focus-visible`
//!    rules change a property of their `transition-property`, from Chromium's
//!    style sheets (`CSSStyleRule`s, the selectors without the pseudo-class
//!    matched against each element): the search button (`border-color`,
//!    `background-color`; hover, focus), the controls of the player (`color`,
//!    `background-color`, `scale`, `outline-offset`) and the sliders' thumbs
//!    (hover, focus), the skin's container and the time toggle (focus rings).
//!    Hovers: the search button and the play, settings and time slider
//!    controls (the other controls follow the same rules); focus: the search
//!    button (the third Tab) and the container (groups `focus container`, in
//!    the `events` sequence `tab to the player controls`). The play button's
//!    and the settings button's focus show a tooltip popup that swb closes
//!    differently (backlog), the mute button's volume popup opens after a
//!    delay that varies between runs, the seek thumb and the time toggle are
//!    focusable only with a loaded stream, and swb has no picture-in-picture
//!    and fullscreen buttons: none of them has a focus group.
//!    Values (the `values` sequence): each action starts transitions (the
//!    hovers over the navigation links, the search button and the controls of
//!    the player, four presses of Tab and five more to the player's container,
//!    the appearance menu with its popup and
//!    the theme change, the spring at the end of the page); with no pause the
//!    next steps pause every transition and set it to 0, 0.5 and 0.95 of its
//!    duration, and the next action begins by finishing them, so that every
//!    action starts from the end state in both browsers. Events (the `events`
//!    sequence, a separate load): the step groups of `data/videojs-steps.json`
//!    with a pause of 300 ms after each step and no seeking; a capture
//!    listener on `document` logs every `transitionrun`, `transitionstart`,
//!    `transitionend`, `transitioncancel`, `animationstart`, `animationend` and
//!    `animationcancel` with its type, property or name, a description of the
//!    target and `elapsedTime` (a cancel has none); the record takes the log
//!    since the last record. The log is compared in order. Where Chromium's
//!    own three runs differ (`data/videojs-events-orders.json`: the order of
//!    the events of different elements that are due at the same time, or a
//!    transition of a radio button of the appearance menu that starts or not
//!    in a frame whose timing depends on the run) the test compares a set or
//!    a range. The first group's events are not compared: the setup script
//!    runs while the reveal ends, and which run sees `animationend` depends on
//!    the timing. `animationiteration` of the marquee is not logged (the group
//!    in which it falls depends on the speed of the run).
//!    The probe rows of this feature run in
//!    [`probe_cases_of_feature_6`] (`tools/probes/css-videojs-6.json`: the
//!    `starting-style` utilities, `finish()` before a style change, a
//!    transition after a transition that completed, the used color scheme
//!    starting a `color` transition, a script's `blur()`) and
//!    [`probe_cases_of_wheel_order`] (`tools/probes/wheel-order.json`: the
//!    scroll position that a `wheel` listener sees).
//! 4. **The console** (item 4), per sequence, as `osm_zagreb.rs`.
//!
//! Items 5 and 6 (the GUI budgets and the `load` event) are measurements, not
//! tests (docs/performance.md, "M13 exit").
//!
//! The data come from `tools/videojs_records.py` (`steps`, `records`,
//! `points`; the module comment of the script has the commands): Chromium
//! 148.0.7778.96 (headless shell, Playwright) replayed the fixture three times
//! per sequence with `swbtools replay --animations on`; the records of the
//! three runs are equal (the exceptions are the events above). The test
//! collects the problems of all checks and asserts once at the end. The
//! differences that the test accepts are in `data/videojs-departures.json`,
//! each with its cause: a backlog item of `docs/roadmap.md`, a listed
//! departure of ADR 0044, or a media feature (M14, M15). A departure that no
//! longer applies fails the test.
//!
//! The recordings have Media Source Extensions, which swb has from M15, so
//! these sequences load with media off (`Page::set_media`). One test loads
//! the page with media on against Chromium without `MediaSource`
//! ([`without_media_source_the_player_fails_and_shows_its_error_dialog`]).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::animation_values::{oklab_as_rgb, same as value_same};
use common::console;
use common::departures::Departures;
use common::records::{Difference, paced, pixel_problems};
use common::{
    PIXEL_TOLERANCE, fixture, load_at_capture_animated, load_at_capture_without_media,
    on_page_thread, replay_page_with, repo_root, step_groups, value,
};
use serde_json::{Value, json};
use swb_automation::run_steps;
use swb_engine::{ClockMode, Page, PageClock, Url};
use swb_net::{CookieJar, Fetcher, NetError, ReplayFetcher, Request, Response, StreamResponse};

/// The largest difference, in CSS px, of a coordinate or a size in the
/// rectangles of a record, and of the widths of the bars. Measured over the
/// 342 rectangles of the first view and of the reveal that both browsers have:
/// 336 are equal, 6 differ by 0.01 (the rounding of the record; the layout
/// unit is 1/64 px); the others that differ do so by 72 px or more (the video
/// element, the time slider and the buttons that M14 and M15 change). So 0.1
/// px passes the rounding and nothing else.
const RECT_TOLERANCE: f64 = 0.1;

/// The most pixels of the JS-off first view that may differ from Chromium's
/// by more than [`PIXEL_TOLERANCE`] in a channel: measured 30,099 of 1,024,000,
/// with animations off and on alike. By region: 19,975 in the poster image of
/// the player (swb's downscale of the 2400 x 1350 placeholder photo to
/// 1180 x 570 differs from Chromium's by 13 to 19 levels in 4 % of the pixels),
/// 8,973 in the two title lines (glyph and stroke edges, and the poster behind
/// them), 813 on the glyph edges of the header (logo, search button, links,
/// y 40 to 80), 257 on the sponsor line (y 720 to 760) and 81 in the controls.
const JS_OFF_DIFFERENT: usize = 30_099;

/// The height of the viewport.
const VIEWPORT_HEIGHT: f64 = 800.0;

/// The most boxes below the first view that may differ from the reference
/// (the references have animations off; see `js_off_first_view_matches_chromium`).
const JS_OFF_BELOW_DIFFERENT: usize = 35;

/// The pause after each step of the `events` sequence, in ms of page time
/// (`swbtools replay --pause-ms`). The `values` sequence has none.
const EVENTS_PAUSE_MS: u64 = 300;

/// The setup script: the record function and the seeking functions.
const SETUP: &str = include_str!("data/videojs.js");

/// The departures file of all sequences (each entry names its `sequence`).
const DEPARTURES: &str = include_str!("data/videojs-departures.json");

/// The data files of one sequence of step groups.
struct Sequence {
    name: &'static str,
    steps: &'static str,
    chromium: &'static str,
    console: &'static str,
    /// The records whose events differ between Chromium's runs (a file for
    /// the `events` sequence).
    orders: Option<&'static str>,
    /// The points file and the colors (the `values` sequence).
    pixels: Option<(&'static str, &'static str)>,
    pause_ms: u64,
}

const VALUES: Sequence = Sequence {
    name: "values",
    steps: include_str!("data/videojs-values-steps.json"),
    chromium: include_str!("data/videojs-values-chromium.json"),
    console: include_str!("data/videojs-values-console.json"),
    orders: None,
    pixels: Some((
        include_str!("data/videojs-pixel-points.json"),
        include_str!("data/videojs-pixels.json"),
    )),
    pause_ms: 0,
};

const EVENTS: Sequence = Sequence {
    name: "events",
    steps: include_str!("data/videojs-events-steps.json"),
    chromium: include_str!("data/videojs-events-chromium.json"),
    console: include_str!("data/videojs-events-console.json"),
    orders: Some(include_str!("data/videojs-events-orders.json")),
    pixels: None,
    pause_ms: EVENTS_PAUSE_MS,
};

/// The record `window.__v()` of the page.
fn record(page: &mut Page) -> Value {
    let text = value(page, "window.__v()");
    serde_json::from_str(text.as_str().unwrap()).unwrap()
}

fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn text(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

/// The numbers of a list of numbers.
fn numbers(v: &Value) -> Vec<f64> {
    array(v).iter().filter_map(Value::as_f64).collect()
}

fn within(a: &[f64], b: &[f64], tolerance: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tolerance)
}

/// The items of a list of entries as a map from a key to the entry; a key
/// that repeats gets `#N`.
fn keyed(list: &[Value], key: impl Fn(&Value) -> String) -> BTreeMap<String, &Value> {
    let mut found = BTreeMap::new();
    for entry in list {
        let base = key(entry);
        let mut name = base.clone();
        let mut n = 1;
        while found.contains_key(&name) {
            n += 1;
            name = format!("{base} #{n}");
        }
        found.insert(name, entry);
    }
    found
}

/// The differences between two keyed lists: an item that only one side has,
/// and an item whose values `same` calls different. `field` makes the name
/// of a difference from a key.
fn keyed_differences(
    group: &str,
    field: impl Fn(&str) -> String,
    got: &BTreeMap<String, &Value>,
    want: &BTreeMap<String, &Value>,
    same: impl Fn(&Value, &Value) -> bool,
) -> Vec<Difference> {
    let keys: BTreeSet<&String> = got.keys().chain(want.keys()).collect();
    let mut found = Vec::new();
    for key in keys {
        let name = field(key);
        let message = match (got.get(key), want.get(key)) {
            (Some(g), Some(w)) if same(g, w) => continue,
            (Some(g), Some(w)) => format!("swb {g} != Chromium {w}"),
            (Some(g), None) => format!("only in swb {g}"),
            (None, w) => format!("only in Chromium {}", w.unwrap()),
        };
        found.push((name.clone(), format!("{group}: {name}: {message}")));
    }
    found
}

/// The rectangles of a record's area as `[selector, index] -> [x, y, w, h]`.
fn rect_differences(group: &str, got: &Value, want: &Value) -> Vec<Difference> {
    let areas: BTreeSet<&String> = got["rects"]
        .as_object()
        .into_iter()
        .chain(want["rects"].as_object())
        .flat_map(|areas| areas.keys())
        .collect();
    let mut found = Vec::new();
    for area in areas {
        let entries =
            |record: &'_ Value| -> Vec<Value> { array(&record["rects"][area.as_str()]).to_vec() };
        let (a, b) = (entries(got), entries(want));
        let key = |e: &Value| format!("{} #{}", text(&e[0]), e[1]);
        let (ka, kb) = (keyed(&a, key), keyed(&b, key));
        let rect = |e: &Value| numbers(&json!(array(e)[2..]));
        found.extend(keyed_differences(
            group,
            |k| format!("rects.{area}[{k}]"),
            &ka,
            &kb,
            |g, w| within(&rect(g), &rect(w), RECT_TOLERANCE),
        ));
    }
    found
}

/// The texts of a record: lists of strings compared as sets of strings, the
/// title (strings and the rectangles of its lines) in order.
fn text_differences(group: &str, got: &Value, want: &Value) -> Vec<Difference> {
    let mut found = Vec::new();
    let names: BTreeSet<&String> = got["texts"]
        .as_object()
        .into_iter()
        .chain(want["texts"].as_object())
        .flat_map(|areas| areas.keys())
        .collect();
    for name in names {
        let (a, b) = (&got["texts"][name.as_str()], &want["texts"][name.as_str()]);
        if name == "title" {
            let same = array(a).len() == array(b).len()
                && array(a).iter().zip(array(b)).all(|(x, y)| match (x, y) {
                    (Value::Array(_), Value::Array(_)) => {
                        within(&numbers(x), &numbers(y), RECT_TOLERANCE)
                    }
                    _ => x == y,
                });
            if !same {
                let field = "texts.title".to_owned();
                found.push((
                    field.clone(),
                    format!("{group}: {field}: swb {a} != Chromium {b}"),
                ));
            }
            continue;
        }
        let strings = |v: &Value| array(v).iter().map(text).collect::<Vec<_>>();
        let (sa, sb) = (strings(a), strings(b));
        for s in sa.iter().filter(|s| !sb.contains(s)) {
            let field = format!("texts.{name}[{s}]");
            found.push((field.clone(), format!("{group}: {field}: only in swb")));
        }
        for s in sb.iter().filter(|s| !sa.contains(s)) {
            let field = format!("texts.{name}[{s}]");
            found.push((field.clone(), format!("{group}: {field}: only in Chromium")));
        }
    }
    found
}

/// The key of an event: type, property or name, target, pseudo-element.
fn event_key(event: &Value) -> String {
    array(event)[..4]
        .iter()
        .map(text)
        .collect::<Vec<_>>()
        .join("|")
}

/// How the events of a record compare (`videojs-events-orders.json`).
enum Events {
    /// The same events in the same order.
    Exact,
    /// The same events in any order.
    Set,
    /// For each event (its JSON text), the smallest and the largest number of
    /// times in a run of Chromium.
    Range(BTreeMap<String, (u64, u64)>),
    /// Not compared.
    Skip,
}

/// The differences between the events of swb and of Chromium.
fn event_differences(group: &str, got: &Value, want: &Value, how: &Events) -> Vec<Difference> {
    let (a, b) = (array(&got["events"]), array(&want["events"]));
    let counts = |list: &[Value]| {
        let mut found: BTreeMap<String, u64> = BTreeMap::new();
        for e in list {
            *found.entry(e.to_string()).or_default() += 1;
        }
        found
    };
    let mut found = Vec::new();
    let mut report = |field: String, message: String| {
        found.push((field.clone(), format!("{group}: {field}: {message}")));
    };
    match how {
        Events::Skip => {}
        Events::Range(ranges) => {
            let have = counts(a);
            let all: BTreeSet<&String> = have.keys().chain(ranges.keys()).collect();
            for key in all {
                let n = have.get(key).copied().unwrap_or(0);
                let (low, high) = ranges.get(key).copied().unwrap_or((0, 0));
                if !(low..=high).contains(&n) {
                    let event: Value = serde_json::from_str(key).unwrap();
                    report(
                        format!("events[{}]", event_key(&event)),
                        format!("swb {n} times, Chromium {low} to {high}: {key}"),
                    );
                }
            }
        }
        Events::Exact | Events::Set => {
            let (ca, cb) = (counts(a), counts(b));
            let all: BTreeSet<&String> = ca.keys().chain(cb.keys()).collect();
            for key in all {
                let (n, m) = (
                    ca.get(key).copied().unwrap_or(0),
                    cb.get(key).copied().unwrap_or(0),
                );
                if n != m {
                    let event: Value = serde_json::from_str(key).unwrap();
                    report(
                        format!("events[{}]", event_key(&event)),
                        format!("swb {n} times, Chromium {m}: {key}"),
                    );
                }
            }
            if matches!(how, Events::Exact) && ca == cb && a != b {
                let at = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(0);
                report(
                    "events.order".to_owned(),
                    format!(
                        "first difference at {at}: swb {} != Chromium {}",
                        a[at], b[at]
                    ),
                );
            }
        }
    }
    found
}

/// The expected value of an animated property: Chromium's `oklab()` results
/// as swb stores them (ADR 0044 section 4).
fn value_text(v: &Value) -> Value {
    Value::String(oklab_as_rgb(&text(v)))
}

/// The differences of a group's record from Chromium's.
fn record_differences(group: &str, got: &Value, want: &Value, events: &Events) -> Vec<Difference> {
    let mut found = Vec::new();
    for key in [
        "scrollY",
        "active",
        "htmlClass",
        "expanded",
        "open",
        "transformed",
        "thumbnail",
        "animate",
        "heroReady",
    ] {
        if got[key] != want[key] {
            found.push((
                key.to_owned(),
                format!("{group}: {key}: swb {} != Chromium {}", got[key], want[key]),
            ));
        }
    }
    if !within(
        &numbers(&got["bars"]),
        &numbers(&want["bars"]),
        RECT_TOLERANCE,
    ) {
        found.push((
            "bars".to_owned(),
            format!(
                "{group}: bars: swb {} != Chromium {}",
                got["bars"], want["bars"]
            ),
        ));
    }
    let list = |record: &'_ Value, key: &str| array(&record[key]).to_vec();
    let animation_key = |e: &Value| array(e)[..4].iter().map(text).collect::<Vec<_>>().join("|");
    let (a, b) = (list(got, "animations"), list(want, "animations"));
    found.extend(keyed_differences(
        group,
        |k| format!("animations[{k}]"),
        &keyed(&a, animation_key),
        &keyed(&b, animation_key),
        |g, w| {
            let at = |e: &Value| (e[4].clone(), numbers(&json!([e[5].clone()])));
            let (x, y) = (at(g), at(w));
            x.0 == y.0 && within(&x.1, &y.1, 0.11)
        },
    ));
    let value_key = |e: &Value| format!("{}|{}|{}", text(&e[0]), text(&e[1]), text(&e[2]));
    let (a, b) = (list(got, "values"), list(want, "values"));
    found.extend(keyed_differences(
        group,
        |k| format!("values[{k}]"),
        &keyed(&a, value_key),
        &keyed(&b, value_key),
        |g, w| value_same(&value_text(&w[3]), &Value::String(text(&g[3]))),
    ));
    let target_key = |e: &Value| text(&e[0]);
    let (a, b) = (list(got, "targets"), list(want, "targets"));
    let rect = |e: &Value| numbers(&json!(array(e)[1..]));
    found.extend(keyed_differences(
        group,
        |k| format!("targets[{k}]"),
        &keyed(&a, target_key),
        &keyed(&b, target_key),
        |g, w| within(&rect(g), &rect(w), RECT_TOLERANCE),
    ));
    found.extend(event_differences(group, got, want, events));
    found.extend(rect_differences(group, got, want));
    found.extend(text_differences(group, got, want));
    found
}

/// The way the events of record `number` compare.
fn events_mode(number: usize, group: &str, orders: Option<&Value>) -> Events {
    if group == "first view" {
        return Events::Skip;
    }
    let Some(orders) = orders else {
        return Events::Skip;
    };
    if array(&orders["set"])
        .iter()
        .any(|n| n.as_u64() == Some(number as u64))
    {
        return Events::Set;
    }
    match orders["range"].get(number.to_string()) {
        Some(list) => Events::Range(
            array(list)
                .iter()
                .map(|e| {
                    (
                        e[0].to_string(),
                        (e[1].as_u64().unwrap(), e[2].as_u64().unwrap()),
                    )
                })
                .collect(),
        ),
        None => Events::Exact,
    }
}

/// Runs a sequence on a fresh load and returns its problems.
fn check(sequence: &Sequence, dir: &Path) -> Vec<String> {
    let started = Instant::now();
    let expected: Vec<Value> = serde_json::from_str(sequence.chromium).unwrap();
    let groups = step_groups(sequence.steps);
    assert_eq!(expected.len(), groups.len(), "a record for each group");
    let orders: Option<Value> = sequence.orders.map(|t| serde_json::from_str(t).unwrap());
    let pixels = sequence.pixels.map(|(points, colors)| {
        let (names, per_group) = common::pixel_points(points);
        let colors: Value = serde_json::from_str(colors).unwrap();
        (names, per_group, colors)
    });
    let mut departures = Departures::load_for(DEPARTURES, Some(sequence.name));

    let fetcher = Arc::new(ReplayFetcher::load(dir).unwrap());
    // Media loads are off: the recording has Media Source Extensions, swb
    // none until M15 (see `load_at_capture_without_media`).
    let mut page = load_at_capture_without_media(dir, Duration::from_millis(1500), fetcher, true);
    assert!(page.has_script_runtime());
    page.evaluate(SETUP, swb_engine::JsonForm::Strict);

    let mut problems = Vec::new();
    let mut sampled = 0;
    for (number, (label, steps)) in groups.iter().enumerate() {
        run_steps(&mut page, &paced(steps, sequence.pause_ms))
            .unwrap_or_else(|e| panic!("group {label}: {e}"));
        let events = events_mode(number, label, orders.as_ref());
        let got = record(&mut page);
        let differences = record_differences(label, &got, &expected[number], &events);
        problems.extend(
            differences
                .into_iter()
                .filter(|(field, _)| !departures.accepts_field(label, field))
                .map(|(_, message)| message),
        );
        if let Some((names, per_group, colors)) = &pixels
            && let Some(at) = names.iter().position(|g| g == label)
        {
            problems.extend(pixel_problems(
                label,
                &mut page,
                &per_group[at],
                &colors[label.as_str()],
                &mut departures,
            ));
            sampled += 1;
        }
    }
    if let Some((names, ..)) = &pixels {
        assert_eq!(
            sampled,
            names.len(),
            "a group of the pixel check is missing"
        );
    }
    for (text, message) in console::differences(&page, sequence.console, &[], &[]) {
        if !departures.accepts_field("console", &format!("console: {text}")) {
            problems.push(message);
        }
    }
    problems.extend(departures.unused());
    eprintln!(
        "videojs {}: {:.1} s",
        sequence.name,
        started.elapsed().as_secs_f64()
    );
    problems
}

/// Runs a sequence in a page thread and asserts that it has no problem;
/// skips if the fixture is missing.
fn run(sequence: &'static Sequence) {
    let Some(dir) = fixture("videojs") else {
        eprintln!("skipped: fixtures/js/videojs is missing");
        return;
    };
    on_page_thread(move || {
        let problems = check(sequence, &dir);
        assert!(
            problems.is_empty(),
            "{} problems:\n{}",
            problems.len(),
            problems.join("\n")
        );
    });
}

/// Items 1 to 3 and 4: the first view, every animation and transition at
/// sampled times (values and pixels), the console.
#[test]
fn values_match_chromium() {
    run(&VALUES);
}

/// Item 3 and 4: the events of the transitions and animations of the step
/// groups of the capture, in Chromium's order, and the console.
#[test]
fn events_match_chromium() {
    run(&EVENTS);
}

/// The reference of the JS-off first view: its screenshot and its boxes.
fn js_off_page() -> Page {
    js_off_page_with(true)
}

/// [`js_off_page`] with the page's animations and transitions on or off.
fn js_off_page_with(animations: bool) -> Page {
    let dir = repo_root().join("fixtures/pages/videojs");
    let fetcher = Arc::new(ReplayFetcher::load(&dir).unwrap());
    let (mut page, url) = replay_page_with(&dir, false, fetcher, animations);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(60)));
    page.run_event_loop(Duration::from_millis(1500), Duration::from_secs(60));
    page
}

/// The pixels of a screenshot of the first view that differ from Chromium's
/// reference by more than [`PIXEL_TOLERANCE`] in a channel, as `(x, y)`.
fn different_pixels(page: &mut Page, reference: &Path) -> Vec<(u32, u32)> {
    let shot = page.screenshot(false).unwrap();
    let image = swb_paint::decode(&std::fs::read(reference).unwrap()).unwrap();
    let want = image.raster().unwrap();
    assert_eq!(
        (want.width(), want.height()),
        (shot.width(), shot.height()),
        "the size of the first view"
    );
    let mut found = Vec::new();
    for y in 0..shot.height() {
        for x in 0..shot.width() {
            let (a, b) = (
                shot.pixel(x, y).unwrap().demultiply(),
                want.pixel(x, y).unwrap().demultiply(),
            );
            let off = [
                a.red().abs_diff(b.red()),
                a.green().abs_diff(b.green()),
                a.blue().abs_diff(b.blue()),
            ]
            .into_iter()
            .any(|d| i32::from(d) > PIXEL_TOLERANCE);
            if off {
                found.push((x, y));
            }
        }
    }
    found
}

/// The regions of a set of pixels: the bounding boxes of the groups of
/// pixels that are within 8 px of each other, as `x0,y0-x1,y1 (count)`.
fn regions(points: &[(u32, u32)]) -> Vec<String> {
    let mut groups: Vec<(u32, u32, u32, u32, usize)> = Vec::new();
    for &(x, y) in points {
        let near = groups
            .iter()
            .position(|g| x + 8 >= g.0 && x <= g.2 + 8 && y + 8 >= g.1 && y <= g.3 + 8);
        match near {
            Some(i) => {
                let g = &mut groups[i];
                *g = (g.0.min(x), g.1.min(y), g.2.max(x), g.3.max(y), g.4 + 1);
            }
            None => groups.push((x, y, x, y, 1)),
        }
    }
    groups
        .iter()
        .map(|g| format!("{},{}-{},{} ({})", g.0, g.1, g.2, g.3, g.4))
        .collect()
}

/// Item 1 with JavaScript off: running animations do not change the first
/// view; the pixels and the boxes equal the references.
#[test]
fn js_off_first_view_matches_chromium() {
    on_page_thread(|| {
        let dir = repo_root().join("fixtures/pages/videojs");
        let mut page = js_off_page();
        let different = different_pixels(&mut page, &dir.join("reference/screenshot.png"));
        assert!(
            different.len() <= JS_OFF_DIFFERENT,
            "{} pixels differ (at most {JS_OFF_DIFFERENT}): {:?}",
            different.len(),
            regions(&different)
        );
        let reference: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("reference/boxes.json")).unwrap(),
        )
        .unwrap();
        let want = array(&reference["elements"]);
        let got = swb_engine::element_boxes(&page);
        assert_eq!(got.len(), want.len(), "the number of elements");
        let (mut wrong, mut below) = (Vec::new(), Vec::new());
        for (i, (g, w)) in got.iter().zip(want).enumerate() {
            assert_eq!(g.tag, text(&w["tag"]).to_ascii_lowercase(), "element {i}");
            let Some(expected) = w["rect"].as_array() else {
                continue;
            };
            let expected: Vec<f64> = expected.iter().filter_map(Value::as_f64).collect();
            let ok = g.rect.is_some_and(|r| {
                within(
                    &[
                        f64::from(r.x),
                        f64::from(r.y),
                        f64::from(r.width),
                        f64::from(r.height),
                    ],
                    &expected,
                    2.0,
                )
            });
            let message = format!("#{i} <{}>: {:?} != {expected:?}", g.tag, g.rect);
            match (ok, expected[1] < VIEWPORT_HEIGHT) {
                (true, _) => {}
                (false, true) => wrong.push(message),
                (false, false) => below.push(message),
            }
        }
        // The elements of the first view: the score of `fixtures/scores.json`.
        let in_view = want
            .iter()
            .filter(|w| w["rect"][1].as_f64().is_some_and(|y| y < VIEWPORT_HEIGHT))
            .count();
        let scores: Value = serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("fixtures/scores.json")).unwrap(),
        )
        .unwrap();
        let stored = scores["videojs"]["geometry"].as_f64().unwrap();
        let score = 1.0 - wrong.len() as f64 / in_view as f64;
        assert!(
            score + 0.002 >= stored,
            "geometry of the first view {score:.4} < stored {stored:.4}: {:?}",
            &wrong[..wrong.len().min(10)]
        );
        // Below the first view the bars (`grow-bar`: paused at width 0 until
        // `data-animate`, which only a script sets) and the marquee (moved by
        // its transform) differ from the references, which have animations
        // off.
        assert!(
            below.len() <= JS_OFF_BELOW_DIFFERENT,
            "{} boxes below the first view differ (at most {JS_OFF_BELOW_DIFFERENT}): {:?}",
            below.len(),
            &below[..below.len().min(10)]
        );
    });
}

/// The elements that the page's animations drive on the JS-off page, with
/// the selector of each: the three reveal elements of the hero (they have no
/// animation without a script: Chromium's `getAnimations()` of the fixture
/// has none, `swbtools animation-survey videojs`) and the two growing bars
/// (their animations are paused at their start until a script sets
/// `data-animate`).
const JS_OFF_REVEAL: [&str; 3] = [
    "h1",
    "h1 .text-stroke-faded-black",
    "main header div[class*=\"aspect-2/1\"]",
];

/// The computed values that the reveal animates (`color`,
/// `-webkit-text-stroke-color`, `opacity`, `transform`, `translate`, `scale`,
/// `clip-path`) and the `width` that the bars animate, of the elements that
/// match `selector`, as text.
fn js_off_values(page: &mut Page, selector: &str) -> Vec<String> {
    page.update_layout();
    let nodes = page.query_selector_all(selector).unwrap_or_default();
    let styles = page.styles().unwrap();
    nodes
        .into_iter()
        .filter_map(|node| styles.get(node))
        .map(|s| {
            format!(
                "color {:?}, stroke {:?}, opacity {}, transform {:?}, translate {:?}, scale {:?}, \
                 clip-path {:?}, width {:?}",
                s.color,
                s.text_stroke_color,
                s.opacity,
                s.transform,
                s.translate,
                s.scale,
                s.clip_path,
                s.width
            )
        })
        .collect()
}

/// Item 1 with JavaScript off, the animations: no animation changes the
/// first view or the page (running animations would change the title's
/// pixels that are already different, so the pixel count does not show it).
/// The reveal elements have the values that they have without animations,
/// and the two bars, whose animations Chromium pauses at their start (width
/// 0) until a script sets `data-animate`, are at width 0 and not growing.
#[test]
fn js_off_animations_leave_the_first_view_alone() {
    on_page_thread(|| {
        let mut animated = js_off_page_with(true);
        let mut plain = js_off_page_with(false);
        for selector in JS_OFF_REVEAL {
            let (on, off) = (
                js_off_values(&mut animated, selector),
                js_off_values(&mut plain, selector),
            );
            assert_eq!(on.len(), 1, "{selector}");
            assert_eq!(on, off, "{selector}: animations on and off");
        }
        let bars = js_off_values(&mut animated, ".data-bar");
        assert_eq!(bars.len(), 2, "the two bars");
        for bar in bars {
            assert!(
                bar.ends_with("width LengthPercentage(Percent(0.0))"),
                "a bar grows with JavaScript off: {bar}"
            );
        }
    });
}

/// A fetcher that inserts a script at the start of the `head` of the
/// document at `url`, as the replay tool's `--init-script` runs one in
/// Chromium before the page's scripts.
struct InitScript {
    inner: Arc<dyn Fetcher>,
    url: Url,
    script: &'static str,
}

impl Fetcher for InitScript {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let mut response = self.inner.fetch(request)?;
        if request.url == self.url && response.status == 200 {
            let html = String::from_utf8_lossy(&response.body).into_owned();
            let at = html
                .find("<head")
                .and_then(|h| html[h..].find('>').map(|e| h + e + 1))
                .unwrap_or(0);
            let tag = format!("<script>{}</script>", self.script);
            response.body = format!("{}{tag}{}", &html[..at], &html[at..]).into_bytes();
        }
        Ok(response)
    }

    fn fetch_stream(&self, request: &Request) -> Result<StreamResponse, NetError> {
        self.inner.fetch_stream(request)
    }

    fn cookie_jar(&self) -> Option<&CookieJar> {
        self.inner.cookie_jar()
    }
}

/// The player of videojs.org with media on (review of M14 feature 6a): the
/// page gives the `video` the HLS playlist as `src` and plays it through
/// Media Source Extensions (M15). Chromium 148 without `MediaSource` (the
/// replay tool with `data/videojs-no-mse-init.js`, which deletes it and
/// records the events of the media element; `data/videojs-no-mse-chromium.json`,
/// three equal runs) fails the load: `loadstart`, then `error` with code 4,
/// `networkState` 3, `readyState` 0, and Video.js shows its error dialog. swb
/// gives the same, with the feature `media` and without it. Departures: the
/// message (Chromium `DEMUXER_ERROR_COULD_NOT_PARSE`; swb with the feature
/// `DEMUXER_ERROR_COULD_NOT_OPEN: FFmpegDemuxer: open context failed`, the
/// playlist body arrives and FFmpeg has no HLS demuxer; without it
/// `MEDIA_ELEMENT_ERROR: Format error`, nothing is fetched) and
/// `canPlayType` of the HLS type (Chromium "maybe", swb "", ground rules).
/// The other tests of this file load with media off: their recordings have
/// `MediaSource`.
#[test]
fn without_media_source_the_player_fails_and_shows_its_error_dialog() {
    let Some(dir) = fixture("videojs") else {
        eprintln!("skipped: fixtures/js/videojs is missing");
        return;
    };
    on_page_thread(move || {
        let meta: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).unwrap())
                .unwrap();
        let fetcher = Arc::new(InitScript {
            inner: Arc::new(ReplayFetcher::load(&dir).unwrap()),
            url: Url::parse(meta["url"].as_str().unwrap()).unwrap(),
            script: include_str!("data/videojs-no-mse-init.js"),
        });
        // As the replay tool: animations off.
        let mut page = load_at_capture_animated(&dir, Duration::from_millis(1500), fetcher, false);
        // The step group of the recording: 3 s.
        page.run_event_loop(Duration::from_millis(3000), Duration::from_secs(60));
        let got = value(&mut page, include_str!("data/videojs-no-mse-record.txt"));
        let got: Value = serde_json::from_str(got.as_str().unwrap()).unwrap();
        let chromium: Value =
            serde_json::from_str(include_str!("data/videojs-no-mse-chromium.json")).unwrap();
        let want = &chromium[1];
        // The events without their times and messages.
        let events = |record: &Value| -> Vec<String> {
            array(&record["ev"])
                .iter()
                .map(|e| {
                    let e = e.as_str().unwrap();
                    let e = e.split_once(' ').map_or(e, |(_, rest)| rest);
                    let e = e.split(" src=").next().unwrap();
                    e.split_once(':').map_or(e, |(code, _)| code).to_owned()
                })
                .collect()
        };
        assert_eq!(
            events(want),
            ["loadstart ns=2 rs=0 err=-", "error ns=3 rs=0 err=4"]
        );
        // With the feature `media` (it plays MP4) the playlist's bytes
        // arrive and the demuxer cannot open them: `progress` and `suspend`
        // come before the `error`, as for any file that arrived and does
        // not open (probe rows of `media-states.json`). Chromium's built-in
        // HLS player instead fails at the rendition playlist, which the
        // fixture lacks, before any `progress`.
        let mut expected = events(want);
        let media = value(
            &mut page,
            "document.createElement('video').canPlayType('video/mp4')",
        ) != json!("");
        if media {
            expected.splice(
                1..1,
                [
                    "progress ns=3 rs=0 err=4".to_owned(),
                    "suspend ns=3 rs=0 err=4".to_owned(),
                ],
            );
        }
        assert_eq!(events(&got), expected, "{got}");
        assert_eq!((&got["ns"], &got["rs"]), (&want["ns"], &want["rs"]));
        assert_eq!(got["err"][0], want["err"][0]);
        assert_eq!(got["src"], want["src"]);
        assert_eq!(got["text"], json!(true));
        // The dialog: its elements and texts (the ids carry a React id).
        let dialog = |record: &Value| -> Vec<String> {
            array(&record["dlg"])
                .iter()
                .map(|d| {
                    let d = d.as_str().unwrap();
                    let (tag, rest) = d.split_once('#').unwrap();
                    let text = rest.split_once(' ').map_or("", |(_, t)| t);
                    format!("{tag} {text}")
                })
                .collect()
        };
        assert_eq!(dialog(&got), dialog(want));
    });
}

/// The cases of `tools/probes/css-videojs-6.json` (this feature's probe
/// file; `tests/data/css-videojs-6.json` has Chromium's values, three equal
/// runs): the cascade of the page's `starting-style` utilities under
/// Tailwind's `@supports` fallback (`-webkit-hyphens` is no property in
/// Chromium, so the fallback does not apply: the fix of this feature), and a
/// script's `finish()` before a style change (the row `finish-then-style-change`:
/// the transition that the script finished ends at its end value).
#[test]
fn probe_cases_of_feature_6() {
    on_page_thread(|| {
        let failures = common::run_probe_file_with(
            "css-videojs-6",
            common::animation_values::open,
            |_, _, expected| expected.clone(),
            |want, got| want == got,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The cases of `tools/probes/wheel-order.json` (`tests/data/wheel-order.json`
/// has Chromium's values, three equal runs): the scroll position that a
/// `wheel` listener sees. With only passive listeners on the path (explicit,
/// or by default on the window, the document and the body) Chromium scrolls
/// first and the listener sees the new `scrollY`; one non-passive listener on
/// the path makes the dispatch come first. The page's overscroll spring
/// reads `scrollY` in such a passive listener.
#[test]
fn probe_cases_of_wheel_order() {
    on_page_thread(|| {
        let failures = common::run_probe_file(
            "wheel-order",
            common::animation_values::open,
            |_, _, expected| expected.clone(),
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The differences, which a test expects to be none.
    fn assert_none(found: &[Difference]) {
        assert_eq!(found, Vec::<Difference>::new());
    }

    #[test]
    fn events_compare_in_order_as_a_set_or_in_a_range() {
        let event = |kind: &str, target: &str| json!([kind, "opacity", target, "", 0]);
        let record = |events: Vec<Value>| json!({ "events": events });
        let (a, b) = (event("transitionrun", "a"), event("transitionrun", "b"));
        let got = record(vec![a.clone(), b.clone()]);
        let want = record(vec![b.clone(), a.clone()]);
        let order = event_differences("g", &got, &want, &Events::Exact);
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].0, "events.order");
        assert_none(&event_differences("g", &got, &want, &Events::Set));
        let missing = event_differences("g", &got, &record(vec![a.clone()]), &Events::Set);
        assert_eq!(missing[0].0, "events[transitionrun|opacity|b|]");
        let ranges = BTreeMap::from([(a.to_string(), (1, 1)), (b.to_string(), (0, 1))]);
        assert_none(&event_differences(
            "g",
            &got,
            &want,
            &Events::Range(ranges.clone()),
        ));
        let twice = record(vec![a.clone(), a.clone()]);
        assert_eq!(
            event_differences("g", &twice, &want, &Events::Range(ranges)).len(),
            1
        );
    }

    #[test]
    fn records_compare_values_with_the_tolerances_of_the_animation_tests() {
        let record = |value: &str, rect: f64| {
            json!({
                "values": [["opacity", "div", "opacity", value]],
                "targets": [["div", 0, 0, rect, 10]],
                "rects": { "hero": [["h1", 0, 1, 2, rect, 4]] },
            })
        };
        let none = Events::Skip;
        assert_none(&record_differences(
            "g",
            &record("0.5", 10.0),
            &record("0.5", 10.05),
            &none,
        ));
        let found = record_differences("g", &record("0.5", 10.0), &record("0.6", 10.5), &none);
        let fields: Vec<&str> = found.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(
            fields,
            [
                "values[opacity|div|opacity]",
                "targets[div]",
                "rects.hero[h1 #0]"
            ]
        );
        // Chromium's `oklab()` equals swb's 8-bit result.
        let want = record("oklab(0 0 0 / 0)", 10.0);
        let got = record("rgba(0, 0, 0, 0)", 10.0);
        assert_none(&record_differences("g", &got, &want, &none));
        // An alpha is compared as the 8-bit value that swb stores (71 is
        // written 0.28), and an alpha that rounds to 1 is `rgb()` (Chromium
        // writes `rgba(..., 1)` in the middle of an interpolation).
        let want = record("oklab(0.850969 0.00136549 0.0331085 / 0.277837)", 10.0);
        let got = record("rgba(215, 205, 182, 0.28)", 10.0);
        assert_none(&record_differences("g", &got, &want, &none));
        let want = record("rgba(204, 195, 171, 1)", 10.0);
        let got = record("rgb(204, 195, 171)", 10.0);
        assert_none(&record_differences("g", &got, &want, &none));
        let want = record("oklab(0.818115 0.000112178 0.0341892 / 0.998762)", 10.0);
        assert_none(&record_differences("g", &got, &want, &none));
    }

    #[test]
    fn the_regions_of_pixels_are_boxes_of_nearby_pixels() {
        let found = regions(&[(0, 0), (3, 4), (100, 100), (101, 100)]);
        assert_eq!(found, ["0,0-3,4 (2)", "100,100-101,100 (2)"]);
    }
}
