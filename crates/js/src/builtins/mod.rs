//! The built-in objects (ECMA-262 clauses 19 to 28).
//!
//! Implemented so far: `console.log`; `Object` (all static functions
//! except `fromEntries` and `groupBy`, `Object.prototype` with Annex B)
//! and `Reflect`; the `Error` constructors; `Array` with `isArray`,
//! `push`, `join`, `forEach`, `map` and `toString`; `Function.prototype`
//! with `call`, `apply`, `bind` and `toString` (the `Function` constructor
//! is M7); `isNaN`, `isFinite` and `Math.pow`; and `String` and `Number`
//! with their prototype methods. `Boolean` and `Symbol` have prototype
//! intrinsics for wrapper objects but no constructors yet. Each function
//! has the `name` and `length` of the specification; methods and
//! constructors are writable, non-enumerable and configurable properties
//! (clause 18).
//!
//! Natives that call back into script code (`forEach`, `map`, `join`
//! through `toString`, and the key loops of `Object` that read getters)
//! open a handle scope per element, so that their scope does not grow
//! with the length of the array, and step the time countdown once per
//! element, also over holes (ADR 0026 section 9). `Function.prototype.call`,
//! `apply`, `Reflect.apply`, `Reflect.construct` and
//! `Array.prototype.toString` end in a deferred call, without Rust
//! recursion.

mod array;
mod console;
mod descriptor;
mod error;
mod function;
mod global;
mod integrity;
mod object;
mod object_proto;
mod primitive;
mod reflect;

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::{checked_array_length, to_length};
use crate::vm::{Intrinsic, NativeCall, NativeFn, STACK_OVERFLOW, SetOutcome, VmError, VmResult};

/// Installs the built-in objects of a realm (after its intrinsics exist
/// and the realm is a root).
pub(crate) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    rt.scoped(|rt| install_all(rt, realm))
}

fn install_all(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    object::install(rt, realm)?;
    reflect::install(rt, realm)?;
    global::install(rt, realm)?;
    function::install(rt, realm)?;
    error::install(rt, realm)?;
    array::install(rt, realm)?;
    primitive::install(rt, realm)?;
    console::install(rt, realm)
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
        let target = match call.new_target() {
            Value::Undefined => Value::Object(call.callee()),
            target => target,
        };
        let key = PropertyKey::String(self.vm.atoms.prototype);
        match self.get_value(target, key)? {
            Value::Object(proto) => Ok(proto),
            _ => self.intrinsic(call.realm(), fallback),
        }
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
        let array = self.new_array()?;
        for (index, value) in [first, second].into_iter().enumerate() {
            let key = PropertyKey::Index(index as u32);
            self.heap
                .create_data_property(array, key, value, &self.vm)?;
        }
        Ok(array)
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
    fn length_of_array_like(&mut self, object: Gc<Object>) -> VmResult<f64> {
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
    fn index_key(&mut self, k: f64) -> VmResult<PropertyKey> {
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

    /// The `TypeError` of a callback that is not callable, as V8 words it
    /// for the array methods ("number 1 is not a function").
    fn not_callable(&self, value: Value) -> VmError {
        let text = match value {
            Value::Undefined => "undefined".to_owned(),
            Value::Null => "object null".to_owned(),
            Value::Bool(b) => format!("boolean {b}"),
            Value::Int(_) | Value::Double(_) => {
                let n = value.as_number().unwrap_or(f64::NAN);
                // V8 shows -0 as 0 here.
                format!("number {}", crate::vm::number::number_to_string(n))
            }
            Value::String(s) => format!("string \"{}\"", self.name_text(s)),
            Value::Symbol(_) => "symbol".to_owned(),
            Value::BigInt(_) => "bigint".to_owned(),
            Value::Object(_) | Value::Empty | Value::Cell(_) => "object".to_owned(),
        };
        VmError::type_error(format!("{text} is not a function"))
    }
}
