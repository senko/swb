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

/// The result is a termination by the heap limit or the deadline.
fn assert_terminated(result: &Result<Value, ScriptError>, source: &str) {
    assert!(
        matches!(
            result,
            Err(ScriptError::Terminated(
                Termination::HeapLimit | Termination::TimeLimit
            ))
        ),
        "{source}: {result:?}"
    );
}

#[test]
fn endless_iterators_end_at_a_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 64 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        rt.eval(
            "var endless = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; } }; } }; \
             function* gen() { while (true) yield 1; } undefined;",
        )
        .unwrap();
        let cases = [
            "[...endless];",
            "[...gen()];",
            "(function () {})(...endless);",
            "new Array(...gen());",
            "var [...r] = endless;",
            "var [a, ...r] = gen();",
            "var r; [, ...r] = endless;",
            "for (var x of endless) { }",
            "for (var { y } of gen()) { }",
            "while (true) { var [a, b] = endless; }",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_terminated(&result, source);
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(rt.eval("[...[1, 2]].length"), Ok(Value::Int(2)));
    });
}

#[test]
fn deeply_nested_patterns_compile_or_fail_cleanly() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let open = "[".repeat(10_000);
        let close = "]".repeat(10_000);
        let objects = "{a:".repeat(10_000);
        let ends = "}".repeat(10_000);
        let cases = [
            format!("var {open}a{close} = 1;"),
            format!("var {objects}b{ends} = 1;"),
            format!("var a; ({open}a{close} = 1);"),
            format!("var b; ({objects}b{ends} = 1);"),
            format!("function f({open}a{close}) {{}}"),
            format!("for (var {open}a{close} of []);"),
            format!("try {{}} catch ({objects}b{ends}) {{}}"),
        ];
        for source in &cases {
            let out = run_in(&mut rt, source);
            assert!(out.contains("RangeError"), "{}", &out[..out.len().min(200)]);
            assert_clean(&rt);
        }
        // Nesting that the parser takes compiles and runs.
        let open = "[".repeat(1_000);
        let close = "]".repeat(1_000);
        let objects = "{a:".repeat(1_000);
        let ends = "}".repeat(1_000);
        let source = format!(
            "var x = {open}7{close}; var {open}a{close} = x; \
             var y = {objects}8{ends}; var {objects}b{ends} = y; print(a, b);"
        );
        assert_eq!(run_in(&mut rt, &source), "7 8");
        assert_clean(&rt);
    });
}

#[test]
fn long_parameter_lists_with_defaults() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // V8's limit of the parameters of a function.
        let params: Vec<String> = (0..100_000).map(|i| format!("p{i} = {i}")).collect();
        let source = format!("var f = function ({}) {{ return 1; }};", params.join(","));
        let out = run_in(&mut rt, &source);
        assert!(
            out.starts_with(
                "SyntaxError: Too many parameters in function definition (only 65534 allowed)"
            ),
            "{}",
            &out[..out.len().min(200)]
        );
        // One register per parameter: 60,000 parameters with defaults fit.
        let params: Vec<String> = (0..60_000).map(|i| format!("p{i} = {i}")).collect();
        let source = format!(
            "var f = function ({}, ...rest) {{ return p59999 + arguments.length + rest.length; }}; \
             print(f(), f.length, f(1));",
            params.join(",")
        );
        assert_eq!(run_in(&mut rt, &source), "59999 0 60000");
        assert_clean(&rt);
    });
}

#[test]
fn spread_of_a_huge_array() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 512 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // Doubling by spread takes the bulk copy of arrays without holes.
        let build = "var big = [0]; while (big.length * 2 <= 1e7) big = [...big, ...big]; \
                     big = [...big, ...big.slice(0, 1e7 - big.length)]; big.length;";
        assert_eq!(rt.eval(build), Ok(Value::Int(10_000_000)));
        assert_eq!(rt.eval("[...big].length;"), Ok(Value::Int(10_000_000)));
        // More arguments than the value stack holds: V8 also throws.
        assert_eq!(
            rt.eval("var m; try { (function () {})(...big); } catch (e) { m = e.message; } m === 'Maximum call stack size exceeded';"),
            Ok(Value::Bool(true))
        );
        let cases = [
            // Three more copies do not fit under the heap limit.
            "var c = [...big, ...big, ...big];",
            // With a hole, every element takes a step.
            "delete big[5]; var d = [...big];",
            "(function () {})(...big);",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_terminated(&result, source);
            assert!(elapsed < Duration::from_secs(5), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn misbehaving_iterators_in_hot_loops() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval(
            "function make(next, ret) { var r = { next: next }; \
             if (ret) Object.defineProperty(r, 'return', ret); \
             return { [Symbol.iterator]() { return r; } }; } \
             var value = () => ({ value: 1, done: false }); \
             var cases = [make(() => { throw new Error('next'); }), make(() => 5), \
             make(value, { value: () => { throw new Error('ret'); } }), make(value, { value: () => 5 }), \
             make(value, { get() { throw new Error('getter'); } }), make(value, { value: 7 })]; undefined;",
        )
        .unwrap();
        let loops = [
            "while (true) { for (var c of cases) { try { for (var x of c) break; } catch (e) { } } }",
            "while (true) { for (var c of cases) { try { for (var x of c) throw 1; } catch (e) { } } }",
            "while (true) { for (var c of cases) { try { var [a] = c; } catch (e) { } } }",
            "while (true) { for (var c of cases) { try { [...c]; } catch (e) { } } }",
            "function* g() { for (var x of cases[3]) yield x; } \
             while (true) { var i = g(); i.next(); try { i.return(1); } catch (e) { } }",
        ];
        for source in loops {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(
            run_in(
                &mut rt,
                "var out = []; for (var c of cases) { try { for (var x of c) break; out.push('ok'); } \
                 catch (e) { out.push(e.message); } } print(out.join());"
            ),
            "next,Iterator result 5 is not an object,ret,Iterator result 5 is not an object,getter,number 7 is not a function"
        );
    });
}

#[test]
fn stress_mode_agrees_on_iteration_and_destructuring() {
    let source = "function* g() { try { yield 1; yield 2; } finally { log.push('fin'); } } var log = []; \
        var r = [...[1, 2], ...g()]; for (var x of g()) break; var [a, , ...b] = [1, 2, 3, 4]; \
        var { p, ...q } = { p: 1, q: 2, s: 3 }; function f(u, v = u + 1, ...w) { return u + v + w.length; } \
        var o = {}; [o.a, { b: o.b = 5 }] = [6, {}]; \
        print(r.join(), log.join(), a, b.join(), p, Object.keys(q).join(), f(1), f(...[1, 2, 3, 4]), o.a, o.b);";
    let expected = "1,2,1,2 fin,fin 1 3,4 1 q,s 3 5 6 5";
    assert_eq!(run(source, true), expected);
    assert_eq!(run(source, false), expected);
}
