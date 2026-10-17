//! What `Map` and `Set` share (ECMA-262 §24.1, §24.2,
//! <https://tc39.es/ecma262/#sec-keyed-collections>): the key
//! canonicalization, the receiver checks with V8's messages, the
//! constructors' loop (`AddEntriesFromIterable`, §24.1.1.2, which `WeakMap`
//! and `WeakSet` share), `forEach`, the iterator objects
//! (`CreateMapIterator`, `CreateSetIterator`, the `next` methods) and
//! `@@species`.
//!
//! The table is in `object/table.rs`. A native loop over a table
//! (`forEach`, the set methods) registers its position with the table, so
//! that a compaction during a callback corrects it: it sees the entries
//! added during the loop and skips the deleted ones, as the specification's
//! list does. Each entry that a loop or an iterator looks at is a step of
//! the time countdown.

use crate::heap::Gc;
use crate::object::{IterationKind, Object, ObjectKind, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WellKnown};

/// Which collection a native works on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Which {
    Map,
    Set,
}

impl Which {
    pub(super) fn name(self) -> &'static str {
        match self {
            Which::Map => "Map",
            Which::Set => "Set",
        }
    }

    fn iterator_prototype(self) -> Intrinsic {
        match self {
            Which::Map => Intrinsic::MapIteratorPrototype,
            Which::Set => Intrinsic::SetIteratorPrototype,
        }
    }

    fn spec(self) -> Collection {
        match self {
            Which::Map => Collection {
                name: "Map",
                prototype: Intrinsic::MapPrototype,
                adder: ("set", Intrinsic::MapPrototypeSet),
                entries: true,
                weak: false,
            },
            Which::Set => Collection {
                name: "Set",
                prototype: Intrinsic::SetPrototype,
                adder: ("add", Intrinsic::SetPrototypeAdd),
                entries: false,
                weak: false,
            },
        }
    }
}

/// What a collection constructor needs to know about its collection.
#[derive(Clone, Copy)]
pub(super) struct Collection {
    pub(super) name: &'static str,
    pub(super) prototype: Intrinsic,
    /// The name of the adder method and its built-in function.
    pub(super) adder: (&'static str, Intrinsic),
    /// Whether the items are entries (`Map`, `WeakMap`).
    pub(super) entries: bool,
    /// Whether the collection is weak (`WeakMap`, `WeakSet`).
    pub(super) weak: bool,
}

impl Runtime {
    /// `CanonicalizeKeyedCollectionKey` (§7.3.37) and the preparation of a
    /// key for the table: `-0` becomes `+0`, a double with an integer
    /// value takes the integer form, and a rope is flattened (a
    /// safepoint: the caller keeps `key` rooted; the time is charged in
    /// proportion to the length).
    pub(crate) fn collection_key(&mut self, key: Value) -> VmResult<Value> {
        match key {
            Value::Double(number) => Ok(if number == 0.0 {
                Value::Int(0)
            } else {
                Value::number(number)
            }),
            Value::String(string) => {
                self.flatten(string)?;
                let units = self.heap.string(string)?.len();
                self.charge_units(units)?;
                Ok(key)
            }
            _ => Ok(key),
        }
    }

    /// `RequireInternalSlot(this, [[MapData]])` or `[[SetData]]` with
    /// V8's message for another receiver ("Method Map.prototype.get called
    /// on incompatible receiver #<Object>"; `method` is the text between
    /// "Method " and " called").
    pub(super) fn this_collection(
        &self,
        call: &NativeCall,
        which: Which,
        method: &str,
    ) -> VmResult<Gc<Object>> {
        if let Value::Object(object) = call.this() {
            let matches = matches!(
                (&self.heap.object(object)?.kind, which),
                (ObjectKind::Map(_), Which::Map) | (ObjectKind::Set(_), Which::Set)
            );
            if matches {
                return Ok(object);
            }
        }
        Err(self.incompatible_receiver(method, call.this()))
    }

    /// V8's `TypeError` for a method called on another receiver.
    pub(super) fn incompatible_receiver(&self, method: &str, receiver: Value) -> VmError {
        VmError::type_error(format!(
            "Method {method} called on incompatible receiver {}",
            match receiver {
                Value::Object(object) => self.object_text(object),
                _ => self.primitive_text(receiver),
            }
        ))
    }

    /// The `size` of a table as a Number.
    pub(super) fn collection_size(&self, object: Gc<Object>) -> VmResult<usize> {
        Ok(self
            .heap
            .table(object)?
            .map_or(0, crate::object::Table::len))
    }
}

// --- Iterators ---

/// `CreateMapIterator` (§24.1.5.1) and `CreateSetIterator` (§24.2.6.1).
pub(super) fn create_iterator(
    rt: &mut Runtime,
    call: &NativeCall,
    which: Which,
    method: &str,
    kind: IterationKind,
) -> VmResult<NativeReturn> {
    let collection = rt.this_collection(call, which, method)?;
    let proto = rt.intrinsic(call.realm(), which.iterator_prototype())?;
    let iterator = rt
        .heap
        .new_collection_iterator(proto, collection, kind, which == Which::Map)?;
    Ok(NativeReturn::Value(iterator.into()))
}

/// The next value of the Map or Set iterator `iterator`, or `None` when it
/// is finished: `next` without its result object (the compiled iteration
/// steps call it when `next` is the built-in one, which is not
/// observable). The value is recorded in the current handle scope.
pub(crate) fn collection_iterator_next_value(
    rt: &mut Runtime,
    iterator: Gc<Object>,
) -> VmResult<Option<Value>> {
    let is_map = matches!(rt.heap.object(iterator)?.kind, ObjectKind::MapIterator(_));
    let (entry, kind, work) = rt.heap.collection_iterator_next(iterator)?;
    rt.charge(work)?;
    let Some(entry) = entry else {
        return Ok(None);
    };
    // A Set stores its elements as keys.
    let value = if is_map { entry.value } else { entry.key };
    let result = match kind {
        IterationKind::Keys => entry.key,
        IterationKind::Values => value,
        IterationKind::Entries => rt.pair_array(entry.key, value)?.into(),
    };
    Ok(Some(rt.heap.record(result)))
}

/// `%MapIteratorPrototype%.next()` (§24.1.5.2.1) and
/// `%SetIteratorPrototype%.next()` (§24.2.6.2.1).
pub(super) fn iterator_next(
    rt: &mut Runtime,
    call: &NativeCall,
    which: Which,
) -> VmResult<NativeReturn> {
    let matches = match (call.this(), which) {
        (Value::Object(object), Which::Map) => {
            matches!(rt.heap.object(object)?.kind, ObjectKind::MapIterator(_))
        }
        (Value::Object(object), Which::Set) => {
            matches!(rt.heap.object(object)?.kind, ObjectKind::SetIterator(_))
        }
        _ => false,
    };
    let Value::Object(iterator) = call.this() else {
        return Err(rt.incompatible_receiver(
            &format!("{} Iterator.prototype.next", which.name()),
            call.this(),
        ));
    };
    if !matches {
        return Err(rt.incompatible_receiver(
            &format!("{} Iterator.prototype.next", which.name()),
            call.this(),
        ));
    }
    let value = collection_iterator_next_value(rt, iterator)?;
    let result = rt.iter_result(value.unwrap_or(Value::Undefined), value.is_none())?;
    Ok(NativeReturn::Value(result.into()))
}

// --- forEach ---

/// `Map.prototype.forEach` (§24.1.3.5) and `Set.prototype.forEach`
/// (§24.2.4.7).
pub(super) fn for_each(
    rt: &mut Runtime,
    call: &NativeCall,
    which: Which,
    method: &str,
) -> VmResult<NativeReturn> {
    let collection = rt.this_collection(call, which, method)?;
    let callback = rt.arg(call, 0);
    if !rt.is_callable(callback)? {
        return Err(rt.not_callable(callback));
    }
    let this_arg = rt.arg(call, 1);
    let id = rt.heap.collection_loop_begin(collection)?;
    let result = for_each_loop(rt, collection, id, callback, this_arg, which);
    rt.heap.collection_loop_end(collection, id);
    result?;
    Ok(NativeReturn::Value(Value::Undefined))
}

fn for_each_loop(
    rt: &mut Runtime,
    collection: Gc<Object>,
    id: usize,
    callback: Value,
    this_arg: Value,
    which: Which,
) -> VmResult<()> {
    loop {
        let more = rt.with_scope(|rt| {
            let (entry, work) = rt.heap.collection_loop_next(collection, id)?;
            rt.charge(work)?;
            let Some(entry) = entry else {
                return Ok(false);
            };
            let value = if which == Which::Map {
                entry.value
            } else {
                entry.key
            };
            rt.call(callback, this_arg, &[value, entry.key, collection.into()])?;
            Ok(true)
        })?;
        if !more {
            return Ok(());
        }
    }
}

// --- Constructors ---

/// `Map ( [ iterable ] )` (§24.1.1.1) and `Set ( [ iterable ] )`
/// (§24.2.2.1).
pub(super) fn construct(
    rt: &mut Runtime,
    call: &NativeCall,
    which: Which,
) -> VmResult<NativeReturn> {
    construct_collection(rt, call, which.spec())
}

/// The constructors of `Map`, `Set` (§24.1.1.1, §24.2.2.1), `WeakMap` and
/// `WeakSet` (§24.3.1.1, §24.4.1.1): the same steps with their adder.
pub(super) fn construct_collection(
    rt: &mut Runtime,
    call: &NativeCall,
    spec: Collection,
) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error(format!(
            "Constructor {} requires 'new'",
            spec.name
        )));
    }
    let proto = rt.prototype_from(call, spec.prototype)?;
    let collection = if spec.weak {
        rt.heap.new_weak_collection(Some(proto), spec.entries)?
    } else {
        rt.heap.new_collection(Some(proto), spec.entries)?
    };
    rt.heap.record(collection);
    let iterable = rt.arg(call, 0);
    if iterable.is_nullish() {
        return Ok(NativeReturn::Value(collection.into()));
    }
    let (name, builtin) = spec.adder;
    let key = rt.heap.key_from_str(name)?;
    let adder = rt.get_value(collection.into(), key)?;
    if !rt.is_callable(adder)? {
        return Err(VmError::type_error(format!(
            "'{}' returned for property '{name}' of object '{}' is not a function",
            rt.primitive_text(adder),
            rt.object_text(collection)
        )));
    }
    let direct = adder == Value::Object(rt.intrinsic(rt.current_realm(), builtin)?);
    let mut record = rt.get_iterator(iterable)?;
    loop {
        let more = rt.with_scope(|rt| {
            let Some(item) = rt.iterator_step_value(&mut record)? else {
                return Ok(false);
            };
            match add_item(rt, collection, spec, adder, direct, item) {
                Ok(()) => Ok(true),
                Err(error) => Err(rt.iterator_close_after_error(&record, error)),
            }
        })?;
        if !more {
            return Ok(NativeReturn::Value(collection.into()));
        }
    }
}

/// One step of the constructor's loop: the call of the adder with the
/// entry (`Map`) or the value (`Set`). When the adder is the built-in
/// one, the table is changed directly (nobody can see the difference).
fn add_item(
    rt: &mut Runtime,
    collection: Gc<Object>,
    spec: Collection,
    adder: Value,
    direct: bool,
    item: Value,
) -> VmResult<()> {
    let (key, value) = if spec.entries {
        if !matches!(item, Value::Object(_)) {
            return Err(VmError::type_error(format!(
                "Iterator value {} is not an entry object",
                rt.describe_value(item)
            )));
        }
        let key = rt.get_value(item, PropertyKey::Index(0))?;
        let value = rt.get_value(item, PropertyKey::Index(1))?;
        (key, value)
    } else {
        (item, Value::Undefined)
    };
    if direct && spec.weak {
        let key = rt.weak_key_or_throw(key, spec.entries)?;
        rt.charge(1)?;
        rt.heap.weak_put(collection, key, value)?;
    } else if direct {
        let key = rt.collection_key(key)?;
        rt.charge(1)?;
        rt.heap.collection_put(collection, key, value)?;
    } else if spec.entries {
        rt.call(adder, collection.into(), &[key, value])?;
    } else {
        rt.call(adder, collection.into(), &[key])?;
    }
    Ok(())
}

// --- @@species ---

/// Defines the `@@species` getter (§24.1.2.3, §24.2.3.2) on a
/// constructor.
pub(super) fn install_species(
    rt: &mut Runtime,
    realm: u32,
    constructor: Gc<Object>,
) -> VmResult<()> {
    let getter = rt.new_builtin(realm, "get [Symbol.species]", 0, species, false)?;
    let key = rt.symbol_key(WellKnown::Species);
    let desc = PropertyDescriptor::accessor(getter.into(), Value::Undefined, false, true);
    rt.define_own_property(constructor, key, desc)?;
    Ok(())
}

/// `get Map[@@species]` and `get Set[@@species]`: the receiver.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn species(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

/// Defines the accessor `size` (§24.1.3.12, §24.2.4.14) on a prototype.
pub(super) fn install_size(
    rt: &mut Runtime,
    realm: u32,
    prototype: Gc<Object>,
    which: Which,
) -> VmResult<()> {
    let name = format!("get {}", "size");
    let func = if which == Which::Map {
        map_size
    } else {
        set_size
    };
    let getter = rt.new_builtin(realm, &name, 0, func, false)?;
    let key = rt.heap.key_from_str("size")?;
    let desc = PropertyDescriptor::accessor(getter.into(), Value::Undefined, false, true);
    rt.define_own_property(prototype, key, desc)?;
    Ok(())
}

fn map_size(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let map = rt.this_collection(call, Which::Map, "get Map.prototype.size")?;
    let size = rt.collection_size(map)?;
    Ok(NativeReturn::Value(Value::number(size as f64)))
}

fn set_size(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let set = rt.this_collection(call, Which::Set, "get Set.prototype.size")?;
    let size = rt.collection_size(set)?;
    Ok(NativeReturn::Value(Value::number(size as f64)))
}

/// Installs `next` and `@@toStringTag` on an iterator prototype and
/// records `next` as an intrinsic for the compiled iteration steps.
pub(super) fn install_iterator_prototype(
    rt: &mut Runtime,
    realm: u32,
    which: Which,
) -> VmResult<()> {
    let proto = rt.intrinsic(realm, which.iterator_prototype())?;
    let (func, tag, intrinsic): (crate::vm::NativeFn, &str, Intrinsic) = match which {
        Which::Map => (
            |rt, call| iterator_next(rt, call, Which::Map),
            "Map Iterator",
            Intrinsic::MapIteratorNext,
        ),
        Which::Set => (
            |rt, call| iterator_next(rt, call, Which::Set),
            "Set Iterator",
            Intrinsic::SetIteratorNext,
        ),
    };
    let next = super::method(rt, realm, proto, "next", 0, func)?;
    rt.set_intrinsic(realm, intrinsic, next);
    super::to_string_tag(rt, proto, tag)
}
