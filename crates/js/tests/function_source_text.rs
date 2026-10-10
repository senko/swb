//! The source text that `Function.prototype.toString` shows for every kind
//! of function (ECMA-262 §20.2.3.5; M7 feature 2c). The compiler cannot run
//! classes, accessors and `async` functions yet (feature 3), so this test
//! checks the span that the parser gives each function: the text of the
//! span is what `toString` returns. The expected texts are the output of
//! Node.js 22 for `toString` of the same functions (the script of the case
//! is the one that Node ran).

use swb_js_syntax::parse_script;
use swb_js_text::{RecursionBudget, String16};

/// The script; the functions that the expected texts come from are the
/// declarations, expressions, methods and classes in it.
const SCRIPT: &str = "function decl ( a , b ) { return a /* c */ + b; }
var expr = function  named ( ) { };
var anon = function(){};
var arrow = ( a ) =>   a * 2;
var arrow2 = async  x => x;
var o = {
  m ( ) { },
  get  g ( ) { return 1; },
  set  s ( v ) { },
  [ 'comp' + 'uted' ] ( ) { },
  * gen ( ) { },
  async am ( ) { },
  async * ag ( ) { },
  'quoted key' ( ) { },
  42 ( ) { },
  f : function ( ) { },
  a : ( ) => 1,
  get [ 'ck' ] ( ) { return 2; },
};
function* g ( ) { yield 1; }
async function af ( ) { }
async function* agf ( ) { }
class  K  { constructor ( ) { } static  sm ( ) { } get  x ( ) { return 1; } static async * sag ( ) { } #priv ( ) { } static { } }
var KE = class   { };
var KN = class Named extends K { };
var KF = class { f = function ( ) { }; static g = ( ) => 1; };
";

/// Texts that Node prints for `toString` of the functions of [`SCRIPT`].
const EXPECTED: &[&str] = &[
    "function decl ( a , b ) { return a /* c */ + b; }",
    "function  named ( ) { }",
    "function(){}",
    "( a ) =>   a * 2",
    "async  x => x",
    "m ( ) { }",
    "get  g ( ) { return 1; }",
    "set  s ( v ) { }",
    "[ 'comp' + 'uted' ] ( ) { }",
    "* gen ( ) { }",
    "async am ( ) { }",
    "async * ag ( ) { }",
    "'quoted key' ( ) { }",
    "42 ( ) { }",
    "function ( ) { }",
    "( ) => 1",
    "get [ 'ck' ] ( ) { return 2; }",
    "function* g ( ) { yield 1; }",
    "async function af ( ) { }",
    "async function* agf ( ) { }",
    "class  K  { constructor ( ) { } static  sm ( ) { } get  x ( ) { return 1; } static async * sag ( ) { } #priv ( ) { } static { } }",
    "sm ( ) { }",
    "get  x ( ) { return 1; }",
    "async * sag ( ) { }",
    "class   { }",
    "class Named extends K { }",
    "class { f = function ( ) { }; static g = ( ) => 1; }",
    "function ( ) { }",
    "( ) => 1",
];

/// The texts of the spans of all the functions and classes of a script (the
/// source text of a class constructor is the whole class, §15.7.14).
fn function_texts(source: &str) -> Vec<String> {
    let text = String16::from(source);
    let mut budget = RecursionBudget::default();
    let script = parse_script(text.as_str16(), &mut budget).expect("the script parses");
    let spans = script
        .ast
        .function_ids()
        .map(|id| script.ast.function(id).span)
        .chain(script.ast.class_ids().map(|id| script.ast.class(id).span));
    spans
        .filter_map(|span| {
            text.as_str16()
                .slice(span.start as usize, span.end as usize)
                .map(swb_js_text::Str16::to_string_lossy)
        })
        .collect()
}

#[test]
fn spans_are_the_source_text_of_the_functions() {
    let texts = function_texts(SCRIPT);
    let missing: Vec<&&str> = EXPECTED
        .iter()
        .filter(|expected| !texts.iter().any(|text| text == **expected))
        .collect();
    assert!(
        missing.is_empty(),
        "no function has the text {missing:?}; the spans are {texts:#?}"
    );
}

#[test]
fn a_method_does_not_include_static() {
    let texts = function_texts("class A { static m ( ) { } static get g ( ) { return 1; } }");
    assert!(texts.iter().any(|t| t == "m ( ) { }"), "{texts:?}");
    assert!(
        texts.iter().any(|t| t == "get g ( ) { return 1; }"),
        "{texts:?}"
    );
}

#[test]
fn a_computed_key_is_part_of_the_text() {
    let texts =
        function_texts("var o = { [ a + b ] ( x ) { }, async [ c ] ( ) { }, * [ d ] ( ) { } };");
    for expected in [
        "[ a + b ] ( x ) { }",
        "async [ c ] ( ) { }",
        "* [ d ] ( ) { }",
    ] {
        assert!(texts.iter().any(|t| t == expected), "{expected}: {texts:?}");
    }
}
