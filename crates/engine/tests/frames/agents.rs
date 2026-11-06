//! One script runtime per origin against Chromium (M9 feature 13b, ADR 0031
//! sections 5, 6 and 11): the cases `agents-*` and `xo-top-*` of
//! `tools/probes/web-frames.json` with Chromium's values, which carry a log between
//! origins in a cookie (two ports of one host share cookies); the rows of older
//! cases whose log comes through `postMessage` (feature 14), read in the frame; and
//! what no probe can show: which runtime holds a document, the budgets across
//! runtimes, the order of tasks. A departure has a row with its reason.

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{Value as Json, json};
use swb_engine::{AgentLimits, FrameId, MAX_AGENTS, Page};

use super::scripted::{compare_case, compare_case_at, evaluate_in};
use super::{MAIN, OTHER, Res, frame, js_value, open};
use crate::common::on_page_thread;

/// A row where swb departs on purpose: (case, query, path of the value, why).
const DEPARTURES: &[(&str, usize, &str, &str)] = &[
    (
        "agents-opaque-state",
        1,
        "[12]",
        "document.referrer of a frame document: feature 14",
    ),
    (
        "agents-opaque-state",
        2,
        "[12]",
        "document.referrer of a frame document: feature 14",
    ),
    (
        "agents-file-state",
        4,
        "[5]",
        "originAgentCluster of a sandboxed frame of a file: page: false in Chromium, true in \
         swb, as a sandboxed frame of an http: page has it in both",
    ),
    (
        "agents-removal",
        0,
        "",
        "a frame of another runtime unloads after the task that removed it, not inside remove(): \
         no script of another runtime runs inside a native. Its unload sees the frame gone \
         (`closed` true, `parent` and `top` null; Chromium: false, the window, the window). The \
         test `removal_of_a_frame_of_another_runtime` has swb's order",
    ),
    (
        "agents-removal-aba",
        0,
        "",
        "as agents-removal; a frame of the remover's runtime unloads inside remove(), children \
         first, as in agents-removal-same",
    ),
    (
        "agents-removal-same",
        0,
        "",
        "a same-origin removal fires pagehide and unload at the children first in swb (ADR 0031 \
         'Done (feature 12b1)'); Chromium at the parent first (backlog)",
    ),
    (
        "xo-top-fresh-early",
        0,
        "",
        "a cross-origin frame navigates the top level only within five seconds of a user's \
         activation in swb (owner's decision of ADR 0031 section 3, relaxed for activations); \
         the Chromium of the probe allows it without an activation",
    ),
    ("xo-top-fresh-late", 0, "", "as xo-top-fresh-early"),
    (
        "agents-focus",
        1,
        "[0]",
        "focus() from another origin: the frame's window gets `focus` after the caller's task, \
         not inside the call; Chromium: inside",
    ),
    (
        "agents-focus",
        4,
        "[0]",
        "as above: the old holder's blur comes after the caller's task",
    ),
    (
        "agents-focus",
        6,
        "[0]",
        "as above, with the moves of one task in their order",
    ),
];

macro_rules! case_test {
    ($test:ident, $case:literal) => {
        #[test]
        fn $test() {
            on_page_thread(|| {
                let failures = compare_case($case, DEPARTURES);
                assert!(failures.is_empty(), "{}", failures.join("\n"));
            });
        }
    };
}

case_test!(equal_keys_and_node_indexes_stay_apart, "agents-equal-keys");
case_test!(what_an_opaque_origin_gives, "agents-opaque-state");
case_test!(the_load_across_origins, "agents-load-order");
case_test!(a_nested_load_across_origins, "agents-load-nested");
case_test!(a_frame_navigates_between_origins, "agents-navigate-away");
case_test!(focus_across_origins, "agents-focus");
case_test!(animation_frames_in_tree_order, "agents-animation-order");
case_test!(layout_across_runtimes, "agents-layout");
case_test!(a_removal_across_origins, "agents-removal");
case_test!(a_removal_through_another_origin, "agents-removal-aba");
case_test!(a_removal_in_one_origin, "agents-removal-same");
case_test!(no_activation_no_top_navigation, "xo-top-fresh-early");
case_test!(no_activation_later, "xo-top-fresh-late");
case_test!(what_a_data_document_gives, "agents-data-state");

/// A frame of a `file:` page has an opaque origin that does not come from a sandbox: it
/// reads cookies and storage as its page does.
#[test]
fn what_a_frame_of_a_file_page_gives() {
    on_page_thread(|| {
        let failures = compare_case_at("file:///case/case.html", "agents-file-state", DEPARTURES);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// A page with a frame of another origin at `/case/x.html`.
fn page_with_frame(child: &str) -> (Page, FrameId) {
    let html = format!(
        "<!DOCTYPE html><title>top</title><iframe id=f src='{OTHER}/case/x.html'></iframe>"
    );
    let (page, _sim) = open(&html, &[("/case/x.html", Res::html(child))], true);
    let f = frame(&page, "f");
    (page, f)
}

fn top(page: &mut Page, source: &str) -> Json {
    super::scripted::evaluate(page, source)
}

/// A frame of another origin has a realm in a runtime of its own, and each
/// document sees only its own: the key 0 and equal node indexes in both.
#[test]
fn a_frame_of_another_origin_has_its_own_runtime() {
    on_page_thread(|| {
        let (mut page, f) = page_with_frame(
            "<!DOCTYPE html><title>frame</title><script>window.mark = 'frame';</script>",
        );
        assert!(page.frame_has_realm(f));
        assert_eq!(page.runtime_count(), 2);
        assert_eq!(page.frames_in_runtime_of(f), Some(1));
        assert_eq!(page.frames_in_runtime_of(FrameId::TOP), Some(1));
        assert_eq!(
            top(&mut page, "[document.title, window.mark === undefined]"),
            json!(["top", true])
        );
        assert_eq!(
            evaluate_in(&mut page, "f", "[document.title, window.mark]"),
            json!(["frame", "frame"])
        );
        // Writes stay where they are, with the same element at the same index.
        top(&mut page, "document.body.setAttribute('w', 'top'); 1");
        evaluate_in(
            &mut page,
            "f",
            "document.body.setAttribute('w', 'frame'); 1",
        );
        assert_eq!(
            top(&mut page, "document.body.getAttribute('w')"),
            json!("top")
        );
        assert_eq!(
            evaluate_in(&mut page, "f", "document.body.getAttribute('w')"),
            json!("frame")
        );
        assert_eq!(
            page.frame_document(f).unwrap().dom().document_element(),
            page.frame_document(FrameId::TOP)
                .unwrap()
                .dom()
                .document_element()
        );
    });
}

/// A promise in a frame of another origin settles through the event loop, and the
/// serialization of its value reads the frame's document.
#[test]
fn an_evaluation_in_a_frame_waits_in_its_runtime() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame("<!DOCTYPE html><title>frame</title>");
        let value = evaluate_in(
            &mut page,
            "f",
            "new Promise(r => setTimeout(() => r({ get title() { return document.title; } }), 20))",
        );
        assert_eq!(value, json!({ "title": "frame" }));
    });
}

/// `about:blank` and `about:srcdoc` children join the runtime of their creator.
#[test]
fn an_about_child_joins_the_runtime_of_its_creator() {
    on_page_thread(|| {
        let (mut page, f) = page_with_frame(
            "<!DOCTYPE html><iframe id=c></iframe><iframe id=s srcdoc='<script>window.m = 1</script>'></iframe>",
        );
        let source =
            "[c.contentWindow.origin === origin, s.contentWindow.m, c.contentDocument !== null]";
        assert_eq!(evaluate_in(&mut page, "f", source), json!([true, 1, true]));
        assert_eq!(page.runtime_count(), 2);
        assert_eq!(page.frames_in_runtime_of(f), Some(3));
    });
}

/// A frame of the top's origin inside a frame of another origin has its realm in the
/// top level's runtime, and reads the top level directly.
#[test]
fn a_b_a_prime_is_in_two_runtimes() {
    on_page_thread(|| {
        let inner = format!("{MAIN}/case/a.html");
        let child = format!("<!DOCTYPE html><iframe id=c src='{inner}'></iframe>");
        let html = format!(
            "<!DOCTYPE html><title>top</title><iframe id=f src='{OTHER}/case/x.html'></iframe>"
        );
        let (mut page, _sim) = open(
            &html,
            &[
                ("/case/x.html", Res::html(&child)),
                (
                    "/case/a.html",
                    Res::html("<!DOCTYPE html><script>window.t = top.document.title</script>"),
                ),
            ],
            true,
        );
        // The document in the middle has no script, so it needs no runtime.
        assert_eq!(page.runtime_count(), 1);
        let value = top(
            &mut page,
            "[f.contentWindow[0].t, f.contentWindow[0].frameElement]",
        );
        assert_eq!(value, json!(["top", null]));
    });
}

/// `cross-origin` query 2, `sandbox` query 0, the second half of `document-domain`:
/// Chromium's values come through `postMessage` (feature 14). The scripts run now, so
/// the test reads the same expressions in the frame.
#[test]
fn the_rows_that_report_through_messages() {
    on_page_thread(|| {
        // cross-origin, query 2.
        let child = "<!DOCTYPE html><script>
            const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
            </script>";
        let (mut page, _) = page_with_frame(child);
        let source = "[E(() => parent.document), E(() => parent.location.href), E(() => top.foo), E(() => parent.length), E(() => typeof parent.postMessage), E(() => window.frameElement), E(() => parent.frames[0] === window), E(() => { parent.location.hash = 'x'; return 'hash set'; })]";
        let got = evaluate_in(&mut page, "f", source);
        let want = js_value("cross-origin", 2);
        let want = want.as_array().unwrap();
        let picked: Vec<String> = [1usize, 2, 3, 4, 5, 7, 9, 10]
            .iter()
            .map(|&i| want[i].as_str().unwrap().replace('X', OTHER))
            .collect();
        assert_eq!(got, json!(picked));

        // sandbox, query 0: `b null SecurityError SecurityError SecurityError`.
        let html = "<!DOCTYPE html><iframe id=b sandbox='allow-scripts' srcdoc=\"<script>window.report = 'b ' + origin + ' ' + (() => { try { return String(parent.document); } catch (e) { return e.name; } })() + ' ' + (() => { try { localStorage.x = 1; return 'ls'; } catch (e) { return e.name; } })() + ' ' + (() => { try { return 'cookie:' + document.cookie; } catch (e) { return e.name; } })()</script>\"></iframe>";
        let (mut page, _) = open(html, &[], true);
        let got = evaluate_in(&mut page, "b", "window.report");
        let want = js_value("sandbox", 0);
        let want = want[0].as_str().unwrap();
        assert_eq!(format!("{} from null true", got.as_str().unwrap()), want);
        assert_eq!(page.runtime_count(), 2);

        // document-domain, the probe of one frame into the other.
        let dd = "<!DOCTYPE html><script>window.mark = 'here';</script>";
        let html = "<!DOCTYPE html><iframe id=fx src='http://x.a.localhost:8001/case/dd.html'></iframe><iframe id=fy src='http://y.a.localhost:8001/case/dd.html'></iframe>";
        let (mut page, _) = open(html, &[("/case/dd.html", Res::html(dd))], true);
        assert_eq!(page.runtime_count(), 3);
        let got = evaluate_in(
            &mut page,
            "fy",
            "(() => { try { return String(parent.frames[0].mark); } catch (err) { return err.name + ': ' + err.message; } })()",
        );
        let want: Json =
            serde_json::from_str(js_value("document-domain", 0).as_str().unwrap()).unwrap();
        assert_eq!(
            got.as_str().unwrap().replace(":8001", ":P"),
            want["probe"].as_str().unwrap()
        );
    });
}

/// `xo-focus` query 1: the frame's window gets `focus` and `blur`.
#[test]
fn the_window_of_a_frame_gets_its_focus_events() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>window.frameLog = []; for (const t of ['focus', 'blur']) addEventListener(t, () => frameLog.push('frame ' + t));</script>",
        );
        for source in [
            "f.contentWindow.focus(); 1",
            "document.body.focus(); window.focus(); 1",
            "f.contentWindow.focus(); 1",
            "window.focus(); 1",
        ] {
            top(&mut page, source);
        }
        assert_eq!(
            evaluate_in(&mut page, "f", "frameLog.join()"),
            json!("frame focus,frame blur,frame focus,frame blur")
        );
    });
}

/// swb's order of `agents-removal`: the removal holds at once for the remover, then
/// the frames unload, a parent before its child, and then a timer of the remover.
#[test]
fn removal_of_a_frame_of_another_runtime() {
    on_page_thread(|| {
        let log = "<script>const LOG = s => { const m = document.cookie.match(/lg=([^;]*)/); document.cookie = 'lg=' + (m ? m[1] : '') + encodeURIComponent(s + ';') + '; path=/'; };</script>";
        let unload = |name: &str| {
            format!(
                "{log}<script>for (const t of ['pagehide', 'unload']) addEventListener(t, () => LOG('{name} ' + t + ' ' + window.closed));</script>"
            )
        };
        let b = format!(
            "<!DOCTYPE html>{}<iframe id=c src=c.html></iframe>",
            unload("b")
        );
        let c = format!("<!DOCTYPE html>{}", unload("c"));
        let html = format!("<!DOCTYPE html>{log}<iframe id=f src='{OTHER}/case/b.html'></iframe>");
        let (mut page, _) = open(
            &html,
            &[
                ("/case/b.html", Res::html(&b)),
                ("/case/c.html", Res::html(&c)),
            ],
            true,
        );
        let got = top(
            &mut page,
            "new Promise(r => { const el = f; const w = el.contentWindow; LOG('before'); el.remove(); LOG('after ' + w.closed + ' ' + el.contentWindow); setTimeout(() => { LOG('timer'); r(decodeURIComponent(document.cookie.match(/lg=([^;]*)/)[1])); }, 50); })",
        );
        assert_eq!(
            got,
            json!(
                "before;after true null;b pagehide true;b unload true;c pagehide true;c unload true;timer;"
            )
        );
        page.pump();
        assert_eq!(page.runtime_count(), 1, "the runtime has no live document");
    });
}

/// The event loop takes the earliest due time, then the lowest sequence number,
/// across runtimes (ADR 0031 section 6).
#[test]
fn tasks_run_in_the_order_of_queuing_across_runtimes() {
    on_page_thread(|| {
        let log = "<script>const LOG = s => { const m = document.cookie.match(/lg=([^;]*)/); document.cookie = 'lg=' + (m ? m[1] : '') + encodeURIComponent(s + ';') + '; path=/'; };</script>";
        let html = format!("<!DOCTYPE html>{log}<iframe id=f src='{OTHER}/case/x.html'></iframe>");
        let child = format!("<!DOCTYPE html>{log}");
        let (mut page, _) = open(&html, &[("/case/x.html", Res::html(&child))], true);
        // Each call is a task of its own; the timers are due in their order of queuing,
        // and one with a larger delay comes after all the others.
        top(&mut page, "setTimeout(() => LOG('top 1'), 100); 1");
        evaluate_in(&mut page, "f", "setTimeout(() => LOG('frame 1'), 100); 1");
        top(
            &mut page,
            "setTimeout(() => LOG('top 2'), 100); setTimeout(() => LOG('top last'), 300); 1",
        );
        evaluate_in(&mut page, "f", "setTimeout(() => LOG('frame 2'), 100); 1");
        page.run_event_loop(Duration::from_millis(600), Duration::from_secs(5));
        let got = top(
            &mut page,
            "decodeURIComponent(document.cookie.match(/lg=([^;]*)/)[1])",
        );
        assert_eq!(got, json!("top 1;frame 1;top 2;frame 2;top last;"));
    });
}

/// A frame that reaches its heap limit loses its runtime alone: the other runtime goes
/// on, the frame's document stays and it runs no more scripts.
#[test]
fn a_runtime_at_the_heap_limit_goes_alone() {
    on_page_thread(|| {
        let (mut page, f) = page_with_frame(
            "<!DOCTYPE html><title>frame</title><script>window.a = []; setTimeout(() => { for (;;) a.push(new Array(1000).fill(1)); }, 10);</script>",
        );
        page.set_agent_limits(AgentLimits {
            heap: 64 << 20,
            page_heap: 512 << 20,
            ..AgentLimits::default()
        });
        page.run_event_loop(Duration::from_millis(100), Duration::from_secs(20));
        assert!(!page.frame_has_realm(f));
        assert_eq!(page.runtime_count(), 1);
        assert_eq!(top(&mut page, "1 + 1"), json!(2));
        assert_eq!(page.frame_document(f).unwrap().url().path(), "/case/x.html");
        // A new document of that origin runs without scripts.
        top(&mut page, "f.src = f.src; 1");
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(5));
        assert!(!page.frame_has_realm(frame(&page, "f")));
        assert_eq!(page.runtime_count(), 1);
    });
}

/// The top level's runtime at the heap limit leaves the other runtimes running.
#[test]
fn the_failure_of_the_top_level_leaves_the_others() {
    on_page_thread(|| {
        let (mut page, f) = page_with_frame(
            "<!DOCTYPE html><script>window.t = 0; setInterval(() => t++, 10);</script>",
        );
        page.set_agent_limits(AgentLimits {
            heap: 64 << 20,
            ..AgentLimits::default()
        });
        page.evaluate(
            "window.a = []; setTimeout(() => { for (;;) a.push(new Array(1000).fill(1)); }, 1); 1",
            swb_engine::JsonForm::Strict,
        );
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(20));
        assert!(!page.has_script_runtime());
        assert!(page.frame_has_realm(f));
        let before = evaluate_in(&mut page, "f", "t").as_i64().unwrap();
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(5));
        assert!(evaluate_in(&mut page, "f", "t").as_i64().unwrap() > before);
    });
}

/// Past the budget of runtimes a document runs without scripts, with its proxy the
/// rules of another origin; the page goes on.
#[test]
fn the_budget_of_runtimes() {
    on_page_thread(|| {
        let count = MAX_AGENTS + 4;
        let mut body = String::from("<!DOCTYPE html>");
        for i in 0..count {
            write!(
                body,
                "<iframe id=f{i} src='http://h{i}.localhost:8001/case/x.html'></iframe>"
            )
            .unwrap();
        }
        let (mut page, _) = open(
            &body,
            &[(
                "/case/x.html",
                Res::html("<!DOCTYPE html><script>window.m = 1</script>"),
            )],
            true,
        );
        assert_eq!(page.runtime_count(), MAX_AGENTS);
        let last = frame(&page, &format!("f{}", count - 1));
        assert!(!page.frame_has_realm(last));
        assert_eq!(
            top(&mut page, "typeof f0.contentWindow.focus"),
            json!("function")
        );
        assert_eq!(evaluate_in(&mut page, "f0", "window.m"), json!(1));
    });
}

/// A frame removed in the `load` of a document inside it shows no page after its
/// `unload` (the order around it: `agents-removal`).
#[test]
fn no_pageshow_after_unload_across_runtimes() {
    on_page_thread(|| {
        let log = "<script>const LOG = s => { const m = document.cookie.match(/lg=([^;]*)/); document.cookie = 'lg=' + (m ? m[1] : '') + encodeURIComponent(s + ';') + '; path=/'; }; for (const t of ['load', 'pageshow', 'unload']) addEventListener(t, () => LOG(ME + ' ' + t));</script>";
        let b = format!(
            "<!DOCTYPE html><script>const ME = 'b';</script>{log}<iframe src='{MAIN}/case/a.html'></iframe>"
        );
        let a = format!(
            "<!DOCTYPE html><script>const ME = 'a'; addEventListener('load', () => top.document.getElementById('p').remove());</script>{log}"
        );
        let html = format!(
            "<!DOCTYPE html><script>const ME = 'top';</script>{log}<iframe id=p src='{OTHER}/case/b.html'></iframe>"
        );
        let (mut page, _) = open(
            &html,
            &[
                ("/case/b.html", Res::html(&b)),
                ("/case/a.html", Res::html(&a)),
            ],
            true,
        );
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(5));
        let got = top(
            &mut page,
            "decodeURIComponent(document.cookie.match(/lg=([^;]*)/)[1])",
        );
        let got = got.as_str().unwrap();
        assert!(got.contains("a unload;"), "{got}");
        assert!(!got.contains("a pageshow;"), "{got}");
        assert!(!got.contains("b pageshow;"), "{got}");
        assert!(got.contains("top load;top pageshow;"), "{got}");
    });
}

/// The same inside one runtime: the listener of a container removes its frame.
#[test]
fn no_pageshow_after_unload_in_one_runtime() {
    on_page_thread(|| {
        let log = "<script>const LOG = s => { const m = document.cookie.match(/lg=([^;]*)/); document.cookie = 'lg=' + (m ? m[1] : '') + encodeURIComponent(s + ';') + '; path=/'; };</script>";
        let child = format!(
            "<!DOCTYPE html>{log}<script>for (const t of ['pageshow', 'unload']) addEventListener(t, () => LOG('b ' + t));</script>"
        );
        let html = format!(
            "<!DOCTYPE html>{log}<iframe id=f></iframe><script>f.addEventListener('load', () => f.remove()); f.src = 'b.html';</script>"
        );
        let (mut page, _) = open(&html, &[("/case/b.html", Res::html(&child))], true);
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(5));
        let got = top(
            &mut page,
            "decodeURIComponent(document.cookie.match(/lg=([^;]*)/)[1])",
        );
        assert_eq!(got, json!("b unload;"));
    });
}

/// The page's cap on the heap acts on frames: what the top level's runtime leaves is
/// for all of them, and the frame that passes it goes alone.
#[test]
fn the_page_cap_on_the_heap_drops_the_frame_that_passes_it() {
    on_page_thread(|| {
        let at = |ms: u32, mib: u32| {
            format!(
                "<!DOCTYPE html><script>setTimeout(() => {{ window.a = []; for (let i = 0; i < {mib}; i++) a.push(new Array(1 << 16).fill(1)); window.done = 1; }}, {ms});</script>"
            )
        };
        let html = "<!DOCTYPE html><title>top</title><iframe id=f src='http://a.localhost:8001/case/f.html'></iframe><iframe id=g src='http://b.localhost:8001/case/g.html'></iframe>";
        let (mut page, _sim) = open(
            html,
            &[
                ("/case/f.html", Res::html(&at(100, 40))),
                ("/case/g.html", Res::html(&at(400, 30))),
            ],
            true,
        );
        page.set_agent_limits(AgentLimits {
            heap: 64 << 20,
            page_heap: 128 << 20,
            ..AgentLimits::default()
        });
        page.run_event_loop(Duration::from_secs(1), Duration::from_secs(10));
        // 64 MiB for the frames, and `f` holds 40: `g` passes the rest at 24.
        assert_eq!(evaluate_in(&mut page, "f", "window.done"), json!(1));
        assert!(!page.frame_has_realm(frame(&page, "g")));
        assert_eq!(page.runtime_count(), 2);
        assert_eq!(top(&mut page, "1 + 1"), json!(2));
    });
}

/// Long tasks in the frame and in the top level use up the budgets of both; past the
/// page's cap an idle frame stops too.
#[test]
fn the_cap_on_long_tasks_stops_all_runtimes() {
    on_page_thread(|| {
        let busy = "window.n = 0; setInterval(() => { const end = performance.now() + 80; while (performance.now() < end); n++; }, 1);";
        let (mut page, f) = page_with_frame(&format!("<!DOCTYPE html><script>{busy}</script>"));
        page.set_agent_limits(AgentLimits {
            long_tasks: Duration::from_millis(200),
            page_long_tasks: Duration::from_millis(400),
            ..AgentLimits::default()
        });
        page.evaluate(busy, swb_engine::JsonForm::Strict);
        page.run_event_loop(Duration::from_secs(3), Duration::from_secs(5));
        assert!(page.script_runtime_mut().unwrap().stopped());
        let frame = evaluate_in(&mut page, "f", "n");
        assert!(
            frame.as_str().is_some_and(|e| e.contains("stopped")),
            "{frame}"
        );
        assert!(page.frame_has_realm(f));
    });
}

/// Frames share what the top level's runtime leaves of the page's long tasks: two busy
/// frames stop, and the top level goes on. Each alone stays inside a runtime's limit,
/// and together they would reach the page's cap, which stops all runtimes.
#[test]
fn the_long_tasks_of_frames_leave_the_top_level_running() {
    on_page_thread(|| {
        let busy = "<!DOCTYPE html><script>window.n = 0; setInterval(() => { const end = performance.now() + 80; while (performance.now() < end); n++; }, 1);</script>";
        let html = "<!DOCTYPE html><title>top</title><iframe id=f src='http://a.localhost:8001/case/x.html'></iframe><iframe id=g src='http://b.localhost:8001/case/x.html'></iframe>";
        let (mut page, _sim) = open(html, &[("/case/x.html", Res::html(busy))], true);
        page.set_agent_limits(AgentLimits {
            long_tasks: Duration::from_millis(200),
            page_long_tasks: Duration::from_millis(400),
            ..AgentLimits::default()
        });
        top(&mut page, "window.t = 0; setInterval(() => t++, 5); 1");
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(5));
        for id in ["f", "g"] {
            let frame = evaluate_in(&mut page, id, "n");
            assert!(
                frame.as_str().is_some_and(|e| e.contains("stopped")),
                "{frame}"
            );
        }
        assert!(!page.script_runtime_mut().unwrap().stopped());
        let before = top(&mut page, "t").as_i64().unwrap();
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(5));
        assert!(top(&mut page, "t").as_i64().unwrap() > before);
    });
}

/// Frames share what the top level's runtime leaves of the page's heap: however much
/// they hold, the top level keeps its own limit. Each frame below stays inside a
/// runtime's limit, and together they would leave the top level less than it holds.
#[test]
fn frames_at_the_heap_limit_leave_the_top_level_its_heap() {
    on_page_thread(|| {
        let at = |ms: u32| {
            format!(
                "<!DOCTYPE html><script>setTimeout(() => {{ window.a = []; for (let i = 0; i < 50; i++) a.push(new Array(1 << 16).fill(1)); }}, {ms});</script>"
            )
        };
        let html = "<!DOCTYPE html><title>top</title><iframe id=f src='http://a.localhost:8001/case/f.html'></iframe><iframe id=g src='http://b.localhost:8001/case/g.html'></iframe>";
        let (mut page, _sim) = open(
            html,
            &[
                ("/case/f.html", Res::html(&at(100))),
                ("/case/g.html", Res::html(&at(300))),
            ],
            true,
        );
        page.set_agent_limits(AgentLimits {
            heap: 64 << 20,
            page_heap: 128 << 20,
            ..AgentLimits::default()
        });
        // The frames first: the top level allocates when they hold what they can.
        page.run_event_loop(Duration::from_millis(700), Duration::from_secs(10));
        top(
            &mut page,
            "window.keep = []; for (let i = 0; i < 24; i++) keep.push(new Array(1 << 16).fill(1)); window.t = 0; setInterval(() => { t++; keep.push(new Array(1000).fill(t)); }, 5); 1",
        );
        page.run_event_loop(Duration::from_millis(300), Duration::from_secs(10));
        assert!(page.has_script_runtime());
        let before = top(&mut page, "t").as_i64().unwrap();
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(5));
        assert!(top(&mut page, "t").as_i64().unwrap() > before);
        assert!(page.script_heap_total() <= 128 << 20);
    });
}

/// The frames share one budget of bytes outside the heap, and the top level's runtime
/// has its own: frames that fill theirs leave it its Blob URLs.
#[test]
fn frames_share_a_byte_budget_and_the_top_level_has_its_own() {
    on_page_thread(|| {
        let child = "<!DOCTYPE html><script>window.n = 0; const b = new Uint8Array(4 << 20); try { for (let i = 0; i < 64; i++) { URL.createObjectURL(new Blob([b])); n++; } } catch (e) { window.failed = e.name; }</script>";
        let html = "<!DOCTYPE html><title>top</title><iframe id=f src='http://a.localhost:8001/case/x.html'></iframe><iframe id=g src='http://b.localhost:8001/case/x.html'></iframe>";
        let (mut page, _sim) = open(html, &[("/case/x.html", Res::html(child))], true);
        let made = evaluate_in(&mut page, "f", "n").as_i64().unwrap()
            + evaluate_in(&mut page, "g", "n").as_i64().unwrap();
        assert_eq!(made, 64);
        // Whichever frame loaded second.
        let failed = [
            evaluate_in(&mut page, "f", "String(window.failed)"),
            evaluate_in(&mut page, "g", "String(window.failed)"),
        ];
        assert!(failed.contains(&json!("QuotaExceededError")), "{failed:?}");
        assert_eq!(
            top(&mut page, "typeof URL.createObjectURL(new Blob(['x']))"),
            json!("string")
        );
    });
}

/// The timers of a removed frame end with it, also while another frame keeps its runtime
/// alive.
#[test]
fn a_removed_frame_runs_no_more_tasks() {
    on_page_thread(|| {
        let child = "<!DOCTYPE html><script>let i = 0; setInterval(() => { document.cookie = name + '=' + ++i + '; path=/'; }, 1);</script>";
        let html = format!(
            "<!DOCTYPE html><title>top</title><iframe id=f name=gone src='{OTHER}/case/x.html'></iframe><iframe id=g name=kept src='{OTHER}/case/x.html'></iframe>"
        );
        let (mut page, _sim) = open(&html, &[("/case/x.html", Res::html(child))], true);
        let count = |name: &str| format!("Number(document.cookie.match(/{name}=([0-9]+)/)[1])");
        page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
        assert!(top(&mut page, &count("gone")).as_i64().unwrap() > 0);
        top(&mut page, "f.remove(); 1");
        page.run_event_loop(Duration::from_millis(20), Duration::from_secs(5));
        let after = top(&mut page, &count("gone"));
        let kept = top(&mut page, &count("kept")).as_i64().unwrap();
        page.run_event_loop(Duration::from_millis(100), Duration::from_secs(5));
        assert_eq!(top(&mut page, &count("gone")), after);
        assert!(top(&mut page, &count("kept")).as_i64().unwrap() > kept);
    });
}

/// A runtime goes with the last document in it, so a sandboxed frame that is replaced
/// again and again (each document a new opaque origin) does not use up the budget.
#[test]
fn a_replaced_sandboxed_frame_leaves_no_runtime_behind() {
    on_page_thread(|| {
        let (mut page, _sim) = open(
            "<!DOCTYPE html><title>top</title><div id=h></div>",
            &[(
                "/case/x.html",
                Res::html("<!DOCTYPE html><script>window.m = 1</script>"),
            )],
            true,
        );
        for _ in 0..100 {
            top(
                &mut page,
                "(() => { h.textContent = ''; const f = document.createElement('iframe'); f.sandbox = 'allow-scripts'; f.src = 'x.html'; h.append(f); return 1; })()",
            );
            page.run_event_loop(Duration::from_millis(5), Duration::from_secs(5));
            assert!(page.runtime_count() <= 2, "{}", page.runtime_count());
        }
    });
}

/// With 32 runtimes, the removal of a frame leaves room for another origin.
#[test]
fn a_removed_frame_gives_its_runtime_back() {
    on_page_thread(|| {
        let mut body = String::from("<!DOCTYPE html>");
        for i in 0..MAX_AGENTS - 1 {
            write!(
                body,
                "<iframe id=f{i} src='http://h{i}.localhost:8001/case/x.html'></iframe>"
            )
            .unwrap();
        }
        let child = Res::html("<!DOCTYPE html><script>window.m = 1</script>");
        let (mut page, _) = open(&body, &[("/case/x.html", child)], true);
        assert_eq!(page.runtime_count(), MAX_AGENTS);
        top(
            &mut page,
            "f0.remove(); const n = document.createElement('iframe'); n.id = 'late'; n.src = 'http://late.localhost:8001/case/x.html'; document.body.append(n); 1",
        );
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(5));
        assert_eq!(page.runtime_count(), MAX_AGENTS);
        assert_eq!(evaluate_in(&mut page, "late", "window.m"), json!(1));
    });
}
