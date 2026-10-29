//! The internal methods of Proxy exotic objects (ECMA-262 §10.5,
//! <https://tc39.es/ecma262/#sec-proxy-object-internal-methods-and-internal-slots>)
//! and the generic forms of the internal methods that dispatch to them
//! (ADR 0026 sections 6 and 15), and to the hooks of host objects
//! (`host/hooks.rs`).
//!
//! Each method follows the steps of the specification in their order: the
//! check of a revoked proxy, the trap lookup (`GetMethod` on the handler,
//! which a script can observe), the call of the trap through re-entry, and
//! the invariant checks against the target. Without a trap, the method
//! applies the generic method to the target, which is a recursion when the
//! target is a proxy. Every method charges the shared recursion budget
//! ([`PROXY_WEIGHT`]) and a time step, so a chain of a million proxies
//! ends with V8's `RangeError` and not with a stack overflow, and a trap
//! that calls the same proxy again ends the same way (the trap call itself
//! charges the weight of a re-entry).
//!
//! The values that a method holds across a trap call (target, handler,
//! key, receiver, values of descriptors) are recorded in the caller's
//! handle scope, and so are its results: a caller that opened no scope
//! must open one.
//!
//! The messages are V8's, measured in Chromium 148
//! (`tools/probes/js-proxy.json`). Where an operation would throw for a
//! `false` result (`DefinePropertyOrThrow`, a strict assignment or
//! `delete`), the generic methods take a `throw` flag and raise the error
//! where the `false` comes from, as V8 does: a trap that returned a falsish
//! value names the trap, a target that refused names its own reason.
//! Nothing that a script can observe happens between that point and the
//! throw of the specification.

use std::collections::HashSet;

use swb_js_text::hash::BuildKeyedHasher;

use crate::heap::Gc;
use crate::host::hooks::{Refusal, SetAnswer};
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::internal::ProtoSet;
use crate::vm::property::Lookup;
use crate::vm::{STACK_OVERFLOW, VmError, VmResult};

/// The recursion budget weight of one internal method of a proxy: its
/// Rust frames and those of the generic method that forwards to the
/// target. Measured per level of a chain of proxies without traps: at
/// most 960 bytes in a debug build (`opt-level = 1`; `[[Get]]` and
/// `[[Set]]`) and 752 bytes in a release build (`[[GetOwnProperty]]`); a
/// margin of 2. With the default budget (4 MiB), a chain of about 2,000
/// proxies works; Chromium 148 throws the same `RangeError` for a chain of
/// 1,000,000. `[[Call]]` and `[[Construct]]` without a trap are tail calls
/// and cost no Rust stack.
pub(crate) const PROXY_WEIGHT: u32 = 2 * 1024;

/// The bytes that a heap check counts for one key of the lists and sets
/// of `[[OwnPropertyKeys]]` (the key in the list and in the hash set).
const KEY_ENTRY_BYTES: usize = 48;

/// The number of traps.
pub(crate) const TRAP_COUNT: usize = 13;

/// The traps of a proxy handler (§10.5, Table 32 in §28.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trap {
    GetPrototypeOf,
    SetPrototypeOf,
    IsExtensible,
    PreventExtensions,
    GetOwnPropertyDescriptor,
    DefineProperty,
    Has,
    Get,
    Set,
    DeleteProperty,
    OwnKeys,
    Apply,
    Construct,
}

impl Trap {
    /// All traps, in the order of their atoms.
    pub(crate) const ALL: [Trap; TRAP_COUNT] = [
        Trap::GetPrototypeOf,
        Trap::SetPrototypeOf,
        Trap::IsExtensible,
        Trap::PreventExtensions,
        Trap::GetOwnPropertyDescriptor,
        Trap::DefineProperty,
        Trap::Has,
        Trap::Get,
        Trap::Set,
        Trap::DeleteProperty,
        Trap::OwnKeys,
        Trap::Apply,
        Trap::Construct,
    ];

    /// The name of the trap (the property of the handler).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Trap::GetPrototypeOf => "getPrototypeOf",
            Trap::SetPrototypeOf => "setPrototypeOf",
            Trap::IsExtensible => "isExtensible",
            Trap::PreventExtensions => "preventExtensions",
            Trap::GetOwnPropertyDescriptor => "getOwnPropertyDescriptor",
            Trap::DefineProperty => "defineProperty",
            Trap::Has => "has",
            Trap::Get => "get",
            Trap::Set => "set",
            Trap::DeleteProperty => "deleteProperty",
            Trap::OwnKeys => "ownKeys",
            Trap::Apply => "apply",
            Trap::Construct => "construct",
        }
    }
}

/// The target, the handler and the trap (if the handler has one) of a
/// proxy.
type TrapParts = (Gc<Object>, Gc<Object>, Option<Value>);

/// The result of the generic `[[Set]]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SetDone {
    /// The boolean result.
    Done(bool),
    /// A setter to call with `this` = `receiver` (then the result is
    /// true).
    Setter { setter: Value, receiver: Value },
}

/// The `TypeError` of an operation on a revoked proxy (V8's wording; the
/// operation is the trap's name or the name of an abstract operation).
pub(crate) fn revoked_error(operation: &str) -> VmError {
    VmError::type_error(format!(
        "Cannot perform '{operation}' on a proxy that has been revoked"
    ))
}

impl Runtime {
    /// Runs `f` as one level of a proxy's internal method: a step of the
    /// time countdown and [`PROXY_WEIGHT`] of the recursion budget.
    fn proxy_level<T>(&mut self, f: impl FnOnce(&mut Runtime) -> VmResult<T>) -> VmResult<T> {
        if self.budget.enter(PROXY_WEIGHT).is_err() {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let result = self.tick().and_then(|()| f(self));
        self.budget.leave(PROXY_WEIGHT);
        result
    }

    /// `ValidateNonRevokedProxy` (§10.5.14), then the target, the handler
    /// and `GetMethod(handler, trap)` (steps 1 to 5 of each internal
    /// method). The handles are recorded in the current handle scope.
    fn proxy_trap(&mut self, proxy: Gc<Object>, trap: Trap) -> VmResult<TrapParts> {
        let data = self
            .heap
            .proxy_data(proxy)?
            .ok_or(VmError::invariant("a proxy method of another object"))?;
        let (Some(target), Some(handler)) = (data.target, data.handler) else {
            return Err(revoked_error(trap.name()));
        };
        self.heap.record(target);
        self.heap.record(handler);
        let method = self.get_value(handler.into(), self.trap_key(trap))?;
        if method.is_nullish() {
            return Ok((target, handler, None));
        }
        if !self.is_callable(method)? {
            return Err(self.not_function_property(method, trap.name(), handler.into()));
        }
        Ok((target, handler, Some(method)))
    }

    /// The key of a trap's name (an atom of the VM).
    fn trap_key(&self, trap: Trap) -> PropertyKey {
        // The variants are declared in the order of `Trap::ALL`.
        let atom = self.vm.atoms.traps.get(trap as usize).copied();
        PropertyKey::String(atom.unwrap_or(self.vm.atoms.empty))
    }

    /// The text of a value in the invariant messages (V8 shows objects as
    /// `[object Object]` and -0 as 0).
    fn trap_value_text(&self, value: Value) -> String {
        match value {
            Value::Object(_) if !self.is_callable(value).unwrap_or(false) => {
                "[object Object]".to_owned()
            }
            Value::Double(0.0) => "0".to_owned(),
            _ => self.value_text(value),
        }
    }

    // --- The generic internal methods ---

    /// `O.[[GetPrototypeOf]]()` for any object (§10.1.1, §10.5.1). The
    /// result is recorded in the current handle scope when it comes from a
    /// trap.
    pub(crate) fn get_prototype_of(&mut self, object: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
        if self.heap.is_proxy(object)? {
            return self.proxy_get_prototype_of(object);
        }
        Ok(self.heap.get_prototype_of(object)?)
    }

    /// `IsExtensible(O)` for any object (§7.2.5, §10.5.3).
    pub(crate) fn is_extensible(&mut self, object: Gc<Object>) -> VmResult<bool> {
        if self.heap.is_proxy(object)? {
            return self.proxy_is_extensible(object);
        }
        Ok(self.heap.is_extensible(object)?)
    }

    /// `O.[[PreventExtensions]]()` for any object (§10.1.4, §10.5.4). With
    /// `throw`, a `false` result is V8's `TypeError`.
    pub(crate) fn prevent_extensions(&mut self, object: Gc<Object>, throw: bool) -> VmResult<bool> {
        if self.heap.is_proxy(object)? {
            return self.proxy_prevent_extensions(object, throw);
        }
        if let Some(hooks) = self.host_hooks(object)? {
            return self.host_prevent_extensions(object, &hooks);
        }
        Ok(self.heap.prevent_extensions(object)?)
    }

    /// `O.[[Delete]](P)` for any object (§10.1.10, §10.5.10). With
    /// `throw`, a `false` result is a `TypeError` with V8's wording.
    pub(crate) fn delete_generic(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        throw: bool,
    ) -> VmResult<bool> {
        if self.heap.is_proxy(object)? {
            return self.proxy_delete(object, key, throw);
        }
        let deleted = match self.host_hooks(object)? {
            Some(hooks) => self.host_delete(object, key, &hooks)?,
            None => self.heap.delete(object, key)?,
        };
        if !deleted && throw {
            if let Some(error) = self.host_refusal(object, key, Refusal::Delete)? {
                return Err(error);
            }
            return Err(VmError::type_error(format!(
                "Cannot delete property '{}' of {}",
                self.key_text(key),
                self.object_text(object)
            )));
        }
        Ok(deleted)
    }

    /// `HasOwnProperty(O, P)` (§7.3.13) for any object, without
    /// allocating the character of a String object.
    pub(crate) fn has_own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<bool> {
        if self.heap.needs_runtime(object)? {
            return Ok(self.exotic_own_property(object, key)?.is_some());
        }
        if let Some((data, index)) = self.typed_numeric(object, key)? {
            return Ok(self.heap.valid_index(&data, index)?.is_some());
        }
        Ok(self.heap.string_object_unit(object, key)?.is_some()
            || self.heap.get_own_property(object, key)?.is_some())
    }

    /// Whether `object` has an own enumerable property `key`
    /// (`[[GetOwnProperty]]` for any object, without allocating the
    /// character of a String object).
    pub(crate) fn own_enumerable(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<bool> {
        if self.heap.needs_runtime(object)? {
            return Ok(self
                .exotic_own_property(object, key)?
                .is_some_and(|property| property.enumerable()));
        }
        if self.heap.string_object_unit(object, key)?.is_some() {
            return Ok(true);
        }
        if let Some((data, index)) = self.typed_numeric(object, key)? {
            return Ok(self.heap.valid_index(&data, index)?.is_some());
        }
        Ok(self
            .heap
            .get_own_property(object, key)?
            .is_some_and(|property| property.enumerable()))
    }

    /// `O.[[GetOwnProperty]](P)` for any object, except the characters of
    /// a String object, which are not stored and read as no property (for
    /// callers that know them to be fixed).
    pub(crate) fn own_property_stored(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        if self.heap.needs_runtime(object)? {
            return self.exotic_own_property(object, key);
        }
        if let Some((data, index)) = self.typed_numeric(object, key)? {
            return self.typed_own_property(&data, index);
        }
        self.own_property_resolved(object, key)
    }

    /// `O.[[Get]](P, Receiver)` along a prototype chain that has a proxy
    /// or a host object with hooks (§10.1.8.1 `OrdinaryGet` as a loop,
    /// §10.5.8 at a proxy, the hooks at a host object). An accessor is
    /// returned for the caller to call.
    #[cold]
    #[inline(never)]
    pub(crate) fn get_generic(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        receiver: Value,
    ) -> VmResult<Lookup> {
        let mut current = object;
        loop {
            if self.heap.is_proxy(current)? {
                return self.proxy_get(current, key, receiver);
            }
            if let Some(hooks) = self.host_hooks(current)? {
                if let Some(found) = self.host_get(current, key, receiver, &hooks)? {
                    return Ok(found);
                }
                current = match self.heap.get_prototype_of(current)? {
                    Some(proto) => proto,
                    None => return Ok(Lookup::Value(Value::Undefined)),
                };
                self.tick()?;
                continue;
            }
            // §10.4.5: a numeric key of a typed array ends the walk.
            if let Some((data, index)) = self.typed_numeric(current, key)? {
                let value = self.typed_index_value(&data, index)?;
                return Ok(Lookup::Value(self.heap.record(value)));
            }
            match self.own_property(current, key)? {
                Some(Property::Data { value, .. }) => return Ok(Lookup::Value(value)),
                Some(Property::Accessor { get, .. }) => {
                    if get.is_undefined() {
                        return Ok(Lookup::Value(Value::Undefined));
                    }
                    return Ok(Lookup::Getter {
                        getter: get,
                        receiver,
                    });
                }
                None => match self.heap.get_prototype_of(current)? {
                    Some(proto) => {
                        self.tick()?;
                        current = proto;
                    }
                    None => return Ok(Lookup::Value(Value::Undefined)),
                },
            }
        }
    }

    /// `O.[[HasProperty]](P)` along a prototype chain that has a proxy
    /// (§10.1.7.1 as a loop, §10.5.7 at a proxy).
    #[cold]
    #[inline(never)]
    pub(crate) fn has_generic(&mut self, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        let mut current = object;
        loop {
            if self.heap.is_proxy(current)? {
                return self.proxy_has(current, key);
            }
            if let Some((data, index)) = self.typed_numeric(current, key)? {
                return Ok(self.heap.valid_index(&data, index)?.is_some());
            }
            let own = match self.host_hooks(current)? {
                Some(hooks) => self.host_own_property(current, key, &hooks)?.is_some(),
                None => {
                    self.heap.get_own_property(current, key)?.is_some()
                        || self.heap.string_object_unit(current, key)?.is_some()
                }
            };
            if own {
                return Ok(true);
            }
            match self.heap.get_prototype_of(current)? {
                Some(proto) => {
                    self.tick()?;
                    current = proto;
                }
                None => return Ok(false),
            }
        }
    }

    /// `O.[[Set]](P, V, Receiver)` when the chain or the receiver has a
    /// proxy (§10.1.9.2 `OrdinarySetWithOwnDescriptor` with the generic
    /// methods, §10.5.9 at a proxy). With `throw` (a strict assignment), a
    /// `false` result is a `TypeError` with V8's wording.
    #[cold]
    #[inline(never)]
    pub(crate) fn set_generic(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        receiver: Value,
        throw: bool,
    ) -> VmResult<SetDone> {
        // Steps 1 and 2 as a loop over the prototype chain.
        let mut current = object;
        loop {
            if self.heap.is_proxy(current)? {
                return self.proxy_set(current, key, value, receiver, throw);
            }
            let mut own = None;
            if let Some(hooks) = self.host_hooks(current)? {
                match self.host_set(current, (key, value, receiver), &hooks)? {
                    SetAnswer::Done(true) => return Ok(SetDone::Done(true)),
                    SetAnswer::Done(false) => {
                        return self.refuse_set(throw, current, receiver, key);
                    }
                    SetAnswer::Ordinary => {}
                    SetAnswer::WithOwn(property) => own = Some(Some(property)),
                    SetAnswer::WithStored => {
                        own = Some(self.heap.get_own_property(current, key)?);
                    }
                }
            }
            // §10.4.5: a numeric key of a typed array.
            if let Some((data, index)) = self.typed_numeric(current, key)? {
                if receiver == Value::Object(current) {
                    self.typed_set_element(&data, index, value)?;
                    return Ok(SetDone::Done(true));
                }
                if self.heap.valid_index(&data, index)?.is_none() {
                    return Ok(SetDone::Done(true));
                }
                break;
            }
            let own = match own {
                Some(own) => own,
                None => self.own_property(current, key)?,
            };
            match own {
                Some(Property::Data { writable: true, .. }) => break,
                Some(Property::Data { .. }) => {
                    return self.refuse_set(throw, current, receiver, key);
                }
                Some(Property::Accessor { set, .. }) => {
                    if set.is_undefined() {
                        return self.refuse_set(throw, current, receiver, key);
                    }
                    return Ok(SetDone::Setter {
                        setter: set,
                        receiver,
                    });
                }
                None => match self.heap.get_prototype_of(current)? {
                    Some(proto) => {
                        self.tick()?;
                        current = proto;
                    }
                    None => break,
                },
            }
        }
        // Step 3: a writable data property, or none.
        let Value::Object(target) = receiver else {
            if throw {
                return Err(self.cannot_create(receiver, key));
            }
            return Ok(SetDone::Done(false));
        };
        match self.own_property(target, key)? {
            Some(
                Property::Accessor { .. }
                | Property::Data {
                    writable: false, ..
                },
            ) => self.refuse_set(throw, target, receiver, key),
            Some(Property::Data { .. }) => {
                let desc = PropertyDescriptor::value(value);
                self.define_for_set(target, key, desc, receiver, throw)
            }
            None => {
                let desc = PropertyDescriptor::data(value, true, true, true);
                self.define_for_set(target, key, desc, receiver, throw)
            }
        }
    }

    /// The definition on the receiver of `[[Set]]` (§10.1.9.2 steps 2.c
    /// and 2.d). A host object that refuses it reports a refused `[[Set]]`
    /// (Chromium's messages differ from those of a definition).
    fn define_for_set(
        &mut self,
        target: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
        receiver: Value,
        throw: bool,
    ) -> VmResult<SetDone> {
        if self.host_hooks(target)?.is_none() {
            let done = self.define_generic(target, key, desc, throw)?;
            return Ok(SetDone::Done(done));
        }
        if self.define_generic(target, key, desc, false)? {
            return Ok(SetDone::Done(true));
        }
        self.refuse_set(throw, target, receiver, key)
    }

    /// A refused `[[Set]]` at `holder` (a read-only property, or an
    /// accessor without a setter): `false`, or a `TypeError` with the
    /// host's message for its own objects, otherwise V8's.
    fn refuse_set(
        &mut self,
        throw: bool,
        holder: Gc<Object>,
        receiver: Value,
        key: PropertyKey,
    ) -> VmResult<SetDone> {
        if !throw {
            return Ok(SetDone::Done(false));
        }
        if let Some(error) = self.host_refusal(holder, key, Refusal::Set)? {
            return Err(error);
        }
        match receiver {
            Value::Object(_) => Err(self.failed_write(holder, key)),
            _ => Err(self.cannot_create(receiver, key)),
        }
    }

    /// `O.[[DefineOwnProperty]](P, Desc)` for any object. With `throw`
    /// (`DefinePropertyOrThrow`), a `false` result is a `TypeError` with
    /// V8's wording.
    pub(crate) fn define_generic(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> VmResult<bool> {
        if self.heap.is_proxy(object)? {
            return self.proxy_define_own_property(object, key, desc, throw);
        }
        if let Some(hooks) = self.host_hooks(object)? {
            let done = self.host_define(object, key, desc, &hooks)?;
            if !done && throw {
                if let Some(error) = self.host_refusal(object, key, Refusal::Define)? {
                    return Err(error);
                }
                return Err(self.define_failure(object, key));
            }
            return Ok(done);
        }
        if throw {
            self.define_or_throw(object, key, desc)?;
            return Ok(true);
        }
        self.define_own_property(object, key, desc)
    }

    // --- The internal methods of a proxy (§10.5) ---

    /// `[[GetPrototypeOf]]` (§10.5.1).
    #[cold]
    #[inline(never)]
    fn proxy_get_prototype_of(&mut self, proxy: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
        self.proxy_level(|rt| {
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::GetPrototypeOf)?;
            let Some(trap) = trap else {
                return rt.get_prototype_of(target);
            };
            let proto = match rt.call(trap, handler.into(), &[target.into()])? {
                Value::Object(proto) => Some(proto),
                Value::Null => None,
                _ => {
                    return Err(VmError::type_error(
                        "'getPrototypeOf' on proxy: trap returned neither object nor null",
                    ));
                }
            };
            if rt.is_extensible(target)? {
                return Ok(proto);
            }
            if rt.get_prototype_of(target)? != proto {
                return Err(VmError::type_error(
                    "'getPrototypeOf' on proxy: proxy target is non-extensible but the trap did not return its actual prototype",
                ));
            }
            Ok(proto)
        })
    }

    /// `[[SetPrototypeOf]]` (§10.5.2). A falsish trap result is
    /// [`ProtoSet::TrapFalse`].
    #[cold]
    #[inline(never)]
    pub(crate) fn proxy_set_prototype_of(
        &mut self,
        proxy: Gc<Object>,
        proto: Option<Gc<Object>>,
    ) -> VmResult<ProtoSet> {
        self.proxy_level(|rt| {
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::SetPrototypeOf)?;
            let Some(trap) = trap else {
                return rt.set_prototype_of(target, proto);
            };
            let value = proto.map_or(Value::Null, Value::Object);
            let result = rt.call(trap, handler.into(), &[target.into(), value])?;
            if !rt.to_boolean(result)? {
                return Ok(ProtoSet::TrapFalse);
            }
            if rt.is_extensible(target)? {
                return Ok(ProtoSet::Done);
            }
            if rt.get_prototype_of(target)? != proto {
                return Err(VmError::type_error(
                    "'setPrototypeOf' on proxy: trap returned truish for setting a new prototype on the non-extensible proxy target",
                ));
            }
            Ok(ProtoSet::Done)
        })
    }

    /// `[[IsExtensible]]` (§10.5.3).
    #[cold]
    #[inline(never)]
    fn proxy_is_extensible(&mut self, proxy: Gc<Object>) -> VmResult<bool> {
        self.proxy_level(|rt| {
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::IsExtensible)?;
            let Some(trap) = trap else {
                return rt.is_extensible(target);
            };
            let result = rt.call(trap, handler.into(), &[target.into()])?;
            let result = rt.to_boolean(result)?;
            let target_result = rt.is_extensible(target)?;
            if result != target_result {
                return Err(VmError::type_error(format!(
                    "'isExtensible' on proxy: trap result does not reflect extensibility of proxy target (which is '{target_result}')"
                )));
            }
            Ok(result)
        })
    }

    /// `[[PreventExtensions]]` (§10.5.4).
    #[cold]
    #[inline(never)]
    fn proxy_prevent_extensions(&mut self, proxy: Gc<Object>, throw: bool) -> VmResult<bool> {
        self.proxy_level(|rt| {
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::PreventExtensions)?;
            let Some(trap) = trap else {
                return rt.prevent_extensions(target, throw);
            };
            let result = rt.call(trap, handler.into(), &[target.into()])?;
            if !rt.to_boolean(result)? {
                if throw {
                    return Err(VmError::type_error(
                        "'preventExtensions' on proxy: trap returned falsish",
                    ));
                }
                return Ok(false);
            }
            if rt.is_extensible(target)? {
                return Err(VmError::type_error(
                    "'preventExtensions' on proxy: trap returned truish but the proxy target is extensible",
                ));
            }
            Ok(true)
        })
    }

    /// `[[GetOwnProperty]]` (§10.5.5). The values of the result are
    /// recorded in the current handle scope.
    #[cold]
    #[inline(never)]
    pub(crate) fn proxy_get_own_property(
        &mut self,
        proxy: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        self.proxy_level(|rt| {
            // The key may be an atom that only the caller's Rust code holds.
            rt.heap.record(key);
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::GetOwnPropertyDescriptor)?;
            let Some(trap) = trap else {
                return rt.own_property(target, key);
            };
            let key_value = rt.heap.key_to_value(key)?;
            let result = rt.call(trap, handler.into(), &[target.into(), key_value])?;
            if !matches!(result, Value::Object(_) | Value::Undefined) {
                return Err(VmError::type_error(format!(
                    "'getOwnPropertyDescriptor' on proxy: trap returned neither object nor undefined for property '{}'",
                    rt.key_text(key)
                )));
            }
            let target_desc = rt.own_property(target, key)?;
            if result.is_undefined() {
                let Some(target_desc) = target_desc else {
                    return Ok(None);
                };
                if !target_desc.configurable() {
                    return Err(VmError::type_error(format!(
                        "'getOwnPropertyDescriptor' on proxy: trap returned undefined for property '{}' which is non-configurable in the proxy target",
                        rt.key_text(key)
                    )));
                }
                if !rt.is_extensible(target)? {
                    return Err(VmError::type_error(format!(
                        "'getOwnPropertyDescriptor' on proxy: trap returned undefined for property '{}' which exists in the non-extensible proxy target",
                        rt.key_text(key)
                    )));
                }
                return Ok(None);
            }
            let extensible = rt.is_extensible(target)?;
            let desc = rt.parse_property_descriptor(result)?;
            let property = complete_descriptor(&desc);
            if !rt.is_compatible(extensible, &descriptor_of(property), target_desc)? {
                return Err(VmError::type_error(format!(
                    "'getOwnPropertyDescriptor' on proxy: trap returned descriptor for property '{}' that is incompatible with the existing property in the proxy target",
                    rt.key_text(key)
                )));
            }
            if !property.configurable() {
                match target_desc {
                    Some(target_desc) if !target_desc.configurable() => {
                        if let (
                            Property::Data {
                                writable: false, ..
                            },
                            Property::Data { writable: true, .. },
                        ) = (property, target_desc)
                        {
                            return Err(VmError::type_error(format!(
                                "'getOwnPropertyDescriptor' on proxy: trap reported non-configurable and non-writable for property '{}' which is non-configurable, writable in the proxy target",
                                rt.key_text(key)
                            )));
                        }
                    }
                    _ => {
                        return Err(VmError::type_error(format!(
                            "'getOwnPropertyDescriptor' on proxy: trap reported non-configurability for property '{}' which is either non-existent or configurable in the proxy target",
                            rt.key_text(key)
                        )));
                    }
                }
            }
            Ok(Some(property))
        })
    }

    /// `IsCompatiblePropertyDescriptor` (§10.1.6.2).
    fn is_compatible(
        &self,
        extensible: bool,
        desc: &PropertyDescriptor,
        current: Option<Property>,
    ) -> VmResult<bool> {
        let decision =
            crate::object::validate_and_apply_descriptor(&self.heap, extensible, desc, current)?;
        Ok(decision)
    }

    /// `[[DefineOwnProperty]]` (§10.5.6).
    #[cold]
    #[inline(never)]
    fn proxy_define_own_property(
        &mut self,
        proxy: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> VmResult<bool> {
        self.proxy_level(|rt| {
            for value in [desc.value, desc.get, desc.set].into_iter().flatten() {
                rt.heap.record(value);
            }
            // The key may be an atom that only the caller's Rust code holds.
            rt.heap.record(key);
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::DefineProperty)?;
            let Some(trap) = trap else {
                return rt.define_generic(target, key, desc, throw);
            };
            let desc_object = rt.partial_descriptor_object(&desc)?;
            let key_value = rt.heap.key_to_value(key)?;
            let result = rt.call(
                trap,
                handler.into(),
                &[target.into(), key_value, desc_object],
            )?;
            if !rt.to_boolean(result)? {
                if throw {
                    return Err(VmError::type_error(format!(
                        "'defineProperty' on proxy: trap returned falsish for property '{}'",
                        rt.key_text(key)
                    )));
                }
                return Ok(false);
            }
            let target_desc = rt.own_property(target, key)?;
            let extensible = rt.is_extensible(target)?;
            let setting_config_false = desc.configurable == Some(false);
            let non_configurable = || {
                VmError::type_error(format!(
                    "'defineProperty' on proxy: trap returned truish for defining non-configurable property '{}' which is either non-existent or configurable in the proxy target",
                    rt.key_text(key)
                ))
            };
            let Some(target_desc) = target_desc else {
                if !extensible {
                    return Err(VmError::type_error(format!(
                        "'defineProperty' on proxy: trap returned truish for adding property '{}'  to the non-extensible proxy target",
                        rt.key_text(key)
                    )));
                }
                if setting_config_false {
                    return Err(non_configurable());
                }
                return Ok(true);
            };
            if !rt.is_compatible(extensible, &desc, Some(target_desc))? {
                return Err(VmError::type_error(format!(
                    "'defineProperty' on proxy: trap returned truish for adding property '{}'  that is incompatible with the existing property in the proxy target",
                    rt.key_text(key)
                )));
            }
            if setting_config_false && target_desc.configurable() {
                return Err(non_configurable());
            }
            if let Property::Data {
                writable: true,
                configurable: false,
                ..
            } = target_desc
                && desc.writable == Some(false)
            {
                return Err(VmError::type_error(format!(
                    "'defineProperty' on proxy: trap returned truish for defining non-configurable property '{}' which cannot be non-writable, unless there exists a corresponding non-configurable, non-writable own property of the target object.",
                    rt.key_text(key)
                )));
            }
            Ok(true)
        })
    }

    /// `[[HasProperty]]` (§10.5.7).
    #[cold]
    #[inline(never)]
    fn proxy_has(&mut self, proxy: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        self.proxy_level(|rt| {
            // The key may be an atom that only the caller's Rust code holds.
            rt.heap.record(key);
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::Has)?;
            let Some(trap) = trap else {
                return rt.has_property(target, key);
            };
            let key_value = rt.heap.key_to_value(key)?;
            let result = rt.call(trap, handler.into(), &[target.into(), key_value])?;
            if rt.to_boolean(result)? {
                return Ok(true);
            }
            if let Some(target_desc) = rt.own_property(target, key)? {
                if !target_desc.configurable() {
                    return Err(VmError::type_error(format!(
                        "'has' on proxy: trap returned falsish for property '{}' which exists in the proxy target as non-configurable",
                        rt.key_text(key)
                    )));
                }
                if !rt.is_extensible(target)? {
                    return Err(VmError::type_error(format!(
                        "'has' on proxy: trap returned falsish for property '{}' but the proxy target is not extensible",
                        rt.key_text(key)
                    )));
                }
            }
            Ok(false)
        })
    }

    /// `[[Get]]` (§10.5.8). Without a trap, the target's accessor is
    /// returned for the caller to call.
    #[cold]
    #[inline(never)]
    pub(crate) fn proxy_get(
        &mut self,
        proxy: Gc<Object>,
        key: PropertyKey,
        receiver: Value,
    ) -> VmResult<Lookup> {
        self.proxy_level(|rt| {
            rt.heap.record(receiver);
            // The key may be an atom that only the caller's Rust code holds.
            rt.heap.record(key);
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::Get)?;
            let Some(trap) = trap else {
                return rt.get_with_receiver(target, key, receiver);
            };
            let key_value = rt.heap.key_to_value(key)?;
            let result = rt.call(
                trap,
                handler.into(),
                &[target.into(), key_value, receiver],
            )?;
            match rt.own_property(target, key)? {
                Some(Property::Data {
                    value,
                    writable: false,
                    configurable: false,
                    ..
                }) if !rt.heap.same_value(result, value)? => {
                    Err(VmError::type_error(format!(
                        "'get' on proxy: property '{}' is a read-only and non-configurable data property on the proxy target but the proxy did not return its actual value (expected '{}' but got '{}')",
                        rt.key_text(key),
                        rt.trap_value_text(value),
                        rt.trap_value_text(result)
                    )))
                }
                Some(Property::Accessor {
                    get: Value::Undefined,
                    configurable: false,
                    ..
                }) if !result.is_undefined() => Err(VmError::type_error(format!(
                    "'get' on proxy: property '{}' is a non-configurable accessor property on the proxy target and does not have a getter function, but the trap did not return 'undefined' (got '{}')",
                    rt.key_text(key),
                    rt.trap_value_text(result)
                ))),
                _ => Ok(Lookup::Value(result)),
            }
        })
    }

    /// `[[Set]]` (§10.5.9).
    #[cold]
    #[inline(never)]
    fn proxy_set(
        &mut self,
        proxy: Gc<Object>,
        key: PropertyKey,
        value: Value,
        receiver: Value,
        throw: bool,
    ) -> VmResult<SetDone> {
        self.proxy_level(|rt| {
            rt.heap.record(value);
            rt.heap.record(receiver);
            // The key may be an atom that only the caller's Rust code holds.
            rt.heap.record(key);
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::Set)?;
            let Some(trap) = trap else {
                return rt.set_with_receiver(target, key, value, receiver, throw);
            };
            let key_value = rt.heap.key_to_value(key)?;
            let result = rt.call(
                trap,
                handler.into(),
                &[target.into(), key_value, value, receiver],
            )?;
            if !rt.to_boolean(result)? {
                if throw {
                    return Err(VmError::type_error(format!(
                        "'set' on proxy: trap returned falsish for property '{}'",
                        rt.key_text(key)
                    )));
                }
                return Ok(SetDone::Done(false));
            }
            match rt.own_property(target, key)? {
                Some(Property::Data {
                    value: target_value,
                    writable: false,
                    configurable: false,
                    ..
                }) if !rt.heap.same_value(value, target_value)? => {
                    Err(VmError::type_error(format!(
                        "'set' on proxy: trap returned truish for property '{}' which exists in the proxy target as a non-configurable and non-writable data property with a different value",
                        rt.key_text(key)
                    )))
                }
                Some(Property::Accessor {
                    set: Value::Undefined,
                    configurable: false,
                    ..
                }) => Err(VmError::type_error(format!(
                    "'set' on proxy: trap returned truish for property '{}' which exists in the proxy target as a non-configurable and non-writable accessor property without a setter",
                    rt.key_text(key)
                ))),
                _ => Ok(SetDone::Done(true)),
            }
        })
    }

    /// `[[Delete]]` (§10.5.10).
    #[cold]
    #[inline(never)]
    fn proxy_delete(&mut self, proxy: Gc<Object>, key: PropertyKey, throw: bool) -> VmResult<bool> {
        self.proxy_level(|rt| {
            // The key may be an atom that only the caller's Rust code holds.
            rt.heap.record(key);
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::DeleteProperty)?;
            let Some(trap) = trap else {
                return rt.delete_generic(target, key, throw);
            };
            let key_value = rt.heap.key_to_value(key)?;
            let result = rt.call(trap, handler.into(), &[target.into(), key_value])?;
            if !rt.to_boolean(result)? {
                if throw {
                    return Err(VmError::type_error(format!(
                        "'deleteProperty' on proxy: trap returned falsish for property '{}'",
                        rt.key_text(key)
                    )));
                }
                return Ok(false);
            }
            let Some(target_desc) = rt.own_property(target, key)? else {
                return Ok(true);
            };
            if !target_desc.configurable() {
                return Err(VmError::type_error(format!(
                    "'deleteProperty' on proxy: trap returned truish for property '{}' which is non-configurable in the proxy target",
                    rt.key_text(key)
                )));
            }
            if !rt.is_extensible(target)? {
                return Err(VmError::type_error(format!(
                    "'deleteProperty' on proxy: trap returned truish for property '{}' but the proxy target is non-extensible",
                    rt.key_text(key)
                )));
            }
            Ok(true)
        })
    }

    /// `[[OwnPropertyKeys]]` (§10.5.11). The keys are recorded in the
    /// current handle scope. The duplicate check and the checks against
    /// the target's keys use a hash set, so they are linear.
    #[cold]
    #[inline(never)]
    pub(crate) fn proxy_own_keys(&mut self, proxy: Gc<Object>) -> VmResult<Vec<PropertyKey>> {
        self.proxy_level(|rt| {
            let (target, handler, trap) = rt.proxy_trap(proxy, Trap::OwnKeys)?;
            let Some(trap) = trap else {
                return rt.own_keys(target);
            };
            let array = rt.call(trap, handler.into(), &[target.into()])?;
            let keys = rt.list_of_keys(array)?;
            let mut unchecked: HashSet<PropertyKey, BuildKeyedHasher> = HashSet::default();
            rt.heap
                .check_room(keys.len().saturating_mul(KEY_ENTRY_BYTES))?;
            unchecked
                .try_reserve(keys.len())
                .map_err(|_| VmError::from(crate::error::Termination::HeapLimit))?;
            for &key in &keys {
                if !unchecked.insert(key) {
                    return Err(VmError::type_error(
                        "'ownKeys' on proxy: trap returned duplicate entries",
                    ));
                }
            }
            rt.charge(keys.len())?;
            let extensible = rt.is_extensible(target)?;
            let target_keys = rt.own_keys(target)?;
            rt.heap.record_keys(&target_keys);
            let mut configurable = Vec::new();
            let mut non_configurable = Vec::new();
            for key in target_keys {
                rt.tick()?;
                match rt.own_property(target, key)? {
                    Some(desc) if !desc.configurable() => non_configurable.push(key),
                    _ => configurable.push(key),
                }
            }
            if extensible && non_configurable.is_empty() {
                return Ok(keys);
            }
            for key in non_configurable {
                if !unchecked.remove(&key) {
                    return Err(rt.missing_key(key));
                }
            }
            if extensible {
                return Ok(keys);
            }
            for key in configurable {
                if !unchecked.remove(&key) {
                    return Err(rt.missing_key(key));
                }
            }
            if !unchecked.is_empty() {
                return Err(VmError::type_error(
                    "'ownKeys' on proxy: trap returned extra keys but proxy target is non-extensible",
                ));
            }
            Ok(keys)
        })
    }

    /// The `TypeError` of a target key that the `ownKeys` trap left out.
    fn missing_key(&self, key: PropertyKey) -> VmError {
        VmError::type_error(format!(
            "'ownKeys' on proxy: trap result did not include '{}'",
            self.key_text(key)
        ))
    }

    /// `CreateListFromArrayLike(value, « String, Symbol »)` (§7.3.20) for
    /// the result of an `ownKeys` trap: the keys, recorded in the current
    /// handle scope. The list grows with a heap check at each doubling, so
    /// a huge `length` ends at the time or heap limit.
    fn list_of_keys(&mut self, value: Value) -> VmResult<Vec<PropertyKey>> {
        let Value::Object(list) = value else {
            return Err(VmError::type_error(
                "CreateListFromArrayLike called on non-object",
            ));
        };
        let length = self.length_of_array_like(list)?;
        let mut keys: Vec<PropertyKey> = Vec::new();
        let mut index = 0.0;
        while index < length {
            self.tick()?;
            if keys.len() == keys.capacity() {
                let grown = (keys.capacity() * 2).max(16);
                self.heap
                    .check_room(grown.saturating_mul(KEY_ENTRY_BYTES))?;
                keys.try_reserve(grown - keys.len())
                    .map_err(|_| VmError::from(crate::error::Termination::HeapLimit))?;
            }
            let element_key = self.index_key(index)?;
            let element = self.get_value(list.into(), element_key)?;
            let key = match element {
                Value::String(string) => self.string_key(string)?,
                Value::Symbol(symbol) => PropertyKey::Symbol(symbol),
                other => {
                    return Err(VmError::type_error(format!(
                        "{} is not a valid property name",
                        self.value_text(other)
                    )));
                }
            };
            keys.push(self.heap.record(key));
            index += 1.0;
        }
        Ok(keys)
    }

    /// The parts of a call of a callable proxy (§10.5.12): the function to
    /// call, its `this` and its arguments (the target with the same `this`
    /// and arguments, or the trap with the handler and
    /// `(target, this, argArray)`), for a tail call. `None` if the `apply`
    /// trap is not callable (the caller reports its own description of the
    /// callee, as V8 does).
    #[cold]
    #[inline(never)]
    pub(crate) fn proxy_call_parts(
        &mut self,
        proxy: Gc<Object>,
        this: Value,
        args: &[Value],
    ) -> VmResult<Option<(Value, Value, Vec<Value>)>> {
        self.with_scope(|rt| {
            rt.tick()?;
            let Some((target, handler, trap)) = rt.proxy_call_trap(proxy, Trap::Apply)? else {
                return Ok(None);
            };
            let Some(trap) = trap else {
                return Ok(Some((target.into(), this, args.to_vec())));
            };
            let list = rt.array_from_values(args)?;
            Ok(Some((
                trap,
                handler.into(),
                vec![target.into(), this, list.into()],
            )))
        })
    }

    /// `[[Construct]]` of a proxy (§10.5.13): `Ok(Err(call))` with the
    /// tail call of the target's `[[Construct]]` (callee, arguments,
    /// `new.target`) when the handler has no trap, `Ok(Ok(object))` with
    /// the trap's result, `None` if the trap is not callable.
    #[allow(clippy::type_complexity)]
    #[cold]
    #[inline(never)]
    pub(crate) fn proxy_construct(
        &mut self,
        proxy: Gc<Object>,
        args: &[Value],
        new_target: Gc<Object>,
    ) -> VmResult<Option<Result<Value, (Value, Vec<Value>, Gc<Object>)>>> {
        self.with_scope(|rt| {
            rt.tick()?;
            let Some((target, handler, trap)) = rt.proxy_call_trap(proxy, Trap::Construct)? else {
                return Ok(None);
            };
            let Some(trap) = trap else {
                return Ok(Some(Err((target.into(), args.to_vec(), new_target))));
            };
            let list = rt.array_from_values(args)?;
            let result = rt.call(
                trap,
                handler.into(),
                &[target.into(), list.into(), new_target.into()],
            )?;
            if !matches!(result, Value::Object(_)) {
                return Err(VmError::type_error(format!(
                    "'construct' on proxy: trap returned non-object ('{}')",
                    rt.value_text(result)
                )));
            }
            Ok(Some(Ok(result)))
        })
    }

    /// [`Runtime::proxy_trap`] for `apply` and `construct`: `None` when
    /// the trap is a value that is not callable.
    fn proxy_call_trap(&mut self, proxy: Gc<Object>, trap: Trap) -> VmResult<Option<TrapParts>> {
        let data = self
            .heap
            .proxy_data(proxy)?
            .ok_or(VmError::invariant("a proxy call of another object"))?;
        let (Some(target), Some(handler)) = (data.target, data.handler) else {
            return Err(revoked_error(trap.name()));
        };
        self.heap.record(target);
        self.heap.record(handler);
        let method = self.get_value(handler.into(), self.trap_key(trap))?;
        if method.is_nullish() {
            return Ok(Some((target, handler, None)));
        }
        if !self.is_callable(method)? {
            return Ok(None);
        }
        Ok(Some((target, handler, Some(method))))
    }

    /// `ProxyCreate(target, handler)` (§10.5.15): a new proxy, recorded in
    /// the current handle scope.
    pub(crate) fn proxy_create(&mut self, target: Value, handler: Value) -> VmResult<Gc<Object>> {
        let (Value::Object(target), Value::Object(handler)) = (target, handler) else {
            return Err(VmError::type_error(
                "Cannot create proxy with a non-object as target or handler",
            ));
        };
        let kind = &self.heap.object(target)?.kind;
        let callable = crate::vm::function::is_callable_kind(kind);
        let constructor = crate::vm::function::is_constructor_kind(kind);
        let data = crate::object::ProxyData {
            target: Some(target),
            handler: Some(handler),
            callable,
            constructor,
        };
        let proxy = self
            .heap
            .new_object_with_kind(None, ObjectKind::Proxy(Box::new(data)))?;
        Ok(self.heap.record(proxy))
    }

    /// `IsArray(value)` (§7.2.2): through proxies, in a loop; a revoked
    /// proxy is V8's `TypeError`.
    pub(crate) fn is_array(&mut self, value: Value) -> VmResult<bool> {
        let Value::Object(mut object) = value else {
            return Ok(false);
        };
        loop {
            match &self.heap.object(object)?.kind {
                ObjectKind::Array { .. } => return Ok(true),
                ObjectKind::Proxy(data) => {
                    let (Some(target), Some(_)) = (data.target, data.handler) else {
                        return Err(revoked_error("IsArray"));
                    };
                    self.tick()?;
                    object = target;
                }
                _ => return Ok(false),
            }
        }
    }

    /// The innermost target of a chain of proxies (the object itself if
    /// it is not a proxy), or `None` if a proxy on the way is revoked. No
    /// script code runs.
    pub(crate) fn proxy_innermost(&self, object: Gc<Object>) -> Option<Gc<Object>> {
        let mut current = object;
        // A chain longer than this is not followed for messages.
        for _ in 0..1_000_000 {
            match &self.heap.object(current).ok()?.kind {
                ObjectKind::Proxy(data) => current = data.target?,
                _ => return Some(current),
            }
        }
        None
    }
}

/// `CompletePropertyDescriptor` (§6.2.6.6) of a descriptor that
/// `ToPropertyDescriptor` made: the property it describes.
fn complete_descriptor(desc: &PropertyDescriptor) -> Property {
    let enumerable = desc.enumerable.unwrap_or(false);
    let configurable = desc.configurable.unwrap_or(false);
    if desc.is_accessor() {
        Property::Accessor {
            get: desc.get.unwrap_or(Value::Undefined),
            set: desc.set.unwrap_or(Value::Undefined),
            enumerable,
            configurable,
        }
    } else {
        Property::Data {
            value: desc.value.unwrap_or(Value::Undefined),
            writable: desc.writable.unwrap_or(false),
            enumerable,
            configurable,
        }
    }
}

/// The complete descriptor of a property.
fn descriptor_of(property: Property) -> PropertyDescriptor {
    match property {
        Property::Data {
            value,
            writable,
            enumerable,
            configurable,
        } => PropertyDescriptor::data(value, writable, enumerable, configurable),
        Property::Accessor {
            get,
            set,
            enumerable,
            configurable,
        } => PropertyDescriptor::accessor(get, set, enumerable, configurable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeConfig;

    fn eval_text(rt: &mut Runtime, source: &str) -> String {
        let value = rt.eval(source).expect("the script runs");
        rt.to_rust_string(value).expect("a text")
    }

    #[test]
    fn complete_descriptor_fills_the_defaults() {
        let desc = PropertyDescriptor {
            value: Some(Value::Int(1)),
            ..PropertyDescriptor::default()
        };
        assert_eq!(
            complete_descriptor(&desc),
            Property::Data {
                value: Value::Int(1),
                writable: false,
                enumerable: false,
                configurable: false,
            }
        );
        let desc = PropertyDescriptor {
            get: Some(Value::Undefined),
            configurable: Some(true),
            ..PropertyDescriptor::default()
        };
        let property = complete_descriptor(&desc);
        assert_eq!(
            property,
            Property::Accessor {
                get: Value::Undefined,
                set: Value::Undefined,
                enumerable: false,
                configurable: true,
            }
        );
        assert_eq!(descriptor_of(property).set, Some(Value::Undefined));
    }

    /// A proxy keeps its target and handler alive, and a revoked proxy
    /// holds neither (the collector traces the slots).
    #[test]
    fn the_slots_are_traced_and_revocation_drops_them() {
        for stress in [false, true] {
            let mut config = RuntimeConfig::default();
            config.heap.stress = stress;
            let mut rt = Runtime::new(config).expect("a runtime");
            let source = "var r = Proxy.revocable({ a: [1, 2, 3] }, { get: function (t, k) { return t[k]; } }); \
                 var p = new Proxy(function () { return 'called'; }, {}); \
                 for (var i = 0; i < 2000; i++) ({ junk: [i] }); \
                 String(r.proxy.a) + ' ' + p()";
            assert_eq!(
                eval_text(&mut rt, source),
                "1,2,3 called",
                "stress {stress}"
            );
            rt.collect();
            let before = rt.heap().counts().objects;
            rt.eval("r.proxy; r.revoke(); r = null;").expect("revoke");
            rt.collect();
            assert!(rt.heap().counts().objects < before, "stress {stress}");
            assert_eq!(rt.heap().stats().total_stale_roots, 0);
        }
    }

    #[test]
    fn is_array_sees_through_proxies_and_throws_for_a_revoked_one() {
        let mut rt = Runtime::new(RuntimeConfig::default()).expect("a runtime");
        let cases = [
            ("Array.isArray(new Proxy(new Proxy([], {}), {}))", "true"),
            ("Array.isArray(new Proxy({}, {}))", "false"),
            (
                "var r = Proxy.revocable([], {}); r.revoke(); try { Array.isArray(r.proxy); } catch (e) { e.message }",
                "Cannot perform 'IsArray' on a proxy that has been revoked",
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(eval_text(&mut rt, source), expected, "{source}");
        }
    }
}
