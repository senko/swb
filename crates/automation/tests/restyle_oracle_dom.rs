//! The engine oracle of the targeted restyle after DOM changes (ADR 0042
//! section 7): with the check mode on, every targeted restyle (after state
//! or DOM changes) is compared with a full restyle, and a difference
//! panics. The sessions are the steps of the JS fixtures' integration tests
//! (GitHub's interaction steps, Ars Technica, Wikipedia, BBC, Craigslist),
//! loaded as those tests load them. The fixtures are in `fixtures/js` (not
//! committed); without them the test skips. The check mode exists only with
//! debug assertions.

#![cfg(debug_assertions)]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use swb_automation::run_steps;

mod common;
use common::{fixture, load_at_capture, on_page_thread, step_groups};

/// Loads the fixture `name`, plays `steps` in check mode and returns the
/// paths of the restyles after DOM changes; `None` without the fixture.
fn play(name: &str, steps_text: &'static str) -> Option<swb_engine::RestylePaths> {
    let dir = fixture(name)?;
    let (tx, rx) = std::sync::mpsc::channel();
    on_page_thread(move || {
        swb_engine::set_restyle_check(true);
        let mut page = load_at_capture(&dir, Duration::from_millis(1500));
        let before = page.flush_counters().dom_restyles;
        let start = page.flush_counters();
        // A step that fails (a selector that the page does not have at
        // this point) does not stop the session.
        for (_, steps) in step_groups(steps_text) {
            for step in steps {
                let _ = run_steps(&mut page, std::slice::from_ref(&step));
            }
        }
        // A step can navigate: the counters are then those of the new
        // document.
        let end = page.flush_counters();
        eprintln!(
            "{}: style runs {}",
            dir.display(),
            end.style_runs.saturating_sub(start.style_runs)
        );
        tx.send(page.flush_counters().dom_restyles.since(&before))
            .unwrap();
    });
    rx.recv().ok()
}

#[test]
fn dom_restyles_equal_full_restyles_on_js_fixture_steps() {
    let runs = [
        (
            "github-swb",
            include_str!("data/github-swb-interactions-steps.json"),
        ),
        ("ars-technica", include_str!("data/ars-technica-steps.json")),
        (
            "wikipedia-web-browser",
            include_str!("data/wikipedia-web-browser-steps.json"),
        ),
        ("bbc", include_str!("data/bbc-steps.json")),
        (
            "craigslist-posting",
            include_str!("data/craigslist-posting-steps.json"),
        ),
    ];
    let checks = swb_engine::restyle_checks();
    let mut any = false;
    for (name, text) in runs {
        let Some(paths) = play(name, text) else {
            eprintln!("skipped: fixtures/js/{name} is missing");
            continue;
        };
        eprintln!("{name}: {paths:?}");
        any = true;
        // GitHub's steps stay on the page; the others can navigate.
        if name == "github-swb" {
            assert!(paths.targeted > 0, "{name}: no targeted restyle");
        }
    }
    if any {
        assert!(swb_engine::restyle_checks() > checks);
    }
}
