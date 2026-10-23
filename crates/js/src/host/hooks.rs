//! Hooks for the internal methods of host objects (ADR 0028 section 6,
//! item 2): the legacy platform objects of Web IDL (§3.9, indexed and named
//! properties) and the named properties object of the global object.
//!
//! A class with hooks makes `ObjectKind::HostExotic` objects. The heap's
//! walks stop at such an object with `Generic`, as at a proxy, and the
//! runtime's generic internal methods (`vm/proxy.rs`, `vm/internal.rs`,
//! `vm/property.rs`) call the functions below. Each hook may answer for a
//! key or leave it to the ordinary method (`None`), which then works on the
//! object's stored properties (expandos). Without a `get` hook, `[[Get]]`
//! is the ordinary one, with the `get_own_property` hook at this object;
//! `[[HasProperty]]` uses the `get_own_property` hook the same way. A
//! `set` hook can also hand the own property to the ordinary steps
//! ([`SetAnswer`]). When the engine throws for a refused `[[Set]]`,
//! `[[DefineOwnProperty]]` or `[[Delete]]`, the `refusal` hook can give the
//! message ([`Refusal`]). `[[SetPrototypeOf]]` is not a hook: the objects
//! that Web IDL gives another one are immutable prototype objects
//! (`Runtime::set_immutable_prototype`), and most of them (the global
//! object, interface prototypes) must stay ordinary for the caches.
//!
//! A hook may run script code (a named setter converts its value with
//! `ToString`), allocate and collect. Each call charges the recursion
//! budget ([`HOOK_WEIGHT`]) and a time step, as a proxy's internal method
//! does, so hooks that reach each other end with V8's `RangeError`, not
//! with a stack overflow; script code that a hook calls charges a native
//! re-entry on top. The object, the key and the values of the call are
//! recorded in the caller's handle scope before the call, and the values
//! of the answer after it.

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{STACK_OVERFLOW, VmError, VmResult};

/// `[[GetOwnProperty]](P)`: `Some` for a property of the host (an index or
/// a name that the object supports), `None` for the ordinary method.
pub type GetOwnPropertyHook =
    fn(&mut Runtime, Gc<Object>, PropertyKey) -> VmResult<Option<Property>>;

/// `[[DefineOwnProperty]](P, Desc)`: `Some(result)`, or `None` for the
/// ordinary method.
pub type DefineOwnPropertyHook =
    fn(&mut Runtime, Gc<Object>, PropertyKey, &PropertyDescriptor) -> VmResult<Option<bool>>;

/// `[[Get]](P, Receiver)`: `Some(value)`, or `None` for the ordinary
/// method (which uses the `get_own_property` hook at this object).
pub type GetHook = fn(&mut Runtime, Gc<Object>, PropertyKey, Value) -> VmResult<Option<Value>>;

/// `[[Set]](P, V, Receiver)` (see [`SetAnswer`]).
pub type SetHook = fn(&mut Runtime, Gc<Object>, PropertyKey, Value, Value) -> VmResult<SetAnswer>;

/// The answer of a [`SetHook`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SetAnswer {
    /// The hook performed `[[Set]]`; its result.
    Done(bool),
    /// The ordinary method: `OrdinarySet`, which reads the own property
    /// with the `get_own_property` hook.
    Ordinary,
    /// `OrdinarySetWithOwnDescriptor` (ECMA-262 §10.1.9.2) with this own
    /// property of the object (Web IDL §3.9.2: the indexed property without
    /// the named ones). The value of a data property is not used.
    WithOwn(Property),
    /// `OrdinarySetWithOwnDescriptor` with the object's stored property
    /// (or none), without the `get_own_property` hook.
    WithStored,
}

/// `[[PreventExtensions]]()`: `Some(result)`, or `None` for the ordinary
/// method. Web IDL legacy platform objects answer `Some(false)` (§3.9.5).
pub type PreventExtensionsHook = fn(&mut Runtime, Gc<Object>) -> VmResult<Option<bool>>;

/// The internal method that refused, for a [`RefusalHook`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `[[Set]]` returned `false` in strict code (or `Reflect.set`'s caller
    /// throws).
    Set,
    /// `[[DefineOwnProperty]]` returned `false` in `DefinePropertyOrThrow`
    /// (`Object.defineProperty`).
    Define,
    /// `[[Delete]]` returned `false` in strict code.
    Delete,
}

/// The message of the `TypeError` for a refused internal method of the
/// object (Chromium words these per interface), or `None` for V8's own
/// message. Called only when the engine throws; it may read the host state
/// but should not run script code.
pub type RefusalHook =
    fn(&mut Runtime, Gc<Object>, PropertyKey, Refusal) -> VmResult<Option<String>>;

/// `[[Delete]](P)`: `Some(result)`, or `None` for the ordinary method.
pub type DeleteHook = fn(&mut Runtime, Gc<Object>, PropertyKey) -> VmResult<Option<bool>>;

/// `[[OwnPropertyKeys]]()`: appends the keys of the host to the list, in
/// the order that the host gives them (Web IDL §3.9.6: the supported
/// indices in ascending order, then the supported names); the runtime
/// appends the ordinary keys of the object after them and drops
/// duplicates.
pub type OwnKeysHook = fn(&mut Runtime, Gc<Object>, &mut Vec<PropertyKey>) -> VmResult<()>;

/// The hooks of a class. A missing hook is the ordinary method.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostHooks {
    /// `[[GetOwnProperty]]`.
    pub get_own_property: Option<GetOwnPropertyHook>,
    /// `[[DefineOwnProperty]]`.
    pub define_own_property: Option<DefineOwnPropertyHook>,
    /// `[[Get]]`.
    pub get: Option<GetHook>,
    /// `[[Set]]`.
    pub set: Option<SetHook>,
    /// `[[Delete]]`.
    pub delete: Option<DeleteHook>,
    /// `[[OwnPropertyKeys]]`.
    pub own_keys: Option<OwnKeysHook>,
    /// `[[PreventExtensions]]` (also behind `Object.freeze`,
    /// `Object.seal`, `Object.preventExtensions` and
    /// `Reflect.preventExtensions`).
    pub prevent_extensions: Option<PreventExtensionsHook>,
    /// The messages of refused writes, definitions and deletions.
    pub refusal: Option<RefusalHook>,
}

/// The recursion budget weight of a hook call: the Rust frames of the
/// generic method and of the hook (the weight of a proxy's internal
/// method, `vm/proxy.rs`).
pub(crate) const HOOK_WEIGHT: u32 = crate::vm::proxy::PROXY_WEIGHT;

impl Runtime {
    /// The hooks of `object`, if it is a host object of a class with hooks.
    #[inline]
    pub(crate) fn host_hooks(&self, object: Gc<Object>) -> VmResult<Option<HostHooks>> {
        let ObjectKind::HostExotic(slot) = self.heap.object(object)?.kind else {
            return Ok(None);
        };
        Ok(self.vm.host.class(slot.class())?.hooks)
    }

    /// Runs one hook call: [`HOOK_WEIGHT`] of the recursion budget and a
    /// time step.
    fn hook_level<T>(&mut self, f: impl FnOnce(&mut Runtime) -> VmResult<T>) -> VmResult<T> {
        if self.budget.enter(HOOK_WEIGHT).is_err() {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let result = self.tick().and_then(|()| f(self));
        self.budget.leave(HOOK_WEIGHT);
        result
    }

    /// `O.[[GetOwnProperty]](P)` of an object whose internal methods need
    /// the runtime (`Heap::needs_runtime`): a host object with hooks or a
    /// proxy.
    pub(crate) fn exotic_own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        match self.host_hooks(object)? {
            Some(hooks) => self.host_own_property(object, key, &hooks),
            None => self.proxy_get_own_property(object, key),
        }
    }

    /// `O.[[GetOwnProperty]](P)` of a host object with hooks.
    pub(crate) fn host_own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        hooks: &HostHooks,
    ) -> VmResult<Option<Property>> {
        if let Some(hook) = hooks.get_own_property {
            self.heap.record(object);
            self.heap.record(key);
            if let Some(property) = self.hook_level(|rt| hook(rt, object, key))? {
                self.record_property(property)?;
                return Ok(Some(property));
            }
        }
        Ok(self.heap.get_own_property(object, key)?)
    }

    /// `O.[[DefineOwnProperty]](P, Desc)` of a host object with hooks.
    pub(crate) fn host_define(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
        hooks: &HostHooks,
    ) -> VmResult<bool> {
        desc.check()?;
        if let Some(hook) = hooks.define_own_property {
            self.heap.record(object);
            self.heap.record(key);
            for value in [desc.value, desc.get, desc.set].into_iter().flatten() {
                self.heap.record(value);
            }
            if let Some(done) = self.hook_level(|rt| hook(rt, object, key, &desc))? {
                return Ok(done);
            }
        }
        Ok(self
            .heap
            .ordinary_define_own_property(object, key, desc, &self.vm)?)
    }

    /// The step of `O.[[Get]](P, Receiver)` at a host object with hooks:
    /// the value, a getter, or `None` when the object has no such own
    /// property (the walk goes on to its prototype).
    pub(crate) fn host_get(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        receiver: Value,
        hooks: &HostHooks,
    ) -> VmResult<Option<crate::vm::Lookup>> {
        use crate::vm::Lookup;
        if let Some(hook) = hooks.get {
            self.heap.record(object);
            self.heap.record(key);
            self.heap.record(receiver);
            if let Some(value) = self.hook_level(|rt| hook(rt, object, key, receiver))? {
                if value.is_internal() {
                    return Err(VmError::invariant("a host hook returned an internal value"));
                }
                return Ok(Some(Lookup::Value(self.heap.record(value))));
            }
        }
        Ok(match self.host_own_property(object, key, hooks)? {
            Some(Property::Data { value, .. }) => Some(Lookup::Value(value)),
            Some(Property::Accessor { get, .. }) if get.is_undefined() => {
                Some(Lookup::Value(Value::Undefined))
            }
            Some(Property::Accessor { get, .. }) => Some(Lookup::Getter {
                getter: get,
                receiver,
            }),
            None => None,
        })
    }

    /// The `set` hook of `O.[[Set]](P, V, Receiver)`: its answer
    /// ([`SetAnswer::Ordinary`] without a hook).
    pub(crate) fn host_set(
        &mut self,
        object: Gc<Object>,
        (key, value, receiver): (PropertyKey, Value, Value),
        hooks: &HostHooks,
    ) -> VmResult<SetAnswer> {
        let Some(hook) = hooks.set else {
            return Ok(SetAnswer::Ordinary);
        };
        self.heap.record(object);
        self.heap.record(key);
        self.heap.record(value);
        self.heap.record(receiver);
        let answer = self.hook_level(|rt| hook(rt, object, key, value, receiver))?;
        if let SetAnswer::WithOwn(property) = answer {
            self.record_property(property)?;
        }
        Ok(answer)
    }

    /// `O.[[PreventExtensions]]()` of a host object with hooks.
    pub(crate) fn host_prevent_extensions(
        &mut self,
        object: Gc<Object>,
        hooks: &HostHooks,
    ) -> VmResult<bool> {
        if let Some(hook) = hooks.prevent_extensions {
            self.heap.record(object);
            if let Some(done) = self.hook_level(|rt| hook(rt, object))? {
                return Ok(done);
            }
        }
        Ok(self.heap.prevent_extensions(object)?)
    }

    /// The host's `TypeError` for a refused internal method of `object`,
    /// if it is a host object whose class words it.
    pub(crate) fn host_refusal(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        why: Refusal,
    ) -> VmResult<Option<VmError>> {
        let Some(hook) = self.host_hooks(object)?.and_then(|hooks| hooks.refusal) else {
            return Ok(None);
        };
        self.heap.record(object);
        self.heap.record(key);
        let message = self.hook_level(|rt| hook(rt, object, key, why))?;
        Ok(message.map(VmError::type_error))
    }

    /// `O.[[Delete]](P)` of a host object with hooks.
    pub(crate) fn host_delete(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        hooks: &HostHooks,
    ) -> VmResult<bool> {
        if let Some(hook) = hooks.delete {
            self.heap.record(object);
            self.heap.record(key);
            if let Some(done) = self.hook_level(|rt| hook(rt, object, key))? {
                return Ok(done);
            }
        }
        Ok(self.heap.ordinary_delete(object, key)?)
    }

    /// `O.[[OwnPropertyKeys]]()` of a host object with hooks: the keys of
    /// the hook in its order, then the stored keys in the order of ordinary
    /// `[[OwnPropertyKeys]]` (ECMA-262 §10.1.11.1), without duplicates (Web
    /// IDL §3.9.6: the supported indices, the supported names, then the
    /// own properties; a supported name such as `"7"` stays among the
    /// names). The keys are recorded.
    pub(crate) fn host_own_keys(
        &mut self,
        object: Gc<Object>,
        hooks: &HostHooks,
    ) -> VmResult<Vec<PropertyKey>> {
        let mut keys = Vec::new();
        if let Some(hook) = hooks.own_keys {
            self.heap.record(object);
            self.hook_level(|rt| hook(rt, object, &mut keys))?;
            self.heap.record_keys(&keys);
        }
        let bound = self.heap.own_key_bound(object)?;
        self.charge(bound.saturating_add(keys.len()))?;
        let stored = self.heap.own_property_keys(object)?;
        if keys.is_empty() {
            return Ok(stored);
        }
        keys.try_reserve(stored.len())
            .map_err(crate::error::Error::from)?;
        keys.extend(stored);
        Ok(without_duplicates(keys))
    }

    /// Records the values of a property in the current handle scope.
    fn record_property(&mut self, property: Property) -> VmResult<()> {
        let values = match property {
            Property::Data { value, .. } => [value, Value::Undefined],
            Property::Accessor { get, set, .. } => [get, set],
        };
        for value in values {
            if value.is_internal() {
                return Err(VmError::invariant("a host hook returned an internal value"));
            }
            self.heap.record(value);
        }
        Ok(())
    }
}

/// Drops the later copies of keys that occur twice (a supported name that
/// is also a supported index, `"0"`).
fn without_duplicates(keys: Vec<PropertyKey>) -> Vec<PropertyKey> {
    let mut seen = std::collections::HashSet::with_capacity(keys.len());
    keys.into_iter().filter(|key| seen.insert(*key)).collect()
}
