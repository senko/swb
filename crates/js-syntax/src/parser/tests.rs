//! Tests of the parser: the tree of each construct of the subset, the
//! early errors with V8's messages (measured in Node.js 22), automatic
//! semicolon insertion, and hostile inputs.

use std::fmt::Write;

use swb_js_text::{RecursionBudget, Str16};

use crate::dump::dump_ast;
use crate::error::{ErrorKind, ParseError};
use crate::parser::{Script, parse_script};

/// Parses `source` in both code-unit widths (the narrow one if the source
/// fits) and checks that both give the same tree or error.
fn parse(source: &str) -> Result<Script, ParseError> {
    let wide: Vec<u16> = source.encode_utf16().collect();
    let mut budget = RecursionBudget::DEFAULT;
    let result = parse_script(Str16::Wide(&wide), &mut budget);
    assert_eq!(
        budget,
        RecursionBudget::DEFAULT,
        "the parse gives the budget back"
    );
    if wide.iter().all(|&u| u < 256) {
        let narrow: Vec<u8> = wide.iter().map(|&u| u as u8).collect();
        let narrow_result = parse_script(Str16::Latin1(&narrow), &mut budget);
        match (&result, &narrow_result) {
            (Ok(a), Ok(b)) => assert_eq!(dump_ast(a), dump_ast(b), "{source}"),
            (Err(a), Err(b)) => assert_eq!(a, b, "{source}"),
            _ => panic!("the widths disagree on {source}"),
        }
    }
    result
}

/// The tree dump of a valid source.
fn ast(source: &str) -> String {
    match parse(source) {
        Ok(script) => dump_ast(&script),
        Err(error) => panic!("{source}: {error}"),
    }
}

/// The message of the error of an invalid source.
fn error(source: &str) -> String {
    match parse(source) {
        Ok(script) => panic!("{source}: no error, parsed as {}", dump_ast(&script)),
        Err(error) => error.message.into_owned(),
    }
}

fn check_ast(cases: &[(&str, &str)]) {
    for &(source, expected) in cases {
        assert_eq!(ast(source), expected, "source: {source}");
    }
}

fn check_errors(cases: &[(&str, &str)]) {
    for &(source, expected) in cases {
        assert_eq!(error(source), expected, "source: {source}");
    }
}

#[test]
fn literals_and_primary_expressions() {
    check_ast(&[
        ("x", "x"),
        ("this", "this"),
        ("null; true; false", "null true false"),
        ("1; 1.5; 0x10; .5", "1 1.5 16 0.5"),
        ("'a'; \"b\\n\"", "\"a\" \"b\\n\""),
        ("`a`", "(` \"a\")"),
        ("`a${b}c${d}e`", "(` \"a\" b \"c\" d \"e\")"),
        ("`${a}`", "(` \"\" a \"\")"),
        ("`a${`b${c}`}`", "(` \"a\" (` \"b\" c \"\") \"\")"),
        ("/a[/]b/gi", "/a[/]b/gi"),
        ("x = /=/", "(= x /=/)"),
        ("[]; [a]; [a, b,]", "(array) (array a) (array a b)"),
        (
            "[,]; [a, , b]; [a,,]",
            "(array hole) (array a hole b) (array a hole)",
        ),
        ("(a)", "(paren a)"),
        ("(a, b)", "(paren (, a b))"),
    ]);
}

#[test]
fn object_literals() {
    check_ast(&[
        ("({})", "(paren (object))"),
        (
            "({ a: 1, 'b': 2, 3: c, [d]: 4, e, f() {}, *g() {}, })",
            "(paren (object (init a 1) (init b 2) (init 3 c) (init [d] 4) (shorthand e) \
             (method f (method ())) (method g (method* ()))))",
        ),
        (
            "({ if: 1, get: 2, set: 3, async: 4, get, set, async })",
            "(paren (object (init if 1) (init get 2) (init set 3) (init async 4) (shorthand get) (shorthand set) (shorthand async)))",
        ),
        (
            "({ get() {}, set() {} })",
            "(paren (object (method get (method ())) (method set (method ()))))",
        ),
        (
            "({ __proto__: a, ['__proto__']: b, __proto__ })",
            "(paren (object (proto __proto__ a) (init [\"__proto__\"] b) (shorthand __proto__)))",
        ),
        (
            "({ '__proto__': a })",
            "(paren (object (proto __proto__ a)))",
        ),
        (
            "({ yield: 1, yield })",
            "(paren (object (init yield 1) (shorthand yield)))",
        ),
    ]);
}

#[test]
fn operators() {
    check_ast(&[
        ("a + b * c", "(+ a (* b c))"),
        ("a * b + c", "(+ (* a b) c)"),
        ("a - b - c", "(- (- a b) c)"),
        ("a ** b ** c", "(** a (** b c))"),
        ("(-a) ** b", "(** (paren (- a)) b)"),
        ("a ** -b", "(** a (- b))"),
        ("a || b && c", "(|| a (&& b c))"),
        ("a ?? b ?? c", "(?? (?? a b) c)"),
        ("(a || b) ?? c", "(?? (paren (|| a b)) c)"),
        ("a | b ?? c", "(?? (| a b) c)"),
        ("a == b != c === d !== e", "(!== (=== (!= (== a b) c) d) e)"),
        ("a < b > c <= d >= e", "(>= (<= (> (< a b) c) d) e)"),
        ("a in b instanceof c", "(instanceof (in a b) c)"),
        ("a << b >> c >>> d", "(>>> (>> (<< a b) c) d)"),
        ("a & b ^ c | d", "(| (^ (& a b) c) d)"),
        ("a % b / c", "(/ (% a b) c)"),
        (
            "!a; ~a; +a; -a; typeof a; void a; delete a.b",
            "(! a) (~ a) (+ a) (- a) (typeof a) (void a) (delete (. a b))",
        ),
        ("typeof typeof a", "(typeof (typeof a))"),
        (
            "++a; --a; a++; a--",
            "(prefix++ a) (prefix-- a) (postfix++ a) (postfix-- a)",
        ),
        ("a+++b", "(+ (postfix++ a) b)"),
        ("- -a", "(- (- a))"),
        ("a ? b : c ? d : e", "(? a b (? c d e))"),
        ("a ? b = 1 : c", "(? a (= b 1) c)"),
        ("a, b, c", "(, a b c)"),
        ("a = b = c", "(= a (= b c))"),
        (
            "a += 1; a -= 1; a *= 1; a /= 1; a %= 1; a **= 1",
            "(+= a 1) (-= a 1) (*= a 1) (/= a 1) (%= a 1) (**= a 1)",
        ),
        (
            "a <<= 1; a >>= 1; a >>>= 1; a &= 1; a |= 1; a ^= 1",
            "(<<= a 1) (>>= a 1) (>>>= a 1) (&= a 1) (|= a 1) (^= a 1)",
        ),
        ("a &&= 1; a ||= 1; a ??= 1", "(&&= a 1) (||= a 1) (??= a 1)"),
        (
            "(a) = 1; (a.b) = 2; ((a)) += 3",
            "(= a 1) (= (. a b) 2) (+= a 3)",
        ),
        ("a / b / c", "(/ (/ a b) c)"),
        ("a\n/b/g", "(/ (/ a b) g)"),
        ("x = a / 2 / /re/", "(= x (/ (/ a 2) /re/))"),
    ]);
}

#[test]
fn member_call_and_new() {
    check_ast(&[
        ("a.b.c", "(. (. a b) c)"),
        ("a[b][c]", "([] ([] a b) c)"),
        ("a.if.true.null", "(. (. (. a if) true) null)"),
        ("f()", "(call f)"),
        ("f(a, b,)", "(call f a b)"),
        (
            "f(a)(b).c[d](e)",
            "(call ([] (. (call (call f a) b) c) d) e)",
        ),
        ("new a", "(new a)"),
        ("new a()", "(new a)"),
        ("new a.b.c(d)", "(new (. (. a b) c) d)"),
        ("new a()()", "(call (new a))"),
        ("new new a()()", "(new (new a))"),
        ("new (a.b)().c", "(. (new (paren (. a b))) c)"),
        ("new a.b[c]", "(new ([] (. a b) c))"),
        ("f() = 1; f()++", "(= (call f) 1) (postfix++ (call f))"),
        ("1..toString()", "(call (. 1 toString))"),
    ]);
}

#[test]
fn functions_and_arrows() {
    check_ast(&[
        ("function f() {}", "(function f ())"),
        (
            "function f(a, b) { return a; }",
            "(function f (a b) (return a))",
        ),
        (
            "function* g() { yield; yield a; }",
            "(function* g () (yield) (yield a))",
        ),
        ("x = function () {}", "(= x (function ()))"),
        ("x = function f(a) {}", "(= x (function f (a)))"),
        ("x = function* () {}", "(= x (function* ()))"),
        ("a => a", "(=> (a) a)"),
        ("() => {}", "(=> ())"),
        ("(a, b) => { return a; }", "(=> (a b) (return a))"),
        ("(a,) => a", "(=> (a) a)"),
        ("(a) => (b) => a + b", "(=> (a) (=> (b) (+ a b)))"),
        ("x = a => a, b", "(, (= x (=> (a) a)) b)"),
        ("a ? b => c : d => e", "(? a (=> (b) c) (=> (d) e))"),
        ("f(a => 1, () => 2)", "(call f (=> (a) 1) (=> () 2))"),
        ("(a => 1)(2)", "(call (paren (=> (a) 1)) 2)"),
        ("a => ({})", "(=> (a) (paren (object)))"),
        ("x => {}\n+ 1", "(=> (x)) (+ 1)"),
        (
            "function* g() { x = yield; f(yield, yield) }",
            "(function* g () (= x (yield)) (call f (yield) (yield)))",
        ),
        (
            "function* g() { yield yield 1 }",
            "(function* g () (yield (yield 1)))",
        ),
        (
            "function* g() { yield /x/ }",
            "(function* g () (yield /x/))",
        ),
        (
            "function* g() { yield\n/x/g }",
            "(function* g () (yield) /x/g)",
        ),
        (
            "function* g() { () => yield }",
            "(function* g () (=> () yield))",
        ),
        ("var yield = 1; yield = 2", "(var (yield 1)) (= yield 2)"),
        (
            "function f() { 'use strict'; }",
            "(function f () \"use strict\")",
        ),
    ]);
}

#[test]
fn statements() {
    check_ast(&[
        ("var a, b = 1;", "(var a (b 1))"),
        ("let a = 1; const b = 2;", "(let (a 1)) (const (b 2))"),
        ("let\na = 1", "(let (a 1))"),
        ("let;", "let"),
        ("let = 1", "(= let 1)"),
        ("let.x", "(. let x)"),
        ("var let", "(var let)"),
        (";", "(empty)"),
        ("{}", "(block)"),
        ("{ a; b }", "(block a b)"),
        ("if (a) b; else c", "(if a b c)"),
        ("if (a) if (b) c; else d", "(if a (if b c d))"),
        ("while (a) b", "(while a b)"),
        ("do a; while (b)", "(do a b)"),
        ("do ; while (0) x", "(do (empty) 0) x"),
        ("for (;;) ;", "(for _ _ _ (empty))"),
        (
            "for (var i = 0; i < n; i++) f(i)",
            "(for (var (i 0)) (< i n) (postfix++ i) (call f i))",
        ),
        ("for (let i = 0, j; ;) ;", "(for (let (i 0) j) _ _ (empty))"),
        ("for (const a = 1;;) ;", "(for (const (a 1)) _ _ (empty))"),
        ("for (i = 0; i; ) ;", "(for (= i 0) i _ (empty))"),
        ("for (let;;) ;", "(for let _ _ (empty))"),
        (
            "for (var a = (b in c);;) ;",
            "(for (var (a (paren (in b c)))) _ _ (empty))",
        ),
        (
            "for (var a = [b in c];;) ;",
            "(for (var (a (array (in b c)))) _ _ (empty))",
        ),
        (
            "a: b: for (;;) { break a; continue b; }",
            "(label a (label b (for _ _ _ (block (break a) (continue b)))))",
        ),
        ("a: { break a; }", "(label a (block (break a)))"),
        (
            "while (1) { break; continue; }",
            "(while 1 (block (break) (continue)))",
        ),
        (
            "function f() { return; return a }",
            "(function f () (return) (return a))",
        ),
        ("throw a", "(throw a)"),
        ("try {} catch (e) {}", "(try (block) (catch e (block)))"),
        (
            "try {} catch {} finally {}",
            "(try (block) (catch _ (block)) (finally (block)))",
        ),
        (
            "try { a } finally { b }",
            "(try (block a) (finally (block b)))",
        ),
        ("debugger;", "(debugger)"),
        ("switch (a) {}", "(switch a)"),
        (
            "switch (a) { case 1: b; break; default: case 2: { c } }",
            "(switch a (case 1 b (break)) (default) (case 2 (block c)))",
        ),
        (
            "x: switch (a) { case 1: let b; function f() {} break x; }",
            "(label x (switch a (case 1 (let b) (function f ()) (break x))))",
        ),
        ("if (a) function f() {}", "(if a (block (function f ())))"),
        ("x: function f() {}", "(label x (function f ()))"),
        (
            "{ function f() {} function f() {} }",
            "(block (function f ()) (function f ()))",
        ),
        ("'use strict'; a", "\"use strict\" a"),
    ]);
}

#[test]
fn automatic_semicolon_insertion() {
    check_ast(&[
        ("a\nb", "a b"),
        ("a\n++b", "a (prefix++ b)"),
        ("a\n++\nb", "a (prefix++ b)"),
        ("a\n(b)", "(call a b)"),
        ("var a = 1\nvar b = 2", "(var (a 1)) (var (b 2))"),
        ("{ a } b", "(block a) b"),
        ("x\n=\n1", "(= x 1)"),
        ("x = -\n1", "(= x (- 1))"),
        ("function f() { return\na }", "(function f () (return) a)"),
        ("for (;;) { break\nx }", "(for _ _ _ (block (break) x))"),
        (
            "x: for (;;) { continue\nx; }",
            "(label x (for _ _ _ (block (continue) x)))",
        ),
        ("do {} while (0) x", "(do (block) 0) x"),
        ("do {} while (0)\nx", "(do (block) 0) x"),
        ("a.b\n.c", "(. (. a b) c)"),
        ("x = 1 /* \n */ ++y", "(= x 1) (prefix++ y)"),
        ("for (;;) let\n x = 1", "(for _ _ _ let) (= x 1)"),
        (
            "var a = function() {}\n(1)",
            "(var (a (call (function ()) 1)))",
        ),
    ]);
    check_errors(&[
        ("a b", "Unexpected identifier 'b'"),
        ("var a = 1 var b = 2", "Unexpected token 'var'"),
        ("a\n++", "Unexpected end of input"),
        ("for (;\n) ;", "Unexpected token ')'"),
        ("if (a) else b", "Unexpected token 'else'"),
        ("x => {} + 1", "Unexpected token '+'"),
        ("x => {}()", "Unexpected token '('"),
        ("x = a\n/b/", "Unexpected end of input"),
    ]);
}

#[test]
fn unsupported_constructs() {
    let cases = [
        ("class A {}", "class"),
        ("x = class {}", "class"),
        ("with (a) {}", "with"),
        ("for (a in b) ;", "for-in"),
        ("for (var a of b) ;", "for-of"),
        ("for await (a of b) ;", "for await"),
        ("let [a] = b", "destructuring"),
        ("var {a} = b", "destructuring"),
        ("[a] = b", "destructuring assignment"),
        ("({a} = b)", "destructuring assignment"),
        ("function f([a]) {}", "destructuring"),
        ("try {} catch ({a}) {}", "destructuring"),
        ("function f(...a) {}", "rest parameter"),
        ("(...a) => 1", "rest parameter"),
        ("(a, ...b) => 1", "rest parameter"),
        ("function f(a = 1) {}", "default parameter"),
        ("(a = 1) => 1", "default parameter"),
        ("([a]) => 1", "destructuring"),
        ("f(...a)", "spread"),
        ("[...a]", "spread"),
        ("({...a})", "spread property"),
        ("({ get a() {} })", "getter or setter"),
        ("({ set a(v) {} })", "getter or setter"),
        ("({ async a() {} })", "async method"),
        ("async function f() {}", "async function"),
        ("x = async function () {}", "async function"),
        ("x = async a => a", "async arrow function"),
        ("x = async (a) => a", "async arrow function"),
        ("function* g() { yield* a }", "yield*"),
        ("a`b`", "tagged template"),
        ("a?.b", "optional chaining"),
        ("function f() { new.target }", "new.target"),
        ("super.a", "super"),
        ("import('a')", "import"),
        ("import a from 'b'", "module syntax"),
        ("export var a", "module syntax"),
        ("1n", "BigInt literal"),
        ("({ 1n: a })", "BigInt literal"),
        ("#a in b", "private name"),
    ];
    for (source, construct) in cases {
        let error = parse(source).expect_err(source);
        assert_eq!(error.message, "not supported yet", "{source}");
        assert_eq!(error.unsupported, Some(construct), "{source}");
        assert_eq!(error.kind, ErrorKind::Syntax);
    }
}

#[test]
fn error_positions() {
    let source = "var a = 1;\nlet b;\r\n  let b;";
    let error = parse(source).expect_err("redeclaration");
    let units: Vec<u16> = source.encode_utf16().collect();
    let lines = crate::LineIndex::new(Str16::Wide(&units));
    let location = lines.location(error.offset);
    assert_eq!((location.line, location.column), (3, 7));
    assert_eq!(
        error.to_string(),
        "SyntaxError: Identifier 'b' has already been declared (at offset 25)"
    );
}

#[test]
fn source_length_limit() {
    use crate::{MAX_SOURCE_LEN, check_source_len};
    assert!(check_source_len(MAX_SOURCE_LEN).is_ok());
    let error = check_source_len(MAX_SOURCE_LEN + 1).expect_err("too long");
    assert_eq!(error.message, "Script is too large");
}

/// Runs `test` on a thread with the 8 MiB stack that ADR 0026 section 9
/// requires of every thread that runs JavaScript.
fn on_engine_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(test)
        .expect("a test thread")
        .join()
        .expect("the test passes");
}

fn assert_too_deep(source: &str) {
    let wide: Vec<u16> = source.encode_utf16().collect();
    let mut budget = RecursionBudget::DEFAULT;
    let error = parse_script(Str16::Wide(&wide), &mut budget)
        .err()
        .unwrap_or_else(|| panic!("{}...: no error", &source[..20]));
    assert_eq!(budget, RecursionBudget::DEFAULT);
    assert_eq!(
        error.kind,
        ErrorKind::Range,
        "{}...: {error}",
        &source[..20]
    );
    assert_eq!(error.message, "Maximum call stack size exceeded");
}

#[test]
fn hostile_deep_nesting() {
    on_engine_stack(|| {
        const N: usize = 100_000;
        let cases = [
            format!("{}1{}", "(".repeat(N), ")".repeat(N)),
            format!("{}{}", "[".repeat(N), "]".repeat(N)),
            format!("x = {}1{}", "{a:".repeat(N), "}".repeat(N)),
            format!("{}{}", "{".repeat(N), "}".repeat(N)),
            format!("{}{}", "function f(){".repeat(N), "}".repeat(N)),
            format!("{}{}", "(function(){".repeat(N), "})".repeat(N)),
            format!("{}1", "a=>".repeat(N)),
            format!("{}1{}", "`${".repeat(N), "}`".repeat(N)),
            format!("{}a", "!".repeat(N)),
            format!("{}a", "- ".repeat(N)),
            format!("{}a", "new ".repeat(N)),
            format!("{}a", "++".repeat(N)),
            format!("{}1", "a=".repeat(N)),
            format!("{}c", "a?b:".repeat(N)),
            format!("{}c{}", "a?".repeat(N), ":d".repeat(N)),
            format!("a{}", "**a".repeat(N)),
            format!("{}1{}", "f(".repeat(N), ")".repeat(N)),
            format!("{}1{}", "a[".repeat(N), "]".repeat(N)),
            format!("{}1", "if(1)".repeat(N)),
            format!("{}1", "while(1)".repeat(N)),
            format!("{}{}", "try{".repeat(N), "}finally{}".repeat(N)),
            (0..N).fold(String::new(), |mut s, i| {
                let _ = write!(s, "l{i}:");
                s
            }) + "1",
            format!("a{}", ".b".repeat(N)),
            format!("a{}", "()".repeat(N)),
            format!("a{}", "[0]".repeat(N)),
        ];
        for source in &cases {
            assert_too_deep(source);
        }
    });
}

#[test]
fn hostile_long_chains() {
    on_engine_stack(|| {
        // Chromium accepts a chain of a million `+` (it does not recurse
        // over it); swb counts each link as a tree level and stops at the
        // budget.
        assert_too_deep(&format!("a{}", "+a".repeat(1_000_000)));
        assert_too_deep(&format!("a{}", "||a".repeat(1_000_000)));
        // A comma list is flat.
        let commas = format!("a{}", ",a".repeat(100_000));
        let script = parse(&commas).expect("a long sequence");
        assert_eq!(script.ast.expr_count(), 100_002);
        // Long chains below the limit parse, and so does deep nesting
        // within the budget.
        assert!(parse(&format!("a{}", "+a".repeat(10_000))).is_ok());
        assert!(parse(&format!("{}1{}", "(".repeat(1000), ")".repeat(1000))).is_ok());
        assert!(
            parse(&format!(
                "{}{}",
                "function f(){".repeat(300),
                "}".repeat(300)
            ))
            .is_ok()
        );
    });
}

#[test]
fn smaller_budgets_stop_earlier() {
    let source: Vec<u16> = "((((((((((1))))))))))".encode_utf16().collect();
    let mut budget = RecursionBudget::new(5 * super::LEVEL_WEIGHT);
    let error = parse_script(Str16::Wide(&source), &mut budget).expect_err("too deep");
    assert_eq!(error.kind, ErrorKind::Range);
    assert_eq!(budget.remaining(), 5 * super::LEVEL_WEIGHT);
    let mut budget = RecursionBudget::new(20 * super::LEVEL_WEIGHT);
    assert!(parse_script(Str16::Wide(&source), &mut budget).is_ok());
}

#[test]
fn hostile_long_lines() {
    const TEN_MB: usize = 10 << 20;
    let cases = [
        format!("x = '{}';", "a".repeat(TEN_MB)),
        format!("x = `{}`;", "a".repeat(TEN_MB)),
        format!("x = /{}/;", "a".repeat(TEN_MB)),
        format!("var {};", "a".repeat(TEN_MB)),
        format!("x = 1; // {}", "a".repeat(TEN_MB)),
        format!("x = {};", "9".repeat(TEN_MB)),
    ];
    for source in &cases {
        let script = parse(source).expect("a long token");
        assert!(script.ast.expr_count() <= 3);
    }
    // A 1 MB line of statements.
    let statements = "x=x+1;".repeat((1 << 20) / 6);
    assert!(parse(&statements).is_ok());
    // Unterminated forms at the end of a long line fail cleanly.
    for source in [
        format!("x = '{}", "a".repeat(TEN_MB)),
        format!("x = ({}", "a,".repeat(TEN_MB / 4)),
        format!("/*{}", "*".repeat(TEN_MB)),
    ] {
        assert!(parse(&source).is_err());
    }
}

/// A program with every construct of the subset.
const SUBSET_PROGRAM: &str = r#"#!/usr/bin/env node
'use strict';
var a = 1, b = 0x1F, c = .5e3;
let d = 'str\n', e = "A", f = `t${a + b}m${c}`;
const g = /re[/]g/gi, h = [1, , 3,], i = { k: 1, 'q': 2, 3: 3, [a]: 4, a, m() { return this; }, *gen() { yield 1; } };
function outer(p, q) {
  var r = p ? q : !p;
  let s = (typeof r === 'string' && r instanceof Object || void 0) ?? null;
  const t = (u, v) => u ** v - -u;
  const w = x => { return x * 2 % 3; };
  function inner() { return arguments.length + r; }
  label: for (let j = 0, k = 10; j < k; j++) {
    if (j & 1) continue label; else if (j >> 2) break label;
    do { j += 1; j -= 1; j *= 1; j /= 1; j %= 7; j <<= 1; j >>= 1; j >>>= 0; } while (false);
    while (k-- > 100) { k |= 0; k &= ~0; k ^= 0; k **= 1; }
  }
  try { throw new Error('x'); } catch (err) { r = err; } finally { s = s || {}; }
  try { r &&= 1; r ||= 2; r ??= 3; } catch { }
  return [r, s, t, w, inner, delete i.k, i['q'], new Date, new Date(1), (a, b)];
}
function* counter(n) { for (var z = 0; z < n; ++z) { yield z; } yield; }
outer(1, 2);
debugger;
;
"#;

#[test]
fn every_prefix_parses_or_fails() {
    on_engine_stack(|| {
        let script = parse(SUBSET_PROGRAM).expect("the subset program");
        assert!(dump_ast(&script).contains("(function* counter (n)"));
        let wide: Vec<u16> = SUBSET_PROGRAM.encode_utf16().collect();
        for end in 0..=wide.len() {
            let mut budget = RecursionBudget::DEFAULT;
            let _ = parse_script(Str16::Wide(&wide[..end]), &mut budget);
            assert_eq!(budget, RecursionBudget::DEFAULT);
        }
        // Suffixes too.
        for start in 0..=wide.len() {
            let mut budget = RecursionBudget::DEFAULT;
            let _ = parse_script(Str16::Wide(&wide[start..]), &mut budget);
        }
    });
}

#[test]
fn early_errors_match_v8() {
    check_errors(EARLY_ERRORS);
}

#[test]
fn valid_sources_parse() {
    for source in VALID {
        if let Err(error) = parse(source) {
            panic!("{source}: {error}");
        }
    }
}

// The tables below come from Node.js 22: `new vm.Script(source)` for each
// source, the message of the SyntaxError or none. Constructs outside the
// subset are left out.

const EARLY_ERRORS: &[(&str, &str)] = &[
    (
        "{ { var x } let x }",
        "Identifier 'x' has already been declared",
    ),
    (
        "{ let x; { var x } }",
        "Identifier 'x' has already been declared",
    ),
    (
        "{ { var x } function x(){} }",
        "Identifier 'x' has already been declared",
    ),
    (
        "{ function x(){} { var x } }",
        "Identifier 'x' has already been declared",
    ),
    (
        "{ { { var x } } { let y } let x }",
        "Identifier 'x' has already been declared",
    ),
    (
        "try {} catch (e) { var e; let f; { var f } }",
        "Identifier 'f' has already been declared",
    ),
    (
        "{ var x; } let x;",
        "Identifier 'x' has already been declared",
    ),
    (
        "for (let x;;) { var x }",
        "Identifier 'x' has already been declared",
    ),
    ("while (a) const b = 1", "Unexpected token 'const'"),
    (
        "var await 1",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "await x",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    ("await ++x", "Unexpected identifier 'x'"),
    ("f(await 1)", "missing ) after argument list"),
    ("let a; let a;", "Identifier 'a' has already been declared"),
    ("let a; var a;", "Identifier 'a' has already been declared"),
    ("var a; let a;", "Identifier 'a' has already been declared"),
    (
        "const a = 1; const a = 2;",
        "Identifier 'a' has already been declared",
    ),
    (
        "{ var a; } let a;",
        "Identifier 'a' has already been declared",
    ),
    (
        "let a; { var a; }",
        "Identifier 'a' has already been declared",
    ),
    (
        "function f(a) { let a; }",
        "Identifier 'a' has already been declared",
    ),
    (
        "function f() {} let f;",
        "Identifier 'f' has already been declared",
    ),
    (
        "let f; function f() {}",
        "Identifier 'f' has already been declared",
    ),
    (
        "'use strict'; { function f() {} function f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "{ function f() {} let f; }",
        "Identifier 'f' has already been declared",
    ),
    (
        "{ function f() {} var f; }",
        "Identifier 'f' has already been declared",
    ),
    (
        "try {} catch (e) { let e; }",
        "Identifier 'e' has already been declared",
    ),
    (
        "for (let i;;) { var i; }",
        "Identifier 'i' has already been declared",
    ),
    (
        "for (let i, i;;) {}",
        "Identifier 'i' has already been declared",
    ),
    (
        "for (const i;;) {}",
        "Missing initializer in const declaration",
    ),
    ("const a;", "Missing initializer in const declaration"),
    (
        "let let = 1;",
        "let is disallowed as a lexically bound name",
    ),
    (
        "'use strict'; var let = 1;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; let = 1;",
        "Unexpected strict mode reserved word",
    ),
    ("a = 1; 1 = 2;", "Invalid left-hand side in assignment"),
    ("a + b = 1;", "Invalid left-hand side in assignment"),
    ("(a, b) = 1;", "Invalid left-hand side in assignment"),
    ("a++ = 1", "Invalid left-hand side in assignment"),
    (
        "++a++",
        "Invalid left-hand side expression in prefix operation",
    ),
    (
        "1++",
        "Invalid left-hand side expression in postfix operation",
    ),
    (
        "++1",
        "Invalid left-hand side expression in prefix operation",
    ),
    ("a + 1 += 2", "Invalid left-hand side in assignment"),
    ("this = 1", "Invalid left-hand side in assignment"),
    ("break;", "Illegal break statement"),
    (
        "continue;",
        "Illegal continue statement: no surrounding iteration statement",
    ),
    ("x: break y;", "Undefined label 'y'"),
    ("x: continue x;", "Undefined label 'x'"),
    (
        "x: { continue x; }",
        "Illegal continue statement: 'x' does not denote an iteration statement",
    ),
    (
        "while (1) { function f() { break; } }",
        "Illegal break statement",
    ),
    ("return;", "Illegal return statement"),
    ("x: x: ;", "Label 'x' has already been declared"),
    ("x: { x: ; }", "Label 'x' has already been declared"),
    (
        "function f() { x: x: ; }",
        "Label 'x' has already been declared",
    ),
    (
        "'use strict'; var eval;",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; var arguments;",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; eval = 1;",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; arguments++;",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; function eval() {}",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; function f(eval) {}",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "function eval() { 'use strict'; }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "function f(eval) { 'use strict'; }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "function f(a, a) { 'use strict'; }",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "'use strict'; function f(a, a) {}",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "(a, a) => 1",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "'use strict'; (eval) => 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "(eval) => { 'use strict'; }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; var implements;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; var static;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; var yield;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; implements = 1;",
        "Unexpected strict mode reserved word",
    ),
    (
        "function static() { 'use strict'; }",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; 010",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "'use strict'; 08",
        "Decimals with leading zeros are not allowed in strict mode.",
    ),
    (
        "'use strict'; '\\07'",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "'use strict'; '\\8'",
        "\\8 and \\9 are not allowed in strict mode.",
    ),
    (
        "function f() { '\\07'; 'use strict'; }",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "'use strict'; delete x;",
        "Delete of an unqualified identifier in strict mode.",
    ),
    (
        "'use strict'; delete (x);",
        "Delete of an unqualified identifier in strict mode.",
    ),
    (
        "'use strict'; delete ((x));",
        "Delete of an unqualified identifier in strict mode.",
    ),
    ("var x = /a/gg;", "Invalid regular expression flags"),
    (
        "function* g() { var yield; }",
        "Unexpected identifier 'yield'",
    ),
    ("function* g(yield) {}", "Unexpected identifier 'yield'"),
    (
        "function* g() { function yield() {} }",
        "Unexpected identifier 'yield'",
    ),
    (
        "function* g() { (yield) => 1 }",
        "Invalid destructuring assignment target",
    ),
    ("function* g() { yield => 1 }", "Unexpected token '=>'"),
    (
        "function* g() { function* yield() {} }",
        "Unexpected identifier 'yield'",
    ),
    (
        "'use strict'; function* yield() {}",
        "Unexpected strict mode reserved word",
    ),
    ("(function* yield() {})", "Unexpected identifier 'yield'"),
    (
        "function* g() { (function* yield() {}) }",
        "Unexpected identifier 'yield'",
    ),
    ("function* g() { yield = 1 }", "Unexpected token '='"),
    ("function* g() { yield ? 1 : 2 }", "Unexpected token '?'"),
    (
        "function* g() { void yield }",
        "Unexpected identifier 'yield'",
    ),
    ("throw\n1", "Illegal newline after throw"),
    ("return\n1", "Illegal return statement"),
    ("var a = 1 var b = 2", "Unexpected token 'var'"),
    ("if (a) else b", "Unexpected token 'else'"),
    (
        "if (1) let x = 1",
        "Lexical declaration cannot appear in a single-statement context",
    ),
    (
        "'use strict'; if (1) function f() {}",
        "In strict mode code, functions can only be declared at top level or inside a block.",
    ),
    (
        "while (1) function f() {}",
        "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.",
    ),
    (
        "'use strict'; x: function f() {}",
        "In strict mode code, functions can only be declared at top level or inside a block.",
    ),
    (
        "while (1) x: function f() {}",
        "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.",
    ),
    (
        "if (1) function* g() {}",
        "Generators can only be declared at the top level or inside a block.",
    ),
    (
        "(a, b) => { let a; }",
        "Identifier 'a' has already been declared",
    ),
    ("(a,)", "Unexpected token ')'"),
    ("()", "Unexpected token ')'"),
    ("(a)\n=> 1", "Unexpected token '=>'"),
    ("((a)) => 1", "Invalid destructuring assignment target"),
    ("(a + b) => 1", "Invalid destructuring assignment target"),
    ("(1) => 1", "Invalid destructuring assignment target"),
    ("(a.b) => 1", "Invalid destructuring assignment target"),
    ("`\\unicode`", "Invalid Unicode escape sequence"),
    (
        "({ __proto__: 1, __proto__: 2 })",
        "Duplicate __proto__ fields are not allowed in object literals",
    ),
    (
        "({ __proto__: 1, '__proto__': 2 })",
        "Duplicate __proto__ fields are not allowed in object literals",
    ),
    ("a ?? b || c", "Unexpected token '||'"),
    ("a || b ?? c", "Unexpected token '??'"),
    (
        "-a ** 2",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "typeof a ** 2",
        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence",
    ),
    (
        "function f() { 'use strict'; 010 }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "({ m(a, a) {} })",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "'use strict'; (function eval() {})",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "(function eval() { 'use strict'; })",
        "Unexpected eval or arguments in strict mode",
    ),
    ("a = 1 = 2", "Invalid left-hand side in assignment"),
    (
        "function f() { a: { function g() { break a; } } }",
        "Undefined label 'a'",
    ),
    ("try {}", "Missing catch or finally after try"),
    (
        "switch (a) { default: default: }",
        "More than one default clause in switch statement",
    ),
    (
        "switch (a) { case 1: continue; }",
        "Illegal continue statement: no surrounding iteration statement",
    ),
    (
        "switch (a) { case 1: let b; let b; }",
        "Identifier 'b' has already been declared",
    ),
    ("switch (a) { b; }", "Unexpected identifier 'b'"),
    ("try {} catch (a, b) {}", "Unexpected token ','"),
    (
        "var v\\u0061r = 1;",
        "Keyword must not contain escaped characters",
    ),
    (
        "\\u0076ar a = 1;",
        "Keyword must not contain escaped characters",
    ),
    (
        "var a = th\\u0069s;",
        "Keyword must not contain escaped characters",
    ),
    ("let\nyield 0", "Unexpected number"),
    (
        "function* g() { let\nyield 0 }",
        "Unexpected identifier 'yield'",
    ),
    (
        "'use strict'; let\nlet",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; for (let = 1;;) ;",
        "Unexpected strict mode reserved word",
    ),
    ("f() &&= 1", "Invalid left-hand side in assignment"),
    ("a b", "Unexpected identifier 'b'"),
    ("a 1", "Unexpected number"),
    ("a 'x'", "Unexpected string"),
    ("a /x/", "Unexpected end of input"),
    ("var", "Unexpected end of input"),
    ("var 1", "Unexpected number"),
    ("var a =", "Unexpected end of input"),
    ("(a", "Unexpected end of input"),
    ("f(a", "missing ) after argument list"),
    ("f(a b)", "missing ) after argument list"),
    ("[a", "Unexpected end of input"),
    ("{", "Unexpected end of input"),
    ("}", "Unexpected token '}'"),
    ("a.1", "Unexpected number"),
    ("a.'x'", "Unexpected string"),
    ("a[1", "Unexpected end of input"),
    ("x = {a b}", "Unexpected identifier 'b'"),
    ("x = {a:}", "Unexpected token '}'"),
    ("x = {1}", "Unexpected number"),
    ("x = {'a'}", "Unexpected string"),
    ("if", "Unexpected end of input"),
    ("if (", "Unexpected end of input"),
    ("if (a", "Unexpected end of input"),
    ("while (a) ", "Unexpected end of input"),
    ("function", "Unexpected end of input"),
    ("function (", "Function statements require a function name"),
    ("function f", "Unexpected end of input"),
    ("function f(", "Unexpected end of input"),
    ("function f(a", "Unexpected end of input"),
    ("function f(a b)", "Unexpected identifier 'b'"),
    ("function f(1)", "Unexpected number"),
    ("function f() {", "Unexpected end of input"),
    ("a ? b", "Unexpected end of input"),
    ("a ? b c", "Unexpected identifier 'c'"),
    ("`${a", "Missing } in template expression"),
    ("`", "Unexpected end of input"),
    (
        "x = await 1",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    (
        "function f() { x = await 1 }",
        "await is only valid in async functions and the top level bodies of modules",
    ),
    ("if (a) const b = 1", "Unexpected token 'const'"),
    (
        "if (a) let b = 1",
        "Lexical declaration cannot appear in a single-statement context",
    ),
    ("a =>", "Unexpected end of input"),
    ("a => {", "Unexpected end of input"),
    ("new", "Unexpected end of input"),
    ("typeof", "Unexpected end of input"),
    ("x = enum", "Unexpected reserved word"),
    ("var enum", "Unexpected reserved word"),
    ("var if", "Unexpected token 'if'"),
    ("enum = 1", "Unexpected reserved word"),
    ("break\nlabel", "Illegal break statement"),
    ("a\n++", "Unexpected end of input"),
    ("if (a) {} else", "Unexpected end of input"),
    ("for (;;", "Unexpected end of input"),
    ("for (;", "Unexpected end of input"),
    (
        "for (const x = 1, y;;) ;",
        "Missing initializer in const declaration",
    ),
    ("x = function* () { yield\n* a }", "Unexpected token '*'"),
    (
        "'use strict'; yield",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; ({ yield })",
        "Unexpected strict mode reserved word",
    ),
    ("({ if })", "Unexpected token 'if'"),
    ("({ this })", "Unexpected token 'this'"),
    (
        "'use strict'; ({ let })",
        "Unexpected strict mode reserved word",
    ),
    ("({ a: 1,, })", "Unexpected token ','"),
    (
        "x = { m(eval) { 'use strict'; } }",
        "Unexpected eval or arguments in strict mode",
    ),
    ("0.toString()", "Invalid or unexpected token"),
    (
        "'use strict'; x = 08.5",
        "Decimals with leading zeros are not allowed in strict mode.",
    ),
    (
        "'use strict'; x = 09",
        "Decimals with leading zeros are not allowed in strict mode.",
    ),
    (
        "x = `a\\07`",
        "Octal escape sequences are not allowed in template strings.",
    ),
    (
        "'use strict'; '\\00'",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "'use strict'; function f() { '\\8'; }",
        "\\8 and \\9 are not allowed in strict mode.",
    ),
    (
        "function f() { '\\8'; 'use strict'; }",
        "\\8 and \\9 are not allowed in strict mode.",
    ),
    (
        "function f() { 'use strict'; '\\8'; }",
        "\\8 and \\9 are not allowed in strict mode.",
    ),
    ("x + (a) => 1", "Malformed arrow function parameter list"),
    ("x + a => 1", "Malformed arrow function parameter list"),
    ("x = () => {} ()", "Unexpected token '('"),
    ("x = () => {}\n()", "Unexpected token ')'"),
    (
        "function* g(){ function f() { yield 1 } }",
        "Unexpected number",
    ),
    ("!a => 1", "Malformed arrow function parameter list"),
    ("a\n=> 1", "Unexpected token '=>'"),
    (
        "(a, b) => { 'use strict'; var static; }",
        "Unexpected strict mode reserved word",
    ),
    (
        "(static) => { 'use strict' }",
        "Unexpected strict mode reserved word",
    ),
    (
        "x = { f(a, a) {} }",
        "Duplicate parameter name not allowed in this context",
    ),
    ("typeof a => 1", "Malformed arrow function parameter list"),
    ("new a => 1", "Malformed arrow function parameter list"),
    ("a.b => 1", "Malformed arrow function parameter list"),
    ("x => {} + 1", "Unexpected token '+'"),
    ("x => {} .a", "Unexpected token '.'"),
    ("x => {}()", "Unexpected token '('"),
    ("({}) = 1", "Invalid left-hand side in assignment"),
    ("`${}`", "Unexpected token '}'"),
    (
        "'use strict'; arguments = 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; ++eval",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; eval++",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; eval += 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; (eval) = 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; try {} catch (eval) {}",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "'use strict'; try {} catch (arguments) {}",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "try {} catch (e) { let e }",
        "Identifier 'e' has already been declared",
    ),
    (
        "try {} catch (e) { function e() {} }",
        "Identifier 'e' has already been declared",
    ),
    (
        "'use strict'; try {} catch (e) { function e() {} }",
        "Identifier 'e' has already been declared",
    ),
    (
        "for (let x = 1; x < 2; x++) function f() {}",
        "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.",
    ),
    (
        "label: for (;;) { label: ; }",
        "Label 'label' has already been declared",
    ),
    ("a: { a: ; }", "Label 'a' has already been declared"),
    ("a: b: a: ;", "Label 'a' has already been declared"),
    ("function f() { break; }", "Illegal break statement"),
    (
        "x: while (1) { (function() { continue x; }) }",
        "Undefined label 'x'",
    ),
    (
        "while (1) { (() => { break; }) }",
        "Illegal break statement",
    ),
    ("x: if (1) continue x;", "Undefined label 'x'"),
    (
        "'use strict'; var package;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; var private;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; var protected;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; var public;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; var interface;",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; var let;",
        "Unexpected strict mode reserved word",
    ),
    ("x = a ?? b && c", "Unexpected token '&&'"),
    ("x = a && b ?? c", "Unexpected token '??'"),
    ("x = a ? b, c : d", "Unexpected token ','"),
    ("x = a\n/b/", "Unexpected end of input"),
    (
        "'use strict'; function f() { arguments = 1 }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "{ let x; var x; }",
        "Identifier 'x' has already been declared",
    ),
    (
        "{ var x; let x; }",
        "Identifier 'x' has already been declared",
    ),
    (
        "{ var f; function f() {} }",
        "Identifier 'f' has already been declared",
    ),
    (
        "function f(a) { let b; var b; }",
        "Identifier 'b' has already been declared",
    ),
    (
        "(function(){ let a; { var a; } })",
        "Identifier 'a' has already been declared",
    ),
    ("function* g() { yield\n* 2 }", "Unexpected token '*'"),
    (
        "'use strict'; '\\08'",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "'use strict'; ({ '\\07': 1 })",
        "Octal escape sequences are not allowed in strict mode.",
    ),
    (
        "'use strict'; ({ 07: 1 })",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "function f() { 'use strict'; return 010 }",
        "Octal literals are not allowed in strict mode.",
    ),
    (
        "'use strict'; function* g() { function f() { yield = 1 } }",
        "Unexpected strict mode reserved word",
    ),
    (
        "function* g() { x = { yield } }",
        "Unexpected identifier 'yield'",
    ),
    (
        "a: { b: { continue a; } }",
        "Illegal continue statement: 'a' does not denote an iteration statement",
    ),
    (
        "while (1) { a: { continue a; } }",
        "Illegal continue statement: 'a' does not denote an iteration statement",
    ),
    ("x = 1; return 2", "Illegal return statement"),
    ("if (1) { return }", "Illegal return statement"),
    ("{ return }", "Illegal return statement"),
    (
        "var f = function f(f) { let f; }",
        "Identifier 'f' has already been declared",
    ),
    (
        "'use strict'; x = function arguments() {}",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "for (let x;;) { var y; let y; }",
        "Identifier 'y' has already been declared",
    ),
    ("function* g() { yield: 1 }", "Unexpected token ':'"),
    (
        "'use strict'; yield: 1",
        "Unexpected strict mode reserved word",
    ),
    (
        "'use strict'; let: 1",
        "Unexpected strict mode reserved word",
    ),
    ("a ? b : c ? d", "Unexpected end of input"),
    ("a ? b, c", "Unexpected token ','"),
    ("x = `\\u{110000}`", "Undefined Unicode code-point"),
    ("x = '\\u{110000}'", "Undefined Unicode code-point"),
    ("(1, 2) => 3", "Invalid destructuring assignment target"),
    ("(a, 1) => 3", "Invalid destructuring assignment target"),
    ("(a, b.c) => 3", "Invalid destructuring assignment target"),
    ("(a)(b) => 3", "Malformed arrow function parameter list"),
    ("a + b => c", "Malformed arrow function parameter list"),
    ("let a, a", "Identifier 'a' has already been declared"),
    (
        "let a; let b; let a",
        "Identifier 'a' has already been declared",
    ),
    (
        "const a = 1, a = 2",
        "Identifier 'a' has already been declared",
    ),
    (
        "function f() { function g() {} let g; }",
        "Identifier 'g' has already been declared",
    ),
    (
        "function f() { let g; function g() {} }",
        "Identifier 'g' has already been declared",
    ),
];

const VALID: &[&str] = &[
    // Hoisting of `var` through blocks (the counters of `declare_var`).
    "{ let x } { var x }",
    "{ var x } { let x }",
    "{ let x; function g(){ var x } }",
    "{ var x; { let x } }",
    "function f(){ { var x } { let x } }",
    "function f(){ { let x } var x }",
    "try {} catch (e) { { var e } }",
    "try {} catch (e) { { let e } }",
    "for (var x;;) { let x }",
    "(function f(){ { var f } })",
    "{ let f; (function f(){ var f }) }",
    "function f(a) { var a; }",
    "{ function f() {} function f() {} }",
    "try {} catch (e) { var e; }",
    "let;",
    "var let = 1;",
    "let\na = 1",
    "(a) = 1;",
    "(a.b) = 1;",
    "a() = 1;",
    "a() ++",
    "x: while(1) continue x;",
    "x: { } x: ;",
    "function f(a, a) {}",
    "function f() { 010; 'use strict'; }",
    "function* g() { yield\n1 }",
    "var yield = 1;",
    "function* yield() {}",
    "function* g() { (function yield() {}) }",
    "yield = 1",
    "function* g() { x = yield; }",
    "function* g() { yield + 1 }",
    "function* g() { a ? yield : yield }",
    "function* g() { yield\n/foo/ }",
    "a\n++b",
    "a\n++\nb",
    "do x\nwhile(0) y",
    "do x; while(0) y",
    "do ; while (0) x",
    "for (;;) let\n x = 1",
    "if (1) function f() {}",
    "x: function f() {}",
    "a => { var a; }",
    "(a,) => 1",
    "() => 1",
    "({ a, b: 1, [c]: 2, d() {} })",
    "({ __proto__: 1, ['__proto__']: 2 })",
    "({ __proto__, __proto__: 2 })",
    "[a, , b]",
    "(-a) ** 2",
    "new a",
    "new a()",
    "new a.b()",
    "'use strict'; ({ eval: 1 })",
    "'use strict'; a.eval = 1",
    "'use strict'; ({ eval() {} })",
    "function* g(a, a) {}",
    "(function(a, a) {})",
    "debugger;",
    "a += b = c",
    "a = b += 1",
    "if (a) ; else ;",
    "label: label2: for(;;) break label;",
    "a: { b: { break a; } }",
    "try {} catch {}",
    "try {} finally {}",
    "`a${`b${c}`}`",
    "var \\u0061 = 1;",
    "var x = { v\\u0061r: 1 }",
    "a.v\\u0061r",
    "function f(a, b, a) { }",
    "'use strict'; ({ a: 1, a: 2 })",
    "a\n(b)",
    "var a = function() {}\n(1)",
    "for (let\nx;;) ;",
    "for (let;;) ;",
    "for (let = 1;;) ;",
    "for (let.x;;) ;",
    "++f()",
    "function f() { return }",
    "x = await",
    "var await",
    "await = 1",
    "function f() { var await; }",
    "var a\\u0077ait",
    "label: 1",
    "x: while(1) { continue\nx; }",
    "var x = 1\n++y",
    "a.b\n.c",
    "new a\n()",
    "do {} while (0) x",
    "do {} while (0)\nx",
    "for (let x = 1, y;;) ;",
    "for (var a, b;;) ;",
    "function* g() { yield /x/ }",
    "function* g() { yield\n/x/g }",
    "function* g() { var o = { yield: 1 } }",
    "function* g() { o.yield }",
    "function* g() { yield; }",
    "function* g() { [yield] }",
    "function* g() { f(yield, yield) }",
    "function* g() { yield, yield }",
    "function* g() { (yield) }",
    "function* g() { yield yield 1 }",
    "'use strict'; ({ yield: 1 })",
    "({ yield })",
    "({ if: 1 })",
    "({ eval })",
    "'use strict'; ({ eval })",
    "({ let })",
    "({ a: 1, })",
    "({ 1: 1, 'b': 2, 0x10: 3, 1.5: 4 })",
    "({ m() {}, *g() {} })",
    "({ get })",
    "({ get: 1 })",
    "({ set() {} })",
    "({ get() {} })",
    "x = {a, b}",
    "x = {'a'() {}}",
    "x = {1() {}}",
    "x = /a/ / 1",
    "x = a / 1 / 2",
    "a\n/b/g",
    "++\na",
    "x = 1 /* \n */ ++y",
    "x = -\n1",
    "x\n=\n1",
    "0..toString()",
    "x = 1_000",
    "x = 0b101",
    "'\\u{1F600}'",
    "x = .5",
    "x = 5.",
    "x = 08.5",
    "'use strict'; x = 0.08",
    "'use strict'; '\\0'",
    "x = a ? b : (c) => d",
    "x = a => a\n(1)",
    "function* g(){ () => yield }",
    "function* g(){ () => { yield } }",
    "function* g(){ function f() { yield } }",
    "(a) => { 'use strict' }",
    "x = { *g() { yield } }",
    "x = { yield() {} }",
    "x = function* () { var o = { [yield]: 1 } }",
    "a => 1 + 2",
    "(a => 1) + 2",
    "a => 1 ? 2 : 3",
    "x = a => 1, b",
    "new (a => 1)",
    "x => {}\n+ 1",
    "f(a => 1, b => 2)",
    "x = ((a, b) => a + b)",
    "`a${b}c${d}`",
    "x = `a\n${b}`",
    "if (a) function f() {} else function g() {}",
    "if (a) ; else function g() {}",
    "x: y: function f() {}",
    "try {} catch (e) { { let e } }",
    "try {} catch (e) { for (var e;;) ; }",
    "for (let x;;) { let x }",
    "for (var x;;) ;",
    "a: { b: ; } b: ;",
    "do continue; while (0)",
    "x: do { continue x; } while (0)",
    "x: if (1) break x;",
    "x: { break x; }",
    "x: try { break x; } finally {}",
    "x: try {} finally { break x; }",
    "var private, protected;",
    "x = { true: 1, null: 2, if: 3 }",
    "x = a.if.true.null",
    "x = 1..a",
    "x = 'a' + `b`",
    "x = new new a()()",
    "x = new a.b.c",
    "x = new a()()",
    "x = new (a.b)().c",
    "x = a[b][c](d)(e).f",
    "x = !a.b",
    "x = -a.b()",
    "x = a++ + ++b",
    "x = a+++b",
    "x = a---b",
    "x = typeof typeof a",
    "x = void 0",
    "x = delete a.b",
    "x = delete a[0]",
    "x = a ** b ** c",
    "x = (a ** b) ** c",
    "x = a ?? b ?? c",
    "x = (a || b) ?? c",
    "x = a ?? (b || c)",
    "x = a | b ?? c",
    "x = a ? b : c ? d : e",
    "x = a ? b : c, d",
    "x = a = b ? c : d",
    "x = a, b = c",
    "x = (a, b, c)",
    "x = a >>>= b",
    "x = a ||= b",
    "x = a &&= b",
    "x = a ??= b",
    "x = a **= b",
    "(a) ||= 1",
    "(a.b) &&= 1",
    "x = [, , ]",
    "x = [1, , 2, ]",
    "x = [,]",
    "x = { a: 1, 'b': 2, 3: 3, [d]: 4, e, f() {} }",
    "x = /[/]/",
    "x = a / b / c",
    "x = (1)\n/2/i",
    "x = `${/a/}`",
    "x = {} / 1",
    "{} /a/",
    "if (a) /b/.test(c)",
    "x = function() {} / 1",
    "x = this / 2",
    "x = a.return / 2",
    "let x; { let x; }",
    "var x; function x(){}",
    "function f(){} var f;",
    "function f(a) { function a(){} }",
    "(function(){ var a; { let a; } })",
    "function f() { let arguments; }",
    "function f() { var arguments; }",
    "function f(arguments) {}",
    "'use strict'; delete this.x",
    "'use strict'; delete x.y",
    "'use strict'; delete (x, y)",
    "function f() { 'use strict' } 010",
    "(function() { 'use strict'; })\n010",
    "function f(a) { 'use strict'; a = 1 }",
    "'use strict'; x = { eval: eval, arguments: arguments }",
    "'use strict'; eval(x); arguments[0]",
    "function g() { yield = 1 }",
    "function* g() { function f() { yield = 1 } }",
    "x = { *yield() {} }",
    "function* g() { x = { yield: yield } }",
    "label: while (1) { label2: { continue label; } }",
    "(() => { return 1 })",
    "function f() { { return } }",
    "var f = function f() { var f; }",
    "(function f() { let f; })",
    "(function f(a) { 'use strict'; })",
    "(function eval(a) { })",
    "try {} catch (e) { var e = 1; }",
    "'use strict'; try {} catch (e) { var e = 1; }",
    "function f() { try {} catch (arguments) {} }",
    "for (const x = 1, y = 2;;) {}",
    "for (let x = 1; x < 3; x++) { let x = 2; }",
    "for (var x;;) { let x; }",
    "x: x",
    "yield: 1",
    "await: 1",
    "let: 1",
    "a?b:c=d",
    "x = /a/u",
    "x = /a/ig",
    "x = 1 + /a/",
    "x = a++ / 2",
    "x = (a) / 2",
    "x = [] / 2",
    "x = {} / 2",
    "a = b => c",
    "(a) => b => c",
    "f(a = 1, b)",
    "var a = 1, b = a, c",
    "var a;\nlet b",
    "let a\nlet b",
    "var a; var a",
    "function f() {} function f() {}",
    "'use strict'; function f() {} function f() {}",
    "if (a) function f() {} else function f() {}",
    "if (a) let\n{}",
];
