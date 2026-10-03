//! Tests of [`NetworkFetcher`] against a local HTTP server.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use flate2::Compression;
use flate2::write::GzEncoder;
use url::Url;

use super::*;
use crate::fetch::fetch_following_redirects;
use crate::request::{Destination, Method};

/// A request as the server received it.
struct Received {
    request_line: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

/// An HTTP/1.1 server on 127.0.0.1 that answers each path with a canned
/// response and closes the connection. The thread ends with the test
/// process.
struct TestServer {
    base: Url,
    received: mpsc::Receiver<Received>,
}

impl TestServer {
    fn start(routes: Vec<(&'static str, Vec<u8>)>) -> Self {
        Self::start_with(|_| routes)
    }

    /// Starts a server whose routes depend on its port (for absolute
    /// `Location` URLs).
    fn start_with(routes: impl FnOnce(u16) -> Vec<(&'static str, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let base = Url::parse(&format!("http://{address}/")).unwrap();
        let routes: HashMap<&str, Vec<u8>> = routes(address.port()).into_iter().collect();
        let (sender, received) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let Some(request) = read_request(&stream) else {
                    continue;
                };
                let path = request
                    .request_line
                    .split(' ')
                    .nth(1)
                    .unwrap_or("")
                    .to_string();
                let response = routes
                    .get(path.as_str())
                    .cloned()
                    .unwrap_or_else(|| raw_response("500 No Route", &[], b""));
                let _ = (&stream).write_all(&response);
                let _ = sender.send(request);
            }
        });
        TestServer { base, received }
    }

    fn url(&self, path: &str) -> Url {
        self.base.join(path).unwrap()
    }

    fn next_request(&self) -> Received {
        self.received.recv_timeout(Duration::from_secs(10)).unwrap()
    }
}

fn read_request(stream: &TcpStream) -> Option<Received> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Received {
        request_line: request_line.trim_end().to_string(),
        headers,
        body,
    })
}

fn raw_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("Content-Length: ");
    response.push_str(&body.len().to_string());
    response.push_str("\r\nConnection: close\r\n\r\n");
    let mut response = response.into_bytes();
    response.extend_from_slice(body);
    response
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

const PAGE: &[u8] = b"<!DOCTYPE html><p>Hello</p>";

fn server() -> TestServer {
    TestServer::start(vec![
        (
            "/plain",
            raw_response(
                "200 OK",
                &[("Content-Type", "text/html; charset=utf-8")],
                PAGE,
            ),
        ),
        (
            "/gzip",
            raw_response(
                "200 OK",
                &[("Content-Type", "text/html"), ("Content-Encoding", "gzip")],
                &gzip(PAGE),
            ),
        ),
        (
            "/missing",
            raw_response(
                "404 Not Found",
                &[("Content-Type", "text/plain")],
                b"no such page",
            ),
        ),
        (
            "/error",
            raw_response("503 Service Unavailable", &[], b"try later"),
        ),
        (
            "/redirect",
            raw_response("302 Found", &[("Location", "/plain")], b""),
        ),
        (
            "/latin1",
            // "café" in Latin-1, which is not valid UTF-8.
            b"HTTP/1.1 200 OK\r\nX-Name: caf\xe9\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        ),
        ("/form", raw_response("200 OK", &[], b"posted")),
        (
            "/truncated",
            // Promises 100 bytes, sends 16, then closes the connection.
            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n0123456789abcdef"
                .to_vec(),
        ),
        (
            "/many-codings",
            raw_response(
                "200 OK",
                &[("Content-Encoding", "gzip, gzip, gzip, gzip, gzip")],
                &gzip(&gzip(&gzip(&gzip(&gzip(PAGE))))),
            ),
        ),
        (
            "/set-cookies",
            raw_response(
                "200 OK",
                &[
                    ("Set-Cookie", "a=1"),
                    ("Set-Cookie", "dir=2; Path=/dir"),
                    // http://127.0.0.1 is a loopback URL, so it counts as
                    // secure.
                    ("Set-Cookie", "s=3; Secure; HttpOnly"),
                    ("Set-Cookie", "bad=4; Domain=example.com"),
                ],
                b"",
            ),
        ),
        (
            "/redirect-307",
            raw_response("307 Temporary Redirect", &[("Location", "/form")], b""),
        ),
        (
            "/redirect-303",
            raw_response("303 See Other", &[("Location", "/plain")], b""),
        ),
        (
            "/delete-cookie",
            raw_response("200 OK", &[("Set-Cookie", "a=; Max-Age=0")], b""),
        ),
        (
            "/redirect-with-cookie",
            raw_response(
                "302 Found",
                &[("Location", "/plain"), ("Set-Cookie", "hop=1")],
                b"",
            ),
        ),
        (
            "/same-site-cookies",
            raw_response(
                "200 OK",
                &[
                    ("Set-Cookie", "strict=1; SameSite=Strict"),
                    ("Set-Cookie", "lax=1; SameSite=Lax"),
                    ("Set-Cookie", "none=1; SameSite=None; Secure"),
                ],
                b"",
            ),
        ),
    ])
}

#[test]
fn plain_response_and_default_headers() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let response = fetcher
        .fetch(&Request::get(server.url("/plain"), Destination::Document))
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, PAGE);
    assert_eq!(response.url, server.url("/plain"));
    let content_type = response.content_type().unwrap();
    assert_eq!(content_type.essence, "text/html");
    assert_eq!(content_type.charset.as_deref(), Some("utf-8"));

    let request = server.next_request();
    assert_eq!(request.request_line, "GET /plain HTTP/1.1");
    assert_eq!(request.headers["user-agent"], USER_AGENT);
    assert!(USER_AGENT.starts_with("Mozilla/5.0 (X11; Linux x86_64) swb/"));
    assert!(request.headers["accept"].starts_with("text/html,"));
    assert_eq!(request.headers["accept-encoding"], "gzip, deflate, br");
    assert_eq!(request.headers["accept-language"], "en-US,en;q=0.9");
}

#[test]
fn accept_header_depends_on_destination() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    for (destination, expected) in [
        (Destination::Style, "text/css,*/*;q=0.1"),
        (
            Destination::Image,
            "image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
        ),
        (Destination::Font, "*/*"),
        (Destination::Script, "*/*"),
    ] {
        fetcher
            .fetch(&Request::get(server.url("/plain"), destination))
            .unwrap();
        assert_eq!(server.next_request().headers["accept"], expected);
    }
}

#[test]
fn request_headers_replace_defaults() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let mut request = Request::get(server.url("/plain"), Destination::Document);
    request.headers.append("Accept", "text/plain");
    request.headers.append("X-Extra", "1");
    fetcher.fetch(&request).unwrap();
    let received = server.next_request();
    assert_eq!(received.headers["accept"], "text/plain");
    assert_eq!(received.headers["x-extra"], "1");
}

#[test]
fn gzip_body_is_decompressed() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let response = fetcher
        .fetch(&Request::get(server.url("/gzip"), Destination::Document))
        .unwrap();
    assert_eq!(response.body, PAGE);
    assert_eq!(response.headers.get("content-encoding"), Some("gzip"));
}

#[test]
fn error_statuses_are_responses() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let response = fetcher
        .fetch(&Request::get(server.url("/missing"), Destination::Document))
        .unwrap();
    assert_eq!(response.status, 404);
    assert_eq!(response.body, b"no such page");
    let response = fetcher
        .fetch(&Request::get(server.url("/error"), Destination::Document))
        .unwrap();
    assert_eq!(response.status, 503);
    assert_eq!(response.body, b"try later");
}

#[test]
fn redirects_are_not_followed_by_the_fetcher() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let response = fetcher
        .fetch(&Request::get(
            server.url("/redirect"),
            Destination::Document,
        ))
        .unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(response.headers.get("location"), Some("/plain"));
    assert_eq!(response.url, server.url("/redirect"));
    server.next_request();
    // The server got exactly one request.
    assert!(
        server
            .received
            .recv_timeout(Duration::from_millis(200))
            .is_err()
    );

    let response = fetch_following_redirects(
        &fetcher,
        Request::get(server.url("/redirect#top"), Destination::Document),
    )
    .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, PAGE);
    assert_eq!(response.url, server.url("/plain#top"));
}

#[test]
fn fragment_is_not_sent() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let url = server.url("/plain?q=1#section");
    let response = fetcher
        .fetch(&Request::get(url.clone(), Destination::Document))
        .unwrap();
    assert_eq!(response.url, url);
    assert_eq!(
        server.next_request().request_line,
        "GET /plain?q=1 HTTP/1.1"
    );
}

#[test]
fn post_sends_body() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let mut request = Request::get(server.url("/form"), Destination::Document);
    request.method = Method::Post;
    request.body = Some(b"name=value".to_vec());
    request
        .headers
        .append("Content-Type", "application/x-www-form-urlencoded");
    let response = fetcher.fetch(&request).unwrap();
    assert_eq!(response.body, b"posted");
    let received = server.next_request();
    assert_eq!(received.request_line, "POST /form HTTP/1.1");
    assert_eq!(received.body, b"name=value");
    assert_eq!(
        received.headers["content-type"],
        "application/x-www-form-urlencoded"
    );
}

#[test]
fn non_utf8_header_values_are_latin1() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let response = fetcher
        .fetch(&Request::get(server.url("/latin1"), Destination::Document))
        .unwrap();
    assert_eq!(response.headers.get("x-name"), Some("caf\u{e9}"));
}

#[test]
fn connection_refused_is_an_error() {
    // Nothing can listen on port 0. (A port from a closed listener can be
    // taken by a test server that runs at the same time.)
    let fetcher = NetworkFetcher::without_proxy();
    let url = Url::parse("http://127.0.0.1:0/").unwrap();
    let result = fetcher.fetch(&Request::get(url, Destination::Document));
    assert!(result.is_err(), "{result:?}");
}

#[test]
fn other_schemes() {
    let fetcher = NetworkFetcher::without_proxy();
    let get =
        |url: &str| fetcher.fetch(&Request::get(Url::parse(url).unwrap(), Destination::Other));
    assert_eq!(get("data:,hi").unwrap().body, b"hi");
    assert_eq!(get("about:blank").unwrap().status, 200);
    assert!(matches!(
        get("about:nothing"),
        Err(NetError::UnknownAboutUrl(_))
    ));
    match get("ftp://example.com/") {
        Err(NetError::UnsupportedScheme(scheme)) => assert_eq!(scheme, "ftp"),
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn truncated_body_is_returned_as_far_as_received() {
    let server = server();
    let response = NetworkFetcher::without_proxy()
        .fetch(&Request::get(server.url("/truncated"), Destination::Image))
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"0123456789abcdef");
}

#[test]
fn too_many_content_codings_are_an_error() {
    let server = server();
    let result = NetworkFetcher::without_proxy().fetch(&Request::get(
        server.url("/many-codings"),
        Destination::Document,
    ));
    assert!(
        matches!(result, Err(NetError::ContentDecoding { .. })),
        "{result:?}"
    );
}

/// Fetches `request` and returns the `Cookie` header that the server got.
fn cookie_sent(server: &TestServer, fetcher: &NetworkFetcher, request: &Request) -> Option<String> {
    fetcher.fetch(request).unwrap();
    let received = server.next_request();
    assert!(received.request_line.contains(request.url.path()));
    received.headers.get("cookie").cloned()
}

/// The names of the cookies in the fetcher's jar, oldest first.
fn cookie_names(fetcher: &NetworkFetcher) -> Vec<String> {
    let jar = fetcher.cookie_jar().expect("a network fetcher has a jar");
    jar.cookies().into_iter().map(|c| c.name).collect()
}

#[test]
fn set_cookie_then_cookie_on_next_request() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let get = |path: &str| Request::get(server.url(path), Destination::Document);
    assert_eq!(cookie_sent(&server, &fetcher, &get("/set-cookies")), None);
    assert_eq!(
        cookie_sent(&server, &fetcher, &get("/plain")).as_deref(),
        Some("a=1; s=3")
    );
    // Longer paths first.
    assert_eq!(
        cookie_sent(&server, &fetcher, &get("/dir/page")).as_deref(),
        Some("dir=2; a=1; s=3")
    );
    assert_eq!(cookie_names(&fetcher), ["a", "dir", "s"]);

    // Max-Age=0 deletes the cookie.
    cookie_sent(&server, &fetcher, &get("/delete-cookie"));
    assert_eq!(
        cookie_sent(&server, &fetcher, &get("/plain")).as_deref(),
        Some("s=3")
    );

    // Each fetcher has its own jar.
    let other = NetworkFetcher::without_proxy();
    assert_eq!(cookie_sent(&server, &other, &get("/plain")), None);
}

#[test]
fn cookies_are_stored_on_redirect_hops() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let response = fetch_following_redirects(
        &fetcher,
        Request::get(server.url("/redirect-with-cookie"), Destination::Document),
    )
    .unwrap();
    assert_eq!(response.url, server.url("/plain"));
    assert_eq!(server.next_request().headers.get("cookie"), None);
    // The second hop sends the cookie that the redirect set.
    assert_eq!(
        server
            .next_request()
            .headers
            .get("cookie")
            .map(String::as_str),
        Some("hop=1")
    );
}

#[test]
fn same_site_cookies_for_cross_site_requests() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let same_site = Request::get(server.url("/same-site-cookies"), Destination::Document);
    cookie_sent(&server, &fetcher, &same_site);
    let other_site = Some(Url::parse("https://example.org/").unwrap().origin());

    // A cross-site subresource sends only SameSite=None cookies.
    let image =
        Request::get(server.url("/plain"), Destination::Image).with_initiator(other_site.clone());
    assert_eq!(
        cookie_sent(&server, &fetcher, &image).as_deref(),
        Some("none=1")
    );
    // A cross-site top-level navigation with GET also sends Lax cookies.
    let link = Request::get(server.url("/plain"), Destination::Document)
        .with_initiator(other_site.clone());
    assert_eq!(
        cookie_sent(&server, &fetcher, &link).as_deref(),
        Some("lax=1; none=1")
    );
    // A cross-site top-level POST sends only SameSite=None cookies.
    let post = Request::post(
        server.url("/form"),
        b"a=1".to_vec(),
        "application/x-www-form-urlencoded",
        Destination::Document,
    )
    .with_initiator(other_site);
    assert_eq!(
        cookie_sent(&server, &fetcher, &post).as_deref(),
        Some("none=1")
    );
    // A same-site subresource sends all of them.
    let image = Request::get(server.url("/plain"), Destination::Image)
        .with_initiator(Some(server.url("/").origin()));
    assert_eq!(
        cookie_sent(&server, &fetcher, &image).as_deref(),
        Some("strict=1; lax=1; none=1")
    );
}

#[test]
fn cross_site_subresource_response_sets_only_same_site_none_cookies() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let image = Request::get(server.url("/same-site-cookies"), Destination::Image)
        .with_initiator(Some(Url::parse("https://example.org/").unwrap().origin()));
    fetcher.fetch(&image).unwrap();
    assert_eq!(cookie_names(&fetcher), ["none"]);
}

#[test]
fn post_sends_origin_and_content_length() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let form = |body: &[u8], initiator: Option<&str>| {
        Request::post(
            server.url("/form"),
            body.to_vec(),
            "application/x-www-form-urlencoded",
            Destination::Document,
        )
        .with_initiator(initiator.map(|s| Url::parse(s).unwrap().origin()))
    };

    fetcher
        .fetch(&form(b"name=value", Some("http://127.0.0.1:1/page")))
        .unwrap();
    let received = server.next_request();
    assert_eq!(received.request_line, "POST /form HTTP/1.1");
    assert_eq!(received.body, b"name=value");
    assert_eq!(received.headers["content-length"], "10");
    assert_eq!(
        received.headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    assert_eq!(received.headers["origin"], "http://127.0.0.1:1");

    // An empty body still has a Content-Length.
    fetcher.fetch(&form(b"", None)).unwrap();
    let received = server.next_request();
    assert_eq!(
        received.headers.get("content-length").map(String::as_str),
        Some("0")
    );
    // No initiator, no Origin header.
    assert!(!received.headers.contains_key("origin"));

    // From an https: page to an http: URL, and from an opaque origin, the
    // Origin is "null".
    for initiator in ["https://example.org/", "data:text/html,x"] {
        fetcher.fetch(&form(b"x", Some(initiator))).unwrap();
        assert_eq!(
            server.next_request().headers["origin"],
            "null",
            "{initiator}"
        );
    }

    // GET requests have no Origin header.
    let get = Request::get(server.url("/plain"), Destination::Document)
        .with_initiator(Some(server.url("/").origin()));
    fetcher.fetch(&get).unwrap();
    assert!(!server.next_request().headers.contains_key("origin"));
}

#[test]
fn post_through_307_and_303_redirects() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let origin = server.url("/").origin();
    let post = |path: &str| {
        Request::post(
            server.url(path),
            b"a=1".to_vec(),
            "application/x-www-form-urlencoded",
            Destination::Document,
        )
        .with_initiator(Some(origin.clone()))
    };

    // 307 keeps the method, the body, Content-Type and Origin.
    let response = fetch_following_redirects(&fetcher, post("/redirect-307")).unwrap();
    assert_eq!(response.body, b"posted");
    assert_eq!(
        server.next_request().request_line,
        "POST /redirect-307 HTTP/1.1"
    );
    let second = server.next_request();
    assert_eq!(second.request_line, "POST /form HTTP/1.1");
    assert_eq!(second.body, b"a=1");
    assert_eq!(second.headers["content-length"], "3");
    assert_eq!(
        second.headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    assert_eq!(second.headers["origin"], origin.ascii_serialization());

    // 303 changes to GET without a body, Content-Type and Origin.
    fetch_following_redirects(&fetcher, post("/redirect-303")).unwrap();
    server.next_request();
    let second = server.next_request();
    assert_eq!(second.request_line, "GET /plain HTTP/1.1");
    assert_eq!(second.body, b"");
    for name in ["content-length", "content-type", "origin"] {
        assert!(!second.headers.contains_key(name), "{name}");
    }
}

#[test]
fn cookie_and_origin_request_headers_are_ignored() {
    let server = server();
    let fetcher = NetworkFetcher::without_proxy();
    let with_headers = |mut request: Request| {
        request.headers.append("Cookie", "injected=1");
        request.headers.append("Origin", "https://evil.example");
        request
    };
    let get = |path: &str| Request::get(server.url(path), Destination::Document);
    fetcher.fetch(&with_headers(get("/plain"))).unwrap();
    let received = server.next_request();
    assert!(!received.headers.contains_key("cookie"));
    assert!(!received.headers.contains_key("origin"));

    // The jar's cookies and the initiator's origin are sent instead.
    cookie_sent(&server, &fetcher, &get("/set-cookies"));
    let post = Request::post(
        server.url("/form"),
        b"a=1".to_vec(),
        "text/plain",
        Destination::Document,
    )
    .with_initiator(Some(server.url("/").origin()));
    fetcher.fetch(&with_headers(post)).unwrap();
    let received = server.next_request();
    assert_eq!(received.headers["cookie"], "a=1; s=3");
    assert_eq!(
        received.headers["origin"],
        server.url("/").origin().ascii_serialization()
    );
}

#[test]
fn loopback_hosts_bypass_the_proxy() {
    let server = server();
    // Nothing listens on port 1, so requests through the proxy fail.
    let proxy = ureq::Proxy::new("http://127.0.0.1:1").unwrap();
    let fetcher = NetworkFetcher {
        http: http::HttpClient::with_proxy(Some(proxy)),
    };
    let get = |url: &str| Request::get(Url::parse(url).unwrap(), Destination::Document);
    let response = fetcher.fetch(&get(server.url("/plain").as_str())).unwrap();
    assert_eq!(response.body, PAGE);
    server.next_request();
    // Other hosts go through the proxy (without it, the name would not
    // resolve: `.invalid` never does).
    let result = fetcher.fetch(&get("http://swb.invalid/"));
    assert!(
        matches!(result, Err(NetError::ConnectionFailed(_))),
        "{result:?}"
    );
}

/// Two sites on one test server: `http://127.0.0.1:PORT` and
/// `http://localhost:PORT`. Both count as secure, so `SameSite=None;
/// Secure` cookies work on both.
#[test]
fn each_redirect_hop_has_its_own_same_site_context() {
    let server = TestServer::start_with(|port| {
        let to_ip = format!("http://127.0.0.1:{port}/plain");
        let to_localhost = format!("http://localhost:{port}/set-cookies-2");
        vec![
            (
                "/same-site-cookies",
                raw_response(
                    "200 OK",
                    &[
                        ("Set-Cookie", "strict=1; SameSite=Strict"),
                        ("Set-Cookie", "lax=1; SameSite=Lax"),
                        ("Set-Cookie", "none=1; SameSite=None; Secure"),
                    ],
                    b"",
                ),
            ),
            ("/plain", raw_response("200 OK", &[], PAGE)),
            (
                "/to-ip",
                raw_response("302 Found", &[("Location", &to_ip)], b""),
            ),
            (
                "/to-localhost",
                raw_response("302 Found", &[("Location", &to_localhost)], b""),
            ),
            (
                "/set-cookies-2",
                raw_response(
                    "200 OK",
                    &[
                        ("Set-Cookie", "lax2=1; SameSite=Lax"),
                        ("Set-Cookie", "none2=1; SameSite=None; Secure"),
                    ],
                    b"",
                ),
            ),
        ]
    });
    let port = server.base.port().unwrap();
    let ip = |path: &str| Url::parse(&format!("http://127.0.0.1:{port}{path}")).unwrap();
    let localhost = |path: &str| Url::parse(&format!("http://localhost:{port}{path}")).unwrap();
    let fetcher = NetworkFetcher::without_proxy();
    let follow = |request: Request| fetch_following_redirects(&fetcher, request).unwrap();
    follow(Request::get(
        ip("/same-site-cookies"),
        Destination::Document,
    ));
    server.next_request();

    // A link on localhost to localhost, redirected to 127.0.0.1: the first
    // hop is same-site, the second is a cross-site navigation, so the
    // Strict cookie of 127.0.0.1 stays at home.
    let link = Request::get(localhost("/to-ip"), Destination::Document)
        .with_initiator(Some(localhost("/").origin()));
    follow(link);
    assert_eq!(server.next_request().headers.get("cookie"), None);
    let second = server.next_request();
    assert_eq!(second.request_line, "GET /plain HTTP/1.1");
    assert_eq!(second.headers["cookie"], "lax=1; none=1");

    // An image on 127.0.0.1, redirected to localhost: the second hop is a
    // cross-site subresource, so its response can set only the
    // SameSite=None cookie.
    let image = Request::get(ip("/to-localhost"), Destination::Image)
        .with_initiator(Some(ip("/").origin()));
    follow(image);
    server.next_request();
    server.next_request();
    let jar = fetcher.cookie_jar().unwrap();
    let on_localhost: Vec<String> = jar
        .cookies()
        .into_iter()
        .filter(|c| c.domain == "localhost")
        .map(|c| c.name)
        .collect();
    assert_eq!(on_localhost, ["none2"]);
}
