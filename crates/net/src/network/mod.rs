//! [`NetworkFetcher`]: fetches from the network and from the local file
//! system.

mod decode;
mod file;
mod http;
#[cfg(test)]
mod tests;

use log::debug;

use crate::builtin;
use crate::cookies::CookieJar;
use crate::error::NetError;
use crate::fetch::Fetcher;
use crate::request::Request;
use crate::response::Response;

/// The `User-Agent` header value.
pub const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64) swb/",
    env!("CARGO_PKG_VERSION")
);

/// The maximum size of a response body in bytes, after decompression.
/// Larger bodies cause [`NetError::BodyTooLarge`].
pub const MAX_BODY_SIZE: u64 = 64 * 1024 * 1024;

/// Fetches `http:`, `https:`, `file:`, `data:` and `about:` URLs.
///
/// - HTTP and HTTPS use `ureq` with `rustls`. Certificates are verified
///   against the root certificates of the operating system. The fetcher
///   does not follow redirects and returns 4xx and 5xx responses as
///   responses. Bodies are decompressed (`gzip`, `deflate`, `br`).
///   Timeouts: 15 s to connect, 60 s for the whole request. Proxies from the
///   environment (`HTTPS_PROXY`, `NO_PROXY` and so on) are used, except for
///   loopback hosts (`localhost`, `*.localhost`, `127.0.0.0/8`, `::1`),
///   which are always connected directly.
/// - `file:` URLs read the file. The `Content-Type` comes from the file
///   name extension; an unknown extension gives no `Content-Type` header.
///   A missing file gives a 404 response. A directory gives an HTML listing.
/// - `data:` URLs follow <https://fetch.spec.whatwg.org/#data-urls>.
/// - `about:blank` is an empty HTML document. Other `about:` URLs are
///   errors.
///
/// Each fetcher has its own [`CookieJar`], empty at the start, for all its
/// HTTP and HTTPS requests ([`Fetcher::cookie_jar`] returns it). A browser
/// session uses one fetcher, so that all its pages share the cookies. There
/// is no cache yet.
#[derive(Debug)]
pub struct NetworkFetcher {
    http: http::HttpClient,
}

impl NetworkFetcher {
    /// Creates a fetcher with an empty cookie jar. Loads the root
    /// certificates of the operating system the first time it is called in
    /// a process.
    pub fn new() -> Self {
        NetworkFetcher {
            http: http::HttpClient::new(),
        }
    }

    /// Creates a fetcher that ignores proxy settings in the environment, so
    /// that tests against a local server work everywhere.
    #[cfg(test)]
    pub(crate) fn without_proxy() -> Self {
        NetworkFetcher {
            http: http::HttpClient::without_proxy(),
        }
    }
}

impl Default for NetworkFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetcher for NetworkFetcher {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        debug!(
            "fetch {} {} ({})",
            request.method, request.url, request.destination
        );
        let result = match request.url.scheme() {
            "http" | "https" => self.http.fetch(request),
            "file" => file::fetch(&request.url),
            _ => builtin::fetch(request).unwrap_or_else(|| {
                Err(NetError::UnsupportedScheme(
                    request.url.scheme().to_string(),
                ))
            }),
        };
        match &result {
            Ok(response) => debug!("{} {}", response.status, request.url),
            Err(error) => debug!("{} failed: {error}", request.url),
        }
        result
    }

    /// Always returns the jar: every network fetcher has one.
    fn cookie_jar(&self) -> Option<&CookieJar> {
        Some(self.http.cookie_jar())
    }
}
