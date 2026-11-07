//! M8 feature 12 for the Wikipedia article "Web browser" with JavaScript on:
//! the local fixture `fixtures/js/wikipedia-web-browser` (git-ignored; set
//! `SWB_JS_FIXTURES` to the directory `fixtures/js` of another checkout), in
//! one load with all steps in order, with the page clock fixed at the
//! `captured` time of the fixture, as the tools fix it. The test skips when
//! the fixture is missing.
//!
//! The steps are in `data/wikipedia-web-browser-steps.json`, 55 groups for
//! the eight features of the target (roadmap, M8 target "Wikipedia"):
//! 1. the Appearance radios (color theme, text size, page width) with the
//!    root classes and the client preference cookie;
//! 2. pin and unpin of the table of contents, Appearance, Tools and Main
//!    menu, also from their dropdowns;
//! 3. the collapse of the table of contents by click, `Enter` and `Space`;
//! 4. a collapsible table and a navbox by click, `Enter` and `Space`;
//! 5. the scroll to 2,000 px and a wheel turn, with the sticky containers
//!    (the sticky header itself is shown to logged-in users only, so the
//!    logged-out fixture cannot test it, in Chromium either);
//! 6. the hover on the link `Web_page`: the link preview (Popups) with the
//!    recorded REST summary and image (reference previews are off for
//!    logged-out users, so the link preview stands in for them), then the
//!    pointer leaves the link;
//! 7. the search box (Codex typeahead, Vue 3): the click shows the
//!    recorded `morelike:` suggestions, typing "web" the recorded
//!    `search/title` results; `ArrowDown` and `ArrowUp` (also the wrap to
//!    the footer item), `Escape`, a click into the box again and a click
//!    outside. Only the text "web" works: the fixture has the responses
//!    for that query only, and the test never opens a result;
//! 8. a click on table-of-contents links (the anchor scroll with
//!    `scroll-margin-top`), then back and forward with `hashchange` and
//!    `popstate`.
//!
//! After the load and after each group the test reads one JSON record of
//! the page (`data/wikipedia-web-browser.js`: root and body classes, the
//! cookie, the pinned and unpinned containers with their rectangles, the
//! dropdown checkboxes, the table of contents state, the collapsible
//! elements, the sticky header and containers, the popups, the hash, the
//! scroll position, the `hashchange` and `popstate` log, the focus, the
//! search box classes, the theme colors and the script errors) and compares
//! it with Chromium's record of the same steps. Rectangles are compared
//! with a tolerance of 3 px (swb lays some boxes out 1 to 3 px off, see
//! [`TOLERANCE`]); everything else, also the scroll position, exactly.
//!
//! Since feature 12b the records have the `VisualEditor` root classes
//! (`ve-available`), its accesskey link in the body, the total length of
//! the CSS in the `style` elements of `head` (not their number: it depends
//! on the frames between the module loads), the absence of the SVG
//! fallback of Popups and the computed `clip-path` of the popup container.
//! The rectangles below the references are recorded only while the
//! references fit one column (swb has no multi-column layout: with the
//! table of contents or the Appearance panel unpinned, the content is wide
//! enough for two columns and everything below the references is 271 px
//! higher in Chromium).
//!
//! Since feature 12d the records have the link preview (`pvr`: the
//! rectangles of its container, image, extract, first paragraph, footer,
//! settings button and first bold word; `pvt`: `aria-hidden`, the text of
//! the extract, the title of the settings button, and the image's
//! `complete` flag, natural size and file name) and the typeahead (`tar`:
//! the rectangles of the Codex root, input, menu, each item, thumbnail,
//! label and description and the footer text; `tat`: the root classes, the
//! `aria-*` and `role` attributes of the input, its value and selection,
//! the menu's class, display and listbox attributes, and per item the id,
//! the highlight class, `aria-selected`, title, label, description,
//! whether the thumbnail is an image (with its file name) or the
//! placeholder, and the link). The step groups after the typing are
//! `ArrowDown` twice, `ArrowUp` three times (the second goes to the footer
//! item: Codex wraps and puts the highlighted title in the input), `Escape`
//! (the menu closes, the input keeps its value), a click into the box
//! (the menu stays closed) and a click outside.
//!
//! What the logged-out fixture cannot test, in Chromium either: the sticky
//! header (shown to logged-in users only) and reference previews (off for
//! logged-out users). The search results are the recorded responses of
//! the fixture (`rest.php/v1/search/page?q=morelike:Web_browser` on focus,
//! `search/title?q=web` after typing), so they test the code path, not
//! the live search. The thumbnails of the menu are CSS background images,
//! so the records show only their URLs; the pixel check covers their
//! decoding and drawing.
//!
//! Known differences from Chromium, which the records do not show:
//! `box-shadow` and `text-overflow: ellipsis` are not implemented (roadmap
//! backlog), so swb draws no shadow around the preview, the menu and the
//! focused input, and cuts a long description of a menu item at the edge
//! without "..."; swb's text caret is on at the moment of a screenshot,
//! Chromium's is in its blink phase. The pixel points avoid those places.
//!
//! Since feature 12c the test also reads pixels (the dark theme: `filter`
//! and gradients), since 12d also for the preview and the menu. After the
//! groups "theme night" and "theme day" it takes a screenshot of the
//! viewport and compares the colors at 23 points with Chromium's, within
//! [`PIXEL_TOLERANCE`] per channel: the five points of the wordmark of the
//! logo (white in the dark theme, black in the light one: `filter:
//! invert(1) ...`), the four of the thumbnail (dimmed by `filter:
//! brightness(0.8)`, so 204 and not 255 in the dark theme), the three of
//! the video poster (not filtered), the three of the band at the bottom of
//! each of the two sticky containers (`linear-gradient(rgba(255, 255, 255,
//! 0), var(--background-color-base))`: the color of the page, no gray band,
//! in both themes) and five areas of the page. Feature 12d adds five groups
//! with their own points (`group_points` in the points file): "hover link"
//! (five points in the preview's image, two in its white text area), "type
//! search" (three in the white of the menu, three in the photo of the
//! second item, two in the WebAssembly logo, two in a thumbnail with the
//! placeholder icon, two in the diagram of the Web crawler item), "arrow
//! down" (three in the highlighted first item, `rgb(234, 236, 240)`),
//! "arrow up 2" (two in the highlighted footer item) and "escape search"
//! (three in the page under the closed menu). The points and the groups are
//! in `data/wikipedia-web-browser-pixel-points.json`; Chromium's colors in
//! `data/wikipedia-web-browser-pixels.json` (no image is committed):
//! `swbtools replay wikipedia-web-browser --steps
//! crates/automation/tests/data/wikipedia-web-browser-steps.json --setup
//! crates/automation/tests/data/wikipedia-web-browser.js --record
//! 'window.__s()' --pixels
//! crates/automation/tests/data/wikipedia-web-browser-pixel-points.json
//! --pixels-out
//! crates/automation/tests/data/wikipedia-web-browser-pixels.json --out
//! crates/automation/tests/data/wikipedia-web-browser-chromium.json`
//! (three equal runs).
//!
//! Where the Chromium values come from: Chromium 148.0.7778.96 (headless
//! shell, Playwright) replayed the same fixture with the same page clock,
//! ran the steps and evaluated the setup script and `window.__s()` after
//! the load and after each group (the command above, a 300 ms pause after
//! each step); the 56 records are in
//! `data/wikipedia-web-browser-chromium.json`. The fixture needs no entry
//! for the typed text "web" beyond those recorded: other text would need
//! other entries.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{console_errors, fixture, load_at_capture, on_page_thread, step_groups, value};
use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::Page;

/// The record of the page (`window.__s()`).
fn record(page: &mut Page) -> Value {
    let text = value(page, "window.__s()");
    serde_json::from_str(text.as_str().unwrap()).unwrap()
}

/// The keys of the record whose values hold rectangles (compared with
/// [`TOLERANCE`]).
const RECTANGLES: &[&str] = &[
    "pin", "pinbtn", "ddc", "coll", "sticky", "pv", "pvr", "tar", "spc", "content",
];

/// The largest difference in px between two rectangle values. swb lays
/// some boxes out 1 px off (fractional layout units); with the small text
/// size, the collapsible tables 5,700 px down the page are 3 px off
/// (backlog in `docs/roadmap.md`).
const TOLERANCE: i64 = 3;

/// The largest difference of a color channel between swb's pixel and
/// Chromium's. The points are in flat areas, in the edges of the wordmark
/// (anti-aliased, equal within 6) and in gradients (within 3).
const PIXEL_TOLERANCE: i32 = 12;

#[test]
fn numbers_in_rectangle_texts() {
    assert_eq!(
        common::numbers("vector-toc in vector-toc-pinned-container 48,-136,176,360 block"),
        (
            "vector-toc in vector-toc-pinned-container #,#,#,# block".to_owned(),
            vec![48, -136, 176, 360]
        )
    );
    let same =
        |a: &str, b: &str, rectangles| common::same(&a.into(), &b.into(), rectangles, TOLERANCE);
    assert!(same("a 1,2", "a 4,0", true));
    assert!(!same("a 1,2", "a 5,2", true));
    assert!(!same("a 1,2", "b 1,2", true));
    assert!(!same("a 1,2", "a 1,3", false));
}

#[test]
fn wikipedia_web_browser_works_with_javascript_on() {
    let Some(dir) = fixture("wikipedia-web-browser") else {
        eprintln!("skipped: fixtures/js/wikipedia-web-browser is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/wikipedia-web-browser-chromium.json")).unwrap();
        let mut page = load_at_capture(&dir, Duration::from_millis(1500));
        assert!(page.has_script_runtime());
        // Chromium has no script error at load.
        assert_eq!(console_errors(&page), Vec::<String>::new());
        assert_eq!(value(&mut page, "document.readyState"), "complete");
        let setup = include_str!("data/wikipedia-web-browser.js");
        assert_eq!(value(&mut page, setup), 1);

        let mut problems = common::differences(
            "load",
            &record(&mut page),
            &expected[0],
            RECTANGLES,
            TOLERANCE,
        );
        let groups = step_groups(include_str!("data/wikipedia-web-browser-steps.json"));
        assert_eq!(groups.len() + 1, expected.len());
        let (pixel_groups, points) =
            common::pixel_points(include_str!("data/wikipedia-web-browser-pixel-points.json"));
        let pixels: Value =
            serde_json::from_str(include_str!("data/wikipedia-web-browser-pixels.json")).unwrap();
        let mut sampled = 0;
        for (index, (name, steps)) in groups.iter().enumerate() {
            run_steps(&mut page, steps).unwrap();
            let got = record(&mut page);
            problems.extend(common::differences(
                name,
                &got,
                &expected[index + 1],
                RECTANGLES,
                TOLERANCE,
            ));
            if let Some(at) = pixel_groups.iter().position(|g| g == name) {
                problems.extend(common::pixel_differences(
                    name,
                    &mut page,
                    &points[at],
                    &pixels[name.as_str()],
                    PIXEL_TOLERANCE,
                ));
                sampled += 1;
            }
        }
        assert_eq!(
            sampled,
            pixel_groups.len(),
            "a group of the pixel check is missing"
        );
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        // No script error after the steps either (the records' `e` list
        // the uncaught ones; this is the console).
        assert_eq!(console_errors(&page), Vec::<String>::new());
    });
}
