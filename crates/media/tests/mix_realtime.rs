//! The mixer's callback does not allocate, free or reallocate (ADR 0047
//! section 8): a test binary with a counting global allocator
//! (`swb_media_ffi::counting`) that counts the calls made on the thread that
//! runs the callback, while the owner (the test's main thread) adds sources,
//! changes their volume, fades them out and frees them, and a producer
//! thread fills the rings. A system call is not counted here (the module
//! comment of `output/mix.rs` has the argument: `try_lock` and atomics only).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use swb_media::SampleRing;
use swb_media::output::fake::FakeDevice;
use swb_media::output::{MAX_SOURCES, ManualClock, MonoClock, Output};
use swb_media_ffi::counting::{self, CountingAllocator};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn the_callback_makes_no_allocator_call() {
    let device = FakeDevice::new();
    let clock = Arc::new(ManualClock::default());
    let output = Output::manual(
        Box::new(device.clone()),
        Arc::clone(&clock) as Arc<dyn MonoClock>,
    );
    output.request(None, true);
    assert!(output.is_ready());

    let stop = Arc::new(AtomicBool::new(false));
    let rings: Vec<Arc<SampleRing>> = (0..MAX_SOURCES)
        .map(|_| Arc::new(SampleRing::new(4096)))
        .collect();

    // The callback thread: counted from the start of its loop.
    let runner = {
        let output = Arc::clone(&output);
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut out = vec![0.0f32; 1024 * 2];
            counting::track_thread(true);
            let before = counting::calls();
            let mut cycles = 0u64;
            while !stop.load(Ordering::Relaxed) {
                output.mix().fill(&mut out, 512);
                cycles += 1;
                std::thread::yield_now();
            }
            let calls = counting::calls() - before;
            counting::track_thread(false);
            (calls, cycles)
        })
    };
    // A producer that keeps the rings partly full (starved at times).
    let producer = {
        let rings = rings.clone();
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let samples = vec![0.2f32; 256 * 2];
            while !stop.load(Ordering::Relaxed) {
                for ring in &rings {
                    ring.push(&samples);
                }
                std::thread::yield_now();
            }
        })
    };
    // The owner: sources join, change their volume, fade out, are freed.
    for round in 0..300 {
        let mut sources = Vec::new();
        for (i, ring) in rings.iter().enumerate() {
            let lead = i64::try_from(i % 5).unwrap_or(0) * 100 - 100;
            if let Some(source) = output.attach(Arc::clone(ring), lead) {
                sources.push(source);
            }
            ring.set_gain(if (round + i) % 3 == 0 { 0.0 } else { 0.7 });
        }
        std::thread::yield_now();
        drop(sources);
        // Frees what has faded (the callback fades it in 10 ms of frames).
        let started = std::time::Instant::now();
        loop {
            output.tick();
            if output.stats().retired == 0 || started.elapsed().as_millis() > 100 {
                break;
            }
            std::thread::yield_now();
        }
    }
    stop.store(true, Ordering::Relaxed);
    let (calls, cycles) = runner.join().expect("the callback thread ends");
    producer.join().expect("the producer ends");
    assert!(cycles > 100, "the callback ran: {cycles}");
    assert_eq!(calls, 0, "allocator calls on the callback thread");
}

/// The counter counts: a tracked thread that allocates and frees is seen.
/// (Without this the test above could pass with a broken hook.)
#[test]
fn the_counter_sees_the_calls_of_a_tracked_thread() {
    let counted = std::thread::spawn(|| {
        counting::track_thread(true);
        let before = counting::calls();
        let v = vec![1u8; 4096];
        std::hint::black_box(&v);
        drop(v);
        let calls = counting::calls() - before;
        counting::track_thread(false);
        calls
    })
    .join()
    .expect("the thread ends");
    assert!(counted >= 2, "{counted}");
}
