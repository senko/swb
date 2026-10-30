//! Requests: URL, method, headers, body, destination, initiator, and the
//! Fetch modes (request mode, credentials mode, redirect mode).

use std::fmt;
use std::sync::Arc;

use url::{Origin, Url};

use crate::budget::Reservation;
use crate::headers::Headers;

/// The HTTP request method.
///
/// The standard methods are variants; any other method is
/// [`Method::Other`] with the name as given. Fetch normalizes the case of
/// `DELETE`, `GET`, `HEAD`, `OPTIONS`, `POST` and `PUT` only: `patch` stays
/// `patch` and is not [`Method::Patch`] (see [`Method::parse`]).
///
/// <https://fetch.spec.whatwg.org/#concept-method>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    /// `GET`
    Get,
    /// `HEAD`
    Head,
    /// `POST`
    Post,
    /// `PUT`
    Put,
    /// `DELETE`
    Delete,
    /// `OPTIONS`
    Options,
    /// `PATCH` (in uppercase only)
    Patch,
    /// Another method token, for example `PROPFIND` or `patch`.
    Other(String),
}

impl Method {
    /// Parses a method as `fetch()` and `Request` do: the name must be a
    /// token (RFC 9110); the case of `DELETE`, `GET`, `HEAD`, `OPTIONS`,
    /// `POST` and `PUT` is normalized to uppercase. Returns `None` if the
    /// name is not a token. The forbidden methods are not refused here (see
    /// [`Method::is_forbidden`]).
    ///
    /// <https://fetch.spec.whatwg.org/#concept-method-normalize>
    pub fn parse(name: &str) -> Option<Method> {
        if !crate::headers::is_token(name) {
            return None;
        }
        let upper = name.to_ascii_uppercase();
        Some(match upper.as_str() {
            "GET" => Method::Get,
            "HEAD" => Method::Head,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "OPTIONS" => Method::Options,
            _ if name == "PATCH" => Method::Patch,
            _ => Method::Other(name.to_owned()),
        })
    }

    /// Returns the method name as sent on the wire.
    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Options => "OPTIONS",
            Method::Patch => "PATCH",
            Method::Other(name) => name,
        }
    }

    /// Returns true for methods that do not change state on the server
    /// (`GET` and `HEAD`). Fetch calls them "safe" in "append a request
    /// `Origin` header".
    ///
    /// <https://httpwg.org/specs/rfc9110.html#safe.methods>
    pub fn is_safe(&self) -> bool {
        matches!(self, Method::Get | Method::Head)
    }

    /// Returns true for an idempotent method: a client may send the request
    /// again when it does not know whether the server received it (`GET`,
    /// `HEAD`, `OPTIONS`, `PUT`, `DELETE`; `TRACE` is refused earlier).
    ///
    /// <https://httpwg.org/specs/rfc9110.html#idempotent.methods>
    pub fn is_idempotent(&self) -> bool {
        matches!(
            self,
            Method::Get | Method::Head | Method::Options | Method::Put | Method::Delete
        )
    }

    /// Returns true for a CORS-safelisted method: `GET`, `HEAD`, `POST`.
    ///
    /// <https://fetch.spec.whatwg.org/#cors-safelisted-method>
    pub fn is_cors_safelisted(&self) -> bool {
        matches!(self, Method::Get | Method::Head | Method::Post)
    }

    /// Returns true for the forbidden methods `CONNECT`, `TRACE` and
    /// `TRACK` (compared without case).
    ///
    /// <https://fetch.spec.whatwg.org/#forbidden-method>
    pub fn is_forbidden(name: &str) -> bool {
        ["CONNECT", "TRACE", "TRACK"]
            .iter()
            .any(|m| name.eq_ignore_ascii_case(m))
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The request mode: what a cross-origin request may do.
///
/// <https://fetch.spec.whatwg.org/#concept-request-mode>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestMode {
    /// A navigation: no CORS.
    Navigate,
    /// A request to another origin is a network error.
    SameOrigin,
    /// A request to another origin gets an opaque response (subresources
    /// without `crossorigin`).
    NoCors,
    /// A request to another origin needs the CORS protocol.
    Cors,
}

impl RequestMode {
    /// The name in lowercase, as `Request.mode` has it.
    pub fn as_str(self) -> &'static str {
        match self {
            RequestMode::Navigate => "navigate",
            RequestMode::SameOrigin => "same-origin",
            RequestMode::NoCors => "no-cors",
            RequestMode::Cors => "cors",
        }
    }
}

/// When a request carries credentials (cookies).
///
/// <https://fetch.spec.whatwg.org/#concept-request-credentials-mode>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredentialsMode {
    /// Never.
    Omit,
    /// Only to the request's own origin.
    SameOrigin,
    /// Always.
    Include,
}

impl CredentialsMode {
    /// The name in lowercase, as `Request.credentials` has it.
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialsMode::Omit => "omit",
            CredentialsMode::SameOrigin => "same-origin",
            CredentialsMode::Include => "include",
        }
    }
}

/// What a request does with a redirect response.
///
/// <https://fetch.spec.whatwg.org/#concept-request-redirect-mode>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedirectMode {
    /// Follow it.
    Follow,
    /// A redirect is a network error.
    Error,
    /// A redirect is returned as an opaque-redirect response.
    Manual,
}

impl RedirectMode {
    /// The name in lowercase, as `Request.redirect` has it.
    pub fn as_str(self) -> &'static str {
        match self {
            RedirectMode::Follow => "follow",
            RedirectMode::Error => "error",
            RedirectMode::Manual => "manual",
        }
    }
}

/// How much a script may see of a response, for the current URL of the
/// request.
///
/// <https://fetch.spec.whatwg.org/#concept-request-response-tainting>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tainting {
    /// Same-origin: everything.
    Basic,
    /// Cross-origin with CORS: the CORS-safelisted and exposed headers.
    Cors,
    /// Cross-origin without CORS: nothing.
    Opaque,
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
    /// The request body, for `POST`, `PUT`, `PATCH` and other methods that
    /// have one.
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
    /// subresource request without one as cross-site. A request whose
    /// method is not `GET` or `HEAD`, or whose [`Request::tainting`] is
    /// `Cors`, sends it in the `Origin` header.
    ///
    /// <https://fetch.spec.whatwg.org/#concept-request-origin>
    pub initiator: Option<Origin>,
    /// The request mode. [`Request::get`] and [`Request::post`] set
    /// `Navigate` for [`Destination::Document`] and `NoCors` otherwise.
    pub mode: RequestMode,
    /// The credentials mode. The constructors set `Include`: the cookie
    /// jar decides by its `SameSite` rules.
    pub credentials: CredentialsMode,
    /// The redirect mode. The constructors set `Follow`.
    pub redirect: RedirectMode,
    /// The response tainting for the current URL. [`fetch_request`]
    /// (`crate::fetch_request`) sets it before each hop; it decides whether
    /// [`CredentialsMode::SameOrigin`] sends cookies and whether the
    /// request carries an `Origin` header.
    pub tainting: Tainting,
    /// True after a redirect from the initiator's origin to another origin
    /// and then to a third: the `Origin` header is `null`.
    ///
    /// <https://fetch.spec.whatwg.org/#concept-request-tainted-origin>
    pub tainted_origin: bool,
    /// True if a script gets the response (`fetch()`, `XMLHttpRequest`):
    /// [`fetch_request`](crate::fetch_request) then returns the filtered
    /// response of its [`ResponseKind`](crate::ResponseKind) (no headers,
    /// status and body for an opaque response; only the exposed headers for
    /// a CORS response). The constructors set false.
    pub exposed_to_script: bool,
    /// The memory that an API request holds outside the script heap
    /// ([`Reservation`]); the fetcher grows it with the response body it
    /// reads. `None` for the page's own loads.
    pub reservation: Option<Arc<Reservation>>,
    /// The "use-CORS-preflight flag": the request needs a preflight even if
    /// it is CORS-safelisted (an `XMLHttpRequest` whose `upload` has
    /// listeners). The constructors set false.
    ///
    /// <https://fetch.spec.whatwg.org/#request-use-cors-preflight-flag>
    pub use_cors_preflight: bool,
}

fn default_mode(destination: Destination) -> RequestMode {
    if destination == Destination::Document {
        RequestMode::Navigate
    } else {
        RequestMode::NoCors
    }
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
            mode: default_mode(destination),
            credentials: CredentialsMode::Include,
            redirect: RedirectMode::Follow,
            tainting: Tainting::Basic,
            tainted_origin: false,
            exposed_to_script: false,
            reservation: None,
            use_cors_preflight: false,
        }
    }

    /// Creates a `POST` request with `body` and a `Content-Type` header,
    /// without an initiator. The fetcher adds `Content-Length`; it adds
    /// `Origin` if [`Request::initiator`] is set.
    pub fn post(url: Url, body: Vec<u8>, content_type: &str, destination: Destination) -> Self {
        let mut headers = Headers::new();
        headers.append("Content-Type", content_type);
        Request {
            method: Method::Post,
            headers,
            body: Some(body),
            ..Request::get(url, destination)
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

    /// Returns true if the initiator is a tuple origin that is the origin
    /// of the request URL (Fetch "same origin"). A request without an
    /// initiator, or from an opaque origin, is never same-origin.
    pub fn is_same_origin(&self) -> bool {
        self.initiator
            .as_ref()
            .is_some_and(|origin| origin.is_tuple() && *origin == self.url.origin())
    }

    /// True if the request carries credentials (cookies) and stores the
    /// cookies of the response: the credentials mode `include`, or
    /// `same-origin` while the response tainting is `Basic`.
    ///
    /// <https://fetch.spec.whatwg.org/#http-network-or-cache-fetch>
    pub fn sends_credentials(&self) -> bool {
        match self.credentials {
            CredentialsMode::Include => true,
            CredentialsMode::SameOrigin => self.tainting == Tainting::Basic,
            CredentialsMode::Omit => false,
        }
    }

    /// The value of the `Origin` header that the request carries, or
    /// `None`: Fetch's "append a request `Origin` header".
    ///
    /// - With `tainting` `Cors` the header is always there. Otherwise only a
    ///   method other than `GET` and `HEAD` sends it (the default referrer
    ///   policy `strict-origin-when-cross-origin` keeps the origin).
    /// - The value is `null` after a redirect-tainted chain, for an opaque
    ///   initiator, and for a non-CORS request from an `https:` origin to a
    ///   URL that is not `https:`.
    /// - A request without an initiator has no `Origin` header.
    ///
    /// <https://fetch.spec.whatwg.org/#append-a-request-origin-header>
    pub fn origin_header(&self) -> Option<String> {
        let origin = self.initiator.as_ref()?;
        let cors = self.tainting == Tainting::Cors;
        if !cors && self.method.is_safe() {
            return None;
        }
        let downgrade = !cors
            && matches!(origin, Origin::Tuple(scheme, _, _) if scheme == "https")
            && self.url.scheme() != "https";
        Some(if self.tainted_origin || downgrade {
            "null".to_owned()
        } else {
            origin.ascii_serialization()
        })
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
        assert_eq!(request.mode, RequestMode::Navigate);
    }

    #[test]
    fn methods_are_normalized_as_fetch_does() {
        assert_eq!(Method::parse("get"), Some(Method::Get));
        assert_eq!(Method::parse("Delete"), Some(Method::Delete));
        assert_eq!(Method::parse("options"), Some(Method::Options));
        // Not normalized: `patch` stays as it is.
        assert_eq!(Method::parse("patch"), Some(Method::Other("patch".into())));
        assert_eq!(Method::parse("PATCH"), Some(Method::Patch));
        assert_eq!(Method::parse("PROPFIND").unwrap().as_str(), "PROPFIND");
        assert_eq!(Method::parse("a b"), None);
        assert_eq!(Method::parse(""), None);
        assert!(Method::is_forbidden("trace"));
        assert!(Method::is_forbidden("CONNECT"));
        assert!(!Method::is_forbidden("PUT"));
    }

    #[test]
    fn origin_header_follows_method_tainting_and_taint() {
        let url = Url::parse("https://b.test/x").unwrap();
        let a = Url::parse("https://a.test/").unwrap().origin();
        let mut request = Request::get(url, Destination::Other).with_initiator(Some(a));
        assert_eq!(request.origin_header(), None);
        request.tainting = Tainting::Cors;
        assert_eq!(request.origin_header().as_deref(), Some("https://a.test"));
        request.tainted_origin = true;
        assert_eq!(request.origin_header().as_deref(), Some("null"));
        request.tainted_origin = false;
        request.tainting = Tainting::Basic;
        request.method = Method::Put;
        assert_eq!(request.origin_header().as_deref(), Some("https://a.test"));
        // https -> http without CORS: null.
        request.url = Url::parse("http://b.test/").unwrap();
        assert_eq!(request.origin_header().as_deref(), Some("null"));
        request.initiator = None;
        assert_eq!(request.origin_header(), None);
    }
}
