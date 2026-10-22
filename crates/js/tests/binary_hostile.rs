//! Hostile cases for `ArrayBuffer` and `DataView` (M7 feature 11a):
//! buffers at and above the heap limit, loops of large allocations,
//! copies, transfers and resizes against the deadline, detaching,
//! shrinking and collecting inside every conversion callback (also in the
//! GC stress mode), and the heap accounting of the bytes outside the
//! arenas. A case ends with a result, an error or a termination; none may
//! run far past the deadline, allocate outside the heap accounting or leave
//! the runtime unusable.

mod common;

use std::time::Duration;

use common::{
    assert_clean, check_cases, eval_within, on_big_stack, run_in, runtime, runtime_with_heap_limit,
};
use swb_js::{Runtime, ScriptError, Termination, Value};

const DEADLINE: Duration = Duration::from_millis(200);

/// The slack after the deadline: the longest single operation (a
/// zero-fill or copy of 64 MiB, a few hundred milliseconds in the debug
/// build of the tests, charged after the work) plus scheduling noise.
const SLACK: Duration = Duration::from_millis(1500);

/// Runs `source` with the deadline `DEADLINE`; returns the result and the time it took.
fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    eval_within(rt, source, DEADLINE)
}

/// A buffer that the heap limit does not allow is a `RangeError` with
/// V8's message, before any work in proportion to its length; the
/// runtime stays usable.
#[test]
fn buffers_above_the_heap_limit_are_range_errors() {
    on_big_stack(|| {
        let mut rt = runtime_with_heap_limit(64, false);
        for (source, expected) in [
            ("new ArrayBuffer(40 << 20).byteLength", "=> 41943040"),
            (
                "try { new ArrayBuffer(80 << 20) } catch (e) { e.name + ': ' + e.message }",
                "=> RangeError: Array buffer allocation failed",
            ),
            (
                "try { new ArrayBuffer(2 ** 34) } catch (e) { e.message }",
                "=> Array buffer allocation failed",
            ),
            (
                "try { new ArrayBuffer(2 ** 35) } catch (e) { e.message }",
                "=> Invalid array buffer length",
            ),
            (
                "var r = new ArrayBuffer(0, { maxByteLength: 2 ** 33 }); \
                 try { r.resize(2 ** 32) } catch (e) { e.message + ' ' + r.byteLength }",
                "=> Array buffer allocation failed 0",
            ),
            (
                "var t = new ArrayBuffer(8); try { t.transfer(1 << 30) } catch (e) { e.message + ' ' + t.detached }",
                "=> Array buffer allocation failed false",
            ),
            (
                "var s = new ArrayBuffer(40 << 20); try { s.slice() } catch (e) { e.message }",
                "=> Array buffer allocation failed",
            ),
            (
                "s = undefined; new ArrayBuffer(40 << 20).byteLength",
                "=> 41943040",
            ),
        ] {
            assert_eq!(run_in(&mut rt, source), expected, "{source}");
            assert_clean(&rt);
        }
    });
}

/// Buffers that a script keeps end at the heap limit with a `RangeError`
/// at the allocation (the room for buffers ends at 90 % of the limit,
/// where a collection would end the script); garbage buffers, detached
/// buffers and slices are given back.
#[test]
fn kept_buffers_end_at_the_heap_limit() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime_with_heap_limit(64, stress);
            assert_eq!(
                run_in(
                    &mut rt,
                    "var keep = []; try { for (;;) keep.push(new ArrayBuffer(4 << 20)); } \
                     catch (e) { e.message + ' ' + keep.length }",
                ),
                "=> Array buffer allocation failed 14",
                "stress {stress}"
            );
            assert_clean(&rt);
            for (source, expected) in [
                (
                    "keep = undefined; new ArrayBuffer(48 << 20).byteLength",
                    "=> 50331648",
                ),
                (
                    "for (var i = 0; i < 50; i++) { var b = new ArrayBuffer(32 << 20); b.transfer(0); } b.detached",
                    "=> true",
                ),
                (
                    "var src = new ArrayBuffer(16 << 20); for (var i = 0; i < 50; i++) src.slice(); src.byteLength",
                    "=> 16777216",
                ),
                (
                    "var g = new ArrayBuffer(0, { maxByteLength: 32 << 20 }); \
                     for (var i = 0; i < 20; i++) { g.resize(32 << 20); g.resize(0); } g.byteLength",
                    "=> 0",
                ),
            ] {
                assert_eq!(run_in(&mut rt, source), expected, "{source}");
                assert_clean(&rt);
            }
        }
    });
}

/// A script that drops its buffers after the `RangeError` of a full heap
/// runs on until the deadline; the heap limit does not end it.
#[test]
fn dropped_buffers_after_the_range_error_run_to_the_deadline() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime_with_heap_limit(64, stress);
            let (result, elapsed) = eval_with_deadline(
                &mut rt,
                "var a = []; for (;;) { try { a.push(new ArrayBuffer(1 << 20)); } catch (e) { a.length = 0; } }",
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

/// Endless loops of large allocations, copies, transfers and resizes end
/// at the deadline: the zero-fill and the copies are charged to the time
/// countdown.
#[test]
fn large_buffer_work_ends_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let setup = "var big = new ArrayBuffer(1 << 26); \
                     var rab = new ArrayBuffer(0, { maxByteLength: 1 << 26 }); \
                     var view = new DataView(new ArrayBuffer(8));";
        assert_eq!(run_in(&mut rt, setup), "");
        for source in [
            "for (;;) new ArrayBuffer(1 << 26);",
            "for (;;) big.slice();",
            "for (;;) big.slice(1, -1).byteLength;",
            "var t = new ArrayBuffer(1 << 26); for (;;) t = t.transfer(t.byteLength === 1 << 26 ? 1 << 20 : 1 << 26);",
            "for (;;) { rab.resize(1 << 26); rab.resize(0); }",
            "for (;;) new DataView(big, 1 << 20).byteLength;",
            "for (;;) view.setFloat64(0, view.getFloat64(0) + 1);",
            "for (;;) { try { new ArrayBuffer(2 ** 40); } catch (e) {} }",
        ] {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
                "{source}: {result:?}"
            );
            assert!(elapsed < DEADLINE + SLACK, "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "big.byteLength"), "=> 67108864");
    });
}

/// Detaching, shrinking, growing and collecting in every conversion
/// callback of the `DataView` methods and constructor and of the
/// `ArrayBuffer` methods (`data/binary_detach.js`, expected output from
/// Node.js 22), normally and in the GC stress mode.
#[test]
fn detaching_in_every_conversion() {
    check_cases(&[(
        include_str!("data/binary_detach.js"),
        include_str!("data/binary_detach.expected").trim_end_matches('\n'),
    )]);
}

/// The bytes outside the arenas count in the heap size: a new buffer adds
/// its length at once, a detached buffer gives it back at once, and a
/// collection measures the live buffers.
#[test]
fn buffer_bytes_are_in_the_heap_accounting() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.collect();
        let base = rt.heap().heap_size();
        assert_eq!(run_in(&mut rt, "var b = new ArrayBuffer(16 << 20);"), "");
        let with_buffer = rt.heap().heap_size();
        assert!(with_buffer >= base + (16 << 20), "{base} {with_buffer}");
        rt.collect();
        assert!(rt.heap().heap_size() >= base + (16 << 20));
        assert_eq!(
            run_in(&mut rt, "var c = b.transfer(); c.byteLength"),
            "=> 16777216"
        );
        let moved = rt.heap().heap_size();
        assert!(
            moved < base + (17 << 20),
            "a transfer moves the bytes: {moved}"
        );
        assert_eq!(run_in(&mut rt, "c.transfer(0).byteLength"), "=> 0");
        let detached = rt.heap().heap_size();
        assert!(detached < base + (1 << 20), "{base} {detached}");
        rt.collect();
        assert!(rt.heap().heap_size() < base + (1 << 20));
    });
}
