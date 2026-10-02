//! Serialization of component values back to CSS text.
//!
//! <https://www.w3.org/TR/css-syntax-3/#serialization> requires only that
//! the output parses back to the same component values (whitespace runs may
//! collapse). Between two values that would otherwise merge into a different
//! token, the serializer inserts an empty comment `/**/`, as the
//! specification's table describes.
//!
//! Identifier and string escaping follows CSSOM:
//! <https://drafts.csswg.org/cssom/#common-serializing-idioms>.

use std::fmt::{self, Write as _};

use crate::tokenizer::Number;
use crate::values::{BlockKind, ComponentValue};

/// Serializes a list of component values.
pub fn serialize_component_values(values: &[ComponentValue]) -> String {
    let mut out = String::new();
    write_values(values, &mut out);
    out
}

/// Writes `ident` as a CSS identifier, escaping where needed.
///
/// <https://drafts.csswg.org/cssom/#serialize-an-identifier>
pub fn serialize_identifier(ident: &str, out: &mut String) {
    if ident == "-" {
        out.push_str("\\-");
        return;
    }
    let starts_with_dash = ident.starts_with('-');
    for (i, c) in ident.chars().enumerate() {
        match c {
            '0'..='9' if i == 0 || (i == 1 && starts_with_dash) => escape_code_point(c, out),
            _ => write_name_char(c, out),
        }
    }
}

/// Writes `s` as a quoted CSS string.
///
/// <https://drafts.csswg.org/cssom/#serialize-a-string>
pub fn serialize_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\0' => out.push('\u{FFFD}'),
            '\u{1}'..='\u{1f}' | '\u{7f}' => escape_code_point(c, out),
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Writes the characters of a name (an identifier without the rules for
/// the first characters).
fn serialize_name(name: &str, out: &mut String) {
    for c in name.chars() {
        write_name_char(c, out);
    }
}

fn write_name_char(c: char, out: &mut String) {
    match c {
        '\0' => out.push('\u{FFFD}'),
        '\u{1}'..='\u{1f}' | '\u{7f}' => escape_code_point(c, out),
        c if c >= '\u{80}' || c == '-' || c == '_' || c.is_ascii_alphanumeric() => out.push(c),
        c => {
            out.push('\\');
            out.push(c);
        }
    }
}

fn escape_code_point(c: char, out: &mut String) {
    let _ = write!(out, "\\{:x} ", u32::from(c));
}

/// The parts of the serialization table that a value takes part in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Ident,
    Function,
    Url,
    BadUrl,
    Delim(char),
    Number,
    Percentage,
    Dimension,
    Cdc,
    OpenParen,
    AtKeyword,
    Hash,
    Other,
}

/// The kinds of the first and the last token of a value's serialization.
fn kinds(value: &ComponentValue) -> (Kind, Kind) {
    let same = |k| (k, k);
    match value {
        ComponentValue::Ident(_) => same(Kind::Ident),
        ComponentValue::AtKeyword(_) => same(Kind::AtKeyword),
        ComponentValue::Hash { .. } => same(Kind::Hash),
        ComponentValue::Url(_) => same(Kind::Url),
        ComponentValue::BadUrl => same(Kind::BadUrl),
        ComponentValue::Delim(c) => same(Kind::Delim(*c)),
        ComponentValue::Number(_) => same(Kind::Number),
        ComponentValue::Percentage(_) => same(Kind::Percentage),
        ComponentValue::Dimension { .. } => same(Kind::Dimension),
        // `U+...` starts like an identifier and ends like a dimension.
        ComponentValue::UnicodeRange { .. } => (Kind::Ident, Kind::Dimension),
        ComponentValue::Cdc => same(Kind::Cdc),
        ComponentValue::Function(_) => (Kind::Function, Kind::Other),
        ComponentValue::Block(b) if b.kind == BlockKind::Paren => (Kind::OpenParen, Kind::Other),
        _ => same(Kind::Other),
    }
}

/// True if `prev` followed by `next` needs a comment between them.
///
/// <https://www.w3.org/TR/css-syntax-3/#serialization> (the table).
fn needs_comment(prev: Kind, next: Kind) -> bool {
    use Kind::{
        AtKeyword, BadUrl, Cdc, Delim, Dimension, Function, Hash, Ident, Number, OpenParen,
        Percentage, Url,
    };
    let identish = matches!(next, Ident | Function | Url | BadUrl | Delim('-'));
    let numeric = matches!(next, Number | Percentage | Dimension);
    match prev {
        Ident => identish || numeric || matches!(next, Cdc | OpenParen),
        AtKeyword | Hash | Dimension => identish || numeric || next == Cdc,
        Delim('#' | '-') => identish || numeric,
        Number => matches!(next, Ident | Function | Url | BadUrl | Cdc | Delim('%')) || numeric,
        Delim('@') => identish || next == Cdc,
        Delim('.' | '+') => numeric,
        Delim('/') => next == Delim('*'),
        _ => false,
    }
}

pub(crate) fn write_values(values: &[ComponentValue], out: &mut String) {
    let mut prev = Kind::Other;
    let mut prev_value = None;
    for value in values {
        let (first, last) = kinds(value);
        if needs_comment(prev, first) || forms_cdo_or_cdc(prev_value, value) {
            out.push_str("/**/");
        }
        write_value(value, out);
        prev = last;
        prev_value = Some(value);
    }
}

/// The identifier `--` merges with a following `>` into `-->`, and with a
/// preceding `<!` into `<!--`. The specification's table misses these.
fn forms_cdo_or_cdc(prev: Option<&ComponentValue>, next: &ComponentValue) -> bool {
    let is_dash_dash = |v: &ComponentValue| matches!(v, ComponentValue::Ident(s) if &**s == "--");
    match prev {
        Some(prev) => {
            (is_dash_dash(prev) && next.is_delim('>')) || (prev.is_delim('!') && is_dash_dash(next))
        }
        None => false,
    }
}

fn write_value(value: &ComponentValue, out: &mut String) {
    match value {
        ComponentValue::Ident(s) => serialize_identifier(s, out),
        ComponentValue::AtKeyword(s) => {
            out.push('@');
            serialize_identifier(s, out);
        }
        ComponentValue::Hash { value, is_id } => {
            out.push('#');
            if *is_id {
                serialize_identifier(value, out);
            } else {
                serialize_name(value, out);
            }
        }
        ComponentValue::String(s) => serialize_string(s, out),
        // These cannot round-trip exactly; the output tokenizes as a bad
        // string (plus whitespace) and a bad URL.
        ComponentValue::BadString => out.push_str("\"\n"),
        ComponentValue::BadUrl => out.push_str("url(()"),
        ComponentValue::Url(url) => {
            out.push_str("url(");
            for c in url.chars() {
                match c {
                    '"' | '\'' | '(' | ')' | '\\' => {
                        out.push('\\');
                        out.push(c);
                    }
                    ' ' | '\t' | '\n' | '\0'..='\u{1f}' | '\u{7f}' => escape_code_point(c, out),
                    c => out.push(c),
                }
            }
            out.push(')');
        }
        // A backslash delim exists only before a newline.
        ComponentValue::Delim('\\') => out.push_str("\\\n"),
        ComponentValue::Delim(c) => out.push(*c),
        ComponentValue::Number(n) => write_number(n, out),
        ComponentValue::Percentage(n) => {
            write_number(n, out);
            out.push('%');
        }
        ComponentValue::Dimension { number, unit } => {
            write_number(number, out);
            write_unit(unit, out);
        }
        ComponentValue::UnicodeRange { start, end } => {
            let _ = write!(out, "U+{start:X}");
            if end != start {
                let _ = write!(out, "-{end:X}");
            }
        }
        ComponentValue::Whitespace => out.push(' '),
        ComponentValue::Cdo => out.push_str("<!--"),
        ComponentValue::Cdc => out.push_str("-->"),
        ComponentValue::Colon => out.push(':'),
        ComponentValue::Semicolon => out.push(';'),
        ComponentValue::Comma => out.push(','),
        ComponentValue::CloseParen => out.push(')'),
        ComponentValue::CloseSquare => out.push(']'),
        ComponentValue::CloseCurly => out.push('}'),
        ComponentValue::Function(f) => {
            serialize_identifier(&f.name, out);
            out.push('(');
            write_values(&f.arguments, out);
            out.push(')');
        }
        ComponentValue::Block(b) => {
            out.push(b.kind.open());
            write_values(&b.contents, out);
            out.push(b.kind.close());
        }
    }
}

/// Writes a number so that it tokenizes back with the same value, type flag
/// and sign flag.
pub(crate) fn write_number(n: &Number, out: &mut String) {
    if n.value.is_sign_negative() {
        out.push('-');
    } else if n.has_sign {
        out.push('+');
    }
    let magnitude = n.value.abs();
    match n.int_value {
        Some(i) if i as f32 == n.value => {
            let _ = write!(out, "{}", i.unsigned_abs());
        }
        // Integers outside the `i32` range: Rust prints integral floats
        // without a decimal point.
        Some(_) => {
            let _ = write!(out, "{magnitude}");
        }
        None => {
            let start = out.len();
            let _ = write!(out, "{magnitude}");
            if !out[start..].contains('.') {
                // Keep the "number" type flag.
                out.push_str(".0");
            }
        }
    }
}

/// Writes a dimension unit. A unit that starts like an exponent (`e3`)
/// gets its first letter escaped, so that it does not merge with the
/// number.
fn write_unit(unit: &str, out: &mut String) {
    let bytes = unit.as_bytes();
    let exponent_like = matches!(bytes.first(), Some(b'e' | b'E'))
        && match bytes.get(1) {
            Some(b) if b.is_ascii_digit() => true,
            Some(b'+' | b'-') => bytes.get(2).is_some_and(u8::is_ascii_digit),
            _ => false,
        };
    if exponent_like {
        out.push_str(if bytes[0] == b'e' { "\\65 " } else { "\\45 " });
        serialize_name(&unit[1..], out);
    } else {
        serialize_identifier(unit, out);
    }
}

impl fmt::Display for ComponentValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        write_value(self, &mut out);
        f.write_str(&out)
    }
}

/// Displays a list of component values (for log messages).
pub(crate) struct DisplayValues<'a>(pub(crate) &'a [ComponentValue]);

impl fmt::Display for DisplayValues<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&serialize_component_values(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_component_values;

    /// Parses, serializes, and parses again; the values must be equal.
    fn assert_round_trip(css: &str) -> String {
        let values = parse_component_values(css);
        let text = serialize_component_values(&values);
        let again = parse_component_values(&text);
        assert_eq!(values, again, "round trip of {css:?} via {text:?}");
        text
    }

    #[test]
    fn round_trips() {
        let cases = [
            "a b c",
            "rgb(1, 2.5, 50%)",
            "url(foo.png) url( 'a b' ) url(a\\)b)",
            "\"quote\\\"d\" 'single'",
            "#fff #123 #-x",
            "@media screen",
            "1px 1.0 +1 -0 -0.0 +.5 1e3 1E-3 .5em",
            "1e3em 1\\65 3 1\\45+3 2e-x",
            "a/**/b a/**/(b) a/**/1 1/**/% 1/**/a #a/**/-b",
            "-/**/-a -/**/1 ./**/5 +/**/5 @/**/a 1/**/-->",
            "//**/*",
            "[a] {b} (c)",
            "\\31 a -\\31 x \\- \\@x",
            "-- --x",
            "-\\->1",
            "<!\\--",
            "a\\ b \\\"c",
            "x\\0 y \\1 z",
            "\"a\\a b\"",
            "<!-- -->",
            ") ] }",
            "U+0-7F",
            "99999999999 -99999999999",
            "2n+1 -n-3 +n",
            "1\\% 2\\.5",
            "h\u{e9}llo \u{1F600}",
        ];
        for css in cases {
            assert_round_trip(css);
        }
    }

    #[test]
    fn serialized_forms() {
        assert_eq!(assert_round_trip("a   b"), "a b");
        assert_eq!(assert_round_trip("RGB( 1 ,2 )"), "RGB( 1 ,2 )");
        assert_eq!(assert_round_trip("1.0"), "1.0");
        assert_eq!(assert_round_trip("+0"), "+0");
        assert_eq!(assert_round_trip("-0"), "-0");
        assert_eq!(assert_round_trip("1.50px"), "1.5px");
        assert_eq!(assert_round_trip("url( a.png )"), "url(a.png)");
        assert_eq!(assert_round_trip("'it''s'"), "\"it\"\"s\"");
        assert_eq!(assert_round_trip("a/**/b"), "a/**/b");
        assert_eq!(assert_round_trip("\\31 a"), "\\31 a");
    }

    #[test]
    fn identifiers_and_strings() {
        let ident = |s: &str| {
            let mut out = String::new();
            serialize_identifier(s, &mut out);
            out
        };
        assert_eq!(ident("foo"), "foo");
        assert_eq!(ident("1a"), "\\31 a");
        assert_eq!(ident("-1a"), "-\\31 a");
        assert_eq!(ident("-"), "\\-");
        assert_eq!(ident("a b"), "a\\ b");
        assert_eq!(ident("a\u{1}"), "a\\1 ");
        let string = |s: &str| {
            let mut out = String::new();
            serialize_string(s, &mut out);
            out
        };
        assert_eq!(string("a\"b\\c\n"), "\"a\\\"b\\\\c\\a \"");
    }

    #[test]
    fn bad_tokens() {
        let values = parse_component_values("url(a b)");
        assert_eq!(serialize_component_values(&values), "url(()");
        assert_eq!(
            parse_component_values("url(()"),
            vec![ComponentValue::BadUrl]
        );
        assert_eq!(
            parse_component_values(&serialize_component_values(&[ComponentValue::BadString]))[0],
            ComponentValue::BadString
        );
    }
}
