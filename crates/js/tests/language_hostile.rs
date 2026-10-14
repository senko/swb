//! Hostile cases for the language forms of M7 feature 3a: `for`-`in` over
//! huge objects, long prototype chains and long String objects, object
//! spread of huge objects, deep optional chains and tagged templates in
//! hot loops. A case ends with a result, an error or a termination at the
//! deadline or the heap limit; none may overflow the Rust stack, allocate
//! outside the heap accounting in proportion to the script's data, or run
//! past the deadline.

mod common;

use std::time::{Duration, Instant};

use common::{on_big_stack, run, run_in, runtime, runtime_with};
use swb_js::{HeapConfig, Runtime, RuntimeConfig, ScriptError, Termination, Value};

const DEADLINE: Duration = Duration::from_millis(100);

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

#[test]
fn key_loops_over_a_huge_object_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build = "var big = {}; for (var i = 0; i < 1000000; i++) big['k' + i] = i; undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        let cases = [
            "while (true) { for (var k in big) { } }",
            "while (true) { for (var k in big) break; }",
            "while (true) { var c = { ...big }; }",
            "while (true) { var c = { a: 1, ...big, b: 2 }; }",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        // Keys that the loop deletes before it reaches them are skipped,
        // each at a time step.
        let (result, elapsed) = eval_with_deadline(
            &mut rt,
            "var n = 0; for (var k in big) { n++; if (n === 1) { for (var j = 1; j < 1000000; j++) delete big['k' + j]; } } n;",
        );
        assert!(
            matches!(
                result,
                Ok(Value::Int(1)) | Err(ScriptError::Terminated(Termination::TimeLimit))
            ),
            "{result:?}"
        );
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        assert_clean(&rt);
        assert_eq!(run_in(&mut rt, "print(1 + 1)"), "2");
    });
}

#[test]
fn a_long_prototype_chain_with_keys() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Each object has its own key and a shared one: the shared key is
        // visited once, from the first object. (The objects get their keys
        // from `Object.create`: an assignment would search the chain.)
        let build = "function chain(n) { var top = { shared: -1 }; \
                     for (var i = 0; i < n; i++) top = Object.create(top, { ['k' + i]: { value: i, enumerable: true }, shared: { value: i, enumerable: true } }); \
                     return top; } var top = chain(2000); undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        assert_eq!(
            run_in(
                &mut rt,
                "var n = 0, first, last; for (var k in top) { if (n === 0) first = k; last = k; n++; } print(n, first, last);"
            ),
            "2001 k1999 k0"
        );
        // 100,000 objects: each step walks the chain to check its key (as
        // in V8, quadratic), at a time step per object.
        assert_eq!(run_in(&mut rt, "top = chain(100000); undefined;"), "");
        let cases = [
            "for (var k in top) { }",
            "while (true) { for (var k in top) break; }",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        // A chain without keys above a keyed bottom needs no set of hidden
        // keys.
        let build = "var low = { only: 1 }; for (var i = 0; i < 100000; i++) low = Object.create(low); undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        assert_eq!(
            run_in(
                &mut rt,
                "var r = []; for (var k in low) r.push(k); print(r.join());"
            ),
            "only"
        );
        let (result, elapsed) =
            eval_with_deadline(&mut rt, "while (true) { for (var k in low) { } }");
        assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
    });
}

#[test]
fn keys_of_a_long_string_object_count_against_the_heap_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 128 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // The string fits; the list of its keys (16 bytes per unit) does
        // not.
        rt.eval(
            "var t = 'a'; for (var i = 0; i < 24; i++) t += t; var s = new String(t); undefined;",
        )
        .unwrap();
        let cases = [
            "for (var k in s) { }",
            "for (var k in t) { }",
            "var c = { ...s };",
            "var c = { ...t };",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(
                matches!(
                    result,
                    Err(ScriptError::Terminated(
                        Termination::HeapLimit | Termination::TimeLimit
                    ))
                ),
                "{source}: {result:?}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
        }
    });
}

#[test]
fn deep_optional_chains_compile_or_fail_cleanly() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // A chain that the compiler can take, short-circuited as a whole.
        let links = "?.a.b[0]".repeat(500);
        let source = format!("var u = null; print(u{links}, typeof u{links});");
        assert_eq!(run_in(&mut rt, &source), "undefined undefined");
        // A self-referencing object through a long chain of `?.`.
        let links = "?.self".repeat(1000);
        let source = format!("var o = {{ v: 7 }}; o.self = o; print(o{links}.v);");
        assert_eq!(run_in(&mut rt, &source), "7");
        // Deeper than the compiler's recursion budget: a `RangeError`, not
        // a crash.
        let links = "?.a".repeat(200_000);
        let out = run_in(&mut rt, &format!("var u; u{links};"));
        assert!(
            out.contains("RangeError") || out.is_empty(),
            "{}",
            &out[..out.len().min(200)]
        );
        assert_clean(&rt);
    });
}

#[test]
fn tagged_templates_in_a_hot_loop() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 16 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // One template object per site: the loop allocates no new ones, so
        // it ends at the deadline, not at the heap limit.
        let (result, elapsed) = eval_with_deadline(
            &mut rt,
            "function t(s) { return s; } var first = null; \
             while (true) { var x = t`a${1}b${2}c`; if (first === null) first = x; else if (first !== x) throw 'new object'; }",
        );
        assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        // New closures of the same code share the site.
        let (result, elapsed) = eval_with_deadline(
            &mut rt,
            "function t(s) { return s; } \
             while (true) { (function () { return t`inner`; })(); }",
        );
        assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    });
}

#[test]
fn names_from_long_computed_keys_are_charged() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build = "var t = 'a'; for (var i = 0; i < 23; i++) t += t; undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        assert_eq!(
            run_in(&mut rt, "print(({ [t]() {} })[t].name.length);"),
            "8388608"
        );
        let cases = [
            "while (true) { var o = { [t]() {} }; }",
            "while (true) { var o = { get [t]() {} }; }",
            "while (true) { var o = { [t]: function () {} }; }",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn stress_mode_agrees_on_the_new_paths() {
    let source = "var p = { inherited: 1 }; var o = Object.create(p); for (var i = 0; i < 300; i++) o['k' + i] = i; \
        var n = 0; for (let k in o) { n++; } var c = { ...o, get g() { return 1; }, ['x' + 1]: function () {} }; \
        function t(s) { return s; } function site() { return t`a${n}b`; } var a = site(), b = site(); \
        var u = null; print(n, Object.keys(c).length, c.x1.name, a === b, a.raw.length, u?.x.y, o?.k1);";
    assert_eq!(run(source, true), "301 302 x1 true 2 undefined 1");
    assert_eq!(run(source, false), "301 302 x1 true 2 undefined 1");
}
