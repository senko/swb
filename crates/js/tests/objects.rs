//! The object model against ECMA-262: the ordinary internal methods
//! (§10.1), the array exotic object (§10.4.2), shapes and dictionary
//! mode (ADR 0026 section 6).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use swb_js::{
    Error, Gc, GetResult, Heap, HeapConfig, NoRoots, Object, Property, PropertyDescriptor,
    PropertyKey, SetResult, ThrowKind, Value,
};

fn heap() -> Heap {
    Heap::new(HeapConfig::default())
}

fn key(heap: &mut Heap, name: &str) -> PropertyKey {
    heap.key_from_str(name).unwrap()
}

fn put(heap: &mut Heap, object: Gc<Object>, name: &str, value: Value) {
    let key = key(heap, name);
    assert!(
        heap.create_data_property(object, key, value, &NoRoots)
            .unwrap()
    );
}

fn define(heap: &mut Heap, object: Gc<Object>, name: &str, desc: PropertyDescriptor) -> bool {
    let key = key(heap, name);
    heap.define_own_property(object, key, desc, &NoRoots)
        .unwrap()
}

fn get(heap: &mut Heap, object: Gc<Object>, name: &str) -> Value {
    let key = key(heap, name);
    match heap.get(object, key, object.into()).unwrap() {
        GetResult::Value(value) => value,
        other @ GetResult::CallGetter { .. } => panic!("expected a value, got {other:?}"),
    }
}

fn set(heap: &mut Heap, object: Gc<Object>, name: &str, value: Value) -> SetResult {
    let key = key(heap, name);
    heap.set(object, key, value, object.into(), &NoRoots)
        .unwrap()
}

fn names(heap: &Heap, object: Gc<Object>) -> Vec<String> {
    heap.own_property_keys(object)
        .unwrap()
        .into_iter()
        .map(|key| match key {
            PropertyKey::Index(index) => index.to_string(),
            PropertyKey::String(atom) => heap.string(atom).unwrap().as_str16().to_string_lossy(),
            PropertyKey::Symbol(_) => "@@symbol".to_string(),
        })
        .collect()
}

fn read_only(value: Value) -> PropertyDescriptor {
    PropertyDescriptor::data(value, false, true, false)
}

#[test]
fn own_property_keys_order() {
    let mut heap = heap();
    let o = heap.new_object(None).unwrap();
    let symbol = heap.alloc_symbol(None).unwrap();
    put(&mut heap, o, "b", Value::Int(1));
    assert!(
        heap.create_data_property(o, PropertyKey::Symbol(symbol), Value::Int(0), &NoRoots)
            .unwrap()
    );
    put(&mut heap, o, "10", Value::Int(2));
    put(&mut heap, o, "a", Value::Int(3));
    put(&mut heap, o, "2", Value::Int(4));
    put(&mut heap, o, "05", Value::Int(5));
    put(&mut heap, o, "4294967295", Value::Int(6));
    put(&mut heap, o, "4294967294", Value::Int(7));
    assert_eq!(
        names(&heap, o),
        [
            "2",
            "10",
            "4294967294",
            "b",
            "a",
            "05",
            "4294967295",
            "@@symbol"
        ]
    );
    // Deleting and adding again moves a string key to the end.
    let b = key(&mut heap, "b");
    assert!(heap.delete(o, b).unwrap());
    put(&mut heap, o, "b", Value::Int(1));
    assert_eq!(
        names(&heap, o),
        [
            "2",
            "10",
            "4294967294",
            "a",
            "05",
            "4294967295",
            "b",
            "@@symbol"
        ]
    );
}

#[test]
fn array_keys_put_length_first() {
    let mut heap = heap();
    let a = heap.new_array(None, 0).unwrap();
    put(&mut heap, a, "x", Value::Null);
    put(&mut heap, a, "1", Value::Null);
    put(&mut heap, a, "0", Value::Null);
    assert_eq!(names(&heap, a), ["0", "1", "length", "x"]);
}

#[test]
fn get_and_set_along_the_prototype_chain() {
    let mut heap = heap();
    let proto = heap.new_object(None).unwrap();
    let child = heap.new_object(Some(proto)).unwrap();
    put(&mut heap, proto, "inherited", Value::Int(1));
    assert_eq!(get(&mut heap, child, "inherited"), Value::Int(1));
    assert_eq!(get(&mut heap, child, "missing"), Value::Undefined);
    // A write creates an own property on the receiver.
    assert_eq!(
        set(&mut heap, child, "inherited", Value::Int(2)),
        SetResult::Done(true)
    );
    assert_eq!(get(&mut heap, child, "inherited"), Value::Int(2));
    assert_eq!(get(&mut heap, proto, "inherited"), Value::Int(1));
    // A read-only inherited property blocks the write.
    assert!(define(&mut heap, proto, "fixed", read_only(Value::Int(1))));
    assert_eq!(
        set(&mut heap, child, "fixed", Value::Int(2)),
        SetResult::Done(false)
    );
    let fixed = key(&mut heap, "fixed");
    assert_eq!(heap.get_own_property(child, fixed).unwrap(), None);
    let has = heap.has_property(child, fixed).unwrap();
    assert!(has);
}

#[test]
fn accessors_return_call_requests() {
    let mut heap = heap();
    let proto = heap.new_object(None).unwrap();
    let child = heap.new_object(Some(proto)).unwrap();
    let getter = heap.new_object(None).unwrap();
    let setter = heap.new_object(None).unwrap();
    let desc = PropertyDescriptor::accessor(getter.into(), setter.into(), true, true);
    assert!(define(&mut heap, proto, "x", desc));
    let x = key(&mut heap, "x");
    assert_eq!(
        heap.get(child, x, child.into()).unwrap(),
        GetResult::CallGetter {
            getter: getter.into(),
            receiver: child.into()
        }
    );
    assert_eq!(
        heap.set(child, x, Value::Int(1), child.into(), &NoRoots)
            .unwrap(),
        SetResult::CallSetter {
            setter: setter.into(),
            receiver: child.into()
        }
    );
    // A getter-only accessor: reads give the call, writes fail.
    let desc = PropertyDescriptor::accessor(getter.into(), Value::Undefined, true, true);
    assert!(define(&mut heap, proto, "y", desc));
    assert_eq!(
        set(&mut heap, child, "y", Value::Int(1)),
        SetResult::Done(false)
    );
    // A primitive receiver cannot get a property.
    let z = key(&mut heap, "z");
    assert_eq!(
        heap.set(child, z, Value::Int(1), Value::Int(5), &NoRoots)
            .unwrap(),
        SetResult::Done(false)
    );
}

#[test]
fn set_with_another_receiver() {
    let mut heap = heap();
    let target = heap.new_object(None).unwrap();
    let receiver = heap.new_object(None).unwrap();
    put(&mut heap, target, "p", Value::Int(1));
    let p = key(&mut heap, "p");
    // The receiver gets the property; the target keeps its value.
    assert_eq!(
        heap.set(target, p, Value::Int(2), receiver.into(), &NoRoots)
            .unwrap(),
        SetResult::Done(true)
    );
    assert_eq!(get(&mut heap, receiver, "p"), Value::Int(2));
    assert_eq!(get(&mut heap, target, "p"), Value::Int(1));
    // An accessor on the receiver blocks the write (§10.1.9.2 step 3.d.i).
    let desc = PropertyDescriptor::accessor(Value::Undefined, Value::Undefined, true, true);
    assert!(define(&mut heap, receiver, "p", desc));
    assert_eq!(
        heap.set(target, p, Value::Int(3), receiver.into(), &NoRoots)
            .unwrap(),
        SetResult::Done(false)
    );
}

#[test]
fn define_own_property_validates() {
    let mut heap = heap();
    let o = heap.new_object(None).unwrap();
    assert!(define(&mut heap, o, "frozen", read_only(Value::Int(1))));
    assert!(define(
        &mut heap,
        o,
        "frozen",
        read_only(Value::Double(1.0))
    ));
    assert!(!define(&mut heap, o, "frozen", read_only(Value::Int(2))));
    assert!(!define(
        &mut heap,
        o,
        "frozen",
        PropertyDescriptor {
            configurable: Some(true),
            ..Default::default()
        }
    ));
    assert_eq!(
        set(&mut heap, o, "frozen", Value::Int(2)),
        SetResult::Done(false)
    );
    // Attribute defaults of a new property are false.
    assert!(define(
        &mut heap,
        o,
        "plain",
        PropertyDescriptor::value(Value::Int(1))
    ));
    let plain = key(&mut heap, "plain");
    assert_eq!(
        heap.get_own_property(o, plain).unwrap(),
        Some(Property::Data {
            value: Value::Int(1),
            writable: false,
            enumerable: false,
            configurable: false
        })
    );
    // An invalid descriptor gives a TypeError for the caller.
    let both = PropertyDescriptor {
        value: Some(Value::Int(1)),
        set: Some(Value::Undefined),
        ..Default::default()
    };
    assert!(matches!(
        heap.define_own_property(o, plain, both, &NoRoots),
        Err(Error::Throw {
            kind: ThrowKind::TypeError,
            ..
        })
    ));
}

#[test]
fn delete_and_prevent_extensions() {
    let mut heap = heap();
    let o = heap.new_object(None).unwrap();
    put(&mut heap, o, "a", Value::Int(1));
    assert!(define(&mut heap, o, "fixed", read_only(Value::Int(1))));
    let a = key(&mut heap, "a");
    let fixed = key(&mut heap, "fixed");
    let missing = key(&mut heap, "missing");
    assert!(heap.delete(o, a).unwrap());
    assert!(!heap.delete(o, fixed).unwrap());
    assert!(heap.delete(o, missing).unwrap());
    assert!(heap.prevent_extensions(o).unwrap());
    assert!(!heap.is_extensible(o).unwrap());
    assert!(
        !heap
            .create_data_property(o, a, Value::Int(1), &NoRoots)
            .unwrap()
    );
    assert_eq!(
        set(&mut heap, o, "fixed2", Value::Int(1)),
        SetResult::Done(false)
    );
    // Existing properties can still change.
    assert!(define(&mut heap, o, "fixed", read_only(Value::Int(1))));
}

#[test]
fn set_prototype_of_rules() {
    let mut heap = heap();
    let a = heap.new_object(None).unwrap();
    let b = heap.new_object(Some(a)).unwrap();
    let c = heap.new_object(Some(b)).unwrap();
    // No cycles.
    assert!(!heap.set_prototype_of(a, Some(c)).unwrap());
    assert!(!heap.set_prototype_of(a, Some(a)).unwrap());
    assert_eq!(heap.get_prototype_of(a).unwrap(), None);
    // An empty object moves to the root shape of the new prototype.
    let empty = heap.new_object(None).unwrap();
    assert!(heap.set_prototype_of(empty, Some(b)).unwrap());
    assert_eq!(
        heap.object(empty).unwrap().shape(),
        heap.object(c).unwrap().shape()
    );
    // An object with properties goes to dictionary mode and keeps them.
    put(&mut heap, a, "p", Value::Int(1));
    let other = heap.new_object(None).unwrap();
    put(&mut heap, other, "q", Value::Int(2));
    assert!(heap.set_prototype_of(a, Some(other)).unwrap());
    let shape = heap.object(a).unwrap().shape();
    assert!(heap.shape(shape).unwrap().is_dictionary());
    assert_eq!(get(&mut heap, a, "p"), Value::Int(1));
    assert_eq!(get(&mut heap, c, "q"), Value::Int(2));
    // Same prototype: true even if not extensible; another: false.
    assert!(heap.prevent_extensions(b).unwrap());
    assert!(heap.set_prototype_of(b, Some(a)).unwrap());
    assert!(!heap.set_prototype_of(b, None).unwrap());
}

#[test]
fn shapes_are_shared_and_branch() {
    let mut heap = heap();
    let proto = heap.new_object(None).unwrap();
    let make = |heap: &mut Heap, keys: &[&str]| {
        let o = heap.new_object(Some(proto)).unwrap();
        for name in keys {
            put(heap, o, name, Value::Int(1));
        }
        heap.object(o).unwrap().shape()
    };
    let ab1 = make(&mut heap, &["a", "b"]);
    let ab2 = make(&mut heap, &["a", "b"]);
    let ac = make(&mut heap, &["a", "c"]);
    let ba = make(&mut heap, &["b", "a"]);
    assert_eq!(ab1, ab2);
    assert_ne!(ab1, ac);
    assert_ne!(ab1, ba);
    let empty1 = make(&mut heap, &[]);
    let other_proto = heap.new_object(None).unwrap();
    let o = heap.new_object(Some(other_proto)).unwrap();
    assert_ne!(empty1, heap.object(o).unwrap().shape());
    assert_eq!(heap.shape(ac).unwrap().proto(), Some(proto));
    assert_eq!(heap.shape(ac).unwrap().property_count(), 2);
}

#[test]
fn dictionary_mode_triggers() {
    let mut heap = heap();
    let is_dictionary = |heap: &Heap, o: Gc<Object>| {
        let shape = heap.object(o).unwrap().shape();
        heap.shape(shape).unwrap().is_dictionary()
    };
    // Delete.
    let o = heap.new_object(None).unwrap();
    put(&mut heap, o, "a", Value::Int(1));
    put(&mut heap, o, "b", Value::Int(2));
    assert!(!is_dictionary(&heap, o));
    let a = key(&mut heap, "a");
    heap.delete(o, a).unwrap();
    assert!(is_dictionary(&heap, o));
    assert_eq!(get(&mut heap, o, "b"), Value::Int(2));
    // Attribute change.
    let o = heap.new_object(None).unwrap();
    put(&mut heap, o, "a", Value::Int(1));
    assert!(define(
        &mut heap,
        o,
        "a",
        PropertyDescriptor {
            enumerable: Some(false),
            ..Default::default()
        }
    ));
    assert!(is_dictionary(&heap, o));
    // A value change alone keeps the shared shape.
    let o = heap.new_object(None).unwrap();
    put(&mut heap, o, "a", Value::Int(1));
    assert!(define(
        &mut heap,
        o,
        "a",
        PropertyDescriptor::value(Value::Int(5))
    ));
    assert!(!is_dictionary(&heap, o));
    assert_eq!(get(&mut heap, o, "a"), Value::Int(5));
    // More than 128 named properties.
    let o = heap.new_object(None).unwrap();
    for i in 0..128 {
        put(&mut heap, o, &format!("p{i}"), Value::Int(i));
    }
    assert!(!is_dictionary(&heap, o));
    put(&mut heap, o, "p128", Value::Int(128));
    assert!(is_dictionary(&heap, o));
    for i in 0..129 {
        assert_eq!(get(&mut heap, o, &format!("p{i}")), Value::Int(i));
    }
}

#[test]
fn dictionary_kind_changes_and_compaction() {
    let mut heap = heap();
    let o = heap.new_object(None).unwrap();
    for i in 0..40 {
        put(&mut heap, o, &format!("p{i}"), Value::Int(i));
    }
    // Data to accessor and back, in dictionary mode.
    let getter = heap.new_object(None).unwrap();
    let accessor = PropertyDescriptor::accessor(getter.into(), Value::Undefined, true, true);
    assert!(define(&mut heap, o, "p3", accessor));
    let p3 = key(&mut heap, "p3");
    assert!(matches!(
        heap.get(o, p3, o.into()).unwrap(),
        GetResult::CallGetter { .. }
    ));
    assert!(define(
        &mut heap,
        o,
        "p3",
        PropertyDescriptor::value(Value::Int(33))
    ));
    assert_eq!(
        heap.get_own_property(o, p3).unwrap(),
        Some(Property::Data {
            value: Value::Int(33),
            writable: false,
            enumerable: true,
            configurable: true
        })
    );
    // Many deletes compact the dictionary; the rest stays correct.
    for i in (0..40).step_by(2) {
        let k = key(&mut heap, &format!("p{i}"));
        assert!(heap.delete(o, k).unwrap());
    }
    let expected: Vec<String> = (1..40).step_by(2).map(|i| format!("p{i}")).collect();
    assert_eq!(names(&heap, o), expected);
    for i in (1..40).step_by(2) {
        let value = if i == 3 { 33 } else { i };
        assert_eq!(get(&mut heap, o, &format!("p{i}")), Value::Int(value));
    }
}

#[test]
fn array_length_follows_indices() {
    let mut heap = heap();
    let a = heap.new_array(None, 0).unwrap();
    put(&mut heap, a, "0", Value::Int(1));
    put(&mut heap, a, "5", Value::Int(2));
    assert_eq!(heap.array_length(a).unwrap(), (6, true));
    assert_eq!(get(&mut heap, a, "length"), Value::Int(6));
    assert_eq!(get(&mut heap, a, "3"), Value::Undefined);
    // `a[4294967294]` is the last index; `a[4294967295]` is a string key.
    put(&mut heap, a, "4294967294", Value::Int(3));
    assert_eq!(heap.array_length(a).unwrap(), (u32::MAX, true));
    assert_eq!(get(&mut heap, a, "length"), Value::Double(4_294_967_295.0));
    put(&mut heap, a, "4294967295", Value::Int(4));
    assert_eq!(heap.array_length(a).unwrap(), (u32::MAX, true));
    assert_eq!(
        names(&heap, a),
        ["0", "5", "4294967294", "length", "4294967295"]
    );
}

#[test]
fn array_set_length_deletes_down_to_non_configurable() {
    let mut heap = heap();
    let a = heap.new_array(None, 0).unwrap();
    for i in 0..10 {
        put(&mut heap, a, &i.to_string(), Value::Int(i));
    }
    let fixed = PropertyDescriptor::data(Value::Int(4), true, true, false);
    assert!(define(&mut heap, a, "4", fixed));
    // `length = 2` stops at index 4: length becomes 5 and [[Set]] fails.
    assert_eq!(
        set(&mut heap, a, "length", Value::Int(2)),
        SetResult::Done(false)
    );
    assert_eq!(heap.array_length(a).unwrap(), (5, true));
    assert_eq!(names(&heap, a), ["0", "1", "2", "3", "4", "length"]);
    // With writable: false, the length becomes read-only even on failure.
    let desc = PropertyDescriptor {
        value: Some(Value::Int(0)),
        writable: Some(false),
        ..Default::default()
    };
    assert!(!define(&mut heap, a, "length", desc));
    assert_eq!(heap.array_length(a).unwrap(), (5, false));
    // A read-only length blocks new indices at or above it.
    assert_eq!(
        set(&mut heap, a, "5", Value::Int(5)),
        SetResult::Done(false)
    );
    assert_eq!(set(&mut heap, a, "0", Value::Int(9)), SetResult::Done(true));
    // [[Set]] of a read-only `length` fails even with the same value;
    // [[DefineOwnProperty]] with the same value succeeds.
    assert_eq!(
        set(&mut heap, a, "length", Value::Int(5)),
        SetResult::Done(false)
    );
    assert!(define(
        &mut heap,
        a,
        "length",
        PropertyDescriptor::value(Value::Int(5))
    ));
    assert!(!define(
        &mut heap,
        a,
        "length",
        PropertyDescriptor::value(Value::Int(6))
    ));
    assert!(!define(
        &mut heap,
        a,
        "length",
        PropertyDescriptor::value(Value::Int(4))
    ));
}

#[test]
fn array_set_length_checks_the_value() {
    let mut heap = heap();
    let a = heap.new_array(None, 0).unwrap();
    let length = key(&mut heap, "length");
    for bad in [
        Value::Double(1.5),
        Value::Int(-1),
        Value::Double(4_294_967_296.0),
        Value::Double(f64::NAN),
        Value::Undefined,
    ] {
        assert_eq!(
            heap.set(a, length, bad, a.into(), &NoRoots),
            Err(Error::Throw {
                kind: ThrowKind::RangeError,
                message: "Invalid array length"
            }),
            "{bad:?}"
        );
    }
    for (good, expected) in [
        (Value::Double(-0.0), 0),
        (Value::Bool(true), 1),
        (Value::Null, 0),
        (Value::Double(4_294_967_295.0), u32::MAX),
    ] {
        assert_eq!(
            heap.set(a, length, good, a.into(), &NoRoots),
            Ok(SetResult::Done(true))
        );
        assert_eq!(heap.array_length(a).unwrap().0, expected);
    }
    // `length` cannot become an accessor, configurable or enumerable.
    let accessor = PropertyDescriptor::accessor(Value::Undefined, Value::Undefined, false, false);
    assert!(
        !heap
            .define_own_property(a, length, accessor, &NoRoots)
            .unwrap()
    );
    let enumerable = PropertyDescriptor {
        enumerable: Some(true),
        ..Default::default()
    };
    assert!(
        !heap
            .define_own_property(a, length, enumerable, &NoRoots)
            .unwrap()
    );
    assert!(!heap.delete(a, length).unwrap());
}

#[test]
fn truncating_a_huge_sparse_array_costs_its_elements() {
    let mut heap = heap();
    let a = heap.new_array(None, u32::MAX).unwrap();
    for index in [0u32, 1000, 1_000_000, 4_000_000_000, 4_294_967_294] {
        let k = PropertyKey::Index(index);
        assert!(
            heap.create_data_property(a, k, Value::Int(1), &NoRoots)
                .unwrap()
        );
    }
    assert!(heap.heap_size() < 1 << 20, "{}", heap.heap_size());
    let start = std::time::Instant::now();
    assert_eq!(
        set(&mut heap, a, "length", Value::Int(0)),
        SetResult::Done(true)
    );
    assert!(start.elapsed().as_millis() < 100);
    assert_eq!(names(&heap, a), ["length"]);
    assert_eq!(heap.array_length(a).unwrap(), (0, true));
}

#[test]
fn elements_with_attributes_and_holes() {
    let mut heap = heap();
    let proto = heap.new_array(None, 0).unwrap();
    let a = heap.new_array(Some(proto), 3).unwrap();
    put(&mut heap, a, "0", Value::Int(0));
    put(&mut heap, a, "2", Value::Int(2));
    // A hole reads through the prototype.
    put(&mut heap, proto, "1", Value::Int(11));
    assert_eq!(get(&mut heap, a, "1"), Value::Int(11));
    // A read-only element.
    assert!(define(&mut heap, a, "2", read_only(Value::Int(2))));
    assert_eq!(
        set(&mut heap, a, "2", Value::Int(5)),
        SetResult::Done(false)
    );
    assert_eq!(get(&mut heap, a, "2"), Value::Int(2));
    // An accessor element.
    let getter = heap.new_object(None).unwrap();
    let accessor = PropertyDescriptor::accessor(getter.into(), Value::Undefined, true, true);
    assert!(define(&mut heap, a, "0", accessor));
    let zero = PropertyKey::Index(0);
    assert!(matches!(
        heap.get(a, zero, a.into()).unwrap(),
        GetResult::CallGetter { .. }
    ));
    assert!(heap.delete(a, zero).unwrap());
    assert_eq!(names(&heap, a), ["2", "length"]);
}

#[test]
fn stale_handles_are_errors() {
    let mut heap = heap();
    let o = heap.new_object(None).unwrap();
    heap.collect(&NoRoots);
    let k = key(&mut heap, "x");
    assert!(matches!(
        heap.get(o, k, Value::Undefined),
        Err(Error::Internal(_))
    ));
    assert!(matches!(
        heap.set(o, k, Value::Null, Value::Undefined, &NoRoots),
        Err(Error::Internal(_))
    ));
    // The slot is reused; the old handle still fails.
    let p = heap.new_object(None).unwrap();
    assert_eq!(p.index(), o.index());
    assert!(heap.object(o).is_err());
    assert!(heap.object(p).is_ok());
}

#[test]
fn stress_mode_with_handle_scopes() {
    let mut heap = Heap::new(HeapConfig {
        stress: true,
        ..HeapConfig::default()
    });
    let scope = heap.open_scope();
    let proto = heap.new_object(None).unwrap();
    heap.record(proto);
    let objects: Vec<Gc<Object>> = (0..50)
        .map(|_| {
            let o = heap.new_object(Some(proto)).unwrap();
            heap.record(o)
        })
        .collect();
    for (i, &o) in objects.iter().enumerate() {
        for j in 0..20 {
            let name = heap.intern_str(&format!("k{j}")).unwrap();
            heap.record(name);
            let value = heap.alloc_str(&format!("v{i}.{j}")).unwrap();
            heap.record(value);
            let k = PropertyKey::String(name);
            assert!(
                heap.create_data_property(o, k, value.into(), &NoRoots)
                    .unwrap()
            );
            heap.safepoint(&NoRoots).unwrap();
        }
    }
    assert!(heap.stats().collections > 100);
    for (i, &o) in objects.iter().enumerate() {
        for j in 0..20 {
            let Value::String(s) = get(&mut heap, o, &format!("k{j}")) else {
                panic!("a string value");
            };
            let text = heap.string(s).unwrap().as_str16().to_string_lossy();
            assert_eq!(text, format!("v{i}.{j}"));
        }
    }
    assert_eq!(heap.stats().last_stale_roots, 0);
    heap.close_scope(scope).unwrap();
    heap.collect(&NoRoots);
    assert!(heap.object(objects[0]).is_err());
}
