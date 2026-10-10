//! Tests of the scope analysis: storage kinds, captures across function
//! levels, per-iteration bindings, TDZ checks, `arguments`, `this` in
//! arrow functions, hoisting and shadowing.
//!
//! The dumps ([`dump_scopes`], [`dump_references`]) list per function its
//! parameters, register count and captures, per scope its bindings with
//! their storage, and per identifier occurrence `name@offset:how` (`=`
//! marks a declaration, `!` a TDZ check).

use std::fmt::Write;

use swb_js_text::{RecursionBudget, Str16};

use crate::dump::{dump_references, dump_scopes};
use crate::parser::{Script, parse_script};

fn analyze(source: &str) -> Script {
    let wide: Vec<u16> = source.encode_utf16().collect();
    parse_script(Str16::Wide(&wide), &mut RecursionBudget::default())
        .unwrap_or_else(|error| panic!("{source}: {error}"))
}

/// Checks the scope dump (without the function lines' leading spaces of
/// `expected`) and the reference dump of `source`.
fn check(source: &str, expected_scopes: &str, expected_references: &str) {
    let script = analyze(source);
    let expected: String = expected_scopes
        .lines()
        .map(|line| line.trim_start_matches(' ').replace('|', " "))
        .map(|line| line + "\n")
        .collect();
    assert_eq!(dump_scopes(&script), expected, "source: {source}");
    assert_eq!(
        dump_references(&script),
        expected_references,
        "source: {source}"
    );
}

// In the expected scope dumps, each `|` stands for one leading space of a scope
// line (the dump indents scope lines by two spaces).

#[test]
fn registers_globals_and_sibling_blocks() {
    check(
        "var a; let b; const c = 1; function f(p) { var q; let r; { let s; } { let t; } return p + q + r; }",
        "function 0 params [] registers 0
         ||scope 0 Script: a global Var, b global Let, c global Const, f global Function
         function 1 f parent 0 params [p] registers 4
         ||scope 1 Function: p r0 Parameter, q r1 Var, r r2 Let
         ||scope 2 Block: s r3 Let
         ||scope 3 Block: t r3 Let",
        "a@4:=global b@11:=global c@20:=global f@36:=global p@38:=r0 q@47:=r1 r@54:=r2 \
         s@63:=r3 t@74:=r3 p@86:r0 q@90:r1 r@94:r2",
    );
}

#[test]
fn captures_through_several_levels() {
    check(
        "function f() { var x; return function g() { return function h() { return x; }; }; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 1
         ||scope 1 Function: x cell r0 Var
         function 2 g parent 1 params [] registers 1 captures [x<-r0]
         ||scope 2 FunctionName: g r0 FunctionName
         function 3 h parent 2 params [] registers 1 captures [x<-c0]
         ||scope 4 FunctionName: h r0 FunctionName",
        "f@9:=global x@19:=cell0 g@38:=r0 h@60:=r0 x@73:cap0",
    );
    check(
        "x => { var y; return z => x + y + z; }",
        "function 0 params [] registers 0
         function 1 parent 0 params [x] registers 2
         ||scope 1 Function: x cell r0 Parameter, y cell r1 Var
         function 2 parent 1 params [z] registers 1 captures [x<-r0 y<-r1]
         ||scope 2 Function: z r0 Parameter",
        "x@0:=cell0 y@11:=cell1 z@21:=r0 x@26:cap0 y@30:cap1 z@34:r0",
    );
    // A captured parameter keeps its register, which holds the cell.
    check(
        "function f(a, b) { return () => a; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [a b] registers 2
         ||scope 1 Function: a cell r0 Parameter, b r1 Parameter
         function 2 parent 1 params [] registers 0 captures [a<-r0]",
        "f@9:=global a@11:=cell0 b@14:=r1 a@32:cap0",
    );
}

#[test]
fn per_iteration_bindings() {
    check(
        "for (let i = 0; i < 3; i++) { f(() => i); }",
        "function 0 params [] registers 1
         ||scope 1 For: per-iteration i cell r0 Let
         function 1 parent 0 params [] registers 0 captures [i<-r0]",
        "i@9:=cell0 i@16:cell0 i@23:cell0 f@30:global i@38:cap0!",
    );
    check(
        "for (let i = 0; i < 3; i++) { f(i); }",
        "function 0 params [] registers 1
         ||scope 1 For: i r0 Let",
        "i@9:=r0 i@16:r0 i@23:r0 f@30:global i@32:r0",
    );
    // `const` needs no copies; `var` is global here.
    check(
        "for (const i = 0;;) { g(() => i); } for (var j = 0;;) { g(() => j); }",
        "function 0 params [] registers 1
         ||scope 0 Script: j global Var
         ||scope 1 For: i cell r0 Const
         function 1 parent 0 params [] registers 0 captures [i<-r0]
         function 2 parent 0 params [] registers 0",
        "i@11:=cell0 g@22:global i@30:cap0! j@45:=global g@56:global j@64:global",
    );
}

#[test]
fn temporal_dead_zone() {
    check(
        "{ let x = 1; x; } x; let y = y;",
        "function 0 params [] registers 1
         ||scope 0 Script: y global Let
         ||scope 1 Block: x r0 Let",
        "x@6:=r0 x@13:r0 x@18:global y@25:=global y@29:global!",
    );
    // A hoisted function can run before the declaration: its loads check.
    check(
        "function f() { g(); let x = 1; x; function g() { x; } }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 2
         ||scope 1 Function: x cell r0 Let, g r1 Function
         function 2 g parent 1 params [] registers 0 captures [x<-r0]",
        "f@9:=global g@15:r1 x@24:=cell0 x@31:cell0 g@43:=r1 x@49:cap0!",
    );
    check(
        "let a = 1; function f() { return a; }",
        "function 0 params [] registers 0
         ||scope 0 Script: a global Let, f global Function
         function 1 f parent 0 params [] registers 0",
        "a@4:=global f@20:=global a@33:global!",
    );
}

#[test]
fn this_and_arguments() {
    check(
        "function f() { return () => () => this; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 1 this
         ||scope 1 Function: this cell r0 This
         function 2 parent 1 params [] registers 0 captures [this<-r0]
         function 3 parent 2 params [] registers 0 captures [this<-c0]",
        "f@9:=global this@34:cap0",
    );
    check(
        "this; () => this",
        "function 0 params [] registers 1 this
         ||scope 0 Script: this cell r0 This
         function 1 parent 0 params [] registers 0 captures [this<-r0]",
        "this@0:cell0 this@12:cap0",
    );
    check(
        "function f() { return () => arguments; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 1 arguments mapped
         ||scope 1 Function: arguments cell r0 Arguments
         function 2 parent 1 params [] registers 0 captures [arguments<-r0]",
        "f@9:=global arguments@28:cap0",
    );
    // At the top level `arguments` is a global name; a lexical declaration
    // shadows the arguments object.
    check(
        "arguments; () => arguments; function f() { let arguments; arguments; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 parent 0 params [] registers 0
         function 2 f parent 0 params [] registers 1
         ||scope 2 Function: arguments r0 Let",
        "arguments@0:global arguments@17:global f@37:=global arguments@47:=r0 arguments@58:r0",
    );
    check(
        "function f() { var arguments; return arguments; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 1 arguments mapped
         ||scope 1 Function: arguments r0 Arguments",
        "f@9:=global arguments@19:=r0 arguments@37:r0",
    );
}

#[test]
fn hoisting_and_shadowing() {
    check(
        "f(); var v = 1; function f() { return v; }",
        "function 0 params [] registers 0
         ||scope 0 Script: v global Var, f global Function
         function 1 f parent 0 params [] registers 0",
        "f@0:global v@9:=global f@25:=global v@38:global",
    );
    check(
        "function f() { var x; function g() { let x; return () => x; } return x; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 2
         ||scope 1 Function: x r0 Var, g r1 Function
         function 2 g parent 1 params [] registers 1
         ||scope 2 Function: x cell r0 Let
         function 3 parent 2 params [] registers 0 captures [x<-r0]",
        "f@9:=global x@19:=r0 g@31:=r1 x@41:=cell0 x@57:cap0! x@69:r0",
    );
    // Duplicate parameters share a binding: the last position.
    check(
        "function f(a, a) { return a; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [a a] registers 2
         ||scope 1 Function: a r1 Parameter",
        "f@9:=global a@11:=r1 a@14:=r1 a@26:r1",
    );
    // A function declaration with a parameter's name shares its binding.
    check(
        "function f(a) { function a() {} return a; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [a] registers 1
         ||scope 1 Function: a r0 Parameter
         function 2 a parent 1 params [] registers 0",
        "f@9:=global a@11:=r0 a@25:=r0 a@39:r0",
    );
    // Block-level functions are block bindings; in sloppy mode code also
    // var bindings (Annex B.3.2: the last reference, `h@55:r1`).
    check(
        "function f() { var x = 1; if (x) { let y = x; function h() { return y; } } }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 4
         ||scope 1 Function: x r0 Var, h r1 BlockFunctionVar
         ||scope 2 Block: y cell r2 Let, h r3 Function
         function 2 h parent 1 params [] registers 0 captures [y<-r2]",
        "f@9:=global x@19:=r0 x@30:r0 y@39:=cell2 x@43:r0 h@55:=r3 y@68:cap0! h@55:r1",
    );
    // A `var` in a catch block with the parameter's name assigns the
    // parameter (Annex B.3.4) and also declares a variable.
    check(
        "try {} catch (e) { var e = 1; e; }",
        "function 0 params [] registers 1
         ||scope 0 Script: e global Var
         ||scope 2 Catch: e r0 CatchParameter",
        "e@14:=r0 e@23:=r0 e@30:r0",
    );
    // A named function expression sees its own name.
    check(
        "x = function f() { return f; }",
        "function 0 params [] registers 0
         function 1 f parent 0 params [] registers 1
         ||scope 1 FunctionName: f r0 FunctionName",
        "x@0:global f@13:=r0 f@26:r0",
    );
}

#[test]
fn direct_eval_makes_cells() {
    check(
        "function f() { eval('x'); var y; function g() { var z; } }",
        "function 0 params [] registers 0 eval
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 6 this arguments mapped eval
         ||scope 1 Function: y cell r0 Var, g cell r1 Function, %env cell r2 DynamicEnv, \
         this cell r3 This, new.target cell r4 NewTarget, arguments cell r5 Arguments
         function 2 g parent 1 params [] registers 1
         ||scope 2 Function: z r0 Var",
        "f@9:=global eval@15:global~1 y@30:=cell0 g@42:=cell1 z@52:=r0 this@10:cell3 \
         new.target@10:cell4 arguments@10:cell5 %env@10:global %env@15:cell2",
    );
}

#[test]
fn function_declarations_per_scope() {
    let script =
        analyze("function a() {} { function b() {} function b() {} } if (x) function c() {}");
    let tree = &script.scopes;
    let counts: Vec<usize> = (0..tree.scope_count())
        .map(|i| {
            tree.function_declarations(crate::ast::ScopeId::from_index(i))
                .len()
        })
        .filter(|&n| n > 0)
        .collect();
    assert_eq!(counts, vec![1, 2, 1]);
}

#[test]
fn register_limit() {
    let mut many = String::new();
    for i in 0..70_000 {
        let _ = write!(many, "let v{i};");
    }
    let wide: Vec<u16> = format!("function f() {{ {many} }}")
        .encode_utf16()
        .collect();
    let error = parse_script(Str16::Wide(&wide), &mut RecursionBudget::default())
        .expect_err("too many registers");
    assert_eq!(error.message, "Too many variables declared in a function");
    // At the top level they are global and need no registers.
    let wide: Vec<u16> = many.encode_utf16().collect();
    assert!(parse_script(Str16::Wide(&wide), &mut RecursionBudget::default()).is_ok());
}

fn parse_error(source: &str) -> crate::ParseError {
    let wide: Vec<u16> = source.encode_utf16().collect();
    parse_script(Str16::Wide(&wide), &mut RecursionBudget::default()).expect_err("an error")
}

#[test]
fn register_limit_keeps_a_reserve() {
    use super::analysis::{Limits, TEMPORARIES_RESERVE};
    let max = Limits::DEFAULT.declared_registers as usize;
    assert_eq!(max, 65_535 - TEMPORARIES_RESERVE as usize);
    let lets = |n: usize| {
        let mut body = String::new();
        for i in 0..n {
            let _ = write!(body, "let v{i};");
        }
        format!("function f() {{ {body} }}")
    };
    let script = analyze(&lets(max));
    assert_eq!(
        script
            .scopes
            .function(crate::ast::FunctionId::from_index(1))
            .register_count as usize,
        max
    );
    assert_eq!(
        parse_error(&lets(max + 1)).message,
        "Too many variables declared in a function"
    );
}

#[test]
fn mapped_arguments_make_parameters_cells() {
    // Sloppy mode, simple parameters, `arguments` used: all parameters
    // are cells that the mapped object holds.
    check(
        "function f(a,b){ arguments[0]=1; return a }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [a b] registers 3 arguments mapped
         ||scope 1 Function: a cell r0 Parameter, b cell r1 Parameter, arguments r2 Arguments",
        "f@9:=global a@11:=cell0 b@13:=cell1 arguments@17:r2 a@40:cell0",
    );
    // The same through an arrow function.
    check(
        "function f(a,b){ return () => arguments }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [a b] registers 3 arguments mapped
         ||scope 1 Function: a cell r0 Parameter, b cell r1 Parameter, arguments cell r2 Arguments
         function 2 parent 1 params [] registers 0 captures [arguments<-r2]",
        "f@9:=global a@11:=cell0 b@13:=cell1 arguments@30:cap0",
    );
    // Strict mode: unmapped, registers.
    check(
        "function f(a,b){ 'use strict'; arguments[0]=1; return a }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [a b] registers 3 arguments
         ||scope 1 Function: a r0 Parameter, b r1 Parameter, arguments r2 Arguments",
        "f@9:=global a@11:=r0 b@13:=r1 arguments@31:r2 a@54:r0",
    );
    // No `arguments`: registers.
    check(
        "function f(a,b){ return a }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [a b] registers 2
         ||scope 1 Function: a r0 Parameter, b r1 Parameter",
        "f@9:=global a@11:=r0 b@13:=r1 a@24:r0",
    );
}

#[test]
fn closure_in_for_head_sees_the_initial_cell() {
    // `g()` returns 0 in Node.js: the closure keeps the cell of the
    // initialization; the loop copies the cell before the first test and
    // before every update (§14.7.4.4), so the body writes the copy.
    check(
        "for (let i = 0, g = () => i; i < 1; i++) { i = 5 }",
        "function 0 params [] registers 2
         ||scope 1 For: per-iteration i cell r0 Let, g r1 Let
         function 1 parent 0 params [] registers 0 captures [i<-r0]",
        "i@9:=cell0 g@16:=r1 i@26:cap0! i@29:cell0 i@36:cell0 i@43:cell0",
    );
}

/// The `needs_tdz` flags of the bindings named `name`, in order.
fn needs_tdz(script: &Script, name: &str) -> Vec<bool> {
    let tree = &script.scopes;
    (0..tree.binding_count())
        .map(|i| tree.binding(crate::ast::BindingId::from_index(i)))
        .filter(|b| script.name_text(b.name) == name)
        .map(|b| b.needs_tdz)
        .collect()
}

#[test]
fn needs_tdz_marks_bindings_read_early() {
    let script = analyze(
        "function f() { { let a = 1; a; } { let b; b = 2; } { c; let c; } \
         { let d; () => d; } { let e = 1; () => 1; } }",
    );
    assert_eq!(needs_tdz(&script, "a"), [false]);
    assert_eq!(needs_tdz(&script, "b"), [false]);
    // Read before the declaration (the register is reused by the blocks
    // before it).
    assert_eq!(needs_tdz(&script, "c"), [true]);
    // A closure can read it.
    assert_eq!(needs_tdz(&script, "d"), [true]);
    assert_eq!(needs_tdz(&script, "e"), [false]);
    // A direct eval can read everything.
    let script = analyze("function f() { let a = 1; eval('a'); }");
    assert_eq!(needs_tdz(&script, "a"), [true]);
    // The clauses of a switch share a scope; a later clause can run
    // without the declaration of an earlier one.
    let script = analyze("function f(x) { switch (x) { case 0: let s = 1; case 1: s; } }");
    assert_eq!(needs_tdz(&script, "s"), [true]);
}

/// Parses `source`; panics when it takes longer than `seconds`.
fn parse_within(source: &str, seconds: u64) -> Script {
    let started = std::time::Instant::now();
    let script = analyze(source);
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(seconds),
        "took {elapsed:?}"
    );
    script
}

#[test]
fn var_declarations_cost_a_constant_each() {
    // 1,000 nested blocks and 20,000 distinct `var`s: every `var` used to
    // walk all enclosing blocks (about 20 million steps).
    let mut source = "{".repeat(1_000);
    for i in 0..20_000 {
        let _ = write!(source, "var v{i};");
    }
    source.push_str(&"}".repeat(1_000));
    let script = parse_within(&source, 2);
    assert_eq!(script.scopes.scope_count(), 1_001);
    assert_eq!(script.scopes.binding_count(), 20_000);
}

#[test]
fn references_resolve_in_bounded_time() {
    // 1,300 nested blocks that each declare a name, and 200,000
    // references to a few outer names at the bottom.
    let mut source = String::new();
    for i in 0..1_300 {
        let _ = write!(source, "{{ let n{i};");
    }
    for _ in 0..50_000 {
        source.push_str("n0; n1; n2; n3;");
    }
    source.push_str(&"}".repeat(1_300));
    let script = parse_within(&source, 3);
    assert_eq!(script.scopes.reference_count(), 1_300 + 200_000);
}

#[test]
fn capture_limits_are_range_errors() {
    use super::analysis::Limits;
    use crate::ErrorKind;
    use crate::parser::parse_script_with_limits;
    let parse = |source: &str, limits: Limits| {
        let wide: Vec<u16> = source.encode_utf16().collect();
        parse_script_with_limits(&wide, limits)
    };
    let limits = Limits {
        total_captures: 20,
        function_captures: 5,
        ..Limits::DEFAULT
    };
    // 4 variables used in a function 3 levels down: 3 x 4 = 12 entries.
    let nested = "function a() { var v0, v1, v2, v3; function b() { function c() { \
                  function d() { return v0 + v1 + v2 + v3; } } } }";
    let script = parse(nested, limits).expect("within the limits");
    let d = crate::ast::FunctionId::from_index(4);
    assert_eq!(script.scopes.function(d).captures.len(), 4);
    // 6 levels: 24 entries, past the total of 20.
    let deeper = "function a() { var v0, v1, v2, v3; function b() { function c() { \
                  function d() { function e() { function f() { function g() { \
                  return v0 + v1 + v2 + v3; } } } } } } }";
    let error = parse(deeper, limits).expect_err("past the total");
    assert_eq!(error.kind, ErrorKind::Range);
    assert_eq!(error.message, "Too many captured variables in a script");
    // One function with 6 captures, past the limit of 5.
    let wide = "function a() { var v0, v1, v2, v3, v4, v5; \
                function b() { return v0 + v1 + v2 + v3 + v4 + v5; } }";
    let error = parse(wide, limits).expect_err("past the function limit");
    assert_eq!(error.kind, ErrorKind::Range);
    let five = wide.replace(" + v5", "");
    assert!(parse(&five, limits).is_ok());
}

#[test]
fn default_capture_limits() {
    use super::analysis::Limits;
    assert_eq!(Limits::DEFAULT.function_captures, 65_535);
    assert_eq!(Limits::DEFAULT.total_captures, 1 << 20);
}

#[test]
fn parameter_expressions_have_their_own_scope() {
    // §10.2.11 step 28: the body's declarations get a scope of their own;
    // the default sees the parameters (with a TDZ for later ones), not
    // the body's `var a`. Registers 0 to 2 receive the arguments.
    check(
        "function f(a, b = () => a + c, c) { var a; var d; let e; return a; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 9
         ||scope 1 Function: a cell r3 Parameter, b r4 Parameter, c cell r5 Parameter
         ||scope 3 FunctionBody: a r6 Var, d r7 Var, e r8 Let
         function 2 parent 1 params [] registers 0 captures [a<-r3 c<-r5]",
        "f@9:=global a@11:=cell3 b@14:=r4 a@24:cap0! c@28:cap1! c@31:=cell5 a@40:=r6 \
         d@47:=r7 e@54:=r8 a@64:r6",
    );
    // Patterns without expressions: one scope; `var a` is the parameter.
    check(
        "function f([a], {b}) { var a; return b; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 4
         ||scope 1 Function: a r2 Parameter, b r3 Parameter",
        "f@9:=global a@12:=r2 b@17:=r3 a@27:=r2 b@37:r3",
    );
    // A body `var arguments` starts with the arguments object, which the
    // function then creates.
    check(
        "function f(a = 1) { var arguments; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 4 arguments
         ||scope 1 Function: a r1 Parameter, arguments r2 Arguments
         ||scope 2 FunctionBody: arguments r3 Var",
        "f@9:=global a@11:=r1 arguments@24:=r3",
    );
}

#[test]
fn for_in_of_bindings() {
    // The right side sees the head's bindings uninitialized; each
    // iteration has new cells.
    check(
        "for (let x of x) { g(() => x); }",
        "function 0 params [] registers 1
         ||scope 1 ForInOf: per-iteration x cell r0 Let
         function 1 parent 0 params [] registers 0 captures [x<-r0]",
        "x@9:=cell0 x@14:cell0! g@19:global x@27:cap0!",
    );
    check(
        "for (const [k, v] of y) g(() => k);",
        "function 0 params [] registers 2
         ||scope 1 ForInOf: per-iteration k cell r0 Const, v r1 Const
         function 1 parent 0 params [] registers 0 captures [k<-r0]",
        "k@12:=cell0 v@15:=r1 y@21:global g@24:global k@32:cap0!",
    );
    check(
        "for (var i in o) i;",
        "function 0 params [] registers 0
         ||scope 0 Script: i global Var",
        "i@9:=global o@14:global i@17:global",
    );
}

#[test]
fn arrow_parameters_move_into_the_arrow() {
    // The function in the default and the reference `b` in it belong to
    // the arrow function, whose parameter `b` it captures.
    check(
        "(a = function g() { return b; }, b) => a;",
        "function 0 params [] registers 0
         function 1 g parent 2 params [] registers 1 captures [b<-r3]
         ||scope 1 FunctionName: g r0 FunctionName
         function 2 parent 0 params [] registers 4
         ||scope 3 Function: a r2 Parameter, b cell r3 Parameter",
        "a@1:=r2 g@14:=r0 b@27:cap0! b@33:=cell3 a@39:r2",
    );
    // A direct `eval` in the parameters belongs to the arrow function: its
    // vars go into an environment outside the parameters, and the arrow
    // captures what the eval code may use from `f`.
    check(
        "function f() { (z = eval('x')) => z; }",
        "function 0 params [] registers 0 eval
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 3 this arguments mapped eval
         ||scope 1 Function: this cell r0 This, new.target cell r1 NewTarget, arguments cell r2 Arguments
         function 2 parent 1 params [] registers 3 captures [this<-r0 new.target<-r1 arguments<-r2] eval
         ||scope 2 Function: z cell r1 Parameter, %env cell r2 DynamicEnv",
        "f@9:=global z@16:=cell1 eval@20:global~1 z@34:cell1 this@15:cap0 new.target@15:cap1 \
         arguments@15:cap2~1 %env@15:global %env@20:cell2",
    );
    let script = analyze("function f() { (z = eval('x')) => z; }");
    let f = crate::ast::FunctionId::from_index(1);
    let arrow = crate::ast::FunctionId::from_index(2);
    assert!(!script.scopes.function(f).has_direct_eval);
    assert!(script.scopes.function(arrow).has_direct_eval);
}

#[test]
fn new_target_catch_patterns_and_with() {
    check(
        "function f() { return () => new.target; }",
        "function 0 params [] registers 0
         ||scope 0 Script: f global Function
         function 1 f parent 0 params [] registers 1
         ||scope 1 Function: new.target cell r0 NewTarget
         function 2 parent 1 params [] registers 0 captures [new.target<-r0]",
        "f@9:=global new.target@28:cap0",
    );
    check(
        "try {} catch ([e, {f}]) { e; }",
        "function 0 params [] registers 2
         ||scope 2 Catch: e r0 CatchParameter, f r1 CatchParameter",
        "e@15:=r0 f@19:=r1 e@26:r0",
    );
    // The `with` scope holds its object environment; the names inside
    // check it first (`~1`).
    check(
        "with (o) { var w; w; }",
        "function 0 params [] registers 1
         ||scope 0 Script: w global Var
         ||scope 1 With: %env r0 DynamicEnv",
        "o@6:global w@15:=global~1 w@18:global~1 %env@9:global %env@15:r0",
    );
}
