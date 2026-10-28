//! The engine's own input as DOM events (M8 feature 6d2): the cases of the
//! Chromium probe `tools/probes/web-input-real.json` run here with the
//! same input steps (`swb_automation::run_steps`, the `input.*` methods),
//! and the values of their queries are the ones that Chromium 148 gave
//! (`tests/data/web-input-real.json`: `just probe --json
//! tools/probes/web-input-real.json`, three equal runs; the cases of the
//! wheel and keys query `scrollX` and `scrollY` since feature 7b, from one
//! run). The tests below check what a probe cannot see: the selection, the
//! navigations, the element states.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{Evaluation, FontContext, JsonForm, LoadState, Page, PageConfig, Size, Url};
use swb_net::NetworkFetcher;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A directory with test pages, removed at the end of the test.
struct Site(PathBuf);

impl Site {
    fn new(name: &str) -> Site {
        let dir =
            std::env::temp_dir().join(format!("swb-real-input-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Site(dir)
    }

    fn page(&self, name: &str, html: &str) -> Url {
        let path = self.0.join(name);
        std::fs::write(&path, html).unwrap();
        Url::from_file_path(path).unwrap()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A page with JavaScript on at `url` (800x600, test fonts), loaded.
fn open(url: Url) -> Page {
    let config = PageConfig {
        fetcher: Arc::new(NetworkFetcher::new()),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(800.0, 600.0),
        1.0,
    );
    page.set_javascript(true);
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// A page that shows `html`.
fn page_of(site: &Site, html: &str) -> Page {
    open(site.page("page.html", html))
}

/// The JSON value of an expression.
fn value(page: &mut Page, source: &str) -> Value {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{source}: {other:?}"),
    }
}

fn steps(text: &str) -> Vec<Value> {
    serde_json::from_str(text).unwrap()
}

/// Runs the cases of the probe file against Chromium's values.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let probe: Value = serde_json::from_str(
            &std::fs::read_to_string(repo_root().join("tools/probes/web-input-real.json")).unwrap(),
        )
        .unwrap();
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-input-real.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let cases = probe["cases"].as_array().unwrap();
        assert_eq!(cases.len(), expected.as_object().unwrap().len());
        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let site = Site::new(name);
            let mut page = page_of(&site, case["html"].as_str().unwrap());
            if let Err(error) = run_steps(&mut page, case["steps"].as_array().unwrap()) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            let queries = case["measure"].as_array().unwrap();
            for (index, query) in queries.iter().enumerate() {
                let source = query["js"].as_str().unwrap();
                let want = &expected[name][index];
                let got = value(&mut page, source);
                if &got != want {
                    failures.push(format!(
                        "{name} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

const BODY: &str = "<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>";

/// A wheel turn scrolls the page; a canceled `wheel` does not; `scroll`
/// fires once per frame, however many wheel turns came before it.
#[test]
fn wheel_scrolls_unless_canceled_and_scroll_fires_once_per_frame() {
    on_page_thread(|| {
        let site = Site::new("wheel");
        let html = format!(
            "{BODY}<div id=a style='height:3000px'></div><div id=b style='position:absolute;left:0;top:0;width:100px;height:100px'></div><script>\
             var log = [];\
             document.addEventListener('scroll', function (e) {{ log.push('scroll ' + e.target.nodeName + ' ' + e.bubbles); }});\
             document.addEventListener('wheel', function (e) {{ log.push('wheel ' + e.deltaY + ' ' + e.cancelable); }}, true);\
             document.getElementById('b').addEventListener('wheel', function (e) {{ e.preventDefault(); log.push('canceled'); }});\
             </script>"
        );
        let mut page = page_of(&site, &html);
        // Over `a`: three turns within one frame make one `scroll`.
        page.wheel(300.0, 300.0, 0.0, 100.0);
        page.wheel(300.0, 300.0, 0.0, 53.0);
        page.wheel(300.0, 300.0, 0.0, 61.0);
        assert_eq!(page.scroll_position().y, 214.0);
        assert_eq!(
            value(&mut page, "log.join('|')"),
            "wheel 100 false|wheel 53 false|wheel 61 false"
        );
        page.run_event_loop(Duration::from_millis(100), Duration::from_secs(5));
        assert_eq!(
            value(&mut page, "log.slice(3).join('|')"),
            "scroll #document true"
        );
        // Over `b`, a listener cancels the wheel: no scroll, no event.
        page.scroll_to(swb_engine::Point::new(0.0, 0.0));
        page.run_event_loop(Duration::from_millis(100), Duration::from_secs(5));
        value(&mut page, "log.length = 0");
        page.wheel(50.0, 50.0, 0.0, 100.0);
        page.run_event_loop(Duration::from_millis(100), Duration::from_secs(5));
        assert_eq!(page.scroll_position().y, 0.0);
        assert_eq!(value(&mut page, "log.join('|')"), "wheel 100 true|canceled");
    });
}

/// A canceled `keydown` gives no default action: no text, no scroll, no
/// Tab focus move; the keys still reach `keyup`.
#[test]
fn canceled_keydown_has_no_default_action() {
    on_page_thread(|| {
        let site = Site::new("keydown");
        let html = format!(
            "{BODY}<input id=a style='position:absolute;left:0;top:0;width:80px'><input id=b style='position:absolute;left:100px;top:0;width:80px'><div style='height:3000px'></div><script>\
             var cancel = false;\
             document.addEventListener('keydown', function (e) {{ if (cancel) e.preventDefault(); }});\
             </script>"
        );
        let mut page = page_of(&site, &html);
        run_steps(&mut page, &steps(r##"[{"focus":"#a"},{"type":"x"}]"##)).unwrap();
        assert_eq!(value(&mut page, "document.getElementById('a').value"), "x");
        value(&mut page, "cancel = true");
        run_steps(
            &mut page,
            &steps(r#"[{"type":"y"},{"key":"Tab"},{"key":"PageDown"},{"key":"Space"}]"#),
        )
        .unwrap();
        assert_eq!(value(&mut page, "document.getElementById('a').value"), "x");
        assert_eq!(value(&mut page, "document.activeElement.id"), "a");
        assert_eq!(page.scroll_position().y, 0.0);
        value(&mut page, "cancel = false");
        run_steps(&mut page, &steps(r#"[{"key":"Tab"}]"#)).unwrap();
        assert_eq!(value(&mut page, "document.activeElement.id"), "b");
    });
}

/// A canceled `click` on a link does not navigate; an uncanceled one does.
/// A canceled `mousedown` does not focus and does not start a selection.
#[test]
fn canceled_click_and_mousedown() {
    on_page_thread(|| {
        let site = Site::new("click");
        let other = site.page("other.html", "<!DOCTYPE html><title>other</title><p>other");
        let html = format!(
            "{BODY}<a id=x href='{other}' style='position:absolute;left:0;top:0'>x</a>\
             <a id=y href='{other}' style='position:absolute;left:100px;top:0'>y</a>\
             <input id=t style='position:absolute;left:0;top:100px;width:100px'>\
             <p id=p style='position:absolute;left:0;top:200px;margin:0'>word other</p><script>\
             document.getElementById('x').addEventListener('click', function (e) {{ e.preventDefault(); }});\
             document.getElementById('t').addEventListener('mousedown', function (e) {{ e.preventDefault(); }});\
             document.getElementById('p').addEventListener('mousedown', function (e) {{ e.preventDefault(); }});\
             </script>"
        );
        let mut page = page_of(&site, &html);
        run_steps(&mut page, &steps(r##"[{"click":"#x"}]"##)).unwrap();
        assert_eq!(page.load_state(), LoadState::Complete);
        assert!(page.url().unwrap().as_str().ends_with("page.html"));
        // The link took the focus on mousedown, as in Chromium.
        assert_eq!(value(&mut page, "document.activeElement.id"), "x");
        run_steps(&mut page, &steps(r##"[{"click":"#t"},{"dblclick":"#p"}]"##)).unwrap();
        assert_eq!(value(&mut page, "document.activeElement.id"), "x");
        assert_eq!(page.selected_text(), "");
        run_steps(&mut page, &steps(r##"[{"click":"#y"}]"##)).unwrap();
        assert!(page.wait_until_loaded(Duration::from_secs(30)));
        assert!(page.url().unwrap().as_str().ends_with("other.html"));
    });
}

/// The activation behavior runs once: a click on a checkbox, a label, a
/// submit button and a summary changes the control once, and a listener
/// that cancels `click` stops the action.
#[test]
fn click_actions_run_once() {
    on_page_thread(|| {
        let site = Site::new("once");
        let html = format!(
            "{BODY}<form id=f onsubmit='window.submits = (window.submits || 0) + 1; return false'>\
             <input type=checkbox id=c style='position:absolute;left:0;top:0'>\
             <label id=l for=c style='position:absolute;left:100px;top:0'>label</label>\
             <button id=s style='position:absolute;left:200px;top:0'>go</button></form>\
             <details id=d style='position:absolute;left:0;top:100px'><summary id=m>m</summary>x</details>"
        );
        let mut page = page_of(&site, &html);
        run_steps(&mut page, &steps(r##"[{"click":"#c"}]"##)).unwrap();
        assert_eq!(
            value(&mut page, "document.getElementById('c').checked"),
            true
        );
        run_steps(&mut page, &steps(r##"[{"click":"#l"}]"##)).unwrap();
        assert_eq!(
            value(&mut page, "document.getElementById('c').checked"),
            false
        );
        run_steps(&mut page, &steps(r##"[{"click":"#s"}]"##)).unwrap();
        assert_eq!(value(&mut page, "window.submits"), 1);
        run_steps(&mut page, &steps(r##"[{"click":"#m"}]"##)).unwrap();
        assert_eq!(value(&mut page, "document.getElementById('d').open"), true);
        run_steps(&mut page, &steps(r##"[{"click":"#m"}]"##)).unwrap();
        assert_eq!(value(&mut page, "document.getElementById('d').open"), false);
        // Canceled: the checkbox, the summary stay.
        value(
            &mut page,
            "document.addEventListener('click', function (e) { e.preventDefault(); })",
        );
        run_steps(&mut page, &steps(r##"[{"click":"#c"},{"click":"#m"}]"##)).unwrap();
        assert_eq!(
            value(&mut page, "document.getElementById('c').checked"),
            false
        );
        assert_eq!(value(&mut page, "document.getElementById('d').open"), false);
    });
}

/// `javascript:` links and form actions run their script in the document;
/// a loop is stopped by the time limit of the task.
#[test]
fn javascript_urls_run_in_the_document() {
    on_page_thread(|| {
        let site = Site::new("jsurl");
        let html = format!(
            "{BODY}<a id=x href=\"javascript:window.ran = (window.ran || 0) + 1\" style='position:absolute;left:0;top:0'>x</a>\
             <form action=\"javascript:window.ran = (window.ran || 0) + 10\" style='position:absolute;left:100px;top:0'><button id=s>go</button></form>\
             <a id=bad href=\"javascript:throw new Error('boom')\" style='position:absolute;left:200px;top:0'>bad</a>"
        );
        let mut page = page_of(&site, &html);
        run_steps(&mut page, &steps(r##"[{"click":"#x"},{"wait":20}]"##)).unwrap();
        assert_eq!(value(&mut page, "window.ran"), 1);
        run_steps(&mut page, &steps(r##"[{"click":"#s"},{"wait":20}]"##)).unwrap();
        assert_eq!(value(&mut page, "window.ran"), 11);
        // The document stays; the error is reported to the console.
        run_steps(&mut page, &steps(r##"[{"click":"#bad"},{"wait":20}]"##)).unwrap();
        assert!(page.url().unwrap().as_str().ends_with("page.html"));
        assert!(
            page.console_messages()
                .iter()
                .any(|m| m.text.contains("boom"))
        );
    });
}

/// The script of a `javascript:` URL is a task of its own: it runs after
/// the click that started it, not inside it. A link for a new browsing
/// context (`target=_blank`) and a click with Ctrl or Shift do not run it
/// in this document (measured in Chromium, `javascript-link-contexts`).
#[test]
fn javascript_urls_are_tasks_and_skip_other_contexts() {
    on_page_thread(|| {
        let site = Site::new("jsurl-task");
        let html = format!(
            "{BODY}<script>var log = []; function L(s) {{ log.push(s); }}</script>\
             <a id=x href=\"javascript:L('ran')\" style='position:absolute;left:0;top:0'>x</a>\
             <a id=b href=\"javascript:L('blank')\" target=_blank style='position:absolute;left:100px;top:0'>b</a>\
             <a id=s href=\"javascript:L('self')\" target=_self style='position:absolute;left:200px;top:0'>s</a>\
             <button id=k style='position:absolute;left:300px;top:0'>k</button>\
             <script>document.getElementById('k').addEventListener('click', function () {{\
               document.getElementById('x').click(); L('after click()'); }});</script>"
        );
        let mut page = page_of(&site, &html);
        run_steps(&mut page, &steps(r##"[{"click":"#k"},{"wait":20}]"##)).unwrap();
        assert_eq!(value(&mut page, "log.join('|')"), "after click()|ran");
        value(&mut page, "log.length = 0");
        run_steps(
            &mut page,
            &steps(
                r##"[{"click":"#b"},{"click":"#x","modifiers":["Control"]},{"click":"#x","modifiers":["Shift"]},{"wait":20}]"##,
            ),
        )
        .unwrap();
        assert_eq!(value(&mut page, "log.join('|')"), "");
        run_steps(&mut page, &steps(r##"[{"click":"#s"},{"wait":20}]"##)).unwrap();
        assert_eq!(value(&mut page, "log.join('|')"), "self");
    });
}

/// After the scripts of the document were stopped (the time limit), clicks
/// and focus changes work as with JavaScript off: a link navigates, a
/// checkbox toggles, a field takes the focus.
#[test]
fn input_works_after_the_scripts_stopped() {
    on_page_thread(|| {
        let site = Site::new("stopped");
        let other = site.page(
            "other.html",
            "<!DOCTYPE html><title>other</title><p id=o>other",
        );
        let html = format!(
            "{BODY}<a id=a href='{other}' style='position:absolute;left:0;top:0'>go</a>\
             <input type=checkbox id=c style='position:absolute;left:0;top:100px'>\
             <input id=t style='position:absolute;left:100px;top:100px;width:100px'>"
        );
        let mut page = page_of(&site, &html);
        let runtime = page.script_runtime_mut().unwrap();
        runtime.set_time_limit(Duration::from_millis(30));
        for _ in 0..3 {
            page.evaluate("while (true);", JsonForm::Strict);
        }
        assert!(page.script_runtime_mut().unwrap().stopped());
        let checkbox = page.document().unwrap().element_by_id("c").unwrap();
        let field = page.document().unwrap().element_by_id("t").unwrap();
        run_steps(&mut page, &steps(r##"[{"click":"#c"},{"click":"#t"}]"##)).unwrap();
        assert_eq!(page.control_checked(checkbox), Some(true));
        assert_eq!(page.focused_element(), Some(field));
        run_steps(&mut page, &steps(r##"[{"click":"#a"}]"##)).unwrap();
        assert!(page.wait_until_loaded(Duration::from_secs(30)));
        assert!(page.url().unwrap().as_str().ends_with("other.html"));
    });
}

/// A click on an image button sends the point of the click, as with
/// JavaScript off; `click()` sends 0,0.
#[test]
fn image_button_click_sends_the_point() {
    on_page_thread(|| {
        let site = Site::new("image");
        site.page("target.html", "<!DOCTYPE html><p>t");
        let html = format!(
            "{BODY}<form action=target.html><input type=image id=i name=im width=100 height=50 alt=xxxxxxxxxxxx></form>"
        );
        let mut page = page_of(&site, &html);
        run_steps(&mut page, &steps(r#"[{"click":[50,25]}]"#)).unwrap();
        assert!(page.wait_until_loaded(Duration::from_secs(30)));
        let url = page.url().unwrap().to_string();
        assert!(url.ends_with("target.html?im.x=50&im.y=25"), "{url}");
        let mut page = page_of(&site, &html);
        value(&mut page, "document.getElementById('i').click()");
        assert!(page.wait_until_loaded(Duration::from_secs(30)));
        let url = page.url().unwrap().to_string();
        assert!(url.ends_with("target.html?im.x=0&im.y=0"), "{url}");
    });
}

/// `type` sends the key events to the body when no field has the focus,
/// and goes on when the target stops being editable.
#[test]
fn type_without_an_editable_focus_sends_key_events() {
    on_page_thread(|| {
        let site = Site::new("type-body");
        let html = format!(
            "{BODY}<input id=t style='position:absolute;left:0;top:0;width:100px'><script>\
             var log = [];\
             document.addEventListener('keydown', function (e) {{ log.push(e.type + '@' + e.target.nodeName); }});\
             document.addEventListener('keyup', function (e) {{ log.push(e.type + '@' + e.target.nodeName); }});\
             document.addEventListener('keydown', function (e) {{ document.getElementById('t').remove(); }}, {{ once: true }});\
             </script>"
        );
        let mut page = page_of(&site, &html);
        run_steps(&mut page, &steps(r##"[{"focus":"#t"},{"type":"ab"}]"##)).unwrap();
        assert_eq!(
            value(&mut page, "log.join('|')"),
            "keydown@INPUT|keyup@BODY|keydown@BODY|keyup@BODY"
        );
    });
}

/// `scroll` events of several scrolled elements come in tree order, not
/// in the order of their node IDs.
#[test]
fn scroll_events_of_elements_are_in_tree_order() {
    on_page_thread(|| {
        let site = Site::new("scroll-order");
        let html = format!(
            "{BODY}<div id=a style='width:100px;height:100px;overflow:auto'><div style='height:500px'></div></div>\
             <div id=b style='width:100px;height:100px;overflow:auto'><div style='height:500px'></div></div>\
             <script>var log = [];\
             var a = document.getElementById('a'), b = document.getElementById('b');\
             var c = document.createElement('div'); c.id = 'c';\
             c.setAttribute('style', 'width:100px;height:100px;overflow:auto');\
             c.innerHTML = '<div style=\"height:500px\"></div>';\
             document.body.insertBefore(c, a);\
             ['a', 'b', 'c'].forEach(function (id) {{\
               document.getElementById(id).addEventListener('scroll', function () {{ log.push(id); }});\
             }});</script>"
        );
        let mut page = page_of(&site, &html);
        // `b` and `a` scroll before `c`, which is first in the tree but
        // was created last.
        run_steps(
            &mut page,
            &steps(r##"[{"hover":"#b"},{"wheel":[0,50]},{"hover":"#a"},{"wheel":[0,50]},{"hover":"#c"},{"wheel":[0,50]},{"wait":100}]"##),
        )
        .unwrap();
        assert_eq!(value(&mut page, "log.join('|')"), "b|a|c");
    });
}
