//! Parses JavaScript files with the scope analysis and reports, per file,
//! whether it parses or the first error (for a construct outside the
//! supported subset, its name). A measurement tool for the parser, not
//! part of `just check`.
//!
//! Usage:
//!
//! - `cargo run --release -p swb-js-syntax --example parse_files --
//!   FILE_OR_DIRECTORY...` (a directory means its `*.js` files);
//! - `... --example parse_files -- --generate MB`: parses a generated
//!   program of about `MB` megabytes that uses only the subset, and
//!   reports the time, the memory of the tree and the scope tables, and
//!   the bytes per token.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

#[path = "common/mod.rs"]
mod common;

use swb_js_syntax::{
    Goal, Interner, Lexer, LexerOptions, LineIndex, ParseError, Script, TokenKind, parse_script,
};
use swb_js_text::{RecursionBudget, Str16, String16};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--generate") {
        let megabytes: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(5);
        generated(megabytes);
        return ExitCode::SUCCESS;
    }
    let files = common::js_files(&args).unwrap_or_default();
    if files.is_empty() {
        eprintln!("usage: parse_files [--generate MB | FILE_OR_DIRECTORY...]");
        return ExitCode::FAILURE;
    }
    let mut parsed = 0;
    let mut first_unsupported: BTreeMap<&str, usize> = BTreeMap::default();
    let mut total = Duration::ZERO;
    let mut bytes = 0;
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            println!("{}: unreadable", file.display());
            continue;
        };
        bytes += text.len();
        let source = String16::from(text.as_str());
        let start = Instant::now();
        let result = parse(source.as_str16());
        total += start.elapsed();
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        match result {
            Ok(script) => {
                parsed += 1;
                println!(
                    "{name}: ok ({} functions, {} KB tree and scopes)",
                    script.ast.function_count(),
                    (script.ast.heap_size() + script.scopes.heap_size()) / 1024
                );
            }
            Err(error) => {
                let construct = error.unsupported.unwrap_or("syntax error");
                *first_unsupported.entry(construct).or_default() += 1;
                println!(
                    "{name}: {} ({:.0} % of the file before it)",
                    describe(&error, source.as_str16()),
                    f64::from(error.offset) * 100.0 / source.len().max(1) as f64
                );
            }
        }
    }
    for (construct, count) in &first_unsupported {
        println!("first error: {construct}: {count} files");
    }
    println!(
        "{parsed} of {} files parse; {} bytes in {:.1} ms",
        files.len(),
        bytes,
        total.as_secs_f64() * 1000.0
    );
    ExitCode::SUCCESS
}

fn parse(source: Str16<'_>) -> Result<Script, ParseError> {
    parse_script(source, &mut RecursionBudget::default())
}

/// The error with its line, column and the source around it.
fn describe(error: &ParseError, source: Str16<'_>) -> String {
    let location = LineIndex::new(source).location(error.offset);
    let offset = error.offset as usize;
    let around = source
        .slice(offset.saturating_sub(30), (offset + 30).min(source.len()))
        .map(|text| text.to_string_lossy().replace('\n', " "))
        .unwrap_or_default();
    let what = match error.unsupported {
        Some(construct) => format!("not supported: {construct}"),
        None => format!("{error}"),
    };
    format!(
        "{what} at {}:{} near `{around}`",
        location.line, location.column
    )
}

/// One block of the generated program; `N` is replaced by a number.
const CHUNK: &str = r"function chunkN(p, q) {
  var total = 0, items = [1, 2.5, 'three', null, true, , { a: p, 'b': q, [p]: q, m() { return this.a; } }];
  let counter = 0;
  const limit = items.length * 2;
  function inner(x) { return x + counter++ - total * limit / 3 % 2; }
  for (let i = 0; i < limit; i++) {
    if (i & 1) { continue; } else if (i >> 3) { break; }
    total += inner(i) ? items[i % items.length] : -i;
    counter = counter >= 10 ? 0 : counter;
  }
  var add = (a, b) => a + b, twice = v => { return add(v, v); };
  while (counter > 0) { counter -= 1; total = total || twice(counter) && !counter; }
  do { total **= 1; } while (total < 0);
  try { if (typeof p === 'undefined' || p instanceof Object) throw new Error('bad'); }
  catch (e) { total = void e; } finally { counter = 0; }
  label: for (var j = 0; j < 3; ++j) { if (j == 2) break label; }
  return { total: total, counter, inner, add, gen: function* () { yield total; yield; } };
}
chunkN(1, 2);
";

/// Parses a generated program of about `megabytes` MB.
fn generated(megabytes: usize) {
    let mut text = String::new();
    let mut n = 0;
    while text.len() < megabytes << 20 {
        text.push_str(&CHUNK.replace('N', &n.to_string()));
        n += 1;
    }
    let source = String16::from(text.as_str());
    let tokens = count_tokens(source.as_str16());
    let mut best = Duration::MAX;
    let mut memory = 0;
    for _ in 0..5 {
        let start = Instant::now();
        let script = parse(source.as_str16()).expect("the generated program parses");
        best = best.min(start.elapsed());
        memory = script.ast.heap_size() + script.scopes.heap_size();
        drop(script);
    }
    let script = parse(source.as_str16()).expect("the generated program parses");
    println!(
        "{} bytes, {} tokens, {} functions, {} expressions, {} scopes, {} bindings, {} references",
        text.len(),
        tokens,
        script.ast.function_count(),
        script.ast.expr_count(),
        script.scopes.scope_count(),
        script.scopes.binding_count(),
        script.scopes.reference_count(),
    );
    println!(
        "parse and scope analysis: best of 5 {:.1} ms ({:.0} MB/s); tree and scope tables {:.1} MB ({:.1} bytes per token); peak RSS {} MB",
        best.as_secs_f64() * 1000.0,
        text.len() as f64 / best.as_secs_f64() / 1e6,
        memory as f64 / 1e6,
        memory as f64 / tokens as f64,
        peak_rss_mb(),
    );
}

/// The number of tokens (the generated program has no regular
/// expression literals or templates, so the `Div` goal is exact).
fn count_tokens(source: Str16<'_>) -> usize {
    let Str16::Latin1(units) = source else {
        return 0;
    };
    let mut lexer = Lexer::new(units, LexerOptions::script(), Interner::new());
    let mut count = 0;
    while lexer
        .next_token(Goal::Div)
        .is_ok_and(|token| token.kind != TokenKind::Eof)
    {
        count += 1;
    }
    count
}

/// The peak resident set size of the process in MB (Linux).
fn peak_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("VmHWM:"))
                .and_then(|line| line.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map_or(0, |kb| kb / 1024)
}
