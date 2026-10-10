//! Planning and running one test: which tests to skip, how to judge the
//! result of a run, and the test groups of the scores file.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use swb_js::ScriptError;
use swb_js_syntax::messages::NOT_SUPPORTED;

use super::meta::Meta;
use crate::shell::{self, Options};

/// Host features that the shell does not have.
const HOST_USES: &[&str] = &[
    "$262.createRealm",
    "$262.agent",
    "$262.detachArrayBuffer",
    "$262.IsHTMLDDA",
];

/// The harness files that tests may include besides `sta.js` and
/// `assert.js` (which every test gets). A test that includes another file
/// is skipped.
pub(crate) const HARNESS_INCLUDES: &[&str] = &[
    "propertyHelper.js",
    "compareArray.js",
    "isConstructor.js",
    "fnGlobalObject.js",
    "nans.js",
    "decimalToHexString.js",
    "nativeErrors.js",
];

/// What to do with a test.
#[derive(Debug, PartialEq)]
pub(crate) enum Plan {
    /// Not run, for this reason.
    Skip(String),
    /// Run in these modes.
    Run { sloppy: bool, strict: bool },
}

/// Decides whether and how a test runs (the skip rules).
pub(crate) fn plan(meta: &Meta, source: &str, features: &BTreeSet<String>) -> Plan {
    let has = |flag: &str| meta.flags.iter().any(|f| f == flag);
    for flag in ["module", "async", "CanBlockIsTrue"] {
        if has(flag) {
            return Plan::Skip(format!("flag {flag}"));
        }
    }
    if let Some(other) = meta.includes.iter().find(|name| {
        !matches!(name.as_str(), "assert.js" | "sta.js")
            && !HARNESS_INCLUDES.contains(&name.as_str())
    }) {
        return Plan::Skip(format!("include {other}"));
    }
    if let Some(feature) = meta.features.iter().find(|f| !features.contains(*f)) {
        return Plan::Skip(format!("feature {feature}"));
    }
    if let Some(host) = HOST_USES.iter().find(|name| source.contains(*name)) {
        return Plan::Skip(format!("host {host}"));
    }
    if meta
        .negative
        .as_ref()
        .is_some_and(|n| n.phase == "resolution")
    {
        return Plan::Skip("phase resolution".to_owned());
    }
    let (sloppy, strict) = if has("raw") || has("noStrict") {
        (true, false)
    } else if has("onlyStrict") {
        (false, true)
    } else {
        (true, true)
    };
    Plan::Run { sloppy, strict }
}

/// The result of one test (all of its modes).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Status {
    Pass,
    Fail(String),
    /// The compiler rejected a construct with "not supported yet".
    Unsupported(String),
    Skipped(String),
}

/// The limits of one run.
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub(crate) time: Duration,
    pub(crate) heap: usize,
    /// The GC stress mode (a collection at every safepoint).
    pub(crate) stress: bool,
}

/// Runs one test in one mode.
pub(crate) fn run_mode(
    harness: &str,
    meta: &Meta,
    source: &str,
    strict: bool,
    limits: Limits,
) -> Status {
    let raw = meta.flags.iter().any(|f| f == "raw");
    let mut text = String::new();
    if strict {
        text.push_str("\"use strict\";\n");
    }
    if !raw {
        text.push_str(harness);
        text.push('\n');
    }
    text.push_str(source);
    let options = Options {
        heap_limit: Some(limits.heap),
        stress: limits.stress,
        time_limit: Some(limits.time),
    };
    let sink: shell::Output = Rc::new(std::cell::RefCell::new(std::io::sink()));
    let mut rt = match shell::new_runtime(&options, sink) {
        Ok(rt) => rt,
        Err(error) => return Status::Fail(format!("runtime: {error}")),
    };
    let result = shell::run(&mut rt, &options, &text, "");
    judge(&rt, meta, result.map(|_| ()))
}

/// Compares the result of a run with the expectation of the test.
fn judge(rt: &swb_js::Runtime, meta: &Meta, result: Result<(), ScriptError>) -> Status {
    // The engine announces missing features with "not supported yet", as a
    // compile error or as a thrown error (the `Function` constructor).
    if let Err(ScriptError::Compile { message, .. } | ScriptError::Uncaught { message, .. }) =
        &result
        && message.contains(NOT_SUPPORTED)
    {
        return Status::Unsupported(message.clone());
    }
    let Some(negative) = &meta.negative else {
        return match result {
            Ok(()) => Status::Pass,
            Err(error) => Status::Fail(failure_text(rt, &error)),
        };
    };
    match (negative.phase.as_str(), result) {
        (_, Ok(())) => Status::Fail(format!(
            "expected {} in phase {}, but the test completed",
            negative.kind, negative.phase
        )),
        ("parse" | "early", Err(ScriptError::Compile { kind, message, .. })) => {
            if kind.name() == negative.kind {
                Status::Pass
            } else {
                Status::Fail(format!(
                    "expected {}, got {kind:?}: {message}",
                    negative.kind
                ))
            }
        }
        ("runtime", Err(error @ ScriptError::Uncaught { .. })) => {
            let name = rt.constructor_name(rt.last_value());
            if name == negative.kind {
                Status::Pass
            } else {
                Status::Fail(format!(
                    "expected {} at run time, got: {}",
                    negative.kind,
                    failure_text(rt, &error)
                ))
            }
        }
        (phase, Err(error)) => Status::Fail(format!(
            "expected {} in phase {phase}, got: {}",
            negative.kind,
            failure_text(rt, &error)
        )),
    }
}

/// The text of an unexpected error, normalised so that failures group:
/// the constructor of the thrown value is the first word, and the
/// operands of assertion messages (`«...»`) are removed.
fn failure_text(rt: &swb_js::Runtime, error: &ScriptError) -> String {
    let text = match error {
        ScriptError::Uncaught { message, name, .. } => {
            let ctor = rt.constructor_name(rt.last_value());
            let name = if ctor.is_empty() { name } else { &ctor };
            if name.is_empty() {
                // A thrown primitive has no name.
                format!("Uncaught {message}")
            } else {
                format!("Uncaught {name}: {message}")
            }
        }
        other => other.to_string(),
    };
    normalise(&text)
}

fn normalise(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '«' => {
                depth += 1;
                out.push('«');
            }
            '»' => {
                depth = (depth - 1).max(0);
                out.push('…');
                out.push('»');
            }
            _ if depth > 0 => {}
            '\n' => out.push(' '),
            _ => out.push(c),
        }
    }
    out.chars().take(110).collect()
}

/// Runs a test in all of its modes on this thread; a panic is a failure.
pub(crate) fn run_test(
    harness: &str,
    meta: &Meta,
    source: &str,
    subset_features: &BTreeSet<String>,
    limits: Limits,
) -> Vec<(&'static str, Status)> {
    let (sloppy, strict) = match plan(meta, source, subset_features) {
        Plan::Skip(reason) => return vec![("-", Status::Skipped(reason))],
        Plan::Run { sloppy, strict } => (sloppy, strict),
    };
    let mut results = Vec::new();
    for (enabled, label, flag) in [(sloppy, "sloppy", false), (strict, "strict", true)] {
        if !enabled {
            continue;
        }
        let status = catch_unwind(AssertUnwindSafe(|| {
            run_mode(harness, meta, source, flag, limits)
        }))
        .unwrap_or_else(|_| Status::Fail("panic".to_owned()));
        results.push((label, status));
    }
    results
}

/// Combines the modes of a test: any failure, then any unsupported
/// construct, then a pass.
pub(crate) fn combine(results: &[(&'static str, Status)]) -> Status {
    let mut unsupported = None;
    for (mode, status) in results {
        match status {
            Status::Fail(message) => return Status::Fail(format!("[{mode}] {message}")),
            Status::Unsupported(_) if unsupported.is_none() => unsupported = Some(status.clone()),
            Status::Skipped(_) => return status.clone(),
            _ => {}
        }
    }
    unsupported.unwrap_or(Status::Pass)
}

/// The group of a test path: the first four components of its directory
/// (three for `test/built-ins/`, whose third component is the built-in).
pub(crate) fn group_of(path: &str) -> String {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let depth = if dir.starts_with("test/built-ins/") {
        3
    } else {
        4
    };
    dir.split('/').take(depth).collect::<Vec<_>>().join("/")
}

/// Collects the `*.js` tests (not fixtures) below `root`, sorted.
pub(super) fn collect_tests(base: &Path, rel: &str, out: &mut Vec<String>) {
    let path = base.join(rel);
    if path.is_file() {
        out.push(rel.to_owned());
        return;
    }
    let Ok(entries) = std::fs::read_dir(&path) else {
        return;
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let child = format!("{}/{name}", rel.trim_end_matches('/'));
        if base.join(&child).is_dir() {
            collect_tests(base, &child, out);
        } else if Path::new(&name).extension().is_some_and(|e| e == "js")
            && !name.contains("_FIXTURE")
        {
            out.push(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::time::Duration;

    use super::super::meta::parse_meta;

    const LIMITS: Limits = Limits {
        time: Duration::from_millis(5_000),
        heap: 128 << 20,
        stress: false,
    };

    fn features(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn skip_rules() {
        let all = features(&["ok"]);
        let skip = |front: &str, body: &str| plan(&parse_meta(front), body, &all);
        assert_eq!(
            skip("/*---\nflags: [module]\n---*/", ""),
            Plan::Skip("flag module".into())
        );
        assert_eq!(
            skip("/*---\nflags: [async]\n---*/", ""),
            Plan::Skip("flag async".into())
        );
        assert_eq!(
            skip("/*---\nflags: [CanBlockIsTrue]\n---*/", ""),
            Plan::Skip("flag CanBlockIsTrue".into())
        );
        assert_eq!(
            skip("/*---\nincludes: [testTypedArray.js]\n---*/", ""),
            Plan::Skip("include testTypedArray.js".into())
        );
        assert_eq!(
            skip("/*---\nincludes: [propertyHelper.js, nans.js]\n---*/", ""),
            Plan::Run {
                sloppy: true,
                strict: true
            }
        );
        assert_eq!(
            skip("/*---\nfeatures: [ok, other]\n---*/", ""),
            Plan::Skip("feature other".into())
        );
        assert_eq!(
            skip("/*---\n---*/", "$262.createRealm();"),
            Plan::Skip("host $262.createRealm".into())
        );
        assert_eq!(
            skip(
                "/*---\nincludes: [assert.js, sta.js]\nfeatures: [ok]\n---*/",
                ""
            ),
            Plan::Run {
                sloppy: true,
                strict: true
            }
        );
    }

    #[test]
    fn mode_flags() {
        let none = BTreeSet::new();
        let run = |flag: &str| {
            plan(
                &parse_meta(&format!("/*---\nflags: [{flag}]\n---*/")),
                "",
                &none,
            )
        };
        assert_eq!(
            run("onlyStrict"),
            Plan::Run {
                sloppy: false,
                strict: true
            }
        );
        assert_eq!(
            run("noStrict"),
            Plan::Run {
                sloppy: true,
                strict: false
            }
        );
        assert_eq!(
            run("raw"),
            Plan::Run {
                sloppy: true,
                strict: false
            }
        );
    }

    #[test]
    fn groups_use_four_components() {
        assert_eq!(
            group_of("test/language/expressions/addition/a.js"),
            "test/language/expressions/addition"
        );
        assert_eq!(
            group_of("test/language/statements/class/elements/a.js"),
            "test/language/statements/class"
        );
        assert_eq!(group_of("test/language/asi/a.js"), "test/language/asi");
        assert_eq!(
            group_of("test/built-ins/Array/prototype/map/a.js"),
            "test/built-ins/Array"
        );
    }

    const HARNESS: &str = "function Test262Error(m) { this.message = m; }\n\
        function check(c) { if (c !== true) throw new Test262Error('failed'); }";

    fn run(front: &str, body: &str) -> Vec<(&'static str, Status)> {
        let source = format!("{front}\n{body}");
        run_test(
            HARNESS,
            &parse_meta(&source),
            &source,
            &BTreeSet::new(),
            LIMITS,
        )
    }

    #[test]
    fn positive_tests_run_in_both_modes() {
        let results = run("/*---\n---*/", "check(typeof this === 'object' || true);");
        assert_eq!(results.len(), 2);
        assert_eq!(combine(&results), Status::Pass);
        // Strict mode changes `this` in a function.
        let results = run(
            "/*---\n---*/",
            "check((function () { return this === undefined; })());",
        );
        assert!(matches!(results[0], ("sloppy", Status::Fail(_))));
        assert_eq!(results[1], ("strict", Status::Pass));
        assert!(matches!(combine(&results), Status::Fail(_)));
    }

    #[test]
    fn negative_tests_check_phase_and_constructor() {
        let parse = "/*---\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/";
        assert_eq!(combine(&run(parse, "var 1;")), Status::Pass);
        assert!(matches!(combine(&run(parse, "var x;")), Status::Fail(_)));
        // A runtime error is not a parse error.
        assert!(matches!(combine(&run(parse, "null.x;")), Status::Fail(_)));
        let runtime = "/*---\nnegative:\n  phase: runtime\n  type: TypeError\n---*/";
        assert_eq!(combine(&run(runtime, "null.x;")), Status::Pass);
        let wrong = "/*---\nnegative:\n  phase: runtime\n  type: RangeError\n---*/";
        assert!(matches!(combine(&run(wrong, "null.x;")), Status::Fail(_)));
        let custom = "/*---\nnegative:\n  phase: runtime\n  type: Test262Error\n---*/";
        assert_eq!(combine(&run(custom, "check(false);")), Status::Pass);
    }

    #[test]
    fn unsupported_constructs_are_counted_apart() {
        let results = run("/*---\n---*/", "class C {}");
        assert!(matches!(combine(&results), Status::Unsupported(_)));
    }

    #[test]
    fn skipped_tests_do_not_run() {
        let results = run("/*---\nflags: [async]\n---*/", "throw 1;");
        assert_eq!(combine(&results), Status::Skipped("flag async".into()));
    }

    #[test]
    fn thrown_primitives_are_not_unnamed() {
        let rt = swb_js::Runtime::new(swb_js::RuntimeConfig::default()).expect("runtime");
        let error = ScriptError::Uncaught {
            name: String::new(),
            message: "Test262: x".into(),
            offset: None,
        };
        assert_eq!(failure_text(&rt, &error), "Uncaught Test262: x");
    }

    #[test]
    fn normalise_removes_operands() {
        assert_eq!(normalise("Expected «a b» and «c»"), "Expected «…» and «…»");
    }
}
