//! Record and replay tests.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use super::*;
use crate::fetch::{Fetcher, fetch_following_redirects};
use crate::headers::Headers;
use crate::request::{Destination, Method, Request};
use crate::response::Response;
use crate::test_util::TempDir;

/// Serves canned responses: URL (without fragment) and method to
/// (status, headers, body).
#[derive(Default)]
struct CannedFetcher {
    responses: HashMap<(String, Method), (u16, Headers, Vec<u8>)>,
}

impl CannedFetcher {
    fn add(
        &mut self,
        method: Method,
        url: &str,
        status: u16,
        headers: &[(&str, &str)],
        body: &[u8],
    ) {
        self.responses.insert(
            (url.to_string(), method),
            (status, headers.iter().copied().collect(), body.to_vec()),
        );
    }
}

impl Fetcher for CannedFetcher {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if let Some(result) = crate::builtin::fetch(request) {
            return result;
        }
        let key = (entry_url(&request.url), request.method);
        let (status, headers, body) = self
            .responses
            .get(&key)
            .cloned()
            .ok_or_else(|| NetError::HostNotFound(key.0.clone()))?;
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
        })
    }
}

const PAGE: &[u8] = b"<!DOCTYPE html><p>page</p>";

fn site() -> CannedFetcher {
    let mut site = CannedFetcher::default();
    let html = [
        ("Content-Type", "text/html; charset=utf-8"),
        ("Cache-Control", "no-cache"),
    ];
    site.add(Method::Get, "https://a.test/", 200, &html, PAGE);
    // Same body as "/", so the body file is shared.
    site.add(Method::Get, "https://a.test/copy", 200, &html, PAGE);
    site.add(
        Method::Get,
        "https://a.test/old",
        301,
        &[("Location", "/")],
        b"",
    );
    site.add(
        Method::Get,
        "https://a.test/style.css",
        200,
        &[("Content-Type", "text/css")],
        b"p { color: red }",
    );
    site.add(
        Method::Get,
        "https://a.test/logo.png",
        200,
        &[("Content-Type", "image/png")],
        b"\x89PNG fake",
    );
    site.add(
        Method::Get,
        "https://a.test/data",
        200,
        &[],
        b"\x00\x01binary",
    );
    site.add(
        Method::Get,
        "https://a.test/missing",
        404,
        &[("Content-Type", "text/html")],
        b"not found",
    );
    site.add(Method::Post, "https://a.test/form", 200, &html, b"thanks");
    site.add(Method::Get, "https://a.test/form", 200, &html, b"form");
    site
}

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn get(target: &str) -> Request {
    Request::get(url(target), Destination::Other)
}

fn post(target: &str) -> Request {
    let mut request = get(target);
    request.method = Method::Post;
    request.body = Some(b"x=1".to_vec());
    request
}

/// The requests that `record_site` makes, in order.
fn site_requests() -> Vec<Request> {
    vec![
        get("https://a.test/#top"),
        get("https://a.test/copy"),
        get("https://a.test/old"),
        get("https://a.test/style.css"),
        get("https://a.test/logo.png"),
        get("https://a.test/data"),
        get("https://a.test/missing"),
        post("https://a.test/form"),
        get("https://a.test/form"),
        get("data:,not-recorded"),
        get("about:blank"),
    ]
}

fn record(dir: &Path, requests: &[Request]) -> Vec<Response> {
    let recorder = RecordingFetcher::new(site(), dir).unwrap();
    requests
        .iter()
        .map(|request| recorder.fetch(request).unwrap())
        .collect()
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join(FILES_DIR))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn record_then_replay_gives_the_same_responses() {
    let dir = TempDir::new();
    let requests = site_requests();
    let recorded = record(dir.path(), &requests);

    let replay = ReplayFetcher::load(dir.path()).unwrap();
    // data: and about: are not recorded.
    assert_eq!(replay.len(), 9);
    for (request, original) in requests.iter().zip(&recorded) {
        let replayed = replay.fetch(request).unwrap();
        assert_eq!(replayed.url, request.url);
        assert_eq!(replayed.status, original.status, "{}", request.url);
        assert_eq!(replayed.body, original.body, "{}", request.url);
        assert_eq!(
            replayed.content_type(),
            original.content_type(),
            "{}",
            request.url
        );
        assert_eq!(
            replayed.headers.get("location"),
            original.headers.get("location")
        );
        // Only content-type and location are stored.
        assert_eq!(replayed.headers.get("cache-control"), None);
    }
}

#[test]
fn replay_matches_method_and_ignores_fragment() {
    let dir = TempDir::new();
    record(dir.path(), &site_requests());
    let replay = ReplayFetcher::load(dir.path()).unwrap();

    let response = replay.fetch(&get("https://a.test/#elsewhere")).unwrap();
    assert_eq!(response.url.as_str(), "https://a.test/#elsewhere");
    assert_eq!(response.body, PAGE);

    assert_eq!(
        replay.fetch(&post("https://a.test/form")).unwrap().body,
        b"thanks"
    );
    assert_eq!(
        replay.fetch(&get("https://a.test/form")).unwrap().body,
        b"form"
    );

    match replay.fetch(&get("https://a.test/unknown")) {
        Err(NetError::NotInFixture(url)) => assert_eq!(url.as_str(), "https://a.test/unknown"),
        other => panic!("unexpected result: {other:?}"),
    }
    // A different query is a different URL.
    assert!(replay.fetch(&get("https://a.test/?q")).is_err());
    assert!(replay.fetch(&post("https://a.test/")).is_err());
}

#[test]
fn replay_follows_recorded_redirects() {
    let dir = TempDir::new();
    let recorder = RecordingFetcher::new(site(), dir.path()).unwrap();
    let live = fetch_following_redirects(&recorder, get("https://a.test/old#x")).unwrap();
    assert_eq!(live.url.as_str(), "https://a.test/#x");

    let replay = ReplayFetcher::load(dir.path()).unwrap();
    assert_eq!(replay.len(), 2);
    let replayed = fetch_following_redirects(&replay, get("https://a.test/old#x")).unwrap();
    assert_eq!(replayed.url, live.url);
    assert_eq!(replayed.status, live.status);
    assert_eq!(replayed.body, live.body);
    assert_eq!(replayed.content_type(), live.content_type());
}

#[test]
fn identical_bodies_share_one_file() {
    let dir = TempDir::new();
    record(dir.path(), &site_requests());
    let page_file = body_file_name(PAGE, ContentType::parse("text/html").as_ref());
    let files = files_in(dir.path());
    // 9 entries; "/" and "/copy" share one file.
    assert_eq!(files.len(), 8, "{files:?}");
    assert!(files.contains(&page_file));
    let extensions: Vec<&str> = files
        .iter()
        .map(|name| name.rsplit('.').next().unwrap())
        .collect();
    for extension in ["html", "css", "png", "bin"] {
        assert!(extensions.contains(&extension), "{extension} in {files:?}");
    }
}

#[test]
fn body_file_names() {
    // SHA-256 of the empty string starts with e3b0c44298fc1c14.
    assert_eq!(body_file_name(b"", None), "e3b0c44298fc1c14.bin");
    let css = ContentType::parse("text/css; charset=utf-8");
    assert_eq!(body_file_name(b"", css.as_ref()), "e3b0c44298fc1c14.css");
    let unknown = ContentType::parse("application/x-unknown");
    assert_eq!(
        body_file_name(b"", unknown.as_ref()),
        "e3b0c44298fc1c14.bin"
    );
}

#[test]
fn manifest_format_is_exact() {
    let dir = TempDir::new();
    record(
        dir.path(),
        &[get("https://a.test/style.css"), get("https://a.test/old")],
    );
    let manifest = fs::read_to_string(dir.path().join(MANIFEST_FILE)).unwrap();
    let css = body_file_name(b"p { color: red }", ContentType::parse("text/css").as_ref());
    let empty = body_file_name(b"", None);
    let expected = format!(
        r#"{{
  "version": 1,
  "entries": [
    {{
      "method": "GET",
      "url": "https://a.test/old",
      "status": 301,
      "headers": [
        [
          "location",
          "/"
        ]
      ],
      "body": "files/{empty}"
    }},
    {{
      "method": "GET",
      "url": "https://a.test/style.css",
      "status": 200,
      "headers": [
        [
          "content-type",
          "text/css"
        ]
      ],
      "body": "files/{css}"
    }}
  ]
}}
"#
    );
    assert_eq!(manifest, expected);
}

#[test]
fn manifest_order_does_not_depend_on_request_order() {
    let forward = TempDir::new();
    let backward = TempDir::new();
    let requests = site_requests();
    record(forward.path(), &requests);
    let reversed: Vec<Request> = requests.iter().rev().cloned().collect();
    record(backward.path(), &reversed);
    let forward_manifest = fs::read(forward.path().join(MANIFEST_FILE)).unwrap();
    let backward_manifest = fs::read(backward.path().join(MANIFEST_FILE)).unwrap();
    assert_eq!(forward_manifest, backward_manifest);
    assert_eq!(files_in(forward.path()), files_in(backward.path()));

    let entries = read_manifest(forward.path()).unwrap();
    let keys: Vec<(String, String)> = entries.iter().map(Entry::key).collect();
    assert!(keys.is_sorted());
    assert_eq!(keys[0], ("https://a.test/".to_string(), "GET".to_string()));
}

#[test]
fn recording_merges_with_existing_manifest() {
    let dir = TempDir::new();
    record(
        dir.path(),
        &[get("https://a.test/"), get("https://a.test/style.css")],
    );
    // A second session: one new URL, one changed response.
    let mut changed = CannedFetcher::default();
    changed.add(
        Method::Get,
        "https://a.test/style.css",
        200,
        &[("Content-Type", "text/css")],
        b"p { color: blue }",
    );
    changed.add(
        Method::Get,
        "https://a.test/new.js",
        200,
        &[("Content-Type", "text/javascript")],
        b"1",
    );
    let recorder = RecordingFetcher::new(changed, dir.path()).unwrap();
    recorder.fetch(&get("https://a.test/style.css")).unwrap();
    recorder.fetch(&get("https://a.test/new.js")).unwrap();

    let replay = ReplayFetcher::load(dir.path()).unwrap();
    assert_eq!(replay.len(), 3);
    assert_eq!(replay.fetch(&get("https://a.test/")).unwrap().body, PAGE);
    assert_eq!(
        replay.fetch(&get("https://a.test/style.css")).unwrap().body,
        b"p { color: blue }"
    );
    assert_eq!(
        replay.fetch(&get("https://a.test/new.js")).unwrap().body,
        b"1"
    );
}

#[test]
fn recording_from_several_threads() {
    let dir = TempDir::new();
    let recorder = Arc::new(RecordingFetcher::new(site(), dir.path()).unwrap());
    let handles: Vec<_> = site_requests()
        .into_iter()
        .map(|request| {
            let recorder = Arc::clone(&recorder);
            thread::spawn(move || recorder.fetch(&request).unwrap())
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let reference = TempDir::new();
    record(reference.path(), &site_requests());
    assert_eq!(
        fs::read(dir.path().join(MANIFEST_FILE)).unwrap(),
        fs::read(reference.path().join(MANIFEST_FILE)).unwrap()
    );
}

#[test]
fn errors_are_not_recorded() {
    let dir = TempDir::new();
    let recorder = RecordingFetcher::new(site(), dir.path()).unwrap();
    assert!(recorder.fetch(&get("https://a.test/nothing")).is_err());
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    assert!(replay.is_empty());
}

#[test]
fn replay_serves_data_and_about_blank() {
    let dir = TempDir::new();
    RecordingFetcher::new(site(), dir.path()).unwrap();
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    assert_eq!(replay.fetch(&get("data:,hello")).unwrap().body, b"hello");
    assert_eq!(replay.fetch(&get("about:blank")).unwrap().status, 200);
}

#[test]
fn load_rejects_invalid_fixtures() {
    let dir = TempDir::new();
    assert!(matches!(
        ReplayFetcher::load(dir.path()),
        Err(NetError::Fixture { .. })
    ));

    let manifest = dir.path().join(MANIFEST_FILE);
    fs::write(&manifest, r#"{"version": 2, "entries": []}"#).unwrap();
    let error = ReplayFetcher::load(dir.path()).unwrap_err();
    assert!(error.to_string().contains("version 2"), "{error}");

    fs::write(&manifest, "not json").unwrap();
    assert!(ReplayFetcher::load(dir.path()).is_err());

    for body in ["../outside", "/etc/passwd", "", "files/../../x"] {
        let json = format!(
            r#"{{"version": 1, "entries": [{{"method": "GET", "url": "https://a.test/",
                "status": 200, "headers": [], "body": "{body}"}}]}}"#
        );
        fs::write(&manifest, json).unwrap();
        let error = ReplayFetcher::load(dir.path()).unwrap_err();
        assert!(
            error.to_string().contains("invalid body path"),
            "{body}: {error}"
        );
    }

    fs::write(
        &manifest,
        r#"{"version": 1, "entries": [{"method": "GET", "url": "https://a.test/",
            "status": 200, "headers": [], "body": "files/missing.bin"}]}"#,
    )
    .unwrap();
    assert!(ReplayFetcher::load(dir.path()).is_err());
}

#[test]
fn load_accepts_unknown_fields_and_any_headers() {
    let dir = TempDir::new();
    fs::create_dir(dir.path().join(FILES_DIR)).unwrap();
    fs::write(dir.path().join("files/a.txt"), "hello").unwrap();
    fs::write(
        dir.path().join(MANIFEST_FILE),
        r#"{"version": 1, "source": "chromium", "entries": [
            {"method": "GET", "url": "https://a.test/a", "status": 200,
             "headers": [["content-type", "text/plain"], ["x-extra", "1"]],
             "body": "files/a.txt", "note": "ignored"},
            {"method": "GET", "url": "https://a.test/a", "status": 500,
             "headers": [], "body": "files/a.txt"}
        ]}"#,
    )
    .unwrap();
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    let response = replay.fetch(&get("https://a.test/a")).unwrap();
    // The first of two duplicate entries is used.
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"hello");
    assert_eq!(response.headers.get("x-extra"), Some("1"));
}

#[test]
fn write_atomic_replaces_file() {
    let dir = TempDir::new();
    let path = dir.path().join("file.txt");
    write_atomic(&path, b"one").unwrap();
    write_atomic(&path, b"two").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"two");
    // No temporary files stay.
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
