//! Storage keys, `storage` events, `document.cookie` of cross-site frames and
//! `document.referrer` of frames against Chromium (M9 feature 14b, ADR 0031 section 3 and
//! "Done (feature 14b)"): the cases `storage-key-*`, `storage-event-*`, `storage-retired-write`,
//! `cookies-frames-script`, `referrer-*` and `domain-about-frames` of
//! `tools/probes/web-frames.json` with Chromium's values (three equal runs; the probe
//! tool starts Chromium with third-party storage partitioning on, which Playwright turns
//! off), and what no probe can show: the budgets, what retires, the key of a document that
//! has none. A departure has a row with its reason.

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{Value as Json, json};
use swb_engine::Page;

use super::scripted::{compare_case, compare_case_at, compare_queries, evaluate, evaluate_in};
use super::{OTHER, Res, frame, open};
use crate::common::{finish_loading, on_page_thread};

/// A row where swb departs on purpose: (case, query, path of the value, why).
const DEPARTURES: &[(&str, usize, &str, &str)] = &[
    (
        "domain-about-frames",
        0,
        "[5]",
        "an about:blank child of a data: frame has the data: URL (cut) as its referrer in \
         Chromium; swb gives nothing, so that a document is not a referrer",
    ),
    (
        "referrer-about-blank",
        0,
        "[2]",
        "swb has no about:blank URL with a fragment: a navigation to about:blank instead of \
         the initial document with the fragment",
    ),
    (
        "referrer-about-blank",
        0,
        "[3]",
        "swb has no about:blank URL with a query: a navigation to about:blank instead",
    ),
    (
        "referrer-about-blank",
        0,
        "[6]",
        "swb reads the attributes of a new frame after the task that inserted it (older than \
         the referrer), so a src set in that task counts as at the insertion",
    ),
    (
        "referrer-about-blank",
        0,
        "[7]",
        "swb has no about:blank URL with a fragment: a navigation to about:blank instead of \
         the same document",
    ),
    (
        "referrer-about-blank",
        0,
        "[8]",
        "swb has no about:blank URL with a query; the referrer is the same",
    ),
    (
        "referrer-about-blank",
        0,
        "[17]",
        "an about:blank that a frame of another origin navigates to has the parent's origin in \
         swb and the frame's in Chromium (older than the referrer), so the parent reads it; \
         the test an_about_blank_document_gets_no_url_of_another_origin",
    ),
    (
        "referrer-about-blank-file",
        0,
        "[3]",
        "the same-task src as row [6] of referrer-about-blank, and swb keeps the origin of the \
         file: page in a navigated about:blank (Chromium: another opaque origin)",
    ),
    (
        "referrer-about-blank-file",
        0,
        "[4]",
        "swb keeps the origin of the file: page in a navigated about:blank (Chromium: another \
         opaque origin), so the page reads it",
    ),
];

macro_rules! case_test {
    ($test:ident, $($case:literal),+) => {
        #[test]
        fn $test() {
            on_page_thread(|| {
                let mut failures = Vec::new();
                $(failures.extend(compare_case($case, DEPARTURES));)+
                assert!(failures.is_empty(), "{}", failures.join("\n"));
            });
        }
    };
}

/// Runs the pages of a chain of top-level navigations (`window.result` is set by the
/// last) and returns the result.
fn run_chain(name: &str) -> Json {
    let (mut page, _sim) = super::open_case(name);
    for _ in 0..60 {
        let result = evaluate(&mut page, "window.result");
        if !result.is_null() {
            return result;
        }
        evaluate(&mut page, "new Promise(r => setTimeout(r, 100))");
        finish_loading(&mut page, Duration::from_secs(20));
    }
    panic!("{name}: the chain did not end");
}

macro_rules! chain_test {
    ($test:ident, $case:literal) => {
        #[test]
        fn $test() {
            on_page_thread(|| {
                let want = super::js_value($case, 0);
                assert_eq!(run_chain($case), want);
            });
        }
    };
}

chain_test!(
    a_third_party_frame_has_an_area_per_top_level_site,
    "storage-key-partition"
);
chain_test!(
    same_site_frames_share_the_area_of_their_origin,
    "storage-key-same-site"
);
chain_test!(
    a_first_party_frame_shares_with_its_top_level,
    "storage-key-same-origin-frame"
);
// A cross-site ancestor above a same-site parent counts: the whole chain gives the key.
chain_test!(the_whole_chain_of_ancestors_counts, "storage-key-chain");

case_test!(
    events_reach_the_other_documents_of_a_key,
    "storage-event-basic"
);
case_test!(events_run_as_tasks, "storage-event-order");
case_test!(handlers_and_the_event_interface, "storage-event-handlers");
case_test!(documents_that_retire_get_no_event, "storage-event-retire");
case_test!(
    events_follow_the_key_and_not_the_nesting,
    "storage-event-nested"
);
case_test!(events_stay_in_their_origin, "storage-event-cross-origin");
case_test!(a_retired_document_has_no_storage, "storage-retired-write");
case_test!(frames_set_cookies_by_the_site_rule, "cookies-frames-script");
case_test!(
    about_frames_have_the_domain_of_their_origin,
    "domain-about-frames"
);

case_test!(
    initial_documents_srcdoc_and_nested_frames_have_referrers,
    "referrer-basic"
);
case_test!(
    the_source_of_a_navigation_is_its_document,
    "referrer-initiator"
);
case_test!(a_sandbox_does_not_change_the_referrer, "referrer-sandbox");
case_test!(
    only_the_initial_about_blank_gets_the_url_of_its_creator,
    "referrer-about-blank"
);
/// A `file:` page: its `srcdoc` and `data:` frames get no referrer (the origin of a `file:`
/// page is opaque), and its initial `about:blank` frame gets the URL.
#[test]
fn the_referrer_of_a_file_page() {
    on_page_thread(|| {
        let mut failures = compare_case_at("file:///case/case.html", "referrer-file", DEPARTURES);
        failures.extend(compare_case_at(
            "file:///case/case.html",
            "referrer-about-blank-file",
            DEPARTURES,
        ));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// A frame of another origin that goes to `about:blank` leaves a document of the parent's
/// origin (a departure, see `DEPARTURES`), which the parent can read: its referrer has only
/// the origin of the frame, not its URL.
#[test]
fn an_about_blank_document_gets_no_url_of_another_origin() {
    on_page_thread(|| {
        let top = format!(
            "<!DOCTYPE html><iframe id=t src='{OTHER}/case/t.html?secret=xyz&tok=1#frag'></iframe>"
        );
        let t = "<!DOCTYPE html><script>addEventListener('message', () => { location.href = 'about:blank'; });</script>";
        let (mut page, _sim) = open(&top, &[("/case/t.html", Res::html(t))], true);
        evaluate(
            &mut page,
            "document.getElementById('t').contentWindow.postMessage('go', '*'); \
             new Promise(r => setTimeout(r, 300))",
        );
        finish_loading(&mut page, Duration::from_secs(20));
        assert_eq!(
            evaluate(
                &mut page,
                "document.getElementById('t').contentDocument.referrer"
            ),
            json!(format!("{OTHER}/"))
        );
    });
}

/// The fragment and the credentials of the parent's URL are not in the referrer, and a
/// URL that a script changed counts.
#[test]
fn the_referrer_strips_the_fragment_and_credentials() {
    on_page_thread(|| {
        let (mut page, _sim) = super::open_case("referrer-url");
        for _ in 0..60 {
            if evaluate(&mut page, "location.pathname.endsWith('hop.html')") == json!(true) {
                break;
            }
            evaluate(&mut page, "new Promise(r => setTimeout(r, 100))");
            finish_loading(&mut page, Duration::from_secs(20));
        }
        let failures = compare_queries(&mut page, "referrer-url", DEPARTURES);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// `referrerpolicy` and `Referrer-Policy` are not read (ADR 0031 section 13): the default
/// policy applies. Chromium's values for the other policies are in the probe.
#[test]
fn the_default_policy_is_the_only_one() {
    on_page_thread(|| {
        let failures = compare_case("referrer-policy", DEPARTURES);
        // Only the rows of the default policy (the empty value, `bogus`, and
        // `strict-origin-when-cross-origin`, p0/q0, p7/q7, p9/q9) must match.
        let other: Vec<_> = failures
            .iter()
            .filter(|f| {
                !["p1", "q1", "q2", "q3", "p4", "p5", "p6", "q8"]
                    .iter()
                    .any(|k| f.contains(&format!("\"{k} <-")))
            })
            .collect();
        assert!(other.is_empty(), "{other:?}");
    });
}

/// A page whose `n` frames are of the origin of the top level and count their `storage`
/// events in the top level's `window.count`.
fn page_with_listening_frames(n: usize) -> Page {
    let mut frames = String::new();
    for i in 0..n {
        write!(frames, "<iframe id=f{i} src='/case/x.html'></iframe>").unwrap();
    }
    let html =
        format!("<!DOCTYPE html><title>top</title><script>window.count = 0;</script>{frames}");
    let child =
        "<!DOCTYPE html><script>addEventListener('storage', () => { top.count++; });</script>";
    let (page, _sim) = open(&html, &[("/case/x.html", Res::html(child))], true);
    page
}

/// Lets the page's tasks run for a while of page time.
fn wait_3s(page: &mut Page) {
    evaluate(page, "new Promise(r => setTimeout(r, 3000))");
}

/// Writes in a burst that passes the budget of waiting events: the writes go on, the
/// events stop at the budget, and what waited is given back when it has run, so that the
/// next write fires again. A frame that goes takes its waiting events and their bytes with
/// it.
#[test]
fn the_budget_of_waiting_events_stops_a_burst_and_gives_back() {
    on_page_thread(|| {
        let mut page = page_with_listening_frames(1);
        evaluate(
            &mut page,
            "(() => { for (let i = 0; i < 70000; i++) localStorage.setItem('k', String(i)); return 1; })()",
        );
        wait_3s(&mut page);
        let count = evaluate(&mut page, "count").as_u64().unwrap();
        assert!(count > 0 && count <= 65_536, "{count} events");
        assert_eq!(
            evaluate(&mut page, "localStorage.getItem('k')"),
            json!("69999")
        );
        // Nothing waits now: one more write fires.
        evaluate(&mut page, "localStorage.setItem('k', 'again'); 1");
        wait_3s(&mut page);
        assert_eq!(evaluate(&mut page, "count").as_u64().unwrap(), count + 1);
    });
}

/// The bytes of the waiting events count, with the strings once for each receiver, so a
/// value at the quota stops after a few writes.
#[test]
fn the_bytes_of_waiting_events_count() {
    on_page_thread(|| {
        let mut page = page_with_listening_frames(3);
        evaluate(
            &mut page,
            "(() => { const v = 'x'.repeat((5 << 20) - 8); for (let i = 0; i < 200; i++) { \
             localStorage.setItem('k', v); localStorage.removeItem('k'); } return 1; })()",
        );
        wait_3s(&mut page);
        let count = evaluate(&mut page, "count").as_u64().unwrap();
        // 256 MiB / (3 receivers x about 5 MiB of the strings of one event).
        assert!(count > 0 && count < 3 * 400, "{count} events");
        assert_eq!(evaluate(&mut page, "localStorage.length"), json!(0));
    });
}

/// A document that left its frame gets no event, and its waiting events leave the budget.
#[test]
fn a_removed_frame_takes_its_events_with_it() {
    on_page_thread(|| {
        let mut page = page_with_listening_frames(2);
        evaluate(
            &mut page,
            "(() => { for (let i = 0; i < 50; i++) localStorage.setItem('k', String(i)); \
             f0.remove(); f1.remove(); return 1; })()",
        );
        wait_3s(&mut page);
        assert_eq!(evaluate(&mut page, "count"), json!(0));
    });
}

/// A listener that makes new events for its own document does not hold up the commit that
/// replaces it: the commit fires what waited when it started. The writer here is another
/// frame, whose object the listener uses, and the top level has another origin, so that no
/// third document fills the budget.
#[test]
fn a_listener_that_writes_again_does_not_hold_up_a_commit() {
    on_page_thread(|| {
        const MOST: u64 = 20_000;
        let top = format!(
            "<!DOCTYPE html><iframe id=a src='{OTHER}/case/a.html'></iframe>\
             <iframe id=b src='{OTHER}/case/b.html'></iframe>"
        );
        let a = format!(
            "<!DOCTYPE html><script>addEventListener('storage', () => {{ const b = parent.frames[1]; \
             b.n++; if (b.n < {MOST}) b.localStorage.setItem('z', String(b.n)); }});</script>"
        );
        let (mut page, _sim) = open(
            &top,
            &[
                ("/case/a.html", Res::html(&a)),
                (
                    "/case/b.html",
                    Res::html("<!DOCTYPE html><script>window.n = 0;</script>"),
                ),
                (
                    "/case/c.html",
                    Res::html("<!DOCTYPE html><script>window.c = 1;</script>"),
                ),
            ],
            true,
        );
        evaluate_in(&mut page, "b", "localStorage.setItem('x', '1'); 1");
        evaluate(
            &mut page,
            &format!(
                "document.getElementById('a').src = '{OTHER}/case/c.html'; \
                 new Promise(r => setTimeout(r, 50))"
            ),
        );
        finish_loading(&mut page, Duration::from_secs(20));
        assert_eq!(evaluate_in(&mut page, "a", "window.c"), json!(1));
        let writes = evaluate_in(&mut page, "b", "n").as_u64().unwrap();
        assert!(writes < MOST, "{writes} writes before the commit");
    });
}

/// Listeners that take long hold up a commit for a bounded time, not for each of the
/// events that wait.
#[test]
fn waiting_events_hold_up_a_commit_for_a_bounded_time() {
    on_page_thread(|| {
        const EVENTS: u64 = 40;
        let html = "<!DOCTYPE html><script>window.count = 0;</script>\
                    <iframe id=a src='/case/a.html'></iframe>";
        let a = "<!DOCTYPE html><script>addEventListener('storage', () => { top.count++; \
                 const t = Date.now(); while (Date.now() - t < 60) {} });</script>";
        let (mut page, _sim) = open(html, &[("/case/a.html", Res::html(a))], true);
        evaluate(
            &mut page,
            &format!(
                "for (let i = 0; i < {EVENTS}; i++) localStorage.setItem('k', String(i)); \
                 document.getElementById('a').src = 'about:blank'; 1"
            ),
        );
        finish_loading(&mut page, Duration::from_secs(20));
        evaluate(&mut page, "new Promise(r => setTimeout(r, 50))");
        assert_eq!(
            evaluate(
                &mut page,
                "document.getElementById('a').contentWindow.location.href"
            ),
            json!("about:blank")
        );
        let count = evaluate(&mut page, "count").as_u64().unwrap();
        assert!(count > 0 && count < EVENTS / 2, "{count} events ran");
    });
}

/// A frame that is gone is no receiver: a later burst of writes spends the budget on the
/// frame that is left, not on the two.
#[test]
fn a_removed_frame_is_no_receiver_of_later_writes() {
    on_page_thread(|| {
        let mut page = page_with_listening_frames(2);
        evaluate(&mut page, "f0.remove(); 1");
        evaluate(
            &mut page,
            "(() => { for (let i = 0; i < 70000; i++) localStorage.setItem('k', String(i)); return 1; })()",
        );
        wait_3s(&mut page);
        let count = evaluate(&mut page, "count").as_u64().unwrap();
        // The budget of 65,536 waiting events is for one receiver.
        assert!(count > 50_000 && count <= 65_536, "{count} events");
    });
}

/// A frame under a top level that has no site (a `data:` page) has no key and no storage
/// (the storage is a boundary between sites: it fails closed), and the page itself throws.
#[test]
fn a_frame_under_a_page_without_a_site_has_no_storage() {
    on_page_thread(|| {
        let html = "<iframe id=f srcdoc=\"<script>window.r = (() => { try { return localStorage.length; } catch (e) { return e.name + ': ' + e.message; } })()</script>\"></iframe>";
        let encoded = html
            .replace('%', "%25")
            .replace('"', "%22")
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace('#', "%23");
        let (mut page, _sim) = super::open_at(&format!("data:text/html,{encoded}"), "", &[], true);
        let f = frame(&page, "f");
        assert_eq!(
            evaluate_in(&mut page, "f", "window.r"),
            json!(
                "SecurityError: Failed to read the 'localStorage' property from 'Window': Access is denied for this document."
            )
        );
        assert!(page.frame_has_realm(f));
        assert_eq!(
            evaluate(
                &mut page,
                "(() => { try { return localStorage.length; } catch (e) { return e.message; } })()"
            ),
            json!(
                "Failed to read the 'localStorage' property from 'Window': Storage is disabled inside 'data:' URLs."
            )
        );
    });
}

/// Documents of one key are of one origin, and a page has one runtime per origin: a write
/// reaches every document of its key from one runtime (ADR 0031 section 5). Two frames of
/// another origin, and a sandboxed frame with `allow-same-origin`, share the runtime of
/// their origin.
#[test]
fn documents_of_one_key_share_a_runtime() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><iframe id=a src='/case/x.html'></iframe>\
                    <iframe id=b sandbox='allow-scripts allow-same-origin' src='/case/x.html'></iframe>\
                    <iframe id=c src='/case/x.html'></iframe>";
        let x = format!("{OTHER}/case/x.html");
        let html = html.replace("/case/x.html", &x);
        let (page, _sim) = open(
            &html,
            &[(
                "/case/x.html",
                Res::html("<!DOCTYPE html><script>window.k = 1;</script>"),
            )],
            true,
        );
        let [a, b, c] = ["a", "b", "c"].map(|id| frame(&page, id));
        assert_eq!(page.frames_in_runtime_of(a), Some(3));
        assert_eq!(page.frames_in_runtime_of(b), Some(3));
        assert_eq!(page.frames_in_runtime_of(c), Some(3));
        assert_eq!(page.runtime_count(), 2);
    });
}
