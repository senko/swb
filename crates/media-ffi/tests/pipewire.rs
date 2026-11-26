//! The C boundary of the PipeWire output against a real server (ADR 0047
//! sections 4 and 8). **Never run by `cargo test`**: the test is `#[ignore]`d
//! because it connects to a PipeWire server, and the only server that a test
//! or a measurement may use is the private daemon of `swbtools pw-private`
//! (a null sink, no sound card). Run by hand, in the environment that
//! `swbtools pw-private env` prints, with the links made by `watch`:
//!
//! ```text
//! uv --directory tools run swbtools pw-private start
//! uv --directory tools run swbtools pw-private watch --seconds 8 &
//! PIPEWIRE_RUNTIME_DIR=$PWD/tmp/pw-private XDG_RUNTIME_DIR=$PWD/tmp/pw-private \
//!   PIPEWIRE_CONFIG_NAME=$PWD/tmp/pw-private/swb-private-client.conf \
//!   cargo test -p swb-media-ffi --features pipewire --test pipewire -- --ignored
//! uv --directory tools run swbtools pw-private stop
//! ```
//!
//! The test refuses to run when the runtime directory is not a `pw-private`
//! directory, so it cannot reach the owner's daemon by mistake.
#![cfg(feature = "pipewire")]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use swb_media_ffi::pipewire::{AudioStream, StreamState};

/// The runtime directory of the environment, if it is a private daemon's.
fn private_runtime_dir() -> Option<String> {
    let pipewire = std::env::var("PIPEWIRE_RUNTIME_DIR").ok()?;
    let xdg = std::env::var("XDG_RUNTIME_DIR").ok()?;
    (pipewire == xdg && pipewire.ends_with("/tmp/pw-private")).then_some(pipewire)
}

#[test]
#[ignore = "connects to PipeWire: run by hand against the private daemon (see the module comment)"]
fn a_stream_opens_streams_stops_and_closes() {
    assert!(
        private_runtime_dir().is_some(),
        "run in the environment of `swbtools pw-private env` (never the owner's daemon)"
    );
    let frames = Arc::new(AtomicU64::new(0));
    let delays = Arc::new(Mutex::new(Vec::<i64>::new()));
    let states = Arc::new(Mutex::new(Vec::<StreamState>::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let opened = Instant::now();
    let mut stream = {
        let (frames, delays, calls) =
            (Arc::clone(&frames), Arc::clone(&delays), Arc::clone(&calls));
        let states = Arc::clone(&states);
        AudioStream::open(
            1024,
            Box::new(move |out, delay| {
                // A sine of 440 Hz on the left channel, silence on the right.
                let at = frames.load(Ordering::Relaxed);
                for (i, pair) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                    let t = (at + i as u64) as f32 / 48_000.0;
                    pair[0] = 0.25 * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
                    pair[1] = 0.0;
                }
                frames.fetch_add((out.len() / 2) as u64, Ordering::Relaxed);
                // Test code: it may lock and allocate; the product's
                // callback does not.
                if calls.fetch_add(1, Ordering::Relaxed) < 8
                    && let Ok(mut d) = delays.lock()
                {
                    d.push(delay);
                }
            }),
            Box::new(move |state| {
                if let Ok(mut s) = states.lock() {
                    s.push(state);
                }
            }),
        )
        .expect("the private daemon runs")
    };
    println!("open: {:?}", opened.elapsed());
    // Streaming needs the links that `swbtools pw-private watch` makes.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !states
        .lock()
        .expect("lock")
        .contains(&StreamState::Streaming)
    {
        assert!(
            Instant::now() < deadline,
            "no STREAMING in 5 s: states {:?} (is `watch` running?)",
            states.lock().expect("lock")
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("streaming after {:?}", opened.elapsed());
    std::thread::sleep(Duration::from_millis(500));
    let pulled = frames.load(Ordering::Relaxed);
    println!(
        "frames in 0.5 s: {pulled}; delays {:?}",
        delays.lock().expect("lock")
    );
    assert!((20_000..=28_000).contains(&pulled), "{pulled}");
    // Stopped: no callback runs.
    stream.set_active(false);
    std::thread::sleep(Duration::from_millis(200));
    let stopped_at = frames.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        frames.load(Ordering::Relaxed),
        stopped_at,
        "no callback while stopped"
    );
    // And started again.
    stream.set_active(true);
    std::thread::sleep(Duration::from_millis(300));
    assert!(frames.load(Ordering::Relaxed) > stopped_at);
    drop(stream);
    // Closed: no callback runs after the drop.
    let closed_at = frames.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(frames.load(Ordering::Relaxed), closed_at);
}
