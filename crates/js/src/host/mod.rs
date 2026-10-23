//! Host classes and host data (ADR 0026 section 12, ADR 0028 sections 4
//! and 6): the parts of the embedding API that the Web API bindings
//! build on.
//!
//! - **Classes.** A runtime has a registry of host classes
//!   ([`Runtime::define_class`]): a name, a parent, optional hooks for the
//!   internal methods ([`HostHooks`]) and an optional constructor. Each
//!   class stores its ancestors, so the brand check (Web IDL "implements")
//!   is one comparison at the depth of the expected class
//!   ([`Runtime::host_data_of`]); a failed check throws Chromium's
//!   `TypeError: Illegal invocation`.
//! - **Interfaces** (`interface.rs`): per realm, each class gets a
//!   prototype object and an interface object ([`Runtime::create_interface`]),
//!   linked to those of its parent; members are defined with
//!   [`Runtime::define_method`], [`Runtime::define_accessor`] and
//!   [`Runtime::define_value`].
//! - **Host objects** ([`Runtime::new_host_object`]) carry their class and
//!   host data ([`HostData`]: a word, or generic data that the collector
//!   traces). A class without hooks makes ordinary objects with a class
//!   (cacheable); a class with hooks makes exotic objects whose internal
//!   methods call the hooks (`hooks.rs`).
//! - **The global object** ([`Runtime::init_global`]): the realm's global
//!   object becomes a host object of a class without hooks, with that
//!   class's prototype; it stays cacheable.
//! - **Host state** ([`HostState`]): the runtime's host data, traced by the
//!   collector as a root, so the binding state (the wrappers, the
//!   listeners, the timers) needs no persistent root per value.
//! - **Termination** ([`Runtime::terminate`]): a native function ends the
//!   running task with a reason.
//! - **Event handlers** (`handler.rs`): functions compiled with a scope
//!   chain of object environments.

mod handler;
pub(crate) mod hooks;
mod interface;

use std::any::Any;

pub use handler::EventHandler;
pub use hooks::{
    DefineOwnPropertyHook, DeleteHook, GetHook, GetOwnPropertyHook, HostHooks, OwnKeysHook, SetHook,
};
pub use interface::{Attributes, Interface};

use crate::error::Termination;
use crate::heap::{Gc, Tracer};
use crate::object::{HostData, HostSlot, Object};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{NativeCall, NativeFn, VmError, VmResult};

/// The message of a failed brand check (Chromium's, measured in
/// `tools/probes/web-embedding.json`).
pub(crate) const ILLEGAL_INVOCATION: &str = "Illegal invocation";

/// The most host classes of a runtime (class numbers stay below 2^31,
/// the bit above marks generic data in an object's slot).
const MAX_CLASSES: usize = 1 << 20;

/// A host class of a runtime: its number in the registry, in the order of
/// [`Runtime::define_class`] (the first class is 0), so that generated
/// bindings can name classes by constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClassId(u32);

impl ClassId {
    /// The number of the class.
    pub fn index(self) -> u32 {
        self.0
    }
}

/// The description of a host class for [`Runtime::define_class`].
#[derive(Clone, Debug)]
pub struct ClassSpec {
    /// The name (of the interface object, and its prototype's
    /// `@@toStringTag`).
    pub name: String,
    /// The parent class (Web IDL inheritance).
    pub parent: Option<ClassId>,
    /// Hooks for internal methods; a class with hooks makes exotic objects
    /// (the inline caches never store them). Subclasses do not inherit the
    /// hooks.
    pub hooks: Option<HostHooks>,
    /// The constructor that `new Interface(...)` calls (it sees
    /// `new.target`); without one, `new` throws `TypeError: Failed to
    /// construct 'Name': Illegal constructor`. A call without `new` throws
    /// either way, with Chromium's messages.
    pub constructor: Option<NativeFn>,
    /// The `length` of the interface object.
    pub length: u32,
}

impl ClassSpec {
    /// A class without parent, hooks or constructor.
    pub fn new(name: &str) -> ClassSpec {
        ClassSpec {
            name: name.to_owned(),
            parent: None,
            hooks: None,
            constructor: None,
            length: 0,
        }
    }

    /// With a parent class.
    #[must_use]
    pub fn parent(mut self, parent: ClassId) -> ClassSpec {
        self.parent = Some(parent);
        self
    }

    /// With hooks for internal methods.
    #[must_use]
    pub fn hooks(mut self, hooks: HostHooks) -> ClassSpec {
        self.hooks = Some(hooks);
        self
    }

    /// With a constructor of `length` arguments.
    #[must_use]
    pub fn constructor(mut self, constructor: NativeFn, length: u32) -> ClassSpec {
        self.constructor = Some(constructor);
        self.length = length;
        self
    }
}

/// A registered class.
pub(crate) struct HostClass {
    pub(crate) name: Box<str>,
    /// The class and its ancestors, the root first (the class itself is
    /// the last entry, at its depth).
    ancestors: Box<[ClassId]>,
    pub(crate) hooks: Option<HostHooks>,
    pub(crate) constructor: Option<NativeFn>,
    pub(crate) length: u32,
}

impl HostClass {
    /// The parent class.
    fn parent(&self) -> Option<ClassId> {
        self.ancestors.iter().rev().nth(1).copied()
    }
}

/// The runtime's host data (ADR 0028 section 6, item 4): any value of the
/// host, reachable from native functions through
/// [`Runtime::host_data_mut`]. The collector calls [`HostState::trace`] as
/// part of the roots, so the JavaScript values that it holds (wrappers,
/// listeners, timer callbacks) stay alive while it holds them; a value
/// that it drops dies at the next collection. The trace must report every
/// heap handle that the data holds and must not change the data.
pub trait HostState: Any {
    /// Reports the heap handles that the data holds (none by default).
    fn trace(&self, tracer: &mut Tracer<'_>) {
        let _ = tracer;
    }
}

/// The host part of the VM: the class registry and the host state.
#[derive(Default)]
pub(crate) struct Host {
    pub(crate) classes: Vec<HostClass>,
    pub(crate) state: Option<Box<dyn HostState>>,
}

impl Host {
    /// Reports the handles of the host state (a root of the collector).
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(state) = &self.state {
            state.trace(tracer);
        }
    }

    /// The class of a number.
    pub(crate) fn class(&self, class: u32) -> VmResult<&HostClass> {
        self.classes
            .get(class as usize)
            .ok_or(VmError::invariant("a host class number without a class"))
    }

    /// Whether the class `class` is `expected` or inherits from it.
    pub(crate) fn implements(&self, class: u32, expected: ClassId) -> bool {
        let (Some(class), Some(wanted)) = (
            self.classes.get(class as usize),
            self.classes.get(expected.0 as usize),
        ) else {
            return false;
        };
        let depth = wanted.ancestors.len() - 1;
        class.ancestors.get(depth) == Some(&expected)
    }
}

impl Runtime {
    // --- Classes ---

    /// Registers a host class and returns its number (the next free one).
    /// The parent must be registered.
    pub fn define_class(&mut self, spec: ClassSpec) -> VmResult<ClassId> {
        let classes = &mut self.vm.host.classes;
        if classes.len() >= MAX_CLASSES {
            return Err(VmError::range_error("too many host classes"));
        }
        let id = ClassId(classes.len() as u32);
        let mut ancestors = match spec.parent {
            Some(parent) => classes
                .get(parent.0 as usize)
                .ok_or(VmError::invariant("a host class with an unknown parent"))?
                .ancestors
                .to_vec(),
            None => Vec::new(),
        };
        ancestors.push(id);
        classes.push(HostClass {
            name: spec.name.into_boxed_str(),
            ancestors: ancestors.into_boxed_slice(),
            hooks: spec.hooks,
            constructor: spec.constructor,
            length: spec.length,
        });
        Ok(id)
    }

    /// The name of a class.
    pub fn class_name(&self, class: ClassId) -> Option<&str> {
        let class = self.vm.host.classes.get(class.0 as usize)?;
        Some(&class.name)
    }

    /// The parent of a class.
    pub fn class_parent(&self, class: ClassId) -> Option<ClassId> {
        self.vm.host.classes.get(class.0 as usize)?.parent()
    }

    // --- Host objects ---

    /// A new host object of `class` with `data`, whose prototype is the
    /// class's prototype in `realm` ([`Runtime::create_interface`] made
    /// it). Recorded in the current handle scope if one is open (inside a
    /// native function); otherwise the host roots it (stores it in its
    /// traced host state) before anything can collect. The allocation
    /// itself never collects.
    pub fn new_host_object(
        &mut self,
        realm: u32,
        class: ClassId,
        data: HostData,
    ) -> VmResult<Gc<Object>> {
        let prototype = self
            .interface(realm, class)
            .ok_or(VmError::invariant(
                "a host object of a class without an interface in the realm",
            ))?
            .prototype;
        self.new_host_object_with_proto(Some(prototype), class, data)
    }

    /// [`Runtime::new_host_object`] with another prototype (a constructor
    /// called through a subclass: the `prototype` of `new.target`, see
    /// [`Runtime::prototype_from_new_target`]).
    pub fn new_host_object_with_proto(
        &mut self,
        proto: Option<Gc<Object>>,
        class: ClassId,
        data: HostData,
    ) -> VmResult<Gc<Object>> {
        let exotic = self.vm.host.class(class.0)?.hooks.is_some();
        let slot = HostSlot::new(class.0, data)?;
        let object = self.heap.new_host_object_with_slot(proto, slot, exotic)?;
        Ok(self.keep(object))
    }

    /// Allocates generic data for a host object (traced and dropped with
    /// the objects that hold it). Recorded like
    /// [`Runtime::new_host_object`].
    pub fn alloc_host_data<T: crate::heap::GenericData>(&mut self, data: T) -> VmResult<HostData> {
        let generic = self.heap.alloc_generic(data)?;
        Ok(HostData::Generic(self.keep(generic)))
    }

    /// The class and the host data of a value, if it is a host object.
    pub fn host_object(&self, value: Value) -> Option<(ClassId, HostData)> {
        let Value::Object(object) = value else {
            return None;
        };
        let slot = self.heap.host_slot(object).ok()??;
        Some((ClassId(slot.class()), slot.data()))
    }

    /// Whether a value is a host object of `class` or of a subclass (the
    /// brand check without the error).
    pub fn implements(&self, value: Value, class: ClassId) -> bool {
        self.host_object(value)
            .is_some_and(|(own, _)| self.vm.host.implements(own.0, class))
    }

    /// The host data of `value` if it is a host object of `class` or of a
    /// subclass (Web IDL "implements"); otherwise `TypeError: Illegal
    /// invocation`, as in Chromium.
    pub fn host_data_of(&self, value: Value, class: ClassId) -> VmResult<HostData> {
        match self.host_object(value) {
            Some((own, data)) if self.vm.host.implements(own.0, class) => Ok(data),
            _ => Err(VmError::type_error(ILLEGAL_INVOCATION)),
        }
    }

    /// The brand check of `this` of a native function (Web IDL §3.7.6,
    /// the steps of operations and attributes): `undefined` and `null`
    /// stand for the global object of the function's realm, then
    /// [`Runtime::host_data_of`].
    pub fn this_host_data(&self, call: &NativeCall, class: ClassId) -> VmResult<HostData> {
        self.host_data_of(self.this_or_global(call)?, class)
    }

    /// `this` of a native call, with the global object of the function's
    /// realm for `undefined` and `null`.
    pub fn this_or_global(&self, call: &NativeCall) -> VmResult<Value> {
        Ok(match call.this() {
            Value::Undefined | Value::Null => self.global_object(call.realm())?.into(),
            this => this,
        })
    }

    // --- The host state ---

    /// Stores the runtime's host data (replaces the old one), which native
    /// functions reach with [`Runtime::host_data_mut`]; the collector
    /// traces it ([`HostState`]).
    pub fn set_host_data<T: HostState>(&mut self, data: T) {
        self.vm.host.state = Some(Box::new(data));
    }

    /// The host data, if it has the type `T`.
    pub fn host_data<T: HostState>(&self) -> Option<&T> {
        let state: &dyn Any = self.vm.host.state.as_deref()?;
        state.downcast_ref()
    }

    /// The host data, if it has the type `T`, for writing.
    pub fn host_data_mut<T: HostState>(&mut self) -> Option<&mut T> {
        let state: &mut dyn Any = self.vm.host.state.as_deref_mut()?;
        state.downcast_mut()
    }

    // --- Termination ---

    /// Ends the running task (ADR 0028 sections 7 and 16): a native
    /// function returns the error, `Err(rt.terminate("..."))`. Like the
    /// time limit, the termination skips every handler and `finally`
    /// block and reaches the host as
    /// `ScriptError::Terminated(Termination::Native(reason))`; a script
    /// that catches nothing of it and a native that drops the error still
    /// end at the next time check, until the host's outermost call
    /// returns. The runtime stays usable for the next task; the host
    /// decides from the reason whether the document runs more scripts.
    pub fn terminate(&mut self, reason: &'static str) -> VmError {
        let termination = Termination::Native(reason);
        self.vm.terminating = Some(termination);
        self.vm.countdown.set(1);
        VmError::Terminated(termination)
    }

    // --- Helpers ---

    /// The heap, for the host's allocations (strings, keys, generic data).
    /// A handle that the host gets from it is not rooted: inside a native
    /// function, record it ([`crate::Heap::record`]); outside, store it in
    /// the traced host state before anything can collect.
    pub fn heap_mut(&mut self) -> &mut crate::Heap {
        &mut self.heap
    }

    /// The global object of a realm.
    pub fn global(&self, realm: u32) -> VmResult<Gc<Object>> {
        self.global_object(realm)
    }

    /// The text of a string or index key (lossy for unpaired surrogates);
    /// `None` for a symbol.
    pub fn key_to_string(&self, key: PropertyKey) -> Option<String> {
        match key {
            PropertyKey::Index(index) => Some(index.to_string()),
            PropertyKey::String(string) => Some(self.heap.string_lossy(string, usize::MAX)),
            PropertyKey::Symbol(_) => None,
        }
    }

    /// The property key of a text: an array index or an atom.
    pub fn key_from_str(&mut self, text: &str) -> VmResult<PropertyKey> {
        Ok(self.heap.key_from_str(text)?)
    }

    /// A new string value, recorded in the current handle scope if one is
    /// open.
    pub fn string_value(&mut self, text: &str) -> VmResult<Value> {
        let string = self.heap.alloc_str(text)?;
        Ok(self.keep(Value::String(string)))
    }

    /// Records a handle in the current handle scope, if one is open.
    pub(crate) fn keep<R: Into<crate::heap::Root> + Copy>(&mut self, root: R) -> R {
        if self.heap.scope_depth() > 0 {
            self.heap.record(root);
        }
        root
    }
}
