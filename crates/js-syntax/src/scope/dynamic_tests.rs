//! Tests of dynamic scope in the analysis (M7 feature 1c): the dynamic
//! environments of `with` bodies and of sloppy direct `eval` (also in the
//! parameters), the dynamic lookups of the references in their reach, the
//! implicit bindings that eval code may use, mapped `arguments`, and
//! hostile inputs.

use std::time::Instant;

use swb_js_text::{RecursionBudget, Str16};

use crate::ast::{FunctionId, RefId};
use crate::parser::{Script, parse_script};
use crate::scope::{BindingKind, DynamicKind};

fn analyze(source: &str) -> Script {
    let wide: Vec<u16> = source.encode_utf16().collect();
    parse_script(Str16::Wide(&wide), &mut RecursionBudget::default())
        .unwrap_or_else(|error| panic!("{source}: {error}"))
}

/// The references of `source` with a dynamic lookup, as `name@offset~n`
/// (without the implicit `%env` and eval references).
fn lookups(source: &str) -> String {
    let script = analyze(source);
    let tree = &script.scopes;
    let mut parts = Vec::new();
    for index in 0..tree.reference_count() {
        let id = RefId::from_index(index);
        let reference = tree.reference(id);
        if let Some(lookup) = tree.dynamic_lookup(id) {
            parts.push(format!(
                "{}@{}~{}",
                script.name_text(reference.name),
                reference.offset,
                lookup.count
            ));
        }
    }
    parts.join(" ")
}

/// The dynamic environments of `source`: kind, scope kind and whether it
/// links to an outer one.
fn envs(source: &str) -> Vec<String> {
    let script = analyze(source);
    let tree = &script.scopes;
    tree.dynamic_envs()
        .iter()
        .map(|env| {
            let kind = match env.kind {
                DynamicKind::With => "with",
                DynamicKind::EvalVars => "eval",
            };
            let outer = if env.outer.is_some() { " outer" } else { "" };
            format!("{kind} {:?}{outer}", tree.scope(env.scope).kind)
        })
        .collect()
}

#[test]
fn with_bodies_check_their_objects_first() {
    // `x` and `y` resolve to the global and to `f`'s var: both check the
    // object first; the block's `let` and `this` do not.
    assert_eq!(
        lookups("function f() { var y; with (o) { let z; x; y; z; this; } }"),
        "x@40~1 y@43~1"
    );
    // Two nested `with` bodies: two objects, innermost first (the inner
    // object expression is in the outer body).
    assert_eq!(lookups("with (a) with (b) x;"), "b@15~1 x@18~2");
    // A binding between them stops the walk.
    assert_eq!(lookups("with (a) { let x; with (b) x; }"), "b@24~1 x@27~1");
    // A closure in the body checks the object too.
    assert_eq!(lookups("with (o) (function () { return x; });"), "x@31~1");
    // `var` declarators with an initializer assign through the object.
    assert_eq!(lookups("with (o) { var v = 1; }"), "v@15~1");
    assert_eq!(
        envs("with (a) { with (b) {} }"),
        ["with With", "with With outer"]
    );
}

#[test]
fn closures_capture_the_with_object() {
    let script = analyze("function f() { with (o) return () => x; }");
    let tree = &script.scopes;
    let arrow = tree.function(FunctionId::from_index(2));
    let captured: Vec<BindingKind> = arrow
        .captures
        .iter()
        .map(|c| tree.binding(c.binding).kind)
        .collect();
    assert_eq!(captured, [BindingKind::DynamicEnv]);
}

#[test]
fn sloppy_direct_eval_has_a_var_object() {
    // References that resolve outside the function's var scope check the
    // vars of the eval code first; the function's own bindings do not.
    assert_eq!(
        lookups("function f(a) { var b; eval(s); return a + b + c; }"),
        "eval@23~1 s@28~1 c@47~1"
    );
    // Also in nested functions and arrow functions.
    assert_eq!(
        lookups("function f() { eval(s); return () => x; }"),
        "eval@15~1 s@20~1 x@37~1"
    );
    // Strict code: eval declares nothing outside itself.
    assert_eq!(lookups("function f() { 'use strict'; eval(s); x; }"), "");
    // Class code is strict, also in a sloppy function.
    assert_eq!(
        lookups("function f() { class C extends (eval(s), B) {} x; }"),
        ""
    );
    // Global code: the eval vars are global object properties.
    assert_eq!(lookups("eval(s); x;"), "");
    assert_eq!(envs("function f() { eval(s); }"), ["eval Function"]);
}

#[test]
fn eval_in_parameters_declares_outside_them() {
    // §10.2.11 step 20: the eval vars of the parameters are outside the
    // parameters (which the walk finds first) and around the body.
    let source = "function f(a = eval(s), b = () => x + a) { var x; return x + c; }";
    assert_eq!(lookups(source), "eval@15~1 s@20~1 x@34~1 c@61~1");
    assert_eq!(envs(source), ["eval Function"]);
    // An eval in the body too: its vars shadow the parameters there.
    let both = "function f(a = eval(s)) { eval(t); return a; }";
    assert_eq!(envs(both), ["eval Function", "eval FunctionBody outer"]);
    assert_eq!(lookups(both), "eval@15~1 s@20~1 eval@26~2 t@31~2 a@42~1");
    // An arrow function's own eval.
    assert_eq!(envs("x => eval(s);"), ["eval Function"]);
}

#[test]
fn functions_with_eval_have_the_implicit_bindings() {
    use BindingKind::{ActiveFunction, Arguments, HomeObject, NewTarget, This};
    let kinds = |source: &str, function: usize| -> Vec<BindingKind> {
        let script = analyze(source);
        let tree = &script.scopes;
        let scope = script.ast.function(FunctionId::from_index(function)).scope;
        tree.bindings_of(scope)
            .iter()
            .map(|&b| tree.binding(b).kind)
            .filter(|kind| {
                matches!(
                    kind,
                    This | NewTarget | Arguments | HomeObject | ActiveFunction
                )
            })
            .collect()
    };
    assert_eq!(kinds("eval(s)", 0), [This]);
    assert_eq!(
        kinds("function f() { eval(s) }", 1),
        [This, NewTarget, Arguments]
    );
    assert_eq!(
        kinds("x = { m() { eval(s) } }", 1),
        [This, NewTarget, Arguments, HomeObject]
    );
    assert_eq!(
        kinds("class A extends B { constructor() { eval(s) } }", 1),
        [This, NewTarget, Arguments, HomeObject, ActiveFunction]
    );
    // Field initializers: no `arguments` (an early error in eval code).
    assert_eq!(
        kinds("class A { x = eval(s) }", 1),
        [This, NewTarget, HomeObject]
    );
    // An arrow function takes them from the enclosing function.
    assert_eq!(
        kinds("function f() { () => eval(s) }", 1),
        [This, NewTarget, Arguments]
    );
    // A parameter named `arguments` is what eval code sees.
    assert_eq!(
        kinds("function f(arguments) { eval(s) }", 1),
        [This, NewTarget]
    );
}

#[test]
fn mapped_arguments_objects() {
    let mapped = |source: &str| -> (bool, bool) {
        let script = analyze(source);
        let f = script.scopes.function(FunctionId::from_index(1));
        (f.arguments_binding.is_some(), f.mapped_arguments)
    };
    // Sloppy, simple parameters, an arguments object: mapped.
    assert_eq!(mapped("function f(a) { arguments; }"), (true, true));
    assert_eq!(mapped("function f(a) { () => arguments; }"), (true, true));
    assert_eq!(mapped("function f(a) { eval(s); }"), (true, true));
    assert_eq!(mapped("function f(a) { var arguments; }"), (true, true));
    assert_eq!(mapped("function* f(a) { arguments; }"), (true, true));
    assert_eq!(mapped("async function f(a) { arguments; }"), (true, true));
    assert_eq!(mapped("x = { m(a) { arguments; } }"), (true, true));
    // Unmapped: strict code, a non-simple parameter list.
    assert_eq!(
        mapped("function f(a) { 'use strict'; arguments; }"),
        (true, false)
    );
    assert_eq!(mapped("function f(a = 1) { arguments; }"), (true, false));
    assert_eq!(mapped("function f(...a) { arguments; }"), (true, false));
    assert_eq!(mapped("function f({a}) { arguments; }"), (true, false));
    // No arguments object.
    assert_eq!(
        mapped("function f(a) { function arguments() {} arguments; }"),
        (false, false)
    );
    assert_eq!(
        mapped("function f(a) { let arguments; arguments; }"),
        (false, false)
    );
    assert_eq!(
        mapped("function f(arguments) { arguments; }"),
        (false, false)
    );
    assert_eq!(mapped("function f(a) { a; }"), (false, false));
    // The parameters of a mapped object are cells.
    let script = analyze("function f(a, b) { arguments; }");
    let f = script.scopes.function(FunctionId::from_index(1));
    for &param in &f.params {
        let storage = script.scopes.binding(param).storage;
        assert!(matches!(storage, crate::Storage::Cell(_)), "{storage:?}");
    }
}

/// Runs `test` on a thread with the 8 MiB stack of the engine.
fn on_engine_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(test)
        .expect("a test thread")
        .join()
        .expect("the test passes");
}

/// Parses `source` and returns the seconds it took.
fn timed(source: &str) -> f64 {
    let start = Instant::now();
    analyze(source);
    start.elapsed().as_secs_f64()
}

#[test]
fn hostile_dynamic_scopes_are_linear() {
    on_engine_stack(|| {
        let n = 100_000;
        let sources = [
            // Many `with` bodies, each around references.
            "with (o) { a; b; c; }\n".repeat(n),
            // Many references in one body, in one function with eval.
            format!("with (o) {{ {} }}", "a; b; c;".repeat(n)),
            format!("function f() {{ eval(s); {} }}", "a; b; c;".repeat(n)),
            // Many functions with eval, each with references.
            "function f() { eval(s); return () => x; }\n".repeat(n),
            // Deeply nested `with` bodies (to the recursion budget's
            // depth) around many references.
            format!(
                "{}{}{}",
                "with (o) {".repeat(600),
                "a; b;".repeat(n),
                "}".repeat(600)
            ),
        ];
        for source in &sources {
            assert!(timed(source) < 30.0, "{}", &source[..30]);
        }
    });
}
