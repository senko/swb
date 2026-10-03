//! Responses and their content type.

use url::Url;

use crate::headers::Headers;
use crate::mime::{self, MimeType};

/// The response to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The URL of the request that produced this response. After
    /// [`fetch_following_redirects`](crate::fetch_following_redirects), this
    /// is the final URL after all redirects. It keeps the fragment of the
    /// request URL.
    pub url: Url,
    /// The HTTP status code. Fetchers set 200 for successful `file:`,
    /// `data:` and `about:` responses and 404 for missing files.
    pub status: u16,
    /// The response headers. For HTTP, these are the headers as received,
    /// including `Content-Encoding`, although the body is already decoded.
    pub headers: Headers,
    /// The response body, decompressed.
    pub body: Vec<u8>,
    /// True if the response came after one or more redirects (Fetch's
    /// response "URL list" has more than one URL). Fetchers set false;
    /// [`fetch_following_redirects`](crate::fetch_following_redirects)
    /// sets it.
    pub redirected: bool,
}

impl Response {
    /// Returns true for status codes 200 to 299.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Returns true for the redirect status codes 301, 302, 303, 307 and 308.
    ///
    /// <https://fetch.spec.whatwg.org/#redirect-status>
    pub fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308)
    }

    /// Returns the content type from the `Content-Type` headers, or `None`
    /// if there is no valid one.
    ///
    /// <https://fetch.spec.whatwg.org/#concept-header-extract-mime-type>
    pub fn content_type(&self) -> Option<ContentType> {
        mime::extract_mime_type(&self.headers).map(ContentType::from)
    }
}

/// The parts of a MIME type that the browser uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentType {
    /// `type/subtype` in ASCII lowercase, for example `text/html`.
    pub essence: String,
    /// The value of the `charset` parameter, in its original case.
    pub charset: Option<String>,
}

impl ContentType {
    /// Parses one MIME type string, for example `text/html; charset=utf-8`.
    /// Returns `None` if it is not a valid MIME type.
    ///
    /// <https://mimesniff.spec.whatwg.org/#parse-a-mime-type>
    pub fn parse(value: &str) -> Option<Self> {
        MimeType::parse(value).map(ContentType::from)
    }
}

impl From<MimeType> for ContentType {
    fn from(mime_type: MimeType) -> Self {
        ContentType {
            charset: mime_type.parameter("charset").map(str::to_string),
            essence: mime_type.essence(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, content_type: &[&str]) -> Response {
        Response {
            url: Url::parse("https://example.com/").unwrap(),
            status,
            headers: content_type.iter().map(|v| ("content-type", *v)).collect(),
            body: Vec::new(),
            redirected: false,
        }
    }

    #[test]
    fn content_type_from_header() {
        let ct = response(200, &["Text/HTML; Charset=\"UTF-8\""]).content_type();
        assert_eq!(
            ct,
            Some(ContentType {
                essence: "text/html".to_string(),
                charset: Some("UTF-8".to_string())
            })
        );
        assert_eq!(response(200, &[]).content_type(), None);
        assert_eq!(response(200, &["nonsense"]).content_type(), None);
        let ct = response(200, &["image/png"]).content_type().unwrap();
        assert_eq!(ct.essence, "image/png");
        assert_eq!(ct.charset, None);
    }

    #[test]
    fn status_classes() {
        assert!(response(200, &[]).is_success());
        assert!(response(204, &[]).is_success());
        assert!(!response(304, &[]).is_success());
        assert!(!response(404, &[]).is_success());
        for status in [301, 302, 303, 307, 308] {
            assert!(response(status, &[]).is_redirect());
        }
        for status in [200, 300, 304, 305, 306] {
            assert!(!response(status, &[]).is_redirect());
        }
    }

    #[test]
    fn parse_content_type_string() {
        let ct = ContentType::parse("text/css;charset=windows-1250").unwrap();
        assert_eq!(ct.essence, "text/css");
        assert_eq!(ct.charset.as_deref(), Some("windows-1250"));
        assert_eq!(ContentType::parse("text"), None);
    }
}
