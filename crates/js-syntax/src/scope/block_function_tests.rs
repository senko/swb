//! Tests of Annex B.3.2 and B.3.3 (M7 feature 1c): which block-level
//! function declarations of sloppy mode code get a var binding, and which
//! binding receives the function. The cases follow test262's
//! `annexB/language/function-code` and V8 (Node.js 22), measured with
//! `typeof` before and after the block.

use swb_js_text::{RecursionBudget, Str16};

use crate::ast::FunctionId;
use crate::parser::{EvalContext, Script, parse_eval, parse_script};
use crate::scope::Storage;

fn analyze(source: &str) -> Script {
    let wide: Vec<u16> = source.encode_utf16().collect();
    parse_script(Str16::Wide(&wide), &mut RecursionBudget::default())
        .unwrap_or_else(|error| panic!("{source}: {error}"))
}

/// For each function declaration in a block, in source order: its name
/// and the kind and scope of the var binding it assigns, or `-`.
fn hoisted(script: &Script) -> Vec<String> {
    let tree = &script.scopes;
    script
        .ast
        .function_ids()
        .filter_map(|id: FunctionId| {
            let function = script.ast.function(id);
            let name = function.name.filter(|_| function.is_declaration)?;
            let declared_in = tree.reference(name.reference).scope;
            if tree.scope(declared_in).hoist_to == declared_in {
                return None;
            }
            let text = script.name_text(name.name);
            Some(match tree.function(id).block_function_var {
                None => format!("{text} -"),
                Some(reference) => {
                    let binding = tree.reference(reference).binding.map(|b| tree.binding(b));
                    match binding {
                        Some(b) => format!("{text} {:?} {:?}", b.kind, tree.scope(b.scope).kind),
                        None => format!("{text} ?"),
                    }
                }
            })
        })
        .collect()
}

fn check(source: &str, expected: &[&str]) {
    assert_eq!(hoisted(&analyze(source)), expected, "{source}");
}

#[test]
fn plain_declarations_in_blocks_get_a_var() {
    check(
        "function g() { { function f() {} } }",
        &["f BlockFunctionVar Function"],
    );
    check("{ function f() {} }", &["f BlockFunctionVar Script"]);
    // An existing var or function of the name receives the value.
    check(
        "function g() { var f; { function f() {} } }",
        &["f Var Function"],
    );
    check(
        "function g() { function f() {} { function f() {} } }",
        &["f Function Function"],
    );
    // `if` clauses (B.3.3), `switch` clauses, labelled declarations and
    // duplicates in a block, as in V8.
    check(
        "function g() { if (x) function f() {} else function h() {} }",
        &["f BlockFunctionVar Function", "h BlockFunctionVar Function"],
    );
    check(
        "function g() { switch (x) { case 1: function f() {} } }",
        &["f BlockFunctionVar Function"],
    );
    check(
        "function g() { { l: function f() {} } }",
        &["f BlockFunctionVar Function"],
    );
    check(
        "function g() { { function f() {} function f() {} } }",
        &["f BlockFunctionVar Function", "f BlockFunctionVar Function"],
    );
    // A var through a simple catch parameter (B.3.4).
    check(
        "function g() { try {} catch (f) { { function f() {} } } }",
        &["f BlockFunctionVar Function"],
    );
    // A function with parameter expressions: the body's var scope.
    check(
        "function g(a = 1) { { function f() {} } }",
        &["f BlockFunctionVar FunctionBody"],
    );
}

#[test]
fn declarations_whose_var_would_be_an_error_are_not_hoisted() {
    check("function g() { let f; { function f() {} } }", &["f -"]);
    check("function g() { { function f() {} } let f; }", &["f -"]);
    check("{ function f() {} } let f;", &["f -"]);
    check("function g() { { let f; { function f() {} } } }", &["f -"]);
    // The inner one of two nested declarations (test262
    // block-decl-nested-blocks-with-fun-decl): its var would conflict
    // with the outer block's lexical `f`. V8 assigns it too (`{ function
    // f(){return 1} { function f(){return 2} } } f()` gives 2 in Node 22,
    // 1 by the spec).
    check(
        "function g() { { function f() {} { function f() {} } } }",
        &["f BlockFunctionVar Function", "f -"],
    );
    check(
        "function g() { try {} catch ([f]) { { function f() {} } } }",
        &["f -"],
    );
    check(
        "function g() { for (let f;;) { function f() {} } }",
        &["f -"],
    );
    check(
        "function g() { for (const f of x) { function f() {} } }",
        &["f -"],
    );
    check(
        "function g() { switch (x) { case 1: let f; case 2: { function f() {} } } }",
        &["f -"],
    );
    // Parameter names (B.3.2.1), also with parameter expressions.
    check("function g(f) { { function f() {} } }", &["f -"]);
    check("function g(f = 1) { { function f() {} } }", &["f -"]);
}

#[test]
fn only_plain_sloppy_declarations_qualify() {
    check("function g() { { function* f() {} } }", &["f -"]);
    check("function g() { { async function f() {} } }", &["f -"]);
    check(
        "function g() { 'use strict'; { function f() {} } }",
        &["f -"],
    );
    check("class C { m() { { function f() {} } } }", &["f -"]);
}

#[test]
fn block_functions_named_arguments() {
    // test262 block-decl-func-skip-arguments: not in a function with an
    // arguments object (V8 assigns it).
    check(
        "function g() { { function arguments() {} } }",
        &["arguments -"],
    );
    check(
        "function g(a = 1) { { function arguments() {} } }",
        &["arguments -"],
    );
    // An arrow function has no arguments object: a var of its own, as in V8.
    check(
        "() => { { function arguments() {} } }",
        &["arguments BlockFunctionVar Function"],
    );
    check(
        "{ function arguments() {} }",
        &["arguments BlockFunctionVar Script"],
    );
}

#[test]
fn eval_code_block_functions_belong_to_the_caller() {
    let eval = |source: &str, context: &EvalContext| {
        let wide: Vec<u16> = source.encode_utf16().collect();
        parse_eval(Str16::Wide(&wide), context, &mut RecursionBudget::default())
            .unwrap_or_else(|error| panic!("{source}: {error}"))
    };
    let direct = EvalContext {
        direct: true,
        in_function: true,
        ..EvalContext::default()
    };
    let script = eval("{ function f() {} } f;", &direct);
    assert_eq!(hoisted(&script), ["f BlockFunctionVar Eval"]);
    let binding = script.scopes.binding_count() - 1;
    let storage = (0..=binding)
        .map(|i| script.scopes.binding(crate::BindingId::from_index(i)))
        .find(|b| b.kind == crate::BindingKind::BlockFunctionVar)
        .map(|b| b.storage);
    assert_eq!(storage, Some(Storage::Caller));
    // Indirect eval: a global; strict eval code: not hoisted.
    let script = eval("{ function f() {} }", &EvalContext::default());
    assert_eq!(hoisted(&script), ["f BlockFunctionVar Eval"]);
    let strict = EvalContext {
        strict: true,
        ..direct.clone()
    };
    assert_eq!(hoisted(&eval("{ function f() {} }", &strict)), ["f -"]);
}
