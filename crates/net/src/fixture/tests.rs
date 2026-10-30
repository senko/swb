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
        let key = (entry_url(&request.url), request.method.clone());
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
            redirected: false,
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
fn extending_fetches_only_missing_requests() {
    let dir = TempDir::new();
    record(dir.path(), &[get("https://a.test/")]);
    let before = fs::read_to_string(dir.path().join(MANIFEST_FILE)).unwrap();
    // The site has a newer page now; the fixture keeps the old one.
    let mut site = site();
    site.add(Method::Get, "https://a.test/", 200, &[], b"new");
    let extending = ExtendingFetcher::new(site, dir.path()).unwrap();
    assert_eq!(extending.fetch(&get("https://a.test/")).unwrap().body, PAGE);
    let css = extending.fetch(&get("https://a.test/style.css")).unwrap();
    assert_eq!(css.body, b"p { color: red }");
    assert!(extending.fetch(&get("https://a.test/missing")).is_ok());
    // Network errors are passed on and not recorded.
    assert!(extending.fetch(&get("https://other.test/")).is_err());

    let replay = ReplayFetcher::load(dir.path()).unwrap();
    assert_eq!(replay.len(), 3);
    assert_eq!(replay.fetch(&get("https://a.test/")).unwrap().body, PAGE);
    assert!(replay.fetch(&get("https://a.test/style.css")).is_ok());
    let after = fs::read_to_string(dir.path().join(MANIFEST_FILE)).unwrap();
    assert!(after.len() > before.len());
}

/// Recording keeps the cookies of the network fetcher; replay has a jar of
/// its own, for scripts, that no request uses.
#[test]
fn replay_has_a_jar_for_scripts_only() {
    let dir = TempDir::new();
    let recorder = RecordingFetcher::new(crate::NetworkFetcher::new(), dir.path()).unwrap();
    let jar = recorder.cookie_jar().unwrap();
    assert!(std::ptr::eq(jar, recorder.inner().cookie_jar().unwrap()));
    let extending = ExtendingFetcher::new(crate::NetworkFetcher::new(), dir.path()).unwrap();
    assert!(extending.cookie_jar().is_some());
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    let jar = replay.cookie_jar().unwrap();
    assert_eq!(jar.cookies(), []);
    let page = Url::parse("https://a.test/").unwrap();
    jar.set_script_cookie(&page, "s=1");
    assert_eq!(jar.script_cookie_string(&page), "s=1");
    assert_eq!(jar.cookies().len(), 1);
    assert!(
        RecordingFetcher::new(site(), dir.path())
            .unwrap()
            .cookie_jar()
            .is_none()
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

// ----- CORS in the replay (ADR 0028 section 12) -----

/// A fixture directory with two files and a `fixture.json`.
fn cors_fixture(meta: Option<&str>) -> (TempDir, ReplayFetcher) {
    let dir = TempDir::new();
    fs::create_dir_all(dir.path().join(FILES_DIR)).unwrap();
    fs::write(dir.path().join("files/a.txt"), b"hello").unwrap();
    let entry = |url: &str| {
        format!(
            r#"{{"method": "GET", "url": "{url}", "status": 200,
                "headers": [["content-type", "text/plain"], ["access-control-allow-origin", "https://stored.test"]],
                "body": "files/a.txt"}}"#
        )
    };
    let entries = [
        entry("https://api.test/data"),
        entry("https://api.test/static/9/app.js"),
        entry("https://api.test/static/12/app.js"),
        entry("https://api.test/static/100/other.js"),
    ]
    .join(",");
    fs::write(
        dir.path().join("manifest.json"),
        format!(r#"{{"version": 1, "entries": [{entries}]}}"#),
    )
    .unwrap();
    if let Some(meta) = meta {
        fs::write(dir.path().join("fixture.json"), meta).unwrap();
    }
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    (dir, replay)
}

fn cors_get(target: &str) -> Request {
    let mut request = get(target);
    request.initiator = Some(url("https://app.test/").origin());
    request.mode = crate::request::RequestMode::Cors;
    request.tainting = crate::request::Tainting::Cors;
    request
}

#[test]
fn replay_reflects_the_origin_of_cors_requests() {
    let (_dir, replay) = cors_fixture(None);
    let response = replay.fetch(&cors_get("https://api.test/data")).unwrap();
    let get = |name: &str| response.headers.get(name);
    assert_eq!(get("access-control-allow-origin"), Some("https://app.test"));
    assert_eq!(get("access-control-allow-credentials"), Some("true"));
    assert_eq!(get("vary"), Some("Origin"));
    // The names of the stored headers (not the stored CORS header).
    assert_eq!(
        get("access-control-expose-headers"),
        Some("content-type, access-control-allow-origin")
    );
    assert_eq!(
        response
            .headers
            .get_all("access-control-allow-origin")
            .count(),
        1
    );
    // A redirect-tainted request: `null`.
    let mut tainted = cors_get("https://api.test/data");
    tainted.tainted_origin = true;
    let response = replay.fetch(&tainted).unwrap();
    assert_eq!(
        response.headers.get("access-control-allow-origin"),
        Some("null")
    );
    // Without an Origin header the response has no CORS header of ours.
    let plain = replay.fetch(&get_plain("https://api.test/data")).unwrap();
    assert!(!plain.headers.contains("access-control-allow-credentials"));
}

fn get_plain(target: &str) -> Request {
    get(target)
}

#[test]
fn replay_answers_preflights_permissively() {
    let (_dir, replay) = cors_fixture(None);
    let mut preflight = cors_get("https://api.test/not-recorded");
    preflight.method = Method::Options;
    preflight.credentials = crate::request::CredentialsMode::Omit;
    preflight
        .headers
        .append("access-control-request-method", "PUT");
    preflight
        .headers
        .append("access-control-request-headers", "content-type,x-token");
    let response = replay.fetch(&preflight).unwrap();
    assert_eq!(response.status, 204);
    let get = |name: &str| response.headers.get(name);
    assert_eq!(get("access-control-allow-origin"), Some("https://app.test"));
    assert_eq!(get("access-control-allow-credentials"), Some("true"));
    assert_eq!(get("access-control-allow-methods"), Some("PUT"));
    assert_eq!(
        get("access-control-allow-headers"),
        Some("content-type,x-token")
    );
    // The check of net accepts it for a credentialed PUT.
    let mut put = cors_get("https://api.test/not-recorded");
    put.method = Method::Put;
    put.credentials = crate::request::CredentialsMode::Include;
    put.headers.append("x-token", "1");
    put.headers.append("content-type", "application/json");
    assert!(crate::cors::check_preflight(&put, &response).is_ok());
}

#[test]
fn replay_patterns_serve_the_greatest_recorded_url() {
    let meta = r#"{"replay_patterns": ["https://api.test/static/*/app.js"]}"#;
    let (_dir, replay) = cors_fixture(Some(meta));
    let served = |u: &str| replay.fetch(&get(u));
    // Exact entries win.
    assert!(served("https://api.test/static/9/app.js").is_ok());
    // A missing number gets the greatest recorded (12, not 9).
    assert!(served("https://api.test/static/5000/app.js").is_ok());
    // Another file name, another segment count, or another host: no match.
    assert!(served("https://api.test/static/5000/nope.js").is_err());
    assert!(served("https://api.test/static/a/b/app.js").is_err());
    assert!(served("https://other.test/static/5/app.js").is_err());
    // Without the pattern the request fails.
    let (_dir2, plain) = cors_fixture(None);
    assert!(
        plain
            .fetch(&get("https://api.test/static/5000/app.js"))
            .is_err()
    );
}

#[test]
fn pattern_and_natural_order() {
    use std::cmp::Ordering;
    assert!(pattern_matches("https://a.test/*/x", "https://a.test/12/x"));
    assert!(!pattern_matches("https://a.test/*/x", "https://a.test//x"));
    assert!(!pattern_matches(
        "https://a.test/*/x",
        "https://a.test/1/2/x"
    ));
    assert!(!pattern_matches(
        "https://a.test/*/x",
        "https://a.test/1/x?q"
    ));
    assert!(pattern_matches("a*b*c", "aXbYc"));
    assert!(pattern_matches("https://a.test/x", "https://a.test/x"));
    assert!(!pattern_matches("https://a.test/x", "https://a.test/xy"));
    assert_eq!(natural_cmp("/d/9/", "/d/12/"), Ordering::Less);
    assert_eq!(natural_cmp("/d/012/", "/d/12/"), Ordering::Equal);
    assert_eq!(natural_cmp("/d/b", "/d/a"), Ordering::Greater);
}

#[test]
fn bad_replay_patterns_are_an_error() {
    let dir = TempDir::new();
    fs::create_dir_all(dir.path().join(FILES_DIR)).unwrap();
    fs::write(
        dir.path().join("manifest.json"),
        r#"{"version": 1, "entries": []}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("fixture.json"),
        r#"{"replay_patterns": [1]}"#,
    )
    .unwrap();
    assert!(ReplayFetcher::load(dir.path()).is_err());
}
