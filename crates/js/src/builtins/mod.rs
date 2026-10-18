//! The built-in objects (ECMA-262 clauses 19 to 28).
//!
//! Implemented so far: `console.log`; `Object` (all static functions,
//! `Object.prototype` with Annex B) and `Reflect`; the `Error`
//! constructors, `AggregateError` and the V8 extensions
//! `Error.captureStackTrace` and `Error.stackTraceLimit`; `Array`
//! complete (`array*.rs`; `Array.fromAsync` in `array_from_async.rs`);
//! `Function.prototype` with `call`, `apply`, `bind`, `toString`,
//! `@@hasInstance` and the restricted `caller` and `arguments` (the
//! `Function` constructor is in `vm/create_function.rs`);
//! `GeneratorFunction`, its prototype and `%GeneratorPrototype%`
//! (`generator.rs`; the generator objects run in `vm/generator.rs`);
//! `AsyncFunction` and its prototype (`async_function.rs`; async
//! functions run in `vm/async_function.rs`); `AsyncGeneratorFunction`,
//! its prototype, `%AsyncGeneratorPrototype%`, `%AsyncIteratorPrototype%`
//! and `%AsyncFromSyncIteratorPrototype%` (`async_generator.rs`; the
//! objects run in `vm/async_generator.rs` and `vm/async_iteration.rs`); the
//! global
//! functions `eval`, `isNaN`, `isFinite`, `parseFloat` and `parseInt`;
//! `Number` and `Math` complete (`number.rs`, `math.rs`); `String`
//! complete, with the string iterator, the URI functions and Annex B
//! (`string*.rs`, `uri.rs`); `RegExp` with the legacy static properties,
//! Annex B `compile` and `%RegExpStringIteratorPrototype%`
//! (`regexp*.rs`; the matcher is the crate `js-regexp`); `Boolean`; `BigInt`
//! (`bigint.rs`); `ArrayBuffer` and `DataView` (`array_buffer.rs`,
//! `data_view.rs`); the typed arrays (`typed_array*.rs`; the methods of
//! `%TypedArray%.prototype` other than `set`, `subarray`, `slice` and
//! the iterators come with M7 feature 11c);
//! `Symbol` with the well-known symbols; `JSON` (`json*.rs`); `Map` and
//! `Set` with the ES2025 set methods (`map.rs`, `set.rs`,
//! `set_methods.rs`, `collection.rs`); `WeakMap`, `WeakSet`, `WeakRef`
//! and `FinalizationRegistry` (`weak.rs`, `weak_ref.rs`); `Promise` (`promise.rs`,
//! `promise_combinators.rs`; the promise objects and jobs are in
//! `vm/promise.rs` and `vm/jobs.rs`); and `%IteratorPrototype%`
//! and `%ArrayIteratorPrototype%`. Each function
//! has the `name` and `length` of the specification; methods and
//! constructors are writable, non-enumerable and configurable properties
//! (clause 18).
//!
//! Natives that call back into script code (`forEach`, `map`, `join`
//! through `toString`, and the key loops of `Object` that read getters)
//! open a handle scope per element, so that their scope does not grow
//! with the length of the array, and step the time countdown once per
//! visited element (ADR 0026 section 9; the array methods skip holes where
//! nothing can observe it, `array_access.rs`). `Function.prototype.call`,
//! `apply`, `Reflect.apply`, `Reflect.construct` and
//! `Array.prototype.toString` end in a deferred call, without Rust
//! recursion.

mod array;
mod array_access;
mod array_buffer;
mod array_copy;
mod array_from_async;
mod array_iteration;
mod array_mutate;
mod array_search;
mod array_sort;
mod array_species;
mod async_function;
mod async_generator;
mod bigint;
mod boolean;
mod collection;
mod console;
mod data_view;
mod descriptor;
mod error;
mod function;
mod generator;
mod global;
mod integrity;
mod iterator;
mod iterator_consume;
mod iterator_from;
mod iterator_helper;
mod json;
mod json_parse;
mod json_revive;
mod json_stringify;
mod map;
mod math;
mod number;
mod object;
mod object_iter;
mod object_proto;
mod primitive;
mod promise;
mod promise_combinators;
mod proxy;
mod reflect;
mod regexp;
mod regexp_exec;
mod regexp_fast;
mod regexp_legacy;
mod regexp_symbol;
mod set;
mod set_methods;
mod string;
mod string_annexb;
mod string_iter;
mod string_match;
mod string_replace;
mod string_search;
mod string_unicode;
mod symbol;
mod typed_array;
mod typed_array_create;
mod typed_array_proto;
mod uri;
mod weak;
mod weak_ref;

pub use array_from_async::FromAsync;
pub(crate) use error::error_header;
pub(crate) use iterator::array_iterator_next_value;

use crate::heap::Gc;
use crate::object::{GetResult, Object, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::{checked_array_length, to_length};
use crate::vm::{
    Intrinsic, NativeCall, NativeFn, STACK_OVERFLOW, SetOutcome, VmError, VmResult, WELL_KNOWN,
    WellKnown,
};

/// The next value of a built-in Array, Map or Set Iterator or an iterator
/// helper, or `None`
/// when it is finished (`next` without its result object; see
/// `Runtime::builtin_step`). The value is recorded in the current handle
/// scope.
pub(crate) fn builtin_iterator_next_value(
    rt: &mut Runtime,
    iterator: Gc<Object>,
) -> VmResult<Option<Value>> {
    match rt.heap.object(iterator)?.kind {
        crate::object::ObjectKind::ArrayIterator(_) => array_iterator_next_value(rt, iterator),
        crate::object::ObjectKind::IteratorHelper(_) => iterator_helper::next_value(rt, iterator),
        _ => collection::collection_iterator_next_value(rt, iterator),
    }
}

/// Installs the built-in objects of a realm (after its intrinsics exist
/// and the realm is a root).
pub(crate) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    rt.scoped(|rt| install_all(rt, realm))
}

fn install_all(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    object::install(rt, realm)?;
    reflect::install(rt, realm)?;
    proxy::install(rt, realm)?;
    let (parse_int, parse_float) = global::install(rt, realm)?;
    math::install(rt, realm)?;
    let function_constructor = function::install(rt, realm)?;
    error::install(rt, realm)?;
    array::install(rt, realm)?;
    primitive::install(rt, realm)?;
    regexp::install(rt, realm)?;
    uri::install(rt, realm)?;
    number::install(rt, realm, parse_int, parse_float)?;
    boolean::install(rt, realm)?;
    bigint::install(rt, realm)?;
    array_buffer::install(rt, realm)?;
    data_view::install(rt, realm)?;
    typed_array::install(rt, realm)?;
    symbol::install(rt, realm)?;
    iterator::install(rt, realm)?;
    map::install(rt, realm)?;
    set::install(rt, realm)?;
    weak::install(rt, realm)?;
    weak_ref::install(rt, realm)?;
    json::install(rt, realm)?;
    console::install(rt, realm)?;
    promise::install(rt, realm)?;
    generator::install(rt, realm, function_constructor)?;
    async_function::install(rt, realm, function_constructor)?;
    async_generator::install(rt, realm, function_constructor)
}

// --- Helpers for the definitions ---

/// Defines a built-in method on `target`: a native function as a
/// writable, non-enumerable, configurable property.
fn method(
    rt: &mut Runtime,
    realm: u32,
    target: Gc<Object>,
    name: &str,
    length: u32,
    func: NativeFn,
) -> VmResult<Gc<Object>> {
    let function = rt.new_builtin(realm, name, length, func, false)?;
    let key = rt.heap.key_from_str(name)?;
    rt.define(target, key, function.into(), true, false, true)?;
    Ok(function)
}

/// Defines a method whose key is a well-known symbol (the function is
/// named "[Symbol.iterator]" and so on). `attributes` is (writable,
/// configurable).
fn symbol_method(
    rt: &mut Runtime,
    realm: u32,
    target: Gc<Object>,
    which: WellKnown,
    length: u32,
    func: NativeFn,
    attributes: (bool, bool),
) -> VmResult<Gc<Object>> {
    let name = WELL_KNOWN
        .iter()
        .find(|(known, _)| *known == which)
        .map_or("", |(_, name)| *name);
    let function = rt.new_builtin(realm, &format!("[Symbol.{name}]"), length, func, false)?;
    let key = rt.symbol_key(which);
    rt.define(
        target,
        key,
        function.into(),
        attributes.0,
        false,
        attributes.1,
    )?;
    Ok(function)
}

/// Defines an accessor with a getter only (non-enumerable, configurable),
/// named "get NAME"; returns the getter.
fn getter(
    rt: &mut Runtime,
    realm: u32,
    target: Gc<Object>,
    name: &str,
    func: NativeFn,
) -> VmResult<Gc<Object>> {
    let function = rt.new_builtin(realm, &format!("get {name}"), 0, func, false)?;
    let key = rt.heap.key_from_str(name)?;
    let desc = PropertyDescriptor::accessor(function.into(), Value::Undefined, false, true);
    rt.define_own_property(target, key, desc)?;
    Ok(function)
}

/// Defines `@@toStringTag` (non-writable, non-enumerable, configurable).
pub(crate) fn to_string_tag(rt: &mut Runtime, target: Gc<Object>, tag: &str) -> VmResult<()> {
    let text = rt.heap.intern_str(tag)?;
    rt.heap.record(text);
    let key = rt.symbol_key(WellKnown::ToStringTag);
    rt.define(target, key, text.into(), false, false, true)
}

/// Defines a property of the global object (writable, non-enumerable,
/// configurable, as the global properties of clause 19).
fn global(rt: &mut Runtime, realm: u32, name: &str, value: Value) -> VmResult<()> {
    let global = rt.global_object(realm)?;
    let key = rt.heap.key_from_str(name)?;
    rt.define(global, key, value, true, false, true)
}

/// Creates a constructor: a native function whose `prototype` is
/// `prototype` (fixed) and whose prototype's `constructor` is the
/// function; installs it as a global.
fn constructor(
    rt: &mut Runtime,
    realm: u32,
    name: &str,
    length: u32,
    func: NativeFn,
    prototype: Gc<Object>,
) -> VmResult<Gc<Object>> {
    let function = rt.new_builtin(realm, name, length, func, true)?;
    let key = PropertyKey::String(rt.vm.atoms.prototype);
    rt.define(function, key, prototype.into(), false, false, false)?;
    let key = PropertyKey::String(rt.vm.atoms.constructor);
    rt.define(prototype, key, function.into(), true, false, true)?;
    global(rt, realm, name, function.into())?;
    Ok(function)
}

// --- Helpers for the natives ---

impl Runtime {
    /// `GetPrototypeFromConstructor` (§10.1.14) for a native constructor:
    /// the `prototype` of `new.target` (the callee for a call without
    /// `new`) if it is an object, else the intrinsic of the callee's
    /// realm.
    fn prototype_from(&mut self, call: &NativeCall, fallback: Intrinsic) -> VmResult<Gc<Object>> {
        self.prototype_from_with(call, |rt, realm| rt.intrinsic(realm, fallback))
    }

    /// [`Runtime::prototype_from`] with the fallback prototype of a realm
    /// given by `fallback` (for the prototypes outside the intrinsics
    /// table, `Realm::typed_arrays`).
    fn prototype_from_with(
        &mut self,
        call: &NativeCall,
        fallback: impl FnOnce(&Runtime, u32) -> VmResult<Gc<Object>>,
    ) -> VmResult<Gc<Object>> {
        let target = match call.new_target() {
            Value::Undefined => Value::Object(call.callee()),
            target => target,
        };
        let key = PropertyKey::String(self.vm.atoms.prototype);
        if let Value::Object(proto) = self.get_value(target, key)? {
            return Ok(proto);
        }
        // `GetFunctionRealm` (a revoked proxy throws).
        let realm = match target {
            Value::Object(target) => self.function_realm_checked(target)?,
            _ => None,
        };
        fallback(self, realm.unwrap_or(call.realm()))
    }

    /// `SpeciesConstructor(object, default)` (§7.3.22) with V8's messages.
    /// `builtin_species` is the built-in `@@species` getter of `default`:
    /// when the `@@species` of the constructor is that getter (its own or
    /// one on its prototype chain, as for the typed array constructors,
    /// which inherit the getter of `%TypedArray%`), it is not called: it
    /// returns its receiver, so the call is not observable. The result is
    /// recorded.
    pub(crate) fn species_constructor(
        &mut self,
        object: Gc<Object>,
        default: Gc<Object>,
        builtin_species: Gc<Object>,
    ) -> VmResult<Value> {
        let key = PropertyKey::String(self.vm.atoms.constructor);
        let constructor = self.get_value(object.into(), key)?;
        let Value::Object(constructor) = constructor else {
            if constructor.is_undefined() {
                return Ok(default.into());
            }
            return Err(VmError::type_error(
                "The .constructor property is not an object",
            ));
        };
        let key = self.symbol_key(WellKnown::Species);
        let species = match self.heap.get(constructor, key, constructor.into())? {
            GetResult::CallGetter { getter, .. } if getter == Value::Object(builtin_species) => {
                constructor.into()
            }
            _ => self.get_value(constructor.into(), key)?,
        };
        if species.is_nullish() {
            return Ok(default.into());
        }
        if self.is_constructor(species)? {
            return Ok(self.heap.record(species));
        }
        Err(VmError::type_error(
            "object.constructor[Symbol.species] is not a constructor",
        ))
    }

    /// `ToObject(this)`, recorded in the native's handle scope.
    fn this_object(&mut self, call: &NativeCall) -> VmResult<Gc<Object>> {
        let object = self.to_object(call.this())?;
        self.heap.record(object);
        Ok(object)
    }

    /// `ToObject` of argument `index`, recorded in the native's handle
    /// scope (V8 words the failure "Cannot convert undefined or null to
    /// object").
    fn this_object_arg(&mut self, call: &NativeCall, index: usize) -> VmResult<Gc<Object>> {
        let object = self.to_object(self.arg(call, index))?;
        self.heap.record(object);
        Ok(object)
    }

    /// `ToPropertyKey(value)`, recorded in the native's handle scope (a
    /// key that is not an index is an atom, which the scope keeps alive).
    fn key_arg(&mut self, value: Value) -> VmResult<PropertyKey> {
        let key = self.to_property_key(value)?;
        Ok(self.heap.record(key))
    }

    /// A new empty array with the `Array.prototype` of the current realm,
    /// recorded in the native's handle scope.
    fn new_array(&mut self) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(self.current_realm(), Intrinsic::ArrayPrototype)?;
        let array = self.heap.new_array(Some(proto), 0)?;
        Ok(self.heap.record(array))
    }

    /// A new array `[first, second]`; both values must be reachable or
    /// recorded by the caller.
    fn pair_array(&mut self, first: Value, second: Value) -> VmResult<Gc<Object>> {
        self.array_from_values(&[first, second])
    }

    /// `CreateListFromArrayLike` (§7.3.19) with the limit of the value
    /// stack: the arguments go there, so a list that cannot fit fails
    /// before it is read (V8 also fails with a `RangeError`). Each value
    /// is recorded in the native's handle scope until the deferred call
    /// moves the list into the value stack.
    fn list_from_array_like(&mut self, value: Value) -> VmResult<Vec<Value>> {
        let Value::Object(list) = value else {
            return Err(VmError::type_error(
                "CreateListFromArrayLike called on non-object",
            ));
        };
        let length = self.length_of_array_like(list)?;
        checked_array_length(length)?;
        // V8's limit for the length of a list of arguments.
        if length >= f64::from(1u32 << 27) {
            return Err(VmError::range_error(crate::object::INVALID_ARRAY_LENGTH));
        }
        if length > self.vm.stack_limit as f64 {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let mut args = Vec::with_capacity(length as usize);
        for index in 0..length as u32 {
            self.tick()?;
            args.push(self.get_value(list.into(), PropertyKey::Index(index))?);
        }
        Ok(args)
    }

    /// `ToObject(this)` for the generic array methods that V8 words as
    /// "Array.prototype.forEach called on null or undefined" (`name` is
    /// the method with its prefix).
    fn this_object_of(&mut self, call: &NativeCall, name: &str) -> VmResult<Gc<Object>> {
        if call.this().is_nullish() {
            return Err(VmError::type_error(format!(
                "{name} called on null or undefined"
            )));
        }
        self.this_object(call)
    }

    /// `LengthOfArrayLike` (§7.3.18): `ToLength(Get(object, "length"))`.
    pub(crate) fn length_of_array_like(&mut self, object: Gc<Object>) -> VmResult<f64> {
        if let Ok((length, _)) = self.heap.array_length(object) {
            return Ok(f64::from(length));
        }
        let key = PropertyKey::String(self.heap.length_atom());
        let value = self.get_value(object.into(), key)?;
        Ok(to_length(self.to_number(value)?))
    }

    /// The property key of an index `k` (an integer, 0 ≤ k < 2^53),
    /// recorded in the current handle scope (a key above the array
    /// indices is an atom, which the scope keeps alive).
    pub(crate) fn index_key(&mut self, k: f64) -> VmResult<PropertyKey> {
        if k < f64::from(crate::string::MAX_ARRAY_LENGTH) {
            return Ok(PropertyKey::Index(k as u32));
        }
        let key = self
            .heap
            .key_from_str(&crate::vm::number::number_to_string(k))?;
        Ok(self.heap.record(key))
    }

    /// `Set(object, key, value, true)` (§7.3.4): a failed write throws; a
    /// setter runs through re-entry.
    fn set_or_throw(&mut self, object: Gc<Object>, key: PropertyKey, value: Value) -> VmResult<()> {
        match self.put_value(object.into(), key, value, true)? {
            SetOutcome::Done => Ok(()),
            SetOutcome::Setter { setter, receiver } => {
                self.call(setter, receiver, &[value])?;
                Ok(())
            }
        }
    }
}
