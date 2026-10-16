//! Iteration objects (ECMA-262 §27.1, <https://tc39.es/ecma262/#sec-iteration>,
//! §23.1.5): the `Iterator` constructor with `Iterator.from` and
//! `Iterator.concat`, `%IteratorPrototype%` with `@@iterator`, `constructor`
//! and `@@toStringTag` (accessors that
//! `SetterThatIgnoresPrototypeProperties` handles) and the helper methods,
//! `%IteratorHelperPrototype%`, `%WrapForValidIteratorPrototype%`
//! (`iterator_helper.rs`, `iterator_consume.rs`, `iterator_from.rs`),
//! `%ArrayIteratorPrototype%` with `next` and `@@toStringTag`, and
//! `Array.prototype.keys`, `values` and `entries` (`array.rs` installs
//! them, with `@@iterator`).
//!
//! An Array Iterator works on any object with a `length` (arrays and
//! array-likes). It reads `length` again at each step, so growth and
//! shrinkage show; it never allocates in proportion to the length. Once
//! it has finished, or when reading `length` or an element threw, it stays
//! finished (the specification's generator closure has completed).

use crate::heap::Gc;
use crate::object::{ArrayIterator, IterationKind, Object, ObjectKind, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

use super::{iterator_consume, iterator_from, iterator_helper};

/// The methods of `Iterator.prototype` in Chromium 148's key order, with
/// their `length`.
const METHODS: [(&str, u32, NativeFn); 11] = [
    ("reduce", 1, iterator_consume::reduce),
    ("toArray", 0, iterator_consume::to_array),
    ("forEach", 1, iterator_consume::for_each),
    ("some", 1, iterator_consume::some),
    ("every", 1, iterator_consume::every),
    ("find", 1, iterator_consume::find),
    ("map", 1, iterator_helper::map),
    ("filter", 1, iterator_helper::filter),
    ("take", 1, iterator_helper::take),
    ("drop", 1, iterator_helper::drop),
    ("flatMap", 1, iterator_helper::flat_map),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let iterator_proto = rt.intrinsic(realm, Intrinsic::IteratorPrototype)?;
    install_constructor(rt, realm, iterator_proto)?;
    for (name, length, func) in METHODS {
        super::method(rt, realm, iterator_proto, name, length, func)?;
    }
    super::symbol_method(
        rt,
        realm,
        iterator_proto,
        WellKnown::Iterator,
        0,
        iterator_iterator,
        (true, true),
    )?;
    let tag_key = rt.symbol_key(WellKnown::ToStringTag);
    install_accessor(
        rt,
        realm,
        iterator_proto,
        tag_key,
        "[Symbol.toStringTag]",
        (tag_get, tag_set),
    )?;
    install_helper_prototypes(rt, realm)?;
    let array_iterator_proto = rt.intrinsic(realm, Intrinsic::ArrayIteratorPrototype)?;
    let next = super::method(rt, realm, array_iterator_proto, "next", 0, next)?;
    rt.set_intrinsic(realm, Intrinsic::ArrayIteratorNext, next);
    super::to_string_tag(rt, array_iterator_proto, "Array Iterator")
}

/// `Iterator` (§27.1.3.1) with `from` and `concat`, and the accessor
/// `Iterator.prototype.constructor` (<https://tc39.es/ecma262/#sec-iterator.prototype.constructor>).
fn install_constructor(rt: &mut Runtime, realm: u32, iterator_proto: Gc<Object>) -> VmResult<()> {
    let iterator = rt.new_builtin(realm, "Iterator", 0, iterator_constructor, true)?;
    let key = PropertyKey::String(rt.vm.atoms.prototype);
    rt.define(iterator, key, iterator_proto.into(), false, false, false)?;
    super::method(rt, realm, iterator, "from", 1, iterator_from::from)?;
    super::method(rt, realm, iterator, "concat", 0, iterator_from::concat)?;
    rt.set_intrinsic(realm, Intrinsic::IteratorConstructor, iterator);
    super::global(rt, realm, "Iterator", iterator.into())?;
    let key = PropertyKey::String(rt.vm.atoms.constructor);
    install_accessor(
        rt,
        realm,
        iterator_proto,
        key,
        "constructor",
        (constructor_get, constructor_set),
    )
}

/// Defines an accessor property (non-enumerable, configurable) whose
/// getter and setter are named "get NAME" and "set NAME".
fn install_accessor(
    rt: &mut Runtime,
    realm: u32,
    target: Gc<Object>,
    key: PropertyKey,
    name: &str,
    (get, set): (NativeFn, NativeFn),
) -> VmResult<()> {
    let getter = rt.new_builtin(realm, &format!("get {name}"), 0, get, false)?;
    let setter = rt.new_builtin(realm, &format!("set {name}"), 1, set, false)?;
    let desc = PropertyDescriptor::accessor(getter.into(), setter.into(), false, true);
    rt.define_own_property(target, key, desc)?;
    Ok(())
}

/// `%IteratorHelperPrototype%` (§27.1.2.1) and
/// `%WrapForValidIteratorPrototype%` (§27.1.3.2.1.1).
fn install_helper_prototypes(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let helper_proto = rt.intrinsic(realm, Intrinsic::IteratorHelperPrototype)?;
    let next = super::method(rt, realm, helper_proto, "next", 0, iterator_helper::next)?;
    rt.set_intrinsic(realm, Intrinsic::IteratorHelperNext, next);
    super::method(
        rt,
        realm,
        helper_proto,
        "return",
        0,
        iterator_helper::return_,
    )?;
    super::to_string_tag(rt, helper_proto, "Iterator Helper")?;
    let wrap_proto = rt.intrinsic(realm, Intrinsic::WrapForValidIteratorPrototype)?;
    super::method(rt, realm, wrap_proto, "next", 0, iterator_from::wrap_next)?;
    super::method(
        rt,
        realm,
        wrap_proto,
        "return",
        0,
        iterator_from::wrap_return,
    )?;
    Ok(())
}

/// `Iterator ( )` (§27.1.3.1): an abstract class. A call, and a `new`
/// that names `Iterator` itself, throw; a subclass works.
fn iterator_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    match call.new_target() {
        Value::Undefined => {
            return Err(VmError::type_error("Constructor Iterator requires 'new'"));
        }
        Value::Object(target) if target == call.callee() => {
            return Err(VmError::type_error(
                "Abstract class Iterator not directly constructable",
            ));
        }
        _ => {}
    }
    let proto = rt.prototype_from(call, Intrinsic::IteratorPrototype)?;
    let object = rt.heap.new_object(Some(proto))?;
    Ok(NativeReturn::Value(object.into()))
}

/// `get Iterator.prototype.constructor`: `%Iterator%`.
fn constructor_get(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let constructor = rt.intrinsic(call.realm(), Intrinsic::IteratorConstructor)?;
    Ok(NativeReturn::Value(constructor.into()))
}

/// `set Iterator.prototype.constructor`.
fn constructor_set(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let key = PropertyKey::String(rt.vm.atoms.constructor);
    setter_that_ignores_prototype_properties(rt, call, key, ".constructor")
}

/// `get Iterator.prototype [ @@toStringTag ]`: "Iterator".
fn tag_get(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    let text = rt.heap.intern_str("Iterator")?;
    Ok(NativeReturn::Value(text.into()))
}

/// `set Iterator.prototype [ @@toStringTag ]`.
fn tag_set(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let key = rt.symbol_key(WellKnown::ToStringTag);
    setter_that_ignores_prototype_properties(rt, call, key, "[Symbol.toStringTag]")
}

/// `SetterThatIgnoresPrototypeProperties(this, home, p, v)`
/// (<https://tc39.es/ecma262/#sec-SetterThatIgnoresPrototypeProperties>):
/// a write to `Iterator.prototype` itself fails; any other receiver gets
/// an own data property, or a `Set` if it has one already. `label` is
/// the suffix of V8's message for a receiver that is not an object.
fn setter_that_ignores_prototype_properties(
    rt: &mut Runtime,
    call: &NativeCall,
    key: PropertyKey,
    label: &str,
) -> VmResult<NativeReturn> {
    let Value::Object(this) = call.this() else {
        return Err(VmError::type_error(format!(
            "set Iterator.prototype{label} called on non-object"
        )));
    };
    let home = rt.intrinsic(call.realm(), Intrinsic::IteratorPrototype)?;
    if this == home {
        return Err(VmError::type_error(format!(
            "Cannot assign to read only property '{}' of object '[object Object]'",
            rt.key_text(key)
        )));
    }
    let value = rt.arg(call, 0);
    if rt.own_property(this, key)?.is_none() {
        rt.create_data_property_or_throw(this, key, value)?;
    } else {
        rt.set_or_throw(this, key, value)?;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `%IteratorPrototype%[@@iterator]()` (§27.1.2.1): returns `this`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn iterator_iterator(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

/// `Array.prototype.entries()` (§23.1.3.5).
pub(super) fn entries(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_array_iterator(rt, call, IterationKind::Entries)
}

/// `Array.prototype.keys()` (§23.1.3.19).
pub(super) fn keys(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_array_iterator(rt, call, IterationKind::Keys)
}

/// `Array.prototype.values()` (§23.1.3.40); also `@@iterator`.
pub(super) fn values(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    create_array_iterator(rt, call, IterationKind::Values)
}

/// `CreateArrayIterator` (§23.1.5.1) for `ToObject(this)`.
fn create_array_iterator(
    rt: &mut Runtime,
    call: &NativeCall,
    kind: IterationKind,
) -> VmResult<NativeReturn> {
    let target = rt.this_object(call)?;
    let proto = rt.intrinsic(call.realm(), Intrinsic::ArrayIteratorPrototype)?;
    let state = ArrayIterator {
        target: Some(target),
        next_index: 0.0,
        kind,
    };
    let iterator = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::ArrayIterator(Box::new(state)))?;
    Ok(NativeReturn::Value(iterator.into()))
}

/// `%ArrayIteratorPrototype%.next()` (§23.1.5.2.1).
fn next(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(iterator) = call.this() else {
        return Err(incompatible(rt, call.this()));
    };
    if !matches!(rt.heap.object(iterator)?.kind, ObjectKind::ArrayIterator(_)) {
        return Err(incompatible(rt, call.this()));
    }
    let value = array_iterator_next_value(rt, iterator)?;
    let result = rt.iter_result(value.unwrap_or(Value::Undefined), value.is_none())?;
    Ok(NativeReturn::Value(result.into()))
}

/// The value of the next step of the Array Iterator `iterator`, or `None`
/// when it is finished: `next` without its result object. The compiled
/// iteration steps call it for an Array Iterator whose `next` is the
/// built-in one, which is not observable. The value is recorded in the
/// current handle scope.
pub(crate) fn array_iterator_next_value(
    rt: &mut Runtime,
    iterator: Gc<Object>,
) -> VmResult<Option<Value>> {
    let ObjectKind::ArrayIterator(state) = &rt.heap.object(iterator)?.kind else {
        return Err(VmError::invariant(
            "a step of an Array Iterator on another object",
        ));
    };
    let (Some(target), index, kind) = (state.target, state.next_index, state.kind) else {
        return Ok(None);
    };
    let step = array_iterator_step(rt, iterator, target, index, kind);
    if !matches!(step, Ok(Some(_))) {
        // Done, or `length` or an element threw: the iterator is finished.
        set_target(rt, iterator, None)?;
    }
    step
}

/// One step of the closure of `CreateArrayIterator`: the value, or `None`
/// at the end of the array-like.
fn array_iterator_step(
    rt: &mut Runtime,
    iterator: Gc<Object>,
    target: Gc<Object>,
    index: f64,
    kind: IterationKind,
) -> VmResult<Option<Value>> {
    let length = rt.length_of_array_like(target)?;
    if index >= length {
        return Ok(None);
    }
    if let ObjectKind::ArrayIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
        state.next_index = index + 1.0;
    }
    let number = Value::number(index);
    let result = match kind {
        IterationKind::Keys => number,
        IterationKind::Values | IterationKind::Entries => {
            let key = rt.index_key(index)?;
            let element = rt.get_value(target.into(), key)?;
            if kind == IterationKind::Values {
                element
            } else {
                rt.pair_array(number, element)?.into()
            }
        }
    };
    Ok(Some(result))
}

fn set_target(rt: &mut Runtime, iterator: Gc<Object>, target: Option<Gc<Object>>) -> VmResult<()> {
    if let ObjectKind::ArrayIterator(state) = &mut rt.heap.object_mut(iterator)?.kind {
        state.target = target;
    }
    Ok(())
}

/// V8's `TypeError` for `next` on another receiver.
fn incompatible(rt: &Runtime, receiver: Value) -> VmError {
    VmError::type_error(format!(
        "Method Array Iterator.prototype.next called on incompatible receiver {}",
        match receiver {
            Value::Object(object) => rt.object_text(object),
            _ => rt.primitive_text(receiver),
        }
    ))
}
