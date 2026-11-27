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
use ureq::RequestExt as _;
use ureq::http;
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};
use url::Url;

use super::{MAX_BODY_SIZE, USER_AGENT, decode};
use crate::cookies::CookieJar;
use crate::error::NetError;
use crate::fetch::StreamResponse;
use crate::headers::Headers;
use crate::request::{Destination, Request};
use crate::response::Response;
use crate::site;

/// The time to open the connection, including the TLS handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// The time for the whole request, including the response body. A streamed
/// body ([`HttpClient::fetch_stream`]) has no such limit; it has this time
/// for the response head.
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
    /// Like `agent` and `direct`, but never reuses a connection: for the
    /// methods that must not be sent twice (see [`HttpClient::fetch`]).
    fresh: ureq::Agent,
    fresh_direct: ureq::Agent,
    cookies: CookieJar,
    /// The time limit of a request (a stream: of its head); tests make it
    /// short.
    request_timeout: Duration,
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

    /// Creates a client that connects directly, with another time limit for
    /// a request.
    #[cfg(test)]
    pub(crate) fn without_proxy_with_timeout(request_timeout: Duration) -> Self {
        Self::with_proxy_and_timeout(None, request_timeout)
    }

    /// Creates a client that sends requests through `proxy`, except
    /// requests to loopback hosts ([`site::is_loopback`]).
    pub(super) fn with_proxy(proxy: Option<ureq::Proxy>) -> Self {
        Self::with_proxy_and_timeout(proxy, REQUEST_TIMEOUT)
    }

    fn with_proxy_and_timeout(proxy: Option<ureq::Proxy>, request_timeout: Duration) -> Self {
        HttpClient {
            agent: build_agent(proxy.clone(), true, request_timeout),
            direct: build_agent(None, true, request_timeout),
            fresh: build_agent(proxy, false, request_timeout),
            fresh_direct: build_agent(None, false, request_timeout),
            cookies: CookieJar::new(),
            request_timeout,
        }
    }

    /// Returns the cookie jar.
    pub(crate) fn cookie_jar(&self) -> &CookieJar {
        &self.cookies
    }

    /// Performs one HTTP request. Does not follow redirects.
    ///
    /// A request is never sent twice when the server may have received it.
    /// `ureq` does not tell whether a connection came from its pool, so:
    /// the idempotent methods ([`Method::is_idempotent`]) use the pool, and
    /// a failure that looks like a closed pooled connection (an HTTP/1.0
    /// server, a keep-alive timeout) is retried on a new connection, as
    /// browsers do; every other method (`POST`, `PATCH`, ...) always
    /// opens a new connection and is never retried.
    pub(crate) fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let (status, headers, mut body) = self.send(request, Timing::Agent)?;
        let account = |bytes: usize| account(request, bytes);
        let raw = read_body(&request.url, &mut body, &account)?;
        let body = decode::decode_body(raw, &headers, MAX_BODY_SIZE, &account)?;
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
            redirected: false,
        })
    }

    /// Like [`HttpClient::fetch`], but the body is a reader over the open
    /// connection: no size limit, no decompression (a media request sends
    /// `Accept-Encoding: identity`).
    ///
    /// Time limits: the connection and the response head keep their limits
    /// (15 s, [`REQUEST_TIMEOUT`]). The body has no limit on its total time
    /// when the request asks for the bytes from the start of the resource
    /// (no `Range`, or `bytes=0-`): the server may answer 200 and send a
    /// stream that has no reopen (a long file, a paused video). A request
    /// for a later range is a seekable resource's: its body has
    /// [`REQUEST_TIMEOUT`] in total, and the caller reopens where the body
    /// stopped. `ureq` has no limit for one read (`timeout_recv_body` is a
    /// total), so a server that stalls on a body without a limit holds the
    /// reading thread until it closes the connection (ADR 0047, notes of
    /// feature 4).
    pub(crate) fn fetch_stream(&self, request: &Request) -> Result<StreamResponse, NetError> {
        let body_time = (!from_start(request)).then_some(self.request_timeout);
        let (status, headers, body) = self.send(request, Timing::Stream { body: body_time })?;
        if headers
            .get("content-encoding")
            .is_some_and(|coding| !coding.trim().eq_ignore_ascii_case("identity"))
        {
            warn!(
                "{}: a streamed body with a content coding is not decoded",
                request.url
            );
        }
        let reader = body.into_with_config().limit(u64::MAX).reader();
        Ok(StreamResponse {
            head: Response {
                url: request.url.clone(),
                status,
                headers,
                body: Vec::new(),
                redirected: false,
            },
            body: Box::new(reader),
        })
    }

    /// Sends `request` and returns the status, the headers and the open
    /// body. The cookies of the response are stored.
    fn send(
        &self,
        request: &Request,
        timing: Timing,
    ) -> Result<(u16, Headers, ureq::Body), NetError> {
        let credentials = request.sends_credentials();
        let cookie = if credentials {
            self.cookies.cookie_header(request)
        } else {
            None
        };
        let retries = if request.method.is_idempotent() {
            RETRIES
        } else {
            0
        };
        let agent = self.agent_for(request, retries > 0);
        let mut attempt = 0;
        let result = loop {
            let http_request = build_request(request, cookie.clone())?;
            let result = match &request.body {
                Some(body) => self.run(agent, http_request.map(|()| body.as_slice()), timing),
                None => self.run(agent, http_request, timing),
            };
            attempt += 1;
            match result {
                Err(ureq::Error::Io(error))
                    if attempt <= retries && is_stale_connection(&error) =>
                {
                    // A pooled connection that the server closed: the next
                    // attempt opens a new one.
                    debug!("{}: stale connection ({error}), retrying", request.url);
                }
                other => break other,
            }
        };
        let response = result.map_err(|error| map_error(error, &request.url))?;
        let (parts, body) = response.into_parts();
        let headers = convert_headers(&parts.headers);
        if credentials {
            self.cookies.store_response_cookies(request, &headers);
        }
        Ok((parts.status.as_u16(), headers, body))
    }

    /// Runs `http_request` on `agent` with the limits of `timing`.
    fn run<S: ureq::AsSendBody>(
        &self,
        agent: &ureq::Agent,
        http_request: http::Request<S>,
        timing: Timing,
    ) -> Result<http::Response<ureq::Body>, ureq::Error> {
        match timing {
            Timing::Agent => agent.run(http_request),
            Timing::Stream { body } => http_request
                .with_agent(agent)
                .configure()
                .timeout_global(None)
                .timeout_recv_response(Some(self.request_timeout))
                .timeout_recv_body(body)
                .run(),
        }
    }

    /// The agent for `request`: the proxy or the direct one (loopback), the
    /// pooled one if `pooled`.
    fn agent_for(&self, request: &Request, pooled: bool) -> &ureq::Agent {
        match (site::is_loopback(&request.url), pooled) {
            (true, true) => &self.direct,
            (false, true) => &self.agent,
            (true, false) => &self.fresh_direct,
            (false, false) => &self.fresh,
        }
    }
}

/// Which time limits a request has.
#[derive(Clone, Copy)]
enum Timing {
    /// The agent's: [`REQUEST_TIMEOUT`] for the whole request.
    Agent,
    /// A streamed body: the response head has the request time limit, the
    /// body this total time (none: no limit).
    Stream { body: Option<Duration> },
}

/// True when `request` asks for the resource from its first byte: no
/// `Range`, or `bytes=0-`.
fn from_start(request: &Request) -> bool {
    request
        .headers
        .get("range")
        .and_then(crate::range::parse_range)
        .is_none_or(|spec| matches!(spec, crate::range::RangeSpec::From(0)))
}

/// How many times an idempotent request is repeated after a pooled
/// connection turned out to be closed. Up to three connections of one host
/// wait in the pool, so a burst of requests can meet more than one closed
/// connection.
const RETRIES: usize = 2;

/// Counts the bytes that a response of `request` holds against the budget
/// of the request, if it has one.
fn account(request: &Request, bytes: usize) -> Result<(), NetError> {
    match &request.reservation {
        Some(reservation) if !reservation.set_response(bytes) => {
            warn!(
                "{}: the response does not fit in the memory budget of {} bytes for API requests",
                request.url,
                reservation.limit()
            );
            Err(NetError::OverBudget {
                limit: reservation.limit(),
            })
        }
        _ => Ok(()),
    }
}

/// True if `error` says that the server closed the connection before the
/// response (a request on a pooled connection that the server had closed).
fn is_stale_connection(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
    )
}

/// Creates a `ureq` agent with the browser's settings.
/// With `pooled` false, no connection is kept for a later request.
fn build_agent(proxy: Option<ureq::Proxy>, pooled: bool, request_timeout: Duration) -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(native_root_certs())
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .proxy(proxy)
        .max_idle_connections(if pooled { 10 } else { 0 })
        // fetch_following_redirects handles redirects.
        .max_redirects(0)
        // 4xx and 5xx are responses, not errors.
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(request_timeout))
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
    *http_request.method_mut() = http::Method::from_bytes(request.method.as_str().as_bytes())
        .map_err(|error| NetError::InvalidUrl {
            url: request.url.clone(),
            reason: format!("invalid method {}: {error}", request.method),
        })?;
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
        (
            "accept-encoding",
            Some(accept_encoding(request.destination).to_owned()),
        ),
        ("accept-language", Some(ACCEPT_LANGUAGE.to_owned())),
        ("origin", request.origin_header()),
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

/// Media is never compressed: the byte source reads ranges of the file
/// as stored (ADR 0047 section 5).
fn accept_encoding(destination: Destination) -> &'static str {
    match destination {
        Destination::Video | Destination::Audio => "identity",
        _ => ACCEPT_ENCODING,
    }
}

fn accept(destination: Destination) -> &'static str {
    match destination {
        Destination::Document | Destination::IFrame => ACCEPT_DOCUMENT,
        Destination::Style => ACCEPT_STYLE,
        Destination::Image => ACCEPT_IMAGE,
        Destination::Font
        | Destination::Script
        | Destination::Video
        | Destination::Audio
        | Destination::Other => ACCEPT_OTHER,
    }
}

/// Reads a response body. A body that ends early (connection closed before
/// `Content-Length` bytes) is returned as far as it was received, as
/// browsers display partial pages and images.
/// `account` is called with the size of the body while it grows.
fn read_body(
    url: &Url,
    body: &mut ureq::Body,
    account: decode::Account<'_>,
) -> Result<Vec<u8>, NetError> {
    let mut raw = Vec::new();
    let mut reader = body.with_config().limit(MAX_BODY_SIZE).reader();
    let mut chunk = vec![0; decode::CHUNK_SIZE];
    let result = loop {
        match reader.read(&mut chunk) {
            Ok(0) => break Ok(()),
            Ok(n) => {
                raw.extend_from_slice(chunk.get(..n).unwrap_or_default());
                account(raw.len())?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => break Err(error),
        }
    };
    match result {
        Ok(()) => Ok(raw),
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
