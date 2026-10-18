//! The typed arrays (ECMA-262 §23.2,
//! <https://tc39.es/ecma262/#sec-typedarray-objects>): `%TypedArray%` (an
//! abstract constructor: a call or a `new` of it throws) with `from`, `of`
//! and `@@species`, and the 12 constructors `Int8Array` to `BigUint64Array`
//! (Table 71) with `BYTES_PER_ELEMENT` on each constructor and prototype.
//! The prototype is in `typed_array_proto.rs`, the creation steps in
//! `typed_array_create.rs`, the exotic internal methods in
//! `object/typed_array.rs` and `vm/typed_array.rs`.
//!
//! The keys are in Chromium 148's order and the messages are V8's
//! (measured with `tools/probes/js-typed.json`). The constructors take
//! all forms of §23.2.5.1: no argument, a length (`ToIndex`), a typed
//! array (with the content-type check between `BigInt` and Number
//! arrays), an `ArrayBuffer` with a byte offset and a length (or none,
//! which tracks the length of a resizable buffer), an iterable and an
//! array-like.

use crate::heap::Gc;
use crate::object::{ElementType, Object};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::TYPED_ARRAY_TYPES;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

use super::typed_array_create::{CreateArgs, invalid_length, type_name};

/// The constructors in the order of Table 71, with their natives.
const CONSTRUCTORS: [(ElementType, NativeFn); TYPED_ARRAY_TYPES] = [
    (ElementType::Int8, |rt, call| {
        construct(rt, call, ElementType::Int8)
    }),
    (ElementType::Uint8, |rt, call| {
        construct(rt, call, ElementType::Uint8)
    }),
    (ElementType::Uint8Clamped, |rt, call| {
        construct(rt, call, ElementType::Uint8Clamped)
    }),
    (ElementType::Int16, |rt, call| {
        construct(rt, call, ElementType::Int16)
    }),
    (ElementType::Uint16, |rt, call| {
        construct(rt, call, ElementType::Uint16)
    }),
    (ElementType::Int32, |rt, call| {
        construct(rt, call, ElementType::Int32)
    }),
    (ElementType::Uint32, |rt, call| {
        construct(rt, call, ElementType::Uint32)
    }),
    (ElementType::BigInt64, |rt, call| {
        construct(rt, call, ElementType::BigInt64)
    }),
    (ElementType::BigUint64, |rt, call| {
        construct(rt, call, ElementType::BigUint64)
    }),
    (ElementType::Float16, |rt, call| {
        construct(rt, call, ElementType::Float16)
    }),
    (ElementType::Float32, |rt, call| {
        construct(rt, call, ElementType::Float32)
    }),
    (ElementType::Float64, |rt, call| {
        construct(rt, call, ElementType::Float64)
    }),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::TypedArrayPrototype)?;
    let abstract_ctor = rt.new_builtin(realm, "TypedArray", 0, abstract_constructor, true)?;
    rt.heap.record(abstract_ctor);
    rt.set_intrinsic(realm, Intrinsic::TypedArrayConstructor, abstract_ctor);
    // Chromium 148's order: length, name, prototype, of, from, @@species.
    let key = PropertyKey::String(rt.vm.atoms.prototype);
    rt.define(abstract_ctor, key, proto.into(), false, false, false)?;
    super::method(rt, realm, abstract_ctor, "of", 0, of)?;
    super::method(rt, realm, abstract_ctor, "from", 1, from)?;
    let species = rt.new_builtin(realm, "get [Symbol.species]", 0, species, false)?;
    rt.set_intrinsic(realm, Intrinsic::TypedArraySpeciesGetter, species);
    let key = rt.symbol_key(WellKnown::Species);
    let desc =
        crate::object::PropertyDescriptor::accessor(species.into(), Value::Undefined, false, true);
    rt.define_own_property(abstract_ctor, key, desc)?;
    super::typed_array_proto::install(rt, realm, abstract_ctor, proto)?;
    for (ty, func) in CONSTRUCTORS {
        install_constructor(rt, realm, ty, func, abstract_ctor)?;
    }
    Ok(())
}

/// One of the 12 constructors and its prototype (§23.2.6, §23.2.7): the
/// constructor inherits from `%TypedArray%`, the prototype from
/// `%TypedArray.prototype%`.
fn install_constructor(
    rt: &mut Runtime,
    realm: u32,
    ty: ElementType,
    func: NativeFn,
    abstract_ctor: Gc<Object>,
) -> VmResult<()> {
    let typed_proto = rt.intrinsic(realm, Intrinsic::TypedArrayPrototype)?;
    let proto = rt.heap.new_object(Some(typed_proto))?;
    rt.heap.record(proto);
    let name = type_name(ty);
    let ctor = rt.new_builtin_with_proto(realm, abstract_ctor, &name, 3, func, true)?;
    let key = PropertyKey::String(rt.vm.atoms.prototype);
    rt.define(ctor, key, proto.into(), false, false, false)?;
    let key = PropertyKey::String(rt.vm.atoms.constructor);
    rt.define(proto, key, ctor.into(), true, false, true)?;
    super::global(rt, realm, &name, ctor.into())?;
    let size = Value::Int(ty.size() as i32);
    let key = rt.heap.key_from_str("BYTES_PER_ELEMENT")?;
    rt.define(ctor, key, size, false, false, false)?;
    rt.define(proto, key, size, false, false, false)?;
    rt.set_typed_array_intrinsics(realm, ty, (ctor, proto));
    Ok(())
}

/// `%TypedArray%()` (§23.2.1.1): an abstract class.
fn abstract_constructor(_: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    Err(VmError::type_error(
        "Abstract class TypedArray not directly constructable",
    ))
}

/// `get %TypedArray%[@@species]` (§23.2.2.4): the receiver.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn species(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

/// `TypedArray(...args)` (§23.2.5.1) for the element type `ty`.
fn construct(rt: &mut Runtime, call: &NativeCall, ty: ElementType) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error(format!(
            "Constructor {} requires 'new'",
            type_name(ty)
        )));
    }
    let realm = call.realm();
    let first = rt.arg(call, 0);
    let array = match first {
        _ if call.argc() == 0 => {
            let proto = rt.typed_prototype_from(call, ty)?;
            rt.allocate_typed_array(realm, proto, ty, 0)?.0
        }
        Value::Object(object) => {
            let proto = rt.typed_prototype_from(call, ty)?;
            if let Some(source) = rt.heap.typed_array(object)? {
                rt.typed_from_typed(realm, proto, ty, source)?
            } else if rt.heap.array_buffer(object)?.is_some() {
                let (offset, length) = (rt.arg(call, 1), rt.arg(call, 2));
                rt.typed_from_buffer(proto, ty, object, offset, length)?
            } else {
                match rt.typed_iterator_method(first, "TypedArray's constructor")? {
                    Some(method) => {
                        let list = rt.iterable_to_list(first, method)?;
                        rt.typed_from_list(realm, proto, ty, list)?
                    }
                    None => rt.typed_from_array_like(realm, proto, ty, object)?,
                }
            }
        }
        _ => {
            let length = rt.index_or_number(first)?.map_err(invalid_length)?;
            let proto = rt.typed_prototype_from(call, ty)?;
            rt.allocate_typed_array(realm, proto, ty, length)?.0
        }
    };
    Ok(NativeReturn::Value(array.into()))
}

impl Runtime {
    /// `GetPrototypeFromConstructor(new.target, "%TypedArray.prototype%")`
    /// of the constructor of `ty` (§10.1.14).
    fn typed_prototype_from(&mut self, call: &NativeCall, ty: ElementType) -> VmResult<Gc<Object>> {
        self.prototype_from_with(call, |rt, realm| {
            Ok(rt.typed_array_intrinsics(realm, ty)?.1)
        })
    }
}

/// The constructor argument of `from` and `of` (step 1 and 2): `this` if
/// it is a constructor, else V8's `TypeError`.
fn constructor_this(rt: &Runtime, call: &NativeCall) -> VmResult<Value> {
    let constructor = call.this();
    if !rt.is_constructor(constructor)? {
        return Err(VmError::type_error(format!(
            "{} is not a constructor",
            rt.primitive_text(constructor)
        )));
    }
    Ok(constructor)
}

/// The values of `from`: the list of an iterable or an array-like object.
#[derive(Clone, Copy)]
enum Source {
    List(Gc<Object>),
    ArrayLike(Gc<Object>),
}

/// `%TypedArray%.from(source [, mapfn [, thisArg]])` (§23.2.2.1): the
/// values of an iterable are all read before the array is made, those of an
/// array-like one by one after.
fn from(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "%TypedArray%.from";
    let constructor = constructor_this(rt, call)?;
    let (source, mapfn, this_arg) = (rt.arg(call, 0), rt.arg(call, 1), rt.arg(call, 2));
    let mapping = if mapfn.is_undefined() {
        None
    } else if rt.is_callable(mapfn)? {
        Some(mapfn)
    } else {
        return Err(rt.not_callable(mapfn));
    };
    if source.is_nullish() {
        return Err(rt.not_iterable(source));
    }
    // The values of an iterable (`IteratorToList`, in an internal array)
    // or the array-like object (`ToObject(source)`).
    let (values, length) = if let Some(method) = rt.typed_iterator_method(source, NAME)? {
        let list = rt.iterable_to_list(source, method)?;
        let (length, _) = rt.heap.array_length(list)?;
        (Source::List(list), u64::from(length))
    } else {
        let array_like = rt.to_object(source)?;
        rt.heap.record(array_like);
        let length = rt.length_of_array_like(array_like)? as u64;
        (Source::ArrayLike(array_like), length)
    };
    let wanted = usize::try_from(length).map_err(|_| invalid_length(length as f64))?;
    let (target, data) =
        rt.typed_create_from_constructor(constructor, CreateArgs::Length(wanted), NAME)?;
    for k in 0..length {
        rt.tick()?;
        rt.with_scope(|rt| {
            let value = match values {
                Source::List(list) => rt
                    .heap
                    .dense_elements(list)?
                    .and_then(|values| values.get(k as usize).copied())
                    .ok_or(VmError::invariant("a list with a hole"))?,
                Source::ArrayLike(array_like) => {
                    let key = rt.index_key(k as f64)?;
                    rt.get_value(array_like.into(), key)?
                }
            };
            let mapped = match mapping {
                Some(mapfn) => rt.call(mapfn, this_arg, &[value, Value::number(k as f64)])?,
                None => value,
            };
            rt.typed_set_element(&data, k as f64, mapped)
        })?;
    }
    Ok(NativeReturn::Value(target.into()))
}

/// `%TypedArray%.of(...items)` (§23.2.2.2).
fn of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "%TypedArray%.of";
    let length = call.argc();
    let constructor = constructor_this(rt, call)?;
    let (target, data) =
        rt.typed_create_from_constructor(constructor, CreateArgs::Length(length), NAME)?;
    for k in 0..length {
        rt.tick()?;
        let value = rt.arg(call, k);
        rt.with_scope(|rt| rt.typed_set_element(&data, k as f64, value))?;
    }
    Ok(NativeReturn::Value(target.into()))
}
