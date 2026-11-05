//! M9 feature 8 for the BBC home page with JavaScript on: the local fixture
//! `fixtures/js/bbc` (git-ignored; set `SWB_JS_FIXTURES` to the directory
//! `fixtures/js` of another checkout), in one load with all steps in order,
//! with the page clock fixed at the `captured` time of the fixture, as the
//! tools fix it, plus a second, short load for the cookie banner's settings
//! link. The test skips when the fixture is missing.
//!
//! The steps are in `data/bbc-steps.json`, 26 groups for the items 2 to 8 of
//! the target (roadmap, M9, "What works means"):
//! 2. the cookie banner: "Yes, I agree" closes it and sets the cookies
//!    `ckns_explicit=2` and `ckns_policy=111` (they were `0` and `000`);
//!    "No, take me to settings" follows its link: the second load
//!    (`data/bbc-settings-steps.json`) clicks it and the test checks the URL
//!    that swb requests;
//! 3. the menu button opens the navigation panel (the search field is
//!    enabled, the body does not scroll, the page scrolls to the search
//!    field); a click on "News" expands the section with its 23 links;
//!    `Escape` closes the panel and focuses the button, the button opens it
//!    again and closes it;
//! 4. the carousels: the arrow Next slides the row to its end (`scrollLeft`
//!    1088 of a row of 2336 px in a scroller of 1248 px) and turns itself
//!    off while Previous turns on; Previous slides it back; the same for the
//!    second carousel (a row of 2244 px);
//! 5. "Save" opens the popover "Sign in to save for later" with Sign In and
//!    Register; `Escape` closes it;
//! 6. Register and Sign In navigate to `session.bbc.com`: the click on
//!    Register is recorded by a listener of the page that stops the
//!    navigation (the test follows one navigation per load), the click on
//!    Sign In is the last step, and the test checks the URL that swb
//!    requests;
//! 7. the language dropdown opens its list (a box of 1232 px by 550 px) and
//!    a second click closes it;
//! 8. the lazy images: eight wheel steps down the page (the groups `wheel 1`
//!    to `wheel 8`, 2000 px, 300 px, 1700 px and five of 2000 px; the page
//!    ends at 12,533 px); after each group the record has the number of
//!    images (122 at the load, 71 at the end), the grey placeholders that
//!    the page replaces when a card's image has loaded (51 at the load, 0
//!    from `wheel 6` on), the loaded photos (20 to 71) and the requested
//!    image URLs (`res`).
//!
//! After the load and after each group the test reads one JSON record of
//! the page (`data/bbc.js`: the focused element, the cookies, the banner,
//! the rectangles of the header's menu button, Register and Sign In, the
//! navigation panel, the first cards, the carousels, the popover, the
//! language list and the image counts) and compares it with Chromium's
//! record of the same steps. Rectangles compare within 3 px
//! ([`TOLERANCE`]), everything else exactly. The record has nothing that
//! depends on the ads, the analytics or the Optimizely experiments, and no
//! time (`ckns_policy_exp`).
//!
//! The consent frame (feature 1 of the list): Chromium's replay shows the
//! Sourcepoint dialog in a cross-origin frame of 1280x800 over the page.
//! swb has no frames yet (M9 features 10 to 15), so there the page has no
//! dialog and pointer input reaches the page. The first group, `consent`,
//! clicks "I agree" in the frame in Chromium (the frame sends `sp.hideMessage`
//! and the page removes it); the test skips it, and compares swb's record at
//! the load with Chromium's record after the group (the same page state:
//! scroll position 0, focus on the body, `overflow` visible, the banner and
//! the header where they are), and with Chromium's record at the load except
//! for the focused element (the frame). Feature 15 of the milestone runs the
//! group in swb too and adds item 1.
//!
//! Scroll anchoring (CSS Scroll Anchoring) is not implemented in swb. In
//! Chromium the first wheel step scrolls 16 px less than the wheel: the
//! content above the cards moves up by 16 px while the page scrolls, and
//! Chromium keeps its anchor node where it is. That moves the lazy images
//! by one at the limit of the distance. The group `anchoring off` sets
//! `overflow-anchor: none` on the root in both browsers (Chromium then
//! scrolls by the wheel's distance, as swb does; measured), so that both
//! run the same page state. `tools/probes/web-bbc-gaps.json` has the
//! Chromium values of scroll anchoring for the day swb implements it.
//!
//! Chromium's replay logs two errors, both after the click in the consent
//! frame (the consent choice requests are not in the fixture): `console.error:
//! ERROR in messagingWithoutDetection: RequestError: Server request error: 0`
//! from `wrapperMessagingWithoutDetection.js`, and the exception `Failed to
//! fetch`. swb has no console error at all: it skips the group.
//!
//! Where the Chromium values come from: Chromium 148.0.7778.96 (headless
//! shell, Playwright) replayed the same fixture with the same page clock,
//! ran the steps and evaluated the setup script and `window.__s()` after the
//! load and after each group (`swbtools replay bbc --steps
//! crates/automation/tests/data/bbc-steps.json --setup
//! crates/automation/tests/data/bbc.js --record ... --pixels
//! crates/automation/tests/data/bbc-pixel-points.json --pixels-out
//! crates/automation/tests/data/bbc-pixels.json --out
//! crates/automation/tests/data/bbc-chromium.json`, a 300 ms pause after each
//! step; three equal runs; the record expression is in docs/testing.md). The
//! expression adds the key `res`, the sorted `ichef` image URLs of the
//! resource timing entries; the test adds the same list from its recording
//! fetcher.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use common::{
    Recording, console_errors, fixture, load_at_capture_with, on_page_thread, step_groups, value,
};
use serde_json::{Value, json};
use swb_automation::run_steps;
use swb_engine::Page;
use swb_net::ReplayFetcher;

/// The keys of the record whose values hold rectangles (compared with
/// [`TOLERANCE`]).
const RECTANGLES: &[&str] = &[
    "banner",
    "cards",
    "headerRects",
    "language",
    "news",
    "newsButton",
    "panel",
    "popover",
    "search",
];

/// The largest difference in px between two rectangle values.
const TOLERANCE: i64 = 3;

/// The largest difference of a color channel between swb's pixel and
/// Chromium's. The points are in flat areas.
const PIXEL_TOLERANCE: i32 = 12;

/// The URL that the link "No, take me to settings" of the cookie banner
/// navigates to.
const COOKIE_SETTINGS: &str =
    "https://www.bbc.com/usingthebbc/cookies/how-can-i-change-my-bbc-cookie-settings/";

/// The URL that the button Sign In navigates to (`ptrt` is the page).
const SIGN_IN: &str =
    "https://session.bbc.com/session?userOrigin=BBCS_BBC&ptrt=https%3A%2F%2Fwww.bbc.com%2F";

/// The Register link that the listener of the group `register` saw.
const REGISTER: &str = "https://session.bbc.com/session?action=register&userOrigin=BBCS_BBC&ptrt=https%3A%2F%2Fwww.bbc.com%2F";

/// The record of the page (`window.__s()`) with the key `res`: the sorted
/// image URLs of the `ichef` hosts that the page requested so far.
fn record(page: &mut Page, fetcher: &Recording) -> Value {
    let text = value(page, "window.__s()");
    let mut record: Value = serde_json::from_str(text.as_str().unwrap()).unwrap();
    let requested: BTreeSet<String> = fetcher
        .urls()
        .into_iter()
        .filter(|url| url.starts_with("https://ichef."))
        .collect();
    record["res"] = json!(requested);
    record
}

/// `record` without the keys in `skip`.
fn without(record: &Value, skip: &[&str]) -> Value {
    let mut record = record.clone();
    for key in skip {
        record.as_object_mut().unwrap().remove(*key);
    }
    record
}

/// The keys of `got` and `expected` that differ, as `name: key: swb ..
/// != Chromium ..`.
fn differences(name: &str, got: &Value, expected: &Value) -> Vec<String> {
    common::differences(name, got, expected, RECTANGLES, TOLERANCE)
}

#[test]
fn bbc_works_with_javascript_on() {
    let Some(dir) = fixture("bbc") else {
        eprintln!("skipped: fixtures/js/bbc is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/bbc-chromium.json")).unwrap();
        let fetcher = Arc::new(Recording::new(ReplayFetcher::load(&dir).unwrap()));
        let mut page = load_at_capture_with(&dir, Duration::from_millis(1500), fetcher.clone());
        assert!(page.has_script_runtime());
        assert_eq!(console_errors(&page), Vec::<String>::new());
        assert_eq!(value(&mut page, "document.readyState"), "complete");
        assert_eq!(value(&mut page, include_str!("data/bbc.js")), 1);

        let (pixel_groups, points) =
            common::pixel_points(include_str!("data/bbc-pixel-points.json"));
        let pixels: Value = serde_json::from_str(include_str!("data/bbc-pixels.json")).unwrap();
        let mut sampled = 0;
        let mut sample = |name: &str, page: &mut Page, problems: &mut Vec<String>| {
            if let Some(at) = pixel_groups.iter().position(|g| g == name) {
                problems.extend(common::pixel_differences(
                    name,
                    page,
                    &points[at],
                    &pixels[name],
                    PIXEL_TOLERANCE,
                ));
                sampled += 1;
            }
        };

        // The load: the same page as Chromium's after the group `consent`
        // (no frame in swb), and as Chromium's at the load except for the
        // focused element (the frame).
        let at_load = record(&mut page, &fetcher);
        let mut problems = differences(
            "load",
            &without(&at_load, &["active"]),
            &without(&expected[0], &["active"]),
        );
        let groups = step_groups(include_str!("data/bbc-steps.json"));
        assert_eq!(groups.len() + 1, expected.len());
        for (index, (name, steps)) in groups.iter().enumerate() {
            let expected = &expected[index + 1];
            if name == "consent" {
                problems.extend(differences(name, &at_load, expected));
                continue;
            }
            run_steps(&mut page, steps).unwrap();
            if name == "sign in" {
                // The page navigates away: Chromium's record is the one of
                // the error page.
                assert_eq!(expected["navigated"], true);
                assert_eq!(page.url().unwrap().as_str(), SIGN_IN);
                continue;
            }
            let got = record(&mut page, &fetcher);
            if name == "register" {
                assert_eq!(got["clicked"], REGISTER);
            }
            // The banner's script hides its container with `display: none`
            // while the button "Yes, I agree" has the focus. Chromium moves
            // the focus to the body after the next frame (HTML "focus
            // fixup rule" for an element that is no longer rendered); swb
            // keeps it (departure; `focus-fixup-*` cases of
            // `tools/probes/web-bbc-gaps.json`).
            let skip: &[&str] = if name == "banner agree" {
                &["active"]
            } else {
                &[]
            };
            problems.extend(differences(
                name,
                &without(&got, skip),
                &without(expected, skip),
            ));
            sample(name, &mut page, &mut problems);
        }
        assert_eq!(
            sampled,
            pixel_groups.len(),
            "a group of the pixel check is missing"
        );
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        // No script error after the steps either (the console lists each
        // message once, so these are the new ones).
        assert_eq!(console_errors(&page), Vec::<String>::new());
    });
}

/// "No, take me to settings" of the cookie banner follows its link (a second
/// load: the page navigates away). The test checks the URL that swb requests.
#[test]
fn bbc_cookie_banner_settings_link_navigates() {
    let Some(dir) = fixture("bbc") else {
        eprintln!("skipped: fixtures/js/bbc is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/bbc-settings-chromium.json")).unwrap();
        let fetcher = Arc::new(Recording::new(ReplayFetcher::load(&dir).unwrap()));
        let mut page = load_at_capture_with(&dir, Duration::from_millis(1500), fetcher.clone());
        assert_eq!(console_errors(&page), Vec::<String>::new());
        let groups = step_groups(include_str!("data/bbc-settings-steps.json"));
        assert_eq!(groups.len() + 1, expected.len());
        for (name, steps) in &groups {
            if name == "consent" {
                continue;
            }
            run_steps(&mut page, steps).unwrap();
        }
        assert_eq!(expected.last().unwrap()["navigated"], true);
        assert_eq!(page.url().unwrap().as_str(), COOKIE_SETTINGS);
        assert!(fetcher.urls().iter().any(|url| url == COOKIE_SETTINGS));
    });
}
