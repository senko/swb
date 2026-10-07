//! Compares `line_breaks` with the measurements of Chromium in
//! `tests/linebreak/` (written by `swbtools linebreaks`; the format is in
//! `tests/linebreak/README.md`).
//!
//! Every difference must be listed in `tests/linebreak/known-differences.txt`
//! with a reason, and every listed difference must still exist, so that the
//! list stays current.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use swb_text::{BreakKind, BreakRules, WordBreak, line_breaks};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/linebreak")
}

fn read(name: &str) -> String {
    let path = data_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The rules of a mode directive (`word-break=... hyphens=...`).
fn rules(mode: &str) -> BreakRules {
    let mut rules = BreakRules::default();
    for field in mode.split_whitespace() {
        match field {
            "word-break=normal" => rules.word_break = WordBreak::Normal,
            "word-break=break-all" => rules.word_break = WordBreak::BreakAll,
            "word-break=keep-all" => rules.word_break = WordBreak::KeepAll,
            "hyphens=manual" => rules.soft_hyphens = true,
            "hyphens=none" => rules.soft_hyphens = false,
            other => panic!("unknown mode field {other}"),
        }
    }
    rules
}

fn code_point(hex: &str) -> char {
    u32::from_str_radix(hex, 16)
        .ok()
        .and_then(char::from_u32)
        .unwrap_or_else(|| panic!("bad code point {hex}"))
}

/// The opportunity before each character of `text` (index 1 onward), by
/// code point.
fn swb_marks(text: &[char], rules: BreakRules) -> Vec<Option<BreakKind>> {
    let string: String = text.iter().collect();
    let found: BTreeMap<usize, BreakKind> = line_breaks(&string, |_| rules).into_iter().collect();
    let mut offset = 0;
    let mut marks = Vec::new();
    for (k, c) in text.iter().enumerate() {
        if k > 0 {
            marks.push(found.get(&offset).copied());
        }
        offset += c.len_utf8();
    }
    marks
}

fn hex(text: &[char]) -> String {
    text.iter()
        .map(|c| format!("{:04X}", u32::from(*c)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The differences in `cases.txt`, one key per string.
fn case_differences(out: &mut BTreeSet<String>, total: &mut usize) {
    let mut mode = String::new();
    for line in read("cases.txt").lines() {
        if let Some(m) = line.strip_prefix("@mode ") {
            m.clone_into(&mut mode);
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut text = Vec::new();
        let mut expected = Vec::new();
        for token in line.split_whitespace() {
            match token {
                "÷" => expected.push(Some(Some(BreakKind::Allowed))),
                "!" => expected.push(Some(Some(BreakKind::Mandatory))),
                "×" => expected.push(Some(None)),
                "?" => expected.push(None),
                hexa => text.push(code_point(hexa)),
            }
        }
        assert_eq!(expected.len() + 1, text.len(), "bad case line: {line}");
        *total += 1;
        let got = swb_marks(&text, rules(&mode));
        let differs = got
            .iter()
            .zip(&expected)
            .any(|(g, e)| e.is_some_and(|e| e != *g));
        if differs {
            out.insert(format!("cases {mode} | {line}"));
        }
    }
}

/// The differences in a matrix file, one key per cell.
fn matrix_differences(name: &str, out: &mut BTreeSet<String>, total: &mut usize) {
    let mut header = String::new();
    let mut mode = String::new();
    let mut context: Vec<char> = Vec::new();
    let mut chars: Vec<char> = Vec::new();
    let mut row = 0;
    for line in read(name).lines() {
        if let Some(h) = line.strip_prefix("@matrix ") {
            h.clone_into(&mut header);
            context = h
                .split_whitespace()
                .filter_map(|f| f.strip_prefix("left=").or_else(|| f.strip_prefix("right=")))
                .map(code_point)
                .collect();
            mode = h
                .split_whitespace()
                .filter(|f| f.starts_with("word-break=") || f.starts_with("hyphens="))
                .collect::<Vec<_>>()
                .join(" ");
            row = 0;
            continue;
        }
        if let Some(list) = line.strip_prefix("@chars ") {
            chars = list.split_whitespace().map(code_point).collect();
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        assert_eq!(
            line.chars().count(),
            chars.len(),
            "bad row in {name}: {header}"
        );
        let rules = rules(&mode);
        let x = chars[row];
        for (column, symbol) in line.chars().enumerate() {
            let expected = match symbol {
                '/' => true,
                '.' => false,
                _ => continue,
            };
            let y = chars[column];
            let text = [context[0], x, y, context[1]];
            *total += 1;
            if swb_marks(&text, rules)[1].is_some() != expected {
                out.insert(format!("{name} {header} | {} {}", hex(&[x]), hex(&[y])));
            }
        }
        row += 1;
    }
}

/// The listed differences (the lines without comments).
fn known() -> BTreeSet<String> {
    read("known-differences.txt")
        .lines()
        .map(|l| {
            l.split_once("  #")
                .map_or(l, |(key, _)| key)
                .trim()
                .to_owned()
        })
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

#[test]
fn line_breaks_match_chromium() {
    let mut found = BTreeSet::new();
    let mut total = 0;
    case_differences(&mut found, &mut total);
    matrix_differences("latin1.txt", &mut found, &mut total);
    matrix_differences("classes.txt", &mut found, &mut total);
    // The data files have 360,751 cases; a smaller number means that a file
    // or a part of it is missing.
    assert!(total >= 360_000, "the data files have {total} cases");
    let known = known();
    let mut report = String::new();
    for key in found.difference(&known) {
        let _ = writeln!(report, "new difference: {key}");
    }
    for key in known.difference(&found) {
        let _ = writeln!(report, "listed but not different: {key}");
    }
    assert!(
        report.is_empty(),
        "{} of {total} cases differ from Chromium:\n{report}",
        found.len()
    );
}
