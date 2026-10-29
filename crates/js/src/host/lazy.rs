//! Lazy creation of interfaces and of the properties of a global object
//! that hold them (ADR 0028 section 5, "Lazy interfaces notes"; section 6,
//! item 9).
//!
//! The host registers two hooks ([`Runtime::set_lazy_hooks`]):
//!
//! - **Interfaces.** [`Runtime::interface_or_create`] (and with it
//!   [`Runtime::new_host_object`] and [`Runtime::prototype_from_new_target`])
//!   asks the host to create the interface of a class that the realm does
//!   not have yet, with its ancestors and members.
//! - **Global properties.** [`Runtime::define_lazy_global`] defines a data
//!   property of a global object whose value is the internal
//!   [`Value::Lazy`] marker with the host's number. The first read asks the
//!   host for the value and stores it in the property. The property has
//!   its attributes from the start (writable, not enumerable,
//!   configurable), so the keys, `in`, `delete`, assignments and
//!   definitions that replace the value need no value.
//!
//! The paths that read property values resolve the marker before anything
//! can see it:
//!
//! - the heap's walks of `[[Get]]` (`Heap::get`, the walk of a cache miss)
//!   answer `Generic` for it, so the runtime takes its generic `[[Get]]`,
//!   which reads through [`Runtime::own_property`];
//! - the runtime's `[[GetOwnProperty]]` (`own_property`,
//!   `own_property_stored`) and the global name lookup (`get_global`)
//!   resolve it;
//! - a definition that keeps the old value (a descriptor without a value
//!   that is no accessor: `Object.freeze`, `{ enumerable: true }`) resolves
//!   it first; the heap refuses such a definition on an unresolved marker
//!   as an internal error;
//! - the caches never make an entry for a slot that holds the marker, and
//!   no slot gets it back after its first value (only the host defines
//!   markers, before any script runs).
//!
//! A creation allocates many objects in the middle of an engine operation
//! (a property read, a wrapper). It runs in a no-GC region, so nothing that
//! the caller holds needs a root; the heap limit is checked after it. A
//! creation that fails (the heap limit; the time limit, which a time charge
//! inside a creation can meet) forgets the interfaces that it registered,
//! so a later use creates them again from the start, and its termination
//! is repeated at every time check until the task ends, so no caller that
//! drops the error keeps running.

use crate::heap::Gc;
use crate::host::{ClassId, Interface};
use crate::object::{Object, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{VmError, VmResult};

/// Creates the interface of a class in a realm, with the interfaces of its
/// ancestors first (the host's hook; see [`Runtime::set_lazy_hooks`]).
pub type InterfaceHook = fn(&mut Runtime, u32, ClassId) -> VmResult<()>;

/// The value of the lazy global property with the host's number in a realm
/// (the host's hook; see [`Runtime::set_lazy_hooks`]).
pub type GlobalHook = fn(&mut Runtime, u32, u32) -> VmResult<Value>;

/// The host's hooks of lazy creation.
#[derive(Clone, Copy, Debug)]
pub struct LazyHooks {
    /// Creates a missing interface ([`Runtime::interface_or_create`]).
    pub interface: InterfaceHook,
    /// Gives the value of a lazy global property
    /// ([`Runtime::define_lazy_global`]). It must not run script code; it
    /// returns an object or a primitive, never an internal value.
    pub global: GlobalHook,
}

impl Runtime {
    /// Registers the host's hooks of lazy creation (once, before any
    /// script runs).
    pub fn set_lazy_hooks(&mut self, hooks: LazyHooks) {
        self.vm.host.lazy = Some(hooks);
    }

    /// Defines the property `name` of the global object of `realm` as a
    /// writable, not enumerable, configurable data property (the attributes
    /// of an interface object on the global object, Web IDL §3.7) whose
    /// value the global hook creates at the first read, with `id` as its
    /// argument. Call it before any script runs, after
    /// [`Runtime::set_lazy_hooks`].
    pub fn define_lazy_global(&mut self, realm: u32, name: &str, id: u32) -> VmResult<()> {
        let global = self.global_object(realm)?;
        let key = self.heap.key_from_str(name)?;
        let desc = PropertyDescriptor::data(Value::Lazy(id), true, false, true);
        if self
            .heap
            .ordinary_define_own_property(global, key, desc, &self.vm)?
        {
            Ok(())
        } else {
            Err(VmError::invariant("a lazy global property was refused"))
        }
    }

    /// The interface of `class` in `realm`; the interface hook creates it
    /// (with its ancestors) at the first call.
    pub fn interface_or_create(&mut self, realm: u32, class: ClassId) -> VmResult<Interface> {
        match self.interface(realm, class) {
            Some(interface) => Ok(interface),
            None => self.create_lazy_interface(realm, class),
        }
    }

    /// [`Runtime::interface_or_create`] for a missing interface.
    #[cold]
    #[inline(never)]
    fn create_lazy_interface(&mut self, realm: u32, class: ClassId) -> VmResult<Interface> {
        let missing = VmError::invariant("a class without an interface in the realm");
        let Some(hooks) = self.vm.host.lazy else {
            return Err(missing);
        };
        self.lazy_region(realm, |rt| (hooks.interface)(rt, realm, class))?;
        self.interface(realm, class).ok_or(missing)
    }

    /// The value of the lazy property `key` of the global object `holder`
    /// that holds the marker `id`: the global hook's value, stored in the
    /// property. Recorded in the current handle scope.
    #[cold]
    #[inline(never)]
    pub(crate) fn resolve_lazy(
        &mut self,
        holder: Gc<Object>,
        key: PropertyKey,
        id: u32,
    ) -> VmResult<Value> {
        let realm = self
            .vm
            .realms
            .iter()
            .position(|record| record.global == holder)
            .ok_or(VmError::invariant(
                "a lazy property outside a global object",
            ))?;
        let hooks = self
            .vm
            .host
            .lazy
            .ok_or(VmError::invariant("a lazy property without a hook"))?;
        let realm = realm as u32;
        let value = self.lazy_region(realm, |rt| (hooks.global)(rt, realm, id))?;
        if value.is_internal() {
            return Err(VmError::invariant("a lazy property got an internal value"));
        }
        let slot = self
            .heap
            .own_data_slot(holder, key)?
            .ok_or(VmError::invariant("a lazy property disappeared"))?;
        // The hook runs no script code, so the property still holds the
        // marker.
        match self.heap.object_mut(holder)?.slots.get_mut(slot as usize) {
            Some(stored) if *stored == Value::Lazy(id) => *stored = value,
            _ => {
                return Err(VmError::invariant(
                    "a lazy property changed during its creation",
                ));
            }
        }
        Ok(self.keep(value))
    }

    /// The own property `key` of `object` from the heap, with a lazy value
    /// resolved (the runtime's `[[GetOwnProperty]]` of an ordinary object).
    pub(crate) fn own_property_resolved(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        match self.heap.get_own_property(object, key)? {
            Some(Property::Data {
                value: Value::Lazy(id),
                ..
            }) => {
                self.resolve_lazy(object, key, id)?;
                Ok(self.heap.get_own_property(object, key)?)
            }
            property => Ok(property),
        }
    }

    /// Resolves the own property `key` of `object` if it holds a lazy
    /// value (before a definition that keeps the value).
    pub(crate) fn resolve_own_lazy(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<()> {
        if let Some(Property::Data {
            value: Value::Lazy(id),
            ..
        }) = self.heap.get_own_property(object, key)?
        {
            self.resolve_lazy(object, key, id)?;
        }
        Ok(())
    }

    /// Runs a hook of lazy creation in `realm`: in a no-GC region and a
    /// handle scope; the heap limit is checked at the end. After a failure
    /// the interfaces that the hook registered are forgotten, and a
    /// termination is repeated at every time check until the task ends.
    fn lazy_region<T>(
        &mut self,
        realm: u32,
        f: impl FnOnce(&mut Runtime) -> VmResult<T>,
    ) -> VmResult<T> {
        let before = self.missing_interfaces(realm);
        let depth = self.heap.no_gc_depth();
        self.heap.enter_no_gc();
        let result = self.scoped(f);
        self.heap.restore_no_gc_depth(depth);
        let result = result.and_then(|value| {
            self.heap.check_room(0)?;
            Ok(value)
        });
        if let Err(error) = &result {
            self.forget_interfaces(realm, &before);
            if let VmError::Terminated(termination) = error {
                self.vm.terminating = Some(*termination);
                self.vm.countdown.set(1);
            }
        }
        result
    }

    /// The length of the interface table of `realm` and the classes below
    /// it that have no interface.
    fn missing_interfaces(&self, realm: u32) -> (usize, Vec<usize>) {
        let Some(record) = self.vm.realms.get(realm as usize) else {
            return (0, Vec::new());
        };
        let missing = record
            .interfaces
            .iter()
            .enumerate()
            .filter(|(_, interface)| interface.is_none())
            .map(|(index, _)| index)
            .collect();
        (record.interfaces.len(), missing)
    }

    /// Forgets the interfaces of `realm` that were missing before a failed
    /// creation (`before` from [`Runtime::missing_interfaces`]).
    fn forget_interfaces(&mut self, realm: u32, (length, missing): &(usize, Vec<usize>)) {
        let Some(record) = self.vm.realms.get_mut(realm as usize) else {
            return;
        };
        record.interfaces.truncate(*length);
        for &index in missing {
            if let Some(slot) = record.interfaces.get_mut(index) {
                *slot = None;
            }
        }
    }
}
