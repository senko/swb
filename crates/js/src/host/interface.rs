//! Interface objects and prototypes of host classes per realm (Web IDL
//! §3.7, <https://webidl.spec.whatwg.org/#es-interfaces>), the
//! definitions of their members, and the host-defined global object
//! (ECMA-262 §9.3.1 `InitializeHostDefinedRealm`).
//!
//! An interface object is a native constructor whose `[[Prototype]]` is
//! the interface object of the parent class (or `%Function.prototype%`),
//! with `length`, `name` and a fixed `prototype`. The prototype object's
//! `[[Prototype]]` is the parent's prototype (or `%Object.prototype%`); it
//! has `constructor` and `@@toStringTag` (the class name). A call of the
//! interface object without `new` throws, and `new` calls the class's
//! constructor or throws, with Chromium's messages (measured in
//! `tools/probes/web-embedding.json`). `instanceof` works with the
//! ordinary `OrdinaryHasInstance` along the prototypes.

use crate::heap::Gc;
use crate::host::{ClassId, HostData, HostSlot};
use crate::object::{Object, ObjectKind, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

/// The interface object and the prototype object of a class in a realm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interface {
    /// The interface object (the constructor).
    pub object: Gc<Object>,
    /// The interface prototype object.
    pub prototype: Gc<Object>,
}

/// The attributes of a property that the host defines. `writable` does
/// not apply to accessors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attributes {
    /// `[[Writable]]`.
    pub writable: bool,
    /// `[[Enumerable]]`.
    pub enumerable: bool,
    /// `[[Configurable]]`.
    pub configurable: bool,
}

impl Attributes {
    /// Regular operations and attributes (Web IDL §3.7.6, §3.7.7):
    /// writable, enumerable, configurable.
    pub const MEMBER: Attributes = Attributes {
        writable: true,
        enumerable: true,
        configurable: true,
    };
    /// Constants (§3.7.5) and `[LegacyUnforgeable]` members: read-only,
    /// enumerable, not configurable.
    pub const CONSTANT: Attributes = Attributes {
        writable: false,
        enumerable: true,
        configurable: false,
    };
    /// Interface objects as properties of the global object, and
    /// `constructor`: writable, not enumerable, configurable.
    pub const HIDDEN: Attributes = Attributes {
        writable: true,
        enumerable: false,
        configurable: true,
    };
}

impl Runtime {
    /// Creates the interface object and the prototype object of `class` in
    /// `realm` (the parent's must exist in the realm) and keeps them in the
    /// realm. The interface object is not a property of the global object;
    /// the host defines it there if the interface is exposed
    /// ([`Runtime::define_value`] with [`Attributes::HIDDEN`]).
    pub fn create_interface(&mut self, realm: u32, class: ClassId) -> VmResult<Interface> {
        let interface = self.begin_interface(realm, class)?;
        self.end_interface(interface)?;
        Ok(interface)
    }

    /// [`Runtime::create_interface`] without the `constructor` property of
    /// the prototype: the host defines the members first and then calls
    /// [`Runtime::end_interface`], so that `constructor` is the last own
    /// property, as Chromium lists it.
    pub fn begin_interface(&mut self, realm: u32, class: ClassId) -> VmResult<Interface> {
        self.scoped(|rt| {
            let record = rt.vm.host.class(class.index())?;
            let (name, length) = (record.name.to_string(), record.length);
            let parent = match rt.class_parent(class) {
                Some(parent) => Some(rt.interface(realm, parent).ok_or(VmError::invariant(
                    "an interface whose parent has no interface in the realm",
                ))?),
                None => None,
            };
            let proto_parent = match parent {
                Some(parent) => parent.prototype,
                None => rt.intrinsic(realm, Intrinsic::ObjectPrototype)?,
            };
            let prototype = rt.heap.new_object(Some(proto_parent))?;
            rt.heap.record(prototype);
            let object_parent = match parent {
                Some(parent) => parent.object,
                None => rt.intrinsic(realm, Intrinsic::FunctionPrototype)?,
            };
            let object = rt.new_builtin_with_proto(
                realm,
                object_parent,
                &name,
                length,
                interface_object,
                true,
            )?;
            rt.set_native_slots(object, Box::new([Value::Int(class.index() as i32)]))?;
            let key = PropertyKey::String(rt.vm.atoms.prototype);
            rt.define(object, key, prototype.into(), false, false, false)?;
            crate::builtins::to_string_tag(rt, prototype, &name)?;
            let interface = Interface { object, prototype };
            rt.set_interface(realm, class, interface)?;
            Ok(interface)
        })
    }

    /// Defines the `constructor` property of an interface's prototype (the
    /// last step of [`Runtime::create_interface`]): writable, not
    /// enumerable, configurable.
    pub fn end_interface(&mut self, interface: Interface) -> VmResult<()> {
        self.keep(interface.object);
        self.keep(interface.prototype);
        let key = PropertyKey::String(self.vm.atoms.constructor);
        self.define(
            interface.prototype,
            key,
            interface.object.into(),
            true,
            false,
            true,
        )
    }

    /// The interface of `class` in `realm`, if it was created.
    pub fn interface(&self, realm: u32, class: ClassId) -> Option<Interface> {
        self.vm
            .realms
            .get(realm as usize)?
            .interfaces
            .get(class.index() as usize)
            .copied()
            .flatten()
    }

    /// Keeps the interface of a class in its realm.
    fn set_interface(&mut self, realm: u32, class: ClassId, interface: Interface) -> VmResult<()> {
        let record = self
            .vm
            .realms
            .get_mut(realm as usize)
            .ok_or(VmError::invariant("a realm index without a realm"))?;
        let index = class.index() as usize;
        if record.interfaces.len() <= index {
            record.interfaces.resize(index + 1, None);
        }
        if let Some(slot) = record.interfaces.get_mut(index) {
            *slot = Some(interface);
        }
        Ok(())
    }

    /// Creates the function of a legacy factory function (Web IDL
    /// §3.7.2, <https://webidl.spec.whatwg.org/#js-legacy-factory-functions>,
    /// `new Image()`): a native constructor of `realm` named `name` with
    /// `length`, whose `prototype` property (not writable, not
    /// enumerable, not configurable) is `prototype`, the interface
    /// prototype object of the interface that has the factory. The
    /// function sees `new.target`; a call without `new` is the native's to
    /// reject. The host defines the result as a property of the global
    /// object at once ([`Runtime::define_value`] with
    /// [`Attributes::HIDDEN`]).
    pub fn define_constructor(
        &mut self,
        realm: u32,
        prototype: Gc<Object>,
        name: &str,
        length: u32,
        func: NativeFn,
    ) -> VmResult<Gc<Object>> {
        self.scoped(|rt| {
            let function = rt.new_builtin(realm, name, length, func, true)?;
            let key = PropertyKey::String(rt.vm.atoms.prototype);
            rt.define(function, key, prototype.into(), false, false, false)?;
            Ok(function)
        })
    }

    /// Defines a method (a native function of `realm` named `name`) on
    /// `object`.
    pub fn define_method(
        &mut self,
        realm: u32,
        object: Gc<Object>,
        name: &str,
        length: u32,
        func: NativeFn,
        attributes: Attributes,
    ) -> VmResult<Gc<Object>> {
        self.scoped(|rt| {
            let function = rt.new_builtin(realm, name, length, func, false)?;
            rt.define_value(object, name, function.into(), attributes)?;
            Ok(function)
        })
    }

    /// Defines an accessor on `object`: native functions of `realm` named
    /// `get NAME` (length 0) and `set NAME` (length 1); a missing one is
    /// `undefined`.
    pub fn define_accessor(
        &mut self,
        realm: u32,
        object: Gc<Object>,
        name: &str,
        (getter, setter): (Option<NativeFn>, Option<NativeFn>),
        attributes: Attributes,
    ) -> VmResult<()> {
        self.scoped(|rt| {
            let get = match getter {
                Some(func) => rt
                    .new_builtin(realm, &format!("get {name}"), 0, func, false)?
                    .into(),
                None => Value::Undefined,
            };
            let set = match setter {
                Some(func) => rt
                    .new_builtin(realm, &format!("set {name}"), 1, func, false)?
                    .into(),
                None => Value::Undefined,
            };
            let key = rt.heap.key_from_str(name)?;
            let desc = PropertyDescriptor::accessor(
                get,
                set,
                attributes.enumerable,
                attributes.configurable,
            );
            rt.define_by_host(object, key, desc)
        })
    }

    /// Defines a data property `name` of `object`.
    pub fn define_value(
        &mut self,
        object: Gc<Object>,
        name: &str,
        value: Value,
        attributes: Attributes,
    ) -> VmResult<()> {
        let key = self.heap.key_from_str(name)?;
        self.define_value_with_key(object, key, value, attributes)
    }

    /// [`Runtime::define_value`] with a key (a symbol key, for example
    /// [`Runtime::iterator_key`]).
    pub fn define_value_with_key(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        attributes: Attributes,
    ) -> VmResult<()> {
        let desc = PropertyDescriptor::data(
            value,
            attributes.writable,
            attributes.enumerable,
            attributes.configurable,
        );
        self.scoped(|rt| rt.define_by_host(object, key, desc))
    }

    /// The key of `@@iterator`.
    pub fn iterator_key(&self) -> PropertyKey {
        self.symbol_key(WellKnown::Iterator)
    }

    /// The key of `@@toStringTag`.
    pub fn to_string_tag_key(&self) -> PropertyKey {
        self.symbol_key(WellKnown::ToStringTag)
    }

    /// A definition by the host. The host defines the properties of its
    /// own objects: on a host object with hooks, the definition is the
    /// ordinary one (the hooks are for scripts; Web IDL defines
    /// `@@toStringTag` of a named properties object that way). Otherwise
    /// `DefinePropertyOrThrow`.
    pub(crate) fn define_by_host(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
    ) -> VmResult<()> {
        self.heap.record(object);
        self.heap.record(key);
        if self.host_hooks(object)?.is_none() {
            return self.define_or_throw(object, key, desc);
        }
        desc.check()?;
        if self
            .heap
            .ordinary_define_own_property(object, key, desc, &self.vm)?
        {
            Ok(())
        } else {
            Err(self.define_failure(object, key))
        }
    }

    /// Sets the internal slots of a native function made without them.
    fn set_native_slots(&mut self, function: Gc<Object>, slots: Box<[Value]>) -> VmResult<()> {
        let bytes = slots.len() * size_of::<Value>();
        match &mut self.heap.object_mut(function)?.kind {
            ObjectKind::Native(native) => native.slots = slots,
            _ => return Err(VmError::invariant("native slots of another object")),
        }
        self.heap.charge(bytes);
        Ok(())
    }

    /// `GetPrototypeFromConstructor(newTarget, class's prototype)`
    /// (ECMA-262 §10.1.14) for the constructor of a host class: the
    /// `prototype` of `new.target` if it is an object (a subclass), else
    /// the prototype of `class` in the realm of `new.target`. Recorded in
    /// the native's handle scope.
    pub fn prototype_from_new_target(
        &mut self,
        call: &NativeCall,
        class: ClassId,
    ) -> VmResult<Gc<Object>> {
        let target = match call.new_target() {
            Value::Undefined => Value::Object(call.callee()),
            target => target,
        };
        let key = PropertyKey::String(self.vm.atoms.prototype);
        if let Value::Object(proto) = self.get_value(target, key)? {
            return Ok(proto);
        }
        let realm = match target {
            Value::Object(target) => self.function_realm_checked(target)?,
            _ => None,
        };
        let realm = realm.unwrap_or(call.realm());
        Ok(self.interface_or_create(realm, class)?.prototype)
    }

    /// Makes the global object of `realm` a host object of `class` with
    /// `data` and with the class's prototype in the realm (ECMA-262 §9.3.1
    /// `InitializeHostDefinedRealm`, HTML's `Window`; the global `this` is
    /// the global object itself, ADR 0028 section 4). The class must have
    /// no hooks: the global object stays an ordinary object for the
    /// caches; an exotic object can be on its prototype chain
    /// (`WindowProperties`).
    pub fn init_global(&mut self, realm: u32, class: ClassId, data: HostData) -> VmResult<()> {
        if self.vm.host.class(class.index())?.hooks.is_some() {
            return Err(VmError::invariant("a global object of a class with hooks"));
        }
        let prototype = self.interface_or_create(realm, class)?.prototype;
        let global = self.global_object(realm)?;
        let slot = HostSlot::new(class.index(), data)?;
        if let HostData::Generic(generic) = data {
            self.heap.generic_check(generic)?;
        }
        let record = self.heap.object_mut(global)?;
        match record.kind {
            ObjectKind::Ordinary | ObjectKind::Host(_) => record.kind = ObjectKind::Host(slot),
            _ => return Err(VmError::invariant("a global object of another kind")),
        }
        if !self.heap.set_prototype_of(global, Some(prototype))? {
            return Err(VmError::invariant(
                "the global object refused its prototype",
            ));
        }
        Ok(())
    }
}

/// The steps of an interface object (Web IDL §3.7.1, Chromium's messages):
/// a call without `new` throws; `new` runs the class's constructor, or
/// throws for a class without one.
fn interface_object(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Int(class) = rt.native_slot(call.callee(), 0)? else {
        return Err(VmError::invariant("an interface object without its class"));
    };
    let record = rt.vm.host.class(class as u32)?;
    let constructor = record.constructor;
    if call.new_target().is_undefined() {
        return Err(VmError::type_error(match constructor {
            Some(_) => format!(
                "Failed to construct '{}': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
                record.name
            ),
            None => "Illegal constructor".to_owned(),
        }));
    }
    match constructor {
        Some(constructor) => constructor(rt, call),
        None => Err(VmError::type_error(format!(
            "Failed to construct '{}': Illegal constructor",
            record.name
        ))),
    }
}
