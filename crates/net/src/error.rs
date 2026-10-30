//! The error type for all fetch operations.

use std::io;
use std::path::PathBuf;

use url::Url;

use crate::fetch::MAX_REDIRECTS;

/// The details of a failed CORS check ([`NetError::Cors`]).
#[derive(Debug)]
pub struct CorsError {
    /// The URL of the request that failed.
    pub url: Url,
    /// The first URL of the redirect chain, if the request was redirected.
    pub redirected_from: Option<Url>,
    /// The serialized origin of the request.
    pub origin: String,
    /// True if the preflight response failed the check.
    pub preflight: bool,
    /// What failed.
    pub failure: crate::cors::CorsFailure,
}

/// A fetch failed, so there is no response.
///
/// HTTP error statuses (4xx, 5xx) are not errors: they are returned as a
/// [`Response`](crate::Response), because a browser shows error pages.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NetError {
    /// The URL scheme is not supported (for example `ftp:`).
    #[error("unsupported URL scheme `{0}`")]
    UnsupportedScheme(String),

    /// An `about:` URL other than `about:blank`.
    #[error("unknown about: URL `{0}`")]
    UnknownAboutUrl(Url),

    /// The data: URL processor returned failure.
    #[error("invalid data: URL")]
    InvalidDataUrl,

    /// The URL has a supported scheme, but cannot be fetched. Examples: a
    /// `file:` URL with a remote host, an HTTP URL that the HTTP library
    /// rejects.
    #[error("invalid URL `{url}`: {reason}")]
    InvalidUrl {
        /// The URL that was requested.
        url: Url,
        /// Why the URL cannot be fetched.
        reason: String,
    },

    /// A redirect response has a `Location` header that is not a valid
    /// `http:` or `https:` URL.
    #[error("invalid redirect from `{url}` to `{location}`")]
    InvalidRedirect {
        /// The URL of the redirect response.
        url: Url,
        /// The value of the `Location` header.
        location: String,
    },

    /// More than [`MAX_REDIRECTS`] redirects. The URL is the first request
    /// URL.
    #[error("more than {MAX_REDIRECTS} redirects, starting at `{0}`")]
    TooManyRedirects(Url),

    /// The response body (after decompression) is larger than the limit.
    #[error("response body is larger than {limit} bytes")]
    BodyTooLarge {
        /// The limit in bytes.
        limit: u64,
    },

    /// The body cannot be decompressed. A body that is damaged after some
    /// decoded bytes is not an error: the decoded part is returned.
    #[error("cannot decode `{coding}` response body: {source}")]
    ContentDecoding {
        /// The content coding, for example `gzip`.
        coding: String,
        /// The decoder error.
        source: io::Error,
    },

    /// The connection or the whole request took too long.
    #[error("request timed out ({0})")]
    Timeout(String),

    /// The DNS lookup found no address for the host.
    #[error("host not found: {0}")]
    HostNotFound(String),

    /// The TCP connection could not be opened.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// The TLS handshake or certificate verification failed.
    #[error("TLS error: {0}")]
    Tls(String),

    /// Another HTTP protocol error.
    #[error("HTTP error: {0}")]
    Http(String),

    /// An I/O error, for example while reading a `file:` URL.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// [`ReplayFetcher`](crate::ReplayFetcher) has no entry for the URL and
    /// method.
    #[error("`{0}` is not in the fixture")]
    NotInFixture(Url),

    /// A fixture directory cannot be read or written, or its manifest is
    /// invalid.
    #[error("fixture `{}`: {message}", path.display())]
    Fixture {
        /// The file or directory that caused the error.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },

    /// The CORS check or the preflight failed (`fetch()` shows a network
    /// error and logs [`NetError::console_message`]).
    #[error("blocked by CORS policy: {}", .0.failure.reason())]
    Cors(Box<CorsError>),

    /// Fetch refused the request before sending it, or refused a redirect
    /// (mode `same-origin`, redirect mode `error`). The text is Chromium's
    /// console message.
    #[error("{0}")]
    Blocked(String),

    /// The memory budget of the document's API requests ran out
    /// ([`ByteBudget`](crate::ByteBudget)): the request or the response
    /// body does not fit.
    #[error("over the memory budget of {limit} bytes for API requests")]
    OverBudget {
        /// The budget in bytes.
        limit: usize,
    },

    /// The caller cancelled the request ([`Loader::cancel`](crate::Loader::cancel)).
    #[error("the request was aborted")]
    Aborted,

    /// A bug in swb, for example a fetcher that panicked.
    #[error("internal error: {0}")]
    Internal(String),
}

impl NetError {
    /// The message that Chromium's console shows for this error when
    /// `api` (`fetch` or `XMLHttpRequest`) failed, or `None` if the browser
    /// shows none (Chromium logs a failed resource load as
    /// `net::ERR_...`, which swb does not copy).
    pub fn console_message(&self, api: &str) -> Option<String> {
        match self {
            NetError::Cors(error) => Some(crate::cors::console_message(
                api,
                &error.url,
                error.redirected_from.as_ref(),
                &error.origin,
                error.preflight,
                &error.failure,
            )),
            NetError::Blocked(message) => Some(message.clone()),
            _ => None,
        }
    }
}
