//! `atob()` and `btoa()` (HTML §8.3.1, "Base64 utility methods",
//! <https://html.spec.whatwg.org/multipage/webappapis.html#atob>) with the
//! forgiving-base64 decode of the Infra Standard
//! (<https://infra.spec.whatwg.org/#forgiving-base64-decode>).
//!
//! A "binary string" is a string whose code units are all below 256; here
//! each byte is one `char` of U+0000 to U+00FF.

use crate::bindings::Exception;

/// The base64 alphabet (RFC 4648 §4).
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Why a decode failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecodeError {
    /// The text has a character above U+00FF (Chromium words this apart).
    OutsideLatin1,
    /// The text is no forgiving base64.
    Invalid,
}

/// The value of a base64 character, if it is one.
fn value_of(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// The forgiving-base64 decode of `input` as a binary string (`atob`).
pub(crate) fn decode(input: &str) -> Result<String, DecodeError> {
    if input.chars().any(|c| u32::from(c) > 0xFF) {
        return Err(DecodeError::OutsideLatin1);
    }
    // Step 1: remove the ASCII whitespace (tab, LF, FF, CR, space).
    let mut data: Vec<u8> = input
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' '))
        .map(|c| c as u8)
        .collect();
    // Step 2: a length that is a multiple of 4 loses one or two final `=`.
    if data.len().is_multiple_of(4) {
        for _ in 0..2 {
            if data.last() == Some(&b'=') {
                data.pop();
            }
        }
    }
    // Step 3: a remainder of 1 can not be a base64 text.
    if data.len() % 4 == 1 {
        return Err(DecodeError::Invalid);
    }
    let mut output = String::with_capacity(data.len() / 4 * 3 + 2);
    let mut bits: u32 = 0;
    let mut count = 0;
    for byte in data {
        let value = value_of(byte).ok_or(DecodeError::Invalid)?;
        bits = (bits << 6) | value;
        count += 1;
        if count == 4 {
            for shift in [16, 8, 0] {
                output.push(char::from((bits >> shift) as u8));
            }
            bits = 0;
            count = 0;
        }
    }
    // Step 10 of the algorithm: the bits that do not fill a byte go.
    match count {
        2 => output.push(char::from((bits >> 4) as u8)),
        3 => {
            output.push(char::from((bits >> 10) as u8));
            output.push(char::from((bits >> 2) as u8));
        }
        _ => {}
    }
    Ok(output)
}

/// The base64 encoding of a binary string (`btoa`); `None` if a character
/// is above U+00FF.
pub(crate) fn encode(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len());
    for c in input.chars() {
        bytes.push(u8::try_from(u32::from(c)).ok()?);
    }
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let [a, b, c] = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let bits = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                let index = ((bits >> shift) & 63) as usize;
                output.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
            } else {
                output.push('=');
            }
        }
    }
    Some(output)
}

/// The exception of a failed `btoa`.
pub(crate) fn encode_error() -> Exception {
    Exception::dom(
        "InvalidCharacterError",
        "The string to be encoded contains characters outside of the Latin1 range.",
    )
}

/// The exception of a failed `atob`.
pub(crate) fn decode_error(error: DecodeError) -> Exception {
    Exception::dom(
        "InvalidCharacterError",
        match error {
            DecodeError::OutsideLatin1 => {
                "The string to be decoded contains characters outside of the Latin1 range."
            }
            DecodeError::Invalid => "The string to be decoded is not correctly encoded.",
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_with_padding() {
        assert_eq!(encode("").as_deref(), Some(""));
        assert_eq!(encode("a").as_deref(), Some("YQ=="));
        assert_eq!(encode("ab").as_deref(), Some("YWI="));
        assert_eq!(encode("abc").as_deref(), Some("YWJj"));
        assert_eq!(encode("\u{ff}\u{0}\u{80}").as_deref(), Some("/wCA"));
        assert_eq!(encode("\u{100}"), None);
    }

    #[test]
    fn decodes_as_forgiving_base64() {
        for (input, expected) in [
            ("", ""),
            ("YQ==", "a"),
            ("YQ", "a"),
            ("YWI=", "ab"),
            (" Y W J j ", "abc"),
            ("YQ==\n", "a"),
            ("AAAAAA", "\0\0\0\0"),
            ("AAAAAA==", "\0\0\0\0"),
            ("YR==", "a"),
            ("/+8=", "\u{ff}\u{ef}"),
        ] {
            assert_eq!(decode(input).as_deref(), Ok(expected), "{input:?}");
        }
    }

    #[test]
    fn refuses_what_the_algorithm_refuses() {
        for input in [
            "Y",
            "YQ=",
            "YQ===",
            "YQ=a",
            "Y=Q=",
            "Y*Q=",
            "-_",
            "A",
            "AAAAA",
            "AAAA=",
            "AAA==",
            "AAAA====",
            "\u{a0}YQ==",
        ] {
            assert_eq!(decode(input), Err(DecodeError::Invalid), "{input:?}");
        }
        assert_eq!(decode("\u{100}AAA"), Err(DecodeError::OutsideLatin1));
    }

    #[test]
    fn round_trips_all_bytes() {
        let all: String = (0..=255u8).map(char::from).collect();
        let encoded = encode(&all).expect("a binary string");
        assert_eq!(decode(&encoded).as_deref(), Ok(all.as_str()));
    }
}
