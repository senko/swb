//! `Map` (ECMA-262 §24.1, <https://tc39.es/ecma262/#sec-map-objects>):
//! the constructor, `Map.groupBy` (§24.1.2.1), `Map.prototype` in
//! Chromium 148's key order (with `getOrInsert` and `getOrInsertComputed`
//! of the upsert proposal, which Chromium 148 has and test262 tests) and
//! `%MapIteratorPrototype%`. The table, the shared natives and the
//! iterators are in `object/table.rs` and `collection.rs`.

use crate::heap::Gc;
use crate::object::{IterationKind, Object};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

use super::collection::{Which, create_iterator, for_each, install_size};

/// `Map.prototype`'s methods in Chromium 148's key order (`size` comes
/// after `keys`), with their `length`.
const METHODS_BEFORE_SIZE: [(&str, u32, NativeFn); 8] = [
    ("get", 1, get),
    ("set", 2, set),
    ("has", 1, has),
    ("delete", 1, delete),
    ("clear", 0, clear),
    ("entries", 0, entries),
    ("forEach", 1, for_each_method),
    ("keys", 0, keys),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::MapPrototype)?;
    let map = super::constructor(rt, realm, "Map", 0, map_constructor, proto)?;
    super::method(rt, realm, map, "groupBy", 2, group_by)?;
    super::collection::install_species(rt, realm, map)?;
    let mut entries_function = None;
    for (name, length, func) in METHODS_BEFORE_SIZE {
        let function = super::method(rt, realm, proto, name, length, func)?;
        match name {
            "set" => rt.set_intrinsic(realm, Intrinsic::MapPrototypeSet, function),
            "entries" => entries_function = Some(function),
            _ => {}
        }
    }
    install_size(rt, realm, proto, Which::Map)?;
    super::method(rt, realm, proto, "values", 0, values)?;
    super::method(rt, realm, proto, "getOrInsert", 2, get_or_insert)?;
    super::method(
        rt,
        realm,
        proto,
        "getOrInsertComputed",
        2,
        get_or_insert_computed,
    )?;
    super::to_string_tag(rt, proto, "Map")?;
    // §24.1.3.14: `@@iterator` is the same function object as `entries`.
    if let Some(function) = entries_function {
        let key = rt.symbol_key(WellKnown::Iterator);
        rt.define(proto, key, function.into(), true, false, true)?;
    }
    super::collection::install_iterator_prototype(rt, realm, Which::Map)
}

fn map_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    super::collection::construct(rt, call, Which::Map)
}

/// The receiver and the canonical key of a method that takes a key.
fn receiver_and_key(
    rt: &mut Runtime,
    call: &NativeCall,
    method: &str,
) -> VmResult<(Gc<Object>, Value)> {
    let map = rt.this_collection(call, Which::Map, method)?;
    let key = rt.arg(call, 0);
    let key = rt.collection_key(key)?;
    Ok((map, key))
}

/// `Map.prototype.get(key)` (§24.1.3.6).
fn get(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (map, key) = receiver_and_key(rt, call, "Map.prototype.get")?;
    let value = rt
        .heap
        .collection_get(map, key)?
        .unwrap_or(Value::Undefined);
    Ok(NativeReturn::Value(value))
}

/// `Map.prototype.set(key, value)` (§24.1.3.11).
fn set(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (map, key) = receiver_and_key(rt, call, "Map.prototype.set")?;
    let value = rt.arg(call, 1);
    rt.heap.collection_put(map, key, value)?;
    Ok(NativeReturn::Value(map.into()))
}

/// `Map.prototype.has(key)` (§24.1.3.9).
fn has(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (map, key) = receiver_and_key(rt, call, "Map.prototype.has")?;
    let found = rt.heap.collection_find(map, key)?.is_some();
    Ok(NativeReturn::Value(Value::Bool(found)))
}

/// `Map.prototype.delete(key)` (§24.1.3.3).
fn delete(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (map, key) = receiver_and_key(rt, call, "Map.prototype.delete")?;
    let removed = rt.heap.collection_remove(map, key)?;
    Ok(NativeReturn::Value(Value::Bool(removed)))
}

/// `Map.prototype.clear()` (§24.1.3.1).
fn clear(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_collection(call, Which::Map, "Map.prototype.clear")?;
    rt.heap.table_mut(map)?.clear();
    Ok(NativeReturn::Value(Value::Undefined))
}

fn entries(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_iterator(
        rt,
        call,
        Which::Map,
        "Map.prototype.entries",
        IterationKind::Entries,
    )
}

fn keys(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_iterator(
        rt,
        call,
        Which::Map,
        "Map.prototype.keys",
        IterationKind::Keys,
    )
}

fn values(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_iterator(
        rt,
        call,
        Which::Map,
        "Map.prototype.values",
        IterationKind::Values,
    )
}

fn for_each_method(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    for_each(rt, call, Which::Map, "Map.prototype.forEach")
}

/// `Map.prototype.getOrInsert(key, value)` (§24.1.3.7 of the upsert
/// proposal).
fn get_or_insert(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (map, key) = receiver_and_key(rt, call, "Map.prototype.getOrInsert")?;
    let value = rt.arg(call, 1);
    if let Some(existing) = rt.heap.collection_get(map, key)? {
        return Ok(NativeReturn::Value(existing));
    }
    rt.heap.collection_append(map, key, value)?;
    Ok(NativeReturn::Value(value))
}

/// `Map.prototype.getOrInsertComputed(key, callback)` (§24.1.3.8): the
/// callback can change the map, so the key is looked up again after it
/// returned.
fn get_or_insert_computed(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_collection(call, Which::Map, "Map.prototype.getOrInsertComputed")?;
    let callback = rt.arg(call, 1);
    if !rt.is_callable(callback)? {
        // V8 words it with the name of the method (measured).
        return Err(VmError::type_error(
            "Map.prototype.getOrInsertComputed is not a function",
        ));
    }
    let key = rt.arg(call, 0);
    let key = rt.collection_key(key)?;
    if let Some(existing) = rt.heap.collection_get(map, key)? {
        return Ok(NativeReturn::Value(existing));
    }
    let value = rt.call(callback, Value::Undefined, &[key])?;
    rt.heap.record(value);
    rt.heap.collection_put(map, key, value)?;
    Ok(NativeReturn::Value(value))
}

/// `Map.groupBy(items, callback)` (§24.1.2.1): `GroupBy` (§7.3.35) with
/// `collection` key coercion, building the map while the loop runs
/// (nothing sees it before the function returns).
fn group_by(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let items = rt.arg(call, 0);
    let callback = rt.arg(call, 1);
    let proto = rt.intrinsic(call.realm(), Intrinsic::MapPrototype)?;
    let map = rt.heap.new_collection(Some(proto), true)?;
    rt.heap.record(map);
    super::object_iter::group_items(rt, "Map.groupBy", items, callback, &mut |rt, key, value| {
        let key = rt.collection_key(key)?;
        if let Some(Value::Object(group)) = rt.heap.collection_get(map, key)? {
            rt.push_value(group, value)?;
        } else {
            let group = rt.array_from_values(&[value])?;
            rt.heap.collection_put(map, key, group.into())?;
        }
        Ok(())
    })?;
    Ok(NativeReturn::Value(map.into()))
}
