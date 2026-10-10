//! The parse-only mode of the runner (`--parse-only`): only the parser,
//! the early errors and the scope analysis of `js-syntax` run. A negative
//! test of phase `parse` passes if the parse fails with a `SyntaxError`;
//! every other test passes if it parses (a negative test of phase
//! `resolution` too: its error comes from linking, which is not part of
//! the parse). Tests with the flag `module` parse with the Module goal.
//! Tests of features after ES2025 are skipped
//! (`crates/js/test262/parse-skip.txt`).

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use swb_js_syntax::messages::NOT_SUPPORTED;
use swb_js_syntax::{ErrorKind, parse_module, parse_script};
use swb_js_text::{RecursionBudget, String16};

use super::meta::Meta;
use super::run::Status;

/// The skip list compiled in.
pub(super) const DEFAULT_SKIP: &str = include_str!("../../test262/parse-skip.txt");

/// The roots of a parse-only run without paths.
pub(super) const ROOTS: &[&str] = &["test/language", "test/built-ins", "test/annexB"];

/// Reads the skip list: `feature NAME` lines; `#` starts a comment.
pub(super) fn parse_skip_list(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| line.split('#').next())
        .filter_map(|line| line.trim().strip_prefix("feature "))
        .map(|name| name.trim().to_owned())
        .collect()
}

/// The goal and mode of one parse of a test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Sloppy,
    Strict,
    Module,
}

/// In which modes a test runs.
fn plan(meta: &Meta, skipped: &BTreeSet<String>) -> Result<&'static [Mode], String> {
    let has = |flag: &str| meta.flags.iter().any(|f| f == flag);
    if let Some(feature) = meta.features.iter().find(|f| skipped.contains(*f)) {
        return Err(format!("feature {feature}"));
    }
    Ok(if has("module") {
        &[Mode::Module]
    } else if has("raw") || has("noStrict") {
        &[Mode::Sloppy]
    } else if has("onlyStrict") {
        &[Mode::Strict]
    } else {
        &[Mode::Sloppy, Mode::Strict]
    })
}

/// Whether a test is about regular expression syntax, whose early errors
/// come with M7 feature 8a.
fn is_regexp_test(path: &str, meta: &Meta) -> bool {
    path.to_ascii_lowercase().contains("regexp")
        || meta.features.iter().any(|f| f.starts_with("regexp"))
}

/// Parses one test in one mode and judges the result.
fn run_mode(path: &str, meta: &Meta, source: &str, mode: Mode) -> Status {
    let raw = meta.flags.iter().any(|f| f == "raw");
    let text = if mode == Mode::Strict && !raw {
        format!("\"use strict\";\n{source}")
    } else {
        source.to_owned()
    };
    let units = String16::from(text.as_str());
    let mut budget = RecursionBudget::default();
    let result = if mode == Mode::Module {
        parse_module(units.as_str16(), &mut budget)
    } else {
        parse_script(units.as_str16(), &mut budget)
    };
    let expects_error = meta
        .negative
        .as_ref()
        .is_some_and(|n| n.phase == "parse" || n.phase == "early");
    match result {
        Err(error) if error.message == NOT_SUPPORTED => Status::Unsupported(format!(
            "{NOT_SUPPORTED} ({})",
            error.unsupported.unwrap_or("?")
        )),
        Err(error) if expects_error => {
            let kind = meta.negative.as_ref().map_or("", |n| n.kind.as_str());
            if error.kind == ErrorKind::Syntax && kind == "SyntaxError" {
                Status::Pass
            } else {
                Status::Fail(format!("expected {kind}, got {error}"))
            }
        }
        Err(error) => Status::Fail(format!("{error}").chars().take(110).collect()),
        Ok(_) if expects_error && is_regexp_test(path, meta) => Status::Fail(
            "expected SyntaxError (regular expression syntax, M7 feature 8a), but it parsed"
                .to_owned(),
        ),
        Ok(_) if expects_error => Status::Fail("expected SyntaxError, but it parsed".to_owned()),
        Ok(_) => Status::Pass,
    }
}

/// Parses a test in all of its modes; a panic is a failure.
pub(super) fn run_test(
    path: &str,
    meta: &Meta,
    source: &str,
    skipped: &BTreeSet<String>,
) -> Vec<(&'static str, Status)> {
    let modes = match plan(meta, skipped) {
        Ok(modes) => modes,
        Err(reason) => return vec![("-", Status::Skipped(reason))],
    };
    let mut results = Vec::new();
    for &mode in modes {
        let label = match mode {
            Mode::Sloppy => "sloppy",
            Mode::Strict => "strict",
            Mode::Module => "module",
        };
        let status = catch_unwind(AssertUnwindSafe(|| run_mode(path, meta, source, mode)))
            .unwrap_or_else(|_| Status::Fail("panic".to_owned()));
        results.push((label, status));
    }
    results
}

#[cfg(test)]
mod tests {
    use super::super::meta::parse_meta;
    use super::super::run::combine;
    use super::*;

    fn run(front: &str, body: &str) -> Status {
        let source = format!("{front}\n{body}");
        let skipped = parse_skip_list("feature later # a comment\n");
        combine(&run_test(
            "test/language/x.js",
            &parse_meta(&source),
            &source,
            &skipped,
        ))
    }

    #[test]
    fn positive_tests_must_parse_in_their_modes() {
        assert_eq!(run("/*---\n---*/", "var x = 1;"), Status::Pass);
        assert!(matches!(run("/*---\n---*/", "var 1;"), Status::Fail(_)));
        // Strict mode applies unless the flags say otherwise.
        assert!(matches!(
            run("/*---\n---*/", "with (a) b;"),
            Status::Fail(_)
        ));
        assert_eq!(
            run("/*---\nflags: [noStrict]\n---*/", "with (a) b;"),
            Status::Pass
        );
        assert_eq!(
            run("/*---\nflags: [raw]\n---*/", "with (a) b;"),
            Status::Pass
        );
    }

    #[test]
    fn negative_parse_tests_need_a_syntax_error() {
        let parse = "/*---\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/";
        assert_eq!(run(parse, "var 1;"), Status::Pass);
        assert!(matches!(run(parse, "var x;"), Status::Fail(_)));
        // A runtime error does not happen in a parse.
        let runtime = "/*---\nnegative:\n  phase: runtime\n  type: TypeError\n---*/";
        assert_eq!(run(runtime, "null.x;"), Status::Pass);
    }

    #[test]
    fn skips_and_modules() {
        assert_eq!(
            run("/*---\nfeatures: [later]\n---*/", "x"),
            Status::Skipped("feature later".into())
        );
        assert_eq!(run("/*---\n---*/", "import('a')"), Status::Pass);
        assert_eq!(run("/*---\n---*/", "class A {}"), Status::Pass);
        // Module tests parse with the Module goal.
        let module = "/*---\nflags: [module]\n---*/";
        assert_eq!(run(module, "export var a; await 1;"), Status::Pass);
        assert!(matches!(
            run("/*---\n---*/", "export var a;"),
            Status::Fail(_)
        ));
        let parse = "/*---\nnegative:\n  phase: parse\n  type: SyntaxError\nflags: [module]\n---*/";
        assert_eq!(run(parse, "export {a};"), Status::Pass);
        // Link errors are not parse errors.
        let link =
            "/*---\nnegative:\n  phase: resolution\n  type: SyntaxError\nflags: [module]\n---*/";
        assert_eq!(run(link, "import {a} from './x.js';"), Status::Pass);
        assert!(matches!(run(link, "export {a};"), Status::Fail(_)));
    }

    #[test]
    fn the_skip_list_in_the_repository_reads() {
        let skipped = parse_skip_list(DEFAULT_SKIP);
        assert!(skipped.contains("decorators"));
        assert!(skipped.contains("explicit-resource-management"));
        assert!(!skipped.contains("destructuring-binding"));
    }
}
