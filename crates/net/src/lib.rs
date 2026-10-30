//! Resource loading: `http`, `https`, `file`, `data` and `about` URLs,
//! cookies, and replay from fixtures.
//!
//! The central abstraction is the [`Fetcher`] trait: one request, one
//! response, no redirects. Implementations:
//!
//! - [`NetworkFetcher`]: the real network (HTTP and HTTPS through `ureq`
//!   and `rustls`), local files, `data:` and `about:blank`.
//! - [`ReplayFetcher`]: answers requests from a fixture directory. Tests
//!   use it, so that they never access the network.
//! - [`RecordingFetcher`]: wraps another fetcher and stores every response
//!   in a fixture directory.
//! - [`ExtendingFetcher`]: answers from a fixture directory and fetches and
//!   records only the requests that it does not have.
//!
//! [`fetch_following_redirects`] follows redirects on top of any fetcher.
//! [`Loader`] runs fetches on worker threads and returns the results through
//! a channel.
//!
//! [`CookieJar`] stores cookies. Each [`NetworkFetcher`] has one jar for
//! all its HTTP requests (see ADR 0012).
//!
//! The fixture directory format is documented in [`fixture`].

mod budget;
mod builtin;
mod cookies;
mod cors;
mod cors_fetch;
mod data_url;
mod error;
mod escape;
mod fetch;
mod file_types;
pub mod fixture;
mod headers;
mod loader;
mod mime;
mod network;
mod request;
mod response;
mod site;
#[cfg(test)]
mod test_util;

pub use budget::{ByteBudget, Reservation};
pub use cookies::{Cookie, CookieJar, SameSite};
pub use cors::{
    CorsFailure, PREFLIGHT_CACHE_ENTRIES, PreflightAnswer, PreflightCache, ResponseKind,
    check_preflight, console_message, cors_check, filter_response, is_cors_safelisted_header,
    needs_preflight, preflight_request, unsafe_request_header_names,
};
pub use cors_fetch::{Fetched, fetch_request};
pub use data_url::percent_decode;
pub use error::{CorsError, NetError};
pub use escape::escape_html;
pub use fetch::{Fetcher, MAX_REDIRECTS, fetch_following_redirects};
pub use fixture::{ExtendingFetcher, RecordingFetcher, ReplayFetcher};
pub use headers::{Headers, is_token};
pub use loader::{API_WORKERS, Completion, Loader, RequestId};
pub use network::{MAX_BODY_SIZE, NetworkFetcher, USER_AGENT};
pub use request::{
    CredentialsMode, Destination, Method, RedirectMode, Request, RequestMode, Tainting,
};
pub use response::{ContentType, Response};
pub use url::{Origin, Url};
