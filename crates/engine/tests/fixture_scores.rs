//! Ratchet test: the geometry score of every page fixture must not drop
//! below the value stored in `fixtures/scores.json` (written by
//! `just update-scores`). See docs/testing.md and ADR 0005.

mod common;

use std::sync::Arc;

use swb_engine::{Size, Url};
use swb_net::ReplayFetcher;

const TOLERANCE: f32 = 2.0;
/// The Python tool and this test align differing tag sequences with
/// different diff implementations; allow for small differences.
const SLACK: f64 = 0.002;

#[test]
fn fixture_geometry_scores_do_not_regress() {
    let root = common::repo_root();
    let text = std::fs::read_to_string(root.join("fixtures/scores.json"))
        .expect("fixtures/scores.json must exist (write it with `just update-scores`)");
    let scores: serde_json::Value = serde_json::from_str(&text).expect("valid scores.json");
    let mut failures = Vec::new();
    for (name, stored) in scores.as_object().expect("an object") {
        let dir = root.join("fixtures/pages").join(name);
        let meta: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("fixture.json")).expect("fixture.json"),
        )
        .expect("valid fixture.json");
        let url = Url::parse(meta["url"].as_str().expect("url")).expect("valid URL");
        let fetcher = Arc::new(ReplayFetcher::load(&dir).expect("fixture loads"));
        let actual = common::load_boxes(fetcher, url, Size::new(1280.0, 800.0));
        let reference = common::read_dump(&dir.join("reference/boxes.json"));
        let result = common::compare(&reference, &actual, TOLERANCE);
        let minimum = stored["geometry"].as_f64().unwrap_or(0.0);
        eprintln!(
            "{name}: geometry {:.4} (stored {minimum:.4})",
            result.geometry
        );
        if result.geometry + SLACK < minimum {
            failures.push(format!(
                "{name}: geometry {:.4} < stored {minimum:.4}\n    {}",
                result.geometry,
                result.mismatches.join("\n    ")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "fixture scores regressed:\n{}",
        failures.join("\n")
    );
}
