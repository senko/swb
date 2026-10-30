//! Hostile cases for the fast paths of M8 feature 11a (ADR 0026 section
//! 16): the cache of `split` must not keep large strings that the script
//! dropped (it keeps only short subjects), the short path of `split` and
//! the one-step `concat` and `filter(Boolean)` stop at their bounds and
//! hand over to the general steps, and loops of element reads past the end
//! end at the deadline. A case ends with a result or a termination at the
//! deadline or the heap limit; none may run past the deadline or end at
//! the heap limit with little live data.

mod common;

use std::time::Duration;

use common::{assert_clean, eval_within, is_limit, on_big_stack, run_in, runtime_with_heap_limit};
use swb_js::{ScriptError, Termination, Value};

const DEADLINE: Duration = Duration::from_millis(200);

/// Splits 600 distinct subjects of 256 KiB twice each (the second split
/// would fill the cache): with a heap limit of 64 MiB, the subjects that
/// the script drops are collected, so the loop ends with its result (a
/// cache of 256 such subjects and their pieces would hold 128 MiB).
#[test]
fn split_cache_keeps_no_large_subjects() {
    on_big_stack(|| {
        let mut rt = runtime_with_heap_limit(64, false);
        let out = run_in(
            &mut rt,
            "var base = 'x'.repeat(131072) + '-'; var n = 0;
             for (var i = 0; i < 600; i++) {
               var s = base + i + '-' + 'y'.repeat(131072);
               n += s.split('-').length + s.split('-').length;
             }
             print(n);",
        );
        assert_eq!(out, "3600");
        // Short subjects churn through the cache: it stays small.
        let out = run_in(
            &mut rt,
            "var m = 0; for (var j = 0; j < 20000; j++) { var t = 'k' + j + '-v'; m += t.split('-').length + t.split('-').length; } print(m);",
        );
        assert_eq!(out, "80000");
        assert_clean(&rt);
    });
}

/// The short path of `split` gives way to the general loop at 16 pieces
/// and at 1,024 units; a split into a million pieces ends at the deadline
/// or with its result.
#[test]
fn split_bounds() {
    on_big_stack(|| {
        let mut rt = runtime_with_heap_limit(256, false);
        let out = run_in(
            &mut rt,
            "var a = []; for (var i = 0; i < 17; i++) a.push(i);
             var s16 = a.slice(0, 16).join('-'), s17 = a.join('-'), long = 'z'.repeat(1023) + '-q';
             print(s16.split('-').length, s17.split('-').length, s17.split('-', 16).length, long.split('-')[1], (long + 'r').split('-')[1]);",
        );
        assert_eq!(out, "16 17 16 q qr");
        let (result, elapsed) = eval_within(
            &mut rt,
            "var big = 'a-'.repeat(1000000); for (;;) { big.split('-'); }",
            DEADLINE,
        );
        assert!(is_limit(&result), "{result:?}");
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
    });
}

/// `concat` and `filter(Boolean)` at and beyond the length of their
/// one-step paths give the same results as the general steps; a `concat`
/// that doubles its array ends at the heap limit or the deadline.
#[test]
fn one_step_bounds() {
    on_big_stack(|| {
        let mut rt = runtime_with_heap_limit(64, false);
        let out = run_in(
            &mut rt,
            "var a = []; for (var i = 0; i < 4100; i++) a.push(i % 3);
             var b = a.slice(0, 4000);
             print(a.concat(a).length, b.concat(b.slice(0, 95)).length, b.concat(b.slice(0, 96)).length,
                   a.filter(Boolean).length, b.filter(Boolean).length, [].concat(a, [, ]).length);",
        );
        assert_eq!(out, "8200 4095 4096 2733 2666 4101");
        let (result, elapsed) = eval_within(
            &mut rt,
            "var d = [1, 2, 3]; for (;;) { d = d.concat(d); }",
            DEADLINE,
        );
        assert!(
            is_limit(&result) || matches!(result, Err(ScriptError::Uncaught { .. })),
            "{result:?}"
        );
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
    });
}

/// Loops of element reads past the end and at holes end at the deadline,
/// also with a long prototype chain (beyond the bound of the fast path).
#[test]
fn element_read_loops_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime_with_heap_limit(64, false);
        let cases = [
            "var a = [1, , 3]; for (var i = 0; ; i++) { a[i & 7]; a[5]; }",
            "var c = [1]; for (var n = 0; n < 50; n++) c = Object.create(c); for (;;) { c[0]; c[9]; }",
        ];
        for source in cases {
            let (result, elapsed) = eval_within(&mut rt, source, DEADLINE);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(
            rt.eval("[1][3] === undefined").ok(),
            Some(Value::Bool(true))
        );
    });
}
