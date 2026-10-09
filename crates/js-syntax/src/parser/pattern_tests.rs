//! Tests of the forms of M7 feature 1a: patterns and destructuring,
//! spread and rest, default parameters, `for`-`in` and `for`-`of`,
//! optional chaining, tagged templates, `new.target`, `BigInt` literals,
//! `yield*`, accessors and `with`; their early errors with V8's messages
//! and hostile inputs. The tables of early errors and valid sources come
//! from Node.js 22 (`new vm.Script(source)`).

use std::fmt::Write;
use std::time::Instant;

use swb_js_text::{RecursionBudget, Str16};

use super::parse_script;
use super::tests::{assert_too_deep, check_ast, check_errors, error, on_engine_stack, parse};

#[test]
fn binding_patterns() {
    check_ast(&[
        ("var [a, , b = 1, ...c] = d", "(var ([a _ (= b 1) ...c] d))"),
        (
            "let {a, b: c, [d]: e = 1, ...f} = g",
            "(let ({(a a) (b c) ([d] (= e 1)) ...f} g))",
        ),
        (
            "const {a: [b, {c}], 'd': e, 1: f, 2n: g} = h",
            "(const ({(a [b {(c c)}]) (d e) (1 f) (2n g)} h))",
        ),
        ("var [] = a, {} = b", "(var ([] a) ({} b))"),
        ("let [[[a]]] = b", "(let ([[[a]]] b))"),
        (
            "try {} catch ([a, {b}]) {}",
            "(try (block) (catch [a {(b b)}] (block)))",
        ),
        ("var {let} = a", "(var ({(let let)} a))"),
    ]);
}

#[test]
fn assignment_patterns() {
    check_ast(&[
        (
            "[a, b.c, d[e], ...f] = g",
            "(= [a (. b c) ([] d e) ...f] g)",
        ),
        (
            "({a, b: c.d, e = 1, ...f} = g)",
            "(paren (= {(a a) (b (. c d)) (e (= e 1)) ...f} g))",
        ),
        (
            "[a = 1, [b] = 2, {c} = 3] = x",
            "(= [(= a 1) (= [b] 2) (= {(c c)} 3)] x)",
        ),
        ("[(a), (b.c)] = x", "(= [a (. b c)] x)"),
        ("[ , , ] = x", "(= [_ _] x)"),
        ("x = [a] = y", "(= x (= [a] y))"),
        ("[[[a]]] = x", "(= [[[a]]] x)"),
        (
            "({__proto__: a, __proto__: b} = c)",
            "(paren (= {(__proto__ a) (__proto__ b)} c))",
        ),
    ]);
}

#[test]
fn parameters() {
    check_ast(&[
        (
            "function f(a, [b, c] = [], {d} = {}, ...e) {}",
            "(function f (a (= [b c] (array)) (= {(d d)} (object)) ...e))",
        ),
        (
            "(a, [b], {c}, d = 1, ...e) => 0",
            "(=> (a [b] {(c c)} (= d 1) ...e) 0)",
        ),
        (
            "([a] = b, {c} = d) => 0",
            "(=> ((= [a] b) (= {(c c)} d)) 0)",
        ),
        ("(...[a]) => 0", "(=> (...[a]) 0)"),
        ("({a = 1}) => 0", "(=> ({(a (= a 1))}) 0)"),
        (
            "x = { set a([b] = c) {} }",
            "(= x (object (set a (setter ((= [b] c))))))",
        ),
    ]);
}

#[test]
fn spread_elements_and_arguments() {
    check_ast(&[
        ("f(...a, b, ...c,)", "(call f (... a) b (... c))"),
        ("new F(...a)", "(new F (... a))"),
        ("[...a, b, ...c]", "(array (... a) b (... c))"),
        (
            "({...a, b, ...c})",
            "(paren (object (...a) (shorthand b) (...c)))",
        ),
    ]);
}

#[test]
fn optional_chains_and_tagged_templates() {
    check_ast(&[
        ("a?.b", "(chain (?. a b))"),
        (
            "a?.b.c?.[d](e)?.(f)",
            "(chain (?.call (call (?.[] (. (?. a b) c) d) e) f))",
        ),
        ("(a?.b).c", "(. (paren (chain (?. a b))) c)"),
        ("a?.b\n.c", "(chain (. (?. a b) c))"),
        ("a ?.5 : 1", "(? a 0.5 1)"),
        ("delete a?.b", "(delete (chain (?. a b)))"),
        ("a`x${b}y`", "(tag a (` \"x\" b \"y\"))"),
        ("new a`x`", "(new (tag a (` \"x\")))"),
        ("a.b`x``y`", "(tag (tag (. a b) (` \"x\")) (` \"y\"))"),
        ("a`\\unicode`", "(tag a (` _))"),
        ("a`${b}\\x`", "(tag a (` \"\" b _))"),
    ]);
}

#[test]
fn new_target_yield_bigint_and_accessors() {
    check_ast(&[
        (
            "function f() { return new.target; }",
            "(function f () (return new.target))",
        ),
        (
            "function f() { () => new.target.x }",
            "(function f () (=> () (. new.target x)))",
        ),
        ("function* g() { yield* a; }", "(function* g () (yield* a))"),
        (
            "function* g() { yield* a, b }",
            "(function* g () (, (yield* a) b))",
        ),
        ("x = 123n + 0x1Fn", "(= x (+ 123n 0x1Fn))"),
        (
            "x = { get a() {}, set a(v) {}, *g() {}, [k]() {}, get [k]() {} }",
            "(= x (object (get a (getter ())) (set a (setter (v))) (method g (method* ())) \
             (method [k] (method ())) (get [k] (getter ()))))",
        ),
        (
            "x = { get, set, get: 1, get() {} }",
            "(= x (object (shorthand get) (shorthand set) (init get 1) (method get (method ()))))",
        ),
    ]);
}

#[test]
fn for_in_of_and_with() {
    check_ast(&[
        ("for (var a in b);", "(for-in (var a) b (empty))"),
        ("for (let a of b);", "(for-of (let a) b (empty))"),
        (
            "for (const [a, b] of c);",
            "(for-of (const [a b]) c (empty))",
        ),
        ("for (a.b in c);", "(for-in (. a b) c (empty))"),
        ("for ([a, b] of c);", "(for-of [a b] c (empty))"),
        ("for ({a = 1} of b);", "(for-of {(a (= a 1))} b (empty))"),
        ("for (var a = 1 in b);", "(for-in (var (a 1)) b (empty))"),
        ("for (let in x);", "(for-in let x (empty))"),
        ("for (let of of x);", "(for-of (let of) x (empty))"),
        ("for (a in b, c);", "(for-in a (, b c) (empty))"),
        ("for (f() of x);", "(for-of (call f) x (empty))"),
        ("for (let [a] = b;;);", "(for (let ([a] b)) _ _ (empty))"),
        ("with (a) { b; }", "(with a (block b))"),
    ]);
}

#[test]
fn m7_early_errors_match_v8() {
    check_errors(M7_EARLY_ERRORS);
    // A call as a for-in target: ECMA-262 2025 rejects it in all modes
    // (13.15.1 and 14.7.5.1 make the early error unconditional). Later
    // drafts and web compatibility allow it in sloppy mode code, where it
    // throws a ReferenceError at run time; test262's
    // `assignmenttargettype/direct-callexpression*.js` are `onlyStrict`.
    // swb rejects it in strict mode code only. V8 accepts it there too.
    check_errors(&[(
        "\"use strict\"; for (\u{2038}f() in x);",
        "Invalid left-hand side in for-loop",
    )]);
    // `new.target` with an escape: V8 rejects it, with its own message.
    check_errors(&[(
        "function f() { \u{2038}new.t\\u0061rget }",
        "'new.target' must not contain escaped characters",
    )]);
}

#[test]
fn m7_valid_sources_parse() {
    for source in M7_VALID {
        if let Err(error) = parse(source) {
            panic!("{source}: {error}");
        }
    }
}

/// A program with every form of feature 1a.
const PATTERN_PROGRAM: &str = r"var [a, , b = 1, ...c] = d, {e, f: [g], [h]: i = 2, ...j} = k;
let {l = 1} = m; const [[n]] = o;
[a, b.c, d[e], ...f] = g; ({a, b: c.d, e = 1, ...f} = g);
function p(q, [r, s] = [], {t} = {}, ...u) { var q; return new.target; }
const v = (w, [x], {y}, z = 1, ...aa) => w + x;
f(...a, b, ...c); new F(...a); [...a, b]; ({...a, b});
a?.b.c?.[d](e)?.(f); (a?.b).c; a`x${b}y`; new a`x`;
x = { get a() { return 1; }, set a(v) {}, *g() { yield* h; }, [k]() {}, 1n: 2 };
for (var ab in ac); for (let [ad, ae] of af); for (ag.ah of ai); for ({aj} in ak);
for (var al = 1 in am); with (an) { ao; }
try {} catch ([ap, {aq}]) {}
";

#[test]
fn every_prefix_of_the_pattern_program_parses_or_fails() {
    on_engine_stack(|| {
        assert!(parse(PATTERN_PROGRAM).is_ok());
        let wide: Vec<u16> = PATTERN_PROGRAM.encode_utf16().collect();
        for end in 0..=wide.len() {
            let mut budget = RecursionBudget::DEFAULT;
            let _ = parse_script(Str16::Wide(&wide[..end]), &mut budget);
            assert_eq!(budget, RecursionBudget::DEFAULT);
        }
        for start in 0..=wide.len() {
            let mut budget = RecursionBudget::DEFAULT;
            let _ = parse_script(Str16::Wide(&wide[start..]), &mut budget);
        }
    });
}

#[test]
fn hostile_deep_patterns() {
    on_engine_stack(|| {
        const N: usize = 100_000;
        let cases = [
            format!("{}a{} = x", "[".repeat(N), "]".repeat(N)),
            format!("({}a{} = x)", "{a:".repeat(N), "}".repeat(N)),
            format!("var {}a{} = x", "[".repeat(N), "]".repeat(N)),
            format!("let {}a{} = x", "{a:".repeat(N), "}".repeat(N)),
            format!("function f({}a{}) {{}}", "[".repeat(N), "]".repeat(N)),
            format!("({}a{}) => 0", "[".repeat(N), "]".repeat(N)),
            format!("for ({}a{} of x);", "[".repeat(N), "]".repeat(N)),
            format!("{}0{}", "(a = ".repeat(N), ") => 0".repeat(N)),
            format!("{}0{}", "function f(a = ".repeat(N), ") {}".repeat(N)),
            format!("a{}", "?.b".repeat(N)),
            format!("a{}", "?.(b)".repeat(N)),
            format!("a{}", "`x`".repeat(N)),
            format!("{}a{}", "[...".repeat(N), "]".repeat(N)),
            format!("{}a{}", "f(...".repeat(N), ")".repeat(N)),
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

#[test]
fn hostile_wide_patterns_are_linear() {
    on_engine_stack(|| {
        // 100,000-element patterns and lists: a quadratic step would take
        // minutes.
        let names = |prefix: &str, n: usize| {
            (0..n).fold(String::new(), |mut s, i| {
                let _ = write!(s, "{prefix}{i},");
                s
            })
        };
        let defaults = |n: usize| {
            (0..n).fold(String::new(), |mut s, i| {
                let _ = write!(s, "a{i} = b,");
                s
            })
        };
        let n = 100_000;
        let sources = [
            format!("[{}] = x", names("a", n)),
            format!("var [{}] = x", names("a", n)),
            format!("let {{{}}} = x", names("a", n)),
            format!("({{{}}} = x)", names("a", n)),
            format!("[{}] = x", defaults(n)),
            format!("({}) => 0", names("a", 60_000)),
            format!("({}) => 0", defaults(30_000)),
            format!("function f({}) {{}}", defaults(30_000)),
            format!("({}) => 0", names("[a", 30_000).replace(',', "],")),
            format!("a{}", "?.b".repeat(10_000)),
            format!("a{}", "`x${b}`".repeat(10_000)),
            format!("f({})", names("...a", 60_000)),
        ];
        for source in &sources {
            assert!(timed_parse(source) < 30.0);
        }
        // More than 65,534 parameters.
        let too_many = format!("({}) => 0", names("a", 70_000));
        assert_eq!(
            error(&too_many),
            "Too many parameters in function definition (only 65534 allowed)"
        );
    });
}

#[test]
fn nested_arrow_heads_move_their_items_once() {
    on_engine_stack(|| {
        // Arrow heads nested 300 deep around 100,000 references: each
        // conversion moves only what its own head recorded directly.
        let inner = (0..100_000).fold(String::from("["), |mut s, i| {
            let _ = write!(s, "r{i},");
            s
        }) + "]";
        let source = format!("{}{inner}{}", "(a = ".repeat(300), ") => a".repeat(300));
        assert!(timed_parse(&source) < 30.0);
        let script = parse(&source).expect("nested arrows");
        // Every arrow function's parent is the one around it.
        let ast = &script.ast;
        let mut depth = 0;
        for id in ast.function_ids().skip(1) {
            let mut parent = ast.function(id).parent;
            let mut levels = 0;
            while let Some(p) = parent {
                levels += 1;
                parent = ast.function(p).parent;
            }
            depth = depth.max(levels);
        }
        assert_eq!(depth, 300);
    });
}

const M7_EARLY_ERRORS: &[(&str, &str)] = &[
    ("[‸f()] = 1", "Invalid destructuring assignment target"),
    ("({a: ‸f()} = 1)", "Invalid destructuring assignment target"),
    ("‸({a}) = 1", "Invalid left-hand side in assignment"),
    ("[‸(a = 1)] = 1", "Invalid destructuring assignment target"),
    ("[‸([a])] = 1", "Invalid destructuring assignment target"),
    (
        "({a: ‸({b})} = 1)",
        "Invalid destructuring assignment target",
    ),
    ("[‸...a,] = 1", "Rest element must be last element"),
    ("[‸...a, b] = 1", "Rest element must be last element"),
    ("({...‸a,} = 1)", "Rest element must be last element"),
    (
        "({...‸{a}} = 1)",
        "`...` must be followed by an assignable reference in assignment contexts",
    ),
    (
        "({...‸[a]} = 1)",
        "`...` must be followed by an assignable reference in assignment contexts",
    ),
    (
        "[...‸(a = 1)] = 1",
        "Invalid destructuring assignment target",
    ),
    ("[‸...a = 1] = 1", "Invalid destructuring assignment target"),
    ("({‸a = 1})", "Invalid shorthand property initializer"),
    (
        "\"use strict\"; [‸eval] = 1",
        "Invalid destructuring assignment target",
    ),
    (
        "\"use strict\"; ({‸eval} = 1)",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; ({a: ‸arguments} = 1)",
        "Invalid destructuring assignment target",
    ),
    ("({a: ‸1} = 1)", "Invalid destructuring assignment target"),
    ("[‸1] = 1", "Invalid destructuring assignment target"),
    ("[‸a + b] = 1", "Invalid destructuring assignment target"),
    (
        "({‸get a(){}} = 1)",
        "Invalid destructuring assignment target",
    ),
    ("({‸a(){}} = 1)", "Invalid destructuring assignment target"),
    (
        "({__proto__: a, ‸__proto__: b})",
        "Duplicate __proto__ fields are not allowed in object literals",
    ),
    ("‸[a, b] += 1", "Invalid left-hand side in assignment"),
    (
        "(...‸a,) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "(a, ‸a) => 1",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "([a, ‸a]) => 1",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "function f(a, [b]) { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "function f(a = 1) { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "function f(...a) { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "function f([a], ‸a) {}",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "function f(a, ...‸a) {}",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "(a = 1, ‸a) => 1",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "var ‸[a]",
        "Missing initializer in destructuring declaration",
    ),
    (
        "var ‸{a}",
        "Missing initializer in destructuring declaration",
    ),
    (
        "let ‸[a]",
        "Missing initializer in destructuring declaration",
    ),
    (
        "const ‸{a}",
        "Missing initializer in destructuring declaration",
    ),
    (
        "let [a, ‸a] = 1",
        "Identifier 'a' has already been declared",
    ),
    (
        "let {a, b: ‸a} = 1",
        "Identifier 'a' has already been declared",
    ),
    (
        "try {} catch ([a, ‸a]) {}",
        "Identifier 'a' has already been declared",
    ),
    (
        "try {} catch ([a]) { let ‸a }",
        "Identifier 'a' has already been declared",
    ),
    (
        "try {} catch ([a]) { var ‸a }",
        "Identifier 'a' has already been declared",
    ),
    (
        "try {} catch ({a}) { for (var ‸a of b); }",
        "Identifier 'a' has already been declared",
    ),
    ("(‸...a)", "Unexpected token '...'"),
    ("(‸...a);", "Unexpected token '...'"),
    ("(a, ‸...b)", "Unexpected token '...'"),
    (
        "(...‸a, b) => 1",
        "Rest parameter must be last formal parameter",
    ),
    (
        "(...‸a = 1) => 1",
        "Rest parameter may not have a default initializer",
    ),
    (
        "function f(...a ‸= 1) {}",
        "Rest parameter may not have a default initializer",
    ),
    (
        "function f(...a‸,) {}",
        "Rest parameter must be last formal parameter",
    ),
    (
        "function f(...a‸, b) {}",
        "Rest parameter must be last formal parameter",
    ),
    ("(a,‸)", "Unexpected token ')'"),
    ("(‸(a)) => 1", "Invalid destructuring assignment target"),
    ("(‸(a), b) => 1", "Invalid destructuring assignment target"),
    ("([‸(a)]) => 1", "Invalid destructuring assignment target"),
    (
        "({a: ‸(b)}) => 1",
        "Invalid destructuring assignment target",
    ),
    ("(‸a.b) => 1", "Invalid destructuring assignment target"),
    ("([‸a.b]) => 1", "Illegal property in declaration context"),
    (
        "({a: ‸b.c}) => 1",
        "Illegal property in declaration context",
    ),
    (
        "function* g() { (a = ‸yield) => 1 }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "function* g() { (a = ‸yield 1) => 1 }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "function* g(a = ‸yield) {}",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "function* g() { (‸yield) => 1 }",
        "Invalid destructuring assignment target",
    ),
    (
        "function* g() { var o = { *m(a = ‸yield) {} } }",
        "Yield expression not allowed in formal parameter",
    ),
    (
        "\"use strict\"; (‸eval) => 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; ([‸eval]) => 1",
        "Invalid destructuring assignment target",
    ),
    (
        "\"use strict\"; ({‸arguments}) => 1",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "(‸eval) => { \"use strict\" }",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "({a}) => { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "(a = 1) => { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "(a, b) => { let ‸a }",
        "Identifier 'a' has already been declared",
    ),
    (
        "(a = 1) => { let ‸a }",
        "Identifier 'a' has already been declared",
    ),
    (
        "function f(a = 1) { let ‸a }",
        "Identifier 'a' has already been declared",
    ),
    (
        "function f([a]) { let ‸a }",
        "Identifier 'a' has already been declared",
    ),
    (
        "function f({a}) { const ‸a = 1 }",
        "Identifier 'a' has already been declared",
    ),
    (
        "function f(a, b = 1) { ‸\"use strict\"; }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "function f(eval = 1) { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "x = { get a‸(b) {} }",
        "Getter must not have any formal parameters.",
    ),
    (
        "x = { set a‸() {} }",
        "Setter must have exactly one formal parameter.",
    ),
    (
        "x = { set a‸(b, c) {} }",
        "Setter must have exactly one formal parameter.",
    ),
    (
        "x = { set a‸(...b) {} }",
        "Setter function argument must not be a rest parameter",
    ),
    ("x = { get ‸*a() {} }", "Unexpected token '*'"),
    (
        "x = { m(a, ‸a) {} }",
        "Duplicate parameter name not allowed in this context",
    ),
    (
        "x = { m([a]) { ‸\"use strict\"; } }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "for (var ‸a, b in c);",
        "Invalid left-hand side in for-in loop: Must have a single binding.",
    ),
    (
        "for (var ‸a, b of c);",
        "Invalid left-hand side in for-of loop: Must have a single binding.",
    ),
    (
        "for (let ‸a, b of c);",
        "Invalid left-hand side in for-of loop: Must have a single binding.",
    ),
    (
        "\"use strict\"; for (var ‸a = 1 in b);",
        "for-in loop variable declaration may not have an initializer.",
    ),
    (
        "for (var ‸a = 1 of b);",
        "for-of loop variable declaration may not have an initializer.",
    ),
    (
        "for (let ‸a = 1 in b);",
        "for-in loop variable declaration may not have an initializer.",
    ),
    (
        "for (const ‸a = 1 in b);",
        "for-in loop variable declaration may not have an initializer.",
    ),
    (
        "for (var [a‸] = 1 in b);",
        "for-in loop variable declaration may not have an initializer.",
    ),
    (
        "for (var {a‸} = 1 of b);",
        "for-of loop variable declaration may not have an initializer.",
    ),
    ("for (let of ‸x);", "Unexpected identifier 'x'"),
    (
        "for (‸let.x of y);",
        "The left-hand side of a for-of loop may not start with 'let'.",
    ),
    (
        "for (let ‸[a];;);",
        "Missing initializer in destructuring declaration",
    ),
    (
        "for (const ‸a;;);",
        "Missing initializer in const declaration",
    ),
    (
        "for (var ‸[a];;);",
        "Missing initializer in destructuring declaration",
    ),
    (
        "for (‸async of x);",
        "The left-hand side of a for-of loop may not be 'async'.",
    ),
    ("for (‸a, b of c);", "Invalid left-hand side in for-loop"),
    ("for (a of b‸, c);", "Unexpected token ','"),
    ("for (‸a + b in c);", "Invalid left-hand side in for-loop"),
    ("for (‸a + b of c);", "Invalid left-hand side in for-loop"),
    (
        "for (‸([a]) of b);",
        "Invalid destructuring assignment target",
    ),
    ("for (‸a = 1 of b);", "Invalid left-hand side in for-loop"),
    ("for (‸a = 1 in b);", "Invalid left-hand side in for-loop"),
    ("for (‸a?.b in c);", "Invalid left-hand side in for-loop"),
    ("for (‸a?.b of c);", "Invalid left-hand side in for-loop"),
    (
        "for (new.‸target in x);",
        "new.target expression is not allowed here",
    ),
    ("for (‸this in x);", "Invalid left-hand side in for-loop"),
    ("for (‸1 in x);", "Invalid left-hand side in for-loop"),
    (
        "for (x of y) ‸function f(){}",
        "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.",
    ),
    (
        "for (let x in y) { var ‸x }",
        "Identifier 'x' has already been declared",
    ),
    (
        "for (let x of y) { var ‸x }",
        "Identifier 'x' has already been declared",
    ),
    (
        "for (let [x, ‸x] of y);",
        "Identifier 'x' has already been declared",
    ),
    (
        "for (let ‸let of y);",
        "let is disallowed as a lexically bound name",
    ),
    (
        "for (let x of y) label: ‸function f() {}",
        "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.",
    ),
    (
        "for (var x in y) ‸let [a] = 1",
        "Lexical declaration cannot appear in a single-statement context",
    ),
    (
        "\"use strict\"; for (‸eval in x);",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; for ([‸eval] of x);",
        "Invalid destructuring assignment target",
    ),
    ("for (let x of y‸, z);", "Unexpected token ','"),
    ("a?.b‸`x`", "Invalid tagged template on optional chain"),
    ("a?.‸`x`", "Invalid tagged template on optional chain"),
    ("a?.b\n‸`x`", "Invalid tagged template on optional chain"),
    ("new a‸?.b", "Invalid optional chain from new expression"),
    ("new a‸?.b()", "Invalid optional chain from new expression"),
    ("‸a?.b = 1", "Invalid left-hand side in assignment"),
    ("‸a?.b += 1", "Invalid left-hand side in assignment"),
    (
        "‸a?.b++",
        "Invalid left-hand side expression in postfix operation",
    ),
    (
        "++‸a?.b",
        "Invalid left-hand side expression in prefix operation",
    ),
    ("[‸a?.b] = 1", "Invalid destructuring assignment target"),
    (
        "({x: ‸a?.b} = 1)",
        "Invalid destructuring assignment target",
    ),
    ("‸(a?.b) = 1", "Invalid left-hand side in assignment"),
    ("`‸\\unicode`", "Invalid Unicode escape sequence"),
    (
        "`\\‸08`",
        "Octal escape sequences are not allowed in template strings.",
    ),
    ("new.‸target", "new.target expression is not allowed here"),
    (
        "() => new.‸target",
        "new.target expression is not allowed here",
    ),
    (
        "function f() { ‸new.target = 1 }",
        "Invalid left-hand side in assignment",
    ),
    (
        "function f() { ‸new.target++ }",
        "Invalid left-hand side expression in postfix operation",
    ),
    ("function f() { new.‸foo }", "Unexpected identifier 'foo'"),
    (
        "(a = new.‸target) => 1",
        "new.target expression is not allowed here",
    ),
    ("function* g() { yield\n‸* a }", "Unexpected token '*'"),
    ("function* g() { yield* ‸}", "Unexpected token '}'"),
    ("‸1.5n", "Invalid or unexpected token"),
    ("‸1e3n", "Invalid or unexpected token"),
    ("‸01n", "Invalid or unexpected token"),
    ("‸08n", "Invalid or unexpected token"),
    ("‸.5n", "Invalid or unexpected token"),
    (
        "\"use strict\"; ‸with (a) b",
        "Strict mode code may not include a with statement",
    ),
    (
        "function f() { \"use strict\"; ‸with (a) b }",
        "Strict mode code may not include a with statement",
    ),
    (
        "with (a) ‸function f() {}",
        "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.",
    ),
    (
        "with (a) ‸let x = 1",
        "Lexical declaration cannot appear in a single-statement context",
    ),
    (
        "let {...‸{a}} = x",
        "`...` must be followed by an identifier in declaration contexts",
    ),
    (
        "let {...‸[a]} = x",
        "`...` must be followed by an identifier in declaration contexts",
    ),
    ("let {...‸a,} = x", "Rest element must be last element"),
    ("let {...‸a, b} = x", "Rest element must be last element"),
    ("let [‸...a,] = x", "Rest element must be last element"),
    (
        "let [‸...a = 1] = x",
        "Invalid destructuring assignment target",
    ),
    ("let [a, ‸...b, c] = x", "Rest element must be last element"),
    (
        "function f({...‸{a}}) {}",
        "`...` must be followed by an identifier in declaration contexts",
    ),
    ("let {a: ‸1} = x", "Invalid destructuring assignment target"),
    ("let {‸if} = x", "Unexpected token 'if'"),
    (
        "\"use strict\"; let {‸eval} = x",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; var [‸arguments] = x",
        "Invalid destructuring assignment target",
    ),
    (
        "let {‸let} = x",
        "let is disallowed as a lexically bound name",
    ),
    (
        "let [‸let] = x",
        "let is disallowed as a lexically bound name",
    ),
    ("let {a,‸,b} = x", "Unexpected token ','"),
    ("let {a ‸b} = x", "Unexpected identifier 'b'"),
    ("let [‸(a)] = x", "Invalid destructuring assignment target"),
    (
        "let {a: ‸(b)} = x",
        "Invalid destructuring assignment target",
    ),
    ("let [‸a.b] = x", "Illegal property in declaration context"),
    (
        "\"use strict\"; function f(a = 1) { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "\"use strict\"; (a = 1) => { ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "function f(a = 1) { ‸\"use strict\"; \"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "function f(a = 1) { \"a\"; ‸\"use strict\" }",
        "Illegal 'use strict' directive in function with non-simple parameter list",
    ),
    (
        "({‸a = 1, b = 2})",
        "Invalid shorthand property initializer",
    ),
    ("({a: 1, ‸b = 2})", "Invalid shorthand property initializer"),
    ("x = {‸a = 1}", "Invalid shorthand property initializer"),
    ("f({‸a = 1})", "Invalid shorthand property initializer"),
    ("[{‸a = 1}]", "Invalid shorthand property initializer"),
    ("[{‸a = 1}].x = 1", "Invalid shorthand property initializer"),
    ("({‸a = 1}).b", "Invalid shorthand property initializer"),
    ("({‸a = 1}) + 1", "Invalid shorthand property initializer"),
    (
        "(({‸a = 1})) => 1",
        "Invalid shorthand property initializer",
    ),
    ("({‸a = 1}, b)", "Invalid shorthand property initializer"),
    ("[‸...a, ...b] = x", "Rest element must be last element"),
    ("‸({ async }) = x", "Invalid left-hand side in assignment"),
    (
        "({ ‸get a() {} } = x)",
        "Invalid destructuring assignment target",
    ),
    (
        "\"use strict\"; ({ ‸yield } = x)",
        "Unexpected strict mode reserved word",
    ),
    (
        "function* g() { ({ ‸yield } = x) }",
        "Unexpected identifier 'yield'",
    ),
    (
        "function* g() { [‸yield] = x }",
        "Invalid destructuring assignment target",
    ),
    (
        "function* g() { ({a: ‸yield} = x) }",
        "Invalid destructuring assignment target",
    ),
    (
        "\"use strict\"; [‸arguments] = x",
        "Invalid destructuring assignment target",
    ),
    (
        "\"use strict\"; ({‸arguments} = x)",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; ({a: ‸eval} = x)",
        "Invalid destructuring assignment target",
    ),
    (
        "\"use strict\"; ({‸eval = 1} = x)",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "\"use strict\"; [‸eval = 1] = x",
        "Unexpected eval or arguments in strict mode",
    ),
    ("function* g() { yield ‸=> 1 }", "Unexpected token '=>'"),
    ("(a, b)\n‸=> c", "Unexpected token '=>'"),
    ("a\n‸=> b", "Unexpected token '=>'"),
    (
        "({ __proto__: a, ‸\"__proto__\": b })",
        "Duplicate __proto__ fields are not allowed in object literals",
    ),
    (
        "({ __proto__: a, ‸__proto__: b }.x)",
        "Duplicate __proto__ fields are not allowed in object literals",
    ),
    (
        "[{ __proto__: a, ‸__proto__: b }]",
        "Duplicate __proto__ fields are not allowed in object literals",
    ),
    (
        "f({ __proto__: a, ‸__proto__: b })",
        "Duplicate __proto__ fields are not allowed in object literals",
    ),
    (
        "function* g() { var o = { ‸yield } }",
        "Unexpected identifier 'yield'",
    ),
    (
        "\"use strict\"; var { ‸yield } = x",
        "Unexpected strict mode reserved word",
    ),
    (
        "function* g() { var { ‸yield } = x }",
        "Unexpected identifier 'yield'",
    ),
    (
        "function* g() { var { a: ‸yield } = x }",
        "Invalid destructuring assignment target",
    ),
    (
        "function* g() { var [‸yield] = x }",
        "Invalid destructuring assignment target",
    ),
    (
        "function* g() { function ‸yield() {} }",
        "Unexpected identifier 'yield'",
    ),
    (
        "for (let ‸let in x);",
        "let is disallowed as a lexically bound name",
    ),
    (
        "for (let [‸let] in x);",
        "let is disallowed as a lexically bound name",
    ),
    (
        "for (const ‸let in x);",
        "let is disallowed as a lexically bound name",
    ),
    (
        "let\n‸let = 1",
        "let is disallowed as a lexically bound name",
    ),
    (
        "\"use strict\"; for (‸let.x in y);",
        "Unexpected strict mode reserved word",
    ),
    (
        "([‸(a) = 1] = x) => 0",
        "Invalid destructuring assignment target",
    ),
    (
        "([‸(a)] = x) => 0",
        "Invalid destructuring assignment target",
    ),
    (
        "({b: ‸(a) = 1} = x) => 0",
        "Invalid destructuring assignment target",
    ),
    ("(‸(a) = 1) => 0", "Invalid destructuring assignment target"),
];

const M7_VALID: &[&str] = &[
    "try {} catch ([let]) {}",
    "try {} catch ({let}) {}",
    "for (f() in x);",
    "for (f() of x);",
    "f() = 1",
    "f() += 1",
    "f()++",
    "[(a)] = 1",
    "({a: (b.c)} = 1)",
    "({a: (b)} = 1)",
    "[...[a]] = 1",
    "[...(a)] = 1",
    "({a = 1}) => 1",
    "({a = 1} = 1)",
    "({__proto__: a, __proto__: b} = 1)",
    "[{__proto__: a, __proto__: b}] = 1",
    "({__proto__: a, __proto__: b}) => 1",
    "x = {__proto__: a, __proto__: b} = 1",
    "({a}) => 1",
    "(a, [b]) => 1",
    "([a] = 1) => 1",
    "({a: b = 1, c: [d] = 2, ...e}) => 1",
    "(...a) => 1",
    "(...[a]) => 1",
    "function f(a, a) {}",
    "var [a] = 1, {b} = 2",
    "for (var [a] of b);",
    "for (let {a} in b);",
    "for (const [a] of b);",
    "var [a, a] = 1",
    "try {} catch (a) { var a }",
    "try {} catch (a) { for (var a of b); }",
    "try {} catch (a) { for (var a in b); }",
    "function f(a,) {}",
    "(a,) => 1",
    "f(...a,)",
    "f(...a, ...b)",
    "new F(...a)",
    "[...a, ...b]",
    "({...a, ...b})",
    "({...a,})",
    "(a = yield) => 1",
    "function* g() { function f(a = yield) {} }",
    "(a) => { \"use strict\" }",
    "(a = 1) => { var a }",
    "function f(a = 1) { var a }",
    "function f(a = 1) { function a(){} }",
    "function f(a) { \"use strict\"; }",
    "x = { get a() {} }",
    "x = { set a([b]) {} }",
    "x = { set a(b = 1) {} }",
    "x = { set a(b,) {} }",
    "x = { get [a]() {}, set [b](c) {} }",
    "x = { get 1() {}, set 'x'(c) {} }",
    "x = { get() {}, set() {} }",
    "x = { get: 1, set: 2 }",
    "x = { *get() {} }",
    "x = { m(a) { \"use strict\"; } }",
    "x = { get\na() {} }",
    "x = { a, b, }",
    "x = { 1n: a }",
    "x = { [a]: 1, [b]() {}, *[c]() {} }",
    "x = { ...a }",
    "x = { a: 1, ...b, c }",
    "for (a in b);",
    "for (a of b);",
    "for (var a in b);",
    "for (var a of b);",
    "for (let a in b);",
    "for (const a of b);",
    "for (var a = 1 in b);",
    "for (let in x);",
    "for (let.x in y);",
    "for (let.x;;);",
    "for (let;;);",
    "for (let = 1;;);",
    "for (let [a] = 1;;);",
    "for (async.x of y);",
    "for ((async) of x);",
    "for (async in x);",
    "for (a in b, c);",
    "for ([a] of b);",
    "for ({a} of b);",
    "for ({a = 1} of b);",
    "for ({a = 1} in b);",
    "for ([a.b] of c);",
    "for ((a) of b);",
    "for ((a.b) in c);",
    "for (let x of x);",
    "for (let of of x);",
    "for (var of of x);",
    "for (of of x);",
    "for (of in x);",
    "for (var x of y) { let x }",
    "for (var x in y) let",
    "a?.b",
    "a?.[b]",
    "a?.(b)",
    "a?.b.c(d)[e]",
    "a?.b?.c",
    "new (a?.b)()",
    "delete a?.b",
    "(a?.b).c = 1",
    "a?.b()",
    "a ?.5 : 1",
    "a?.5:1",
    "a ? .5 : 1",
    "a.b`x`",
    "new a`x`",
    "new a.b`x`()",
    "a`x``y`",
    "a`\\unicode`",
    "a`\\u{110000}`",
    "a`\\08`",
    "a`${b}\\x`",
    "function f() { new.target }",
    "function f() { () => new.target }",
    "function f() { new.target.x = 1 }",
    "function f() { new new.target }",
    "function f() { new.target() }",
    "function f() { x = { m() { new.target } } }",
    "x = { m(a = new.target) {} }",
    "function f(a = new.target) {}",
    "function* g() { yield* a }",
    "function* g() { yield * a }",
    "function* g() { yield*\na }",
    "function* g() { x = yield* a, b }",
    "function* g() { yield a, b }",
    "1n",
    "0x1fn",
    "0o7n",
    "0b1n",
    "1_0n",
    "x = 1n + 2n",
    "with (a) b",
    "with (a) { var b }",
    "with (a) let\n{}",
    "let [...[a]] = x",
    "let {1: a} = x",
    "let {[a]: b} = x",
    "let {a = 1, b: c = 2} = x",
    "let {if: a} = x",
    "let {eval} = x",
    "var {let} = x",
    "let [a,,b] = x",
    "({a = 1}, {b = 2}) => 1",
    "[...a] = x",
    "[...a.b] = x",
    "({...a.b} = x)",
    "({...a} = x)",
    "({...(a)} = x)",
    "({...(a.b)} = x)",
    "[a = 1, [b] = 2, {c} = 3] = x",
    "({a: [b] = 1} = x)",
    "({a = 1} = x)",
    "({a: b = 1} = x)",
    "({\"a\": b, 1: c, [d]: e} = x)",
    "[(a), (b.c), (d[e])] = x",
    "[(a) = 1] = x",
    "[((a))] = x",
    "({a: ((b))} = x)",
    "[a, ...(b)] = x",
    "[ , , ] = x",
    "[a, , b, ] = x",
    "x = [a] = y",
    "[[[a]]] = x",
    "({ async } = x)",
    "({ get } = x)",
    "({ await } = x)",
    "({ yield } = x)",
    "function* g() { [a = yield] = x }",
    "var [a, , b = 1, ...c] = d",
    "let {a, b: c, [d]: e = 1, ...f} = g",
    "const {a: [b, {c}], 'd': e, 1: f, 2n: g} = h",
    "[a, b.c, d[e], ...f] = g",
    "({a, b: c.d, e = 1, ...f} = g)",
    "function f(a, [b, c] = [], {d} = {}, ...e) {}",
    "(a, [b], {c}, d = 1, ...e) => 0",
    "([a] = b, {c} = d) => 0",
    "f(...a, b, ...c)",
    "[...a, b, ...c]",
    "({...a, b, ...c})",
    "a?.b.c?.[d](e)?.(f)",
    "(a?.b).c",
    "a`x${b}y`",
    "a.b`x``y`",
    "x = { get a() {}, set a(v) {}, *g() {}, [k]() {}, get [k]() {} }",
    "function f() { return new.target; }",
    "function* g() { yield* a; }",
    "x = 123n + 0x1Fn",
    "for (let a of b);",
    "for (const [a, b] of c);",
    "for (a.b in c);",
    "for ([a, b] of c);",
    "for ({a} in b);",
    "with (a) { b; }",
    "try {} catch ([a, {b}]) {}",
    "(a = 1) => { var a; }",
    "function f(a = 1) { var b; let c; }",
    "a = b => c => d",
    "x = (a, b) => a + b",
    "f(async)",
    "function* g(a = () => yield) {}",
    "function* g() { (a = () => yield) => 1 }",
    "function* g() { (a = function* () { yield }) => 1 }",
    "function* g() { (yield) }",
    "({ get get() {} })",
    "({ __proto__: a, __proto__ })",
    "({ __proto__: a, __proto__() {} })",
    "({ __proto__: a, ['__proto__']: b })",
    "({ __proto__: a, __proto__: b }) => 1",
    "({ a: { __proto__: x, __proto__: y } } = z)",
    "function f() { (a = new.target) => a }",
    "function f(a = eval(\"\")) {}",
    "for (let [a = b] of c);",
    "for (let {a = b} in c);",
    "for (let x = 1;;) { let x; }",
    "for (let x of y) { let x; }",
    "for (let x in y) { function x() {} }",
    "function f() { for (var x of y) {} }",
    "label: for (var x of y) continue label;",
    "for (x of y) { break; }",
    "var [a = function(){}] = b",
    "let x; for (x of y);",
    "let [x] = y, z = x;",
    "var {a: b = c = d} = e",
    "[a] = [b] = c",
    "[...a] = [...b] = c",
    "x = ([a]) => [b] = a",
    "(a, b) => (c, d) => e",
    "((a, b) => c)(d)",
    "(a) => (b) = c",
    "(a = b = c) => 0",
    "(a = (b, c)) => 0",
    "([a, b] = [c, d]) => 0",
    "({a: [b = c]} = d) => 0",
    "(a, ...b) => 0",
    "(...a) => (...b) => 0",
    "async(a, b)",
    "async (a, b)",
    "async",
    "var async = 1; async + 1",
    "({ async: 1 })",
    "x = { async() {} }",
    "x = { async }",
    "x = { async: async }",
    "x = { get async() {} }",
    "x = { set async(v) {} }",
    "x = { *async() {} }",
    "(function* () { yield\n/x/g })",
    "(function* () { yield /x/g })",
    "(function* () { yield* /x/g })",
    "function* g() { var o = { [yield]: 1 } }",
    "function* g() { var o = { yield: 1 } }",
    "var { yield } = x",
    "var [let] = x",
];
