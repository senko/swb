//! Limits and hostile cases of the compiler and the interpreter (ADR 0026
//! section 9): deep recursion, native re-entry, deep nesting, the value
//! stack, the heap limit, constructs outside the subset, and the API
//! around them. All run on a thread with an 8 MiB stack; in a debug build
//! they also show that no case overflows the Rust stack.

mod common;

use std::fmt::Write;

use common::{error_text, on_big_stack, run, run_in, runtime};
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
        let mut rt = Runtime::new(config).unwrap();
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
        let mut rt = Runtime::new(config).unwrap();
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
        let mut rt = Runtime::new(config).unwrap();
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
        let expected = source.find("null.x").unwrap() as u32;
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
        for (source, construct) in [
            ("try { } catch (e) { }", "try statement"),
            ("var r = /a/;", "regular expression literal"),
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
            error_text(&error),
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
            error_text(&error),
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
                error_text(&error),
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
        let mut rt = Runtime::new(config).unwrap();
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
                error_text(&error),
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
            error_text(&error),
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
            error_text(&error),
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
        let mut rt = Runtime::new(config).unwrap();
        assert_eq!(rt.eval("1 + 1"), Ok(Value::Int(2)));
    });
}
