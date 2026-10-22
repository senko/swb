//! `BigInt` literals in the compiler: the value of a literal
//! (§12.9.3.1, <https://tc39.es/ecma262/#sec-static-semantics-mv>), its
//! text as a property key (`{1n: x}`, `class { 1n() {} }`: the decimal
//! text of the value, §7.1.19 `ToPropertyKey`), and the check of their
//! size before the compile.
//!
//! The lexer keeps the digits of a literal with their radix prefix and
//! without separators; `js-text` computes the value. A literal above the
//! size limit is a `SyntaxError` ("Maximum `BigInt` size exceeded") at compile
//! time, as in V8 (measured in Chromium 148, also in a function that never
//! runs and as a property key), with swb's lower limit (ADR 0026
//! section 15).

use swb_js_syntax::{ClassKey, ExprKind, PatternKind, PropertyKey, Script};
use swb_js_text::String16;
use swb_js_text::bigint::{self, BigInt, BigIntError, MAX_BITS};

/// V8's message for a literal above the size limit.
pub(super) const TOO_LARGE: &str = "Maximum BigInt size exceeded";

/// The shortest literal text whose value can exceed the limit: `0x` and
/// one hexadecimal digit more than `MAX_BITS / 4` (the other radixes need
/// more digits). Shorter literals are not parsed by the size check.
const SHORTEST_OVERSIZED: usize = (MAX_BITS / 4) as usize + 3;

/// The value of a literal's text (the digits with the radix prefix).
pub(super) fn literal_value(text: &String16) -> Result<BigInt, BigIntError> {
    bigint::parse_literal(text.as_str16())
}

/// The key text of a literal used as a property name: its decimal value.
/// `None` only for a literal above the limit (rejected before the compile)
/// or a failed allocation.
pub(super) fn key_text(text: &String16) -> Option<String16> {
    let value = literal_value(text).ok()?;
    let digits = bigint::to_string_radix(&value, 10).ok()?;
    Some(String16::from_latin1(digits))
}

/// Whether a literal's value is above the size limit.
fn oversized(text: &String16) -> bool {
    text.len() >= SHORTEST_OVERSIZED && matches!(literal_value(text), Err(BigIntError::TooLarge))
}

/// The source offset of the first literal above the size limit (an
/// expression, a key of an object literal, a class element or an object
/// pattern), if any.
pub(super) fn first_oversized(script: &Script) -> Option<u32> {
    let ast = &script.ast;
    let mut first: Option<u32> = None;
    let mut check = |offset: u32, text: &String16| {
        if first.is_none_or(|at| offset < at) && oversized(text) {
            first = Some(offset);
        }
    };
    for id in ast.expr_ids() {
        let expr = ast.expr(id);
        match expr.kind {
            ExprKind::BigInt(digits) => check(expr.span.start, ast.string(digits)),
            ExprKind::Object(properties) => {
                for property in ast.properties(properties) {
                    if let PropertyKey::BigInt(digits) = property.key {
                        check(property.span.start, ast.string(digits));
                    }
                }
            }
            _ => {}
        }
    }
    for id in ast.class_ids() {
        for element in ast.class_elements(ast.class(id).elements) {
            if let ClassKey::Property(PropertyKey::BigInt(digits)) = element.key {
                check(element.span.start, ast.string(digits));
            }
        }
    }
    for id in ast.pattern_ids() {
        if let PatternKind::Object { properties, .. } = ast.pattern(id).kind {
            for property in ast.pattern_properties(properties) {
                if let PropertyKey::BigInt(digits) = property.key {
                    check(property.span.start, ast.string(digits));
                }
            }
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_decimal_texts() {
        let key = |text: &str| key_text(&String16::from(text)).map(|k| k.to_string_lossy());
        assert_eq!(key("0x10").as_deref(), Some("16"));
        assert_eq!(key("0b101").as_deref(), Some("5"));
        assert_eq!(key("0O17").as_deref(), Some("15"));
        assert_eq!(
            key("18446744073709551616").as_deref(),
            Some("18446744073709551616")
        );
        assert_eq!(key("0").as_deref(), Some("0"));
    }

    #[test]
    fn the_size_check_is_exact_at_the_limit() {
        let hex = |digits: usize, first: char| {
            let mut text = String::from("0x");
            text.push(first);
            text.extend(std::iter::repeat_n('f', digits - 1));
            String16::from(text.as_str())
        };
        let limit = (MAX_BITS / 4) as usize;
        assert!(!oversized(&hex(limit, 'f')));
        assert!(oversized(&hex(limit + 1, '1')));
        // Leading zeros do not count.
        assert!(!oversized(&hex(limit + 1, '0')));
        assert!(!oversized(&String16::from("12345")));
    }
}
