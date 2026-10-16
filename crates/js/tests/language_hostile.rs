//! Hostile cases for the language forms of M7 feature 3a: `for`-`in` over
//! huge objects, long prototype chains and long String objects, object
//! spread of huge objects, deep optional chains and tagged templates in
//! hot loops; and of the later sessions of feature 3 (classes, private
//! names, `eval` and `with`, the `Function` constructor and mapped
//! `arguments`); and of the number built-ins of feature 4 (long digit
//! strings, the costly conversions in loops, `Math.max` over a million
//! arguments); and of the ropes of feature 5a (a million alternating
//! appends and prepends, doubling to the length limit, flattens without
//! room under the heap limit, rope keys in hot loops, ropes across
//! collections); and of the arrays of feature 6a (the moving methods on
//! sparse lengths of 2^32 - 1 and 2^53 - 1, copies of huge sparse arrays,
//! comparators that change the array or answer at random, deep `flat`, a
//! growing `concat`); and of the iterator helpers of feature 6c (chains of
//! 100,000 helpers, deeply nested `flatMap`, wide `Iterator.concat`,
//! endless iterators with `toArray` and the other consumers, re-entrant
//! `next` and `return`). A case ends with a result, an error or a termination at the
//! deadline or the heap limit; none may overflow the Rust stack, allocate
//! outside the heap accounting in proportion to the script's data, or run
//! past the deadline.

mod common;

use std::time::{Duration, Instant};

use common::{on_big_stack, run, run_in, runtime, runtime_with, runtime_with_tests};
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

// --- Classes (M7 feature 3c) ---

#[test]
fn classes_with_many_members() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let mut methods = String::from("class M {");
        methods.extend((0..100_000).map(|i| format!(" m{i}() {{ return {i}; }}")));
        methods.push_str(
            " } print(new M().m99999(), Object.getOwnPropertyNames(M.prototype).length);",
        );
        assert_eq!(run_in(&mut rt, &methods), "99999 100001");
        let mut fields = String::from("class F {");
        fields.extend((0..100_000).map(|i| format!(" f{i} = {i}; static s{i} = {i};")));
        fields.push_str(" } var f = new F(); print(f.f99999, F.s99999, Object.keys(f).length);");
        assert_eq!(run_in(&mut rt, &fields), "99999 99999 100000");
        let mut computed = String::from("var n = 0; class K {");
        for _ in 0..100_000 {
            computed.push_str(" [n++] = n;");
        }
        computed.push_str(" } print(new K()[99999], n);");
        assert_eq!(run_in(&mut rt, &computed), "100000 100000");
        assert_clean(&rt);
    });
}

#[test]
fn a_long_chain_of_derived_classes() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Each level is a frame (the constructor) of the interpreter loop,
        // not Rust recursion: 10,000 levels end with V8's `RangeError`.
        let cases = [
            "var C = class { x = 1; }; for (var i = 0; i < 10000; i++) C = class extends C { y = i; constructor() { super(); } }; \
             try { new C(); } catch (e) { print(e.constructor.name, e.message); }",
            "var D = class { x = 1; }; for (var i = 0; i < 10000; i++) D = class extends D { z = i; }; \
             try { new D(); } catch (e) { print(e.constructor.name, e.message); }",
        ];
        for source in cases {
            assert_eq!(
                run_in(&mut rt, source),
                "RangeError Maximum call stack size exceeded",
                "{source}"
            );
            assert_clean(&rt);
        }
        // 4,000 levels fit.
        assert_eq!(
            run_in(
                &mut rt,
                "var E = class { x = 1; }; for (var i = 0; i < 4000; i++) E = class extends E { ['k' + i] = i; constructor(...a) { super(...a); } }; \
                 var e = new E(); print(e.x, e.k3999, Object.keys(e).length);"
            ),
            "1 4000 4001"
        );
        assert_clean(&rt);
    });
}

#[test]
fn class_definitions_in_a_hot_loop() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 64 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        let cases = [
            "while (true) { var K = class extends Object { m() { return super.toString; } static s = 1; x = [1, 2]; static { this.t = 2; } }; new K().m(); }",
            "var keep = []; while (true) keep.push(class { m() {} get g() { return 1; } static f = () => 1; });",
            "var keep = []; class B { constructor() { keep.push(this); } } class D extends B { a = [1, 2, 3, 4, 5, 6, 7, 8]; } while (true) new D();",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_terminated(&result, source);
            assert!(elapsed < Duration::from_secs(5), "{source}: {elapsed:?}");
            assert_clean(&rt);
            rt.eval("var keep = undefined;").unwrap();
        }
    });
}

#[test]
fn computed_class_keys_that_throw_or_run_long() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        assert_eq!(
            run_in(
                &mut rt,
                "var log = []; try { class C { [(log.push(1), 'a')]() {} [{ toString() { throw new Error('key'); } }]() {} [log.push(2)] = 1; } } \
                 catch (e) { print(e.message, log.join(), typeof C); }"
            ),
            "key 1 undefined"
        );
        let cases = [
            "class C1 { [{ toString() { while (true); } }]() {} }",
            "class C2 { static [{ toString() { while (true); } }] = 1; }",
            "class C3 { [{ toString() { for (;;) {} } }] = 1; }",
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
fn super_properties_through_a_long_prototype_chain() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build = "var bottom = { found: 1 }; var top = bottom; for (var i = 0; i < 100000; i++) top = Object.create(top); \
                     var o = { __proto__: top, read() { return super.found; }, miss() { while (true) super.missing; }, \
                     write() { while (true) super.missing = 1; }, call() { while (true) super.hasOwnProperty('x'); } }; undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        assert_eq!(run_in(&mut rt, "print(o.read());"), "1");
        for source in ["o.miss();", "o.write();", "o.call();"] {
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
fn class_errors_leave_the_runtime_clean() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            // The error of a primitive result belongs to the caller: the
            // constructor's frame is gone before it is raised, also when
            // that frame is the first one of a nested run.
            (
                "class A {} class B extends A { constructor() { super(); return 5; } } new B();",
                "Uncaught TypeError: Derived constructors may only return object or undefined",
            ),
            (
                "Reflect.construct(B, []);",
                "Uncaught TypeError: Derived constructors may only return object or undefined",
            ),
            (
                "[1].map(() => new B());",
                "Uncaught TypeError: Derived constructors may only return object or undefined",
            ),
            (
                "class N extends A { constructor() {} } new N();",
                "Uncaught ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
            ),
            (
                "A();",
                "Uncaught TypeError: Class constructor A cannot be invoked without 'new'",
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(run_in(&mut rt, source), expected, "{source}");
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "print(new A() instanceof A)"), "true");
    });
}

#[test]
fn stress_mode_agrees_on_classes() {
    let source = "class A { a = 1; static s = 'S'; constructor(x) { this.x = x; } m() { return this.x + this.a; } get g() { return 'g'; } } \
        class B extends A { b = this.a + 1; [Symbol.iterator] = () => 1; constructor() { super(5); } m() { return super.m() * 10 + this.b; } \
        static { this.t = super.s + '!'; } } var b = new B(); var o = { __proto__: b, n() { return super.g; } }; \
        class E extends Error {} var e = new E('boom'); \
        print(b.m(), B.t, o.n(), b[Symbol.iterator].name, e instanceof E, e.message);";
    let expected = "62 S! g [Symbol.iterator] true boom";
    assert_eq!(run(source, true), expected);
    assert_eq!(run(source, false), expected);
}

// --- Private names (M7 feature 3d) ---

#[test]
fn classes_with_many_private_names() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // A private name takes no register (one private environment per
        // class), so 100,000 of them work, as in V8.
        let mut fields = String::from("class C {");
        fields.extend((0..100_000).map(|i| format!(" #f{i} = {i};")));
        fields.push_str(
            " static last(o) { return o.#f99999 + o.#f0; } } var c = new C(); print(C.last(c), Object.keys(c).length);",
        );
        assert_eq!(run_in(&mut rt, &fields), "99999 0");
        let mut methods = String::from("class M {");
        methods.extend((0..100_000).map(|i| format!(" #m{i}() {{ return {i}; }}")));
        methods.push_str(
            " static last(o) { return o.#m99999() + (#m5 in o); } } print(M.last(new M()));",
        );
        assert_eq!(run_in(&mut rt, &methods), "100000");
        let mut statics = String::from("class S {");
        statics.extend(
            (0..100_000)
                .map(|i| format!(" static #s{i} = {i}; static get #g{i}() {{ return {i}; }}")),
        );
        statics.push_str(" static last() { return S.#s99999 + S.#g99999; } } print(S.last());");
        assert_eq!(run_in(&mut rt, &statics), "199998");
        assert_clean(&rt);
    });
}

#[test]
fn one_object_with_private_fields_of_many_classes() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // The return override stamps one object with a field of each of
        // 10,000 classes; a chain of 4,000 derived classes gives one
        // instance a field and a brand of each.
        let source = "class R { constructor(o) { return o; } } var target = {}; var readers = []; \
             for (var i = 0; i < 10000; i++) { var K = class extends R { #f = i; static read(o) { return o.#f; } }; new K(target); readers.push(K.read); } \
             print(readers[0](target), readers[9999](target), Object.keys(target).length); \
             var C = class { #base = 'b'; static b(o) { return o.#base; } }; var B0 = C; \
             for (var i = 0; i < 4000; i++) C = class extends C { #f = i; #m() { return 1; } static get(o) { return o.#m() + o.#f; } }; \
             var c = new C(); print(C.get(c), B0.b(c), Reflect.ownKeys(c).length);";
        assert_eq!(run_in(&mut rt, source), "0 9999 0\n4001 b 0");
        assert_clean(&rt);
    });
}

#[test]
fn private_class_bodies_in_a_hot_loop() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 64 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // Each evaluation creates new Private Names and a new brand; the
        // old ones are garbage.
        assert_eq!(
            rt.eval(
                "var n = 0; for (var i = 0; i < 100000; i++) { const K = class { #a = i; #m() { return this.#a; } static #s = 1; \
                 static get(o) { return o.#m() + K.#s; } }; n += K.get(new K()); } n;"
            ),
            Ok(Value::Double(5_000_050_000.0))
        );
        let cases = [
            "var keep = []; while (true) { const K = class { #a = [1, 2, 3]; #m() {} static #s() {} }; keep.push(new K()); }",
            "var keep = {}; class R { constructor(o) { return o; } } while (true) { new (class extends R { #f = 1; #g = 2; })(keep); }",
        ];
        // Live classes and stamped objects end at the heap limit or the
        // deadline (each in a runtime of its own: a full heap stays full).
        for source in cases {
            let mut rt = runtime_with(config);
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_terminated(&result, source);
            assert!(elapsed < Duration::from_secs(5), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn private_access_in_hot_loops_ends_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build = "class P { #x = 1; #m() { return 1; } get #g() { return 2; } set #g(v) {} \
             static loops(o, w) { if (w === 0) while (true) o.#x; if (w === 1) while (true) o.#m(); if (w === 2) while (true) #x in o; \
             if (w === 3) while (true) o.#g = o.#g; if (w === 4) while (true) { try { ({}).#x; } catch (e) {} } \
             if (w === 5) while (true) o.#x++; } } var p = new P(); undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        for w in 0..6 {
            let source = format!("P.loops(p, {w});");
            let (result, elapsed) = eval_with_deadline(&mut rt, &source);
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
fn stress_mode_agrees_on_private_names() {
    let source = "class R { constructor(o) { return o; } } \
        class A extends R { #a = [1, 2]; static #s = 's'; #m() { return this.#a.length; } get #g() { return A.#s; } \
        static read(o) { return o.#m() + o.#g + (#a in o) + (#m in o); } } \
        var objs = []; for (var i = 0; i < 50; i++) { var o = { k: i }; new A(o); objs.push(o); } \
        var keys = 0; for (var o of objs) keys += Object.keys(o).length; \
        print(A.read(objs[49]), keys, A.read(new A({})));";
    let expected = "2struetrue 50 2struetrue";
    assert_eq!(run(source, true), expected);
    assert_eq!(run(source, false), expected);
}

// --- `eval` and `with` (M7 feature 3e) ---

#[test]
fn eval_recursion_ends_at_the_frame_limit() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Eval code runs as frames of the interpreter loop: the recursion
        // ends with the `RangeError` of the frame limit, without Rust
        // recursion (the parse and the compile give their budget back).
        let cases = [
            "function f() { eval('f()'); } f();",
            "function g(n) { return eval('g(n + 1)'); } g(0);",
            "function h() { return eval('eval(\"h()\")'); } h();",
            "var ie = eval; function k() { ie('k()'); } k();",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
            assert_eq!(
                result.map_err(|e| e.to_string()),
                Err("Uncaught RangeError: Maximum call stack size exceeded".to_owned()),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(20), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        assert_eq!(run_in(&mut rt, "eval('1 + 1')"), "=> 2");
    });
}

#[test]
fn compiled_eval_code_is_garbage_after_the_call() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 32 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // 100,000 different strings in global code and in a function (the
        // vars of the function's calls stay in its record until it
        // returns; the code of each call is garbage).
        assert_eq!(
            rt.eval("var s = 0; for (var i = 0; i < 100000; i++) s += eval('(' + i + ' + 1)'); s"),
            Ok(Value::Double(5_000_050_000.0))
        );
        assert_eq!(
            rt.eval(
                "function f() { var t = 0; for (var i = 0; i < 100000; i++) t += eval('var v' + i + ' = ' + i + '; v' + i); return t; } f()"
            ),
            Ok(Value::Double(4_999_950_000.0))
        );
        rt.collect();
        let live = rt.heap().stats().live_bytes;
        assert!(live < 8 << 20, "{live}");
        assert_clean(&rt);
    });
}

#[test]
fn eval_of_huge_strings_stays_under_the_heap_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 256 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        // 16 million code units of statements or of an array literal: the
        // parse tree does not fit, so the script ends at the heap limit
        // before the tree exists; a string literal of 16 million units
        // fits.
        let cases = [
            (
                "var body = ' y;'; while (body.length < 16000000) body += body; eval('var y = 1;' + body);",
                false,
            ),
            (
                "var arr = '1,'; while (arr.length < 16000000) arr += arr; eval('[' + arr + ']').length;",
                false,
            ),
            (
                "var big = 'x'; while (big.length < 16000000) big += big; eval('\"' + big + '\"').length;",
                true,
            ),
        ];
        for (source, fits) in cases {
            let mut rt = runtime_with(config);
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(10));
            if fits {
                assert_eq!(result, Ok(Value::Int(16_777_216)), "{source}");
            } else {
                assert_eq!(
                    result,
                    Err(ScriptError::Terminated(Termination::HeapLimit)),
                    "{source}"
                );
            }
            assert!(elapsed < Duration::from_secs(10), "{source}: {elapsed:?}");
            assert_clean(&rt);
            assert_eq!(rt.eval("eval('2 * 3')"), Ok(Value::Int(6)));
        }
    });
}

#[test]
fn with_lookups_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // A chain of 100,000 objects with getters: every lookup walks the
        // chain for the name (a time step per object) and calls the getters
        // that it finds.
        let build = "var o = { bottom: 1 }; Object.defineProperty(o, 'g', { get: function () { return 1; } }); \
                     for (var i = 0; i < 100000; i++) o = Object.create(o, { ['g' + i]: { get: function () { return i; } } }); undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        let cases = [
            "var n = 0; with (o) { while (true) { n += g; typeof missing; } }",
            "with (o) { while (true) { typeof missing; bottom = bottom; } }",
            "function f() { with (o) { return () => { while (true) g0; }; } } f()();",
            "while (true) { eval('missing'.length + ''); }",
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
fn deep_with_nesting_is_bounded() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // 10,000 nested `with` statements in one source exceed the
        // recursion budget of the parser (a `RangeError`, as in V8); 1,000
        // work, and so do 5,000 records on the frames of a recursion.
        let nest = |n: usize| -> String {
            let mut text = String::new();
            for i in 0..n {
                text += "with ({ k";
                text += &i.to_string();
                text += ": 0 }) ";
            }
            text
        };
        let deep = nest(10_000) + "k0;";
        let Err(ScriptError::Compile { kind, .. }) = rt.eval(&deep) else {
            panic!("10,000 nested with statements compiled");
        };
        assert_eq!(kind, swb_js::ThrowKind::RangeError);
        let ok = nest(1_000) + "k0 + k999 + typeof nothing;";
        assert_eq!(run_in(&mut rt, &ok), "=> 0undefined");
        assert_eq!(
            run_in(
                &mut rt,
                "function deep(n) { with ({ d: n }) { return n ? deep(n - 1) : d; } } deep(5000)"
            ),
            "=> 0"
        );
        assert_clean(&rt);
    });
}

#[test]
fn nested_eval_in_eval_code_is_bounded() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Each function of eval code that calls `eval` captures every name
        // of the caller: a function with 20,000 vars whose eval code holds
        // many such functions would cost functions x 20,000 in one
        // compile. Past the capture limit the eval throws a `RangeError`;
        // below it, one compile stays far below a second.
        let program = |functions: usize| -> String {
            let mut text = String::from("function f() { ");
            for i in 0..20_000 {
                let i = i.to_string();
                text += "var v";
                text += &i;
                text += " = ";
                text += &i;
                text += "; ";
            }
            text += "return eval('";
            for _ in 0..functions {
                text += "(function () { eval(0); }); ";
            }
            text += "1'); } f();";
            text
        };
        // Without a nested `eval`: a cell that a function uses is also
        // captured by every function around it (depth x names).
        let nested = |depth: usize| -> String {
            let mut text = String::from("function f() { ");
            let mut uses = String::new();
            for i in 0..20_000 {
                let i = i.to_string();
                text += "var v";
                text += &i;
                text += " = 1; ";
                uses += "v";
                uses += &i;
                uses += "; ";
            }
            text += "return eval('";
            text += &"(function () { ".repeat(depth);
            text += &uses;
            text += &" })();".repeat(depth);
            text += " 7";
            text += "'); } f();";
            text
        };
        assert_eq!(rt.eval(&program(10)), Ok(Value::Int(1)));
        assert_eq!(rt.eval(&nested(10)), Ok(Value::Int(7)));
        let deep = nested(100);
        let (result, elapsed) = eval_with_deadline_of(&mut rt, &deep, Duration::from_secs(5));
        assert_eq!(
            result.map_err(|e| e.to_string()),
            Err("Uncaught RangeError: Too many captured variables in a script".to_owned())
        );
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
        for functions in [100, 3_000] {
            let source = program(functions);
            let (result, elapsed) = eval_with_deadline_of(&mut rt, &source, Duration::from_secs(5));
            assert_eq!(
                result.map_err(|e| e.to_string()),
                Err("Uncaught RangeError: Too many captured variables in a script".to_owned()),
                "{functions} functions"
            );
            assert!(elapsed < Duration::from_secs(3), "{functions}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn function_constructor_with_huge_text_stays_under_the_heap_limit() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 256 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        // A body of 16 million code units: statements do not fit (the
        // script ends at the heap limit before the tree exists), a string
        // literal fits. Huge parameter text is charged the same way.
        let cases = [
            (
                "var body = ' y;'; while (body.length < 16000000) body += body; Function('var y = 1;' + body);",
                false,
            ),
            (
                "var p = '1,'; while (p.length < 16000000) p += p; Function('a = [' + p + ']', '');",
                false,
            ),
            (
                "var big = 'x'; while (big.length < 16000000) big += big; Function('return \"' + big + '\"')().length;",
                true,
            ),
        ];
        for (source, fits) in cases {
            let mut rt = runtime_with(config);
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(10));
            if fits {
                assert_eq!(result, Ok(Value::Int(16_777_216)), "{source}");
            } else {
                assert_eq!(
                    result,
                    Err(ScriptError::Terminated(Termination::HeapLimit)),
                    "{source}"
                );
            }
            assert!(elapsed < Duration::from_secs(10), "{source}: {elapsed:?}");
            assert_clean(&rt);
            assert_eq!(
                rt.eval("Function('a', 'return a * 3')(2)"),
                Ok(Value::Int(6))
            );
        }
    });
}

#[test]
fn function_constructor_code_is_garbage_after_the_call() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 32 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // 100,000 functions with different bodies: the script around each
        // is garbage at once, each function's code with the function.
        assert_eq!(
            rt.eval(
                "var s = 0; for (var i = 0; i < 100000; i++) s += Function('a', 'var v' + i + ' = a; return v' + i + ' + ' + i)(1); s"
            ),
            Ok(Value::Double(5_000_050_000.0))
        );
        rt.collect();
        let live = rt.heap().stats().live_bytes;
        assert!(live < 8 << 20, "{live}");
        // Functions that stay alive count their source against the limit.
        let (result, _) = eval_with_deadline_of(
            &mut rt,
            "var big = 'x'; while (big.length < 1000000) big += big; var keep = []; while (true) keep.push(Function('return 1; // ' + big + keep.length));",
            Duration::from_secs(10),
        );
        assert_eq!(result, Err(ScriptError::Terminated(Termination::HeapLimit)));
        assert_clean(&rt);
    });
}

#[test]
fn function_constructor_parameters_and_recursion_are_bounded() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // 100,000 parameters: V8's `SyntaxError` (also for 8 million,
        // where the parse stops at the limit); 64,000 mapped parameters
        // work (the parameter map has an entry per argument); 65,535 too
        // many again.
        let cases = [
            (
                "var p = []; for (var i = 0; i < 100000; i++) p.push('b' + i); try { Function(p.join(), ''); } catch (e) { e.message }",
                "=> Too many parameters in function definition (only 65534 allowed)",
            ),
            (
                "var p = 'a,'; while (p.length < 16000000) p += p; try { Function(p + 'z', ''); } catch (e) { e.message }",
                "=> Too many parameters in function definition (only 65534 allowed)",
            ),
            (
                "var p = []; for (var i = 0; i < 65535; i++) p.push('b' + i); try { Function(p.join(), 'return arguments'); } catch (e) { e.message }",
                "=> Too many parameters in function definition (only 65534 allowed)",
            ),
            (
                "var p = [], a = []; for (var i = 0; i < 64000; i++) { p.push('a' + i); a.push(i); } \
                 var f = Function(p.join(), 'a0 = 5; arguments[63999] = 7; return [arguments[0], a63999, arguments.length].join()'); \
                 f.apply(null, a)",
                "=> 5,7,64000",
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(run_in(&mut rt, source), expected, "{source}");
            assert_clean(&rt);
        }
        // Recursion through functions that `Function` makes ends at the
        // frame limit: each call is a frame of the interpreter loop.
        let recursion = [
            "var f = Function('f()'); f();",
            "var g = Function('return Function(\"return g()\")()'); g();",
            "var h = Function('a', 'return h.apply(null, arguments)'); h(1, 2);",
        ];
        for source in recursion {
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
            assert_eq!(
                result.map_err(|e| e.to_string()),
                Err("Uncaught RangeError: Maximum call stack size exceeded".to_owned()),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(20), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn mapped_arguments_in_hot_loops_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            "function f(a, b) { while (true) { arguments[0] = a + 1; a = arguments[0]; } } f(0, 1);",
            "function g(a, b) { while (true) { delete arguments[1]; arguments[1] = b; Object.defineProperty(arguments, '0', { value: a }); b = arguments[1]; } } g(0, 1);",
            "function h(a) { while (true) { eval('a++'); arguments[0]--; } } h(0);",
            "function k(a) { var x = arguments; while (true) { x = (function (b) { return arguments; })(x); } } k(0);",
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
fn stress_mode_agrees_on_function_constructor_and_arguments() {
    let cases = [
        "var fs = []; for (var i = 0; i < 20; i++) fs.push(new Function('a', 'b', 'return a * b + ' + i)); var t = 0; for (var f of fs) t += f(2, 3); t",
        "function m(a, b) { var g = () => { a = b; arguments[1] = 'x'; }; g(); return [a, b, arguments[0], arguments[1]].join(); } m(1, 2)",
        "function n(a) { Object.defineProperty(arguments, '0', { writable: false }); a = 9; return arguments[0] + ':' + a; } n(4)",
        "class F extends Function {} var ff = new F('return 4'); ff() + (Object.getPrototypeOf(ff) === F.prototype)",
    ];
    for source in cases {
        assert_eq!(run(source, false), run(source, true), "{source}");
    }
}

/// Like [`eval_with_deadline`], with another deadline.
fn eval_with_deadline_of(
    rt: &mut Runtime,
    source: &str,
    deadline: Duration,
) -> (Result<Value, ScriptError>, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + deadline));
    let result = rt.eval(source);
    rt.set_deadline(None);
    (result, start.elapsed())
}

/// Runs `source` with a deadline and expects the time limit, at most
/// `slack` after the deadline: a built-in that does much work per call
/// must charge the time countdown for it.
fn assert_ends_at_deadline(rt: &mut Runtime, source: &str, deadline: Duration, slack: Duration) {
    let (result, elapsed) = eval_with_deadline_of(rt, source, deadline);
    assert_eq!(
        result,
        Err(ScriptError::Terminated(Termination::TimeLimit)),
        "{source}"
    );
    assert!(elapsed < deadline + slack, "{source}: {elapsed:?}");
    assert_clean(rt);
}

/// A loop that parses 2^-1075 (the midpoint between 0 and `Number.MIN_VALUE`)
/// written out in 752 decimal digits with `op` (`m` is the text): the
/// slowest case of Rust's float parser, about 13 µs per call.
fn midpoint_loop(op: &str) -> String {
    format!("var m = '{FIVE_TO_1075}e-1075'; while (true) {op};")
}

/// 5^1075 in decimal.
const FIVE_TO_1075: &str = "24703282292062327208828439643411068618252990130716238221279284125033775363510437593264991818081799618989828234772285886546332835517796989819938739800539093906315035659515570226392290858392449105184435931802849936536152500319370457678249219365623669863658480757001585769269903706311928279558551332927834338409351978015531246597263579574622766465272827220056374006485499977096599470454020828166226237857393450736339007967761930577506740176324673600968951340535537458516661134223766678604162159680461914467291840300530057530849048765391711386591646239524912623653881879636239373280423891018672348497668235089863388587925628302755995657524455507255189313690836254779186948667994968324049705821028513185451396213837722826145437693412532098591327667236328125";

#[test]
fn costly_number_conversions_in_loops_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Each call takes a few microseconds; without charges a time check
        // would come only after 10,000 calls (tens of milliseconds).
        let cases: [&str; 15] = [
            "while (true) Number.MIN_VALUE.toString(2);",
            "while (true) Number.MAX_VALUE.toString(3);",
            "while (true) Number.MAX_VALUE.toString(2);",
            "while (true) (2.2250738585072014e-308 * 1.37).toString(2);",
            "while (true) (1e-300).toString(36);",
            "var t = ''; for (var j = 0; j < 646; j++) t += '2'; while (true) parseInt(t, 3);",
            &midpoint_loop("Number(m)"),
            &midpoint_loop("parseFloat(m)"),
            "while (true) Number.MIN_VALUE.toPrecision(100);",
            "while (true) Number.MAX_VALUE.toPrecision(100);",
            "while (true) Number.MIN_VALUE.toExponential(100);",
            "while (true) (2.2250738585072014e-308).toExponential(100);",
            "while (true) (1e20).toFixed(100);",
            "while (true) Number.MIN_VALUE.toFixed(100);",
            "while (true) Number.MAX_VALUE.toLocaleString();",
        ];
        for source in cases {
            assert_ends_at_deadline(
                &mut rt,
                source,
                Duration::from_millis(20),
                Duration::from_millis(40),
            );
        }
        assert_eq!(
            run_in(
                &mut rt,
                "print(Number.MIN_VALUE.toString(2).length, Number.MAX_VALUE.toString(3).length)"
            ),
            "1076 647"
        );
    });
}

#[test]
fn long_digit_strings_parse_in_bounded_memory_and_time() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 96 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // 16 M units each: ones, zeros, and zeros with a fraction point.
        rt.eval(
            "var d = '1'; for (var i = 0; i < 24; i++) d += d; \
             var z = '0'; for (var i = 0; i < 24; i++) z += z; undefined;",
        )
        .unwrap();
        let cases = [
            ("parseInt(d)", f64::INFINITY),
            ("parseInt(d, 16)", f64::INFINITY),
            ("parseInt(d, 7)", f64::INFINITY),
            ("parseInt(z, 36)", 0.0),
            ("parseFloat(d)", f64::INFINITY),
            ("parseFloat(z)", 0.0),
            ("Number(d)", f64::INFINITY),
            ("+z", 0.0),
            ("Number(z + '1e-16777216')", 0.0),
            ("Number('0.' + z + '1e16777217')", 1.0),
            ("Number(d + 'e-16777215')", 1.111_111_111_111_111_2),
            ("parseFloat('.' + z + '25e16777216')", 0.25),
        ];
        for (source, expected) in cases {
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(5));
            let value = result.unwrap_or_else(|e| panic!("{source}: {e:?}"));
            assert_eq!(value.as_number(), Some(expected), "{source}");
            assert!(elapsed < Duration::from_secs(2), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        // A scan charges one step per 16 units, so a loop of them ends at
        // the deadline.
        for source in [
            "while (true) parseInt(d);",
            "while (true) parseFloat(z);",
            "while (true) +d;",
        ] {
            assert_ends_at_deadline(
                &mut rt,
                source,
                Duration::from_millis(100),
                Duration::from_millis(100),
            );
        }
    });
}

#[test]
fn math_max_over_a_million_arguments() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build =
            "var a = []; for (var i = 0; i < 1000000; i++) a.push(i % 1000 - 500); undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        assert_eq!(
            run_in(
                &mut rt,
                "print(Math.max.apply(null, a), Math.min.apply(null, a), Math.hypot.apply(null, a) > 0, Math.sumPrecise(a))"
            ),
            "499 -500 true -500000"
        );
        // Conversions that run script code, a million times.
        let objects = "var o = []; for (var i = 0; i < 1000000; i++) o.push({ valueOf: function () { return 1; } }); \
                       print(Math.max.apply(null, o), Math.hypot.apply(null, o));";
        assert_eq!(run_in(&mut rt, objects), "1 1000");
        assert_ends_at_deadline(
            &mut rt,
            "while (true) Math.max.apply(null, a);",
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        assert_ends_at_deadline(
            &mut rt,
            "while (true) Math.hypot.apply(null, o);",
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
    });
}

// --- Ropes (M7 feature 5a) ---

#[test]
fn a_million_alternating_appends_and_prepends_then_one_flatten() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var s = ''; for (var i = 0; i < 1000000; i++) s = i % 2 ? 'x' + s : s + 'y'; \
                      print(s.length, s[0], s[499999], s[500000], s[999999]);";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(30));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(run_in(&mut rt, ""), "1000000 x x y y");
        assert_clean(&rt);
        // A rope of 2M units fits in a heap of 12 MiB (with the garbage of
        // the loop): the leaves are full, so the memory per code unit stays
        // bounded.
        let small = RuntimeConfig {
            heap: HeapConfig {
                limit: 12 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(small);
        let build =
            "var s = ''; for (var i = 0; i < 2000000; i++) s = i % 2 ? 'x' + s : s + 'y'; s.length";
        assert_eq!(rt.eval(build), Ok(Value::Int(2_000_000)));
        assert_clean(&rt);
    });
}

/// The string value of a global variable.
fn global_string(rt: &mut Runtime, name: &str) -> swb_js::Gc<swb_js::JsString> {
    match rt.eval(name) {
        Ok(Value::String(string)) => string,
        other => panic!("{name} is not a string: {other:?}"),
    }
}

#[test]
fn doubling_to_the_length_limit_and_flattens_without_room() {
    on_big_stack(|| {
        let mut rt = runtime_with_tests(RuntimeConfig {
            heap: HeapConfig {
                limit: 16 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        });
        // The rope of 2^28 units is a few nodes; the next doubling is
        // V8's RangeError.
        let build = "var s = 'a'; var n = 0; try { for (;;) { s += s; n++; } } \
                     catch (e) { print(n, s.length, e.name, e.message); }";
        assert_eq!(
            run_in(&mut rt, build),
            "28 268435456 RangeError Invalid string length"
        );
        // Every path that needs the code units flattens first; the flat
        // text does not fit, so the script ends at the heap limit (no
        // allocation over the limit, no panic).
        let paths = [
            "s[0]",
            "var o = {}; o[s] = 1;",
            "s in {}",
            "Number(s)",
            "s * 1",
            "parseInt(s)",
            "parseFloat(s)",
            "new String(s)",
            "Object(s)",
            "eval(s)",
            "(0, eval)(s)",
            "Function(s)",
            "Symbol.for(s)",
            "var o = {}; o[s + 'x'] = 1;",
        ];
        for source in paths {
            assert_eq!(
                rt.eval(source),
                Err(ScriptError::Terminated(Termination::HeapLimit)),
                "{source}"
            );
            assert_clean(&rt);
            let s = global_string(&mut rt, "s");
            assert!(rt.heap().string(s).unwrap().is_rope(), "{source}");
        }
        // Reading the rope without its code units still works.
        assert_eq!(
            run_in(
                &mut rt,
                "print(s.length, s === s, s === s + 'b', s < s + 'b', typeof s, String(s) === s, !!s)"
            ),
            "268435456 true false true string true true"
        );
        // Copies that a script sizes stay within the limit too.
        for source in ["[s, s].join('')", "throw new Error(s)", "`${s}${s}`.length"] {
            let result = rt.eval(source);
            assert!(
                matches!(
                    result,
                    Err(ScriptError::Terminated(Termination::HeapLimit)
                        | ScriptError::Uncaught { .. })
                ),
                "{source}: {result:?}"
            );
            assert_clean(&rt);
        }
    });
}

#[test]
fn rope_keys_in_hot_loops_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build = "var big = ''; for (var i = 0; i < 100000; i++) big += 'abcdefghij'; \
                     var r = ''; for (var i = 0; i < 1100; i++) r += 'r'; undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        let cases = [
            // A new rope of 1M units as a key each time: a flatten each.
            "var o = {}; while (true) { o[big + 'x'] = 1; }",
            "var o = {}; var i = 0; while (true) { o[big + i++] = 1; }",
            // Short rope keys, many of them.
            "var o = {}; var i = 0; while (true) { o[r + i++] = 1; }",
            // Index reads that flatten a new rope each time.
            "while (true) { (big + 'y')[5]; }",
            // Rebalances: pieces too long to merge.
            "var s = ''; while (true) { s = s + r; }",
            "var s = ''; var i = 0; while (true) { s = i++ % 2 ? r + s : s + r; }",
        ];
        for source in cases {
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
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn ropes_stay_alive_across_collections() {
    for stress in [false, true] {
        let source = "var keep = []; var s = ''; \
                      for (var i = 0; i < 3000; i++) { s = i % 3 ? s + 'ab' : 'c' + s; if (i % 500 === 0) { keep.push(s); gc(); } } \
                      var t = keep[5] + keep[3]; gc(); \
                      print(s.length, keep.length, keep[5].length, t.length, t[0], t[t.length - 1], keep[1] === keep[1] + ''); \
                      gc(); print(keep[5][1], s[s.length - 1]);";
        assert_eq!(
            run(source, stress),
            "5000 6 4168 6669 c b true\nc b",
            "stress: {stress}"
        );
    }
}

// --- String built-ins (M7 feature 5b) ---

/// A runtime with a heap limit in MiB and the test functions.
fn runtime_with_limit(mebibytes: usize) -> Runtime {
    runtime_with_tests(RuntimeConfig {
        heap: HeapConfig {
            limit: mebibytes << 20,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    })
}

#[test]
fn repeat_and_pad_to_the_length_limit_under_a_small_heap() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(16);
        // The ropes of `repeat` and `padEnd` need a few nodes; the length
        // limit is V8's RangeError, before anything is allocated.
        let build = "var r = 'ab'.repeat(2 ** 28 - 12); var p = 'x'.padEnd(2 ** 29 - 24, 'yz'); \
                     var q = 'q'.padStart(2 ** 29 - 24, 'abc'); \
                     print(r.length, p.length, q.length); \
                     var e = []; for (var f of [() => 'a'.repeat(2 ** 29 - 23), () => 'a'.padStart(2 ** 29 - 23), \
                       () => 'ab'.repeat(2 ** 28), () => 'x'.padEnd(2 ** 53), () => r.concat(r), () => r + p]) \
                       { try { f(); } catch (x) { e.push(x.name + ': ' + x.message); } } print(e.join('|'));";
        assert_eq!(
            run_in(&mut rt, build),
            "536870888 536870888 536870888\n\
             RangeError: Invalid string length|RangeError: Invalid string length|\
             RangeError: Invalid string length|RangeError: Invalid string length|\
             RangeError: Invalid string length|RangeError: Invalid string length"
        );
        // Every method that needs the code units flattens first; the flat
        // text does not fit under the heap limit, so the script ends there.
        let paths = [
            "r.indexOf('x')",
            "r.lastIndexOf('x')",
            "r.includes('x')",
            "r.startsWith('ab')",
            "r.slice(1)",
            "r.substring(1)",
            "r.trim()",
            "r.charAt(5)",
            "r.at(5)",
            "r.charCodeAt(5)",
            "r.codePointAt(5)",
            "r.split('x')",
            "r.replace('x', 'y')",
            "r.replaceAll('x', 'y')",
            "r.isWellFormed()",
            "r.toWellFormed()",
            "encodeURIComponent(r)",
            "decodeURI(r)",
            "escape(r)",
            "unescape(r)",
            "[...r]",
            "r.anchor('x')",
            "String.raw({ raw: [r, r] }, 1)",
        ];
        for source in paths {
            let result = rt.eval(source);
            assert!(
                matches!(
                    result,
                    Err(ScriptError::Terminated(Termination::HeapLimit)
                        | ScriptError::Uncaught { .. })
                ),
                "{source}: {result:?}"
            );
            assert_clean(&rt);
        }
        assert_eq!(
            run_in(&mut rt, "print(r.length, 'ab'.repeat(3))"),
            "536870888 ababab"
        );
    });
}

#[test]
fn searches_in_long_strings_stay_linear() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // A needle of 1M units that almost matches at every position: the
        // prefix-function search reads each unit once.
        let source = "var h = 'a'.repeat(4000000); var n = 'a'.repeat(1000000) + 'b'; \
                      print(h.indexOf(n), h.lastIndexOf(n), h.includes(n), h.startsWith(n), h.endsWith(n), \
                            (h + 'b').indexOf(n), (h + 'b').lastIndexOf(n), h.split(n).length, h.replaceAll(n, 'x').length);";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(
            run_in(&mut rt, ""),
            "-1 -1 false false false 3000000 3000000 1 4000000"
        );
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        assert_clean(&rt);
        // A needle just under the length of the prefix-function search
        // compares up to 15 units at each position; the loop ends at the
        // deadline, because the comparisons are charged.
        let build = "var h = 'a'.repeat(8000000); var n = 'a'.repeat(14) + 'b'; undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        for source in [
            "while (true) h.indexOf(n);",
            "while (true) h.lastIndexOf(n);",
            "while (true) h.includes(n, 1);",
            "while (true) h.split(n);",
            "while (true) h.replaceAll(n, 'x');",
        ] {
            assert_ends_at_deadline(
                &mut rt,
                source,
                Duration::from_millis(100),
                Duration::from_millis(400),
            );
        }
        // Wide and narrow strings with a wide needle.
        let wide = "var w = ('a\\u0100').repeat(3000000); var m = ('a\\u0100').repeat(500000) + 'b'; undefined;";
        assert_eq!(run_in(&mut rt, wide), "");
        assert_ends_at_deadline(
            &mut rt,
            "while (true) { w.indexOf(m); w.lastIndexOf(m); h.indexOf('\\u0100'); }",
            Duration::from_millis(100),
            Duration::from_millis(400),
        );
    });
}

#[test]
fn split_into_a_million_pieces() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var s = 'ab,'.repeat(1000000); var p = s.split(','); var u = s.slice(0, 1000000).split(''); \
                      print(p.length, p[0], p[999999], p[1000000] === '', u.length, u[999999]);";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(run_in(&mut rt, ""), "1000001 ab ab true 1000000 a");
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        assert_clean(&rt);
        for source in [
            "while (true) s.split(',');",
            "while (true) s.split('');",
            "while (true) s.split(',', 500000);",
        ] {
            assert_ends_at_deadline(
                &mut rt,
                source,
                Duration::from_millis(200),
                Duration::from_millis(400),
            );
        }
        // Under a small heap the pieces end the script at the limit.
        let mut small = runtime_with_limit(24);
        let build = "var s = 'ab,'.repeat(4000000); undefined;";
        assert_eq!(run_in(&mut small, build), "");
        for source in [
            "s.split(',')",
            "s.split('')",
            "var a = s.split(','); var b = s.split(','); a.length + b.length",
        ] {
            assert_eq!(
                small.eval(source),
                Err(ScriptError::Terminated(Termination::HeapLimit)),
                "{source}"
            );
            assert_clean(&small);
        }
        assert_eq!(run_in(&mut small, "print('x,y'.split(',').length)"), "2");
    });
}

#[test]
fn replace_all_with_many_matches() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var s = 'a'.repeat(2000000); \
                      print(s.replaceAll('a', 'bc').length, s.replaceAll('', '-').length, \
                            s.replaceAll('a', '$&$&').length, s.replaceAll('a', () => 'z').length);";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(run_in(&mut rt, ""), "4000000 4000001 4000000 2000000");
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
        assert_clean(&rt);
        // `$'` copies the tail for each match: quadratic output that the
        // length limit, the heap limit or the deadline ends.
        let quad = "var q = 'a'.repeat(200000); q.replaceAll('a', \"$'\");";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, quad, Duration::from_secs(5));
        assert!(
            matches!(
                result,
                Err(
                    ScriptError::Terminated(Termination::TimeLimit | Termination::HeapLimit)
                        | ScriptError::Uncaught { .. }
                )
            ),
            "{result:?}"
        );
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        assert_clean(&rt);
        // A template of many `$<` without a `>` is scanned once, not once
        // per `$<`.
        let named = "var t = '$<'.repeat(2000000); \
                     print('a'.replace('a', t).length, 'xay'.replace('a', '[$<n>|$<|$&]'));";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, named, Duration::from_secs(5));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(run_in(&mut rt, ""), "4000000 x[$<n>|$<|a]y");
        assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
        assert_clean(&rt);
        for source in [
            "while (true) s.replaceAll('a', 'bc');",
            "while (true) s.replaceAll('', 'x');",
            "while (true) s.replace('a', 'x');",
        ] {
            assert_ends_at_deadline(
                &mut rt,
                source,
                Duration::from_millis(200),
                Duration::from_millis(400),
            );
        }
        // A callback that is called many times, and one that throws.
        assert_ends_at_deadline(
            &mut rt,
            "var n = 0; while (true) s.replaceAll('a', function () { n++; return 'b'; });",
            Duration::from_millis(200),
            Duration::from_millis(400),
        );
        assert_eq!(
            run_in(
                &mut rt,
                "try { s.replaceAll('a', function () { throw new Error('stop'); }); } catch (e) { print(e.message); }"
            ),
            "stop"
        );
        assert_clean(&rt);
        // The output passes the heap limit before the length limit.
        let mut small = runtime_with_limit(24);
        assert_eq!(
            run_in(&mut small, "var s = 'a'.repeat(5000000); undefined;"),
            ""
        );
        assert_eq!(
            small.eval("s.replaceAll('a', 'bcdefgh')"),
            Err(ScriptError::Terminated(Termination::HeapLimit))
        );
        assert_clean(&small);
    });
}

#[test]
fn uri_and_escape_functions_on_long_input() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var s = '\\u00e9\\u4f60\\ud83d\\ude00a '.repeat(500000); \
                      var e = encodeURIComponent(s); var d = decodeURIComponent(e); \
                      var x = escape(s); var u = unescape(x); \
                      print(e.length, d === s, x.length, u === s);";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(run_in(&mut rt, ""), "15500000 true 12500000 true");
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
        assert_clean(&rt);
        for source in [
            "while (true) encodeURIComponent(s);",
            "while (true) decodeURIComponent(e);",
            "while (true) escape(s);",
            "while (true) unescape(x);",
            "while (true) { try { decodeURI(e + '%'); } catch (err) { } }",
        ] {
            assert_ends_at_deadline(
                &mut rt,
                source,
                Duration::from_millis(200),
                Duration::from_millis(400),
            );
        }
        // Malformed input of every kind is a URIError, at any length (twelve inputs, two of which become valid with the first letters of the filler: `%4a`, `%C3%aa`).
        assert_eq!(
            run_in(
                &mut rt,
                "var n = 0; for (var bad of ['%', '%4', '%G1', '%C3', '%C3%', '%C3%41', '%E0%80%80', '%ED%A0%80', '%F4%90%80%80', '%FF', '%80', '%F8%80%80%80%80']) \
                 { try { decodeURIComponent(bad + 'a'.repeat(100000)); } catch (err) { if (err instanceof URIError) n++; } } print(n);"
            ),
            "10"
        );
        // The output of an encode passes the heap limit before the length
        // limit.
        let mut small = runtime_with_limit(24);
        assert_eq!(
            run_in(&mut small, "var s = '\\u4f60'.repeat(3000000); undefined;"),
            ""
        );
        assert_eq!(
            small.eval("encodeURIComponent(s)"),
            Err(ScriptError::Terminated(Termination::HeapLimit))
        );
        assert_clean(&small);
    });
}

#[test]
fn string_raw_from_char_code_and_iteration_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        for source in [
            "while (true) String.raw({ raw: { length: 2 ** 40 } });",
            "var big = 'ab'.repeat(5000000); while (true) { for (var c of big) { } }",
            "var big = 'ab'.repeat(5000000); while (true) { [...big]; }",
            "var a = []; for (var i = 0; i < 1000000; i++) a.push(i); while (true) String.fromCharCode.apply(null, a);",
            "var a = []; for (var i = 0; i < 1000000; i++) a.push(i % 1000); while (true) String.fromCodePoint.apply(null, a);",
            "var s = ' '.repeat(5000000) + 'x' + ' '.repeat(5000000); while (true) s.trim();",
            "var s = 'a\\ud800'.repeat(3000000); while (true) { s.isWellFormed(); s.toWellFormed(); }",
        ] {
            let (result, elapsed) =
                eval_with_deadline_of(&mut rt, source, Duration::from_millis(300));
            assert!(
                matches!(
                    result,
                    Err(
                        ScriptError::Terminated(Termination::TimeLimit | Termination::HeapLimit)
                            | ScriptError::Uncaught { .. }
                    )
                ),
                "{source}: {result:?}"
            );
            assert!(elapsed < Duration::from_secs(3), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn string_methods_hold_up_under_the_stress_collector() {
    let source = "var parts = []; for (var i = 0; i < 40; i++) { \
                    var s = ('xyz' + i + '\\u0100').repeat(30); \
                    parts.push(s.replaceAll('y', function (m, p) { return '[' + p + ']'; }).split('z').join('Z').slice(5, 80).padEnd(100, '\\u0101') + \
                      encodeURIComponent(s.slice(0, 10)) + escape(s.slice(0, 8)) + s.at(-1) + [...s.slice(0, 6)].join('.')); } \
                  print(parts.length, parts[39].length, parts[0].slice(0, 30));";
    let normal = run(source, false);
    assert_eq!(run(source, true), normal);
    assert!(normal.starts_with("40 "), "{normal}");
}

// --- JSON (M7 feature 5c) ---

#[test]
fn json_nesting_costs_no_rust_stack() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // 300,000 levels: parse (iterative), the reviver walk and stringify
        // (limited to 100,000 levels, V8's RangeError), a walk down.
        let script = "var t = '['.repeat(300000) + '1' + ']'.repeat(300000); \
             var v = JSON.parse(t); var n = 0; for (var x = v; Array.isArray(x); x = x[0]) n++; print(n); \
             var e = []; \
             try { JSON.stringify(v); } catch (x) { e.push(x.name + ': ' + x.message); } \
             try { JSON.parse(t, function (k, w) { return w; }); } catch (x) { e.push(x.name + ': ' + x.message); } \
             try { JSON.parse('['.repeat(1000000)); } catch (x) { e.push(x.name + ': ' + x.message); } \
             try { JSON.parse('{\"a\":'.repeat(200000)); } catch (x) { e.push(x.name + ': ' + x.message); } \
             print(e.join('|')); \
             var w = v; for (var i = 0; i < 250000; i++) w = w[0]; print(JSON.stringify(w).length); \
             var calls = 0; var d = JSON.parse('['.repeat(3999) + ']'.repeat(3999), function (k, x) { calls++; return x; }); \
             print(calls, JSON.stringify(d).length, JSON.stringify(d, null, 1).length); \
             var f = []; \
             for (var depth of [4000, 4001]) { \
               var u = JSON.parse('['.repeat(depth) + ']'.repeat(depth)); \
               try { JSON.parse('['.repeat(depth) + ']'.repeat(depth), function (k, x) { return x; }); f.push('ok'); } catch (x) { f.push(x.name); } \
               try { JSON.stringify(u, null, 1); f.push('ok'); } catch (x) { f.push(x.name); } \
               try { JSON.stringify(u, function (k, x) { return x; }); f.push('ok'); } catch (x) { f.push(x.name); } \
               try { JSON.stringify(u, ['a']); f.push('ok'); } catch (x) { f.push(x.name); } \
               try { JSON.stringify(u); f.push('ok'); } catch (x) { f.push(x.name); } \
             } print(f.join());";
        assert_eq!(
            run_in(&mut rt, script),
            "300000\n\
             RangeError: Maximum call stack size exceeded|RangeError: Maximum call stack size exceeded|\
             SyntaxError: Unexpected end of JSON input|SyntaxError: Unexpected end of JSON input\n\
             100001\n\
             3999 7998 15999998\n\
             ok,ok,ok,ok,ok,RangeError,RangeError,RangeError,RangeError,ok"
        );
        assert_clean(&rt);
        // A replacer or `toJSON` that makes a new object at every level
        // ends with the RangeError, not with the heap limit.
        let script = "var e = []; \
             try { JSON.stringify({}, function (k, v) { return { x: v }; }); } catch (x) { e.push(x.name); } \
             var a = { toJSON: function () { return { self: a }; } }; \
             try { JSON.stringify(a); } catch (x) { e.push(x.name); } \
             function r(k, v) { return JSON.parse('[1]', r); } \
             try { JSON.parse('[1]', r); } catch (x) { e.push(x.name); } \
             var o = { toJSON: function () { return JSON.stringify(o); } }; \
             try { JSON.stringify(o); } catch (x) { e.push(x.name); } \
             print(e.join());";
        assert_eq!(
            run_in(&mut rt, script),
            "RangeError,RangeError,RangeError,RangeError"
        );
        assert_clean(&rt);
    });
}

#[test]
fn json_cycle_check_is_linear_in_deep_structures() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let script = "var chain = {}; var cur = chain; for (var i = 0; i < 90000; i++) { cur.next = {}; cur = cur.next; } \
             print(JSON.stringify(chain).length); \
             cur.back = chain; \
             try { JSON.stringify(chain); } catch (e) { var lines = e.message.split('\\n'); print(lines.length, lines[3]); } \
             cur.back = undefined; cur.again = chain.next; \
             try { JSON.stringify(chain); } catch (e) { print(e.message.split('\\n').length); }";
        let start = Instant::now();
        assert_eq!(
            run_in(&mut rt, script),
            "810002\n7     |     property 'next' -> object with constructor 'Object'\n7"
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
        assert_clean(&rt);
    });
}

#[test]
fn json_long_strings_many_keys_and_huge_output() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let script = "var s = 'ab\\u00e9\"\\\\\\n'.repeat(2 ** 22); var j = JSON.stringify(s); \
             print(s.length, j.length, JSON.parse(j) === s); \
             var big = {}; for (var i = 0; i < 300000; i++) big['key' + i] = [i, 'v' + i]; \
             var text = JSON.stringify(big); var back = JSON.parse(text, function (k, v) { return v; }); \
             print(text.length, Object.keys(back).length, back.key299999[1]); \
             var sp = JSON.stringify(big, null, 10); print(sp.length, JSON.stringify(JSON.parse(sp)) === text);";
        let result = run_in(&mut rt, script);
        assert!(
            result.starts_with("25165824 37748738 true\n8966671 300000 v299999\n"),
            "{result}"
        );
        assert_clean(&rt);
        // Output that would pass the heap limit or V8's length limit ends
        // before the allocation: a RangeError or the heap limit.
        let script = "var inner = []; for (var i = 0; i < 100000; i++) inner.push(i); \
             var v = inner; for (var i = 0; i < 3000; i++) v = [v]; \
             JSON.stringify(v, null, 10);";
        let result = rt.eval(script);
        assert!(
            matches!(
                result,
                Err(ScriptError::Terminated(Termination::HeapLimit) | ScriptError::Uncaught { .. })
            ),
            "{result:?}"
        );
        assert_clean(&rt);
        let mut small = runtime_with_limit(32);
        let script = "var s = 'x'.repeat(2 ** 26); var e = []; \
             try { JSON.stringify([s, s, s, s, s, s, s, s]); } catch (x) { e.push(x.name); } e.join();";
        let result = small.eval(script);
        assert!(
            matches!(
                result,
                Ok(_) | Err(ScriptError::Terminated(Termination::HeapLimit))
            ),
            "{result:?}"
        );
        assert_clean(&small);
    });
}

#[test]
fn json_work_ends_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            // 4 billion holes, one step each.
            "var a = []; a.length = 4294967295; JSON.stringify(a);",
            // A text with a million members, parsed again and again.
            "var parts = []; for (var i = 0; i < 1000000; i++) parts.push('\"k' + i + '\":' + i); \
             var text = '{' + parts.join(',') + '}'; \
             while (true) { JSON.parse(text, function (k, v) { return v; }); }",
            "var big = {}; for (var i = 0; i < 500000; i++) big['k' + i] = i; \
             while (true) { JSON.stringify(big, null, 2); }",
            // A text that never ends, parsed again and again.
            "var t = '[' + '1,'.repeat(1000000); while (true) { try { JSON.parse(t); } catch (e) { } }",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(
                matches!(
                    result,
                    Err(ScriptError::Terminated(Termination::TimeLimit)
                        | ScriptError::Uncaught { .. })
                ),
                "{source}: {result:?}"
            );
            assert!(elapsed < Duration::from_secs(5), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn json_revivers_that_change_the_holder() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let script = "var seen = 0; \
             var a = JSON.parse('[' + '1,'.repeat(20000) + '1]', function (k, v) { \
               seen++; if (k === '0') { this.length = 10; for (var i = 0; i < 5; i++) this.push(i); } return v; }); \
             print(seen, a.length); \
             var o = JSON.parse(JSON.stringify(Object.fromEntries(Array.from({ length: 3000 }, (_, i) => ['k' + i, i]))), \
               function (k, v) { if (k === 'k0') { for (var i = 1; i < 3000; i += 2) delete this['k' + i]; this.extra = 1; } return v; }); \
             print(Object.keys(o).length);";
        let result = run_in(&mut rt, script);
        // The array walk visits the 20,001 indices it found at the start;
        // the object walk skips what the reviver deleted.
        assert!(result.starts_with("20002 "), "{result}");
        assert_clean(&rt);
    });
}

#[test]
fn json_parse_records_count_against_the_heap_limit() {
    on_big_stack(|| {
        // The array needs about 100 MiB of heap; the parse records of the
        // reviver (outside the heap) are about 70 bytes more per value, so
        // the parse with a reviver needs about 300 MiB.
        let mut small = runtime_with_limit(128);
        let script = "var t = '[' + '1,'.repeat(3000000) + '1]'; \
                      var a = JSON.parse(t); print(a.length); \
                      JSON.parse(t, function (k, v) { return v; });";
        let result = small.eval(script);
        assert_eq!(result, Err(ScriptError::Terminated(Termination::HeapLimit)));
        assert_eq!(run_in(&mut small, ""), "3000001");
        assert_clean(&small);
    });
}

#[test]
fn json_holds_up_under_the_stress_collector() {
    let source = "var parts = []; for (var i = 0; i < 25; i++) { \
                    var v = JSON.parse('{\"a\":[' + i + ',\"s' + i + '\",{\"b\":null}],\"c\":\"x\\\\u00e9\"}', \
                      function (k, x, c) { return typeof x === 'string' ? x + (c.source || '').length : x; }); \
                    parts.push(JSON.stringify(v, function (k, x) { return typeof x === 'number' ? x * 2 : x; }, i % 4)); } \
                  print(parts.length, parts[24].length, parts[3].slice(0, 20));";
    let normal = run(source, false);
    assert_eq!(run(source, true), normal);
    assert!(normal.starts_with("25 "), "{normal}");
}

// --- Unicode string methods (M7 feature 5d) ---

#[test]
fn a_million_combining_marks_are_sorted_and_composed_in_linear_time() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Marks of two classes alternate: the canonical ordering sorts a
        // run of a million marks (not by insertion); the composition
        // reads each mark once.
        let source = "var m = 'a' + '\\u0301\\u0323'.repeat(500000); \
                      var d = m.normalize('NFD'); var c = m.normalize('NFC'); \
                      var k = ('a' + '\\u0323\\u0307'.repeat(500000)).normalize('NFC'); \
                      print(m.length, d.length, d.charCodeAt(1).toString(16), d.charCodeAt(2).toString(16), \
                            d.charCodeAt(d.length - 1).toString(16), c.length, k.length);";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(
            run_in(&mut rt, ""),
            "1000001 1000001 323 323 301 1000000 1000000"
        );
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
        // A loop over a long run ends at the deadline: the work is charged.
        assert_ends_at_deadline(
            &mut rt,
            "while (true) { d.normalize('NFC'); m.normalize('NFD'); }",
            Duration::from_millis(100),
            Duration::from_millis(600),
        );
    });
}

#[test]
fn long_texts_of_expanding_characters() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var e = '\\u0390'.repeat(2000000).toUpperCase(); \
                      var f = '\\ufdfa'.repeat(500000).normalize('NFKD'); \
                      var g = '\\u0130'.repeat(1000000).toLowerCase(); \
                      print(e.length, f.length, g.length, e.charCodeAt(5999999).toString(16), \
                            g.charCodeAt(1999999).toString(16));";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(run_in(&mut rt, ""), "6000000 9000000 2000000 301 307");
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
        // The result must fit under the heap limit: the loop ends with a
        // termination, and the heap is usable afterwards.
        let mut small = runtime_with_limit(16);
        let result = small
            .eval("var s = '\\ufdfa'.repeat(1000000); s.normalize('NFKD'); print('not reached');");
        assert_eq!(result, Err(ScriptError::Terminated(Termination::HeapLimit)));
        assert_clean(&small);
        let result = small.eval("var t = '\\u0390'.repeat(3000000); t.toUpperCase();");
        assert_eq!(result, Err(ScriptError::Terminated(Termination::HeapLimit)));
        assert_clean(&small);
        assert_eq!(run_in(&mut small, "print('ab'.toUpperCase())"), "AB");
        assert_ends_at_deadline(
            &mut rt,
            "while (true) { e.toLowerCase(); f.normalize('NFKC'); }",
            Duration::from_millis(100),
            Duration::from_millis(800),
        );
    });
}

#[test]
fn locale_compare_of_long_strings() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var s1 = 'a'.repeat(3000000), s2 = s1 + 'b'; \
                      var w1 = 'e\\u0301'.repeat(1500000), w2 = w1 + '\\u00e9'; \
                      print(s1.localeCompare(s2), s2.localeCompare(s1), s1.localeCompare(s1 + ''), \
                            w1.localeCompare(w2), w2.localeCompare(w1), w1.localeCompare('\\u00e9'.repeat(1500000)), \
                            ('x' + '\\u0300'.repeat(1000000)).localeCompare('x' + '\\u0301'.repeat(1000000)));";
        let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
        assert_eq!(result, Ok(Value::Undefined), "{elapsed:?}");
        assert_eq!(run_in(&mut rt, ""), "-1 1 0 -1 1 0 1");
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
        assert_clean(&rt);
        // Every comparison reads the strings up to the difference; a
        // loop ends at the deadline.
        assert_ends_at_deadline(
            &mut rt,
            "while (true) { s1.localeCompare(s2); w1.localeCompare(w2); }",
            Duration::from_millis(100),
            Duration::from_millis(800),
        );
    });
}

// --- Arrays (M7 feature 6a) ---

/// A termination at the time or the heap limit.
fn is_limit(result: &Result<Value, ScriptError>) -> bool {
    matches!(
        result,
        Err(ScriptError::Terminated(
            Termination::TimeLimit | Termination::HeapLimit
        ))
    )
}

#[test]
fn moving_methods_on_huge_sparse_lengths_cost_the_stored_elements() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let setup = "var a = []; a.length = 4294967295; a[3] = 'p'; a[4294967290] = 'q'; \
                     var o = {length: 9007199254740991, 2: 'x', 9007199254740900: 'y'}; ";
        let cases = [
            ("a.reverse(); Object.keys(a).join()", "4,4294967291"),
            (
                "a.shift(); Object.keys(a).join() + ' ' + a.length",
                "2,4294967289 4294967294",
            ),
            // The elements move, then the new length is a RangeError.
            (
                "var e; try { a.unshift(1, 2); } catch (x) { e = x.name; } e + ' ' + Object.keys(a).join() + ' ' + a.length",
                "RangeError 0,1,5,4294967292 4294967295",
            ),
            (
                "a.splice(1, 2); Object.keys(a).join() + ' ' + a.length",
                "1,4294967288 4294967293",
            ),
            (
                "var e; try { a.splice(1, 0, 'n'); } catch (x) { e = x.name; } e + ' ' + Object.keys(a).join() + ' ' + a.length",
                "RangeError 1,4,4294967291 4294967295",
            ),
            (
                "a.length = 4294967290; a.unshift(1, 2); Object.keys(a).join() + ' ' + a.length",
                "0,1,5 4294967292",
            ),
            ("a.copyWithin(0, 3); Object.keys(a).join()", "0,4294967287"),
            ("a.copyWithin(10, 0); Object.keys(a).join()", "3,13"),
            (
                "a.sort(); Object.keys(a).join() + ' ' + a[0] + a[1]",
                "0,1 pq",
            ),
            (
                "a.fill(0, 4294967290); Object.keys(a).join()",
                "3,4294967290,4294967291,4294967292,4294967293,4294967294",
            ),
            (
                "[a.indexOf('q'), a.lastIndexOf('p'), a.includes(undefined), a.includes('z'), a.join('').length].join()",
                "4294967290,3,true,false,2",
            ),
            (
                "var c = 0; a.forEach(function () { c++; }); a.map(String); a.filter(Boolean); a.some(Number); a.every(String); a.reduce(function (x) { return x; }); c",
                "2",
            ),
            (
                "Array.prototype.reverse.call(o); [o[9007199254740988], o[90], Object.keys(o).length].join()",
                "x,y,3",
            ),
            (
                "Array.prototype.shift.call(o); [o[1], o[9007199254740899], o.length].join()",
                "x,y,9007199254740990",
            ),
            (
                "Array.prototype.copyWithin.call(o, 0, 2, 3); [o[0], o[2]].join()",
                "x,x",
            ),
            (
                "Array.prototype.sort.call(o); [o[0], o[1], o.length].join()",
                "x,y,9007199254740991",
            ),
            (
                "[Array.prototype.lastIndexOf.call(o, 'x'), Array.prototype.indexOf.call(o, 'y'), Array.prototype.includes.call(o, 'y')].join()",
                "2,9007199254740900,true",
            ),
        ];
        for (source, expected) in cases {
            let script = format!("{setup}print(eval({source:?}));");
            let (result, elapsed) =
                eval_with_deadline_of(&mut rt, &script, Duration::from_secs(10));
            assert_eq!(result, Ok(Value::Undefined), "{source}");
            assert_eq!(run_in(&mut rt, ""), expected, "{source}");
            assert!(elapsed < Duration::from_secs(1), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        // A loop of them ends at the deadline.
        assert_ends_at_deadline(
            &mut rt,
            "var b = []; b.length = 4294967295; b[7] = 1; while (true) { b.reverse(); b.shift(); b.unshift(0); }",
            DEADLINE,
            Duration::from_millis(500),
        );
    });
}

#[test]
fn copies_of_huge_sparse_arrays_end_at_a_limit() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // These make an element for each hole: the heap limit or the
        // deadline ends them (V8 runs for minutes).
        for source in [
            "Array(4294967295).toReversed()",
            "Array(4294967295).toSorted()",
            "Array(4294967295).with(0, 1)",
            "Array(4294967295).toSpliced(0, 1)",
            "Array(4294967295).fill(0)",
            "Array.from({length: 4294967295})",
            "Array.prototype.find.call({length: 9007199254740991}, function () {})",
            "var a = Array(1000000).fill(1); while (true) a = a.concat(a);",
            "var a = [1]; while (true) a.push.apply(a, a);",
        ] {
            let (result, elapsed) =
                eval_with_deadline_of(&mut rt, source, Duration::from_millis(500));
            assert!(is_limit(&result), "{source}: {result:?}");
            assert!(elapsed < Duration::from_secs(5), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
        // A result that would pass 2^32 - 1 elements is V8's RangeError.
        assert_eq!(
            run_in(
                &mut rt,
                "var r = []; for (var s of ['Array(4294967295).concat([1])', 'Array(4294967295).toSpliced(0, 0, 1)', \
                 'Array.prototype.toReversed.call({length: 4294967296})']) { \
                 try { eval(s); r.push('no error'); } catch (e) { r.push(e.name); } } print(r.join());"
            ),
            "RangeError,RangeError,RangeError"
        );
        assert_clean(&rt);
    });
}

#[test]
fn comparators_that_change_the_array_or_lie() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            let cases = [
                // The comparator empties, grows, freezes and reshapes the
                // array; the sort writes its private copy back.
                (
                    "var a = [5, 3, 1, 4, 2]; a.sort(function (x, y) { a.length = 0; return x - y; }); a.join()",
                    "1,2,3,4,5",
                ),
                (
                    "var a = [5, 3, 1, 4, 2]; a.sort(function (x, y) { a[100] = x; return x - y; }); a.slice(0, 5).join() + ' ' + a.length",
                    "1,2,3,4,5 101",
                ),
                (
                    "var a = [5, 3, 1]; try { a.sort(function (x, y) { Object.freeze(a); return x - y; }); } catch (e) { print(e.name); } a.join()",
                    "TypeError\n5,3,1",
                ),
                (
                    "var a = [5, 3, 1]; a.sort(function (x, y) { Object.setPrototypeOf(a, {}); return x - y; }); Array.prototype.join.call(a)",
                    "1,3,5",
                ),
                (
                    "var a = [5, 3, 1]; try { a.sort(function () { throw 7; }); } catch (e) { print(e); } a.join()",
                    "7\n5,3,1",
                ),
                // Inconsistent answers still give a permutation.
                (
                    "var a = []; for (var i = 0; i < 5000; i++) a.push(i); var n = 0; a.sort(function () { n = (n * 1103515245 + 12345) % 2147483648; return (n & 4) - 2; }); \
                  var s = 0; for (var i = 0; i < a.length; i++) s += a[i]; s + ' ' + a.length",
                    "12497500 5000",
                ),
                (
                    "var a = []; for (var i = 0; i < 3000; i++) a.push(i % 7); a.sort(function () { return NaN; }); a.slice(0, 8).join()",
                    "0,1,2,3,4,5,6,0",
                ),
            ];
            for (source, expected) in cases {
                let (result, elapsed) = eval_with_deadline_of(
                    &mut rt,
                    &format!("print(eval({source:?}));"),
                    Duration::from_secs(20),
                );
                assert_eq!(result, Ok(Value::Undefined), "{source}");
                assert_eq!(run_in(&mut rt, ""), expected, "{source} (stress {stress})");
                assert!(elapsed < Duration::from_secs(10), "{source}: {elapsed:?}");
                assert_clean(&rt);
            }
        }
        // A comparator loop ends at the deadline.
        let mut rt = runtime(false);
        assert_ends_at_deadline(
            &mut rt,
            "var a = []; for (var i = 0; i < 100000; i++) a.push(i % 1000); while (true) a.sort(function (x, y) { return y - x; });",
            DEADLINE,
            Duration::from_millis(500),
        );
    });
}

#[test]
fn deep_flat_and_flat_map_use_no_rust_recursion() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        assert_eq!(
            run_in(
                &mut rt,
                "var a = [1]; for (var i = 0; i < 100000; i++) a = [a]; \
                 try { a.flat(Infinity); } catch (e) { print(e.name, e.message); } \
                 print(a.flat(8000).flat(8000).length, a.flatMap(function (x) { return x; }).length); \
                 var c = [1]; c.push(c); try { c.flat(Infinity); } catch (e) { print(e.name); }"
            ),
            "RangeError Maximum call stack size exceeded\n1 1\nRangeError"
        );
        assert_clean(&rt);
        // Wide and deep: the time countdown ends it.
        assert_ends_at_deadline(
            &mut rt,
            "var w = []; for (var i = 0; i < 1000; i++) w.push([i, [i, [i]]]); while (true) w.flat(Infinity);",
            DEADLINE,
            Duration::from_millis(500),
        );
    });
}

#[test]
fn unicode_methods_hold_up_under_the_stress_collector() {
    let source = "var parts = []; for (var i = 0; i < 25; i++) { \
                    var s = 'A\\u03a3b\\u0301\\u0323\\u00df' + 'x'.repeat(i * 1000); \
                    parts.push(s.toLowerCase().length + s.toUpperCase().length + s.normalize('NFD').length \
                      + s.normalize('NFKC').length + s.localeCompare(s + 'y') + s.toLocaleUpperCase().length); } \
                  print(parts.length, parts[24], parts.join().length);";
    let normal = run(source, false);
    assert_eq!(run(source, true), normal);
    assert!(normal.starts_with("25 "), "{normal}");
}

#[test]
fn chains_of_deferred_and_bound_calls_use_no_rust_recursion() {
    on_big_stack(|| {
        // A native that calls its argument (`call`, `apply`,
        // `Reflect.apply`), bound to the previous function, 200,000 times.
        // Each level was a Rust recursion without a charge (found in the
        // 6c review: 10,000 levels overflowed the stack and aborted the
        // process). The calls are tail calls now, in one window that each
        // level replaces. Chromium 148 returns the result too.
        let mut rt = runtime_with_limit(256);
        let cases = [
            "Function.prototype.call.bind(f)",
            "Function.prototype.apply.bind(f)",
            "Reflect.apply.bind(null, f, null, [])",
            "Function.prototype.call.bind(f, null)",
        ];
        for make in cases {
            let source = format!(
                "var f = function () {{ return 7; }}; for (var i = 0; i < 200000; i++) f = {make}; f() + f()"
            );
            let (result, elapsed) =
                eval_with_deadline_of(&mut rt, &source, Duration::from_secs(30));
            assert_eq!(result, Ok(Value::Int(14)), "{make}");
            assert!(elapsed < Duration::from_secs(10), "{make}: {elapsed:?}");
            assert_clean(&rt);
        }
        // An error at the end of the chain, a non-constructor in a
        // construct chain, and a generator resumed through tail calls.
        let checks = [
            (
                "var g = function () { throw 3; }; for (var i = 0; i < 100000; i++) g = Function.prototype.call.bind(g); try { g(); 0 } catch (e) { e }",
                Value::Int(3),
            ),
            (
                "var c = function () { this.x = 1; }; for (var i = 0; i < 1000; i++) c = Reflect.construct.bind(null, c, []); try { c(); 0 } catch (e) { e instanceof TypeError ? 5 : 6 }",
                Value::Int(5),
            ),
            (
                "function* gen() { yield 4; } var it = gen(); Reflect.apply(Function.prototype.call, it.next, [it]).value",
                Value::Int(4),
            ),
        ];
        for (source, expected) in checks {
            let (result, _) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(30));
            assert_eq!(result, Ok(expected), "{source}");
            assert_clean(&rt);
        }
        // Endless or quadratic tail calls without frames end at the
        // deadline: an argument list that contains itself, and `call`
        // over a million copies of `call` (each level copies the rest).
        let endless = [
            "var a = [Reflect.apply, null, null]; a[2] = a; Reflect.apply(Reflect.apply, null, a)",
            "var c = Function.prototype.call; var args = []; for (var i = 0; i < 1000000; i++) args.push(c); args.push(function () { return 5; }); c.apply(c, args)",
        ];
        for source in endless {
            let (result, elapsed) =
                eval_with_deadline_of(&mut rt, source, Duration::from_millis(500));
            assert!(
                matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
                "{source}: {result:?}"
            );
            assert!(elapsed < Duration::from_secs(5), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn collection_churn_keeps_the_memory_bounded() {
    on_big_stack(|| {
        // A million inserts and deletes of keys of every kind finish under
        // a heap limit of 24 MiB: the table follows its live entries.
        let mut rt = runtime_with_limit(24);
        let cases = [
            "var m = new Map(); for (var i = 0; i < 1000000; i++) { m.set(i, i); if (i >= 5) m.delete(i - 5); } m.size",
            "var m = new Map(); for (var i = 0; i < 300000; i++) { m.set('key' + i, i); if (i >= 5) m.delete('key' + (i - 5)); } m.size",
            "var s = new Set(); for (var i = 0; i < 1000000; i++) { s.add(i + 0.5); if (i >= 5) s.delete(i - 4.5); } s.size",
            "var s = new Set(); for (var i = 0; i < 300000; i++) { var o = {}; s.add(o); s.delete(o); } s.size",
            // With iterators in flight, the positions follow every compaction.
            "var m = new Map(); var its = []; for (var j = 0; j < 20; j++) its.push(m.keys()); for (var i = 0; i < 500000; i++) { m.set(i, i); m.delete(i - 3); } its.length + m.size",
            "var m = new Map(); for (var r = 0; r < 20; r++) { for (var i = 0; i < 20000; i++) m.set(i, i); m.clear(); } m.size",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
            assert!(result.is_ok(), "{source}: {result:?}");
            assert!(elapsed < Duration::from_secs(10), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn abandoned_iterators_do_not_fill_the_heap() {
    on_big_stack(|| {
        // Millions of iterators, finished or not, over a table that
        // lives: the heap limit of 24 MiB holds because the collector
        // frees the iterators and the table prunes its list.
        let mut rt = runtime_with_limit(24);
        let cases = [
            "var m = new Map([[1, 1], [2, 2]]); for (var i = 0; i < 1500000; i++) { var it = m.entries(); } 1",
            "var m = new Map([[1, 1], [2, 2]]); for (var i = 0; i < 1000000; i++) { var it = m.keys(); it.next(); } 1",
            "var s = new Set([1, 2, 3]); for (var i = 0; i < 1000000; i++) { for (var v of s) { break; } } 1",
            "var s = new Set([1, 2, 3]); for (var i = 0; i < 500000; i++) { for (var v of s) { } s.forEach(function () { }); } 1",
            // A table that changes while the iterators are made.
            "var m = new Map(); for (var i = 0; i < 500000; i++) { var it = m.keys(); m.set(i, i); m.delete(i - 1); } m.size",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(30));
            assert!(result.is_ok(), "{source}: {result:?}");
            assert!(elapsed < Duration::from_secs(15), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn queues_and_churn_with_unfinished_iterators_stay_linear() {
    on_big_stack(|| {
        // Found in the 6b review: the unfinished iterators block the
        // compaction until a collection, so the deleted entries pile up.
        // Each new iterator starts after the deleted entries at the start
        // (the LRU idiom), a delete takes the key out of the index (a key
        // deleted and added again), and deleted entries further on hold
        // skips (a full iteration per step). All three were quadratic:
        // 400,000 steps of the first took 13.7 s, 3 million of the second
        // over 2 minutes, 400,000 of the third 14 s.
        let mut rt = runtime_with_limit(64);
        let cases = [
            "var m = new Map(); for (var i = 0; i < 300000; i++) { m.set(i, i); if (m.size > 10) m.delete(m.keys().next().value); } m.size",
            "var s = new Set(); for (var i = 0; i < 300000; i++) { s.add(i); if (s.size > 10) { for (var v of s) { s.delete(v); break; } } } s.size",
            "var m = new Map(); for (var i = 0; i < 10; i++) m.set(i, i); for (var i = 0; i < 1000000; i++) { var it = m.keys(); it.next(); m.delete(i % 10); m.set(i % 10, 1); } m.size",
            // Deleted entries in the middle: a full iteration jumps over them.
            "var m = new Map([[-1, 1]]); for (var i = 0; i < 300000; i++) { m.keys(); m.set(i, 1); m.delete(i); for (var k of m) { } } m.size",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(30));
            assert!(result.is_ok(), "{source}: {result:?}");
            assert!(elapsed < Duration::from_secs(10), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn many_live_iterators_make_no_quadratic_work() {
    on_big_stack(|| {
        // 100,000 iterators that stay alive, and a table that changes
        // 500,000 times: a compaction costs its entries plus its iterators
        // and runs only when the deleted entries are as many as the
        // iterators, so the work stays linear.
        let mut rt = runtime_with_limit(128);
        let (result, elapsed) = eval_with_deadline_of(
            &mut rt,
            "var m = new Map([[0, 0]]); var its = []; for (var j = 0; j < 100000; j++) its.push(m.keys()); \
             for (var i = 1; i < 500000; i++) { m.set(i, i); m.delete(i - 1); } \
             var s = 0; for (var j = 0; j < its.length; j += 1000) s += its[j].next().value; s",
            Duration::from_secs(30),
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
        assert_clean(&rt);
        // Clearing is constant work, not one step per iterator.
        let (result, elapsed) = eval_with_deadline_of(
            &mut rt,
            "var big = new Set(); for (var i = 0; i < 100000; i++) { big.add(i); } var its = []; \
             for (var j = 0; j < 50000; j++) its.push(big.values()); \
             for (var r = 0; r < 100000; r++) { big.clear(); big.add(r); } 1",
            Duration::from_secs(30),
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    });
}

#[test]
fn collections_with_many_keys_end_at_the_heap_limit() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(32);
        let cases = [
            "var m = new Map(); for (var i = 0; ; i++) m.set(i, i);",
            "var m = new Map(); for (var i = 0; ; i++) m.set('a long key to cost memory ' + i, {});",
            "var s = new Set(); for (var i = 0; ; i++) s.add({});",
            "var s = new Set(); for (var i = 0; ; i++) s.add(i + 0.25);",
            "var big = []; for (var i = 0; i < 100000; i++) big.push(i); var s = new Set(big); while (true) { s = s.union(new Set(big.map(function (x) { return x + s.size; }))); }",
            "var s = new Set([1]); while (true) { s = s.symmetricDifference({ size: 1, has: function () { return false; }, keys: function () { return { next: function () { return { value: Math.random(), done: false }; } }; } }); }",
            "var m = Map.groupBy({ [Symbol.iterator]: function () { var i = 0; return { next: function () { return { value: i++, done: false }; } }; } }, function (x) { return x; });",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(10));
            assert!(is_limit(&result), "{source}: {result:?}");
            assert!(elapsed < Duration::from_secs(10), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn colliding_looking_keys_do_not_make_collections_slow() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(256);
        // Keys that a weak hash puts in a few buckets: multiples of powers
        // of two, doubles with equal low bits, strings with a common
        // prefix and suffix, and many symbols.
        let cases = [
            "var m = new Map(); for (var i = 0; i < 200000; i++) m.set(i * 65536, i); var s = 0; for (var i = 0; i < 200000; i++) s += m.get(i * 65536); s",
            "var m = new Map(); for (var i = 0; i < 200000; i++) m.set(i * 4294967296, i); var s = 0; for (var i = 0; i < 200000; i++) s += m.get(i * 4294967296); s",
            "var m = new Set(); for (var i = 0; i < 200000; i++) m.add(i * 1.5e300); m.size",
            "var m = new Set(); for (var i = 0; i < 100000; i++) m.add(-i * 8); m.size",
            "var m = new Map(); for (var i = 0; i < 200000; i++) m.set('same prefix, same suffix ' + (100000 + i) + ' and the same end', i); m.size",
            "var m = new Map(); for (var i = 0; i < 200000; i++) m.set(String.fromCharCode(i & 0xffff, i >> 16), i); m.size",
            "var m = new Map(); for (var i = 0; i < 200000; i++) m.set(Symbol(), i); m.size",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline_of(&mut rt, source, Duration::from_secs(20));
            assert!(result.is_ok(), "{source}: {result:?}");
            assert!(elapsed < Duration::from_secs(5), "{source}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn collection_loops_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(256);
        let cases = [
            // Loops that add entries as they go never end by themselves.
            "var m = new Map([[0, 0]]); m.forEach(function (v, k) { m.set(k + 1, 0); m.delete(k); });",
            "var s = new Set([0]); for (var v of s) { s.add(v + 1); s.delete(v); }",
            "var s = new Set([0]); s.forEach(function (v) { s.delete(v); s.add(v); });",
            "var m = new Map([[0, 0]]); for (var e of m) { m.delete(e[0]); m.set(e[0], 0); }",
            "var s = new Set([0]); while (true) { var r = s.union({ size: 0, has: function () { return false; }, keys: function () { return { next: function () { return { value: 1, done: false }; } }; } }); }",
            "var s = new Set([1]); while (true) { s.isSupersetOf({ size: 0, has: function () { return true; }, keys: function () { return { next: function () { return { value: 1, done: false }; }, return: function () { return {}; } }; } }); }",
            "var s = new Set(); for (var i = 0; i < 1000; i++) s.add(i); var o = { size: 1e9, has: function (v) { return true; }, keys: function () { return [][Symbol.iterator](); } }; while (true) { s.isSubsetOf(o); s.intersection(o); }",
            // Long runs of deleted entries that iterators skip.
            "var m = new Map(); for (var i = 0; i < 300000; i++) m.set(i, i); var its = []; for (var j = 0; j < 400000; j++) its.push(m.keys()); for (var i = 0; i < 299999; i++) m.delete(i); while (true) { for (var k = 0; k < its.length; k++) its[k].next(); }",
        ];
        for source in cases {
            assert_ends_at_deadline(&mut rt, source, DEADLINE, Duration::from_millis(500));
        }
    });
}

#[test]
fn stress_mode_agrees_on_collections() {
    let source = "var m = new Map(); var its = []; for (var i = 0; i < 400; i++) { m.set('k' + (i % 150), { i: i }); if (i % 3 === 0) m.delete('k' + ((i * 7) % 150)); if (i % 50 === 0) its.push(m.entries()); } \
                  var n = 0; for (var it of its) { for (var e of it) n += e[1].i; } \
                  var s = new Set(m.keys()); var t = s.union(new Set(['x', 'y'])).difference(new Set(['k1', 'k2'])); \
                  var g = Map.groupBy(m.keys(), function (k) { return k.length; }); \
                  print(m.size, n, s.size, t.size, [...g.keys()].join(), g.get(3).length);";
    let normal = run(source, false);
    assert_eq!(run(source, true), normal);
    assert!(normal.starts_with("124 "), "{normal}");
}

// --- Iterator helpers (M7 feature 6c) ---

#[test]
fn deep_helper_chains_end_with_a_catchable_error() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(256);
        // A chain of 100,000 helpers of each kind is made without
        // recursion; the first `next`, `return` or `toArray` that goes
        // through all of it ends with a `RangeError` (V8 does the same).
        // The helpers that did start stay usable afterwards.
        for (name, make) in [
            ("map", "it = it.map(function (x) { return x; });"),
            ("filter", "it = it.filter(function (x) { return true; });"),
            ("take", "it = it.take(5);"),
            ("drop", "it = it.drop(0);"),
            ("flatMap", "it = it.flatMap(function (x) { return [x]; });"),
        ] {
            for use_it in ["it.next()", "it.return()", "it.toArray()", "it.next()"] {
                let source = format!(
                    "var it = [1, 2, 3].values(); for (var i = 0; i < 100000; i++) {{ {make} }} \
                     var r; try {{ r = {use_it}; }} catch (e) {{ r = e.name + ': ' + e.message; }} String(r)"
                );
                let (result, elapsed) =
                    eval_with_deadline_of(&mut rt, &source, Duration::from_secs(20));
                assert!(
                    result.is_ok() || is_limit(&result),
                    "{name} {use_it}: {result:?}"
                );
                assert!(elapsed < Duration::from_secs(15), "{name}: {elapsed:?}");
                assert_clean(&rt);
            }
        }
        // The error is the stack error, and the runtime still works.
        assert_eq!(
            run_in(
                &mut rt,
                "var it = [1].values(); for (var i = 0; i < 100000; i++) it = it.map(function (x) { return x; }); \
                 try { it.next(); } catch (e) { print(e.name, e.message); } \
                 print([1, 2].values().map(function (x) { return x * 2; }).toArray().join());"
            ),
            "RangeError Maximum call stack size exceeded\n2,4"
        );
        assert_clean(&rt);
    });
}

#[test]
fn flat_map_of_deeply_nested_iterators_and_wide_concats() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(256);
        assert_eq!(
            run_in(
                &mut rt,
                "var a = [1]; for (var i = 0; i < 100000; i++) a = [a]; \
                 var f = function (x) { return Array.isArray(x) ? x.values().flatMap(f) : [x]; }; \
                 try { a.values().flatMap(f).next(); } catch (e) { print(e.name, e.message); } \
                 var g = function (x) { return x; }; var h = a.values(); \
                 for (var i = 0; i < 100000; i++) h = h.flatMap(g); \
                 try { h.next(); } catch (e) { print(e.name); } \
                 var parts = []; for (var i = 0; i < 200000; i++) parts.push([i]); \
                 print(Iterator.concat.apply(null, parts).toArray().length); \
                 print(Iterator.concat.apply(null, parts).map(function (x) { return x; }).drop(199990).toArray().join());"
            ),
            "RangeError Maximum call stack size exceeded\nRangeError\n200000\n199990,199991,199992,199993,199994,199995,199996,199997,199998,199999"
        );
        assert_clean(&rt);
        // Many arguments for `concat`: a result, or the limit of the
        // stack for arguments.
        let (result, _) = eval_with_deadline_of(
            &mut rt,
            "var parts = new Array(4000000).fill([1]); try { Iterator.concat.apply(null, parts).next(); } catch (e) { e.name }",
            Duration::from_secs(20),
        );
        assert!(result.is_ok() || is_limit(&result), "{result:?}");
        assert_clean(&rt);
    });
}

#[test]
fn endless_iterators_end_at_the_time_or_heap_limit() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(64);
        let endless = "var n = 0; var it = { __proto__: Iterator.prototype, next: function () { return { done: false, value: n++ }; } }; ";
        let cases = [
            "it.toArray()",
            "it.take(Infinity).toArray()",
            "it.take(Number.MAX_SAFE_INTEGER).toArray()",
            "it.map(function (x) { return { x: x }; }).toArray()",
            "it.drop(Infinity).next()",
            "it.drop(Number.MAX_SAFE_INTEGER).next()",
            "it.filter(function () { return false; }).next()",
            "it.flatMap(function () { return []; }).next()",
            "it.flatMap(function (x) { return [x, x]; }).toArray()",
            "it.forEach(function () { })",
            "it.some(function () { return false; })",
            "it.every(function () { return true; })",
            "it.find(function () { return false; })",
            "it.reduce(function (a, b) { return a + b; }, 0)",
            "Iterator.from(it).toArray()",
            "Iterator.concat(it, it).toArray()",
            "Iterator.concat({ [Symbol.iterator]: function () { return it; } }).map(function (x) { return [x]; }).toArray()",
            "it.map(function (x) { return x; }).filter(function () { return true; }).drop(3).toArray()",
        ];
        for case in cases {
            let source = format!("{endless}{case}");
            let (result, elapsed) =
                eval_with_deadline_of(&mut rt, &source, Duration::from_millis(300));
            assert!(is_limit(&result), "{case}: {result:?}");
            assert!(elapsed < Duration::from_secs(5), "{case}: {elapsed:?}");
            assert_clean(&rt);
        }
        // The same over a built-in iterator that is never exhausted.
        let (result, _) = eval_with_deadline_of(
            &mut rt,
            "var a = [1]; var it = a.values(); var h = it.map(function (x) { a.push(x); return x; }); h.toArray()",
            Duration::from_millis(300),
        );
        assert!(is_limit(&result), "{result:?}");
        assert_clean(&rt);
    });
}

#[test]
fn huge_counts_and_endless_empty_inner_iterators() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(64);
        assert_eq!(
            run_in(
                &mut rt,
                "var a = [1, 2, 3]; \
                 print(a.values().take(Infinity).toArray().join(), a.values().take(Number.MAX_SAFE_INTEGER).toArray().join(), \
                       a.values().drop(Infinity).toArray().length, a.values().drop(Number.MAX_SAFE_INTEGER).toArray().length, \
                       a.values().take(2 ** 52).drop(2 ** 52).toArray().length); \
                 try { a.values().take(2 ** 53); } catch (e) { print(e.name); } \
                 try { a.values().drop(1e300); } catch (e) { print(e.name); }"
            ),
            "1,2,3 1,2,3 0 0 0\nRangeError\nRangeError"
        );
        assert_clean(&rt);
        // A finite source of empty inner iterables is a finite loop.
        let (result, elapsed) = eval_with_deadline_of(
            &mut rt,
            "var big = new Array(300000).fill(0); big.values().flatMap(function () { return []; }).toArray().length",
            Duration::from_secs(20),
        );
        assert_eq!(result, Ok(Value::Int(0)));
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
        assert_clean(&rt);
    });
}

#[test]
fn reentrant_helpers_keep_their_state() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(64);
        // A `next` or `return` that runs inside the helper's own step is a
        // `TypeError`, a hundred thousand times over, and the helper keeps
        // working afterwards.
        assert_eq!(
            run_in(
                &mut rt,
                "var h, bad = 0, calls = 0; \
                 var it = { __proto__: Iterator.prototype, i: 0, \
                   next: function () { calls++; try { h.next(); } catch (e) { bad++; } try { h.return(); } catch (e) { bad++; } return { done: this.i >= 100000, value: this.i++ }; }, \
                   return: function () { try { h.next(); } catch (e) { bad++; } return {}; } }; \
                 h = it.map(function (x) { try { h.next(); } catch (e) { bad++; } return x; }); \
                 var sum = 0; for (var x of h) sum += x; \
                 print(sum, bad, calls); \
                 var g = it.take(1); g.next(); g.return(); print(bad);"
            ),
            "4999950000 300002 100001\n300002"
        );
        assert_clean(&rt);
    });
}

#[test]
fn helper_loops_end_at_the_deadline_with_throwing_callbacks() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(64);
        // Callbacks that throw close the iterator; a `return` that never
        // finishes quickly still ends at the deadline.
        let cases = [
            "while (true) { try { [1, 2].values().map(function () { throw 1; }).next(); } catch (e) { } }",
            "while (true) { try { Iterator.prototype.take.call({ return: function () { return {}; } }, NaN); } catch (e) { } }",
            "while (true) { [1, 2, 3].values().flatMap(function (x) { return [x]; }).return(); }",
            "var it = { __proto__: Iterator.prototype, next: function () { return { done: false, value: 1 }; }, return: function () { return {}; } }; while (true) { var h = it.map(function (x) { return x; }); h.next(); h.return(); }",
            "while (true) { Iterator.concat([1], [2], [3]).toArray(); }",
            "while (true) { Iterator.from({ next: function () { return { done: true }; } }).toArray(); }",
        ];
        for source in cases {
            assert_ends_at_deadline(&mut rt, source, DEADLINE, Duration::from_millis(500));
        }
    });
}

#[test]
fn stress_mode_agrees_on_helpers() {
    let source = "var log = []; \
                  function mk(n) { var i = 0; return { __proto__: Iterator.prototype, next: function () { return { done: i >= n, value: { n: i++, s: 'v' + i } }; }, return: function () { log.push('r' + i); return {}; } }; } \
                  var a = mk(40).map(function (o) { return { w: o, t: 'x' + o.n }; }).filter(function (m) { return m.w.n % 3 !== 0; }).drop(2).take(15).toArray(); \
                  var b = mk(10).flatMap(function (o) { return mk(3).map(function (p) { return o.n * 10 + p.n; }); }).toArray(); \
                  var c = Iterator.concat([1, 2], mk(3).map(function (o) { return o.s; }), new Set([9])).toArray(); \
                  var d = mk(50).reduce(function (acc, o) { return { sum: acc.sum + o.n, last: o.s }; }, { sum: 0 }); \
                  print(a.length, a[14].t, b.length, b[29], c.join(), d.sum, d.last, log.join());";
    let normal = run(source, false);
    assert_eq!(run(source, true), normal);
    assert!(normal.starts_with("15 "), "{normal}");
}

#[test]
fn deep_yield_star_chains_end_with_a_catchable_error() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(256);
        // A chain of 100,000 generators that each delegate to the next
        // (made by a loop) and a recursion of 100,000 levels through
        // `yield*`: every use that goes through the whole chain ends with
        // the frame limit's `RangeError` (Chromium 148 too), never with a
        // Rust stack overflow, and the runtime stays usable.
        let chain = "function* leaf() { yield 1; yield 2; } var it = leaf(); \
                     for (var i = 0; i < 100000; i++) { const inner = it; it = (function* () { yield* inner; })(); } ";
        let recursion = "function* g(n) { if (n > 0) yield* g(n - 1); else yield 'bottom'; } var it = g(100000); ";
        for setup in [chain, recursion] {
            for use_it in [
                "it.next()",
                "(it.next(), it.return(5))",
                "(it.next(), it.throw(5))",
                "[...it]",
                "(function () { for (var x of it) { } })()",
                "it.return(5)",
                "it.throw(5)",
            ] {
                let source = format!(
                    "{setup} var r; try {{ r = JSON.stringify({use_it}); }} catch (e) {{ r = e && e.name ? e.name + ': ' + e.message : 'threw ' + e; }} r"
                );
                let (result, elapsed) =
                    eval_with_deadline_of(&mut rt, &source, Duration::from_secs(20));
                let Ok(value) = result else {
                    panic!("{use_it}: {result:?}");
                };
                let text = rt.display(value);
                assert!(
                    text.contains("RangeError: Maximum call stack size exceeded")
                        || text.contains("\"done\":true")
                        || text.contains("threw 5"),
                    "{use_it}: {text}"
                );
                assert!(elapsed < Duration::from_secs(10), "{use_it}: {elapsed:?}");
                assert_clean(&rt);
            }
        }
        // A generator that delegates to a new instance of itself.
        assert_eq!(
            run_in(
                &mut rt,
                "function* self() { yield* self(); } try { self().next(); } catch (e) { print(e.name, e.message); } \
                 function* two() { yield* [1, 2]; } print([...two()].join());"
            ),
            "RangeError Maximum call stack size exceeded\n1,2"
        );
        assert_clean(&rt);
    });
}

#[test]
fn misbehaving_delegates_in_hot_loops() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(64);
        // Delegates whose methods throw, return non-objects, are not
        // callable or are getters that throw: each `yield*` fails cleanly,
        // over and over in a loop that ends at the deadline.
        let delegates = [
            "{ [Symbol.iterator]() { return this; }, next() { throw new Error('n'); } }",
            "{ [Symbol.iterator]() { return this; }, next() { return 1; } }",
            "{ [Symbol.iterator]() { return this; }, next: 5 }",
            "{ [Symbol.iterator]() { return 5; } }",
            "{ [Symbol.iterator]() { return this; }, get next() { throw 1; } }",
            "{ [Symbol.iterator]() { return this; }, next() { return { get done() { throw 2; } }; } }",
            "{ [Symbol.iterator]() { return this; }, next() { return { done: true, get value() { throw 3; } }; } }",
        ];
        for delegate in delegates {
            let source = format!(
                "var d = {delegate}; function* g() {{ yield* d; }} var n = 0; \
                 while (true) {{ try {{ g().next(); }} catch (e) {{ n++; }} }}"
            );
            assert_ends_at_deadline(&mut rt, &source, DEADLINE, Duration::from_millis(500));
            assert_clean(&rt);
        }
        // `throw` and `return` that misbehave, on one suspended generator
        // after another.
        let abrupt = [
            (
                "throw",
                "{ [Symbol.iterator]() { return this; }, next() { return { done: false }; }, throw() { return 1; } }",
            ),
            (
                "throw",
                "{ [Symbol.iterator]() { return this; }, next() { return { done: false }; }, return() { return 1; } }",
            ),
            (
                "throw",
                "{ [Symbol.iterator]() { return this; }, next() { return { done: false }; }, return() { throw 4; } }",
            ),
            (
                "throw",
                "{ [Symbol.iterator]() { return this; }, next() { return { done: false }; }, throw: 5 }",
            ),
            (
                "return",
                "{ [Symbol.iterator]() { return this; }, next() { return { done: false }; }, return() { return null; } }",
            ),
            (
                "return",
                "{ [Symbol.iterator]() { return this; }, next() { return { done: false }; }, get return() { throw 6; } }",
            ),
            (
                "return",
                "{ [Symbol.iterator]() { return this; }, next() { return { done: false }; }, return() { return { done: false }; } }",
            ),
        ];
        for (method, delegate) in abrupt {
            let source = format!(
                "var d = {delegate}; function* g() {{ yield* d; }} var n = 0; \
                 while (true) {{ var it = g(); it.next(); try {{ it.{method}(0); }} catch (e) {{ n++; }} }}"
            );
            assert_ends_at_deadline(&mut rt, &source, DEADLINE, Duration::from_millis(500));
            assert_clean(&rt);
        }
    });
}

#[test]
fn reentrant_next_through_a_delegate() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(64);
        // The delegate calls the delegating generator's `next`, `throw`
        // and `return`: each is "Generator is already running", 100,000
        // times, and the generator keeps working.
        assert_eq!(
            run_in(
                &mut rt,
                "var o, bad = 0, i = 0; \
                 var d = { [Symbol.iterator]() { return this; }, \
                   next() { for (var m of ['next', 'throw', 'return']) { try { o[m](); } catch (e) { if (e.message === 'Generator is already running') bad++; } } \
                     return { done: i >= 100000, value: i++ }; } }; \
                 function* g() { return yield* d; } o = g(); var sum = 0; for (var x of o) sum += x; \
                 print(sum, bad, JSON.stringify(o.next()));"
            ),
            "4999950000 300003 {\"done\":true}"
        );
        assert_clean(&rt);
    });
}

#[test]
fn endless_delegates_end_at_the_time_or_heap_limit() {
    on_big_stack(|| {
        let mut rt = runtime_with_limit(64);
        let endless = "var endless = { [Symbol.iterator]() { return this; }, next() { return { done: false, value: 1 }; } }; \
                       function* forever() { while (true) yield 1; } \
                       function* wrap(k, src) { if (k === 0) yield* src; else yield* wrap(k - 1, src); } ";
        let cases = [
            "[...(function* () { yield* endless; })()]",
            "for (var x of (function* () { yield* endless; })()) { }",
            "[...wrap(10, forever())]",
            "for (var x of wrap(10, endless)) { }",
            "var it = wrap(50, forever()); while (true) it.next();",
            "(function* () { while (true) yield* [1, 2, 3]; })().toArray()",
            "(function* () { while (true) yield* []; })().next()",
            "(function* () { while (true) yield* ''; })().next()",
            "while (true) wrap(10, endless).next();",
            "Array.from(wrap(5, forever()))",
            "new Set(wrap(5, (function* () { var i = 0; while (true) yield i++; })()))",
        ];
        for case in cases {
            let source = format!("{endless}{case}");
            let (result, elapsed) =
                eval_with_deadline_of(&mut rt, &source, Duration::from_millis(300));
            assert!(is_limit(&result), "{case}: {result:?}");
            assert!(elapsed < Duration::from_secs(5), "{case}: {elapsed:?}");
            assert_clean(&rt);
        }
    });
}

#[test]
fn stress_mode_agrees_on_yield_star() {
    let source = "var log = []; \
                  function mk(n) { var i = 0; return { [Symbol.iterator]() { return this; }, next(v) { log.push('n' + (v && v.k)); return { done: i >= n, value: { n: i++ } }; }, \
                    return(v) { log.push('r'); return { done: true, value: v }; }, throw(e) { log.push('t'); return { done: true, value: { n: -1 } }; } }; } \
                  function* mid(k) { var r = yield* (k ? mid(k - 1) : mk(5)); return { n: r.n + 1 }; } \
                  var a = []; var it = mid(5); var x; while (!(x = it.next({ k: a.length })).done) a.push(x.value.n); \
                  var b = mid(3); b.next(); var c = b.throw(1); var d = mid(3); d.next(); var e = d.return({ n: 'R' }); \
                  var GF = Object.getPrototypeOf(function* () {}).constructor; var f = [...GF('a', 'yield* a; yield* \"xy\"')([1, 2])]; \
                  print(a.join(), x.value.n, JSON.stringify(c), JSON.stringify(e), f.join(), log.join());";
    let normal = run(source, false);
    assert_eq!(run(source, true), normal);
    assert!(normal.starts_with("0,1,2,3,4 "), "{normal}");
}
