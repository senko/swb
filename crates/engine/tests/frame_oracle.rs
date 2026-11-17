//! The oracle for `Page::frame_outdated` (ADR 0040 section 2): scripted
//! input and tasks on page fixtures; whenever `frame_outdated()` is false, a
//! render gives the same pixels as the last render. A missed invalidation
//! shows as a difference here. The JavaScript runs use the fixtures in
//! `fixtures/js/` when they exist (they are not committed).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use swb_engine::{Key, Modifiers, MouseButton, Page, Size};

mod common;
use common::{SHIFT, load_fixture, on_page_thread, pump_for, render_viewport};

const VIEW: Size = Size::new(1280.0, 800.0);

/// Renders when the frame is outdated, else renders again and compares
/// with the last pixels. Returns the number of frames that were outdated.
struct Oracle {
    last: Option<Vec<u8>>,
    frames: usize,
    checks: usize,
}

impl Oracle {
    fn step(&mut self, page: &mut Page, label: &str) {
        let outdated = page.frame_outdated();
        let data = render_viewport(page).data().to_vec();
        if outdated {
            self.frames += 1;
        } else {
            self.checks += 1;
            if let Some(last) = &self.last {
                assert!(
                    *last == data,
                    "{label}: the frame was not outdated, but the pixels changed"
                );
            }
        }
        self.last = Some(data);
    }
}

/// The scripted session: moves, wheel, drags, clicks, keys, resize, each
/// followed by the event loop and an oracle step.
fn session(page: &mut Page, oracle: &mut Oracle) {
    let points = [
        (100.0, 100.0),
        (640.0, 200.0),
        (300.0, 400.0),
        (900.0, 300.0),
        (50.0, 700.0),
        (1200.0, 50.0),
    ];
    oracle.step(page, "first");
    pump_for(page, Duration::from_millis(150));
    oracle.step(page, "after the first tasks");
    for (i, &(x, y)) in points.iter().enumerate() {
        page.mouse_move(x, y);
        oracle.step(page, &format!("move {i}"));
        page.mouse_move(x + 1.0, y + 1.0);
        oracle.step(page, &format!("small move {i}"));
        pump_for(page, Duration::from_millis(20));
        oracle.step(page, &format!("tasks after move {i}"));
    }
    for (i, &(x, y)) in points.iter().enumerate() {
        page.wheel(x, y, 0.0, 250.0);
        oracle.step(page, &format!("wheel {i}"));
        pump_for(page, Duration::from_millis(20));
        oracle.step(page, &format!("tasks after wheel {i}"));
    }
    page.wheel(640.0, 400.0, 0.0, -10_000.0);
    oracle.step(page, "wheel to the top");
    // Drags and double clicks (selection), at points that are probably text.
    for (i, &(x, y)) in points.iter().enumerate().take(4) {
        page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 2);
        oracle.step(page, &format!("double press {i}"));
        page.mouse_up(x, y, MouseButton::Primary);
        oracle.step(page, &format!("double release {i}"));
        page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
        page.mouse_move(x + 150.0, y + 60.0);
        oracle.step(page, &format!("drag {i}"));
        page.mouse_up(x + 150.0, y + 60.0, MouseButton::Primary);
        oracle.step(page, &format!("drag end {i}"));
    }
    page.key_down(&Key::Character("a".into()), common::CTRL);
    oracle.step(page, "ctrl-a");
    page.clear_selection();
    oracle.step(page, "clear selection");
    for (i, (key, mods)) in [
        (Key::Tab, Modifiers::NONE),
        (Key::Tab, Modifiers::NONE),
        (Key::Tab, Modifiers::NONE),
        (Key::Tab, SHIFT),
        (Key::ArrowDown, Modifiers::NONE),
        (Key::PageDown, Modifiers::NONE),
        (Key::End, Modifiers::NONE),
        (Key::Home, Modifiers::NONE),
        (Key::Escape, Modifiers::NONE),
    ]
    .iter()
    .enumerate()
    {
        page.key_down(key, *mods);
        page.key_up(key, *mods);
        oracle.step(page, &format!("key {i}"));
        pump_for(page, Duration::from_millis(10));
        oracle.step(page, &format!("tasks after key {i}"));
    }
    page.set_viewport(Size::new(900.0, 700.0), 1.0);
    oracle.step(page, "resize");
    page.set_viewport(Size::new(900.0, 700.0), 2.0);
    oracle.step(page, "scale");
    pump_for(page, Duration::from_millis(100));
    oracle.step(page, "tasks at the end");
}

fn run_fixture(name: &str, javascript: bool) -> Option<(usize, usize)> {
    let mut page = load_fixture(name, javascript, VIEW, |page| {
        page.set_scroll_indicators(true);
    })?;
    let mut oracle = Oracle {
        last: None,
        frames: 0,
        checks: 0,
    };
    session(&mut page, &mut oracle);
    eprintln!(
        "{name} (js {javascript}): {} outdated frames, {} frames checked as current",
        oracle.frames, oracle.checks
    );
    Some((oracle.frames, oracle.checks))
}

#[test]
fn no_stale_frames_on_fixtures_without_javascript() {
    let runs = on_page_thread(|| {
        ["hacker-news", "wikipedia-web-browser", "github-swb"]
            .map(|name| run_fixture(name, false))
            .to_vec()
    });
    for (frames, checks) in runs.into_iter().flatten() {
        assert!(frames > 5 && checks > 5, "the session must do both");
    }
}

#[test]
fn no_stale_frames_on_fixtures_with_javascript() {
    let runs = on_page_thread(|| {
        ["hacker-news", "wikipedia-web-browser", "github-swb"]
            .map(|name| run_fixture(name, true))
            .to_vec()
    });
    for (frames, checks) in runs.into_iter().flatten() {
        assert!(frames > 5 && checks > 5, "the session must do both");
    }
}
