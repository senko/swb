//! Cookies: the [`CookieJar`] of a browser session.
//!
//! The jar follows RFC 6265bis
//! (<https://datatracker.ietf.org/doc/draft-ietf-httpbis-rfc6265bis/>):
//!
//! - `parse.rs`: the `Set-Cookie` header (§5.6); `date.rs`: cookie dates
//!   (§5.1.1).
//! - `store.rs`: the storage model (§5.7), retrieval of the `Cookie`
//!   header (§5.8), expiry, and Chromium's limits.
//! - This file: the jar and the same-site context of a request (§5.2).
//!
//! Where the draft leaves a choice to the user agent, swb does what
//! Chromium does:
//!
//! - A cookie without `SameSite` is treated as `Lax` ("Lax by default").
//! - Sites are schemeful: `http://a.com` and `https://a.com` are different
//!   sites.
//! - `http:` URLs to `localhost` and to loopback IP addresses count as
//!   secure (they can set and get `Secure` cookies). Unlike Chromium, swb
//!   does not count `*.localhost` (see `site::is_secure`).
//! - Cookies live at most 400 days; a name and value have at most 4096
//!   bytes together; an attribute value has at most 1024 bytes.
//! - At most 180 cookies per registrable domain (then the least recently
//!   used ones are evicted down to 150) and 3300 in total (down to 3000).
//! - Third-party cookies (`SameSite=None; Secure`) are allowed.
//!
//! Not supported: the non-HTTP API (`document.cookie`, so `HttpOnly` has no
//! effect yet), `Partitioned` cookies (the attribute is ignored, so they
//! are stored unpartitioned), cookie priorities, Chromium's "Lax+POST"
//! exception for new cookies without `SameSite`, and persistence: the jar
//! lives in memory and is empty when swb starts.

mod date;
mod parse;
mod store;

use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, log};
use url::Url;

use crate::headers::Headers;
use crate::request::Request;
use crate::site::Site;
use store::{Access, Store};

/// The `SameSite` attribute of a cookie.
///
/// <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.6.7>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SameSite {
    /// Sent only with same-site requests.
    Strict,
    /// Also sent with cross-site top-level navigations that use `GET`.
    Lax,
    /// Sent with all requests. Requires `Secure`.
    None,
    /// No attribute, or an unknown value. Treated as [`SameSite::Lax`].
    Default,
}

impl SameSite {
    /// Returns the name of the value: `Strict`, `Lax`, `None` or `Default`.
    pub fn as_str(self) -> &'static str {
        match self {
            SameSite::Strict => "Strict",
            SameSite::Lax => "Lax",
            SameSite::None => "None",
            SameSite::Default => "Default",
        }
    }
}

/// A cookie in a [`CookieJar`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    /// The name. It can be empty.
    pub name: String,
    /// The value.
    pub value: String,
    /// The host that set the cookie (host-only cookies) or the value of
    /// the `Domain` attribute, without a leading dot.
    pub domain: String,
    /// True if the cookie is sent only to [`Cookie::domain`] itself, not to
    /// its subdomains (it had no `Domain` attribute).
    pub host_only: bool,
    /// The path.
    pub path: String,
    /// Sent only over secure connections.
    pub secure: bool,
    /// Hidden from scripts.
    pub http_only: bool,
    /// The `SameSite` attribute.
    pub same_site: SameSite,
    /// The expiry time in seconds since the Unix epoch, or `None` for a
    /// session cookie.
    pub expires: Option<i64>,
}

/// How a request relates to the site of the document that started it.
///
/// swb has no frames, so the "site for cookies" of a subresource request
/// is the site of its initiator (the top-level document), and that of a
/// top-level navigation is the site of the request URL. This gives
/// Chromium's three contexts:
///
/// <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.2>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SameSiteContext {
    /// The initiator is same-site with the request URL, or the request is a
    /// top-level navigation without an initiator (the user started it).
    /// All cookies apply.
    SameSite,
    /// A top-level navigation started by a cross-site document. Retrieval
    /// sends `Lax` cookies only for `GET`; any cookie can be set.
    CrossSiteNavigation,
    /// A subresource request to another site, or a subresource request
    /// without an initiator. Only `SameSite=None` cookies are sent and
    /// set.
    CrossSite,
}

impl SameSiteContext {
    /// Returns the context of `request`, from the sites of its initiator
    /// and its URL, and from its destination.
    pub(crate) fn of(request: &Request) -> Self {
        let same_site = match &request.initiator {
            // Only the user starts requests without an initiator, and the
            // user starts only navigations. A subresource request without
            // one is a bug in the caller; it gets the fewest cookies.
            None => request.is_top_level_navigation(),
            Some(initiator) => match (Site::of_origin(initiator), Site::of_url(&request.url)) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        };
        if same_site {
            SameSiteContext::SameSite
        } else if request.is_top_level_navigation() {
            SameSiteContext::CrossSiteNavigation
        } else {
            SameSiteContext::CrossSite
        }
    }
}

/// The cookie store of one browser session, in memory.
///
/// [`NetworkFetcher`](crate::NetworkFetcher) has one jar. It adds the
/// `Cookie` header to each HTTP request and stores the `Set-Cookie` headers
/// of each response, including redirect responses. Only `http:` and
/// `https:` URLs have cookies. The jar can be used from several threads.
#[derive(Default)]
pub struct CookieJar {
    store: Mutex<Store>,
}

impl CookieJar {
    /// Creates an empty jar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the value of the `Cookie` header for `request`, or `None` if
    /// no cookie applies.
    pub fn cookie_header(&self, request: &Request) -> Option<String> {
        self.cookie_header_at(request, unix_now())
    }

    /// Stores the cookies of the `Set-Cookie` headers in `headers`, the
    /// response headers for `request`. Invalid cookies are ignored with a
    /// warning; cookies that the request may not set (for example a
    /// cross-site subresource) are ignored with a debug message.
    pub fn store_response_cookies(&self, request: &Request, headers: &Headers) {
        self.store_response_cookies_at(request, headers, unix_now());
    }

    /// Returns all cookies that have not expired, oldest first.
    pub fn cookies(&self) -> Vec<Cookie> {
        self.lock().all(unix_now())
    }

    /// Removes all cookies.
    pub fn clear(&self) {
        self.lock().clear();
    }

    pub(crate) fn cookie_header_at(&self, request: &Request, now: i64) -> Option<String> {
        if !has_cookies(&request.url) {
            return None;
        }
        self.lock().header(&access(request, now))
    }

    pub(crate) fn store_response_cookies_at(&self, request: &Request, headers: &Headers, now: i64) {
        if !has_cookies(&request.url) {
            return;
        }
        let access = access(request, now);
        let mut store = self.lock();
        for line in headers.get_all("set-cookie") {
            if let Err(reason) = store.set(line, &access) {
                let level = if reason.is_malformed() {
                    Level::Warn
                } else {
                    Level::Debug
                };
                let name = log_name(line);
                log!(level, "{}: cookie {name} rejected: {reason}", request.url);
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        // The store has no invariants that a panic could break halfway.
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for CookieJar {
    /// Shows the number of cookies, not their values. Does not wait for
    /// the lock.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("CookieJar");
        if let Ok(store) = self.store.try_lock() {
            debug.field("cookies", &store.len(unix_now()));
        }
        debug.finish_non_exhaustive()
    }
}

fn access(request: &Request, now: i64) -> Access<'_> {
    Access {
        url: &request.url,
        context: SameSiteContext::of(request),
        safe_method: request.method.is_safe(),
        now,
    }
}

/// Returns the cookie name of a `Set-Cookie` value for log messages. A
/// nameless cookie gives `<nameless>`: its value can be a session token.
fn log_name(line: &str) -> String {
    let name_value = line.split(';').next().unwrap_or_default();
    match name_value.split_once('=') {
        Some((name, _)) if !name.trim().is_empty() => format!("{:?}", name.trim()),
        _ => "<nameless>".to_owned(),
    }
}

/// Only HTTP and HTTPS responses set cookies and only HTTP and HTTPS
/// requests send them.
fn has_cookies(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// Returns the current time in seconds since the Unix epoch.
fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        Err(error) => i64::try_from(error.duration().as_secs()).map_or(i64::MIN, |s| -s),
    }
}

#[cfg(test)]
mod tests;
