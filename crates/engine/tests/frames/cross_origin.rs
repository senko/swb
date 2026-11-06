//! The cross-origin `Window` and `Location` against Chromium (M9 feature 13a,
//! ADR 0031 section 5): every query of the cases `cross-origin`, `nested-origins`
//! and `xo-*` of `tools/probes/web-frames.json` with Chromium's values. A departure
//! has a row with its reason.

use serde_json::Value as Json;
use swb_engine::{Evaluation, JsonForm, Page};

use super::scripted::compare_case;
use super::{OTHER, Res, open};
use crate::common::on_page_thread;

/// A row where swb departs on purpose: (case, query, path of the value, why).
/// An empty path is the whole value of the query.
const DEPARTURES: &[(&str, usize, &str, &str)] = &[
    (
        "cross-origin",
        2,
        "[6]",
        "document.referrer of a frame document: feature 14b",
    ),
    (
        "cross-origin",
        2,
        "[8]",
        "Location lacks ancestorOrigins (ADR 0028 section 13)",
    ),
    (
        "xo-focus",
        0,
        "[16]",
        "f.focus() when the frame holds the focus: Chromium's top window gets focus and blur \
         first; swb has no change of the holder",
    ),
    (
        "xo-focus",
        1,
        "",
        "the frame's log: as the row above, that f.focus() gives the frame no further pair of \
         events in swb",
    ),
    (
        "xo-aba",
        0,
        "[27]",
        "document.referrer of a frame document: feature 14",
    ),
    (
        "xo-children",
        2,
        "[1]",
        "a frame that a script renames is found by its new name in swb; Chromium, through a \
         cross-origin parent, finds it by no name",
    ),
    ("xo-rename", 0, "[1]", "as in xo-children"),
    ("xo-rename", 0, "[3]", "as in xo-children"),
    (
        "xo-descriptors",
        5,
        "[43]",
        "null[w]: swb names the type of the key in the TypeError; Chromium names no key",
    ),
    (
        "xo-functions",
        0,
        "[43]",
        "DOMException has no `stack` in swb; not a matter of frames",
    ),
    (
        "xo-functions",
        1,
        "[81]",
        "new Event('x', w): the text of the dictionary member is not in the message of swb; the \
         error and its name are",
    ),
    ("xo-location-this", 1, "[30]", "as in xo-functions"),
    (
        "xo-keys-more",
        0,
        "[63]",
        "console.dir does not exist in swb; not a matter of frames",
    ),
    (
        "xo-keys-more",
        0,
        "[64]",
        "console.table does not exist in swb; not a matter of frames",
    ),
    (
        "xo-navigate",
        1,
        "[10]",
        "the own names of a Location of a same-origin document: swb lacks `ancestorOrigins` \
         and `valueOf` (ADR 0028 section 13)",
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

case_test!(the_probe_of_a_cross_origin_window, "cross-origin");
case_test!(nested_origins, "nested-origins");
case_test!(descriptors_and_internal_methods, "xo-descriptors");
case_test!(functions_are_of_the_callers_realm, "xo-functions");
case_test!(post_message_checks_its_arguments, "xo-post-message");
case_test!(the_cross_origin_location, "xo-location");
case_test!(navigations_through_a_location, "xo-navigate");
case_test!(a_javascript_url_is_refused, "xo-javascript-url");
case_test!(who_may_navigate, "xo-navigate-rules");
case_test!(children_through_a_cross_origin_window, "xo-children");
case_test!(a_b_a_prime, "xo-aba");
case_test!(focus_of_a_cross_origin_window, "xo-focus");
case_test!(close_does_nothing, "xo-close");
case_test!(a_frame_that_is_gone, "xo-destroyed");
case_test!(one_site_across_a_change_of_target, "xo-stale-sites");
case_test!(this_of_the_location_functions, "xo-location-this");
case_test!(keys_and_symbols, "xo-keys-more");
case_test!(a_kept_location, "xo-kept-location");
case_test!(a_frame_that_changes_its_name, "xo-rename");
case_test!(more_of_a_frame_that_is_gone, "xo-gone-more");
case_test!(
    a_write_to_location_has_the_flags_of_its_moment,
    "xo-navigate-flags-at-the-write"
);
case_test!(a_promise_native_throws_too, "xo-promise-natives");
case_test!(
    the_last_write_and_a_sandboxed_parent_give_the_flags,
    "xo-navigate-flags-more"
);
case_test!(
    promise_natives_of_other_interfaces,
    "xo-promise-natives-others"
);

fn evaluate(page: &mut Page, source: &str) -> Json {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap_or(Json::String(text)),
        other => panic!("{other:?}"),
    }
}

/// A frame past the budget of realms (ADR 0031 section 11) keeps its proxy without
/// a target although its document has this origin, so the rules of another origin
/// apply, and `O` is the origin of the frame itself. A departure that Chromium
/// cannot show, because it has no such budget: its row is this test.
#[test]
fn a_frame_past_the_budget_of_realms_has_the_rules_of_another_origin() {
    on_page_thread(|| {
        let (mut page, _sim) = open("<!DOCTYPE html><title>t</title>", &[], true);
        let value = evaluate(
            &mut page,
            "(() => { const E = g => { try { return String(g()); } catch (e) { return e.name + ': ' + e.message.split(location.origin).join('O'); } }; \
             let last; for (let i = 0; i < 300; i++) { last = document.createElement('iframe'); document.body.append(last); last.contentWindow; } \
             const w = last.contentWindow; \
             return [E(() => w.document), E(() => last.contentDocument), E(() => w.closed), E(() => w.length), E(() => w.parent === window), E(() => typeof w.focus), E(() => w.location.href), E(() => Object.getPrototypeOf(w))]; })()",
        );
        assert_eq!(
            value,
            serde_json::json!([
                "SecurityError: Failed to read a named property 'document' from 'Window': Blocked a frame with origin \"O\" from accessing a cross-origin frame.",
                "null",
                "false",
                "0",
                "true",
                "function",
                "SecurityError: Failed to read a named property 'href' from 'Location': Blocked a frame with origin \"O\" from accessing a cross-origin frame.",
                "null"
            ])
        );
    });
}

/// A navigation through a `Location` goes through the page after the task; a later
/// request for the frame replaces an earlier one, so a loop costs one request.
#[test]
fn a_loop_of_writes_to_a_location_makes_one_navigation() {
    on_page_thread(|| {
        let other = format!("{OTHER}/case/a.html");
        let html = format!("<!DOCTYPE html><iframe id=f src='{other}'></iframe>");
        let (mut page, sim) = open(
            &html,
            &[
                ("/case/a.html", Res::html("<!DOCTYPE html><title>a</title>")),
                ("/case/b.html", Res::html("<!DOCTYPE html><title>b</title>")),
            ],
            true,
        );
        let f = super::frame(&page, "f");
        let before = sim.requests().len();
        let value = evaluate(
            &mut page,
            &format!(
                "new Promise(r => {{ f.onload = () => r('done'); const l = f.contentWindow.location; for (let i = 0; i < 1000; i++) l.href = '{OTHER}/case/a.html?' + i; l.href = '{OTHER}/case/b.html'; }})"
            ),
        );
        assert_eq!(value, "done");
        let requests = sim.requests();
        assert_eq!(requests.len(), before + 1, "{requests:?}");
        assert_eq!(super::frame_url(&page, f), format!("{OTHER}/case/b.html"));
    });
}

/// `Location` kept across a collection: the proxy keeps it alive.
#[test]
fn the_location_lives_as_long_as_the_proxy() {
    on_page_thread(|| {
        let other = format!("{OTHER}/case/a.html");
        let html = format!("<!DOCTYPE html><iframe id=f src='{other}'></iframe>");
        let (mut page, _sim) = open(
            &html,
            &[("/case/a.html", Res::html("<!DOCTYPE html><title>a</title>"))],
            true,
        );
        let value = evaluate(
            &mut page,
            "(() => { const w = f.contentWindow; const seen = new WeakSet([w.location]); \
             const focus = w.focus; let junk = []; \
             for (let i = 0; i < 300000; i++) { junk.push({i}); if (junk.length > 1000) junk = []; } \
             return [seen.has(w.location), focus === w.focus]; })()",
        );
        assert_eq!(value, serde_json::json!([true, true]));
    });
}
