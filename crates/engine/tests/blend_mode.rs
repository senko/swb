//! `mix-blend-mode` of boxes with a transform or a `backdrop-filter`, of
//! frame documents and of SVG elements (M12 feature 4d): the pixels against
//! Chromium 148 (cases of `tools/probes/mix-blend-mode.json`: `own-rotate`,
//! `own-rotate-in-rounded-clip`, `own-scale-in-rounded-clip`,
//! `own-backdrop-filter`, `own-backdrop-filter-brightness`,
//! `parent-backdrop-filter-isolates`, `in-iframe`, `in-svg`). The pages are
//! files in a temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Size};
use swb_net::NetworkFetcher;

mod common;
use common::Site;

/// The style of every case: three stripes (233,236,239), (200,60,40) and
/// (60,120,200) of 60 x 30 px at the top left, and a box `#x` of
/// (120,200,90) over them.
const STYLE: &str = "<!DOCTYPE html><style>body{margin:0;background:#fff} \
    #bg{position:relative;width:60px;height:90px} #bg i{display:block;height:30px} \
    #a{background:rgb(233,236,239)} #b{background:rgb(200,60,40)} #c{background:rgb(60,120,200)} \
    #x{position:absolute;left:0;top:0;width:60px;height:90px;background:rgb(120,200,90)}";

/// The three stripes, and `rest` in the `#bg` box.
fn page(css: &str, rest: &str) -> String {
    format!("{STYLE}{css}</style><div id=bg><i id=a></i><i id=b></i><i id=c></i>{rest}</div>")
}

/// The colors of the screenshot of `html` at `points`.
fn colors(name: &str, html: &str, points: &[(u32, u32)]) -> Vec<(u8, u8, u8)> {
    let site = Site::new(name);
    let url = site.page("page.html", html);
    let mut page: Page =
        common::new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(200.0, 150.0));
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(20));
    let shot = page.screenshot(false).unwrap();
    points
        .iter()
        .map(|&(x, y)| {
            let p = shot.pixel(x, y).unwrap().demultiply();
            (p.red(), p.green(), p.blue())
        })
        .collect()
}

/// True if every channel of each color is within `tolerance` of `want`.
fn close(got: &[(u8, u8, u8)], want: &[(u8, u8, u8)], tolerance: u8) -> bool {
    got.len() == want.len()
        && got.iter().zip(want).all(|(g, w)| {
            g.0.abs_diff(w.0) <= tolerance
                && g.1.abs_diff(w.1) <= tolerance
                && g.2.abs_diff(w.2) <= tolerance
        })
}

const STRIPES: [(u32, u32); 3] = [(30, 15), (30, 45), (30, 75)];

/// The points of the cases with a rotated or scaled box: the stripes and
/// two points near the left and right edge.
const EDGES: [(u32, u32); 5] = [(30, 15), (30, 45), (30, 75), (8, 40), (52, 40)];

const DIFFERENCE: [(u8, u8, u8); 3] = [(113, 36, 149), (80, 140, 50), (60, 80, 110)];

fn check(name: &str, html: &str, points: &[(u32, u32)], want: &[(u8, u8, u8)]) {
    let got = colors(name, html, points);
    assert!(close(&got, want, 3), "{name}: {got:?} against {want:?}");
}

#[test]
fn a_rotated_box_blends() {
    // Chromium: the transformed result blends, with or without a clip.
    let want = [
        DIFFERENCE[0],
        DIFFERENCE[1],
        DIFFERENCE[2],
        (80, 140, 50),
        (80, 140, 50),
    ];
    let html = page(
        "#x{transform:rotate(10deg);mix-blend-mode:difference}",
        "<div id=x></div>",
    );
    check("blend-rotate", &html, &EDGES, &want);
    let clip = "#p{position:absolute;left:0;top:0;width:60px;height:90px;overflow:hidden;\
                border-radius:12px}#x{mix-blend-mode:difference;";
    let html = page(
        &format!("{clip}transform:rotate(10deg)}}"),
        "<div id=p><div id=x></div></div>",
    );
    check("blend-rotate-rounded", &html, &EDGES, &want);
    let html = page(
        &format!("{clip}transform:scale(.8)}}"),
        "<div id=p><div id=x></div></div>",
    );
    check("blend-scale-rounded", &html, &EDGES, &want);
}

#[test]
fn a_blend_in_a_rotated_parent_is_isolated() {
    // The transformed parent is an isolated group: no blend.
    let html = page(
        "#p{position:absolute;left:0;top:0;width:60px;height:90px;transform:rotate(10deg)}\
         #x{mix-blend-mode:difference}",
        "<div id=p><div id=x></div></div>",
    );
    check(
        "blend-rotated-parent",
        &html,
        &STRIPES,
        &[(120, 200, 90); 3],
    );
}

#[test]
fn a_blend_with_a_backdrop_filter_blends_the_filtered_backdrop() {
    // Chromium: the backdrop filter, then the box, then the blend.
    let html = page(
        "#x{background:rgba(120,200,90,.5);backdrop-filter:blur(2px);mix-blend-mode:difference}",
        "<div id=x></div>",
    );
    check(
        "blend-backdrop-blur",
        &html,
        &STRIPES,
        &[(173, 136, 194), (140, 100, 45), (60, 100, 155)],
    );
    let html = page(
        "#x{background:rgba(120,200,90,.5);backdrop-filter:blur(2px) brightness(.5);\
         mix-blend-mode:difference}",
        "<div id=x></div>",
    );
    check(
        "blend-backdrop-brightness",
        &html,
        &STRIPES,
        &[(59, 100, 75), (60, 100, 45), (60, 100, 55)],
    );
}

#[test]
fn a_backdrop_filter_parent_isolates_a_blended_child() {
    let html = page(
        "#p{position:absolute;left:0;top:0;width:60px;height:90px;backdrop-filter:blur(1px)}\
         #x{mix-blend-mode:difference}",
        "<div id=p><div id=x></div></div>",
    );
    check(
        "blend-backdrop-parent",
        &html,
        &STRIPES,
        &[(120, 200, 90); 3],
    );
}

#[test]
fn the_document_of_a_frame_is_an_isolated_group() {
    let frame = "<style>body{margin:0;height:90px}#x{position:absolute;left:0;top:0;width:60px;\
                 height:90px;background:rgb(120,200,90);mix-blend-mode:difference}</style>\
                 <div id=x></div>";
    let html = page(
        "#f{position:absolute;left:0;top:0;width:60px;height:90px;border:0}",
        &format!("<iframe id=f srcdoc=\"{frame}\"></iframe>"),
    );
    check("blend-frame", &html, &STRIPES, &[(120, 200, 90); 3]);
}

#[test]
fn an_svg_element_blends() {
    let html = page(
        "#v{position:absolute;left:0;top:0}",
        "<svg id=v width=60 height=90><rect width=60 height=90 fill='rgb(120,200,90)' \
         style='mix-blend-mode:difference'/></svg>",
    );
    check("blend-svg", &html, &STRIPES, &DIFFERENCE);
    // A group blends as a whole, and the stacking context of the `svg`
    // parent isolates it.
    let html = page(
        "#v{position:absolute;left:0;top:0;z-index:1}",
        "<svg id=v width=60 height=90><g style='mix-blend-mode:difference'>\
         <rect width=60 height=90 fill='rgb(120,200,90)'/></g></svg>",
    );
    check("blend-svg-group", &html, &STRIPES, &[(120, 200, 90); 3]);
}
