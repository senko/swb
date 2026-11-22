//! CSS animations (M13, ADR 0044): the cases of the Chromium probes
//! `tools/probes/css-animations.json` and `tools/probes/css-animations-3a.json`
//! run here, and the values of their `js` queries must be the ones that
//! Chromium 148 gave (`tests/data/css-animations.json` and
//! `tests/data/css-animations-3a.json`: `uv run --project tools swbtools probe
//! --json FILE`, three equal runs, then `jq '.cases | map({(.name):
//! [.results[] | (.value // null)]}) | add'`).
//!
//! Feature 3b runs the animations: the rows that seek (`pause()`, then
//! `currentTime`) and read computed values, `getAnimations()`, the play
//! states and `getKeyframes()` are checked, with the tolerances of ADR 0044
//! section 12 ([`close`]). Feature 3c adds the events, `ready` and
//! `finished`, `cancel()`, `finish()`, the script overrides and the rest of
//! the API. [`WAITS`] lists the rows that need a later feature, with the
//! reason; [`SWB_ROWS`] the deliberate departures.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{
    DataUrls, OnePage, SKIP_QUERY as SKIP, on_page_thread, open_script_page, run_probe_file_with,
};

use std::sync::Arc;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

/// The rows of `css-animations.json` that wait for a later feature: (case,
/// query index or `None` for every query of the case, reason).
const WAITS: &[(&str, Option<usize>, &str)] = &[
    // Missing: CSS transitions (feature 5).
    ("get-animations-order", None, "transitions (feature 5)"),
];

/// The rows of `css-animations.json` where swb departs from Chromium on
/// purpose: (case, query index, part of Chromium's value, swb's part;
/// every occurrence).
const SWB_ROWS: &[(&str, usize, &str, &str)] = &[
    // `Element.prototype.animate` does not exist (ADR 0044 section 7: no
    // target page calls it; feature tests take their fallback).
    (
        "animation-api",
        0,
        "function function function function function function function function function object function function",
        "function function undefined function function function function function function object function function",
    ),
    // `pending` right after the load: Chromium's animations of the page's
    // style sheet still wait for its first frame when the probe asks; swb's
    // rendering updates during the load computed the styles (the style
    // step for animations) and the next update started them. A race in
    // Chromium, not a rule: `pending` after `pause()` (row 13) is equal.
    (
        "animation-api",
        2,
        "k running true true",
        "k running false true",
    ),
];

/// The lines of rows of registered `<transform-list>` properties with a 3D
/// function (`translate3d()`, `scale3d()`, `rotateX()`, `matrix3d()`,
/// `perspective()`, `translateZ()`): swb's transforms are 2D, so such a
/// pair is discrete (ADR 0044 section 14, a departure of feature 3a).
/// (case, the names of the lines `TIME NAME VALUE`).
const SWB_3D_LINES: &[(&str, &[&str])] = &[
    (
        "registered-tl-2",
        &["t2", "t4", "t6", "t7", "t12", "t14", "t20", "t27"],
    ),
    ("registered-tl-3", &["t28", "t29", "t30"]),
];

/// A line of an expected value that any line matches.
const ANY_LINE: &str = "\u{1}any";

/// The cases of `css-animations-3a.json` that read members swb does not
/// have yet: `CSSKeyframesRule.cssRules`, `appendRule()` and
/// `CSSKeyframeRule` (backlog; no target page uses them).
const NOT_IMPLEMENTED: &[&str] = &["keyframes-rule-members"];

/// The values where swb departs from Chromium on purpose in
/// `css-animations-3a.json`: (case, query index, part of the value as
/// Chromium gives it, the part as swb gives it; every occurrence).
const SWB_LINES: &[(&str, usize, &str, &str)] = &[
    // An infinite math function in `cubic-bezier()` or `linear()` is the
    // largest `f64`, written as the number `1.79769e+308`. The declaration
    // that `element.style.setProperty()` sets reaches swb's cascade as
    // that text, a literal, which saturates to the `f32` range. Values
    // from style sheets keep Chromium's value (unit tests in
    // `animation/easing/tests.rs`).
    ("number-ranges", 7, "1.79769e+308", "3.40282e+38"),
    // swb has no `animation-trigger` (Chromium 148 parses it; no page of
    // the targets uses it).
    (
        "aliases",
        0,
        "animationTrigger=true",
        "animationTrigger=false",
    ),
    (
        "aliases",
        1,
        "animation-trigger:none=true",
        "animation-trigger:none=false",
    ),
    (
        "aliases",
        2,
        "animation-trigger=[none]",
        "animation-trigger=[]",
    ),
    // `transition-timing-function` takes `linear()` in feature 5.
    (
        "aliases",
        1,
        "transition-timing-function:linear(0, 1)=true",
        "transition-timing-function:linear(0, 1)=false",
    ),
    // When the layered lists differ in length, Chromium's `cssText` writes
    // an `animation` that does not parse back to the same longhands (its
    // block keeps the longhands). swb's cascade parses `cssText` again, so
    // swb writes the longhands there; `getPropertyValue('animation')` is
    // Chromium's.
    (
        "shorthand-blocks",
        0,
        "css{animation: 3s ease 0s 1 normal none running a, ease 0s 1 normal none running b;}",
        "css{animation-timing-function: ease, ease; animation-delay: 0s, 0s; \
         animation-iteration-count: 1, 1; animation-direction: normal, normal; \
         animation-fill-mode: none, none; animation-play-state: running, running; \
         animation-name: a, b; animation-timeline: auto; animation-range: normal; \
         animation-duration: 3s;}",
    ),
    (
        "shorthand-blocks",
        1,
        "css{animation: 1s ease 1s 1 normal none running a, 2s;}",
        "css{animation-duration: 1s; animation-timing-function: ease; \
         animation-iteration-count: 1; animation-direction: normal; \
         animation-fill-mode: none; animation-play-state: running; \
         animation-name: a; animation-timeline: auto; animation-range: normal; \
         animation-delay: 1s, 2s;}",
    ),
];

/// The cases of `css-animations-3a.json` that wait for a later change, with
/// the reason: `getKeyframes()` with computed values, item 4 of feature 3c,
/// was not done (ADR 0044 section 16, "Not done in 3c").
const WAITS_3A: &[(&str, &str)] = &[
    // Chromium's `getKeyframes()` of a CSS animation gives computed values
    // (`var()` substituted, `currentcolor` resolved), the underlying value
    // in the implicit keyframes, the properties of a keyframe in its own
    // order (`left` before `top` whatever the declarations; the longhands
    // of `margin` and `background` mixed) and a keyframe without values;
    // swb gives the specified values in the order of their first
    // declaration, and the element's current value in implicit keyframes.
    (
        "keyframes-resolution",
        "computed keyframe values and property order",
    ),
];

/// The expected value of a query: Chromium's, with the departures, or
/// [`SKIP`].
fn expected(file: &str, name: &str, index: usize, chromium: &Value) -> Value {
    let skipped = match file {
        "css-animations" => WAITS
            .iter()
            .any(|&(case, query, _)| case == name && query.is_none_or(|q| q == index)),
        _ => NOT_IMPLEMENTED.contains(&name) || WAITS_3A.iter().any(|&(c, _)| c == name),
    };
    if skipped {
        return Value::String(SKIP.to_owned());
    }
    let Some(text) = chromium.as_str() else {
        return chromium.clone();
    };
    let mut text = text.to_owned();
    let lines: Vec<(&str, usize, &str, &str)> = match file {
        "css-animations-3a" => SWB_LINES.to_vec(),
        _ => SWB_ROWS.to_vec(),
    };
    for (case, query, chromium_part, swb_part) in lines {
        if case == name && query == index {
            text = text.replace(chromium_part, swb_part);
        }
    }
    if let Some((_, names)) = SWB_3D_LINES.iter().find(|(case, _)| *case == name) {
        text = text
            .split('\n')
            .map(|line| {
                let name = line.split(' ').nth(1).unwrap_or("");
                if names.contains(&name) {
                    ANY_LINE
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    Value::String(oklab_as_rgb(&text))
}

/// `text` with each `oklab(L a b)` and `oklab(L a b / A)` written as the
/// 8-bit sRGB `rgb()` or `rgba()` that swb computes (ADR 0044 section 4:
/// swb stores the result of an interpolation between non-legacy colors as
/// 8-bit sRGB; tests compare such values with a tolerance of 1 per
/// channel). The conversion is the one of CSS Color 4 §9.2
/// (<https://www.w3.org/TR/css-color-4/#color-conversion-code>).
#[allow(clippy::many_single_char_names)] // The names of the formulas.
fn oklab_as_rgb(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("oklab(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "oklab(".len()..];
        let Some(end) = after.find(')') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let inner = &after[..end];
        let (lab, alpha) = match inner.split_once('/') {
            Some((lab, alpha)) => (lab, alpha.trim().parse::<f64>().ok()),
            None => (inner, None),
        };
        let v: Vec<f64> = lab
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        let [l, a, b] = v[..] else {
            out.push_str(&rest[at..at + "oklab(".len()]);
            rest = after;
            continue;
        };
        let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
        let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
        let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
        let linear = [
            4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_,
            -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_,
            -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701 * s_,
        ];
        let channel = |c: f64| {
            let c = if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (c.clamp(0.0, 1.0) * 255.0).round()
        };
        let [r, g, b] = linear.map(channel);
        match alpha {
            Some(alpha) => write!(out, "rgba({r}, {g}, {b}, {alpha})").unwrap(),
            None => write!(out, "rgb({r}, {g}, {b})").unwrap(),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Splits `text` into the parts between numbers and the numbers.
fn tokens(text: &str) -> (Vec<String>, Vec<f64>) {
    let mut words = vec![String::new()];
    let mut numbers = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let starts_number = c.is_ascii_digit()
            || (c == b'-' || c == b'.')
                && bytes
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_digit() || *n == b'.');
        let after_word = i > 0 && (bytes[i - 1].is_ascii_alphabetic() || bytes[i - 1] == b'_');
        if starts_number && !after_word {
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_digit()
                    || bytes[j] == b'.'
                    || ((bytes[j] == b'e' || bytes[j] == b'E')
                        && bytes
                            .get(j + 1)
                            .is_some_and(|n| n.is_ascii_digit() || *n == b'-' || *n == b'+'))
                    || ((bytes[j] == b'-' || bytes[j] == b'+')
                        && (bytes[j - 1] == b'e' || bytes[j - 1] == b'E')))
            {
                j += 1;
            }
            if let Ok(n) = text[i..j].parse::<f64>() {
                numbers.push(n);
                words.push(String::new());
                i = j;
                continue;
            }
        }
        words.last_mut().unwrap().push(c as char);
        i += 1;
    }
    (words, numbers)
}

/// True if `a` and `b` are equal with the tolerances of ADR 0044 section
/// 12: the text without numbers equal; numbers within a relative 1e-5 or
/// an absolute 1e-4 (Chromium prints 6 significant digits); the channels
/// of `rgb()` and `rgba()` within 1.
fn close(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (wa, na) = tokens(a);
    let (wb, nb) = tokens(b);
    if wa != wb || na.len() != nb.len() {
        return false;
    }
    let mut in_color = false;
    let mut channel = 0;
    for (i, (x, y)) in na.iter().zip(&nb).enumerate() {
        let before = &wa[i];
        if before.ends_with("rgb(") || before.ends_with("rgba(") {
            in_color = true;
            channel = 0;
        } else if in_color && !before.starts_with(',') {
            in_color = false;
        }
        let tolerance = if in_color && channel < 3 {
            1.0
        } else if in_color && channel == 3 {
            // An 8-bit alpha.
            0.5 / 255.0 + 1e-4
        } else {
            (1e-5 * x.abs().max(y.abs())).max(1e-4)
        };
        channel += 1;
        if (x - y).abs() > tolerance {
            return false;
        }
    }
    true
}

fn same(want: &Value, got: &Value) -> bool {
    if want.as_str() == Some(SKIP) || want == got {
        return true;
    }
    match (want.as_str(), got.as_str()) {
        (Some(a), Some(b)) if a.contains(ANY_LINE) => {
            let wa: Vec<&str> = a.split('\n').collect();
            let wb: Vec<&str> = b.split('\n').collect();
            wa.len() == wb.len()
                && wa
                    .iter()
                    .zip(&wb)
                    .all(|(x, y)| *x == ANY_LINE || close(x, y))
        }
        (Some(a), Some(b)) => close(a, b),
        _ => false,
    }
}

fn open(case: &Value) -> Page {
    let url = "file:///case.html";
    let fetcher = DataUrls(OnePage {
        url,
        html: case["html"].as_str().unwrap().to_owned(),
    });
    let default = serde_json::json!([400, 300]);
    let viewport = case.get("viewport").unwrap_or(&default).as_array().unwrap();
    open_script_page(
        Arc::new(fetcher),
        Url::parse(url).unwrap(),
        Size::new(
            viewport[0].as_f64().unwrap() as f32,
            viewport[1].as_f64().unwrap() as f32,
        ),
    )
}

#[test]
fn the_tolerances_of_numbers_and_colors() {
    assert!(close("opacity:0.183855", "opacity:0.18385512"));
    assert!(!close("opacity:0.18", "opacity:0.19"));
    assert!(close("rgb(201, 191, 174)", "rgb(200, 192, 174)"));
    assert!(!close("rgb(201, 191, 174)", "rgb(199, 191, 174)"));
    assert!(close("rgba(1, 2, 3, 0.5)", "rgba(1, 2, 3, 0.500001)"));
    assert!(!close("rgba(1, 2, 3, 0.5)", "rgba(1, 2, 3, 0.6)"));
    assert!(close(
        "matrix(1, 0, 0, 1, -0.0193, 0)",
        "matrix(1, 0, 0, 1, -0.01933, 0)"
    ));
    assert!(close("2.22045e-16", "0"));
    assert!(!close("a 1", "b 1"));
}

/// The rows of `css-animations.json`: the animated values at seek times,
/// `getAnimations()`, the play states, the timing and the keyframes.
#[test]
fn animation_probe_rows() {
    on_page_thread(|| {
        let failures = run_probe_file_with(
            "css-animations",
            open,
            |name, index, value| expected("css-animations", name, index, value),
            same,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The rows of `css-animations-3b.json`: the values at the seek times of
/// the pixel tests below and the before flag of `steps()` keyframe easings
/// (`keyframe-easing-before-flag`).
#[test]
fn animation_probe_rows_3b() {
    on_page_thread(|| {
        let failures =
            run_probe_file_with("css-animations-3b", open, |_, _, value| value.clone(), same);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The rows of `css-animations-3c.json` where swb departs from Chromium on
/// purpose: (case, query index, Chromium's value, swb's value).
const SWB_ROWS_3C: &[(&str, usize, &str, &str)] = &[
    // The constructors of `DocumentTimeline`, `Animation` and
    // `KeyframeEffect` throw (ADR 0044 section 7: no page of the targets
    // makes animations by script; `element.animate()` does not exist
    // either).
    (
        "timeline-time",
        3,
        "object | TypeError: Failed to construct 'AnimationTimeline': Illegal constructor | object | object |",
        "TypeError: Failed to construct 'DocumentTimeline': Illegal constructor | TypeError: Failed to construct 'AnimationTimeline': Illegal constructor | TypeError: Failed to construct 'Animation': Illegal constructor | TypeError: Failed to construct 'KeyframeEffect': Illegal constructor |",
    ),
    // The `animationstart` of `#a` (its pending play resolved by the
    // catch-up of the `getAnimations()` of this query) and the
    // `animationend` of `#b` (finished in the same task) have the same
    // scheduled time; composite order puts `#a` first. Chromium gives
    // either order between runs of the whole probe file.
    (
        "finish-and-start-time",
        1,
        "animationend k 1 - b | animationstart k 0 - a",
        "animationstart k 0 - a | animationend k 1 - b",
    ),
];

/// The rows of `css-animations-3c.json`: the events (order with
/// pseudo-elements and cancels in one update, a listener that cancels),
/// the `ready` and `finished` promises (Base UI's `Promise.all` of
/// `finished` settles on `display: none` and on a removed name, without
/// an `unhandledrejection`), `finish()`, `startTime`, `document.timeline`
/// and `AnimationEvent`.
#[test]
fn animation_probe_rows_3c() {
    on_page_thread(|| {
        let failures = run_probe_file_with(
            "css-animations-3c",
            open,
            |name, index, value| {
                let mut value = value.clone();
                for &(case, query, chromium, swb) in SWB_ROWS_3C {
                    if case == name && query == index {
                        value = Value::String(value.as_str().unwrap().replace(chromium, swb));
                    }
                }
                value
            },
            same,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The properties: grammar, `element.style`, `getComputedStyle`, the
/// shorthand and its serialization, the aliases, and (with running
/// animations) the keyframes and the animated registered custom properties
/// (`css-animations-3a.json`).
#[test]
fn animation_properties() {
    on_page_thread(|| {
        let failures = run_probe_file_with(
            "css-animations-3a",
            open,
            |name, index, value| expected("css-animations-3a", name, index, value),
            same,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The case `name` of the probe file `tools/probes/{file}.json`.
fn probe_case(file: &str, name: &str) -> Value {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join(format!("tools/probes/{file}.json"))).unwrap();
    let probe: Value = serde_json::from_str(&text).unwrap();
    probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
        .clone()
}

/// The colors of the screenshot of `page` at `points` relative to the
/// top-left corner of the first element of `selector`.
fn colors(page: &mut Page, selector: &str, points: &[(u32, u32)]) -> Vec<[u8; 3]> {
    let origin = common::value(
        page,
        &format!(
            "(() => {{ const r = document.querySelector('{selector}').getBoundingClientRect(); \
             return [Math.round(r.x), Math.round(r.y)]; }})()"
        ),
    );
    let (x0, y0) = (
        origin[0].as_u64().unwrap() as u32,
        origin[1].as_u64().unwrap() as u32,
    );
    let shot = page.screenshot(false).unwrap();
    points
        .iter()
        .map(|&(x, y)| {
            let p = shot.pixel(x0 + x, y0 + y).unwrap().demultiply();
            [p.red(), p.green(), p.blue()]
        })
        .collect()
}

fn near(got: &[[u8; 3]], want: &[[u8; 3]], tolerance: u8) -> bool {
    got.len() == want.len()
        && got
            .iter()
            .zip(want)
            .all(|(g, w)| g.iter().zip(w).all(|(a, b)| a.abs_diff(*b) <= tolerance))
}

/// The pixels of the hero of videojs.org at seek times (the `pixels` rows
/// of `hero-reveal` in `css-animations.json`, at 2000 ms, and of
/// `hero-reveal-pixels-850` in `css-animations-3b.json`, in the middle of
/// the reveal; Chromium 148, three equal runs). The video box is
/// translucent and moved by a sub-pixel; the title's color is between its
/// keyframes. Tolerance 3 (glyph edges); the box at 850 ms is exact.
#[test]
fn hero_reveal_pixels_at_seek_times() {
    on_page_thread(|| {
        let case = probe_case("css-animations-3b", "hero-reveal-pixels-850");
        let mut page = open(&case);
        let values = common::value(&mut page, case["measure"][0]["js"].as_str().unwrap());
        assert_eq!(
            values,
            "opacity:0.946079 ; transform:matrix(0.999461, 0, 0, 0.999461, 0, 0.647046)"
        );
        let points = [(30, 20), (100, 60), (210, 105), (40, 175), (300, 175)];
        let got = colors(&mut page, "header", &points);
        let want = [
            [196, 5, 8],
            [196, 5, 8],
            [196, 5, 8],
            [171, 153, 165],
            [120, 90, 150],
        ];
        assert!(near(&got[..3], &want[..3], 0), "{got:?}");
        assert!(near(&got, &want, 3), "{got:?}");
        // The end of the reveal (the probe's last seek, 2000 ms).
        common::value(&mut page, "seek(2000)");
        let got = colors(
            &mut page,
            "header",
            &[(30, 20), (100, 60), (40, 175), (300, 175)],
        );
        let want = [[200, 0, 0], [200, 0, 0], [200, 181, 188], [120, 90, 150]];
        assert!(near(&got, &want, 3), "{got:?}");
    });
}

/// The shimmer at seek times: the animated `background-position` paints as
/// the same static value (`shimmer-static-100` of `css-animations-3b.json`).
/// Departure: swb's glyphs clipped from a gradient (`background-clip:
/// text`) differ from Chromium's by up to 12 per channel, animated or not
/// (Chromium 168,168,178 at (10, 15), swb 156,156,168; backlog).
#[test]
fn shimmer_pixels_at_seek_times() {
    on_page_thread(|| {
        let points = [(10, 15), (150, 15), (290, 15)];
        let case = probe_case("css-animations-3b", "shimmer-pixels-625");
        let mut page = open(&case);
        assert_eq!(
            common::value(&mut page, case["measure"][0]["js"].as_str().unwrap()),
            "background-position:100% 0px"
        );
        let animated = colors(&mut page, "#s", &points);
        let mut still = open(&probe_case("css-animations-3b", "shimmer-static-100"));
        let fixed = colors(&mut still, "#s", &points);
        assert_eq!(animated, fixed);
        let chromium = [[168, 168, 178], [219, 219, 224], [255, 255, 255]];
        assert!(near(&animated, &chromium, 12), "{animated:?}");
        // The start of the next iteration (2500 ms) is the start state.
        common::value(&mut page, "seek(2500)");
        let wrapped = colors(&mut page, "#s", &points);
        common::value(&mut page, "seek(0)");
        assert_eq!(wrapped, colors(&mut page, "#s", &points));
    });
}

/// The check mode of the paint-only path (ADR 0044 section 9, feature 4a)
/// on the probe pages at their seek times: every query of `css-animations`,
/// `css-animations-3b` and `css-animations-3c` runs (it pauses the
/// animations and seeks them), then the page renders. A frame that took the
/// paint-only path is compared with a full layout and display list, and a
/// difference panics.
#[cfg(debug_assertions)]
#[test]
fn animation_probe_pages_with_the_paint_check() {
    on_page_thread(|| {
        swb_engine::set_restyle_check(true);
        let before = swb_engine::paint_checks();
        for file in ["css-animations", "css-animations-3b", "css-animations-3c"] {
            let text = std::fs::read_to_string(
                common::repo_root().join(format!("tools/probes/{file}.json")),
            )
            .unwrap();
            let probe: Value = serde_json::from_str(&text).unwrap();
            for case in probe["cases"].as_array().unwrap() {
                let mut page = open(case);
                page.set_animations(true);
                let steps = case["steps"].as_array().cloned().unwrap_or_default();
                if swb_automation::run_steps(&mut page, &steps).is_err() {
                    continue;
                }
                page.screenshot(false).unwrap();
                for query in case["measure"].as_array().unwrap() {
                    if let Some(source) = query["js"].as_str() {
                        common::value(&mut page, source);
                        page.screenshot(false).unwrap();
                    }
                }
            }
        }
        let checked = swb_engine::paint_checks() - before;
        eprintln!("paint-only frames compared with a full layout: {checked}");
        assert!(checked > 0, "no paint-only frame");
    });
}
