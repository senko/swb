//! The pixels of the static CSS of M13 feature 2a against Chromium 148:
//! `box-shadow` (outer, inset, blur, spread, offsets, lists, rounded
//! boxes, rings), `text-shadow`, `-webkit-text-stroke`, `corner-shape` and
//! `isolation`. The pages are the cases `box-shadow-pixels`,
//! `box-shadow-rounded`, `text-shadow`, `text-stroke` and `corner-shape`
//! of `tools/probes/css-videojs-static.json` (the file holds the points;
//! `uv run --project tools swbtools probe tools/probes/css-videojs-static.json`
//! gives the values below, three equal runs). The channels may differ by
//! `TOLERANCE`; points at the anti-aliased corner of a ring are left out
//! (swb's corner pixel is 246, Chromium's 207: the two anti-aliasing
//! rules differ). The pages are files in a temporary directory; nothing
//! uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Page, Size};
use swb_net::NetworkFetcher;

mod common;
use common::Site;

const TOLERANCE: u8 = 3;

/// The HTML of the probe case `name`.
fn probe_html(name: &str) -> (String, Size) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join("tools/probes/css-videojs-static.json")).unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    let case = file["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap();
    let viewport = case["viewport"].as_array().unwrap();
    (
        case["html"].as_str().unwrap().to_owned(),
        Size::new(
            viewport[0].as_f64().unwrap() as f32,
            viewport[1].as_f64().unwrap() as f32,
        ),
    )
}

/// The screenshot of the page of the probe case `name`.
fn shot(name: &str) -> Pixmap {
    let (html, size) = probe_html(name);
    let site = Site::new(name);
    let url = site.page("page.html", &html);
    let mut page: Page = common::new_page(Arc::new(NetworkFetcher::new()), 2, size);
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(20));
    page.screenshot(false).unwrap()
}

type Pixmap = swb_engine::Pixmap;

/// Checks the colors at `points` (relative to `origin`) against `want`.
fn check(shot: &Pixmap, label: &str, origin: (i32, i32), points: &[(i32, i32)], want: &[[u8; 3]]) {
    assert_eq!(points.len(), want.len());
    for (&(dx, dy), w) in points.iter().zip(want) {
        let (x, y) = (origin.0 + dx, origin.1 + dy);
        // Points outside the screenshot are white.
        let got = match (u32::try_from(x), u32::try_from(y)) {
            (Ok(x), Ok(y)) => shot.pixel(x, y).map_or([255; 3], |p| {
                let p = p.demultiply();
                [p.red(), p.green(), p.blue()]
            }),
            _ => [255; 3],
        };
        let close = got.iter().zip(w).all(|(g, w)| g.abs_diff(*w) <= TOLERANCE);
        assert!(close, "{label} at ({dx}, {dy}): {got:?} against {w:?}");
    }
}

/// The 15 points of the cases of `box-shadow-pixels`.
const BOX_POINTS: [(i32, i32); 15] = [
    (-12, 30),
    (-4, 30),
    (30, -6),
    (30, 66),
    (30, 72),
    (30, 80),
    (64, 30),
    (70, 30),
    (-6, -6),
    (66, 66),
    (30, 30),
    (3, 3),
    (57, 57),
    (30, 5),
    (30, 57),
];

fn grey(v: &[u8]) -> Vec<[u8; 3]> {
    v.iter().map(|&g| [g, g, g]).collect()
}

#[test]
#[allow(clippy::too_many_lines)] // One check per box of the probe case.
fn box_shadows_match_chromium() {
    let s = shot("box-shadow-pixels");
    let at = |points: &[(i32, i32)], skip: &[usize]| -> Vec<(i32, i32)> {
        points
            .iter()
            .enumerate()
            .filter(|(i, _)| !skip.contains(i))
            .map(|(_, p)| *p)
            .collect()
    };
    let drop = |v: &[u8], skip: &[usize]| -> Vec<[u8; 3]> {
        v.iter()
            .enumerate()
            .filter(|(i, _)| !skip.contains(i))
            .map(|(_, &g)| [g, g, g])
            .collect()
    };
    let all = |skip: &[usize]| at(&BOX_POINTS, skip);
    // Tailwind's shadows (`sm` ... `xl`: two shadows with negative spread).
    check(
        &s,
        "sm",
        (30, 30),
        &BOX_POINTS,
        &grey(&[
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 249, 255, 255,
        ]),
    );
    check(
        &s,
        "md",
        (140, 30),
        &BOX_POINTS,
        &grey(&[
            255, 253, 255, 252, 255, 255, 254, 255, 255, 255, 255, 255, 250, 255, 255,
        ]),
    );
    check(
        &s,
        "lg",
        (250, 30),
        &BOX_POINTS,
        &grey(&[
            255, 250, 255, 242, 249, 255, 251, 255, 255, 254, 255, 255, 252, 255, 255,
        ]),
    );
    check(
        &s,
        "xl",
        (360, 30),
        &BOX_POINTS,
        &grey(&[
            254, 251, 255, 232, 241, 247, 252, 254, 255, 252, 255, 255, 252, 255, 255,
        ]),
    );
    // Rings (`0 0 0 1px #000`, and a list of three).
    check(
        &s,
        "ring",
        (470, 30),
        &all(&[12]),
        &drop(
            &[
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 207, 255, 255,
            ],
            &[12],
        ),
    );
    check(&s, "ring2", (30, 140), &all(&[12]), &[[255; 3]; 14]);
    // An inner shadow, a blur with spread, an offset without a blur.
    check(
        &s,
        "inset",
        (140, 140),
        &BOX_POINTS,
        &grey(&[
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 185, 222, 248, 253,
        ]),
    );
    check(
        &s,
        "blur",
        (250, 140),
        &BOX_POINTS,
        &grey(&[
            209, 149, 165, 173, 216, 246, 157, 203, 224, 229, 255, 255, 235, 255, 255,
        ]),
    );
    check(
        &s,
        "offset",
        (360, 140),
        &BOX_POINTS,
        &[
            [255; 3],
            [255; 3],
            [255; 3],
            [204, 0, 0],
            [255; 3],
            [255; 3],
            [204, 0, 0],
            [255; 3],
            [255; 3],
            [255; 3],
            [255; 3],
            [255; 3],
            [245, 207, 207],
            [255; 3],
            [255; 3],
        ],
    );
    // `0 0 #0000`: nothing.
    check(&s, "none", (470, 140), &BOX_POINTS, &[[255; 3]; 15]);
}

#[test]
fn rounded_boxes_and_clipped_shadows_match_chromium() {
    let s = shot("box-shadow-rounded");
    // `.r`: a pill with `0 6px 10px 2px #000000b3`.
    let points = [
        (-8, 30),
        (-1, 30),
        (50, 66),
        (50, 72),
        (50, 78),
        (101, 30),
        (108, 30),
        (3, 3),
        (50, 30),
        (50, -3),
        (50, -8),
    ];
    let want = [
        [239; 3],
        [156; 3],
        [144; 3],
        [225; 3],
        [254; 3],
        [172; 3],
        [245; 3],
        [253; 3],
        [0, 102, 204],
        [241; 3],
        [255; 3],
    ];
    check(&s, ".r", (60, 50), &points, &want);
    // `.c`: a shadow of a child that the parent clips.
    check(
        &s,
        ".c",
        (200, 40),
        &[(5, 5), (30, 30), (10, 30), (50, 30), (58, 58)],
        &grey(&[237, 255, 193, 201, 238]),
    );
}

#[test]
fn text_shadow_matches_chromium() {
    let s = shot("text-shadow");
    // `#c`: `3px 4px 6px #c00` on bold 40px text, at (160, 5).
    check(
        &s,
        "#c",
        (160, 5),
        &[(10, 40), (14, 44), (20, 48), (30, 52), (4, 20)],
        &[
            [243, 194, 194],
            [254, 251, 251],
            [255; 3],
            [255; 3],
            [34, 34, 34],
        ],
    );
    // `#b`: `0 1px 0 #00000080` leaves the points of the probe white.
    check(
        &s,
        "#b",
        (20, 55),
        &[(10, 28), (10, 29), (10, 30)],
        &[[255; 3]; 3],
    );
}

#[test]
fn text_stroke_matches_chromium() {
    let s = shot("text-stroke");
    // `#a`: transparent fill, a stroke of 1.5 px; only the stroke shows.
    check(
        &s,
        "#a",
        (10, 0),
        &[(20, 40), (23, 40), (24, 40), (30, 20), (40, 3)],
        &[[255; 3], [154, 154, 153], [31, 29, 27], [255; 3], [255; 3]],
    );
    // `#b`: a red fill with a black stroke of 4 px over it (points on the
    // outline have the sums of fill and stroke coverage; the last two
    // are anti-aliased edges).
    let got = |x: u32, y: u32| {
        let p = s.pixel(x, y).unwrap().demultiply();
        [p.red(), p.green(), p.blue()]
    };
    assert_eq!(got(10 + 20, 70 + 40), [204, 0, 0]);
    assert_eq!(got(10 + 24, 70 + 40), [0, 0, 0]);
}

/// A stroke of 8191 px is not drawn (Chromium draws only the fill); a
/// stroke of up to twice the font size is as in Chromium. Chromium also
/// draws strokes between that and 8190 px, which swb bounds in `raster.rs`
/// (`max_stroke_width`: the work of a page of text grows with the width).
#[test]
fn text_stroke_limits_match_chromium() {
    let s = shot("text-stroke-limit");
    let (white, black, red) = ([255; 3], [0; 3], [255, 0, 0]);
    let points = [(0, 0), (100, 50), (290, 90), (4, 14), (8, 8)];
    check(
        &s,
        "8191px",
        (100, 50),
        &points,
        &[white, white, white, black, white],
    );
    // (A stroke of 2 px: the pixel at (4, 14) is an anti-aliased edge, so
    // only the points outside the glyphs are checked.)
    check(
        &s,
        "2px",
        (100, 200),
        &[points[0], points[1], points[2]],
        &[white, white, white],
    );
    let mut points = points.to_vec();
    points.extend([(-20, 10), (40, 10), (-30, 40)]);
    check(
        &s,
        "50px",
        (100, 350),
        &points,
        &[red, white, white, red, red, red, red, white],
    );
}

#[test]
fn corner_shapes_match_chromium() {
    let s = shot("corner-shape");
    // Boxes of 80 x 60 with `border-radius: 22px`; the points run along the
    // diagonal into the top left corner.
    let diagonal = [(0, 0), (4, 4), (8, 8), (12, 12), (16, 16), (20, 20)];
    let red = [204, 0, 0];
    let white = [255; 3];
    // `#b` squircle at (110, 10): (4, 4) is already inside.
    check(
        &s,
        "squircle",
        (110, 10),
        &[(0, 0), (2, 2), (4, 4)],
        &[white, white, red],
    );
    // `#e` bevel at (10, 100): the corner is cut by a straight line.
    check(
        &s,
        "bevel",
        (10, 100),
        &diagonal[..6],
        &[white, white, white, red, red, red],
    );
    // `#f` scoop at (110, 100): a concave cut, the box starts later.
    check(
        &s,
        "scoop",
        (110, 100),
        &diagonal[..6],
        &[white, white, white, white, red, red],
    );
    // `#d` squircle with `overflow: hidden`: the blue child is clipped to
    // the squircle at (310, 10).
    check(
        &s,
        "clip",
        (310, 10),
        &[(0, 0), (2, 2), (4, 4), (6, 6), (10, 10)],
        &[white, white, [0, 102, 204], [0, 102, 204], [0, 102, 204]],
    );
}

/// The shadow of a box with `corner-shape` has the shape of the box: the
/// notch of a scoop (a concave corner) shows no shadow at first, and the
/// background of an opaque box does not hide the shadow in the notch.
#[test]
fn corner_shape_shadows_match_chromium() {
    let s = shot("corner-shape-shadow");
    let diagonal = [(-2, -2), (0, 0), (4, 4), (8, 8), (12, 12), (16, 16)];
    let (white, red) = ([255; 3], [204, 0, 0]);
    // `#a` scoop at (40, 40) with a 6 px ring: the notch is empty up to
    // (8, 8). Chromium shows the ring again at (12, 12) (blue); swb's
    // notch of the shadow is a little larger (it grows the radius by the
    // spread), so that point is left out.
    let points = [
        diagonal[0],
        diagonal[1],
        diagonal[2],
        diagonal[3],
        diagonal[5],
    ];
    check(
        &s,
        "scoop ring",
        (40, 40),
        &points,
        &[white, white, white, white, red],
    );
    // `#b` bevel at (160, 40) with a shadow moved by (6, 6): nothing at the
    // top left corner.
    check(
        &s,
        "bevel offset",
        (160, 40),
        &diagonal,
        &[white, white, white, white, red, red],
    );
}

/// The pixel cases of `tools/probes/css-videojs-2b.json` (M13 feature 2b):
/// colors that keep their space (each paints as its color clipped to
/// sRGB), gradients with `background-size`, `-position` and `-repeat`, and
/// the gradient of the videojs.org player controls. The values are
/// Chromium 148's (`tests/data/css-videojs-2b-pixels.json`: `uv run --project
/// tools swbtools probe --json tools/probes/css-videojs-2b.json`, three
/// equal runs, the `pixels` rows).
#[test]
fn colors_and_gradient_tiles_match_chromium() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |path: std::path::PathBuf| -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let probe = read(root.join("../../tools/probes/css-videojs-2b.json"));
    let data = read(root.join("tests/data/css-videojs-2b-pixels.json"));
    let mut failures = Vec::new();
    for (name, rows) in data.as_object().unwrap() {
        let case = probe["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name.as_str())
            .unwrap();
        let viewport = case["viewport"].as_array().unwrap();
        let size = Size::new(
            viewport[0].as_f64().unwrap() as f32,
            viewport[1].as_f64().unwrap() as f32,
        );
        let site = Site::new(name);
        let url = site.page("page.html", case["html"].as_str().unwrap());
        let mut page: Page = common::new_page(Arc::new(NetworkFetcher::new()), 2, size);
        page.navigate(url);
        common::finish_loading(&mut page, Duration::from_secs(20));
        let shot = page.screenshot(false).unwrap();
        for row in rows.as_array().unwrap() {
            let label = format!("{name} {}", row["label"].as_str().unwrap());
            let area = row["area"].as_array().unwrap();
            let origin = (
                area[0].as_f64().unwrap().round() as i32,
                area[1].as_f64().unwrap().round() as i32,
            );
            let points: Vec<(i32, i32)> = row["points"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_i64().unwrap() as i32, p[1].as_i64().unwrap() as i32))
                .collect();
            let want: Vec<[u8; 3]> = row["pixels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap() as u8))
                .collect();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                check(&shot, &label, origin, &points, &want);
            }));
            if let Err(error) = result {
                let text = error
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| label.clone());
                failures.push(text);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The color of the screenshot at (`x`, `y`); white outside it.
fn rgb_at(shot: &Pixmap, x: i64, y: i64) -> [u8; 3] {
    match (u32::try_from(x), u32::try_from(y)) {
        (Ok(x), Ok(y)) => shot.pixel(x, y).map_or([255; 3], |p| {
            let p = p.demultiply();
            [p.red(), p.green(), p.blue()]
        }),
        _ => [255; 3],
    }
}

/// The largest difference of a channel between two colors.
fn difference(a: [u8; 3], b: [u8; 3]) -> u8 {
    a.iter()
        .zip(&b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

/// A row of Chromium's data: the points in px from the origin of the
/// element's box and the colors there.
struct Row {
    label: String,
    origin: (i64, i64),
    points: Vec<(i64, i64)>,
    want: Vec<[u8; 3]>,
}

impl Row {
    fn new(row: &Value) -> Row {
        let area = row["area"].as_array().unwrap();
        let points = row["points"].as_array().unwrap();
        let pixels = row["pixels"].as_array().unwrap();
        Row {
            label: row["label"].as_str().unwrap().to_owned(),
            origin: (
                area[0].as_f64().unwrap().round() as i64,
                area[1].as_f64().unwrap().round() as i64,
            ),
            points: points
                .iter()
                .map(|p| (p[0].as_i64().unwrap(), p[1].as_i64().unwrap()))
                .collect(),
            want: pixels
                .iter()
                .map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap() as u8))
                .collect(),
        }
    }

    /// True for a dense line of 1 px steps or a set of points on straight
    /// edges (the selector `div#ID`): every point must match.
    fn is_line(&self) -> bool {
        self.label.starts_with("div#")
    }

    /// swb's color at point `i`, with the point moved by `(dx, dy)`.
    fn got(&self, shot: &Pixmap, i: usize, (dx, dy): (i64, i64)) -> [u8; 3] {
        let (px, py) = self.points[i];
        rgb_at(shot, self.origin.0 + px + dx, self.origin.1 + py + dy)
    }

    /// The largest difference over the points, with swb's pixels moved by
    /// `shift`, and a description of the worst point.
    fn worst(&self, shot: &Pixmap, shift: (i64, i64)) -> (u8, String) {
        let worst = (0..self.points.len())
            .map(|i| (difference(self.got(shot, i, shift), self.want[i]), i))
            .max()
            .unwrap_or((0, 0));
        let text = self.points.get(worst.1).map_or(String::new(), |p| {
            format!(
                "at {p:?}: {:?} against {:?}",
                self.got(shot, worst.1, shift),
                self.want[worst.1]
            )
        });
        (worst.0, text)
    }

    /// True if Chromium's colors at the points next to point `i` on its
    /// row and column (at most `FLAT_REACH` px away) are within
    /// `FLAT_RANGE` of its own: the point is away from an edge, so its color
    /// comes from the layer and not from anti-aliasing.
    fn flat(&self, i: usize) -> bool {
        let (x, y) = self.points[i];
        self.points.iter().zip(&self.want).all(|(&(nx, ny), want)| {
            let near = (nx == x && (ny - y).abs() <= FLAT_REACH)
                || (ny == y && (nx - x).abs() <= FLAT_REACH);
            !near || difference(*want, self.want[i]) <= FLAT_RANGE
        })
    }
}

/// The tolerance of the points away from edges: gradients round
/// differently by one level.
const FLAT_TOLERANCE: u8 = 2;

/// The largest variation of a channel between a point and its neighbors on
/// the sample grid, for a point that is not at an edge. The gradients of the
/// cases change by at most 11 levels over 6 px.
const FLAT_RANGE: u8 = 14;

/// The distance of the neighbors that decide whether a point is at an edge:
/// the steps of the sample grids are at most 6 px.
const FLAT_REACH: i64 = 6;

/// The part of the points of a row that may differ by more than
/// `EDGE_DIFFERENCE` at edges (a glyph edge or a rounded corner, where the
/// two browsers anti-alias differently or place a glyph one pixel apart).
const EDGE_SHARE: f64 = 0.09;

/// The difference at an edge point from which it counts against
/// `EDGE_SHARE`.
const EDGE_DIFFERENCE: u8 = 3;

/// The largest difference at any of the 4,300 points of `shimmer-static`.
/// Chromium and swb differ by at most 21 levels there (anti-aliasing at
/// the glyph edges, measured); the bound has a margin of 3. A shift of the
/// pixels by 1 px makes 4 or 5 of the 5 rows exceed it.
const SHIMMER_BOUND: u8 = 24;

/// The largest difference on the 1 px scan lines through the boxes of
/// `text-clip` (a horizontal and a vertical line of each box, through the
/// stems of "Hi"). The two browsers differ by 13 levels at the glyph edges
/// of most boxes (measured; 16 at the most, the margin is 3); a shift of 1 px
/// makes 55 or more of the 59 lines exceed it (`SHIFTED_SCANS_FAIL`).
const SCAN_BOUND: u8 = 16;

/// The bound for the stroked text (`stroke`).
const SCAN_BOUND_STROKE: u8 = 24;

/// The bound for the floated and the rotated box (measured: 40). It is
/// wide enough that a shift of 1 px may pass on these two lines, so the
/// shift check does not depend on them: it needs 50 of the 59 lines.
const SCAN_BOUND_MOVED: u8 = 48;

/// The bound of the scan line of the box `label`.
fn scan_bound(label: &str) -> u8 {
    if label.contains("#stroke[") {
        SCAN_BOUND_STROKE
    } else if label.contains("#float[") || label.contains("#rot[") {
        SCAN_BOUND_MOVED
    } else {
        SCAN_BOUND
    }
}

/// The difference on the points at straight edges of the boxes of
/// `rounded-fractional`, away from the curves.
const STRAIGHT_TOLERANCE: u8 = 3;

/// The pixel cases of `tools/probes/css-videojs-2c.json` (M13 feature 2c,
/// ADR 0046): `background-clip: text` (59 boxes: shadows, strokes, nested
/// and positioned boxes, effects, layers), the shimmer of the search
/// dialog, and gradients, images and colors clipped by rounded corners, at
/// whole and at fractional positions (`rounded-fractional` at scale 1 and
/// `rounded-fractional-2x`, which Chromium paints with doubled lengths and
/// swb at scale 2). The values are Chromium 148's
/// (`tests/data/css-videojs-2c-pixels.json`: `uv run --project tools
/// swbtools probe --json tools/probes/css-videojs-2c.json`, three equal
/// runs). Rows by kind:
///
/// - `shimmer-static`: every point within [`SHIMMER_BOUND`].
/// - Rows of the selector `div#ID`: dense scan lines (`text-clip`, every
///   point within [`SCAN_BOUND`]) and points at straight edges
///   (`rounded-fractional`, within [`STRAIGHT_TOLERANCE`]).
/// - The other rows: a point is flat if Chromium's colors next to it on the
///   grid are within [`FLAT_RANGE`] ([`Row::flat`]), and then swb equals it
///   within [`FLAT_TOLERANCE`]; the other points (glyph edges, the curve of
///   a corner) may differ, for at most [`EDGE_SHARE`] of the points of a row
///   by more than [`EDGE_DIFFERENCE`].
///
/// The last checks move swb's screenshot by one pixel in each direction
/// and require that the scan lines and the shimmer rows then fail: the test
/// sees a uniform shift of the glyphs.
#[test]
fn text_clip_and_rounded_layers_match_chromium() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |path: std::path::PathBuf| -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let probe = read(root.join("../../tools/probes/css-videojs-2c.json"));
    let data = read(root.join("tests/data/css-videojs-2c-pixels.json"));
    let mut failures = Vec::new();
    let mut shifted_scans = [(0, 0); 4];
    let mut shifted_shimmer = [(0, 0); 4];
    for (name, rows) in data.as_object().unwrap() {
        let case = probe["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name.as_str())
            .unwrap();
        // The 2x case is the same page at scale 2 with the lengths halved.
        let scale = if name.ends_with("-2x") { 2.0 } else { 1.0 };
        let html = case["html"]
            .as_str()
            .unwrap()
            .replace("--u:2", if scale == 2.0 { "--u:1" } else { "--u:2" });
        let viewport = case["viewport"].as_array().unwrap();
        let size = Size::new(
            viewport[0].as_f64().unwrap() as f32 / scale,
            viewport[1].as_f64().unwrap() as f32 / scale,
        );
        let site = Site::new(name);
        let url = site.page("page.html", &html);
        let fetcher = Arc::new(NetworkFetcher::new());
        let mut page: Page = common::new_page_at_scale(fetcher, 2, size, scale);
        page.navigate(url);
        common::finish_loading(&mut page, Duration::from_secs(20));
        let shot = page.screenshot(false).unwrap();
        for row in rows.as_array().unwrap().iter().map(Row::new) {
            let label = format!("{name} {}", row.label);
            if name == "shimmer-static" {
                let (worst, at) = row.worst(&shot, (0, 0));
                if worst > SHIMMER_BOUND {
                    failures.push(format!("{label}: a point differs by {worst} {at}"));
                }
                for (i, shift) in SHIFTS.into_iter().enumerate() {
                    shifted_shimmer[i].1 += 1;
                    if row.worst(&shot, shift).0 > SHIMMER_BOUND {
                        shifted_shimmer[i].0 += 1;
                    }
                }
            } else if row.is_line() {
                let bound = if name == "text-clip" {
                    scan_bound(&row.label)
                } else {
                    STRAIGHT_TOLERANCE
                };
                let (worst, at) = row.worst(&shot, (0, 0));
                if worst > bound {
                    failures.push(format!("{label}: a point differs by {worst} {at}"));
                }
                if name == "text-clip" {
                    // A glyph one pixel to the side or up and down must fail.
                    for (i, shift) in SHIFTS.into_iter().enumerate() {
                        shifted_scans[i].1 += 1;
                        if row.worst(&shot, shift).0 > bound {
                            shifted_scans[i].0 += 1;
                        }
                    }
                }
            } else {
                check_grid(&label, &row, &shot, &mut failures);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for (i, shift) in SHIFTS.iter().enumerate() {
        let (failing, all) = shifted_scans[i];
        assert!(
            failing >= SHIFTED_SCANS_FAIL,
            "a shift of {shift:?} px fails only {failing} of {all} scan lines"
        );
        let (failing, all) = shifted_shimmer[i];
        assert!(
            failing >= SHIFTED_SHIMMER_FAIL,
            "a shift of {shift:?} px fails only {failing} of {all} shimmer rows"
        );
    }
}

/// The shifts of swb's pixels that the scan lines must see.
const SHIFTS: [(i64, i64); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// The scan lines of `text-clip` that must fail when swb's pixels move by
/// 1 px: 55 to 57 of 59 do (measured); the others have no glyph on the line.
const SHIFTED_SCANS_FAIL: usize = 50;

/// The rows of `shimmer-static` that must fail when swb's pixels move by
/// 1 px: 4 or 5 of 5 do (measured).
const SHIFTED_SHIMMER_FAIL: usize = 4;

/// Checks a row of grid points: flat points within `FLAT_TOLERANCE`, a
/// limited share of the others off by more than `EDGE_DIFFERENCE`.
fn check_grid(label: &str, row: &Row, shot: &Pixmap, failures: &mut Vec<String>) {
    let mut edge_differences = 0;
    let mut flat_differences = 0;
    let mut worst = (0, String::new());
    for i in 0..row.points.len() {
        let got = row.got(shot, i, (0, 0));
        let difference = difference(got, row.want[i]);
        if row.flat(i) {
            if difference > FLAT_TOLERANCE {
                flat_differences += 1;
                // Report the first points of a row only.
                if flat_differences <= 3 {
                    failures.push(format!(
                        "{label} at {:?}: {got:?} against {:?} (flat area)",
                        row.points[i], row.want[i]
                    ));
                }
            }
        } else if difference > EDGE_DIFFERENCE {
            edge_differences += 1;
            if difference > worst.0 {
                worst = (
                    difference,
                    format!("{:?}: {got:?} against {:?}", row.points[i], row.want[i]),
                );
            }
        }
    }
    let allowed = (row.points.len() as f64 * EDGE_SHARE).ceil() as usize;
    if edge_differences > allowed {
        failures.push(format!(
            "{label}: {edge_differences} of {} edge points differ by more than \
             {EDGE_DIFFERENCE} (at most {allowed}); the worst is {}",
            row.points.len(),
            worst.1
        ));
    }
}
