//! `swb-js test262`: the test262 runner (ADR 0026 section 13).
//!
//! It reads the frontmatter of each test (INTERPRETING.md of test262),
//! skips the tests that need more than the engine supports, runs the others
//! in strict and sloppy mode with the harness files `sta.js` and
//! `assert.js`, and sums the results per group (the first four components
//! of a test's directory, for example
//! `test/language/expressions/addition`). Each test gets a fresh runtime
//! on a thread with an 8 MiB stack. A scores file records the pass count
//! per group; a run fails if a count goes down.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::shell::STACK_SIZE;

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
const DEFAULT_CONFIG: &str = include_str!("../../test262/subset.txt");
/// The scores file in the repository (the default of `--scores`).
const DEFAULT_SCORES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test262/scores.json");

mod meta;
mod report;
mod run;

use meta::{Meta, Subset, parse_meta, parse_subset};
use report::{read_scores, summarise};
use run::{Limits, Status, collect_tests, group_of, run_mode, run_test};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn unreadable_test_files_fail() {
        let error = read_test(Path::new("/nonexistent-dir"), "test/a.js").expect_err("missing");
        assert!(error.starts_with("cannot read test/a.js: "), "{error}");
    }
}
