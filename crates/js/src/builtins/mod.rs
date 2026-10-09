//! The built-in objects (ECMA-262 clauses 19 to 28).
//!
//! The spike (session 5) has the ones that its tests and the test262
//! harness need first: `console.log`, `Object` with `keys`, the `Error`
//! constructors, `Array` with `isArray`, `push`, `join`, `forEach` and
//! `map`, `Function.prototype.call`, `apply` and `toString`, and `String`
//! and `Number` with their prototype methods. Each function has the
//! `name` and `length` of the specification; methods and constructors are
//! writable, non-enumerable and configurable properties (clause 18).
//!
//! Natives that call back into script code (`forEach`, `map`, and `join`
//! through `toString`) open a handle scope per element, so that their
//! scope does not grow with the length of the array, and step the time
//! countdown once per element, also over holes (ADR 0026 section 9).
//! `Function.prototype.call`, `apply` and `Array.prototype.toString` end
//! in a deferred call, without Rust recursion.

mod array;
mod console;
mod error;
mod function;
mod object;
mod primitive;

pub(crate) use console::inspect;

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, SetOutcome, VmError, VmResult};

/// The largest integer that `ToLength` gives (2^53 − 1).
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Installs the built-in objects of a realm (after its intrinsics exist
/// and the realm is a root).
pub(crate) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let scope = rt.heap.open_scope();
    let result = install_all(rt, realm);
    rt.heap.close_scope(scope)?;
    result
}

fn install_all(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    object::install(rt, realm)?;
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
        Ok(to_length(self.to_numeric(value)?))
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

/// `ToIntegerOrInfinity` (§7.1.5) of a Number.
fn to_integer_or_infinity(x: f64) -> f64 {
    if x.is_nan() {
        return 0.0;
    }
    // `trunc` keeps infinities; `+ 0.0` turns -0 into +0.
    x.trunc() + 0.0
}

/// `ToLength` (§7.1.20) of a Number.
fn to_length(x: f64) -> f64 {
    to_integer_or_infinity(x).clamp(0.0, MAX_SAFE_INTEGER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_conversions() {
        assert_eq!(to_length(-5.0), 0.0);
        assert_eq!(to_length(f64::NAN), 0.0);
        assert_eq!(to_length(f64::INFINITY), MAX_SAFE_INTEGER);
        assert_eq!(to_length(3.9), 3.0);
        assert!(to_integer_or_infinity(-0.5).is_sign_positive());
        assert_eq!(to_integer_or_infinity(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }
}
