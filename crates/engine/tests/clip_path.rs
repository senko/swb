//! `clip-path` with basic shapes and geometry boxes (M8 feature 12b): the
//! cases of the Chromium probe `tools/probes/clip-path.json`.
//!
//! - `js_cases.rs`: the syntax (what `CSS.supports` accepts), the value
//!   that `element.style` and `getComputedStyle` give, the stacking
//!   context, and hit testing (`elementFromPoint`), as the probe printed
//!   them in Chromium 148 (three equal runs).
//! - `paint_cases.rs`: the ink of boxes with a `clip-path` in a screenshot
//!   (the sum of the darkness of the pixels by luminance, and the bounding
//!   box of the pixels that are not white), as the probe measured it: the
//!   shapes of all kinds, the geometry boxes, transforms, children,
//!   inline boxes, tables, flex items and replaced elements, and SVG
//!   elements.
//!
//! The pages are served from memory; the test fonts are bundled. Scripts
//! run on a thread with a 16 MiB stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

#[path = "clip_path/js_cases.rs"]
mod js_cases;
#[path = "clip_path/paint_cases.rs"]
mod paint_cases;

mod common;

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Pixmap, Size, Url};
use swb_net::{Fetcher, Headers, NetError, NetworkFetcher, Request, Response};

use common::{answer, differences, new_page, on_page_threads};

/// A probe case with JavaScript queries: (query, Chromium's value).
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// The area of an element (x, y, width, height), the ink in it and the
/// bounding box of the ink relative to the area.
pub(crate) type InkItem = ([f32; 4], f64, Option<[i32; 4]>);

/// A probe case with ink queries.
pub(crate) struct PaintCase {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) items: &'static [InkItem],
}

/// Where swb departs from Chromium on purpose: (case, query index, swb's
/// value, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[];

/// Lines of the answers of queries that test many values (one value per
/// line) where swb departs from Chromium on purpose: (case, query index,
/// line, swb's line, why).
const SWB_LINES: &[(&str, usize, usize, &str, &str)] = &[
    (
        "cp-syntax-path",
        0,
        3,
        r#""path(nonzero, 'M0 0 h10 v10 z')" => true [path("M 0 0 h 10 v 10 Z")] {path("M 0 0 H 10 V 10 Z")} <clip-path: path("M 0 0 h 10 v 10 Z");>"#,
        "Chromium computes `path(nonzero, ...)` as `path(evenodd, ...)` (a defect: the paint of nonzero and even-odd paths is the same); swb keeps the rule",
    ),
    (
        "cp-syntax-misc",
        8,
        15,
        "circle(1ex) => circle(5px) | circle(1ex)",
        "swb's `ex` is half an `em`",
    ),
];

/// The ink items where swb departs from Chromium on purpose: (case, item,
/// why). The ink and the box are not compared there.
const SWB_INK: &[(&str, usize, &str)] = &[(
    "cp-paint-contexts",
    5,
    "a button and a text field: the edge of the circle on the control differs by 6 % of the ink",
)];

const PAGE: &str = "file:///case.html";

/// Serves one page and `data:` URLs; other URLs get a 404.
struct OnePage(&'static str);

impl Fetcher for OnePage {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if request.url.scheme() == "data" {
            return NetworkFetcher::new().fetch(request);
        }
        let (status, body) = if request.url.as_str() == PAGE {
            (200, self.0.as_bytes().to_vec())
        } else {
            (404, b"not found".to_vec())
        };
        let headers: Headers = [("content-type", "text/html")].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
            redirected: false,
        })
    }
}

/// A page that shows `html`, loaded, in the viewport of the probe.
fn open(html: &'static str, javascript: bool) -> Page {
    let mut page = new_page(Arc::new(OnePage(html)), 2, Size::new(900.0, 700.0));
    page.set_javascript(javascript);
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

fn run_js_case(case: &'static Case) -> Vec<String> {
    let mut page = open(case.html, true);
    let mut failures = Vec::new();
    for (index, &(query, chromium)) in case.queries.iter().enumerate() {
        let want = SWB_VALUES
            .iter()
            .find(|v| v.0 == case.name && v.1 == index)
            .map_or(chromium, |v| v.2);
        let mut want: serde_json::Value = if want.starts_with("error: ") {
            serde_json::Value::String(want.to_owned())
        } else {
            serde_json::from_str(want).unwrap()
        };
        if let Some(text) = want.as_str() {
            let mut lines: Vec<&str> = text.lines().collect();
            for d in SWB_LINES
                .iter()
                .filter(|d| d.0 == case.name && d.1 == index)
            {
                lines[d.2] = d.3;
            }
            want = serde_json::Value::String(lines.join("\n"));
        }
        let got = answer(&mut page, query);
        if got != want {
            failures.push(format!(
                "{} #{index}: {}\n{}",
                case.name,
                &query[..query.len().min(120)],
                differences(&want, &got)
            ));
        }
    }
    failures
}

#[test]
fn syntax_values_stacking_and_hit_testing() {
    let runs = on_page_threads(js_cases::CASES.iter().map(|case| move || run_js_case(case)));
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The ink of the area `area` (x, y, width, height) of a screenshot, as
/// the probe tool measures it: the sum of the darkness of the pixels (by
/// luminance, 1 for black), and the bounding box of the pixels that are
/// not white, relative to the area.
#[allow(clippy::many_single_char_names, clippy::cast_possible_truncation)]
fn ink_in(pixmap: &Pixmap, area: [f32; 4]) -> (f64, Option<[i32; 4]>) {
    let [x, y, w, h] = area;
    let (x0, y0) = (x.round() as i32, y.round() as i32);
    let (x1, y1) = ((x + w).round() as i32, (y + h).round() as i32);
    let mut ink = 0.0;
    let mut bbox: Option<[i32; 4]> = None;
    for py in y0..y1 {
        for px in x0..x1 {
            let (Ok(ux), Ok(uy)) = (u32::try_from(px), u32::try_from(py)) else {
                continue;
            };
            let Some(p) = pixmap.pixel(ux, uy) else {
                continue;
            };
            let p = p.demultiply();
            let luma = (f64::from(p.red()) * 299.0
                + f64::from(p.green()) * 587.0
                + f64::from(p.blue()) * 114.0)
                / 1000.0;
            let darkness = 255.0 - luma.round();
            if darkness > 0.0 {
                ink += darkness / 255.0;
                let (rx, ry) = (px - x0, py - y0);
                bbox = Some(match bbox {
                    None => [rx, ry, rx, ry],
                    Some([a, b, c, d]) => [a.min(rx), b.min(ry), c.max(rx), d.max(ry)],
                });
            }
        }
    }
    (ink, bbox.map(|[a, b, c, d]| [a, b, c - a + 1, d - b + 1]))
}

/// True if swb's ink is as the probe tool's comparison accepts it: the ink
/// within 4 % (at least 2) and each edge of the box within 2 px.
fn ink_matches(ink: f64, bbox: Option<[i32; 4]>, want: &(f64, Option<[i32; 4]>)) -> bool {
    if (ink - want.0).abs() > (0.04 * want.0).max(2.0) {
        return false;
    }
    match (bbox, want.1) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let edges = |r: [i32; 4]| [r[0], r[1], r[0] + r[2], r[1] + r[3]];
            edges(a)
                .iter()
                .zip(edges(b))
                .all(|(a, b)| (a - b).abs() <= 2)
        }
        _ => false,
    }
}

fn run_paint_case(case: &'static PaintCase) -> Vec<String> {
    let mut page = open(case.html, false);
    let pixmap = page.screenshot(true).unwrap();
    let mut failures = Vec::new();
    for (index, (area, ink, bbox)) in case.items.iter().enumerate() {
        if SWB_INK.iter().any(|d| d.0 == case.name && d.1 == index) {
            continue;
        }
        let (got_ink, got_bbox) = ink_in(&pixmap, *area);
        if !ink_matches(got_ink, got_bbox, &(*ink, *bbox)) {
            failures.push(format!(
                "{} #{index}: expected ink {ink:.2} box {bbox:?}, got ink {got_ink:.2} box {got_bbox:?}",
                case.name
            ));
        }
    }
    failures
}

#[test]
fn painted_shapes_match_chromium() {
    let runs = on_page_threads(
        paint_cases::CASES
            .iter()
            .map(|case| move || run_paint_case(case)),
    );
    let failures: Vec<String> = runs.into_iter().flatten().collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
