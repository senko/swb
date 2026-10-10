//! Tests of eval code (M7 feature 1c, §19.2.1.1 `PerformEval`): the early
//! errors that depend on the call site, with V8's messages and positions
//! (measured with `eval` in Node.js 22: in a sloppy function, a strict
//! function, a method, a class field initializer, a derived constructor
//! and at the top level), the resolution of eval code against its caller,
//! and the storage of its declarations.

use std::fmt::Write;

use swb_js_text::{RecursionBudget, Str16, String16};

use super::tests::{assert_too_deep, on_engine_stack, parse as parse_script_text, split_marker};
use crate::dump::{dump_ast, dump_references, dump_scopes};
use crate::error::ParseError;
use crate::parser::{EvalContext, Script, parse_eval};

/// Parses eval code in both code-unit widths (the narrow one if it fits)
/// and checks that both give the same tree or error.
fn parse(source: &str, context: &EvalContext) -> Result<Script, ParseError> {
    let wide: Vec<u16> = source.encode_utf16().collect();
    let mut budget = RecursionBudget::DEFAULT;
    let result = parse_eval(Str16::Wide(&wide), context, &mut budget);
    assert_eq!(budget, RecursionBudget::DEFAULT, "the budget comes back");
    if wide.iter().all(|&u| u < 256) {
        let narrow: Vec<u8> = wide.iter().map(|&u| u as u8).collect();
        let narrow_result = parse_eval(Str16::Latin1(&narrow), context, &mut budget);
        match (&result, &narrow_result) {
            (Ok(a), Ok(b)) => assert_eq!(dump_ast(a), dump_ast(b), "{source}"),
            (Err(a), Err(b)) => assert_eq!(a, b, "{source}"),
            _ => panic!("the widths disagree on {source}"),
        }
    }
    result
}

/// The call sites of the tables.
#[derive(Clone, Copy)]
enum Site {
    /// A sloppy function (also an arrow function in it).
    Function,
    /// A strict function.
    Strict,
    /// The top level of a script.
    Global,
    /// A method of an object literal.
    Method,
    /// A class field initializer.
    Field,
    /// A derived class constructor.
    Derived,
}

fn context(site: Site) -> EvalContext {
    let mut context = EvalContext {
        direct: true,
        in_function: true,
        ..EvalContext::default()
    };
    match site {
        Site::Function => {}
        Site::Strict => context.strict = true,
        Site::Global => context.in_function = false,
        Site::Method => context.in_method = true,
        Site::Field => {
            context.in_method = true;
            context.in_class_field_initializer = true;
            context.strict = true;
        }
        Site::Derived => {
            context.in_method = true;
            context.in_derived_constructor = true;
            context.strict = true;
        }
    }
    context
}

fn check_eval_errors(site: Site, cases: &[(&str, &str)]) {
    let context = context(site);
    for &(marked, expected) in cases {
        let (source, offset) = split_marker(marked);
        match parse(&source, &context) {
            Ok(script) => panic!("{source}: no error, parsed as {}", dump_ast(&script)),
            Err(error) => {
                assert_eq!(error.message, expected, "message of {source:?}");
                assert_eq!(error.offset, offset, "offset of {source:?}");
            }
        }
    }
}

fn check_eval_valid(site: Site, sources: &[&str]) {
    let context = context(site);
    for source in sources {
        if let Err(error) = parse(source, &context) {
            panic!("{source}: {error}");
        }
    }
}

#[test]
fn eval_in_functions_matches_v8() {
    check_eval_errors(
        Site::Function,
        &[
            ("‸super.x", "'super' keyword unexpected here"),
            ("‸super()", "'super' keyword unexpected here"),
            (
                "this‸.#x",
                "Private field '#x' must be declared in an enclosing class",
            ),
            ("‸return", "Illegal return statement"),
            ("let x; var ‸x;", "Identifier 'x' has already been declared"),
            ("import.‸meta", "Cannot use 'import.meta' outside a module"),
            ("‸export var a;", "Unexpected token 'export'"),
            (
                "‸import a from \"m\";",
                "Cannot use import statement outside a module",
            ),
            (
                "class C { m() { this‸.#y } }",
                "Private field '#y' must be declared in an enclosing class",
            ),
            (
                "‸#x in y",
                "Private field '#x' must be declared in an enclosing class",
            ),
            ("‸super[x]", "'super' keyword unexpected here"),
            ("() => ‸super.x", "'super' keyword unexpected here"),
            ("‸return 1", "Illegal return statement"),
            ("var x; let ‸x;", "Identifier 'x' has already been declared"),
            ("let x; let ‸x;", "Identifier 'x' has already been declared"),
            (
                "function f() {} let ‸f;",
                "Identifier 'f' has already been declared",
            ),
            (
                "\"use strict\"; ‸with (o) x",
                "Strict mode code may not include a with statement",
            ),
            (
                "\"use strict\"; var ‸eval",
                "Unexpected eval or arguments in strict mode",
            ),
            ("‸export var a", "Unexpected token 'export'"),
            (
                "‸import a from \"m\"",
                "Cannot use import statement outside a module",
            ),
            (
                "label: ‸label: x",
                "Label 'label' has already been declared",
            ),
            ("‸break", "Illegal break statement"),
            (
                "‸continue",
                "Illegal continue statement: no surrounding iteration statement",
            ),
            ("‸super.x", "'super' keyword unexpected here"),
        ],
    );
    check_eval_valid(
        Site::Function,
        &[
            "new.target",
            "arguments",
            "() => new.target",
            "var x; with (a) {}",
            "await",
            "yield",
            "import(x)",
            "function g() { new.target }",
            "arguments = 1",
            "var arguments",
            "class C { #x; m() { this.#x } }",
            "var await; await",
            "with (o) x",
            "#!x\n1",
            "<!-- x\n1",
            "x\n--> y",
            "{ function f() {} } f",
            "if (1) function f() {}",
            "l: function f() {}",
            "x: { break x; }",
            "for (;;) { break; }",
            "new.target",
            "arguments",
        ],
    );
}

#[test]
fn eval_in_methods_and_classes_matches_v8() {
    check_eval_errors(
        Site::Method,
        &[
            ("‸super()", "'super' keyword unexpected here"),
            (
                "function g() { ‸super.x }",
                "'super' keyword unexpected here",
            ),
        ],
    );
    check_eval_valid(
        Site::Method,
        &[
            "new.target",
            "super.x",
            "super[x]",
            "() => super.x",
            "arguments",
            "class C { m() { super.x } }",
        ],
    );
    check_eval_errors(
        Site::Field,
        &[
            (
                "‸arguments",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            (
                "() => ‸arguments",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            (
                "typeof ‸arguments",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            ("‸super()", "'super' keyword unexpected here"),
            (
                "(a = ‸arguments) => a",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            (
                "class C { x = ‸arguments }",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            (
                "var ‸arguments",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            (
                "let ‸arguments",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            (
                "‸arguments => 1",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            (
                "({ ‸arguments })",
                "'arguments' is not allowed in class field initializer or static initialization block",
            ),
            (
                "this‸.#x",
                "Private field '#x' must be declared in an enclosing class",
            ),
        ],
    );
    check_eval_valid(
        Site::Field,
        &[
            "new.target",
            "super.x",
            "function f() { arguments }",
            "class C { m() { arguments } }",
            "({ arguments: 1 })",
            "x.arguments",
        ],
    );
    check_eval_errors(
        Site::Derived,
        &[(
            "function f() { ‸super() }",
            "'super' keyword unexpected here",
        )],
    );
    check_eval_valid(
        Site::Derived,
        &[
            "super()",
            "super.x",
            "() => super()",
            "new.target",
            "arguments",
        ],
    );
}

#[test]
fn eval_in_strict_and_global_code_matches_v8() {
    check_eval_errors(
        Site::Strict,
        &[
            (
                "var x; ‸with (a) {}",
                "Strict mode code may not include a with statement",
            ),
            (
                "‸with (o) x",
                "Strict mode code may not include a with statement",
            ),
            ("var ‸eval", "Unexpected eval or arguments in strict mode"),
            (
                "‸arguments = 1",
                "Unexpected eval or arguments in strict mode",
            ),
            ("‸010", "Octal literals are not allowed in strict mode."),
            (
                "delete ‸x",
                "Delete of an unqualified identifier in strict mode.",
            ),
        ],
    );
    check_eval_valid(Site::Strict, &["new.target"]);
    check_eval_errors(
        Site::Global,
        &[
            ("new.‸target", "new.target expression is not allowed here"),
            ("‸super.x", "'super' keyword unexpected here"),
            (
                "() => new.‸target",
                "new.target expression is not allowed here",
            ),
            (
                "this‸.#x",
                "Private field '#x' must be declared in an enclosing class",
            ),
            ("var x; let ‸x", "Identifier 'x' has already been declared"),
        ],
    );
    check_eval_valid(
        Site::Global,
        &["function f() { new.target }", "arguments", "this"],
    );
}

#[test]
fn the_callers_private_names() {
    let mut context = context(Site::Method);
    context.strict = true;
    context.private_names = vec![String16::from("#x")];
    let script = parse(
        "this.#x; #x in this; class C { #y; m() { this.#x + this.#y } }",
        &context,
    )
    .expect("the caller declares #x");
    // `#x` resolves against the caller.
    assert!(dump_references(&script).contains("#x@5:caller"));
    check_eval_errors_with(
        &context,
        &[(
            "this‸.#y",
            "Private field '#y' must be declared in an enclosing class",
        )],
    );
}

fn check_eval_errors_with(context: &EvalContext, cases: &[(&str, &str)]) {
    for &(marked, expected) in cases {
        let (source, offset) = split_marker(marked);
        let error = parse(&source, context).expect_err(&source);
        assert_eq!((error.message.as_ref(), error.offset), (expected, offset));
    }
}

#[test]
fn sloppy_direct_eval_declares_in_the_callers_variable_environment() {
    let script = parse(
        "var a = 1; function f() { return a + b + c; } let b; this; c; arguments; new.target;",
        &context(Site::Function),
    )
    .expect("eval code");
    assert_eq!(
        dump_scopes(&script),
        "function 0 params [] registers 1
  scope 0 Eval: a caller Var, f caller Function, b cell r0 Let
function 1 f parent 0 params [] registers 0 captures [b<-r0]
"
    );
    // The eval code's own vars and the names it does not declare resolve
    // against the caller; its lexical declarations are its own.
    assert_eq!(
        dump_references(&script),
        "a@4:=caller f@20:=caller a@33:caller b@37:cap0! c@41:caller b@50:=cell0 \
         this@53:caller c@59:caller arguments@62:caller new.target@73:caller"
    );
}

#[test]
fn strict_and_indirect_eval_code() {
    // Strict eval code: its vars are its own.
    let script = parse("var a; let b; a + b + c;", &context(Site::Strict)).expect("eval code");
    assert_eq!(
        dump_scopes(&script),
        "function 0 params [] registers 2\n  scope 0 Eval: a r0 Var, b r1 Let\n"
    );
    assert_eq!(
        dump_references(&script),
        "a@4:=r0 b@11:=r1 a@14:r0 b@18:r1 c@22:caller"
    );
    // Indirect eval: global code whose lexical declarations are its own.
    let script =
        parse("var a; let b; a + b + c + this;", &EvalContext::default()).expect("eval code");
    assert_eq!(
        dump_scopes(&script),
        "function 0 params [] registers 2 this\n  scope 0 Eval: a global Var, b r0 Let, this r1 This\n"
    );
    assert_eq!(
        dump_references(&script),
        "a@4:=global b@11:=r0 a@14:global b@18:r0 c@22:global this@26:r1"
    );
}

#[test]
fn hostile_deep_annex_b_and_dynamic_scope() {
    on_engine_stack(|| {
        const N: usize = 100_000;
        let cases = [
            format!("{}x", "with (o) ".repeat(N)),
            format!("{}x{}", "with (o) {".repeat(N), "}".repeat(N)),
            format!("{}{}", "{ function f() {} ".repeat(N), "}".repeat(N)),
            format!("{}function f() {{}}", "if (x) ".repeat(N)),
            format!("{}function f() {{}}", "if (x) {} else ".repeat(N)),
            (0..N).fold(String::new(), |mut s, i| {
                let _ = write!(s, "l{i}: ");
                s
            }) + "function f() {}",
            format!("{}{}", "function f() { eval(s); ".repeat(N), "}".repeat(N)),
            format!("{}{}", "(() => { eval(s); ".repeat(N), "})".repeat(N)),
            format!("{}0{}", "eval(".repeat(N), ")".repeat(N)),
        ];
        for source in &cases {
            assert_too_deep(source);
        }
    });
}

#[test]
fn hostile_wide_block_functions_are_linear() {
    on_engine_stack(|| {
        let repeat = |item: &str, n: usize| {
            (0..n).fold(String::new(), |mut s, i| {
                s.push_str(&item.replace("{i}", &i.to_string()));
                s
            })
        };
        // The var bindings of a function are registers: at most about
        // 64,500 in one function.
        let (n, m) = (100_000, 60_000);
        let sources = [
            repeat("{ function f{i}() {} }\n", n),
            format!(
                "function g() {{ {} }}",
                repeat("if (x) function f{i}() {}\n", m)
            ),
            format!("function g() {{ {} }}", repeat("{ function f() {} }\n", n)),
            format!(
                "function g() {{ switch (x) {{ {} }} }}",
                // A block binding and a var binding per function.
                repeat("case {i}: function f{i}() {}\n", m / 2)
            ),
            format!("{} let x;", repeat("{ l: function x() {} }\n", n)),
        ];
        for source in &sources {
            let start = std::time::Instant::now();
            parse_script_text(source).unwrap_or_else(|error| panic!("{error}"));
            assert!(start.elapsed().as_secs_f64() < 30.0);
        }
    });
}
