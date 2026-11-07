//! `filter` and the gradients in premultiplied space (M8 feature 12c): the
//! cases of the Chromium probes `tools/probes/filter.json` and
//! `tools/probes/gradients.json`, with the values that Chromium 148 gave
//! (three equal runs; `cases.rs` is generated from them).
//!
//! - `JS_CASES`: the syntax (what `CSS.supports` accepts), the value of
//!   `element.style` and `getComputedStyle` for about 400 filter values,
//!   the alias `-webkit-filter`, inheritance and `var()`, hit testing
//!   (`elementFromPoint`) and the scroll size around blurs and shadows.
//! - `BOX_CASES`: the boxes and the offset parents of the children of
//!   filtered boxes (the containing block of absolutely positioned and
//!   fixed boxes; inline boxes and `url()` and identity functions).
//! - `PIXEL_CASES`: the colors in a screenshot at points (the probe's
//!   `pixels` query): the color functions on seven colors, blur profiles
//!   of seven radii, 26 drop shadows, the stacking context, the order with
//!   `opacity`, `clip-path` and masks, the root and body, inline boxes,
//!   transforms, overflow, images, and the gradients with transparent
//!   stops over a light and a dark page, also repeating. The rows of the
//!   blur profiles and drop shadows sample every third and fifth px.
//!
//! The pages are served from memory; the test fonts are bundled. Scripts
//! run on a thread with a 16 MiB stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

#[path = "filter/cases.rs"]
mod cases;

mod common;

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Size, Url};

use common::{OnePage, answer, differences, new_page, on_page_threads};

/// A probe case with JavaScript queries: (query, Chromium's value).
pub(crate) struct JsCase {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) viewport: (f32, f32),
    pub(crate) javascript: bool,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// A probe case with `boxes` queries (selector, the rectangles of its
/// matches in document coordinates) and JavaScript queries.
pub(crate) struct BoxCase {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) boxes: &'static [(&'static str, &'static [[f32; 4]])],
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// The colors at points of one element: the top left corner of the page
/// area that the points are relative to, the points as `"x,y x,y ..."`, the
/// colors as `"rrggbb rrggbb ..."`, where `rrggbb*n` is n equal colors (text
/// keeps the generated file small).
pub(crate) struct PixelItem(
    pub(crate) (i32, i32),
    pub(crate) &'static str,
    pub(crate) &'static str,
);

/// The points and colors of an item in pairs.
fn samples(item: &PixelItem) -> Vec<((i32, i32), [u8; 3])> {
    let points = item.1.split_whitespace().map(|p| {
        let (x, y) = p.split_once(',').unwrap();
        (x.parse().unwrap(), y.parse().unwrap())
    });
    let colors = item.2.split_whitespace().flat_map(|run| {
        let (c, count) = run.split_once('*').unwrap_or((run, "1"));
        let channel = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).unwrap();
        vec![[channel(0), channel(2), channel(4)]; count.parse().unwrap()]
    });
    points.zip(colors).collect()
}

/// A probe case with `pixels` queries. `scripts` run before the screenshot.
pub(crate) struct PixelCase {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) viewport: (f32, f32),
    pub(crate) javascript: bool,
    pub(crate) scripts: &'static [&'static str],
    pub(crate) items: &'static [PixelItem],
}

/// The largest difference of a channel that counts as equal (the probe tool
/// uses 2 too).
const TOLERANCE: i32 = 2;

/// Lines of the answers of queries that test many values (one value per
/// line) where swb departs from Chromium on purpose: (case, query index,
/// line, swb's line, why).
const SWB_LINES: &[(&str, usize, usize, &str, &str)] = &[
    (
        "flt-syntax-hue-rotate",
        0,
        9,
        r#""hue-rotate(calc(1deg + 2deg))" => false [] {}"#,
        "swb's `calc()` has no angles",
    ),
    (
        "flt-syntax-drop-shadow",
        0,
        31,
        r#""drop-shadow(1px 2px 3px color-mix(in srgb, red, blue))" => true [drop-shadow(color-mix(in srgb, red, blue) 1px 2px 3px)] {drop-shadow(rgb(128, 0, 128) 1px 2px 3px)}"#,
        "Chromium writes a computed color of `color-mix()` as `color(srgb 0.5 0 0.5)`; swb has `rgb()`",
    ),
];

/// The pixel items where swb departs from Chromium on purpose: (case, item,
/// why). The colors are not compared there.
const SWB_PIXELS: &[(&str, usize, &str)] = &[
    (
        "fr-500px",
        0,
        "swb limits the standard deviation of a blur to 256 device px (ADR 0028 section 16): the faint tint is 249, Chromium's 253",
    ),
    (
        "fx-svg-child",
        0,
        "swb paints the `filter` of an `svg` element but not of the shapes and groups in it (ADR 0028, feature 12c notes)",
    ),
    ("fx-svg-child", 1, "as the item before"),
];

const PAGE: &str = common::CASE_PAGE;

/// A page that shows `html` (with `@H@` replaced by the helper functions of
/// the probe cases), loaded, in a viewport of `size`.
fn open(html: &str, size: Size, javascript: bool) -> Page {
    let html = html.replace("@H@", cases::HELPERS);
    let mut page = new_page(Arc::new(OnePage::with_data_urls(html)), 2, size);
    page.set_javascript(javascript);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// The answers of `queries` in `page` that differ from Chromium's.
fn check_queries(
    case: &str,
    page: &mut Page,
    queries: &[(&'static str, &'static str)],
) -> Vec<String> {
    let mut failures = Vec::new();
    for (index, &(query, chromium)) in queries.iter().enumerate() {
        let mut want: serde_json::Value = serde_json::from_str(chromium).unwrap();
        if let Some(text) = want.as_str() {
            let mut lines: Vec<&str> = text.lines().collect();
            for d in SWB_LINES.iter().filter(|d| d.0 == case && d.1 == index) {
                lines[d.2] = d.3;
            }
            want = serde_json::Value::String(lines.join("\n"));
        }
        let got = answer(page, query);
        if got != want {
            failures.push(format!(
                "{case} #{index}: {}\n{}",
                &query[..query.len().min(120)],
                differences(&want, &got)
            ));
        }
    }
    failures
}

fn run_js_case(case: &'static JsCase) -> Vec<String> {
    let size = Size::new(case.viewport.0, case.viewport.1);
    let mut page = open(case.html, size, case.javascript);
    check_queries(case.name, &mut page, case.queries)
}

#[test]
fn syntax_values_alias_and_hit_testing() {
    let runs = on_page_threads(cases::JS_CASES.iter().map(|case| move || run_js_case(case)));
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn run_box_case(case: &'static BoxCase) -> Vec<String> {
    let mut page = open(case.html, Size::new(500.0, 700.0), true);
    let mut failures = Vec::new();
    for &(selector, rects) in case.boxes {
        let expression = format!(
            "JSON.stringify(Array.from(document.querySelectorAll({selector:?}), e => {{ const r = e.getBoundingClientRect(); return [r.x + scrollX, r.y + scrollY, r.width, r.height]; }}))"
        );
        let got: Vec<[f32; 4]> =
            serde_json::from_str(answer(&mut page, &expression).as_str().unwrap()).unwrap();
        if got.len() != rects.len() {
            failures.push(format!(
                "{} {selector}: {} boxes, Chromium {}",
                case.name,
                got.len(),
                rects.len()
            ));
            continue;
        }
        for (i, (g, w)) in got.iter().zip(rects).enumerate() {
            // An element without a box has a rectangle of zeros.
            let off = g
                .iter()
                .zip(w)
                .any(|(a, b)| !b.is_nan() && (a - b).abs() > 1.0);
            if off {
                failures.push(format!(
                    "{} {selector}[{i}]: swb {g:?}, Chromium {w:?}",
                    case.name
                ));
            }
        }
    }
    failures.extend(check_queries(case.name, &mut page, case.queries));
    failures
}

#[test]
fn filtered_boxes_contain_their_positioned_descendants() {
    let runs = on_page_threads(
        cases::BOX_CASES
            .iter()
            .map(|case| move || run_box_case(case)),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn run_pixel_case(case: &'static PixelCase) -> Vec<String> {
    let size = Size::new(case.viewport.0, case.viewport.1);
    let mut page = open(case.html, size, case.javascript);
    for script in case.scripts {
        let _ = answer(&mut page, script);
    }
    let shot = page.screenshot(true).unwrap();
    let mut failures = Vec::new();
    for (index, item) in case.items.iter().enumerate() {
        if SWB_PIXELS.iter().any(|d| d.0 == case.name && d.1 == index) {
            continue;
        }
        let mut wrong = Vec::new();
        let samples = samples(item);
        for &((x, y), want) in &samples {
            let (px, py) = (item.0.0 + x, item.0.1 + y);
            // Outside the screenshot the page is white (as in the probe).
            let got = match (u32::try_from(px), u32::try_from(py)) {
                (Ok(ux), Ok(uy)) => shot.pixel(ux, uy).map_or([255; 3], |p| {
                    let p = p.demultiply();
                    [p.red(), p.green(), p.blue()]
                }),
                _ => [255; 3],
            };
            if got
                .iter()
                .zip(want)
                .any(|(g, w)| (i32::from(*g) - i32::from(w)).abs() > TOLERANCE)
            {
                wrong.push(format!("({x}, {y}): swb {got:?}, Chromium {want:?}"));
            }
        }
        if !wrong.is_empty() {
            failures.push(format!(
                "{} item {index}: {} of {} points differ, e.g. {}",
                case.name,
                wrong.len(),
                samples.len(),
                wrong.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
            ));
        }
    }
    failures
}

#[test]
fn painted_filters_and_gradients_match_chromium() {
    let runs = on_page_threads(
        cases::PIXEL_CASES
            .iter()
            .map(|case| move || run_pixel_case(case)),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
