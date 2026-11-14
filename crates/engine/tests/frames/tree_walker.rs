//! `TreeWalker` across the documents of a runtime (M10 feature 9a): every query
//! of the cases of `tools/probes/web-tree-walker-frames.json` with Chromium's
//! values (`tests/data/web-tree-walker-frames.json`: three equal runs of
//! `swbtools probe --json`): the realm of a walker and of the wrappers it returns,
//! filters of another realm and the realm of their errors, a current node in
//! another document, roots and current nodes that move between documents, a walker
//! whose frame leaves the tree or navigates. A departure has a row with its
//! reason.

use super::open;
use crate::common::{evaluate_json as evaluate, on_page_thread};
use serde_json::Value as Json;

/// A query where swb departs on purpose: (case, query index, swb's value, why).
const DEPARTURES: &[(&str, usize, &str, &str)] = &[(
    "frames-filter-errors",
    0,
    r#"[[true,false,"Failed to execute 'nextNode' on 'TreeWalker': Failed to execute 'acceptNode' on 'NodeFilter': The provided callback is not callable."],[false,true,"Failed to execute 'nextNode' on 'TreeWalker': Failed to execute 'acceptNode' on 'NodeFilter': The provided callback is not callable."]]"#,
    "the TypeError for a filter without a callable acceptNode is made in the realm of the native \
     that runs (nextNode of the walker's realm); Chromium makes it in the realm of the filter \
     object (the callback's realm). The engine has no way to make an error in another realm \
     (the same departure as for NodeIterator)",
)];

fn probe() -> Json {
    serde_json::from_str(include_str!(
        "../../../../tools/probes/web-tree-walker-frames.json"
    ))
    .unwrap()
}

fn chromium() -> Json {
    serde_json::from_str(include_str!("../data/web-tree-walker-frames.json")).unwrap()
}

/// Runs the queries of a case in a page with JavaScript on and returns the differences.
fn run_case(name: &str) -> Vec<String> {
    let probe = probe();
    let case = probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no case {name}"));
    let html = case["html"].as_str().unwrap();
    let data = chromium();
    let results = data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no Chromium values for {name}"))["results"]
        .as_array()
        .unwrap()
        .clone();
    let (mut page, _sim) = open(html, &[], true);
    let mut failures = Vec::new();
    for (index, result) in results.iter().enumerate() {
        let source = result["label"].as_str().unwrap();
        let got = evaluate(&mut page, source);
        let want = match DEPARTURES.iter().find(|d| d.0 == name && d.1 == index) {
            Some(d) => serde_json::from_str(d.2).unwrap_or_else(|_| Json::String(d.2.to_owned())),
            None => result["value"].clone(),
        };
        if got != want {
            failures.push(format!(
                "{name} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
            ));
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

case_test!(
    walkers_and_wrappers_have_the_realm_chromium_gives,
    "frames-realms"
);
case_test!(filters_and_errors_of_other_realms, "frames-filters");
case_test!(
    the_error_of_a_filter_without_acceptnode_has_a_realm,
    "frames-filter-errors"
);
case_test!(
    a_current_node_in_a_frame_walks_that_frames_tree,
    "frames-top"
);
case_test!(
    a_root_that_moves_to_another_document_takes_its_walker_along,
    "frames-move-root-a"
);
case_test!(
    a_root_that_moves_after_a_walk_is_followed,
    "frames-move-root-b"
);
case_test!(
    a_current_node_that_moves_to_the_top_level_walks_there,
    "frames-move-node-a"
);
case_test!(
    a_current_node_that_moves_into_a_frame_walks_there,
    "frames-move-node-b"
);
case_test!(
    a_walker_works_on_a_frame_that_left_the_tree,
    "frames-leave-a"
);
case_test!(
    a_walker_of_the_top_level_works_on_a_frame_that_left_the_tree,
    "frames-leave-b"
);
case_test!(
    a_walker_works_on_a_document_the_frame_navigated_away_from,
    "frames-navigate"
);
