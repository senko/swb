//! The hover state and the boundary events after the content under a
//! still pointer changes (M12 feature 3a, Chromium's fake mouse move): the
//! rendering update hit-tests once, only after something that can change
//! the result changed, and sends the events of a real move without
//! `pointermove` and `mousemove`. The Chromium values of the cases are in
//! `crates/automation/tests/events_after_layout.rs`; these tests check the
//! counts and the bounds that a probe cannot see.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{
    ClockMode, Evaluation, JsonForm, Modifiers, MouseButton, Page, PageClock, Size, Url,
};

use common::{OnePage, new_page, on_page_thread};

/// A page with JavaScript on and virtual time that shows `html`, loaded.
fn open(html: &'static str) -> Page {
    let mut page = new_page(Arc::new(OnePage::new(html)), 2, Size::new(800.0, 600.0));
    page.set_javascript(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.navigate(Url::parse(common::CASE_PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// The value of an expression, as JSON text.
fn eval(page: &mut Page, expression: &str) -> String {
    match page.evaluate(expression, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{expression}: {other:?}"),
    }
}

/// Runs the event loop for `ms` of page time.
fn wait(page: &mut Page, ms: u64) {
    page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(10));
}

/// A page with two stacked links (40 px each, at 20 px from the top) and a
/// log of the pointer events at the document.
const TWO_LINKS: &str = "<!DOCTYPE html><style>body{margin:0}#box{position:absolute;left:20px;top:20px;width:200px}\
a{display:block;height:40px}.rev{display:flex;flex-direction:column-reverse}</style>\
<div id=box><a id=a href=#a>a</a><a id=b href=#b>b</a></div><script>\
window.log = [];\
for (const t of ['pointerover', 'pointerout', 'pointerenter', 'pointerleave', 'mouseover', 'mouseout', 'mousemove', 'pointermove']) \
document.addEventListener(t, (e) => log.push(t + ':' + (e.target.id || e.target.nodeName) + (e.relatedTarget ? '<' + (e.relatedTarget.id || e.relatedTarget.nodeName) : '')), true);\
</script>";

fn log_of(page: &mut Page) -> String {
    eval(page, "log.join(' ')").trim_matches('"').to_owned()
}

#[test]
fn a_page_that_changes_nothing_gets_no_events() {
    on_page_thread(|| {
        let mut page = open(TWO_LINKS);
        page.mouse_move(60.0, 30.0);
        wait(&mut page, 100);
        let before = log_of(&mut page);
        assert!(before.contains("pointerover:a"), "{before}"); // Tasks that change attributes the pointer does not see, a scroll
        // that does not move, time passing: nothing new.
        eval(&mut page, "document.body.style.color = 'red'; 0");
        wait(&mut page, 100);
        eval(&mut page, "scrollTo(0, 0); 0");
        wait(&mut page, 100);
        assert_eq!(log_of(&mut page), before);
    });
}

#[test]
fn a_class_that_moves_the_hovered_link_away_sends_the_events_without_a_move() {
    on_page_thread(|| {
        let mut page = open(TWO_LINKS);
        page.mouse_move(60.0, 30.0);
        wait(&mut page, 100);
        eval(&mut page, "log.length = 0; box.classList.add('rev'); 0");
        // Not in the same task.
        assert_eq!(log_of(&mut page), "");
        wait(&mut page, 40);
        assert_eq!(
            log_of(&mut page),
            "pointerout:a<b pointerleave:a<b pointerover:b<a pointerenter:b<a mouseout:a<b mouseover:b<a"
        );
        assert_eq!(
            eval(&mut page, "document.querySelector('a:hover').id"),
            "\"b\""
        );
        // The hover state stays: no further events.
        wait(&mut page, 200);
        assert_eq!(eval(&mut page, "log.length"), "6");
    });
}

#[test]
fn a_removed_hovered_link_gives_the_new_element_the_over_with_the_old_one_as_related_target() {
    on_page_thread(|| {
        let mut page = open(TWO_LINKS);
        page.mouse_move(60.0, 30.0);
        wait(&mut page, 100);
        eval(&mut page, "log.length = 0; a.remove(); 0");
        wait(&mut page, 40);
        // `a` is gone and `b` moved up: it is under the pointer; the old
        // target gets no `out`.
        let log = log_of(&mut page);
        assert!(log.starts_with("pointerover:b<a"), "{log}");
        assert!(!log.contains("pointerout"), "{log}");
        assert!(!log.contains("move"), "{log}");
    });
}

#[test]
fn a_script_scroll_updates_the_hover_after_the_task() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>body{margin:0}a{display:block;height:40px}</style>\
<div id=c></div><script>\
for (let i = 0; i < 30; i++) { const a = document.createElement('a'); a.id = 'r' + i; a.href = '#'; c.appendChild(a); }\
window.log = [];\
document.addEventListener('mouseover', (e) => log.push(e.target.id), true);\
</script>";
        let mut page = open(html);
        page.mouse_move(60.0, 50.0);
        wait(&mut page, 100);
        eval(&mut page, "log.length = 0; scrollTo(0, 80); 0");
        wait(&mut page, 40);
        assert_eq!(eval(&mut page, "log.join()"), "\"r3\"");
        assert_eq!(
            eval(&mut page, "document.querySelector('a:hover').id"),
            "\"r3\""
        );
    });
}

#[test]
fn the_pointer_outside_the_page_gets_nothing() {
    on_page_thread(|| {
        let mut page = open(TWO_LINKS);
        // No pointer: a change finds nothing to hover.
        eval(&mut page, "box.classList.add('rev'); 0");
        wait(&mut page, 100);
        assert_eq!(log_of(&mut page), "");
        // The pointer left the page: the same.
        page.mouse_move(60.0, 30.0);
        wait(&mut page, 100);
        page.mouse_leave();
        eval(&mut page, "log.length = 0; box.classList.remove('rev'); 0");
        wait(&mut page, 100);
        assert_eq!(log_of(&mut page), "");
    });
}

#[test]
fn a_press_keeps_the_events_and_the_release_goes_to_the_new_element() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>body{margin:0}#box{position:absolute;left:20px;top:20px;width:200px}\
a{display:block;height:40px}.rev{display:flex;flex-direction:column-reverse}</style>\
<div id=box><a id=a href=#a>a</a><a id=b href=#b>b</a></div><script>\
window.log = [];\
for (const t of ['pointerover', 'pointerout', 'mouseover', 'mouseup', 'click']) \
document.addEventListener(t, (e) => log.push(t + ':' + (e.target.id || e.target.nodeName)), true);\
</script>";
        let mut page = open(html);
        page.mouse_move(60.0, 30.0);
        wait(&mut page, 100);
        page.mouse_down(60.0, 30.0, MouseButton::Primary, Modifiers::default(), 1);
        eval(&mut page, "log.length = 0; box.classList.add('rev'); 0");
        wait(&mut page, 40);
        assert_eq!(
            eval(&mut page, "log.join()"),
            "\"pointerout:a,pointerover:b,mouseover:b\""
        );
        // :active stays on the pressed link (Chromium, measured).
        assert_eq!(
            eval(&mut page, "document.querySelector('a:active').id"),
            "\"a\""
        );
        page.mouse_up(60.0, 30.0, MouseButton::Primary);
        wait(&mut page, 40);
        let log = eval(&mut page, "log.join()");
        assert!(log.contains("mouseup:b"), "{log}");
    });
}

/// A page whose `mouseover` handler moves the element away: the next update
/// hovers the other link, whose handler moves it back. One update per
/// rendering update: the events grow by a bounded number per frame.
#[test]
fn a_ping_pong_page_gets_one_update_per_frame() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>body{margin:0}#box{position:absolute;left:20px;top:20px;width:200px}\
a{display:block;height:40px}.rev{display:flex;flex-direction:column-reverse}</style>\
<div id=box><a id=a href=#a>a</a><a id=b href=#b>b</a></div><script>\
window.count = 0;\
document.addEventListener('mouseover', () => { count++; box.classList.toggle('rev'); }, true);\
</script>";
        let mut page = open(html);
        page.mouse_move(60.0, 30.0);
        // Ten frames.
        wait(&mut page, 170);
        let count: u32 = eval(&mut page, "count").parse().unwrap();
        assert!((2..=14).contains(&count), "{count}");
    });
}

/// The same for a page that replaces the element under the pointer in
/// every animation frame.
#[test]
fn replacing_the_element_every_frame_gets_one_update_per_frame() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>body{margin:0}#box{position:absolute;left:20px;top:20px;width:200px}\
a{display:block;height:40px}</style><div id=box><a href=#>x</a></div><script>\
window.count = 0; window.frames_run = 0;\
document.addEventListener('mouseover', () => { count++; }, true);\
const tick = () => { frames_run++; box.innerHTML = '<a href=#>y</a>'; requestAnimationFrame(tick); };\
requestAnimationFrame(tick);\
</script>";
        let mut page = open(html);
        page.mouse_move(60.0, 30.0);
        wait(&mut page, 340);
        let frames: u32 = eval(&mut page, "frames_run").parse().unwrap();
        let count: u32 = eval(&mut page, "count").parse().unwrap();
        assert!(frames >= 15, "{frames}");
        assert!(count <= frames + 2, "{count} {frames}");
    });
}

/// The document of a frame gets the update too.
#[test]
fn the_document_of_a_frame_gets_the_events_after_its_content_changes() {
    on_page_thread(|| {
        let inner = "<!DOCTYPE html><style>body{margin:0}a{display:block;height:40px}</style>\
<div id=box><a id=a href=#a>a</a></div><script>\
window.log = [];\
document.addEventListener('mouseover', (e) => log.push(e.target.id), true);\
</script>";
        let html = format!(
            "<!DOCTYPE html><style>body{{margin:0}}iframe{{position:absolute;left:0;top:0;width:300px;height:200px;border:0}}</style>\
<iframe id=f srcdoc='{}'></iframe>",
            inner.replace('\'', "&#39;")
        );
        let html: &'static str = Box::leak(html.into_boxed_str());
        let mut page = open(html);
        wait(&mut page, 100);
        page.mouse_move(60.0, 20.0);
        wait(&mut page, 100);
        let log = eval(&mut page, "f.contentWindow.log.join()");
        assert_eq!(log, "\"a\"");
        eval(
            &mut page,
            "f.contentDocument.getElementById('box').innerHTML = '<a id=b href=#b>b</a>'; 0",
        );
        wait(&mut page, 60);
        assert_eq!(eval(&mut page, "f.contentWindow.log.join()"), "\"a,b\"");
    });
}

/// A change under the pointer during a press that began in a frame is only
/// noticed after the release: the update that comes during the press keeps
/// the hover state, and the release marks the hover stale again, so the
/// next update finds the new element without a pointer move.
#[test]
fn a_change_during_a_press_in_a_frame_is_seen_after_the_release() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>body{margin:0}iframe{position:absolute;left:0;top:0;width:300px;height:200px;border:0}</style>\
<iframe id=f srcdoc='<p>x</p>'></iframe><script>\
window.log = [];\
document.addEventListener('mouseover', (e) => log.push(e.target.id || e.target.nodeName), true);\
</script>";
        let mut page = open(html);
        wait(&mut page, 100);
        page.mouse_move(60.0, 20.0);
        wait(&mut page, 100);
        assert_eq!(eval(&mut page, "log.join()"), "\"f\"");
        page.mouse_down(60.0, 20.0, MouseButton::Primary, Modifiers::default(), 1);
        eval(&mut page, "f.style.display = 'none'; 0");
        wait(&mut page, 60);
        // The press keeps the hover state.
        assert_eq!(eval(&mut page, "log.join()"), "\"f\"");
        page.mouse_up(60.0, 20.0, MouseButton::Primary);
        wait(&mut page, 60);
        assert_eq!(eval(&mut page, "log.join()"), "\"f,HTML\"");
    });
}
