//! What the exit tests of the page records share (`videojs.rs`,
//! `osm_zagreb.rs`): the type of a difference, the pace of the steps of a
//! group and the pixel check of a group with the departures.

use serde_json::{Value, json};
use swb_engine::Page;

use super::departures::Departures;
use super::{PIXEL_TOLERANCE, Point, pixel_mismatches};

/// A difference: the field it names and its text.
pub(crate) type Difference = (String, String);

/// The steps of a group with the pause after each step (none if 0), as
/// `swbtools replay` runs them.
pub(crate) fn paced(steps: &[Value], pause_ms: u64) -> Vec<Value> {
    if pause_ms == 0 {
        return steps.to_vec();
    }
    steps
        .iter()
        .flat_map(|step| [step.clone(), json!({ "wait": pause_ms })])
        .collect()
}

/// The pixel differences of a group, without those that a departure
/// accepts.
pub(crate) fn pixel_problems(
    label: &str,
    page: &mut Page,
    points: &[Point],
    pixels: &Value,
    departures: &mut Departures,
) -> Vec<String> {
    pixel_mismatches(label, page, points, pixels, PIXEL_TOLERANCE)
        .into_iter()
        .filter(|(name, _)| !departures.accepts_point(name))
        .map(|(_, message)| message)
        .collect()
}
