//! The internal methods of `WindowProxy` exotic objects (HTML §7.2.3,
//! <https://html.spec.whatwg.org/multipage/nav-history-apis.html#the-windowproxy-exotic-object>;
//! ADR 0031 section 5).
//!
//! The generic internal methods (`proxy.rs`) treat a `WindowProxy` as they
//! treat a proxy and hand it to the functions of this module. With a target
//! (the same-origin case of the HTML steps), each method forwards to the
//! target, the global object of a realm of this runtime; the receiver of a
//! `[[Get]]` or a `[[Set]]` stays the proxy, so that a getter sees it as its
//! `this`. Without a target, each method calls the host's hooks
//! ([`WindowProxyHooks`]), which write the rules for cross-origin objects.
//!
//! The target cannot be a proxy and its prototype chain cannot reach one:
//! the host makes the global object and the objects of its chain immutable
//! prototype objects. So a forwarded method does not recur through proxies;
//! it needs no recursion budget of its own.

use crate::heap::Gc;
use crate::object::{Object, Property, PropertyDescriptor, WindowProxySlot};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::internal::ProtoSet;
use crate::vm::property::Lookup;
use crate::vm::proxy::SetDone;
use crate::vm::{VmError, VmResult};

/// `[[GetPrototypeOf]]` of a [`WindowProxyHooks`].
pub type GetPrototypeHook = fn(&mut Runtime, Gc<Object>) -> VmResult<Option<Gc<Object>>>;

/// `[[SetPrototypeOf]]` of a [`WindowProxyHooks`].
pub type SetPrototypeHook = fn(&mut Runtime, Gc<Object>, Option<Gc<Object>>) -> VmResult<bool>;

/// `[[GetOwnProperty]]` of a [`WindowProxyHooks`].
pub type WindowOwnPropertyHook =
    fn(&mut Runtime, Gc<Object>, PropertyKey) -> VmResult<Option<Property>>;

/// `[[DefineOwnProperty]]` of a [`WindowProxyHooks`].
pub type WindowDefineHook =
    fn(&mut Runtime, Gc<Object>, PropertyKey, &PropertyDescriptor) -> VmResult<bool>;

/// `child_frame` of a [`WindowProxyHooks`]: the object and the index.
pub type ChildFrameHook = fn(&mut Runtime, Gc<Object>, u32) -> VmResult<Option<Gc<Object>>>;

/// The internal methods of a `WindowProxy` without a target. Each hook
/// follows the steps of HTML §7.2.3 for a cross-origin `Window`; it may
/// throw. `[[IsExtensible]]` is not a hook: it is always true.
#[derive(Clone, Copy, Debug)]
pub struct WindowProxyHooks {
    /// `[[GetPrototypeOf]]`.
    pub get_prototype_of: GetPrototypeHook,
    /// `[[SetPrototypeOf]]`.
    pub set_prototype_of: SetPrototypeHook,
    /// `[[PreventExtensions]]`.
    pub prevent_extensions: fn(&mut Runtime, Gc<Object>) -> VmResult<bool>,
    /// `[[GetOwnProperty]]`.
    pub get_own_property: WindowOwnPropertyHook,
    /// `[[DefineOwnProperty]]`.
    pub define_own_property: WindowDefineHook,
    /// `[[HasProperty]]`.
    pub has: fn(&mut Runtime, Gc<Object>, PropertyKey) -> VmResult<bool>,
    /// `[[Get]]`: the object, the key and the receiver.
    pub get: fn(&mut Runtime, Gc<Object>, PropertyKey, Value) -> VmResult<Value>,
    /// `[[Set]]`: the object, the key, the value and the receiver.
    pub set: fn(&mut Runtime, Gc<Object>, PropertyKey, Value, Value) -> VmResult<bool>,
    /// `[[Delete]]`.
    pub delete: fn(&mut Runtime, Gc<Object>, PropertyKey) -> VmResult<bool>,
    /// `[[OwnPropertyKeys]]`.
    pub own_keys: fn(&mut Runtime, Gc<Object>) -> VmResult<Vec<PropertyKey>>,
    /// The number of child frames of the frame of a proxy (HTML "document-tree
    /// child navigables"). A proxy with a target asks it too: the index keys
    /// of a `WindowProxy` are the child frames, not properties of the target.
    pub child_count: fn(&mut Runtime, Gc<Object>) -> VmResult<u32>,
    /// The `WindowProxy` of the child frame at an index, in tree order.
    pub child_frame: ChildFrameHook,
}

impl Runtime {
    // --- The embedding API ---

    /// A new `WindowProxy` of the host's frame `frame` (below 2^31). A target
    /// must be the global object of a realm of this runtime; without one,
    /// the internal methods call the hooks of
    /// [`Runtime::set_window_proxy_hooks`]. Recorded in the current handle
    /// scope if one is open; otherwise the host roots it.
    pub fn new_window_proxy(
        &mut self,
        frame: u32,
        target: Option<Gc<Object>>,
    ) -> VmResult<Gc<Object>> {
        self.check_window_target(target)?;
        let slot = WindowProxySlot::new(frame, target)?;
        let proxy = self
            .heap
            .new_object_with_kind(None, crate::object::ObjectKind::WindowProxy(slot))?;
        Ok(self.keep(proxy))
    }

    /// Changes the target of a `WindowProxy` (a navigation commits, or the
    /// frame's document moves to another agent). The proxy keeps its
    /// identity; what was set through it stays on the old target, and an
    /// inline cache of the old target misses.
    pub fn set_window_proxy_target(
        &mut self,
        proxy: Gc<Object>,
        target: Option<Gc<Object>>,
    ) -> VmResult<()> {
        self.check_window_target(target)?;
        Ok(self.heap.set_window_proxy_target(proxy, target)?)
    }

    /// The target of `object` if it is a `WindowProxy` with a target, else
    /// `object` (for the texts of messages and of the console).
    pub(crate) fn through_window_proxy(&self, object: Gc<Object>) -> Gc<Object> {
        match self.heap.window_proxy(object) {
            Ok(Some(slot)) => slot.target().unwrap_or(object),
            _ => object,
        }
    }

    /// The target of `value`, if it is a `WindowProxy` with a target.
    pub fn window_proxy_target(&self, value: Value) -> Option<Gc<Object>> {
        let Value::Object(object) = value else {
            return None;
        };
        self.heap.window_proxy(object).ok()??.target()
    }

    /// The host's frame id of `value`, if it is a `WindowProxy`.
    pub fn window_proxy_frame(&self, value: Value) -> Option<u32> {
        let Value::Object(object) = value else {
            return None;
        };
        Some(self.heap.window_proxy(object).ok()??.frame())
    }

    /// Sets the internal methods of the `WindowProxy` objects without a
    /// target.
    pub fn set_window_proxy_hooks(&mut self, hooks: WindowProxyHooks) {
        self.vm.host.window_proxy = Some(hooks);
    }

    /// Refuses a target that is not the global object of a realm.
    fn check_window_target(&self, target: Option<Gc<Object>>) -> VmResult<()> {
        match target {
            Some(target) if self.realm_of_global(target).is_none() => Err(VmError::invariant(
                "a WindowProxy target that is no global object",
            )),
            _ => Ok(()),
        }
    }

    /// The hooks; a runtime without hooks refuses every access.
    fn window_hooks(&self) -> VmResult<WindowProxyHooks> {
        self.vm.host.window_proxy.ok_or(VmError::type_error(
            "Blocked a frame from accessing a window",
        ))
    }

    /// The child frame of `proxy` at `index` (HTML §7.2.3.3 step 2.2), through
    /// the hooks; a runtime without hooks has no frames.
    fn window_child(&mut self, proxy: Gc<Object>, index: u32) -> VmResult<Option<Gc<Object>>> {
        let Some(hooks) = self.vm.host.window_proxy else {
            return Ok(None);
        };
        self.heap.record(proxy);
        let child = self.hook_level(|rt| (hooks.child_frame)(rt, proxy, index))?;
        if let Some(child) = child {
            self.heap.record(child);
        }
        Ok(child)
    }

    // --- The internal methods ---

    /// `[[GetPrototypeOf]]`.
    pub(crate) fn window_get_prototype_of(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
    ) -> VmResult<Option<Gc<Object>>> {
        if let Some(target) = slot.target() {
            return Ok(self.heap.get_prototype_of(target)?);
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        let proto = self.hook_level(|rt| (hooks.get_prototype_of)(rt, proxy))?;
        if let Some(proto) = proto {
            self.heap.record(proto);
        }
        Ok(proto)
    }

    /// `[[SetPrototypeOf]]`: `SetImmutablePrototype` (ECMA-262 §10.4.7.2).
    pub(crate) fn window_set_prototype_of(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
        proto: Option<Gc<Object>>,
    ) -> VmResult<ProtoSet> {
        let same = if let Some(target) = slot.target() {
            self.heap.get_prototype_of(target)? == proto
        } else {
            let hooks = self.window_hooks()?;
            self.heap.record(proxy);
            self.hook_level(|rt| (hooks.set_prototype_of)(rt, proxy, proto))?
        };
        Ok(if same {
            ProtoSet::Done
        } else {
            ProtoSet::Immutable
        })
    }

    /// `[[PreventExtensions]]`: "Return false." The caller words the error
    /// (measured: `Object.preventExtensions`, `freeze` and `seal` have the
    /// `TypeError`s of a typed array that refuses).
    pub(crate) fn window_prevent_extensions(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
    ) -> VmResult<bool> {
        if slot.target().is_some() {
            return Ok(false);
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        self.hook_level(|rt| (hooks.prevent_extensions)(rt, proxy))
    }

    /// `[[GetOwnProperty]]`.
    pub(crate) fn window_get_own_property(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        // The key may be an atom that only the caller's Rust code holds.
        self.heap.record(key);
        if let Some(target) = slot.target() {
            // HTML §7.2.3.3: an array index names a child frame (measured:
            // not writable, enumerable, configurable), and nothing else.
            if let PropertyKey::Index(index) = key {
                return Ok(self
                    .window_child(proxy, index)?
                    .map(|child| Property::Data {
                        value: child.into(),
                        writable: false,
                        enumerable: true,
                        configurable: true,
                    }));
            }
            return self.own_property(target, key);
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        let property = self.hook_level(|rt| (hooks.get_own_property)(rt, proxy, key))?;
        if let Some(property) = property {
            let (first, second) = property.slot_values();
            self.heap.record(first);
            if let Some(second) = second {
                self.heap.record(second);
            }
        }
        Ok(property)
    }

    /// `[[DefineOwnProperty]]`.
    pub(crate) fn window_define_own_property(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
        key: PropertyKey,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> VmResult<bool> {
        self.heap.record(key);
        if let Some(target) = slot.target() {
            // HTML §7.2.3.5 refuses an array index. Chromium throws in
            // `DefinePropertyOrThrow` and otherwise answers true without a
            // property (measured, `Reflect.defineProperty`).
            if let PropertyKey::Index(index) = key {
                if throw {
                    return Err(VmError::type_error(format!(
                        "Failed to set an indexed property [{index}] on 'Window': Index property setter is not supported."
                    )));
                }
                return Ok(true);
            }
            return self.define_generic(target, key, desc, throw);
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        for value in [desc.value, desc.get, desc.set].into_iter().flatten() {
            self.heap.record(value);
        }
        let done = self.hook_level(|rt| (hooks.define_own_property)(rt, proxy, key, &desc))?;
        if !done && throw {
            return Err(self.define_failure(proxy, key));
        }
        Ok(done)
    }

    /// `[[HasProperty]]`.
    pub(crate) fn window_has(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
        key: PropertyKey,
    ) -> VmResult<bool> {
        self.heap.record(key);
        if let Some(target) = slot.target() {
            if let PropertyKey::Index(index) = key
                && self.window_child(proxy, index)?.is_some()
            {
                return Ok(true);
            }
            return self.has_property(target, key);
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        self.hook_level(|rt| (hooks.has)(rt, proxy, key))
    }

    /// `[[Get]]`.
    pub(crate) fn window_get(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
        key: PropertyKey,
        receiver: Value,
    ) -> VmResult<Lookup> {
        self.heap.record(key);
        if let Some(target) = slot.target() {
            if let PropertyKey::Index(index) = key
                && let Some(child) = self.window_child(proxy, index)?
            {
                return Ok(Lookup::Value(child.into()));
            }
            return self.get_with_receiver(target, key, receiver);
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        self.heap.record(receiver);
        let value = self.hook_level(|rt| (hooks.get)(rt, proxy, key, receiver))?;
        Ok(Lookup::Value(self.heap.record(value)))
    }

    /// `[[Set]]`. A write through the proxy itself runs as a write to the
    /// target with the target as the receiver, without the steps through the
    /// proxy's `[[GetOwnProperty]]` and `[[DefineOwnProperty]]`, which
    /// forward to the same object; a setter still sees the proxy.
    pub(crate) fn window_set(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
        (key, value, receiver): (PropertyKey, Value, Value),
        throw: bool,
    ) -> VmResult<SetDone> {
        self.heap.record(key);
        self.heap.record(value);
        self.heap.record(receiver);
        if let Some(target) = slot.target() {
            if receiver != Value::Object(proxy) {
                return self.set_with_receiver(target, key, value, receiver, throw);
            }
            // An array index: no property, an error in strict code only
            // (Chromium's wording, measured; it differs from the wording of
            // `[[DefineOwnProperty]]`).
            if let PropertyKey::Index(index) = key {
                if throw {
                    return Err(VmError::type_error(format!(
                        "Failed to set an indexed property [{index}] on 'Window': Indexed property setter is not supported."
                    )));
                }
                return Ok(SetDone::Done(true));
            }
            return Ok(
                match self.set_with_receiver(target, key, value, target.into(), throw)? {
                    SetDone::Setter { setter, .. } => SetDone::Setter { setter, receiver },
                    done @ SetDone::Done(_) => done,
                },
            );
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        let done = self.hook_level(|rt| (hooks.set)(rt, proxy, key, value, receiver))?;
        if !done && throw {
            return Err(self.failed_write(proxy, key));
        }
        Ok(SetDone::Done(done))
    }

    /// `[[Delete]]`.
    pub(crate) fn window_delete(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
        key: PropertyKey,
        throw: bool,
    ) -> VmResult<bool> {
        self.heap.record(key);
        if let Some(target) = slot.target() {
            // HTML §7.2.3.6: an array index of a child frame stays; any other
            // has no property (measured, case `scripted-window-delete-index`).
            if let PropertyKey::Index(index) = key {
                let child = self.window_child(proxy, index)?;
                if child.is_some() && throw {
                    return Err(VmError::type_error(format!(
                        "Failed to delete an indexed property [{index}] from 'Window': Index property deleter is not supported."
                    )));
                }
                return Ok(child.is_none());
            }
            return self.delete_generic(target, key, throw);
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        let deleted = self.hook_level(|rt| (hooks.delete)(rt, proxy, key))?;
        if !deleted && throw {
            return Err(VmError::type_error(format!(
                "Cannot delete property '{}' of {}",
                self.key_text(key),
                self.object_text(proxy)
            )));
        }
        Ok(deleted)
    }

    /// `[[OwnPropertyKeys]]`.
    pub(crate) fn window_own_keys(
        &mut self,
        proxy: Gc<Object>,
        slot: WindowProxySlot,
    ) -> VmResult<Vec<PropertyKey>> {
        if let Some(target) = slot.target() {
            let keys = self.own_keys(target)?;
            // HTML §7.2.3.8: the indices of the child frames come first.
            let count = match self.vm.host.window_proxy {
                Some(hooks) => {
                    self.heap.record(proxy);
                    self.hook_level(|rt| (hooks.child_count)(rt, proxy))?
                }
                None => 0,
            };
            if count == 0 {
                return Ok(keys);
            }
            let mut all: Vec<PropertyKey> = (0..count).map(PropertyKey::Index).collect();
            all.extend(keys);
            return Ok(all);
        }
        let hooks = self.window_hooks()?;
        self.heap.record(proxy);
        let keys = self.hook_level(|rt| (hooks.own_keys)(rt, proxy))?;
        self.heap.record_keys(&keys);
        Ok(keys)
    }
}
