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

/// A page in virtual time with painted frames (as the GUI), rendered once
/// (the visibility test runs after a display list).
fn load_painted(html: &str, javascript: bool) -> Page {
    let mut page = load(html, javascript, true);
    page.set_paints_frames(true);
    common::render_viewport(&mut page);
    page
}

/// An infinite 1 s marquee (`transform` in an `overflow: hidden` strip)
/// and a fading box (`opacity`) at `top`, and a spinner (`steps(8)`) under
/// `display: none`.
fn offscreen(top: u32) -> String {
    format!(
        "<!DOCTYPE html><style>body {{ margin: 0; height: 5000px }} \
         .strip {{ position: absolute; top: {top}px; width: 300px; height: 15px; overflow: hidden }} \
         .track {{ width: 600px; height: 15px; background: red; animation: m 1s linear infinite }} \
         .fade {{ position: absolute; top: {top}px; left: 320px; width: 20px; height: 20px; \
           background: blue; animation: f 1s linear infinite }} \
         .hidden {{ display: none }} .seg {{ width: 5px; height: 5px; animation: f 1s steps(8) infinite }} \
         @keyframes m {{ to {{ transform: translateX(-50%) }} }} \
         @keyframes f {{ to {{ opacity: 0 }} }}</style>\
         <div class=strip><div class=track></div></div><div class=fade></div>\
         <div class=hidden><div class=seg></div></div>"
    )
}

#[test]
fn offscreen_animations_cost_only_their_event_times() {
    on_page_thread(|| {
        let mut page = load_painted(&offscreen(2000), true);
        let before = page.animation_updates();
        page.run_event_loop(Duration::from_secs(10), Duration::from_secs(30));
        let updates = page.animation_updates() - before;
        // The iteration ends of the two animations (in step), not 60 frames
        // per second.
        assert!((9..=13).contains(&updates), "{updates} updates in 10 s");
    });
}

#[test]
fn visible_animations_need_each_frame_and_scrolling_shows_them() {
    on_page_thread(|| {
        let mut page = load_painted(&offscreen(100), true);
        let before = page.animation_updates();
        page.run_event_loop(Duration::from_secs(1), Duration::from_secs(30));
        let updates = page.animation_updates() - before;
        assert!((55..=65).contains(&updates), "{updates} updates in 1 s");
        // Offscreen, then scrolled into view: the next display list shows
        // them, and the frames follow.
        let mut page = load_painted(&offscreen(2000), true);
        page.scroll_to(swb_engine::Point::new(0.0, 1900.0));
        common::render_viewport(&mut page);
        let before = page.animation_updates();
        page.run_event_loop(Duration::from_secs(1), Duration::from_secs(30));
        let updates = page.animation_updates() - before;
        assert!((55..=65).contains(&updates), "{updates} updates in 1 s");
        // Scrolled away again.
        page.scroll_to(swb_engine::Point::new(0.0, 0.0));
        common::render_viewport(&mut page);
        let before = page.animation_updates();
        page.run_event_loop(Duration::from_secs(1), Duration::from_secs(30));
        let updates = page.animation_updates() - before;
        assert!(updates <= 3, "{updates} updates in 1 s");
    });
}

/// A page with two keyframes rules and a log of the animation events.
const EVENTS: &str = "<!DOCTYPE html><style>body { margin: 0 } #e { width: 10px; height: 10px } \
    @keyframes a { from { opacity: 1 } to { opacity: 0.5 } } \
    @keyframes b { from { width: 10px } to { width: 20px } }</style><div id=e></div><script> \
    window.LOG = []; \
    for (const t of ['animationstart', 'animationiteration', 'animationend', 'animationcancel']) \
      document.addEventListener(t, (e) => LOG.push(e.type + ' ' + e.animationName + ' ' \
        + (e.type === 'animationcancel' ? '~' : e.elapsedTime))); \
    </script>";

/// The case `events-restart-and-pause` of `css-animations.json` in one
/// script, in virtual time: an animation created paused starts (its event
/// comes at once), runs after `animation-play-state: running` and ends; a
/// finished animation whose name goes gets no `animationcancel`.
#[test]
fn events_of_a_paused_start_and_a_restart() {
    on_page_thread(|| {
        let mut page = load(EVENTS, true, true);
        let js = "(async () => { const wait = (ms) => new Promise((r) => setTimeout(r, ms)); \
            const e = document.getElementById('e'); e.style.animation = 'a 200ms linear paused'; \
            await wait(100); e.style.animationPlayState = 'running'; await wait(300); \
            const one = LOG.join(', '); LOG.length = 0; e.style.animation = 'none'; \
            await wait(20); e.style.animation = 'a 50ms linear'; await wait(20); \
            e.style.animation = 'b 50ms linear'; await wait(200); \
            return one + ' | ' + LOG.join(', '); })()";
        assert_eq!(
            value(&mut page, js),
            "\"animationstart a 0, animationend a 0.2 | animationstart a 0, \
             animationcancel a ~, animationstart b 0, animationend b 0.05\""
        );
    });
}

#[test]
fn events_of_a_style_change_without_a_read() {
    on_page_thread(|| {
        let mut page = load(EVENTS, true, true);
        value(
            &mut page,
            "document.getElementById('e').style.animation = 'a 50ms linear'",
        );
        page.run_event_loop(Duration::from_millis(20), Duration::from_secs(30));
        value(
            &mut page,
            "document.getElementById('e').style.animation = 'b 50ms linear'",
        );
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(30));
        assert_eq!(
            value(&mut page, "LOG.join(', ')"),
            "\"animationstart a 0, animationcancel a ~, animationstart b 0, animationend b 0.05\""
        );
    });
}

/// An infinite 1 s fill animation of a `circle` in an `<svg>` at `top`. The
/// fragment tree has a box for the `<svg>` and none for the `<circle>`.
fn svg_owner(top: u32) -> String {
    format!(
        "<!DOCTYPE html><style>body {{ margin: 0; height: 5000px }} \
         svg {{ position: absolute; top: {top}px }} \
         @keyframes k {{ from {{ fill: red }} to {{ fill: blue }} }} \
         circle {{ animation: k 1s linear infinite }}</style>\
         <svg width=100 height=100><circle cx=50 cy=50 r=40></circle></svg>"
    )
}

#[test]
fn an_owner_inside_an_svg_paints_through_the_svg_box() {
    on_page_thread(|| {
        for javascript in [false, true] {
            let mut page = load_painted(&svg_owner(100), javascript);
            let before = page.animation_updates();
            page.run_event_loop(Duration::from_secs(1), Duration::from_secs(30));
            let updates = page.animation_updates() - before;
            assert!(
                (55..=65).contains(&updates),
                "js {javascript}: {updates} updates in 1 s"
            );
            // The svg box out of the viewport: the owner costs no frames.
            let mut page = load_painted(&svg_owner(2000), javascript);
            let before = page.animation_updates();
            page.run_event_loop(Duration::from_secs(1), Duration::from_secs(30));
            let updates = page.animation_updates() - before;
            assert!(updates <= 3, "js {javascript}: {updates} updates in 1 s");
        }
    });
}

/// A frame document without scripts (`sandbox` without `allow-scripts`)
/// whose animation ends: its events have no task to take them, and are
/// dropped, so that no update stays due.
#[test]
fn a_frame_document_without_scripts_leaves_no_events() {
    on_page_thread(|| {
        let inner = "<style>@keyframes k { to { opacity: 0.5 } } \
            div { width: 20px; height: 20px; background: red; animation: k 100ms linear }\
            </style><div></div>";
        let html = format!(
            "<!DOCTYPE html><style>body {{ margin: 0 }} iframe {{ border: 0; width: 100px; \
             height: 100px }}</style><iframe sandbox srcdoc=\"{inner}\"></iframe>\
             <script>var a = 1</script>"
        );
        let mut page = load(&html, true, true);
        page.set_paints_frames(true);
        common::render_viewport(&mut page);
        page.run_event_loop(Duration::from_millis(600), Duration::from_secs(30));
        assert!(!page.animations_running(), "an update is still due");
        let before = page.animation_updates();
        page.run_event_loop(Duration::from_secs(1), Duration::from_secs(30));
        assert_eq!(page.animation_updates(), before);
    });
}

/// Four infinite animations whose `ready` and `finished` promises a script
/// watches, and a change after 300 ms that cancels three of them in one
/// update (`display: none`, a removed name, a removed element).
const PROMISE_ORDER: &str = "<!DOCTYPE html><style>@keyframes a { to { opacity: 0.5 } } \
    .x { width: 50px; height: 50px; background: red; animation: a 10s infinite }</style>\
    <div class=x id=d1></div><div class=x id=d2></div><div class=x id=d3></div>\
    <div class=x id=d4></div><script>\
    window.out = []; \
    window.addEventListener('unhandledrejection', (e) => out.push('UNHANDLED ' + e.reason)); \
    const as = document.getAnimations(); \
    as.forEach((a, i) => { a.ready.then(() => out.push('ready' + i)); \
      a.finished.then(() => out.push('fin' + i), (e) => out.push('rej' + i + ' ' + e.name)); }); \
    setTimeout(() => { d1.style.display = 'none'; d2.style.animationName = 'none'; \
      d3.remove(); }, 300);</script>";

/// Promises settle in the order of the animations' creation, in every run
/// (the waiting list is not a hash map; with virtual time a run repeats).
#[test]
fn promises_settle_in_the_order_of_creation() {
    on_page_thread(|| {
        let expected = "\"ready0 ready1 ready2 ready3 rej0 AbortError rej1 AbortError \
                        rej2 AbortError\"";
        for run in 0..8 {
            let mut page = load(PROMISE_ORDER, true, true);
            page.run_event_loop(Duration::from_millis(600), Duration::from_secs(30));
            assert_eq!(value(&mut page, "out.join(' ')"), expected, "run {run}");
        }
    });
}
