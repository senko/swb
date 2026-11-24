//! The console check of the exit tests (`osm_zagreb.rs`, `videojs.rs`): every
//! message at the level `error` or `warn` of swb's page appears as often as
//! in Chromium's runs (a range), and a message that Chromium has appears in
//! swb too.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use swb_engine::{ConsoleLevel, Page};

/// The text of a page message as Chromium's replay reports it: for an
/// uncaught exception the message without swb's prefix (`a promise:
/// Uncaught (in promise) TypeError: `) and without the name of an error
/// class.
pub(crate) fn page_message(text: &str) -> String {
    let Some((_, rest)) = text.split_once("Uncaught ") else {
        return text.to_owned();
    };
    let rest = rest.strip_prefix("(in promise) ").unwrap_or(rest);
    match rest.split_once(": ") {
        Some((name, message)) if name.ends_with("Error") && !name.contains(' ') => {
            message.to_owned()
        }
        _ => rest.to_owned(),
    }
}

/// The differences of the console: a message (its text without the parts in
/// `strip`) that swb's page has more or fewer times than the range of
/// Chromium's runs (the file `chromium`: `swbtools` `console_counts`), as the
/// text of the message and a description. A message that starts with one of
/// `optional` may be missing in swb (its lower bound is 0).
pub(crate) fn differences(
    page: &Page,
    chromium: &str,
    strip: &[&str],
    optional: &[&str],
) -> Vec<(String, String)> {
    let entries: Vec<Value> = serde_json::from_str(chromium).unwrap();
    let mut want: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for entry in &entries {
        let mut text = entry["text"].as_str().unwrap().to_owned();
        for part in strip {
            text = text.replace(part, "");
        }
        if text.starts_with("Failed to load resource") {
            continue;
        }
        let range = want.entry(text).or_default();
        range.0 += entry["min"].as_u64().unwrap();
        range.1 += entry["max"].as_u64().unwrap();
    }
    let mut got: BTreeMap<String, u64> = BTreeMap::new();
    for message in page.console_messages() {
        if matches!(message.level, ConsoleLevel::Error | ConsoleLevel::Warn) {
            *got.entry(page_message(&message.text)).or_default() += 1;
        }
    }
    let texts: BTreeSet<&String> = got.keys().chain(want.keys()).collect();
    texts
        .into_iter()
        .filter_map(|text| {
            let n = got.get(text).copied().unwrap_or(0);
            let (mut low, high) = want.get(text).copied().unwrap_or((0, 0));
            if optional.iter().any(|prefix| text.starts_with(prefix)) {
                low = 0;
            }
            if (low..=high).contains(&n) {
                return None;
            }
            let who = if n == 0 {
                "swb lacks it"
            } else {
                "count differs"
            };
            Some((
                text.clone(),
                format!("console ({who}): swb {n} times, Chromium {low} to {high} times: {text}"),
            ))
        })
        .collect()
}

/// [`differences`] as the texts of the problems.
pub(crate) fn problems(
    page: &Page,
    chromium: &str,
    strip: &[&str],
    optional: &[&str],
) -> Vec<String> {
    differences(page, chromium, strip, optional)
        .into_iter()
        .map(|(_, message)| message)
        .collect()
}
