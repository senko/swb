//! Tests of the forms of M7 feature 1b: classes (heritage, methods,
//! accessors, fields, static blocks, private names and `#x in o`), `super`,
//! `async` functions, arrow functions, methods and generators, `await`,
//! `for await`; `await` and `yield` as identifiers by context; their early
//! errors with V8's messages and positions, and hostile inputs. The tables
//! of early errors and valid sources come from Node.js 22
//! (`new vm.Script(source)`; the position is the caret of the error).

use std::fmt::Write;
use std::time::Instant;

use swb_js_text::{RecursionBudget, Str16};

use super::parse_script;
use super::tests::{assert_too_deep, check_ast, check_errors, error, on_engine_stack, parse};
use crate::dump::{dump_references, dump_scopes};

#[test]
fn class_trees() {
    check_ast(&[
        ("class A {}", "(class A)"),
        ("class A extends B.c {}", "(class A (extends (. B c)))"),
        ("x = class {}", "(= x (class))"),
        (
            "x = class A extends (B, C) {}",
            "(= x (class A (extends (paren (, B C)))))",
        ),
        (
            "class A { constructor(a) { super.x; } m() {} static n() {} get a() {} set a(v) {} static get b() {} }",
            "(class A (constructor (ctor (a) (super. x))) (method m (method ())) (static method n (method ())) (get a (getter ())) (set a (setter (v))) (static get b (getter ())))",
        ),
        (
            "class A { *g() {} async m() {} async *ag() {} static async *s() {} }",
            "(class A (method g (method* ())) (method m (async method ())) (method ag (async method* ())) (static method s (async method* ())))",
        ),
        (
            "class A { ['k']() {} 1() {} 'x'() {} if() {} #p() {} }",
            "(class A (method [\"k\"] (method ())) (method 1 (method ())) (method x (method ())) (method if (method ())) (method #p (method ())))",
        ),
        (
            "class A { a; b = 1; [c] = 2; static d; static e = this; #f = 3; static #g }",
            "(class A (field a) (field b 1) (field [c] 2) (static field d) (static field e this) (field #f 3) (static field #g))",
        ),
        (
            "class A { static { var x; let y; } static {} }",
            "(class A (static block (var x) (let y)) (static block))",
        ),
        (
            "class A { 'constructor'() {} }",
            "(class A (constructor (ctor ())))",
        ),
        (
            "class A { get; set; static; async }",
            "(class A (field get) (field set) (field static) (field async))",
        ),
        (
            "class A { static async *#m() {} get #a() {} set #a(v) {} }",
            "(class A (static method #m (async method* ())) (get #a (getter ())) (set #a (setter (v))))",
        ),
        (
            "class A { get\na() {} static\nb() {} async\nc() {} }",
            "(class A (get a (getter ())) (static method b (method ())) (field async) (method c (method ())))",
        ),
    ]);
}

#[test]
fn super_and_private_name_trees() {
    check_ast(&[
        (
            "class A extends B { constructor() { super(1, ...a); super.m(); super['k'] = 1; } }",
            "(class A (extends B) (constructor (ctor () (super-call 1 (... a)) (call (super. m)) (= (super[] \"k\") 1))))",
        ),
        (
            "x = { m() { return super.x; } }",
            "(= x (object (method m (method () (return (super. x))))))",
        ),
        (
            "class A { #x; m(o) { return this.#x + o?.#x + (#x in o); } }",
            "(class A (field #x) (method m (method (o) (return (+ (+ (. this #x) (chain (?. o #x))) (paren (in #x o)))))))",
        ),
        (
            "class A { #x; m() { this.#x = 1; this.#x++; [this.#x] = [1]; } }",
            "(class A (field #x) (method m (method () (= (. this #x) 1) (postfix++ (. this #x)) (= [(. this #x)] (array 1)))))",
        ),
        (
            "class A { #x; m(o) { return #x in o in p && #x in o < 1; } }",
            "(class A (field #x) (method m (method (o) (return (&& (in (in #x o) p) (< (in #x o) 1))))))",
        ),
    ]);
}

#[test]
fn async_trees() {
    check_ast(&[
        (
            "async function f() { await x; }",
            "(async function f () (await x))",
        ),
        (
            "async function* g() { yield await x; for await (const y of z) {} }",
            "(async function* g () (yield (await x)) (for-await-of (const y) z (block)))",
        ),
        ("x = async function () {}", "(= x (async function ()))"),
        (
            "x = async function* named() {}",
            "(= x (async function* named ()))",
        ),
        ("x = async () => await 1", "(= x (async => () (await 1)))"),
        ("x = async x => x", "(= x (async => (x) x))"),
        (
            "x = async (a, [b], {c} = {}, ...d) => a",
            "(= x (async => (a [b] (= {(c c)} (object)) ...d) a))",
        ),
        ("x = async (a)", "(= x (call async a))"),
        ("x = async", "(= x async)"),
        ("x = async(a, b)", "(= x (call async a b))"),
        (
            "x = { async m() {}, async *n() {}, async: 1, async() {}, async }",
            "(= x (object (method m (async method ())) (method n (async method* ())) (init async 1) (method async (method ())) (shorthand async)))",
        ),
        (
            "for (async of => {};;) ;",
            "(for (async => (of)) _ _ (empty))",
        ),
        (
            "async function f() { for await (async of x) ; }",
            "(async function f () (for-await-of async x (empty)))",
        ),
        (
            "async function f() { for await (x.y of z) ; }",
            "(async function f () (for-await-of (. x y) z (empty)))",
        ),
        (
            "label: async\nfunction f() {}",
            "(label label async) (function f ())",
        ),
        ("var await; await: await", "(var await) (label await await)"),
        (
            "async function f() { () => await }",
            "(async function f () (=> () await))",
        ),
    ]);
}

#[test]
fn class_and_async_early_errors_match_v8() {
    check_errors(CLASS_ASYNC_ERRORS);
}

#[test]
fn class_and_async_valid_sources_parse() {
    for source in CLASS_ASYNC_VALID {
        if let Err(error) = parse(source) {
            panic!("{source}: {error}");
        }
    }
}

/// Where swb follows ECMA-262 2025 and V8 (Node.js 22) does not, or V8's
/// message is not a rule: the swb error of each source.
#[test]
fn deviations_from_v8() {
    check_errors(&[
        // §14.7.5: `for await` is a keyword sequence; a keyword must not
        // contain escapes (§12.7.1). V8 accepts it.
        (
            "async function f() { for \u{2038}aw\\u0061it (x of y); }",
            "Keyword must not contain escaped characters",
        ),
        // §13.15.1: an optional chain is not a valid assignment target.
        // V8 accepts `this?.#x = 1`.
        (
            "class C { #x; m() { \u{2038}this?.#x = 1 } }",
            "Invalid left-hand side in assignment",
        ),
        // A call is no assignment target in strict mode code (see
        // `m7_early_errors_match_v8`); class code is strict. V8 accepts it.
        (
            "x = class extends B { constructor() { \u{2038}super() = 1 } }",
            "Invalid left-hand side in assignment",
        ),
        // All parts of a class are strict mode code (§15.7, note). V8
        // accepts legacy octal literals in computed keys and static blocks.
        (
            "x = class { [\u{2038}010]() {} }",
            "Octal literals are not allowed in strict mode.",
        ),
        (
            "x = class { static { \u{2038}010 } }",
            "Octal literals are not allowed in strict mode.",
        ),
        // V8 reports an invalid pattern in a `for await` declaration as
        // "Invalid left-hand side in for-await-of loop: Must have a
        // single binding." without a position; swb gives the pattern's
        // error.
        (
            "async function f() { for await (const [\u{2038}...x = []] of [[]]) ; }",
            "Invalid destructuring assignment target",
        ),
        (
            "async function f() { for await (const [\u{2038}...x, y] of [[]]) ; }",
            "Rest element must be last element",
        ),
        // ECMA-262 2025 gives field initializers the `[Await]` parameter
        // of the class; swb follows V8 (instance fields: `await` is an
        // identifier; static fields: reserved).
        (
            "async function f() { x = class { x = await \u{2038}1 } }",
            "Unexpected number",
        ),
        (
            "x = class { static x = \u{2038}await }",
            "Unexpected reserved word",
        ),
    ]);
}

/// A program with every form of feature 1b.
const CLASS_ASYNC_PROGRAM: &str = r"class A extends B.c {
  #p = 1; static #q; [k] = 2; 'f'; static g = this;
  constructor(a, ...b) { super(a, ...b); const f = () => super.m(new.target); }
  m() { return this.#p + (#q in A) + super.x; } static n() {} get a() {} set a(v) {}
  *gen() { yield 1; } async am() { await this; } async *ag() { for await (const x of y) yield x; }
  static { var v = 1; let w = v; this.#q = super.y; }
  get #r() {} set #r(v) {} static async *#s() {}
}
x = class { #t; static has(o) { return #t in o; } };
async function af(p = 1) { await p; return async () => await p; }
async function* ag() { yield* x; }
const h = async (a, [b], {c} = {}, ...d) => a, i = async e => e, j = async(a, b);
x = { async m() {}, async *n() {}, get [k]() { return super.k; }, async, async: 1 };
for (async of => {};;) break;
";

#[test]
fn every_prefix_and_suffix_of_the_class_program_parses_or_fails() {
    on_engine_stack(|| {
        assert!(parse(CLASS_ASYNC_PROGRAM).is_ok());
        let wide: Vec<u16> = CLASS_ASYNC_PROGRAM.encode_utf16().collect();
        for end in 0..=wide.len() {
            let mut budget = RecursionBudget::DEFAULT;
            let _ = parse_script(Str16::Wide(&wide[..end]), &mut budget);
            assert_eq!(budget, RecursionBudget::DEFAULT);
        }
        for start in 0..=wide.len() {
            let mut budget = RecursionBudget::DEFAULT;
            let _ = parse_script(Str16::Wide(&wide[start..]), &mut budget);
            assert_eq!(budget, RecursionBudget::DEFAULT);
        }
    });
}

#[test]
fn hostile_deep_classes_and_async() {
    on_engine_stack(|| {
        const N: usize = 100_000;
        let cases = [
            format!("{}{}", "class A { m() { ".repeat(N), "} }".repeat(N)),
            format!("x = {}B{}", "class extends ".repeat(N), " {}".repeat(N)),
            format!("x = {}B{}", "class extends (".repeat(N), ") {}".repeat(N)),
            format!("x = {}0{}", "class { [".repeat(N), "]() {} }".repeat(N)),
            format!("x = {}0{}", "class { x = ".repeat(N), " }".repeat(N)),
            format!(
                "x = {}0{}",
                "class { static { x = ".repeat(N),
                " } }".repeat(N)
            ),
            format!("{}0", "async a => ".repeat(N)),
            format!("{}0", "async (a) => ".repeat(N)),
            format!("{}0{}", "async(".repeat(N), ")".repeat(N)),
            format!("{}{}", "async function f() { ".repeat(N), "}".repeat(N)),
            format!("async function f() {{ {}x }}", "await ".repeat(N)),
            format!("async function f() {{ {}x }}", "await -".repeat(N)),
            format!(
                "x = {{ m() {{ {}0{} }} }}",
                "super[".repeat(N),
                "]".repeat(N)
            ),
            format!(
                "class A {{ #x; m() {{ {}0{} }} }}",
                "#x in (".repeat(N),
                ")".repeat(N)
            ),
        ];
        for source in &cases {
            assert_too_deep(source);
        }
    });
}

/// Parses `source` and returns the seconds it took.
fn timed_parse(source: &str) -> f64 {
    let start = Instant::now();
    if let Err(error) = parse(source) {
        panic!("{}...: {error}", &source[..40.min(source.len())]);
    }
    start.elapsed().as_secs_f64()
}

/// `count` copies of `item` with `{i}` replaced by the index.
fn repeat_indexed(item: &str, count: usize) -> String {
    (0..count).fold(String::new(), |mut s, i| {
        let _ = write!(s, "{}", item.replace("{i}", &i.to_string()));
        s
    })
}

#[test]
fn hostile_wide_classes_are_linear() {
    on_engine_stack(|| {
        // 100,000 members, private names and awaits: a quadratic step
        // would take minutes.
        let n = 100_000;
        let sources = [
            format!("class A {{ {} }}", repeat_indexed("m{i}() {} ", n)),
            format!("class A {{ {} }}", repeat_indexed("f{i} = {i}; ", n)),
            format!("class A {{ {} }}", repeat_indexed("static f{i}; ", n)),
            format!(
                "class A {{ {} }}",
                repeat_indexed("static { var v{i}; } ", n)
            ),
            // Each private name takes a register of the enclosing
            // function (at most 64,511 declared registers).
            format!(
                "class A {{ {} {} }}",
                repeat_indexed("#p{i}; ", 60_000),
                repeat_indexed("m{i}() { this.#p{i}; } ", 60_000)
            ),
            // Uses before the declarations, resolved when the class ends.
            format!(
                "class A {{ {} {} }}",
                repeat_indexed("m{i}() { return #p{i} in this; } ", 60_000),
                repeat_indexed("#p{i}; ", 60_000)
            ),
            // Uses that cross 100 nested classes to the outermost one.
            format!(
                "class A {{ {} {} {} }}",
                repeat_indexed("#p{i}; ", 1_000),
                "m() { class B { ".repeat(100) + &repeat_indexed("n{i}() { this.#p{i}; } ", 1_000),
                "} }".repeat(100)
            ),
            format!(
                "async function f() {{ {} }}",
                repeat_indexed("await a{i}; ", n)
            ),
            format!(
                "async function f() {{ {} }}",
                repeat_indexed("for await (x of y{i}); ", n)
            ),
            format!("x = [{}]", repeat_indexed("async a => a, ", n)),
            format!("x = [{}]", repeat_indexed("async (a, b) => a, ", n)),
            format!("x = [{}]", repeat_indexed("async(a, b), ", n)),
            format!("x = {{ {} }}", repeat_indexed("async m{i}() {}, ", n)),
            format!("x = {{ m() {{ {} }} }}", repeat_indexed("super.a{i}; ", n)),
        ];
        for source in &sources {
            assert!(timed_parse(source) < 30.0);
        }
        // 100,000 private names: past the register limit, a clean error.
        let source = format!("class A {{ {} }}", repeat_indexed("#p{i}; ", n));
        let start = Instant::now();
        assert_eq!(error(&source), "Too many variables declared in a function");
        assert!(start.elapsed().as_secs_f64() < 30.0);
    });
}

#[test]
fn class_scopes_and_implicit_bindings() {
    let script = parse(
        "let outer = 1;
class A extends B {
  #p = outer;
  static s = this;
  constructor(x) {
    super(x);
    const f = () => super.m(this.#p, new.target);
    const g = () => super();
  }
  m() { return A; }
  static { var v = 1; let w = v; this.#p; }
  static { var v = 2; }
}",
    )
    .expect("the class");
    assert_eq!(
        dump_scopes(&script),
        "function 0 params [] registers 2
  scope 0 Script: outer global Let, A global Class
  scope 1 Class: A cell r0 ClassName
  scope 2 ClassBody: #p cell r1 PrivateName
function 1 parent 0 params [] registers 0
function 2 parent 0 params [] registers 3 captures [#p<-r1] this
  scope 4 Function: this r0 This
  scope 9 StaticBlock: v r1 Var, w r2 Let
  scope 10 StaticBlock: v r1 Var
function 3 parent 0 params [x] registers 7 captures [#p<-r1] this
  scope 5 Function: x r0 Parameter, f r1 Const, g r2 Const, this cell r3 This, new.target cell r4 NewTarget, %function cell r5 ActiveFunction, super cell r6 HomeObject
function 4 parent 3 params [] registers 0 captures [this<-r3 super<-r6 #p<-c0 new.target<-r4]
function 5 parent 3 params [] registers 0 captures [this<-r3 new.target<-r4 %function<-r5]
function 6 parent 0 params [] registers 0 captures [A<-r0]
"
    );
    // The inner name binding of `A` has a TDZ check in the method; the
    // field initializer reads the global `outer`.
    let references = dump_references(&script);
    assert!(references.contains("A@199:cap0!"), "{references}");
    assert!(references.contains("outer@42:global!"), "{references}");
}

#[test]
fn classes_in_arrow_parameters_move_with_them() {
    let script = parse(
        "function outer() {
  let y = 1;
  return (a = class C extends (y, B) { [eval('k')]() { return C; } static { this.z = a; } x = y; }) => a;
}",
    )
    .expect("the arrow");
    let scopes = dump_scopes(&script);
    // The class scope and the class's functions belong to the arrow
    // function (5), which also has the direct `eval` of the computed key.
    assert!(
        scopes.contains("function 5 parent 1 params [] registers 3 captures [y<-r0] eval\n  scope 2 Class: C cell r2 ClassName"),
        "{scopes}"
    );
    for function in [
        "function 2 parent 5",
        "function 3 parent 5",
        "function 4 parent 5",
    ] {
        assert!(scopes.contains(function), "{scopes}");
    }
}

#[test]
fn function_source_texts() {
    // `Function.prototype.toString` (§20.2.3.5) slices these spans.
    let source = "class A { static async *m() {} get [k]() {} constructor(a) {} x = 1; static { } }
x = async (a) => a; y = async a => a; z = async function f() {}; w = { async m() {} };";
    let script = parse(source).expect("the source");
    let texts: Vec<&str> = script
        .ast
        .function_ids()
        .skip(1)
        .map(|id| {
            let span = script.ast.function(id).span;
            &source[span.start as usize..span.end as usize]
        })
        .collect();
    assert_eq!(
        texts,
        [
            "async *m() {}",
            "get [k]() {}",
            "constructor(a) {}",
            "x = 1; static { } }",
            "static { } }",
            "async (a) => a",
            "async a => a",
            "async function f() {}",
            "async m() {}",
        ]
    );
}

#[test]
fn async_arrow_functions_capture_like_arrows() {
    let script =
        parse("async function af(p) { await p; const g = async (q) => await q + p; return g; }")
            .expect("async functions");
    assert_eq!(
        dump_scopes(&script),
        "function 0 params [] registers 0
  scope 0 Script: af global Function
function 1 af parent 0 params [p] registers 2
  scope 1 Function: p cell r0 Parameter, g r1 Const
function 2 parent 1 params [q] registers 1 captures [p<-r0]
  scope 2 Function: q r0 Parameter
"
    );
    // The callee `async` of the arrow head is no use of a binding.
    let references = dump_references(&script);
    assert!(
        references.contains("async@42:dead q@49:=r0"),
        "{references}"
    );
}

#[test]
fn private_names_resolve_through_enclosing_classes() {
    let script = parse(
        "class A { #x; m() { return class B { #y; n(o) { return o.#x + o.#y + (#x in o); } }; } }",
    )
    .expect("nested classes");
    let references = dump_references(&script);
    // `#x` in B's method is a capture of A's private name (through `m`);
    // `#y` of B's own.
    assert!(references.contains("#x@57:cap0"), "{references}");
    assert!(references.contains("#y@64:cap1"), "{references}");
    assert!(references.contains("#x@70:cap0"), "{references}");
    let scopes = dump_scopes(&script);
    assert!(scopes.contains("captures [#x<-c0 #y<-r1]"), "{scopes}");
}

const CLASS_ASYNC_ERRORS: &[(&str, &str)] = &[
    (
        "async function f(){ class C { x = await ‸1 } }",
        "Unexpected number",
    ),
    (
        "async function f(){ class C { static x = ‸await 1 } }",
        "Unexpected reserved word",
    ),
    ("class C { static { ‸await } }", "Unexpected reserved word"),
    (
        "class C { static { var ‸await } }",
        "Unexpected reserved word",
    ),
    ("class C { static { ‸return } }", "Illegal return statement"),
    (
        "class C { x = ‸arguments }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class C { x = () => ‸arguments }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class C { static { ‸arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "class C { constructor(){} ‸constructor(){} }",
        "A class may only have one constructor",
    ),
    (
        "class C { static ‸prototype(){} }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "class C { static ‸prototype = 1 }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "class C { ‸constructor = 1 }",
        "Classes may not have a field named 'constructor'",
    ),
    (
        "class C { get ‸constructor(){} }",
        "Class constructor may not be an accessor",
    ),
    (
        "class C { *‸constructor(){} }",
        "Class constructor may not be a generator",
    ),
    (
        "class C { async ‸constructor(){} }",
        "Class constructor may not be an async method",
    ),
    (
        "class C { ‸#constructor }",
        "Classes may not have a field named 'constructor'",
    ),
    (
        "class C { #a; ‸#a }",
        "Identifier '#a' has already been declared",
    ),
    (
        "class C { get #a(){} static set #a(v){‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "class C { m() { this‸.#a } }",
        "Private field '#a' must be declared in an enclosing class",
    ),
    (
        "class C { #a; m() { delete this.‸#a } }",
        "Private fields can not be deleted",
    ),
    (
        "class C { m() { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "function f() { ‸super.x }",
        "'super' keyword unexpected here",
    ),
    ("({ m() { ‸super() } })", "'super' keyword unexpected here"),
    (
        "class C { x = ‸super() }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { static { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    ("‸super.x", "'super' keyword unexpected here"),
    (
        "class C { m() { function f() { ‸super.x } } }",
        "'super' keyword unexpected here",
    ),
    (
        "function f() { for ‸await (x of y); }",
        "Unexpected reserved word",
    ),
    ("for ‸await (x of y);", "Unexpected reserved word"),
    (
        "async function f() { for await (x ‸in y); }",
        "Unexpected token 'in'",
    ),
    (
        "async function f() { for await (let ‸x = 1;;); }",
        "for-await-of loop variable declaration may not have an initializer.",
    ),
    ("async (a)\n‸=> 1", "Unexpected token '=>'"),
    (
        "async ‸await => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async (‸await) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    ("async (x = ‸await 1) => 1", "missing ) after argument list"),
    (
        "function* g() { async (x = ‸yield) => 1 }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "function* g() { async (‸yield) => 1 }",
        "Invalid destructuring assignment target",
    ),
    (
        "async function f() { (x = ‸await 1) => 1 }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f() { (await‸) => 1 }",
        "Unexpected token ')'",
    ),
    (
        "async function f() { var ‸await }",
        "Unexpected reserved word",
    ),
    ("async function f(‸await) {}", "Unexpected reserved word"),
    (
        "async function f() { function ‸await() {} }",
        "Unexpected reserved word",
    ),
    (
        "function* g() { function* ‸yield() {} }",
        "Unexpected identifier 'yield'",
    ),
    (
        "function* g() { var ‸yield }",
        "Unexpected identifier 'yield'",
    ),
    ("({ async get ‸m() {} })", "Unexpected identifier 'm'"),
    ("({ async\n‸m() {} })", "Unexpected identifier 'm'"),
    (
        "class C { m() ‸{ #x in this } }",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "class C { #x; m() { (‸#x) in this } }",
        "Unexpected identifier '#x'",
    ),
    ("class C { #x; m() { ‸#x } }", "Unexpected identifier '#x'"),
    ("class C extends a ‸= b {}", "Unexpected token '='"),
    ("class C extends async () ‸=> 1 {}", "Unexpected token '=>'"),
    ("class ‸yield {}", "Unexpected strict mode reserved word"),
    ("class ‸let {}", "Unexpected strict mode reserved word"),
    ("class ‸static {}", "Unexpected strict mode reserved word"),
    ("class C { a ‸b }", "Unexpected identifier 'b'"),
    ("class C { a = 1 ‸b }", "Unexpected identifier 'b'"),
    (
        "class C { 'constructor'(){} ‸'constructor'(){} }",
        "A class may only have one constructor",
    ),
    (
        "(class { constructor(){} ‸constructor(){} })",
        "A class may only have one constructor",
    ),
    ("async function f() { () => await ‸1 }", "Unexpected number"),
    (
        "async function f() { (x = await‸) => 1 }",
        "Unexpected token ')'",
    ),
    (
        "async function f(x = ‸await 1) {}",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f(x = ‸await) {}",
        "Illegal await-expression in formal parameters of async function",
    ),
    ("async function f() { await ‸}", "Unexpected token '}'"),
    ("async function f() { await‸: 1 }", "Unexpected token ':'"),
    (
        "async function f() { x = { ‸await } }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { await ‸=> 1 }",
        "Unexpected token '=>'",
    ),
    (
        "async function f() { async ‸await => 1 }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { class ‸await {} }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { class C extends ‸await x {} }",
        "Unexpected reserved word",
    ),
    (
        "function f() { class C { [await ‸x]() {} } }",
        "Unexpected identifier 'x'",
    ),
    (
        "async function f() { class C { m(x = await ‸1) {} } }",
        "Unexpected number",
    ),
    (
        "async function f() { class C { async m(x = ‸await 1) {} } }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f() { class C { async m(‸await) {} } }",
        "Unexpected reserved word",
    ),
    ("x = async ()\n‸=> 1", "Unexpected token '=>'"),
    (
        "x = async (...a‸, b) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "async (a, ‸a) => 1",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "async (x = 1) => { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "async (x) => { let ‸x }",
        "Identifier 'x' has already been declared",
    ),
    (
        "async x => { let ‸x }",
        "Identifier 'x' has already been declared",
    ),
    (
        "\"use strict\"; async (‸eval) => 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; async ‸eval => 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; async (‸yield) => 1",
        "Unexpected strict mode reserved word",
    ),
    (
        "function* g() { async ‸yield => 1 }",
        "Unexpected identifier 'yield'",
    ),
    (
        "async function* g() { yield\n‸* x }",
        "Unexpected token '*'",
    ),
    (
        "async function* g() { await ‸yield 1 }",
        "Unexpected identifier 'yield'",
    ),
    (
        "async function* g() { yield await ‸}",
        "Unexpected token '}'",
    ),
    (
        "async function f() { ‸await 1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f() { new ‸await 1 }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { ‸(await 1)++ }",
        "Invalid left-hand side expression in postfix operation",
    ),
    (
        "async function f() { ‸await x = 1 }",
        "Invalid left-hand side in assignment",
    ),
    (
        "async function f() { [‸await x] = 1 }",
        "Invalid destructuring assignment target",
    ),
    (
        "async function f() { for await (let of ‸x) {} }",
        "Unexpected identifier 'x'",
    ),
    (
        "async function f() { for await (‸let.x of y) {} }",
        "The left-hand side of a for-of loop may not start with 'let'.",
    ),
    (
        "async function f() { for await (x‸, y of z) {} }",
        "Unexpected token ','",
    ),
    (
        "async function f() { for await (var ‸x = 1 of y) {} }",
        "for-await-of loop variable declaration may not have an initializer.",
    ),
    (
        "async function f() { for await (x of y‸, z) {} }",
        "Unexpected token ','",
    ),
    (
        "async function f() { for (‸async of x) {} }",
        "The left-hand side of a for-of loop may not be 'async'.",
    ),
    (
        "function f() { for (‸async of x) {} }",
        "The left-hand side of a for-of loop may not be 'async'.",
    ),
    (
        "async function f() { ‸aw\\u0061it x }",
        "Keyword must not contain escaped characters",
    ),
    (
        "async function f() { var ‸aw\\u0061it }",
        "Unexpected reserved word",
    ),
    (
        "‸\\u0061sync function f() {}",
        "Keyword must not contain escaped characters",
    ),
    (
        "x = \\u0061sync ‸function f() {}",
        "Unexpected token 'function'",
    ),
    (
        "x = ‸\\u0061sync () => 1",
        "Malformed arrow function parameter list",
    ),
    ("x = \\u0061sync ‸x => 1", "Unexpected identifier 'x'"),
    (
        "({ ‸\\u0061sync m() {} })",
        "Keyword must not contain escaped characters",
    ),
    (
        "class C { ‸\\u0061sync m() {} }",
        "Keyword must not contain escaped characters",
    ),
    (
        "class C { st\\u0061tic ‸m() {} }",
        "Unexpected identifier 'm'",
    ),
    (
        "class C { ‸g\\u0065t m() {} }",
        "Keyword must not contain escaped characters",
    ),
    (
        "function* g() { ‸yi\\u0065ld }",
        "Keyword must not contain escaped characters",
    ),
    (
        "async function f() { x = { ‸aw\\u0061it } }",
        "Unexpected reserved word",
    ),
    (
        "class C { static { ‸aw\\u0061it } }",
        "Unexpected reserved word",
    ),
    (
        "class C { #a(){} ‸#a }",
        "Identifier '#a' has already been declared",
    ),
    (
        "class C { #a; #a(){‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "class C { get #a(){} get #a(){‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "class C { #a(){} get #a(){‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "class C { static #a; ‸#a }",
        "Identifier '#a' has already been declared",
    ),
    (
        "class C { get #a(){} set #a(v){} get #a(){‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "class C { ‸#constructor(){} }",
        "Class constructor may not be a private method",
    ),
    (
        "class C { get ‸#constructor(){} }",
        "Class constructor may not be a private method",
    ),
    (
        "class C { static ‸#constructor }",
        "Classes may not have a field named 'constructor'",
    ),
    (
        "class C { async *‸constructor(){} }",
        "Class constructor may not be a generator",
    ),
    (
        "class C { set ‸constructor(v){} }",
        "Class constructor may not be an accessor",
    ),
    (
        "class C { static async *‸prototype(){} }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "class C { static get ‸prototype(){} }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "class C { static ‸'prototype'(){} }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "class C { ‸'constructor' = 1 }",
        "Classes may not have a field named 'constructor'",
    ),
    (
        "class C { static ‸constructor = 1 }",
        "Classes may not have a field named 'constructor'",
    ),
    ("class C { m() { super.‸#x } }", "Unexpected private field"),
    (
        "class C { #x; m() { super.‸#x } }",
        "Unexpected private field",
    ),
    (
        "class C { m() { ‸super } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C { m() { ‸super?.x } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C { m() { new ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { constructor() { new ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { constructor() { ‸super?.() } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { m(x = ‸super()) {} }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { m() { () => ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { x = () => ‸super() }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { constructor() { function f() { ‸super() } } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { constructor() { class D { constructor() { ‸super() } } } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C extends B { constructor() { ({ m() { ‸super() } }) } }",
        "'super' keyword unexpected here",
    ),
    (
        "class C { #x; m() { delete this?.‸#x } }",
        "Private fields can not be deleted",
    ),
    (
        "class C { #x; m() { delete (this.#x‸) } }",
        "Private fields can not be deleted",
    ),
    (
        "class C { #x; m() { delete ((this.#x)‸) } }",
        "Private fields can not be deleted",
    ),
    (
        "class C { #x; m() { delete f().‸#x } }",
        "Private fields can not be deleted",
    ),
    (
        "class C { #x; m() { delete this?.y.‸#x } }",
        "Private fields can not be deleted",
    ),
    (
        "class C { #x; m() { return 1 + ‸#x in this } }",
        "Unexpected identifier '#x'",
    ),
    (
        "class C { #x; m() { return #x in ‸#x in this } }",
        "Unexpected identifier '#x'",
    ),
    (
        "class C { #x; m() { return ‸#x } }",
        "Unexpected identifier '#x'",
    ),
    (
        "class C { #x; m() { for (‸#x in this;;); } }",
        "Unexpected identifier '#x'",
    ),
    (
        "class C { #x; m() { return ‸#x instanceof this } }",
        "Unexpected identifier '#x'",
    ),
    (
        "class C { #x; m() { return a < ‸#x in this } }",
        "Unexpected identifier '#x'",
    ),
    (
        "this‸.#x",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "‸#x in this",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "class C { m() { this‸.#x } } let a; let a;",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "class C { m() { this‸.#x; this.#y } }",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "class C { m() { class D { n() { this‸.#y } } this.#x } }",
        "Private field '#y' must be declared in an enclosing class",
    ),
    (
        "class C { m() { class D { #y; n() { this.#y } } this‸.#x } }",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "class C extends (o => o‸.#x) { #x }",
        "Private field '#x' must be declared in an enclosing class",
    ),
    ("class C { ‸# x }", "Invalid or unexpected token"),
    ("class C { ‸#\nx }", "Invalid or unexpected token"),
    (
        "class C { static { ‸await 0 } }",
        "Unexpected reserved word",
    ),
    (
        "class C { static { for ‸await (x of y); } }",
        "Unexpected reserved word",
    ),
    (
        "class C { static { ‸await: 1 } }",
        "Unexpected reserved word",
    ),
    (
        "class C { static { (‸await) => 1 } }",
        "Unexpected reserved word",
    ),
    (
        "class C { static { let x; var ‸x } }",
        "Identifier 'x' has already been declared",
    ),
    ("class C { static { ‸break } }", "Illegal break statement"),
    (
        "for (;;) { class C { static { ‸break } } }",
        "Illegal break statement",
    ),
    (
        "function* g() { class C { x = ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "function* g() { class C { static { ‸yield } } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "class C { static { ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "class C { x = ‸yield }",
        "Unexpected strict mode reserved word",
    ),
    ("if (1) ‸class C {}", "Unexpected token 'class'"),
    (
        "if (1) ‸async function f() {}",
        "Async functions can only be declared at the top level or inside a block.",
    ),
    ("l: ‸class C {}", "Unexpected token 'class'"),
    (
        "l: ‸async function f() {}",
        "Async functions can only be declared at the top level or inside a block.",
    ),
    (
        "while (1) ‸async function f() {}",
        "Async functions can only be declared at the top level or inside a block.",
    ),
    (
        "{ async function f() {} ‸async function f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "{ async function f() {} ‸function f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "{ function f() {} ‸async function f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "class C {} ‸class C {}",
        "Identifier 'C' has already been declared",
    ),
    (
        "var C; ‸class C {}",
        "Identifier 'C' has already been declared",
    ),
    (
        "class C {} var ‸C",
        "Identifier 'C' has already been declared",
    ),
    (
        "let C; ‸class C {}",
        "Identifier 'C' has already been declared",
    ),
    (
        "class C {} ‸function C() {}",
        "Identifier 'C' has already been declared",
    ),
    (
        "function f() { class C {} var ‸C }",
        "Identifier 'C' has already been declared",
    ),
    (
        "class C { get a‸(x) {} }",
        "Getter must not have any formal parameters.",
    ),
    (
        "class C { set a‸() {} }",
        "Setter must have exactly one formal parameter.",
    ),
    ("class C { get\nx ‸= 1 }", "Unexpected token '='"),
    ("class C { x = 1 ‸y = 2 }", "Unexpected identifier 'y'"),
    (
        "class C { m() { let x; var ‸x } }",
        "Identifier 'x' has already been declared",
    ),
    (
        "class C { constructor(a = 1) { ‸\"use strict\" } }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    ("class C extends B‸, D {}", "Unexpected token ','"),
    ("class C extends {}‸", "Unexpected end of input"),
    ("class C extends B {}‸.x", "Unexpected token '.'"),
    ("class C {} ++‸", "Unexpected end of input"),
    (
        "x = ‸class {} ++",
        "Invalid left-hand side expression in postfix operation",
    ),
    (
        "async function f() { let ‸await }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { let [await‸] = 1 }",
        "Unexpected token ']'",
    ),
    (
        "async function f() { try {} catch (‸await) {} }",
        "Unexpected reserved word",
    ),
    (
        "async function f(a = class { [‸await 1] = 1 }) {}",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async (a = ‸await) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async ({a: ‸await}) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async ([‸await]) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async (...‸await) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "async function f() { async (x = ‸await 1) => 1 }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f() { async (await‸) => 1 }",
        "Unexpected token ')'",
    ),
    ("async x\n‸=> 1", "Unexpected token '=>'"),
    ("async ‸x;", "Unexpected identifier 'x'"),
    ("async ‸x y", "Unexpected identifier 'x'"),
    (
        "async (...a‸,) => 1",
        "Rest parameter must be last formal parameter",
    ),
    ("async (a,‸,b) => 1", "Unexpected token ','"),
    ("x = async (x) => {} ‸+ 1", "Unexpected token '+'"),
    (
        "async function f() { (‸await 1) => 1 }",
        "Invalid destructuring assignment target",
    ),
    (
        "async function f() { (a, ‸await 1) => 1 }",
        "Invalid destructuring assignment target",
    ),
    (
        "async function f() { (a, b = ‸await 1) => 1 }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "function f() { \"use strict\"; async function g(‸eval) {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; async function ‸eval() {}",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "async function f(a = 1) { ‸\"use strict\"; }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "\"use strict\"; async function f(a, ‸a) {}",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "async function f([a, ‸a]) {}",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "async function f(a) { let ‸a }",
        "Identifier 'a' has already been declared",
    ),
    ("x = async function ‸await() {}", "Unexpected reserved word"),
    (
        "x = async function* ‸yield() {}",
        "Unexpected identifier 'yield'",
    ),
    ("x = function* ‸yield() {}", "Unexpected identifier 'yield'"),
    (
        "x = async function* ‸await() {}",
        "Unexpected reserved word",
    ),
    (
        "async function f() { x = async function ‸await() {} }",
        "Unexpected reserved word",
    ),
    (
        "async function* g() { x = function* ‸yield() {} }",
        "Unexpected identifier 'yield'",
    ),
    (
        "\"use strict\"; var ‸st\\u0061tic",
        "Keyword must not contain escaped characters",
    ),
    (
        "\"use strict\"; ‸st\\u0061tic",
        "Keyword must not contain escaped characters",
    ),
    (
        "\"use strict\"; var ‸yi\\u0065ld",
        "Unexpected strict mode reserved word",
    ),
    (
        "\"use strict\"; ‸yi\\u0065ld",
        "Unexpected strict mode reserved word",
    ),
    (
        "\"use strict\"; function* g() { ‸yi\\u0065ld }",
        "Keyword must not contain escaped characters",
    ),
    (
        "\"use strict\"; function* g() { var ‸yi\\u0065ld }",
        "Unexpected strict mode reserved word",
    ),
    (
        "\"use strict\"; function* g() { var ‸yield }",
        "Unexpected strict mode reserved word",
    ),
    (
        "\"use strict\"; function* g() { yield‸: 1 }",
        "Unexpected token ':'",
    ),
    (
        "\"use strict\"; var ‸l\\u0065t",
        "Keyword must not contain escaped characters",
    ),
    (
        "\"use strict\"; ‸l\\u0065t = 1",
        "Keyword must not contain escaped characters",
    ),
    (
        "function* g() { var ‸yi\\u0065ld }",
        "Unexpected identifier 'yield'",
    ),
    (
        "class C { *g() { var ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    ("class C { *g() { yield‸: 1 } }", "Unexpected token ':'"),
    (
        "class C { *g() { var ‸yi\\u0065ld } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "class C { *g() { ‸yi\\u0065ld } }",
        "Keyword must not contain escaped characters",
    ),
    (
        "class C { g() { ‸yi\\u0065ld } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "class C { g() { var ‸yi\\u0065ld } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "class ‸st\\u0061tic {}",
        "Unexpected strict mode reserved word",
    ),
    (
        "class C extends B { constructor() { (‸super)() } }",
        "'super' keyword unexpected here",
    ),
    ("class C extends [] ‸=> {} {}", "Unexpected token '=>'"),
    ("x = class extends [] ‸=> {} {}", "Unexpected token '=>'"),
    (
        "class C { #x; ‸#x }",
        "Identifier '#x' has already been declared",
    ),
    (
        "class C { #x; static ‸#x }",
        "Identifier '#x' has already been declared",
    ),
    ("class C { ‸#\\u0000; }", "Invalid or unexpected token"),
    ("class C { ‸#\\u200D_ZWJ; }", "Invalid or unexpected token"),
    (
        "class C {\n  #x;\n  #x‸;\n}",
        "Identifier '#x' has already been declared",
    ),
    (
        "class C { #x;\n #x‸; }",
        "Identifier '#x' has already been declared",
    ),
    (
        "class C { #x; #x‸; }",
        "Identifier '#x' has already been declared",
    ),
    ("x = ‸\\u0000", "Invalid or unexpected token"),
    ("x = ‸#\\u0000", "Invalid or unexpected token"),
    ("class C extends () ‸=> {} {}", "Unexpected token '=>'"),
    ("class C extends (‸) {}", "Unexpected token ')'"),
    (
        "for (const [‸...x = []] of [[]]) ;",
        "Invalid destructuring assignment target",
    ),
    (
        "async function f() { for await (let ‸x, y of z) ; }",
        "Invalid left-hand side in for-await-of loop: Must have a single binding.",
    ),
    (
        "async function f() { for await ([‸...x = []] of [[]]) ; }",
        "Invalid destructuring assignment target",
    ),
    (
        "for (var [‸...x = []] of [[]]) ;",
        "Invalid destructuring assignment target",
    ),
    (
        "for (let [‸...x, y] of [[]]) ;",
        "Rest element must be last element",
    ),
    (
        "async function f() { for await (var x ‸o\\u0066 []) ; }",
        "Keyword must not contain escaped characters",
    ),
    ("for (var x ‸o\\u0066 []) ;", "Unexpected token 'of'"),
    (
        "x = { ‸\\u0061sync* m(){} }",
        "Keyword must not contain escaped characters",
    ),
    ("x = { g\\u0065t ‸*m(){} }", "Unexpected token '*'"),
    (
        "function* g() { void ‸yi\\u0065ld }",
        "Unexpected identifier 'yield'",
    ),
    (
        "(async (...x = [‸]) => {});",
        "Rest parameter may not have a default initializer",
    ),
    (
        "(async (...x = ‸1) => {});",
        "Rest parameter may not have a default initializer",
    ),
    (
        "(async (a, ...x = [‸]) => {});",
        "Rest parameter may not have a default initializer",
    ),
    (
        "((...‸x = []) => {});",
        "Rest parameter may not have a default initializer",
    ),
    (
        "async function f() { for await (const [x‸] = 1 of []) {} }",
        "for-await-of loop variable declaration may not have an initializer.",
    ),
    (
        "async function f() { for await (const {x‸} = 1 of []) {} }",
        "for-await-of loop variable declaration may not have an initializer.",
    ),
    (
        "async function f() { for await (let [x‸] = 1;;) {} }",
        "for-await-of loop variable declaration may not have an initializer.",
    ),
    (
        "async ‸function () {} = 1;",
        "Function statements require a function name",
    ),
    (
        "function* g() { ‸yi\\u0065ld: 1 }",
        "Keyword must not contain escaped characters",
    ),
    ("class A extends x‸++ {}", "Unexpected token '++'"),
    ("class A extends ‸!x {}", "Unexpected token '!'"),
    ("class A extends ‸typeof x {}", "Unexpected token 'typeof'"),
    ("class A extends a ‸|| b {}", "Unexpected token '||'"),
    ("class A extends a ‸? b : c {}", "Unexpected token '?'"),
    ("class A extends x ‸= y {}", "Unexpected token '='"),
    (
        "class A extends ‸yield {}",
        "Unexpected strict mode reserved word",
    ),
    (
        "function* g() { class A extends ‸yield {} }",
        "Unexpected strict mode reserved word",
    ),
    ("class A extends‸", "Unexpected end of input"),
    ("class A {‸", "Unexpected end of input"),
    ("class A { m() {}‸", "Unexpected end of input"),
    ("class‸", "Unexpected end of input"),
    ("class ‸{}", "Unexpected token '{'"),
    (
        "x = class { #a; m() { return this.#a + ‸#a in this } }",
        "Unexpected identifier '#a'",
    ),
    (
        "x = class { #a; m() { return delete this.‸#a } }",
        "Private fields can not be deleted",
    ),
    (
        "x = class { #a; m() { super.‸#a } }",
        "Unexpected private field",
    ),
    (
        "x = class { #a; m() { this‸.#b } }",
        "Private field '#b' must be declared in an enclosing class",
    ),
    (
        "x = class { #a; m() ‸{ #b in this } }",
        "Private field '#b' must be declared in an enclosing class",
    ),
    (
        "x = class { #a; m() { ‸#a } }",
        "Unexpected identifier '#a'",
    ),
    (
        "x = class { #a; m() { (‸#a) } }",
        "Unexpected identifier '#a'",
    ),
    (
        "x = class { #a; m() { ‸#a.x } }",
        "Unexpected identifier '#a'",
    ),
    (
        "x = class { #a; m() { ({ ‸#a: 1 }) } }",
        "Unexpected identifier '#a'",
    ),
    (
        "x = class { #a; m() { ({ ‸#a() {} }) } }",
        "Unexpected identifier '#a'",
    ),
    (
        "x = class { #a; m() { var ‸#a } }",
        "Unexpected identifier '#a'",
    ),
    (
        "x = class { #a; m() { function ‸#a() {} } }",
        "Unexpected identifier '#a'",
    ),
    (
        "x = class { #a; m() { this?.#a\n‸`x` } }",
        "Invalid tagged template on optional chain",
    ),
    (
        "x = class { #a; m() { this?.#a‸`x` } }",
        "Invalid tagged template on optional chain",
    ),
    ("x = class { ‸# a }", "Invalid or unexpected token"),
    (
        "x = class { m() { class B { #b; } return this‸.#b } }",
        "Private field '#b' must be declared in an enclosing class",
    ),
    (
        "x = class { #a = 1; #a = ‸2 }",
        "Identifier '#a' has already been declared",
    ),
    (
        "x = class { get #a() {} get #a() {‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "x = class { set #a(v) {} set #a(v) {‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "x = class { get #a() {} set #a(v) {} ‸#a }",
        "Identifier '#a' has already been declared",
    ),
    (
        "x = class { static get #a() {} set #a(v) {‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "x = class { #a() {} #a() {‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "x = class { static #a() {} #a() {‸} }",
        "Identifier '#a' has already been declared",
    ),
    (
        "x = class { static #a; static ‸#a }",
        "Identifier '#a' has already been declared",
    ),
    (
        "x = class { constructor() {} ‸'constructor'() {} }",
        "A class may only have one constructor",
    ),
    (
        "x = class { \"constructor\"() {} ‸constructor() {} }",
        "A class may only have one constructor",
    ),
    (
        "x = class { constructor() {} ‸constructor() {} constructor() {} }",
        "A class may only have one constructor",
    ),
    (
        "x = class { get ‸constructor() {} }",
        "Class constructor may not be an accessor",
    ),
    (
        "x = class { set ‸constructor(v) {} }",
        "Class constructor may not be an accessor",
    ),
    (
        "x = class { *‸constructor() {} }",
        "Class constructor may not be a generator",
    ),
    (
        "x = class { async ‸constructor() {} }",
        "Class constructor may not be an async method",
    ),
    (
        "x = class { async *‸constructor() {} }",
        "Class constructor may not be a generator",
    ),
    (
        "x = class { ‸constructor = 1 }",
        "Classes may not have a field named 'constructor'",
    ),
    (
        "x = class { ‸'constructor' }",
        "Classes may not have a field named 'constructor'",
    ),
    (
        "x = class { static ‸constructor }",
        "Classes may not have a field named 'constructor'",
    ),
    (
        "x = class { ‸#constructor }",
        "Classes may not have a field named 'constructor'",
    ),
    (
        "x = class { ‸#constructor() {} }",
        "Class constructor may not be a private method",
    ),
    (
        "x = class { static ‸#constructor() {} }",
        "Class constructor may not be a private method",
    ),
    (
        "x = class { static ‸prototype() {} }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "x = class { static ‸prototype }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "x = class { static ‸'prototype' = 1 }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "x = class { static *‸prototype() {} }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "x = class { static async ‸prototype() {} }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "x = class { static set ‸prototype(v) {} }",
        "Classes may not have a static property named 'prototype'",
    ),
    (
        "x = class { constructor() { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { constructor() { function f() { ‸super() } } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { constructor() { ({ m() { ‸super() } }) } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { constructor() { class C { constructor() { ‸super() } } } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { m() { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { static m() { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { get x() { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { x = ‸super() }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { static { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { [‸super()]() {} }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { constructor() { ‸super } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { constructor() { ‸super?.x } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class extends B { constructor() { new ‸super } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = { m() { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = { m() { function f() { ‸super.x } } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = { m: function () { ‸super.x } }",
        "'super' keyword unexpected here",
    ),
    (
        "x = { m: () => ‸super.x }",
        "'super' keyword unexpected here",
    ),
    ("x = { [‸super.x]: 1 }", "'super' keyword unexpected here"),
    (
        "function f() { ‸super() }",
        "'super' keyword unexpected here",
    ),
    (
        "function f(a = ‸super.x) {}",
        "'super' keyword unexpected here",
    ),
    ("() => ‸super.x", "'super' keyword unexpected here"),
    ("‸super()", "'super' keyword unexpected here"),
    ("‸super", "'super' keyword unexpected here"),
    (
        "x = class { [‸super.x]() {} }",
        "'super' keyword unexpected here",
    ),
    (
        "x = class { x = ‸arguments }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { x = () => ‸arguments }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { x = { ‸arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { x = class { [‸arguments] } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { x = class { y = ‸arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static x = ‸arguments }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { ‸arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { () => ‸arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { var ‸arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { ‸arguments = 1 } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "function f() { class A { x = ‸arguments } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { [new.‸target] = 1 }",
        "new.target expression is not allowed here",
    ),
    (
        "x = class { x = ‸yield }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { static x = ‸await }",
        "Unexpected reserved word",
    ),
    (
        "x = class { x = (‸yield) }",
        "Unexpected strict mode reserved word",
    ),
    (
        "function* g() { x = class { x = ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "async function f() { x = class { x = await ‸1 } }",
        "Unexpected number",
    ),
    (
        "async function f() { x = class { static x = ‸await 1 } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { ‸await } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { ‸await 1 } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { var ‸await } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { let ‸await } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { function ‸await() {} } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { class ‸await {} } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { (class ‸await {}) } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { (‸await) => 1 } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { ‸await: 1 } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { x = { ‸await } } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { static { ‸return } }",
        "Illegal return statement",
    ),
    (
        "x = class { static { ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    ("x = class { static { ‸break } }", "Illegal break statement"),
    (
        "x = class { static { ‸continue } }",
        "Illegal continue statement: no surrounding iteration statement",
    ),
    (
        "l: x = class { static { break ‸l } }",
        "Undefined label 'l'",
    ),
    (
        "for (;;) x = class { static { ‸continue } }",
        "Illegal continue statement: no surrounding iteration statement",
    ),
    (
        "x = class { static { let a; let ‸a } }",
        "Identifier 'a' has already been declared",
    ),
    (
        "x = class { static { let a; var ‸a } }",
        "Identifier 'a' has already been declared",
    ),
    (
        "x = class { static { function a() {} let ‸a } }",
        "Identifier 'a' has already been declared",
    ),
    (
        "x = class { static { { function a() {} ‸function a() {} } } }",
        "Identifier 'a' has already been declared",
    ),
    ("x = class { static‸", "Unexpected end of input"),
    ("{} ‸}", "Unexpected token '}'"),
    ("x = class { static async ‸{} }", "Unexpected token '{'"),
    ("x = class { static get ‸{} }", "Unexpected token '{'"),
    ("x = class { a ‸b }", "Unexpected identifier 'b'"),
    ("x = class { a = 1‸, b = 2 }", "Unexpected token ','"),
    (
        "x = class { m() { var ‸let } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { m() { var ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { m() { var ‸static } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { m() { var ‸eval } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "x = class { m(‸eval) {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "x = class { m(a, ‸a) {} }",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "x = class { m(‸arguments) {} }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "x = class { m() { ‸with (a) {} } }",
        "Strict mode code may not include a with statement",
    ),
    (
        "x = class { m() { ‸010 } }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { m() { '\\‸010' } }",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "x = class extends (function () { ‸with (a) {} }) {}",
        "Strict mode code may not include a with statement",
    ),
    (
        "x = class extends (function () { ‸010 }) {}",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "x = class { m() { delete ‸x } }",
        "Delete of an unqualified identifier in strict mode.",
    ),
    (
        "x = ‸class { a = 1 } = 1",
        "Invalid left-hand side in assignment",
    ),
    ("class A {} ‸= 1", "Unexpected token '='"),
    (
        "x = class { m() { ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { m() { for ‸await (x of y); } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { *m(a = ‸yield) {} }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "x = class { async m(a = ‸await x) {} }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "x = class { async *m(a = ‸yield) {} }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "x = class { async *m(a = ‸await x) {} }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "x = class { m(a = ‸yield) {} }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { async m(‸await) {} }",
        "Unexpected reserved word",
    ),
    (
        "x = class { *m(‸yield) {} }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { async m() { var ‸await } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { *m() { var ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { async *m() { var ‸await } }",
        "Unexpected reserved word",
    ),
    (
        "x = class { async m(a = 1) { ‸\"use strict\"; } }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "x = class { async m(a) { let ‸a } }",
        "Identifier 'a' has already been declared",
    ),
    (
        "x = class { async m(a, ‸a) {} }",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "x = class { get a‸(b) {} }",
        "Getter must not have any formal parameters.",
    ),
    (
        "x = class { set a‸() {} }",
        "Setter must have exactly one formal parameter.",
    ),
    (
        "x = class { set a‸(b, c) {} }",
        "Setter must have exactly one formal parameter.",
    ),
    (
        "x = class { set a‸(...b) {} }",
        "Setter function argument must not be a rest parameter",
    ),
    (
        "x = class { static get a‸(b) {} }",
        "Getter must not have any formal parameters.",
    ),
    (
        "x = class { get #a‸(b) {} }",
        "Getter must not have any formal parameters.",
    ),
    (
        "x = class { set #a‸() {} }",
        "Setter must have exactly one formal parameter.",
    ),
    (
        "x = class { async get ‸a() {} }",
        "Unexpected identifier 'a'",
    ),
    (
        "x = class { async set ‸a(v) {} }",
        "Unexpected identifier 'a'",
    ),
    ("x = class { get ‸*a() {} }", "Unexpected token '*'"),
    ("x = class { m() {}‸, n() {} }", "Unexpected token ','"),
    ("x = class { m() ‸}", "Unexpected token '}'"),
    ("x = class { m() {} ‸= 1 }", "Unexpected token '='"),
    ("x = class { 'a' ‸b }", "Unexpected identifier 'b'"),
    ("x = class { [a] ‸b }", "Unexpected identifier 'b'"),
    ("x = class { #a ‸b }", "Unexpected identifier 'b'"),
    ("async function f() { await‸; }", "Unexpected token ';'"),
    (
        "async function f() { await ‸-1 ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f() { ‸await x ** 2 }",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "async function f() { ++‸await x }",
        "Invalid left-hand side expression in prefix operation",
    ),
    ("async function f() { await ‸= 1 }", "Unexpected token '='"),
    ("async function f() { await‸.x }", "Unexpected token '.'"),
    (
        "async function f() { (async function ‸await() {}) }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { let { ‸await } = x }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { let { a: await ‸} = x }",
        "Unexpected token '}'",
    ),
    (
        "async function f() { [await‸] = x }",
        "Unexpected token ']'",
    ),
    (
        "async function f() { ({ ‸await } = x) }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { for (await of ‸x); }",
        "Unexpected identifier 'x'",
    ),
    (
        "async function f() { for (await ‸in x); }",
        "Unexpected token 'in'",
    ),
    ("async function f() { await‸: ; }", "Unexpected token ':'"),
    (
        "async function f() { break ‸await }",
        "Unexpected reserved word",
    ),
    (
        "async function f(a = ‸await x) {}",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f(a = ‸await) {}",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f([‸await]) {}",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f({ ‸await }) {}",
        "Unexpected reserved word",
    ),
    ("async function f(...‸await) {}", "Unexpected reserved word"),
    (
        "async function f(a = function () { ‸await x }) {}",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "async function f(a = () => await ‸x) {}",
        "Unexpected identifier 'x'",
    ),
    (
        "async function f(a = class { [‸await] = 1 }) {}",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f(a = class { static x = ‸await }) {}",
        "Unexpected reserved word",
    ),
    (
        "function f() { ‸await x }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "function* g() { ‸await x }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "async function* g(‸yield) {}",
        "Unexpected identifier 'yield'",
    ),
    (
        "async function* g() { await ‸yield }",
        "Unexpected identifier 'yield'",
    ),
    (
        "async function* g() { var ‸yield }",
        "Unexpected identifier 'yield'",
    ),
    (
        "async function* g() { var ‸await }",
        "Unexpected reserved word",
    ),
    (
        "async function* g(a = ‸yield) {}",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "async function* g(a = ‸await x) {}",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function* g() { (a = ‸yield) => 1 }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "async function* g() { (a = ‸await x) => 1 }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function* g() { async (a = ‸yield) => 1 }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "x = async\n‸function () {}",
        "Function statements require a function name",
    ),
    (
        "x = async (...a‸,) => a",
        "Rest parameter must be last formal parameter",
    ),
    (
        "x = async (a, ...b‸, c) => a",
        "Rest parameter must be last formal parameter",
    ),
    (
        "x = async (...a = ‸1) => a",
        "Rest parameter may not have a default initializer",
    ),
    (
        "x = async () => () => ‸await x",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "x = async (a) => { let ‸a }",
        "Identifier 'a' has already been declared",
    ),
    (
        "x = async a => { let ‸a }",
        "Identifier 'a' has already been declared",
    ),
    (
        "x = async (a, ‸a) => 1",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "x = async (x = 1) => { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "\"use strict\"; x = async (‸eval) => 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; x = async (‸yield) => 1",
        "Unexpected strict mode reserved word",
    ),
    (
        "\"use strict\"; x = async ‸yield => 1",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = async (‸await) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "x = async ‸await => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "x = async (a = ‸await) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "x = async (a = ‸await 1) => 1",
        "missing ) after argument list",
    ),
    (
        "x = async ([‸await]) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "x = async ({ a: ‸await }) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "x = async (...‸await) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "x = async (a = (‸await) => 1) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "x = async (a = ‸await => 1) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    (
        "x = async (a = class { [‸await] = 1 }) => 1",
        "'await' is not a valid identifier name in an async function",
    ),
    ("x = async (x)\n‸=> 1", "Unexpected token '=>'"),
    ("x = async x\n‸=> 1", "Unexpected token '=>'"),
    ("x = async (x) => {}‸(1)", "Unexpected token '('"),
    ("x = async (x) => {}‸.y", "Unexpected token '.'"),
    ("x = async (x) => {} ‸? 1 : 2", "Unexpected token '?'"),
    (
        "x = ‸!async () => 1",
        "Malformed arrow function parameter list",
    ),
    (
        "x = ‸new async () => 1",
        "Malformed arrow function parameter list",
    ),
    (
        "x = async ({ ‸a = 1 })",
        "Invalid shorthand property initializer",
    ),
    (
        "x = async ({ ‸a = 1 }, b)",
        "Invalid shorthand property initializer",
    ),
    (
        "x = async (b, { ‸a = 1 })",
        "Invalid shorthand property initializer",
    ),
    (
        "x = [async ({ ‸a = 1 })]",
        "Invalid shorthand property initializer",
    ),
    (
        "x = async ({ ‸a = 1 }).x",
        "Invalid shorthand property initializer",
    ),
    (
        "function* g() { x = async (a = ‸yield) => 1 }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "function* g() { x = async (‸yield) => 1 }",
        "Invalid destructuring assignment target",
    ),
    (
        "function* g() { x = async ‸yield => 1 }",
        "Unexpected identifier 'yield'",
    ),
    (
        "async function f() { x = async (a = ‸await x) => 1 }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f() { x = async (await‸) => 1 }",
        "Unexpected token ')'",
    ),
    (
        "async function f() { x = async ‸await => 1 }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { x = (a = ‸await x) => 1 }",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async function f() { x = (await‸) => 1 }",
        "Unexpected token ')'",
    ),
    (
        "async function f() { x = await ‸=> 1 }",
        "Unexpected token '=>'",
    ),
    (
        "async function f() { x = () => { ‸await x } }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "for (‸async of => {} of x);",
        "Invalid left-hand side in for-loop",
    ),
    (
        "for (‸async of x);",
        "The left-hand side of a for-of loop may not be 'async'.",
    ),
    (
        "for (‸async\nof x);",
        "The left-hand side of a for-of loop may not be 'async'.",
    ),
    (
        "async function f() { for await (let of ‸x); }",
        "Unexpected identifier 'x'",
    ),
    (
        "async function f() { for await (‸let.x of y); }",
        "The left-hand side of a for-of loop may not start with 'let'.",
    ),
    (
        "async function f() { for await (var x ‸in y); }",
        "Unexpected token 'in'",
    ),
    (
        "async function f() { for await (x‸;;); }",
        "Unexpected token ';'",
    ),
    (
        "async function f() { for await (‸;;); }",
        "Unexpected token ';'",
    ),
    (
        "async function f() { for await (x of y‸, z); }",
        "Unexpected token ','",
    ),
    (
        "async function f() { for await (var ‸x, y of z); }",
        "Invalid left-hand side in for-await-of loop: Must have a single binding.",
    ),
    (
        "async function f() { for await (let ‸x = 1 of y); }",
        "for-await-of loop variable declaration may not have an initializer.",
    ),
    (
        "async function f() { for await (var ‸x = 1 of y); }",
        "for-await-of loop variable declaration may not have an initializer.",
    ),
    (
        "async function f() { for await (x ‸= 1 of y); }",
        "Unexpected token '='",
    ),
    (
        "async function f() { for await (‸1 of y); }",
        "Invalid left-hand side in for-loop",
    ),
    (
        "async function f() { for await (x of y) ‸let z; }",
        "Lexical declaration cannot appear in a single-statement context",
    ),
    (
        "async function f() { for await (x of y) ‸function g() {} }",
        "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.",
    ),
    (
        "async function f() { for await (let x of y) { var ‸x } }",
        "Identifier 'x' has already been declared",
    ),
    (
        "async function f() { for await ‸x of y; }",
        "Unexpected identifier 'x'",
    ),
    (
        "async function f() { for await (x of y) ‸}",
        "Unexpected token '}'",
    ),
    (
        "async function f() { () => { for ‸await (x of y); } }",
        "Unexpected reserved word",
    ),
    (
        "async function f() { function g() { for ‸await (x of y); } }",
        "Unexpected reserved word",
    ),
    (
        "x = { async m(a = ‸await x) {} }",
        "Illegal await-expression in formal parameters of async function",
    ),
    ("x = { async m(‸await) {} }", "Unexpected reserved word"),
    ("x = { async\n‸m() {} }", "Unexpected identifier 'm'"),
    ("x = { async *‸() {} }", "Unexpected token '('"),
    ("x = { async m ‸}", "Unexpected token '}'"),
    ("x = { async m‸: 1 }", "Unexpected token ':'"),
    ("x = { async get ‸m() {} }", "Unexpected identifier 'm'"),
    (
        "x = { async m() { ‸super() } }",
        "'super' keyword unexpected here",
    ),
    (
        "if (1) {} else ‸async function f() {}",
        "Async functions can only be declared at the top level or inside a block.",
    ),
    (
        "while (0) ‸async function f() {}",
        "Async functions can only be declared at the top level or inside a block.",
    ),
    (
        "l: l2: ‸async function f() {}",
        "Async functions can only be declared at the top level or inside a block.",
    ),
    (
        "{ async function f() {} var ‸f }",
        "Identifier 'f' has already been declared",
    ),
    (
        "{ function* f() {} ‸function* f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "{ function f() {} ‸function* f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "\"use strict\"; { function f() {} ‸function f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "switch (1) { case 1: async function f() {} case 2: ‸async function f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "let f; ‸async function f() {}",
        "Identifier 'f' has already been declared",
    ),
    (
        "async function f() {} let ‸f",
        "Identifier 'f' has already been declared",
    ),
    (
        "async function f() { async function g() {} let ‸g }",
        "Identifier 'g' has already been declared",
    ),
    ("x = { async\n‸*m() {} }", "Unexpected token '*'"),
    ("x = class { a = 1\n‸++b }", "Unexpected token '++'"),
    ("x = class { *\n‸() {} }", "Unexpected token '('"),
    (
        "function* g() { class C { m(a = ‸yield) {} } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "function* g() { class C { *m(a = ‸yield) {} } }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "async function f() { class C { [await 1] = await ‸2 } }",
        "Unexpected number",
    ),
    (
        "x = class { [class { m() { return this‸.#z } }]() {} }",
        "Private field '#z' must be declared in an enclosing class",
    ),
    ("new.‸target", "new.target expression is not allowed here"),
    (
        "x = () => new.‸target",
        "new.target expression is not allowed here",
    ),
    ("label: ‸class C {}", "Unexpected token 'class'"),
    ("do ‸class C {} while (0)", "Unexpected token 'class'"),
    ("while (0) ‸class C {}", "Unexpected token 'class'"),
    ("for (;;) ‸class C {}", "Unexpected token 'class'"),
    ("for (x of y) ‸class C {}", "Unexpected token 'class'"),
    ("with (o) ‸class C {}", "Unexpected token 'class'"),
    (
        "switch (1) { case 1: class C {} case 2: ‸class C {} }",
        "Identifier 'C' has already been declared",
    ),
    (
        "function f(C) { ‸class C {} }",
        "Identifier 'C' has already been declared",
    ),
    (
        "catch_test: try {} catch (C) { ‸class C {} }",
        "Identifier 'C' has already been declared",
    ),
    (
        "x = class { static { let x; { var ‸x } } }",
        "Identifier 'x' has already been declared",
    ),
    (
        "x = class { static { label: ‸label: ; } }",
        "Label 'label' has already been declared",
    ),
    (
        "x = class { static { label: { ‸label: ; } } }",
        "Label 'label' has already been declared",
    ),
    (
        "x = class { static { if (1) ‸function f() {} } }",
        "In strict mode code, functions can only be declared at top level or inside a block.",
    ),
    (
        "x = class { static { ‸with (o) {} } }",
        "Strict mode code may not include a with statement",
    ),
    (
        "x = class { static { var ‸yield } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { static { var ‸let } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { static { var ‸static } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = class { static { var ‸eval } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "x = class { static { ‸eval = 1 } }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "x = class { static { ‸arguments.length } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { ({ ‸arguments }) } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { (‸arguments) => 1 } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { ‸arguments => 1 } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { async ‸arguments => 1 } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
    (
        "x = class { static { async (‸arguments) => 1 } }",
        "'arguments' is not allowed in class field initializer or static initialization block",
    ),
];

const CLASS_ASYNC_VALID: &[&str] = &[
    "async function f(){ class C { [await 1] = 1 } }",
    "class C { x = await }",
    "class C { static { () => await } }",
    "class C { static { function f() { arguments } } }",
    "class C { prototype = 1 }",
    "class C { get #a(){} set #a(v){} }",
    "class C extends B { constructor() { super() } }",
    "class C extends B { constructor() { () => super() } }",
    "({ m() { super.x } })",
    "class C { x = super.x }",
    "class C { static { super.x } }",
    "async function f() { for await (x of y); }",
    "async function f() { for await (async of y); }",
    "async () => await 1",
    "async (a, b) => 1",
    "async(a, b)",
    "async\nfunction f(){}",
    "async a => 1",
    "async function await() {}",
    "function f() { var await }",
    "async function f() { function g(await) {} }",
    "var yield",
    "async function* g() { yield await 1 }",
    "x = async function*(){}",
    "({ async *m() {} })",
    "({ async m() { await 1 } })",
    "class C { async\nm() {} }",
    "class C { static async *#m() {} }",
    "class C { #x; static m(o) { return #x in o } }",
    "class C { #x; m() { this?.#x } }",
    "class C extends (a, b) {}",
    "class C extends a.b {}",
    "class C extends a() {}",
    "class await {}",
    "class C { static async *[Symbol.iterator]() {} }",
    "class C { static static() {} }",
    "class C { static get static() {} }",
    "class C { get; set; static; async }",
    "class C { get\na(){} }",
    "class C { static\na(){} }",
    "class C { async\na(){} }",
    "class C { a\nb }",
    "class C { ['constructor'](){} ['constructor'](){} }",
    "class C { static constructor(){} static constructor(){} }",
    "new class {}",
    "class C { x = this }",
    "class C { x = new.target }",
    "class C { static { new.target } }",
    "let C = class C2 extends C2 {}",
    "async function f() { () => await }",
    "async function f() { () => { var await } }",
    "async function f() { function g() { var await } }",
    "async function f() { (x = await 1) }",
    "async function f() { await x }",
    "await: 1",
    "async function f() { x = { await: 1 } }",
    "async function f() { x.await }",
    "function* g() { yield }",
    "async function f() {}",
    "x = async function f() {}",
    "x = async () => 1",
    "x = async (a)",
    "async function f() { class C { [await x]() {} } }",
    "x = async () => 1, async () => 2",
    "x = async (a, ...b) => 1",
    "x = async (a,) => 1",
    "x = async (a,)",
    "x = async ()",
    "x = async (...a)",
    "x = async (...a) + 1",
    "async (a = 1, [b], {c}) => 1",
    "async ([a] = b) => 1",
    "async (eval) => 1",
    "async (x) => { \"use strict\" }",
    "async (yield) => 1",
    "async yield => 1",
    "async function* g() { yield* x }",
    "async function f() { await await 1 }",
    "async function f() { await -1 }",
    "async function f() { -await 1 }",
    "async function f() { typeof await 1 }",
    "async function f() { for await (x of y) {} for await (let x of y) {} for await (const [a] of y) {} }",
    "async function f() { for await (var x of y) {} }",
    "async function f() { for await (x.y of z) {} }",
    "async function f() { for\nawait (x of y) {} }",
    "async function f() { for await\n(x of y) {} }",
    "async function f() { for await (async of x) {} }",
    "function f() { for (async of => 1;;) {} }",
    "async function f() { label: for await (x of y) continue label }",
    "function f() { var aw\\u0061it }",
    "x = \\u0061sync (a)",
    "class C { static ['prototype'](){} }",
    "class C { m() { new super.x } }",
    "class C extends B { constructor() { super.x() } }",
    "class C extends B { constructor(x = super()) {} }",
    "class C extends B { constructor() { class D extends E { constructor() { super() } } } }",
    "class C { #x; m() { delete this.#x.y } }",
    "class C { #x; m() { this.#x = 1; this.#x++; [this.#x] = [1]; ({a: this.#x} = {}); for (this.#x of []); } }",
    "class C { #x; m() { return #x in this in {} } }",
    "class C { #x; m() { return (#x in this) } }",
    "class C { #x; m() { return #x in this < 1 } }",
    "class C { #x; static { class D extends (o => o.#x) {} } }",
    "class C { #x; [this.#x] = 1 }",
    "class C { static { async function f() { await 1 } } }",
    "class C { static { var x; var x; let y; } }",
    "class C { static { x: break x } }",
    "function* g() { class C { [yield] = 1 } }",
    "class C { m() {} m() {} }",
    "class C { static m() {} m() {} }",
    "class C { get a() {} set a(v) {} }",
    "class C { static async\n*m() {} }",
    "class C { async\n*m() {} }",
    "class C { get\n*m(){} }",
    "class C { static\n*m(){} }",
    "class C { a; b = 1; [c]; 'd'; 1; #e = 2; static f; static #g = 1 }",
    "class C { x\ny }",
    "class C { x = 1;; }",
    "class C { static\nx }",
    "class C { static x\ny }",
    "class C { \"use strict\"; }",
    "class C { constructor() { \"use strict\" } }",
    "class C extends B { constructor() { super.x; super[x]; super(); } }",
    "x = class extends B {}",
    "x = class C extends C {}",
    "class C { m() {} } class D extends C { static m() { return super.m() } }",
    "x = class { static name() {} }",
    "x = { async *[Symbol.asyncIterator]() {} }",
    "x = { async get() {}, async set() {}, async async() {}, async static() {} }",
    "x = { async 'a'() {}, async 1() {}, async [a]() {} }",
    "async function f() { await new Promise(r => r()) }",
    "async function f() { return await f() }",
    "async () => { for await (const x of y) {} }",
    "async function* f() { for await (const x of y) yield x }",
    "async function f() { label: await 1 }",
    "async function f(a = () => await) {}",
    "async (a = () => await) => 1",
    "async (a = async () => await 1) => 1",
    "async (x);",
    "async ()",
    "async (a = 1, ...b) => 1",
    "async (x) => 1 ? 2 : 3",
    "async (x) => {}\n(1)",
    "(async (x) => {})(1)",
    "async\n(x)",
    "x = (async)",
    "async.x",
    "async = 1",
    "async++",
    "async[0]",
    "var async",
    "async: 1",
    "function async() {}",
    "async function* f() {}",
    "x = async function*() { yield* x; await y; for await (z of w); }",
    "({ async* m() {} })",
    "class C { async* m() {} }",
    "async function f() { \"use strict\"; }",
    "async function f(a, a) {}",
    "function* g() { x = async function yield() {} }",
    "async function f() { x = function await() {} }",
    "class C { [(a) => {}] }",
    "class C { #\\u0078 }",
    "class C { #x\\u200c }",
    "class C { #\u{2118} }",
    "class A {}",
    "class A extends B {}",
    "class A extends B.c {}",
    "class A extends B[c] {}",
    "class A extends f() {}",
    "class A extends new B {}",
    "class A extends new B() {}",
    "class A extends (B, C) {}",
    "class A extends B`x` {}",
    "class A extends B?.c {}",
    "class A extends class {} {}",
    "class A extends function () {} {}",
    "class A extends null {}",
    "class A extends this {}",
    "class A extends 1 {}",
    "class A extends \"x\" {}",
    "class A extends [] {}",
    "class A extends {} {}",
    "function* g() { class A extends (yield) {} }",
    "async function f() { class A extends (await x) {} }",
    "(class {})",
    "(class A {})",
    "(class extends B {})",
    "(class A extends B {})",
    "x = class A { m() { A = 1 } }",
    "class A { m() { A = 1 } }",
    "x = class { [x]() {} [y] = 1; static [z] }",
    "x = class { 'a'() {} \"b\" = 1; 1() {} 2 = 3; 1n() {} 2n = 3 }",
    "x = class { get [a]() {} set [a](v) {} static get [b]() {} static set [b](v) {} }",
    "x = class { *[a]() {} async [b]() {} async *[c]() {} static *[d]() {} static async [e]() {} static async *[f]() {} }",
    "x = class { #a() {} get #b() {} set #b(v) {} *#c() {} async #d() {} async *#e() {} }",
    "x = class { static #a() {} static get #b() {} static set #b(v) {} static *#c() {} static async #d() {} static async *#e() {} }",
    "x = class { #a = 1; static #b = 2; #c; static #d }",
    "x = class { #a; static m(o) { o.#a = 1; o.#a += 1; o.#a++; --o.#a; [o.#a] = [1]; ({ x: o.#a } = {}); for (o.#a of []); for (o.#a in {}); } }",
    "x = class { #a; m() { return this.#a() + this?.#a + this.b.#a + this?.b.#a + this.#a`x` } }",
    "x = class { #a; m() { delete this.#a.b; delete this.#a[0]; delete this.#a() } }",
    "x = class { #a; m() { return typeof this.#a } }",
    "x = class { #a; m() { new this.#a } }",
    "x = class { #a; m() { new this.#a() } }",
    "x = class { #a; m() { this.#a\n = 1 } }",
    "x = class { #a; m() { this\n.#a } }",
    "x = class { #a; m() { this.\n#a } }",
    "x = class { #a; m() { this?.\n#a } }",
    "x = class { #a = #a in this }",
    "x = class { #a = () => this.#a }",
    "x = class { [this.#a] = 1; #a }",
    "x = class extends (class { #a; m() { return this.#a } }) { #a }",
    "x = class { #a; m() { class B { #b; n() { return this.#a + this.#b } } } }",
    "x = class { static { this.#a } #a }",
    "x = class { #a; static { this.#a } }",
    "x = class { constructor() {} }",
    "x = class { constructor() {} ['constructor']() {} }",
    "x = class { constructor() {} static constructor() {} }",
    "x = class { static get constructor() {} }",
    "x = class { static *constructor() {} }",
    "x = class { static async constructor() {} }",
    "x = class { ['constructor'] = 1 }",
    "x = class { static ['prototype'] = 1 }",
    "x = class { prototype() {} }",
    "x = class { get prototype() {} }",
    "x = class { static #prototype }",
    "x = class extends B { constructor() { super() } }",
    "x = class extends B { constructor() { super(); super() } }",
    "x = class extends B { constructor() { super(...a, b) } }",
    "x = class extends B { constructor() { super.x() } }",
    "x = class extends B { constructor() { super[x] = 1 } }",
    "x = class extends B { constructor() { () => super() } }",
    "x = class extends B { constructor() { () => () => super() } }",
    "x = class extends B { constructor() { async () => super() } }",
    "x = class extends B { constructor() { class C extends D { constructor() { super() } } } }",
    "x = class extends B { constructor(a = super()) {} }",
    "x = class extends B { constructor(a = () => super()) {} }",
    "x = class extends B { constructor() { super.x } }",
    "x = class extends B { constructor() { super\n() } }",
    "x = class extends B { constructor() { new super.x } }",
    "x = class extends B { constructor() { new super.x() } }",
    "x = class extends B { constructor() { super.x`y` } }",
    "x = class extends B { constructor() { super.x = 1; super[0] = 1; super.x++; [super.x] = [1]; ({ a: super[0] } = {}) } }",
    "x = class extends B { constructor() { delete super.x } }",
    "x = class extends B { constructor() { delete super[x] } }",
    "x = class extends B { constructor() { typeof super.x } }",
    "x = { m() { super.x } }",
    "x = { m() { super[x] } }",
    "x = { get x() { return super.x } }",
    "x = { set x(v) { super.x = v } }",
    "x = { *g() { yield super.x } }",
    "x = { async m() { await super.x } }",
    "x = { m() { () => super.x } }",
    "x = { m(a = super.x) {} }",
    "x = class { x = super.x; static y = super.y; static { super.z } }",
    "x = { m() { class A { [super.x]() {} } } }",
    "x = { m() { class A extends super.x {} } }",
    "x = class { m() { class A extends super.x {} } }",
    "x = class { x = function () { arguments } }",
    "x = class { x = a.arguments }",
    "x = class { x = { arguments: 1 } }",
    "x = class { x = class { m() { arguments } } }",
    "x = class { static { function f() { arguments } } }",
    "x = class { [arguments] = 1 }",
    "function f() { class A { [arguments] = 1 } }",
    "x = class { x = eval('1') }",
    "x = class { x = this }",
    "x = class { x = new.target }",
    "x = class { static x = new.target }",
    "x = class { static { new.target } }",
    "x = class { x = () => new.target }",
    "function f() { class A { [new.target] = 1 } }",
    "x = class { x = await }",
    "function* g() { x = class { [yield] = 1 } }",
    "function* g() { x = class { [yield]() {} } }",
    "function* g() { x = class extends (yield) {} }",
    "async function f() { x = class { x = await } }",
    "async function f() { x = class { [await 1] = 1 } }",
    "async function f() { x = class extends (await 1) {} }",
    "x = class { static { (function await() {}) } }",
    "x = class { static { () => await } }",
    "x = class { static { () => { await } } }",
    "x = class { static { async function f() { await 1 } } }",
    "x = class { static { l: break l } }",
    "x = class { static { l: { break l } } }",
    "x = class { static { var a; let b; const c = 1; function d() {} class E {} } }",
    "x = class { static { var a; var a } }",
    "x = class { static { var a } static { let a } }",
    "x = class { static { function a() {} function a() {} } }",
    "x = class { static { \"use strict\" } }",
    "x = class { static {} static {} }",
    "x = class { static { this; super.x; new.target } }",
    "x = class { static\n{} }",
    "x = class { a\n= 1 }",
    "x = class { a\nb }",
    "x = class { a = 1\nb = 2 }",
    "x = class { a = 1; b = 2; }",
    "x = class { get }",
    "x = class { set; }",
    "x = class { static }",
    "x = class { async }",
    "x = class { get = 1 }",
    "x = class { async = 1 }",
    "x = class { static = 1 }",
    "x = class { get() {} set() {} static() {} async() {} }",
    "x = class { static get() {} static set() {} static static() {} static async() {} }",
    "x = class { get get() {} set set(v) {} async async() {} static static() {} }",
    "x = class { 'use strict' }",
    "x = class { yield = 1 }",
    "x = class { await = 1 }",
    "x = class { let = 1 }",
    "x = class { if = 1 }",
    "x = class { if() {} }",
    "x = class { get if() {} }",
    "x = class { #if; #yield; #await; #let }",
    "x = class C { m() { C = 1 } }",
    "x = class {}.x",
    "x = class {}()",
    "new class {}()",
    "new class extends B {}",
    "x = class { m() {} }\n.x",
    "x = class { m() {} }\n[0]",
    "x = class { m() { return } }",
    "x = class { static m() { return } }",
    "x = class { x = (() => { return 1 })() }",
    "x = class { *m() { yield } }",
    "x = class { *m() { yield* x } }",
    "x = class { async m() { await x } }",
    "x = class { async *m() { yield await x } }",
    "x = class { async *m() { for await (x of y); } }",
    "x = class { async m() { for await (x of y); } }",
    "x = class { m(a = await) {} }",
    "x = class { async m() { \"use strict\"; } }",
    "x = class { get a() {} }",
    "x = class { set a(b = 1) {} }",
    "x = class { set a([b]) {} }",
    "x = class { async\na() {} }",
    "x = class { async *\na() {} }",
    "x = class { async\n*a() {} }",
    "x = class { static async\na() {} }",
    "x = class { get\na() {} }",
    "x = class { set\na(v) {} }",
    "x = class { static\na() {} }",
    "x = class { static\n*a() {} }",
    "x = class { static\nasync a() {} }",
    "x = class { static async *\n#a() {} }",
    "x = class { *\na() {} }",
    "x = class { *\n#a() {} }",
    "x = class { ;;; }",
    "x = class { ; m() {} ; }",
    "x = class { m() {}; n() {} }",
    "x = class { m() {} n() {} }",
    "x = class { m }",
    "x = class { [a]\nb }",
    "x = class { #a\nb }",
    "async function f() { await await x }",
    "async function f() { await (x) }",
    "async function f() { (await x).y }",
    "async function f() { await x.y }",
    "async function f() { await x() }",
    "async function f() { await new X() }",
    "async function f() { await f()() }",
    "async function f() { await x ? y : z }",
    "async function f() { await x || y }",
    "async function f() { x = await y }",
    "async function f() { x += await y }",
    "async function f() { [await x] }",
    "async function f() { ({ a: await x }) }",
    "async function f() { `${await x}` }",
    "async function f() { f(await x) }",
    "async function f() { await\nx }",
    "async function f() { await /x/ }",
    "async function f() { await +x }",
    "async function f() { (await x) ** 2 }",
    "async function f() { delete await x }",
    "async function f() { void await x }",
    "async function f() { await x++ }",
    "async function f() { await ++x }",
    "async function f() { await[0] }",
    "async function f() { await(1) }",
    "async function f() { x = { await() {} } }",
    "async function f() { (function await() {}) }",
    "async function f() { label: { break label } }",
    "async function f(a = function () { await }) {}",
    "async function f(a = async function () { await x }) {}",
    "async function f(a = async () => await x) {}",
    "async function f(a = class { x = await }) {}",
    "function f() { await }",
    "function f() { var await; await = 1; await: ; }",
    "function f(await) {}",
    "function* g(await) {}",
    "async function* g() { yield }",
    "async function* g() { yield await x }",
    "async function* g() { for await (x of y) yield x }",
    "x = async function () {}",
    "x = async function* () {}",
    "x = async function\n() {}",
    "async\nfunction f() {}",
    "x = async () => {}",
    "x = async (a) => a",
    "x = async a => a",
    "x = async (a, b) => a",
    "x = async ([a]) => a",
    "x = async ({ a }) => a",
    "x = async (a = 1) => a",
    "x = async (...a) => a",
    "x = async (a, ...b) => a",
    "x = async (a,) => a",
    "x = async ([a], {b}, c = 1, ...d) => a",
    "x = async () => await x",
    "x = async () => { await x }",
    "x = async () => async () => await x",
    "x = async (eval) => 1",
    "x = async (arguments) => 1",
    "x = async eval => 1",
    "x = async x => { \"use strict\" }",
    "x = async (yield) => 1",
    "x = async yield => 1",
    "x = async (a = () => await) => 1",
    "x = async (a = function () { await }) => 1",
    "x = async\nx => 1",
    "x = async (x) => {}\n(1)",
    "x = async (x) => x, y",
    "x = a ? async () => 1 : async () => 2",
    "x = (async () => 1)()",
    "x = async () => 1 ()",
    "x = async ``",
    "x = async\n``",
    "x = async (a) + 1",
    "x = async (a, b)",
    "x = async (...a, b)",
    "x = async ({ a = 1 }) => 1",
    "x = async ([a] = 1)",
    "x = async (a = yield) => 1",
    "function* g() { x = async (a) => yield }",
    "function* g() { x = async function* () { yield } }",
    "async function f() { x = () => await }",
    "for (async of => {};;);",
    "for (async in x);",
    "for (async.x of y);",
    "for ((async) of x);",
    "async function f() { for await (async of x); }",
    "async function f() { for await (var x of y); }",
    "async function f() { for await (let x of y); }",
    "async function f() { for await (const x of y); }",
    "async function f() { for await (let [a, b] of y); }",
    "async function f() { for await (const { a } of y); }",
    "async function f() { for await ([a, b] of y); }",
    "async function f() { for await ({ a } of y); }",
    "async function f() { for await (x.y of z); }",
    "async function f() { for await (x[0] of z); }",
    "async function f() { for await (f() of z); }",
    "async function f() { for await (let[a] of y); }",
    "async function f() { for await (let x of x); }",
    "async function f() { for\nawait (x of y); }",
    "async function f() { for await\n(x of y); }",
    "async () => { for await (x of y); }",
    "x = { async m() {} }",
    "x = { async *m() {} }",
    "x = { async 'm'() {} }",
    "x = { async [m]() {} }",
    "x = { async 1() {} }",
    "x = { async get() {} }",
    "x = { async m() { await x } }",
    "x = { async *m() { yield await x } }",
    "x = { async: 1 }",
    "x = { async }",
    "x = { async = 1 } = {}",
    "x = { async() {} }",
    "x = { get async() {} }",
    "x = { async async() {} }",
    "x = { async super() {} }",
    "x = { async m() { super.x } }",
    "{ async function f() {} }",
    "{ function f() {} function f() {} }",
    "async function f() {} async function f() {}",
    "async function f() {} var f",
    "var f; async function f() {}",
    "function f() { async function g() {} var g }",
    "var async; async\n(1)",
    "var async; async\nx => 1",
    "var async; async\n[0]",
    "var async; x = async\n? 1 : 2",
    "async\n++x",
    "async function f() {}\n(1)",
    "x = async function () {}\n(1)",
    "let async = 1; async = 2",
    "async: async\nfunction f() {}",
    "x = class { async\n*m() {} }",
    "x = class { async\nget m() {} }",
    "x = class { static async *m() {} }",
    "x = class { static async\n*m() {} }",
    "x = class { 'async'() {} }",
    "x = class { async() {} }",
    "x = class { async = 1; get = 2; set = 3; static = 4 }",
    "x = class { static async = 1 }",
    "x = class { static get = 1 }",
    "x = class { static get\na() {} }",
    "x = class { async *\n[a]() {} }",
    "x = class { get [a]\n() {} }",
    "x = class { m\n() {} }",
    "x = class { m(\n) {} }",
    "x = class { ['a']\n= 1 }",
    "x = class { a\n['b'] }",
    "x = class { a = 1\n['b'] }",
    "x = class { a\n*b() {} }",
    "x = class { a\n#b }",
    "x = class { a\nin }",
    "x = class { static\nstatic }",
    "x = class { static\n= 1 }",
    "x = class { static\n() {} }",
    "x = class { get\n() {} }",
    "x = class { async\n() {} }",
    "x = class { #a\n() {} }",
    "x = class { get\n#a() {} }",
    "x = class { static #a\n() {} }",
    "function* g() { class C { [yield 1]() {} } }",
    "function* g() { class C extends (yield 1) {} }",
    "function* g() { class C { *m() { yield } } }",
    "async function* g() { class C { [yield await 1]() {} } }",
    "x = class { x = () => this; y = function () { return this } }",
    "x = class { x = async () => await 1 }",
    "x = class { x = function* () { yield 1 } }",
    "x = class { static x = async () => await 1 }",
    "x = class { #x = 1; static check(o) { return #x in o } }",
    "x = class { m() { return class { #x; n() { return this.#x } } } }",
    "x = class { #x; m() { return class { n() { return this.#x } } } }",
    "x = class { m() { return class { n() { return this.#x } } } #x }",
    "x = class { #x; m() { return class extends this.#x {} } }",
    "x = class { #x; m() { return class { [this.#x]() {} } } }",
    "x = class { static { class D { #y; static m(o) { return o.#y } } } }",
    "x = class { [class { #z; m() { return this.#z } }]() {} }",
    "x = class { #z; [class { m() { return this.#z } }]() {} }",
    "x = class { #z; static m() { return eval('this.#z') } }",
    "x = class { #z; m() { return (0, eval)('this.#z') } }",
    "function f() { class C { x = new.target } }",
    "function f() { class C { static { new.target } } }",
    "x = { m() { new.target } }",
    "x = class { m() { new.target } }",
    "switch (1) { case 1: class C {} }",
    "try { class C {} } catch { class C {} }",
    "{ class C {} } { class C {} }",
    "class C {} { class C {} }",
    "{ class C {} } class C {}",
    "function f() { class C {} } class C {}",
    "class C {} let D = C",
    "let C; { class C {} }",
    "function f(C) { { class C {} } }",
    "x = function C() { class C {} }",
    "x = class C { constructor() { var C } }",
    "x = class C { constructor() { let C } }",
    "x = class C { [C] = 1 }",
    "x = class C { static C = C }",
    "class C { static { var C } }",
    "class C { static { let C } }",
    "class C { static { class C {} } }",
    "try {} catch (C) { { class C {} } }",
    "x = class { static { var x; { var x } } }",
    "x = class { static { { let x } var x } }",
    "label: x = class { static { label: ; } }",
    "x = class { static { function f() { return } } }",
    "x = class { static { () => { return } } }",
    "x = class { static { class D { m() { return } } } }",
    "x = class { static { this.x = 1 } }",
    "x = class { static { let x = 1; x = 2 } }",
    "x = class { static { const x = 1; x = 2 } }",
    "x = class { static { \"use strict\"; } }",
    "x = class { static { debugger } }",
    "x = class { static { ({ arguments: 1 }) } }",
    "x = class { static { ({ arguments() {} }) } }",
    "x = class { static { x.arguments } }",
];
