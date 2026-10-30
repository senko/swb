//! Exit of M8 feature 6 for Hacker News: the front page of the local
//! fixture `fixtures/js/hacker-news` (git-ignored; set `SWB_JS_FIXTURES`
//! to the directory `fixtures/js` of another checkout) with JavaScript on.
//! `hn.js` runs to the end without errors, its `click` listener on
//! `document` is registered and runs on every click, and the links, the
//! "more" link and the search form work as with JavaScript off. The test
//! skips when the fixture is missing.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::{fixture, on_page_thread, replay_page, value};
use serde_json::{Value, json};
use swb_automation::run_steps;
use swb_engine::{ConsoleLevel, Page};

/// The front page, loaded (1280x800, test fonts).
fn load(dir: &Path, javascript: bool) -> Page {
    let (mut page, url) = replay_page(dir, javascript);
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(60)));
    page.run_event_loop(Duration::from_millis(300), Duration::from_secs(10));
    page
}

/// The URL that the page navigates to after the steps (the pending
/// navigation's URL while it loads).
fn navigation(dir: &Path, javascript: bool, steps: &Value) -> (String, Page) {
    let mut page = load(dir, javascript);
    run_steps(&mut page, steps.as_array().unwrap()).unwrap();
    let url = page.url().unwrap().to_string();
    (url, page)
}

#[test]
fn hacker_news_works_with_javascript_on() {
    let Some(dir) = fixture("hacker-news") else {
        eprintln!("skipped: fixtures/js/hacker-news is missing");
        return;
    };
    on_page_thread(move || {
        // `hn.js` ran to the end: its globals exist, the script made no
        // error and the page is complete.
        let mut page = load(&dir, true);
        assert!(page.has_script_runtime());
        let errors: Vec<_> = page
            .console_messages()
            .into_iter()
            .filter(|m| m.level == ConsoleLevel::Error)
            .collect();
        assert!(errors.is_empty(), "script errors: {errors:?}");
        assert_eq!(value(&mut page, "document.readyState"), "complete");
        assert_eq!(
            value(
                &mut page,
                "[typeof upclass, typeof onclick, typeof vote, typeof n1]"
            ),
            json!(["function", "function", "function", "number"])
        );
        // The `click` listener of `document` runs on every click: it calls
        // `upclass`, which the test counts (a hit on the page background
        // and on a link).
        value(
            &mut page,
            "window.calls = 0; var wrapped = upclass; upclass = function () { calls++; return wrapped.apply(this, arguments); }",
        );
        run_steps(&mut page, json!([{"click": [5, 790]}]).as_array().unwrap()).unwrap();
        assert_eq!(value(&mut page, "calls"), 1);
        run_steps(
            &mut page,
            json!([{"click": "a.morelink"}]).as_array().unwrap(),
        )
        .unwrap();
        assert_eq!(value(&mut page, "calls"), 2);

        // Links, the "more" link and the search form: the same navigations
        // as with JavaScript off.
        let cases = [
            (
                json!([{"click": ".titleline a"}]),
                "https://quad9.net/news/blog/au-revoir-france/",
            ),
            (
                json!([{"click": "a.morelink"}]),
                "https://news.ycombinator.com/?p=2",
            ),
            (
                json!([{"click": "input[name=q]"}, {"type": "rust"}, {"key": "Enter"}]),
                "https://hn.algolia.com/?q=rust",
            ),
        ];
        for (steps, expected) in cases {
            let (on, _) = navigation(&dir, true, &steps);
            let (off, _) = navigation(&dir, false, &steps);
            assert_eq!(on, expected, "JavaScript on, steps {steps}");
            assert_eq!(off, expected, "JavaScript off, steps {steps}");
        }
    });
}
