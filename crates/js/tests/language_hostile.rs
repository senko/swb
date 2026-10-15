//! Hostile cases for the language forms of M7 feature 3a: `for`-`in` over
//! huge objects, long prototype chains and long String objects, object
//! spread of huge objects, deep optional chains and tagged templates in
//! hot loops; and of the later sessions of feature 3 (classes, private
//! names, `eval` and `with`, the `Function` constructor and mapped
//! `arguments`). A case ends with a result, an error or a termination at the
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
