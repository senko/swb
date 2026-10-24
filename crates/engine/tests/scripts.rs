//! Scripts in the page (ADR 0028 section 2, feature 4c2): one runtime per
//! document with JavaScript on, the inline classic scripts at the end of
//! the parse, DOM changes of scripts processed when the state comes back,
//! `Page::evaluate` and the console, the runtime dropped on navigation,
//! and the page's refusal of JavaScript without a big enough stack. Pages
//! come from an in-memory fetcher; scripts run on a thread with a 16 MiB
//! stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{ConsoleLevel, Evaluation, FontContext, JsonForm, Page, PageConfig, Size, Url};
use swb_net::Fetcher;

mod common;
use common::{TestSite, finish_loading, new_page};

const URL: &str = "https://site.test/page.html";
const OTHER: &str = "https://site.test/other.html";

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A page with JavaScript on that shows `html` (and `other` at OTHER).
fn load(html: &str, other: &str) -> Page {
    let mut site = TestSite::default();
    site.pages.insert(URL.to_owned(), html.to_owned());
    site.pages.insert(OTHER.to_owned(), other.to_owned());
    let mut page = new_page(
        Arc::new(site) as Arc<dyn Fetcher>,
        2,
        Size::new(600.0, 400.0),
    );
    page.set_javascript(true);
    page.navigate(Url::parse(URL).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    page
}

fn value(page: &mut Page, source: &str) -> String {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{source}: {other:?}"),
    }
}

#[test]
fn inline_scripts_run_after_the_parse_and_change_the_dom() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><title>Before</title><script>window.seen = document.body.childNodes.length; document.title = 'After';</script><body><p>a</p><p>b</p>",
            "",
        );
        assert!(page.has_script_runtime());
        // The script ran after the parse: it saw the whole body.
        assert_eq!(value(&mut page, "window.seen"), "2");
        // The title change reached the engine when the state came back.
        assert_eq!(page.title(), "After");
        assert_eq!(value(&mut page, "document.title"), r#""After""#);
        // A change in an evaluation too.
        value(&mut page, "document.title = 'Later'");
        assert_eq!(page.title(), "Later");
    });
}

#[test]
fn console_and_errors_of_the_page() {
    on_page_thread(|| {
        let page = load(
            "<!DOCTYPE html><script>console.log('one', 1); undefinedFunction();</script><script>console.warn('two')</script>",
            "",
        );
        let messages: Vec<(ConsoleLevel, String)> = page
            .console_messages()
            .into_iter()
            .map(|m| (m.level, m.text))
            .collect();
        assert_eq!(
            messages,
            [
                (ConsoleLevel::Log, "one 1".to_owned()),
                (
                    ConsoleLevel::Error,
                    "Uncaught ReferenceError: undefinedFunction is not defined".to_owned()
                ),
                (ConsoleLevel::Warn, "two".to_owned()),
            ]
        );
    });
}

#[test]
fn each_document_gets_a_new_runtime() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.mark = 1; console.log('first');</script>",
            "<!DOCTYPE html><title>Other</title>",
        );
        assert_eq!(value(&mut page, "typeof window.mark"), r#""number""#);
        page.navigate(Url::parse(OTHER).unwrap());
        finish_loading(&mut page, Duration::from_secs(20));
        assert_eq!(page.title(), "Other");
        // A new global object, and the console of the new document.
        assert_eq!(value(&mut page, "typeof window.mark"), r#""undefined""#);
        assert_eq!(page.console_messages(), []);
        page.go_back();
        finish_loading(&mut page, Duration::from_secs(20));
        assert_eq!(value(&mut page, "window.mark"), "1");
    });
}

#[test]
fn javascript_off_has_no_runtime() {
    on_page_thread(|| {
        let mut site = TestSite::default();
        site.pages.insert(
            URL.to_owned(),
            "<!DOCTYPE html><title>A</title><script>document.title = 'B'</script>".to_owned(),
        );
        let mut page = new_page(
            Arc::new(site) as Arc<dyn Fetcher>,
            2,
            Size::new(600.0, 400.0),
        );
        page.navigate(Url::parse(URL).unwrap());
        finish_loading(&mut page, Duration::from_secs(20));
        assert!(!page.has_script_runtime());
        assert_eq!(page.title(), "A");
        assert_eq!(page.evaluate("1", JsonForm::Strict), None);
        assert_eq!(page.console_messages(), []);
    });
}

/// A page whose thread does not have a big stack refuses JavaScript, also
/// from the automation API (`page.setJavaScript` calls `set_javascript`).
#[test]
fn a_page_without_a_big_stack_keeps_javascript_off() {
    let mut site = TestSite::default();
    site.pages.insert(
        URL.to_owned(),
        "<!DOCTYPE html><title>A</title><script>document.title = 'B'</script>".to_owned(),
    );
    let config = PageConfig {
        fetcher: Arc::new(site),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: false,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(600.0, 400.0),
        1.0,
    );
    assert!(!page.javascript_available());
    page.set_javascript(true);
    assert!(!page.javascript());
    page.navigate(Url::parse(URL).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    assert!(!page.document_has_scripting());
    assert!(!page.has_script_runtime());
    assert_eq!(page.title(), "A");
}

/// The exit criterion of feature 4: `document.title` gives Chromium's
/// value on the page fixtures with JavaScript on (Chromium 148 with
/// JavaScript on, measured with a scratch probe of each fixture's document,
/// 2026-10-23).
#[test]
fn document_title_on_the_fixtures() {
    on_page_thread(|| {
        let root = common::repo_root().join("fixtures/pages");
        let cases = [
            ("senko-net", "Senko's corner of the Web"),
            ("hacker-news", "Hacker News"),
            ("wikipedia-web-browser", "Web browser - Wikipedia"),
            (
                "ars-technica",
                "Ars Technica - Serving the Technologist since 1998. News, reviews, and analysis.",
            ),
            (
                "bbc",
                "BBC Home - Breaking News, World News, US News, Sports, Business, Innovation, Climate, Culture, Travel, Video & Audio",
            ),
            (
                "craigslist-posting",
                "Halloween costume - Anna from Frozen - baby & kid stuff - by owner - household sale - craigslist",
            ),
        ];
        for (name, title) in cases {
            let dir = root.join(name);
            let fixture: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).unwrap())
                    .unwrap();
            let url = Url::parse(fixture["url"].as_str().unwrap()).unwrap();
            let fetcher = swb_net::ReplayFetcher::load(&dir).unwrap();
            let mut page = new_page(Arc::new(fetcher), 4, Size::new(1280.0, 800.0));
            page.set_javascript(true);
            page.navigate(url);
            finish_loading(&mut page, Duration::from_secs(60));
            let got = value(&mut page, "document.title");
            assert_eq!(got, serde_json::to_string(title).unwrap(), "{name}");
        }
    });
}
