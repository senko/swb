//! The tokens of Web IDL (<https://webidl.spec.whatwg.org/#idl-grammar>,
//! "Lexical grammar"): identifiers, integers, decimals, strings and other
//! characters; whitespace and comments are skipped. Keywords are
//! identifiers here; the parser matches them by text.

use crate::Error;

/// A token and the line it starts on (1-based).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) line: u32,
}

/// The kind of a token.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokenKind {
    /// An identifier or a keyword, with a leading `_` (the escape of
    /// keywords as names) removed.
    Ident(String),
    /// An integer literal.
    Integer(i64),
    /// A decimal literal, as written.
    Decimal(String),
    /// A string literal without the quotes.
    String(String),
    /// `...`.
    Ellipsis,
    /// Any other single character.
    Other(char),
}

/// Splits `text` into tokens. `file` names the source in errors.
pub(crate) fn tokenize(file: &str, text: &str) -> Result<Vec<Token>, Error> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    let error = |line: u32, message: String| Error {
        file: file.to_owned(),
        line,
        message,
    };
    while let Some(&c) = chars.get(i) {
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while chars.get(i).is_some_and(|&c| c != '\n') {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            let start = line;
            i += 2;
            loop {
                match chars.get(i) {
                    None => return Err(error(start, "unterminated comment".to_owned())),
                    Some('*') if chars.get(i + 1) == Some(&'/') => {
                        i += 2;
                        break;
                    }
                    Some('\n') => {
                        line += 1;
                        i += 1;
                    }
                    Some(_) => i += 1,
                }
            }
        } else if c == '"' {
            let start = i + 1;
            let mut end = start;
            while chars.get(end).is_some_and(|&c| c != '"') {
                if chars.get(end) == Some(&'\n') {
                    return Err(error(line, "unterminated string".to_owned()));
                }
                end += 1;
            }
            if end >= chars.len() {
                return Err(error(line, "unterminated string".to_owned()));
            }
            let text: String = chars.get(start..end).unwrap_or_default().iter().collect();
            tokens.push(Token {
                kind: TokenKind::String(text),
                line,
            });
            i = end + 1;
        } else if c == '.' && chars.get(i + 1) == Some(&'.') && chars.get(i + 2) == Some(&'.') {
            tokens.push(Token {
                kind: TokenKind::Ellipsis,
                line,
            });
            i += 3;
        } else if c.is_ascii_digit()
            || (c == '-' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let (kind, next) = number(&chars, i).map_err(|message| error(line, message))?;
            tokens.push(Token { kind, line });
            i = next;
        } else if is_ident_start(&chars, i) {
            let mut end = i + 1;
            while chars
                .get(end)
                .is_some_and(|&c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                end += 1;
            }
            let mut text: String = chars.get(i..end).unwrap_or_default().iter().collect();
            if text.starts_with('_') {
                text.remove(0);
            }
            tokens.push(Token {
                kind: TokenKind::Ident(text),
                line,
            });
            i = end;
        } else {
            tokens.push(Token {
                kind: TokenKind::Other(c),
                line,
            });
            i += 1;
        }
    }
    Ok(tokens)
}

/// Whether an identifier starts at `i`: `[_-]?[A-Za-z]`.
fn is_ident_start(chars: &[char], i: usize) -> bool {
    match chars.get(i) {
        Some(c) if c.is_ascii_alphabetic() => true,
        Some('_' | '-') => chars.get(i + 1).is_some_and(char::is_ascii_alphabetic),
        _ => false,
    }
}

/// An integer (decimal, hexadecimal `0x..` or octal `0..`) or a decimal
/// literal at `start`, and the index after it.
fn number(chars: &[char], start: usize) -> Result<(TokenKind, usize), String> {
    let mut end = start;
    if chars.get(end) == Some(&'-') {
        end += 1;
    }
    let digits_start = end;
    let hex = chars.get(end) == Some(&'0') && matches!(chars.get(end + 1), Some('x' | 'X'));
    if hex {
        end += 2;
        while chars.get(end).is_some_and(char::is_ascii_hexdigit) {
            end += 1;
        }
    } else {
        while chars
            .get(end)
            .is_some_and(|&c| c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E')
            || (matches!(chars.get(end), Some('+' | '-'))
                && matches!(chars.get(end - 1), Some('e' | 'E')))
        {
            end += 1;
        }
    }
    let text: String = chars.get(start..end).unwrap_or_default().iter().collect();
    let body: String = chars
        .get(digits_start..end)
        .unwrap_or_default()
        .iter()
        .collect();
    let negative = digits_start > start;
    if !hex && body.contains(['.', 'e', 'E']) {
        return Ok((TokenKind::Decimal(text), end));
    }
    let magnitude = if hex {
        i64::from_str_radix(body.get(2..).unwrap_or_default(), 16)
    } else if body.len() > 1 && body.starts_with('0') {
        i64::from_str_radix(body.get(1..).unwrap_or_default(), 8)
    } else {
        body.parse::<i64>()
    }
    .map_err(|_| format!("bad integer literal '{text}'"))?;
    Ok((
        TokenKind::Integer(if negative { -magnitude } else { magnitude }),
        end,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<TokenKind> {
        tokenize("t.idl", text)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn tokens_of_each_kind() {
        assert_eq!(
            kinds(
                "interface _namespace { const long X = 0x1F; }; // c\n/* d */ \"s\" ... -1.5 017"
            ),
            vec![
                TokenKind::Ident("interface".into()),
                TokenKind::Ident("namespace".into()),
                TokenKind::Other('{'),
                TokenKind::Ident("const".into()),
                TokenKind::Ident("long".into()),
                TokenKind::Ident("X".into()),
                TokenKind::Other('='),
                TokenKind::Integer(31),
                TokenKind::Other(';'),
                TokenKind::Other('}'),
                TokenKind::Other(';'),
                TokenKind::String("s".into()),
                TokenKind::Ellipsis,
                TokenKind::Decimal("-1.5".into()),
                TokenKind::Integer(15),
            ]
        );
    }

    #[test]
    fn lines_count_newlines_in_comments() {
        let tokens = tokenize("t.idl", "a\n/* x\ny */ b\n\nc").unwrap();
        let lines: Vec<u32> = tokens.iter().map(|t| t.line).collect();
        assert_eq!(lines, vec![1, 3, 5]);
    }

    #[test]
    fn errors_have_lines() {
        let error = tokenize("t.idl", "a\n\"open").unwrap_err();
        assert_eq!(error.to_string(), "t.idl:2: unterminated string");
        let error = tokenize("t.idl", "a /* b").unwrap_err();
        assert_eq!(error.line, 1);
        assert!(tokenize("t.idl", "x = 99999999999999999999;").is_err());
    }
}
