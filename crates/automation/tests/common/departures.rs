//! The differences from Chromium that an exit test accepts, each with its
//! cause (`osm_zagreb.rs`, `videojs.rs`).

use std::collections::BTreeSet;

use serde_json::Value;

/// The differences from Chromium that a sequence accepts, as the file of the
/// sequence lists them: a list of `{"cause": TEXT, "groups": [NAMES],
/// "fields": [PREFIXES], "points": [NAMES]}`. A difference of a record is
/// named `group: FIELD` (a field of the record such as `sidebar`,
/// `rects.panel[.overlay-layers label #2]` or `outlines.d[0]`); an entry
/// accepts it if the group is in `groups` (an empty list: every group) and the
/// name starts with one of the `fields`. A pixel point is accepted if its
/// name is in `points`.
///
/// Each use is counted: a departure whose group or point no longer differs
/// is a problem, so the list stays exact.
pub(crate) struct Departures {
    list: Vec<Departure>,
    used: BTreeSet<(usize, String)>,
}

struct Departure {
    cause: String,
    groups: Vec<String>,
    fields: Vec<String>,
    points: Vec<String>,
}

impl Departures {
    /// The departures of the text of a departures file.
    pub(crate) fn load(text: &str) -> Self {
        Departures::load_for(text, None)
    }

    /// The departures of the file that have no `"sequence"` key or the value
    /// `sequence` (a file that serves several tests of one page).
    pub(crate) fn load_for(text: &str, sequence: Option<&str>) -> Self {
        let entries: Vec<Value> = serde_json::from_str(text).unwrap();
        let strings = |entry: &Value, key: &str| -> Vec<String> {
            entry[key]
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|s| s.as_str().unwrap().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        let list = entries
            .iter()
            .filter(|entry| match (sequence, entry["sequence"].as_str()) {
                (Some(want), Some(have)) => want == have,
                _ => true,
            })
            .map(|entry| Departure {
                cause: entry["cause"].as_str().unwrap().to_owned(),
                groups: strings(entry, "groups"),
                fields: strings(entry, "fields"),
                points: strings(entry, "points"),
            })
            .collect();
        Departures {
            list,
            used: BTreeSet::new(),
        }
    }

    /// True if a departure accepts the difference `field` of `group`.
    pub(crate) fn accepts_field(&mut self, group: &str, field: &str) -> bool {
        let found = self.list.iter().position(|d| {
            (d.groups.is_empty() || d.groups.iter().any(|g| g == group))
                && d.fields.iter().any(|prefix| field.starts_with(prefix))
        });
        if let Some(at) = found {
            self.used.insert((at, group.to_owned()));
        }
        found.is_some()
    }

    /// True if a departure accepts the pixel point `name`.
    pub(crate) fn accepts_point(&mut self, name: &str) -> bool {
        let found = self
            .list
            .iter()
            .position(|d| d.points.iter().any(|p| p == name));
        if let Some(at) = found {
            self.used.insert((at, name.to_owned()));
        }
        found.is_some()
    }

    /// The departures (or their groups and points) that no difference used.
    pub(crate) fn unused(&self) -> Vec<String> {
        let mut found = Vec::new();
        for (at, d) in self.list.iter().enumerate() {
            let seen = |key: &String| self.used.contains(&(at, key.clone()));
            if !d.fields.is_empty() {
                let stale: Vec<&String> = if d.groups.is_empty() {
                    let any = self.used.iter().any(|(i, _)| *i == at);
                    if any {
                        vec![]
                    } else {
                        d.fields.iter().collect()
                    }
                } else {
                    d.groups.iter().filter(|g| !seen(g)).collect()
                };
                if !stale.is_empty() {
                    found.push(format!(
                        "departure no longer needed ({}): {stale:?} of fields {:?}",
                        d.cause, d.fields
                    ));
                }
            }
            let stale: Vec<&String> = d.points.iter().filter(|p| !seen(p)).collect();
            if !stale.is_empty() {
                found.push(format!(
                    "departure no longer needed ({}): points {stale:?}",
                    d.cause
                ));
            }
        }
        found
    }
}
