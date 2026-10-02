//! HTTP and HTTPS through `ureq` (blocking) and `rustls`.
//!
//! All HTTP traffic passes through [`HttpClient::fetch`]. A cookie jar
//! belongs here: [`request_headers`] adds the `Cookie` header and
//! [`read_response`] stores `Set-Cookie` headers.

use std::io::{self, Read as _};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use log::{debug, warn};
use ureq::http;
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};
use url::Url;

use super::{MAX_BODY_SIZE, USER_AGENT, decode};
use crate::error::NetError;
use crate::headers::Headers;
use crate::request::{Destination, Method, Request};
use crate::response::Response;

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

/// A `ureq` agent with the browser's settings.
#[derive(Debug)]
pub(crate) struct HttpClient {
    agent: ureq::Agent,
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

    fn with_proxy(proxy: Option<ureq::Proxy>) -> Self {
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(native_root_certs())
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let agent = ureq::Agent::config_builder()
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
            .new_agent();
        HttpClient { agent }
    }

    /// Performs one HTTP request. Does not follow redirects.
    pub(crate) fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let http_request = build_request(request)?;
        let result = match &request.body {
            Some(body) => self.agent.run(http_request.map(|()| body.as_slice())),
            None => self.agent.run(http_request),
        };
        let response = result.map_err(|error| map_error(error, &request.url))?;
        read_response(&request.url, response)
    }
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
fn build_request(request: &Request) -> Result<http::Request<()>, NetError> {
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
    for (name, value) in request_headers(request).iter() {
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

/// Returns the headers to send: the default headers, then the headers of
/// the request. A request header replaces the default header with the same
/// name.
fn request_headers(request: &Request) -> Headers {
    let defaults = [
        ("user-agent", USER_AGENT),
        ("accept", accept(request.destination)),
        ("accept-encoding", ACCEPT_ENCODING),
        ("accept-language", ACCEPT_LANGUAGE),
    ];
    let mut headers: Headers = defaults
        .into_iter()
        .filter(|(name, _)| !request.headers.contains(name))
        .collect();
    headers.extend(request.headers.iter());
    headers
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

/// Reads the body and converts the response.
fn read_response(url: &Url, response: http::Response<ureq::Body>) -> Result<Response, NetError> {
    let (parts, mut body) = response.into_parts();
    let headers: Headers = parts
        .headers
        .iter()
        .map(|(name, value)| (name.as_str(), decode_header_value(value.as_bytes())))
        .collect();
    let raw = read_body(url, &mut body)?;
    let body = decode::decode_body(raw, &headers, MAX_BODY_SIZE)?;
    Ok(Response {
        url: url.clone(),
        status: parts.status.as_u16(),
        headers,
        body,
    })
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
