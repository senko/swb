//! The hover transitions of videojs.org (M13 feature 5, ADR 0044) on the
//! JavaScript fixture (`fixtures/js/videojs`, local; the test skips
//! without it): a hover over the search button and over a control of the
//! player starts `CSSTransition`s of the properties that the `:hover`
//! rules change; at their start their values are the values before the
//! hover, at their end the values after it; the events come as
//! `transitionrun`, `transitionstart`, `transitionend` per property. The
//! values between come from the probe rows (`css_transitions.rs`:
//! `hover-colors-after` and `hover-scale-translate-opacity` take the rules
//! of these buttons). The spring and the hero reveal of the page's scripts
//! are the probe rows `inline-style-spring` and `attribute-popup`.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use swb_engine::{ClockMode, Page, PageClock, Size};

mod common;
use common::{load_fixture, on_page_thread, value};

/// The computed values of `PROPS` of the element at the point.
const READ: &str = "(() => { const e = window.__t; const s = getComputedStyle(e); \
     return window.__props.map((p) => p + ':' + s.getPropertyValue(p)).join(';'); })()";

/// Hovers the element at (`x`, `y`), checks its transitions, and returns
/// their properties.
fn hover(page: &mut Page, x: f32, y: f32) -> String {
    value(
        page,
        &format!(
            "(() => {{ window.__t = document.elementFromPoint({x}, {y}).closest('button'); \
             window.__log = []; for (const t of ['transitionrun', 'transitionstart', \
             'transitionend', 'transitioncancel']) window.__t.addEventListener(t, (e) => \
             window.__log.push(e.type + ' ' + e.propertyName)); \
             window.__props = getComputedStyle(window.__t).transitionProperty.split(', ') \
             .flatMap((p) => p === 'border-color' ? ['border-top-color', 'border-left-color'] \
             : [p]); return window.__t.tagName; }})()"
        ),
    );
    let before = value(page, READ);
    page.mouse_move(x, y);
    page.run_event_loop(Duration::from_millis(1), Duration::from_secs(10));
    let list = value(
        page,
        "window.__t.getAnimations().map((a) => a.constructor.name + ' ' + \
         a.transitionProperty + ' ' + a.playState).join(', ')",
    );
    assert!(list.contains("CSSTransition"), "no transition: {list}");
    assert!(!list.contains("CSSAnimation"), "{list}");
    // At the start: the values before the hover.
    value(
        page,
        "window.__t.getAnimations().forEach((a) => { a.pause(); a.currentTime = 0; })",
    );
    assert_eq!(value(page, READ), before, "{list}");
    // At the end: the values of the hover, without a transition.
    value(
        page,
        "window.__t.getAnimations().forEach((a) => a.finish())",
    );
    let end = value(page, READ);
    page.run_event_loop(Duration::from_millis(500), Duration::from_secs(10));
    assert_eq!(value(page, "window.__t.getAnimations().length"), "0");
    assert_eq!(value(page, READ), end);
    assert_ne!(end, before, "the hover changes nothing: {list}");
    // Each property: run, start, end.
    let log = value(page, "window.__log.join('\\n')");
    for property in list.split(", ").filter_map(|a| a.split(' ').nth(1)) {
        let events: Vec<&str> = log
            .trim_matches('"')
            .split("\\n")
            .filter(|l| l.ends_with(&format!(" {property}")))
            .map(|l| l.split(' ').next().unwrap())
            .collect();
        assert_eq!(
            events,
            ["transitionrun", "transitionstart", "transitionend"],
            "{property}: {log}"
        );
    }
    list
}

#[test]
fn hovers_start_the_transitions_of_the_page() {
    let Some(lists) = on_page_thread(|| {
        let mut page = load_fixture("videojs", true, Size::new(1280.0, 800.0), |page| {
            page.set_animations(true);
            // The recording has Media Source Extensions (M15 in swb). With
            // media on, the player's HLS source fails and Video.js opens an
            // error dialog over the controls that the test hovers, as
            // Chromium does without `MediaSource` (the automation test
            // `without_media_source_the_player_fails_and_shows_its_error_dialog`).
            page.set_media(false);
            page.set_clock(PageClock::new(ClockMode::Virtual, None));
            page.set_paints_frames(true);
        })?;
        page.run_event_loop(Duration::from_millis(2000), Duration::from_secs(30));
        // The search button (border and background colors) and a control of
        // the player (colors, `outline-offset`, `scale`).
        let search = hover(&mut page, 806.0, 60.0);
        page.mouse_move(640.0, 400.0);
        page.run_event_loop(Duration::from_millis(500), Duration::from_secs(10));
        let control = hover(&mut page, 84.0, 680.0);
        Some((search, control))
    }) else {
        return;
    };
    eprintln!("{lists:?}");
}
