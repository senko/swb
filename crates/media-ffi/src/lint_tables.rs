//! Ties the crate's lint tables to the workspace's (ADR 0047 section 4).
//!
//! The crate cannot use `[lints] workspace = true` (Cargo cannot override
//! one lint of the workspace table), so `Cargo.toml` has a copy. This test
//! fails when the copy differs from the workspace tables in anything but
//! the allowed differences below.

use std::collections::BTreeMap;

/// The differences of the crate's tables: (table, lint, value).
const DIFFERENCES: &[(&str, &str, &str)] = &[
    ("rust", "unsafe_code", "\"allow\""),
    ("rust", "unsafe_op_in_unsafe_fn", "\"deny\""),
    ("clippy", "undocumented_unsafe_blocks", "\"deny\""),
    ("clippy", "multiple_unsafe_ops_per_block", "\"deny\""),
    ("clippy", "missing_safety_doc", "\"deny\""),
];

type Tables = BTreeMap<(String, String), String>;

/// The `key = value` lines of the tables `[PREFIX.rust]` and
/// `[PREFIX.clippy]`, with spaces removed from the values. Enough for
/// the simple tables of these two files.
fn lint_tables(toml: &str, prefix: &str) -> Tables {
    let mut tables = Tables::new();
    let mut current: Option<String> = None;
    for line in toml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = header
                .strip_prefix(prefix)
                .and_then(|t| t.strip_prefix('.'))
                .filter(|t| *t == "rust" || *t == "clippy")
                .map(str::to_owned);
            continue;
        }
        let Some(table) = &current else { continue };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value: String = value.chars().filter(|c| !c.is_whitespace()).collect();
        tables.insert((table.clone(), key.trim().to_owned()), value);
    }
    tables
}

#[test]
fn lint_tables_follow_the_workspace() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let workspace = std::fs::read_to_string(format!("{dir}/../../Cargo.toml"))
        .expect("the workspace manifest is readable in tests");
    let own = std::fs::read_to_string(format!("{dir}/Cargo.toml"))
        .expect("the crate manifest is readable in tests");
    let mut expected = lint_tables(&workspace, "workspace.lints");
    assert!(
        expected.contains_key(&("rust".to_owned(), "unsafe_code".to_owned())),
        "the workspace forbids unsafe code"
    );
    for (table, key, value) in DIFFERENCES {
        expected.insert(
            ((*table).to_owned(), (*key).to_owned()),
            (*value).to_owned(),
        );
    }
    let actual = lint_tables(&own, "lints");
    assert_eq!(
        actual, expected,
        "crates/media-ffi/Cargo.toml [lints] must be the workspace tables with the differences of ADR 0047 section 4"
    );
}

#[test]
fn parser_reads_both_tables() {
    let toml = "[x.rust]\na = \"warn\"\n# c\n[x.clippy]\nb = { level = \"warn\", priority = -1 }\n[y]\nc = 1\n";
    let t = lint_tables(toml, "x");
    assert_eq!(t.len(), 2);
    assert_eq!(
        t.get(&("clippy".to_owned(), "b".to_owned()))
            .map(String::as_str),
        Some("{level=\"warn\",priority=-1}")
    );
}
