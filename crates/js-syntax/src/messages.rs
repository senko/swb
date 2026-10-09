//! The error messages of the parser. The wording is V8's, measured in
//! Node.js 22 (`new vm.Script(source)`), so that scripts that inspect
//! `error.message` see what they see in Chromium.

pub(crate) const NOT_SUPPORTED: &str = "not supported yet";
pub(crate) const STACK_OVERFLOW: &str = "Maximum call stack size exceeded";
pub(crate) const SOURCE_TOO_LONG: &str = "Script is too large";

pub(crate) const UNEXPECTED_END: &str = "Unexpected end of input";
pub(crate) const UNEXPECTED_NUMBER: &str = "Unexpected number";
pub(crate) const UNEXPECTED_STRING: &str = "Unexpected string";
pub(crate) const UNEXPECTED_TEMPLATE: &str = "Unexpected template string";
pub(crate) const UNEXPECTED_RESERVED: &str = "Unexpected reserved word";
pub(crate) const UNEXPECTED_STRICT_RESERVED: &str = "Unexpected strict mode reserved word";
pub(crate) const UNEXPECTED_EVAL_OR_ARGUMENTS: &str = "Unexpected eval or arguments in strict mode";
/// `await` is an identifier in a script; V8 explains the likely mistake
/// when a statement continues after it.
pub(crate) const AWAIT_OUTSIDE_ASYNC: &str =
    "await is only valid in async functions and the top level bodies of modules";
pub(crate) const ESCAPED_KEYWORD: &str = "Keyword must not contain escaped characters";

pub(crate) const MISSING_CONST_INITIALIZER: &str = "Missing initializer in const declaration";
pub(crate) const LET_IN_LEXICAL: &str = "let is disallowed as a lexically bound name";
pub(crate) const LEXICAL_IN_STATEMENT: &str =
    "Lexical declaration cannot appear in a single-statement context";
pub(crate) const STRICT_FUNCTION: &str =
    "In strict mode code, functions can only be declared at top level or inside a block.";
pub(crate) const SLOPPY_FUNCTION: &str = "In non-strict mode code, functions can only be declared at top level, inside a block, or as the body of an if statement.";
pub(crate) const GENERATOR_IN_STATEMENT: &str =
    "Generators can only be declared at the top level or inside a block.";
pub(crate) const FUNCTION_NAME_REQUIRED: &str = "Function statements require a function name";
pub(crate) const DUPLICATE_PARAMETER: &str = "Duplicate parameter name not allowed in this context";

pub(crate) const INVALID_ASSIGNMENT_TARGET: &str = "Invalid left-hand side in assignment";
pub(crate) const INVALID_PREFIX_TARGET: &str =
    "Invalid left-hand side expression in prefix operation";
pub(crate) const INVALID_POSTFIX_TARGET: &str =
    "Invalid left-hand side expression in postfix operation";
pub(crate) const INVALID_DESTRUCTURING_TARGET: &str = "Invalid destructuring assignment target";
pub(crate) const MALFORMED_ARROW_PARAMETERS: &str = "Malformed arrow function parameter list";
pub(crate) const DUPLICATE_PROTO: &str =
    "Duplicate __proto__ fields are not allowed in object literals";
pub(crate) const UNARY_BEFORE_EXPONENT: &str = "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence";
pub(crate) const DELETE_IDENTIFIER: &str = "Delete of an unqualified identifier in strict mode.";
pub(crate) const MISSING_PAREN_AFTER_ARGUMENTS: &str = "missing ) after argument list";
pub(crate) const MISSING_TEMPLATE_BRACE: &str = "Missing } in template expression";

pub(crate) const ILLEGAL_BREAK: &str = "Illegal break statement";
pub(crate) const ILLEGAL_CONTINUE: &str =
    "Illegal continue statement: no surrounding iteration statement";
pub(crate) const ILLEGAL_RETURN: &str = "Illegal return statement";
pub(crate) const NEWLINE_AFTER_THROW: &str = "Illegal newline after throw";
pub(crate) const MISSING_CATCH_OR_FINALLY: &str = "Missing catch or finally after try";
pub(crate) const MULTIPLE_DEFAULTS: &str = "More than one default clause in switch statement";
pub(crate) const STRICT_WITH: &str = "Strict mode code may not include a with statement";

pub(crate) const STRICT_OCTAL_LITERAL: &str = "Octal literals are not allowed in strict mode.";
pub(crate) const STRICT_LEADING_ZERO: &str =
    "Decimals with leading zeros are not allowed in strict mode.";
pub(crate) const STRICT_OCTAL_ESCAPE: &str =
    "Octal escape sequences are not allowed in strict mode.";
pub(crate) const STRICT_EIGHT_OR_NINE: &str = "\\8 and \\9 are not allowed in strict mode.";

/// Not measured: V8 allows about 4 million variables per function; swb
/// has 16-bit register operands (ADR 0026 section 4).
pub(crate) const TOO_MANY_VARIABLES: &str = "Too many variables declared in a function";
/// Not measured: V8 has no such limit. A `RangeError` in swb.
pub(crate) const TOO_MANY_CAPTURES: &str = "Too many captured variables in a script";
pub(crate) const TOO_MANY_PARAMETERS: &str =
    "Too many parameters in function definition (only 65534 allowed)";
pub(crate) const TOO_MANY_ARGUMENTS: &str =
    "Too many arguments in function call (only 65535 allowed)";

/// "Identifier 'x' has already been declared".
pub(crate) fn already_declared(name: &str) -> String {
    format!("Identifier '{name}' has already been declared")
}

/// "Label 'x' has already been declared".
pub(crate) fn label_redeclared(name: &str) -> String {
    format!("Label '{name}' has already been declared")
}

/// "Undefined label 'x'".
pub(crate) fn undefined_label(name: &str) -> String {
    format!("Undefined label '{name}'")
}

/// "Illegal continue statement: 'x' does not denote an iteration statement".
pub(crate) fn continue_not_loop(name: &str) -> String {
    format!("Illegal continue statement: '{name}' does not denote an iteration statement")
}

/// "Unexpected token 'x'".
pub(crate) fn unexpected_token(text: &str) -> String {
    format!("Unexpected token '{text}'")
}

/// "Unexpected identifier 'x'".
pub(crate) fn unexpected_identifier(name: &str) -> String {
    format!("Unexpected identifier '{name}'")
}
