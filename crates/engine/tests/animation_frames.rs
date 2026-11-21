//! The time of CSS animations in the page (ADR 0044 sections 2 and 8, M13
//! feature 3b): the rendering updates that animations need, with and
//! without scripts, in virtual time; the catch-up of a screenshot; idle
//! pages with running animations; `--animations off`. Pages come from an
//! in-memory fetcher; scripts run on a thread with a 16 MiB stack.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{ClockMode, Page, PageClock, Size, Url};
use swb_net::Fetcher;

mod common;
use common::{TestSite, finish_loading, new_page, on_page_thread, value};

const URL: &str = "https://site.test/page.html";

/// A page in virtual time that shows `html`.
fn load(html: &str, javascript: bool, animations: bool) -> Page {
    let mut site = TestSite::default();
    site.pages.insert(URL.to_owned(), html.to_owned());
    let mut page = new_page(
        Arc::new(site) as Arc<dyn Fetcher>,
        2,
        Size::new(400.0, 300.0),
    );
    page.set_javascript(javascript);
    page.set_animations(animations);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.navigate(Url::parse(URL).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    page
}

/// An infinite animation of 1 s: `left` from 0 to 100px.
const INFINITE: &str = "<!DOCTYPE html><style>body { margin: 0 } \
    #a { position: absolute; width: 10px; height: 10px; background: red; \
    animation: k 1s linear infinite } \
    @keyframes k { from { left: 0 } to { left: 100px } }</style><div id=a></div>";

/// The left edge of `#a` in the layout of `page`.
fn left(page: &mut Page) -> f32 {
    let node = common::node(page, "a");
    page.element_box(node).map_or(f32::NAN, |r| r.x)
}

#[test]
fn settling_costs_the_iterations_not_the_frames() {
    on_page_thread(|| {
        for javascript in [false, true] {
            let mut page = load(INFINITE, javascript, true);
            let before = page.animation_updates();
            page.run_event_loop(Duration::from_secs(10), Duration::from_secs(30));
            let updates = page.animation_updates() - before;
            // One update per iteration (plus the first, which resolves the
            // pending play), not 60 per second.
            assert!(
                (9..=13).contains(&updates),
                "javascript {javascript}: {updates} updates in 10 s"
            );
        }
    });
}

/// `INFINITE` with a chain of 500 timers of 10 ms; each task also reads the
/// computed style if `read` is true.
fn with_timer_chain(read: bool) -> String {
    let body = if read {
        "getComputedStyle(document.getElementById('a')).left;"
    } else {
        ""
    };
    format!(
        "{INFINITE}<script>let n = 0; function f() {{ {body} if (++n < 500) setTimeout(f, 10); }} \
         f();</script>"
    )
}

#[test]
fn tasks_that_read_no_style_cost_no_catch_up() {
    on_page_thread(|| {
        let mut page = load(&with_timer_chain(false), true, true);
        let before = page.animation_updates();
        page.run_event_loop(Duration::from_secs(5), Duration::from_secs(30));
        let updates = page.animation_updates() - before;
        // The updates of the iteration ends only, as without the timers;
        // 500 tasks in 5 s add none.
        assert!((4..=8).contains(&updates), "{updates} updates in 5 s");
        // Reading the style catches up to the grid.
        let before = page.animation_updates();
        let l = left(&mut page);
        assert!(page.animation_updates() <= before + 1);
        assert!(l.is_finite());
    });
}

#[test]
fn tasks_that_read_the_style_catch_up_at_the_grid() {
    on_page_thread(|| {
        let mut page = load(&with_timer_chain(true), true, true);
        let before = page.animation_updates();
        page.run_event_loop(Duration::from_secs(5), Duration::from_secs(30));
        let updates = page.animation_updates() - before;
        // One catch-up per grid frame that a task reads: far more than the
        // iteration ends, at most one per 10 ms task.
        assert!((100..=500).contains(&updates), "{updates} updates in 5 s");
    });
}

#[test]
fn a_screenshot_catches_up_to_the_frame_grid() {
    on_page_thread(|| {
        let mut page = load(INFINITE, false, true);
        page.run_event_loop(Duration::from_millis(1250), Duration::from_secs(30));
        // 1250 ms is on the grid (75 frames): a quarter of the second
        // iteration.
        assert!((left(&mut page) - 25.0).abs() < 0.01, "{}", left(&mut page));
        page.screenshot(false).unwrap();
    });
}

#[test]
fn running_animations_leave_the_page_idle() {
    on_page_thread(|| {
        let mut page = load(INFINITE, true, true);
        assert!(page.wait_until_idle(Duration::from_millis(500), Duration::from_secs(10)));
    });
}

#[test]
fn animations_off_resets_the_properties() {
    on_page_thread(|| {
        let mut page = load(INFINITE, true, false);
        page.run_event_loop(Duration::from_millis(500), Duration::from_secs(30));
        assert_eq!(left(&mut page), 0.0);
        assert_eq!(
            value(
                &mut page,
                "getComputedStyle(document.getElementById('a')).animationName + ' ' \
                 + document.getAnimations().length"
            ),
            "\"none 0\""
        );
        assert_eq!(page.animation_updates(), 0);
    });
}
