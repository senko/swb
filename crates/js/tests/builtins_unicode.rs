//! `String.prototype` case mapping, `normalize` and `localeCompare` (M7
//! feature 5d). The data files are made by `data/gen-unicode-cases.js` from
//! the output of `just probe tools/probes/js-unicode.json` (Chromium 148).
//!
//! - `chromium_cases`: 134 snippets, normally and in the GC stress mode.
//! - `case_mapping_of_every_code_point`, `normalization_of_every_code_point`:
//!   the results for all 1,114,112 code points. There are no differences
//!   from Chromium, which uses Unicode 17.0 as the tables do.
//! - `collation_order_agrees_with_chromium`: the order of 225 words.
//! - The other cases are scripts with their expected output by the
//!   specification.

mod common;

use common::{check_cases, run};

fn lines_differ(actual: &str, expected: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for (actual, expected) in actual.lines().zip(expected.lines()) {
        if actual != expected {
            failures.push(format!("expected: {expected}\nactual:   {actual}"));
        }
    }
    assert_eq!(actual.lines().count(), expected.lines().count());
    failures
}

#[test]
fn chromium_cases() {
    let script = include_str!("data/unicode_cases.js");
    let expected = include_str!("data/unicode_cases.expected");
    for stress in [false, true] {
        let failures = lines_differ(&run(script, stress), expected);
        assert!(
            failures.is_empty(),
            "stress: {stress}\n{}",
            failures.join("\n")
        );
    }
}

#[test]
fn case_mapping_of_every_code_point() {
    let actual = run(include_str!("data/unicode_case_all.js"), false);
    let failures = lines_differ(&actual, include_str!("data/unicode_case_all.expected"));
    assert!(
        failures.is_empty(),
        "{}",
        failures[..failures.len().min(20)].join("\n")
    );
}

#[test]
fn normalization_of_every_code_point() {
    let actual = run(include_str!("data/unicode_normalize_all.js"), false);
    let digest = include_str!("data/unicode_normalize_all.digest");
    let (count, hash) = digest.trim().split_once(' ').expect("count and hash");
    assert_eq!(actual.lines().count().to_string(), count);
    assert_eq!(format!("{:016x}", fnv1a(&actual)), hash);
}

/// The FNV-1a hash (64 bits) of the lines, each followed by a line feed.
fn fnv1a(lines: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in lines.bytes().chain(std::iter::once(b'\n')) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[test]
fn collation_order_agrees_with_chromium() {
    // All 25,200 pairs of 225 words in Chromium's order: 16 are in the
    // other order in swb (all U+0402, which ICU sorts after U+0434), none
    // compares as equal.
    let actual = run(include_str!("data/unicode_collation.js"), false);
    assert_eq!(actual, "25200 16 0");
}

#[test]
fn case_mapping_cases() {
    check_cases(&[
        (
            "print('ABC'.toLowerCase(), 'abc'.toUpperCase(), 'ß'.toUpperCase())",
            "abc ABC SS",
        ),
        // The same string comes back when nothing changes.
        (
            "var s = 'abc' + 1; var t = s.toLowerCase(); print(t === s, Object.is(t, s), 'ABC'.toLowerCase() === 'abc')",
            "true true true",
        ),
        (
            "var a = 'x'.repeat(5000) + '\\u00e9'; print(a.toLowerCase() === a, a.toUpperCase().length, a.toUpperCase().slice(-1))",
            "true 5001 \u{c9}",
        ),
        (
            "var a = ('A\\u03a3'.repeat(20000)); var b = a.toLowerCase(); print(b.length, b.slice(0, 4) === 'a\\u03c3a\\u03c3', b.slice(-2) === 'a\\u03c2')",
            "40000 true true",
        ),
    ]);
}
