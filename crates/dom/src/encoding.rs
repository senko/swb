//! Character encoding detection for HTML documents.
//!
//! Follows "determining the character encoding" in the HTML standard
//! (<https://html.spec.whatwg.org/multipage/parsing.html#determining-the-character-encoding>):
//! byte order mark, transport-layer charset, `<meta>` prescan, then a
//! default. For the default we use content sniffing: valid UTF-8 is decoded
//! as UTF-8, everything else as windows-1252. This matches what Chromium
//! does for unlabeled pages in practice.

use std::borrow::Cow;

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252, X_USER_DEFINED};

/// How many bytes the `<meta>` prescan examines.
const PRESCAN_LIMIT: usize = 1024;

/// Decodes an HTML document. Returns the text and the encoding used.
pub fn decode_html<'a>(
    bytes: &'a [u8],
    transport_charset: Option<&str>,
) -> (Cow<'a, str>, &'static Encoding) {
    let encoding = sniff_encoding(bytes, transport_charset);
    // `decode` gives the byte order mark priority over `encoding`, as the
    // standard requires, and replaces malformed sequences with U+FFFD.
    let (text, used, _had_errors) = encoding.decode(bytes);
    (text, used)
}

/// Determines the encoding of an HTML document without decoding it. A byte
/// order mark, if present, still takes priority when decoding.
pub fn sniff_encoding(bytes: &[u8], transport_charset: Option<&str>) -> &'static Encoding {
    if let Some((encoding, _)) = Encoding::for_bom(bytes) {
        return encoding;
    }
    if let Some(encoding) = transport_charset.and_then(|l| Encoding::for_label(l.as_bytes())) {
        return encoding;
    }
    let head = &bytes[..bytes.len().min(PRESCAN_LIMIT)];
    if let Some(encoding) = prescan(head) {
        return encoding;
    }
    if std::str::from_utf8(bytes).is_ok() {
        UTF_8
    } else {
        WINDOWS_1252
    }
}

/// "Prescan a byte stream to determine its encoding".
/// <https://html.spec.whatwg.org/multipage/parsing.html#prescan-a-byte-stream-to-determine-its-encoding>
fn prescan(bytes: &[u8]) -> Option<&'static Encoding> {
    let mut pos = 0;
    while pos < bytes.len() {
        let rest = &bytes[pos..];
        if rest.starts_with(b"<!--") {
            // The dashes of `-->` may be those of `<!--`, so `<!-->` is a
            // complete comment.
            pos += find(&rest[2..], b"-->").map_or(rest.len(), |i| i + 2 + 3);
        } else if starts_with_ignore_case(rest, b"<meta")
            && rest.get(5).is_some_and(|&b| is_space(b) || b == b'/')
        {
            pos += 6;
            if let Some(encoding) = process_meta(bytes, &mut pos) {
                return Some(encoding);
            }
        } else if rest.len() >= 2
            && rest[0] == b'<'
            && (rest[1].is_ascii_alphabetic()
                || (rest[1] == b'/' && rest.get(2).is_some_and(u8::is_ascii_alphabetic)))
        {
            // A start or end tag: skip its name and attributes.
            pos += rest
                .iter()
                .position(|&b| is_space(b) || b == b'>')
                .unwrap_or(rest.len());
            while get_attribute(bytes, &mut pos).is_some() {}
        } else if rest.starts_with(b"<!") || rest.starts_with(b"</") || rest.starts_with(b"<?") {
            pos += rest
                .iter()
                .position(|&b| b == b'>')
                .map_or(rest.len(), |i| i + 1);
        } else {
            pos += 1;
        }
    }
    None
}

/// Processes the attributes of a `<meta>` tag. `pos` is just after
/// `<meta` and the following space.
fn process_meta(bytes: &[u8], pos: &mut usize) -> Option<&'static Encoding> {
    let mut seen_names: Vec<Vec<u8>> = Vec::new();
    let mut got_pragma = false;
    let mut need_pragma: Option<bool> = None;
    let mut charset: Option<&'static Encoding> = None;

    while let Some((name, value)) = get_attribute(bytes, pos) {
        if seen_names.contains(&name) {
            continue;
        }
        seen_names.push(name.clone());
        match name.as_slice() {
            b"http-equiv" => {
                if value.eq_ignore_ascii_case(b"content-type") {
                    got_pragma = true;
                }
            }
            b"content" => {
                if charset.is_none()
                    && let Some(label) = charset_from_content(&value)
                    && let Some(encoding) = Encoding::for_label(&label)
                {
                    charset = Some(encoding);
                    need_pragma = Some(true);
                }
            }
            b"charset" => {
                charset = Encoding::for_label(&value);
                need_pragma = Some(false);
            }
            _ => {}
        }
    }

    let need_pragma = need_pragma?;
    if need_pragma && !got_pragma {
        return None;
    }
    let charset = charset?;
    if charset == UTF_16BE || charset == UTF_16LE {
        return Some(UTF_8);
    }
    if charset == X_USER_DEFINED {
        return Some(WINDOWS_1252);
    }
    Some(charset)
}

/// "Get an attribute" from the prescan algorithm. Advances `pos`. Returns
/// `None` at the end of the tag (after consuming `>`) or of the input.
/// Names are lowercased.
fn get_attribute(bytes: &[u8], pos: &mut usize) -> Option<(Vec<u8>, Vec<u8>)> {
    let at = |p: usize| bytes.get(p).copied();
    while at(*pos).is_some_and(|b| is_space(b) || b == b'/') {
        *pos += 1;
    }
    match at(*pos) {
        None => return None,
        Some(b'>') => {
            *pos += 1;
            return None;
        }
        _ => {}
    }
    let mut name = Vec::new();
    let mut value = Vec::new();
    // Attribute name.
    loop {
        match at(*pos) {
            Some(b'=') if !name.is_empty() => {
                *pos += 1;
                break;
            }
            Some(b) if is_space(b) => {
                while at(*pos).is_some_and(is_space) {
                    *pos += 1;
                }
                if at(*pos) != Some(b'=') {
                    return Some((name, value));
                }
                *pos += 1;
                break;
            }
            None | Some(b'/' | b'>') => return Some((name, value)),
            Some(b) => {
                name.push(b.to_ascii_lowercase());
                *pos += 1;
            }
        }
    }
    // Attribute value.
    while at(*pos).is_some_and(is_space) {
        *pos += 1;
    }
    match at(*pos) {
        Some(quote @ (b'"' | b'\'')) => {
            *pos += 1;
            while let Some(b) = at(*pos) {
                *pos += 1;
                if b == quote {
                    return Some((name, value));
                }
                value.push(b.to_ascii_lowercase());
            }
            return Some((name, value));
        }
        None | Some(b'>') => return Some((name, value)),
        Some(_) => {}
    }
    while let Some(b) = at(*pos) {
        if is_space(b) || b == b'>' {
            break;
        }
        value.push(b.to_ascii_lowercase());
        *pos += 1;
    }
    Some((name, value))
}

/// "Extract a character encoding from a meta element".
/// <https://html.spec.whatwg.org/multipage/urls-and-fetching.html#algorithm-for-extracting-a-character-encoding-from-a-meta-element>
fn charset_from_content(content: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0;
    loop {
        let found = find_ignore_case(&content[pos..], b"charset")?;
        pos += found + b"charset".len();
        while content.get(pos).copied().is_some_and(is_space) {
            pos += 1;
        }
        if content.get(pos) == Some(&b'=') {
            pos += 1;
            break;
        }
    }
    while content.get(pos).copied().is_some_and(is_space) {
        pos += 1;
    }
    let rest = &content[pos..];
    match rest.first() {
        Some(&quote @ (b'"' | b'\'')) => {
            let end = rest[1..].iter().position(|&b| b == quote)?;
            Some(rest[1..=end].to_vec())
        }
        Some(_) => {
            let end = rest
                .iter()
                .position(|&b| is_space(b) || b == b';')
                .unwrap_or(rest.len());
            Some(rest[..end].to_vec())
        }
        None => None,
    }
}

fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn find_ignore_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

fn starts_with_ignore_case(haystack: &[u8], prefix: &[u8]) -> bool {
    haystack.len() >= prefix.len() && haystack[..prefix.len()].eq_ignore_ascii_case(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{ISO_8859_2, WINDOWS_1250};

    #[test]
    fn bom_wins() {
        let bytes = b"\xEF\xBB\xBF<meta charset=latin2>x";
        assert_eq!(sniff_encoding(bytes, Some("windows-1250")), UTF_8);
    }

    #[test]
    fn transport_charset_beats_meta() {
        assert_eq!(
            sniff_encoding(b"<meta charset=latin2>", Some("windows-1250")),
            WINDOWS_1250
        );
    }

    #[test]
    fn meta_charset_forms() {
        assert_eq!(prescan(b"<meta charset=\"ISO-8859-2\">"), Some(ISO_8859_2));
        assert_eq!(prescan(b"<META CHARSET='latin2'>"), Some(ISO_8859_2));
        assert_eq!(
            prescan(
                b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=iso-8859-2\">"
            ),
            Some(ISO_8859_2)
        );
        // content= without http-equiv is ignored.
        assert_eq!(
            prescan(b"<meta content=\"text/html; charset=iso-8859-2\">"),
            None
        );
        // UTF-16 in meta means UTF-8.
        assert_eq!(prescan(b"<meta charset=utf-16>"), Some(UTF_8));
    }

    #[test]
    fn meta_inside_comment_is_ignored() {
        assert_eq!(prescan(b"<!-- <meta charset=latin2> --><p>"), None);
    }

    #[test]
    fn short_comments_end_early() {
        assert_eq!(prescan(b"<!--><meta charset=latin2>"), Some(ISO_8859_2));
        assert_eq!(prescan(b"<!---><meta charset=latin2>"), Some(ISO_8859_2));
    }

    #[test]
    fn meta_after_other_tags() {
        let html = b"<!DOCTYPE html><html lang=en><head><title>x</title><meta charset=latin2>";
        assert_eq!(prescan(html), Some(ISO_8859_2));
    }

    #[test]
    fn default_sniffs_utf8() {
        assert_eq!(sniff_encoding("<p>č".as_bytes(), None), UTF_8);
        assert_eq!(sniff_encoding(b"<p>\xE8", None), WINDOWS_1252);
    }

    #[test]
    fn charset_from_content_variants() {
        assert_eq!(
            charset_from_content(b"text/html; charset=utf-8"),
            Some(b"utf-8".to_vec())
        );
        assert_eq!(
            charset_from_content(b"text/html;charset = 'koi8-r'"),
            Some(b"koi8-r".to_vec())
        );
        assert_eq!(charset_from_content(b"text/html"), None);
        assert_eq!(charset_from_content(b"charset=\"unterminated"), None);
    }
}
