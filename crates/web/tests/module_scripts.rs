//! Module graphs in the runtime with a test document (ADR 0034 sub-feature
//! 3b-1): the module map's fetches and their answers, the readiness of a
//! script element when its graph is complete, the fetch sharing of URLs that
//! differ only in the fragment, the reports of graph errors, the fetch
//! options of descendants and of `import()`, each with and without the GC
//! stress mode. The orders against Chromium's are in
//! `crates/engine/tests/probe_net.rs` (`module_graph_probe_cases_match_chromium`).

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::time::Duration;

use common::{TestDocument, answer, load_with, on_big_stack};
use swb_web::{ClockMode, FetchedScript, ModuleFetch, ScriptRuntime};

const HEAD: &str = "<!DOCTYPE html><script>var log = []; \
    onerror = (m, s, l, c) => { log.push('err:' + m + '@' + s + ':' + l + ':' + c); }; \
    document.addEventListener('DOMContentLoaded', () => log.push('dcl')); \
    document.addEventListener('error', (e) => log.push('error-event'), true); \
    </script>";

/// A runtime with `body` after [`HEAD`] whose module fetches the test
/// answers; the list of the fetches.
fn page(body: &str, stress: bool) -> (ScriptRuntime, Fetches) {
    let (rt, fetches, _) = page_with_delays(body, stress);
    (rt, fetches)
}

/// [`page`], and the ids of the fetches that delay `load` from the moment
/// a script element joins them.
fn page_with_delays(body: &str, stress: bool) -> (ScriptRuntime, Fetches, Delayed) {
    let doc = TestDocument::accepting_fetches(&format!("{HEAD}{body}"));
    let fetches = std::rc::Rc::clone(&doc.module_fetches);
    let delayed = std::rc::Rc::clone(&doc.delayed_fetches);
    let rt = load_with(doc, stress, ClockMode::Virtual);
    (rt, fetches, delayed)
}

type Delayed = std::rc::Rc<std::cell::RefCell<Vec<u64>>>;

type Fetches = std::rc::Rc<std::cell::RefCell<Vec<(u64, String, ModuleFetch)>>>;

/// Answers the fetch of `url` with `source` (`None`: it fails), then runs
/// the tasks that are due.
fn answer_fetch(rt: &mut ScriptRuntime, fetches: &Fetches, url: &str, source: Option<&str>) {
    let id = fetches
        .borrow()
        .iter()
        .find(|(_, u, _)| u == url)
        .map_or_else(
            || panic!("no fetch of {url}: {:?}", fetches.borrow()),
            |f| f.0,
        );
    let script = source.map(|text| FetchedScript {
        source: text.to_owned(),
        url: url.to_owned(),
    });
    rt.module_fetched(id, script);
    settle(rt);
}

/// Runs the tasks that are due and fires `load` when it is due.
fn settle(rt: &mut ScriptRuntime) {
    for _ in 0..4 {
        rt.run_due_tasks(Duration::from_secs(5));
        rt.fire_load_event();
    }
}

fn urls(fetches: &Fetches) -> Vec<String> {
    fetches.borrow().iter().map(|f| f.1.clone()).collect()
}

#[test]
fn an_inline_module_runs_when_its_graph_is_complete() {
    on_big_stack(|| {
        for stress in [false, true] {
            let (mut rt, fetches) = page(
                "<script type=module>import {v} from './a.js'; import './a.js#f'; \
                 log.push('m' + v)</script>",
                stress,
            );
            // One request for the two URLs of `a.js` (fetch sharing); the
            // deferred module waits, and so does `DOMContentLoaded`.
            assert_eq!(urls(&fetches), ["file:///probe/a.js"]);
            assert_eq!(answer(&mut rt, "log"), serde_json::json!([]));
            assert!(!rt.document_loaded());
            answer_fetch(
                &mut rt,
                &fetches,
                "file:///probe/a.js",
                Some("export var v = 1; log.push('a ' + import.meta.url)"),
            );
            // Two modules: the URL with the fragment is another module.
            assert_eq!(
                answer(&mut rt, "log"),
                serde_json::json!([
                    "a file:///probe/a.js",
                    "a file:///probe/a.js#f",
                    "m1",
                    "dcl"
                ])
            );
            assert!(rt.document_loaded());
        }
    });
}

#[test]
fn descendants_inherit_the_fetch_options_and_import_does_not_delay_load() {
    on_big_stack(|| {
        let (mut rt, fetches) = page(
            "<script type=module src=r.js crossorigin=use-credentials></script>",
            false,
        );
        answer_fetch(
            &mut rt,
            &fetches,
            "file:///probe/r.js",
            Some("import './d.js'"),
        );
        answer_fetch(
            &mut rt,
            &fetches,
            "file:///probe/d.js",
            Some("import('./i.js')"),
        );
        let options: Vec<(String, ModuleFetch)> = fetches
            .borrow()
            .iter()
            .map(|f| (f.1.clone(), f.2))
            .collect();
        let with = |credentials, delays_load| ModuleFetch {
            credentials,
            delays_load,
        };
        assert_eq!(
            options,
            [
                ("file:///probe/r.js".to_owned(), with(true, true)),
                ("file:///probe/d.js".to_owned(), with(true, true)),
                // `import()` of a module: its options, but no delay.
                ("file:///probe/i.js".to_owned(), with(true, false)),
            ]
        );
        assert!(rt.document_loaded());
    });
}

#[test]
fn graph_errors_reach_onerror_and_failed_fetches_fire_error() {
    on_big_stack(|| {
        for stress in [false, true] {
            let (mut rt, fetches) = page(
                "<script type=module>import 'bare'</script>\
                 <script type=module>import './missing.js'</script>\
                 <script type=module>import './bad.js'</script>",
                stress,
            );
            answer_fetch(&mut rt, &fetches, "file:///probe/missing.js", None);
            answer_fetch(
                &mut rt,
                &fetches,
                "file:///probe/bad.js",
                Some("log.push('bad');\nvar v = ;"),
            );
            assert_eq!(
                answer(&mut rt, "log"),
                serde_json::json!([
                    "err:Uncaught TypeError: Failed to resolve module specifier \"bare\". \
                     Relative references must start with either \"/\", \"./\", or \"../\".\
                     @file:///probe/page.html:0:0",
                    "error-event",
                    "err:Uncaught SyntaxError: Unexpected token ';'@file:///probe/bad.js:2:9",
                    "dcl"
                ])
            );
        }
    });
}

#[test]
fn a_module_in_the_map_is_not_fetched_again() {
    on_big_stack(|| {
        for stress in [false, true] {
            let (mut rt, fetches) = page(
                "<script type=module>import * as a from './a.js'; window.first = a;</script>",
                stress,
            );
            answer_fetch(
                &mut rt,
                &fetches,
                "file:///probe/a.js",
                Some("export var v = 2"),
            );
            rt.collect();
            assert_eq!(
                answer(
                    &mut rt,
                    "var same; import('./a.js').then((b) => { same = b === first && b.v; }); 1"
                ),
                serde_json::json!(1)
            );
            settle(&mut rt);
            rt.collect();
            assert_eq!(answer(&mut rt, "same"), serde_json::json!(2));
            assert_eq!(urls(&fetches), ["file:///probe/a.js"]);
        }
    });
}

#[test]
fn a_rejection_of_top_level_await_is_reported_when_it_happens() {
    on_big_stack(|| {
        let (mut rt, fetches) = page(
            "<script type=module>import './t.js'; log.push('m')</script>",
            false,
        );
        answer_fetch(
            &mut rt,
            &fetches,
            "file:///probe/t.js",
            Some(
                "log.push('t'); await new Promise((r) => setTimeout(r, 0));\nthrow new RangeError('late')",
            ),
        );
        // `DOMContentLoaded` does not wait for the `await`; the timer runs,
        // the body goes on and rejects: the report comes then, at the place
        // of the error; `m` never runs.
        assert_eq!(
            answer(&mut rt, "log"),
            serde_json::json!([
                "t",
                "dcl",
                "err:Uncaught RangeError: late@file:///probe/t.js:2:7"
            ])
        );
    });
}

#[test]
fn an_element_that_joins_the_fetch_of_import_delays_load() {
    on_big_stack(|| {
        // The `import()` starts the fetch without a delay of `load`; the
        // async element after it joins the fetch (Chromium, measured: case
        // `import-call-timing-quiet-then-async`: `load` waits for it).
        let (mut rt, fetches, delayed) = page_with_delays(
            "<script>import('./s.js').then(() => log.push('imp'))</script>\
             <script type=module async src=s.js></script>",
            false,
        );
        let id = fetches.borrow()[0].0;
        assert_eq!(urls(&fetches), ["file:///probe/s.js"]);
        assert!(!fetches.borrow()[0].2.delays_load);
        assert_eq!(*delayed.borrow(), [id]);
        // Another `import()` of the URL does not change that.
        answer(&mut rt, "import('./s.js').catch(() => 1); 1");
        assert_eq!(*delayed.borrow(), [id]);
    });
}

#[test]
fn an_import_after_the_element_keeps_the_fetch_as_it_is() {
    on_big_stack(|| {
        let (mut rt, fetches, delayed) = page_with_delays(
            "<script type=module async src=s.js></script>\
             <script>import('./s.js')</script>",
            false,
        );
        assert!(fetches.borrow()[0].2.delays_load);
        answer(&mut rt, "import('./s.js').catch(() => 1); 1");
        assert!(delayed.borrow().is_empty());
    });
}

/// The roots of the loads of the page (`graphs`) and the errors of the map
/// live across collections that run while the answers of one arrival are
/// handled: two inline modules wait for one module; the answer of the first
/// graph queues its job, and the second one allocates before the job runs.
#[test]
fn graph_roots_survive_collections_between_the_arrival_and_the_job() {
    on_big_stack(|| {
        for stress in [false, true] {
            let (mut rt, fetches) = page(
                "<script type=module>import {v} from './a.js'; log.push('m1 ' + v)</script>\
                 <script type=module>import {v} from './a.js'; log.push('m2 ' + v)</script>",
                stress,
            );
            rt.collect();
            answer_fetch(
                &mut rt,
                &fetches,
                "file:///probe/a.js",
                Some("export var v = 3"),
            );
            rt.collect();
            assert_eq!(
                answer(&mut rt, "log"),
                serde_json::json!(["m1 3", "m2 3", "dcl"])
            );
        }
    });
}

#[test]
fn parse_errors_of_descendants_survive_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let (mut rt, fetches) = page(
                "<script type=module>import './bad.js'</script>\
                 <script type=module>import './bad.js'</script>",
                stress,
            );
            rt.collect();
            answer_fetch(&mut rt, &fetches, "file:///probe/bad.js", Some("var v = ;"));
            rt.collect();
            assert_eq!(
                answer(&mut rt, "log"),
                serde_json::json!([
                    "err:Uncaught SyntaxError: Unexpected token ';'@file:///probe/bad.js:1:9",
                    "err:Uncaught SyntaxError: Unexpected token ';'@file:///probe/bad.js:1:9",
                    "dcl"
                ])
            );
        }
    });
}
