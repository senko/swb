//! Tests of [`CookieJar`]: storage, retrieval, `SameSite` and limits. Time is
//! passed explicitly, so no test sleeps.

use super::*;
use crate::request::{Destination, Method};
use store::{
    COOKIES_AFTER_PURGE, COOKIES_PER_DOMAIN_AFTER_PURGE, MAX_COOKIES, MAX_COOKIES_PER_DOMAIN,
};

const NOW: i64 = 1_700_000_000;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

/// A top-level navigation that the user started (same-site).
fn navigation(s: &str) -> Request {
    Request::get(url(s), Destination::Document)
}

/// A subresource request from a document at `initiator`.
fn subresource(s: &str, initiator: &str) -> Request {
    Request::get(url(s), Destination::Image).with_initiator(Some(url(initiator).origin()))
}

/// A navigation started by a document at `initiator`.
fn link(s: &str, initiator: &str) -> Request {
    navigation(s).with_initiator(Some(url(initiator).origin()))
}

fn set_cookie_headers(lines: &[&str]) -> Headers {
    lines.iter().map(|line| ("Set-Cookie", *line)).collect()
}

fn set_at(jar: &CookieJar, request: &Request, lines: &[&str], now: i64) {
    jar.store_response_cookies_at(request, &set_cookie_headers(lines), now);
}

fn set(jar: &CookieJar, request: &Request, lines: &[&str]) {
    set_at(jar, request, lines, NOW);
}

fn header(jar: &CookieJar, request: &Request) -> Option<String> {
    jar.cookie_header_at(request, NOW)
}

fn header_for(jar: &CookieJar, s: &str) -> Option<String> {
    header(jar, &navigation(s))
}

fn names(jar: &CookieJar) -> Vec<String> {
    jar.lock().all(NOW).into_iter().map(|c| c.name).collect()
}

#[test]
fn host_only_cookie() {
    let jar = CookieJar::new();
    set(&jar, &navigation("https://www.example.com/"), &["a=1"]);
    assert_eq!(
        header_for(&jar, "https://www.example.com/x"),
        Some("a=1".into())
    );
    assert_eq!(header_for(&jar, "https://example.com/"), None);
    assert_eq!(header_for(&jar, "https://sub.www.example.com/"), None);
    let cookie = &jar.lock().all(NOW)[0];
    assert_eq!(cookie.domain, "www.example.com");
    assert!(cookie.host_only);
    assert_eq!(cookie.same_site, SameSite::Default);
}

#[test]
fn domain_cookie() {
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("https://www.example.com/"),
        &["a=1; Domain=.Example.com"],
    );
    for target in [
        "https://example.com/",
        "https://www.example.com/",
        "https://a.b.example.com/",
    ] {
        assert_eq!(header_for(&jar, target), Some("a=1".into()), "{target}");
    }
    assert_eq!(header_for(&jar, "https://notexample.com/"), None);
    assert_eq!(header_for(&jar, "https://example.org/"), None);
    let cookie = &jar.lock().all(NOW)[0];
    assert_eq!(cookie.domain, "example.com");
    assert!(!cookie.host_only);
}

#[test]
fn domain_must_match_the_host_and_not_be_a_public_suffix() {
    for (from, line) in [
        ("https://www.example.com/", "a=1; Domain=other.com"),
        (
            "https://www.example.com/",
            "a=1; Domain=sub.www.example.com",
        ),
        ("https://www.example.com/", "a=1; Domain=com"),
        ("https://a.example.co.uk/", "a=1; Domain=co.uk"),
        ("https://user.github.io/", "a=1; Domain=github.io"),
        ("https://www.example.com/", "a=1; Domain=exämple.com"),
        ("http://127.0.0.1/", "a=1; Domain=0.0.1"),
        ("http://app.localhost/", "a=1; Domain=localhost"),
        ("http://x.nas.lan/", "a=1; Domain=nas.lan"),
    ] {
        let jar = CookieJar::new();
        set(&jar, &navigation(from), &[line]);
        assert!(names(&jar).is_empty(), "{from} {line}");
    }
}

#[test]
fn domain_equal_to_a_host_without_registrable_domain_is_host_only() {
    for (from, domain) in [
        ("http://127.0.0.1/", "127.0.0.1"),
        ("http://localhost/", "localhost"),
        ("https://github.io/", "github.io"),
    ] {
        let jar = CookieJar::new();
        set(&jar, &navigation(from), &[&format!("a=1; Domain={domain}")]);
        let cookies = jar.lock().all(NOW);
        assert_eq!(cookies.len(), 1, "{from}");
        assert!(cookies[0].host_only, "{from}");
        assert_eq!(cookies[0].domain, domain);
    }
}

#[test]
fn domain_attribute_is_canonicalized() {
    // An internationalized name becomes Punycode, as in the URL.
    let jar = CookieJar::new();
    let from = navigation("https://www.ex\u{e4}mple.com/");
    set(&jar, &from, &["a=1; Domain=EX\u{c4}MPLE.com"]);
    let cookies = jar.lock().all(NOW);
    assert_eq!(cookies.len(), 1);
    assert_eq!(cookies[0].domain, "xn--exmple-cua.com");
    assert!(!cookies[0].host_only);
    assert_eq!(
        header_for(&jar, "https://ex\u{e4}mple.com/"),
        Some("a=1".into())
    );

    // IPv4 forms are normalized.
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("http://127.0.0.1/"),
        &["a=1; Domain=0x7f.0.0.1"],
    );
    assert_eq!(jar.lock().all(NOW)[0].domain, "127.0.0.1");

    // An attribute that is not a valid host rejects the cookie.
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("https://www.example.com/"),
        &["a=1; Domain=exa mple.com", "b=1; Domain=[::1"],
    );
    assert_eq!(names(&jar).len(), 0);
}

#[test]
fn private_suffix_domains_are_separate_sites() {
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("https://a.user.github.io/"),
        &["a=1; Domain=user.github.io"],
    );
    assert_eq!(
        header_for(&jar, "https://user.github.io/"),
        Some("a=1".into())
    );
    assert_eq!(header_for(&jar, "https://other.github.io/"), None);
}

#[test]
fn paths() {
    let jar = CookieJar::new();
    let from = navigation("https://a.test/dir/page");
    set(
        &jar,
        &from,
        &["default=1", "docs=1; Path=/docs", "bad=1; Path=relative"],
    );
    assert_eq!(header_for(&jar, "https://a.test/"), None);
    assert_eq!(
        header_for(&jar, "https://a.test/dir"),
        Some("default=1; bad=1".into())
    );
    assert_eq!(
        header_for(&jar, "https://a.test/dir/other"),
        Some("default=1; bad=1".into())
    );
    assert_eq!(header_for(&jar, "https://a.test/dirt"), None);
    assert_eq!(
        header_for(&jar, "https://a.test/docs"),
        Some("docs=1".into())
    );
    assert_eq!(
        header_for(&jar, "https://a.test/docs/x?y"),
        Some("docs=1".into())
    );
    assert_eq!(header_for(&jar, "https://a.test/doc"), None);
}

#[test]
fn order_is_longer_path_then_creation() {
    let jar = CookieJar::new();
    let from = navigation("https://a.test/");
    set(
        &jar,
        &from,
        &["first=1", "deep=1; Path=/a/b", "second=1", "mid=1; Path=/a"],
    );
    assert_eq!(
        header_for(&jar, "https://a.test/a/b/c"),
        Some("deep=1; mid=1; first=1; second=1".into())
    );
    // A replaced cookie keeps its creation time, so its position.
    set(&jar, &from, &["first=2"]);
    assert_eq!(
        header_for(&jar, "https://a.test/"),
        Some("first=2; second=1".into())
    );
}

#[test]
fn replacement_needs_same_name_domain_host_only_and_path() {
    let jar = CookieJar::new();
    let from = navigation("https://www.example.com/");
    set(
        &jar,
        &from,
        &[
            "x=1",
            "x=2; Domain=www.example.com",
            "x=3; Path=/p",
            "x=4; Domain=example.com",
        ],
    );
    assert_eq!(names(&jar).len(), 4);
    set(&jar, &from, &["x=5; Domain=example.com"]);
    let values: Vec<String> = jar.lock().all(NOW).into_iter().map(|c| c.value).collect();
    assert_eq!(values, ["1", "2", "3", "5"]);
}

#[test]
fn nameless_and_valueless_cookies() {
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("https://a.test/"),
        &["lonely", "empty=", "="],
    );
    assert_eq!(
        header_for(&jar, "https://a.test/"),
        Some("lonely; empty=".into())
    );
}

#[test]
fn secure_cookies_need_a_secure_url() {
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("http://example.com/"),
        &["insecure=1; Secure"],
    );
    assert_eq!(names(&jar).len(), 0);

    set(
        &jar,
        &navigation("https://example.com/"),
        &["s=1; Secure", "plain=1"],
    );
    assert_eq!(
        header_for(&jar, "https://example.com/"),
        Some("s=1; plain=1".into())
    );
    assert_eq!(
        header_for(&jar, "http://example.com/"),
        Some("plain=1".into())
    );

    // Loopback hosts count as secure.
    set(
        &jar,
        &navigation("http://localhost:8000/"),
        &["local=1; Secure"],
    );
    assert_eq!(
        header_for(&jar, "http://localhost:8000/"),
        Some("local=1".into())
    );
}

#[test]
fn insecure_urls_leave_secure_cookies_alone() {
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("https://www.example.com/"),
        &["id=1; Secure; Domain=example.com"],
    );
    let insecure = navigation("http://www.example.com/");
    set(&jar, &insecure, &["id=2", "id=3; Path=/x", "other=1"]);
    assert_eq!(names(&jar), ["id", "other"]);
    // A secure URL can replace it.
    set(
        &jar,
        &navigation("https://www.example.com/"),
        &["id=4; Domain=example.com"],
    );
    assert_eq!(
        header_for(&jar, "http://www.example.com/"),
        Some("id=4; other=1".into())
    );

    // The reverse: an insecure URL cannot set a domain cookie that would
    // overlay a secure host-only cookie of a subdomain.
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("https://www.example.com/"),
        &["id=1; Secure"],
    );
    set(&jar, &insecure, &["id=2; Domain=example.com"]);
    let cookies = jar.lock().all(NOW);
    assert_eq!(cookies.len(), 1);
    assert_eq!(cookies[0].value, "1");
}

#[test]
fn expiry() {
    let jar = CookieJar::new();
    let from = navigation("https://a.test/");
    set(
        &jar,
        &from,
        &[
            "session=1",
            "short=1; Max-Age=60",
            "past=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
        ],
    );
    let request = navigation("https://a.test/");
    assert_eq!(header(&jar, &request), Some("session=1; short=1".into()));
    assert_eq!(
        jar.cookie_header_at(&request, NOW + 59),
        Some("session=1; short=1".into())
    );
    assert_eq!(
        jar.cookie_header_at(&request, NOW + 60),
        Some("session=1".into())
    );
    // Expired cookies are removed.
    assert_eq!(jar.lock().all(i64::MIN).len(), 1);

    // Max-Age=0 and a date in the past delete a cookie.
    set(&jar, &from, &["a=1", "b=1"]);
    set(
        &jar,
        &from,
        &["a=; Max-Age=0", "b=; Expires=Thu, 01 Jan 1970 00:00:00 GMT"],
    );
    assert_eq!(names(&jar), ["session"]);
}

#[test]
fn lifetime_is_capped_at_400_days() {
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("https://a.test/"),
        &["a=1; Max-Age=999999999"],
    );
    assert_eq!(jar.lock().all(NOW)[0].expires, Some(NOW + 400 * 86_400));
}

#[test]
fn long_default_paths_are_rejected() {
    let jar = CookieJar::new();
    // The default paths are "/ppp…" with 1024 and 1025 bytes.
    let ok = format!("https://a.test/{}/page", "p".repeat(1023));
    let long = format!("https://a.test/{}/page", "p".repeat(1024));
    set(&jar, &navigation(&ok), &["ok=1"]);
    set(&jar, &navigation(&long), &["long=1"]);
    assert_eq!(names(&jar), ["ok"]);
}

#[test]
fn cookie_prefixes() {
    let secure = navigation("https://example.com/dir/");
    for (line, accepted) in [
        ("__Secure-a=1", false),
        ("__secure-a=1; Secure", true),
        ("__Host-a=1; Secure", false),
        ("__Host-a=1; Secure; Path=/dir", false),
        ("__Host-a=1; Secure; Path=/; Domain=example.com", false),
        ("__Host-a=1; Path=/", false),
        ("__HOST-a=1; Secure; Path=/", true),
        ("=__Host-x", false),
        ("__Secure-x", false),
        ("x=__Host-x", true),
    ] {
        let jar = CookieJar::new();
        set(&jar, &secure, &[line]);
        assert_eq!(names(&jar).len(), usize::from(accepted), "{line}");
    }
    // On a host without a registrable domain, `Domain=<the host>` gives a
    // host-only cookie, but `__Host-` forbids the attribute itself.
    for (from, domain) in [
        ("https://127.0.0.1/", "127.0.0.1"),
        ("http://localhost/", "localhost"),
    ] {
        let jar = CookieJar::new();
        set(
            &jar,
            &navigation(from),
            &[&format!("__Host-x=1; Secure; Path=/; Domain={domain}")],
        );
        assert_eq!(names(&jar).len(), 0, "{from}");
        set(&jar, &navigation(from), &["__Host-x=1; Secure; Path=/"]);
        assert_eq!(names(&jar), ["__Host-x"], "{from}");
    }
}

#[test]
fn same_site_context() {
    let context = |request: &Request| SameSiteContext::of(request);
    assert_eq!(
        context(&navigation("https://a.test/")),
        SameSiteContext::SameSite
    );
    assert_eq!(
        context(&subresource(
            "https://cdn.example.com/x.png",
            "https://www.example.com/"
        )),
        SameSiteContext::SameSite
    );
    assert_eq!(
        context(&subresource(
            "https://example.org/x.png",
            "https://example.com/"
        )),
        SameSiteContext::CrossSite
    );
    assert_eq!(
        context(&link("https://example.org/", "https://example.com/")),
        SameSiteContext::CrossSiteNavigation
    );
    // Hosts without a registrable domain are their own sites.
    assert_eq!(
        context(&subresource("https://a.test/x.png", "https://www.a.test/")),
        SameSiteContext::CrossSite
    );
    // Sites are schemeful.
    assert_eq!(
        context(&subresource("https://a.test/x.png", "http://a.test/")),
        SameSiteContext::CrossSite
    );
    // An opaque origin is not same-site with anything.
    assert_eq!(
        context(&subresource("https://a.test/x.png", "data:text/html,x")),
        SameSiteContext::CrossSite
    );
    // A subresource without an initiator gets the fewest cookies.
    assert_eq!(
        context(&Request::get(
            url("https://a.test/x.png"),
            Destination::Image
        )),
        SameSiteContext::CrossSite
    );
}

/// Sets one cookie of each `SameSite` value on `https://example.com/`.
fn jar_with_same_site_cookies() -> CookieJar {
    let jar = CookieJar::new();
    set(
        &jar,
        &navigation("https://example.com/"),
        &[
            "strict=1; SameSite=Strict",
            "lax=1; SameSite=Lax",
            "default=1",
            "none=1; SameSite=None; Secure",
        ],
    );
    jar
}

#[test]
fn same_site_retrieval() {
    let jar = jar_with_same_site_cookies();
    let all = Some("strict=1; lax=1; default=1; none=1".to_owned());
    assert_eq!(header(&jar, &navigation("https://example.com/")), all);
    assert_eq!(
        header(
            &jar,
            &subresource("https://example.com/i.png", "https://www.example.com/")
        ),
        all
    );
    // A cross-site subresource gets only SameSite=None cookies.
    assert_eq!(
        header(
            &jar,
            &subresource("https://example.com/i.png", "https://example.org/")
        ),
        Some("none=1".into())
    );
    // A cross-site top-level GET navigation also gets Lax and Default.
    assert_eq!(
        header(&jar, &link("https://example.com/", "https://example.org/")),
        Some("lax=1; default=1; none=1".into())
    );
    // A cross-site top-level POST navigation gets only SameSite=None.
    let mut post = link("https://example.com/", "https://example.org/");
    post.method = Method::Post;
    assert_eq!(header(&jar, &post), Some("none=1".into()));
    // A same-site POST gets all of them.
    let mut post = link("https://example.com/", "https://example.com/form");
    post.method = Method::Post;
    assert_eq!(header(&jar, &post), all);
}

#[test]
fn same_site_storage() {
    let lines = [
        "strict=1; SameSite=Strict",
        "lax=1; SameSite=Lax",
        "default=1",
        "none=1; SameSite=None; Secure",
        "insecure-none=1; SameSite=None",
    ];
    // A cross-site subresource response sets only SameSite=None cookies.
    let jar = CookieJar::new();
    set(
        &jar,
        &subresource("https://example.com/i.png", "https://example.org/"),
        &lines,
    );
    assert_eq!(names(&jar), ["none"]);
    // A cross-site navigation response sets all valid cookies.
    let jar = CookieJar::new();
    set(
        &jar,
        &link("https://example.com/", "https://example.org/"),
        &lines,
    );
    assert_eq!(names(&jar), ["strict", "lax", "default", "none"]);
}

#[test]
fn per_domain_limit_evicts_least_recently_used() {
    let jar = CookieJar::new();
    let from = navigation("https://a.test/");
    set(&jar, &from, &["old=1; Path=/old", "used=1; Path=/used"]);
    for i in 0..MAX_COOKIES_PER_DOMAIN - 2 {
        set(&jar, &from, &[&format!("c{i}=1; Path=/c")]);
    }
    assert_eq!(names(&jar).len(), MAX_COOKIES_PER_DOMAIN);
    // Reading "used" makes it the most recently used cookie.
    assert_eq!(
        header_for(&jar, "https://a.test/used"),
        Some("used=1".into())
    );
    // One more triggers the purge.
    set(&jar, &from, &["last=1"]);
    let names = names(&jar);
    assert_eq!(names.len(), COOKIES_PER_DOMAIN_AFTER_PURGE);
    assert!(!names.contains(&"old".to_owned()));
    assert!(names.contains(&"used".to_owned()));
    assert!(names.contains(&"last".to_owned()));
    // Other domains are not affected.
    set(&jar, &navigation("https://b.test/"), &["b=1"]);
    assert_eq!(header_for(&jar, "https://b.test/"), Some("b=1".into()));
}

#[test]
fn global_limit_evicts_least_recently_used() {
    let jar = CookieJar::new();
    let per_domain = 100;
    let domains = MAX_COOKIES / per_domain;
    for d in 0..domains {
        let from = navigation(&format!("https://d{d}.test/"));
        let lines: Vec<String> = (0..per_domain).map(|i| format!("c{i}=1")).collect();
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        set(&jar, &from, &lines);
    }
    assert_eq!(jar.lock().all(NOW).len(), MAX_COOKIES);
    // Reading d0's cookies makes them recently used.
    assert!(header_for(&jar, "https://d0.test/").is_some());
    set(&jar, &navigation("https://new.test/"), &["n=1"]);
    let cookies = jar.lock().all(NOW);
    assert_eq!(cookies.len(), COOKIES_AFTER_PURGE);
    assert!(cookies.iter().any(|c| c.domain == "new.test"));
    assert_eq!(
        cookies.iter().filter(|c| c.domain == "d0.test").count(),
        per_domain
    );
    // The oldest unused domain lost its cookies.
    assert!(!cookies.iter().any(|c| c.domain == "d1.test"));
}

#[test]
fn only_http_and_https_urls_have_cookies() {
    let jar = CookieJar::new();
    for target in ["file:///tmp/a.html", "data:text/html,x", "about:blank"] {
        set(&jar, &navigation(target), &["a=1"]);
        assert_eq!(header_for(&jar, target), None);
    }
    assert_eq!(names(&jar).len(), 0);
}

#[test]
fn log_messages_do_not_show_values() {
    assert_eq!(log_name("sid=secret; Domain=x"), "\"sid\"");
    assert_eq!(log_name(" sid =secret"), "\"sid\"");
    // A nameless cookie's value can be the session token.
    assert_eq!(log_name("secret"), "<nameless>");
    assert_eq!(log_name("=secret"), "<nameless>");
    assert_eq!(log_name("secret; a=b"), "<nameless>");
}

#[test]
fn public_api_uses_the_system_clock() {
    let jar = CookieJar::new();
    let request = navigation("https://a.test/");
    jar.store_response_cookies(&request, &set_cookie_headers(&["a=1; Max-Age=3600"]));
    assert_eq!(jar.cookie_header(&request), Some("a=1".into()));
    assert_eq!(jar.cookies().len(), 1);
    assert!(format!("{jar:?}").contains("cookies: 1"));
    jar.clear();
    assert_eq!(jar.cookies(), []);
}

#[test]
fn hostile_set_cookie_values_do_not_panic() {
    let jar = CookieJar::new();
    let lines = [
        "",
        ";",
        "=",
        "a=b; Domain=..",
        "a=b; Domain=.",
        "a=b; Domain=[::1]",
        "a=b; Domain=a..test",
        "a=b; Path=/\u{e9}; Domain=\u{e9}.test",
        "a=b; Expires=Fri, 31 Dec 9999 23:59:59 GMT; Max-Age=-1",
        "__Host-=x",
        "\u{e9}=\u{fc}",
    ];
    for from in [
        "https://a.test/",
        "https://www.example.com/",
        "http://127.0.0.1:1/",
        "http://[::1]/",
        "https://xn--nxasmq6b.test/",
        "https://a..test/",
    ] {
        for request in [
            navigation(from),
            link(from, "https://other.test/"),
            subresource(from, "https://other.test/"),
        ] {
            set_at(&jar, &request, &lines, NOW);
            set_at(&jar, &request, &lines, i64::MAX);
            set_at(&jar, &request, &lines, i64::MIN);
            let _ = header(&jar, &request);
        }
    }
}
