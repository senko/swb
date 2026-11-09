//! The support code of the generated binding glue (ADR 0028 section 5).
//!
//! The generated glue of a member converts the arguments (`convert`),
//! calls the hand-written function of the member's interface trait with a
//! binding context ([`Cx`]), converts the result (`to_js`) and turns an
//! [`Exception`] into a thrown value with Chromium's message
//! (`exception`). What the glue needs from the engine side (`crate::state`)
//! is the [`Host`] trait: the wrapper of a node, the content attributes of
//! an element for `[Reflect]`, and the [`GlueState`] in the host state.
//!
//! Platform objects: a node's wrapper has the host data
//! [`node_data_in`]`(document, id)` (a word: the key of the node's document
//! in the high half, the index of the node in the low half, ADR 0031 section
//! 5); other platform objects carry generic data. Arguments and `this` of an
//! interface that inherits from `Node` convert to a [`NodeId`] of the current
//! document; those of other interfaces to a [`Platform`] (the object and its
//! host data).

pub mod ce;
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

    /// The realm of the objects that the member makes: the realm of the
    /// called function. In a runtime with several documents it is the realm
    /// of `this` (measured: `w.Node.prototype.cloneNode.call(e)` gives an object
    /// of the realm of `e`), which is not the function's when `this` belongs to
    /// another document, and not the document's when `this` is a node that
    /// moved.
    pub fn realm(&self) -> u32 {
        if self.rt.host_flag() {
            return crate::state::objects_realm(self.rt).unwrap_or(self.realm);
        }
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
        let realm = self.realm();
        let proto = match self.call {
            Some(call) => self.rt.prototype_from_new_target(call, self.class)?,
            None => self.rt.interface_or_create(realm, self.class)?.prototype,
        };
        let object = self
            .rt
            .new_host_object_with_proto(realm, Some(proto), self.class, data)?;
        glue::install_unforgeables::<H>(self.rt, realm, self.class, object)?;
        Ok(object)
    }
}

/// A node argument of any document of the runtime (`[AnyDocument]` in the IDL):
/// the wrapper, and the document and node that the wrapper names at the
/// conversion. The `NodeId` means the arena of `document`, so that a native
/// cannot read it in another arena: [`NodeRef::here`] gives it for the current
/// document, and the table of ADR 0031 "Done (feature 12b2)" says what each
/// native does with a node of another one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeRef {
    /// The wrapper.
    pub object: Gc<Object>,
    /// The key of the node's document.
    pub document: u32,
    /// The node, in the arena of `document`.
    pub node: NodeId,
}

impl NodeRef {
    /// The node of a wrapper with the host data `data`.
    #[inline]
    pub fn of(object: Gc<Object>, data: HostData) -> Option<NodeRef> {
        let HostData::Word(word) = data else {
            return None;
        };
        Some(NodeRef {
            object,
            document: node_document(word),
            node: NodeId::from_index(word as u32 as usize)?,
        })
    }

    /// The node if it belongs to the current document.
    #[inline]
    pub fn here(self, rt: &Runtime) -> Option<NodeId> {
        if rt.host_flag() && crate::state::document_key(rt) != Ok(self.document) {
            return None;
        }
        Some(self.node)
    }

    /// The node as the wrapper names it now: a script that ran since the
    /// conversion may have moved it.
    pub fn refreshed(self, rt: &Runtime) -> Option<NodeRef> {
        let (_, data) = rt.host_object(Value::Object(self.object))?;
        NodeRef::of(self.object, data)
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

/// The bits of a wrapper's host data word below the document key.
const DOCUMENT_SHIFT: u32 = 32;

/// The host data of the wrapper of a node of a runtime's first document.
pub fn node_data(node: NodeId) -> HostData {
    node_data_in(0, node)
}

/// The host data of the wrapper of a node of the document `document`.
pub fn node_data_in(document: u32, node: NodeId) -> HostData {
    HostData::Word((u64::from(document) << DOCUMENT_SHIFT) | node.index() as u64)
}

/// The document key in a wrapper's host data word.
pub(crate) fn node_document(word: u64) -> u32 {
    (word >> DOCUMENT_SHIFT) as u32
}

/// The word of a node of the document `document`.
pub(crate) fn word_of(document: u32, node: NodeId) -> u64 {
    (u64::from(document) << DOCUMENT_SHIFT) | node.index() as u64
}

/// The document and the node of a word.
pub(crate) fn word_parts(word: u64) -> Option<(u32, NodeId)> {
    Some((
        node_document(word),
        NodeId::from_index(word as u32 as usize)?,
    ))
}

/// The node of a wrapper's host data, without a look at its document. A
/// native whose `this` is the wrapper may use it: the glue made the node's
/// document the current one (`crate::state::enter_call`). For any other
/// wrapper, use [`node_here`].
pub fn node_of(data: HostData) -> Option<NodeId> {
    match data {
        HostData::Word(word) => NodeId::from_index(word as u32 as usize),
        HostData::Generic(_) => None,
    }
}

/// The node of a wrapper's host data if the node belongs to the current
/// document, so that its `NodeId` means this arena (ADR 0031 section 5).
pub fn node_here(rt: &Runtime, data: HostData) -> Option<NodeId> {
    let node = node_of(data)?;
    if rt.host_flag()
        && let HostData::Word(word) = data
        && Ok(node_document(word)) != crate::state::document_key(rt)
    {
        return None;
    }
    Some(node)
}

/// What the generated glue needs from the engine side.
/// `crate::state::WebState` implements it for the binding state (the
/// runtime's host data); the
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

    /// The steps of an HTML element constructor (`[HTMLConstructor]`, HTML
    /// §3.2.3) of the interface `class`, called with `new`: the element of a
    /// custom element definition. The default has no definitions.
    fn html_constructor(rt: &mut Runtime, call: &NativeCall, class: ClassId) -> Result<Value> {
        let _ = (rt, call, class);
        Err(Exception::type_error("Illegal constructor"))
    }

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
/// `body` or `frameset` element forwards some handlers to the window), the
/// window, or another event target (`MediaQueryList.onchange`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandlerTarget {
    /// An element or the document.
    Node(NodeId),
    /// The window.
    Window,
    /// Another platform object that is an `EventTarget`.
    Object(Gc<Object>),
}

/// The state of the glue in a runtime: the templates of the
/// `[LegacyUnforgeable]` members (an object per realm and interface that
/// holds the accessors, which every instance shares), the iterator
/// prototype objects of pair iterators, the legacy factory functions and
/// the built-in objects that interfaces use, kept from before any script
/// ran (interfaces are created at their first use, ADR 0028 section 5,
/// "Lazy interfaces notes").
#[derive(Debug, Default)]
pub struct GlueState {
    pub(crate) unforgeable: Vec<((u32, ClassId), Gc<Object>)>,
    /// The iterator prototype objects of the interfaces with pair
    /// iterators (Web IDL §3.7.9.2), per realm and iterator class.
    pub(crate) iterator_prototypes: Vec<((u32, ClassId), Gc<Object>)>,
    /// The legacy factory functions (`Image`), per realm and name.
    pub(crate) factories: Vec<((u32, &'static str), Gc<Object>)>,
    /// The built-in objects of each realm that the glue uses.
    pub(crate) builtins: Vec<(u32, glue::Builtins)>,
}

impl GlueState {
    /// Reports the objects that the state holds (call it from the host
    /// state's [`HostState::trace`]).
    pub fn trace(&self, tracer: &mut Tracer<'_>) {
        for (_, object) in self.unforgeable.iter().chain(&self.iterator_prototypes) {
            tracer.object(*object);
        }
        for (_, object) in &self.factories {
            tracer.object(*object);
        }
        for (_, builtins) in &self.builtins {
            builtins.trace(tracer);
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
        let data = node_data_in(7, node);
        assert_eq!(node_of(data), Some(node));
        assert_eq!(data, HostData::Word((7 << 32) + 41));
        assert_eq!(node_document((7 << 32) + 41), 7);
    }
}
