//! The heap limit as a bound on memory, the 90 % rule, no-GC regions,
//! handle scopes after early returns, roots of every kind, and object
//! kind payloads (spike session 3, fix round).

#![allow(clippy::unwrap_used)]

use swb_js::{
    BigInt, Error, Gc, Generic, GenericData, Heap, HeapConfig, NoRoots, PropertyKey, RootSource,
    Termination, Tracer, Value,
};

/// The roots of a test VM: a value stack.
struct Stack(Vec<Value>);

impl RootSource for Stack {
    fn trace_roots(&self, tracer: &mut Tracer<'_>) {
        for value in &self.0 {
            tracer.value(*value);
        }
    }
}

/// Allocates with `make` until the heap size passes `target`, with a
/// safepoint after each allocation; keeps everything alive in `stack`.
fn fill(
    heap: &mut Heap,
    stack: &mut Stack,
    target: usize,
    mut make: impl FnMut(&mut Heap) -> Value,
) {
    while heap.heap_size() < target {
        let value = make(heap);
        stack.0.push(value);
        heap.safepoint(&*stack).unwrap();
    }
}

fn resident_bytes() -> usize {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    let pages: usize = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * 4096
}

/// The three phases of the review: objects, strings, symbols; everything
/// is freed between the phases.
fn three_phases(limit: usize) -> usize {
    let mut heap = Heap::new(HeapConfig {
        limit,
        ..HeapConfig::default()
    });
    let mut stack = Stack(Vec::new());
    let target = limit / 2;
    for phase in 0..3 {
        fill(&mut heap, &mut stack, target, |heap| match phase {
            0 => heap.new_object(None).unwrap().into(),
            1 => heap.alloc_str(&"x".repeat(100)).unwrap().into(),
            _ => heap.alloc_symbol(None).unwrap().into(),
        });
        stack.0.clear();
        heap.collect(&stack);
        // The arenas gave back their free slots: what is left is small.
        assert!(
            heap.heap_size() < limit / 100,
            "phase {phase}: {} bytes after the phase",
            heap.heap_size()
        );
    }
    heap.heap_size()
}

#[test]
fn freed_phases_leave_a_small_accounted_size() {
    three_phases(64 << 20);
}

/// Run with `cargo test -p swb-js --test limits -- --ignored`: the RSS
/// check needs a process of its own (no other test running).
#[test]
#[ignore = "measures the resident set of the whole process"]
fn resident_memory_stays_near_the_limit() {
    let limit = 128 << 20;
    let before = resident_bytes();
    three_phases(limit);
    let growth = resident_bytes().saturating_sub(before);
    eprintln!(
        "RSS growth {} MiB (limit {} MiB)",
        growth >> 20,
        limit >> 20
    );
    assert!(growth < limit + limit / 4, "RSS grew by {growth} bytes");
}

#[test]
fn collection_above_ninety_percent_ends_the_script() {
    let limit = 8 << 20;
    let config = HeapConfig {
        limit,
        min_threshold: 1 << 20,
        ..HeapConfig::default()
    };
    // Live data at 95 % of the limit.
    let mut heap = Heap::new(config);
    let mut stack = Stack(Vec::new());
    while heap.heap_size() < limit / 100 * 95 {
        stack
            .0
            .push(heap.alloc_str(&"x".repeat(100)).unwrap().into());
    }
    let before = heap.stats().collections;
    assert_eq!(
        heap.safepoint(&stack),
        Err(Error::Terminated(Termination::HeapLimit))
    );
    // One collection, then the script ends; it does not collect again.
    assert_eq!(heap.stats().collections, before + 1);

    // Live data at 50 %: the safepoint collects and the script goes on.
    let mut heap = Heap::new(config);
    let mut stack = Stack(Vec::new());
    while heap.heap_size() < limit / 2 {
        stack
            .0
            .push(heap.alloc_str(&"x".repeat(100)).unwrap().into());
    }
    assert_eq!(heap.safepoint(&stack), Ok(true));
}

#[test]
fn doubled_arena_capacity_does_not_end_the_script() {
    // Small objects up to 75 % of the limit, with a safepoint after each:
    // the object arena's vector doubles on the way, but its reserved
    // capacity above the length is not counted, so no collection ends
    // the script.
    let limit = 8 << 20;
    let config = HeapConfig {
        limit,
        min_threshold: 1 << 20,
        ..HeapConfig::default()
    };
    let mut heap = Heap::new(config);
    let mut stack = Stack(Vec::new());
    fill(&mut heap, &mut stack, limit / 4 * 3, |heap| {
        heap.new_object(None).unwrap().into()
    });
    assert_eq!(heap.safepoint(&stack), Ok(false));
    heap.collect(&stack);
    assert!(heap.heap_size() <= limit / 4 * 3 + (1 << 20));
}

#[test]
fn no_gc_region_still_checks_the_limit() {
    let limit = 4 << 20;
    let mut heap = Heap::new(HeapConfig {
        limit,
        min_threshold: 1 << 20,
        ..HeapConfig::default()
    });
    assert_eq!(heap.no_gc_depth(), 0);
    heap.enter_no_gc();
    heap.enter_no_gc();
    assert_eq!(heap.no_gc_depth(), 2);
    // Garbage over the limit: no collection runs, the safepoint ends the
    // script.
    while heap.heap_size() <= limit {
        heap.alloc_str(&"x".repeat(100)).unwrap();
    }
    let before = heap.stats().collections;
    assert_eq!(
        heap.safepoint(&NoRoots),
        Err(Error::Terminated(Termination::HeapLimit))
    );
    assert_eq!(heap.stats().collections, before);
    // An unwinder repairs a missed `leave_no_gc`.
    heap.restore_no_gc_depth(0);
    assert_eq!(heap.no_gc_depth(), 0);
    assert_eq!(heap.safepoint(&NoRoots), Ok(true));
    assert_eq!(heap.stats().collections, before + 1);
}

/// A native-style function that returns early with `?`, leaving its scope
/// open.
fn native(heap: &mut Heap, fail: bool) -> Result<Value, Error> {
    let scope = heap.open_scope();
    let text = heap.alloc_str("temporary")?;
    heap.record(text);
    if fail {
        Err(Termination::HeapLimit)?;
    }
    heap.close_scope_with(scope, text.into())
}

#[test]
fn scopes_are_repaired_after_an_early_return() {
    let mut heap = Heap::new(HeapConfig::default());
    let outer = heap.open_scope();
    let depth = heap.scope_depth();
    let kept = heap.alloc_str("kept").unwrap();
    heap.record(kept);
    // A success closes its scope (the result goes to the outer one).
    let result = native(&mut heap, false).unwrap();
    assert_eq!(heap.scope_depth(), depth);
    // A failure leaves one scope open.
    assert!(native(&mut heap, true).is_err());
    assert_eq!(heap.scope_depth(), depth + 1);
    assert!(heap.scope_len() >= 3);
    heap.close_scopes_to(depth);
    assert_eq!(heap.scope_depth(), depth);
    heap.collect(&NoRoots);
    assert!(heap.is_live_string(kept));
    let Value::String(result) = result else {
        panic!("a string");
    };
    assert!(heap.is_live_string(result));
    // The temporary of the failed call is gone: kept + result are live
    // strings (and the engine's atom).
    assert_eq!(heap.counts().strings, 3);
    // Closing to a depth above the current one does nothing.
    heap.close_scopes_to(depth + 5);
    assert_eq!(heap.scope_depth(), depth);
    heap.close_scope(outer).unwrap();
}

#[test]
fn dead_scope_tokens_do_not_close_later_scopes() {
    let mut heap = Heap::new(HeapConfig::default());
    let depth = heap.scope_depth();
    let lost = heap.open_scope();
    heap.close_scopes_to(depth);
    let later = heap.open_scope();
    assert!(heap.close_scope(lost).is_err());
    assert_eq!(heap.scope_depth(), depth + 1);
    heap.close_scope(later).unwrap();
}

#[test]
fn passing_a_value_out_of_the_outermost_scope_returns_it_unrooted() {
    let mut heap = Heap::new(HeapConfig::default());
    let scope = heap.open_scope();
    let s = heap.alloc_str("s").unwrap();
    let value = heap.close_scope_with(scope, s.into()).unwrap();
    assert_eq!(value, Value::String(s));
    assert_eq!(heap.scope_depth(), 0);
}

struct Code(Vec<Value>);

impl GenericData for Code {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for value in &self.0 {
            tracer.value(*value);
        }
    }

    fn heap_size(&self) -> usize {
        self.0.capacity() * size_of::<Value>()
    }
}

#[test]
fn every_kind_of_handle_can_be_recorded() {
    let mut heap = Heap::new(HeapConfig::default());
    let scope = heap.open_scope();
    let inner = heap.alloc_str("in a cell").unwrap();
    let cell = heap.alloc_cell(inner.into()).unwrap();
    let held = heap.alloc_str("in generic data").unwrap();
    let generic = heap.alloc_generic(Code(vec![held.into()])).unwrap();
    let big = heap
        .alloc_bigint(BigInt {
            negative: false,
            limbs: vec![7],
        })
        .unwrap();
    let atom = heap.key_from_str("a key").unwrap();
    let described = heap.alloc_str("description").unwrap();
    let symbol = heap.alloc_symbol(Some(described)).unwrap();
    let junk = heap.alloc_str("junk").unwrap();
    heap.record(cell);
    heap.record(generic);
    heap.record(big);
    heap.record(atom);
    heap.record(PropertyKey::Symbol(symbol));
    heap.record(PropertyKey::Index(3));
    heap.collect(&NoRoots);
    assert!(heap.is_live_string(inner));
    assert!(heap.is_live_string(held));
    assert!(heap.bigint(big).is_ok());
    assert!(matches!(atom, PropertyKey::String(s) if heap.is_live_string(s)));
    assert!(heap.is_live_string(described));
    assert!(!heap.is_live_string(junk));
    heap.close_scope(scope).unwrap();
    heap.collect(&NoRoots);
    assert!(heap.cell(cell).is_err());
    assert!(heap.generic::<Code>(generic).is_err());
    assert!(heap.symbol(symbol).is_err());

    // Persistent roots take the same kinds.
    let cell = heap.alloc_cell(Value::Null).unwrap();
    let key = heap.key_from_str("persistent key").unwrap();
    let persistent_cell = heap.persist(cell.into());
    let persistent_key = heap.persist(key.into());
    heap.collect(&NoRoots);
    assert!(heap.cell(cell).is_ok());
    assert!(matches!(key, PropertyKey::String(s) if heap.is_live_string(s)));
    heap.release(persistent_cell).unwrap();
    heap.release(persistent_key).unwrap();
    heap.collect(&NoRoots);
    assert!(heap.cell(cell).is_err());
}

#[test]
fn a_key_list_can_be_rooted() {
    let mut heap = Heap::new(HeapConfig::default());
    let object = heap.new_object(None).unwrap();
    for name in ["alpha", "beta", "gamma"] {
        let key = heap.key_from_str(name).unwrap();
        heap.create_data_property(object, key, Value::Int(1), &NoRoots)
            .unwrap();
    }
    let keys = heap.own_property_keys(object).unwrap();
    let scope = heap.open_scope();
    heap.record_keys(&keys);
    // The object dies; the keys stay.
    heap.collect(&NoRoots);
    assert!(!heap.is_live_object(object));
    for key in &keys {
        assert!(matches!(key, PropertyKey::String(s) if heap.is_live_string(*s)));
    }
    heap.close_scope(scope).unwrap();
    heap.collect(&NoRoots);
    assert!(
        keys.iter()
            .all(|key| matches!(key, PropertyKey::String(s) if !heap.is_live_string(*s)))
    );
}

#[test]
fn host_objects_trace_their_payload() {
    let mut heap = Heap::new(HeapConfig::default());
    let held = heap.alloc_str("native data refers to me").unwrap();
    let data: Gc<Generic> = heap.alloc_generic(Code(vec![held.into()])).unwrap();
    let host = heap.new_host_object(None, 42, data).unwrap();
    let root = heap.persist(host.into());
    heap.collect(&NoRoots);
    assert!(heap.is_live_string(held));
    assert!(heap.generic::<Code>(data).is_ok());
    heap.release(root).unwrap();
    heap.collect(&NoRoots);
    assert!(!heap.is_live_object(host));
    assert!(!heap.is_live_string(held));
    assert!(heap.generic::<Code>(data).is_err());
    // Stale data is refused at once.
    assert!(heap.new_host_object(None, 1, data).is_err());
}

#[test]
fn a_stale_prototype_is_refused_at_once() {
    let mut heap = Heap::new(HeapConfig::default());
    let proto = heap.new_object(None).unwrap();
    heap.collect(&NoRoots);
    assert!(heap.new_object(Some(proto)).is_err());
    assert!(heap.new_array(Some(proto), 0).is_err());
}

#[test]
fn sparse_array_returns_to_dense_when_empty() {
    let mut heap = Heap::new(HeapConfig::default());
    let array = heap.new_array(None, 0).unwrap();
    let root = heap.persist(array.into());
    let empty = heap.heap_size();
    heap.create_data_property(
        array,
        PropertyKey::Index(1_000_000),
        Value::Int(1),
        &NoRoots,
    )
    .unwrap();
    let length = heap.length_atom();
    heap.set(
        array,
        PropertyKey::String(length),
        Value::Int(0),
        array.into(),
        &NoRoots,
    )
    .unwrap();
    heap.collect(&NoRoots);
    assert!(heap.heap_size() <= empty + 2048);
    heap.release(root).unwrap();
}
