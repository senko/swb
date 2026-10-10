//! Tests of the Module goal (M7 feature 1c): import and export
//! declarations, import attributes, `import()` and `import.meta`,
//! top-level `await`, the module early errors with V8's messages and
//! positions (measured with `node --check` on `.mjs` files in Node.js 22),
//! the module records and the module scope, and hostile inputs.

use std::fmt::Write;
use std::time::Instant;

use swb_js_text::{RecursionBudget, Str16};

use super::tests::{on_engine_stack, split_marker};
use crate::dump::{dump_ast, dump_references, dump_scopes};
use crate::error::{ErrorKind, ParseError};
use crate::module::{ExportEntry, ExportImportName, ImportName, ModuleRecord};
use crate::parser::{Script, parse_module};

/// Parses a module in both code-unit widths (the narrow one if it fits)
/// and checks that both give the same tree or error.
fn parse(source: &str) -> Result<Script, ParseError> {
    let wide: Vec<u16> = source.encode_utf16().collect();
    let mut budget = RecursionBudget::DEFAULT;
    let result = parse_module(Str16::Wide(&wide), &mut budget);
    assert_eq!(budget, RecursionBudget::DEFAULT, "the budget comes back");
    if wide.iter().all(|&u| u < 256) {
        let narrow: Vec<u8> = wide.iter().map(|&u| u as u8).collect();
        let narrow_result = parse_module(Str16::Latin1(&narrow), &mut budget);
        match (&result, &narrow_result) {
            (Ok(a), Ok(b)) => assert_eq!(dump_ast(a), dump_ast(b), "{source}"),
            (Err(a), Err(b)) => assert_eq!(a, b, "{source}"),
            _ => panic!("the widths disagree on {source}"),
        }
    }
    result
}

fn module(source: &str) -> Script {
    parse(source).unwrap_or_else(|error| panic!("{source}: {error}"))
}

fn record(source: &str) -> ModuleRecord {
    module(source).module.expect("a module has a record")
}

/// The text of a name of `script`.
fn text(script: &Script, name: crate::NameId) -> String {
    script.name_text(name)
}

/// Checks the message and the offset (the position marker) of each error.
fn check_module_errors(cases: &[(&str, &str)]) {
    for &(marked, expected) in cases {
        let (source, offset) = split_marker(marked);
        match parse(&source) {
            Ok(script) => panic!("{source}: no error, parsed as {}", dump_ast(&script)),
            Err(error) => {
                assert_eq!(error.message, expected, "message of {source:?}");
                assert_eq!(error.offset, offset, "offset of {source:?}");
            }
        }
    }
}

fn check_module_valid(sources: &[&str]) {
    for source in sources {
        if let Err(error) = parse(source) {
            panic!("{source}: {error}");
        }
    }
}

/// The early errors of module code, as V8 reports them.
const MODULE_ERRORS: &[(&str, &str)] = &[
    ("import {a as ‸\"s\"} from \"m\";", "Unexpected string"),
    ("import {‸\"s\"} from \"m\";", "Unexpected reserved word"),
    ("import {‸default} from \"m\";", "Unexpected reserved word"),
    ("import {a as ‸if} from \"m\";", "Unexpected reserved word"),
    ("import * ‸from \"m\";", "Unexpected identifier 'from'"),
    (
        "import a from \"m\" with { type: \"json\", ‸type: \"css\" };",
        "Import assertion has duplicate key 'type'",
    ),
    (
        "import a from \"m\" with { \"type\": \"json\", ‸type: \"css\" };",
        "Import assertion has duplicate key 'type'",
    ),
    (
        "import a from \"m\" with { type: ‸json };",
        "Unexpected identifier 'json'",
    ),
    (
        "import a from \"m\" with { ‸1: \"x\" };",
        "Unexpected number",
    ),
    (
        "import a from \"m\" ‸assert { type: \"json\" };",
        "Unexpected identifier 'assert'",
    ),
    ("import a‸, b from \"m\";", "Unexpected token ','"),
    (
        "import a from \"m\"; ‸import a from \"n\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "import a from \"m\"; let ‸a;",
        "Identifier 'a' has already been declared",
    ),
    (
        "let a; ‸import a from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "import a from \"m\"; var ‸a;",
        "Identifier 'a' has already been declared",
    ),
    (
        "var a; ‸import a from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "import a from \"m\"; ‸function a(){}",
        "Identifier 'a' has already been declared",
    ),
    (
        "function a(){} ‸function a(){}",
        "Identifier 'a' has already been declared",
    ),
    (
        "function a(){} var ‸a;",
        "Identifier 'a' has already been declared",
    ),
    (
        "var a; ‸function a(){}",
        "Identifier 'a' has already been declared",
    ),
    (
        "import {a, ‸a} from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "import {a, b as ‸a} from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "import a from \"m\" ‸import b from \"n\";",
        "Unexpected token 'import'",
    ),
    ("import a from ‸m;", "Unexpected identifier 'm'"),
    ("import a ‸\"m\";", "Unexpected string"),
    ("import a from‸;", "Unexpected token ';'"),
    (
        "import ‸eval from \"m\";",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "import ‸arguments from \"m\";",
        "Unexpected eval or arguments in strict mode",
    ),
    ("import ‸await from \"m\";", "Unexpected reserved word"),
    (
        "import ‸yield from \"m\";",
        "Unexpected strict mode reserved word",
    ),
    (
        "import ‸let from \"m\";",
        "Unexpected strict mode reserved word",
    ),
    (
        "import {a as ‸await} from \"m\";",
        "Unexpected reserved word",
    ),
    ("{ import ‸a from \"m\"; }", "Unexpected identifier 'a'"),
    (
        "function f() { import ‸a from \"m\"; }",
        "Unexpected identifier 'a'",
    ),
    ("if (1) import ‸a from \"m\";", "Unexpected identifier 'a'"),
    ("export {‸a};", "Export 'a' is not defined in module"),
    (
        "var a; export {a as b, ‸a as b};",
        "Duplicate export of 'b'",
    ),
    ("var a; export {a}; export {‸a};", "Duplicate export of 'a'"),
    (
        "var a; export {a as default}; export ‸default 1;",
        "Duplicate export of 'default'",
    ),
    (
        "export default 1; export ‸default 2;",
        "Identifier '.default' has already been declared",
    ),
    (
        "var a; export {a as ‸\"\\uD800\"};",
        "Invalid module export name: contains unpaired surrogate",
    ),
    (
        "export {‸\"s\"};",
        "String literal module export names must be followed by a 'from' clause",
    ),
    (
        "export {‸\"s\" as a};",
        "String literal module export names must be followed by a 'from' clause",
    ),
    (
        "export {‸\"\\uD800\"} from \"m\";",
        "Invalid module export name: contains unpaired surrogate",
    ),
    ("export {‸if};", "Unexpected reserved word"),
    ("export {‸await};", "Unexpected reserved word"),
    ("export {‸let};", "Unexpected reserved word"),
    ("export {‸implements};", "Unexpected reserved word"),
    ("export {‸a as if};", "Export 'a' is not defined in module"),
    (
        "export * as ns from \"m\"; export * as ‸ns from \"n\";",
        "Duplicate export of 'ns'",
    ),
    ("export *‸;", "Unexpected token ';'"),
    (
        "export const ‸a;",
        "Missing initializer in const declaration",
    ),
    (
        "export ‸function(){}",
        "Function statements require a function name",
    ),
    (
        "export async ‸function(){}",
        "Function statements require a function name",
    ),
    ("export class ‸{}", "Unexpected token '{'"),
    (
        "export default class C {} var ‸C;",
        "Identifier 'C' has already been declared",
    ),
    (
        "export default function f(){} var ‸f;",
        "Identifier 'f' has already been declared",
    ),
    ("export default ‸var a = 1;", "Unexpected token 'var'"),
    (
        "export default ‸let a = 1;",
        "Unexpected strict mode reserved word",
    ),
    ("export default 1‸, 2;", "Unexpected token ','"),
    (
        "export default ‸yield;",
        "Unexpected strict mode reserved word",
    ),
    ("export let a; export {‸a};", "Duplicate export of 'a'"),
    (
        "export function f(){} export {‸f};",
        "Duplicate export of 'f'",
    ),
    ("export var a; export ‸var a;", "Duplicate export of 'a'"),
    (
        "export let a; export let ‸a;",
        "Identifier 'a' has already been declared",
    ),
    ("{ ‸export var a; }", "Unexpected token 'export'"),
    (
        "function f(){ ‸export var a; }",
        "Unexpected token 'export'",
    ),
    ("‸export", "Unexpected token 'export'"),
    ("‸export x;", "Unexpected token 'export'"),
    ("export ‸async\nfunction g(){}", "Unexpected token 'async'"),
    ("export ‸async;", "Unexpected token 'async'"),
    ("export ‸async () => 1", "Unexpected token 'async'"),
    ("export {a ‸b};", "Unexpected identifier 'b'"),
    ("export {a,‸,b};", "Unexpected token ','"),
    ("await‸;", "Unexpected token ';'"),
    ("function f(){ ‸await 1 }", "Unexpected reserved word"),
    ("function f(){ ‸await }", "Unexpected reserved word"),
    ("function f(){ var ‸await; }", "Unexpected reserved word"),
    ("var ‸await;", "Unexpected reserved word"),
    ("let ‸await;", "Unexpected reserved word"),
    ("() => ‸await 1", "Unexpected reserved word"),
    (
        "(a = ‸await 1) => a",
        "Illegal await-expression in formal parameters of async function",
    ),
    (
        "async (a = ‸await 1) => a",
        "Illegal await-expression in formal parameters of async function",
    ),
    ("x = await‸", "Unexpected end of input"),
    ("await‸: 1", "Unexpected token ':'"),
    ("class C { x = ‸await 1 }", "Unexpected reserved word"),
    ("class C { static { ‸await } }", "Unexpected reserved word"),
    (
        "‸with (a) b;",
        "Strict mode code may not include a with statement",
    ),
    ("var ‸eval;", "Unexpected eval or arguments in strict mode"),
    (
        "delete ‸x;",
        "Delete of an unqualified identifier in strict mode.",
    ),
    ("‸010", "Octal literals are not allowed in strict mode."),
    ("<!-‸- x", "HTML comments are not allowed in modules"),
    ("--‸> x", "HTML comments are not allowed in modules"),
    ("x\n--‸> y", "HTML comments are not allowed in modules"),
    ("new.‸target", "new.target expression is not allowed here"),
    ("‸super.x", "'super' keyword unexpected here"),
    ("‸return", "Illegal return statement"),
    ("import.‸met", "Unexpected identifier 'met'"),
    ("‸import.meta = 1", "Invalid left-hand side in assignment"),
    (
        "‸import.\\u006deta",
        "'import.meta' must not contain escaped characters",
    ),
    ("import‸()", "import() requires a specifier"),
    ("import(x, y, ‸z)", "Unexpected identifier 'z'"),
    ("import(‸...x)", "Unexpected token '...'"),
    ("new ‸import(x)", "Cannot use new with import"),
    (
        "label: ‸function f(){}",
        "In strict mode code, functions can only be declared at top level or inside a block.",
    ),
    (
        "if (1) ‸function f(){}",
        "In strict mode code, functions can only be declared at top level or inside a block.",
    ),
    (
        "‸#x in y",
        "Private field '#x' must be declared in an enclosing class",
    ),
    (
        "class C { #x; m()‸{ #y in this } }",
        "Private field '#y' must be declared in an enclosing class",
    ),
    (
        "export default function f(){} export {‸f as default};",
        "Duplicate export of 'default'",
    ),
    (
        "export {a as default}; var a; export ‸default function(){}",
        "Duplicate export of 'default'",
    ),
    ("a <!-‸- b", "HTML comments are not allowed in modules"),
    ("a\n<!-‸- b", "HTML comments are not allowed in modules"),
    ("x\n--‸>y", "HTML comments are not allowed in modules"),
    ("x\n  --‸> y", "HTML comments are not allowed in modules"),
    ("/* a */ --‸> y", "HTML comments are not allowed in modules"),
    ("/*\n*/ --‸> y", "HTML comments are not allowed in modules"),
    (
        "x = 1 /*\n*/ --‸> y",
        "HTML comments are not allowed in modules",
    ),
    (
        "‸import.meta++",
        "Invalid left-hand side expression in postfix operation",
    ),
    (
        "for (‸import.meta of x);",
        "Invalid left-hand side in for-loop",
    ),
    (
        "[‸import.meta] = x",
        "Invalid destructuring assignment target",
    ),
    ("‸import(x) = 1", "Invalid left-hand side in assignment"),
    ("typeof import‸", "Unexpected end of input"),
    ("import‸", "Unexpected end of input"),
    ("import.‸", "Unexpected end of input"),
    (
        "import a from \"m\"; import {b as ‸a} from \"n\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "export {a as b}; export {‸c as b}; var a, c;",
        "Duplicate export of 'b'",
    ),
    (
        "export default function() {} export default ‸class {}",
        "Identifier '.default' has already been declared",
    ),
    (
        "export default class {} export ‸default 1;",
        "Identifier '.default' has already been declared",
    ),
    (
        "export {x as default} from \"m\"; export ‸default 1;",
        "Duplicate export of 'default'",
    ),
    (
        "export async function f(){} export {‸f};",
        "Duplicate export of 'f'",
    ),
    (
        "export var {a, b: [c]} = x; export {‸c};",
        "Duplicate export of 'c'",
    ),
    (
        "export var {a, b: [c]} = x; export {‸d};",
        "Export 'd' is not defined in module",
    ),
    (
        "export {a}; export {‸b}; var a;",
        "Export 'b' is not defined in module",
    ),
    (
        "export {‸b}; export {a}; var a;",
        "Export 'b' is not defined in module",
    ),
    (
        "export {a as \"x\"}; export {‸b as \"x\"}; var a, b;",
        "Duplicate export of 'x'",
    ),
    (
        "export {\"x\"} from \"m\"; export {‸\"x\"} from \"n\";",
        "Duplicate export of 'x'",
    ),
    (
        "import {‸\"\\uD800\" as a} from \"m\";",
        "Invalid module export name: contains unpaired surrogate",
    ),
    (
        "async function f() { var ‸await; }",
        "Unexpected reserved word",
    ),
    ("function f(‸await) {}", "Unexpected reserved word"),
    (
        "function* g() { yield ‸await; }",
        "Unexpected reserved word",
    ),
    ("x = { ‸await }", "Unexpected reserved word"),
    ("class ‸await {}", "Unexpected reserved word"),
    ("function ‸await() {}", "Unexpected reserved word"),
    ("let [await‸] = x;", "Unexpected token ']'"),
    ("try {} catch (‸await) {}", "Unexpected reserved word"),
    (
        "‸arguments = 1;",
        "Unexpected eval or arguments in strict mode",
    ),
    ("‸eval = 1;", "Unexpected eval or arguments in strict mode"),
    (
        "export default 1; export default ‸function(){}",
        "Identifier '.default' has already been declared",
    ),
    (
        "export default 1; export default ‸class {}",
        "Identifier '.default' has already been declared",
    ),
    (
        "export default class {} export default ‸function(){}",
        "Identifier '.default' has already been declared",
    ),
    (
        "export default 1; export default ‸async function(){}",
        "Identifier '.default' has already been declared",
    ),
    (
        "export default 1; export ‸default function f(){}",
        "Duplicate export of 'default'",
    ),
    (
        "export default 1; export ‸default class C {}",
        "Duplicate export of 'default'",
    ),
    (
        "export default class C {} export ‸default class D {}",
        "Duplicate export of 'default'",
    ),
    (
        "let a; ‸import * as a from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "import * as a from \"m\"; let ‸a;",
        "Identifier 'a' has already been declared",
    ),
    (
        "let a; import {‸a} from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "let a; import {b as ‸a} from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "import a, {‸a} from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    (
        "‸import a, * as a from \"m\";",
        "Identifier 'a' has already been declared",
    ),
    ("import {‸yield} from \"m\";", "Unexpected reserved word"),
    ("import {‸let} from \"m\";", "Unexpected reserved word"),
    (
        "import {‸eval} from \"m\";",
        "Unexpected eval or arguments in strict mode",
    ),
    ("export {‸yield};", "Unexpected reserved word"),
    (
        "export {eval}; var ‸eval;",
        "Unexpected eval or arguments in strict mode",
    ),
    (
        "export {‸arguments};",
        "Export 'arguments' is not defined in module",
    ),
    (
        "import {‸d\\u0065fault} from \"m\";",
        "Unexpected reserved word",
    ),
    ("export {‸d\\u0065fault};", "Unexpected reserved word"),
    (
        "var a; export {a as default}; export {‸a as default};",
        "Duplicate export of 'default'",
    ),
    (
        "export default function f(){} export ‸default function g(){}",
        "Duplicate export of 'default'",
    ),
    (
        "export var a; export {‸b as a}; var b;",
        "Duplicate export of 'a'",
    ),
    (
        "export {b as a}; export ‸var a; var b;",
        "Duplicate export of 'a'",
    ),
    (
        "export {b as a}; export ‸let a; var b;",
        "Duplicate export of 'a'",
    ),
    (
        "export {b as a}; export ‸function a(){} var b;",
        "Duplicate export of 'a'",
    ),
    (
        "export {b as a}; export ‸class a {} var b;",
        "Duplicate export of 'a'",
    ),
    (
        "export {b as a}; export ‸var [a] = []; var b;",
        "Duplicate export of 'a'",
    ),
    (
        "export * as a from \"m\"; export ‸var a;",
        "Duplicate export of 'a'",
    ),
    ("export ‸var a, a;", "Duplicate export of 'a'"),
    ("export ‸var [a, a] = x;", "Duplicate export of 'a'"),
    (
        "export let [a, ‸a] = x;",
        "Identifier 'a' has already been declared",
    ),
    (
        "export default function(){} export {‸x as default}; var x;",
        "Duplicate export of 'default'",
    ),
    (
        "import a from \"m\" with { type: \"json\" } ‸import b from \"n\";",
        "Unexpected token 'import'",
    ),
    ("import \"m\" with‸;", "Unexpected token ';'"),
    (
        "import \"m\" with {a: \"b\" ‸c: \"d\"};",
        "Unexpected identifier 'c'",
    ),
    (
        "import \"m\" with {a: \"b\",‸, c: \"d\"};",
        "Unexpected token ','",
    ),
    ("import \"m\" with {a‸};", "Unexpected token '}'"),
    ("import \"m\" with {a:‸};", "Unexpected token '}'"),
    ("import \"m\" with {‸...a};", "Unexpected token '...'"),
    ("import \"m\" with {‸[a]: \"b\"};", "Unexpected token '['"),
    ("import \"m\" with {a: ‸`b`};", "Unexpected template string"),
    (
        "import \"m\" with {\"a\": \"b\", ‸a: \"c\"};",
        "Import assertion has duplicate key 'a'",
    ),
    (
        "import \"m\" with {a: \"b\", ‸\"a\": \"c\"};",
        "Import assertion has duplicate key 'a'",
    ),
    (
        "export * from \"m\" with { a: \"x\", ‸a: \"y\" };",
        "Import assertion has duplicate key 'a'",
    ),
    (
        "import x from \"m\"; ‸with (a) {}",
        "Strict mode code may not include a with statement",
    ),
    (
        "if (1) {} else import ‸x from \"m\";",
        "Unexpected identifier 'x'",
    ),
    (
        "export * as ‸\"\\uD800\" from \"m\";",
        "Invalid module export name: contains unpaired surrogate",
    ),
    (
        "export {a as ‸\"\\uDC00\"}; var a;",
        "Invalid module export name: contains unpaired surrogate",
    ),
    (
        "({...‸import.meta} = {})",
        "Invalid destructuring assignment target",
    ),
    (
        "[...‸import.meta] = []",
        "Invalid destructuring assignment target",
    ),
    (
        "({a: ‸import.meta} = {})",
        "Invalid destructuring assignment target",
    ),
    (
        "[‸import.meta] = []",
        "Invalid destructuring assignment target",
    ),
    (
        "({...‸import(x)} = {})",
        "Invalid destructuring assignment target",
    ),
    (
        "[...‸import(x)] = []",
        "Invalid destructuring assignment target",
    ),
    (
        "import* ‸\\u0061s self from \"m\";",
        "'as' must not contain escaped characters",
    ),
    (
        "import {} ‸\\u0066rom \"m\";",
        "'from' must not contain escaped characters",
    ),
    (
        "import {a ‸\\u0061s b} from \"m\";",
        "Unexpected identifier 'as'",
    ),
    (
        "export {a ‸\\u0061s b} from \"m\";",
        "Unexpected identifier 'as'",
    ),
    (
        "import a ‸\\u0066rom \"m\";",
        "'from' must not contain escaped characters",
    ),
    (
        "export * ‸\\u0061s b from \"m\";",
        "Unexpected identifier 'as'",
    ),
    (
        "export * ‸\\u0066rom \"m\";",
        "'from' must not contain escaped characters",
    ),
    (
        "export {} ‸\\u0066rom \"m\";",
        "Unexpected identifier 'from'",
    ),
    (
        "var a; export {a ‸\\u0061s b};",
        "Unexpected identifier 'as'",
    ),
    ("var a; export {a as b, a as b‸", "Unexpected end of input"),
    (
        "var a; export {a as b, a as b} from‸",
        "Unexpected end of input",
    ),
    (
        "var a; export {a as b, a as b} ‸x",
        "Unexpected identifier 'x'",
    ),
    (
        "import \"m\" ‸\\u0077ith {};",
        "Keyword must not contain escaped characters",
    ),
];

/// Module sources that V8 accepts.
const MODULE_VALID: &[&str] = &[
    "import a from \"m\";",
    "import a, * as b from \"m\";",
    "import a, {b, c as d} from \"m\";",
    "import {} from \"m\";",
    "import {a,} from \"m\";",
    "import {\"s\" as a} from \"m\";",
    "import {default as a} from \"m\";",
    "import {if as a} from \"m\";",
    "import * as a from \"m\";",
    "import \"m\";",
    "import \"m\" with { type: \"json\" };",
    "import a from \"m\" with { type: \"json\" };",
    "import a from \"m\" with { };",
    "import a from \"m\" with { type: \"json\", };",
    "import a from \"m\" with { if: \"x\" };",
    "import a from \"m\"\nwith { type: \"json\" };",
    "import a from 'm' ;import b from 'n'",
    "var a; export {a};",
    "var a; export {a as b, a as c};",
    "var a; export {a as \"s\"};",
    "export {\"s\"} from \"m\";",
    "export {\"s\" as \"t\"} from \"m\";",
    "export {if} from \"m\";",
    "export * from \"m\";",
    "export * as ns from \"m\";",
    "export * as \"s\" from \"m\";",
    "export * as default from \"m\";",
    "export * from \"m\" with { type: \"json\" };",
    "export {a} from \"m\" with { type: \"json\" };",
    "export var a = 1;",
    "export let a = 1;",
    "export const a = 1;",
    "export function f(){}",
    "export async function f(){}",
    "export function* g(){}",
    "export async function* g(){}",
    "export class C {}",
    "export default function(){}",
    "export default function f(){}",
    "export default function*(){}",
    "export default async function(){}",
    "export default async function*(){}",
    "export default async function f(){}",
    "export default class {}",
    "export default class C {}",
    "export default 1;",
    "export default (function(){});",
    "export default async () => 1;",
    "export default async\nfunction f(){}",
    "export default a = 1;",
    "export default {};",
    "export default [];",
    "export default await 1;",
    "export {x as y}; let x;",
    "await 1;",
    "for await (x of y);",
    "async function f(){ await 1 }",
    "class C { [await 1](){} }",
    "\"@01\"",
    "this",
    "arguments",
    "import.meta",
    "import.meta.url",
    "import(x)",
    "import(x, y)",
    "import(x, y,)",
    "import(x,)",
    "import(x).then",
    "typeof import.meta",
    "import {a} from \"m\"; export {a};",
    "import * as ns from \"m\"; export {ns};",
    "export {x}; import {x} from \"m\";",
    "import a from \"m\"; a = 1;",
    "import a from \"m\"; a++;",
    "import {a} from \"m\"; ({a} = {});",
    "import a from \"m\"; for (a in b);",
    "x-->y",
    "import a from \"m\"; export default a; export {a};",
    "import.meta.url = 1",
    "import.meta()",
    "new import.meta",
    "import.meta`x`",
    "import(x)()",
    "export {default} from \"m\";",
    "export {default as x} from \"m\";",
    "import a from \"\\uD800\";",
    "await\n1",
    "await /x/",
    "const a = await import(\"m\");",
    "label: { await 1; }",
    "if (await x) {}",
    "x = { await: 1 }",
    "x = { await() {} }",
    "x.await",
    "import {static as s} from \"m\";",
    "import {\\u0061} from \"m\";",
    "import {d\\u0065fault as x} from \"m\";",
    "export {d\\u0065fault} from \"m\";",
    "export {a as d\\u0065fault}; var a;",
    "import a from \"m\" with { type: \"json\" }; export {a};",
    "import \"m\" with {};",
    "import \"m\" with {\"a\": \"b\", \"c\": \"d\"};",
    "import \"m\" with {\"\\uD800\": \"b\"};",
    "import \"m\" with {a: \"\\uD800\"};",
    "import x from \"m\"\nwith {}",
    "import {x} from \"m\"; x = 1;",
    "import {x} from \"m\"; x++;",
    "import x from \"m\"; for (x of y);",
    "import(\"m\", {with: {type: \"json\"}})",
    "import.meta.url;\nimport.meta.resolve(\"x\")",
    "a?.import.meta",
    "class C { static { import.meta } }",
    "function f() { import.meta }",
    "() => import.meta",
    "async function f() { await import(\"m\") }",
    "label: import(\"m\")",
    "export {a as b, c as d} from \"m\"; export {e} from \"m\";",
    "export {a as \"b\"}; var a;",
    "export {\"a\" as b} from \"m\";",
    "export {a as \"x\\uD83D\\uDE00\"}; var a;",
];

#[test]
fn module_early_errors_match_v8() {
    check_module_errors(MODULE_ERRORS);
}

#[test]
fn valid_modules_parse() {
    check_module_valid(MODULE_VALID);
}

#[test]
fn module_trees() {
    let cases = [
        ("import a from 'm';", "(import)"),
        ("export var a = 1;", "(export (var (a 1)))"),
        ("export default 1 + 2;", "(export-default (+ 1 2))"),
        (
            "export default function () {}",
            "(export (function *default* ()))",
        ),
        ("export default class {}", "(export (class))"),
        ("export { a as b }; var a;", "(export) (var a)"),
        ("export * as ns from 'm';", "(export)"),
        ("await x;", "(await x)"),
        ("x = import.meta.url;", "(= x (. import.meta url))"),
        (
            "import('m', { with: {} });",
            "(import \"m\" (object (init with (object))))",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(dump_ast(&module(source)), expected, "{source}");
    }
}

#[test]
fn requests_are_unique_with_their_attributes() {
    let script = module(
        "import a from 'm'; import b from 'm'; import c from 'm' with { type: 'json' };
         import d from 'm' with { 'type': 'json' }; export * from 'n' with { b: '2', a: '1' };
         import 'o';",
    );
    let record = script.module.as_ref().expect("a record");
    let requests: Vec<(String, Vec<(String, String)>)> = record
        .requests
        .iter()
        .map(|request| {
            let attributes = request
                .attributes
                .iter()
                .map(|a| {
                    let key = script.ast.string(a.key).to_string_lossy();
                    (key, script.ast.string(a.value).to_string_lossy())
                })
                .collect();
            (
                script.ast.string(request.specifier).to_string_lossy(),
                attributes,
            )
        })
        .collect();
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|&(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    };
    assert_eq!(
        requests,
        vec![
            ("m".to_owned(), vec![]),
            ("m".to_owned(), pairs(&[("type", "json")])),
            // Sorted by key (§16.2.2.4).
            ("n".to_owned(), pairs(&[("a", "1"), ("b", "2")])),
            ("o".to_owned(), vec![]),
        ]
    );
    let entries: Vec<u32> = record.import_entries.iter().map(|e| e.request).collect();
    assert_eq!(entries, [0, 0, 1, 1]);
}

#[test]
fn import_entries() {
    let script = module("import d, * as ns from 'm'; import { a, b as c, 'x y' as e } from 'n';");
    let record = script.module.as_ref().expect("a record");
    let entries: Vec<String> = record
        .import_entries
        .iter()
        .map(|e| {
            let import = match e.import_name {
                ImportName::Name(name) => text(&script, name),
                ImportName::Namespace => "*".to_owned(),
            };
            format!("{} {import} {}", e.request, text(&script, e.local_name))
        })
        .collect();
    assert_eq!(
        entries,
        ["0 default d", "0 * ns", "1 a a", "1 b c", "1 x y e"]
    );
}

/// The export entries of a list as `export import local request`.
fn export_texts(script: &Script, entries: &[ExportEntry]) -> Vec<String> {
    let name = |n: Option<crate::NameId>| n.map_or("-".to_owned(), |n| text(script, n));
    entries
        .iter()
        .map(|e| {
            let import = match e.import_name {
                None => "-".to_owned(),
                Some(ExportImportName::Name(n)) => text(script, n),
                Some(ExportImportName::All) => "all".to_owned(),
                Some(ExportImportName::AllButDefault) => "all-but-default".to_owned(),
            };
            let request = e.request.map_or("-".to_owned(), |r| r.to_string());
            format!(
                "{} {import} {} {request}",
                name(e.export_name),
                name(e.local_name)
            )
        })
        .collect()
}

#[test]
fn export_entries_are_sorted_as_parse_module_says() {
    let script = module(
        "import d, * as ns from 'm'; import { a } from 'n';
         export { d, ns, a as b, v }; var v;
         export * from 'o'; export * as all from 'o'; export { x as y } from 'n';
         export default function () {} export let [p, { q }] = r; export class C {}",
    );
    let record = script.module.as_ref().expect("a record");
    assert_eq!(
        export_texts(&script, &record.local_exports),
        [
            "ns - ns -",
            "v - v -",
            "default - *default* -",
            "p - p -",
            "q - q -",
            "C - C -"
        ]
    );
    assert_eq!(
        export_texts(&script, &record.indirect_exports),
        ["d default - 0", "b a - 1", "all all - 2", "y x - 1"]
    );
    assert_eq!(
        export_texts(&script, &record.star_exports),
        ["- all-but-default - 2"]
    );
}

#[test]
fn top_level_await() {
    assert!(record("await x;").has_top_level_await);
    assert!(record("for await (x of y);").has_top_level_await);
    assert!(record("class C { [await x]() {} }").has_top_level_await);
    assert!(!record("async function f() { await x; }").has_top_level_await);
    assert!(!record("x = async () => await y;").has_top_level_await);
    assert!(!record("x;").has_top_level_await);
}

#[test]
fn module_scope() {
    let script = module(
        "import a from 'm'; import * as ns from 'n'; export let b = a; var c = 1;
         function f() { return a + ns + b + c; } export { f }; this;",
    );
    assert_eq!(
        dump_scopes(&script),
        "function 0 params [] registers 4 captures [a<-import0 ns<-import1] this
  scope 0 Module: a import Import, ns import Import, b cell r0 Let, c cell r1 Var, f cell r2 Function, this r3 This
function 1 f parent 0 params [] registers 0 captures [a<-c0 ns<-c1 b<-r0 c<-r1]
"
    );
    // Imports are read through captures with a TDZ check; the exported
    // and captured bindings are cells.
    assert_eq!(
        dump_references(&script),
        "b@55:=cell0 a@59:cap0! c@66:=cell1 f@91:=cell2 a@104:cap0! ns@108:cap1! b@113:cap2! \
         c@117:cap3 this@136:r3"
    );
}

#[test]
fn module_functions_are_lexical() {
    check_module_errors(&[
        (
            "function f() {} ‸function f() {}",
            "Identifier 'f' has already been declared",
        ),
        (
            "function f() {} var ‸f;",
            "Identifier 'f' has already been declared",
        ),
        (
            "var f; ‸function f() {}",
            "Identifier 'f' has already been declared",
        ),
    ]);
    // Blocks keep the sloppy-free (strict) block semantics.
    check_module_errors(&[(
        "{ function f() {} ‸function f() {} }",
        "Identifier 'f' has already been declared",
    )]);
}

/// Parses `source` and returns the seconds it took.
fn timed(source: &str) -> f64 {
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
fn hostile_wide_modules_are_linear() {
    on_engine_stack(|| {
        let n = 100_000;
        // The top-level bindings of a module are registers of the module
        // function: at most about 64,500.
        let m = 60_000;
        let sources = [
            repeat_indexed("import a{i} from 'm{i}'; ", n),
            repeat_indexed("import { a{i} } from 'm'; ", n),
            format!("import {{ {} }} from 'm';", repeat_indexed("a{i}, ", n)),
            repeat_indexed("import a{i} from 'm' with { type: 'json', k{i}: '' }; ", n),
            repeat_indexed("export var a{i}; ", m),
            format!(
                "export {{ {} }}; var x;",
                repeat_indexed("x as 'n{i}', ", n)
            ),
            format!(
                "{} export {{ {} }};",
                repeat_indexed("var a{i};", m),
                repeat_indexed("a{i}, ", m)
            ),
            repeat_indexed("export * from 'm{i}'; ", n),
            repeat_indexed("export { a as b{i} } from 'm'; ", n),
            // The module function receives each used import as a capture:
            // at most 65,535.
            repeat_indexed("import a{i} from 'm'; ", 60_000)
                + &repeat_indexed("function f{i}() { return a{i}; }", 60_000),
            repeat_indexed("await a{i}; ", n),
        ];
        for source in &sources {
            assert!(timed(source) < 30.0);
        }
    });
}

#[test]
fn hostile_deep_modules() {
    on_engine_stack(|| {
        const N: usize = 100_000;
        let cases = [
            format!("{}x", "await ".repeat(N)),
            format!("{}0{}", "import(".repeat(N), ")".repeat(N)),
            format!("export default {}0{}", "(".repeat(N), ")".repeat(N)),
            format!("{}{}", "{".repeat(N), "}".repeat(N)),
        ];
        for source in &cases {
            let error = parse(source).expect_err("too deep");
            assert_eq!(error.kind, ErrorKind::Range, "{}", &source[..20]);
        }
    });
}
