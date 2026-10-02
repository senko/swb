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
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let routes: HashMap<&str, Vec<u8>> = routes.into_iter().collect();
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
