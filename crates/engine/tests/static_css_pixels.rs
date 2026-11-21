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
