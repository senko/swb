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
//! M10 9c adds three tests for the interactions (items 3 to 8 and 10):
//! [`github_interactions_match_chromium`] (the header menus, the Code
//! popover, the branch selector, "Go to file" and the "..." button),
//! [`github_tab_order_matches_chromium`] (75 Tab stops) and
//! [`github_navigation_matches_chromium`] (13 clicks that navigate). Their
//! data are `data/github-swb-interactions-*`, `-tab-*` and `-nav-*`;
//! `tools/github_swb_records.py` makes them from three Chromium runs each.
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

use common::{
    PIXEL_TOLERANCE, Recording, fixture, load_at_capture_with, on_page_thread, step_groups, value,
};
use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{ConsoleLevel, Page};
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

/// The keys of the interaction record (`data/github-swb-interactions.js`)
/// whose values hold rectangles, compared with [`TOLERANCE`]. The other keys
/// compare exactly.
const INTERACTION_RECTANGLES: &[&str] =
    &["activeRect", "bodyTail", "dialogs", "inputs", "popovers"];

/// The prefix of the URLs of the page's worker chunks. Chromium requests
/// them (its `Worker` exists, the chunk is not in the fixture); swb's
/// `Worker` is a stand-in that fetches and runs nothing (M12 feature 2), so
/// the page takes its worker path, gets no answer and requests nothing.
const WORKER_CHUNKS: &str = "https://github.com/assets-cdn/worker/";

/// The page's warning when `Worker` does not exist. Since M12 feature 2 swb
/// has a `Worker` and the page no longer logs it (as in Chromium).
const NO_WORKERS: &str = "Web workers are not available.";

/// The record `window.__i()` of the page.
fn interaction_record(page: &mut Page) -> Value {
    let text = value(page, "window.__i()");
    serde_json::from_str(text.as_str().unwrap()).unwrap()
}

/// The record `window.__t()` of the page (the focused element).
fn tab_record(page: &mut Page) -> Value {
    let text = value(page, "window.__t()");
    serde_json::from_str(text.as_str().unwrap()).unwrap()
}

/// A page loaded as in the first-view test, with the record script `script`
/// evaluated.
fn loaded_page(dir: &std::path::Path, script: &str) -> (Page, Arc<Recording>) {
    let fetcher = Arc::new(Recording::new(ReplayFetcher::load(dir).unwrap()));
    let mut page = load_at_capture_with(dir, Duration::from_millis(1500), fetcher.clone());
    assert!(page.has_script_runtime());
    assert_eq!(value(&mut page, script), 1);
    (page, fetcher)
}

/// The largest difference of the number of elements of the document. The
/// number varies between loads by one or two elements (the diagnosis of
/// 8b: one of ten Chromium runs had two fewer `react-app` elements, and one of
/// the runs of swb under load had one fewer); the groups differ by tens.
const COUNT_TOLERANCE: i64 = 3;

/// The differences of the record `got` from `want`: the rectangles within
/// [`TOLERANCE`], the number of elements within [`COUNT_TOLERANCE`], the rest
/// exactly (also the number of children of `body`).
fn compare_interaction(name: &str, got: &Value, want: &Value) -> Vec<String> {
    let counts = |record: &Value| -> (i64, String) {
        let text = record["counts"].as_str().unwrap();
        let (elements, children) = text.split_once(',').unwrap();
        (elements.parse().unwrap(), children.to_owned())
    };
    let mut problems = common::differences(
        name,
        &common::without(got, &["counts"]),
        &common::without(want, &["counts"]),
        INTERACTION_RECTANGLES,
        TOLERANCE,
    );
    let ((got_elements, got_children), (want_elements, want_children)) =
        (counts(got), counts(want));
    if (got_elements - want_elements).abs() > COUNT_TOLERANCE || got_children != want_children {
        problems.push(format!(
            "{name}: counts: swb {} != Chromium {}",
            got["counts"], want["counts"]
        ));
    }
    problems
}

/// What the record of swb must show after the group `name`, whatever
/// Chromium's record says (the states themselves compare with Chromium's
/// record; these checks keep the test from passing when both are empty):
/// the problems, as texts.
fn interaction_state(name: &str, got: &Value) -> Vec<String> {
    let text = |key: &str| got[key].as_str().unwrap_or_default().to_owned();
    let (expanded, active, dialogs, inputs, popovers) = (
        text("expanded"),
        text("active"),
        text("dialogs"),
        text("inputs"),
        text("popovers"),
    );
    // A click focuses a button without `:focus-visible`; Escape leaves the
    // focus on the button and shows the ring.
    let on_button = |label: &str| active.starts_with("button") && active.contains(label);
    let ring = got["focusVisible"] == true;
    let menu = |label: &str, links: u64| {
        expanded.contains(label) && got["menuLinks"] == links && on_button(label) && !ring
    };
    let closed_menu = |label: &str| expanded.is_empty() && on_button(label) && ring;
    let ok = match name {
        "platform open" => menu("Platform", 17),
        "solutions open" => menu("Solutions", 15),
        "resources switch" => menu("Resources", 16) && !expanded.contains("Solutions"),
        "open source open" => menu("Open Source", 8),
        "enterprise open" => menu("Enterprise", 4),
        "platform escape" => closed_menu("Platform"),
        "resources escape" => closed_menu("Resources"),
        "open source escape" => closed_menu("Open Source"),
        "enterprise escape" => closed_menu("Enterprise"),
        "code open" => expanded.contains("|Code") && inputs.contains("#clone-with-https"),
        "code cli tab" => inputs.contains("v=gh repo clone senko/swb"),
        "code https tab" => inputs.contains("v=https://github.com/senko/swb.git"),
        "code escape" => closed_menu("Code"),
        "branch open" => {
            dialogs.contains("Select a branch") && active.starts_with("input||Filter branches")
        }
        "branch type" => inputs.contains("v=xyz") && dialogs.contains("Select a branch"),
        "branch clear" => inputs.ends_with("v=") && dialogs.contains("Select a branch"),
        "branch tags tab" => dialogs.contains("Select a tag"),
        "branch escape" => {
            dialogs.is_empty() && active.contains("#ref-picker-repos-header-ref-selector")
        }
        "go to file click" => active.starts_with("input||Go to file") && dialogs.is_empty(),
        "go to file type" => dialogs.contains("#file-results-list") && inputs.contains("v=cr"),
        "go to file escape" => {
            dialogs.is_empty() && active.starts_with("input||Go to file") && ring
        }
        "blank click" => active.starts_with("div"),
        "t escape" => active.starts_with("body"),
        "key t" => active.starts_with("input||Go to file") && ring,
        "commit dots" => {
            popovers.contains("Open commit details")
                && expanded.contains("latest-commit-details-toggle")
        }
        "commit dots escape" => {
            popovers.is_empty() && expanded.contains("latest-commit-details-toggle")
        }
        _ => true,
    };
    if ok {
        vec![]
    } else {
        vec![format!("{name}: not the expected state: {got}")]
    }
}

/// M10 9c, items 3 to 7: the header menus, the Code popover, the branch
/// selector, "Go to file" and the "..." button of the commit line, one load
/// and 26 groups of steps (`data/github-swb-interactions-steps.json`), each
/// with the record `window.__i()` (the URL, the scroll, the focused element
/// with its rectangle, `:focus-visible` and ring, the expanded buttons, the
/// open popovers and dialogs, the element counts, the tail of `body`, the
/// visible links of the header menus and the inputs) against Chromium's
/// (`data/github-swb-interactions-chromium.json`), the pixels at 17 points
/// of the open overlays, the console, and the URLs that the fixture lacks.
/// The rectangles compare within [`TOLERANCE`].
///
/// Departures (each is in the roadmap backlog):
/// - The listbox of "Go to file" is 15 px narrower in Chromium (its classic
///   scrollbar takes space in a scroll container, swb's takes none): the
///   group `go to file type` expects 625 for 640.
/// - `box-shadow` is not painted, so the 1 px ring and the blurs around the
///   menus and the overlays are missing. The pixel points are inside the
///   overlays and at least 15 px away from their edges.
/// - Chromium places the overlays with CSS anchor positioning (`position:
///   fixed`, `z-index: 100`), swb with the page's script (`position:
///   absolute`). The boxes are equal at scroll 0, where the test opens them.
/// - swb's `Worker` runs no script: Chromium requests a worker chunk that
///   swb does not ([`WORKER_CHUNKS`]). (Before M12 feature 2 swb had no
///   `Worker` and the page logged [`NO_WORKERS`] once.)
#[test]
fn github_interactions_match_chromium() {
    let Some(dir) = fixture("github-swb") else {
        eprintln!("skipped: fixtures/js/github-swb is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/github-swb-interactions-chromium.json"))
                .unwrap();
        let groups = step_groups(include_str!("data/github-swb-interactions-steps.json"));
        assert_eq!(groups.len() + 1, expected.len());
        let (mut page, fetcher) =
            loaded_page(&dir, include_str!("data/github-swb-interactions.js"));
        let (pixel_groups, points) = common::pixel_points(include_str!(
            "data/github-swb-interactions-pixel-points.json"
        ));
        let pixels: Value =
            serde_json::from_str(include_str!("data/github-swb-interactions-pixels.json")).unwrap();
        let mut sampled = 0;
        let mut problems =
            compare_interaction("load", &interaction_record(&mut page), &expected[0]);
        for (index, (name, steps)) in groups.iter().enumerate() {
            run_steps(&mut page, steps).unwrap();
            let got = interaction_record(&mut page);
            let mut want = expected[index + 1].clone();
            if name == "go to file type" {
                let dialogs = want["dialogs"]
                    .as_str()
                    .unwrap()
                    .replace("625,122", "640,122");
                want["dialogs"] = Value::String(dialogs);
            }
            problems.extend(compare_interaction(name, &got, &want));
            problems.extend(interaction_state(name, &got));
            if let Some(at) = pixel_groups.iter().position(|g| g == name) {
                problems.extend(common::pixel_differences(
                    name,
                    &mut page,
                    &points[at],
                    &pixels[name],
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

        // The console: the page's errors (the rejections of the requests
        // that the fixture lacks) are Chromium's, in number; the page does
        // not warn about missing workers (swb has the stand-in), and
        // the only other message is the warning of the stand-in, which is
        // not on the page's console.
        let chromium: Vec<String> =
            serde_json::from_str(include_str!("data/github-swb-interactions-errors.json")).unwrap();
        let messages: Vec<String> = page
            .console_messages()
            .into_iter()
            .filter(|m| matches!(m.level, ConsoleLevel::Error | ConsoleLevel::Warn))
            .map(|m| m.text)
            .collect();
        let (workers, errors): (Vec<_>, Vec<_>) = messages
            .iter()
            .partition(|text| text.starts_with(NO_WORKERS));
        assert_eq!(workers.len(), 0, "{messages:?}");
        let errors: Vec<&str> = errors
            .iter()
            .map(|text| {
                text.strip_prefix("Uncaught (in promise) TypeError: ")
                    .unwrap_or(text)
            })
            .collect();
        let chromium: Vec<&str> = chromium
            .iter()
            .map(|text| text.strip_prefix("exception: ").unwrap_or(text))
            .collect();
        assert_eq!(errors, chromium, "console errors");

        // The URLs that the fixture lacks (the POST of the clone protocol,
        // the tag list, the file list, the image of the Copilot app).
        let chromium: Vec<String> =
            serde_json::from_str(include_str!("data/github-swb-interactions-missing.json"))
                .unwrap();
        let chromium: Vec<String> = chromium
            .into_iter()
            .filter(|url| !url.starts_with(WORKER_CHUNKS))
            .collect();
        assert_eq!(fetcher.missing(), chromium, "URLs that the fixture lacks");
    });
}

/// M10 9c, item 10: the Tab order. 70 Tab and 5 Shift+Tab presses from the
/// load (`data/github-swb-tab-steps.json`), each with the record
/// `window.__t()`: the focused element (tag, test id, label, type, link and
/// text), its rectangle, `:focus-visible`, the focus ring (style, width,
/// color, offset) and the scroll position, against Chromium's
/// (`data/github-swb-tab-chromium.json`). All 76 records are equal.
///
/// The rectangles and the scroll are relative to the header (see
/// `data/github-swb-tab.js`): after the search button has the focus, the
/// page's header island retries lazy chunks that the fixture lacks and then
/// shows an error heading (+14 px), at a stop of the order that differs from
/// run to run in both browsers.
#[test]
fn github_tab_order_matches_chromium() {
    let Some(dir) = fixture("github-swb") else {
        eprintln!("skipped: fixtures/js/github-swb is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/github-swb-tab-chromium.json")).unwrap();
        let groups = step_groups(include_str!("data/github-swb-tab-steps.json"));
        assert_eq!(groups.len(), 75);
        assert_eq!(groups.len() + 1, expected.len());
        let (mut page, _) = loaded_page(&dir, include_str!("data/github-swb-tab.js"));
        let mut problems =
            common::differences("load", &tab_record(&mut page), &expected[0], &[], 0);
        let mut visited = Vec::new();
        for (index, (name, steps)) in groups.iter().enumerate() {
            run_steps(&mut page, steps).unwrap();
            let got = tab_record(&mut page);
            problems.extend(common::differences(
                name,
                &got,
                &expected[index + 1],
                &[],
                0,
            ));
            // A zero-size element in the order is not an element that the
            // page shows (the intermittent `More items` button of 9c).
            assert!(
                !got["rect"].as_str().unwrap().ends_with(",0,0"),
                "{name}: {got}"
            );
            visited.push(got["active"].as_str().unwrap().to_owned());
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        // The order itself: the skip link, the logo, the five menu buttons,
        // "Pricing", the search button, "Sign in" and "Sign up", and the rest.
        assert!(visited[0].contains("Skip to content"));
        assert!(visited[8].contains("Search or jump to"));
        assert!(visited[9].contains("/login"));
        assert!(visited[10].contains("/signup"));
        assert!(visited[16].contains("Pull requests"));
        assert!(visited[68].contains("THIRD_PARTY_NOTICES.md"));
        assert!(visited[74].contains("LICENSE"));
    });
}

/// The URL that Chromium's error page has after a navigation to a URL that the
/// fixture lacks (the replay fails the request).
const CHROMIUM_ERROR_PAGE: &str = "chrome-error://chromewebdata/";

/// One case of [`github_navigation_matches_chromium`]: the problems, and
/// whether the click left the page (in Chromium).
fn navigation_case(
    dir: &std::path::Path,
    name: &str,
    steps: &[Value],
    want: &Value,
) -> (Vec<String>, bool) {
    let mut problems = Vec::new();
    let (mut page, fetcher) = loaded_page(dir, "1");
    assert!(fetcher.missing().is_empty(), "{name}: the load lacks URLs");
    run_steps(&mut page, steps).unwrap();
    let want_missing: Vec<String> = want["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|url| url.as_str().unwrap().to_owned())
        .collect();
    if fetcher.missing() != want_missing {
        problems.push(format!(
            "{name}: URLs that the fixture lacks: swb {:?} != Chromium {want_missing:?}",
            fetcher.missing()
        ));
    }
    let after = want["records"][1]["url"].as_str().unwrap();
    if after == CHROMIUM_ERROR_PAGE {
        let url = page.url().unwrap().to_string();
        if !want_missing.contains(&url) {
            problems.push(format!("{name}: swb is at {url}, not at a requested URL"));
        }
        return (problems, true);
    }
    let url = value(&mut page, "location.href");
    let url = url.as_str().unwrap();
    if url != after {
        problems.push(format!("{name}: URL: swb {url} != Chromium {after}"));
    }
    (problems, false)
}

/// M10 9c, item 8: the 13 links and buttons that navigate. Each case is a
/// load of its own with one click (`data/github-swb-nav-steps.json`): the
/// notification, Fork and Star buttons (a sign-in page), "Sign in", "Sign
/// up", the tabs Issues and Pull requests, the branch, commit and message
/// links, a folder and a file of the list (the page fetches the new view
/// and changes the URL itself), and the anchor of the README. The URLs that
/// the fixture lacks (what the click requested), compared as sets as
/// `bbc.rs` does, and the URL of the page after the click equal Chromium's
/// (`data/github-swb-nav-chromium.json`). A click that leaves the page ends
/// in Chromium's error page (the replay fails the request); swb's URL is
/// then the requested one, which is in the set.
#[test]
fn github_navigation_matches_chromium() {
    let Some(dir) = fixture("github-swb") else {
        eprintln!("skipped: fixtures/js/github-swb is missing");
        return;
    };
    let expected: Value =
        serde_json::from_str(include_str!("data/github-swb-nav-chromium.json")).unwrap();
    let cases = step_groups(include_str!("data/github-swb-nav-steps.json"));
    assert_eq!(cases.len(), 13);
    assert_eq!(expected.as_object().unwrap().len(), 13);
    // The cases are independent loads: four at a time, each on a thread with
    // the stack of the page thread.
    let mut problems = Vec::new();
    let mut leaving = 0;
    for chunk in cases.chunks(4) {
        let runs: Vec<_> = chunk
            .iter()
            .map(|(name, steps)| {
                let (dir, name, steps) = (dir.clone(), name.clone(), steps.clone());
                let want = expected[name.as_str()].clone();
                std::thread::Builder::new()
                    .stack_size(16 << 20)
                    .spawn(move || navigation_case(&dir, &name, &steps, &want))
                    .unwrap()
            })
            .collect();
        for run in runs {
            let (found, left) = run.join().expect("a navigation case panicked");
            problems.extend(found);
            leaving += usize::from(left);
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    // The buttons of the header and the repository, "Sign in" and "Sign up"
    // leave the page; the tabs and links are soft navigations or loads that
    // the fixture lacks (compared above).
    assert!(leaving >= 6, "{leaving} cases leave the page");
}
