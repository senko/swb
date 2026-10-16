//! Generators complete (M7 feature 7a): `yield*`, `GeneratorFunction`,
//! `%GeneratorFunction.prototype%` and `%GeneratorPrototype%`. The measured
//! Chromium cases (`tools/probes/js-generators.json`), delegates whose
//! methods allocate and collect, and the depth of `yield*` chains. Each
//! case except the deep ones also runs in the GC stress mode.

mod common;

use common::{check_cases, run};

/// The snippets of `tools/probes/js-generators.json` with Chromium 148's
/// results (`data/generators_cases.expected`; the script and the expected
/// output are made by `data/gen-generator-cases.js` from the output of
/// `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/generators_cases.js"),
        include_str!("data/generators_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with chains and recursions of thousands of
/// generators, without the stress mode.
#[test]
fn chromium_cases_heavy() {
    check_chromium(
        include_str!("data/generators_cases_heavy.js"),
        include_str!("data/generators_cases_heavy.expected"),
        &[false],
    );
}

fn check_chromium(script: &str, expected: &str, modes: &[bool]) {
    for &stress in modes {
        let actual = run(script, stress);
        let failures: Vec<String> = actual
            .lines()
            .zip(expected.lines())
            .filter(|(actual, expected)| actual != expected)
            .map(|(actual, expected)| format!("expected: {expected}\nactual:   {actual}"))
            .collect();
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "number of lines (stress: {stress})"
        );
        assert!(
            failures.is_empty(),
            "stress: {stress}\n{}",
            failures.join("\n")
        );
    }
}

/// Delegates whose `next`, `throw` and `return` allocate and collect,
/// result objects with collecting getters, delegates and received values
/// that only a suspended generator holds, chains of generators with
/// collections at every level (the expected output is that of Node.js 22;
/// `just jsdiff` compares it).
#[test]
fn delegates_with_collections() {
    check_cases(&[(
        include_str!("data/generators_gc.js"),
        include_str!("data/generators_gc.expected").trim_end_matches('\n'),
    )]);
}

/// The value of a `yield*` expression in every position, `finally` blocks
/// around it on `return`, and generator methods with `super`.
#[test]
fn yield_star_in_expressions_and_statements() {
    check_cases(&[
        (
            "function* g() { var a = [yield* [1], yield* [2]]; var o = { k: yield* [3] }; \
             return a.concat(o.k); } var it = g(); var out = []; var x; \
             while (!(x = it.next()).done) out.push(x.value); out.join() + ' ' + x.value.join()",
            "=> 1,2,3 ,,",
        ),
        (
            "function* inner() { try { yield 1; } finally { print('inner'); } } \
             function* outer() { try { try { yield* inner(); } finally { print('a'); } } \
             finally { print('b'); } } var it = outer(); it.next(); JSON.stringify(it.return(5))",
            "inner\na\nb\n=> {\"value\":5,\"done\":true}",
        ),
        (
            "function* inner() { try { yield 1; } finally { return 'override'; } } \
             function* outer() { var r = yield* inner(); yield 'after ' + r; } \
             var it = outer(); it.next(); JSON.stringify([it.return(5), it.next()])",
            "=> [{\"value\":\"override\",\"done\":true},{\"done\":true}]",
        ),
        (
            "class A { *g() { yield 'A'; } } class B extends A { *g() { yield* super.g(); \
             yield 'B'; } } [...new B().g()].join()",
            "=> A,B",
        ),
        (
            "var o = { *[Symbol.iterator]() { yield* [1, 2]; } }; [...o, ...o].join()",
            "=> 1,2,1,2",
        ),
        // `for`-`of` inside a delegating generator, closed by `return`.
        (
            "var log = []; var src = { [Symbol.iterator]() { return { next() { return { value: 1, \
             done: false }; }, return() { log.push('closed'); return {}; } }; } }; \
             function* g() { for (var x of src) yield* [x, x]; } var it = g(); it.next(); \
             it.next(); it.return(); log.join()",
            "=> closed",
        ),
        // A `yield*` inside a `catch` and inside a `finally`.
        (
            "function* g() { try { throw 1; } catch (e) { yield* [e, 2]; } finally { yield* [3]; } } \
             [...g()].join()",
            "=> 1,2,3",
        ),
    ]);
}

/// `GeneratorFunction` across `new`, `Reflect.construct` and subclasses.
#[test]
fn generator_function_constructor() {
    check_cases(&[
        (
            "var GF = Object.getPrototypeOf(function* () {}).constructor; \
             var f = GF('a', 'yield a; yield* arguments'); [...f(1, 2, 3)].join()",
            "=> 1,1,2,3",
        ),
        (
            "var GF = Object.getPrototypeOf(function* () {}).constructor; \
             class X extends GF { constructor() { super('yield 7'); this.tag = 'x'; } } \
             var x = new X(); [x.tag, x().next().value, x instanceof X].join()",
            "=> x,7,true",
        ),
        (
            "var GF = Object.getPrototypeOf(function* () {}).constructor; \
             try { GF('yield', ''); } catch (e) { e.name + ': ' + e.message }",
            "=> SyntaxError: Unexpected identifier 'yield'",
        ),
    ]);
}

/// A chain of 100,000 generators that each delegate to the next ends with
/// a catchable `RangeError` (the frame limit), as in Chromium 148, and
/// leaves the runtime usable; `return` and `throw` through a chain of
/// 5,000 run without Rust recursion.
#[test]
fn deep_delegation_chains() {
    let out = run(
        "function* leaf() { yield 1; } var it = leaf(); \
         for (var i = 0; i < 100000; i++) { const inner = it; it = (function* () { yield* inner; })(); } \
         var a; try { it.next(); a = 'no error'; } catch (e) { a = e.name + ': ' + e.message; } \
         var b = JSON.stringify(it.return(5)); \
         it = leaf(); for (var i = 0; i < 5000; i++) { const inner = it; it = (function* () { yield* inner; })(); } \
         var c = JSON.stringify(it.next()); var d = JSON.stringify(it.return(7)); \
         it = leaf(); for (var i = 0; i < 5000; i++) { const inner = it; it = (function* () { yield* inner; })(); } \
         it.next(); var e; try { it.throw('T'); } catch (x) { e = x; } [a, b, c, d, e].join(' ')",
        false,
    );
    assert_eq!(
        out,
        "=> RangeError: Maximum call stack size exceeded {\"value\":5,\"done\":true} \
         {\"value\":1,\"done\":false} {\"value\":7,\"done\":true} T"
    );
}
