//! Tests of the media byte source against the local test server
//! ([`crate::test_server`]) and the replay of fixtures. No network.

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::fetch::Fetcher;
use crate::fixture::ReplayFetcher;
use crate::loader::Loader;
use crate::network::NetworkFetcher;
use crate::request::{CredentialsMode, Destination, RequestMode};
use crate::test_server::{Body, Lie, Route, TestServer, pattern_byte};
use crate::test_util::TempDir;

const MIB: usize = 1 << 20;

fn loader_for(fetcher: Arc<dyn Fetcher>) -> Loader {
    Loader::new(fetcher, 1, Arc::new(|| {}))
}

fn loader() -> Loader {
    loader_for(Arc::new(NetworkFetcher::without_proxy()))
}

fn request(url: Url) -> Request {
    Request::get(url, Destination::Video)
}

fn options(cache: usize, keep_behind: usize, read_ahead: ReadAhead) -> MediaOptions {
    MediaOptions {
        cache_bytes: cache,
        keep_behind_bytes: keep_behind,
        read_ahead,
    }
}

/// Waits until `condition` holds.
fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "timeout: {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

fn read_exact(resource: &mut MediaResource, n: usize) -> Vec<u8> {
    let mut data = vec![0; n];
    let mut filled = 0;
    while filled < n {
        let got = resource.read(&mut data[filled..]).unwrap();
        assert!(got > 0, "the resource ended at {filled} of {n}");
        filled += got;
    }
    data
}

fn read_to_end(resource: &mut MediaResource) -> Vec<u8> {
    let mut data = Vec::new();
    let mut buf = vec![0; 40_000];
    loop {
        match resource.read(&mut buf).unwrap() {
            0 => return data,
            n => data.extend_from_slice(&buf[..n]),
        }
    }
}

fn is_pattern(offset: u64, bytes: &[u8]) -> bool {
    bytes
        .iter()
        .enumerate()
        .all(|(i, &b)| b == pattern_byte(offset + i as u64))
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len as u64).map(pattern_byte).collect()
}

fn ranges(server: &TestServer) -> Vec<String> {
    server
        .requests()
        .iter()
        .map(|r| format!("{} {}", r.path, r.range.clone().unwrap_or_default()))
        .collect()
}

#[test]
fn the_first_request_asks_for_bytes_from_zero_and_the_whole_file_reads() {
    let server = TestServer::start(vec![("/v.mp4", Route::bytes(pattern(300_000)))]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    assert_eq!(resource.size(), Some(300_000));
    assert_eq!(read_to_end(&mut resource), pattern(300_000));
    assert_eq!(read_exact_at_end(&mut resource), 0);
    let log = server.requests();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].range.as_deref(), Some("bytes=0-"));
    assert_eq!(
        log[0].headers.get("accept-encoding").map(String::as_str),
        Some("identity")
    );
    assert!(resource.handle().stats().seekable);
}

fn read_exact_at_end(resource: &mut MediaResource) -> usize {
    resource.read(&mut [0; 10]).unwrap()
}

#[test]
fn seeks_inside_the_cache_send_no_request() {
    let server = TestServer::start(vec![("/v.mp4", Route::bytes(pattern(300_000)))]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    assert_eq!(read_exact(&mut resource, 200_000), pattern(200_000));
    assert_eq!(resource.seek(1000).unwrap(), 1000);
    assert_eq!(read_exact(&mut resource, 100), pattern(1100)[1000..]);
    assert_eq!(resource.seek(299_900).unwrap(), 299_900);
    assert_eq!(read_to_end(&mut resource), pattern(300_000)[299_900..]);
    // Past the end reads nothing.
    assert_eq!(resource.seek(1 << 40).unwrap(), 1 << 40);
    assert_eq!(read_exact_at_end(&mut resource), 0);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn a_far_seek_opens_a_request_at_the_block_boundary() {
    let server = TestServer::start(vec![("/v.mp4", Route::new(Body::Pattern(32 * MIB as u64)))]);
    let loader = loader();
    let opts = options(8 * MIB, 2 * MIB, ReadAhead::Bytes(MIB as u64));
    let mut resource = loader.open_media_with(request(server.url("/v.mp4")), opts);
    assert_eq!(resource.size(), Some(32 * MIB as u64));
    assert!(is_pattern(0, &read_exact(&mut resource, 100)));
    // 20 MiB + 5 bytes: the request starts at the block, the read at the byte.
    let target = 20 * MIB as u64 + 5;
    resource.seek(target).unwrap();
    assert!(is_pattern(target, &read_exact(&mut resource, 1000)));
    let log = ranges(&server);
    assert_eq!(log[0], "/v.mp4 bytes=0-");
    assert_eq!(log.last().unwrap(), &format!("/v.mp4 bytes={}-", 20 * MIB));
    // Back to the start: the first blocks are within the cache or fetched
    // again at their own boundary.
    resource.seek(3).unwrap();
    assert!(is_pattern(3, &read_exact(&mut resource, 5000)));
    for line in ranges(&server) {
        let from: u64 = line
            .split("bytes=")
            .nth(1)
            .unwrap()
            .trim_end_matches('-')
            .parse()
            .unwrap();
        assert_eq!(from % BLOCK as u64, 0, "{line}");
    }
}

#[test]
fn the_request_ends_when_the_cache_holds_enough_and_a_new_one_opens_later() {
    let server = TestServer::start(vec![("/v.mp4", Route::new(Body::Pattern(64 * MIB as u64)))]);
    let loader = loader();
    let opts = options(16 * MIB, 4 * MIB, ReadAhead::Bytes(2 * MIB as u64));
    let mut resource = loader.open_media_with(request(server.url("/v.mp4")), opts);
    let handle = resource.handle();
    assert!(is_pattern(0, &read_exact(&mut resource, 10)));
    // The job stops at the target: 2 MiB (and one block), not the 64 MiB.
    wait_for("the read-ahead target", || {
        handle.stats().cached_bytes >= 2 * MIB
    });
    thread::sleep(Duration::from_millis(100));
    let stats = handle.stats();
    assert!(stats.cached_bytes <= 2 * MIB + 2 * BLOCK, "{stats:?}");
    assert_eq!(stats.requests, 1);
    // The reader comes within half of the target: a second request, at a
    // block boundary after the first one's end.
    let mut at = 10;
    while handle.stats().requests < 2 {
        assert!(is_pattern(at, &read_exact(&mut resource, 50_000)));
        at += 50_000;
        assert!(at < 4 * MIB as u64, "no second request");
    }
    wait_for("the second request", || server.requests().len() == 2);
    let log = ranges(&server);
    let second: u64 = log[1]
        .split("bytes=")
        .nth(1)
        .unwrap()
        .trim_end_matches('-')
        .parse()
        .unwrap();
    assert!(
        second >= 2 * MIB as u64 && second.is_multiple_of(BLOCK as u64),
        "{log:?}"
    );
    // The cache stays within its limit while the whole body is read.
    while at < 40 * MIB as u64 {
        assert!(is_pattern(at, &read_exact(&mut resource, 64 * 1024)));
        at += 64 * 1024;
    }
    assert!(
        handle.stats().peak_cached_bytes <= 16 * MIB,
        "{:?}",
        handle.stats()
    );
}

#[test]
fn read_ahead_in_seconds_uses_the_bit_rate_and_the_limits() {
    let server = TestServer::start(vec![("/v.mp4", Route::new(Body::Pattern(40 * MIB as u64)))]);
    let loader = loader();
    // 40 MiB over 40 s is 1 MiB/s: 3 s are 3 MiB.
    let mut resource = loader.open_media_with(
        request(server.url("/v.mp4")),
        options(32 * MIB, 4 * MIB, ReadAhead::METADATA),
    );
    let handle = resource.handle();
    handle.set_duration(Some(40.0));
    assert_eq!(resource.size(), Some(40 * MIB as u64));
    read_exact(&mut resource, 1);
    wait_for("3 MiB", || handle.stats().cached_bytes >= 3 * MIB);
    thread::sleep(Duration::from_millis(100));
    assert!(
        handle.stats().cached_bytes <= 3 * MIB + 2 * BLOCK,
        "{:?}",
        handle.stats()
    );
    // A target above the cache limit (minus the kept bytes) is clamped.
    handle.set_read_ahead(ReadAhead::Seconds(1000.0));
    wait_for("the clamped target", || {
        handle.stats().cached_bytes >= 28 * MIB
    });
    thread::sleep(Duration::from_millis(100));
    assert!(
        handle.stats().cached_bytes <= 28 * MIB + 2 * BLOCK,
        "{:?}",
        handle.stats()
    );
}

#[test]
fn cancel_from_another_thread_ends_a_blocked_read_at_once() {
    // The body stalls after 1,000 bytes: no block is complete.
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(300_000)).stalling_after(1000),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    assert_eq!(resource.size(), Some(300_000));
    let handle = resource.handle();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        handle.cancel();
        Instant::now()
    });
    let start = Instant::now();
    let error = resource.read(&mut [0; 100]).unwrap_err();
    let ended = Instant::now();
    let cancelled_at = canceller.join().unwrap();
    assert!(
        start.elapsed() >= Duration::from_millis(150),
        "the read waited"
    );
    assert!(ended.duration_since(cancelled_at) < Duration::from_millis(500));
    assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
    // Later calls fail at once, too.
    assert!(resource.read(&mut [0; 100]).is_err());
    assert!(resource.seek(5).is_err());
}

#[test]
fn cancel_while_the_server_does_not_answer() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(1000)).holding_headers(Duration::from_secs(60)),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    let handle = resource.handle();
    assert!(!handle.wait_open(Duration::from_millis(100)));
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        handle.cancel();
    });
    let start = Instant::now();
    assert_eq!(resource.size(), None);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(resource.read(&mut [0; 10]).is_err());
    canceller.join().unwrap();
}

#[test]
fn a_cancelled_request_gives_its_place_in_the_pool_to_the_next() {
    let stalled = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(1000)).holding_headers(Duration::from_secs(60)),
    )]);
    let good = TestServer::start(vec![("/v.mp4", Route::bytes(pattern(1000)))]);
    let loader = loader();
    let blocked: Vec<MediaResource> = (0..pool::MAX_THREADS)
        .map(|_| loader.open_media(request(stalled.url("/v.mp4"))))
        .collect();
    wait_for("16 connections", || {
        stalled.requests().len() == pool::MAX_THREADS
    });
    let mut waiting = loader.open_media(request(good.url("/v.mp4")));
    assert!(!waiting.handle().wait_open(Duration::from_millis(200)));
    blocked[0].cancel();
    assert!(waiting.handle().wait_open(Duration::from_secs(10)));
    assert_eq!(read_to_end(&mut waiting), pattern(1000));
}

#[test]
fn a_server_without_range_gives_a_stream_that_cannot_seek() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(200_000)).without_ranges(),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    assert_eq!(resource.size(), None);
    let error = resource.seek(10).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert_eq!(read_to_end(&mut resource), pattern(200_000));
    assert!(!resource.handle().stats().seekable);
    // The request still asked for a range.
    assert_eq!(ranges(&server), ["/v.mp4 bytes=0-"]);
}

#[test]
fn an_endless_stream_stays_within_its_memory_and_ends_with_the_reader() {
    let server = TestServer::start(vec![("/v.mp4", Route::new(Body::Endless))]);
    let loader = loader();
    let opts = options(8 * MIB, 2 * MIB, ReadAhead::Bytes(MIB as u64));
    let mut resource = loader.open_media_with(request(server.url("/v.mp4")), opts);
    let handle = resource.handle();
    // Nothing reads: the job waits when the target is reached.
    assert!(is_pattern(0, &read_exact(&mut resource, 10)));
    wait_for("the target", || handle.stats().cached_bytes >= MIB);
    thread::sleep(Duration::from_millis(200));
    assert!(
        handle.stats().cached_bytes <= MIB + 2 * BLOCK,
        "{:?}",
        handle.stats()
    );
    // Reading 40 MiB keeps the cache within its limit.
    let mut at = 10;
    while at < 40 * MIB as u64 {
        assert!(is_pattern(at, &read_exact(&mut resource, 100_000)));
        at += 100_000;
    }
    assert!(
        handle.stats().peak_cached_bytes <= 8 * MIB,
        "{:?}",
        handle.stats()
    );
    // Dropping the reader ends the job and the connection.
    drop(resource);
    drop(handle);
    wait_for("the connection closed", || server.live_connections() == 0);
}

#[test]
fn statuses_other_than_200_and_206_fail_the_resource() {
    let server = TestServer::start(vec![
        ("/404", Route::status(404)),
        ("/500", Route::status(500)),
        ("/empty", Route::bytes(Vec::new())),
    ]);
    let loader = loader();
    for (path, status) in [
        ("/404", 404),
        ("/500", 500),
        ("/empty", 416),
        ("/none", 404),
    ] {
        let mut resource = loader.open_media(request(server.url(path)));
        assert_eq!(resource.size(), None, "{path}");
        assert!(resource.read(&mut [0; 10]).is_err(), "{path}");
        assert!(resource.seek(0).is_err(), "{path}");
        let error = resource.handle().error().expect("an error");
        assert!(
            matches!(&*error, NetError::Status(s) if *s == status),
            "{path}: {error:?}"
        );
    }
}

#[test]
fn redirects_are_followed_and_later_ranges_go_to_the_final_url() {
    let server = TestServer::start(vec![
        ("/a", Route::redirect("/b")),
        ("/b", Route::redirect("/c.mp4")),
        ("/c.mp4", Route::new(Body::Pattern(16 * MIB as u64))),
    ]);
    let loader = loader();
    let opts = options(8 * MIB, 2 * MIB, ReadAhead::Bytes(MIB as u64));
    let mut resource = loader.open_media_with(request(server.url("/a")), opts);
    assert_eq!(resource.size(), Some(16 * MIB as u64));
    assert!(is_pattern(0, &read_exact(&mut resource, 100)));
    resource.seek(10 * MIB as u64).unwrap();
    assert!(is_pattern(10 * MIB as u64, &read_exact(&mut resource, 100)));
    let log = ranges(&server);
    assert_eq!(
        log,
        [
            "/a bytes=0-",
            "/b bytes=0-",
            "/c.mp4 bytes=0-",
            &format!("/c.mp4 bytes={}-", 10 * MIB),
        ]
    );
}

#[test]
fn a_body_that_closes_early_is_requested_again_where_it_stopped() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(MIB)).closing_after(300_000, 1),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    assert_eq!(read_to_end(&mut resource), pattern(MIB));
    // 4 whole blocks arrived (262,144 bytes), the rest of the block did not.
    assert_eq!(ranges(&server), ["/v.mp4 bytes=0-", "/v.mp4 bytes=262144-"]);
}

#[test]
fn a_server_that_always_closes_early_fails_the_resource() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(MIB)).closing_after(100, 1000),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    let start = Instant::now();
    assert!(resource.read(&mut [0; 10]).is_err());
    assert!(start.elapsed() < Duration::from_secs(10));
    // The first request, two more, then it gives up.
    assert_eq!(server.requests().len(), 3);
    assert!(resource.handle().error().is_some());
}

#[test]
fn a_failed_resource_still_serves_its_cache() {
    // The first 200,000 bytes arrive, then the connection closes and the
    // next request gets a 500.
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(MIB))
            .closing_after(200_000, 1)
            .failing_after(1, 500),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    // Blocks 0 to 2 (196,608 bytes) are cached; the next ones fail.
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let error = loop {
        match resource.read(&mut buf) {
            Ok(0) => panic!("the resource ended"),
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(error) => break error,
        }
    };
    assert_eq!(data, pattern(196_608));
    assert!(error.to_string().contains("500"), "{error}");
}

#[test]
fn cors_applies_to_the_request_of_the_element() {
    let origin = Url::parse("https://page.test/").unwrap().origin();
    let server = TestServer::start(vec![
        ("/plain.mp4", Route::bytes(pattern(5000))),
        (
            "/cors.mp4",
            Route::bytes(pattern(5000))
                .with_header("Access-Control-Allow-Origin", "https://page.test"),
        ),
    ]);
    let loader = loader();
    let cors = |path: &str| {
        let mut r = request(server.url(path)).with_initiator(Some(origin.clone()));
        r.mode = RequestMode::Cors;
        r.credentials = CredentialsMode::SameOrigin;
        r
    };
    // `crossorigin`: the check needs `Access-Control-Allow-Origin`.
    let mut denied = loader.open_media(cors("/plain.mp4"));
    assert_eq!(denied.size(), None);
    assert!(matches!(
        denied.handle().error().as_deref(),
        Some(NetError::Cors(_))
    ));
    let mut allowed = loader.open_media(cors("/cors.mp4"));
    assert_eq!(read_to_end(&mut allowed), pattern(5000));
    let sent = &server.requests();
    assert_eq!(
        sent.last()
            .unwrap()
            .headers
            .get("origin")
            .map(String::as_str),
        Some("https://page.test")
    );
    // Without the attribute the request is `no-cors`: an opaque response
    // plays.
    let plain = request(server.url("/plain.mp4")).with_initiator(Some(origin));
    let mut opaque = loader.open_media(plain);
    assert_eq!(read_to_end(&mut opaque), pattern(5000));
}

#[test]
fn file_urls_read_directly_and_seek() {
    let dir = TempDir::new();
    let path = dir.path().join("a.mp4");
    fs::write(&path, pattern(100_000)).unwrap();
    let loader = loader();
    let mut resource = loader.open_media(request(Url::from_file_path(&path).unwrap()));
    assert_eq!(resource.size(), Some(100_000));
    assert_eq!(read_exact(&mut resource, 10), pattern(10));
    resource.seek(99_990).unwrap();
    assert_eq!(read_to_end(&mut resource), pattern(100_000)[99_990..]);
    let missing = dir.path().join("none.mp4");
    let mut gone = loader.open_media(request(Url::from_file_path(&missing).unwrap()));
    assert!(gone.read(&mut [0; 4]).is_err());
    assert!(matches!(
        gone.handle().error().as_deref(),
        Some(NetError::Status(404))
    ));
    let mut folder = loader.open_media(request(Url::from_directory_path(dir.path()).unwrap()));
    assert!(folder.read(&mut [0; 4]).is_err());
    assert!(folder.handle().error().is_some());
    // Cancel works for a file, too.
    resource.cancel();
    assert!(resource.read(&mut [0; 4]).is_err());
}

#[test]
fn data_and_memory_resources_are_in_memory() {
    let loader = loader();
    let url = Url::parse("data:video/mp4;base64,AAECAwQFBgcICQ==").unwrap();
    let mut resource = loader.open_media(request(url));
    assert_eq!(resource.size(), Some(10));
    assert_eq!(read_exact(&mut resource, 3), [0, 1, 2]);
    resource.seek(8).unwrap();
    assert_eq!(read_to_end(&mut resource), [8, 9]);
    let mut blob = MediaResource::from_bytes(vec![7u8; 5]);
    assert_eq!(blob.size(), Some(5));
    assert_eq!(read_to_end(&mut blob), [7; 5]);
    let mut bad = loader.open_media(request(Url::parse("data:nocomma").unwrap()));
    assert!(bad.read(&mut [0; 4]).is_err());
    let mut other = loader.open_media(request(Url::parse("ftp://a.test/x").unwrap()));
    assert!(other.read(&mut [0; 4]).is_err());
}

#[test]
fn resume_reads_the_cache_again_after_a_cancel() {
    let server = TestServer::start(vec![("/v.mp4", Route::new(Body::Pattern(32 * MIB as u64)))]);
    let loader = loader();
    let opts = options(8 * MIB, 2 * MIB, ReadAhead::Bytes(MIB as u64));
    let mut resource = loader.open_media_with(request(server.url("/v.mp4")), opts);
    let handle = resource.handle();
    assert!(is_pattern(0, &read_exact(&mut resource, 1000)));
    handle.cancel();
    assert!(resource.read(&mut [0; 10]).is_err());
    handle.resume();
    let mut again = handle.reader();
    assert!(is_pattern(0, &read_exact(&mut again, 1000)));
    // Past the cache a new request opens.
    again.seek(20 * MIB as u64).unwrap();
    assert!(is_pattern(20 * MIB as u64, &read_exact(&mut again, 1000)));
}

#[test]
fn several_resources_share_the_page_budget() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::new(Body::Pattern(200 * MIB as u64)),
    )]);
    let loader = loader();
    let budget = Arc::clone(loader.media_budget());
    let opts = options(64 * MIB, 8 * MIB, ReadAhead::Bytes(8 * MIB as u64));
    let mut resources: Vec<MediaResource> = (0..8)
        .map(|_| loader.open_media_with(request(server.url("/v.mp4")), opts))
        .collect();
    for resource in &mut resources {
        assert!(is_pattern(0, &read_exact(resource, 10)));
    }
    wait_for("the caches fill", || budget.used() >= 8 * 8 * MIB);
    assert!(budget.used() <= budget.limit());
    drop(resources);
    wait_for("the budget is free", || budget.used() == 0);
}

#[test]
fn the_page_budget_stalls_a_resource_that_cannot_reserve() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::new(Body::Pattern(200 * MIB as u64)),
    )]);
    let loader = loader();
    // Take all but 2 MiB of the page budget.
    let hold = crate::budget::Reservation::new(loader.media_budget(), PAGE_BUDGET_BYTES - 2 * MIB)
        .expect("fits");
    let opts = options(64 * MIB, 8 * MIB, ReadAhead::Bytes(16 * MIB as u64));
    let mut resource = loader.open_media_with(request(server.url("/v.mp4")), opts);
    let handle = resource.handle();
    assert!(is_pattern(0, &read_exact(&mut resource, 10)));
    wait_for("stalled", || handle.stats().stalled);
    assert!(handle.stats().cached_bytes <= 2 * MIB);
    // The reader still gets its data: the job waits for room, which reading
    // ahead of the window frees.
    let mut at = 10u64;
    while at < 6 * MIB as u64 {
        assert!(is_pattern(at, &read_exact(&mut resource, 65_536)));
        at += 65_536;
    }
    drop(hold);
    wait_for("room again", || !handle.stats().stalled);
}

/// A fixture directory with one recorded media response; the body file is
/// `body` (copied or linked by the caller into `files/`).
fn fixture_with(dir: &Path, url: &str, body_name: &str) {
    fs::create_dir_all(dir.join("files")).unwrap();
    let manifest = format!(
        r#"{{"version": 1, "entries": [{{"method": "GET", "url": "{url}", "status": 200,
        "headers": [["content-type", "video/mp4"]], "body": "files/{body_name}"}}]}}"#
    );
    fs::write(dir.join("manifest.json"), manifest).unwrap();
}

#[test]
fn replay_answers_range_requests_from_a_whole_recording() {
    let dir = TempDir::new();
    fixture_with(dir.path(), "https://media.test/v.mp4", "v.mp4");
    fs::write(dir.path().join("files/v.mp4"), pattern(10_000)).unwrap();
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    let url = Url::parse("https://media.test/v.mp4").unwrap();
    let ask = |range: Option<&str>| {
        let mut r = request(url.clone());
        if let Some(range) = range {
            r.headers.append("range", range);
        }
        replay.fetch(&r).unwrap()
    };
    let whole = ask(None);
    assert_eq!((whole.status, whole.body.len()), (200, 10_000));
    assert!(!whole.headers.contains("content-range"));
    let part = ask(Some("bytes=100-199"));
    assert_eq!(part.status, 206);
    assert_eq!(
        part.headers.get("content-range"),
        Some("bytes 100-199/10000")
    );
    assert_eq!(part.headers.get("accept-ranges"), Some("bytes"));
    assert_eq!(part.body, pattern(10_000)[100..200]);
    let open = ask(Some("bytes=9990-"));
    assert_eq!(
        open.headers.get("content-range"),
        Some("bytes 9990-9999/10000")
    );
    assert_eq!(open.body.len(), 10);
    let suffix = ask(Some("bytes=-5"));
    assert_eq!(suffix.body, pattern(10_000)[9995..]);
    let outside = ask(Some("bytes=10000-"));
    assert_eq!(outside.status, 416);
    assert_eq!(outside.headers.get("content-range"), Some("bytes */10000"));
    assert_eq!(outside.body.len(), 0);
    // A range that is not valid or not a single one is ignored: 200.
    for range in ["bytes=5-2", "bytes=0-1,5-6", "items=0-1"] {
        assert_eq!(ask(Some(range)).status, 200, "{range}");
    }
    // The stream is the same as the buffered body.
    let mut r = request(url);
    r.headers.append("range", "bytes=10-19");
    let streamed = replay.fetch_stream(&r).unwrap();
    assert_eq!(streamed.head.status, 206);
    assert_eq!(streamed.head.body.len(), 0);
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut { streamed.body }, &mut bytes).unwrap();
    assert_eq!(bytes, pattern(10_000)[10..20]);
}

#[test]
fn a_large_replay_body_is_read_at_an_offset_not_loaded() {
    let dir = TempDir::new();
    fixture_with(dir.path(), "https://media.test/big.mp4", "big.mp4");
    // Over the limit for bodies that stay in memory.
    let big = pattern(9 * MIB);
    fs::write(dir.path().join("files/big.mp4"), &big).unwrap();
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    let loader = loader_for(Arc::new(replay));
    let url = Url::parse("https://media.test/big.mp4").unwrap();
    let opts = options(4 * MIB, MIB, ReadAhead::Bytes(MIB as u64));
    let mut resource = loader.open_media_with(request(url), opts);
    assert_eq!(resource.size(), Some(9 * MIB as u64));
    assert_eq!(read_exact(&mut resource, 100), big[..100]);
    resource.seek(8 * MIB as u64 + 7).unwrap();
    assert_eq!(read_to_end(&mut resource), big[8 * MIB + 7..]);
    assert!(resource.handle().stats().peak_cached_bytes <= 4 * MIB);
}

/// The 4K test file (161 MB), when the checkout has it (`fixtures/js` is a
/// link to the main checkout's files).
fn real_file() -> Option<std::path::PathBuf> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/js/media/mux-highest.mp4");
    path.exists().then_some(path)
}

/// Reads `resource` to its end and compares it with `path`; returns the
/// number of bytes.
fn compare_with_file(resource: &mut MediaResource, path: &Path) -> u64 {
    let mut file = File::open(path).unwrap();
    let mut total = 0u64;
    let mut mine = vec![0u8; MIB];
    let mut theirs = vec![0u8; MIB];
    loop {
        let n = resource.read(&mut mine).unwrap();
        if n == 0 {
            return total;
        }
        io::Read::read_exact(&mut file, &mut theirs[..n]).unwrap();
        assert!(mine[..n] == theirs[..n], "the bytes differ at {total}");
        total += n as u64;
    }
}

#[test]
fn the_4k_file_is_never_held_whole_over_the_local_server() {
    let Some(path) = real_file() else {
        eprintln!("skipped: fixtures/js/media/mux-highest.mp4 is not there");
        return;
    };
    let server = TestServer::start(vec![("/v.mp4", Route::file(&path))]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    let handle = resource.handle();
    let size = resource.size().unwrap();
    assert_eq!(size, fs::metadata(&path).unwrap().len());
    handle.set_duration(Some(35.968));
    handle.set_read_ahead(ReadAhead::AUTO);
    assert_eq!(compare_with_file(&mut resource, &path), size);
    let stats = handle.stats();
    eprintln!("4K over the server: {stats:?}");
    assert!(stats.peak_cached_bytes <= ELEMENT_CACHE_BYTES, "{stats:?}");
    assert!(loader.media_budget().used() <= ELEMENT_CACHE_BYTES);
}

#[test]
fn the_4k_file_is_never_held_whole_through_the_replay() {
    let Some(path) = real_file() else {
        eprintln!("skipped: fixtures/js/media/mux-highest.mp4 is not there");
        return;
    };
    let dir = TempDir::new();
    fixture_with(dir.path(), "https://media.test/4k.mp4", "4k.mp4");
    // A link, not a copy.
    symlink(
        fs::canonicalize(&path).unwrap(),
        dir.path().join("files/4k.mp4"),
    )
    .unwrap();
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    let loader = loader_for(Arc::new(replay));
    let url = Url::parse("https://media.test/4k.mp4").unwrap();
    let mut resource = loader.open_media(request(url));
    let handle = resource.handle();
    handle.set_duration(Some(35.968));
    let size = resource.size().unwrap();
    assert_eq!(compare_with_file(&mut resource, &path), size);
    let stats = handle.stats();
    eprintln!("4K through the replay: {stats:?}");
    assert!(stats.peak_cached_bytes <= ELEMENT_CACHE_BYTES, "{stats:?}");
    // A seek to 60 %: one request at the block of the target.
    let target = size * 6 / 10;
    resource.seek(target).unwrap();
    let buf = [0u8; 100];
    let mut file = File::open(&path).unwrap();
    io::Seek::seek(&mut file, io::SeekFrom::Start(target)).unwrap();
    let mut expected = [0u8; 100];
    io::Read::read_exact(&mut file, &mut expected).unwrap();
    assert_eq!(read_exact(&mut resource, buf.len()), expected);
}

// ---- Fix round of the review of feature 4 ----

/// Runs `f` on a thread and returns its result, or `None` when it does not
/// end within `limit` (the thread stays blocked: a failing test only).
fn within<T: Send + 'static>(limit: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(limit).ok()
}

#[test]
fn resume_after_a_cancel_while_opening_opens_the_resource_again() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(1000)).holding_headers(Duration::from_millis(300)),
    )]);
    let loader = loader();
    let resource = loader.open_media(request(server.url("/v.mp4")));
    let handle = resource.handle();
    wait_for("the request arrived", || !server.requests().is_empty());
    thread::sleep(Duration::from_millis(50));
    handle.cancel();
    // The first request ends (the server answers) and finds itself stale.
    wait_for("the first request ended", || server.live_connections() == 0);
    handle.resume();
    let mut reader = handle.reader();
    let read = within(Duration::from_secs(10), move || {
        let mut buf = [0u8; 10];
        reader.read(&mut buf).map(|n| (n, buf))
    })
    .expect("the read must not block forever");
    let (n, buf) = read.unwrap();
    assert!(n > 0 && is_pattern(0, &buf[..n]));
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn dropping_the_last_owner_frees_the_cache_while_a_job_waits_in_the_network() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::new(Body::Pattern(64 * MIB as u64)).stalling_after(3 * MIB as u64),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    let handle = resource.handle();
    read_exact(&mut resource, 10);
    wait_for("cached", || handle.stats().cached_bytes >= 3 * MIB);
    let budget = Arc::clone(loader.media_budget());
    assert!(budget.used() >= 3 * MIB);
    // A cancel keeps the cache for a resume.
    handle.cancel();
    assert!(budget.used() >= 3 * MIB);
    drop(resource);
    drop(handle);
    // The job thread still waits for the stalled server (it holds the
    // cache), but the budget has its bytes back.
    assert_eq!(budget.used(), 0);
    assert_eq!(server.live_connections(), 1);
}

#[test]
fn a_stream_that_idles_longer_than_the_request_limit_reads_on() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(3_000_000)).without_ranges(),
    )]);
    // A request limit of 400 ms; the reader pauses for 1.5 s.
    let fetcher = Arc::new(NetworkFetcher::without_proxy_with_timeout(
        Duration::from_millis(400),
    ));
    let loader = loader_for(fetcher);
    let opts = options(8 * MIB, 2 * MIB, ReadAhead::Bytes(MIB as u64));
    let mut resource = loader.open_media_with(request(server.url("/v.mp4")), opts);
    assert!(is_pattern(0, &read_exact(&mut resource, 10)));
    thread::sleep(Duration::from_millis(1500));
    let rest = read_to_end(&mut resource);
    assert_eq!(rest.len(), 3_000_000 - 10);
    assert!(is_pattern(10, &rest));
    assert!(resource.handle().error().is_none());
}

#[test]
fn the_body_of_a_later_range_has_the_request_limit_but_a_body_from_the_start_has_none() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(100_000)).stalling_after(1000),
    )]);
    let fetcher = NetworkFetcher::without_proxy_with_timeout(Duration::from_millis(300));
    let open = |range: &str| {
        let mut r = request(server.url("/v.mp4"));
        r.headers.set("range", range);
        let mut body = fetcher.fetch_stream(&r).unwrap().body;
        let mut first = [0u8; 1000];
        io::Read::read_exact(&mut body, &mut first).unwrap();
        body
    };
    // A later range: the body ends with an error after the limit.
    let mut later = open("bytes=10-");
    let ended = within(Duration::from_secs(10), move || {
        io::Read::read(&mut later, &mut [0u8; 10])
    })
    .expect("the limit ends the read");
    assert!(ended.is_err(), "{ended:?}");
    // From the start: the read still waits after the limit.
    let mut start = open("bytes=0-");
    let waiting = within(Duration::from_millis(1200), move || {
        io::Read::read(&mut start, &mut [0u8; 10])
    });
    assert!(waiting.is_none(), "{waiting:?}");
}

/// A seekable resource over a lying server: opens, reads the start, then a
/// far seek reopens and meets the lie.
fn lying_server_fails_the_reopen(lie: Lie) {
    let server = TestServer::start(vec![("/v.mp4", Route::bytes(pattern(3 * MIB)).lying(lie))]);
    let loader = loader();
    let opts = options(8 * MIB, 2 * MIB, ReadAhead::Bytes(MIB as u64));
    let mut resource = loader.open_media_with(request(server.url("/v.mp4")), opts);
    let handle = resource.handle();
    assert!(is_pattern(0, &read_exact(&mut resource, 1000)));
    wait_for("the first block range is cached", || {
        handle.stats().cached_bytes >= MIB
    });
    resource.seek(5 * MIB as u64 / 2).unwrap();
    let error = resource.read(&mut [0; 10]).unwrap_err();
    assert!(
        error.to_string().contains("range") || error.to_string().contains("Range"),
        "{error}"
    );
    assert!(
        matches!(handle.error().as_deref(), Some(NetError::Range(_))),
        "{:?}",
        handle.error()
    );
    // The blocks that were read stay readable, through another reader.
    let mut again = handle.reader();
    assert!(is_pattern(0, &read_exact(&mut again, 1000)));
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn a_206_that_starts_at_another_byte_than_requested_fails_the_resource() {
    lying_server_fails_the_reopen(Lie::ShiftedStart);
}

#[test]
fn a_total_that_changes_between_responses_fails_the_resource() {
    lying_server_fails_the_reopen(Lie::ChangingTotal);
}

#[test]
fn a_200_to_a_request_for_a_later_range_fails_the_resource() {
    lying_server_fails_the_reopen(Lie::WholeForLaterRanges);
}

#[test]
fn a_resource_below_the_floor_progresses_while_others_hold_the_page_budget() {
    let server = TestServer::start(vec![
        ("/big.mp4", Route::new(Body::Pattern(200 * MIB as u64))),
        ("/small.mp4", Route::new(Body::Pattern(5 * MIB as u64))),
    ]);
    let fetcher: Arc<dyn Fetcher> = Arc::new(NetworkFetcher::without_proxy());
    let pool = MediaPool::new();
    let budget = ByteBudget::new(4 * MIB);
    let opts = options(64 * MIB, 0, ReadAhead::Bytes(16 * MIB as u64));
    let open_one = |path: &str| open(request(server.url(path)), &fetcher, &pool, &budget, opts);
    let mut first = open_one("/big.mp4");
    let mut second = open_one("/big.mp4");
    read_exact(&mut first, 10);
    read_exact(&mut second, 10);
    wait_for("the budget is full", || budget.used() >= 4 * MIB);
    // Both big resources have their cache and wait: nothing is left.
    let mut third = open_one("/small.mp4");
    let read = within(Duration::from_secs(20), move || {
        let data = read_to_end(&mut third);
        (data.len(), is_pattern(0, &data))
    })
    .expect("a resource below the floor must progress");
    assert_eq!(read, (5 * MIB, true));
    drop((first, second));
    wait_for("the budget is free", || budget.used() == 0);
}

#[test]
fn a_retry_gives_back_the_block_that_did_not_complete() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(300_000)).closing_after(100_000, 1),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    let handle = resource.handle();
    assert_eq!(read_to_end(&mut resource), pattern(300_000));
    assert_eq!(server.requests().len(), 2);
    wait_for("reserved equals the cached bytes", || {
        handle.stats().cached_bytes == 300_000
    });
}

#[test]
fn a_stream_failure_gives_back_the_block_that_did_not_complete() {
    // A body that ends before its `Content-Length`; no `Range`: a stream.
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::bytes(pattern(300_000))
            .without_ranges()
            .closing_after(100_000, 1),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    let handle = resource.handle();
    let mut buf = vec![0u8; 40_000];
    let mut got = 0;
    let error = loop {
        match resource.read(&mut buf) {
            Ok(0) => panic!("the stream ended without an error at {got}"),
            Ok(n) => got += n,
            Err(error) => break error,
        }
    };
    assert_eq!(got, 65_536, "{error}");
    assert!(handle.error().is_some());
    wait_for("only the complete block is reserved", || {
        handle.stats().cached_bytes == 65_536
    });
}

#[test]
fn a_stale_job_gives_back_its_block_when_its_read_ends() {
    let server = TestServer::start(vec![(
        "/v.mp4",
        Route::new(Body::Pattern(64 * MIB as u64)).stalling_after(100_000),
    )]);
    let loader = loader();
    let mut resource = loader.open_media(request(server.url("/v.mp4")));
    let handle = resource.handle();
    read_exact(&mut resource, 10);
    // One complete block, and the job waits inside the second.
    wait_for("the second block is reserved", || {
        handle.stats().cached_bytes == 2 * 65_536
    });
    handle.cancel();
    // The server stops: the stale job's read ends and it gives its block
    // back (the resource and its cache are still there).
    drop(server);
    wait_for("the stale job's block is back", || {
        handle.stats().cached_bytes == 65_536
    });
}

#[test]
fn replay_drops_the_stored_length_and_range_headers_of_a_partial_answer() {
    let dir = TempDir::new();
    fs::create_dir_all(dir.path().join("files")).unwrap();
    fs::write(dir.path().join("files/v.mp4"), pattern(10_000)).unwrap();
    let manifest = r#"{"version": 1, "entries": [{"method": "GET",
        "url": "https://media.test/v.mp4", "status": 200,
        "headers": [["content-type", "video/mp4"], ["content-length", "10000"],
                    ["accept-ranges", "none"], ["content-range", "bytes 0-1/2"]],
        "body": "files/v.mp4"}]}"#;
    fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    let replay = ReplayFetcher::load(dir.path()).unwrap();
    let url = Url::parse("https://media.test/v.mp4").unwrap();
    let ask = |range: &str| {
        let mut r = request(url.clone());
        r.headers.append("range", range);
        replay.fetch(&r).unwrap()
    };
    let part = ask("bytes=100-199");
    assert_eq!(part.status, 206);
    assert!(!part.headers.contains("content-length"));
    assert_eq!(part.headers.get_all("accept-ranges").count(), 1);
    assert_eq!(part.headers.get("accept-ranges"), Some("bytes"));
    assert_eq!(part.headers.get_all("content-range").count(), 1);
    assert_eq!(
        part.headers.get("content-range"),
        Some("bytes 100-199/10000")
    );
    let outside = ask("bytes=10000-");
    assert_eq!(outside.status, 416);
    assert!(!outside.headers.contains("content-length"));
    assert_eq!(outside.headers.get_all("content-range").count(), 1);
    assert_eq!(outside.headers.get("content-range"), Some("bytes */10000"));
    assert_eq!(outside.headers.get_all("accept-ranges").count(), 1);
    // A range that is ignored (200): the whole body keeps its length, and
    // `Accept-Ranges` is ours.
    let whole = ask("bytes=5-2");
    assert_eq!(whole.status, 200);
    assert_eq!(whole.headers.get("content-length"), Some("10000"));
    assert_eq!(whole.headers.get_all("accept-ranges").count(), 1);
    assert_eq!(whole.headers.get("accept-ranges"), Some("bytes"));
}
