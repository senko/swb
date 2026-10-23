//! Tests of the source of `Function` constructor calls
//! (§20.2.1.1.1 `CreateDynamicFunction`, [`super::parse_dynamic_function`]):
//! the parameters and the body cannot reach into each other, with V8's
//! messages (measured in Node.js 22), and the function has no binding of
//! its name.

use swb_js_text::{RecursionBudget, Str16};

use crate::ast::{ExprKind, StmtKind};
use crate::error::ParseError;
use crate::parser::{Script, parse_dynamic_function};

/// Parses the source that the `Function` constructor builds from `params`
/// and `body`.
fn parse(params: &str, body: &str) -> Result<Script, ParseError> {
    let prefix = "(function anonymous(";
    let source = format!("{prefix}{params}\n) {{\n{body}\n}})");
    let units: Vec<u16> = source.encode_utf16().collect();
    let params_end = (prefix.encode_utf16().count() + params.encode_utf16().count() + 1) as u32;
    let mut budget = RecursionBudget::DEFAULT;
    let result = parse_dynamic_function(Str16::Wide(&units), params_end, &mut budget);
    assert_eq!(budget, RecursionBudget::DEFAULT, "the budget comes back");
    result
}

fn message(params: &str, body: &str) -> String {
    match parse(params, body) {
        Ok(_) => "ok".to_owned(),
        Err(error) => error.message.into_owned(),
    }
}

#[test]
fn valid_sources_give_one_function_without_a_name_binding() {
    for (params, body) in [
        ("", ""),
        ("a, b", "return a + b"),
        ("a = 1, {b}, ...c", "return [a, b, c]"),
        ("a //", "return a"),
        ("a", "return a //"),
        ("a", "-->\nreturn a"),
        ("\n-->", ""),
        ("<!--", ""),
        ("a,", "with (a) { return b }"),
        ("yield, await", "return yield + await"),
        ("a", "return function anonymous() { return anonymous }"),
    ] {
        let script = parse(params, body).unwrap_or_else(|e| panic!("{params} / {body}: {e}"));
        let function = script.dynamic_function.expect("the function");
        assert!(script.ast.function(function).name.is_none(), "{params}");
        let [statement] = script.ast.stmts(script.ast.function(script.top).body) else {
            panic!("one statement");
        };
        let StmtKind::Expr(expr) = script.ast.stmt(*statement).kind else {
            panic!("an expression statement");
        };
        assert!(matches!(script.ast.expr(expr).kind, ExprKind::Paren(_)));
    }
    // `anonymous` is not bound: it resolves to a global.
    let script = parse("", "return anonymous").unwrap_or_else(|e| panic!("{e}"));
    let bound = script
        .scopes
        .reference_ids()
        .filter(|&id| script.name_text(script.scopes.reference(id).name) == "anonymous")
        .any(|id| script.scopes.reference(id).binding.is_some());
    assert!(!bound);
}

#[test]
fn parts_that_reach_into_each_other_are_errors_with_v8_messages() {
    let cases = [
        ("/*", "*/){", "Unexpected end of arg string"),
        ("a, /*", "*/ b", "Unexpected end of arg string"),
        ("a = `", "`){", "Unexpected end of arg string"),
        ("a)", "return 1", "Arg string terminates parameters early"),
        (")", "", "Arg string terminates parameters early"),
        ("a=1)", "{", "Arg string terminates parameters early"),
        ("a b", "", "Arg string terminates parameters early"),
        ("a", "})(function(){", "Single function literal required"),
        ("a", "}); (function(){", "Single function literal required"),
        ("a", "return )", "Unexpected token ')'"),
        ("a", "}", "Unexpected token '}'"),
        (
            "a",
            "return 1 }, function() {",
            "Single function literal required",
        ),
        (
            "a",
            "return 1 } function x() {",
            "Unexpected token 'function'",
        ),
        ("a", "}\n{", "Unexpected token '{'"),
        ("a =", "", "Unexpected token ')'"),
        (",a", "", "Unexpected token ','"),
        ("-->", "", "Unexpected token '--'"),
        ("a", "/*", "Invalid or unexpected token"),
        ("`", "`", "Unexpected template string"),
        ("a", "super.x", "'super' keyword unexpected here"),
        ("a", "let a", "Identifier 'a' has already been declared"),
        (
            "a = 1",
            "'use strict'",
            "Illegal 'use strict' directive in function with non-simple parameter list",
        ),
    ];
    for (params, body, expected) in cases {
        assert_eq!(message(params, body), expected, "{params} / {body}");
    }
}

#[test]
fn a_huge_parameter_list_stops_at_the_limit() {
    let params = "a,".repeat(200_000) + "z";
    assert_eq!(
        message(&params, ""),
        "Too many parameters in function definition (only 65534 allowed)"
    );
}

/// Parses the source of an event handler `onclick` with `params` and
/// `body` (`parse_event_handler`).
fn parse_handler(params: &str, body: &str) -> Result<Script, ParseError> {
    let prefix = "(function onclick(";
    let source = format!("{prefix}{params}) {{\n{body}\n}})");
    let units: Vec<u16> = source.encode_utf16().collect();
    let params_end = (prefix.encode_utf16().count() + params.encode_utf16().count()) as u32;
    let mut budget = RecursionBudget::DEFAULT;
    let result = crate::parser::parse_event_handler(Str16::Wide(&units), params_end, &mut budget);
    assert_eq!(budget, RecursionBudget::DEFAULT, "the budget comes back");
    result
}

#[test]
fn event_handler_names_resolve_against_the_outer_environment() {
    let script =
        parse_handler("event", "return [event, onclick, value]").unwrap_or_else(|e| panic!("{e}"));
    let function = script.dynamic_function.expect("the function");
    assert!(script.ast.function(function).name.is_none());
    let resolution = |name: &str| {
        script
            .scopes
            .reference_ids()
            .find(|&id| script.name_text(script.scopes.reference(id).name) == name)
            .map(|id| script.scopes.reference(id).resolution)
    };
    // The parameter is local; the handler's own name and the other free
    // names resolve outside (the element's `onclick`, the form's `value`).
    assert_ne!(resolution("event"), Some(crate::Resolution::Caller));
    assert_eq!(resolution("onclick"), Some(crate::Resolution::Caller));
    assert_eq!(resolution("value"), Some(crate::Resolution::Caller));
    for (body, expected) in [
        ("}", "Unexpected token '}'"),
        ("}); alert(1); ({", "Single function literal required"),
        ("return )", "Unexpected token ')'"),
        ("let event", "Identifier 'event' has already been declared"),
        ("return yield + await", "ok"),
    ] {
        let message = match parse_handler("event", body) {
            Ok(_) => "ok".to_owned(),
            Err(error) => error.message.into_owned(),
        };
        assert_eq!(message, expected, "{body}");
    }
    assert!(parse_handler("event", "'use strict'; return yield").is_err());
}
