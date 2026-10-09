//! Measurements of the heap for the spike report (ADR 0026,
//! "Consequences"): the collection pause for one million small objects,
//! allocation speed, the cost of a handle access with the generation
//! check, the cost of recording a handle in a handle scope, and the
//! memory per small object.
//!
//! Run with `cargo run --release -p swb-js --example heap_bench`.

#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::{Duration, Instant};

use swb_js::{Gc, GetResult, Heap, HeapConfig, NoRoots, Object, PropertyKey, Value};

const COUNT: u32 = 1_000_000;

/// Passes over all objects in the access measurements.
const ROUNDS: u64 = 20;

/// Handles recorded in the handle scope measurement.
const RECORDS: u64 = 10_000_000;

/// The resident set size of the process in bytes (Linux), or 0.
fn rss() -> usize {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|text| text.split_whitespace().nth(1)?.parse::<usize>().ok())
        .map_or(0, |pages| pages * 4096)
}

fn per(duration: Duration, count: u64) -> f64 {
    duration.as_nanos() as f64 / count as f64
}

struct Keys {
    a: PropertyKey,
    b: PropertyKey,
    c: PropertyKey,
    text: Value,
}

fn keys(heap: &mut Heap) -> Keys {
    let text = heap.intern_str("constant").unwrap().into();
    Keys {
        a: heap.key_from_str("a").unwrap(),
        b: heap.key_from_str("b").unwrap(),
        c: heap.key_from_str("c").unwrap(),
        text,
    }
}

/// A small object `{a: i, b: i + 0.5, c: "constant"}`.
fn small_object(heap: &mut Heap, keys: &Keys, i: u32, roots: &Vec<Value>) -> Gc<Object> {
    let object = heap.new_object(None).unwrap();
    heap.create_data_property(object, keys.a, Value::Int(i as i32), roots)
        .unwrap();
    heap.create_data_property(object, keys.b, Value::Double(f64::from(i) + 0.5), roots)
        .unwrap();
    heap.create_data_property(object, keys.c, keys.text, roots)
        .unwrap();
    object
}

/// Builds `COUNT` small objects; every `keep`-th one goes into a rooted
/// array. Returns the heap, the root list (the array) and the objects.
fn build(keep: u32) -> (Heap, Vec<Value>, Vec<Gc<Object>>) {
    let mut heap = Heap::new(HeapConfig::default());
    let keys = keys(&mut heap);
    let array = heap.new_array(None, 0).unwrap();
    let roots = vec![array.into(), keys.text];
    let mut objects = Vec::with_capacity(COUNT as usize);
    let mut kept = 0;
    heap.enter_no_gc();
    for i in 0..COUNT {
        let object = small_object(&mut heap, &keys, i, &roots);
        if i % keep == 0 {
            let index = PropertyKey::Index(kept);
            heap.create_data_property(array, index, object.into(), &roots)
                .unwrap();
            kept += 1;
        }
        objects.push(object);
    }
    heap.leave_no_gc();
    (heap, roots, objects)
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

fn gc_pause() {
    for (label, keep) in [("all reachable", 1), ("half reachable", 2)] {
        let (mut heap, roots, _) = build(keep);
        let before = heap.heap_size();
        let first = heap.collect(&roots);
        let mut pauses = vec![first.last_pause];
        for _ in 0..4 {
            pauses.push(heap.collect(&roots).last_pause);
        }
        println!(
            "GC pause, 1M objects x 3 properties, {label}: first {:.1} ms (freed {}), median of 5 {:.1} ms; heap {:.1} MB -> {:.1} MB",
            first.last_pause.as_secs_f64() * 1e3,
            first.last_freed,
            median(pauses).as_secs_f64() * 1e3,
            before as f64 / 1e6,
            heap.heap_size() as f64 / 1e6,
        );
    }
}

fn allocation() {
    // Garbage: a safepoint after each object, as the interpreter would
    // have between instructions; collections run at the threshold.
    let mut heap = Heap::new(HeapConfig::default());
    let keys = keys(&mut heap);
    let roots = vec![keys.text];
    let start = Instant::now();
    for i in 0..COUNT {
        black_box(small_object(&mut heap, &keys, i, &roots));
        heap.safepoint(&roots).unwrap();
    }
    let elapsed = start.elapsed();
    println!(
        "Allocation, {{a, b, c}} objects that die young: {:.0} ns per object ({} collections)",
        per(elapsed, u64::from(COUNT)),
        heap.stats().collections
    );
    // Retained: all objects stay alive; collections only at the end.
    let start = Instant::now();
    let (heap, _, _) = build(1);
    let elapsed = start.elapsed();
    println!(
        "Allocation, {{a, b, c}} objects kept in an array: {:.0} ns per object",
        per(elapsed, u64::from(COUNT))
    );
    drop(heap);
    let start = Instant::now();
    let mut heap = Heap::new(HeapConfig::default());
    heap.enter_no_gc();
    for _ in 0..COUNT {
        black_box(heap.new_object(None).unwrap());
    }
    println!(
        "Allocation, empty objects: {:.0} ns per object",
        per(start.elapsed(), u64::from(COUNT))
    );
}

fn access() {
    let (mut heap, roots, objects) = build(1);
    heap.collect(&roots);
    let keys = keys(&mut heap);
    let start = Instant::now();
    let mut sum = 0u64;
    for _ in 0..ROUNDS {
        for &object in &objects {
            sum += u64::from(heap.object(black_box(object)).unwrap().shape().index());
        }
    }
    black_box(sum);
    let checked = start.elapsed();
    // Baseline: the same loop over a plain vector, with a bounds check.
    let len = objects
        .iter()
        .map(|o| o.index() as usize + 1)
        .max()
        .unwrap_or(0);
    let mut plain = vec![0u32; len];
    for object in &objects {
        plain[object.index() as usize] = object.generation();
    }
    let start = Instant::now();
    let mut sum = 0u64;
    for _ in 0..ROUNDS {
        for &object in &objects {
            sum += u64::from(plain[black_box(object).index() as usize]);
        }
    }
    black_box(sum);
    let baseline = start.elapsed();
    let accesses = ROUNDS * u64::from(COUNT);
    println!(
        "Handle access with generation check, 1M objects in order: {:.2} ns per access (plain vector of u32 by the same index: {:.2} ns)",
        per(checked, accesses),
        per(baseline, accesses)
    );
    // A cache-resident working set: 1000 objects, many rounds.
    let few = &objects[..1000];
    let rounds = 20_000u64;
    let start = Instant::now();
    let mut sum = 0u64;
    for _ in 0..rounds {
        for &object in few {
            sum += u64::from(heap.object(black_box(object)).unwrap().shape().index());
        }
    }
    black_box(sum);
    println!(
        "Handle access with generation check, 1000 objects (in cache): {:.2} ns per access",
        per(start.elapsed(), rounds * 1000)
    );
    // A property read through [[Get]] (own data property, shared shape).
    let start = Instant::now();
    let mut sum = 0i64;
    for _ in 0..ROUNDS {
        for &object in &objects {
            if let GetResult::Value(Value::Int(a)) =
                heap.get(object, keys.a, object.into()).unwrap()
            {
                sum += i64::from(a);
            }
        }
    }
    black_box(sum);
    println!(
        "[[Get]] of an own data property: {:.1} ns per read",
        per(start.elapsed(), accesses)
    );
}

fn handle_scopes() {
    let mut heap = Heap::new(HeapConfig::default());
    let value: Value = heap.new_object(None).unwrap().into();
    let start = Instant::now();
    let outer = heap.open_scope();
    for _ in 0..RECORDS / 100 {
        let scope = heap.open_scope();
        for _ in 0..100 {
            heap.record(black_box(value));
        }
        heap.close_scope(scope).unwrap();
    }
    heap.close_scope(outer).unwrap();
    println!(
        "Handle scope: {:.2} ns per recorded handle (scopes of 100, open and close included)",
        per(start.elapsed(), RECORDS)
    );
}

fn memory() {
    let rss_before = rss();
    let (mut heap, roots, objects) = build(1);
    heap.collect(&roots);
    let rss_after = rss();
    let count = objects.len() as f64;
    println!(
        "Memory per {{a, b, c}} object: {:.0} bytes by the heap's accounting, {:.0} bytes RSS (includes the 16-byte array element and this program's handle vector of 8 bytes)",
        heap.heap_size() as f64 / count,
        (rss_after.saturating_sub(rss_before)) as f64 / count,
    );
    drop(heap);
    let _ = NoRoots;
}

fn main() {
    println!("swb-js heap measurements (release build)");
    memory();
    gc_pause();
    allocation();
    access();
    handle_scopes();
}
