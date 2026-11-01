//! M9 feature 1 for Ars Technica with JavaScript on: the local fixture
//! `fixtures/js/ars-technica` (git-ignored; set `SWB_JS_FIXTURES` to the
//! directory `fixtures/js` of another checkout), in one load with all steps
//! in order, with the page clock fixed at the `captured` time of the fixture,
//! as the tools fix it. The test skips when the fixture is missing.
//!
//! The steps are in `data/ars-technica-steps.json`, 26 groups for the
//! features 1 to 6 of the target (roadmap, M9, "What works means"):
//! 1. the consent dialog "We Care About Your Privacy": "Your Privacy
//!    Choices" opens the preferences with the tabs Purposes, Features and
//!    Vendors; the vendor list request is not in the fixture and fails (in
//!    Chromium too), the dialog shows its error text and enables "I accept";
//!    "I accept" sets the cookie `fides_consent` and closes the dialog;
//! 2. the sections menu: a click opens it, `Escape` closes it, a click
//!    outside closes it;
//! 3. the theme switch: Dark, Hyperlight (light), Day & Night (dusk) and
//!    System change the colors, the class of `html`, the selected item and
//!    the `theme` cookie; the menu stays open after a pick, and a click
//!    outside closes it;
//! 4. the grid settings dialog and the sign-in dialog open and close (the
//!    sign-in form request fails, as in Chromium's replay);
//! 5. the view switcher starts its navigation: the button "Classic" sets
//!    `location.href` to `/classic-preview/`, which the fixture lacks (the
//!    test checks the URL that swb requests; Chromium shows its error
//!    page);
//! 6. the sticky header: the wheel moves the page down and up; the header
//!    hides (`top: -57px`) when the page scrolls down and comes back when
//!    it scrolls up by more than 18 px.
//!
//! After the load and after each group the test reads one JSON record of
//! the page (`data/ars-technica.js`: the class of `html`, the colors, the
//! cookies, the consent dialog with its tabs and panels, the header menus,
//! the theme items, the dialogs, the view buttons, the scroll position and
//! the header, the focused element with `:focus-visible`) and compares it
//! with Chromium's record of the same steps. Rectangles compare within 3 px
//! ([`TOLERANCE`]), everything else exactly. The record has nothing that
//! depends on the A/B headline variant (random per load in both browsers),
//! the ads or the third-party scripts.
//!
//! Not in this test yet (the roadmap's features 3 and 5 of M9 add them): the
//! story times in the form of Chromium ("5:59 PM", `Intl`) and the blurred
//! backdrop of the grid settings dialog (`backdrop-filter`, the pixels
//! around the dialog).
//!
//! Script errors: Chromium's replay has the errors of the ad scripts that
//! the fixture lacks (not swb's: it does not load them) and no
//! `document.write` error. swb has none either (M9 feature 2): the two A/B
//! headline scripts write their `ars-ajax-handler.php` script elements, and
//! the test checks that swb requests them after the scripts of the head and
//! before the tracking script at the end of the page, as Chromium's replay
//! does (the `group`, `uid` and `ts` parts are random or time-based and
//! masked).
//!
//! Where the Chromium values come from: Chromium 148.0.7778.96 (headless
//! shell, Playwright) replayed the same fixture with the same page clock,
//! ran the steps and evaluated the setup script and `window.__s()` after
//! the load and after each group (`swbtools replay ars-technica --steps
//! crates/automation/tests/data/ars-technica-steps.json --setup
//! crates/automation/tests/data/ars-technica.js --record 'window.__s ?
//! window.__s() : JSON.stringify({navigated: true})' --pixels
//! crates/automation/tests/data/ars-technica-pixel-points.json --pixels-out
//! crates/automation/tests/data/ars-technica-pixels.json --out
//! crates/automation/tests/data/ars-technica-chromium.json`, a 300 ms pause
//! after each step; three equal runs).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{console_errors, fixture, load_at_capture_with, on_page_thread, step_groups, value};
use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::Page;
use swb_net::{CookieJar, Fetcher, NetError, ReplayFetcher, Request, Response};

/// The keys of the record whose values hold rectangles (compared with
/// [`TOLERANCE`]).
const RECTANGLES: &[&str] = &["modalRect", "panelRects", "dialogRects", "headerRect"];

/// The largest difference in px between two rectangle values.
const TOLERANCE: i64 = 3;

/// The largest difference of a color channel between swb's pixel and
/// Chromium's. The points are in flat areas.
const PIXEL_TOLERANCE: i32 = 12;

/// The request of an A/B script with the random parts masked (`group`,
/// `uid`, `ts`).
const AB_REQUEST: &str = "https://arstechnica.com/services/ars-ajax-handler.php?nonce=72098c0351&action=ars_ab_impression&id=2178084&group=G&uid=U&ts=T";

/// The scripts of the site's own hosts that swb requests, in the order of
/// the document: `jquery` in the head, the two A/B scripts that the
/// headline scripts write in the body, then the scripts at the end of the
/// page.
///
/// Chromium's replay (`performance.getEntriesByType('resource')` in
/// `swbtools replay`, three runs) requests `jquery`, the five scripts at
/// the end of the page, the two A/B scripts and `cntracking`: its preload
/// scanner finds the scripts at the end before the parser reaches the
/// headline scripts (swb has no preload scanner, ADR 0028 section 8), and
/// the order of the three scripts `app`, `ads` and `stats` varies between
/// runs. The parser-driven requests agree: `jquery` first, the two A/B
/// requests together, `cntracking` last.
const OWN_SCRIPTS: [&str; 9] = [
    "https://cdn.arstechnica.net/wp/wp-includes/js/jquery/jquery.min.js?ver=3.7.1",
    AB_REQUEST,
    AB_REQUEST,
    "https://cdn.arstechnica.net/wp-content/plugins/article-forum-connect/public/js/iframe-resizer.parent.js?ver=5.3.1",
    "https://cdn.arstechnica.net/wp-content/plugins/article-forum-connect/public/js/iframe.js?ver=1.2.4",
    "https://cdn.arstechnica.net/wp-content/themes/ars-v9/public/js/app.322081.js",
    "https://cdn.arstechnica.net/wp-content/themes/ars-v9/public/js/ads.ae0b0b.js",
    "https://cdn.arstechnica.net/wp-content/themes/ars-v9/public/js/stats.01cffe.js",
    "https://cdn.arstechnica.net/wp-content/themes/ars-v9/public/js/cntracking.4577bc.js",
];

/// A fetcher that records the URLs of its requests, in order, and answers
/// from the fixture.
struct Recording {
    inner: ReplayFetcher,
    urls: Mutex<Vec<String>>,
}

impl Fetcher for Recording {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        self.urls.lock().unwrap().push(request.url.to_string());
        self.inner.fetch(request)
    }

    fn cookie_jar(&self) -> Option<&CookieJar> {
        self.inner.cookie_jar()
    }
}

/// The requests for scripts of the site's own hosts, with the random parts
/// of the A/B requests masked.
fn own_script_requests(urls: &[String]) -> Vec<String> {
    let ab = |url: &str| {
        url.starts_with("https://arstechnica.com/services/ars-ajax-handler.php?")
            .then(|| {
                let (head, _) = url.split_once("&group=").unwrap();
                format!("{head}&group=G&uid=U&ts=T")
            })
    };
    urls.iter()
        .filter_map(|url| {
            ab(url).or_else(|| {
                let own = url.starts_with("https://cdn.arstechnica.net/");
                let path = url.split(['?', '#']).next().unwrap();
                let script = std::path::Path::new(path)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("js"));
                (own && script).then(|| url.clone())
            })
        })
        .collect()
}

/// The URL that the button "Classic" of the view switcher navigates to.
const CLASSIC_PREVIEW: &str = "https://arstechnica.com/classic-preview/";

/// The record of the page (`window.__s()`).
fn record(page: &mut Page) -> Value {
    let text = value(page, "window.__s()");
    serde_json::from_str(text.as_str().unwrap()).unwrap()
}

#[test]
fn ars_technica_works_with_javascript_on() {
    let Some(dir) = fixture("ars-technica") else {
        eprintln!("skipped: fixtures/js/ars-technica is missing");
        return;
    };
    on_page_thread(move || {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("data/ars-technica-chromium.json")).unwrap();
        let fetcher = Arc::new(Recording {
            inner: ReplayFetcher::load(&dir).unwrap(),
            urls: Mutex::new(Vec::new()),
        });
        let mut page = load_at_capture_with(&dir, Duration::from_millis(1500), fetcher.clone());
        assert!(page.has_script_runtime());
        assert_eq!(console_errors(&page), Vec::<String>::new());
        // The two headline scripts write their A/B scripts with
        // `document.write` (M9 feature 2): swb requests them where Chromium
        // does.
        assert_eq!(
            own_script_requests(&fetcher.urls.lock().unwrap()),
            OWN_SCRIPTS
        );
        assert_eq!(value(&mut page, "document.readyState"), "complete");
        assert_eq!(value(&mut page, include_str!("data/ars-technica.js")), 1);

        let (pixel_groups, points) =
            common::pixel_points(include_str!("data/ars-technica-pixel-points.json"));
        let pixels: Value =
            serde_json::from_str(include_str!("data/ars-technica-pixels.json")).unwrap();
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

        let mut problems = differences("load", &record(&mut page), &expected[0]);
        sample("load", &mut page, &mut problems);
        let groups = step_groups(include_str!("data/ars-technica-steps.json"));
        assert_eq!(groups.len() + 1, expected.len());
        for (index, (name, steps)) in groups.iter().enumerate() {
            run_steps(&mut page, steps).unwrap();
            if name == "view classic" {
                // The page navigates away: Chromium's record is the one of
                // the error page.
                assert_eq!(expected[index + 1]["navigated"], true);
                assert_eq!(page.url().unwrap().as_str(), CLASSIC_PREVIEW);
                continue;
            }
            problems.extend(differences(name, &record(&mut page), &expected[index + 1]));
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

/// The keys of `got` and `expected` that differ, as `name: key: swb ..
/// != Chromium ..`.
fn differences(name: &str, got: &Value, expected: &Value) -> Vec<String> {
    common::differences(name, got, expected, RECTANGLES, TOLERANCE)
}
