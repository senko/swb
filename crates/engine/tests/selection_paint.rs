//! The tie between the DOM's selection and the engine's highlight and copy
//! (M10 sub-feature 9b-2, ADR 0039 section 3): the paint cases of
//! `tools/probes/web-selection.json` against Chromium 148's pixels (a script's
//! range is painted with the highlight color, also with element boundary
//! points; a collapsed range paints nothing), the repaint after a script's
//! change without a DOM change, the copy of a script's selection, and the
//! user's selection seen by scripts and kept live by DOM changes. The other
//! cases of the probe run in `crates/automation/tests/selection.rs`.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_engine::{Modifiers, MouseButton, Page, Size};
use swb_net::NetworkFetcher;

mod common;
use common::{Site, answer, on_page_thread, repo_root, rgb};

const HIGHLIGHT: Rgb = (51, 103, 209);
const WHITE: Rgb = (255, 255, 255);

/// Chromium's pixels of the paint cases (three equal runs of `swbtools probe
/// --json`): case, element label, the color at each point.
/// A color.
type Rgb = (u8, u8, u8);

const CHROMIUM: &[(&str, &str, [Rgb; 6])] = &[
    (
        "paint-script-range",
        "#a",
        [WHITE, HIGHLIGHT, HIGHLIGHT, WHITE, WHITE, WHITE],
    ),
    ("paint-script-range", "#b", [WHITE; 6]),
    ("paint-element-range", "#a", [WHITE; 6]),
    (
        "paint-element-range",
        "#b",
        [HIGHLIGHT, HIGHLIGHT, WHITE, WHITE, WHITE, WHITE],
    ),
    ("paint-collapsed", "#a", [WHITE; 6]),
];

/// The points of the paint queries, relative to the element's box.
const POINTS: [(f32, f32); 6] = [
    (12.0, 2.0),
    (36.0, 2.0),
    (60.0, 2.0),
    (84.0, 2.0),
    (108.0, 2.0),
    (132.0, 2.0),
];

/// A page of 400x300 (as the probe) with JavaScript on `js`, loaded and
/// settled.
fn open(site: &Site, html: &str, js: bool) -> Page {
    let mut page = common::new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(400.0, 300.0));
    page.set_javascript(js);
    page.navigate(site.page("page.html", html));
    common::finish_loading(&mut page, Duration::from_secs(20));
    page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
    page
}

fn probe_case(name: &str) -> Value {
    let probe: Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("tools/probes/web-selection.json")).unwrap(),
    )
    .unwrap();
    probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
        .clone()
}

/// The colors at the probe's points of the element `id`.
fn colors(page: &mut Page, id: &str) -> [Rgb; 6] {
    let r = common::rect(page, id);
    let shot = page.screenshot(false).unwrap();
    POINTS.map(|(x, y)| rgb(&shot, (r.x + x) as u32, (r.y + y) as u32))
}

#[test]
fn script_ranges_paint_as_in_chromium() {
    on_page_thread(|| {
        let mut failures = Vec::new();
        for &(name, label, want) in CHROMIUM {
            let case = probe_case(name);
            let site = Site::new(&format!("selection-{name}"));
            let mut page = open(&site, case["html"].as_str().unwrap(), true);
            let got = colors(&mut page, label.trim_start_matches('#'));
            if got != want {
                failures.push(format!("{name} {label}: {got:?}, Chromium {want:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

const PAGE: &str = "<!DOCTYPE html><body style='margin:0'>\
    <span id=a style='font:40px/50px monospace;position:absolute;left:0;top:0'>MMMMMM</span>\
    <p id=p style='position:absolute;left:0;top:60px;margin:0;font:16px/20px sans-serif'>one <b>two</b> three</p>";

/// A script's change of the selection repaints without a DOM change, and
/// the copied text is the script's selection.
#[test]
fn a_script_selection_repaints_and_copies() {
    on_page_thread(|| {
        let site = Site::new("selection-repaint");
        let mut page = open(&site, PAGE, true);
        assert_eq!(colors(&mut page, "a")[1], WHITE);
        let set = "getSelection().setBaseAndExtent(a.firstChild, 1, a.firstChild, 3); 1";
        answer(&mut page, set);
        assert_eq!(
            colors(&mut page, "a"),
            [WHITE, HIGHLIGHT, HIGHLIGHT, WHITE, WHITE, WHITE]
        );
        assert_eq!(page.selected_text(), "MM");
        // A change of the selection's range object repaints too.
        answer(
            &mut page,
            "getSelection().getRangeAt(0).setEnd(a.firstChild, 5); 1",
        );
        assert_eq!(
            colors(&mut page, "a"),
            [WHITE, HIGHLIGHT, HIGHLIGHT, HIGHLIGHT, HIGHLIGHT, WHITE]
        );
        // Element boundary points across elements: the copied text is the
        // rendered text.
        answer(
            &mut page,
            "getSelection().setBaseAndExtent(document.body, 0, p, 3); 1",
        );
        assert_eq!(page.selected_text(), "MMMMMM\n\none two three");
        answer(&mut page, "getSelection().removeAllRanges(); 1");
        assert_eq!(colors(&mut page, "a"), [WHITE; 6]);
        assert_eq!(page.selected_text(), "");
    });
}

/// The user's drag is a range that scripts see, the live range steps keep it
/// where the DOM changes, and a removal of its text does not drop it.
#[test]
fn a_users_selection_is_a_live_range_of_the_dom() {
    on_page_thread(|| {
        let site = Site::new("selection-user");
        let mut page = open(&site, PAGE, true);
        let r = common::rect(&mut page, "a");
        let (y, x0, x1) = (r.y + 25.0, r.x + 30.0, r.x + 100.0);
        page.mouse_move(x0, y);
        page.mouse_down(x0, y, MouseButton::Primary, Modifiers::NONE, 1);
        page.mouse_move(x1, y);
        page.mouse_up(x1, y, MouseButton::Primary);
        let read = "(() => { const s = getSelection(); return [s.type, s.anchorNode === a.firstChild, s.anchorOffset, s.focusOffset, String(s)]; })()";
        assert_eq!(
            answer(&mut page, read).to_string(),
            r#"["Range",true,1,4,"MMM"]"#
        );
        assert_eq!(page.selected_text(), "MMM");
        // A data change before the selection moves it.
        answer(&mut page, "a.firstChild.insertData(0, 'xx'); 1");
        assert_eq!(page.selected_text(), "MMM");
        assert_eq!(
            answer(&mut page, "getSelection().anchorOffset").to_string(),
            "3"
        );
        // The removal of the text moves the boundary points to its parent.
        answer(&mut page, "a.firstChild.remove(); 1");
        assert_eq!(
            answer(&mut page, "[getSelection().rangeCount, getSelection().anchorNode === a, getSelection().isCollapsed]").to_string(),
            "[1,true,true]"
        );
        assert_eq!(page.selected_text(), "");
    });
}

/// A `getRangeAt` in a `selectionchange` listener during a drag (which gives
/// the selection's entry a new id) does not restart the drag: Chromium keeps
/// the selection (measured in the 9b-2 review). Only a script's replacement
/// of the selection restarts it.
#[test]
fn get_range_at_during_a_drag_keeps_the_drag() {
    on_page_thread(|| {
        let site = Site::new("selection-drag-get-range-at");
        let mut page = open(&site, PAGE, true);
        answer(
            &mut page,
            "document.addEventListener('selectionchange', () => getSelection().getRangeAt(0)); 1",
        );
        let r = common::rect(&mut page, "a");
        let (y, x0, x1, x2) = (r.y + 25.0, r.x + 30.0, r.x + 100.0, r.x + 200.0);
        page.mouse_move(x0, y);
        page.mouse_down(x0, y, MouseButton::Primary, Modifiers::NONE, 1);
        page.mouse_move(x1, y);
        page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
        page.mouse_move(x2, y);
        page.mouse_up(x2, y, MouseButton::Primary);
        assert_eq!(page.selected_text(), "MMMMM");
    });
}

/// The queue of selection events is bounded; a queue full of the `select`
/// events of a text control still takes the document's `selectionchange`
/// (one at most: it is coalesced).
#[test]
fn a_full_queue_of_select_events_keeps_the_documents_selectionchange() {
    on_page_thread(|| {
        let site = Site::new("selection-full-queue");
        let html = "<!DOCTYPE html><input id=i value=abcdef><p id=p>text</p>";
        let mut page = open(&site, html, true);
        let script = "window.n = 0; \
            document.addEventListener('selectionchange', e => { if (e.target === document) n++; }); \
            for (let k = 0; k < 5000; k++) i.setSelectionRange(0, 1 + (k % 2)); \
            getSelection().collapse(p.firstChild, 1); 1";
        answer(&mut page, script);
        page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
        assert_eq!(answer(&mut page, "n").to_string(), "1");
    });
}

/// Without scripts, select all selects from the first text to the end of the
/// body, and the copy is the rendered text.
#[test]
fn select_all_without_scripts() {
    on_page_thread(|| {
        let site = Site::new("selection-all");
        let mut page = open(&site, PAGE, false);
        assert!(page.select_all());
        assert!(page.selection().is_some());
        assert_eq!(page.selected_text(), "MMMMMM\n\none two three");
        assert_eq!(colors(&mut page, "a"), [HIGHLIGHT; 6]);
        assert!(page.clear_selection());
        assert!(page.selection().is_none());
    });
}
