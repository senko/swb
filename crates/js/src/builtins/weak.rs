//! `WeakMap` (ECMA-262 §24.3, <https://tc39.es/ecma262/#sec-weakmap-objects>)
//! with `getOrInsert` and `getOrInsertComputed` of the upsert proposal
//! (Chromium 148 has them), and `WeakSet` (§24.4). The keys are objects and
//! symbols that are not registered (`CanBeHeldWeakly`, §9.13, with the
//! `symbols-as-weakmap-keys` feature; well-known symbols are allowed, as
//! the specification says and Chromium 148 does). The table is in
//! `object/weak.rs` and `object/table.rs`; the collector treats a
//! `WeakMap` entry as an ephemeron (`heap/gc.rs`). The prototypes have
//! Chromium 148's key order, and the messages are V8's (measured with
//! `tools/probes/js-weak.json`).

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, WeakKey};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult};

use super::collection::{Collection, construct_collection};

/// V8's message for a `WeakMap` key that cannot be held weakly.
const INVALID_MAP_KEY: &str = "Invalid value used as weak map key";
/// V8's message for a `WeakSet` value that cannot be held weakly.
const INVALID_SET_VALUE: &str = "Invalid value used in weak set";

const WEAK_MAP: Collection = Collection {
    name: "WeakMap",
    prototype: Intrinsic::WeakMapPrototype,
    adder: ("set", Intrinsic::WeakMapPrototypeSet),
    entries: true,
    weak: true,
};

const WEAK_SET: Collection = Collection {
    name: "WeakSet",
    prototype: Intrinsic::WeakSetPrototype,
    adder: ("add", Intrinsic::WeakSetPrototypeAdd),
    entries: false,
    weak: true,
};

/// `WeakMap.prototype`'s methods in Chromium 148's key order.
const MAP_METHODS: [(&str, u32, NativeFn); 6] = [
    ("delete", 1, map_delete),
    ("get", 1, map_get),
    ("set", 2, map_set),
    ("has", 1, map_has),
    ("getOrInsert", 2, get_or_insert),
    ("getOrInsertComputed", 2, get_or_insert_computed),
];

/// `WeakSet.prototype`'s methods in Chromium 148's key order.
const SET_METHODS: [(&str, u32, NativeFn); 3] = [
    ("delete", 1, set_delete),
    ("has", 1, set_has),
    ("add", 1, set_add),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::WeakMapPrototype)?;
    super::constructor(rt, realm, "WeakMap", 0, weak_map_constructor, proto)?;
    for (name, length, func) in MAP_METHODS {
        let function = super::method(rt, realm, proto, name, length, func)?;
        if name == "set" {
            rt.set_intrinsic(realm, Intrinsic::WeakMapPrototypeSet, function);
        }
    }
    super::to_string_tag(rt, proto, "WeakMap")?;
    let proto = rt.intrinsic(realm, Intrinsic::WeakSetPrototype)?;
    super::constructor(rt, realm, "WeakSet", 0, weak_set_constructor, proto)?;
    for (name, length, func) in SET_METHODS {
        let function = super::method(rt, realm, proto, name, length, func)?;
        if name == "add" {
            rt.set_intrinsic(realm, Intrinsic::WeakSetPrototypeAdd, function);
        }
    }
    super::to_string_tag(rt, proto, "WeakSet")
}

impl Runtime {
    /// `CanBeHeldWeakly(value)` (§9.13): the key of an object or of a
    /// symbol that `Symbol.for` did not register.
    pub(crate) fn weak_key(&self, value: Value) -> Option<WeakKey> {
        match value {
            Value::Symbol(symbol) if self.symbol_key_for(symbol).is_some() => None,
            _ => WeakKey::of(value),
        }
    }

    /// [`Runtime::weak_key`], or V8's `TypeError` for a `WeakMap` key
    /// (`map`) or a `WeakSet` value.
    pub(crate) fn weak_key_or_throw(&self, value: Value, map: bool) -> VmResult<WeakKey> {
        self.weak_key(value).ok_or_else(|| {
            VmError::type_error(if map {
                INVALID_MAP_KEY
            } else {
                INVALID_SET_VALUE
            })
        })
    }

    /// `RequireInternalSlot(this, [[WeakMapData]])` or `[[WeakSetData]]`
    /// with V8's message ("Method WeakMap.prototype.get called on
    /// incompatible receiver #<Object>").
    fn this_weak_collection(
        &self,
        call: &NativeCall,
        map: bool,
        method: &str,
    ) -> VmResult<Gc<Object>> {
        if let Value::Object(object) = call.this() {
            let matches = matches!(
                (&self.heap.object(object)?.kind, map),
                (ObjectKind::WeakMap(_), true) | (ObjectKind::WeakSet(_), false)
            );
            if matches {
                return Ok(object);
            }
        }
        Err(self.incompatible_receiver(method, call.this()))
    }
}

fn weak_map_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct_collection(rt, call, WEAK_MAP)
}

fn weak_set_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct_collection(rt, call, WEAK_SET)
}

/// `WeakMap.prototype.delete(key)` (§24.3.3.2).
fn map_delete(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_weak_collection(call, true, "WeakMap.prototype.delete")?;
    remove(rt, map, rt.arg(call, 0))
}

/// `WeakMap.prototype.get(key)` (§24.3.3.3).
fn map_get(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_weak_collection(call, true, "WeakMap.prototype.get")?;
    let value = match rt.weak_key(rt.arg(call, 0)) {
        Some(key) => rt.heap.weak_get(map, key)?.unwrap_or(Value::Undefined),
        None => Value::Undefined,
    };
    Ok(NativeReturn::Value(value))
}

/// `WeakMap.prototype.has(key)` (§24.3.3.4).
fn map_has(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_weak_collection(call, true, "WeakMap.prototype.has")?;
    contains(rt, map, rt.arg(call, 0))
}

/// `WeakMap.prototype.set(key, value)` (§24.3.3.5).
fn map_set(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_weak_collection(call, true, "WeakMap.prototype.set")?;
    let key = rt.weak_key_or_throw(rt.arg(call, 0), true)?;
    rt.heap.weak_put(map, key, rt.arg(call, 1))?;
    Ok(NativeReturn::Value(map.into()))
}

/// `WeakMap.prototype.getOrInsert(key, value)` (upsert proposal).
fn get_or_insert(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_weak_collection(call, true, "WeakMap.prototype.getOrInsert")?;
    let key = rt.weak_key_or_throw(rt.arg(call, 0), true)?;
    if let Some(existing) = rt.heap.weak_get(map, key)? {
        return Ok(NativeReturn::Value(existing));
    }
    let value = rt.arg(call, 1);
    rt.heap.weak_put(map, key, value)?;
    Ok(NativeReturn::Value(value))
}

/// `WeakMap.prototype.getOrInsertComputed(key, callbackfn)` (upsert
/// proposal): the callable check comes before the key check (test262 and
/// Chromium 148), and the callback can change the map, so the key is
/// looked up again after it returned.
fn get_or_insert_computed(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_weak_collection(call, true, "WeakMap.prototype.getOrInsertComputed")?;
    let callback = rt.arg(call, 1);
    if !rt.is_callable(callback)? {
        // V8 words it with the name of the method (measured).
        return Err(VmError::type_error(
            "WeakMap.prototype.getOrInsertComputed is not a function",
        ));
    }
    let key = rt.arg(call, 0);
    let weak = rt.weak_key_or_throw(key, true)?;
    if let Some(existing) = rt.heap.weak_get(map, weak)? {
        return Ok(NativeReturn::Value(existing));
    }
    let value = rt.call(callback, Value::Undefined, &[key])?;
    rt.heap.record(value);
    rt.heap.weak_put(map, weak, value)?;
    Ok(NativeReturn::Value(value))
}

/// `WeakSet.prototype.add(value)` (§24.4.3.1).
fn set_add(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let set = rt.this_weak_collection(call, false, "WeakSet.prototype.add")?;
    let key = rt.weak_key_or_throw(rt.arg(call, 0), false)?;
    rt.heap.weak_put(set, key, Value::Undefined)?;
    Ok(NativeReturn::Value(set.into()))
}

/// `WeakSet.prototype.delete(value)` (§24.4.3.3).
fn set_delete(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let set = rt.this_weak_collection(call, false, "WeakSet.prototype.delete")?;
    remove(rt, set, rt.arg(call, 0))
}

/// `WeakSet.prototype.has(value)` (§24.4.3.4).
fn set_has(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let set = rt.this_weak_collection(call, false, "WeakSet.prototype.has")?;
    contains(rt, set, rt.arg(call, 0))
}

/// `has`: false for a value that cannot be held weakly.
fn contains(rt: &Runtime, collection: Gc<Object>, key: Value) -> VmResult<NativeReturn> {
    let found = match rt.weak_key(key) {
        Some(key) => rt.heap.weak_get(collection, key)?.is_some(),
        None => false,
    };
    Ok(NativeReturn::Value(Value::Bool(found)))
}

/// `delete`: false for a value that cannot be held weakly.
fn remove(rt: &mut Runtime, collection: Gc<Object>, key: Value) -> VmResult<NativeReturn> {
    let removed = match rt.weak_key(key) {
        Some(key) => rt.heap.weak_remove(collection, key)?,
        None => false,
    };
    Ok(NativeReturn::Value(Value::Bool(removed)))
}
