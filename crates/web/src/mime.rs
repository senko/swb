//! Small helpers for MIME type strings, shared by `navigator` (the
//! `sendBeacon` content type) and `xhr` (the response type). They do not
//! implement the MIME Sniffing parser
//! (<https://mimesniff.spec.whatwg.org/#parse-a-mime-type>): they cut the
//! essence at the first `;`, as the callers need.

/// The essence of a MIME type string: the text before the first `;`,
/// without surrounding whitespace, in ASCII lower case.
pub(crate) fn essence(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::essence;

    #[test]
    fn the_essence_is_lower_case_and_has_no_parameters() {
        assert_eq!(essence("Text/HTML; charset=utf-8"), "text/html");
        assert_eq!(essence("  text/plain "), "text/plain");
        assert_eq!(essence(";charset=x"), "");
        assert_eq!(essence(""), "");
    }
}
