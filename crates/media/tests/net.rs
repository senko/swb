//! The network byte source under the demuxer (M14 feature 4, ADR 0047
//! section 5): a test file served by the local test server of `swb-net`
//! opens, seeks and decodes through `NetSource`; a server without `Range`
//! gives a stream that cannot seek; cancelling ends a blocked demuxer.
#![cfg(feature = "ffmpeg")]

mod common;

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use common::{decode_all, fixture, fixture_bytes, open};
use swb_media::{Decoded, MediaError, MediaFile, NetSource};
use swb_net::test_server::{Route, TestServer};
use swb_net::{Destination, Loader, NetworkFetcher, ReadAhead, Request};

fn loader() -> Loader {
    Loader::new(Arc::new(NetworkFetcher::new()), 1, Arc::new(|| {}))
}

/// Opens `name` from `server` through the loader.
fn open_net(
    loader: &Loader,
    server: &TestServer,
    name: &str,
) -> (Result<MediaFile, MediaError>, swb_net::MediaHandle) {
    let resource = loader.open_media(Request::get(
        server.url(&format!("/{name}")),
        Destination::Video,
    ));
    let handle = resource.handle();
    (MediaFile::open(Box::new(NetSource::new(resource))), handle)
}

/// The presentation times of the video frames that `file` decodes next.
fn video_times(file: &mut MediaFile, count: usize) -> Vec<Option<i64>> {
    let mut times = Vec::new();
    while times.len() < count {
        match file.decode_next().expect("decodes") {
            Some(Decoded::Video(frame)) => times.push(frame.pts_us),
            Some(Decoded::Audio(_)) => {}
            None => break,
        }
    }
    times
}

#[test]
fn a_file_over_range_requests_opens_seeks_and_decodes_like_the_file() {
    // The index (`moov`) is at the end: the demuxer seeks to it.
    let name = "mp4-h264-high-aac-moov-at-end.mp4";
    let server = TestServer::start(vec![(
        "/mp4-h264-high-aac-moov-at-end.mp4",
        Route::file(fixture(name)),
    )]);
    let loader = loader();
    let (opened, handle) = open_net(&loader, &server, name);
    let mut net = opened.expect("opens over the server");
    let mut local = open(name).expect("opens from the file");
    assert!(net.info().seekable);
    assert_eq!(net.info().duration_us, local.info().duration_us);
    assert_eq!(net.info().video, local.info().video);
    let totals = decode_all(&mut net).expect("decodes to the end");
    let expected = decode_all(&mut local).expect("decodes to the end");
    assert_eq!(totals.video_frames, expected.video_frames);
    assert_eq!(totals.audio_frames, expected.audio_frames);
    // A seek to the middle gives the frames that the file gives.
    let target = net.info().duration_us.expect("a duration") / 2;
    net.seek(target).expect("seeks");
    local.seek(target).expect("seeks");
    assert_eq!(video_times(&mut net, 5), video_times(&mut local, 5));
    assert!(handle.stats().seekable);
    assert!(handle.stats().requests >= 1);
    // The first request was `bytes=0-`.
    assert_eq!(server.requests()[0].range.as_deref(), Some("bytes=0-"));
}

#[test]
fn a_seek_back_to_the_start_after_the_end_decodes_again() {
    let name = "mp4-h264-high-aac.mp4";
    let server = TestServer::start(vec![("/mp4-h264-high-aac.mp4", Route::file(fixture(name)))]);
    let loader = loader();
    let (opened, handle) = open_net(&loader, &server, name);
    let mut net = opened.expect("opens");
    // The least read-ahead (1 MiB) is more than this file: no request ends
    // early.
    handle.set_read_ahead(ReadAhead::Bytes(1));
    let expected = decode_all(&mut open(name).expect("opens")).expect("decodes");
    let totals = decode_all(&mut net).expect("decodes");
    assert_eq!(totals.video_frames, expected.video_frames);
    // Back to the start after the end.
    net.seek(0).expect("seeks");
    assert_eq!(video_times(&mut net, 1).len(), 1);
}

#[test]
fn a_server_without_range_gives_a_stream_that_cannot_seek() {
    // This file has its index at the start, so it plays as a stream.
    let name = "mp4-h264-high-aac.mp4";
    let server = TestServer::start(vec![(
        "/mp4-h264-high-aac.mp4",
        Route::bytes(fixture_bytes(name)).without_ranges(),
    )]);
    let loader = loader();
    let (opened, handle) = open_net(&loader, &server, name);
    let mut net = opened.expect("a stream opens");
    assert!(!net.info().seekable);
    assert!(!handle.stats().seekable);
    assert_eq!(handle.stats().size, None);
    let expected = decode_all(&mut open(name).expect("opens")).expect("decodes");
    let totals = decode_all(&mut net).expect("decodes");
    assert_eq!(totals.video_frames, expected.video_frames);
    assert!(net.seek(1_000_000).is_err(), "a stream cannot seek");
}

#[test]
fn the_4k_file_opens_and_seeks_over_the_server_with_a_bounded_cache() {
    let path = common::repo_path("fixtures/js/media/mux-highest.mp4");
    if !path.exists() {
        eprintln!("skipped: fixtures/js/media/mux-highest.mp4 is not there");
        return;
    }
    let server = TestServer::start(vec![("/4k.mp4", Route::file(&path))]);
    let loader = loader();
    let (opened, handle) = open_net(&loader, &server, "4k.mp4");
    let mut net = opened.expect("opens");
    let video = net.info().video.clone().expect("a video track");
    assert_eq!(video.coded_size.0, 3840);
    assert!(net.info().seekable);
    let duration = net.info().duration_us.expect("a duration");
    handle.set_duration(Some(duration as f64 / 1e6));
    // A seek to 60 % decodes frames near the target.
    let target = duration * 6 / 10;
    net.seek(target).expect("seeks");
    let times = video_times(&mut net, 2);
    assert_eq!(times.len(), 2);
    let stats = handle.stats();
    eprintln!(
        "4K open and seek: {stats:?}, requests: {}, sent: {}",
        server.requests().len(),
        server.bytes_sent()
    );
    assert!(stats.peak_cached_bytes <= swb_net::media::ELEMENT_CACHE_BYTES);
}

#[test]
fn cancel_ends_a_demuxer_that_waits_for_a_stalled_server() {
    let name = "mp4-h264-high-aac.mp4";
    // 50,000 bytes of the file arrive, then nothing.
    let server = TestServer::start(vec![(
        "/mp4-h264-high-aac.mp4",
        Route::file(fixture(name)).stalling_after(50_000),
    )]);
    let loader = loader();
    let resource = loader.open_media(Request::get(
        server.url("/mp4-h264-high-aac.mp4"),
        Destination::Video,
    ));
    let handle = resource.handle();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        handle.cancel();
        Instant::now()
    });
    let started = Instant::now();
    let result = MediaFile::open(Box::new(NetSource::new(resource)));
    let ended = Instant::now();
    let cancelled_at = canceller.join().expect("the thread ends");
    assert!(
        matches!(result, Err(MediaError::Source(_))),
        "{:?}",
        result.err()
    );
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(ended.duration_since(cancelled_at) < Duration::from_secs(1));
}
