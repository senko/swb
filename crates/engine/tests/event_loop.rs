//! The event loop of the page (ADR 0028 section 7, feature 5b): timers
//! that fire through `Page::pump`, the page clock in real and virtual time
//! with a fixed start, `Page::run_event_loop` (the headless `--settle-ms`),
//! `Page::wait_until_idle` (`page.waitForIdle`), evaluations of promises
//! that settle later, and the drop of a runtime that reached the heap
//! limit. Pages come from an in-memory fetcher; scripts run on a thread
//! with a 16 MiB stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use swb_engine::{ClockMode, Evaluation, JsonForm, Page, PageClock, Size, Url};
use swb_net::Fetcher;

mod common;
use common::{TestSite, finish_loading, new_page};

const URL: &str = "https://site.test/page.html";

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A page with JavaScript on and `clock` that shows `html`.
fn load(html: &str, clock: PageClock) -> Page {
    let mut site = TestSite::default();
    site.pages.insert(URL.to_owned(), html.to_owned());
    let mut page = new_page(
        Arc::new(site) as Arc<dyn Fetcher>,
        2,
        Size::new(600.0, 400.0),
    );
    page.set_javascript(true);
    page.set_clock(clock);
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

/// A virtual clock that starts at 2026-01-01T00:00:00Z.
fn virtual_clock() -> PageClock {
    PageClock::new(ClockMode::Virtual, Some(1_767_225_600_000.0))
}

#[test]
fn timers_fire_through_the_pump_in_real_time() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><title>a</title><script>setTimeout(() => { document.title = 'b'; }, 30);</script>",
            PageClock::real(),
        );
        assert_eq!(page.title(), "a");
        let pump = page.pump();
        assert!(pump.next.is_some(), "the timer is due later");
        let started = Instant::now();
        while page.title() != "b" && started.elapsed() < Duration::from_secs(5) {
            let next = page.pump().next;
            if let Some(next) = next {
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        }
        assert_eq!(page.title(), "b");
        assert!(started.elapsed() >= Duration::from_millis(20));
        assert_eq!(page.pump().next, None);
    });
}

#[test]
fn virtual_time_jumps_to_the_timers_and_starts_at_a_fixed_time() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.log = [new Date().toISOString()]; setTimeout(() => log.push(Date.now() - 1767225600000), 5000); setTimeout(() => log.push('late'), 60000);</script>",
            virtual_clock(),
        );
        // Nothing is due: the pump does not move virtual time.
        assert_eq!(page.pump().next, None);
        let started = Instant::now();
        page.run_event_loop(Duration::from_secs(10), Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            value(&mut page, "log"),
            r#"["2026-01-01T00:00:00.000Z",5000]"#
        );
        assert_eq!(value(&mut page, "Math.round(performance.now())"), "10000");
    });
}

#[test]
fn waiting_for_idle_runs_timer_chains_and_gives_up_on_intervals() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.n = 0; (function f() { if (++n < 10) setTimeout(f, 50); })();</script>",
            virtual_clock(),
        );
        assert!(page.wait_until_idle(Duration::from_millis(100), Duration::from_secs(5)));
        assert_eq!(value(&mut page, "n"), "10");
        value(&mut page, "setInterval(() => n++, 10)");
        let started = Instant::now();
        assert!(!page.wait_until_idle(Duration::from_millis(100), Duration::from_millis(300)));
        assert!(started.elapsed() < Duration::from_secs(2));
    });
}

#[test]
fn evaluations_wait_for_promises_through_the_event_loop() {
    on_page_thread(|| {
        let mut page = load("<!DOCTYPE html>", PageClock::real());
        assert_eq!(
            value(
                &mut page,
                "new Promise((r) => setTimeout(() => r('later'), 40))"
            ),
            r#""later""#
        );
        assert_eq!(
            page.evaluate(
                "new Promise((_, reject) => setTimeout(() => reject(new TypeError('no')), 10))",
                JsonForm::Strict
            ),
            Some(Evaluation::Error("TypeError: no".to_owned()))
        );
        let started = Instant::now();
        let never = page.evaluate_with_timeout(
            "new Promise((r) => setTimeout(r, 1e7))",
            JsonForm::Strict,
            Duration::from_millis(100),
        );
        assert_eq!(
            never,
            Some(Evaluation::Error(
                "Error: the promise did not settle in time".to_owned()
            ))
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        // A rejection that the evaluation handled is not reported.
        assert!(
            !page
                .console_messages()
                .iter()
                .any(|m| m.text.contains("in promise"))
        );
    });
}

#[test]
fn a_runtime_at_the_heap_limit_is_dropped_and_the_page_stays() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><title>kept</title><script>setInterval(() => {}, 10);</script>",
            PageClock::real(),
        );
        page.script_runtime_mut().unwrap().set_heap_limit(16 << 20);
        let hog = page.evaluate(
            "const a = []; for (;;) a.push([1, 2, 3, 4]);",
            JsonForm::Strict,
        );
        assert!(matches!(hog, Some(Evaluation::Error(_))), "{hog:?}");
        assert!(!page.has_script_runtime());
        assert_eq!(page.title(), "kept");
        assert_eq!(page.pump().next, None);
        assert_eq!(
            page.evaluate("1", JsonForm::Strict),
            Some(Evaluation::Error(
                "Error: the scripts of the document were stopped".to_owned()
            ))
        );
    });
}
