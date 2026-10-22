//! Hostile cases for the typed arrays (M7 feature 11b): arrays at and
//! above the heap limit, huge lengths and offsets, endless or huge sources
//! against the deadline, loops of large copies and conversions, detaching,
//! shrinking, growing and collecting inside every callback (also in the GC
//! stress mode), an overlapping `set` within one buffer, and the heap
//! accounting. A case ends with a result, an error or a termination; none
//! may run far past the deadline, allocate outside the heap accounting or
//! leave the runtime unusable.

mod common;

use std::time::Duration;

use common::{
    assert_clean, check_cases, eval_within, on_big_stack, run_in, runtime, runtime_with_heap_limit,
};
use swb_js::{Runtime, ScriptError, Termination, Value};

const DEADLINE: Duration = Duration::from_millis(200);

/// The slack after the deadline: the longest single operation (a
/// zero-fill, copy or conversion of 64 MiB, a few hundred milliseconds in
/// the debug build of the tests, charged after the work) plus scheduling
/// noise.
const SLACK: Duration = Duration::from_millis(1500);

/// Runs `source` with the deadline `DEADLINE`; returns the result and the time it took.
fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    eval_within(rt, source, DEADLINE)
}

/// An array that the heap limit does not allow is a `RangeError` with
/// V8's message before any work in proportion to its length; lengths and
/// offsets up to 2^53 − 1 are checked without overflow; the runtime stays
/// usable.
#[test]
fn arrays_above_the_heap_limit_are_range_errors() {
    on_big_stack(|| {
        let mut rt = runtime_with_heap_limit(64, false);
        let catch =
            |body: &str| format!("try {{ {body} }} catch (e) {{ e.name + ': ' + e.message }}");
        for (source, expected) in [
            ("new Uint8Array(40 << 20).length".to_owned(), "=> 41943040"),
            (catch("new Uint8Array(80 << 20)"), "=> RangeError: Array buffer allocation failed"),
            (catch("new Float64Array(10 << 20)"), "=> RangeError: Array buffer allocation failed"),
            (
                catch("new Float64Array(2 ** 33)"),
                "=> RangeError: Invalid typed array length: 8589934592",
            ),
            (
                catch("new Uint8Array(2 ** 53 - 1)"),
                "=> RangeError: Invalid typed array length: 9007199254740991",
            ),
            (
                catch("new Uint8Array({ get length() { return 80 << 20; }, get 0() { throw 1; } })"),
                "=> RangeError: Array buffer allocation failed",
            ),
            (
                catch("Uint8Array.from({ length: 80 << 20 }, function () { throw 1; })"),
                "=> RangeError: Array buffer allocation failed",
            ),
            (
                catch("new Float64Array(new Uint8Array(16 << 20))"),
                "=> RangeError: Array buffer allocation failed",
            ),
            (
                catch("var s = new Uint8Array(40 << 20); s.slice()"),
                "=> RangeError: Array buffer allocation failed",
            ),
            ("s.subarray(1).length".to_owned(), "=> 41943039"),
            (
                catch("s.set(new Uint16Array(s.buffer, 0, 10 << 20))"),
                "=> RangeError: Array buffer allocation failed",
            ),
            ("s.set(new Uint16Array(s.buffer, 0, 1 << 20)); s.length".to_owned(), "=> 41943040"),
            (
                catch("new Uint8Array(new ArrayBuffer(8), 2 ** 53 - 1)"),
                "=> RangeError: Start offset 9007199254740991 is outside the bounds of the buffer",
            ),
            (
                catch("new Float64Array(new ArrayBuffer(8), 0, 2 ** 53 - 1)"),
                "=> RangeError: Invalid typed array length: 9007199254740991",
            ),
            (
                catch("new Uint8Array(4).set([], 2 ** 53 - 1)"),
                "=> RangeError: offset is out of bounds",
            ),
            (
                catch("new BigUint64Array(4).set({ length: 2 ** 53 - 1 })"),
                "=> RangeError: offset is out of bounds",
            ),
            (
                "new Uint8Array(4).subarray(-(2 ** 53), 2 ** 53).length + new Uint8Array(4).slice(-Infinity).length"
                    .to_owned(),
                "=> 8",
            ),
            ("s = undefined; new Uint8Array(48 << 20).length".to_owned(), "=> 50331648"),
        ] {
            assert_eq!(run_in(&mut rt, &source), expected, "{source}");
            assert_clean(&rt);
        }
    });
}

/// Arrays that a script keeps end at the heap limit with a `RangeError`
/// at the allocation, not with the end of the script; then the dropped
/// arrays are given back. Normally and in the GC stress mode.
#[test]
fn kept_arrays_end_at_the_heap_limit() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime_with_heap_limit(64, stress);
            let out = run_in(
                &mut rt,
                "var keep = []; try { for (;;) keep.push(new Float32Array(1 << 20)); } \
                 catch (e) { e.message + ' ' + (keep.length >= 12 && keep.length <= 14) }",
            );
            assert_eq!(
                out, "=> Array buffer allocation failed true",
                "stress {stress}"
            );
            assert_clean(&rt);
            for (source, expected) in [
                (
                    "keep = undefined; new Uint8Array(48 << 20).length",
                    "=> 50331648",
                ),
                (
                    "for (var i = 0; i < 40; i++) { var t = new Uint8Array(16 << 20); t.buffer.transfer(); } t.length",
                    "=> 0",
                ),
                (
                    "var src = new Uint8Array(16 << 20); for (var i = 0; i < 40; i++) src.slice(); src.length",
                    "=> 16777216",
                ),
            ] {
                assert_eq!(run_in(&mut rt, source), expected, "{source}");
                assert_clean(&rt);
            }
        }
    });
}

/// A script that drops its arrays after the `RangeError` of a full heap
/// runs on until the deadline.
#[test]
fn dropped_arrays_after_the_range_error_run_to_the_deadline() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime_with_heap_limit(64, stress);
            let (result, elapsed) = eval_with_deadline(
                &mut rt,
                "var a = []; for (;;) { try { a.push(new Int16Array(1 << 19)); } catch (e) { a.length = 0; } }",
            );
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
                "stress {stress}: {result:?}"
            );
            assert!(elapsed < DEADLINE + SLACK, "stress {stress}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

/// Endless and huge sources end at the deadline (or at the heap limit for
/// a source that grows a list): each element of an array-like or an
/// iterable is a step of the time countdown, and so is each call of the
/// mapping function of `from` and each element of `set` from an
/// array-like.
#[test]
fn endless_sources_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let setup = "var endless = { [Symbol.iterator]() { return { next() { return { done: false, value: 1 }; } }; } }; \
                     var huge = { length: 1e8 }; var big = new Uint8Array(1e8);";
        assert_eq!(run_in(&mut rt, setup), "");
        for source in [
            "new Uint8Array(huge);",
            "new Float64Array(endless);",
            "Uint8Array.from(huge, function (x) { return x; });",
            "Uint8Array.from(endless);",
            "big.set(huge);",
            "new BigInt64Array(new Proxy({}, { get: (t, k) => (typeof k === 'symbol' ? undefined : k === 'length' ? 1e8 : 1n) }));",
            "for (const x of big) {}",
            "for (const x of big.entries()) {}",
            "for (var k in big) {}",
            "[...big];",
            "Object.keys(big);",
            "JSON.stringify(big);",
            "Array.prototype.indexOf.call(big, 1);",
        ] {
            // Each source starts from a collected heap: the garbage of an
            // endless source (millions of iterator results) leaves the
            // slot vector of the objects long, and objects that the next
            // script makes before its first collection sit at its end and
            // keep that length counted; the 800 MB of the `BigInt64Array`
            // then fail as a `RangeError` (backlog, M7 13a).
            rt.collect();
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
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
        assert_eq!(run_in(&mut rt, "big.length"), "=> 100000000");
    });
}

/// Endless loops of large copies, conversions and allocations end at the
/// deadline: the bulk work is charged to the time countdown.
#[test]
fn large_typed_array_work_ends_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let setup = "var big = new Uint8Array(1 << 26); var other = new Uint8Array(1 << 26); \
                     var floats = new Float64Array(1 << 23); var rab = new ArrayBuffer(0, { maxByteLength: 1 << 26 }); \
                     var tracking = new Uint8Array(rab);";
        assert_eq!(run_in(&mut rt, setup), "");
        for source in [
            "for (;;) new Uint8Array(1 << 26);",
            "for (;;) big.slice();",
            "for (;;) big.set(other);",
            "for (;;) big.set(big.subarray(1, 1 << 25), 7);",
            "for (;;) new Uint8Array(big);",
            "for (;;) new Float32Array(floats);",
            "for (;;) new Uint8Array(floats.buffer).set(new Uint16Array(floats.buffer, 0, 1 << 24), 1);",
            "for (;;) big.subarray(1).length;",
            "for (;;) { rab.resize(1 << 26); tracking.slice(); rab.resize(0); }",
            "for (;;) { try { new Uint8Array(2 ** 40); } catch (e) {} }",
            "for (var i = 0; ; i++) big[i & 0xffff] = big[(i + 1) & 0xffff] + 1;",
        ] {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
                "{source}: {result:?}"
            );
            assert!(elapsed < DEADLINE + SLACK, "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "big.length"), "=> 67108864");
    });
}

/// Detaching, shrinking, growing and collecting in every callback of the
/// typed arrays (`data/typed_detach.js`; the expected output is Chromium
/// 148's, from `tools/probes/js-typed.json`, except the KNOWN lines of
/// `gen-typed-cases.js`), normally and in the GC stress mode.
#[test]
fn detaching_in_every_callback() {
    check_cases(&[(
        include_str!("data/typed_detach.js"),
        include_str!("data/typed_detach.expected").trim_end_matches('\n'),
    )]);
}

/// A `slice` whose `start` conversion shrinks a length-tracking array to
/// no element copies nothing into the new array (Chromium 148: a length
/// of 7), for the same element type and for another one (found in the
/// review: the empty copy was an internal error).
#[test]
fn slice_of_an_array_shrunk_to_nothing() {
    check_cases(&[
        (
            "var a = new Uint8Array(new ArrayBuffer(8, { maxByteLength: 16 })); \
             var r = a.slice({ valueOf() { a.buffer.resize(0); return 1; } }); \
             print(r.length + ' ' + r[0] + ' ' + r[6] + ' ' + a.length);",
            "7 0 0 0",
        ),
        (
            "var a = new Float64Array(new ArrayBuffer(64, { maxByteLength: 128 })); \
             var r = a.slice({ valueOf() { a.buffer.resize(0); return 1; } }); \
             print(r.length + ' ' + r[0] + ' ' + r[6] + ' ' + a.length);",
            "7 0 0 0",
        ),
        (
            "var a = new Float32Array(new ArrayBuffer(32, { maxByteLength: 64 })); \
             a.constructor = { [Symbol.species]: function (n) { return new Float64Array(n); } }; \
             var r = a.slice({ valueOf() { a.buffer.resize(0); return 1; } }); \
             print(r.length + ' ' + r[0] + ' ' + a.length);",
            "7 0 0",
        ),
    ]);
}

/// `set` from a typed array of another element type over the same buffer
/// reads a copy of the source bytes; for the same type it moves the bytes
/// (`memmove`); `slice` into a view of the same buffer copies forward byte
/// by byte. Checked against a model written in the script, also in the GC
/// stress mode.
#[test]
fn overlapping_copies_within_one_buffer() {
    check_cases(&[
        (
            "var b = new ArrayBuffer(1 << 16); var u8 = new Uint8Array(b); \
             for (var i = 0; i < u8.length; i++) u8[i] = i * 7 + (i >> 8); \
             var copy = u8.slice(); var u16 = new Uint16Array(b, 2, 20000); u8.set(u16, 1); \
             var ok = true; for (var k = 0; k < 20000; k++) \
             ok = ok && u8[1 + k] === ((copy[2 + 2 * k] | copy[3 + 2 * k] << 8) & 255); \
             for (var k = 20001; k < u8.length; k++) ok = ok && u8[k] === copy[k]; print(ok);",
            "true",
        ),
        (
            "var u8 = new Uint8Array(1 << 16); for (var i = 0; i < u8.length; i++) u8[i] = i; \
             var copy = u8.slice(); u8.set(u8.subarray(0, 40000), 10); var ok = true; \
             for (var k = 0; k < 40000; k++) ok = ok && u8[10 + k] === copy[k]; \
             u8.set(u8.subarray(20, 30000), 3); \
             for (var k = 0; k < 29980; k++) ok = ok && u8[3 + k] === (k + 20 < 10 ? copy[k + 20] : copy[k + 10]); print(ok);",
            "true",
        ),
        (
            "var t = new Uint8Array(64); for (var i = 0; i < 64; i++) t[i] = i; \
             t.constructor = { [Symbol.species]: function (n) { return new Uint8Array(t.buffer, 3, n); } }; \
             t.slice(0, 40); var ok = true; for (var k = 0; k < 43; k++) ok = ok && t[k] === k % 3; print(ok);",
            "true",
        ),
    ]);
}

/// The bytes of a typed array's buffer count in the heap size, a
/// `subarray` adds none, and an array that is dropped gives its buffer
/// back at the next collection; the typed array keeps its buffer alive.
#[test]
fn typed_array_bytes_are_in_the_heap_accounting() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.collect();
        let base = rt.heap().heap_size();
        assert_eq!(run_in(&mut rt, "var t = new Float64Array(2 << 20);"), "");
        let with_array = rt.heap().heap_size();
        assert!(with_array >= base + (16 << 20), "{base} {with_array}");
        assert_eq!(
            run_in(
                &mut rt,
                "var views = []; for (var i = 0; i < 1000; i++) views.push(t.subarray(i)); void 0"
            ),
            ""
        );
        let with_views = rt.heap().heap_size();
        assert!(
            with_views < with_array + (1 << 20),
            "{with_array} {with_views}"
        );
        assert_eq!(
            run_in(
                &mut rt,
                "var keep = new Uint8Array(new ArrayBuffer(8)); views = undefined; t = undefined;"
            ),
            ""
        );
        rt.collect();
        let dropped = rt.heap().heap_size();
        assert!(dropped < base + (1 << 20), "{base} {dropped}");
        assert_eq!(
            run_in(&mut rt, "keep[7] = 9; keep.buffer.byteLength + keep[7]"),
            "=> 17"
        );
    });
}
