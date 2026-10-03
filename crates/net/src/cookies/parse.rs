//! Parsing the `Set-Cookie` header field.
//!
//! <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.6>

use super::{SameSite, date};

/// The maximum size of a cookie's name and value together, in bytes.
pub(super) const MAX_NAME_VALUE_SIZE: usize = 4096;

/// The maximum size of an attribute value, in bytes. Longer attributes are
/// ignored.
pub(super) const MAX_ATTRIBUTE_VALUE_SIZE: usize = 1024;

/// The maximum lifetime of a cookie: 400 days, in seconds. `Expires` and
/// `Max-Age` values beyond it are reduced to it.
pub(super) const MAX_LIFETIME: i64 = 400 * 24 * 60 * 60;

/// The expiry time of a cookie that `Max-Age` made expire at once: the
/// earliest representable time.
pub(super) const EXPIRED: i64 = i64::MIN;

/// A parsed `Set-Cookie` header value. Where an attribute occurs several
/// times, the last valid one counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetCookie {
    pub(super) name: String,
    pub(super) value: String,
    /// The expiry time from `Expires`, in seconds since the Unix epoch.
    pub(super) expires: Option<i64>,
    /// The expiry time from `Max-Age`. It takes precedence over `expires`.
    pub(super) max_age: Option<i64>,
    /// The value of `Domain`, without a leading dot, in ASCII lowercase.
    /// It can be empty (`Domain=.`).
    pub(super) domain: Option<String>,
    /// The value of `Path`. A value that does not start with `/` means the
    /// default path.
    pub(super) path: Option<String>,
    pub(super) secure: bool,
    pub(super) http_only: bool,
    /// `SameSite`, or [`SameSite::Default`] without the attribute.
    pub(super) same_site: SameSite,
}

impl SetCookie {
    /// Returns the expiry time, or `None` for a session cookie.
    pub(super) fn expiry(&self) -> Option<i64> {
        self.max_age.or(self.expires)
    }
}

/// Parses a `Set-Cookie` header value. `now` is the current time in seconds
/// since the Unix epoch. Returns `None` if the cookie must be ignored.
pub(super) fn parse(line: &str, now: i64) -> Option<SetCookie> {
    // Step 1: control characters other than HTAB.
    if line
        .chars()
        .any(|c| matches!(c, '\u{0}'..='\u{8}' | '\u{a}'..='\u{1f}' | '\u{7f}'))
    {
        return None;
    }
    // Steps 2 to 6.
    let (name_value, attributes) = line.split_once(';').unwrap_or((line, ""));
    let (name, value) = name_value.split_once('=').unwrap_or(("", name_value));
    let (name, value) = (trim_wsp(name), trim_wsp(value));
    if name.len() + value.len() > MAX_NAME_VALUE_SIZE {
        return None;
    }
    let mut cookie = SetCookie {
        name: name.to_owned(),
        value: value.to_owned(),
        expires: None,
        max_age: None,
        domain: None,
        path: None,
        secure: false,
        http_only: false,
        same_site: SameSite::Default,
    };
    for attribute in attributes.split(';') {
        let (name, value) = attribute.split_once('=').unwrap_or((attribute, ""));
        let (name, value) = (trim_wsp(name), trim_wsp(value));
        if value.len() > MAX_ATTRIBUTE_VALUE_SIZE {
            continue;
        }
        apply_attribute(&mut cookie, name, value, now);
    }
    Some(cookie)
}

/// Processes one cookie attribute (§5.6.1 to §5.6.7). Unknown attributes
/// (`Partitioned`, `Priority` and others) are ignored.
fn apply_attribute(cookie: &mut SetCookie, name: &str, value: &str, now: i64) {
    let is = |expected: &str| name.eq_ignore_ascii_case(expected);
    if is("expires") {
        if let Some(time) = date::parse(value) {
            cookie.expires = Some(time.min(now.saturating_add(MAX_LIFETIME)));
        }
    } else if is("max-age") {
        if let Some(seconds) = parse_max_age(value) {
            cookie.max_age = Some(if seconds <= 0 {
                EXPIRED
            } else {
                now.saturating_add(seconds.min(MAX_LIFETIME))
            });
        }
    } else if is("domain") {
        if !value.is_empty() {
            let domain = value.strip_prefix('.').unwrap_or(value);
            cookie.domain = Some(domain.to_ascii_lowercase());
        }
    } else if is("path") {
        cookie.path = Some(value.to_owned());
    } else if is("secure") {
        cookie.secure = true;
    } else if is("httponly") {
        cookie.http_only = true;
    } else if is("samesite") {
        cookie.same_site = if value.eq_ignore_ascii_case("none") {
            SameSite::None
        } else if value.eq_ignore_ascii_case("strict") {
            SameSite::Strict
        } else if value.eq_ignore_ascii_case("lax") {
            SameSite::Lax
        } else {
            SameSite::Default
        };
    }
}

/// Parses a `Max-Age` value: an optional `-` and digits. Values too large
/// for an `i64` saturate.
///
/// <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.6.2>
fn parse_max_age(value: &str) -> Option<i64> {
    let (negative, digits) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.bytes().fold(0_i64, |n, d| {
        n.saturating_mul(10).saturating_add(i64::from(d - b'0'))
    });
    Some(if negative { -magnitude } else { magnitude })
}

/// Removes leading and trailing spaces and tabs (`WSP`).
fn trim_wsp(s: &str) -> &str {
    s.trim_matches([' ', '\t'])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    fn parse_ok(line: &str) -> SetCookie {
        parse(line, NOW).unwrap()
    }

    #[test]
    fn name_and_value() {
        let cookie = parse_ok("  sid = abc def ; ");
        assert_eq!(
            (cookie.name.as_str(), cookie.value.as_str()),
            ("sid", "abc def")
        );
        let cookie = parse_ok("a=b=c");
        assert_eq!((cookie.name.as_str(), cookie.value.as_str()), ("a", "b=c"));
        // Without "=", the string is the value of a nameless cookie.
        let cookie = parse_ok("lonely");
        assert_eq!(
            (cookie.name.as_str(), cookie.value.as_str()),
            ("", "lonely")
        );
        let cookie = parse_ok("=v");
        assert_eq!((cookie.name.as_str(), cookie.value.as_str()), ("", "v"));
        let cookie = parse_ok("\"quoted\"=\"x\"");
        assert_eq!(cookie.value, "\"x\"");
    }

    #[test]
    fn control_characters_reject_the_cookie() {
        assert!(parse("a=b\u{1}", NOW).is_none());
        assert!(parse("a=b; Path=/\u{7f}", NOW).is_none());
        assert!(parse("a=b\tc", NOW).is_some());
    }

    #[test]
    fn size_limits() {
        let name = "n".repeat(96);
        let value = "v".repeat(4000);
        assert!(parse(&format!("{name}={value}"), NOW).is_some());
        assert!(parse(&format!("{name}x={value}"), NOW).is_none());
        let long_path = format!("/{}", "p".repeat(1024));
        let cookie = parse_ok(&format!("a=b; Path=/ok; Path={long_path}"));
        assert_eq!(cookie.path.as_deref(), Some("/ok"));
    }

    #[test]
    fn attributes() {
        let cookie = parse_ok(
            "a=b; Domain=.Example.COM; Path=/docs; Secure; HttpOnly; SameSite=strict; Partitioned; Priority=High",
        );
        assert_eq!(cookie.domain.as_deref(), Some("example.com"));
        assert_eq!(cookie.path.as_deref(), Some("/docs"));
        assert!(cookie.secure);
        assert!(cookie.http_only);
        assert_eq!(cookie.same_site, SameSite::Strict);
        assert_eq!(cookie.expiry(), None);

        let cookie = parse_ok("a=b; secure=yes; HTTPONLY; Domain=; Path");
        assert!(cookie.secure && cookie.http_only);
        assert_eq!(cookie.domain, None);
        assert_eq!(cookie.path.as_deref(), Some(""));
    }

    #[test]
    fn same_site_values() {
        assert_eq!(parse_ok("a=b").same_site, SameSite::Default);
        assert_eq!(parse_ok("a=b; SameSite=Lax").same_site, SameSite::Lax);
        assert_eq!(parse_ok("a=b; SameSite=NONE").same_site, SameSite::None);
        assert_eq!(parse_ok("a=b; SameSite=Bogus").same_site, SameSite::Default);
        // The last attribute counts, even with an unknown value.
        assert_eq!(
            parse_ok("a=b; SameSite=Strict; SameSite=x").same_site,
            SameSite::Default
        );
    }

    #[test]
    fn max_age() {
        assert_eq!(parse_ok("a=b; Max-Age=60").expiry(), Some(NOW + 60));
        assert_eq!(parse_ok("a=b; Max-Age=0").expiry(), Some(EXPIRED));
        assert_eq!(parse_ok("a=b; Max-Age=-5").expiry(), Some(EXPIRED));
        assert_eq!(
            parse_ok("a=b; Max-Age=99999999999999999999999").expiry(),
            Some(NOW + MAX_LIFETIME)
        );
        for invalid in ["", "-", "+5", "5s", " 5x", "1.5", "--1"] {
            assert_eq!(
                parse_ok(&format!("a=b; Max-Age={invalid}")).max_age,
                None,
                "{invalid}"
            );
        }
        // An invalid value does not replace a valid one.
        assert_eq!(
            parse_ok("a=b; Max-Age=60; Max-Age=x").expiry(),
            Some(NOW + 60)
        );
    }

    #[test]
    fn expires() {
        let cookie = parse_ok("a=b; Expires=Wed, 21 Oct 2015 07:28:00 GMT");
        assert_eq!(cookie.expiry(), Some(1_445_412_480));
        let cookie = parse_ok("a=b; Expires=Fri, 31 Dec 9999 23:59:59 GMT");
        assert_eq!(cookie.expiry(), Some(NOW + MAX_LIFETIME));
        assert_eq!(parse_ok("a=b; Expires=soon").expiry(), None);
        // Max-Age wins, in either order.
        let cookie = parse_ok("a=b; Max-Age=10; Expires=Wed, 21 Oct 2015 07:28:00 GMT");
        assert_eq!(cookie.expiry(), Some(NOW + 10));
        let cookie = parse_ok("a=b; Expires=Wed, 21 Oct 2015 07:28:00 GMT; Max-Age=10");
        assert_eq!(cookie.expiry(), Some(NOW + 10));
    }

    #[test]
    fn hostile_input_does_not_panic() {
        for line in [
            "",
            ";",
            "=",
            ";;;=;=",
            "a=b;=;Domain;Domain=.;Max-Age=-;Expires=;SameSite",
            "é=ü; Domain=ÄÖ.com; Path=/ä",
            "a=b; Max-Age=-99999999999999999999999",
        ] {
            let _ = parse(line, NOW);
            let _ = parse(line, i64::MAX);
            let _ = parse(line, i64::MIN);
        }
    }
}
