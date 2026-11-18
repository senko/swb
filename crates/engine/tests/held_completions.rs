//! In virtual time, the completion of a request that an input handler
//! started waits for a rendering update (M12 feature 3c, ADR 0028 section 7
//! "Clocks"). The Chromium values of the cases (frames between a request and
//! its completion) are in `crates/automation/tests/network_frames.rs`; these
//! tests check the order, the order of several completions, the bounds and
//! that nothing changes for requests of timers and for pages with nothing
//! pending.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{
    ClockMode, Evaluation, JsonForm, Modifiers, MouseButton, Page, PageClock, Size, Url,
};
use swb_net::{Fetcher, NetError, Request, Response};

use common::{html_response, new_page, on_page_thread};

const PAGE: &str = "http://probe.test/case.html";

/// Serves the page and `x.txt`.
struct Site(String);

impl Fetcher for Site {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        Ok(match request.url.path() {
            "/case.html" => html_response(request, 200, self.0.as_bytes().to_vec()),
            "/x.txt" => html_response(request, 200, b"hello".to_vec()),
            _ => html_response(request, 404, b"not found".to_vec()),
        })
    }
}

/// A page with a link `a` (60 px wide, 40 px high, at the top left) in a
/// box, a log `log`, and `script` after them. JavaScript on; the page is
/// loaded. In virtual time if `virtual_time`.
fn open(script: &str, virtual_time: bool) -> Page {
    let html = format!(
        "<!DOCTYPE html><style>body{{margin:0}}#box{{position:absolute;left:0;top:0;width:200px}}\
a,b{{display:block;height:40px}}</style><div id=box><a id=a href=#a>a</a></div><script>\
window.log = []; const L = (s) => log.push(s);\
document.addEventListener('mouseover', (e) => L('over:' + e.target.id), true);\
const swap = () => {{ box.innerHTML = '<b id=c>c</b>'; }};\
const get = (tag) => fetch('x.txt').then((r) => r.text()).then(() => L(tag));\
{script}</script>"
    );
    let mut page = new_page(Arc::new(Site(html)), 2, Size::new(800.0, 600.0));
    page.set_javascript(true);
    if virtual_time {
        page.set_clock(PageClock::new(ClockMode::Virtual, None));
    }
    page.navigate(Url::parse(PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

fn eval(page: &mut Page, expression: &str) -> String {
    match page.evaluate(expression, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{expression}: {other:?}"),
    }
}

fn log_of(page: &mut Page) -> String {
    eval(page, "log.join(' ')").trim_matches('"').to_owned()
}

/// Hovers the link, then clicks it.
fn click(page: &mut Page) {
    page.mouse_move(30.0, 20.0);
    page.run_event_loop(Duration::from_millis(100), Duration::from_secs(10));
    page.mouse_down(30.0, 20.0, MouseButton::Primary, Modifiers::default(), 1);
    page.mouse_up(30.0, 20.0, MouseButton::Primary);
}

/// Runs the event loop for 500 ms of page time: in virtual time it ends
/// when the page time is up, and the requests that run have completed.
fn idle(page: &mut Page) {
    page.run_event_loop(Duration::from_millis(500), Duration::from_secs(20));
}

#[test]
fn the_completion_of_a_click_request_comes_after_a_rendering_update() {
    on_page_thread(|| {
        let mut page = open(
            "a.addEventListener('click', () => { L('click'); swap(); get('done'); });",
            true,
        );
        click(&mut page);
        idle(&mut page);
        // The update hit-tests the new element under the still pointer
        // (`over:c`) before the response is handled.
        assert_eq!(log_of(&mut page), "over:a click over:c done");
    });
}

#[test]
fn the_completion_of_a_timer_request_does_not_wait() {
    on_page_thread(|| {
        let mut page = open(
            "setTimeout(() => { L('timer'); swap(); get('done'); }, 50);",
            true,
        );
        page.mouse_move(30.0, 20.0);
        page.run_event_loop(Duration::from_millis(100), Duration::from_secs(10));
        idle(&mut page);
        // No update between the timer and the response (Chromium: no frame in
        // most runs).
        assert_eq!(log_of(&mut page), "over:a timer done over:c");
    });
}

#[test]
fn the_completion_of_a_request_in_real_time_does_not_wait() {
    on_page_thread(|| {
        let mut page = open(
            "a.addEventListener('click', () => { L('click'); swap(); get('done'); });",
            false,
        );
        click(&mut page);
        idle(&mut page);
        // The response (a few ms of real time at most) comes before the
        // frame (16.7 ms after the change).
        assert_eq!(log_of(&mut page), "over:a click done over:c");
    });
}

#[test]
fn several_completions_come_in_the_order_of_their_requests() {
    on_page_thread(|| {
        let mut page = open(
            "a.addEventListener('click', () => { swap(); for (let i = 1; i <= 6; i++) get('d' + i); });",
            true,
        );
        click(&mut page);
        idle(&mut page);
        let expected: Vec<String> = (1..=6).map(|i| format!("d{i}")).collect();
        assert_eq!(
            log_of(&mut page),
            format!("over:a over:c {}", expected.join(" "))
        );
    });
}

#[test]
fn a_chain_of_requests_from_a_click_costs_one_frame() {
    on_page_thread(|| {
        let mut page = open(
            "let n = 0, t0 = 0; const next = () => { if (++n < 200) fetch('x.txt').then((r) => r.text()).then(next); \
             else L('chain ' + n + (performance.now() - t0 < 20 ? ' fast' : ' slow')); };\
             a.addEventListener('click', () => { t0 = performance.now(); fetch('x.txt').then((r) => r.text()).then(next); });",
            true,
        );
        click(&mut page);
        idle(&mut page);
        // One frame (16.7 ms) for the first completion; the rest follow at once.
        assert_eq!(log_of(&mut page), "over:a chain 200 fast");
    });
}

#[test]
fn a_page_with_nothing_pending_gets_no_extra_frame() {
    on_page_thread(|| {
        let mut page = open("a.addEventListener('click', () => { L('click'); });", true);
        click(&mut page);
        let before = page.clock().now();
        // Nothing is pending: the idle wait ends at once, without a jump to
        // a frame (16.7 ms).
        assert!(page.wait_until_idle(Duration::ZERO, Duration::from_secs(20)));
        assert!(page.clock().now() - before < 5.0);
        assert_eq!(log_of(&mut page), "over:a click");
    });
}

#[test]
fn a_request_after_a_timer_that_a_click_started_does_not_wait() {
    on_page_thread(|| {
        // The request starts in a timer task, not in the click's dispatch.
        let mut page = open(
            "a.addEventListener('click', () => { setTimeout(() => { swap(); get('done'); }, 0); });",
            true,
        );
        click(&mut page);
        idle(&mut page);
        assert_eq!(log_of(&mut page), "over:a done over:c");
    });
}

#[test]
fn pending_work_that_waits_does_not_end_the_idle_wait_early() {
    on_page_thread(|| {
        let mut page = open("a.addEventListener('click', () => { get('done'); });", true);
        click(&mut page);
        assert!(page.wait_until_idle(Duration::from_millis(50), Duration::from_secs(20)));
        assert_eq!(log_of(&mut page), "over:a done");
    });
}
