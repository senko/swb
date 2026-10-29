//! Idle callbacks in the event loop (ADR 0028 section 7 step 4, M8 feature
//! 8a2): `requestIdleCallback` runs when no task and no frame is due, with a
//! deadline that the next task, the next frame or 50 ms bounds; the
//! `timeout` option runs a callback that waited too long; a loop of idle
//! callbacks neither keeps the page busy nor idle for ever. Virtual time
//! makes each run the same.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{ClockMode, Evaluation, JsonForm, Page, PageClock, Size, Url};
use swb_net::Fetcher;

#[path = "idle/cases.rs"]
mod cases;
mod common;
use common::{TestSite, finish_loading, new_page};

/// A probe case with Chromium's values: its page and its queries (query,
/// value).
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

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

/// A page with JavaScript on and a virtual clock that shows `html`.
fn load(html: &str) -> Page {
    let mut site = TestSite::default();
    site.pages.insert(URL.to_owned(), html.to_owned());
    let mut page = new_page(
        Arc::new(site) as Arc<dyn Fetcher>,
        2,
        Size::new(600.0, 400.0),
    );
    page.set_javascript(true);
    page.set_clock(PageClock::new(
        ClockMode::Virtual,
        Some(1_767_225_600_000.0),
    ));
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

/// Every case of the Chromium probe `tools/probes/web-idle.json`, in virtual
/// time (the engine runs the idle periods; the page's own event loop is what
/// the cases measure).
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let mut failures = Vec::new();
        for case in cases::CASES {
            let mut page = load(case.html);
            for (index, &(query, chromium)) in case.queries.iter().enumerate() {
                let got = match page.evaluate(query, JsonForm::Strict) {
                    Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
                    Some(Evaluation::Error(text)) => {
                        serde_json::Value::String(format!("error: {text}"))
                    }
                    None => serde_json::Value::String("error: no script runtime".to_owned()),
                };
                let want = if chromium.starts_with("error: ") {
                    serde_json::Value::String(chromium.to_owned())
                } else {
                    serde_json::from_str(chromium).unwrap()
                };
                if got != want {
                    failures.push(format!(
                        "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                        case.name
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// Runs the event loop for `ms` of page time.
fn run(page: &mut Page, ms: u64) {
    page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(20));
}

#[test]
fn an_idle_callback_runs_after_the_tasks_that_are_due() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.log = [];
             requestIdleCallback((d) => log.push('idle ' + d.didTimeout + ' ' + (d.timeRemaining() > 0)));
             setTimeout(() => log.push('timer'), 0);
             Promise.resolve().then(() => log.push('micro'));
             log.push('script');</script>",
        );
        run(&mut page, 200);
        assert_eq!(
            value(&mut page, "log.join()"),
            r#""script,micro,timer,idle false true""#
        );
    });
}

#[test]
fn callbacks_run_in_the_order_of_their_handles_and_new_ones_wait_for_the_next_period() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.log = [];
             requestIdleCallback(() => { log.push('a'); requestIdleCallback(() => log.push('c')); });
             requestIdleCallback(() => log.push('b'));
             const h = requestIdleCallback(() => log.push('cancelled'));
             cancelIdleCallback(h);
             setTimeout(() => log.push('t30'), 30);</script>",
        );
        run(&mut page, 20);
        assert_eq!(value(&mut page, "log.join()"), r#""a,b""#);
        // The first period ended at the timer, 30 ms after it started; the
        // next one starts then, after the timer.
        run(&mut page, 100);
        assert_eq!(value(&mut page, "log.join()"), r#""a,b,t30,c""#);
    });
}

#[test]
fn the_deadline_ends_at_the_next_task() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.left = [];
             setTimeout(() => {}, 20);
             requestIdleCallback((d) => left.push(Math.round(d.timeRemaining())));</script>",
        );
        run(&mut page, 100);
        let left: Vec<f64> =
            serde_json::from_str(&value(&mut page, "left")).expect("a list of numbers");
        assert_eq!(left.len(), 1);
        assert!(left[0] > 0.0 && left[0] <= 20.0, "time left: {}", left[0]);
    });
}

#[test]
fn time_remaining_runs_out_in_virtual_time() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.result = null;
             requestIdleCallback((d) => {
               const start = performance.now(); let calls = 0;
               while (d.timeRemaining() > 0) calls++;
               result = [calls > 10, calls < 1000, Math.round(performance.now() - start) <= 50];
             });</script>",
        );
        run(&mut page, 100);
        assert_eq!(
            value(&mut page, "JSON.stringify(result)"),
            r#""[true,true,true]""#
        );
    });
}

#[test]
fn a_callback_with_a_timeout_runs_when_the_timeout_is_over() {
    on_page_thread(|| {
        // The second callback is requested inside the first idle period, so
        // the next period starts only 50 ms later; the 10 ms timeout is first.
        let mut page = load(
            "<!DOCTYPE html><script>window.log = [];
             requestIdleCallback(() => {
               const t = performance.now();
               requestIdleCallback((d) => log.push([d.didTimeout, d.timeRemaining(), Math.round(performance.now() - t)]), { timeout: 10 });
             });</script>",
        );
        run(&mut page, 200);
        assert_eq!(value(&mut page, "JSON.stringify(log)"), "\"[[true,0,10]]\"");
    });
}

#[test]
fn a_callback_that_ran_before_its_timeout_does_not_run_again() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.count = 0;
             requestIdleCallback(() => count++, { timeout: 1000 });</script>",
        );
        run(&mut page, 3000);
        assert_eq!(value(&mut page, "count"), "1");
    });
}

#[test]
fn a_loop_of_idle_callbacks_runs_20_times_a_second_and_the_page_is_idle() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.count = 0;
             function f() { count++; requestIdleCallback(f); }
             f();</script>",
        );
        run(&mut page, 1000);
        let count: u32 = value(&mut page, "count").parse().unwrap();
        assert!((18..=22).contains(&count), "{count} callbacks in a second");
        // A loop does not keep the page from being idle (waitForIdle).
        assert!(page.wait_until_idle(Duration::from_millis(100), Duration::from_secs(20)));
    });
}

#[test]
fn a_single_idle_callback_makes_the_page_wait_until_it_ran() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.ran = false; requestIdleCallback(() => { ran = true; });</script>",
        );
        assert!(page.wait_until_idle(Duration::from_millis(100), Duration::from_secs(20)));
        assert_eq!(value(&mut page, "ran"), "true");
    });
}

#[test]
fn idle_callbacks_and_animation_frames_share_the_time() {
    on_page_thread(|| {
        let mut page = load(
            "<!DOCTYPE html><script>window.frames = 0; window.idles = 0; window.left = [];
             function frame() { frames++; if (frames < 30) requestAnimationFrame(frame); }
             requestAnimationFrame(frame);
             function idle(d) { idles++; left.push(d.timeRemaining()); if (idles < 30) requestIdleCallback(idle); }
             requestIdleCallback(idle);</script>",
        );
        run(&mut page, 600);
        assert_eq!(value(&mut page, "frames"), "30");
        let idles: u32 = value(&mut page, "idles").parse().unwrap();
        assert!(idles >= 20, "{idles} idle callbacks while frames run");
        // Between two frames, an idle period never lasts longer than a frame.
        assert_eq!(
            value(
                &mut page,
                "left.slice(0, 20).every((t) => t >= 0 && t <= 17)"
            ),
            "true"
        );
    });
}
