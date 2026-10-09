//! The collector, roots, weak tables, accounting and the heap limit
//! (ADR 0026 section 8), and the hostile cases of the heap.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::collections::{HashSet, VecDeque};

use swb_js::{
    Error, Gc, GetResult, Heap, HeapConfig, NoRoots, Object, PropertyKey, RootSource, Termination,
    Tracer, Value,
};

/// A small deterministic generator (xorshift64*), so that the random
/// graphs are the same in every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The roots of a test VM: a value stack.
struct Stack(Vec<Value>);

impl RootSource for Stack {
    fn trace_roots(&self, tracer: &mut Tracer<'_>) {
        for value in &self.0 {
            tracer.value(*value);
        }
    }
}

/// A random object graph and its model: for each object, its edges as
/// (key, target index), and the expected prototype.
struct Graph {
    objects: Vec<Gc<Object>>,
    edges: Vec<Vec<(PropertyKey, usize)>>,
    protos: Vec<Option<usize>>,
}

fn build_graph(heap: &mut Heap, rng: &mut Rng, count: usize) -> Graph {
    let mut objects = Vec::new();
    let mut protos = Vec::new();
    for i in 0..count {
        // Some objects inherit from an earlier one; some are arrays.
        let proto = (i > 0 && rng.below(4) == 0).then(|| rng.below(i));
        let proto_handle = proto.map(|p| objects[p]);
        let object = if rng.below(5) == 0 {
            heap.new_array(proto_handle, 0).unwrap()
        } else {
            heap.new_object(proto_handle).unwrap()
        };
        heap.record(object);
        objects.push(object);
        protos.push(proto);
    }
    let mut edges = vec![Vec::new(); count];
    for (i, list) in edges.iter_mut().enumerate() {
        for e in 0..rng.below(6) {
            let target = rng.below(count);
            let key = match rng.below(3) {
                0 => PropertyKey::Index(rng.below(200) as u32),
                1 => {
                    let key = heap.key_from_str(&format!("e{e}")).unwrap();
                    if let PropertyKey::String(atom) = key {
                        heap.record(atom);
                    }
                    key
                }
                _ => {
                    let description = heap.alloc_str(&format!("sym{i}.{e}")).unwrap();
                    let symbol = heap.alloc_symbol(Some(description)).unwrap();
                    PropertyKey::Symbol(heap.record(symbol))
                }
            };
            if list.iter().any(|&(k, _)| k == key) {
                continue;
            }
            let value = objects[target].into();
            assert!(
                heap.create_data_property(objects[i], key, value, &NoRoots)
                    .unwrap()
            );
            list.push((key, target));
        }
    }
    Graph {
        objects,
        edges,
        protos,
    }
}

fn reachable(graph: &Graph, roots: &[usize]) -> HashSet<usize> {
    let mut seen: HashSet<usize> = roots.iter().copied().collect();
    let mut queue: VecDeque<usize> = roots.iter().copied().collect();
    while let Some(i) = queue.pop_front() {
        let targets = graph.edges[i]
            .iter()
            .map(|&(_, t)| t)
            .chain(graph.protos[i]);
        for target in targets {
            if seen.insert(target) {
                queue.push_back(target);
            }
        }
    }
    seen
}

fn check_graph(heap: &Heap, graph: &Graph, live: &HashSet<usize>) {
    for (i, &object) in graph.objects.iter().enumerate() {
        if !live.contains(&i) {
            assert!(heap.object(object).is_err(), "object {i} should be freed");
            continue;
        }
        assert!(heap.object(object).is_ok(), "object {i} should be alive");
        let proto = heap.get_prototype_of(object).unwrap();
        assert_eq!(proto, graph.protos[i].map(|p| graph.objects[p]));
        for &(key, target) in &graph.edges[i] {
            if let PropertyKey::Symbol(symbol) = key {
                let description = heap.symbol(symbol).unwrap().description.unwrap();
                assert!(heap.is_live_string(description));
            }
            let value = heap.get(object, key, object.into()).unwrap();
            assert_eq!(value, GetResult::Value(graph.objects[target].into()));
        }
    }
}

fn random_graph_run(config: HeapConfig, count: usize, seed: u64) {
    let mut heap = Heap::new(config);
    let mut rng = Rng(seed);
    // The build roots every new thing in a handle scope (the stress mode
    // collects whenever a property vector grows).
    let build = heap.open_scope();
    let graph = build_graph(&mut heap, &mut rng, count);
    // Roots of three kinds: a VM stack, persistent roots, a handle scope.
    let stack_roots: Vec<usize> = (0..5).map(|_| rng.below(count)).collect();
    let persistent_roots: Vec<usize> = (0..3).map(|_| rng.below(count)).collect();
    let scope_roots: Vec<usize> = (0..3).map(|_| rng.below(count)).collect();
    let stack = Stack(
        stack_roots
            .iter()
            .map(|&i| graph.objects[i].into())
            .collect(),
    );
    let tokens: Vec<_> = persistent_roots
        .iter()
        .map(|&i| heap.persist(graph.objects[i].into()))
        .collect();
    heap.close_scope(build).unwrap();
    let scope = heap.open_scope();
    for &i in &scope_roots {
        heap.record(graph.objects[i]);
    }
    let all_roots: Vec<usize> = [&stack_roots[..], &persistent_roots, &scope_roots].concat();
    let live = reachable(&graph, &all_roots);
    assert!(live.len() < count, "the test needs garbage");
    let stats = heap.collect(&stack);
    assert_eq!(stats.last_stale_roots, 0);
    assert_eq!(heap.counts().objects, live.len());
    check_graph(&heap, &graph, &live);

    // Drop the scope and the persistent roots; allocate new objects into
    // the freed slots; the old handles stay stale.
    heap.close_scope(scope).unwrap();
    for token in tokens {
        heap.release(token).unwrap();
    }
    heap.collect(&stack);
    let live = reachable(&graph, &stack_roots);
    check_graph(&heap, &graph, &live);
    let fresh: Vec<_> = (0..count).map(|_| heap.new_object(None).unwrap()).collect();
    let reused = fresh
        .iter()
        .filter(|f| graph.objects.iter().any(|o| o.index() == f.index()))
        .count();
    assert!(reused > 0);
    check_graph(&heap, &graph, &live);
    heap.collect(&NoRoots);
    assert_eq!(heap.counts().objects, 0);
    assert!(fresh.iter().all(|&f| heap.object(f).is_err()));
}

#[test]
fn random_graphs_survive_collection_intact() {
    for seed in [1, 2, 3, 0x5EED] {
        random_graph_run(HeapConfig::default(), 3000, seed);
    }
}

#[test]
fn random_graph_in_stress_mode() {
    let config = HeapConfig {
        stress: true,
        ..HeapConfig::default()
    };
    random_graph_run(config, 300, 7);
}

#[test]
fn weak_tables_drop_dead_entries() {
    let mut heap = Heap::new(HeapConfig::default());
    let base = heap.counts();
    let kept = heap.new_object(None).unwrap();
    let root = heap.persist(kept.into());
    for i in 0..1000 {
        let proto = heap.new_object(None).unwrap();
        let o = heap.new_object(Some(proto)).unwrap();
        let k = heap.key_from_str(&format!("name{i}")).unwrap();
        assert!(
            heap.create_data_property(o, k, Value::Int(1), &NoRoots)
                .unwrap()
        );
    }
    let k = heap.key_from_str("kept").unwrap();
    assert!(
        heap.create_data_property(kept, k, Value::Int(1), &NoRoots)
            .unwrap()
    );
    let before = heap.counts();
    assert!(before.atoms >= base.atoms + 1001);
    assert!(before.transitions >= 1001);
    assert!(before.root_shapes >= 1001);
    heap.collect(&NoRoots);
    let after = heap.counts();
    // "length" and "kept" stay; the transition and root shape of `kept`
    // stay.
    assert_eq!(after.atoms, base.atoms + 1);
    assert_eq!(after.transitions, 1);
    assert_eq!(after.root_shapes, 1);
    assert_eq!(after.objects, 1);
    heap.release(root).unwrap();
    heap.collect(&NoRoots);
    assert_eq!(heap.counts().transitions, 0);
    assert_eq!(heap.counts().root_shapes, 0);
    assert_eq!(heap.counts().shapes, 0);
}

#[test]
fn safepoints_collect_past_the_threshold() {
    let mut heap = Heap::new(HeapConfig {
        min_threshold: 1 << 20,
        ..HeapConfig::default()
    });
    assert!(!heap.safepoint(&NoRoots).unwrap());
    while heap.allocated_since_gc() <= heap.threshold() {
        heap.new_object(None).unwrap();
    }
    assert!(heap.gc_due());
    assert!(heap.safepoint(&NoRoots).unwrap());
    assert_eq!(heap.stats().collections, 1);
    assert_eq!(heap.allocated_since_gc(), 0);
    assert!(heap.heap_size() < 64 << 10);
    // The threshold follows the live size, with the minimum.
    assert_eq!(heap.threshold(), 1 << 20);
}

#[test]
fn heap_limit_terminates_and_reservations_collect() {
    let config = HeapConfig {
        limit: 4 << 20,
        min_threshold: 1 << 20,
        ..HeapConfig::default()
    };
    let mut heap = Heap::new(config);
    // Garbage does not count against a reservation: it is collected.
    for _ in 0..20_000 {
        heap.alloc_str("garbage garbage garbage").unwrap();
    }
    heap.reserve(3 << 20, &NoRoots).unwrap();
    assert!(heap.stats().collections >= 1);
    // A request above the limit fails, before any allocation.
    assert_eq!(
        heap.reserve(5 << 20, &NoRoots),
        Err(Error::Terminated(Termination::HeapLimit))
    );
    // Live data past the limit ends the script at a safepoint.
    let mut stack = Stack(Vec::new());
    let result = loop {
        let s = heap.alloc_str("live live live live live").unwrap();
        stack.0.push(s.into());
        if let Err(error) = heap.safepoint(&stack) {
            break error;
        }
    };
    assert_eq!(result, Error::Terminated(Termination::HeapLimit));
    // The heap is usable afterwards; dropping the roots frees it.
    stack.0.clear();
    heap.collect(&stack);
    assert!(heap.heap_size() < 1 << 20);
    heap.reserve(1 << 20, &stack).unwrap();
}

#[test]
fn growing_one_object_past_the_limit_terminates() {
    let mut heap = Heap::new(HeapConfig {
        limit: 8 << 20,
        min_threshold: 1 << 20,
        ..HeapConfig::default()
    });
    let array = heap.new_array(None, 0).unwrap();
    let root = heap.persist(array.into());
    let mut result = Ok(true);
    for index in 0..10_000_000 {
        result =
            heap.create_data_property(array, PropertyKey::Index(index), Value::Int(1), &NoRoots);
        if result.is_err() {
            break;
        }
    }
    assert_eq!(result, Err(Error::Terminated(Termination::HeapLimit)));
    assert!(heap.heap_size() <= 8 << 20);
    heap.release(root).unwrap();
    heap.collect(&NoRoots);
    assert!(heap.heap_size() < 1 << 20);
}

#[test]
fn no_gc_regions_skip_collections() {
    let mut heap = Heap::new(HeapConfig {
        stress: true,
        ..HeapConfig::default()
    });
    let s = heap.alloc_str("unrooted").unwrap();
    heap.enter_no_gc();
    heap.reserve(100, &NoRoots).unwrap();
    assert!(!heap.safepoint(&NoRoots).unwrap());
    assert!(heap.is_live_string(s));
    heap.leave_no_gc();
    assert!(heap.safepoint(&NoRoots).unwrap());
    assert!(!heap.is_live_string(s));
}

#[test]
fn cells_and_bigints_are_traced() {
    struct Roots(Gc<swb_js::ValueCell>, Gc<swb_js::BigInt>);
    impl RootSource for Roots {
        fn trace_roots(&self, tracer: &mut Tracer<'_>) {
            tracer.cell(self.0);
            tracer.bigint(self.1);
        }
    }
    let mut heap = Heap::new(HeapConfig::default());
    let s = heap.alloc_str("in a cell").unwrap();
    let cell = heap.alloc_cell(s.into()).unwrap();
    let big = heap
        .alloc_bigint(swb_js::BigInt {
            negative: false,
            limbs: vec![1, 2],
        })
        .unwrap();
    heap.collect(&Roots(cell, big));
    assert!(heap.is_live_string(s));
    assert_eq!(heap.bigint(big).unwrap().limbs, [1, 2]);
    heap.cell_mut(cell).unwrap().value = Value::Undefined;
    heap.collect(&Roots(cell, big));
    assert!(!heap.is_live_string(s));
    heap.collect(&NoRoots);
    assert!(heap.cell(cell).is_err());
    assert!(heap.bigint(big).is_err());
}

#[test]
fn stale_roots_are_counted_not_followed() {
    let mut heap = Heap::new(HeapConfig::default());
    let o = heap.new_object(None).unwrap();
    heap.collect(&NoRoots);
    let p = heap.new_object(None).unwrap();
    assert_eq!(p.index(), o.index());
    // A stale root must not keep the new object in the same slot alive.
    let stats = heap.collect(&Stack(vec![o.into()]));
    assert_eq!(stats.last_stale_roots, 1);
    assert!(heap.object(p).is_err());
}

// --- Hostile cases ---------------------------------------------------

#[test]
fn hostile_ten_million_properties_on_one_object() {
    let mut heap = Heap::new(HeapConfig {
        limit: 256 << 20,
        ..HeapConfig::default()
    });
    let object = heap.new_object(None).unwrap();
    let root = heap.persist(object.into());
    let mut stored = 0;
    let mut outcome = Ok(true);
    for i in 0..10_000_000u32 {
        let name = heap.intern_str(&format!("k{i}"));
        outcome = name.and_then(|name| {
            heap.create_data_property(object, PropertyKey::String(name), Value::Int(1), &NoRoots)
        });
        if outcome.is_err() {
            break;
        }
        stored += 1;
    }
    // Either all properties fit, or the heap limit ends the script.
    if outcome.is_err() {
        assert_eq!(outcome, Err(Error::Terminated(Termination::HeapLimit)));
    }
    assert!(stored > 100_000);
    let k = heap.key_from_str("k77777").unwrap();
    assert_eq!(
        heap.get(object, k, Value::Undefined),
        Ok(GetResult::Value(Value::Int(1)))
    );
    heap.release(root).unwrap();
    heap.collect(&NoRoots);
    assert!(heap.heap_size() < 1 << 20);
    assert_eq!(heap.counts().atoms, 1);
}

#[test]
fn hostile_deep_prototype_chain() {
    let mut heap = Heap::new(HeapConfig::default());
    let base = heap.new_object(None).unwrap();
    let k = heap.key_from_str("deep").unwrap();
    assert!(
        heap.create_data_property(base, k, Value::Int(42), &NoRoots)
            .unwrap()
    );
    let mut leaf = base;
    for _ in 0..100_000 {
        leaf = heap.new_object(Some(leaf)).unwrap();
    }
    let missing = heap.key_from_str("missing").unwrap();
    assert_eq!(
        heap.get(leaf, k, leaf.into()),
        Ok(GetResult::Value(Value::Int(42)))
    );
    assert_eq!(
        heap.get(leaf, missing, leaf.into()),
        Ok(GetResult::Value(Value::Undefined))
    );
    assert_eq!(heap.has_property(leaf, missing), Ok(false));
    // A cycle through the whole chain is refused.
    assert_eq!(heap.set_prototype_of(base, Some(leaf)), Ok(false));
    // Marking the chain uses the work list, not the Rust stack.
    heap.collect(&Stack(vec![leaf.into()]));
    assert_eq!(heap.counts().objects, 100_001);
    assert_eq!(
        heap.get(leaf, k, leaf.into()),
        Ok(GetResult::Value(Value::Int(42)))
    );
    heap.collect(&NoRoots);
    assert_eq!(heap.counts().objects, 0);
}

#[test]
fn hostile_array_of_maximal_length() {
    let mut heap = Heap::new(HeapConfig::default());
    let array = heap.new_array(None, u32::MAX).unwrap();
    let stack = Stack(vec![array.into()]);
    let last = PropertyKey::Index(u32::MAX - 1);
    assert!(
        heap.create_data_property(array, last, Value::Int(1), &stack)
            .unwrap()
    );
    let length = heap.key_from_str("length").unwrap();
    assert_eq!(
        heap.get(array, length, array.into()),
        Ok(GetResult::Value(Value::Double(4_294_967_295.0)))
    );
    for index in (0..u32::MAX).step_by(1 << 22) {
        let k = PropertyKey::Index(index);
        assert!(
            heap.create_data_property(array, k, Value::Int(2), &stack)
                .unwrap()
        );
    }
    heap.collect(&stack);
    assert!(heap.heap_size() < 1 << 20, "{}", heap.heap_size());
    assert_eq!(heap.own_property_keys(array).unwrap().len(), 1025 + 1);
    let set = heap
        .set(array, length, Value::Int(1), array.into(), &stack)
        .unwrap();
    assert_eq!(set, swb_js::SetResult::Done(true));
    assert_eq!(heap.own_property_keys(array).unwrap().len(), 2);
}

#[test]
fn hostile_many_distinct_atoms() {
    let mut heap = Heap::new(HeapConfig::default());
    let kept: Vec<Value> = (0..1000)
        .map(|i| heap.intern_str(&format!("kept{i}")).unwrap().into())
        .collect();
    for i in 0..1_000_000 {
        heap.intern_str(&format!("atom{i}")).unwrap();
        heap.safepoint(&Stack(kept.clone())).unwrap();
    }
    heap.collect(&Stack(kept.clone()));
    assert_eq!(heap.counts().atoms, 1001);
    for (i, value) in kept.iter().enumerate() {
        let Value::String(atom) = *value else {
            panic!("an atom");
        };
        assert_eq!(heap.intern_str(&format!("kept{i}")).unwrap(), atom);
    }
}
