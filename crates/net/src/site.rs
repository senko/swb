//! Registrable domains, sites and secure origins, for cookies.
//!
//! The registrable domain ("eTLD+1") comes from the Public Suffix List,
//! compiled into the `psl` crate. Browsers use both sections of the list
//! (ICANN and private domains such as `github.io`), and so does swb.
//!
//! A site is a scheme and a registrable domain ("schemeful same-site", as
//! in Chromium since version 89), so `http://a.com` and `https://a.com`
//! are different sites.
//!
//! - <https://url.spec.whatwg.org/#host-registrable-domain>
//! - <https://html.spec.whatwg.org/multipage/browsers.html#sites>
//!
//! Deviation from the URL spec, as in Chromium: a host under a top-level
//! domain that is not in the list (for example `localhost` or `nas.lan`)
//! has no registrable domain. The URL spec applies the list's implicit `*`
//! rule, which makes `lan` a public suffix and `nas.lan` a registrable
//! domain. Chromium (`registry_controlled_domains`, "exclude unknown
//! registries") treats such a host as its own site, and it does not allow
//! `Domain` cookies for it.

use std::net::Ipv4Addr;

use psl::Psl as _;
use url::{Host, Origin, Url};

/// Returns the registrable domain of `host`: the public suffix and one more
/// label. Returns `None` for IP addresses, for public suffixes, for hosts
/// under an unknown top-level domain, and for names with empty labels.
///
/// `host` must be in ASCII lowercase, as the URL parser produces it
/// (internationalized names in Punycode).
pub(crate) fn registrable_domain(host: &str) -> Option<&str> {
    if !is_domain_name(host) {
        return None;
    }
    let domain = psl::List.domain(host.as_bytes())?;
    if !domain.suffix().is_known() {
        return None;
    }
    // The domain is a suffix of `host`, and it starts after a dot, so the
    // offset is a character boundary.
    let start = host.len().checked_sub(domain.as_bytes().len())?;
    host.get(start..)
}

/// Returns the key that groups `host` with the hosts of the same site:
/// its registrable domain, or the host itself if it has none.
pub(crate) fn site_host(host: &str) -> &str {
    registrable_domain(host).unwrap_or(host)
}

/// True if `host` is an IP address as the URL parser serializes it: IPv4
/// in dotted decimal, IPv6 in brackets.
pub(crate) fn is_ip_address(host: &str) -> bool {
    host.starts_with('[') || host.parse::<Ipv4Addr>().is_ok()
}

/// True if `host` is a domain name that the public suffix lookup can
/// handle: not empty, not an IP address, no empty labels (one trailing dot
/// is allowed: `example.com.`).
fn is_domain_name(host: &str) -> bool {
    let name = host.strip_suffix('.').unwrap_or(host);
    !name.is_empty() && !is_ip_address(host) && name.split('.').all(|label| !label.is_empty())
}

/// A site: a scheme and a registrable domain (or a host without one).
///
/// <https://html.spec.whatwg.org/multipage/browsers.html#obtain-a-site>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Site {
    scheme: String,
    host: String,
}

impl Site {
    /// Returns the site of `origin`, or `None` for an opaque origin (a
    /// `data:` or `file:` document, for example), which is not same-site
    /// with anything.
    pub(crate) fn of_origin(origin: &Origin) -> Option<Site> {
        match origin {
            Origin::Opaque(_) => None,
            Origin::Tuple(scheme, host, _port) => {
                let host = host.to_string();
                Some(Site {
                    scheme: scheme.clone(),
                    host: site_host(&host).to_owned(),
                })
            }
        }
    }

    /// Returns the site of the origin of `url`.
    pub(crate) fn of_url(url: &Url) -> Option<Site> {
        Site::of_origin(&url.origin())
    }
}

/// True if a request to `url` counts as secure for cookies: `https:`, or
/// `http:` to `localhost`, `127.0.0.0/8` or `::1`. Chromium treats such
/// "potentially trustworthy" URLs as secure for `Secure` cookies since
/// version 89.
///
/// Deviation: Chromium also treats `*.localhost` as secure, because it
/// resolves those names to loopback itself. swb uses the system resolver,
/// which can send `*.localhost` to a DNS server, so only the plain name
/// `localhost` (from `/etc/hosts`) counts. Requests to all loopback hosts
/// bypass proxies (see [`is_loopback`]).
///
/// <https://w3c.github.io/webappsec-secure-contexts/#is-origin-trustworthy>
pub(crate) fn is_secure(url: &Url) -> bool {
    match url.scheme() {
        "https" | "wss" => true,
        "http" | "ws" => match url.host() {
            Some(Host::Domain(name)) => name.strip_suffix('.').unwrap_or(name) == "localhost",
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            None => false,
        },
        _ => false,
    }
}

/// True if the host of `url` is a loopback host: `localhost`,
/// `*.localhost`, `127.0.0.0/8` or `::1`. Requests to these hosts never
/// use a proxy, as in Chromium (its implicit `<-loopback>` bypass rule), so
/// that their cookies do not leave the machine.
pub(crate) fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => {
            let name = name.strip_suffix('.').unwrap_or(name);
            name == "localhost" || name.ends_with(".localhost")
        }
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn registrable_domains() {
        for (host, expected) in [
            ("example.com", Some("example.com")),
            ("www.example.com", Some("example.com")),
            ("a.b.example.co.uk", Some("example.co.uk")),
            ("co.uk", None),
            ("com", None),
            // A private section rule.
            ("user.github.io", Some("user.github.io")),
            ("github.io", None),
            ("example.com.", Some("example.com.")),
            // Punycode: 食狮.公司.cn
            (
                "www.xn--85x722f.xn--55qx5d.cn",
                Some("xn--85x722f.xn--55qx5d.cn"),
            ),
            // Unknown top-level domains have no registrable domain.
            ("localhost", None),
            ("app.localhost", None),
            ("nas.lan", None),
            ("127.0.0.1", None),
            ("[::1]", None),
        ] {
            assert_eq!(registrable_domain(host), expected, "{host}");
        }
    }

    #[test]
    fn hostile_names_do_not_panic() {
        for host in [
            "", ".", "..", "...", ".com", "com.", "a..com", "..com", "a.", "-", "a.-", "é.com",
            "\u{0}", "co.uk.", ".co.uk",
        ] {
            let _ = registrable_domain(host);
            let _ = site_host(host);
        }
        assert_eq!(registrable_domain("a..example.com"), None);
        assert_eq!(registrable_domain(".example.com"), None);
    }

    #[test]
    fn sites_are_schemeful() {
        let site = |s: &str| Site::of_url(&url(s));
        assert_eq!(
            site("https://a.example.com/"),
            site("https://b.example.com:8443/x")
        );
        assert_ne!(site("https://example.com/"), site("http://example.com/"));
        assert_ne!(site("https://a.github.io/"), site("https://b.github.io/"));
        assert_ne!(site("http://127.0.0.1/"), site("http://localhost/"));
        assert_eq!(site("http://127.0.0.1:1/"), site("http://127.0.0.1:2/"));
        assert_eq!(site("data:,x"), None);
        assert_eq!(site("file:///tmp/a.html"), None);
    }

    #[test]
    fn secure_urls() {
        assert!(is_secure(&url("https://example.com/")));
        assert!(!is_secure(&url("http://example.com/")));
        assert!(is_secure(&url("http://localhost:8000/")));
        assert!(is_secure(&url("http://localhost./")));
        // The system resolver could send this name to a DNS server.
        assert!(!is_secure(&url("http://app.localhost/")));
        assert!(is_secure(&url("http://127.0.0.2/")));
        assert!(is_secure(&url("http://[::1]/")));
        assert!(!is_secure(&url("http://localhost.example/")));
        assert!(!is_secure(&url("file:///tmp/")));
    }

    #[test]
    fn loopback_urls() {
        for loopback in [
            "http://localhost/",
            "http://app.localhost:8000/",
            "https://127.0.0.1/",
            "http://127.1.2.3/",
            "http://[::1]/",
        ] {
            assert!(is_loopback(&url(loopback)), "{loopback}");
        }
        for other in [
            "http://localhost.example/",
            "http://10.0.0.1/",
            "http://[::2]/",
            "data:,x",
        ] {
            assert!(!is_loopback(&url(other)), "{other}");
        }
    }
}
