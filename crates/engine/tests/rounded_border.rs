//! Rounded boxes with borders (M12 feature 4c, items B and E): the pixels
//! of boxes at fractional positions, a border with a transparent side, and
//! the join of two links of a pagination, against Chromium 148
//! (`tools/probes/border-radius-sides.json`,
//! `tools/probes/border-radius-pagination.json`). The pages are files in a
//! temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Pixmap, Size};
use swb_net::NetworkFetcher;

mod common;
use common::Site;

fn screenshot(test: &str, body: &str) -> Pixmap {
    let site = Site::new(test);
    let url = site.page(
        "page.html",
        &format!("<!DOCTYPE html><body style='margin:0;background:#fff'>{body}"),
    );
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(300.0, 200.0));
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(20));
    page.screenshot(false).unwrap()
}

fn color(shot: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let p = shot.pixel(x, y).unwrap().demultiply();
    (p.red(), p.green(), p.blue())
}

#[test]
fn a_rounded_box_at_a_fractional_position_is_snapped_to_pixels() {
    // Chromium 148, probe cases `frac-top-rounded` and
    // `frac-top-rounded-bg`: the box at (10.3, 10.4) of 60x40 px.
    let shot = screenshot(
        "rounded-snap",
        "<div style='margin:10.4px 0 0 10.3px;width:60px;height:40px;box-sizing:border-box;\
         border:4px solid #0d6efd;border-radius:12px'></div>\
         <div style='margin:20.4px 0 0 10.3px;width:60px;height:40px;background:#e9ecef;\
         border-radius:12px'></div>",
    );
    let blue = (13, 110, 253);
    // The top border covers the rows 10 to 13 (the box starts at 10), the
    // left border the columns 10 to 13, without a partly covered row or
    // column.
    for y in 10..14 {
        assert_eq!(color(&shot, 40, y), blue, "row {y}");
    }
    assert_eq!(color(&shot, 40, 9), (255, 255, 255));
    assert_eq!(color(&shot, 40, 14), (255, 255, 255));
    for x in 10..14 {
        assert_eq!(color(&shot, x, 30), blue, "column {x}");
    }
    assert_eq!(color(&shot, 9, 30), (255, 255, 255));
    assert_eq!(color(&shot, 14, 30), (255, 255, 255));
    // The second box starts at y = 10.4 + 40 + 20.4 = 70.8: row 71.
    assert_eq!(color(&shot, 40, 70), (255, 255, 255));
    assert_eq!(color(&shot, 40, 71), (233, 236, 239));
    assert_eq!(color(&shot, 9, 90), (255, 255, 255));
    assert_eq!(color(&shot, 10, 90), (233, 236, 239));
}

#[test]
fn the_spinner_border_is_a_c() {
    // Bootstrap's `.spinner-border`: Chromium 148's pixels (probe case
    // `spinner`) at box coordinates; the box is at (10, 10).
    let shot = screenshot(
        "rounded-spinner",
        "<div style='position:absolute;left:10px;top:10px;width:32px;height:32px;box-sizing:border-box;\
         border:4px solid #222;border-right-color:transparent;border-radius:50%'></div>",
    );
    for (x, y) in [
        (23, 5),
        (21, 3),
        (3, 15),
        (15, 3),
        (15, 29),
        (23, 27),
        (7, 25),
    ] {
        assert_eq!(color(&shot, 10 + x, 10 + y), (34, 34, 34), "({x}, {y})");
    }
    for (x, y) in [(29, 15), (29, 17), (27, 9), (27, 23), (29, 11), (29, 21)] {
        assert_eq!(color(&shot, 10 + x, 10 + y), (255, 255, 255), "({x}, {y})");
    }
}

#[test]
fn two_links_of_a_pagination_join_with_a_seam_at_the_square_corners() {
    // Chromium 148, probe case `pagination-26-25-16`: the active link
    // (last child, rounded at the right) over the first link. Its corner
    // pixel at the left is covered 0.79 by the border (135 over 222), as
    // the sides of the border are painted one by one and the background
    // stays inside the border.
    let css = "ul { display: flex; list-style: none; padding: 0; margin: 20px 0 0 16px }\
        .l { position: relative; display: block; padding: 6px 0; text-align: center; \
             box-sizing: border-box; height: 31px; border: 1px solid #dee2e6; background: #fff; \
             color: #0d6efd; text-decoration: none }\
        li:not(:first-child) .l { margin-left: -1px }\
        li:first-child .l { border-top-left-radius: 6px; border-bottom-left-radius: 6px }\
        li:last-child .l { border-top-right-radius: 6px; border-bottom-right-radius: 6px }\
        .l.active { z-index: 3; color: #fff; background: #7092ff; border-color: #7092ff }";
    let shot = screenshot(
        "rounded-pagination",
        &format!(
            "<style>{css}</style><ul><li><a class=l style='width:26px'>1</a></li>\
         <li><a class='l active' style='width:25px'>12</a></li></ul>"
        ),
    );
    // Box coordinates of the first link (16, 20): the second starts at 25.
    let at = |x: u32, y: u32| color(&shot, 16 + x, 20 + y);
    assert!((130..=146).contains(&at(25, 0).0), "{:?}", at(25, 0));
    assert!((130..=146).contains(&at(25, 30).0), "{:?}", at(25, 30));
    assert_eq!(at(24, 0), (222, 226, 230));
    assert_eq!(at(26, 0), (112, 146, 255));
    assert_eq!(at(25, 1), (112, 146, 255));
    assert_eq!(at(25, 15), (112, 146, 255));
}
