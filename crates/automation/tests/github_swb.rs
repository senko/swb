//! M10 feature 8b: the first view of github.com/senko/swb with JavaScript
//! on, against Chromium's record of the same load. The local fixture is
//! `fixtures/js/github-swb` (git-ignored; set `SWB_JS_FIXTURES` to the
//! directory `fixtures/js` of another checkout). The page loads at 1280 x 800
//! with the page clock fixed at the `captured` time of the fixture, as the
//! tools fix it. The test skips when the fixture is missing.
//! `github.rs` checks the platform parts of the same page (modules, custom
//! elements, shadow roots, the 23 relative times).
//!
//! What the test checks (M10, "What works means", items 1, 2 and 9):
//!
//! 1. The first view (item 1). After the load and 1.5 s of page time one
//!    JSON record of the page (`data/github-swb.js`, `window.__s()`) has the
//!    rectangles and the texts of: the header (logo, the five menu buttons
//!    with their state, "Pricing", the search button, "Sign in", "Sign up"
//!    and the settings button); the repository header (breadcrumbs, name,
//!    the seven tabs, Notifications, Fork and Star); the branch selector, the
//!    branch and tag counts, "Go to file" and the Code button; the
//!    latest-commit line (author, message, hash, relative time, the check
//!    icon, the commit count); the 21 rows of the file list (the row, the
//!    name, the last commit message, the relative time with its text); and
//!    the sidebar (About, Resources, Releases, Contributors). Rectangles
//!    compare within 1 px ([`TOLERANCE`]) and texts exactly. `bbc.rs` uses
//!    3 px, but swb's record equals Chromium's to the pixel since feature
//!    8a, and 3 px would hide the 3 px error that `vertical-align: top` had
//!    on the file names, the name cells and the boxes that clip the names.
//!    The record also has the state of the React islands: for each of the
//!    four `react-partial` elements and the `react-app` the name, `data-ssr`,
//!    the class `loaded`, the length of the text, the number of elements
//!    below it and the number of those that carry a React fiber (the key
//!    `__reactFiber$...` that React sets on a DOM node that it rendered or
//!    hydrated). That the counts are equal in both browsers shows that React
//!    hydrated the server-rendered islands (`marketing-header`, `global-nav-bar`,
//!    `code-view`) and rendered the client-only ones, and that the 8 `fetch`
//!    calls of the page rendered their JSON (the latest commit, the counts,
//!    the file list with its commit messages, the sidebar). The counts of the
//!    `react-app` compare within 3 elements: in one of ten Chromium runs it
//!    had two fewer (a timing difference). The pixels at 42 points of the
//!    first screen (`data/github-swb-pixel-points.json`: flat areas, borders
//!    and icons that both browsers paint the same; the colors of Chromium's
//!    screenshot are in `data/github-swb-pixels.json`) compare within
//!    [`PIXEL_TOLERANCE`].
//! 2. The console (item 2). swb has no console message at the level `error`
//!    or `warn`. Chromium's replay has one line
//!    (`data/github-swb-console.json`): the network log of the 404 of
//!    `_global-navigation/payloads.json`. It is a message of the browser, not
//!    of the page (no script sees it; neither `console.error` nor an `error`
//!    event of the page ran: the diagnosis of feature 8 wrapped them), and swb
//!    does not write such lines. The test takes Chromium's page messages as
//!    the lines that are not `Failed to load resource`, requires that swb
//!    has no message that they do not list, and checks that swb requested
//!    `payloads.json`.
//! 3. The relative times (item 9) are in `github.rs` (`rendered_times`: the
//!    texts of the 23 elements against Chromium's replay). This test only
//!    compares the texts of the rows and of the commit line.
//! 4. The time (item 12) is no check: tests run in the dev profile. The
//!    release numbers are in the devlog of feature 8b.
//!
//! The record has nothing that differs between two runs of Chromium: no React
//! `useId` value (`_r_..._`: Chromium's React makes three more client ids
//! than swb's; backlog), no inline `anchor-name` style (the page sets it only
//! where CSS anchor positioning exists; backlog), no time.
//!
//! Where the Chromium values come from: Chromium 148.0.7778.96 (headless
//! shell, Playwright) replayed the same fixture with the same page clock
//! (`swbtools replay github-swb --steps
//! crates/automation/tests/data/github-swb-steps.json --setup
//! crates/automation/tests/data/github-swb.js --record 'window.__s()'
//! --pixels crates/automation/tests/data/github-swb-pixel-points.json
//! --pixels-out crates/automation/tests/data/github-swb-pixels.json
//! --console-out FILE --out crates/automation/tests/data/github-swb-chromium.json`;
//! three equal runs; the steps file has no group, the record is the state
//! after the load).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Recording, fixture, load_at_capture_with, on_page_thread, value};
use serde_json::Value;
use swb_engine::ConsoleLevel;
use swb_net::ReplayFetcher;

/// The keys of the record whose values hold rectangles (compared with
/// [`TOLERANCE`]), and the page size.
const RECTANGLES: &[&str] = &[
    "actionLinksRects",
    "actionsRects",
    "branchBarRects",
    "breadcrumbsRects",
    "checksRects",
    "commitBoxRects",
    "commitRects",
    "headerButtonsRects",
    "logoRects",
    "menusRects",
    "messagesRects",
    "nameCellsRects",
    "nameClipsRects",
    "namesRects",
    "pricingRects",
    "repoNameRects",
    "rowsRects",
    "sidebarRects",
    "size",
    "tabsRects",
    "timesRects",
];

/// The largest difference in px between two rectangle values.
const TOLERANCE: i64 = 1;

/// The largest difference of a color channel between swb's pixel and
/// Chromium's. The points are in flat areas, on borders and on icons.
const PIXEL_TOLERANCE: i32 = 12;

/// The path of the request whose 404 Chromium logs.
const PAYLOADS: &str = "https://github.com/_global-navigation/payloads.json";

/// The `fetch` calls of the page during the load, in their order (both
/// browsers, the diagnosis of feature 8): the start of the URL after
/// `https://github.com/`.
const FETCHES: &[&str] = &[
    "senko/swb/latest-commit",
    "senko/swb/recently-touched-branches",
    "senko/swb/branch-and-tag-count",
    "senko/swb/tree-commit-info",
    "senko/swb/refs?type=branch",
    "senko/swb/overview-files/main",
    "senko/swb/_sidebar",
    "_global-navigation/payloads.json",
];

#[test]
fn github_first_view_matches_chromium() {
    let Some(dir) = fixture("github-swb") else {
        eprintln!("skipped: fixtures/js/github-swb is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/github-swb-chromium.json")).unwrap();
        assert_eq!(expected.len(), 1, "the load is the only record");
        let fetcher = Arc::new(Recording::new(ReplayFetcher::load(&dir).unwrap()));
        let mut page = load_at_capture_with(&dir, Duration::from_millis(1500), fetcher.clone());
        assert!(page.has_script_runtime());
        assert_eq!(value(&mut page, "document.readyState"), "complete");
        assert_eq!(value(&mut page, include_str!("data/github-swb.js")), 1);

        // The record: rectangles within the tolerance, the rest exactly.
        let text = value(&mut page, "window.__s()");
        let got: Value = serde_json::from_str(text.as_str().unwrap()).unwrap();
        // A part with no element would equal Chromium's part with no element
        // (a selector that matches nothing): every part has elements, and
        // the parts of the file list have 21.
        for (key, list) in got.as_object().unwrap() {
            if let Some(part) = key.strip_suffix("Rects") {
                let want = if [
                    "rows",
                    "names",
                    "nameCells",
                    "nameClips",
                    "messages",
                    "times",
                ]
                .contains(&part)
                {
                    21
                } else {
                    list.as_array().unwrap().len()
                };
                assert_eq!(list.as_array().unwrap().len(), want, "{key}");
                assert!(want > 0, "{key} is empty");
                assert_eq!(got[format!("{part}Texts")].as_array().unwrap().len(), want);
            }
        }
        assert_eq!(got["rows"], 22, "the file list: a header row and 21 rows");
        assert_eq!(got["islands"].as_array().unwrap().len(), 4);
        // The counts of the `react-app` compare within 3 elements (see the
        // header), the rest within the tolerance.
        let counts = |record: &Value| common::without(record, &["appCounts"]);
        let problems = common::differences(
            "load",
            &counts(&got),
            &counts(&expected[0]),
            RECTANGLES,
            TOLERANCE,
        );
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        assert!(
            common::same(&got["appCounts"], &expected[0]["appCounts"], true, 3),
            "react-app counts: swb {} != Chromium {}",
            got["appCounts"],
            expected[0]["appCounts"]
        );

        // The pixels of the first screen.
        let (_, points) = common::pixel_points(include_str!("data/github-swb-pixel-points.json"));
        let pixels: Value =
            serde_json::from_str(include_str!("data/github-swb-pixels.json")).unwrap();
        let problems = common::pixel_differences(
            "load",
            &mut page,
            &points[0],
            &pixels["load"],
            PIXEL_TOLERANCE,
        );
        assert!(problems.is_empty(), "{}", problems.join("\n"));

        // The console: no error or warning that Chromium's page does not log.
        let chromium: Vec<Value> =
            serde_json::from_str(include_str!("data/github-swb-console.json")).unwrap();
        let page_messages: Vec<String> = chromium
            .iter()
            .map(|m| m["text"].as_str().unwrap().to_owned())
            .filter(|text| !text.starts_with("Failed to load resource"))
            .collect();
        let unexpected: Vec<String> = page
            .console_messages()
            .into_iter()
            .filter(|m| matches!(m.level, ConsoleLevel::Error | ConsoleLevel::Warn))
            .map(|m| m.text)
            .filter(|text| !page_messages.contains(text))
            .collect();
        assert!(unexpected.is_empty(), "console messages: {unexpected:?}");
        // The request behind Chromium's network line was made, in swb too.
        let urls = fetcher.urls();
        assert!(urls.iter().any(|url| url.starts_with(PAYLOADS)));
        // The 8 `fetch` calls of the page, in order (the page loads more
        // URLs: scripts, styles and images).
        let fetched: Vec<&str> = urls
            .iter()
            .filter_map(|url| url.strip_prefix("https://github.com/"))
            .filter(|rest| FETCHES.iter().any(|f| rest.starts_with(f)))
            .collect();
        assert_eq!(fetched.len(), FETCHES.len(), "{fetched:?}");
        for (url, start) in fetched.iter().zip(FETCHES) {
            assert!(url.starts_with(start), "{url} is not {start}");
        }
    });
}
