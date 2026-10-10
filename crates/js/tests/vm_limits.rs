//! Limits and hostile cases of the compiler and the interpreter (ADR 0026
//! section 9): deep recursion, native re-entry, deep nesting, the value
//! stack, the heap limit, the time limit and termination, constructs
//! outside the subset, and the API around them. All run on a thread with an 8 MiB stack; in a debug build
//! they also show that no case overflows the Rust stack.

mod common;

use std::fmt::Write;
use std::time::{Duration, Instant};

use common::{on_big_stack, run, run_in, runtime, runtime_with};
use swb_js::{
    HeapConfig, NativeCall, NativeReturn, RecursionBudget, Runtime, RuntimeConfig, ScriptError,
    Termination, ThrowKind, Value, VmResult,
};

const STACK_OVERFLOW: &str = "Uncaught RangeError: Maximum call stack size exceeded";

#[test]
fn deep_recursion_is_a_range_error() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let out = run_in(&mut rt, "function f(n) { return f(n + 1) + 1; } f(0)");
        assert_eq!(out, STACK_OVERFLOW);
        // The runtime stays usable; the frames and the stack are empty.
        assert_eq!(
            run_in(
                &mut rt,
                "function g(n) { return n ? g(n - 1) + 1 : 0; } g(9000)"
            ),
            "=> 9000"
        );
        assert_eq!(rt.recursion_budget(), RecursionBudget::DEFAULT);
    });
}

#[test]
fn getter_recursion_uses_frames_not_the_rust_stack() {
    // A getter that reads itself is a chain of deferred calls.
    assert_eq!(
        run(
            "var o = {}; defineAccessor(o, 'x', function () { return this.x; }); o.x",
            false
        ),
        STACK_OVERFLOW
    );
    assert_eq!(
        run(
            "var o = {}; defineAccessor(o, 'x', undefined, function (v) { this.x = v; }); o.x = 1",
            false
        ),
        STACK_OVERFLOW
    );
    // Deferred calls through a native: also frames.
    assert_eq!(
        run("function f() { return later(f); } f()", false),
        STACK_OVERFLOW
    );
}

#[test]
fn native_reentry_charges_the_recursion_budget() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let out = run_in(
            &mut rt,
            "function f(n) { return callTwice(f, n + 1); } f(0)",
        );
        assert_eq!(out, STACK_OVERFLOW);
        assert_eq!(rt.recursion_budget(), RecursionBudget::DEFAULT);
        // Re-entry through ToPrimitive charges it too.
        let out = run_in(
            &mut rt,
            "var o = {valueOf: function () { return o + 1; }}; o + 1",
        );
        assert_eq!(out, STACK_OVERFLOW);
        assert_eq!(rt.recursion_budget(), RecursionBudget::DEFAULT);
        // A moderate depth works.
        assert_eq!(
            run_in(
                &mut rt,
                "function g(n) { return n == 0 ? 1 : callTwice(g, n - 1); } g(8)"
            ),
            "=> 256"
        );
    });
}

#[test]
fn the_frame_and_stack_limits_are_settings() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            frame_limit: 100,
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        let depth = |rt: &mut Runtime, n: u32| {
            rt.eval(&format!(
                "function f(n) {{ return n ? f(n - 1) + 1 : 0; }} f({n})"
            ))
        };
        assert_eq!(depth(&mut rt, 98), Ok(Value::Int(98)));
        assert!(matches!(
            depth(&mut rt, 100),
            Err(ScriptError::Uncaught { ref message, .. }) if message == "Maximum call stack size exceeded"
        ));
        let config = RuntimeConfig {
            stack_limit: 1000,
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // About 4 values per frame: the limit of 1000 values comes first.
        assert!(rt.eval("function f(n) { return f(n + 1); } f(0)").is_err());
        assert_eq!(rt.eval("1 + 1"), Ok(Value::Int(2)));
    });
}

#[test]
fn deep_recursion_in_stress_mode() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                stress: true,
                ..HeapConfig::default()
            },
            frame_limit: 300,
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        let result =
            rt.eval("function f(n) { var o = {n: n}; return n ? f(n - 1) + o.n : 0; } f(250)");
        assert_eq!(result, Ok(Value::Int(31375)));
        assert!(
            rt.eval("function g(n) { var o = {}; return g(n + 1); } g(0)")
                .is_err()
        );
        assert_eq!(rt.heap().stats().total_stale_roots, 0);
    });
}

#[test]
fn suspended_generators_survive_collections() {
    // 50 generators, each with a captured counter and a local object,
    // advanced in turns with collections between; other calls use the
    // value stack while they are suspended.
    let source = "
        function make(k) {
            var count = 0;
            function* gen() {
                var local = {k: k};
                while (true) { count++; yield local.k * 1000 + count; }
            }
            return {gen: gen(), peek: function () { return count; }};
        }
        function deep(n) { return n ? deep(n - 1) + 1 : 0; }
        var all = [];
        for (var i = 0; i < 50; i++) all[i] = make(i);
        var sum = 0;
        for (var round = 0; round < 4; round++) {
            for (var j = 0; j < 50; j++) {
                sum += all[j].gen.next().value;
                deep(20);
            }
            gc();
        }
        print(sum, all[7].peek(), all[49].gen.next().value);
    ";
    for stress in [false, true] {
        assert_eq!(run(source, stress), "4900500 4 49005");
    }
}

#[test]
fn uncaught_errors_report_the_offset() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "var a = 1;\nfunction f() { return null.x; }\nf();";
        let Err(ScriptError::Uncaught {
            offset, message, ..
        }) = rt.eval(source)
        else {
            panic!("an uncaught error");
        };
        assert_eq!(message, "Cannot read properties of null (reading 'x')");
        // The position of a property read is the name, as in V8.
        let expected = source.find("null.x").unwrap() as u32 + "null.".len() as u32;
        assert_eq!(offset, Some(expected));
        // The error object is the last value.
        let Value::Object(_) = rt.last_value() else {
            panic!("the error object");
        };
        let Err(ScriptError::Uncaught { offset, .. }) = rt.eval("\n\n  throw 1") else {
            panic!("an uncaught value");
        };
        assert_eq!(offset, Some(4));
    });
}

#[test]
fn global_declarations_conflict_across_scripts() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        assert_eq!(run_in(&mut rt, "let a = 1; var b = 2; const c = 3;"), "");
        assert_eq!(
            run_in(&mut rt, "let a = 5;"),
            "Uncaught SyntaxError: Identifier 'a' has already been declared"
        );
        assert_eq!(
            run_in(&mut rt, "var a;"),
            "Uncaught SyntaxError: Identifier 'a' has already been declared"
        );
        assert_eq!(
            run_in(&mut rt, "let b;"),
            "Uncaught SyntaxError: Identifier 'b' has already been declared"
        );
        assert_eq!(run_in(&mut rt, "a + b + c"), "=> 6");
        assert_eq!(
            run_in(&mut rt, "c = 4"),
            "Uncaught TypeError: Assignment to constant variable."
        );
        // A failed declaration does not create any binding of its script.
        assert_eq!(
            run_in(&mut rt, "let d = 1; let a = 2;"),
            "Uncaught SyntaxError: Identifier 'a' has already been declared"
        );
        assert_eq!(run_in(&mut rt, "typeof d"), "=> undefined");
        // A script that ends early leaves its later lexical bindings in
        // the dead zone (as in V8).
        assert_eq!(
            run_in(&mut rt, "null.x; let e = 1;"),
            "Uncaught TypeError: Cannot read properties of null (reading 'x')"
        );
        assert_eq!(
            run_in(&mut rt, "e"),
            "Uncaught ReferenceError: Cannot access 'e' before initialization"
        );
        // Functions see global bindings of later scripts.
        assert_eq!(run_in(&mut rt, "function g() { return later; }"), "");
        assert_eq!(run_in(&mut rt, "let later = 'yes'; g()"), "=> yes");
    });
}

#[test]
fn constructs_outside_the_subset_do_not_compile() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // The parser accepts the forms of M7 feature 1a; the compiler
        // rejects them until feature 3.
        for (source, construct) in [
            ("({ get x() { return 1; } })", "getter or setter"),
            ("var r = /a/;", "regular expression literal"),
            ("var [a] = [1];", "destructuring"),
            ("let {b} = {};", "destructuring"),
            ("[a] = [1];", "destructuring assignment"),
            ("function f(a = 1) {}", "default value"),
            ("function f(...a) {}", "rest element"),
            ("(([a]) => a)", "destructuring"),
            ("try {} catch ([e]) {}", "destructuring"),
            ("f(...a);", "spread"),
            ("[...a];", "spread"),
            ("({...a});", "object spread"),
            ("a?.b;", "optional chaining"),
            ("a`x`;", "tagged template"),
            ("function f() { return new.target; }", "new.target"),
            ("1n;", "BigInt literal"),
            ("({ 1n: 2 });", "BigInt literal"),
            ("function* g() { yield* a; }", "yield*"),
            ("for (var k in {});", "for-in"),
            ("for (var k of []);", "for-of"),
            ("with ({}) {}", "with"),
            // The forms of feature 1b.
            ("class A {}", "class"),
            ("x = class { m() {} };", "class"),
            ("async function f() {}", "async function"),
            ("x = async () => 1;", "async function"),
            ("x = { async m() {} };", "async function"),
            ("x = { m() { return super.x; } };", "super"),
            // The first construct in the source counts.
            ("x = 1; for (k of a?.b);", "for-of"),
            ("x = async function () { await 1; };", "async function"),
            (
                "x = async function () { for await (k of a); };",
                "async function",
            ),
        ] {
            let Err(ScriptError::Compile { kind, message, .. }) = rt.eval(source) else {
                panic!("{source} compiled");
            };
            assert_eq!(kind, ThrowKind::SyntaxError);
            assert_eq!(message, format!("not supported yet ({construct})"));
        }
        let arguments = vec!["0"; 70_000].join(",");
        let Err(error) = rt.eval(&format!("f({arguments})")) else {
            panic!("70,000 arguments compiled");
        };
        assert_eq!(
            error.to_string(),
            "SyntaxError: Too many arguments in function call (only 65535 allowed)"
        );
    });
}

#[test]
fn the_compiler_checks_the_register_total() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // 64,000 declared registers (the analysis allows 64,511) and a call
        // that needs 2,002 temporaries: more than 65,535 in all.
        let mut vars = String::new();
        for i in 0..64_000 {
            let _ = write!(vars, "var v{i};");
        }
        let args = vec!["0"; 2000].join(",");
        let source = format!("function f() {{ {vars} g({args}); }}");
        let Err(error) = rt.eval(&source) else {
            panic!("too many registers compiled");
        };
        assert_eq!(
            error.to_string(),
            "SyntaxError: Expression needs too many registers (only 65535 allowed in a function)"
        );
        // Without the call it compiles and runs.
        let source = format!("function f() {{ {vars} return 1; }} f()");
        assert_eq!(rt.eval(&source), Ok(Value::Int(1)));
    });
}

#[test]
fn deep_nesting_compiles_or_fails_cleanly() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases: Vec<(String, &str)> = vec![
            (format!("{}1{}", "(".repeat(1000), ")".repeat(1000)), "=> 1"),
            (
                format!("{}1{}", "[".repeat(1000), "]".repeat(1000)),
                "=> [object Array]",
            ),
            (format!("{}0", "1 ? 0 : ".repeat(1300)), "=> 0"),
            (format!("var a; {}1", "a = ".repeat(1000)), "=> 1"),
            (format!("{}1", "- ".repeat(2000)), "=> 1"),
            (format!("{}1{}", "{".repeat(1000), "}".repeat(1000)), "=> 1"),
            (
                format!(
                    "{}1{}",
                    "(function () { return ".repeat(300),
                    "; })()".repeat(300)
                ),
                "=> 1",
            ),
            // Long left-associative chains compile in a loop.
            (format!("0{}", " + 1".repeat(15_000)), "=> 15000"),
            (
                format!("var a = 1; if (a{}) 'y'", " && a".repeat(15_000)),
                "=> y",
            ),
            (format!("0{}", " || 0".repeat(15_000)), "=> 0"),
        ];
        for (source, expected) in cases {
            let out = run_in(&mut rt, &source);
            assert_eq!(out, expected, "{}", &source[..40.min(source.len())]);
            assert_eq!(rt.recursion_budget(), RecursionBudget::DEFAULT);
        }
        // Deeper than the budget: a RangeError, from the parser or the
        // compiler, never a crash.
        for source in [
            format!("{}1", "- ".repeat(20_000)),
            format!("var o = {{}}; o.a = o; o{}", ".a".repeat(15_000)),
            format!("{}1{}", "f(".repeat(5000), ")".repeat(5000)),
        ] {
            let Err(error) = rt.eval(&source) else {
                panic!("a nest beyond the budget compiled");
            };
            assert_eq!(
                error.to_string(),
                "RangeError: Maximum call stack size exceeded"
            );
        }
    });
}

#[test]
fn the_heap_limit_ends_a_growing_script() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            heap: HeapConfig {
                limit: 32 << 20,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        // Doubling a string: the concatenation reserves first.
        assert_eq!(
            rt.eval("var s = 'x'; while (true) s = s + s;"),
            Err(ScriptError::Terminated(Termination::HeapLimit))
        );
        // Allocating objects in a loop: a backward jump is a safepoint.
        assert_eq!(
            rt.eval("var keep = []; var i = 0; while (true) { keep[i] = {i: i}; i++; }"),
            Err(ScriptError::Terminated(Termination::HeapLimit))
        );
        // The runtime is usable after the garbage is gone.
        assert_eq!(rt.eval("s = 0; keep = 0; 1 + 1"), Ok(Value::Int(2)));
        rt.collect();
        assert!(rt.heap().heap_size() < 4 << 20);
    });
}

#[test]
fn the_host_calls_script_functions() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Values that the API returns live until the next run; globals
        // keep these alive.
        let holder = rt.eval("var holder = {k: 2}; holder").unwrap();
        let f = rt
            .eval("var fn = function (a, b) { return a * b + this.k; }; fn")
            .unwrap();
        assert_eq!(
            rt.call_function(f, holder, &[Value::Int(6), Value::Int(7)]),
            Ok(Value::Int(44))
        );
        let generator = rt
            .eval("var it = (function* () { yield 1; yield 2; })(); it")
            .unwrap();
        let next = rt.eval("it.next").unwrap();
        let first = rt.call_function(next, generator, &[]).unwrap();
        assert!(matches!(first, Value::Object(_)));
        assert_eq!(run_in(&mut rt, "it.next().value"), "=> 2");
        assert!(matches!(
            rt.call_function(Value::Int(1), Value::Undefined, &[]),
            Err(ScriptError::Uncaught { ref message, .. }) if message == "1 is not a function"
        ));
    });
}

#[test]
fn call_arguments_and_variables_have_their_own_limits() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // V8: more than 65,535 arguments is a SyntaxError with this text.
        for callee in ["f", "new f"] {
            let source = format!("{callee}({})", vec!["0"; 65_536].join(","));
            let Err(error) = rt.eval(&source) else {
                panic!("65,536 arguments compiled");
            };
            assert_eq!(
                error.to_string(),
                "SyntaxError: Too many arguments in function call (only 65535 allowed)"
            );
        }
        // 65,535 arguments are legal in V8 but need more registers than
        // the 16-bit operands address: the register message, not the
        // message about variables.
        let source = format!("f({})", vec!["0"; 65_535].join(","));
        let Err(error) = rt.eval(&source) else {
            panic!("65,535 arguments compiled");
        };
        assert_eq!(
            error.to_string(),
            "SyntaxError: Expression needs too many registers (only 65535 allowed in a function)"
        );
        // More declared variables than registers: the message about
        // variables, also without any call.
        let mut vars = String::new();
        for i in 0..70_000 {
            let _ = write!(vars, "var v{i};");
        }
        let Err(error) = rt.eval(&format!("function f() {{ {vars} }}")) else {
            panic!("70,000 variables compiled");
        };
        assert_eq!(
            error.to_string(),
            "SyntaxError: Too many variables declared in a function"
        );
    });
}

#[test]
fn global_declarations_are_all_checked_before_any_is_created() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // §16.1.7 steps 11 and 12: `undefined` cannot be redeclared as a
        // function, so `fa` and `vb` are not created either. (V8 creates
        // the bindings that precede the failing one.)
        let out = run_in(
            &mut rt,
            "var vb = 1; function fa() {} function undefined() {}",
        );
        assert_eq!(
            out,
            "Uncaught SyntaxError: Identifier 'undefined' has already been declared"
        );
        assert_eq!(
            run_in(&mut rt, "print(typeof fa, typeof vb)"),
            "undefined undefined"
        );
        // A later valid script declares them as usual.
        assert_eq!(run_in(&mut rt, "var vb = 2; function fa() {} vb"), "=> 2");
    });
}

/// `reenter(f, n)`: calls `f` with the number 5 as `this` n times and
/// returns the growth of the native's handle scope.
fn reenter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let f = rt.arg(call, 0);
    let n = rt.arg(call, 1).as_number().unwrap_or(0.0) as usize;
    let before = rt.heap().scope_len();
    for _ in 0..n {
        rt.call(f, Value::Int(5), &[])?;
    }
    let grown = rt.heap().scope_len() - before;
    Ok(NativeReturn::Value(Value::from(grown as f64)))
}

#[test]
fn a_sloppy_this_wrapper_is_not_recorded_in_the_scope_of_a_native() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            rt.define_global_function("reenter", 2, reenter).unwrap();
            // Each call records its result (one handle); the wrapper object
            // of `this` is rooted by the frame and needs none.
            let out = run_in(
                &mut rt,
                "function f() { return typeof this; } reenter(f, 200)",
            );
            assert_eq!(out, "=> 200", "stress: {stress}");
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}

#[allow(clippy::unnecessary_wraps)]
fn bad_later(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Call {
        callee: call.callee().into(),
        this: Value::Undefined,
        args: vec![Value::Empty],
    })
}

#[test]
fn the_host_cannot_pass_internal_values_to_script() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let f = rt.eval("var id = function (a) { return a; }; id").unwrap();
        let bad = Value::Empty;
        assert!(rt.call_function(f, Value::Undefined, &[bad]).is_err());
        assert!(rt.call_function(f, bad, &[]).is_err());
        assert!(rt.call_function(bad, Value::Undefined, &[]).is_err());
        // A deferred call from a native checks its values too.
        rt.define_global_function("badLater", 0, bad_later).unwrap();
        let Err(error) = rt.eval("badLater()") else {
            panic!("an internal value reached script");
        };
        assert!(matches!(error, ScriptError::Internal(_)), "{error:?}");
        assert_eq!(rt.eval("id(3)"), Ok(Value::Int(3)));
    });
}

#[test]
fn the_stack_limit_is_clamped() {
    on_big_stack(|| {
        let config = RuntimeConfig {
            stack_limit: usize::MAX,
            ..RuntimeConfig::default()
        };
        let mut rt = runtime_with(config);
        assert_eq!(rt.eval("1 + 1"), Ok(Value::Int(2)));
    });
}

// --- Termination, the time limit, exceptions at the limits ---

/// The deadline of the hostile cases (the tests check that a run ends
/// within a few seconds of it, also in a debug build).
const DEADLINE: Duration = Duration::from_millis(200);

/// Runs `source` with a deadline; returns the result and the time it took.
fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + DEADLINE));
    let result = rt.eval(source);
    rt.set_deadline(None);
    (result, start.elapsed())
}

/// The state that every run must leave behind: no frames, an empty value
/// stack, no open handle scope, no no-GC region.
fn assert_clean(rt: &Runtime) {
    assert_eq!(rt.stack_depths(), (0, 0));
    assert_eq!(rt.heap().scope_depth(), 0);
    assert_eq!(rt.heap().no_gc_depth(), 0);
}

#[test]
fn endless_scripts_end_at_the_deadline() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let cases = [
            "for (;;) {}",
            "while (true) { try {} finally {} }",
            "var x = 0; while (true) { try { x++; } catch (e) {} }",
            "function f() { f(); } for (;;) { try { f(); } catch (e) {} }",
            "function g() { return g(); } while (true) { try { g(); } catch (e) { continue; } }",
            "var a = []; a.length = 4294967295; a.forEach(function () {})",
            "var a = []; a.length = 4294967295; a.join('')",
            "var a = []; a.length = 4294967295; a.map(function (x) { return x; })",
            "var o = {length: 4294967295}; Array.prototype.forEach.call(o, function () {})",
            "function* g() { while (true) yield 1; } var it = g(); while (true) it.next();",
            "var o = {}; defineAccessor(o, 'x', function () { return 1; }); while (true) o.x;",
            "function f() { return later(f); } while (true) { try { f(); } catch (e) {} }",
            "while (true) { [1, 2, 3].forEach(function () {}); }",
            "outer: while (true) { switch (1) { case 1: continue outer; } }",
        ];
        for source in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(elapsed < Duration::from_secs(5), "{source}: {elapsed:?}");
            assert_clean(&rt);
            assert_eq!(rt.eval("1 + 1"), Ok(Value::Int(2)));
        }
    });
}

#[test]
fn termination_skips_handlers_and_finally_blocks() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            let cases = [
                // In a `try` with `catch` and `finally`.
                "var ran = []; try { for (;;) {} } catch (e) { ran.push('catch'); } finally { ran.push('finally'); }",
                // Inside a `finally` block.
                "try { throw 1; } finally { for (;;) {} }",
                // Inside a native callback, in a `try` outside.
                "try { [1, 2].forEach(function () { for (;;) {} }); } catch (e) { ran.push('outer catch'); }",
                // Inside a callback that a native calls through re-entry.
                "try { callTwice(function () { for (;;) {} }, 1); } finally { ran.push('finally 2'); }",
                // Inside a generator.
                "var running = (function* () { try { yield 1; for (;;) {} } finally { ran.push('gen finally'); } })(); running.next(); running.next();",
            ];
            rt.eval("var ran = [];").unwrap();
            for source in cases {
                let (result, _) = eval_with_deadline(&mut rt, source);
                assert_eq!(
                    result,
                    Err(ScriptError::Terminated(Termination::TimeLimit)),
                    "{source}"
                );
                assert_clean(&rt);
            }
            // No handler and no `finally` block ran.
            assert_eq!(run_in(&mut rt, "ran.length"), "=> 0");
            // The generator that was running is closed.
            assert_eq!(
                run_in(&mut rt, "var r = running.next(); print(r.value, r.done)"),
                "undefined true"
            );
            rt.collect();
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}

#[test]
fn suspended_generators_resume_after_a_termination() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            rt.eval(
                "function* g() { var n = 0; try { while (true) yield n++; } finally { print('closed'); } }
                 var a = g(); a.next(); a.next();",
            )
            .unwrap();
            let (result, _) = eval_with_deadline(&mut rt, "a.next(); for (;;) {}");
            assert_eq!(result, Err(ScriptError::Terminated(Termination::TimeLimit)));
            assert_clean(&rt);
            gc_now(&mut rt);
            assert_eq!(
                run_in(&mut rt, "print(a.next().value); a.return(9).value"),
                "3\nclosed\n=> 9"
            );
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    });
}

/// A collection with the runtime's roots (also in stress mode, where the
/// heap collects at every safepoint anyway).
fn gc_now(rt: &mut Runtime) {
    let stats = rt.collect();
    assert_eq!(stats.last_stale_roots, 0);
}

#[test]
fn another_thread_can_end_a_script() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let handle = rt.termination_handle();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            handle.terminate();
        });
        let start = Instant::now();
        assert_eq!(
            rt.eval("var n = 0; while (true) { try { n++; } finally { n--; } }"),
            Err(ScriptError::Terminated(Termination::HostRequest))
        );
        assert!(start.elapsed() < Duration::from_secs(5));
        stopper.join().unwrap();
        // The request is used up; the next script runs.
        assert!(!rt.termination_handle().is_requested());
        assert_eq!(run_in(&mut rt, "n"), "=> 0");
        assert_clean(&rt);
        // A request also ends a run of the host through a native call.
        rt.termination_handle().terminate();
        let f = rt.eval("(function () { for (;;) {} })").unwrap();
        assert_eq!(
            rt.call_function(f, Value::Undefined, &[]),
            Err(ScriptError::Terminated(Termination::HostRequest))
        );
        assert_clean(&rt);
    });
}

#[test]
fn a_million_throws_keep_memory_steady() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.eval("function thrower(i) { throw new Error('e' + i); }")
            .unwrap();
        rt.collect();
        let before = rt.heap().heap_size();
        let out = run_in(
            &mut rt,
            "var caught = 0; for (var i = 0; i < 1000000; i++) { try { thrower(i); } catch (e) { caught++; } finally { } } caught",
        );
        assert_eq!(out, "=> 1000000");
        assert_clean(&rt);
        rt.collect();
        let after = rt.heap().heap_size();
        assert!(
            after < before + (64 << 10),
            "the heap grew from {before} to {after} bytes"
        );
        // Raised errors (created only when caught) and thrown primitives
        // through native frames.
        let out = run_in(
            &mut rt,
            "var n = 0; for (var i = 0; i < 100000; i++) { try { null.x; } catch (e) { n++; } try { callTwice(function () { throw i; }, 0); } catch (e) { n++; } } n",
        );
        assert_eq!(out, "=> 200000");
        assert_clean(&rt);
    });
}

#[test]
fn deep_joins_and_catches_stay_inside_the_recursion_budget() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // Nested arrays: each level of `join` re-enters through `toString`.
        let out = run_in(
            &mut rt,
            "var a = []; for (var i = 0; i < 100000; i++) a = [a]; try { a.join(); } catch (e) { print(e.name, e.message); } 'after'",
        );
        assert_eq!(out, "RangeError Maximum call stack size exceeded\n=> after");
        assert_eq!(rt.recursion_budget(), RecursionBudget::DEFAULT);
        // A stack overflow in native re-entry, caught in script code, then
        // again.
        let out = run_in(
            &mut rt,
            "function f(n) { return callTwice(f, n + 1); } var k = 0; for (var i = 0; i < 3; i++) { try { f(0); } catch (e) { k++; } } k",
        );
        assert_eq!(out, "=> 3");
        assert_eq!(rt.recursion_budget(), RecursionBudget::DEFAULT);
        assert_clean(&rt);
        // A cycle through two arrays.
        let out = run_in(&mut rt, "var x = [1]; var y = [x, 2]; x.push(y); String(x)");
        assert_eq!(out, "=> 1,,2");
    });
}

#[test]
fn uncaught_errors_through_finally_and_natives_report_the_offset() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // A throw that passes a `finally` block: the offset is where the
        // `finally` block rethrows it. (V8 reports the original throw;
        // swb keeps no message location yet.)
        let source = "try {\n  throw new TypeError('t');\n} finally {\n  var x = 1;\n}";
        let Err(ScriptError::Uncaught {
            name,
            message,
            offset,
        }) = rt.eval(source)
        else {
            panic!("an uncaught error");
        };
        assert_eq!((name.as_str(), message.as_str()), ("TypeError", "t"));
        assert!(offset.is_some());
        // An error that a native raises: the offset of the call (V8
        // reports the name of the method).
        let source = "var a = 1;\n[1].forEach(2);";
        let Err(ScriptError::Uncaught {
            message, offset, ..
        }) = rt.eval(source)
        else {
            panic!("an uncaught error");
        };
        assert_eq!(message, "number 2 is not a function");
        assert_eq!(offset, Some(source.find("forEach").unwrap() as u32));
        // A caught error leaves no offset behind.
        assert_eq!(
            rt.eval("try { null.x; } catch (e) { 5 }"),
            Ok(Value::Int(5))
        );
        assert_clean(&rt);
    });
}

/// `scopeLen()`: the number of values in all open handle scopes.
#[allow(clippy::unnecessary_wraps)]
fn scope_len(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::from(
        rt.heap().scope_len() as f64
    )))
}

#[test]
fn natives_that_call_back_do_not_grow_their_scope() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        rt.define_global_function("scopeLen", 0, scope_len).unwrap();
        // The scope sizes seen by the callback stay the same over many
        // elements, in `forEach`, `map` and `join` (through `toString`).
        let out = run_in(
            &mut rt,
            "var a = []; for (var i = 0; i < 5000; i++) a[i] = i;
             var sizes = [];
             a.forEach(function (x) { if (x == 1 || x == 4999) sizes.push(scopeLen()); });
             a.map(function (x) { if (x == 1 || x == 4999) sizes.push(scopeLen()); return {x: x}; });
             var b = []; for (var j = 0; j < 5000; j++) b[j] = {toString: function () { if (this.i == 1 || this.i == 4999) sizes.push(scopeLen()); return ''; }, i: j};
             b.join();
             print(sizes[0] == sizes[1], sizes[2] == sizes[3], sizes[4] == sizes[5], sizes.length)",
        );
        assert_eq!(out, "true true true 6");
    });
}

#[test]
fn handler_tables_are_verified() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let listing = rt
            .disassemble("try { f(); } catch (e) { g(e); } finally { h(); }")
            .unwrap();
        assert!(listing.contains("EndFinally"), "{listing}");
        // Deep nesting of `try` compiles within the budget or fails with a
        // RangeError, never a crash.
        let depth = 3000;
        let source = format!(
            "{}{}",
            "try { ".repeat(depth),
            "} finally { }".repeat(depth)
        );
        match rt.eval(&source) {
            Ok(_) => {}
            Err(ScriptError::Compile { kind, .. }) => assert_eq!(kind, ThrowKind::RangeError),
            Err(other) => panic!("{other:?}"),
        }
        let mut source = String::new();
        for i in 0..200 {
            let _ = write!(source, "try {{ if (n == {i}) throw {i}; ");
        }
        for _ in 0..200 {
            source.push_str("} finally { n += 1000; } ");
        }
        let out = run_in(
            &mut rt,
            &format!("var n = 150; try {{ {source} }} catch (e) {{ print(e, n); }}"),
        );
        assert_eq!(out, "150 151150");
    });
}

// --- Work-proportional time charges ---
//
// Each script runs an endless loop whose single iteration is big work for
// one native call or one operator. A deadline of 100 ms must end it. Bound
// of the test: 2 x the deadline + 50 ms in a release build; 1 s in a
// debug build (the dev profile is optimised, but it checks overflow and
// the machine may be loaded).

fn ends_by_deadline(src: &str) -> Duration {
    let deadline = Duration::from_millis(100);
    let bound = if cfg!(debug_assertions) {
        Duration::from_secs(1)
    } else {
        deadline * 2 + Duration::from_millis(50)
    };
    let mut rt = runtime(false);
    rt.set_deadline(Some(Instant::now() + deadline));
    let start = Instant::now();
    let result = rt.eval(src);
    let took = start.elapsed();
    assert!(
        matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
        "{result:?} for {src}"
    );
    assert!(took < bound, "{took:?} (bound {bound:?}) for {src}");
    took
}

const BIG_STRING: &str = "var s='x'; for(var i=0;i<24;i++) s=s+s; ";

#[test]
fn time_limit_object_keys_of_a_big_object() {
    ends_by_deadline("var o={}; for(var i=0;i<100000;i++) o['k'+i]=i; for(;;){ Object.keys(o) }");
}

#[test]
fn time_limit_concatenation_of_big_strings() {
    ends_by_deadline(&format!("{BIG_STRING}for(;;){{ var t=s+'a' }}"));
}

#[test]
fn time_limit_join_of_big_strings() {
    ends_by_deadline(&format!("{BIG_STRING}for(;;){{ [s,s].join('') }}"));
}

#[test]
fn time_limit_join_with_a_big_separator() {
    ends_by_deadline(&format!("{BIG_STRING}for(;;){{ [1,2].join(s) }}"));
}

#[test]
fn time_limit_console_log_of_a_big_array() {
    ends_by_deadline(
        "var a=[]; for(var i=0;i<100000;i++) a.push(String(i)); for(;;){ console.log(a) }",
    );
}

#[test]
fn time_limit_console_log_of_a_big_object() {
    ends_by_deadline("var o={}; for(var i=0;i<100000;i++) o['k'+i]=i; for(;;){ console.log(o) }");
}

#[test]
fn time_limit_object_keys_of_a_big_string_object() {
    ends_by_deadline(&format!("{BIG_STRING}Object.keys(new String(s))"));
}

#[test]
fn time_limit_push_with_many_arguments() {
    ends_by_deadline(
        "var a=[], args=[]; for(var i=0;i<60000;i++) args.push(i); for(;;){ a.push.apply(a,args) }",
    );
}

#[test]
fn time_limit_apply_with_a_big_array_like() {
    ends_by_deadline(
        "var args=[]; for(var i=0;i<60000;i++) args.push(i); function f(){} for(;;){ f.apply(null,args) }",
    );
}

#[test]
fn console_log_cuts_long_keys() {
    let mut rt = runtime(false);
    let out = run_in(
        &mut rt,
        "var o={}, k='x'; for(var i=0;i<20;i++) k=k+k; for(var j=0;j<3;j++) o[k+j]=1; console.log(o)",
    );
    // Three keys of 10,000 units each, with the rest of the name counted.
    assert!(out.len() < 100_000, "{}", out.len());
    assert!(
        out.contains("... 1038577 more characters"),
        "{}",
        &out[..60]
    );
}

#[test]
fn console_log_stops_a_huge_structure() {
    let mut rt = runtime(false);
    let out = run_in(
        &mut rt,
        "var s='x'; for(var i=0;i<13;i++) s=s+s; \
         var a=[]; for(var i=0;i<100;i++) a.push(s); \
         var b=[]; for(var i=0;i<100;i++) b.push(a); \
         var c=[]; for(var i=0;i<100;i++) c.push(b); console.log(c)",
    );
    assert!(out.len() < 20_000_000, "{}", out.len());
}
