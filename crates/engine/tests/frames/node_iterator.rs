//! `NodeIterator` across the documents of a runtime (M9 feature 15a, ADR 0028
//! section 17): every query of the cases of `tools/probes/web-node-iterator-frames.json`
//! with Chromium's values (`tests/data/web-node-iterator-frames.json`: three equal
//! runs of `swbtools probe --json`): the realm of an iterator and of the wrappers
//! it returns, filters of another realm and the realm of their errors, a removal
//! in a frame seen by an iterator of the top level, roots and nodes that move
//! between documents with the iterator live, an iterator whose frame leaves the
//! tree or navigates, and a filter that removes or navigates its own frame. A
//! departure has a row with its reason.

use super::open;
use crate::common::{evaluate_json as evaluate, on_page_thread};
use serde_json::Value as Json;

/// A query where swb departs on purpose: (case, query index, swb's value, why).
const DEPARTURES: &[(&str, usize, &str, &str)] = &[(
    "frames-filter-errors",
    0,
    r#"["TypeError,false,true,false,false", "TypeError,true,false,false,false", "TypeError,true,false,false,false"]"#,
    "the TypeError for a filter without a callable acceptNode is made in the realm of the native \
     that runs (nextNode of the iterator's realm); Chromium makes it in the realm of the filter \
     object (the callback's realm). The engine has no way to make an error in another realm",
)];

fn probe() -> Json {
    serde_json::from_str(include_str!(
        "../../../../tools/probes/web-node-iterator-frames.json"
    ))
    .unwrap()
}

fn chromium() -> Json {
    serde_json::from_str(include_str!("../data/web-node-iterator-frames.json")).unwrap()
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
    iterators_and_wrappers_have_the_realm_chromium_gives,
    "frames-realms"
);
case_test!(filters_and_errors_of_other_realms, "frames-filters");
case_test!(
    the_error_of_a_filter_without_acceptnode_has_a_realm,
    "frames-filter-errors"
);
case_test!(
    a_removal_in_a_frame_moves_an_iterator_of_the_top_level,
    "frames-top"
);
case_test!(
    a_root_that_moves_to_a_frame_takes_the_iterator,
    "frames-move-root"
);
case_test!(
    a_node_that_moves_between_documents_moves_the_iterators_in_it,
    "frames-move-node"
);
case_test!(
    an_iterator_works_on_a_frame_that_left_the_tree,
    "frames-leave"
);
case_test!(
    an_iterator_works_on_a_document_that_the_frame_navigated_away_from,
    "frames-navigate-a"
);
case_test!(
    a_filter_that_removes_its_frame_ends_the_walk_cleanly,
    "frames-navigate-b"
);
case_test!(
    a_filter_that_navigates_its_frame_ends_the_walk_cleanly,
    "frames-navigate-c"
);
case_test!(
    a_reference_that_moves_to_another_document_stays_the_reference,
    "frames-ref-moves"
);
case_test!(
    a_candidate_that_moves_to_another_document_is_accepted_there,
    "frames-candidate-moves"
);
case_test!(
    a_filter_that_moves_the_root_does_not_lose_the_accepted_node,
    "frames-root-moves"
);
case_test!(
    a_filter_that_moves_the_root_goes_on_in_the_new_document,
    "frames-root-moves-2"
);
case_test!(
    the_pointers_outside_a_root_that_moves_stay_where_they_are,
    "frames-root-moves-3"
);
