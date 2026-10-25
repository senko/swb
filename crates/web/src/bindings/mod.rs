//! The support code of the generated binding glue (ADR 0028 section 5).
//!
//! The generated glue of a member converts the arguments (`convert`),
//! calls the hand-written function of the member's interface trait with a
//! binding context ([`Cx`]), converts the result (`to_js`) and turns an
//! [`Exception`] into a thrown value with Chromium's message
//! (`exception`). What the glue needs from the engine side (feature 4c)
//! is the [`Host`] trait: the wrapper of a node, the content attributes of
//! an element for `[Reflect]`, and the [`GlueState`] in the host state.
//!
//! Platform objects: a node's wrapper has the host data
//! [`node_data`]`(id)` (a word, the index of the node); other platform
//! objects carry generic data. Arguments and `this` of an interface that
//! inherits from `Node` convert to a [`NodeId`]; those of other interfaces
//! to a [`Platform`] (the object and its host data).

pub mod convert;
pub mod dom_exception;
pub mod exception;
pub mod glue;
pub mod iterable;
pub mod legacy;
pub mod overload;
pub mod reflect;
pub mod to_js;

pub use exception::{Context, Exception};
pub use swb_dom::NodeId;

use swb_js::{
    ClassId, Gc, HostData, HostState, NativeCall, Object, Runtime, Tracer, Value, VmError, VmResult,
};

/// The result of a hand-written member function.
pub type Result<T> = std::result::Result<T, Exception>;

/// The binding context of a call: the runtime, the realm of the called
/// function, the interface of the member and, for a call from script, the
/// native call (with `new.target` of a constructor).
pub struct Cx<'a> {
    /// The runtime. Between two uses of the host state, the hand-written
    /// code may call script (ADR 0028 section 2: copy out of the DOM,
    /// release it, then call).
    pub rt: &'a mut Runtime,
    realm: u32,
    class: ClassId,
    call: Option<&'a NativeCall>,
}

impl<'a> Cx<'a> {
    /// The context of a native call of a member of `class`.
    pub fn new(rt: &'a mut Runtime, call: &'a NativeCall, class: ClassId) -> Cx<'a> {
        Cx {
            rt,
            realm: call.realm(),
            class,
            call: Some(call),
        }
    }

    /// A context without a native call (a hook, a callback from the
    /// engine).
    pub fn without_call(rt: &'a mut Runtime, realm: u32, class: ClassId) -> Cx<'a> {
        Cx {
            rt,
            realm,
            class,
            call: None,
        }
    }

    /// The realm of the called function.
    pub fn realm(&self) -> u32 {
        self.realm
    }

    /// The interface of the member.
    pub fn class(&self) -> ClassId {
        self.class
    }

    /// The native call, if the member was called from script.
    pub fn call(&self) -> Option<&NativeCall> {
        self.call
    }

    /// The prototype that a constructor gives its object when script
    /// called it: the `prototype` of `new.target` (so script subclasses
    /// work). None without a native call.
    pub fn prototype_from_new_target(&mut self) -> VmResult<Option<Gc<Object>>> {
        match self.call {
            Some(call) => Ok(Some(self.rt.prototype_from_new_target(call, self.class)?)),
            None => Ok(None),
        }
    }

    /// A new instance of the interface for a constructor: a host object
    /// with `data`, whose prototype comes from `new.target` (so script
    /// subclasses work), with the `[LegacyUnforgeable]` members of the
    /// interface.
    pub fn new_instance<H: Host>(&mut self, data: HostData) -> VmResult<Gc<Object>> {
        let proto = match self.call {
            Some(call) => self.rt.prototype_from_new_target(call, self.class)?,
            None => {
                self.rt
                    .interface(self.realm, self.class)
                    .ok_or(VmError::type_error("Illegal constructor"))?
                    .prototype
            }
        };
        let object = self
            .rt
            .new_host_object_with_proto(Some(proto), self.class, data)?;
        glue::install_unforgeables::<H>(self.rt, self.realm, self.class, object)?;
        Ok(object)
    }
}

/// A platform object that is not a node: the object and its host data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Platform {
    /// The object.
    pub object: Gc<Object>,
    /// Its host data.
    pub data: HostData,
}

/// The host data of the wrapper of a node.
pub fn node_data(node: NodeId) -> HostData {
    HostData::Word(node.index() as u64)
}

/// The node of a wrapper's host data.
pub fn node_of(data: HostData) -> Option<NodeId> {
    match data {
        HostData::Word(word) => NodeId::from_index(usize::try_from(word).ok()?),
        HostData::Generic(_) => None,
    }
}

/// What the generated glue needs from the engine side. Feature 4c
/// implements it for the binding state (the runtime's host data); the
/// hand-written behaviour of each interface implements the interface's
/// generated trait (for example `generated::node::NodeImpl`) for the same
/// type.
pub trait Host: HostState + Sized {
    /// The glue's own state, kept in the host state and traced with it.
    fn glue_state(&mut self) -> &mut GlueState;

    /// The wrapper of `node` in `realm`: the same object at every call
    /// (created at the first, with the prototype of the node's most derived
    /// interface, the host data [`node_data`]`(node)` and the
    /// `[LegacyUnforgeable]` members of [`glue::install_unforgeables`]).
    fn wrap_node(rt: &mut Runtime, realm: u32, node: NodeId) -> VmResult<Gc<Object>>;

    /// `[Reflect]`: the value of the content attribute `name` (no
    /// namespace) of `element`, or `None`.
    fn reflect_get(cx: &mut Cx<'_>, element: NodeId, name: &str) -> Result<Option<String>>;

    /// `[Reflect]`: sets the content attribute `name` of `element`.
    fn reflect_set(cx: &mut Cx<'_>, element: NodeId, name: &str, value: &str) -> Result<()>;

    /// `[Reflect]`: removes the content attribute `name` of `element`.
    fn reflect_remove(cx: &mut Cx<'_>, element: NodeId, name: &str) -> Result<()>;

    /// `[ReflectURL]`: `value` parsed relative to the base URL of the node
    /// document of `element` and serialized, or `None` on failure.
    fn reflect_url(cx: &mut Cx<'_>, element: NodeId, value: &str) -> Option<String>;

    /// The getter of an event handler IDL attribute `name` (`onclick`) of
    /// `target` (HTML "getting the current value of the event handler"):
    /// a function or `null`. The default has no handlers.
    fn event_handler(rt: &mut Runtime, target: HandlerTarget, name: &str) -> VmResult<Value> {
        let _ = (rt, target, name);
        Ok(Value::Null)
    }

    /// The setter of an event handler IDL attribute: a callable `value`
    /// is the handler, any other value deactivates it. The default does
    /// nothing.
    fn set_event_handler(
        rt: &mut Runtime,
        target: HandlerTarget,
        name: &str,
        value: Value,
    ) -> VmResult<()> {
        let _ = (rt, target, name, value);
        Ok(())
    }
}

/// The object an event handler IDL attribute belongs to: a node (a
/// `body` or `frameset` element forwards some handlers to the window), or
/// the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandlerTarget {
    /// An element or the document.
    Node(NodeId),
    /// The window.
    Window,
}

/// The state of the glue in a runtime: the templates of the
/// `[LegacyUnforgeable]` members (an object per realm and interface that
/// holds the accessors, which every instance shares) and the iterator
/// prototype objects of pair iterators.
#[derive(Debug, Default)]
pub struct GlueState {
    pub(crate) unforgeable: Vec<((u32, ClassId), Gc<Object>)>,
    /// The iterator prototype objects of the interfaces with pair
    /// iterators (Web IDL §3.7.9.2), per realm and iterator class.
    pub(crate) iterator_prototypes: Vec<((u32, ClassId), Gc<Object>)>,
}

impl GlueState {
    /// Reports the objects that the state holds (call it from the host
    /// state's [`HostState::trace`]).
    pub fn trace(&self, tracer: &mut Tracer<'_>) {
        for (_, object) in self.unforgeable.iter().chain(&self.iterator_prototypes) {
            tracer.object(*object);
        }
    }
}

/// Records a value in the open handle scope, if there is one (inside a
/// native call there always is); a value that the glue builds outside
/// one must be rooted by the caller.
pub fn keep(rt: &mut Runtime, value: Value) {
    if rt.heap().scope_depth() > 0 {
        rt.heap_mut().record(value);
    }
}

/// The glue state of the runtime's host data `H`.
pub(crate) fn glue_state<H: Host>(rt: &mut Runtime) -> VmResult<&mut GlueState> {
    rt.host_data_mut::<H>()
        .map(Host::glue_state)
        .ok_or(VmError::type_error("the binding state is missing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_data_round_trips() {
        let node = NodeId::from_index(41).unwrap();
        assert_eq!(node_of(node_data(node)), Some(node));
        assert_eq!(node_of(HostData::Word(u64::MAX)), None);
    }
}
