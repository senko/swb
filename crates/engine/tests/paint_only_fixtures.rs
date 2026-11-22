//! The check mode of the paint-only path (ADR 0044 section 9, M13 feature
//! 4a) on the videojs fixtures with their animations on: the hero reveal,
//! the marquee, the spinner and the bars through many animation ticks, in
//! virtual time with painted frames as the GUI runs them. Every frame whose
//! layout went through a paint-only refresh is compared with a full layout
//! and display list (fragment by fragment and item by item), and a
//! difference panics. The JS fixture is not committed: the test skips
//! without it.

#![cfg(debug_assertions)]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use swb_engine::{ClockMode, Page, PageClock, Size};

mod common;
use common::{load_fixture, on_page_thread, render_viewport};

const VIEW: Size = Size::new(1280.0, 800.0);

/// The frames of one run, from the counters.
struct Run {
    /// Run that ran the animation step and a render.
    frames: usize,
    refreshes: u32,
    fallbacks: u32,
    layouts: u32,
    checks: u64,
}

/// Loads the fixture with animations on and plays `ticks` frames of 33 ms
/// (with `scroll` at the given frames), each followed by a render.
fn play(name: &str, javascript: bool, ticks: usize, scroll: &[(usize, f32)]) -> Option<Run> {
    swb_engine::set_restyle_check(true);
    let mut page = load_fixture(name, javascript, VIEW, |page| {
        page.set_animations(true);
        page.set_clock(PageClock::new(ClockMode::Virtual, None));
        page.set_paints_frames(true);
    })?;
    render_viewport(&mut page);
    let before = page.flush_counters();
    let checks = swb_engine::paint_checks();
    let mut frames = 0;
    for tick in 0..ticks {
        if let Some(&(_, y)) = scroll.iter().find(|&&(at, _)| at == tick) {
            page.scroll_to(swb_engine::Point::new(0.0, y));
        }
        page.run_event_loop(Duration::from_millis(33), Duration::from_secs(30));
        if page.frame_outdated() {
            frames += 1;
        }
        render_viewport(&mut page);
    }
    let after = page.flush_counters();
    let result = Run {
        frames,
        refreshes: after.paint_refreshes - before.paint_refreshes,
        fallbacks: after.paint_fallbacks - before.paint_fallbacks,
        layouts: after.layout_runs - before.layout_runs,
        checks: swb_engine::paint_checks() - checks,
    };
    report(name, javascript, &page, &result);
    Some(result)
}

fn report(name: &str, javascript: bool, page: &Page, r: &Run) {
    eprintln!(
        "{name} (js {javascript}): {} frames, {} paint-only refreshes ({} compared with a full \
         layout), {} fallbacks, {} layouts, {} animations running: {}",
        r.frames,
        r.refreshes,
        r.checks,
        r.fallbacks,
        r.layouts,
        usize::from(page.animations_running()),
        page.flush_counters().state_restyles.restyled_elements
    );
}

#[test]
fn videojs_animations_without_scripts() {
    let result = on_page_thread(|| play("videojs", false, 90, &[(30, 600.0), (60, 0.0)]));
    let result = result.expect("the page fixture is in the repository");
    assert!(
        result.refreshes >= 20 && result.checks >= 20,
        "{} paint-only refreshes, {} compared",
        result.refreshes,
        result.checks
    );
}

#[test]
fn videojs_animations_with_scripts() {
    let result = on_page_thread(|| play("videojs", true, 90, &[(30, 600.0), (60, 0.0)]));
    let Some(result) = result else {
        eprintln!("skipped: no fixtures/js/videojs");
        return;
    };
    assert!(
        result.refreshes >= 20 && result.checks >= 20,
        "{} paint-only refreshes, {} compared",
        result.refreshes,
        result.checks
    );
}
