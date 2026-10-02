//! File name extensions and their MIME types.
//!
//! `file:` URLs get their `Content-Type` from the extension. Fixture body
//! files get an extension from the content type. Both use the table here.

/// Extension (lowercase, without the dot) and MIME type. When two extensions
/// map to the same MIME type, the first one is used for fixture files.
const TABLE: [(&str, &str); 21] = [
    ("html", "text/html"),
    ("htm", "text/html"),
    ("xhtml", "application/xhtml+xml"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("mjs", "text/javascript"),
    ("json", "application/json"),
    ("txt", "text/plain"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("ico", "image/x-icon"),
    ("bmp", "image/bmp"),
    ("avif", "image/avif"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
];

/// MIME types that servers use as alternatives to the ones in [`TABLE`].
const ALIASES: [(&str, &str); 8] = [
    ("application/javascript", "js"),
    ("application/x-javascript", "js"),
    ("image/vnd.microsoft.icon", "ico"),
    ("application/font-woff", "woff"),
    ("application/x-font-woff", "woff"),
    ("application/font-woff2", "woff2"),
    ("application/x-font-ttf", "ttf"),
    ("application/x-font-otf", "otf"),
];

/// Returns the MIME type for a file name extension (case-insensitive).
pub(crate) fn mime_type_for_extension(extension: &str) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(ext, _)| ext.eq_ignore_ascii_case(extension))
        .map(|(_, mime)| *mime)
}

/// Returns the file name extension for a MIME type essence (`type/subtype`,
/// lowercase).
pub(crate) fn extension_for_mime_type(essence: &str) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(_, mime)| *mime == essence)
        .map(|(ext, _)| *ext)
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(mime, _)| *mime == essence)
                .map(|(_, ext)| *ext)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_to_mime_type() {
        assert_eq!(mime_type_for_extension("html"), Some("text/html"));
        assert_eq!(mime_type_for_extension("HTM"), Some("text/html"));
        assert_eq!(mime_type_for_extension("woff2"), Some("font/woff2"));
        assert_eq!(mime_type_for_extension("exe"), None);
    }

    #[test]
    fn mime_type_to_extension() {
        assert_eq!(extension_for_mime_type("text/html"), Some("html"));
        assert_eq!(extension_for_mime_type("image/jpeg"), Some("jpg"));
        assert_eq!(extension_for_mime_type("text/javascript"), Some("js"));
        assert_eq!(
            extension_for_mime_type("application/javascript"),
            Some("js")
        );
        assert_eq!(extension_for_mime_type("application/octet-stream"), None);
    }
}
