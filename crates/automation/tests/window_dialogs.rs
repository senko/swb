//! The simple dialogs, `print()` and `navigator.clipboard` (M8 feature
//! 11c2, ADR 0028 sections 4 and 13): the cases of the Chromium probe
//! `tools/probes/web-window-dialogs.json` run here with their input steps
//! (`swb_automation::run_steps`) and the values of their queries are the
//! ones that Chromium 148 gave (`tests/data/web-window-dialogs.json`:
//! `just probe --json tools/probes/web-window-dialogs.json`, three equal
//! runs). The tests below check what a probe cannot see.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{Evaluation, FontContext, JsonForm, Page, PageConfig, Size, Url};
use swb_net::NetworkFetcher;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A directory with test pages, removed at the end of the test.
struct Site(PathBuf);

impl Site {
    fn new(name: &str) -> Site {
        let dir =
            std::env::temp_dir().join(format!("swb-window-dialogs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Site(dir)
    }

    fn page(&self, name: &str, html: &str) -> Url {
        let path = self.0.join(name);
        std::fs::write(&path, html).unwrap();
        Url::from_file_path(path).unwrap()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A page with JavaScript on at `url` (400x200, test fonts), loaded.
fn open(url: Url) -> Page {
    let config = PageConfig {
        fetcher: Arc::new(NetworkFetcher::new()),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(400.0, 200.0),
        1.0,
    );
    page.set_javascript(true);
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// The JSON value of an expression.
fn value(page: &mut Page, source: &str) -> Value {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{source}: {other:?}"),
    }
}

/// Runs the cases of the probe file against Chromium's values.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let probe: Value = serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/web-window-dialogs.json"))
                .unwrap(),
        )
        .unwrap();
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-window-dialogs.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let cases = probe["cases"].as_array().unwrap();
        assert_eq!(cases.len(), expected.as_object().unwrap().len());
        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let site = Site::new(name);
            let mut page = open(site.page("page.html", case["html"].as_str().unwrap()));
            let steps = case["steps"].as_array().cloned().unwrap_or_default();
            if let Err(error) = run_steps(&mut page, &steps) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            let queries = case["measure"].as_array().unwrap();
            for (index, query) in queries.iter().enumerate() {
                let source = query["js"].as_str().unwrap();
                let want = &expected[name][index];
                let got = value(&mut page, source);
                if &got != want {
                    failures.push(format!(
                        "{name} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The page of a test, loaded.
fn page_of(site: &Site, html: &str) -> Page {
    open(site.page("page.html", html))
}

/// A dialog in a loop is cheap: 30,000 calls with long messages finish at
/// once (the hostile-page set has a loop that the time limit ends).
#[test]
fn many_dialogs_with_long_messages_take_bounded_time() {
    on_page_thread(|| {
        let site = Site::new("loop");
        let mut page = page_of(&site, "<!DOCTYPE html><body>");
        let started = std::time::Instant::now();
        let answer = value(
            &mut page,
            "(() => { const m = 'x'.repeat(10000); let n = 0; for (let i = 0; i < 10000; i++) { alert(m); if (confirm(m)) n++; if (prompt(m, m) !== null) n++; } return n; })()",
        );
        assert_eq!(answer, 0);
        assert!(started.elapsed() < Duration::from_secs(20));
    });
}

/// `print()` before the load is completely done only sets the flag; the
/// events fire once after the `load` event, and a listener that throws
/// does not stop the other listeners or reach the `error` event.
#[test]
fn print_waits_for_the_load_and_swallows_listener_errors() {
    on_page_thread(|| {
        let site = Site::new("print");
        let html = "<!DOCTYPE html><body><script>\
            window.L = [];\
            addEventListener('beforeprint', () => { L.push('bp1'); throw new Error('x'); });\
            addEventListener('beforeprint', () => L.push('bp2'));\
            addEventListener('afterprint', () => L.push('ap'));\
            addEventListener('error', (e) => L.push('error ' + e.message));\
            addEventListener('load', () => L.push('load'));\
            print(); print(); L.push('called');\
            </script>";
        let mut page = page_of(&site, html);
        assert_eq!(
            value(&mut page, "L.join()"),
            "called,load,bp1,bp2,ap",
            "one printing, after the load, and no error event"
        );
        // After the load, `print()` runs the steps at once, and error
        // reports work again.
        value(&mut page, "print(); L.push('after'); 1");
        assert_eq!(value(&mut page, "L.slice(5).join()"), "bp1,bp2,ap,after");
        value(
            &mut page,
            "setTimeout(() => { throw new Error('later'); }, 0); 1",
        );
        page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
        assert_eq!(
            value(&mut page, "L[L.length - 1]"),
            "error Uncaught Error: later"
        );
    });
}
