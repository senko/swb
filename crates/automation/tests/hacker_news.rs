//! Exit of M8 feature 6 for Hacker News: the front page of the local
//! fixture `fixtures/js/hacker-news` (git-ignored; set `SWB_JS_FIXTURES`
//! to the directory `fixtures/js` of another checkout) with JavaScript on.
//! `hn.js` runs to the end without errors, its `click` listener on
//! `document` is registered and runs on every click, and the links, the
//! "more" link and the search form work as with JavaScript off. The test
//! skips when the fixture is missing.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use swb_automation::run_steps;
use swb_engine::{ConsoleLevel, Evaluation, FontContext, JsonForm, Page, PageConfig, Size, Url};
use swb_net::ReplayFetcher;

/// The directory of the fixture, or `None` (the test then skips).
fn fixture() -> Option<PathBuf> {
    let root = std::env::var_os("SWB_JS_FIXTURES").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/js"),
        PathBuf::from,
    );
    let dir = root.join("hacker-news");
    dir.join("fixture.json").is_file().then_some(dir)
}

fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// The front page, loaded (1280x800, test fonts).
fn load(dir: &Path, javascript: bool) -> Page {
    let meta: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).unwrap()).unwrap();
    let url = Url::parse(meta["url"].as_str().unwrap()).unwrap();
    let config = PageConfig {
        fetcher: Arc::new(ReplayFetcher::load(dir).unwrap()),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(1280.0, 800.0),
        1.0,
    );
    page.set_javascript(javascript);
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(60)));
    page.run_event_loop(Duration::from_millis(300), Duration::from_secs(10));
    page
}

fn value(page: &mut Page, source: &str) -> Value {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{source}: {other:?}"),
    }
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
    let Some(dir) = fixture() else {
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
