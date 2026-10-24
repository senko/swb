//! The life of the window's document (feature 5c, ADR 0028 section 8) in
//! the runtime with a test document: the unload (`pagehide`, `unload`),
//! a parse that goes on after the document's scripts stopped, and the
//! readiness and current script outside the parse. The order of scripts
//! with the network is in `crates/engine/tests/script_processing.rs`.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::time::Duration;

use common::{TestDocument, answer, load, on_big_stack};
use swb_web::{ClockMode, PageClock, ScriptRuntime};

#[test]
fn the_unload_fires_pagehide_then_unload() {
    on_big_stack(|| {
        let html = "<!DOCTYPE html><script>var log = [];\
                    addEventListener('pageshow', (e) => log.push(e.type));\
                    addEventListener('pagehide', (e) => log.push([e.type, e.persisted, e.bubbles, e.cancelable, e.target === document, e.constructor.name].join()));\
                    addEventListener('unload', (e) => log.push([e.type, e.cancelable, e.target === document, document.readyState].join()));\
                    </script>";
        let mut rt = load(html, false);
        rt.unload_document();
        // A second unload does nothing.
        rt.unload_document();
        assert_eq!(
            answer(&mut rt, "log"),
            serde_json::json!([
                "pageshow",
                "pagehide,false,true,true,true,PageTransitionEvent",
                "unload,false,true,complete"
            ])
        );
        // Scripts that a script inserts after the unload do not run.
        assert_eq!(
            answer(
                &mut rt,
                "var s = document.createElement('script'); s.text = 'window.ran = 1'; document.head.appendChild(s); window.ran === undefined"
            ),
            serde_json::json!(true)
        );
    });
}

#[test]
fn the_parse_goes_on_after_the_scripts_stopped() {
    on_big_stack(|| {
        let mut rt = ScriptRuntime::new(PageClock::new(ClockMode::Real, None)).unwrap();
        rt.set_time_limit(Duration::from_millis(30));
        let html = "<!DOCTYPE html><body><script>for (;;) {}</script><script>for (;;) {}</script>\
                    <script>for (;;) {}</script><script>window.ran = 1</script><p>after</p>";
        rt.lend(Box::new(TestDocument::parsing(html)));
        rt.start_document();
        assert!(rt.stopped());
        // The document went to its end, without the fourth script.
        assert!(rt.load_event_due());
        rt.fire_load_event();
        assert!(rt.document_loaded());
        let host = rt.take_back().unwrap();
        let doc = host.into_any().downcast::<TestDocument>().unwrap();
        let body = swb_dom::outer_html(&doc.dom, swb_dom::NodeId::DOCUMENT);
        assert!(body.contains("<p>after</p>"), "{body}");
    });
}

#[test]
fn readiness_and_current_script_without_a_parse() {
    on_big_stack(|| {
        // A runtime that did not start its document (a test that lends a
        // parsed one): the readiness is HTML's initial "complete".
        let mut rt = ScriptRuntime::new(PageClock::new(ClockMode::Real, None)).unwrap();
        let dom = swb_dom::parse_html_with("<!DOCTYPE html><p>x", true);
        rt.lend(Box::new(TestDocument::parsed(dom)));
        assert!(rt.document_loaded());
        assert!(!rt.load_event_due());
        assert_eq!(
            answer(&mut rt, "[document.readyState, document.currentScript]"),
            serde_json::json!(["complete", null])
        );
        // With the parse: "loading" in an inline script, "complete" after
        // the load event.
        let mut rt = load(
            "<!DOCTYPE html><script>window.r = [document.readyState, document.currentScript.text.length]</script>",
            false,
        );
        assert_eq!(answer(&mut rt, "r"), serde_json::json!(["loading", 68]));
        assert_eq!(
            answer(&mut rt, "[document.readyState, document.currentScript]"),
            serde_json::json!(["complete", null])
        );
    });
}
