//! Event handlers, focus, `click()`, activation behavior and the form
//! methods from scripts (M8 feature 6d1) in a page with JavaScript on.
//! The cases of the Chromium probe `tools/probes/web-input-handlers.json`
//! run here (they use the engine's form state, focus and requests); the
//! tests below check what a probe cannot see: the navigation that a click
//! on a link or a form submission starts, the page's element states, the
//! scroll after `focus()`.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Size};
use swb_net::NetworkFetcher;

mod common;
use common::{Site, finish_loading, new_page, node};

/// A probe case: its name, its page and its queries with Chromium's
/// values.
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

#[path = "script_input/cases.rs"]
mod cases;

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        "focus-basic",
        8,
        r#""a=input#a b=input#b c=button#c d=body e=div#e f=div#f g=a#g h=body hid=body dis=body ndi=body vh=body svgr=rect#svgr sa=a#sa st=body ta=textarea#ta sel=select#sel dt=body sm=summary#sm fsi=body ce=body""#,
        "an element with `contenteditable` is not focusable: swb edits only form controls (crates/engine/src/focus.rs)",
    ),
    (
        "handler-attrs",
        5,
        r#""function|function|function onclick(event) {\n\n}|function""#,
        "the handler of an empty attribute is an ordinary function whose source is the compiled text; Chromium's `toString` shows `{ [native code] }`",
    ),
    (
        "handler-attrs",
        8,
        r#""undefined,undefined,undefined,undefined""#,
        "swb has no `window.name`, which the handler sees as `name`",
    ),
];

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A page with JavaScript on that shows `html`.
fn open(site: &Site, html: &str) -> Page {
    let url = site.page("page.html", html);
    let mut page = new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(600.0, 400.0));
    page.set_javascript(true);
    page.navigate(url);
    finish_loading(&mut page, Duration::from_secs(30));
    page
}

/// Runs a script as a task and lays the page out.
fn run(page: &mut Page, source: &str) -> String {
    let value = match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{source}: {other:?}"),
    };
    page.update_layout();
    value
}

/// The probe cases.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let mut failures = Vec::new();
        for case in cases::CASES {
            let site = Site::new(&format!("script-input-{}", case.name));
            let mut page = open(&site, case.html);
            for (index, &(query, chromium)) in case.queries.iter().enumerate() {
                let want = SWB_VALUES
                    .iter()
                    .find(|v| v.0 == case.name && v.1 == index)
                    .map_or(chromium, |v| v.2);
                let got = match page.evaluate(query, JsonForm::Strict) {
                    Some(Evaluation::Value(text)) => text,
                    other => format!("{other:?}"),
                };
                let want_value: serde_json::Value = serde_json::from_str(want).unwrap();
                let got_value: serde_json::Value = serde_json::from_str(&got)
                    .unwrap_or_else(|_| serde_json::Value::String(got.clone()));
                if got_value != want_value {
                    failures.push(format!(
                        "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                        case.name
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

const BODY: &str = "<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>";

/// `a.click()` follows the link after the task: another document loads; a
/// click that a listener cancels does not; a fragment link scrolls.
#[test]
fn a_click_follows_a_link_after_the_task() {
    on_page_thread(|| {
        let site = Site::new("script-input-link");
        site.page("two.html", "<!DOCTYPE html><p id=two>two</p>");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<a id=a href=two.html>a</a><a id=c href=two.html>c</a><div style='height:2000px'></div><a id=f href='#end'>f</a><div id=end>end</div>"
            ),
        );
        let one = page.url().unwrap().to_string();
        run(
            &mut page,
            "document.getElementById('c').addEventListener('click', (e) => e.preventDefault()); document.getElementById('c').click()",
        );
        assert_eq!(page.url().unwrap().to_string(), one);
        run(&mut page, "document.getElementById('f').click()");
        assert!(page.url().unwrap().to_string().ends_with("#end"));
        assert!(
            page.scroll_position().y > 100.0,
            "{:?}",
            page.scroll_position()
        );
        // A dispatched click event follows the link, too.
        run(
            &mut page,
            "document.getElementById('a').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))",
        );
        finish_loading(&mut page, Duration::from_secs(30));
        assert!(page.url().unwrap().to_string().ends_with("two.html"));
        assert!(page.document().unwrap().element_by_id("two").is_some());
    });
}

/// A form submission from a script navigates after the task: `submit()`
/// without a `submit` event, `requestSubmit()` with it.
#[test]
fn a_submitted_form_navigates_after_the_task() {
    on_page_thread(|| {
        let site = Site::new("script-input-submit");
        site.page("target.html", "<!DOCTYPE html><p id=target>target</p>");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<form id=f action=target.html method=get><input name=q value=hello><button id=b name=go value=1>go</button></form><script>window.events = 0; document.getElementById('f').addEventListener('submit', () => events++);</script>"
            ),
        );
        let one = page.url().unwrap().to_string();
        // A canceled submit event stops the navigation.
        run(
            &mut page,
            "const f = document.getElementById('f'); f.addEventListener('submit', (e) => e.preventDefault(), { once: true }); f.requestSubmit()",
        );
        assert_eq!(page.url().unwrap().to_string(), one);
        assert_eq!(run(&mut page, "window.events"), "1");
        // `submit()` fires no event.
        run(&mut page, "document.getElementById('f').submit()");
        finish_loading(&mut page, Duration::from_secs(30));
        let url = page.url().unwrap().to_string();
        assert!(url.ends_with("target.html?q=hello"), "{url}");
        assert!(page.document().unwrap().element_by_id("target").is_some());
    });
}

/// `requestSubmit(button)` sends the button's name and value, and a click
/// on a submit button submits the form.
#[test]
fn a_submit_button_click_submits_with_the_button() {
    on_page_thread(|| {
        let site = Site::new("script-input-button");
        site.page("target.html", "<!DOCTYPE html><p id=target>target</p>");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<form id=f action=target.html><input name=q value=x><button id=b name=go value=1>go</button></form>"
            ),
        );
        run(&mut page, "document.getElementById('b').click()");
        finish_loading(&mut page, Duration::from_secs(30));
        let url = page.url().unwrap().to_string();
        assert!(url.ends_with("target.html?q=x&go=1"), "{url}");
    });
}

/// A required field that is empty stops the submission and gets the
/// focus.
#[test]
fn an_invalid_form_is_not_submitted() {
    on_page_thread(|| {
        let site = Site::new("script-input-invalid");
        site.page("target.html", "<!DOCTYPE html><p id=target>target</p>");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<form id=f action=target.html><input id=r name=q required><button id=b>go</button></form>"
            ),
        );
        let one = page.url().unwrap().to_string();
        run(&mut page, "document.getElementById('b').click()");
        assert_eq!(page.url().unwrap().to_string(), one);
        assert_eq!(page.focused_element(), Some(node(&page, "r")));
        // `submit()` does not validate.
        run(&mut page, "document.getElementById('f').submit()");
        finish_loading(&mut page, Duration::from_secs(30));
        assert!(page.url().unwrap().to_string().contains("target.html"));
    });
}

/// The focus of a script is the page's focus: the engine's accessor, the
/// `:focus` rules, and the scroll that `focus()` asks for.
#[test]
fn focus_from_a_script_reaches_the_page() {
    on_page_thread(|| {
        let site = Site::new("script-input-focus");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<style>input:focus {{ outline: 2px solid red }}</style><input id=a><div style='height:2000px'></div><input id=b>"
            ),
        );
        let a = node(&page, "a");
        let b = node(&page, "b");
        run(&mut page, "document.getElementById('a').focus()");
        assert_eq!(page.focused_element(), Some(a));
        assert!(page.scroll_position().y < 1.0);
        run(&mut page, "document.getElementById('b').focus()");
        assert_eq!(page.focused_element(), Some(b));
        assert!(
            page.scroll_position().y > 100.0,
            "{:?}",
            page.scroll_position()
        );
        run(
            &mut page,
            "document.getElementById('a').focus({ preventScroll: true })",
        );
        run(&mut page, "document.scrollingElement; 1");
        // `blur()` removes the focus.
        run(&mut page, "document.activeElement.blur()");
        assert_eq!(page.focused_element(), None);
    });
}

/// The user's focus is visible to scripts, and `activeElement` follows
/// the engine's focus fix-up when the focused element leaves the page.
#[test]
fn the_page_focus_is_the_active_element() {
    on_page_thread(|| {
        let site = Site::new("script-input-active");
        let mut page = open(&site, &format!("{BODY}<input id=a><input id=b>"));
        let a = node(&page, "a");
        assert!(page.focus(Some(a), true));
        assert_eq!(run(&mut page, "document.activeElement.id"), "\"a\"");
        run(&mut page, "document.getElementById('a').remove()");
        assert_eq!(
            run(&mut page, "document.activeElement.localName"),
            "\"body\""
        );
    });
}

/// The texts of the console of the page.
fn console_texts(page: &Page) -> Vec<String> {
    page.console_messages()
        .iter()
        .map(|m| m.text.clone())
        .collect()
}

/// `window.onerror` sees the errors of scripts, timers and handlers with
/// five arguments, and `return true` keeps the message out of the console.
#[test]
fn window_onerror_sees_script_errors() {
    on_page_thread(|| {
        let site = Site::new("script-input-onerror");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<script>window.got = []; window.onerror = function (message, source, line, column, error) {{ got.push([message, source.length > 0, line, column, error instanceof TypeError, arguments.length].join('|')); return got.length < 3; }};</script><script>null.x;</script><div id=d onclick=\"throw new RangeError('handler')\"></div>"
            ),
        );
        run(&mut page, "setTimeout(() => undefined.y, 0); 1");
        page.wait_until_idle(Duration::from_millis(30), Duration::from_secs(5));
        run(&mut page, "document.getElementById('d').click(); 1");
        let got = run(&mut page, "got.join('  ')");
        assert!(
            got.contains(
                "Uncaught TypeError: Cannot read properties of null (reading 'x')|true|1|"
            ) && got.contains("|true|5"),
            "{got}"
        );
        assert!(got.contains("Uncaught RangeError: handler"), "{got}");
        // The first two errors were handled (`return true`), the third
        // was not.
        let console = console_texts(&page);
        assert!(
            !console.iter().any(|t| t.contains("reading 'x'")),
            "{console:?}"
        );
        assert!(
            console.iter().any(|t| t.contains("RangeError: handler")),
            "{console:?}"
        );
    });
}

/// `onunhandledrejection` sees a rejected promise, and `preventDefault`
/// keeps the console line away.
#[test]
fn onunhandledrejection_sees_rejections() {
    on_page_thread(|| {
        let site = Site::new("script-input-rejection");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<script>window.seen = []; window.onunhandledrejection = (e) => {{ seen.push(e.type + ':' + e.reason); e.preventDefault(); }}; Promise.reject('r1');</script>"
            ),
        );
        run(&mut page, "Promise.reject('r2'); 1");
        page.wait_until_idle(Duration::from_millis(30), Duration::from_secs(5));
        assert_eq!(
            run(&mut page, "seen.join()"),
            "\"unhandledrejection:r1,unhandledrejection:r2\""
        );
        let console = console_texts(&page);
        assert!(
            !console.iter().any(|t| t.contains("in promise")),
            "{console:?}"
        );
    });
}

/// The `load` event reaches `window.onload`, whether `body` or a script
/// sets it.
#[test]
fn body_onload_is_the_window_handler() {
    on_page_thread(|| {
        let site = Site::new("script-input-onload");
        let mut page = open(
            &site,
            &format!(
                "{BODY}<script>window.order = []; window.addEventListener('load', () => order.push('listener'));</script><script>document.body.setAttribute('onload', \"order.push('attr')\");</script>"
            ),
        );
        // The attribute came after the listener.
        assert_eq!(run(&mut page, "order.join()"), "\"listener,attr\"");
        assert_eq!(
            run(
                &mut page,
                "typeof window.onload + ',' + (document.body.onload === window.onload)"
            ),
            "\"function,true\""
        );
    });
}
