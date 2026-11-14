//! M10 sub-feature 3b-2 (ADR 0034 "3b-2 implementation notes") for the front
//! page of github.com/senko/swb with JavaScript on: the local fixture
//! `fixtures/js/github-swb` (git-ignored; set `SWB_JS_FIXTURES` to the
//! directory `fixtures/js` of another checkout). The page has an inline import
//! map (`react`, `react-dom` and others), 111 `modulepreload` links and 9
//! module scripts whose graphs have 95 modules. The test checks that
//!
//! - the 9 module scripts ran, in document order (Chromium's order, ADR 0034
//!   section 10), each once;
//! - no module script failed to load, link or parse, and the console has no
//!   message about a module or an import map.
//!
//! How the order shows: swb logs `module script URL: run` at the `debug` level
//! when a module script's graph is evaluated and `... : error to rethrow`
//! when its graph has a parse, resolution or link error
//! (`scripts/modules.rs`); the test installs a logger that keeps those lines.
//! The expected order is the order of the `script type=module src` elements in
//! the document. The test skips when the fixture is missing.
//!
//! The same load also counts the custom element callbacks (ADR 0035
//! sub-feature 4b; `custom_elements/reactions.rs` logs each callback at the
//! `debug` level) and compares them with Chromium's replay, and reads the
//! shadow roots of `relative-time` and `tool-tip` (ADR 0036 sub-feature 5a),
//! their sheets and adopted sheets (5c-1, 5c-2) and the text that renders in
//! the `relative-time` hosts (sub-feature 5b).
//! One test does all: the logger is global, and two loads at once would mix
//! their lines.
//!
//! `github_swb.rs` checks the first view of the same page (rectangles,
//! texts, islands, pixels, console) against Chromium's record.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Mutex;
use std::time::Duration;

use common::{console_errors, fixture, load_at_capture, on_page_thread, value};

/// The lines of the module script log.
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

/// The module path of the logger calls that the module test keeps.
const MODULE_LOG: &str = "swb_web::scripts::modules";

/// The module path prefix of the custom element logs.
const CUSTOM_LOG: &str = "swb_web::custom_elements";

impl log::Log for Capture {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == MODULE_LOG || metadata.target().starts_with(CUSTOM_LOG)
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            LINES.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

/// Words of the console messages about modules and import maps.
const MODULE_MESSAGES: &[&str] = &[
    "module specifier",
    "does not provide an export",
    "dynamically imported module",
    "module script",
    "import map",
    "Failed to load module",
];

#[test]
fn github_module_scripts_run_in_document_order_and_custom_element_callbacks_match() {
    let Some(dir) = fixture("github-swb") else {
        eprintln!("skipped: fixtures/js/github-swb is missing");
        return;
    };
    on_page_thread(move || {
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Debug);
        let mut page = load_at_capture(&dir, Duration::from_millis(1500));
        assert!(page.has_script_runtime());
        let expected: Vec<String> = serde_json::from_value(value(
            &mut page,
            "Array.from(document.querySelectorAll('script[type=module][src]')).map(s => s.src)",
        ))
        .unwrap();
        assert_eq!(expected.len(), 9, "{expected:?}");
        let lines = LINES.lock().unwrap().clone();
        let errors: Vec<&String> = lines
            .iter()
            .filter(|line| line.ends_with(": error to rethrow"))
            .collect();
        assert!(errors.is_empty(), "module scripts with errors: {errors:?}");
        let ran: Vec<String> = lines
            .iter()
            .filter_map(|line| line.strip_suffix(": run"))
            .filter_map(|line| line.strip_prefix("module script "))
            .map(str::to_owned)
            .collect();
        assert_eq!(ran, expected);
        let module_errors: Vec<String> = console_errors(&page)
            .into_iter()
            .filter(|text| MODULE_MESSAGES.iter().any(|word| text.contains(word)))
            .collect();
        assert!(
            module_errors.is_empty(),
            "console errors about modules: {module_errors:?}"
        );
        custom_element_callbacks(&lines);
        shadow_roots(&value(&mut page, SHADOW_ROOTS));
        rendered_times(&mut page);
        let errors_before = console_errors(&page).len();
        hover_shows_the_tooltip_in_the_top_layer(&mut page);
        tab_order_has_no_shadow_content(&mut page);
        click_on_relative_time_follows_the_link(&mut page);
        // The input reaches page scripts that call APIs swb lacks
        // (`createTreeWalker`, `getSelection`, a fetch with no recording):
        // the base commit logs the same messages. No other error is new.
        let new: Vec<String> = console_errors(&page)[errors_before..]
            .iter()
            .filter(|e| {
                !["createTreeWalker", "getSelection", "Failed to fetch"]
                    .iter()
                    .any(|known| e.contains(known))
            })
            .cloned()
            .collect();
        assert!(new.is_empty(), "{new:?}");
    });
}

/// The Tab order of the page (ADR 0036 sub-feature 5d): the shadow trees of
/// `relative-time` and `tool-tip` have no focusable content, so no element
/// of the sequence is in a shadow tree and the sequence has no element that
/// is not in the document tree. Prints the sequence for the notes.
fn tab_order_has_no_shadow_content(page: &mut swb_engine::Page) {
    let mut sequence = Vec::new();
    for _ in 0..60 {
        page.key_down(&swb_engine::Key::Tab, swb_engine::Modifiers::NONE);
        let Some(node) = page.focused_element() else {
            break;
        };
        let doc = page.document().unwrap();
        assert!(!doc.in_shadow_tree(node), "a shadow element has the focus");
        sequence.push(node);
    }
    let doc = page.document().unwrap();
    let names: Vec<String> = sequence
        .iter()
        .map(|&n| {
            let e = doc.element(n).unwrap();
            format!("{}#{}", e.local_name(), e.id().unwrap_or(""))
        })
        .collect();
    eprintln!("github tab order ({}): {names:?}", names.len());
    assert!(names.len() > 10, "{names:?}");
}

/// A hover over the "..." button of the latest-commit line shows its
/// `TooltipV2` popover (`showPopover` from a React handler) in the top layer
/// at Chromium's box: 125 x 28 at (166, 358) in the viewport (ADR 0037
/// "What GitHub needs", the replay's "hover dots" step). The box is the
/// last item of the paint order, so a hit test at its center finds it. A
/// click on the button hides it by light dismiss (sub-feature 6c); Escape
/// closes whatever a click opened.
fn hover_shows_the_tooltip_in_the_top_layer(page: &mut swb_engine::Page) {
    let at = value(
        page,
        "(() => { const r = document.querySelector('[data-testid=latest-commit-details-toggle]')\
           .getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()",
    );
    page.mouse_move(
        at[0].as_f64().unwrap() as f32,
        at[1].as_f64().unwrap() as f32,
    );
    page.run_event_loop(Duration::from_millis(1000), Duration::from_secs(10));
    let open = value(
        page,
        "(() => { const o = Array.from(document.querySelectorAll('[popover]'))\
           .filter(e => e.matches(':popover-open')); \
           return o.map(e => { const r = e.getBoundingClientRect(); \
             const c = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2); \
             return [e.localName + '.' + e.className.split(' ')[0], r.x, r.y, r.width, r.height, \
               e.contains(c)]; }); })()",
    );
    let open = open.as_array().unwrap();
    assert_eq!(open.len(), 1, "one open popover: {open:?}");
    let tip = &open[0];
    assert_eq!(tip[0], "span.prc-TooltipV2-Tooltip-tLeuB", "{tip}");
    let rect: Vec<f64> = (1..5).map(|i| tip[i].as_f64().unwrap()).collect();
    let want = [166.0, 358.0, 125.0, 28.0];
    assert!(
        rect.iter().zip(want).all(|(a, b)| (a - b).abs() <= 1.0),
        "the tooltip's box: {rect:?}, Chromium {want:?}"
    );
    assert_eq!(tip[5], true, "the tooltip is on top at its center: {tip}");
    // The click on the button hides the tooltip by light dismiss (ADR 0037
    // section 4; in the replay: `beforetoggle` `open>closed` without a call
    // of `hidePopover`). The page also expands the commit message.
    let (x, y) = (
        at[0].as_f64().unwrap() as f32,
        at[1].as_f64().unwrap() as f32,
    );
    let toggles = "(window.__tt = []), document.addEventListener('beforetoggle', \
        e => window.__tt.push(e.newState), true), 1";
    value(page, toggles);
    page.click(x, y);
    page.run_event_loop(Duration::from_millis(500), Duration::from_secs(10));
    let after = value(
        page,
        "[document.querySelectorAll(':popover-open').length, window.__tt.join('>')]",
    );
    assert_eq!(after[0], 0, "the click hid the tooltip: {after}");
    assert!(
        after[1].as_str().unwrap().starts_with("closed"),
        "a `beforetoggle` to the closed state without a call: {after}"
    );
    // A second click collapses the message again (the Tab order below is the
    // page's usual); a tooltip that it shows goes away with Escape.
    page.click(x, y);
    page.run_event_loop(Duration::from_millis(500), Duration::from_secs(10));
    page.key_down(&swb_engine::Key::Escape, swb_engine::Modifiers::NONE);
    page.run_event_loop(Duration::from_millis(200), Duration::from_secs(10));
    let open = value(page, "document.querySelectorAll(':popover-open').length");
    assert_eq!(open, 0, "no popover is open after Escape");
    page.mouse_move(1.0, 1.0);
    page.run_event_loop(Duration::from_millis(200), Duration::from_secs(10));
}

/// A click on a `relative-time` inside a link follows the link (ADR 0036
/// sub-feature 5d): the target of the click is the span in the shadow tree,
/// and the activation target is the link around the host. The fixture has
/// no such link (GitHub's relative times are not in links on this page), so
/// the test wraps the third one in a link, as the commit lists of GitHub do.
fn click_on_relative_time_follows_the_link(page: &mut swb_engine::Page) {
    value(
        page,
        "(() => { const e = document.querySelectorAll('relative-time')[2]; \
           const a = document.createElement('a'); a.id = 'wrap'; a.href = '/senko/swb/commits/x'; \
           e.parentNode.insertBefore(a, e); a.appendChild(e); \
           e.shadowRoot.querySelector('span[part=root]').scrollIntoView({block: 'center'}); \
           return 1; })()",
    );
    page.run_event_loop(Duration::from_millis(100), Duration::from_secs(10));
    let at = value(
        page,
        "(() => { const e = document.getElementById('wrap').firstChild; \
           const r = e.shadowRoot.querySelector('span[part=root]').getBoundingClientRect(); \
           window.clicks = []; \
           document.addEventListener('click', ev => clicks.push(ev.target.localName + '<' + \
             ev.composedPath()[0].localName)); \
           return [r.x + r.width / 2, r.y + r.height / 2, r.width]; })()",
    );
    assert!(at[2].as_f64().unwrap() > 0.0, "the time has a box: {at}");
    let (x, y) = (
        at[0].as_f64().unwrap() as f32,
        at[1].as_f64().unwrap() as f32,
    );
    page.click(x, y);
    assert_eq!(
        value(page, "clicks.join()"),
        "relative-time<span",
        "the listener on the document sees the host; the path starts at the span"
    );
    let url = page.url().unwrap();
    assert_eq!(url.path(), "/senko/swb/commits/x", "{url}");
}

/// The text that renders in each `relative-time` host (ADR 0036 sub-feature
/// 5b): the text fragments whose node is in the host's shadow tree, in
/// document order of the hosts. The light text of a host (the date that
/// the server wrote, "Oct 18, 2026") is outside the flat tree and has no
/// fragment.
fn rendered_times(page: &mut swb_engine::Page) {
    page.update_layout();
    let hosts = page.query_selector_all("relative-time").unwrap();
    assert_eq!(hosts.len(), 23);
    let doc = page.document().unwrap();
    let mut texts = vec![String::new(); hosts.len()];
    let mut light = Vec::new();
    page.fragments().unwrap().walk(|f, _| {
        let swb_layout::FragmentRef::Text(t) = f else {
            return;
        };
        if let Some(i) = hosts.iter().position(|&h| doc.parent(t.node) == Some(h)) {
            light.push((i, t.text.to_string()));
        }
        let host = doc
            .shadow_including_ancestors(t.node)
            .find(|a| hosts.contains(a));
        if let Some(i) = host.and_then(|h| hosts.iter().position(|&x| x == h)) {
            texts[i].push_str(&t.text);
        }
    });
    assert!(light.is_empty(), "light text renders: {light:?}");
    let shown: Vec<String> = texts.iter().map(|t| t.trim().to_owned()).collect();
    // The first one is in a `span.d-sm-none`, which the page's style sheet
    // hides at this viewport width (as in Chromium).
    let mut want: Vec<&str> = REPLAY_TIMES.to_vec();
    want[0] = "";
    assert_eq!(shown, want);
}

/// The shadow roots of the page as the test reads them: the number of hosts,
/// the text of the `part="root"` span of each `relative-time` root in document
/// order, and the `style` and `slot` children of `tool-tip`'s root with the
/// text that the slot gets.
const SHADOW_ROOTS: &str = "(() => {
    const all = Array.from(document.querySelectorAll('*'));
    const hosts = all.filter(e => e.shadowRoot);
    const times = Array.from(document.querySelectorAll('relative-time')).map(e => {
        const span = e.shadowRoot && e.shadowRoot.querySelector('span[part=root]');
        return span ? span.textContent : null;
    });
    const tip = document.querySelector('tool-tip');
    const root = tip && tip.shadowRoot;
    const slot = root && root.querySelector('slot');
    const sheets = hosts.map(h => Array.from(h.shadowRoot.querySelectorAll('style'))
        .map(s => s.textContent.includes('--tooltip-top') ? 'own' : 'polyfill').join('+'));
    const tipStyle = tip && getComputedStyle(tip);
    return {
        hosts: hosts.length,
        sheets: sheets.reduce((counts, s) => { counts[s] = (counts[s] || 0) + 1; return counts; }, {}),
        rootSheets: hosts.map(h => h.shadowRoot.styleSheets.length).reduce((a, b) => a + b, 0),
        adopted: hosts.map(h => h.localName + ':' + h.shadowRoot.adoptedStyleSheets.length)
            .filter(t => !t.endsWith(':0')),
        documentAdopted: document.adoptedStyleSheets.length,
        adoptedSheets: new Set(hosts.map(h => h.shadowRoot.adoptedStyleSheets)
            .concat([document.adoptedStyleSheets]).flatMap(a => Array.from(a))).size,
        popoverPolyfill: ![Element.prototype.matches, Element.prototype.closest,
            Document.prototype.querySelectorAll, window.ToggleEvent]
            .every(f => /native code/.test(String(f))),
        nativeShowPopover: /native code/.test(String(HTMLElement.prototype.showPopover)),
        tipStyle: tip ? [tip.getClientRects().length, tipStyle.display, tipStyle.textAlign,
            tipStyle.opacity, tipStyle.whiteSpace].join(' ') : null,
        times,
        tip: root ? {
            children: Array.from(root.children).map(c => c.localName),
            styles: root.querySelectorAll('style').length,
            hostStyle: Array.from(root.querySelectorAll('style')).some(s => s.textContent.includes('--tooltip-top')),
            slotted: slot ? slot.assignedNodes().map(n => n.textContent.trim()) : null,
        } : null,
    };
})()";

/// The number of callback lines that start with `prefix`.
fn count(callbacks: &[&str], prefix: &str) -> usize {
    callbacks.iter().filter(|l| l.starts_with(prefix)).count()
}

/// Chromium's replay of the page (ADR 0035 "What GitHub needs") has 35
/// `connectedCallback` and 76 `attributeChangedCallback` calls. 23 and 69 of
/// them belong to the 23 `relative-time` elements, whose constructor calls
/// `attachShadow` (ADR 0036 sub-feature 5a): `createElement` constructs them,
/// the page sets their attributes and inserts them, and the sink enqueues
/// these callbacks. The other elements get theirs from their upgrades: 12
/// `connectedCallback` and 7 `attributeChangedCallback` (`tool-tip` 1 and 4,
/// `ghcc-consent` 1 and 3, `react-partial` 4, `react-app` 1, `turbo-frame` 1,
/// `cookie-consent-link` 2, `clipboard-copy` 2). No disconnected or adopted
/// callback.
fn custom_element_callbacks(lines: &[String]) {
    let callbacks: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("custom element callback "))
        .collect();
    assert_eq!(
        count(&callbacks, "disconnectedCallback"),
        0,
        "{callbacks:?}"
    );
    assert_eq!(count(&callbacks, "adoptedCallback"), 0, "{callbacks:?}");
    let (connected, changed) = (
        count(&callbacks, "connectedCallback "),
        count(&callbacks, "attributeChangedCallback "),
    );
    assert_eq!(connected, 35, "{callbacks:?}");
    assert_eq!(changed, 76, "{callbacks:?}");
    // The origin of the callbacks: an upgrade enqueues the attributes and
    // the connected callback of an element that was in the document before
    // its definition; the sink enqueues what the DOM changes cause later.
    // The page defines its parsed elements after the parser made them, so
    // their 19 callbacks come from upgrades; the 23 `relative-time` elements
    // that scripts make get theirs from the sink.
    assert_eq!(count(&callbacks, "connectedCallback (upgrade)"), 12);
    assert_eq!(count(&callbacks, "attributeChangedCallback (upgrade)"), 7);
    assert_eq!(
        count(&callbacks, "connectedCallback (sink): relative-time"),
        23
    );
    assert_eq!(
        count(&callbacks, "attributeChangedCallback (sink): relative-time"),
        69
    );
}

/// The texts of the 23 `relative-time` elements in Chromium's replay at the
/// page clock, in document order.
const REPLAY_TIMES: [&str; 23] = [
    "4 days ago",
    "4 days ago",
    "2 weeks ago",
    "2 weeks ago",
    "4 days ago",
    "4 days ago",
    "2 weeks ago",
    "2 weeks ago",
    "4 days ago",
    "3 weeks ago",
    "3 weeks ago",
    "3 weeks ago",
    "last week",
    "4 days ago",
    "4 days ago",
    "3 weeks ago",
    "2 weeks ago",
    "5 days ago",
    "2 weeks ago",
    "3 weeks ago",
    "2 weeks ago",
    "3 weeks ago",
    "3 weeks ago",
];

/// The replay's shadow roots (ADR 0036 "What GitHub needs"): 24 hosts; the
/// `part="root"` span of each of the 23 `relative-time` roots has the text of
/// the replay at the page clock, in document order; `tool-tip`'s root has
/// its `style` with the `:host` rules and a default slot that gets the light
/// text "Dismiss alert".
fn shadow_roots(roots: &serde_json::Value) {
    assert_eq!(roots["hosts"], 24, "{roots}");
    // ADR 0037 sub-feature 6a: swb has `popover`, so GitHub's popover
    // polyfill does not run (it installs only without `popover` in
    // `HTMLElement.prototype`): no constructed sheet is adopted anywhere, as
    // in Chromium (until 6a the polyfill adopted one sheet in the document
    // and the 24 roots, ADR 0036 5c-2). `tool-tip`'s root has its own
    // `<style>` and its sheets apply to its host: `:host` sets
    // `text-align`, `opacity` and `white-space`; the closed popover has no
    // box (the UA sheet's `[popover]:not(:popover-open)`).
    assert_eq!(
        roots["sheets"],
        serde_json::json!({"": 23, "own": 1}),
        "{roots}"
    );
    assert_eq!(roots["rootSheets"], 1, "{roots}");
    assert_eq!(roots["adopted"], serde_json::json!([]), "{roots}");
    assert_eq!(roots["documentAdopted"], 0, "{roots}");
    assert_eq!(roots["adoptedSheets"], 0, "{roots}");
    assert_eq!(roots["popoverPolyfill"], false, "{roots}");
    assert_eq!(roots["nativeShowPopover"], true, "{roots}");
    assert_eq!(roots["tipStyle"], "0 none center 0 normal", "{roots}");
    let times: Vec<String> = serde_json::from_value(roots["times"].clone()).unwrap();
    assert_eq!(times, REPLAY_TIMES, "{roots}");
    let tip = &roots["tip"];
    assert_eq!(tip["hostStyle"], true, "{roots}");
    assert_eq!(
        tip["slotted"],
        serde_json::json!(["Dismiss alert"]),
        "{roots}"
    );
}
