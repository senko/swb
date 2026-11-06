//! Nodes across the documents of a runtime, the caller's document and the window
//! of an error, against Chromium (M9 feature 12b2, ADR 0031 "Done (feature
//! 12b2)"): every query of the cases `nodes-*`, `caller-*` and `errors-*` of
//! `tools/probes/web-frames.json` with Chromium's values. A case fills an object
//! of named rows; a departure has a row with its reason.

use serde_json::Value as Json;
use swb_engine::{Evaluation, JsonForm, Page};

use super::{OTHER, Res, case_html, case_resource, open, probe_file, results};
use crate::common::on_page_thread;

/// A row where swb departs on purpose: (case, path of the value, why).
const DEPARTURES: &[(&str, &str, &str)] = &[
    (
        "nodes-other-arguments",
        ".io",
        "IntersectionObserver.observe of an element of another document: a TypeError in swb \
         (ADR 0031, table of node arguments; an observer's targets are per document)",
    ),
    ("nodes-other-arguments", ".ro", "as above: ResizeObserver"),
    (
        "nodes-other-arguments",
        ".ioUnobserve",
        "as above: unobserve",
    ),
    (
        "nodes-attr",
        ".detachedAcross[3]",
        "Attr.ownerDocument: an Attr is no Node in swb (ADR 0028 section 3); the rows of this \
         member are all of this reason",
    ),
    ("nodes-attr", ".detachedFromFrame[3]", "as above"),
    ("nodes-attr", ".moved[1]", "as above"),
    ("nodes-attr", ".removeAfterMove[3]", "as above"),
    ("nodes-attr", ".frameAttrMoved[0]", "as above"),
    ("nodes-attr", ".removedAttrStays[0]", "as above"),
    ("nodes-attr", ".replace[4]", "as above"),
    (
        "caller-javascript-links",
        ".docAfter[0]",
        "the string result of a javascript: URL does not replace the document (ADR 0028 \
         \"Feature 6d2 notes\")",
    ),
    ("caller-javascript-links", ".docAfter[1]", "as above"),
    (
        "nodes-reads-across",
        ".observers",
        "IntersectionObserver with a root of another document: a TypeError in swb",
    ),
    (
        "nodes-mutation-observers",
        ".m4Before[0][7]",
        "a MutationRecord of an observer of the frame's realm that the parent reads: of the \
         observer's realm in swb, Chromium gives the parent's",
    ),
    ("nodes-mutation-observers", ".m4Before[0][8]", "as above"),
    ("nodes-mutation-observers", ".m4After[0][7]", "as above"),
    ("nodes-mutation-observers", ".m4After[0][8]", "as above"),
];

/// The differences of two values, by path.
fn differences(path: &str, swb: &Json, chromium: &Json, out: &mut Vec<String>) {
    match (swb, chromium) {
        (Json::Array(a), Json::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                differences(&format!("{path}[{index}]"), x, y, out);
            }
        }
        (Json::Object(a), Json::Object(b)) => {
            for key in b.keys().chain(a.keys().filter(|k| !b.contains_key(*k))) {
                differences(
                    &format!("{path}.{key}"),
                    a.get(key).unwrap_or(&Json::Null),
                    b.get(key).unwrap_or(&Json::Null),
                    out,
                );
            }
        }
        _ if swb == chromium => {}
        _ => out.push(format!("{path}: swb {swb}, Chromium {chromium}")),
    }
}

/// True if the row `path` of the case `name` is a departure: the row itself or
/// a row around it.
fn departs(name: &str, path: &str) -> bool {
    DEPARTURES.iter().any(|&(case, row, _)| {
        case == name && (path == row || path.starts_with(&format!("{row}[")))
    })
}

/// Opens a case with JavaScript on and compares its query.
fn run_case(name: &str) -> Vec<String> {
    let probe = probe_file();
    let case = probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap();
    let owned: Vec<(String, Res)> = case["resources"]
        .as_object()
        .unwrap()
        .keys()
        .map(|path| (format!("/case/{path}"), case_resource(name, path)))
        .collect();
    let list: Vec<(&str, Res)> = owned.iter().map(|(p, r)| (p.as_str(), r.clone())).collect();
    let html = case_html(name)
        .replace("$OTHER", OTHER)
        .replace("$FOLDER", "case");
    let (mut page, _sim) = open(&html, &list, true);
    let mut failures = Vec::new();
    for result in results(name).iter().filter(|r| r["kind"] == "js") {
        let source = result["label"].as_str().unwrap();
        let swb = in_order(evaluate(&mut page, source));
        let want = in_order(result["value"].clone());
        let mut found = Vec::new();
        differences("", &swb, &want, &mut found);
        for difference in found {
            let path = difference.split(": ").next().unwrap_or("");
            if !departs(name, path) {
                failures.push(format!("{name} {difference}"));
            }
        }
    }
    failures
}

/// The rows whose order depends on the timing of the rendering update and of
/// the two windows' tasks are sorted: their values are compared, not their
/// order (the runs of Chromium differ in it).
fn in_order(mut value: Json) -> Json {
    for key in ["log", "unh", "ran"] {
        if let Some(Json::Array(items)) = value.get_mut(key) {
            items.sort_by_key(Json::to_string);
        }
    }
    value
}

fn evaluate(page: &mut Page, source: &str) -> Json {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap_or(Json::String(text)),
        Some(Evaluation::Error(text)) => Json::String(format!("error: {text}")),
        None => Json::String("no evaluation".to_owned()),
    }
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

case_test!(a_moved_wrapper_stays_the_same_object, "nodes-move-wrapper");
case_test!(objects_of_a_moved_node_have_its_realm, "nodes-move-realms");
case_test!(every_insertion_moves_across, "nodes-insertions");
case_test!(checks_of_insertions_across, "nodes-insertion-errors");
case_test!(adopt_and_import_across, "nodes-adopt-import");
case_test!(reads_across, "nodes-reads-across");
case_test!(other_arguments_across, "nodes-other-arguments");
case_test!(mutation_observers_follow_nodes, "nodes-mutation-observers");
case_test!(live_state_follows_nodes, "nodes-live-state");
case_test!(focus_of_a_moved_node, "nodes-focus");
case_test!(frames_in_a_moved_subtree, "nodes-frames-in-subtree");
case_test!(scripts_in_a_moved_subtree, "nodes-scripts-in-subtree");
case_test!(attr_arguments_across, "nodes-attr");
case_test!(retired_documents_take_and_give_nodes, "nodes-retired");
case_test!(a_sandbox_stops_a_moved_script, "nodes-sandbox-script");
case_test!(a_script_moves_the_focus_of_the_page, "focus-by-script");
case_test!(
    a_script_moves_the_focus_through_nested_frames,
    "focus-by-script-nested"
);
case_test!(errors_go_to_the_window_of_the_function, "errors-callbacks");
case_test!(
    rejections_go_to_the_window_chromium_names,
    "errors-promises"
);
case_test!(urls_through_another_window, "caller-urls");
case_test!(messages_name_the_caller, "caller-messages");
case_test!(javascript_urls_with_a_target, "caller-javascript-links");
case_test!(
    an_element_takes_the_focus_from_its_iframe,
    "focus-iframe-then-inner"
);
case_test!(rejections_come_grouped_by_window, "errors-promises-grouped");

/// A move that does not fit the target's arena ends the task, and the subtree stays
/// whole in its document: at the first check, and when a `blur` listener in the steps
/// of the removal fills the arena after that check.
#[test]
fn a_move_that_does_not_fit_leaves_the_subtree_whole() {
    const WHOLE: &str = "[n.nodeName, n.childNodes.length, n.firstChild.nodeName, \
        n.lastChild.textContent, n.querySelector('template').content.firstChild.textContent, \
        n.ownerDocument === d]";
    on_page_thread(|| {
        let (mut page, _sim) = open(
            "<!DOCTYPE html><iframe id=f srcdoc=\"<div id=n><input id=i><template><p>t</p>\
             </template><b>bold</b></div>\"></iframe><div id=tgt></div>",
            &[],
            true,
        );
        evaluate(
            &mut page,
            "window.d = f.contentDocument; window.n = d.getElementById('n'); \
             window.i = d.getElementById('i'); i.focus(); \
             i.addEventListener('blur', () => { for (let k = 0; k < 35; k++) \
             document.createElement('s'); }); 1",
        );
        let want = serde_json::json!(["DIV", 3, "INPUT", "bold", "t", true]);
        assert_eq!(evaluate(&mut page, WHOLE), want);
        // Room for the subtree at the first check; the listener leaves too little.
        let dom = page.document().unwrap();
        let fill = dom.limits().max_nodes - dom.len() - 40;
        evaluate(
            &mut page,
            &format!("for (let k = 0; k < {fill}; k++) document.createElement('s'); 1"),
        );
        evaluate(&mut page, "tgt.appendChild(n); 1");
        assert_eq!(evaluate(&mut page, WHOLE), want);
        assert_eq!(evaluate(&mut page, "n.parentNode"), Json::Null);
        assert_eq!(evaluate(&mut page, "tgt.childNodes.length"), 0);
        // In its place again, and now the first check refuses.
        assert_eq!(
            evaluate(&mut page, "d.body.appendChild(n); n.parentNode === d.body"),
            true
        );
        evaluate(&mut page, "tgt.appendChild(n); 1");
        assert_eq!(evaluate(&mut page, WHOLE), want);
        assert_eq!(evaluate(&mut page, "n.parentNode === d.body"), true);
        assert_eq!(evaluate(&mut page, "d.getElementById('n') === n"), true);
    });
}
