//! `Array` (ECMA-262 §23.1, <https://tc39.es/ecma262/#sec-array-objects>):
//! the constructor, `Array.isArray`, `Array.from`, `Array.of`, the
//! installation of `Array.prototype` in Chromium 148's key order, and the
//! helpers that the method modules share. The methods are in
//! `array_iteration.rs` (the callback methods), `array_search.rs`
//! (searches, `at`, `join`), `array_mutate.rs` (the methods that change
//! the receiver), `array_copy.rs` (the methods that make a new array) and
//! `array_sort.rs`; the element loops are in `array_access.rs`.
//!
//! The methods are generic: they work on any object with a `length`
//! (`LengthOfArrayLike`, up to 2^53 − 1; the indices from 2^32 − 1 on are
//! ordinary string keys). They follow the specification's order of
//! conversions, reads and writes, and V8's messages (measured in Chromium
//! 148, `tools/probes/js-arrays.json`). Where the receiver is an array
//! with dense elements and the result is the same, they work on the
//! element vector; on sparse elements they visit only the stored indices
//! (`array_access.rs`). Each visited index is a step of the time
//! countdown.

use crate::heap::Gc;
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::{
    MAX_SAFE_INTEGER, checked_array_length, clamp_from_end, to_integer_or_infinity,
};
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

use super::array_access::{Flow, Item};

/// The most elements that a method reserves before it has read them.
pub(super) const INITIAL_CAPACITY: usize = 1 << 12;

/// `Array.prototype`'s methods in Chromium 148's key order (after
/// `length` and `constructor`), with their `length`.
const METHODS: [(&str, u32, NativeFn); 38] = [
    ("at", 1, super::array_search::at),
    ("concat", 1, super::array_copy::concat),
    ("copyWithin", 2, super::array_mutate::copy_within),
    ("fill", 1, super::array_mutate::fill),
    ("find", 1, super::array_iteration::find),
    ("findIndex", 1, super::array_iteration::find_index),
    ("findLast", 1, super::array_iteration::find_last),
    ("findLastIndex", 1, super::array_iteration::find_last_index),
    ("lastIndexOf", 1, super::array_search::last_index_of),
    ("pop", 0, super::array_mutate::pop),
    ("push", 1, super::array_mutate::push),
    ("reverse", 0, super::array_mutate::reverse),
    ("shift", 0, super::array_mutate::shift),
    ("unshift", 1, super::array_mutate::unshift),
    ("slice", 2, super::array_copy::slice),
    ("sort", 1, super::array_sort::sort),
    ("splice", 2, super::array_mutate::splice),
    ("includes", 1, super::array_search::includes),
    ("indexOf", 1, super::array_search::index_of),
    ("join", 1, super::array_search::join),
    ("keys", 0, super::iterator::keys),
    ("entries", 0, super::iterator::entries),
    ("values", 0, super::iterator::values),
    ("forEach", 1, super::array_iteration::for_each),
    ("filter", 1, super::array_iteration::filter),
    ("flat", 0, super::array_copy::flat),
    ("flatMap", 1, super::array_copy::flat_map),
    ("map", 1, super::array_iteration::map),
    ("every", 1, super::array_iteration::every),
    ("some", 1, super::array_iteration::some),
    ("reduce", 1, super::array_iteration::reduce),
    ("reduceRight", 1, super::array_iteration::reduce_right),
    ("toReversed", 0, super::array_copy::to_reversed),
    ("toSorted", 1, super::array_sort::to_sorted),
    ("toSpliced", 2, super::array_copy::to_spliced),
    ("with", 2, super::array_copy::with),
    ("toLocaleString", 0, super::array_search::to_locale_string),
    ("toString", 0, super::array_search::to_string),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ArrayPrototype)?;
    let array = super::constructor(rt, realm, "Array", 1, array_constructor, proto)?;
    rt.set_intrinsic(realm, Intrinsic::ArrayConstructor, array);
    super::method(rt, realm, array, "isArray", 1, is_array)?;
    super::method(rt, realm, array, "from", 1, from)?;
    super::method(
        rt,
        realm,
        array,
        "fromAsync",
        1,
        super::array_from_async::from_async,
    )?;
    super::method(rt, realm, array, "of", 0, of)?;
    super::array_species::install_species(rt, realm, array)?;
    for (name, length, func) in METHODS {
        let function = super::method(rt, realm, proto, name, length, func)?;
        if name == "values" {
            // §23.1.3.40: `@@iterator` is the same function object.
            rt.set_intrinsic(realm, Intrinsic::ArrayPrototypeValues, function);
        }
    }
    let values = rt.intrinsic(realm, Intrinsic::ArrayPrototypeValues)?;
    let key = rt.symbol_key(WellKnown::Iterator);
    rt.define(proto, key, values.into(), true, false, true)?;
    super::array_species::install_unscopables(rt, proto)
}

// --- Helpers of the method modules ---

/// `ToObject(this)` and `LengthOfArrayLike` (§7.3.18), the first steps
/// of most methods. With `name`, a `null` or `undefined` receiver gets
/// V8's message "Array.prototype.NAME called on null or undefined",
/// otherwise "Cannot convert undefined or null to object".
pub(super) fn receiver(
    rt: &mut Runtime,
    call: &NativeCall,
    name: Option<&str>,
) -> VmResult<(Gc<Object>, f64)> {
    let object = match name {
        Some(name) => rt.this_object_checked(call, &format!("Array.prototype.{name}"))?,
        None => rt.this_object(call)?,
    };
    let length = rt.length_of_array_like(object)?;
    Ok((object, length))
}

/// Argument `index` if it is callable, else V8's `TypeError` ("number 1
/// is not a function").
pub(super) fn callback_arg(rt: &mut Runtime, call: &NativeCall, index: usize) -> VmResult<Value> {
    let callback = rt.arg(call, index);
    if !rt.is_callable(callback)? {
        return Err(rt.not_callable(callback));
    }
    Ok(callback)
}

/// A relative index argument of `slice` and the like (§23.1.3.28 steps 3
/// to 5): `ToIntegerOrInfinity`, counted from the end if negative,
/// clamped to `0..=length`; `default` for `undefined` where the step says
/// so.
pub(super) fn relative_index(
    rt: &mut Runtime,
    value: Value,
    length: f64,
    default: Option<f64>,
) -> VmResult<f64> {
    if let (Value::Undefined, Some(default)) = (value, default) {
        return Ok(default);
    }
    let relative = to_integer_or_infinity(rt.to_number(value)?);
    Ok(clamp_from_end(relative, length))
}

/// The `TypeError` of a result length above 2^53 − 1 (V8's message for
/// `unshift`, `splice`, `toSpliced` and `concat`).
pub(super) fn length_overflow() -> VmError {
    VmError::type_error(crate::object::INVALID_ARRAY_LENGTH)
}

/// Checks that `length` does not pass 2^53 − 1.
pub(super) fn check_safe_length(length: f64) -> VmResult<()> {
    if length > MAX_SAFE_INTEGER {
        return Err(length_overflow());
    }
    Ok(())
}

impl Runtime {
    /// A new array of the current realm with room for `capacity` elements
    /// (the dense builder, `object/element_access.rs`), recorded in the
    /// current handle scope.
    pub(super) fn new_array_with_capacity(&mut self, capacity: usize) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(self.current_realm(), Intrinsic::ArrayPrototype)?;
        let array = self
            .heap
            .new_array_with_capacity(Some(proto), capacity, &self.vm)?;
        Ok(self.heap.record(array))
    }

    /// `CreateArrayFromList(values)` (§7.3.17) in the current realm, in one
    /// step, recorded in the current handle scope. The values must be
    /// alive (rooted by the caller); they stay alive during the
    /// reservation.
    pub(crate) fn array_from_values(&mut self, values: &[Value]) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(self.current_realm(), Intrinsic::ArrayPrototype)?;
        let array = self.heap.new_array_from(Some(proto), values, &self.vm)?;
        Ok(self.heap.record(array))
    }

    /// Appends `value` to `array` (`CreateDataPropertyOrThrow(array,
    /// length, value)` for an array that the native made): the dense
    /// builder, or the general definition for another form.
    pub(super) fn push_value(&mut self, array: Gc<Object>, value: Value) -> VmResult<()> {
        if self.heap.push_element(array, value)? {
            return Ok(());
        }
        let (length, _) = self.heap.array_length(array)?;
        self.create_index(array, f64::from(length), value)
    }

    /// Whether `object` is an `Array` whose `constructor` is the `Array`
    /// of the current realm and has the built-in `@@species`, so that
    /// `ArraySpeciesCreate` gives a plain new array: then the method may
    /// build its result with the dense builder. Reads only data
    /// properties; any other form takes the specification's steps.
    pub(super) fn default_species(&mut self, object: Gc<Object>) -> VmResult<bool> {
        if !matches!(self.heap.object(object)?.kind, ObjectKind::Array { .. }) {
            return Ok(false);
        }
        let key = PropertyKey::String(self.vm.atoms.constructor);
        let constructor = match self.heap.get(object, key, object.into())? {
            crate::object::GetResult::Value(value) => value,
            crate::object::GetResult::CallGetter { .. } | crate::object::GetResult::Generic => {
                return Ok(false);
            }
        };
        let realm = self.current_realm();
        let array = self.intrinsic(realm, Intrinsic::ArrayConstructor)?;
        if constructor != Value::Object(array) {
            return Ok(false);
        }
        let species = self.symbol_key(WellKnown::Species);
        match self.heap.get_own_property(array, species)? {
            Some(crate::object::Property::Accessor { get, .. }) => {
                Ok(get == Value::Object(self.intrinsic(realm, Intrinsic::ArraySpeciesGetter)?))
            }
            _ => Ok(false),
        }
    }
}

// --- The constructor and the static functions ---

/// `Array(...values)` (§23.1.1.1), with and without `new`.
fn array_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let proto = rt.prototype_from(call, Intrinsic::ArrayPrototype)?;
    let argc = call.argc();
    if argc == 1 {
        let length = rt.arg(call, 0);
        if let Some(number) = length.as_number() {
            let array = rt
                .heap
                .new_array(Some(proto), checked_array_length(number)?)?;
            return Ok(NativeReturn::Value(array.into()));
        }
    }
    let args: Vec<Value> = (0..argc).map(|i| rt.arg(call, i)).collect();
    rt.charge(argc)?;
    let array = rt.heap.new_array_from(Some(proto), &args, &rt.vm)?;
    Ok(NativeReturn::Value(array.into()))
}

/// `Array.isArray(arg)` (§23.1.2.2): `IsArray`, through proxies.
fn is_array(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let result = rt.is_array(rt.arg(call, 0))?;
    Ok(NativeReturn::Value(Value::Bool(result)))
}

/// `Array.of(...items)` (§23.1.2.3).
fn of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let len = call.argc();
    let constructor = call.this();
    let realm_array = rt.intrinsic(rt.current_realm(), Intrinsic::ArrayConstructor)?;
    if constructor == Value::Object(realm_array) || !rt.is_constructor(constructor)? {
        let args: Vec<Value> = (0..len).map(|i| rt.arg(call, i)).collect();
        rt.charge(len)?;
        let array = rt.array_from_values(&args)?;
        return Ok(NativeReturn::Value(array.into()));
    }
    let result = rt.construct(constructor, &[Value::number(len as f64)])?;
    let Value::Object(object) = result else {
        return Err(VmError::invariant("a constructor returned a non-object"));
    };
    for k in 0..len {
        rt.tick()?;
        let value = rt.arg(call, k);
        rt.create_index(object, k as f64, value)?;
    }
    rt.set_length(object, len as f64)?;
    Ok(NativeReturn::Value(result))
}

/// The `TypeError` of an `@@iterator` that is not callable (V8's message).
fn from_iterator_error() -> VmError {
    VmError::type_error(
        "%Array%.from requires that the property of the first argument, items[Symbol.iterator], when exists, be a function",
    )
}

/// `Array.from(items, mapfn, thisArg)` (§23.1.2.1).
fn from(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let constructor = call.this();
    let items = rt.arg(call, 0);
    let mapfn = rt.arg(call, 1);
    let this_arg = rt.arg(call, 2);
    let mapping = if mapfn.is_undefined() {
        None
    } else if rt.is_callable(mapfn)? {
        Some(mapfn)
    } else {
        return Err(rt.not_callable(mapfn));
    };
    if items.is_nullish() {
        return Err(rt.not_iterable(items));
    }
    let key = rt.symbol_key(WellKnown::Iterator);
    let method = rt.get_value(items, key)?;
    let realm_array = rt.intrinsic(rt.current_realm(), Intrinsic::ArrayConstructor)?;
    let plain = constructor == Value::Object(realm_array) || !rt.is_constructor(constructor)?;
    if !method.is_nullish() {
        if !rt.is_callable(method)? {
            return Err(from_iterator_error());
        }
        let target = if plain {
            rt.new_array_with_capacity(0)?
        } else {
            match rt.construct(constructor, &[])? {
                Value::Object(object) => object,
                _ => return Err(VmError::invariant("a constructor returned a non-object")),
            }
        };
        from_iterable(rt, target, items, method, mapping, this_arg)?;
        return Ok(NativeReturn::Value(target.into()));
    }
    let array_like = rt.to_object(items)?;
    rt.heap.record(array_like);
    let len = rt.length_of_array_like(array_like)?;
    let target = if plain {
        checked_array_length(len)?;
        // The vector grows while the loop runs: a getter can still throw
        // before a long array-like is read.
        rt.new_array_with_capacity((len as usize).min(INITIAL_CAPACITY))?
    } else {
        match rt.construct(constructor, &[Value::number(len)])? {
            Value::Object(object) => object,
            _ => return Err(VmError::invariant("a constructor returned a non-object")),
        }
    };
    rt.for_index(array_like, 0.0, len, false, |rt, item| {
        let (first, end, value) = match item {
            Item::Value(index, value) => (index, index + 1.0, value),
            Item::Gap(from, to) => (from, to, Value::Undefined),
        };
        for index in first as u64..end as u64 {
            if index != first as u64 {
                rt.tick()?;
            }
            let index = index as f64;
            let mapped = match mapping {
                Some(mapfn) => rt.call(mapfn, this_arg, &[value, Value::number(index)])?,
                None => value,
            };
            if plain && rt.heap.push_element(target, mapped)? {
                continue;
            }
            rt.create_index(target, index, mapped)?;
        }
        Ok(Flow::Continue)
    })?;
    rt.set_length(target, len)?;
    Ok(NativeReturn::Value(target.into()))
}

/// Steps 5.c to 5.e of `Array.from` for an iterable: the loop over the
/// iterator, which closes the iterator when a step after `next` fails.
fn from_iterable(
    rt: &mut Runtime,
    target: Gc<Object>,
    items: Value,
    method: Value,
    mapping: Option<Value>,
    this_arg: Value,
) -> VmResult<()> {
    let mut record = rt.get_iterator_from_method(items, method)?;
    let builtin = rt.builtin_array_step(record.iterator, record.next_method);
    if let (Some(iterator), None) = (builtin, mapping)
        && rt
            .heap
            .dense_array(target)?
            .is_some_and(|state| state.hole_free() && state.writable())
        && rt.spread_array_iterator(target, iterator)?
    {
        let (length, _) = rt.heap.array_length(target)?;
        return rt.set_length(target, f64::from(length));
    }
    let mut k = 0.0;
    loop {
        rt.tick()?;
        let done = rt.with_scope(|rt| {
            if k >= MAX_SAFE_INTEGER {
                let error = VmError::type_error(crate::object::INVALID_ARRAY_LENGTH);
                return Err(rt.iterator_close_after_error(&record, error));
            }
            let next = match builtin {
                Some(iterator) => super::array_iterator_next_value(rt, iterator)?,
                None => rt.iterator_step_value(&mut record)?,
            };
            let Some(next) = next else {
                return Ok(true);
            };
            let mapped = match mapping {
                Some(mapfn) => match rt.call(mapfn, this_arg, &[next, Value::number(k)]) {
                    Ok(value) => value,
                    Err(error) => return Err(rt.iterator_close_after_error(&record, error)),
                },
                None => next,
            };
            if rt.heap.push_element(target, mapped)? {
                return Ok(false);
            }
            match rt.create_index(target, k, mapped) {
                Ok(()) => Ok(false),
                Err(error) => Err(rt.iterator_close_after_error(&record, error)),
            }
        })?;
        if done {
            return rt.set_length(target, k);
        }
        k += 1.0;
    }
}
