//! `swb-js bench-compile DIR`: parse and compile time, code size and peak
//! memory for the `*.js` files of a directory (docs/performance.md,
//! "JavaScript engine"). Writes one JSON object to stdout; the Python
//! tool `jsbench` formats it.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use swb_js::{Runtime, RuntimeConfig, ScriptError};

/// Runs the subcommand. Returns the exit code.
pub(crate) fn main(args: &[String]) -> u8 {
    let Some(dir) = args.first() else {
        eprintln!("usage: swb-js bench-compile DIR");
        return 64;
    };
    let mut files: Vec<_> = match std::fs::read_dir(Path::new(dir)) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "js"))
            .collect(),
        Err(error) => {
            eprintln!("swb-js: cannot read {dir}: {error}");
            return 66;
        }
    };
    files.sort();
    let (sources, unreadable) = read_sources(&files);
    let mut report = measure(&sources);
    report["unreadable"] = serde_json::json!(unreadable);
    println!("{report}");
    0
}

/// Reads the files. A file that cannot be read is reported (path and I/O
/// message), not measured as an empty script.
fn read_sources(files: &[std::path::PathBuf]) -> (Vec<String>, Vec<String>) {
    let mut sources = Vec::new();
    let mut unreadable = Vec::new();
    for path in files {
        match std::fs::read(path) {
            Ok(bytes) => sources.push(String::from_utf8_lossy(&bytes).into_owned()),
            Err(error) => unreadable.push(format!("{}: {error}", path.display())),
        }
    }
    (sources, unreadable)
}

/// Compiles every source (best of three passes) and builds the report.
fn measure(sources: &[String]) -> serde_json::Value {
    let Ok(mut rt) = Runtime::new(RuntimeConfig::default()) else {
        return serde_json::json!({ "error": "cannot create the runtime" });
    };
    let mut failures: BTreeMap<String, u32> = BTreeMap::new();
    let mut compiled = 0u32;
    let mut compiled_bytes = 0usize;
    let mut code_bytes = 0usize;
    // Source units (bytes of the text) that the front end read: whole
    // files for scripts that compile, the offset of the error otherwise.
    let mut reached_bytes = 0usize;
    let mut best_all = Duration::MAX;
    let mut best_compiled = Duration::MAX;
    for pass in 0..3 {
        let mut all = Duration::ZERO;
        let mut ok = Duration::ZERO;
        for source in sources {
            let start = Instant::now();
            let result = rt.compile_stats(source);
            let elapsed = start.elapsed();
            all += elapsed;
            if result.is_ok() {
                ok += elapsed;
            }
            if pass == 0 {
                match result {
                    Ok(stats) => {
                        compiled += 1;
                        compiled_bytes += source.len();
                        reached_bytes += source.len();
                        code_bytes += stats.bytes;
                    }
                    Err(ScriptError::Compile {
                        message, offset, ..
                    }) => {
                        reached_bytes += (offset as usize).min(source.len());
                        *failures.entry(message).or_default() += 1;
                    }
                    Err(error) => {
                        *failures.entry(error.to_string()).or_default() += 1;
                    }
                }
            }
            rt.collect();
        }
        best_all = best_all.min(all);
        best_compiled = best_compiled.min(ok);
    }
    serde_json::json!({
        "files": sources.len(),
        "source_bytes": sources.iter().map(String::len).sum::<usize>(),
        "compiled": compiled,
        "compiled_source_bytes": compiled_bytes,
        "code_bytes": code_bytes,
        "compile_seconds": best_compiled.as_secs_f64(),
        "attempt_seconds": best_all.as_secs_f64(),
        "reached_bytes": reached_bytes,
        "peak_rss_kib": peak_rss_kib(),
        "failures": failures,
    })
}

/// The peak resident set size of the process in KiB (Linux; 0 elsewhere).
fn peak_rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmHWM:")
                    .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
            })
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_files_are_reported() {
        let dir = std::env::temp_dir().join(format!("swb-js-bench-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let good = dir.join("a.js");
        std::fs::write(&good, "1;").expect("write");
        // A directory named like a script cannot be read as a file.
        let bad = dir.join("b.js");
        std::fs::create_dir_all(&bad).expect("dir");
        let (sources, unreadable) = read_sources(&[good, bad]);
        assert_eq!(sources, ["1;"]);
        assert_eq!(unreadable.len(), 1);
        assert!(unreadable[0].contains("b.js"), "{unreadable:?}");
    }
}
