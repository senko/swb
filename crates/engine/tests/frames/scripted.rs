//! Scripts in same-origin frame documents against Chromium (M9 feature 12b1,
//! ADR 0031 sections 2, 5 and 6): every query of the cases `scripted-*` of
//! `tools/probes/web-frames.json`, and the frame queries of the cases that
//! `QUERIES_LATER` listed until this feature (`geometry`, `about-blank` without
//! `document.open`, `load-order`, `limits`, `self-embed`, `windowproxy`,
//! `sandbox`), with Chromium's values. A departure has a row with its reason.

use std::time::{Duration, Instant};

use serde_json::Value as Json;
use swb_engine::{Evaluation, JsonForm, Page, Size};

use super::{MAIN, OTHER, Res, Sim, case_html, case_resource, open, open_at, probe_file, results};
use crate::common::{new_page, on_page_thread};

/// A row where swb departs on purpose: (case, query, path of the value, why).
/// An empty path is the whole value of the query.
const DEPARTURES: &[(&str, usize, &str, &str)] = &[
    (
        "scripted-parent-layout",
        0,
        "[10]",
        "a frame whose iframe has display: none keeps its last layout in swb \
         (documentElement.clientWidth stays); Chromium gives 0 without a box",
    ),
    (
        "scripted-self-remove",
        0,
        "[0]",
        "the parent's load event fires after the task that removed the last frame that delayed \
         it; Chromium fires it inside remove(), before the script goes on",
    ),
    (
        "scripted-retired",
        4,
        "[6]",
        "the interfaces of a retired window stay: w.Event is a constructor in swb; Chromium has \
         no w.Event after the frame is gone",
    ),
    (
        "scripted-retired",
        4,
        "[8]",
        "the interfaces of a retired window stay (w.XMLHttpRequest)",
    ),
    (
        "scripted-retired",
        4,
        "[9]",
        "with the row above: the listener and XMLHttpRequest of the retired window run in swb",
    ),
    (
        "scripted-retired",
        5,
        "[6]",
        "the interfaces of a retired window stay (w.Image)",
    ),
    (
        "limits",
        0,
        "[4]",
        "the nesting limit: 32 in swb (ADR 0031 section 11), no limit below 300 in Chromium",
    ),
    (
        "limits",
        0,
        "",
        "the realm budget of 256 (ADR 0031 section 11) runs out in the first part of the query: \
         the test `frames_in_one_task_and_their_limits` compares it in two parts",
    ),
    (
        "windowproxy",
        4,
        "",
        "no joint session history (ADR 0031 section 2): history.back() traverses the top level",
    ),
    (
        "windowproxy",
        3,
        "[0]",
        "no joint session history: history.length counts the entries of the top level",
    ),
    (
        "windowproxy",
        3,
        "[1]",
        "no joint session history: history.length counts the entries of the top level",
    ),
    (
        "windowproxy",
        0,
        "[25]",
        "no joint session history: history.length counts the entries of the top level",
    ),
    (
        "windowproxy",
        1,
        "[5]",
        "no joint session history: history.length counts the entries of the top level",
    ),
    (
        "geometry",
        0,
        "[0]",
        "documentElement.clientHeight in quirks mode: Chromium's html box fills the viewport (a \
         quirk), swb's has the height of its content; the same for the rows below; not a matter \
         of frames (backlog)",
    ),
    ("geometry", 0, "[1]", "as above"),
    ("geometry", 0, "[2]", "as above"),
    ("geometry", 0, "[4]", "as above"),
    ("geometry", 0, "[5]", "as above"),
    ("geometry", 0, "[6]", "as above"),
    (
        "scripted-window-members",
        0,
        "[21]",
        "the order of the own names of the global object: `undefined` before `Object` in swb; \
         not a matter of frames",
    ),
    (
        "scripted-retired",
        5,
        "[0]",
        "document.open on a frame document: feature 15",
    ),
    (
        "scripted-retired",
        5,
        "[1]",
        "document.write on a frame document: feature 15",
    ),
    (
        "sandbox",
        0,
        "[0]",
        "the log comes through postMessage: feature 14; the test \
         `the_rows_that_report_through_messages` reads the same values in the frame",
    ),
];

/// Entries of a log that differ by the timing of a load, not by a rule: (case,
/// entry, why). The test removes them from both sides.
const IGNORED_ENTRIES: &[(&str, &str, &str)] = &[
    (
        "scripted-frame-document",
        "img load",
        "Chromium's preload scanner starts the image before the parser reaches it, and its load \
         comes before the end of the parse; swb starts it when the parser inserts the element",
    ),
    (
        "scripted-sandbox-scripts",
        "svgLoad=1",
        "the frame that allows scripts: swb fires no `load` at an `svg` element; not a matter of \
         frames (backlog)",
    ),
];

/// Text that differs between runs: ports and the case's folder.
fn normalize(text: &str) -> String {
    let mut out = text.to_owned();
    for (from, to) in [
        (MAIN, "http://H0"),
        (OTHER, "http://H1"),
        ("/case/", "/case-N/"),
    ] {
        out = out.replace(from, to);
    }
    let mut result = String::new();
    let mut rest = out.as_str();
    while let Some(at) = rest.find("127.0.0.1:") {
        let (head, tail) = rest.split_at(at);
        result.push_str(head);
        result.push('H');
        rest = tail["127.0.0.1:".len()..].trim_start_matches(|c: char| c.is_ascii_digit());
    }
    result.push_str(rest);
    let mut clean = String::new();
    let mut chars = result.as_str();
    while let Some(at) = chars.find("case-") {
        let (head, tail) = chars.split_at(at);
        clean.push_str(head);
        clean.push_str("case-N");
        chars = tail["case-".len()..].trim_start_matches(|c: char| c.is_ascii_digit() || c == 'N');
    }
    clean.push_str(chars);
    clean.replace("case-N/case-N.html", "case-N/case.html")
}

/// The value in a comparable form: strings without ports.
fn comparable(value: &Json) -> Json {
    match value {
        Json::String(text) => Json::String(normalize(text)),
        Json::Array(items) => Json::Array(items.iter().map(comparable).collect()),
        other => other.clone(),
    }
}

/// The differences of two values, by path.
fn differences(path: &str, swb: &Json, chromium: &Json, out: &mut Vec<(String, String)>) {
    match (swb, chromium) {
        (Json::Array(a), Json::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                differences(&format!("{path}[{index}]"), x, y, out);
            }
        }
        _ if swb == chromium => {}
        _ => out.push((path.to_owned(), format!("swb {swb}, Chromium {chromium}"))),
    }
}

pub(super) fn evaluate(page: &mut Page, source: &str) -> Json {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap_or(Json::String(text)),
        Some(Evaluation::Error(text)) => Json::String(format!("error: {text}")),
        None => Json::String("no evaluation".to_owned()),
    }
}

/// The resources of a case of the probe file, under `/case/`.
fn resources(name: &str) -> Vec<(String, Res)> {
    let probe = probe_file();
    let case = probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap();
    let Some(list) = case["resources"].as_object() else {
        return Vec::new();
    };
    list.keys()
        .map(|path| (format!("/case/{path}"), case_resource(name, path)))
        .collect()
}

/// The `frame` of each query of a case of the probe file: `#id` of an `iframe` of
/// the top-level document.
fn query_frames(name: &str) -> Vec<Option<String>> {
    let probe = probe_file();
    let case = probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap();
    case["measure"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| {
            q["frame"]
                .as_str()
                .map(|f| f.trim_start_matches('#').to_owned())
        })
        .collect()
}

/// Evaluates `source` in the frame of the `iframe` `id` of the top-level document.
pub(super) fn evaluate_in(page: &mut Page, id: &str, source: &str) -> Json {
    let frame = super::frame(page, id);
    match page.evaluate_in_frame(frame, source, JsonForm::Strict, Duration::from_secs(10)) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap_or(Json::String(text)),
        Some(Evaluation::Error(text)) => Json::String(format!("error: {text}")),
        None => Json::String("no evaluation".to_owned()),
    }
}

/// Opens a case with JavaScript on and compares each `js` query. Returns the
/// differences that are no departures.
fn run_case(name: &str) -> Vec<String> {
    compare_case(name, DEPARTURES)
}

/// [`run_case`] with other departures: (case, query, path, why).
pub(super) fn compare_case(name: &str, departures: &[(&str, usize, &str, &str)]) -> Vec<String> {
    compare_case_at(&format!("{MAIN}/case/case.html"), name, departures)
}

/// [`compare_case`] with the page at the URL `top`.
pub(super) fn compare_case_at(
    top: &str,
    name: &str,
    departures: &[(&str, usize, &str, &str)],
) -> Vec<String> {
    let owned = resources(name);
    let list: Vec<(&str, Res)> = owned.iter().map(|(p, r)| (p.as_str(), r.clone())).collect();
    let html = case_html(name)
        .replace("$OTHER", OTHER)
        .replace("$FOLDER", "case");
    let (mut page, _sim) = open_at(top, &html, &list, true);
    let mut failures = Vec::new();
    let queries = results(name);
    let frames = query_frames(name);
    for (query, result) in queries.iter().filter(|r| r["kind"] == "js").enumerate() {
        let source = result["label"].as_str().unwrap();
        let without = |value: Json| match value {
            Json::Array(items) => Json::Array(
                items
                    .into_iter()
                    .filter(|item| {
                        !IGNORED_ENTRIES
                            .iter()
                            .any(|&(c, entry, _)| c == name && item == entry)
                    })
                    .collect(),
            ),
            other => other,
        };
        let value = match frames.get(query).and_then(Option::as_deref) {
            Some(id) => evaluate_in(&mut page, id, source),
            None => evaluate(&mut page, source),
        };
        let swb = without(comparable(&value));
        let want = without(comparable(&result["value"]));
        let mut found = Vec::new();
        differences("", &swb, &want, &mut found);
        for (path, text) in found {
            let departs = departures
                .iter()
                .any(|&(c, q, p, _)| c == name && q == query && (p.is_empty() || p == path));
            if !departs {
                failures.push(format!("{name} query {query}{path}: {text}"));
            }
        }
    }
    failures
}

macro_rules! case_test {
    ($test:ident, $case:literal) => {
        #[test]
        fn $test() {
            on_page_thread(|| {
                let failures = run_case($case);
                assert!(failures.is_empty(), "{}", failures.join("\n"));
            });
        }
    };
}

/// Opens `html` with JavaScript on.
fn scripted_page(html: &str, resources: &[(&str, Res)]) -> Page {
    open(html, resources, true).0
}

/// A frame that no script touches has no realm; one read makes it, and
/// `evaluate_in_frame` runs in it.
#[test]
fn a_frame_gets_its_realm_at_its_first_need() {
    on_page_thread(|| {
        let mut page = scripted_page(
            "<!DOCTYPE html><iframe id=a></iframe><iframe id=b srcdoc='<p>x'></iframe>\
             <iframe id=s srcdoc='<script>window.ran = 1</script>'></iframe>",
            &[],
        );
        let (a, b, s) = (
            super::frame(&page, "a"),
            super::frame(&page, "b"),
            super::frame(&page, "s"),
        );
        assert!(!page.frame_has_realm(a));
        assert!(!page.frame_has_realm(b));
        assert!(page.frame_has_realm(s));
        assert_eq!(
            evaluate(&mut page, "[frames.length, s.contentWindow.ran]"),
            serde_json::json!([3, 1])
        );
        assert!(!page.frame_has_realm(a));
        assert_eq!(evaluate(&mut page, "typeof a.contentWindow"), "object");
        assert!(page.frame_has_realm(a));
        let answer = page.evaluate_in_frame(
            b,
            "[location.href, parent.frames.length, self === parent.b.contentWindow]",
            JsonForm::Strict,
            Duration::from_secs(5),
        );
        assert_eq!(
            answer,
            Some(Evaluation::Value(r#"["about:srcdoc",3,true]"#.to_owned()))
        );
        assert!(page.frame_has_realm(b));
    });
}

/// A sandbox without `allow-scripts`: the document's scripts do not run, also
/// one that the parent appends; the parent reaches the nodes.
#[test]
fn a_sandbox_stops_scripts_and_not_the_parent() {
    on_page_thread(|| {
        let mut page = scripted_page(
            "<!DOCTYPE html><iframe id=c sandbox='allow-same-origin' \
             srcdoc='<p id=p>c<script>window.ran = 1</script>'></iframe>",
            &[],
        );
        let value = evaluate(
            &mut page,
            "(() => { const d = c.contentDocument, w = c.contentWindow; const s = d.createElement('script'); \
             s.textContent = 'window.ranToo = 1'; d.body.appendChild(s); \
             return [d.getElementById('p') !== null, String(w.ran), String(w.ranToo)]; })()",
        );
        assert_eq!(value, serde_json::json!([true, "undefined", "undefined"]));
    });
}

case_test!(
    a_sandbox_stops_what_it_stops_in_chromium,
    "scripted-sandbox-scripts"
);
case_test!(
    a_child_frame_cannot_be_deleted,
    "scripted-window-delete-index"
);
case_test!(
    a_navigation_has_the_flags_of_its_start,
    "scripted-sandbox-timing"
);
case_test!(
    an_about_frame_has_the_cookies_and_storage_of_its_creator,
    "scripted-about-frame-state"
);

/// Sets past the limit of navigations start nothing, so an earlier set is the one
/// that commits, and it has the flags of its own moment: the `sandbox` change after
/// the loop does not count (as the row `c1` of `scripted-sandbox-timing`).
#[test]
fn the_set_that_commits_has_its_own_flags() {
    on_page_thread(|| {
        let c = Res::html("<!DOCTYPE html><script>parent.ran = 1</script>");
        let mut page = scripted_page(
            "<!DOCTYPE html><iframe id=f sandbox='allow-same-origin'></iframe>",
            &[("/case/c.html", c)],
        );
        let value = evaluate(
            &mut page,
            "new Promise(r => { for (let i = 0; i < 100; i++) f.src = 'c.html?' + i; \
             f.removeAttribute('sandbox'); \
             setTimeout(() => r([String(window.ran), f.contentDocument.URL.includes('c.html')]), 300); })",
        );
        assert_eq!(value, serde_json::json!(["undefined", true]));
    });
}

/// A limit that ends a script of a frame document does not hold the page's
/// load: nothing else may come to check the frame again.
#[test]
fn a_limit_in_a_frame_does_not_hold_the_load_of_the_page() {
    const STOP: &str = "for (;;) {}";
    let frames = [
        format!("<iframe srcdoc=\"<script>{STOP}</script>\"></iframe>"),
        format!(
            "<iframe srcdoc=\"<script>addEventListener('DOMContentLoaded', () => {{ {STOP} }})</script>\"></iframe>"
        ),
        format!(
            "<iframe srcdoc=\"<script>document.addEventListener('readystatechange', () => {{ if (document.readyState == 'complete') {{ {STOP} }} }})</script>\"></iframe>"
        ),
        format!(
            "<iframe srcdoc=\"<script>addEventListener('load', () => {{ {STOP} }})</script>\"></iframe>"
        ),
        format!("<iframe srcdoc='<body onload=\"{STOP}\">'></iframe>"),
        format!("<iframe onload='{STOP}' srcdoc='<p>x'></iframe>"),
        format!(
            "<iframe srcdoc=\"<iframe srcdoc='<script>addEventListener(&quot;load&quot;, () => {{ {STOP} }})</script>'></iframe>\"></iframe>"
        ),
    ];
    on_page_thread(move || {
        for frame in &frames {
            let html = format!("<!DOCTYPE html><script>var ran = 1</script>{frame}");
            let sim = Sim::new(&[("/case/case.html", Res::html(&html))]);
            let mut page = new_page(sim, 4, Size::new(800.0, 600.0));
            page.set_javascript(true);
            page.navigate(super::url(&format!("{MAIN}/case/case.html")));
            let deadline = Instant::now() + Duration::from_secs(5);
            while page.script_runtime_mut().is_none() && Instant::now() < deadline {
                page.process_network();
            }
            page.script_runtime_mut()
                .unwrap()
                .set_time_limit(Duration::from_millis(100));
            assert!(page.wait_until_loaded(Duration::from_secs(20)), "{frame}");
            assert_eq!(evaluate(&mut page, "document.readyState"), "complete");
        }
    });
}

/// `document.write` of the parent into a sandboxed frame runs no script.
#[test]
fn a_sandbox_stops_what_a_parent_writes() {
    on_page_thread(|| {
        let mut page = scripted_page(
            "<!DOCTYPE html><iframe id=c sandbox='allow-same-origin' srcdoc='<p>x'></iframe>",
            &[],
        );
        let value = evaluate(
            &mut page,
            "new Promise(r => { window.hits = []; const d = c.contentDocument; \
             d.write(\"<script>parent.hits.push('written')<\\/script><img src=x onerror=\\\"parent.hits.push('img')\\\">\"); \
             d.close(); setTimeout(() => r(hits), 100); })",
        );
        assert_eq!(value, serde_json::json!([]));
    });
}

/// `src = 'javascript:...'` runs in the frame's realm as a task, and an empty
/// one does nothing (core-js).
#[test]
fn a_javascript_url_runs_in_the_frame() {
    on_page_thread(|| {
        let mut page = scripted_page("<!DOCTYPE html><iframe id=f></iframe>", &[]);
        let value = evaluate(
            &mut page,
            "new Promise(r => { const w = f.contentWindow; f.src = 'javascript:window.seen = location.href'; \
             setTimeout(() => { f.src = 'javascript:'; setTimeout(() => r([w.seen, f.contentDocument.URL]), 0); }, 0); })",
        );
        assert_eq!(value, serde_json::json!(["about:blank", "about:blank"]));
    });
}

/// More frames than the budget of realms: the windows past it have no target
/// and `contentDocument` is `null`; the page goes on.
#[test]
fn the_budget_of_realms_stops_realms_and_not_the_page() {
    on_page_thread(|| {
        let mut page = scripted_page("<!DOCTYPE html><p>x", &[]);
        let value = evaluate(
            &mut page,
            "(() => { const fs = []; for (let i = 0; i < 300; i++) { const f = document.createElement('iframe'); \
             document.body.appendChild(f); fs.push(f); } \
             const docs = fs.filter(f => f.contentDocument !== null).length; \
             const refused = (() => { try { return String(fs[299].contentWindow.document); } catch (e) { return e.name; } })(); \
             return [frames.length, docs, refused]; })()",
        );
        assert_eq!(value, serde_json::json!([300, 255, "SecurityError"]));
    });
}

/// A console message has the number of the frame of its document.
#[test]
fn a_console_message_names_its_frame() {
    on_page_thread(|| {
        let page = scripted_page(
            "<!DOCTYPE html><iframe id=a srcdoc='<script>console.log(\"from a\")</script>'></iframe>\
             <script>console.log('from top')</script>",
            &[],
        );
        let a = super::frame(&page, "a");
        let messages = page.console_messages();
        let text = |t: &str| messages.iter().find(|m| m.text == t).map(|m| m.frame);
        assert_eq!(text("from top"), Some(0));
        assert_eq!(text("from a"), Some(a.number()));
    });
}

case_test!(parent_layout_shows_in_the_frame, "scripted-parent-layout");
case_test!(
    the_first_navigation_reuses_the_window,
    "scripted-initial-reuse"
);
case_test!(a_retired_window_behaves_as_in_chromium, "scripted-retired");
case_test!(a_frame_can_remove_its_own_iframe, "scripted-self-remove");
case_test!(
    load_listeners_add_and_remove_frames,
    "scripted-load-recursion"
);
case_test!(
    the_parser_makes_frames_with_their_load,
    "scripted-parser-frames"
);
case_test!(
    scripts_of_a_frame_document_run_in_order,
    "scripted-frame-document"
);
case_test!(location_of_a_frame_navigates_it, "scripted-location");
case_test!(
    a_frame_of_the_origin_navigates_the_top,
    "scripted-nav-top-none"
);
case_test!(a_sandbox_refuses_the_top_navigation, "scripted-nav-top-sb");
case_test!(
    the_activation_token_refuses_a_script,
    "scripted-nav-top-sb-activation"
);
case_test!(
    the_top_navigation_token_allows,
    "scripted-nav-top-sb-allowed"
);
case_test!(a_sandbox_refuses_the_parent, "scripted-nav-parent-sb");
case_test!(replace_names_itself_in_the_error, "scripted-nav-replace-sb");
case_test!(a_sandboxed_frame_navigates_itself, "scripted-nav-self-sb");
case_test!(a_script_submit_needs_allow_forms, "scripted-sandbox-submit");
case_test!(window_members_with_frames, "scripted-window-members");
case_test!(the_event_loop_runs_each_document, "scripted-event-loop");
case_test!(geometry_from_scripts, "geometry");
case_test!(load_order_with_frame_scripts, "load-order");

/// The case `about-blank` up to the call of `document.open` (feature 15).
#[test]
fn about_blank_is_synchronous() {
    on_page_thread(|| {
        let html = case_html("about-blank").replace("$OTHER", OTHER);
        let (mut page, _sim) = open(&html, &[], true);
        let log = comparable(&evaluate(&mut page, "log.slice(0, 8)"));
        let want = comparable(&results("about-blank")[0]["value"]);
        assert_eq!(log, Json::Array(want.as_array().unwrap()[..8].to_vec()));
    });
}

/// The case `limits` in two parts: the realm budget of ADR 0031 section 11
/// runs out in the first one, so the second one needs a new page.
#[test]
fn frames_in_one_task_and_their_limits() {
    on_page_thread(|| {
        let want = comparable(&results("limits")[0]["value"]);
        let html = case_html("limits").replace("$OTHER", OTHER);
        let (mut page, _sim) = open(&html, &[], true);
        let first = evaluate(
            &mut page,
            "(() => { const out = []; const fs = []; for (let i = 0; i < 1100; i++) { const f = document.createElement('iframe'); document.body.appendChild(f); fs.push(f); } out.push(frames.length, fs.filter(f => f.contentWindow === null).length, fs.findIndex(f => f.contentWindow === null)); fs.forEach(f => f.remove()); out.push(frames.length); return out; })()",
        );
        assert_eq!(first, Json::Array(want.as_array().unwrap()[..4].to_vec()));
        let (mut page, _sim) = open(&html, &[], true);
        let depth = evaluate(
            &mut page,
            "(() => { let doc = document, depth = 0; for (let i = 0; i < 300; i++) { const f = doc.createElement('iframe'); doc.body.appendChild(f); if (!f.contentWindow) break; doc = f.contentDocument; depth++; } return depth; })()",
        );
        assert_eq!(depth, Json::from(swb_engine::MAX_DEPTH));
    });
}

case_test!(a_page_that_embeds_itself, "self-embed");
case_test!(window_proxy_of_a_frame, "windowproxy");
case_test!(sandbox_flags_in_scripts, "sandbox");
