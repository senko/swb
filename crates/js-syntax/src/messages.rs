//! The error messages of the lexer and the parser. The wording is V8's,
//! measured in Node.js 22 (`new vm.Script(source)`), so that scripts that
//! inspect `error.message` see what they see in Chromium. The four `pub`
//! constants are also used by the compiler and the interpreter, so that
//! each text exists once.

pub const NOT_SUPPORTED: &str = "not supported yet";
pub const STACK_OVERFLOW: &str = "Maximum call stack size exceeded";
pub(crate) const SOURCE_TOO_LONG: &str = "Script is too large";

/// Also the error for a template without its end (Node.js 22).
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
pub(crate) const ESCAPED_NEW_TARGET: &str = "'new.target' must not contain escaped characters";

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
pub(crate) const INVALID_SHORTHAND_INITIALIZER: &str = "Invalid shorthand property initializer";
pub(crate) const REST_NOT_LAST: &str = "Rest element must be last element";
pub(crate) const REST_NOT_ASSIGNABLE: &str =
    "`...` must be followed by an assignable reference in assignment contexts";
pub(crate) const REST_NOT_IDENTIFIER: &str =
    "`...` must be followed by an identifier in declaration contexts";
pub(crate) const REST_PARAMETER_NOT_LAST: &str = "Rest parameter must be last formal parameter";
pub(crate) const REST_PARAMETER_DEFAULT: &str = "Rest parameter may not have a default initializer";
pub(crate) const MISSING_DESTRUCTURING_INITIALIZER: &str =
    "Missing initializer in destructuring declaration";
pub(crate) const PROPERTY_IN_DECLARATION: &str = "Illegal property in declaration context";
pub(crate) const YIELD_IN_PARAMETER: &str = "Yield expression not allowed in formal parameter";
pub(crate) const USE_STRICT_NON_SIMPLE: &str =
    "Illegal 'use strict' directive in function with non-simple parameter list";
pub(crate) const GETTER_PARAMETERS: &str = "Getter must not have any formal parameters.";
pub(crate) const SETTER_PARAMETERS: &str = "Setter must have exactly one formal parameter.";
pub(crate) const SETTER_REST: &str = "Setter function argument must not be a rest parameter";
pub(crate) const FOR_IN_SINGLE_BINDING: &str =
    "Invalid left-hand side in for-in loop: Must have a single binding.";
pub(crate) const FOR_OF_SINGLE_BINDING: &str =
    "Invalid left-hand side in for-of loop: Must have a single binding.";
pub(crate) const FOR_IN_INITIALIZER: &str =
    "for-in loop variable declaration may not have an initializer.";
pub(crate) const FOR_OF_INITIALIZER: &str =
    "for-of loop variable declaration may not have an initializer.";
pub(crate) const INVALID_FOR_TARGET: &str = "Invalid left-hand side in for-loop";
pub(crate) const FOR_OF_LET: &str = "The left-hand side of a for-of loop may not start with 'let'.";
pub(crate) const FOR_OF_ASYNC: &str = "The left-hand side of a for-of loop may not be 'async'.";
pub(crate) const OPTIONAL_CHAIN_TEMPLATE: &str = "Invalid tagged template on optional chain";
pub(crate) const OPTIONAL_CHAIN_NEW: &str = "Invalid optional chain from new expression";
pub(crate) const NEW_TARGET_OUTSIDE_FUNCTION: &str = "new.target expression is not allowed here";
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

pub(crate) const ASYNC_FUNCTION_IN_STATEMENT: &str =
    "Async functions can only be declared at the top level or inside a block.";
pub(crate) const AWAIT_IN_PARAMETER: &str =
    "Illegal await-expression in formal parameters of async function";
pub(crate) const AWAIT_BINDING_IN_ASYNC: &str =
    "'await' is not a valid identifier name in an async function";
pub(crate) const FOR_AWAIT_SINGLE_BINDING: &str =
    "Invalid left-hand side in for-await-of loop: Must have a single binding.";
pub(crate) const FOR_AWAIT_INITIALIZER: &str =
    "for-await-of loop variable declaration may not have an initializer.";
pub(crate) const UNEXPECTED_SUPER: &str = "'super' keyword unexpected here";
pub(crate) const UNEXPECTED_PRIVATE_FIELD: &str = "Unexpected private field";
pub(crate) const DELETE_PRIVATE: &str = "Private fields can not be deleted";
pub(crate) const ARGUMENTS_IN_CLASS_INIT: &str =
    "'arguments' is not allowed in class field initializer or static initialization block";
pub(crate) const DUPLICATE_CONSTRUCTOR: &str = "A class may only have one constructor";
pub(crate) const STATIC_PROTOTYPE: &str =
    "Classes may not have a static property named 'prototype'";
pub(crate) const CONSTRUCTOR_FIELD: &str = "Classes may not have a field named 'constructor'";
pub(crate) const CONSTRUCTOR_ACCESSOR: &str = "Class constructor may not be an accessor";
pub(crate) const CONSTRUCTOR_GENERATOR: &str = "Class constructor may not be a generator";
pub(crate) const CONSTRUCTOR_ASYNC: &str = "Class constructor may not be an async method";
pub(crate) const CONSTRUCTOR_PRIVATE: &str = "Class constructor may not be a private method";

pub(crate) const STRICT_OCTAL_LITERAL: &str = "Octal literals are not allowed in strict mode.";
pub(crate) const STRICT_LEADING_ZERO: &str =
    "Decimals with leading zeros are not allowed in strict mode.";
pub(crate) const STRICT_OCTAL_ESCAPE: &str =
    "Octal escape sequences are not allowed in strict mode.";
pub(crate) const STRICT_EIGHT_OR_NINE: &str = "\\8 and \\9 are not allowed in strict mode.";

/// Not measured: V8 allows about 4 million variables per function; swb
/// has 16-bit register operands (ADR 0026 section 4).
pub const TOO_MANY_VARIABLES: &str = "Too many variables declared in a function";
/// Not measured: V8 has no such limit. A `RangeError` in swb.
pub(crate) const TOO_MANY_CAPTURES: &str = "Too many captured variables in a script";
pub(crate) const TOO_MANY_PARAMETERS: &str =
    "Too many parameters in function definition (only 65534 allowed)";
pub const TOO_MANY_ARGUMENTS: &str = "Too many arguments in function call (only 65535 allowed)";

// Modules and `import` (measured with `node --check` on `.mjs` files).
pub(crate) const IMPORT_OUTSIDE_MODULE: &str = "Cannot use import statement outside a module";
pub(crate) const IMPORT_META_OUTSIDE_MODULE: &str = "Cannot use 'import.meta' outside a module";
pub(crate) const ESCAPED_IMPORT_META: &str = "'import.meta' must not contain escaped characters";
pub(crate) const IMPORT_CALL_SPECIFIER: &str = "import() requires a specifier";
pub(crate) const NEW_IMPORT: &str = "Cannot use new with import";
pub(crate) const HTML_COMMENT_IN_MODULE: &str = "HTML comments are not allowed in modules";
pub(crate) const UNPAIRED_SURROGATE_EXPORT_NAME: &str =
    "Invalid module export name: contains unpaired surrogate";
pub(crate) const STRING_EXPORT_WITHOUT_FROM: &str =
    "String literal module export names must be followed by a 'from' clause";
/// V8 calls the binding of `export default` `.default`.
pub(crate) const DEFAULT_EXPORT_REDECLARED: &str =
    "Identifier '.default' has already been declared";

// Lexical errors.
pub(crate) const INVALID_TOKEN: &str = "Invalid or unexpected token";
pub(crate) const UNTERMINATED_REGEXP: &str = "Invalid regular expression: missing /";
pub(crate) const INVALID_HEX_ESCAPE: &str = "Invalid hexadecimal escape sequence";
pub(crate) const INVALID_UNICODE_ESCAPE: &str = "Invalid Unicode escape sequence";
pub(crate) const UNDEFINED_CODE_POINT: &str = "Undefined Unicode code-point";
pub(crate) const OCTAL_IN_TEMPLATE: &str =
    "Octal escape sequences are not allowed in template strings.";
pub(crate) const EIGHT_NINE_IN_TEMPLATE: &str = "\\8 and \\9 are not allowed in template strings.";
pub(crate) const SEPARATOR_AFTER_ZERO: &str = "Numeric separator can not be used after leading 0.";
pub(crate) const SEPARATOR_TWICE: &str = "Only one underscore is allowed as numeric separator";
pub(crate) const SEPARATOR_AT_END: &str =
    "Numeric separators are not allowed at the end of numeric literals";

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

/// "Private field '#x' must be declared in an enclosing class" (`name`
/// includes the `#`).
pub(crate) fn undeclared_private(name: &str) -> String {
    format!("Private field '{name}' must be declared in an enclosing class")
}

/// "Import assertion has duplicate key 'x'" (V8's wording from before
/// import attributes).
pub(crate) fn duplicate_import_attribute(key: &str) -> String {
    format!("Import assertion has duplicate key '{key}'")
}

/// "Duplicate export of 'x'".
pub(crate) fn duplicate_export(name: &str) -> String {
    format!("Duplicate export of '{name}'")
}

/// "Export 'x' is not defined in module".
pub(crate) fn undefined_export(name: &str) -> String {
    format!("Export '{name}' is not defined in module")
}

/// "'x' must not contain escaped characters" (a contextual keyword of the
/// module grammar, such as `from` or `as`).
pub(crate) fn escaped_contextual_keyword(name: &str) -> String {
    format!("'{name}' must not contain escaped characters")
}

/// "Unexpected token 'x'".
pub(crate) fn unexpected_token(text: &str) -> String {
    format!("Unexpected token '{text}'")
}

/// "Unexpected identifier 'x'".
pub(crate) fn unexpected_identifier(name: &str) -> String {
    format!("Unexpected identifier '{name}'")
}
