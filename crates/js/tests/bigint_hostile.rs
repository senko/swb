//! Hostile cases for `BigInt` (M7 feature 10b): loops of the slowest
//! operations on values at the size limit (2^18 bits) against the
//! deadline, values that grow until the heap limit, `BigInt` of huge
//! strings, the largest value converted to text in a loop, comparisons
//! with long strings, operations that throw in a loop, messages that
//! quote a value at the limit, and literals above the limit. A case ends
//! with a result, an error or a termination; none may run far past the
//! deadline, allocate outside the heap accounting or leave the runtime
//! unusable.

mod common;

use std::time::Duration;

use common::{assert_clean, eval_within, on_big_stack, run_in, runtime, runtime_with_tests};
use swb_js::{HeapConfig, Runtime, RuntimeConfig, ScriptError, Termination, ThrowKind, Value};

const DEADLINE: Duration = Duration::from_millis(200);

/// The slack after the deadline: the slowest single operation (`toString`
/// in radix 10 at the limit, 32 ms in a release build, several times that
/// in the debug build of the tests) plus scheduling noise.
const SLACK: Duration = Duration::from_millis(1500);

/// Runs `source` with the deadline `DEADLINE`; returns the result and the time it took.
fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    eval_within(rt, source, DEADLINE)
}

/// Endless loops of the quadratic operations on values at the limit end
/// at the deadline: each operation is charged before it runs.
#[test]
fn loops_at_the_size_limit_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let setup = "var big = 2n ** 262143n - 1n; var half = 2n ** 131071n - 3n; \
                     var hex = big.toString(16);";
        assert_eq!(run_in(&mut rt, setup), "");
        for source in [
            "for (;;) half * half;",
            "for (;;) big / (half + 5n);",
            "for (;;) big % 1000000007n;",
            "for (;;) big.toString();",
            "for (;;) big.toString(36);",
            "for (;;) 3n ** 165000n;",
            "for (;;) BigInt('0x' + hex);",
            "for (;;) String(big).length;",
            "for (;;) `${big}`;",
            "for (;;) big << 1n >> 1n;",
            "for (;;) big & -half | half ^ big;",
            "for (;;) BigInt.asUintN(262144, -half);",
            "for (;;) big == '0x' + hex;",
            "for (;;) Number(big) + Number(half);",
            "for (;;) big.toLocaleString();",
        ] {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
                "{source}: {result:?}"
            );
            assert!(elapsed < DEADLINE + SLACK, "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        // A sum that passes the limit ends with the RangeError.
        assert_eq!(
            run_in(
                &mut rt,
                "var t = big; try { for (;;) t += t; } catch (e) { e.message }"
            ),
            "=> Maximum BigInt size exceeded"
        );
        assert_eq!(run_in(&mut rt, "1n + 1n"), "=> 2");
    });
}

/// Values that grow until the heap limit end the script there; the heap
/// accounting counts every limb.
#[test]
fn growth_ends_at_the_heap_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 64 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with_tests(config);
        for source in [
            // 32 KiB each.
            "var a = []; var x = 2n ** 262000n; for (var i = 0n; ; i++) a.push(x + i);",
            // Growing values, all kept.
            "var b = []; var y = 1n; for (;;) { y = (y << 1000n) | 1n; if (y > 2n ** 260000n) y = 1n; b.push(y); }",
            "var m = new Map(); for (var i = 0n; ; i++) m.set(i, 2n ** 200000n + i);",
            "var w = []; for (var i = 0; ; i++) w.push(Object(2n ** 250000n + BigInt(i)));",
        ] {
            let result = rt.eval(source);
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::HeapLimit))),
                "{source}: {result:?}"
            );
            assert_clean(&rt);
            // The runtime stays usable once the garbage is released.
            rt.eval("a = b = m = w = undefined").ok();
        }
        assert_eq!(
            run_in(&mut rt, "String(2n ** 100n)"),
            "=> 1267650600228229401496703205376"
        );
    });
}

/// Garbage values that no one keeps are collected: a loop of a million
/// large temporaries stays under a small heap limit.
#[test]
fn temporaries_are_collected() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 32 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with_tests(config);
        assert_eq!(
            run_in(
                &mut rt,
                "var x = 2n ** 4000n; var s = 0n; for (var i = 0; i < 200000; i++) s = (s + x * 3n) % 1000000007n; s > 0n"
            ),
            "=> true"
        );
        assert_clean(&rt);
    });
}

/// `BigInt` of huge strings: a valid text above the limit is a
/// `RangeError`, an invalid one a `SyntaxError` with a short message; the
/// scan is charged, so a loop of them ends at the deadline.
#[test]
fn huge_strings() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        assert_eq!(
            run_in(
                &mut rt,
                "var d = '7'.repeat(5e7); try { BigInt(d) } catch (e) { e.name + ': ' + e.message }"
            ),
            "=> RangeError: Maximum BigInt size exceeded"
        );
        assert_eq!(
            run_in(
                &mut rt,
                "try { BigInt(d + 'x') } catch (e) { e.name + ' ' + e.message.length }"
            ),
            "=> SyntaxError 1028"
        );
        // Comparisons with a huge valid text decide by the sign.
        assert_eq!(
            run_in(
                &mut rt,
                "[1n < d, 1n > '-' + d, 1n == d, 2n ** 262143n < d].join()"
            ),
            "=> true,true,false,true"
        );
        let (result, elapsed) =
            eval_with_deadline(&mut rt, "for (;;) { try { BigInt(d) } catch (e) {} }");
        assert!(
            matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
            "{result:?}"
        );
        assert!(elapsed < DEADLINE + SLACK, "{elapsed:?}");
        let (result, elapsed) = eval_with_deadline(&mut rt, "for (;;) 5n < d;");
        assert!(
            matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
            "{result:?}"
        );
        assert!(elapsed < DEADLINE + SLACK, "{elapsed:?}");
        assert_clean(&rt);
    });
}

/// Operations that throw, in loops: no handle or root leaks, and the
/// endless form ends at the deadline.
#[test]
fn throwing_operations_in_loops() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var n = 0; var big = 2n ** 262143n; for (var i = 0; i < 20000; i++) { \
             try { 1n + 1; } catch (e) { n++; } \
             try { +1n; } catch (e) { n++; } \
             try { 1n >>> 1n; } catch (e) { n++; } \
             try { 1n / 0n; } catch (e) { n++; } \
             try { big * 2n; } catch (e) { n++; } \
             try { big << big; } catch (e) { n++; } \
             try { 3n ** (2n ** 64n); } catch (e) { n++; } \
             try { BigInt(1.5); } catch (e) { n++; } \
             try { BigInt('z'); } catch (e) { n++; } \
             try { BigInt.asUintN(2 ** 53 - 1, -1n); } catch (e) { n++; } \
             try { Math.max(1n); } catch (e) { n++; } \
             try { JSON.stringify(1n); } catch (e) { n++; } \
           } n";
        assert_eq!(run_in(&mut rt, source), "=> 240000");
        assert_clean(&rt);
        let (result, elapsed) =
            eval_with_deadline(&mut rt, "for (;;) { try { big * big; } catch (e) {} }");
        assert!(
            matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
            "{result:?}"
        );
        assert!(elapsed < DEADLINE + SLACK, "{elapsed:?}");
        assert_clean(&rt);
    });
}

/// Messages that quote a value at the limit (its decimal text, 35 ms in
/// a release build) in endless loops end at the deadline: the text is
/// charged although the message is built behind a shared borrow.
#[test]
fn messages_with_big_values_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        assert_eq!(run_in(&mut rt, "var big = 2n ** 262143n - 1n;"), "");
        for operation in [
            "big in big",
            "({}) instanceof big",
            "Object.create(big)",
            "class A extends big {}",
            "Symbol.keyFor(big)",
            "[1, 2].sort(big)",
            "(function () { 'use strict'; big.x = 1; })()",
        ] {
            let source = format!("for (;;) {{ try {{ {operation}; }} catch (e) {{}} }}");
            let (result, elapsed) = eval_with_deadline(&mut rt, &source);
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
                "{operation}: {result:?}"
            );
            assert!(elapsed < DEADLINE + SLACK, "{operation}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

/// A literal above the size limit is a `SyntaxError` at compile time
/// (also in a function that never runs, as a key and in a pattern); one
/// at the limit compiles and runs.
#[test]
fn literals_at_and_above_the_limit() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let at_limit = format!("0x{}n", "f".repeat(65536));
        let above = format!("0x1{}n", "0".repeat(65536));
        for source in [
            format!("var x = {above};"),
            format!("function never() {{ return {above}; }}"),
            format!("var o = {{ {above}: 1 }};"),
            format!("class C {{ {above}() {{}} }}"),
            format!("var {{ {above}: a }} = {{}};"),
            format!("var y = -{above};"),
        ] {
            let result = rt.eval(&source);
            let Err(ScriptError::Compile { kind, message, .. }) = result else {
                panic!("{}: {result:?}", &source[..20]);
            };
            assert_eq!(kind, ThrowKind::SyntaxError);
            assert_eq!(message, "Maximum BigInt size exceeded");
        }
        assert_eq!(
            run_in(&mut rt, &format!("({at_limit}).toString(16).length")),
            "=> 65536"
        );
        assert_eq!(
            run_in(
                &mut rt,
                &format!("Object.keys({{ {at_limit}: 1 }})[0].length")
            ),
            "=> 78914"
        );
        assert_eq!(
            run_in(
                &mut rt,
                &format!("({at_limit}) - 1n === 2n * (2n ** 262143n - 1n)")
            ),
            "=> true"
        );
        assert_clean(&rt);
    });
}

/// The literal constants of a code object survive collections (the code
/// object traces them), in the stress mode.
#[test]
fn literal_constants_survive_collections() {
    on_big_stack(|| {
        let mut rt = runtime(true);
        let source = "function f() { return [123456789012345678901234567890n, -0xFFFFFFFFFFFFFFFFFFn, 7n]; } \
             var junk = []; for (var i = 0; i < 3000; i++) junk.push({ i: i, b: BigInt(i) * 2n ** 70n }); \
             junk = null; f().join() + ' ' + (f()[0] === 123456789012345678901234567890n)";
        assert_eq!(
            run_in(&mut rt, source),
            "=> 123456789012345678901234567890,-4722366482869645213695,7 true"
        );
        assert_clean(&rt);
    });
}
