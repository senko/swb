//! `swb-js test262`: the test262 runner (ADR 0026 section 13).
//!
//! It reads the frontmatter of each test (INTERPRETING.md of test262),
//! skips the tests that need more than the spike supports, runs the others
//! in strict and sloppy mode with the harness files `sta.js` and
//! `assert.js`, and sums the results per group (the first four components
//! of a test's directory, for example
//! `test/language/expressions/addition`). Each test gets a fresh runtime
//! on a thread with an 8 MiB stack. A scores file records the pass count
//! per group; a run fails if a count goes down.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use swb_js::ScriptError;

use crate::shell::{self, Options, STACK_SIZE};

const HELP: &str = "\
swb-js test262: run test262 tests (just test262)

Usage: swb-js test262 [OPTIONS] [PATH...]

  PATH               run only the tests under these paths (relative to the
                     test262 directory, e.g. test/language/statements/if);
                     without PATH: the directories of the subset file
  --dir DIR          the test262 directory (default: $TEST262_DIR or out/test262)
  --all              run all of test/language/ (to choose the subset)
  --config FILE      the subset file (default: crates/js/test262/subset.txt)
  --scores FILE      the scores file (default: crates/js/test262/scores.json)
  --update           write the pass counts of the subset's groups to the scores
                     file (needs a run of whole groups: no file or inner path)
  --stress           GC stress mode (slow; looks for rooting bugs)
  --jobs N           threads (default: the number of CPUs)
  --time-limit MS    per run (default 10000)
  --heap-limit MIB   per run (default 256)
  --report FILE      write one line per test and mode (TSV) to FILE
  --top N            show the N most common failure messages (default 10)
  --help             this text

Exit code 1 if a pass count is below the scores file, a test panicked, no
test was found or the scores file is corrupt; 64 if --update cannot apply.
";

/// The subset file compiled in (the default of `--config`).
const DEFAULT_CONFIG: &str = include_str!("../test262/subset.txt");
/// The scores file in the repository (the default of `--scores`).
const DEFAULT_SCORES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test262/scores.json");

// --- Frontmatter ---

/// The fields of the `/*--- ... ---*/` block that the runner uses.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Meta {
    pub(crate) includes: Vec<String>,
    pub(crate) flags: Vec<String>,
    pub(crate) features: Vec<String>,
    pub(crate) negative: Option<Negative>,
}

/// `negative:` (phase and constructor name).
#[derive(Debug, PartialEq)]
pub(crate) struct Negative {
    pub(crate) phase: String,
    pub(crate) kind: String,
}

/// Reads the frontmatter of a test. A test without it has no fields.
pub(crate) fn parse_meta(source: &str) -> Meta {
    let Some(start) = source.find("/*---") else {
        return Meta::default();
    };
    let body = &source[start + 5..];
    let body = body.find("---*/").map_or(body, |end| &body[..end]);
    // Top-level keys with the lines that belong to them.
    let mut keys: Vec<(&str, &str, Vec<&str>)> = Vec::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(last) = keys.last_mut() {
                last.2.push(line.trim());
            }
        } else if let Some((key, value)) = line.split_once(':') {
            keys.push((key.trim(), value.trim(), Vec::new()));
        }
    }
    let mut meta = Meta::default();
    for (key, inline, block) in keys {
        match key {
            "includes" => meta.includes = list(inline, &block),
            "flags" => meta.flags = list(inline, &block),
            "features" => meta.features = list(inline, &block),
            "negative" => meta.negative = negative(&block),
            _ => {}
        }
    }
    meta
}

/// A YAML list: `[a, b]` inline or `- a` lines.
fn list(inline: &str, block: &[&str]) -> Vec<String> {
    let mut items = Vec::new();
    if let Some(inner) = inline.strip_prefix('[') {
        let inner = inner.trim_end().trim_end_matches(']');
        items.extend(inner.split(',').map(|item| item.trim().to_owned()));
    }
    for line in block {
        if let Some(item) = line.strip_prefix('-') {
            items.push(item.trim().to_owned());
        }
    }
    items.retain(|item| !item.is_empty());
    items
}

fn negative(block: &[&str]) -> Option<Negative> {
    let mut phase = None;
    let mut kind = None;
    for line in block {
        match line.split_once(':') {
            Some(("phase", value)) => phase = Some(value.trim().to_owned()),
            Some(("type", value)) => kind = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    Some(Negative {
        phase: phase?,
        kind: kind?,
    })
}

// --- Subset ---

/// The subset: the groups to run and the features in scope.
#[derive(Debug, Default)]
pub(crate) struct Subset {
    pub(crate) dirs: Vec<String>,
    pub(crate) features: BTreeSet<String>,
}

/// Parses the subset file: `dir PATH` and `feature NAME` lines; `#`
/// starts a comment.
pub(crate) fn parse_subset(text: &str) -> Subset {
    let mut subset = Subset::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        match line.split_once(char::is_whitespace) {
            Some(("dir", path)) => subset.dirs.push(path.trim().to_owned()),
            Some(("feature", name)) => {
                subset.features.insert(name.trim().to_owned());
            }
            _ => {}
        }
    }
    subset
}

// --- What to run ---

/// Host features that the shell does not have.
const HOST_USES: &[&str] = &[
    "$262.createRealm",
    "$262.agent",
    "$262.detachArrayBuffer",
    "$262.IsHTMLDDA",
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
    if let Some(other) = meta
        .includes
        .iter()
        .find(|name| !matches!(name.as_str(), "assert.js" | "sta.js"))
    {
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

// --- Running ---

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
    let result = shell::run(&mut rt, &options, &text);
    judge(&rt, meta, result.map(|_| ()))
}

/// Compares the result of a run with the expectation of the test.
fn judge(rt: &swb_js::Runtime, meta: &Meta, result: Result<(), ScriptError>) -> Status {
    // The engine announces missing features with "not supported yet", as a
    // compile error or as a thrown error (the `Function` constructor).
    if let Err(ScriptError::Compile { message, .. } | ScriptError::Uncaught { message, .. }) =
        &result
        && message.contains("not supported yet")
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

// --- Files ---

/// The group of a test path: the first four components of its directory.
pub(crate) fn group_of(path: &str) -> String {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    dir.split('/').take(4).collect::<Vec<_>>().join("/")
}

/// Collects the `*.js` tests (not fixtures) below `root`, sorted.
fn collect_tests(base: &Path, rel: &str, out: &mut Vec<String>) {
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

// --- Totals ---

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Counts {
    pass: u32,
    fail: u32,
    unsupported: u32,
    skipped: u32,
}

impl Counts {
    fn add(&mut self, status: &Status) {
        match status {
            Status::Pass => self.pass += 1,
            Status::Fail(_) => self.fail += 1,
            Status::Unsupported(_) => self.unsupported += 1,
            Status::Skipped(_) => self.skipped += 1,
        }
    }
}

/// The command line of the runner.
struct Args {
    dir: PathBuf,
    all: bool,
    config: Option<String>,
    scores: String,
    update: bool,
    jobs: usize,
    limits: Limits,
    report: Option<String>,
    top: usize,
    paths: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Option<Args>, String> {
    let mut parsed = Args {
        dir: PathBuf::from(std::env::var("TEST262_DIR").unwrap_or_else(|_| "out/test262".into())),
        all: false,
        config: None,
        scores: DEFAULT_SCORES.to_owned(),
        update: false,
        jobs: std::thread::available_parallelism().map_or(4, usize::from),
        limits: Limits {
            time: Duration::from_millis(10_000),
            heap: 256 << 20,
            stress: false,
        },
        report: None,
        top: 10,
        paths: Vec::new(),
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().cloned().ok_or(format!("{name} needs a value"));
        let number = |text: String, name: &str| {
            text.parse::<u64>()
                .map_err(|_| format!("{name} needs a number"))
        };
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--dir" => parsed.dir = PathBuf::from(value(arg)?),
            "--all" => parsed.all = true,
            "--config" => parsed.config = Some(value(arg)?),
            "--scores" => parsed.scores = value(arg)?,
            "--update" => parsed.update = true,
            "--stress" => parsed.limits.stress = true,
            "--jobs" => parsed.jobs = number(value(arg)?, arg)?.max(1) as usize,
            "--time-limit" => parsed.limits.time = Duration::from_millis(number(value(arg)?, arg)?),
            "--heap-limit" => parsed.limits.heap = (number(value(arg)?, arg)? as usize) << 20,
            "--report" => parsed.report = Some(value(arg)?),
            "--top" => parsed.top = number(value(arg)?, arg)? as usize,
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            path => parsed.paths.push(path.trim_end_matches('/').to_owned()),
        }
    }
    Ok(Some(parsed))
}

/// One test's outcome for the report.
struct Done {
    path: String,
    modes: Vec<(&'static str, Status)>,
}

/// Runs the subcommand. Returns the exit code.
pub(crate) fn main(args: &[String]) -> u8 {
    let args = match parse_args(args) {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{HELP}");
            return 0;
        }
        Err(message) => {
            eprintln!("swb-js test262: {message}\n\n{HELP}");
            return 64;
        }
    };
    let config = match &args.config {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("swb-js test262: cannot read {path}: {error}");
                return 66;
            }
        },
        None => DEFAULT_CONFIG.to_owned(),
    };
    let subset = parse_subset(&config);
    let Some(harness) = load_harness(&args.dir) else {
        eprintln!(
            "swb-js test262: no harness in {} (set TEST262_DIR, or run `just test262`)",
            args.dir.display()
        );
        return 66;
    };
    if let Some(error) = check_harness(&harness, args.limits) {
        eprintln!("swb-js test262: the harness files do not run: {error}");
        return 1;
    }
    let roots: Vec<String> = if !args.paths.is_empty() {
        args.paths.clone()
    } else if args.all {
        vec!["test/language".to_owned()]
    } else {
        subset.dirs.clone()
    };
    let mut tests = Vec::new();
    for root in &roots {
        collect_tests(&args.dir, root, &mut tests);
    }
    tests.sort();
    tests.dedup();
    if args.paths.is_empty() && !args.all {
        // A `dir` line names one group, not the groups below it.
        tests.retain(|test| subset.dirs.contains(&group_of(test)));
    }
    if tests.is_empty() {
        eprintln!(
            "swb-js test262: no tests found under {} in {}",
            roots.join(", "),
            args.dir.display()
        );
        return 1;
    }
    let scores = match read_scores(&args.scores) {
        Ok(scores) => scores,
        Err(message) => {
            eprintln!("swb-js test262: {message}");
            return 1;
        }
    };
    let done = run_all(&args, &subset, &harness, &tests);
    summarise(&args, &subset, &roots, &scores, &done)
}

/// `assert.js` and `sta.js` (in this order of dependency: `sta.js` first).
fn load_harness(dir: &Path) -> Option<String> {
    let sta = std::fs::read_to_string(dir.join("harness/sta.js")).ok()?;
    let assert = std::fs::read_to_string(dir.join("harness/assert.js")).ok()?;
    Some(format!("{sta}\n{assert}"))
}

/// Runs the harness files alone; the error if they fail.
fn check_harness(harness: &str, limits: Limits) -> Option<String> {
    let meta = Meta::default();
    match run_mode(harness, &meta, "", false, limits) {
        Status::Pass => None,
        other => Some(format!("{other:?}")),
    }
}

/// Reads a test file. A file that cannot be read is a failed test with the
/// I/O message, not an empty source (which would pass).
fn read_test(dir: &Path, path: &str) -> Result<String, String> {
    std::fs::read(dir.join(path))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| format!("cannot read {path}: {error}"))
}

/// Reads and runs one test file.
fn run_file(
    args: &Args,
    subset: &Subset,
    harness: &str,
    path: &str,
) -> Vec<(&'static str, Status)> {
    match read_test(&args.dir, path) {
        Ok(source) => {
            let meta = parse_meta(&source);
            run_test(harness, &meta, &source, &subset.features, args.limits)
        }
        Err(message) => vec![("-", Status::Fail(message))],
    }
}

/// Runs the tests on worker threads (8 MiB stacks).
fn run_all(args: &Args, subset: &Subset, harness: &str, tests: &[String]) -> Vec<Done> {
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<Done>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..args.jobs {
            let worker = || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = tests.get(index) else {
                        return;
                    };
                    let modes = run_file(args, subset, harness, path);
                    if let Ok(mut done) = done.lock() {
                        done.push(Done {
                            path: path.clone(),
                            modes,
                        });
                    }
                }
            };
            let spawned = std::thread::Builder::new()
                .stack_size(STACK_SIZE)
                .spawn_scoped(scope, worker);
            if let Err(error) = spawned {
                eprintln!("swb-js test262: cannot start a thread: {error}");
            }
        }
    });
    let mut done = done.into_inner().unwrap_or_default();
    done.sort_by(|a, b| a.path.cmp(&b.path));
    done
}

/// Prints the table and the failure causes, writes the report and the
/// scores, and returns the exit code.
fn summarise(
    args: &Args,
    subset: &Subset,
    roots: &[String],
    scores: &BTreeMap<String, u32>,
    done: &[Done],
) -> u8 {
    let mut groups: BTreeMap<String, Counts> = BTreeMap::new();
    let mut causes: BTreeMap<String, u32> = BTreeMap::new();
    let mut skips: BTreeMap<String, u32> = BTreeMap::new();
    let mut unsupported: BTreeMap<String, u32> = BTreeMap::new();
    let mut panics = 0;
    let mut report = String::new();
    for test in done {
        let status = combine(&test.modes);
        groups.entry(group_of(&test.path)).or_default().add(&status);
        match &status {
            Status::Fail(message) => {
                let cause = message.split_once("] ").map_or(&message[..], |(_, m)| m);
                if cause == "panic" {
                    panics += 1;
                }
                *causes.entry(cause.to_owned()).or_default() += 1;
            }
            Status::Unsupported(message) => *unsupported.entry(message.clone()).or_default() += 1,
            Status::Skipped(reason) => *skips.entry(reason.clone()).or_default() += 1,
            Status::Pass => {}
        }
        for (mode, status) in &test.modes {
            let (word, message) = match status {
                Status::Pass => ("pass", ""),
                Status::Fail(m) => ("fail", m.as_str()),
                Status::Unsupported(m) => ("unsupported", m.as_str()),
                Status::Skipped(m) => ("skipped", m.as_str()),
            };
            let _ = writeln!(report, "{}\t{mode}\t{word}\t{message}", test.path);
        }
    }
    if let Some(path) = &args.report
        && let Err(error) = std::fs::write(path, report)
    {
        eprintln!("swb-js test262: cannot write {path}: {error}");
    }
    let regressions = if args.update {
        Vec::new()
    } else {
        regressions(scores, roots, &groups)
    };
    for line in &regressions {
        eprintln!("REGRESSION {line}");
    }
    print_table(&groups, scores);
    print_top("Top failures", &causes, args.top);
    print_top("Unsupported constructs", &unsupported, 5);
    print_top("Skip reasons", &skips, 8);
    let total = groups.values().fold(Counts::default(), |mut t, c| {
        t.pass += c.pass;
        t.fail += c.fail;
        t.unsupported += c.unsupported;
        t.skipped += c.skipped;
        t
    });
    println!(
        "\ntotal: {} pass, {} fail, {} unsupported, {} skipped ({} tests)",
        total.pass,
        total.fail,
        total.unsupported,
        total.skipped,
        done.len()
    );
    if args.update {
        return write_scores(args, subset, roots, scores, &groups);
    }
    if panics > 0 {
        eprintln!("{panics} test(s) panicked");
    }
    u8::from(!regressions.is_empty() || panics > 0)
}

fn print_top(title: &str, map: &BTreeMap<String, u32>, n: usize) {
    if map.is_empty() || n == 0 {
        return;
    }
    let mut items: Vec<_> = map.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("\n{title}:");
    for (message, count) in items.into_iter().take(n) {
        println!("{count:6}  {message}");
    }
}

/// The recorded pass counts. A missing file is empty (with a warning); a
/// file that cannot be read or parsed is an error.
fn read_scores(path: &str) -> Result<BTreeMap<String, u32>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("swb-js test262: warning: no scores file {path}; starting empty");
            return Ok(BTreeMap::new());
        }
        Err(error) => return Err(format!("cannot read the scores file {path}: {error}")),
    };
    serde_json::from_str(&text)
        .map_err(|error| format!("the scores file {path} is corrupt: {error}"))
}

/// Whether the run covers a group completely: a root is the group or a
/// directory above it. A root below the group (or a single file) covers
/// only a part of it.
fn covers(roots: &[String], group: &str) -> bool {
    roots.iter().any(|root| {
        group == root
            || group
                .strip_prefix(root.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// The recorded groups that went down, as `GROUP: NOW < RECORDED`. Only
/// groups that the run covers completely count.
fn regressions(
    scores: &BTreeMap<String, u32>,
    roots: &[String],
    groups: &BTreeMap<String, Counts>,
) -> Vec<String> {
    let mut found = Vec::new();
    for (group, &recorded) in scores {
        if !covers(roots, group) {
            continue;
        }
        let now = groups.get(group).map_or(0, |c| c.pass);
        if now < recorded {
            found.push(format!("{group}: {now} < {recorded}"));
        }
    }
    found
}

fn print_table(groups: &BTreeMap<String, Counts>, scores: &BTreeMap<String, u32>) {
    println!(
        "{:<58} {:>6} {:>6} {:>6} {:>6}  change",
        "group", "pass", "fail", "unsup", "skip"
    );
    for (group, c) in groups {
        let change = match scores.get(group) {
            Some(&old) if c.pass > old => format!("+{}", c.pass - old),
            Some(&old) if c.pass < old => format!("-{}", old - c.pass),
            Some(_) => String::new(),
            None => "new".to_owned(),
        };
        println!(
            "{group:<58} {:>6} {:>6} {:>6} {:>6}  {change}",
            c.pass, c.fail, c.unsupported, c.skipped
        );
    }
}

/// The scores file after an update: the recorded counts, with the counts of
/// the groups of the subset that the run covered. `Err` names the reason
/// that the run cannot update the file.
fn updated_scores(
    scores: &BTreeMap<String, u32>,
    subset: &Subset,
    roots: &[String],
    groups: &BTreeMap<String, Counts>,
) -> Result<BTreeMap<String, u32>, String> {
    if let Some(partial) = groups.keys().find(|group| !covers(roots, group)) {
        return Err(format!(
            "--update needs a run of whole groups; the run covers only a part of {partial} \
             (name the group directory, or run without PATH)"
        ));
    }
    let mut updated = scores.clone();
    for (group, c) in groups {
        if subset.dirs.contains(group) {
            updated.insert(group.clone(), c.pass);
        }
    }
    Ok(updated)
}

/// Writes the pass counts of the subset groups that ran into the scores
/// file (other groups keep their counts).
fn write_scores(
    args: &Args,
    subset: &Subset,
    roots: &[String],
    scores: &BTreeMap<String, u32>,
    groups: &BTreeMap<String, Counts>,
) -> u8 {
    let updated = match updated_scores(scores, subset, roots, groups) {
        Ok(updated) => updated,
        Err(message) => {
            eprintln!("swb-js test262: {message}");
            return 64;
        }
    };
    let mut text = serde_json::to_string_pretty(&updated).unwrap_or_default();
    text.push('\n');
    match std::fs::write(&args.scores, text) {
        Ok(()) => {
            println!("wrote {}", args.scores);
            0
        }
        Err(error) => {
            eprintln!("swb-js test262: cannot write {}: {error}", args.scores);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        time: Duration::from_millis(5_000),
        heap: 128 << 20,
        stress: false,
    };

    fn features(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn frontmatter_inline_and_block_lists() {
        let meta = parse_meta(
            "// c\n/*---\ndescription: |\n  text\n  more: text\nincludes: [assert.js, sta.js]\nflags:\n  - onlyStrict\n  - raw\nfeatures: [a, b]\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/\nx;",
        );
        assert_eq!(meta.includes, ["assert.js", "sta.js"]);
        assert_eq!(meta.flags, ["onlyStrict", "raw"]);
        assert_eq!(meta.features, ["a", "b"]);
        assert_eq!(
            meta.negative,
            Some(Negative {
                phase: "parse".into(),
                kind: "SyntaxError".into()
            })
        );
    }

    #[test]
    fn frontmatter_missing_or_empty() {
        assert_eq!(parse_meta("x = 1;"), Meta::default());
        let meta = parse_meta("/*---\nflags: []\n---*/");
        assert_eq!(meta, Meta::default());
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
            skip("/*---\nincludes: [propertyHelper.js]\n---*/", ""),
            Plan::Skip("include propertyHelper.js".into())
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
    }

    #[test]
    fn subset_file() {
        let subset = parse_subset("# c\ndir test/a/b # x\nfeature f1\n\nfeature f2\n");
        assert_eq!(subset.dirs, ["test/a/b"]);
        assert_eq!(subset.features, features(&["f1", "f2"]));
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

    fn counts(pass: u32) -> Counts {
        Counts {
            pass,
            ..Counts::default()
        }
    }

    fn map<T: Clone>(items: &[(&str, T)]) -> BTreeMap<String, T> {
        items
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn coverage_is_by_whole_groups() {
        let roots = |r: &[&str]| r.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(covers(&roots(&["test/language/a/b"]), "test/language/a/b"));
        assert!(covers(&roots(&["test/language"]), "test/language/a/b"));
        assert!(!covers(
            &roots(&["test/language/a/b/c.js"]),
            "test/language/a/b"
        ));
        assert!(!covers(
            &roots(&["test/language/a/b/c"]),
            "test/language/a/b"
        ));
        assert!(!covers(
            &roots(&["test/language/a/bc"]),
            "test/language/a/b"
        ));
    }

    #[test]
    fn narrow_runs_do_not_regress_a_group() {
        let scores = map(&[("test/language/a/b", 5), ("test/language/a/c", 2)]);
        let groups = map(&[("test/language/a/b", counts(1))]);
        // One file of a group: not a regression.
        let narrow = ["test/language/a/b/x.js".to_owned()];
        let found = regressions(&scores, &narrow, &groups);
        assert!(found.is_empty(), "{found:?}");
        // The whole group: a regression; the other group is not covered.
        let whole = ["test/language/a/b".to_owned()];
        assert_eq!(
            regressions(&scores, &whole, &groups),
            ["test/language/a/b: 1 < 5"]
        );
    }

    #[test]
    fn update_writes_covered_subset_groups_only() {
        let subset = parse_subset("dir test/language/a/b\n");
        let scores = map(&[("test/language/a/b", 5), ("test/language/a/c", 2)]);
        let groups = map(&[
            ("test/language/a/b", counts(7)),
            ("test/language/a/d", counts(3)),
        ]);
        let all = ["test/language".to_owned()];
        let updated = updated_scores(&scores, &subset, &all, &groups).expect("whole groups");
        assert_eq!(
            updated,
            map(&[("test/language/a/b", 7), ("test/language/a/c", 2)])
        );
        // A partial run is refused.
        let part = ["test/language/a/b/x.js".to_owned()];
        let error = updated_scores(&scores, &subset, &part, &groups).expect_err("partial");
        assert!(
            error.contains("--update needs a run of whole groups"),
            "{error}"
        );
    }

    #[test]
    fn scores_file_errors() {
        let dir = std::env::temp_dir().join(format!("swb-js-scores-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = |name: &str| dir.join(name).to_str().expect("utf-8").to_owned();
        assert_eq!(read_scores(&path("missing.json")), Ok(BTreeMap::new()));
        std::fs::write(path("bad.json"), "{not json").expect("write");
        assert!(read_scores(&path("bad.json")).is_err());
        // A directory cannot be read as a file.
        assert!(read_scores(&dir.to_string_lossy()).is_err());
        std::fs::write(path("ok.json"), "{\"g\": 3}").expect("write");
        assert_eq!(read_scores(&path("ok.json")), Ok(map(&[("g", 3)])));
    }

    #[test]
    fn unreadable_test_files_fail() {
        let error = read_test(Path::new("/nonexistent-dir"), "test/a.js").expect_err("missing");
        assert!(error.starts_with("cannot read test/a.js: "), "{error}");
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
