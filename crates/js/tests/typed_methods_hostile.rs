//! Hostile cases for the methods of `%TypedArray%.prototype`, `Atomics`
//! and the base64 and hex methods of `Uint8Array` (M7 feature 11c): loops
//! over arrays of 64 MiB against the deadline (the default sort, `join`,
//! `toBase64`, the searches, `fill`, `reverse` and the callback methods),
//! arrays and strings at the heap limit (a `RangeError`, or the end of
//! the script for a string), malformed and huge base64 and hex input,
//! and detaching, shrinking, growing and collecting inside every callback
//! of the methods (also in the GC stress mode). A case ends with a result,
//! an error or a termination; none may run far past the deadline, allocate
//! outside the heap accounting or leave the runtime unusable.

mod common;

use std::time::{Duration, Instant};

use common::{check_cases, on_big_stack, run_in, runtime_with_tests};
use swb_js::{HeapConfig, Runtime, RuntimeConfig, ScriptError, Termination, Value};

const DEADLINE: Duration = Duration::from_millis(200);

/// The slack after the deadline: the longest single operation (the sort of
/// a run, a copy or a conversion of 64 MiB, a few hundred milliseconds in
/// the debug build of the tests, charged after the work) plus scheduling
/// noise.
const SLACK: Duration = Duration::from_millis(1500);

/// Runs `source` with a deadline; returns the result and the time it took.
fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + DEADLINE));
    let result = rt.eval(source);
    rt.set_deadline(None);
    (result, start.elapsed())
}

fn assert_clean(rt: &Runtime) {
    assert_eq!(rt.stack_depths(), (0, 0));
    assert_eq!(rt.heap().scope_depth(), 0);
    assert_eq!(rt.heap().no_gc_depth(), 0);
    assert_eq!(rt.heap().stats().total_stale_roots, 0);
}

/// A runtime with a heap limit of `mib` MiB and the test functions.
fn limited(mib: usize, stress: bool) -> Runtime {
    runtime_with_tests(RuntimeConfig {
        heap: HeapConfig {
            limit: mib << 20,
            stress,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    })
}

/// A `try` around `body` that gives the name and the message of the error.
fn catch(body: &str) -> String {
    format!("try {{ {body} }} catch (e) {{ e.name + ': ' + e.message }}")
}

/// Detaching, shrinking, growing and collecting in every callback of the
/// methods (`data/typed_methods_detach.js`; the expected output is
/// Chromium 148's, from `tools/probes/js-typed.json`, except the KNOWN
/// lines of `gen-typed-cases.js`), normally and in the GC stress mode.
#[test]
fn detaching_in_every_method_callback() {
    check_cases(&[(
        include_str!("data/typed_methods_detach.js"),
        include_str!("data/typed_methods_detach.expected").trim_end_matches('\n'),
    )]);
}

/// Every loop that a method runs over a large array ends at the deadline:
/// the native loops charge the time countdown in chunks, and the loops
/// with callbacks take a step per element.
#[test]
fn loops_over_large_arrays_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = limited(768, false);
        let setup = "var big = new Uint8Array(1 << 26); var floats = new Float64Array(1 << 23); \
                     var ints = new Int32Array(1 << 24); var bigs = new BigInt64Array(1 << 22); \
                     var text = 'AAAA'.repeat(1 << 22); var hex = '00'.repeat(1 << 23); \
                     var target = new Uint8Array(1 << 26); \
                     function fill() { for (var i = 0; i < floats.length; i++) floats[i] = (i * 7919) % 1000003; \
                     for (var i = 0; i < ints.length; i += 7) ints[i] = i; } var unused = fill();";
        assert_eq!(run_in(&mut rt, setup), "");
        for source in [
            "big.fill(1)",
            "big.reverse()",
            "big.copyWithin(1, 0)",
            "big.indexOf(7)",
            "big.lastIndexOf(7)",
            "big.includes(7)",
            "floats.indexOf(-1)",
            "bigs.includes(5n)",
            "big.sort()",
            "big.toSorted()",
            "ints.sort()",
            "floats.sort()",
            "floats.toSorted()",
            "bigs.sort()",
            "big.toReversed()",
            "big.with(1, 2)",
            "big.join()",
            "floats.join('-')",
            "big.toLocaleString()",
            "big.toBase64()",
            "big.toBase64({ alphabet: 'base64url', omitPadding: true })",
            "big.toHex()",
            "Uint8Array.fromBase64(text)",
            "Uint8Array.fromHex(hex)",
            "target.setFromBase64(text)",
            "target.setFromHex(hex)",
            "big.every(function (x) { return true; })",
            "big.some(function (x) { return false; })",
            "big.forEach(function (x) {})",
            "big.find(function (x) { return false; })",
            "big.findLastIndex(function (x) { return false; })",
            "big.map(function (x) { return x; })",
            "big.filter(function (x) { return true; })",
            "big.reduce(function (a, x) { return a + x; }, 0)",
            "big.reduceRight(function (a, x) { return a; }, 0)",
            "floats.sort(function (a, b) { return a - b; })",
            "floats.toSorted(function (a, b) { return b - a; })",
            "Atomics.add(ints, 1, 1)",
            "Atomics.compareExchange(bigs, 1, 0n, 0n)",
        ] {
            // An allocation that the heap limit refuses is a `RangeError`,
            // which the loop catches and goes on.
            let looped = format!("for (;;) {{ try {{ {source}; }} catch (e) {{}} }}");
            let (result, elapsed) = eval_with_deadline(&mut rt, &looped);
            assert!(
                matches!(
                    result,
                    Err(ScriptError::Terminated(
                        Termination::TimeLimit | Termination::HeapLimit
                    ))
                ),
                "{source}: {result:?}"
            );
            assert!(elapsed < DEADLINE + SLACK, "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "big.length + floats.length"), "=> 75497472");
    });
}

/// One call of a method over a very large array also ends at the deadline
/// (not after the whole loop): the chunks of the work charge the
/// countdown.
#[test]
fn single_calls_over_large_arrays_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = limited(768, false);
        let setup = "var big = new Uint8Array(3 << 24); var floats = new Float64Array(1 << 24); \
                     function fill() { for (var i = 0; i < floats.length; i++) floats[i] = (i * 7919) % 1000003; } \
                     var unused = fill();";
        assert_eq!(run_in(&mut rt, setup), "");
        for source in [
            "floats.sort();",
            "floats.sort(function (a, b) { return a - b; });",
            "floats.toLocaleString();",
            "floats.join();",
        ] {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            // A call that finishes before the deadline is fine, too.
            assert!(
                matches!(
                    result,
                    Ok(_)
                        | Err(ScriptError::Terminated(
                            Termination::TimeLimit | Termination::HeapLimit
                        ))
                ),
                "{source}: {result:?}"
            );
            assert!(elapsed < DEADLINE + SLACK, "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

/// Arrays, scratch memory and strings that the heap limit does not allow:
/// a new array, the memory of a sort, the list of `filter`, the output
/// of the decoders and the encodings are a `RangeError` (a string that
/// `join` would build ends the script at the heap limit, as
/// `Array.prototype.join` does); a text above the string limit is V8's
/// `RangeError`; the runtime stays usable.
#[test]
fn allocations_above_the_heap_limit_are_range_errors() {
    on_big_stack(|| {
        let range = "=> RangeError: Array buffer allocation failed";
        // An array of 36 MiB: no second array, no list, no text fits.
        let mut rt = limited(64, false);
        assert_eq!(
            run_in(
                &mut rt,
                "var big = new Uint8Array(36 << 20); var tiny = new Uint8Array(1000);"
            ),
            ""
        );
        for (source, expected) in [
            (catch("big.map(function (x) { return x; })"), range),
            (catch("big.toReversed()"), range),
            (catch("big.toSorted()"), range),
            (catch("big.with(0, 1)"), range),
            (catch("big.slice()"), range),
            (catch("big.filter(function (x) { return true; })"), range),
            (catch("big.sort(function (a, b) { return a - b; })"), range),
            (catch("big.toBase64()"), range),
            (catch("big.toHex()"), range),
            (
                catch("tiny.join('x'.repeat(1 << 20))"),
                "=> RangeError: Invalid string length",
            ),
            (
                catch("new Uint8Array(1 << 20).join('x'.repeat(1 << 20))"),
                "=> RangeError: Invalid string length",
            ),
            ("tiny.toBase64().length".to_owned(), "=> 1336"),
            (
                "Uint8Array.fromBase64(tiny.toBase64()).length".to_owned(),
                "=> 1000",
            ),
            ("tiny.join().length".to_owned(), "=> 1999"),
        ] {
            assert_eq!(run_in(&mut rt, &source), expected, "{source}");
            assert_clean(&rt);
        }
        // The text of `join` that the heap cannot hold ends the script.
        for source in ["big.join();", "big.toLocaleString();"] {
            let result = rt.eval(source);
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::HeapLimit))),
                "{source}: {result:?}"
            );
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "big.length"), "=> 37748736");
        // The scratch memory of a sort: 16 bytes per eight-byte element.
        let mut rt = limited(64, false);
        assert_eq!(
            run_in(&mut rt, "var floats = new Float64Array(3 << 20);"),
            ""
        );
        for source in [
            "floats.sort()",
            "floats.sort(function (a, b) { return a - b; })",
        ] {
            assert_eq!(run_in(&mut rt, &catch(source)), range, "{source}");
            assert_clean(&rt);
        }
        assert_eq!(
            run_in(&mut rt, "new Float64Array(1 << 20).sort().length"),
            "=> 1048576"
        );
        // The output of the decoders.
        for (setup, source, length) in [
            (
                "var s = 'AAAA'.repeat(12 << 20);",
                "Uint8Array.fromBase64(s)",
                "=> 50331648",
            ),
            (
                "var s = '00'.repeat(25 << 20);",
                "Uint8Array.fromHex(s)",
                "=> 52428800",
            ),
        ] {
            let mut rt = limited(64, false);
            assert_eq!(run_in(&mut rt, setup), "");
            assert_eq!(run_in(&mut rt, &catch(source)), range, "{source}");
            assert_clean(&rt);
            assert_eq!(run_in(&mut rt, "s.length"), length);
        }
    });
}

/// The memory follows the data: a default sort needs the keys and the
/// merge buffer but no list for the NaNs, a counting sort lists only the
/// NaNs it finds, and a decoder grows its output with the decoded bytes
/// and not with the length of the string.
#[test]
fn memory_follows_the_data() {
    on_big_stack(|| {
        // 8 MiB of floats: keys and merge buffer (16 MiB) fit under 90 % of
        // 32 MiB; a NaN list of 8 bytes per element (8 MiB more) did not.
        let mut rt = limited(32, false);
        assert_eq!(
            run_in(
                &mut rt,
                "var f = new Float64Array(1 << 20); \
                 for (var i = 0; i < f.length; i += 3) f[i] = NaN; \
                 f[1] = -1; f[4] = 5;"
            ),
            "=> 5"
        );
        assert_eq!(
            run_in(
                &mut rt,
                "f.sort(); var n = 0; while (f[f.length - 1 - n] !== f[f.length - 1 - n]) n++; \
                 [f[0], f[1], n].join()"
            ),
            "=> -1,0,349526"
        );
        assert_clean(&rt);
        // 6 MiB of Float16 with no NaN: a counting sort needs no list.
        assert_eq!(
            run_in(&mut rt, "new Float16Array(3 << 20).fill(2).sort().length"),
            "=> 3145728"
        );
        // 20 MiB of whitespace or bad characters: the output stays empty.
        for (source, expected) in [
            (
                "Uint8Array.fromBase64(' '.repeat(20 << 20)).length".to_owned(),
                "=> 0",
            ),
            (
                catch("Uint8Array.fromBase64('!'.repeat(20 << 20))"),
                "=> SyntaxError: Found a character that cannot be part of a valid base64 string.",
            ),
            (
                catch("Uint8Array.fromHex('zz'.repeat(10 << 20))"),
                "=> SyntaxError: Input string must contain hex characters in even length",
            ),
        ] {
            let mut rt = limited(32, false);
            assert_eq!(run_in(&mut rt, &source), expected, "{source}");
            assert_clean(&rt);
        }
    });
}

/// Malformed, huge and odd input of the base64 and hex decoders: a
/// `SyntaxError`, a `TypeError` or a result, never a panic, and the time
/// is bounded.
#[test]
fn malformed_base64_and_hex_input() {
    on_big_stack(|| {
        let mut rt = limited(256, false);
        let setup = "var spaces = ' '.repeat(1 << 24); var many = 'A'.repeat(1 << 24); \
                     var t = new Uint8Array(1 << 20);";
        assert_eq!(run_in(&mut rt, setup), "");
        let syntax =
            "=> SyntaxError: Found a character that cannot be part of a valid base64 string.";
        for (source, expected) in [
            (catch("Uint8Array.fromBase64(spaces + '$')"), syntax),
            (catch("Uint8Array.fromBase64(many + '$')"), syntax),
            // The target is full before the error: no error (FromBase64 returns).
            (
                catch("typeof t.setFromBase64(many + '$').read"),
                "=> number",
            ),
            (catch("t.setFromBase64(many.slice(0, 100) + '$')"), syntax),
            (
                catch("Uint8Array.fromBase64(many, { lastChunkHandling: 'strict' }).length"),
                "=> 12582912",
            ),
            (catch("Uint8Array.fromBase64(spaces).length"), "=> 0"),
            (
                catch(
                    "Uint8Array.fromBase64(spaces + 'A' + spaces, { lastChunkHandling: 'strict' })",
                ),
                "=> SyntaxError: The base64 input terminates with a single character, excluding padding (=).",
            ),
            (
                catch("Uint8Array.fromHex('0'.repeat((1 << 24) + 1))"),
                "=> SyntaxError: Input string must contain hex characters in even length",
            ),
            (
                catch("Uint8Array.fromHex('0'.repeat(1 << 24) + 'zz')"),
                "=> SyntaxError: Input string must contain hex characters in even length",
            ),
            // The target is full before the bad pair: no error.
            (
                catch("t.setFromHex('00'.repeat(1 << 22) + 'zz').written"),
                "=> 1048576",
            ),
            (
                catch("new Uint8Array(1 << 23).setFromHex('00'.repeat(1 << 22) + 'zz').written"),
                "=> SyntaxError: Input string must contain hex characters in even length",
            ),
            (
                catch("t.setFromHex('01'.repeat(1 << 22)).written"),
                "=> 1048576",
            ),
            (catch("t.setFromBase64(many).read"), "=> 1398100"),
            (
                catch("t.setFromBase64(spaces + 'AA==' + spaces).read"),
                "=> 33554436",
            ),
        ] {
            assert_eq!(run_in(&mut rt, &source), expected, "{source}");
            assert_clean(&rt);
        }
        // The bytes decoded before the error are in the array.
        assert_eq!(
            run_in(
                &mut rt,
                "var u = new Uint8Array(8); try { u.setFromBase64('AQIDBAUG$$$$'); } catch (e) {} u.join()"
            ),
            "=> 1,2,3,4,5,6,0,0"
        );
        assert_eq!(
            run_in(
                &mut rt,
                "var u = new Uint8Array(8); try { u.setFromHex('010203zz'); } catch (e) {} u.join()"
            ),
            "=> 1,2,3,0,0,0,0,0"
        );
    });
}

/// Callbacks and comparators that collect, throw or change the array in
/// every step, with the GC stress mode on and the heap small: the values
/// that the methods hold across the calls are rooted.
#[test]
fn collections_inside_callbacks_and_comparators() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = limited(64, stress);
            for (source, expected) in [
                (
                    "var t = new BigInt64Array([5n, 3n, 9n, 1n, 7n]); \
                     t.sort(function (a, b) { gc(); return a < b ? -1 : 1; }).join()",
                    "=> 1,3,5,7,9",
                ),
                (
                    "var t = new BigUint64Array([5n, 3n, 9n, 1n, 7n]); \
                     t.toSorted(function (a, b) { gc(); return a > b ? -1 : 1; }).join()",
                    "=> 9,7,5,3,1",
                ),
                (
                    "var t = new BigInt64Array([5n, 3n, 9n, 1n, 7n]); \
                     t.map(function (x) { gc(); return x * 2n; }).join()",
                    "=> 10,6,18,2,14",
                ),
                (
                    "var t = new BigInt64Array([5n, 3n, 9n, 1n, 7n]); \
                     t.filter(function (x) { gc(); return x > 3n; }).join()",
                    "=> 5,9,7",
                ),
                (
                    "var t = new BigInt64Array([5n, 3n, 9n, 1n, 7n]); \
                     t.reduce(function (a, x) { gc(); return a + x; }, 0n)",
                    "=> 25",
                ),
                (
                    "var t = new BigInt64Array([5n, 3n, 9n, 1n, 7n]); \
                     t.find(function (x) { gc(); return x === 9n; })",
                    "=> 9",
                ),
                (
                    "var t = new BigInt64Array([5n, 3n, 9n, 1n, 7n]); \
                     t.findLast(function (x) { gc(); return x < 9n; })",
                    "=> 7",
                ),
                (
                    "var t = new BigInt64Array([5n, 3n]); var old = BigInt.prototype.toLocaleString; \
                     BigInt.prototype.toLocaleString = function () { gc(); return 'L' + this; }; \
                     try { t.toLocaleString() } finally { BigInt.prototype.toLocaleString = old; }",
                    "=> L5,L3",
                ),
                (
                    "var t = new Float64Array([5, 3, 9, 1, 7]); \
                     t.with({ valueOf() { gc(); return 1; } }, { valueOf() { gc(); return 4; } }).join()",
                    "=> 5,4,9,1,7",
                ),
                (
                    "Atomics.add(new BigInt64Array(2), 1, { valueOf() { gc(); return 3n; } })",
                    "=> 0",
                ),
                (
                    "var s = 'x'.repeat(100); var u = new Uint8Array(100); \
                     u.setFromBase64('QUJD'.repeat(25), { get alphabet() { gc(); return 'base64'; } }).written",
                    "=> 75",
                ),
            ] {
                assert_eq!(
                    run_in(&mut rt, source),
                    expected,
                    "stress {stress}: {source}"
                );
                assert_clean(&rt);
            }
        }
    });
}
