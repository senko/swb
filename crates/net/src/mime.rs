//! MIME types: parsing, serialization, and extraction from a header list.
//!
//! - Parse: <https://mimesniff.spec.whatwg.org/#parse-a-mime-type>
//! - Serialize: <https://mimesniff.spec.whatwg.org/#serialize-a-mime-type>
//! - Extract from headers:
//!   <https://fetch.spec.whatwg.org/#concept-header-extract-mime-type>

use std::fmt;

use crate::headers::Headers;

/// A parsed MIME type record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MimeType {
    /// The type, in ASCII lowercase.
    pub(crate) type_: String,
    /// The subtype, in ASCII lowercase.
    pub(crate) subtype: String,
    /// Parameters in order. Names are in ASCII lowercase, values keep their
    /// case. Each name occurs at most once.
    pub(crate) parameters: Vec<(String, String)>,
}

impl MimeType {
    /// `text/plain;charset=US-ASCII`, the default MIME type of data: URLs.
    pub(crate) fn text_plain_us_ascii() -> Self {
        MimeType {
            type_: "text".to_string(),
            subtype: "plain".to_string(),
            parameters: vec![("charset".to_string(), "US-ASCII".to_string())],
        }
    }

    /// Parses a MIME type. Returns `None` on failure.
    ///
    /// <https://mimesniff.spec.whatwg.org/#parse-a-mime-type>
    pub(crate) fn parse(input: &str) -> Option<Self> {
        let input = input.trim_matches(is_http_whitespace);
        let mut cursor = Cursor::new(input);

        let type_ = cursor.collect_while(|c| c != '/');
        if type_.is_empty() || !type_.chars().all(is_http_token_code_point) {
            return None;
        }
        if cursor.is_at_end() {
            return None;
        }
        cursor.advance();

        let subtype = cursor
            .collect_while(|c| c != ';')
            .trim_end_matches(is_http_whitespace);
        if subtype.is_empty() || !subtype.chars().all(is_http_token_code_point) {
            return None;
        }

        let mut mime_type = MimeType {
            type_: type_.to_ascii_lowercase(),
            subtype: subtype.to_ascii_lowercase(),
            parameters: Vec::new(),
        };
        while !cursor.is_at_end() {
            // Skip the U+003B (;).
            cursor.advance();
            cursor.collect_while(is_http_whitespace);
            let name = cursor
                .collect_while(|c| c != ';' && c != '=')
                .to_ascii_lowercase();
            if let Some(c) = cursor.peek() {
                if c == ';' {
                    continue;
                }
                // Skip the U+003D (=).
                cursor.advance();
            }
            if cursor.is_at_end() {
                break;
            }
            let value = if cursor.peek() == Some('"') {
                let value = collect_http_quoted_string(&mut cursor, true);
                cursor.collect_while(|c| c != ';');
                value
            } else {
                let value = cursor
                    .collect_while(|c| c != ';')
                    .trim_end_matches(is_http_whitespace);
                if value.is_empty() {
                    continue;
                }
                value.to_string()
            };
            if !name.is_empty()
                && name.chars().all(is_http_token_code_point)
                && value.chars().all(is_http_quoted_string_token_code_point)
                && mime_type.parameter(&name).is_none()
            {
                mime_type.parameters.push((name, value));
            }
        }
        Some(mime_type)
    }

    /// Returns `type/subtype`.
    pub(crate) fn essence(&self) -> String {
        format!("{}/{}", self.type_, self.subtype)
    }

    /// Returns the value of the parameter `name` (in ASCII lowercase).
    pub(crate) fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn set_parameter(&mut self, name: &str, value: String) {
        match self.parameters.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = value,
            None => self.parameters.push((name.to_string(), value)),
        }
    }
}

/// Serializes the MIME type.
///
/// <https://mimesniff.spec.whatwg.org/#serialize-a-mime-type>
impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.type_, self.subtype)?;
        for (name, value) in &self.parameters {
            write!(f, ";{name}=")?;
            if !value.is_empty() && value.chars().all(is_http_token_code_point) {
                f.write_str(value)?;
            } else {
                f.write_str("\"")?;
                for c in value.chars() {
                    if c == '"' || c == '\\' {
                        f.write_str("\\")?;
                    }
                    write!(f, "{c}")?;
                }
                f.write_str("\"")?;
            }
        }
        Ok(())
    }
}

/// Extracts the MIME type from the `Content-Type` headers.
///
/// <https://fetch.spec.whatwg.org/#concept-header-extract-mime-type>
pub(crate) fn extract_mime_type(headers: &Headers) -> Option<MimeType> {
    let values: Vec<&str> = headers.get_all("content-type").collect();
    if values.is_empty() {
        return None;
    }
    let mut charset: Option<String> = None;
    let mut essence: Option<String> = None;
    let mut mime_type: Option<MimeType> = None;
    for value in split_header_value(&values.join(", ")) {
        let Some(mut temporary) = MimeType::parse(&value) else {
            continue;
        };
        let temporary_essence = temporary.essence();
        if temporary_essence == "*/*" {
            continue;
        }
        if essence.as_deref() == Some(temporary_essence.as_str()) {
            if temporary.parameter("charset").is_none()
                && let Some(charset) = &charset
            {
                temporary.set_parameter("charset", charset.clone());
            }
        } else {
            charset = temporary.parameter("charset").map(str::to_string);
            essence = Some(temporary_essence);
        }
        mime_type = Some(temporary);
    }
    mime_type
}

/// Splits a combined header value at commas that are not inside quoted
/// strings.
///
/// <https://fetch.spec.whatwg.org/#header-value-get-decode-and-split>
fn split_header_value(input: &str) -> Vec<String> {
    let mut cursor = Cursor::new(input);
    let mut values = Vec::new();
    let mut temporary = String::new();
    loop {
        temporary.push_str(cursor.collect_while(|c| c != '"' && c != ','));
        if cursor.peek() == Some('"') {
            temporary.push_str(&collect_http_quoted_string(&mut cursor, false));
            if !cursor.is_at_end() {
                continue;
            }
        }
        values.push(temporary.trim_matches(is_http_tab_or_space).to_string());
        temporary.clear();
        if cursor.is_at_end() {
            return values;
        }
        // Skip the U+002C (,).
        cursor.advance();
    }
}

/// Collects an HTTP quoted string. The cursor must point at a U+0022 (").
/// With `extract_value`, returns the unquoted value; otherwise returns the
/// source text, including the quotes.
///
/// <https://fetch.spec.whatwg.org/#collect-an-http-quoted-string>
fn collect_http_quoted_string(cursor: &mut Cursor<'_>, extract_value: bool) -> String {
    let start = cursor.position;
    let mut value = String::new();
    // Skip the opening U+0022 (").
    cursor.advance();
    loop {
        value.push_str(cursor.collect_while(|c| c != '"' && c != '\\'));
        let Some(quote_or_backslash) = cursor.peek() else {
            break;
        };
        cursor.advance();
        if quote_or_backslash == '\\' {
            let Some(escaped) = cursor.peek() else {
                value.push('\\');
                break;
            };
            value.push(escaped);
            cursor.advance();
        } else {
            break;
        }
    }
    if extract_value {
        value
    } else {
        cursor.input[start..cursor.position].to_string()
    }
}

/// A position in a string, for the "collect a sequence of code points"
/// style of spec algorithms.
struct Cursor<'a> {
    input: &'a str,
    /// Byte offset; always on a char boundary.
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(input: &'a str) -> Self {
        Cursor { input, position: 0 }
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn advance(&mut self) {
        if let Some(c) = self.peek() {
            self.position += c.len_utf8();
        }
    }

    fn collect_while(&mut self, predicate: impl Fn(char) -> bool) -> &'a str {
        let rest = &self.input[self.position..];
        let length = rest.find(|c| !predicate(c)).unwrap_or(rest.len());
        self.position += length;
        &rest[..length]
    }
}

/// <https://fetch.spec.whatwg.org/#http-whitespace>
fn is_http_whitespace(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\t' | ' ')
}

/// <https://fetch.spec.whatwg.org/#http-tab-or-space>
fn is_http_tab_or_space(c: char) -> bool {
    matches!(c, '\t' | ' ')
}

/// <https://mimesniff.spec.whatwg.org/#http-token-code-point>
fn is_http_token_code_point(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '.'
                | '^'
                | '_'
                | '`'
                | '|'
                | '~'
        )
}

/// <https://mimesniff.spec.whatwg.org/#http-quoted-string-token-code-point>
fn is_http_quoted_string_token_code_point(c: char) -> bool {
    matches!(c, '\t' | ' '..='~' | '\u{80}'..='\u{ff}')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses and serializes; `None` means failure.
    fn roundtrip(input: &str) -> Option<String> {
        MimeType::parse(input).map(|m| m.to_string())
    }

    #[test]
    fn parse_simple() {
        let mime = MimeType::parse("text/html;charset=gbk").unwrap();
        assert_eq!(mime.essence(), "text/html");
        assert_eq!(mime.parameter("charset"), Some("gbk"));
    }

    #[test]
    fn parse_lowercases_type_and_names_but_not_values() {
        assert_eq!(
            roundtrip("TEXT/HTML;CHARSET=GBK").as_deref(),
            Some("text/html;charset=GBK")
        );
    }

    #[test]
    fn parse_parameters() {
        let cases = [
            ("text/html;charset=\"gbk\"", "text/html;charset=gbk"),
            ("text/html;charset=gbk(", "text/html;charset=\"gbk(\""),
            ("text/html;x=(;charset=gbk", "text/html;x=\"(\";charset=gbk"),
            (
                "text/html;charset=gbk;charset=windows-1255",
                "text/html;charset=gbk",
            ),
            (
                "text/html;charset=\";charset=foo\";charset=GBK",
                "text/html;charset=\";charset=foo\"",
            ),
            ("text/html;charset=\"\\\"\"", "text/html;charset=\"\\\"\""),
            ("text/html;charset=\"gbk\"x", "text/html;charset=gbk"),
            ("text/html;charset=", "text/html"),
            ("text/html;charset", "text/html"),
            ("text/html;charset=;x=y", "text/html;x=y"),
            ("text/html; charset=gbk", "text/html;charset=gbk"),
            ("text/html;charset= gbk", "text/html;charset=\" gbk\""),
            ("text/html;charset =gbk", "text/html"),
            ("text/html;", "text/html"),
            ("text/html;;;;charset=gbk", "text/html;charset=gbk"),
            ("text/html;charset=\"gbk", "text/html;charset=gbk"),
            ("text/html;charset=\"\\", "text/html;charset=\"\\\\\""),
            (" text/html ", "text/html"),
            ("*/*", "*/*"),
        ];
        for (input, expected) in cases {
            assert_eq!(roundtrip(input).as_deref(), Some(expected), "{input:?}");
        }
    }

    #[test]
    fn parse_failures() {
        for input in [
            "",
            "text",
            "text/",
            "/html",
            "te xt/html",
            "text/ht ml",
            "text/html\u{c}",
            "text/\u{e9}",
            "(/x",
        ] {
            assert_eq!(roundtrip(input), None, "{input:?}");
        }
    }

    #[test]
    fn parse_keeps_latin1_values() {
        assert_eq!(
            roundtrip("text/html;charset=\u{e9}").as_deref(),
            Some("text/html;charset=\"\u{e9}\"")
        );
        // A code point above U+00FF is not a quoted-string token code point.
        assert_eq!(
            roundtrip("text/html;x=\u{100}").as_deref(),
            Some("text/html")
        );
    }

    #[test]
    fn split_respects_quotes() {
        assert_eq!(
            split_header_value("text/html;x=\"a,b\", text/plain ,"),
            ["text/html;x=\"a,b\"", "text/plain", ""]
        );
        assert_eq!(split_header_value(""), [""]);
        assert_eq!(
            split_header_value("\"unterminated, x"),
            ["\"unterminated, x"]
        );
    }

    fn extract(values: &[&str]) -> Option<String> {
        let headers: Headers = values.iter().map(|v| ("Content-Type", *v)).collect();
        extract_mime_type(&headers).map(|m| m.to_string())
    }

    #[test]
    fn extract_from_headers() {
        assert_eq!(extract(&[]), None);
        assert_eq!(
            extract(&["text/plain, text/html"]).as_deref(),
            Some("text/html")
        );
        assert_eq!(
            extract(&["text/html;charset=gbk", "text/html"]).as_deref(),
            Some("text/html;charset=gbk")
        );
        assert_eq!(
            extract(&["text/html;charset=gbk, */*"]).as_deref(),
            Some("text/html;charset=gbk")
        );
        assert_eq!(
            extract(&["text/html;charset=gbk, text/plain, text/html"]).as_deref(),
            Some("text/html")
        );
        assert_eq!(
            extract(&["text/html;charset=gbk", "text/html;charset=utf-8"]).as_deref(),
            Some("text/html;charset=utf-8")
        );
        assert_eq!(extract(&["text/html, bogus"]).as_deref(), Some("text/html"));
        assert_eq!(extract(&["bogus"]), None);
    }
}
