//! Tests of the review fixes of M7 feature 1b: the class name `await` in
//! async arrow heads, private name resolution, the dead `async` callee,
//! and the messages of `arguments`, bare private names and `await` in
//! class code. The tables come from Node.js 22 (`new vm.Script(source)`;
//! the position is the caret of the error).

use std::fmt::Write;
use std::time::Instant;

use super::tests::{check_errors, on_engine_stack, parse};
use crate::dump::{dump_references, dump_scopes};

const ASYNC_ARROW_CLASS_NAME: &[(&str, &str)] = &[
    (
        "async (x = class \u{2038}await {}) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async (x = (class \u{2038}await {})) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async (x = [class \u{2038}await {}]) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async (x = class { [class \u{2038}await {}]() {} }) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async (x = class extends (class \u{2038}await {}) {}) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async function f(){ class \u{2038}await {} }",
        "Unexpected reserved word",
    ),
    (
        "async function f(){ (class \u{2038}await {}) }",
        "Unexpected reserved word",
    ),
    (
        "async x => class \u{2038}await {}",
        "Unexpected reserved word",
    ),
    (
        "async (x = class \u{2038}await {}, y) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async (a, b = 1, x = class \u{2038}await {}) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async ({a = class \u{2038}await {}}) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async (x = async function \u{2038}await(){}) => 1",
        "Unexpected reserved word",
    ),
];

const CLASS_CODE_MESSAGES: &[(&str, &str)] = &[
    (
        "class A { x = function \u{2038}arguments() {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { function \u{2038}arguments() {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { try {} catch (\u{2038}arguments) {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { let \u{2038}arguments; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { var \u{2038}arguments; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = (\u{2038}arguments) => 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = \u{2038}arguments }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = () => \u{2038}arguments }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = function (\u{2038}arguments) {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { \u{2038}arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { (\u{2038}arguments) => 1 } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { ({\u{2038}arguments}) => 1 } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { ({\u{2038}arguments} = 1) } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { \u{2038}arguments = 1 } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = \u{2038}arguments = 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = [\u{2038}arguments] = 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = class \u{2038}arguments {} }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { class \u{2038}arguments {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function* \u{2038}arguments() {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { for (let \u{2038}arguments of []) ; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { for (var \u{2038}arguments in {}) ; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = ({ \u{2038}arguments }) }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = ({ \u{2038}arguments }) => 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "\u{2038}#x",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "x \u{2038}= #x",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "\u{2038}(#x)",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "\u{2038}#x in o",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "x \u{2038}= #x + 1",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "class A {\u{2038}} #x",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "class A {} x \u{2038}= #x in y",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "function f(){ \u{2038}await 1 }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "class A { x = \u{2038}await 1; }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "class A { static x = \u{2038}await 1; }",
        "Unexpected reserved word",
    ),
    (
        "function f(){ \u{2038}await 1; }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "function f(){ x = \u{2038}await 1 }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "function f(){ \u{2038}await x }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "async function f(){ -\u{2038}await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ typeof \u{2038}await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ delete \u{2038}await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ !\u{2038}await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ ~\u{2038}await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ +\u{2038}await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ void \u{2038}await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ \u{2038}-x ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ \u{2038}await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ \u{2038}-(await 1) ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ -\u{2038}await (1) ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ -\u{2038}await x.y ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f(){ -await \u{2038}-x ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "class A { static { (class \u{2038}arguments {}) } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { class \u{2038}arguments {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = class \u{2038}arguments {} }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = function \u{2038}arguments() {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function* \u{2038}arguments() {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = async function \u{2038}arguments() {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function f(\u{2038}arguments) {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function f(...\u{2038}arguments) {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function f([\u{2038}arguments]) {} }",
        "Invalid destructuring assignment target",
    ),
    (
        "class A { x = function f({\u{2038}arguments}) {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function f(a = 1, \u{2038}arguments) {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = { m(\u{2038}arguments) {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = { set s(\u{2038}arguments) {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = (\u{2038}arguments) => 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = (...\u{2038}arguments) => 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = ([\u{2038}arguments]) => 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = \u{2038}arguments => 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = async \u{2038}arguments => 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = async (\u{2038}arguments) => 1 }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { let \u{2038}arguments; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { const \u{2038}arguments = 1; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { let [\u{2038}arguments] = []; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { let {\u{2038}arguments} = {}; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { let {a: \u{2038}arguments} = {}; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { var [\u{2038}arguments] = []; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { try {} catch ({\u{2038}arguments}) {} } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { try {} catch (\u{2038}arguments) {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { function f() { var \u{2038}arguments; } } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { function f(\u{2038}arguments) {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { function \u{2038}arguments() {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { async function \u{2038}arguments() {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { function* \u{2038}arguments() {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { for (const \u{2038}arguments of []) ; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { for (\u{2038}arguments of []) ; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { for (\u{2038}arguments in {}) ; } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { \u{2038}arguments++ } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { static { [\u{2038}arguments] = [] } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = function() { var \u{2038}arguments } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function() { let \u{2038}arguments } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function() { class \u{2038}arguments {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = () => { var \u{2038}arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = () => { let \u{2038}arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class A { x = () => { function \u{2038}arguments() {} } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = () => function \u{2038}arguments() {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = function \u{2038}eval() {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { static { let \u{2038}eval; } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "class A { x = (\u{2038}eval) => 1 }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "async function f() { x = class { x = await \u{2038}1 } }",
        "Unexpected number",
    ),
    (
        "async function f(){ class C { x = await \u{2038}1 } }",
        "Unexpected number",
    ),
    (
        "async function f(){ class C { x = await \u{2038}1; } }",
        "Unexpected number",
    ),
    (
        "async function f(){ class C { static x = \u{2038}await 1 } }",
        "Unexpected reserved word",
    ),
    (
        "class A { x = \u{2038}await 1 }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "class A { x = \u{2038}await 1; }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "class A { x = 1 + \u{2038}await 1 }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    ("class A { x = (await \u{2038}1) }", "Unexpected number"),
    (
        "class A { static { \u{2038}await 1 } }",
        "Unexpected reserved word",
    ),
];

const ASYNC_REST_COMMA: &[(&str, &str)] = &[
    (
        "async (...a\u{2038}, /*,*/ b) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "async (...a /*,*/\u{2038}, b) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "async (...a\u{2038}, b) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "async (...a\u{2038},) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "async (...a /*,*/ \u{2038},) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "async (a, ...b\u{2038}, ...c) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "async (a, ...b /* , */ \u{2038}, ...c) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "async (...a // ,\n\u{2038}, b) => 1",
        "Rest parameter must be last formal parameter",
    ),
];

/// V8 accepts these; swb rejects them as strict mode code (§15.7, note).
const OCTAL_IN_CLASS_CODE: &[(&str, &str)] = &[
    (
        "x = class { \u{2038}010 = 1 }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { \u{2038}010() {} }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { static \u{2038}010 = 1 }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { get \u{2038}010() {} }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { '\\\u{2038}07' = 1 }",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "x = class { '\\\u{2038}07'() {} }",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "x = class { x = \u{2038}010 }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { x = '\\\u{2038}07' }",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "x = class { static x = \u{2038}010 }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { static x = '\\\u{2038}07' }",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "x = class { x = \u{2038}09 }",
        "Decimals with leading zeros are not allowed in strict mode.",
    ),
    (
        "x = class { x = '\u{2038}\\8' }",
        "\\8 and \\9 are not allowed in strict mode.",
    ),
    (
        "x = class extends (\u{2038}010) {}",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class extends f('\\\u{2038}07') {}",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "class A { \u{2038}010 = 1 }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "class A extends (\u{2038}010) {}",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { static { '\\\u{2038}07' } }",
        "Octal escape sequences are not allowed in strict mode.",
    ),
];

const PRIVATE_FIRST_USE: &[(&str, &str)] = &[
    (
        "class A { m() { class B { n() { this\u{2038}.#z } } this.#y } }",
        "Private field '#z' must be declared in an enclosing class",
    ),
    (
        "class A { m() { this\u{2038}.#y; class B { n() { this.#z } } } }",
        "Private field '#y' must be declared in an enclosing class",
    ),
    (
        "class A { #a; m() { class B { n() { this\u{2038}.#b; this.#a } } this.#c } }",
        "Private field '#b' must be declared in an enclosing class",
    ),
    (
        "class A { m() { class B { n() { this\u{2038}.#a; this.#b } } } #b }",
        "Private field '#a' must be declared in an enclosing class",
    ),
    (
        "class A { m() { class B { n() { this\u{2038}.#a } } class C { n() { this.#b } } } }",
        "Private field '#a' must be declared in an enclosing class",
    ),
    (
        "class A { m() { class B { n() { this\u{2038}.#b } } class C { n() { this.#a } } } }",
        "Private field '#b' must be declared in an enclosing class",
    ),
    (
        "class A { #q; m() { class B { #p; n() { class C { k() { this.#p; this.#q; this\u{2038}.#r; this.#s } } } } } #s }",
        "Private field '#r' must be declared in an enclosing class",
    ),
    (
        "class A { #q; m() { class B { #p; n() { class C { k() { this.#p; this.#q; this.#s; this\u{2038}.#r } } } } } #s }",
        "Private field '#r' must be declared in an enclosing class",
    ),
];

/// A class expression named `await` in the head of an async arrow
/// function (§15.9: the head is reparsed with [+Await]).
#[test]
fn class_name_await_in_async_arrow_heads() {
    check_errors(ASYNC_ARROW_CLASS_NAME);
    for source in [
        "async (x = function await(){}) => 1",
        "async (x = class { m() { class await {} } }) => 1",
        "async (x = class { y = class await {} }) => 1",
        "async (x = class await {})",
        "async (class await {})",
        "(x = class await {}) => 1",
        "function f(){ class await {} }",
        "async (x = class {}) => 1",
    ] {
        parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}

/// `arguments` in class initializers and static blocks, bare private
/// names, `await` as an identifier in field initializers and `**` after
/// an `await` operand.
#[test]
fn class_code_messages_match_v8() {
    check_errors(CLASS_CODE_MESSAGES);
    for source in [
        "class A { x = function () { arguments } }",
        "function f(){ await (1) }",
        "function f(){ await }",
        "function f(){ await; }",
        "async function f(){ class C { x = await } }",
        "class A { x = await\n1 }",
    ] {
        parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}

/// The comma after a spread argument of `async(...)` is found in the
/// token stream, not in the text (comments).
#[test]
fn async_rest_comma_is_not_found_in_comments() {
    check_errors(ASYNC_REST_COMMA);
}

/// Unresolved private names: the first use in source order is the error,
/// also through nested classes.
#[test]
fn undeclared_private_names_report_the_first_use() {
    check_errors(PRIVATE_FIRST_USE);
}

/// 300 nested classes around 20,000 distinct undeclared names: the
/// uses move up through the classes without being visited again.
#[test]
fn many_undeclared_private_names_in_deep_classes_are_fast() {
    on_engine_stack(|| {
        const DEPTH: usize = 300;
        const NAMES: usize = 20_000;
        let mut source = "class C { m() { ".repeat(DEPTH);
        for i in 0..NAMES {
            write!(source, "this.#q{i}; ").expect("writing to a string");
        }
        source.push_str(&"} }".repeat(DEPTH));
        let start = Instant::now();
        let error = parse(&source).expect_err("undeclared names");
        assert!(start.elapsed().as_secs() < 5, "slow: {:?}", start.elapsed());
        assert_eq!(
            error.message,
            "Private field '#q0' must be declared in an enclosing class"
        );
        // A declaration in the outermost class resolves the names.
        let mut source = "class C { #q0; m() { ".to_owned();
        source.push_str(&"(class D { n() { this.#q0; } }); ".repeat(DEPTH));
        source.push_str("} }");
        parse(&source).expect("resolved");
    });
}

/// The `async` of an `async(...) =>` head is no use of a binding: it
/// forces no capture.
#[test]
fn async_arrow_callee_is_not_a_reference() {
    let script = parse("function f() { let async = 1; function g() { async () => 1 } }")
        .expect("async arrow function");
    let scopes = dump_scopes(&script);
    assert!(!scopes.contains("captures"), "{scopes}");
    let references = dump_references(&script);
    assert!(references.contains("async@45:dead"), "{references}");
    assert!(references.contains("async@19:=r0"), "{references}");
    let script = parse("function f() { let async = 1; function g() { return async } }")
        .expect("a use of async");
    assert!(dump_scopes(&script).contains("captures [async<-"));
}

/// Legacy octal literals and escapes in the names, the field initializers
/// and the heritage of a class: V8 accepts them (see `deviations_from_v8`).
#[test]
fn octal_in_class_code_is_an_error() {
    check_errors(OCTAL_IN_CLASS_CODE);
}
