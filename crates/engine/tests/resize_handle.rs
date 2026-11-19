//! The resize handle of text areas and scroll containers with `resize`
//! (M12 feature 4c, item D): its pixels against Chromium 148's
//! (`tools/probes/textarea-resizer.json`). The pages are files in a
//! temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Size};
use swb_net::NetworkFetcher;

mod common;
use common::Site;

/// Boxes of 120x70 px with the corners at (10, 10), (210, 10), (410, 10)
/// and (610, 10): a text area, a right-to-left text area, a text area
/// with `resize: none` and a `div` with `overflow: auto; resize: both`.
const PAGE: &str = "<!DOCTYPE html><style>\
    body { margin: 0; background: #fff }\
    .b { position: absolute; top: 10px; width: 120px; height: 70px; box-sizing: border-box;\
         font: 14px Arial; padding: 6px }\
    div.b { border: 1px solid #888; overflow: auto; resize: both }\
    </style>\
    <textarea class=b style='left: 10px'></textarea>\
    <textarea class=b dir=rtl style='left: 210px'></textarea>\
    <textarea class=b style='left: 410px; resize: none'></textarea>\
    <div class=b style='left: 610px'></div>\
    <textarea class=b style='left: 810px; background: #223; border: 1px solid #fff; color: #fff'></textarea>";

/// The screenshot of [`PAGE`], made once for the tests of this file (they
/// run in parallel and share the page's directory).
fn screenshot() -> &'static swb_engine::Pixmap {
    static SHOT: std::sync::OnceLock<swb_engine::Pixmap> = std::sync::OnceLock::new();
    SHOT.get_or_init(render)
}

fn render() -> swb_engine::Pixmap {
    let site = Site::new("resize-handle");
    let url = site.page("page.html", PAGE);
    let mut page: Page =
        common::new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(1000.0, 100.0));
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(20));
    page.screenshot(false).unwrap()
}

fn color(shot: &swb_engine::Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let p = shot.pixel(x, y).unwrap().demultiply();
    (p.red(), p.green(), p.blue())
}

/// The pixels of the box at `left` (its border box starts at `left`, 10)
/// in the 18x18 square at its bottom right (or left) corner that are not
/// white.
fn dark_pixels(shot: &swb_engine::Pixmap, left: u32, at_left: bool) -> Vec<(u32, u32, u8)> {
    let mut found = Vec::new();
    for j in 52..69 {
        for i in 0..17u32 {
            let i = if at_left { i + 1 } else { i + 102 };
            let c = color(shot, left + i, 10 + j);
            if c != (255, 255, 255) {
                assert!(c.0 == c.1 && c.1 == c.2, "{c:?}");
                found.push((i, j, c.0));
            }
        }
    }
    found
}

#[test]
fn a_text_area_has_the_handle_of_chromium() {
    let shot = screenshot();
    // Chromium 148: dark strokes of (102, 102, 102) on white, in box
    // coordinates; the light stroke next to them is white on white.
    let long: Vec<(u32, u32, u8)> = (0..7).map(|k| (117 - k, 61 + k, 102)).collect();
    let short: Vec<(u32, u32, u8)> = (0..3).map(|k| (117 - k, 65 + k, 102)).collect();
    let mut expected: Vec<(u32, u32, u8)> = long.into_iter().chain(short).collect();
    expected.sort_by_key(|&(x, y, _)| (y, x));
    assert_eq!(dark_pixels(shot, 10, false), expected);
}

#[test]
fn a_right_to_left_text_area_has_it_at_the_left() {
    let shot = screenshot();
    let long: Vec<(u32, u32, u8)> = (0..7).map(|k| (2 + k, 61 + k, 102)).collect();
    let short: Vec<(u32, u32, u8)> = (0..3).map(|k| (2 + k, 65 + k, 102)).collect();
    let mut expected: Vec<(u32, u32, u8)> = long.into_iter().chain(short).collect();
    expected.sort_by_key(|&(x, y, _)| (y, x));
    // The left border column (x = 0) is not part of the 17 columns.
    let found: Vec<_> = dark_pixels(shot, 210, true)
        .into_iter()
        .filter(|&(_, _, v)| v == 102)
        .collect();
    assert_eq!(found, expected);
    // And none at the right.
    assert!(
        dark_pixels(shot, 210, false)
            .iter()
            .all(|&(_, _, v)| v != 102)
    );
}

#[test]
fn resize_none_has_no_handle_and_a_scroll_container_with_resize_has_one() {
    let shot = screenshot();
    assert_eq!(dark_pixels(shot, 410, false), vec![]);
    let div = dark_pixels(shot, 610, false);
    assert_eq!(div.iter().filter(|p| p.2 == 102).count(), 10);
}

#[test]
fn the_strokes_are_black_and_white_with_alpha_0_6() {
    // On the background (34, 34, 51): the dark stroke is (13, 13, 20), the
    // light stroke below it (166, 166, 173); Chromium 148's pixels at box
    // coordinates (117, 61) and (117, 62) of the dark text area.
    let shot = screenshot();
    let (x, y) = (810 + 117, 10 + 62);
    let near = |c: (u8, u8, u8), want: (u8, u8, u8)| {
        c.0.abs_diff(want.0) <= 1 && c.1.abs_diff(want.1) <= 1 && c.2.abs_diff(want.2) <= 1
    };
    assert!(
        near(color(shot, x, y - 1), (13, 13, 20)),
        "{:?}",
        color(shot, x, y - 1)
    );
    assert!(
        near(color(shot, x, y), (166, 166, 173)),
        "{:?}",
        color(shot, x, y)
    );
}
