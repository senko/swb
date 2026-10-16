//! `NormalizationTest.txt` of the Unicode Character Database 17.0.0
//! against `swb_js_text::unicode::normalize`.
//!
//! - `sample`: the committed sample `tests/data/normalization_sample.txt`.
//! - `full_file` (ignored): the whole file, from `$NORMALIZATION_TEST` or
//!   `out/ucd/17.0.0/NormalizationTest.txt` (made by `swbtools
//!   js-unicode-tables`). Run it with `cargo test -p swb-js-text --test
//!   normalization -- --ignored`. It also checks that every code point
//!   that Part 1 does not list stays as it is in all four forms.

use std::collections::HashSet;

use swb_js_text::String16;
use swb_js_text::unicode::normalize::{Form, normalize};

fn units(cps: &[u32]) -> Vec<u16> {
    let mut text = String16::new();
    for &cp in cps {
        text.push_code_point(cp);
    }
    text.as_str16().units().collect()
}

fn run(source: &[u32], form: Form) -> Vec<u16> {
    let input = units(source);
    normalize(swb_js_text::Str16::Wide(&input), form)
        .as_str16()
        .units()
        .collect()
}

fn parse_column(column: &str) -> Vec<u32> {
    column
        .split_whitespace()
        .map(|cp| u32::from_str_radix(cp, 16).expect("a hex code point"))
        .collect()
}

/// Checks the conformance invariants of every line. Returns the number of
/// lines, the failures and the code points of Part 1.
fn check(file: &str) -> (usize, Vec<String>, HashSet<u32>) {
    let mut lines = 0;
    let mut failures = Vec::new();
    let mut part = String::new();
    let mut listed = HashSet::new();
    for line in file.lines() {
        if let Some(name) = line.strip_prefix("@Part") {
            part = name.chars().take(1).collect();
            continue;
        }
        let data = line.split('#').next().unwrap_or("").trim();
        if data.is_empty() {
            continue;
        }
        let columns: Vec<Vec<u32>> = data.split(';').take(5).map(parse_column).collect();
        assert_eq!(columns.len(), 5, "{line}");
        if part == "1" && columns[0].len() == 1 {
            listed.insert(columns[0][0]);
        }
        lines += 1;
        let [c1, c2, c3, c4, c5] = [
            &columns[0],
            &columns[1],
            &columns[2],
            &columns[3],
            &columns[4],
        ];
        for (form, expected) in [
            (Form::Nfc, c2),
            (Form::Nfd, c3),
            (Form::Nfkc, c4),
            (Form::Nfkd, c5),
        ] {
            for input in [c1, c2, c3] {
                if run(input, form) != units(expected) {
                    failures.push(format!("{form:?} of {input:X?}: {line}"));
                }
            }
        }
        // c4 and c5 are the fixed points of the compatibility forms and
        // of NFC and NFD (c4 == NFC(c4) == NFC(c5), c5 == NFD(c4) == NFD(c5)).
        for (form, expected, input) in [
            (Form::Nfc, c4, c4),
            (Form::Nfc, c4, c5),
            (Form::Nfd, c5, c4),
            (Form::Nfd, c5, c5),
            (Form::Nfkc, c4, c4),
            (Form::Nfkc, c4, c5),
            (Form::Nfkd, c5, c4),
            (Form::Nfkd, c5, c5),
        ] {
            if run(input, form) != units(expected) {
                failures.push(format!("{form:?} of {input:X?}: {line}"));
            }
        }
    }
    (lines, failures, listed)
}

#[test]
fn sample() {
    let (lines, failures, _) = check(include_str!("data/normalization_sample.txt"));
    assert!(lines > 700, "{lines} lines");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
}

#[test]
#[ignore = "reads the whole NormalizationTest.txt (see the module comment)"]
fn full_file() {
    let path = std::env::var("NORMALIZATION_TEST").unwrap_or_else(|_| {
        format!(
            "{}/../../out/ucd/17.0.0/NormalizationTest.txt",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let file = std::fs::read_to_string(&path).expect("NormalizationTest.txt");
    let (lines, failures, listed) = check(&file);
    println!("{lines} lines, {} failures", failures.len());
    let mut changed = Vec::new();
    for cp in (0..=0x0010_FFFF_u32).filter(|cp| !listed.contains(cp)) {
        for form in [Form::Nfc, Form::Nfd, Form::Nfkc, Form::Nfkd] {
            if run(&[cp], form) != units(&[cp]) {
                changed.push(format!("{cp:X} {form:?}"));
            }
        }
    }
    println!("{} unlisted code points change", changed.len());
    assert!(lines > 19_000, "{lines} lines");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    assert!(
        changed.is_empty(),
        "{:?}",
        &changed[..changed.len().min(20)]
    );
}
