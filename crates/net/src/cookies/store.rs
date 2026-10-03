//! The cookie store: the storage model, retrieval, expiry and limits.
//!
//! - Storage model: <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.7>
//! - Retrieval: <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.8>
//! - Limits: Chromium's `CookieMonster` (`net/cookies/cookie_monster.cc`).
//!
//! Cookies are grouped by the site host of their domain (the registrable
//! domain, or the host if it has none). The storage model rejects a
//! `Domain` attribute with a different registrable domain, so retrieval
//! looks only at the group of the request host, and the per-domain limit
//! counts one group. As in Chromium (cookies keyed by registrable domain),
//! this differs from the draft in one case: a cookie with
//! `Domain=amazonaws.com` (set by `www.amazonaws.com`) is not sent to
//! `bucket.s3.amazonaws.com`, whose registrable domain is itself because
//! `s3.amazonaws.com` is a public suffix.
//!
//! Times are seconds since the Unix epoch. Creation and access order use a
//! logical clock, so that cookies created in the same second keep their
//! order and tests are deterministic.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt;

use url::{Host, Url};

use super::parse::{self, MAX_ATTRIBUTE_VALUE_SIZE, SetCookie};
use super::{Cookie, SameSite, SameSiteContext};
use crate::site;

/// The maximum number of cookies per registrable domain (Chromium's
/// `kDomainMaxCookies`).
pub(super) const MAX_COOKIES_PER_DOMAIN: usize = 180;

/// The number of cookies that a domain keeps when it has too many
/// (`kDomainMaxCookies - kDomainPurgeCookies` in Chromium).
pub(super) const COOKIES_PER_DOMAIN_AFTER_PURGE: usize = 150;

/// The maximum number of cookies in the store (Chromium's `kMaxCookies`).
pub(super) const MAX_COOKIES: usize = 3300;

/// The number of cookies that the store keeps when it has too many
/// (`kMaxCookies - kPurgeCookies` in Chromium).
pub(super) const COOKIES_AFTER_PURGE: usize = 3000;

/// What the store needs to know about a request.
pub(super) struct Access<'a> {
    /// The request URL. It is an `http:` or `https:` URL, so it has a host.
    pub(super) url: &'a Url,
    pub(super) context: SameSiteContext,
    /// True for `GET`.
    pub(super) safe_method: bool,
    /// The current time.
    pub(super) now: i64,
}

/// Why a cookie was not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Rejection {
    /// A control character, or a name and value longer than 4096 bytes.
    Malformed,
    /// The name and the value are empty.
    Empty,
    /// The request URL has no host.
    NoHost,
    /// The `Domain` attribute is not a valid host.
    InvalidDomain,
    /// The `Domain` attribute does not match the host, or it is a public
    /// suffix.
    DomainMismatch,
    /// The path or the domain is longer than 1024 bytes. Attribute values
    /// longer than that are ignored when they are parsed, so the long value
    /// comes from the request URL.
    TooLong,
    /// `Secure` from a request that is not secure.
    SecureFromInsecureUrl,
    /// A request that is not secure tried to replace a `Secure` cookie.
    OverlaysSecureCookie,
    /// `SameSite=None` without `Secure`.
    NoneWithoutSecure,
    /// `SameSite` other than `None` from a cross-site subresource request.
    CrossSite,
    /// A `__Secure-` cookie without `Secure`.
    SecurePrefix,
    /// A `__Host-` cookie without `Secure`, with `Domain`, or with a path
    /// other than `/`.
    HostPrefix,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Rejection::Malformed => "malformed",
            Rejection::Empty => "empty name and value",
            Rejection::NoHost => "the URL has no host",
            Rejection::InvalidDomain => "Domain is not a valid host",
            Rejection::DomainMismatch => "Domain does not match the host",
            Rejection::TooLong => "path or domain longer than 1024 bytes",
            Rejection::SecureFromInsecureUrl => "Secure from an insecure URL",
            Rejection::OverlaysSecureCookie => "would replace a Secure cookie",
            Rejection::NoneWithoutSecure => "SameSite=None without Secure",
            Rejection::CrossSite => "cross-site request",
            Rejection::SecurePrefix => "__Secure- prefix without Secure",
            Rejection::HostPrefix => "__Host- prefix requirements not met",
        })
    }
}

impl Rejection {
    /// True if the cookie is invalid in a response from this host, whatever
    /// the request. False for rejections that depend on the request: its
    /// same-site context, whether it is secure, the cookies in the store,
    /// or the length of its URL.
    pub(super) fn is_malformed(self) -> bool {
        matches!(
            self,
            Rejection::Malformed
                | Rejection::Empty
                | Rejection::InvalidDomain
                | Rejection::DomainMismatch
                | Rejection::NoneWithoutSecure
                | Rejection::SecurePrefix
                | Rejection::HostPrefix
        )
    }
}

#[derive(Debug)]
struct StoredCookie {
    cookie: Cookie,
    /// Creation order. A cookie that replaces another keeps its value.
    created: u64,
    last_access: u64,
}

impl StoredCookie {
    /// The eviction order: least recently used first. `created` is unique,
    /// so no two cookies have the same key.
    fn lru_key(&self) -> (u64, u64) {
        (self.last_access, self.created)
    }
}

/// The cookies of a [`CookieJar`](super::CookieJar).
#[derive(Debug, Default)]
pub(super) struct Store {
    /// Cookies grouped by the site host of their domain.
    groups: HashMap<String, Vec<StoredCookie>>,
    /// The logical clock for creation and access order.
    clock: u64,
}

impl Store {
    /// Processes one `Set-Cookie` header value received in the response to
    /// a request (§5.6 and §5.7).
    pub(super) fn set(&mut self, line: &str, access: &Access<'_>) -> Result<(), Rejection> {
        let host = access.url.host_str().ok_or(Rejection::NoHost)?;
        let set_cookie = parse::parse(line, access.now).ok_or(Rejection::Malformed)?;
        let cookie = create(set_cookie, host, access)?;
        if !cookie.secure
            && !site::is_secure(access.url)
            && self.overlays_secure_cookie(&cookie, access.now)
        {
            return Err(Rejection::OverlaysSecureCookie);
        }
        self.insert(cookie, access.now);
        Ok(())
    }

    /// Returns the value of the `Cookie` header for a request (§5.8.3), or
    /// `None` if no cookie applies.
    pub(super) fn header(&mut self, access: &Access<'_>) -> Option<String> {
        let host = access.url.host_str()?;
        let path = access.url.path();
        let secure = site::is_secure(access.url);
        let tick = self.tick();
        let key = site::site_host(host);
        let group = self.groups.get_mut(key)?;
        group.retain(|c| !c.cookie.is_expired(access.now));
        if group.is_empty() {
            self.groups.remove(key);
            return None;
        }
        let mut list: Vec<&mut StoredCookie> = group
            .iter_mut()
            .filter(|c| {
                let cookie = &c.cookie;
                cookie.matches_host(host)
                    && path_matches(path, &cookie.path)
                    && (!cookie.secure || secure)
                    && is_sent(cookie.same_site, access.context, access.safe_method)
            })
            .collect();
        if list.is_empty() {
            return None;
        }
        // Longer paths first, then older cookies first.
        list.sort_by(|a, b| {
            b.cookie
                .path
                .len()
                .cmp(&a.cookie.path.len())
                .then(a.created.cmp(&b.created))
        });
        for stored in &mut list {
            stored.last_access = tick;
        }
        Some(serialize(list.iter().map(|stored| &stored.cookie)))
    }

    /// Returns the number of cookies that have not expired.
    pub(super) fn len(&self, now: i64) -> usize {
        self.groups
            .values()
            .flatten()
            .filter(|c| !c.cookie.is_expired(now))
            .count()
    }

    /// Returns all cookies that have not expired, oldest first.
    pub(super) fn all(&self, now: i64) -> Vec<Cookie> {
        let mut cookies: Vec<&StoredCookie> = self
            .groups
            .values()
            .flatten()
            .filter(|c| !c.cookie.is_expired(now))
            .collect();
        cookies.sort_by_key(|c| c.created);
        cookies.into_iter().map(|c| c.cookie.clone()).collect()
    }

    /// Removes all cookies.
    pub(super) fn clear(&mut self) {
        self.groups.clear();
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// §5.7 step 16: a request that is not secure must not set a cookie
    /// that would overlay a `Secure` cookie with the same name.
    fn overlays_secure_cookie(&self, new: &Cookie, now: i64) -> bool {
        let Some(group) = self.groups.get(site::site_host(&new.domain)) else {
            return false;
        };
        group.iter().any(|c| {
            let old = &c.cookie;
            old.secure
                && !old.is_expired(now)
                && old.name == new.name
                && (domain_matches(&old.domain, &new.domain)
                    || domain_matches(&new.domain, &old.domain))
                && path_matches(&new.path, &old.path)
        })
    }

    /// §5.7 steps 22 and 23: replaces a cookie with the same name, domain,
    /// host-only flag and path, keeping its creation time; then evicts
    /// expired cookies and applies the limits. An expired cookie only
    /// removes the cookie that it replaces.
    fn insert(&mut self, cookie: Cookie, now: i64) {
        let key = site::site_host(&cookie.domain).to_owned();
        let tick = self.tick();
        let group = self.groups.entry(key.clone()).or_default();
        group.retain(|c| !c.cookie.is_expired(now));
        let mut created = tick;
        if let Some(index) = group.iter().position(|c| c.cookie.same_identity(&cookie)) {
            created = group.swap_remove(index).created;
        }
        if !cookie.is_expired(now) {
            group.push(StoredCookie {
                cookie,
                created,
                last_access: tick,
            });
        }
        if group.is_empty() {
            self.groups.remove(&key);
            return;
        }
        if group.len() > MAX_COOKIES_PER_DOMAIN {
            keep_most_recently_used(group, COOKIES_PER_DOMAIN_AFTER_PURGE);
        }
        self.apply_global_limit(now);
    }

    /// Chromium's global limit: above [`MAX_COOKIES`], remove expired
    /// cookies, then the least recently used ones down to
    /// [`COOKIES_AFTER_PURGE`].
    fn apply_global_limit(&mut self, now: i64) {
        if self.count() <= MAX_COOKIES {
            return;
        }
        for group in self.groups.values_mut() {
            group.retain(|c| !c.cookie.is_expired(now));
        }
        let count = self.count();
        if count > MAX_COOKIES {
            let mut keys: Vec<(u64, u64)> = self
                .groups
                .values()
                .flatten()
                .map(StoredCookie::lru_key)
                .collect();
            keys.sort_unstable();
            if let Some(&cutoff) = keys.get(count - COOKIES_AFTER_PURGE - 1) {
                for group in self.groups.values_mut() {
                    group.retain(|c| c.lru_key() > cutoff);
                }
            }
        }
        self.groups.retain(|_, group| !group.is_empty());
    }

    fn count(&self) -> usize {
        self.groups.values().map(Vec::len).sum()
    }
}

/// Removes the least recently used cookies of `group` until `keep` remain.
fn keep_most_recently_used(group: &mut Vec<StoredCookie>, keep: usize) {
    group.sort_unstable_by_key(|c| Reverse(c.lru_key()));
    group.truncate(keep);
}

/// Creates the cookie from a parsed `Set-Cookie` value: §5.7 steps 2 to 21,
/// except step 16 (which needs the store).
fn create(set_cookie: SetCookie, host: &str, access: &Access<'_>) -> Result<Cookie, Rejection> {
    if set_cookie.name.is_empty() && set_cookie.value.is_empty() {
        return Err(Rejection::Empty);
    }
    let (domain, host_only) = domain_and_host_only(host, set_cookie.domain.as_deref())?;
    let path_attribute = set_cookie.path.as_deref();
    let path = match path_attribute {
        Some(path) if path.starts_with('/') => path.to_owned(),
        _ => default_path(access.url),
    };
    // Deviation: the draft limits only attribute values. The default path
    // and a host-only domain come from the URL, which has no length limit,
    // so a limit here bounds the memory of the store.
    if path.len() > MAX_ATTRIBUTE_VALUE_SIZE || domain.len() > MAX_ATTRIBUTE_VALUE_SIZE {
        return Err(Rejection::TooLong);
    }
    if set_cookie.secure && !site::is_secure(access.url) {
        return Err(Rejection::SecureFromInsecureUrl);
    }
    match set_cookie.same_site {
        SameSite::None if !set_cookie.secure => return Err(Rejection::NoneWithoutSecure),
        SameSite::None => {}
        _ if access.context == SameSiteContext::CrossSite => return Err(Rejection::CrossSite),
        _ => {}
    }
    // Not `host_only`: a host without a registrable domain turns
    // `Domain=<the host>` into a host-only cookie, but `__Host-` forbids the
    // attribute itself, as in Chromium.
    let host_path = set_cookie.domain.is_none() && path_attribute.is_some() && path == "/";
    check_prefixes(&set_cookie, host_path)?;
    Ok(Cookie {
        expires: set_cookie.expiry(),
        name: set_cookie.name,
        value: set_cookie.value,
        domain,
        host_only,
        path,
        secure: set_cookie.secure,
        http_only: set_cookie.http_only,
        same_site: set_cookie.same_site,
    })
}

/// Checks the cookie name prefixes (§5.7 steps 19 to 21). `host_path` is
/// true if the cookie has no `Domain` attribute and has a `Path` attribute
/// that gives the path `/`.
fn check_prefixes(set_cookie: &SetCookie, host_path: bool) -> Result<(), Rejection> {
    let name = set_cookie.name.as_str();
    // A nameless cookie must not look like a prefixed one in the header.
    let nameless_value = if name.is_empty() {
        set_cookie.value.as_str()
    } else {
        ""
    };
    if (has_prefix(name, "__Secure-") && !set_cookie.secure)
        || has_prefix(nameless_value, "__Secure-")
    {
        return Err(Rejection::SecurePrefix);
    }
    if (has_prefix(name, "__Host-") && !(set_cookie.secure && host_path))
        || has_prefix(nameless_value, "__Host-")
    {
        return Err(Rejection::HostPrefix);
    }
    Ok(())
}

/// Returns the `Cookie` header value for `cookies`, in order (§5.8.3
/// step 4): `name=value` pairs separated by `; `; a nameless cookie gives
/// only its value.
fn serialize<'a>(cookies: impl Iterator<Item = &'a Cookie>) -> String {
    let mut header = String::new();
    for cookie in cookies {
        if !header.is_empty() {
            header.push_str("; ");
        }
        if !cookie.name.is_empty() {
            header.push_str(&cookie.name);
            header.push('=');
        }
        header.push_str(&cookie.value);
    }
    header
}

/// Applies the `Domain` attribute (§5.7 steps 7 to 10) and returns the
/// cookie's domain and host-only flag.
///
/// As in Chromium (`cookie_util::GetCookieDomainWithString`):
///
/// - The attribute is canonicalized with the URL host parser, so an
///   internationalized name becomes Punycode and IPv4 forms are
///   normalized. The draft rejects a non-ASCII attribute instead.
/// - The attribute must have the same registrable domain as the host, which
///   also rejects public suffixes. A host without a registrable domain (an
///   IP address, a public suffix, a host under an unknown top-level domain)
///   accepts only its own name, and the cookie is then host-only.
fn domain_and_host_only(host: &str, attribute: Option<&str>) -> Result<(String, bool), Rejection> {
    let Some(attribute) = attribute.filter(|domain| !domain.is_empty()) else {
        return Ok((host.to_owned(), true));
    };
    let domain = Host::parse(attribute)
        .map_err(|_| Rejection::InvalidDomain)?
        .to_string();
    match site::registrable_domain(host) {
        None if domain == host => Ok((domain, true)),
        Some(site)
            if site::registrable_domain(&domain) == Some(site) && domain_matches(host, &domain) =>
        {
            Ok((domain, false))
        }
        _ => Err(Rejection::DomainMismatch),
    }
}

/// Returns the default path of a cookie set by a response to `url`.
///
/// <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.1.4>
fn default_path(url: &Url) -> String {
    let path = url.path();
    match path.rfind('/') {
        Some(index) if index > 0 && path.starts_with('/') => {
            path.get(..index).unwrap_or("/").to_owned()
        }
        _ => "/".to_owned(),
    }
}

/// True if `string` domain-matches `domain` (both in lowercase).
///
/// <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.1.3>
fn domain_matches(string: &str, domain: &str) -> bool {
    string == domain
        || (!domain.is_empty()
            && string
                .strip_suffix(domain)
                .is_some_and(|rest| rest.ends_with('.'))
            && !site::is_ip_address(string))
}

/// True if `request_path` path-matches `cookie_path`.
///
/// <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.1.4>
fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    match request_path.strip_prefix(cookie_path) {
        None => false,
        Some(rest) => rest.is_empty() || cookie_path.ends_with('/') || rest.starts_with('/'),
    }
}

/// True if `name` starts with `prefix`, compared case-insensitively.
fn has_prefix(name: &str, prefix: &str) -> bool {
    name.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// The `SameSite` rule of retrieval (§5.8.3): which cookies a request in
/// `context` sends. `Default` is treated as `Lax` ("Lax by default", as in
/// Chromium).
fn is_sent(same_site: SameSite, context: SameSiteContext, safe_method: bool) -> bool {
    match context {
        SameSiteContext::SameSite => true,
        SameSiteContext::CrossSiteNavigation => {
            same_site == SameSite::None
                || (safe_method && matches!(same_site, SameSite::Lax | SameSite::Default))
        }
        SameSiteContext::CrossSite => same_site == SameSite::None,
    }
}

impl Cookie {
    fn is_expired(&self, now: i64) -> bool {
        self.expires.is_some_and(|expires| expires <= now)
    }

    fn matches_host(&self, host: &str) -> bool {
        if self.host_only {
            self.domain == host
        } else {
            domain_matches(host, &self.domain)
        }
    }

    /// True if `other` would replace this cookie (§5.7 step 22).
    fn same_identity(&self, other: &Cookie) -> bool {
        self.name == other.name
            && self.domain == other.domain
            && self.host_only == other.host_only
            && self.path == other.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_paths() {
        let default = |s: &str| default_path(&Url::parse(s).unwrap());
        assert_eq!(default("https://a.test/"), "/");
        assert_eq!(default("https://a.test/page"), "/");
        assert_eq!(default("https://a.test/dir/"), "/dir");
        assert_eq!(default("https://a.test/dir/page?q=/x"), "/dir");
        assert_eq!(default("https://a.test/a/b/c"), "/a/b");
    }

    #[test]
    fn path_matching() {
        assert!(path_matches("/", "/"));
        assert!(path_matches("/docs", "/docs"));
        assert!(path_matches("/docs/", "/docs"));
        assert!(path_matches("/docs/web", "/docs"));
        assert!(path_matches("/docs/web", "/docs/"));
        assert!(path_matches("/anything", "/"));
        assert!(!path_matches("/docsets", "/docs"));
        assert!(!path_matches("/doc", "/docs"));
        assert!(!path_matches("/", "/docs"));
    }

    #[test]
    fn domain_matching() {
        assert!(domain_matches("example.com", "example.com"));
        assert!(domain_matches("www.example.com", "example.com"));
        assert!(domain_matches("a.b.example.com", "example.com"));
        assert!(!domain_matches("badexample.com", "example.com"));
        assert!(!domain_matches("example.com", "www.example.com"));
        assert!(domain_matches("1.2.3.4", "1.2.3.4"));
        assert!(!domain_matches("1.2.3.4", "2.3.4"));
        assert!(!domain_matches("example.com.", ""));
    }

    #[test]
    fn prefixes_are_case_insensitive() {
        assert!(has_prefix("__Secure-id", "__Secure-"));
        assert!(has_prefix("__SECURE-id", "__Secure-"));
        assert!(has_prefix("__host-id", "__Host-"));
        assert!(!has_prefix("__Host", "__Host-"));
        assert!(!has_prefix("_\u{e9}_Host-id", "__Host-"));
    }
}
