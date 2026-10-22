//! `Set` (ECMA-262 §24.2, <https://tc39.es/ecma262/#sec-set-objects>):
//! the constructor, `Set.prototype` in Chromium 148's key order and
//! `%SetIteratorPrototype%`. The ES2025 set methods are in
//! `set_methods.rs`; the table is in `object/table.rs` (with its heap
//! operations in `object/collection.rs`), the shared natives and the
//! iterators are in `collection.rs`. A `Set` stores its elements as the
//! keys of the table.

use crate::object::IterationKind;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmResult, WellKnown};

use super::collection::{Which, create_iterator, for_each, install_size};
use super::set_methods as methods;

/// `Set.prototype`'s methods in Chromium 148's key order up to `size`,
/// with their `length`.
const BEFORE_SIZE: [(&str, u32, NativeFn); 11] = [
    ("has", 1, has),
    ("add", 1, add),
    ("delete", 1, delete),
    ("difference", 1, methods::difference),
    ("clear", 0, clear),
    ("entries", 0, entries),
    ("forEach", 1, for_each_method),
    ("intersection", 1, methods::intersection),
    ("isSubsetOf", 1, methods::is_subset_of),
    ("isSupersetOf", 1, methods::is_superset_of),
    ("isDisjointFrom", 1, methods::is_disjoint_from),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::SetPrototype)?;
    let set = super::constructor(rt, realm, "Set", 0, set_constructor, proto)?;
    super::collection::install_species(rt, realm, set)?;
    for (name, length, func) in BEFORE_SIZE {
        let function = super::method(rt, realm, proto, name, length, func)?;
        if name == "add" {
            rt.set_intrinsic(realm, Intrinsic::SetPrototypeAdd, function);
        }
    }
    install_size(rt, realm, proto, Which::Set)?;
    super::method(
        rt,
        realm,
        proto,
        "symmetricDifference",
        1,
        methods::symmetric_difference,
    )?;
    super::method(rt, realm, proto, "union", 1, methods::union)?;
    let values_function = super::method(rt, realm, proto, "values", 0, values)?;
    // §24.2.4.13, §24.2.4.18: `keys` and `@@iterator` are the same function
    // object as `values`.
    let keys = rt.heap.key_from_str("keys")?;
    rt.define(proto, keys, values_function.into(), true, false, true)?;
    super::to_string_tag(rt, proto, "Set")?;
    let key = rt.symbol_key(WellKnown::Iterator);
    rt.define(proto, key, values_function.into(), true, false, true)?;
    super::collection::install_iterator_prototype(rt, realm, Which::Set)
}

fn set_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    super::collection::construct(rt, call, Which::Set)
}

/// `Set.prototype.has(value)` (§24.2.4.8).
fn has(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let set = rt.this_collection(call, Which::Set, "Set.prototype.has")?;
    let key = rt.arg(call, 0);
    let key = rt.collection_key(key)?;
    let found = rt.heap.collection_find(set, key)?.is_some();
    Ok(NativeReturn::Value(Value::Bool(found)))
}

/// `Set.prototype.add(value)` (§24.2.4.1).
fn add(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let set = rt.this_collection(call, Which::Set, "Set.prototype.add")?;
    let key = rt.arg(call, 0);
    let key = rt.collection_key(key)?;
    rt.heap.collection_put(set, key, Value::Undefined)?;
    Ok(NativeReturn::Value(set.into()))
}

/// `Set.prototype.delete(value)` (§24.2.4.4).
fn delete(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let set = rt.this_collection(call, Which::Set, "Set.prototype.delete")?;
    let key = rt.arg(call, 0);
    let key = rt.collection_key(key)?;
    let removed = rt.heap.collection_remove(set, key)?;
    Ok(NativeReturn::Value(Value::Bool(removed)))
}

/// `Set.prototype.clear()` (§24.2.4.2).
fn clear(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let set = rt.this_collection(call, Which::Set, "Set.prototype.clear")?;
    rt.heap.collection_table_mut(set)?.clear();
    Ok(NativeReturn::Value(Value::Undefined))
}

fn entries(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_iterator(
        rt,
        call,
        Which::Set,
        "Set.prototype.entries",
        IterationKind::Entries,
    )
}

fn values(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_iterator(
        rt,
        call,
        Which::Set,
        "Set.prototype.values",
        IterationKind::Values,
    )
}

fn for_each_method(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    for_each(rt, call, Which::Set, "Set.prototype.forEach")
}
