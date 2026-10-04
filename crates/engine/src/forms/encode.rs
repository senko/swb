//! The encodings of form data: `application/x-www-form-urlencoded`,
//! `multipart/form-data` and `text/plain`.
//!
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>
//! and <https://url.spec.whatwg.org/#concept-urlencoded-serializer>.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

use encoding_rs::Encoding;

/// One entry of an entry list: a name and a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) value: EntryValue,
}

/// The value of an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EntryValue {
    /// A string.
    Text(String),
    /// A file (swb sends only the empty file of a file input without a
    /// selected file).
    File {
        filename: String,
        content_type: String,
        data: Vec<u8>,
    },
}

impl Entry {
    /// An entry with a string value.
    pub(crate) fn text(name: &str, value: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            value: EntryValue::Text(value.to_owned()),
        }
    }

    /// The value as a string: a file's name for a file.
    fn value_string(&self) -> &str {
        match &self.value {
            EntryValue::Text(s) => s,
            EntryValue::File { filename, .. } => filename,
        }
    }
}

/// Replaces every CR not followed by LF, every LF not preceded by CR, and
/// CR LF with CR LF, as the form encodings require for names and values
/// (<https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#convert-to-a-list-of-name-value-pairs>,
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#multipart/form-data-encoding-algorithm>).
/// Infra's "normalize newlines" converts to LF instead; see
/// `normalize_newlines` in the parent module.
fn to_crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\r\n");
            }
            '\n' => out.push_str("\r\n"),
            c => out.push(c),
        }
    }
    out
}

/// Encodes `s` with the output encoding of `encoding`. Characters that it
/// cannot encode become HTML decimal numeric character references.
fn encode<'a>(encoding: &'static Encoding, s: &'a str) -> Cow<'a, [u8]> {
    encoding.output_encoding().encode(s).0
}

/// The `application/x-www-form-urlencoded` serialization of the entries:
/// names and values with normalized line breaks, encoded, percent-encoded
/// (space as `+`), joined with `&`.
pub(crate) fn urlencoded(entries: &[Entry], encoding: &'static Encoding) -> String {
    let mut out = String::new();
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        percent_encode(&encode(encoding, &to_crlf(&entry.name)), &mut out);
        out.push('=');
        percent_encode(&encode(encoding, &to_crlf(entry.value_string())), &mut out);
    }
    out
}

/// The urlencoded byte serializer: ASCII alphanumerics and `*-._` stay,
/// space becomes `+`, other bytes are percent-encoded.
/// <https://url.spec.whatwg.org/#concept-urlencoded-byte-serializer>
fn percent_encode(bytes: &[u8], out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &b in bytes {
        match b {
            b' ' => out.push('+'),
            b'*' | b'-' | b'.' | b'_' => out.push(char::from(b)),
            b if b.is_ascii_alphanumeric() => out.push(char::from(b)),
            b => {
                out.push('%');
                out.push(char::from(HEX[usize::from(b >> 4)]));
                out.push(char::from(HEX[usize::from(b & 0xF)]));
            }
        }
    }
}

/// The `text/plain` encoding: `name=value` lines with CR LF, encoded.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#text/plain-encoding-algorithm>
pub(crate) fn text_plain(entries: &[Entry], encoding: &'static Encoding) -> Vec<u8> {
    let mut out = String::new();
    for entry in entries {
        out.push_str(&to_crlf(&entry.name));
        out.push('=');
        out.push_str(&to_crlf(entry.value_string()));
        out.push_str("\r\n");
    }
    encode(encoding, &out).into_owned()
}

/// The `multipart/form-data` encoding (RFC 7578) with `boundary`.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#multipart-form-data>
pub(crate) fn multipart(entries: &[Entry], encoding: &'static Encoding, boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        out.extend_from_slice(b"--");
        out.extend_from_slice(boundary.as_bytes());
        out.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
        out.extend_from_slice(&encode(encoding, &escape_name(&entry.name)));
        out.push(b'"');
        match &entry.value {
            EntryValue::Text(value) => {
                out.extend_from_slice(b"\r\n\r\n");
                out.extend_from_slice(&encode(encoding, &to_crlf(value)));
            }
            EntryValue::File {
                filename,
                content_type,
                data,
            } => {
                out.extend_from_slice(b"; filename=\"");
                out.extend_from_slice(&encode(encoding, &escape_name(filename)));
                out.extend_from_slice(b"\"\r\nContent-Type: ");
                out.extend_from_slice(content_type.as_bytes());
                out.extend_from_slice(b"\r\n\r\n");
                out.extend_from_slice(data);
            }
        }
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary.as_bytes());
    out.extend_from_slice(b"--\r\n");
    out
}

/// Escapes a name or a file name for a `Content-Disposition` header: line
/// breaks are normalized, then `"`, CR and LF are percent-encoded.
fn escape_name(name: &str) -> String {
    to_crlf(name)
        .replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// A new multipart boundary: `----swbFormBoundary` and 16 letters and
/// digits that differ between calls.
pub(crate) fn boundary() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mut state = time ^ COUNTER.fetch_add(1, Ordering::Relaxed).rotate_left(32);
    let mut out = String::from("----swbFormBoundary");
    for _ in 0..16 {
        // splitmix64
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        out.push(char::from(ALPHABET[(z % 62) as usize]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<Entry> {
        vec![
            Entry::text("q", "a b&c=d"),
            Entry::text("č", "x\ny\r\nz\r"),
            Entry::text("*-._~!", "€"),
        ]
    }

    #[test]
    fn urlencoded_utf8() {
        assert_eq!(
            urlencoded(&entries(), encoding_rs::UTF_8),
            "q=a+b%26c%3Dd&%C4%8D=x%0D%0Ay%0D%0Az%0D%0A&*-._%7E%21=%E2%82%AC"
        );
    }

    #[test]
    fn urlencoded_legacy_encoding_uses_character_references() {
        let entries = [Entry::text("a", "é€č")];
        // windows-1252 has é and €, but not č: it becomes &#269;.
        assert_eq!(
            urlencoded(&entries, encoding_rs::WINDOWS_1252),
            "a=%E9%80%26%23269%3B"
        );
        // UTF-16 forms are submitted as UTF-8.
        assert_eq!(
            urlencoded(&entries, encoding_rs::UTF_16LE),
            "a=%C3%A9%E2%82%AC%C4%8D"
        );
    }

    #[test]
    fn text_plain_lines() {
        let entries = [Entry::text("a", "1 2"), Entry::text("b", "x\ny")];
        assert_eq!(
            text_plain(&entries, encoding_rs::UTF_8),
            b"a=1 2\r\nb=x\r\ny\r\n"
        );
    }

    #[test]
    fn multipart_parts() {
        let entries = [
            Entry::text("a\"b", "line1\nline2"),
            Entry {
                name: "f".to_owned(),
                value: EntryValue::File {
                    filename: String::new(),
                    content_type: "application/octet-stream".to_owned(),
                    data: Vec::new(),
                },
            },
        ];
        let body = multipart(&entries, encoding_rs::UTF_8, "XYZ");
        let expected = "--XYZ\r\nContent-Disposition: form-data; name=\"a%22b\"\r\n\r\nline1\r\nline2\r\n\
                        --XYZ\r\nContent-Disposition: form-data; name=\"f\"; filename=\"\"\r\n\
                        Content-Type: application/octet-stream\r\n\r\n\r\n--XYZ--\r\n";
        assert_eq!(String::from_utf8(body).unwrap(), expected);
    }

    #[test]
    fn boundaries_differ() {
        let (a, b) = (boundary(), boundary());
        assert_ne!(a, b);
        assert_eq!(a.len(), "----swbFormBoundary".len() + 16);
        assert!(a.bytes().all(|c| c == b'-' || c.is_ascii_alphanumeric()));
    }
}
