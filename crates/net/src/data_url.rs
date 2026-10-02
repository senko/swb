//! The data: URL processor.
//!
//! <https://fetch.spec.whatwg.org/#data-url-processor>

use url::{Position, Url};

use crate::mime::MimeType;

/// The result of processing a data: URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DataUrl {
    pub(crate) mime_type: MimeType,
    pub(crate) body: Vec<u8>,
}

/// Runs the data: URL processor. Returns `None` on failure.
///
/// <https://fetch.spec.whatwg.org/#data-url-processor>
pub(crate) fn process(url: &Url) -> Option<DataUrl> {
    if url.scheme() != "data" {
        return None;
    }
    // The URL serialization without the fragment and without "data:".
    let input = url[..Position::AfterQuery].strip_prefix("data:")?;
    let (mime_type, encoded_body) = input.split_once(',')?;
    let mime_type = mime_type.trim_matches(|c: char| c.is_ascii_whitespace());
    let mut body = percent_decode(encoded_body.as_bytes());

    let mime_type = match strip_base64_suffix(mime_type) {
        Some(stripped) => {
            body = forgiving_base64_decode(&body)?;
            stripped
        }
        None => mime_type,
    };
    let mime_type = if mime_type.starts_with(';') {
        MimeType::parse(&format!("text/plain{mime_type}"))
    } else {
        MimeType::parse(mime_type)
    };
    Some(DataUrl {
        mime_type: mime_type.unwrap_or_else(MimeType::text_plain_us_ascii),
        body,
    })
}

/// If `mime_type` ends with `;`, zero or more spaces, and `base64` (ASCII
/// case-insensitive), returns the part before the `;`.
fn strip_base64_suffix(mime_type: &str) -> Option<&str> {
    let split = mime_type.len().checked_sub("base64".len())?;
    let (head, tail) = mime_type.split_at_checked(split)?;
    if !tail.eq_ignore_ascii_case("base64") {
        return None;
    }
    head.trim_end_matches(' ').strip_suffix(';')
}

/// Percent-decodes bytes. A `%` that is not followed by two hex digits stays
/// as it is.
///
/// <https://url.spec.whatwg.org/#percent-decode>
fn percent_decode(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let byte = input[i];
        if byte == b'%'
            && let Some(&[high, low]) = input.get(i + 1..i + 3)
            && let (Some(high), Some(low)) = (hex_value(high), hex_value(low))
        {
            output.push((high << 4) | low);
            i += 3;
        } else {
            output.push(byte);
            i += 1;
        }
    }
    output
}

fn hex_value(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|d| d as u8)
}

/// Decodes base64 and ignores ASCII whitespace. Returns `None` on failure.
///
/// The spec works on the isomorphic decoding of the bytes. Bytes 0x80 and
/// above become code points that are not in the base64 alphabet, so working
/// on bytes gives the same result.
///
/// <https://infra.spec.whatwg.org/#forgiving-base64-decode>
fn forgiving_base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut data: Vec<u8> = input
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if data.len().is_multiple_of(4) {
        if data.ends_with(b"==") {
            data.truncate(data.len() - 2);
        } else if data.ends_with(b"=") {
            data.truncate(data.len() - 1);
        }
    }
    if data.len() % 4 == 1 {
        return None;
    }
    let mut output = Vec::with_capacity(data.len() / 4 * 3 + 2);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in &data {
        buffer = (buffer << 6) | u32::from(base64_value(byte)?);
        bits += 6;
        if bits == 24 {
            output.extend_from_slice(&[(buffer >> 16) as u8, (buffer >> 8) as u8, buffer as u8]);
            buffer = 0;
            bits = 0;
        }
    }
    match bits {
        12 => output.push((buffer >> 4) as u8),
        18 => output.extend_from_slice(&[(buffer >> 10) as u8, (buffer >> 2) as u8]),
        _ => {}
    }
    Some(output)
}

/// The value of a character in the base64 alphabet (RFC 4648, table 1).
fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns (serialized MIME type, body), or `None` on failure.
    fn run(input: &str) -> Option<(String, Vec<u8>)> {
        let url = Url::parse(input).unwrap();
        process(&url).map(|d| (d.mime_type.to_string(), d.body))
    }

    fn ok(input: &str, mime: &str, body: &[u8]) {
        assert_eq!(
            run(input),
            Some((mime.to_string(), body.to_vec())),
            "{input:?}"
        );
    }

    fn fails(input: &str) {
        assert_eq!(run(input), None, "{input:?}");
    }

    #[test]
    fn spec_example() {
        // The example in https://fetch.spec.whatwg.org/#data-urls.
        ok(
            "data:,Hello%2C%20World!",
            "text/plain;charset=US-ASCII",
            b"Hello, World!",
        );
    }

    #[test]
    fn plain() {
        ok("data:,X", "text/plain;charset=US-ASCII", b"X");
        ok("data://test/,X", "text/plain;charset=US-ASCII", b"X");
        ok("data:,", "text/plain;charset=US-ASCII", b"");
        ok("data:text/html,<p>Hi</p>", "text/html", b"<p>Hi</p>");
        ok("data:text/html    ,X", "text/html", b"X");
        ok("data:,X#fragment", "text/plain;charset=US-ASCII", b"X");
        ok("data:,X?query", "text/plain;charset=US-ASCII", b"X?query");
        ok("data:,a,b", "text/plain;charset=US-ASCII", b"a,b");
    }

    #[test]
    fn missing_comma_fails() {
        fails("data:");
        fails("data:text/html");
        fails("data:text/plain;base64");
    }

    #[test]
    fn mime_type_handling() {
        ok("data:;charset=x,X", "text/plain;charset=x", b"X");
        ok(
            "data:text/plain;Charset=UTF-8,%C2%B1",
            "text/plain;charset=UTF-8",
            &[0xC2, 0xB1],
        );
        ok("data:IMAGE/PNG,X", "image/png", b"X");
        ok("data:x/x;charset=x;charset=y,X", "x/x;charset=x", b"X");
        // An invalid MIME type falls back to the default.
        ok("data:text,X", "text/plain;charset=US-ASCII", b"X");
        ok(
            "data:text/html%3Bcharset=utf-8,X",
            "text/plain;charset=US-ASCII",
            b"X",
        );
    }

    #[test]
    fn percent_decoding() {
        ok("data:,%FF", "text/plain;charset=US-ASCII", &[0xFF]);
        ok("data:,%", "text/plain;charset=US-ASCII", b"%");
        ok("data:,%x", "text/plain;charset=US-ASCII", b"%x");
        ok("data:,%4", "text/plain;charset=US-ASCII", b"%4");
        ok("data:,%41%4a%4A", "text/plain;charset=US-ASCII", b"AJJ");
    }

    #[test]
    fn base64() {
        ok("data:text/plain;base64,WA", "text/plain", b"X");
        // Without ";base64" the MIME type is empty, so it is the default.
        ok("data:;base64,WA", "text/plain;charset=US-ASCII", b"X");
        ok("data:;BASE64,WA", "text/plain;charset=US-ASCII", b"X");
        ok("data:;base64,WA==", "text/plain;charset=US-ASCII", b"X");
        ok(
            "data:;base64,SGVsbG8=",
            "text/plain;charset=US-ASCII",
            b"Hello",
        );
        ok("data:;base64,ab", "text/plain;charset=US-ASCII", &[0x69]);
        ok("data:;base64,", "text/plain;charset=US-ASCII", b"");
        ok(
            "data:;charset=utf-8;base64,WA",
            "text/plain;charset=utf-8",
            b"X",
        );
        ok("data:x/x;base64;base64,WA", "x/x", b"X");
        ok("data:x/x;base64 ,WA", "x/x", b"X");
        ok("data:x/x;  base64,WA", "x/x", b"X");
        ok("data:x/x;base64,W%20A", "x/x", b"X");
        ok("data:x/x;base64,W%0cA", "x/x", b"X");
        ok("data:x/x;base64,W\tA", "x/x", b"X");
    }

    #[test]
    fn not_base64() {
        ok("data:x/x;base64;charset=x,WA", "x/x;charset=x", b"WA");
        ok("data:x/x;base 64,WA", "x/x", b"WA");
        ok("data:;base64;,WA", "text/plain", b"WA");
        ok("data:x/xbase64,WA", "x/xbase64", b"WA");
        ok("data:base64,WA", "text/plain;charset=US-ASCII", b"WA");
    }

    #[test]
    fn invalid_base64_fails() {
        fails("data:;base64,1");
        fails("data:;base64,W===");
        fails("data:;base64,WA=");
        fails("data:;base64,W=A");
        fails("data:;base64,WA=A");
        fails("data:;base64,%FF");
        fails("data:;base64,W-A");
    }

    #[test]
    fn non_data_url_fails() {
        fails("https://example.com/,x");
    }
}
