//! `document.write`, `writeln`, `open` and `close` during the parse (M9
//! feature 2, ADR 0028 section 8) in the runtime with a test document: the
//! order of nested and external written scripts, the insertion point, the
//! bounds and the termination of a written script. The Chromium probe
//! `tools/probes/web-document-write.json` runs in
//! `crates/engine/tests/script_processing.rs` with a fetcher that delays
//! its scripts; here external scripts fail at once (the test document
//! fetches nothing), which still holds the parser until the failure
//! arrives. Each case also runs in the GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::time::Duration;

use common::{TestDocument, answer, load, on_big_stack};
use swb_web::{ClockMode, PageClock, ScriptRuntime};

/// Loads `html` (without and with the GC stress mode) and checks the answer
/// of `query` against `expected`.
fn check(html: &str, query: &str, expected: serde_json::Value) {
    let html = html.to_owned();
    let query = query.to_owned();
    on_big_stack(move || {
        for stress in [false, true] {
            let mut rt = load(&html, stress);
            assert_eq!(
                answer(&mut rt, &query),
                expected,
                "{html} (stress {stress})"
            );
        }
    });
}

const LOG: &str = "<script>var L = []; function log(x) { L.push(x); }</script>";

#[test]
fn text_is_read_before_write_returns() {
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><script>document.write('<p id=a>x</p><di');\
             log(!!document.getElementById('a'));document.write('v id=b></div>');\
             log(!!document.getElementById('b'));log(document.write('<i></i>'));\
             document.writeln('<pre id=p>t');document.writeln('u', 'v');\
             log(document.getElementById('p').textContent)</script>"
        ),
        "L",
        serde_json::json!([true, true, null, "t\nuv\n"]),
    );
}

#[test]
fn written_scripts_run_nested_in_order() {
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><script>\
             document.write('<script>log(\"a\");document.write(\"<script>log(\\\\\"b\\\\\")<\\\\/script>\");log(\"c\")<\\/script>');\
             log('d')</script><script>log('e:' + document.scripts.length)</script>"
        ),
        "L",
        serde_json::json!(["a", "b", "c", "d", "e:5"]),
    );
}

#[test]
fn an_external_written_script_holds_the_parser() {
    // The external script fails to load (the test document fetches
    // nothing); the parse waits for that, and the markup after the script
    // in the written text, and the later writes, come after it.
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><script>\
             document.write('<script src=x.js><\\/script>foo<script>log(\"in\")<\\/script>');\
             document.write('bar');log('end:' + document.scripts.length);</script>\
             <script>log('next:' + [].filter.call(document.body.childNodes, function (n) {{ return n.nodeType === 3; }}).map(function (n) {{ return n.data; }}).join(''))</script>"
        ),
        "L.join(' ')",
        serde_json::json!("end:3 in next:foobar"),
    );
}

#[test]
fn current_script_and_ready_state_in_written_scripts() {
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><script id=o>\
             document.write('<script id=w>log([document.currentScript.id, document.readyState, document.currentScript.previousElementSibling.id].join())<\\/script>');\
             log(document.currentScript.id)</script>"
        ),
        "L",
        serde_json::json!(["w,loading,o", "o"]),
    );
}

#[test]
fn errors_in_written_scripts_have_no_url_and_do_not_reach_the_writer() {
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><script>\
             window.onerror = function (m, s, l, c) {{ log([m, s, l, c].join('|')); }};\
             document.write('<script>throw new Error(\"boom\")<\\/script>');log('after');\
             document.write('<script>\\n\\n var 1x;<\\/script>');log('after2');</script>"
        ),
        "L.map(function (x) { return x.replace(location.href, 'DOC'); })",
        serde_json::json!([
            "Uncaught Error: boom||1|7",
            "after",
            "Uncaught SyntaxError: Failed to execute 'write' on 'Document': Invalid or unexpected token|DOC|3|6",
            "after2"
        ]),
    );
}

#[test]
fn nesting_stops_at_level_22() {
    // A script that writes itself runs 22 times (Chromium 148); the write
    // of the last one is ignored and does not throw.
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><script>window.k = (window.k || 0) + 1;\
             document.write('<script>' + document.currentScript.text + '<\\/script>');\
             log(window.k)</script>"
        ),
        "[window.k, L.length, document.scripts.length]",
        serde_json::json!([22, 22, 23]),
    );
}

#[test]
fn writes_without_an_insertion_point_are_ignored() {
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><p id=orig>x</p><script>\
             setTimeout(function () {{ document.write('<p id=t>t</p>'); log(document.readyState); \
             log(document.open() === document); log(document.close()); }}, 0);\
             addEventListener('load', function () {{ document.write('<p id=l>l</p>'); }});</script>"
        ),
        "[L, !!document.getElementById('orig'), !!document.getElementById('t'), !!document.getElementById('l')]",
        serde_json::json!([["complete", true, null], true, false, false]),
    );
}

#[test]
fn open_and_close_during_the_parse_do_nothing() {
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><script>\
             log(document.open() === document);log(document.open('text/html', 'replace') === document);\
             log(document.close());document.write('<p id=w>w</p>');log(!!document.getElementById('w'));\
             </script><p id=after>x</p>"
        ),
        "[L, !!document.getElementById('after')]",
        serde_json::json!([[true, true, null, true], true]),
    );
}

#[test]
fn an_inline_script_inserted_by_a_script_may_write() {
    // A script that a script inserts runs inside the call that inserts it,
    // while the insertion point of the parser's script exists. (An
    // external one, an async or a deferred one has none: the probe cases
    // `async-write-*` and `inserted-script-write`.)
    check(
        &format!(
            "<!DOCTYPE html>{LOG}<body><script>\
             var s = document.createElement('script'); s.text = 'document.write(\"<b id=ins></b>\")';\
             document.body.appendChild(s); log(!!document.getElementById('ins'));</script>"
        ),
        "L",
        serde_json::json!([true]),
    );
}

#[test]
fn written_text_that_waits_is_bounded() {
    on_big_stack(|| {
        let html = format!(
            "<!DOCTYPE html>{LOG}<body><script>\
             document.write('<script src=x.js><\\/script><!--');\
             var big = 'y'.repeat(24 << 20);\
             document.write(big); document.write(big);\
             document.write(big); document.write('-->'); log('done');</script><p id=after>"
        );
        let mut rt = load(&html, false);
        // The third write of 24 MiB would pass 64 MiB: dropped; the closing
        // `-->` still fits.
        assert_eq!(
            answer(
                &mut rt,
                "[L, document.body.lastChild.localName, document.querySelector('p') !== null]"
            ),
            serde_json::json!([["done"], "p", true])
        );
        assert_eq!(
            answer(
                &mut rt,
                "[].filter.call(document.body.childNodes, function (n) { return n.nodeType === 8; })[0].data.length"
            ),
            serde_json::json!(48 << 20)
        );
    });
}

#[test]
fn a_written_script_that_runs_too_long_ends_with_the_task() {
    on_big_stack(|| {
        let mut rt = ScriptRuntime::new(PageClock::new(ClockMode::Real, None)).unwrap();
        rt.set_time_limit(Duration::from_millis(40));
        let html = "<!DOCTYPE html><body><script>\
                    document.write('<script>for (;;) {}<\\/script><p id=w>w</p>');\
                    window.after = 1;</script><p id=rest>x</p>";
        rt.lend(Box::new(TestDocument::parsing(html)));
        rt.start_document();
        // The termination ends the task of the outer script too; the
        // document goes on to its end.
        assert!(rt.load_event_due());
        rt.fire_load_event();
        let host = rt.take_back().unwrap();
        let doc = host.into_any().downcast::<TestDocument>().unwrap();
        let markup = swb_dom::outer_html(&doc.dom, swb_dom::NodeId::DOCUMENT);
        assert!(markup.contains("<p id=\"rest\">x</p>"), "{markup}");
    });
}
