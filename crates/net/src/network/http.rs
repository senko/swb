//! HTTP and HTTPS through `ureq` (blocking) and `rustls`.
//!
//! All HTTP traffic passes through [`HttpClient::fetch`]. The client's
//! [`CookieJar`] adds the `Cookie` header to each request and stores the
//! `Set-Cookie` headers of each response as soon as the headers arrive,
//! before the body. [`fetch_following_redirects`](crate::fetch_following_redirects)
//! calls the client once per redirect hop, so every hop sends and stores
//! cookies.

use std::io::{self, Read as _};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use log::{debug, warn};
use ureq::http;
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};
use url::Url;

use super::{MAX_BODY_SIZE, USER_AGENT, decode};
use crate::cookies::CookieJar;
use crate::error::NetError;
use crate::headers::Headers;
use crate::request::{Destination, Method, Request};
use crate::response::Response;
use crate::site;

/// The time to open the connection, including the TLS handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// The time for the whole request, including the response body.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The `Accept` header for documents. Chromium also sends
/// `application/signed-exchange;v=b3;q=0.7`; swb does not support signed
/// exchanges, so it does not ask for them.
const ACCEPT_DOCUMENT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8";

/// The `Accept` header for style sheets, as Chromium sends it.
const ACCEPT_STYLE: &str = "text/css,*/*;q=0.1";

/// The `Accept` header for images, based on what Chromium sends.
/// AVIF is not advertised: the image decoder does not support it.
const ACCEPT_IMAGE: &str = "image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8";

/// The `Accept` header for other destinations.
const ACCEPT_OTHER: &str = "*/*";

/// The content codings that [`decode`] supports.
const ACCEPT_ENCODING: &str = "gzip, deflate, br";

const ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

/// `ureq` agents with the browser's settings, and the cookie jar.
#[derive(Debug)]
pub(crate) struct HttpClient {
    /// Uses the proxy, if there is one.
    agent: ureq::Agent,
    /// Connects directly, for loopback hosts.
    direct: ureq::Agent,
    cookies: CookieJar,
}

impl HttpClient {
    /// Creates a client that uses the proxy settings from the environment.
    pub(crate) fn new() -> Self {
        Self::with_proxy(ureq::Proxy::try_from_env())
    }

    /// Creates a client that connects directly.
    #[cfg(test)]
    pub(crate) fn without_proxy() -> Self {
        Self::with_proxy(None)
    }

    /// Creates a client that sends requests through `proxy`, except
    /// requests to loopback hosts ([`site::is_loopback`]).
    pub(super) fn with_proxy(proxy: Option<ureq::Proxy>) -> Self {
        HttpClient {
            agent: build_agent(proxy),
            direct: build_agent(None),
            cookies: CookieJar::new(),
        }
    }

    /// Returns the cookie jar.
    pub(crate) fn cookie_jar(&self) -> &CookieJar {
        &self.cookies
    }

    /// Performs one HTTP request. Does not follow redirects.
    pub(crate) fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let cookie = self.cookies.cookie_header(request);
        let http_request = build_request(request, cookie)?;
        let agent = if site::is_loopback(&request.url) {
            &self.direct
        } else {
            &self.agent
        };
        let result = match &request.body {
            Some(body) => agent.run(http_request.map(|()| body.as_slice())),
            None => agent.run(http_request),
        };
        let response = result.map_err(|error| map_error(error, &request.url))?;
        let (parts, mut body) = response.into_parts();
        let headers = convert_headers(&parts.headers);
        self.cookies.store_response_cookies(request, &headers);
        let raw = read_body(&request.url, &mut body)?;
        let body = decode::decode_body(raw, &headers, MAX_BODY_SIZE)?;
        Ok(Response {
            url: request.url.clone(),
            status: parts.status.as_u16(),
            headers,
            body,
        })
    }
}

/// Creates a `ureq` agent with the browser's settings.
fn build_agent(proxy: Option<ureq::Proxy>) -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(native_root_certs())
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .proxy(proxy)
        // fetch_following_redirects handles redirects.
        .max_redirects(0)
        // 4xx and 5xx are responses, not errors.
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(REQUEST_TIMEOUT))
        // Sent to proxies; requests set their own User-Agent header.
        .user_agent(USER_AGENT)
        .tls_config(tls)
        .build()
        .new_agent()
}

/// Loads the root certificates of the operating system once per process.
fn native_root_certs() -> RootCerts {
    static ROOTS: OnceLock<RootCerts> = OnceLock::new();
    ROOTS
        .get_or_init(|| {
            let result = rustls_native_certs::load_native_certs();
            for error in &result.errors {
                warn!("cannot load root certificates: {error}");
            }
            if result.certs.is_empty() {
                warn!("no root certificates found, HTTPS requests will fail");
            }
            debug!("loaded {} root certificates", result.certs.len());
            let certs: Vec<Certificate<'static>> = result
                .certs
                .iter()
                .map(|cert| Certificate::from_der(cert.as_ref()).to_owned())
                .collect();
            RootCerts::new_with_certs(&certs)
        })
        .clone()
}

/// Converts the request to an `http` crate request without the body.
/// `cookie` is the value of the `Cookie` header from the jar.
fn build_request(request: &Request, cookie: Option<String>) -> Result<http::Request<()>, NetError> {
    let mut target = request.url.clone();
    target.set_fragment(None);
    let uri = http::Uri::try_from(target.as_str()).map_err(|error| NetError::InvalidUrl {
        url: request.url.clone(),
        reason: error.to_string(),
    })?;
    let mut http_request = http::Request::new(());
    *http_request.method_mut() = match request.method {
        Method::Get => http::Method::GET,
        Method::Post => http::Method::POST,
    };
    *http_request.uri_mut() = uri;
    let headers = http_request.headers_mut();
    for (name, value) in request_headers(request, cookie).iter() {
        match (
            http::HeaderName::from_bytes(name.as_bytes()),
            http::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            (Ok(name), Ok(value)) => {
                headers.append(name, value);
            }
            _ => warn!("invalid request header {name}: {value:?}, not sent"),
        }
    }
    Ok(http_request)
}

/// Request headers that only the fetcher sets: [`request_headers`] ignores
/// them in [`Request::headers`]. They are "forbidden request-header" names
/// in Fetch. A caller's value would replace the jar's cookies and be sent
/// again on every redirect hop, also to other sites.
///
/// <https://fetch.spec.whatwg.org/#forbidden-request-header>
const COMPUTED_HEADERS: [&str; 2] = ["cookie", "origin"];

/// Returns the headers to send: the default headers, then the headers of
/// the request. A request header replaces the default header with the same
/// name, except the [`COMPUTED_HEADERS`]: the defaults include `Origin`
/// (see [`origin_header`]) and `Cookie` (from the jar), and `Cookie` and
/// `Origin` in the request are dropped. `ureq` adds `Host` and, for a
/// request with a body, `Content-Length`.
fn request_headers(request: &Request, cookie: Option<String>) -> Headers {
    let defaults = [
        ("user-agent", Some(USER_AGENT.to_owned())),
        ("accept", Some(accept(request.destination).to_owned())),
        ("accept-encoding", Some(ACCEPT_ENCODING.to_owned())),
        ("accept-language", Some(ACCEPT_LANGUAGE.to_owned())),
        ("origin", origin_header(request)),
        ("cookie", cookie),
    ];
    let is_computed = |name: &str| COMPUTED_HEADERS.contains(&name);
    let mut headers: Headers = defaults
        .into_iter()
        .filter_map(|(name, value)| Some((name, value?)))
        .filter(|(name, _)| is_computed(name) || !request.headers.contains(name))
        .collect();
    for (name, value) in request.headers.iter() {
        if is_computed(name) {
            debug!("{}: ignoring the request header {name}", request.url);
        } else {
            headers.append(name, value);
        }
    }
    headers
}

/// Returns the value of the `Origin` header: the initiator's origin for a
/// request whose method is not `GET`, as Fetch sends it with the default
/// referrer policy (`strict-origin-when-cross-origin`). It is `null` for an
/// opaque origin and for a request from an `https:` origin to a URL that
/// is not `https:`. A request without an initiator has no `Origin` header.
///
/// Not implemented: the "redirect-tainted origin" (`null` after a redirect
/// chain that went from the initiator's origin to another origin and then
/// to a third one).
///
/// <https://fetch.spec.whatwg.org/#append-a-request-origin-header>
fn origin_header(request: &Request) -> Option<String> {
    if request.method.is_safe() {
        return None;
    }
    let origin = request.initiator.as_ref()?;
    let downgrade = matches!(origin, url::Origin::Tuple(scheme, _, _) if scheme == "https")
        && request.url.scheme() != "https";
    Some(if downgrade {
        "null".to_owned()
    } else {
        origin.ascii_serialization()
    })
}

fn accept(destination: Destination) -> &'static str {
    match destination {
        Destination::Document => ACCEPT_DOCUMENT,
        Destination::Style => ACCEPT_STYLE,
        Destination::Image => ACCEPT_IMAGE,
        Destination::Font | Destination::Script | Destination::Other => ACCEPT_OTHER,
    }
}

/// Reads a response body. A body that ends early (connection closed before
/// `Content-Length` bytes) is returned as far as it was received, as
/// browsers display partial pages and images.
fn read_body(url: &Url, body: &mut ureq::Body) -> Result<Vec<u8>, NetError> {
    let mut raw = Vec::new();
    let mut reader = body.with_config().limit(MAX_BODY_SIZE).reader();
    match reader.read_to_end(&mut raw) {
        Ok(_) => Ok(raw),
        Err(error) => {
            let error = match error.downcast::<ureq::Error>() {
                Ok(ureq_error) => map_error(ureq_error, url),
                Err(io_error) => map_io_error(io_error),
            };
            match error {
                NetError::BodyTooLarge { .. } => Err(error),
                _ if !raw.is_empty() => {
                    warn!("{url}: body truncated after {} bytes: {error}", raw.len());
                    Ok(raw)
                }
                _ => Err(error),
            }
        }
    }
}

/// Converts the response headers.
fn convert_headers(headers: &http::HeaderMap) -> Headers {
    headers
        .iter()
        .map(|(name, value)| (name.as_str(), decode_header_value(value.as_bytes())))
        .collect()
}

/// Decodes a header value as UTF-8 if it is valid UTF-8, otherwise as
/// Latin-1.
fn decode_header_value(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(value) => value.to_string(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

fn map_error(error: ureq::Error, url: &Url) -> NetError {
    match error {
        ureq::Error::Timeout(timeout) => NetError::Timeout(timeout.to_string()),
        ureq::Error::HostNotFound => {
            NetError::HostNotFound(url.host_str().unwrap_or_default().to_string())
        }
        ureq::Error::ConnectionFailed => NetError::ConnectionFailed(format!(
            "{}:{}",
            url.host_str().unwrap_or_default(),
            url.port_or_known_default().unwrap_or_default()
        )),
        ureq::Error::BodyExceedsLimit(_) => NetError::BodyTooLarge {
            limit: MAX_BODY_SIZE,
        },
        ureq::Error::BadUri(reason) => NetError::InvalidUrl {
            url: url.clone(),
            reason,
        },
        ureq::Error::Tls(message) => NetError::Tls(message.to_string()),
        ureq::Error::Rustls(error) => NetError::Tls(error.to_string()),
        ureq::Error::Io(error) => map_io_error(error),
        other => NetError::Http(other.to_string()),
    }
}

/// rustls reports handshake and certificate errors as I/O errors.
fn map_io_error(error: io::Error) -> NetError {
    if let Some(tls_error) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        return NetError::Tls(tls_error.to_string());
    }
    if matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkUnreachable
    ) {
        return NetError::ConnectionFailed(error.to_string());
    }
    NetError::Io(error)
}
