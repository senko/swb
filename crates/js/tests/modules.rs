//! Modules without other modules (ADR 0028 section 6, item 7;
//! `Runtime::eval_module_at`): strict code with `this` undefined whose
//! top-level declarations belong to the module; the `import` and `export`
//! forms, `import()`, `import.meta` and top-level `await` do not compile.
//! The expected values are Chromium's (`tools/probes/web-module-scripts.json`,
//! case `semantics`). Each test runs with and without the GC stress mode.

#![allow(clippy::unwrap_used)]

mod common;

use std::fmt::Write;

use common::{Output, on_big_stack, runtime};
use swb_js::{Runtime, ScriptError, ThrowKind};

/// Runs `source` as a module: the printed lines, then the error if any.
fn run_module_in(rt: &mut Runtime, source: &str) -> String {
    let result = rt
        .eval_module_at(source, "m.js", (0, 0))
        .and_then(|value| rt.run_jobs().map(|()| value));
    let mut lines = rt.host_data_mut::<Output>().unwrap().take();
    if let Err(error) = result {
        lines.push(error.to_string());
    }
    lines.join("\n")
}

/// Runs a module in a new runtime, with and without the stress mode.
fn run_module(source: &str) -> String {
    let source = source.to_owned();
    on_big_stack(move || {
        let mut outputs = Vec::new();
        for stress in [false, true] {
            let mut rt = runtime(stress);
            outputs.push(run_module_in(&mut rt, &source));
            assert_eq!(rt.heap().stats().total_stale_roots, 0, "stale roots");
        }
        assert_eq!(outputs[0], outputs[1], "the stress mode changes the result");
        outputs.remove(0)
    })
}

#[test]
fn this_is_undefined_and_the_code_is_strict() {
    assert_eq!(
        run_module(
            "print(this === undefined, (function () { return this === undefined })());
             print((function () { return typeof this }).call(5));
             try { undeclared1 = 1; print('sloppy') } catch (e) { print(e.name) }
             print(typeof arguments);
             print(eval('var ev = 1; typeof ev'), typeof ev);
             print(globalThis === this, typeof globalThis);",
        ),
        "true true\nnumber\nReferenceError\nundefined\nnumber undefined\nfalse object"
    );
}

#[test]
fn top_level_declarations_stay_in_the_module() {
    assert_eq!(
        run_module(
            "var v1 = 1; let l1 = 2; const c1 = 3; function f1() {} class K1 {}
             print(['v1', 'l1', 'c1', 'f1', 'K1'].map(k => k in globalThis).join());
             print(typeof hoisted, typeof hoisted2); function hoisted() {} var hoisted2 = 1;
             try { tdz } catch (e) { print(e.name) } let tdz = 1;
             globalThis.fromModule = 1;
             globalThis.getter = function () { return v1 + l1 + c1 };
             print(getter(), typeof K1, typeof f1);",
        ),
        "false,false,false,false,false\nfunction undefined\nReferenceError\n6 function function"
    );
}

#[test]
fn a_second_module_does_not_see_the_declarations_of_the_first() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        assert_eq!(
            run_module_in(
                &mut rt,
                "var shared = 1; let other = 2; globalThis.seen = 3;"
            ),
            ""
        );
        assert_eq!(
            run_module_in(&mut rt, "print(typeof shared, typeof other, typeof seen)"),
            "undefined undefined number"
        );
        assert_eq!(
            run_module_in(&mut rt, "print(shared)"),
            "Uncaught ReferenceError: shared is not defined"
        );
        // A classic script does not see them either.
        assert!(rt.eval("typeof shared + typeof other").is_ok());
        assert_eq!(run_module_in(&mut rt, "print(typeof shared)"), "undefined",);
    });
}

#[test]
fn errors_name_the_module_and_the_position() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let error = rt
            .eval_module_at("\n  null.x;", "http://x.test/m.js", (4, 10))
            .unwrap_err();
        let ScriptError::Uncaught {
            name,
            message,
            offset,
        } = error
        else {
            panic!("{error:?}");
        };
        assert_eq!(name, "TypeError");
        assert_eq!(message, "Cannot read properties of null (reading 'x')");
        let location = rt.throw_location().unwrap();
        assert_eq!(location.name, "http://x.test/m.js");
        // Line 3 of the module, which starts on line 5 of the page.
        assert_eq!((location.line, location.column), (6, 8));
        assert!(offset.is_some());
        let stack = rt
            .eval_module_at(
                "print(new Error('z').stack)\nfunction g() { return new Error('w').stack }\nprint(g())",
                "m.js",
                (0, 0),
            )
            .map(drop);
        assert_eq!(stack, Ok(()));
        let lines = rt.host_data_mut::<Output>().unwrap().take();
        assert_eq!(
            lines,
            [
                "Error: z\n    at m.js:1:7",
                "Error: w\n    at g (m.js:2:23)\n    at m.js:3:7"
            ]
        );
    });
}

#[test]
fn syntax_that_is_an_error_in_modules() {
    for (source, message) in [
        ("var x = ;", "Unexpected token ';'"),
        (
            "var o = 010;",
            "Octal literals are not allowed in strict mode.",
        ),
        (
            "with ({}) {}",
            "Strict mode code may not include a with statement",
        ),
        (
            "var a = 1; <!-- c\n",
            "HTML comments are not allowed in modules",
        ),
        ("var await = 1;", "Unexpected reserved word"),
        (
            "function f(a, a) {}",
            "Duplicate parameter name not allowed in this context",
        ),
        (
            "var s = '\\08';",
            "Octal escape sequences are not allowed in strict mode.",
        ),
    ] {
        let source = source.to_owned();
        on_big_stack(move || {
            let mut rt = runtime(false);
            let error = rt.eval_module_at(&source, "m.js", (0, 0)).unwrap_err();
            assert!(!error.is_unsupported(), "{source}: {error}");
            assert_eq!(error.to_string(), format!("SyntaxError: {message}"));
        });
    }
}

#[test]
fn the_same_text_is_a_script() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // As a script, `010` and `with` are legal sloppy code.
        assert!(rt.eval("var o = 010; with ({}) {}").is_ok());
        assert!(rt.eval_module_at("var o = 010;", "m.js", (0, 0)).is_err());
    });
}

#[test]
fn modules_that_need_other_modules_do_not_compile() {
    for (source, construct) in [
        ("import './dep.js';", "module syntax"),
        ("import {a} from './dep.js'; print(a)", "module syntax"),
        ("import * as ns from './dep.js';", "module syntax"),
        ("export const a = 1;", "module syntax"),
        ("export default 1;", "module syntax"),
        ("const a = 1; export { a };", "module syntax"),
        ("export * from './dep.js';", "module syntax"),
        ("print(1); import('./dep.js').then(print);", "import()"),
        ("print(import.meta.url);", "import.meta"),
        ("await 0; print('after');", "top-level await"),
        ("for await (const x of []) ;", "top-level await"),
        (
            "print('x'); const y = await Promise.resolve(1);",
            "top-level await",
        ),
    ] {
        let source = source.to_owned();
        on_big_stack(move || {
            for stress in [false, true] {
                let mut rt = runtime(stress);
                let error = rt.eval_module_at(&source, "m.js", (0, 0)).unwrap_err();
                assert!(error.is_unsupported(), "{source}: {error}");
                let ScriptError::Compile { kind, message, .. } = &error else {
                    panic!("{source}: {error:?}");
                };
                assert_eq!(*kind, ThrowKind::SyntaxError);
                assert_eq!(message, &format!("not supported yet ({construct})"));
                // Nothing ran.
                let printed = rt.host_data_mut::<Output>().unwrap().take();
                assert_eq!(printed, Vec::<String>::new());
            }
        });
    }
}

#[test]
fn await_in_functions_is_not_a_top_level_await() {
    assert_eq!(
        run_module(
            "async function f() { await 0; print('in f'); }
             f().then(() => print('then'));
             print('top');",
        ),
        "top\nin f\nthen"
    );
}

#[test]
fn the_budgets_of_scripts_apply() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        // More top-level bindings than registers: the module fails to
        // compile, as a function with so many variables does.
        let mut source = String::new();
        for i in 0..70_000 {
            write!(source, "let v{i};").unwrap();
        }
        let error = rt.eval_module_at(&source, "m.js", (0, 0)).unwrap_err();
        assert!(!error.is_unsupported());
        assert!(matches!(error, ScriptError::Compile { .. }), "{error:?}");
        // A module of 60,000 bindings is under the limit.
        let mut source = String::new();
        for i in 0..60_000 {
            write!(source, "let v{i} = {i};").unwrap();
        }
        source.push_str("print(v59999);");
        assert_eq!(run_module_in(&mut rt, &source), "59999");
        // A deep expression is a range error, not a crash.
        let deep = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
        assert!(rt.eval_module_at(&deep, "m.js", (0, 0)).is_err());
        // An endless loop ends at the deadline.
        rt.set_deadline(Some(
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        ));
        let error = rt
            .eval_module_at("for (;;) {}", "m.js", (0, 0))
            .unwrap_err();
        assert!(matches!(error, ScriptError::Terminated(_)), "{error:?}");
    });
}

#[test]
fn closures_keep_module_bindings_after_the_run() {
    on_big_stack(|| {
        let mut rt = runtime(true);
        assert_eq!(
            run_module_in(
                &mut rt,
                "let n = 0; globalThis.next = () => ++n; globalThis.cls = class { static x = n + 10 };"
            ),
            ""
        );
        rt.collect();
        assert_eq!(rt.eval("next() + next()").unwrap().as_number(), Some(3.0));
        assert_eq!(rt.eval("cls.x").unwrap().as_number(), Some(10.0));
    });
}
