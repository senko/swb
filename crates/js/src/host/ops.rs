//! The ECMAScript abstract operations that the Web IDL bindings use
//! (ADR 0028 section 5; Web IDL §3.2,
//! <https://webidl.spec.whatwg.org/#es-type-mapping>): conversions,
//! property access, iteration, arrays and objects, promises and error
//! objects. Each method is a thin public entry to the engine's own
//! implementation of the operation, so the bindings convert values as the
//! built-in functions do.
//!
//! Values that these methods return are recorded in the current handle
//! scope (inside a native function, the scope of the call), as the other
//! results of the embedding API. Outside a native function (the host's own
//! setup), each method runs in a scope of its own, and the caller stores a
//! result in its traced state before the next allocation.

use crate::error::ThrowKind;
use crate::heap::Gc;
use crate::object::{Object, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, IteratorRecord, VmError, VmResult};

/// An iterator that a native function steps (ECMA-262 §7.4.1, an
/// Iterator Record): made by [`Runtime::iterator_from_method`], stepped by
/// [`Runtime::iterator_next`]. Its values are recorded in the handle scope
/// that was open when it was made, so it must not outlive that scope.
#[derive(Clone, Copy, Debug)]
pub struct ScriptIterator(IteratorRecord);

impl Runtime {
    /// `ToNumber(value)` (ECMA-262 §7.1.4). Can run script code
    /// (`valueOf`, `toString`).
    pub fn number_of(&mut self, value: Value) -> VmResult<f64> {
        self.in_scope(|rt| rt.to_number(value))
    }

    /// `ToBoolean(value)` (ECMA-262 §7.1.2).
    pub fn truthy(&self, value: Value) -> VmResult<bool> {
        self.to_boolean(value)
    }

    /// `IsCallable(value)` (ECMA-262 §7.2.3).
    pub fn callable(&self, value: Value) -> VmResult<bool> {
        self.is_callable(value)
    }

    /// The realm of the running code (of the innermost function; realm 0
    /// when no script runs).
    pub fn running_realm(&self) -> u32 {
        self.current_realm()
    }

    /// `O.[[GetPrototypeOf]]()` (ECMA-262 §10.1.1; a proxy's trap can run
    /// script code). Charges a time step.
    pub fn prototype_of(&mut self, object: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
        self.tick()?;
        self.in_scope(|rt| rt.get_prototype_of(object))
    }

    /// The own property `key` that `object` stores (`OrdinaryGetOwnProperty`,
    /// ECMA-262 §10.1.5.1), without the hooks of a host object: the
    /// expandos of a legacy platform object. A proxy has none. A lazy
    /// value of a global object (`lazy.rs`), which this read cannot
    /// create, is an internal error: legacy platform objects have none.
    pub fn stored_own_property(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        if self.heap.is_proxy(object)? {
            return Ok(None);
        }
        match self.heap.get_own_property(object, key)? {
            Some(Property::Data {
                value: Value::Lazy(_),
                ..
            }) => Err(VmError::invariant(
                "a stored read of a lazy property that is not created",
            )),
            property => Ok(property),
        }
    }

    /// `GetV(base, key)` (ECMA-262 §7.3.3): a getter runs as a re-entry.
    pub fn get_key(&mut self, base: Value, key: PropertyKey) -> VmResult<Value> {
        self.in_scope(|rt| rt.get_value(base, key))
    }

    /// `Set(object, key, value, throw)` (ECMA-262 §7.3.4): with `throw`, a
    /// refused write throws a `TypeError`; without, it is ignored.
    pub fn set_key(
        &mut self,
        base: Value,
        key: PropertyKey,
        value: Value,
        throw: bool,
    ) -> VmResult<()> {
        self.in_scope(|rt| match rt.put_value(base, key, value, throw)? {
            crate::vm::SetOutcome::Done => Ok(()),
            crate::vm::SetOutcome::Setter { setter, receiver } => {
                rt.call(setter, receiver, &[value]).map(|_| ())
            }
        })
    }

    /// `object.[[GetOwnProperty]](key)` (ECMA-262 §10.1.5; the traps of a
    /// proxy and the hooks of a host object run).
    pub fn own_property_of(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        self.in_scope(|rt| rt.own_property(object, key))
    }

    /// `object.[[OwnPropertyKeys]]()` (ECMA-262 §10.1.11).
    pub fn own_keys_of(&mut self, object: Gc<Object>) -> VmResult<Vec<PropertyKey>> {
        self.in_scope(|rt| rt.own_keys(object))
    }

    /// `CreateDataPropertyOrThrow(object, key, value)` (ECMA-262 §7.3.7).
    pub fn create_data_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
    ) -> VmResult<()> {
        self.in_scope(|rt| {
            rt.heap.record(object);
            rt.create_data_property_or_throw(object, key, value)
        })
    }

    /// Defines a property of a host's own object with a descriptor (the
    /// accessors of `[LegacyUnforgeable]` members, which are shared between
    /// the instances of an interface): `DefinePropertyOrThrow`, or the
    /// ordinary definition on a host object with hooks.
    pub fn define_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
    ) -> VmResult<()> {
        self.scoped(|rt| rt.define_by_host(object, key, desc))
    }

    /// A new ordinary object with `%Object.prototype%` of `realm`.
    pub fn new_plain_object(&mut self, realm: u32) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(realm, Intrinsic::ObjectPrototype)?;
        let object = self.heap.new_object(Some(proto))?;
        Ok(self.keep(object))
    }

    /// `CreateArrayFromList(values)` (ECMA-262 §7.3.17) in `realm`. The
    /// values must be rooted by the caller.
    pub fn new_array_of(&mut self, realm: u32, values: &[Value]) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(realm, Intrinsic::ArrayPrototype)?;
        let array = self.heap.new_array_from(Some(proto), values, &self.vm)?;
        Ok(self.keep(array))
    }

    /// `GetIteratorFromMethod(value, method)` (ECMA-262 §7.4.2). Only
    /// inside a native function or an open handle scope, which keeps the
    /// iterator alive.
    pub fn iterator_from_method(
        &mut self,
        value: Value,
        method: Value,
    ) -> VmResult<ScriptIterator> {
        if self.heap.scope_depth() == 0 {
            return Err(VmError::invariant("an iterator outside a handle scope"));
        }
        Ok(ScriptIterator(
            self.get_iterator_from_method(value, method)?,
        ))
    }

    /// `IteratorStepValue(iterator)` (ECMA-262 §7.4.10): the next value, or
    /// `None` when the iterator is done. Each step ticks the time limit,
    /// so an endless iterator ends at the deadline.
    pub fn iterator_next(&mut self, iterator: &mut ScriptIterator) -> VmResult<Option<Value>> {
        if self.heap.scope_depth() == 0 {
            return Err(VmError::invariant("an iterator outside a handle scope"));
        }
        self.iterator_step_value(&mut iterator.0)
    }

    /// `PromiseResolve(%Promise%, value)` (ECMA-262 §27.2.4.7.1) of
    /// `realm`: `value` itself if it is a promise of that constructor,
    /// else a new promise resolved with it (Web IDL §3.2.24, the
    /// conversion to a promise type).
    pub fn resolved_promise(&mut self, realm: u32, value: Value) -> VmResult<Value> {
        let constructor = self.intrinsic(realm, Intrinsic::PromiseConstructor)?;
        self.in_scope(|rt| rt.promise_resolve(constructor, value, realm))
    }

    /// A new promise of `realm` rejected with `reason` (Web IDL "a promise
    /// rejected with").
    pub fn rejected_promise(&mut self, realm: u32, reason: Value) -> VmResult<Value> {
        self.in_scope(|rt| {
            rt.heap.record(reason);
            let promise = rt.new_promise(realm)?;
            rt.reject_promise(promise, reason)?;
            Ok(promise.into())
        })
    }

    /// Runs `f` in the open handle scope, or in a scope of its own when
    /// none is open.
    fn in_scope<T>(&mut self, f: impl FnOnce(&mut Runtime) -> VmResult<T>) -> VmResult<T> {
        if self.heap.scope_depth() > 0 {
            f(self)
        } else {
            self.scoped(f)
        }
    }

    /// A new error object of `kind` with `message` in the current realm,
    /// with a `stack` of the running frames (what `new TypeError(message)`
    /// creates).
    pub fn new_error(&mut self, kind: ThrowKind, message: &str) -> VmResult<Gc<Object>> {
        let object = self.error_object(kind, message)?;
        Ok(self.keep(object))
    }
}
