//! Hostile cases for the `RegExp` built-in (M7 feature 8b): global
//! `replace`, `split` and `match` on strings of ten million code units,
//! `matchAll` and `replace` with empty matches, catastrophic backtracking
//! at the deadline and at a termination request, backtrack stacks at the
//! heap limit and beyond V8's bound, collections between the slices of a
//! match, and `valueOf` of `lastIndex` that recompiles the pattern. A case
//! ends with a result, an error or a termination; none may run in
//! quadratic time, allocate outside the heap accounting in proportion to
//! the script's data, or run past the deadline.

mod common;

use std::fmt::Write;
use std::time::{Duration, Instant};

use common::{assert_clean, eval_within, on_big_stack, run, run_in, runtime, runtime_with_tests};
use swb_js::{HeapConfig, Runtime, RuntimeConfig, ScriptError, Termination, Value};

const DEADLINE: Duration = Duration::from_millis(200);

/// Runs `source` with the deadline `DEADLINE`; returns the result and the time it took.
fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    eval_within(rt, source, DEADLINE)
}

/// Ten million code units: the global loops are linear (a debug build of
/// the tests takes a few seconds for each).
#[test]
fn global_loops_over_ten_million_units() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            (
                "var s = 'ab,'.repeat(3333333) + 'x'; s.replace(/,/g, ';').length",
                "=> 10000000",
            ),
            (
                "var s = 'ab,'.repeat(3333333) + 'x'; s.replace(/(b),/g, '$1;').length",
                "=> 10000000",
            ),
            (
                "var s = 'ab,'.repeat(3333333) + 'x'; s.split(/,/).length",
                "=> 3333334",
            ),
            (
                "var s = 'ab,'.repeat(3333333) + 'x'; s.split(/(,)/, 1000).length",
                "=> 1000",
            ),
            (
                "var s = 'ab,'.repeat(3333333) + 'x'; s.match(/b/g).length",
                "=> 3333333",
            ),
            (
                "var s = 'x'.repeat(1e7); s.replace(/(?:)/g, '').length",
                "=> 10000000",
            ),
            // Two million pieces of one unit (each is a string, as in
            // `split('')`).
            (
                "var s = 'x'.repeat(2e6); s.split(/(?:)/).length",
                "=> 2000000",
            ),
            (
                "var s = 'x'.repeat(1e7); s.replace(/x*$/, 'y').length",
                "=> 1",
            ),
            (
                "var s = '\\u{1F600}'.repeat(2e6); s.replace(/(?:)/gu, '-').length",
                "=> 6000001",
            ),
        ];
        for (source, expected) in cases {
            let start = Instant::now();
            assert_eq!(run_in(&mut rt, source), expected, "{source}");
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "{source}: {:?}",
                start.elapsed()
            );
            assert_clean(&rt);
        }
    });
}

/// `matchAll` and a replacer function over a million empty matches.
#[test]
fn empty_matches_advance() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            (
                "var n = 0; for (var m of 'x'.repeat(1e6).matchAll(/(?:)/g)) n++; n",
                "=> 1000001",
            ),
            (
                "var n = 0; for (var m of '\\u{1F600}'.repeat(3e5).matchAll(/(?:)/gu)) n++; n",
                "=> 300001",
            ),
            (
                "var n = 0; 'x'.repeat(1e6).replace(/(?:)/g, function () { n++; return ''; }); n",
                "=> 1000001",
            ),
            (
                "var r = /(?:)/g; var it = 'abc'.matchAll(r); r.lastIndex = 99; [...it].length",
                "=> 4",
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(run_in(&mut rt, source), expected, "{source}");
            assert_clean(&rt);
        }
    });
}

/// Catastrophic backtracking ends at the deadline: the matcher runs in
/// slices with the time check between them, also inside the loops of the
/// symbol methods.
#[test]
fn catastrophic_backtracking_ends_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            "/(a*)*b/.test('a'.repeat(40))",
            "/(a|aa)+$/.exec('a'.repeat(60) + 'b')",
            "'a'.repeat(40).replace(/(a*)*b/g, 'x')",
            "'a'.repeat(40).split(/(a*)*b/)",
            "[...'a'.repeat(40).matchAll(/(a*)*b/g)]",
            "'a'.repeat(40).search(/(?:a+)+b/)",
            "while (true) /(?:)/g.exec('x')",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(2), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "/(a*)*b/.test('aab')"), "=> true");
    });
}

/// A termination request from another thread ends a long match.
#[test]
fn a_termination_request_ends_a_match() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let handle = rt.termination_handle();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            handle.terminate();
        });
        let start = Instant::now();
        let result = rt.eval("/(a*)*b/.test('a'.repeat(50))");
        thread.join().unwrap();
        assert_eq!(
            result,
            Err(ScriptError::Terminated(Termination::HostRequest))
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_clean(&rt);
    });
}

/// The backtrack stack stops at V8's `RangeError` beyond its bound, and at
/// the room that the heap limit leaves (after a collection); the runtime
/// stays usable.
#[test]
fn backtrack_stacks_at_the_limits() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var e; try { /^(?:a|b)*c/.exec('a'.repeat(2e7)); } catch (x) { e = x; } \
                      e instanceof RangeError ? e.message : 'no error'";
        assert_eq!(
            run_in(&mut rt, source),
            "=> Maximum call stack size exceeded"
        );
        assert_clean(&rt);
        // A small heap limit: the stack fits only after the garbage is
        // collected, or not at all.
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 96 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with_tests(config);
        let garbage =
            "for (var i = 0; i < 200000; i++) { var g = { i: i, s: 'x' + i }; } undefined";
        assert_eq!(run_in(&mut rt, garbage), "");
        let fits = "/^(?:a|b)*c/.exec('a'.repeat(5e5))";
        assert_eq!(run_in(&mut rt, fits), "=> null");
        let too_big =
            "var e; try { /^(?:a|b)*c/.exec('a'.repeat(5e6)); } catch (x) { e = x; } e.message";
        assert_eq!(
            run_in(&mut rt, too_big),
            "=> Maximum call stack size exceeded"
        );
        assert_clean(&rt);
        assert_eq!(run_in(&mut rt, "/a+/.exec('caab')[0]"), "=> aa");
    });
}

/// The results that a replacer function needs are kept in vectors charged
/// to the heap: a huge number of matches ends at the heap limit.
#[test]
fn collected_matches_count_against_the_heap_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 64 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with_tests(config);
        let result = rt.eval(
            "var s = '(ab)'.repeat(5e6); s.replace(/(\\()(a)(b)(\\))/g, function () { return ''; })",
        );
        assert!(
            matches!(
                result,
                Err(ScriptError::Terminated(Termination::HeapLimit) | ScriptError::Uncaught { .. })
            ),
            "{result:?}"
        );
        assert_clean(&rt);
        assert_eq!(run_in(&mut rt, "'abc'.replace(/b/, () => 'x')"), "=> axc");
    });
}

/// Collections between the slices of a match change nothing: long matches
/// in the stress mode (a collection at every safepoint, also between the
/// slices).
#[test]
fn collections_between_slices() {
    let source = "var s = 'ab'.repeat(200000); \
                  var a = /^(?:a|b)*$/.exec(s)[0].length; \
                  var b = /^((?:a|b)*?)(?<end>bab)?$/.exec(s + 'bab'); \
                  var c = s.replace(/(a)(b)/g, '$2$1').slice(0, 6); \
                  var d = s.split(/(?=a)/).length; \
                  print(a, b[1].length, b.groups.end, c, d);";
    for stress in [false, true] {
        assert_eq!(run(source, stress), "400000 400000 bab bababa 200000");
    }
}

/// `lastIndex` is read before the program and the flags: a `valueOf` that
/// calls `compile` changes them for the running `exec`.
#[test]
fn value_of_of_last_index_recompiles() {
    let source = "var r = /a/g; r.lastIndex = { valueOf() { r.compile('b', 'y'); return 1; } }; \
                  var m = r.exec('abb'); print(m && m.index, r.lastIndex, r.flags, r.source); \
                  var q = /x/g; q.lastIndex = { valueOf() { q.compile('(y)'); return 5; } }; \
                  print(q.exec('zy')[1], q.lastIndex);";
    // `compile` wrote 0 to `lastIndex`; the non-global `exec` writes nothing.
    for stress in [false, true] {
        assert_eq!(run(source, stress), "1 2 y b\ny 0");
    }
}

/// Many distinct literals in one script compile at their first
/// evaluation; a literal in a hot loop compiles once.
#[test]
fn many_literals() {
    on_big_stack(|| {
        let mut source = String::from("var n = 0;\n");
        for i in 0..5000 {
            writeln!(source, "if (/x{i}(y)?/.test('x{i}')) n++;").unwrap();
        }
        source.push_str("for (var i = 0; i < 100000; i++) if (/(?:z|q)+$/.test('qz')) n++;\nn");
        let mut rt = runtime(false);
        assert_eq!(run_in(&mut rt, &source), "=> 105000");
        assert_clean(&rt);
    });
}
