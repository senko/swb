//! Requests: URL, method, headers, body, destination and initiator.

use std::fmt;

use url::{Origin, Url};

use crate::headers::Headers;

/// The HTTP request method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// `GET`
    Get,
    /// `POST`
    Post,
}

impl Method {
    /// Returns the method name in uppercase, as sent on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }

    /// Returns true for methods that do not change state on the server
    /// (`GET`).
    ///
    /// <https://httpwg.org/specs/rfc9110.html#safe.methods>
    pub fn is_safe(self) -> bool {
        self == Method::Get
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the resource is for. It selects the `Accept` header, decides which
/// cookies are sent (see [`Request::initiator`]) and appears in log
/// messages.
///
/// See <https://fetch.spec.whatwg.org/#concept-request-destination>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Destination {
    /// A top-level navigation: a page for the top-level browsing context.
    /// Frames, when swb supports them, need a destination of their own
    /// (Fetch's `iframe`), because cookies treat them as subresources.
    Document,
    /// A style sheet.
    Style,
    /// An image.
    Image,
    /// A web font.
    Font,
    /// A script.
    Script,
    /// Anything else.
    Other,
}

impl Destination {
    /// Returns the destination name in lowercase.
    pub fn as_str(self) -> &'static str {
        match self {
            Destination::Document => "document",
            Destination::Style => "style",
            Destination::Image => "image",
            Destination::Font => "font",
            Destination::Script => "script",
            Destination::Other => "other",
        }
    }
}

impl fmt::Display for Destination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A request for one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The URL to fetch. A fragment is allowed; it is not sent to servers.
    pub url: Url,
    /// The request method.
    pub method: Method,
    /// Headers to send, in addition to the default headers of the fetcher.
    /// A header here replaces a default header with the same name.
    /// `Cookie` and `Origin` are ignored: the fetcher computes them from
    /// its cookie jar and from [`Request::initiator`].
    pub headers: Headers,
    /// The request body, for `POST`.
    pub body: Option<Vec<u8>>,
    /// What the resource is for.
    pub destination: Destination,
    /// The origin of the document that started the request: the document
    /// that loads a subresource, or the document with the link or form
    /// that starts a navigation. `None` for a navigation that the user
    /// started (an address typed in the address bar, reload, back and
    /// forward).
    ///
    /// Cookies use it to decide whether the request is same-site
    /// (RFC 6265bis §5.2, see [`CookieJar`](crate::CookieJar)): a
    /// top-level navigation without an initiator counts as same-site, a
    /// subresource request without one as cross-site. A `POST` request with
    /// an initiator sends it in the `Origin` header.
    ///
    /// <https://fetch.spec.whatwg.org/#concept-request-origin>
    pub initiator: Option<Origin>,
}

impl Request {
    /// Creates a `GET` request without extra headers and without an
    /// initiator.
    pub fn get(url: Url, destination: Destination) -> Self {
        Request {
            url,
            method: Method::Get,
            headers: Headers::new(),
            body: None,
            destination,
            initiator: None,
        }
    }

    /// Creates a `POST` request with `body` and a `Content-Type` header,
    /// without an initiator. The fetcher adds `Content-Length`; it adds
    /// `Origin` if [`Request::initiator`] is set.
    pub fn post(url: Url, body: Vec<u8>, content_type: &str, destination: Destination) -> Self {
        let mut headers = Headers::new();
        headers.append("Content-Type", content_type);
        Request {
            url,
            method: Method::Post,
            headers,
            body: Some(body),
            destination,
            initiator: None,
        }
    }

    /// Sets [`Request::initiator`].
    #[must_use]
    pub fn with_initiator(mut self, initiator: Option<Origin>) -> Self {
        self.initiator = initiator;
        self
    }

    /// Returns true if the request loads a page into the top-level
    /// browsing context.
    pub fn is_top_level_navigation(&self) -> bool {
        self.destination == Destination::Document
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_sets_method_body_and_content_type() {
        let url = Url::parse("https://a.test/login").unwrap();
        let request = Request::post(
            url.clone(),
            b"a=1".to_vec(),
            "application/x-www-form-urlencoded",
            Destination::Document,
        )
        .with_initiator(Some(url.origin()));
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.body.as_deref(), Some(&b"a=1"[..]));
        assert_eq!(
            request.headers.get("content-type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(request.initiator, Some(url.origin()));
        assert!(request.is_top_level_navigation());
        assert!(!Method::Post.is_safe());
        assert!(Method::Get.is_safe());
    }
}
