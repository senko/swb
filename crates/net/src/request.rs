//! Requests: URL, method, headers, body and destination.

use std::fmt;

use url::Url;

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
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the resource is for. It selects the `Accept` header and appears in
/// log messages.
///
/// See <https://fetch.spec.whatwg.org/#concept-request-destination>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Destination {
    /// A page to show in a browsing context.
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
    pub headers: Headers,
    /// The request body, for `POST`.
    pub body: Option<Vec<u8>>,
    /// What the resource is for.
    pub destination: Destination,
}

impl Request {
    /// Creates a `GET` request without extra headers.
    pub fn get(url: Url, destination: Destination) -> Self {
        Request {
            url,
            method: Method::Get,
            headers: Headers::new(),
            body: None,
            destination,
        }
    }
}
