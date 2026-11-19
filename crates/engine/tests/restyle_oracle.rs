//! The engine oracle of the targeted restyle (ADR 0041 section 6): with the
//! check mode on, every restyle after a state change that takes the
//! targeted path is compared with a full restyle (deep equality of all
//! styles, ordinals, `use` instances and the container log), and a
//! difference panics. The sessions play the `--bench` hover grid, presses,
//! Tab and Shift+Tab on the page fixtures, and on the JS fixtures when
//! `fixtures/js` exists (not committed), plus a page whose script moves the
//! focus and reads `getComputedStyle`. The check mode exists only with
//! debug assertions.
//!
//! The file also holds two tests of the restyle after DOM changes (ADR
//! 0042): `script_dom_changes_equal_full_restyles` runs scripts that change
//! the DOM in the same check mode, and `slot_changes_restyle_in_full` has
//! no oracle: it checks that slot changes restyle the document in full.

#![cfg(debug_assertions)]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Key, Modifiers, MouseButton, Page, RestylePaths, Size, Url};

mod common;
use common::{CASE_PAGE, OnePage, SHIFT, new_page, on_page_threads, value};

const VIEW: Size = Size::new(1280.0, 800.0);

fn render(page: &mut Page) {
    common::render_viewport(page);
}

/// The hover grid of `--bench` (8 x 6 points), two rounds, presses at some
/// points, then Tab and Shift+Tab; a render after each input. `ORACLE_GRID=CxR`
/// makes the grid finer.
fn session(page: &mut Page) {
    page.mouse_leave();
    render(page);
    let (columns, rows) = std::env::var("ORACLE_GRID")
        .ok()
        .and_then(|g| {
            let (c, r) = g.split_once('x')?;
            Some((c.parse().ok()?, r.parse().ok()?))
        })
        .unwrap_or((8, 6));
    for round in 0..2 {
        for row in 0..rows {
            for column in 0..columns {
                let x = VIEW.width * (column as f32 + 0.5) / columns as f32;
                let y = VIEW.height * (row as f32 + 0.5) / rows as f32;
                page.mouse_move(x, y);
                render(page);
                if (row * columns + column + round) % 7 == 0 {
                    page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
                    render(page);
                    page.mouse_up(x, y, MouseButton::Primary);
                    render(page);
                }
            }
        }
    }
    for i in 0..12 {
        let modifiers = if i % 4 == 3 { SHIFT } else { Modifiers::NONE };
        page.key_down(&Key::Tab, modifiers);
        page.key_up(&Key::Tab, modifiers);
        render(page);
    }
}

/// Loads a fixture and plays the session; the paths of its restyles, or
/// `None` without the fixture.
fn run_fixture(name: &str, javascript: bool) -> Option<(String, RestylePaths)> {
    let mut page = common::load_fixture(name, javascript, VIEW, |_| {})?;
    render(&mut page);
    let before = page.flush_counters().state_restyles;
    session(&mut page);
    let paths = page.flush_counters().state_restyles.since(&before);
    Some((format!("{name} (js {javascript})"), paths))
}

/// Checks that each fixture that loaded took the targeted path. Without
/// any fixture (the JS fixtures are not in the repository, so CI has none)
/// there is nothing to check.
fn check(runs: Vec<Option<(String, RestylePaths)>>) {
    let mut loaded = 0;
    for (name, paths) in runs.into_iter().flatten() {
        eprintln!("{name}: {paths:?}");
        assert!(
            paths.targeted + paths.nothing > 0,
            "{name}: no targeted restyle"
        );
        loaded += 1;
    }
    if loaded == 0 {
        eprintln!("skipped: no fixture");
        return;
    }
    assert!(swb_engine::restyle_checks() > 0);
}

#[test]
fn targeted_restyles_equal_full_restyles_on_page_fixtures() {
    swb_engine::set_restyle_check(true);
    let runs = on_page_threads(
        [
            "ars-technica",
            "bbc",
            "craigslist-posting",
            "github-swb",
            "hacker-news",
            "senko-net",
            "wikipedia-web-browser",
        ]
        .map(|name| move || run_fixture(name, false)),
    );
    // The page fixtures are in the repository: none may be missing.
    assert!(
        runs.iter().all(Option::is_some),
        "a page fixture is missing"
    );
    check(runs);
}

#[test]
fn targeted_restyles_equal_full_restyles_on_js_fixtures() {
    swb_engine::set_restyle_check(true);
    let runs = on_page_threads(
        ["github-swb", "ars-technica", "wikipedia-web-browser"]
            .map(|name| move || run_fixture(name, true)),
    );
    check(runs);
}

/// A script that moves the focus in a loop and reads `getComputedStyle`
/// between the moves: the values are those of the current focus, and the
/// flush after the task restyles from the states of the last styles.
#[test]
fn script_focus_and_computed_style() {
    swb_engine::set_restyle_check(true);
    common::on_page_thread(|| {
        let html = "<!doctype html><style>\
            button:focus { color: rgb(255, 0, 0) }\
            .row:focus-within > .label { color: rgb(0, 128, 0) }\
            .row:has(:focus-visible) { outline: 1px solid blue }\
            </style>\
            <div class=row><span class=label>a</span><button id=a>a</button></div>\
            <div class=row><span class=label>b</span><button id=b>b</button></div>";
        let fetcher = Arc::new(OnePage::new(html));
        let mut page = new_page(fetcher, 1, VIEW);
        page.set_javascript(true);
        page.navigate(Url::parse(CASE_PAGE).unwrap());
        assert!(page.wait_until_loaded(Duration::from_secs(10)));
        render(&mut page);
        let colors = value(
            &mut page,
            "const out = []; const a = document.getElementById('a'), \
             b = document.getElementById('b'); \
             for (let i = 0; i < 50; i++) { (i % 2 ? a : b).focus(); } \
             out.push(getComputedStyle(a).color, getComputedStyle(b).color); \
             a.focus(); \
             out.push(getComputedStyle(a).color, \
               getComputedStyle(document.querySelector('.label')).color); \
             a.blur(); out.push(getComputedStyle(a).color); JSON.stringify(out)",
        );
        assert_eq!(
            colors,
            "\"[\\\"rgb(255, 0, 0)\\\",\\\"rgb(0, 0, 0)\\\",\\\"rgb(255, 0, 0)\\\",\\\"rgb(0, 128, 0)\\\",\\\"rgb(0, 0, 0)\\\"]\""
        );
        let before = page.flush_counters().state_restyles;
        value(&mut page, "document.getElementById('b').focus(); 1");
        render(&mut page);
        let paths = page.flush_counters().state_restyles.since(&before);
        assert_eq!(paths.targeted, 1, "{paths:?}");
    });
    assert!(swb_engine::restyle_checks() > 0);
}

/// Loads `html` (with scripts) at 1280 x 800.
fn scripted_page(html: &'static str) -> Page {
    let fetcher = Arc::new(OnePage::new(html));
    let mut page = new_page(fetcher, 1, VIEW);
    page.set_javascript(true);
    page.navigate(Url::parse(CASE_PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(10)));
    render(&mut page);
    page
}

/// ADR 0042: a script that changes classes, attributes, text and the tree
/// in loops, with `getComputedStyle` and `getBoundingClientRect` between
/// the changes (forced flushes in the middle of a task). In check mode each
/// targeted restyle after DOM changes equals a full restyle.
#[test]
fn script_dom_changes_equal_full_restyles() {
    swb_engine::set_restyle_check(true);
    common::on_page_thread(|| {
        let mut page = scripted_page(
            "<!doctype html><style>\
             .open > .menu { display: block } .menu { display: none }\
             li:nth-child(odd) { color: rgb(1, 2, 3) } li + li { margin-top: 1px }\
             .list:has(.hot) { outline: 1px solid red } p:empty { height: 4px }\
             [aria-expanded=true] ~ .tip { color: rgb(0, 128, 0) }\
             ol > li { display: list-item }\
             </style>\
             <div id=nav><button id=b aria-expanded=false>m</button><span class=tip>t</span>\
             <div class=menu><a href=#>x</a></div></div>\
             <ul class=list id=l><li>a</li><li>b</li></ul><p id=p>t</p><ol id=o><li>1</li></ol>",
        );
        let before = page.flush_counters().dom_restyles;
        let out = value(
            &mut page,
            "const nav = document.getElementById('nav'), b = document.getElementById('b'), \
             l = document.getElementById('l'), p = document.getElementById('p'), \
             o = document.getElementById('o'); const out = []; \
             for (let i = 0; i < 20; i++) { \
               nav.classList.toggle('open'); b.setAttribute('aria-expanded', String(i % 2 == 0)); \
               out.push(getComputedStyle(nav.querySelector('.menu')).display); \
               const li = document.createElement('li'); li.textContent = 'n' + i; \
               if (i % 3 == 0) li.className = 'hot'; l.insertBefore(li, l.firstChild); \
               out.push(getComputedStyle(l).outlineStyle, getComputedStyle(li).color); \
               if (i % 4 == 3) { l.lastChild.remove(); o.append(document.createElement('li')); } \
               p.textContent = i % 2 ? '' : 'x'; \
               out.push(String(p.getBoundingClientRect().height > 0)); \
               if (i % 5 == 4) l.querySelector('.hot')?.remove(); \
             } \
             out.push(getComputedStyle(document.querySelector('.tip')).color); \
             JSON.stringify(out.slice(-6))",
        );
        assert!(out.contains("rgb"), "{out}");
        render(&mut page);
        let paths = page.flush_counters().dom_restyles.since(&before);
        eprintln!("script: {paths:?}");
        assert!(paths.targeted > 10, "{paths:?}");
    });
    assert!(swb_engine::restyle_checks() > 0);
}

/// ADR 0042 section 4: a change of the `slot` attribute of a host's child,
/// or of the `name` of a slot, changes the assigned nodes; the DOM notes a
/// shadow change, and the styles are computed in full (with `::slotted()`
/// rules that see the new assignment).
#[test]
fn slot_changes_restyle_in_full() {
    common::on_page_thread(|| {
        let mut page = scripted_page(
            "<!doctype html><div id=h><span id=c slot=a>c</span></div>\
             <script>const r = document.getElementById('h').attachShadow({mode: 'open'}); \
             r.innerHTML = '<style>::slotted(*) { color: rgb(0, 0, 255) } \
             slot[name=b]::slotted(*) { color: rgb(255, 0, 0) }</style>\
             <slot name=a></slot><slot name=b></slot>';</script>",
        );
        let color =
            |page: &mut Page| value(page, "getComputedStyle(document.getElementById('c')).color");
        assert_eq!(color(&mut page), "\"rgb(0, 0, 255)\"");
        let before = page.flush_counters();
        value(&mut page, "document.getElementById('c').slot = 'b'; 1");
        render(&mut page);
        assert_eq!(color(&mut page), "\"rgb(255, 0, 0)\"");
        value(
            &mut page,
            "document.getElementById('h').shadowRoot.querySelector('slot[name=b]').name = 'x'; 1",
        );
        render(&mut page);
        assert_eq!(color(&mut page), "\"rgb(0, 0, 0)\"");
        let after = page.flush_counters();
        let dom = after.dom_restyles.since(&before.dom_restyles);
        assert_eq!(dom.targeted + dom.nothing, 0, "{dom:?}");
        assert!(after.style_runs >= before.style_runs + 2);
    });
}
