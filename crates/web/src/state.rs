//! The binding state of a runtime (ADR 0028 sections 2 and 4): the
//! runtime's host data. It holds the document while the engine lends it
//! for a task, the wrapper vector (one wrapper per node, indexed by
//! `NodeId`), the side table of the `[SameObject]` objects of nodes, the
//! event listeners of nodes and of the window, the window's current event,
//! the clock, the event loop (tasks, timers) and the glue state; the collector traces all of it, so wrappers, their
//! expandos, `WeakMap` entries keyed by them and listeners live as long as
//! the runtime.
//!
//! Access from the bindings follows the rule of ADR 0028 section 2: a
//! function copies what it needs out of the DOM ([`dom`]), releases the
//! borrow, and only then allocates or calls script.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use swb_dom::{Document, NodeData, NodeId, ns};
use swb_js::{ClassId, Gc, HostData, HostState, Object, Runtime, Tracer, Value, VmError, VmResult};

use crate::bindings::{self as b, Cx, GlueState};
use crate::clock::DocumentClock;
use crate::event_loop::EventLoop;
use crate::events::{Handlers, Listeners};
use crate::generated as g;
use crate::host::EngineHost;
use crate::runtime::ConsoleMessage;

/// The `[SameObject]` objects that belong to a node (the side table's
/// second key).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Slot {
    /// `Node.childNodes`.
    ChildNodes,
    /// `ParentNode.children`.
    Children,
    /// `Element.classList`.
    ClassList,
    /// `Element.attributes`.
    Attributes,
    /// `HTMLElement.dataset`.
    Dataset,
    /// A `DOMTokenList` of the attribute with this name (`relList`,
    /// `sizes`, `blocking`, `sandbox`, `htmlFor`).
    TokenList(&'static str),
    /// `table.rows`, `tBodies`, `tr.cells`, `select.options` and the
    /// other lists of an element (feature 6c).
    ElementList(crate::dom::ElementList),
    /// `SVGElement.className`.
    ClassName,
    /// `Document.implementation`.
    Implementation,
    /// `document.forms` and the other collections of a document.
    Collection(crate::dom::DocCollection),
}

/// The parsed tokens of token lists by element and attribute name.
pub(crate) type TokenCache = HashMap<(NodeId, swb_dom::LocalName), Rc<Vec<String>>>;

/// The elements of the window's named properties by `name`: the first one
/// and how many there are (up to 2).
pub(crate) type WindowNames = HashMap<String, (NodeId, u8)>;

/// The binding state of a runtime.
#[derive(Default)]
pub(crate) struct WebState {
    glue: GlueState,
    /// The document while the engine lends it for a task.
    pub(crate) host: Option<Box<dyn EngineHost>>,
    /// The wrapper of each node that a script has seen, by node index.
    wrappers: Vec<Option<Gc<Object>>>,
    /// The `[SameObject]` objects of nodes.
    side: HashMap<(NodeId, Slot), Gc<Object>>,
    /// Values that the host holds across runs (the result of an
    /// evaluation while it is serialized, the serializer).
    pub(crate) pinned: Vec<Value>,
    /// The class of the named properties object of the global object
    /// (`WindowProperties`, registered after the generated classes).
    pub(crate) window_properties: Option<ClassId>,
    /// The cache of the `name` attributes of the window's named
    /// properties (`window[name]`): the DOM generation it was built for and
    /// the first element of each name.
    pub(crate) window_names: Option<(u64, WindowNames)>,
    /// The `Attr` objects that scripts have seen, by element and attribute
    /// name. An attribute that is removed keeps its object, which then has
    /// no owner (the entry is dropped).
    pub(crate) attr_objects: HashMap<(NodeId, swb_dom::Namespace, swb_dom::LocalName), Gc<Object>>,
    /// The `childNodes` and `children` lists that hold a cache, and the DOM
    /// generation of the caches (`dom/lists.rs` drops the stale ones).
    pub(crate) list_registry: Vec<Gc<swb_js::Generic>>,
    pub(crate) list_registry_generation: u64,
    /// The parsed tokens of the token lists that were read in one DOM
    /// generation (`dom/token_list.rs`), by element and attribute name.
    pub(crate) token_cache: (u64, TokenCache),
    /// The parsed selector lists of `querySelector*`, `matches` and
    /// `closest`, by text (the page that calls them in a loop parses each
    /// string once).
    pub(crate) selectors: HashMap<String, Rc<swb_css::SelectorList>>,
    /// The documents that `DOMParser` made (they have the URL of the
    /// window's document).
    pub(crate) parsed_documents: std::collections::HashSet<NodeId>,
    /// The `input` elements whose `indeterminate` is true.
    pub(crate) indeterminate: std::collections::HashSet<NodeId>,
    /// The event listener lists of nodes (a node with listeners has a
    /// wrapper; both live as long as the document).
    pub(crate) listeners: HashMap<NodeId, Listeners>,
    /// The event listener list of the window.
    pub(crate) window_listeners: Listeners,
    /// The event handlers (`onclick`, ...) of nodes and of the window.
    pub(crate) handlers: Handlers,
    /// The elements that are busy with a click, a focus, a submission or
    /// a reset (the flags of the HTML algorithms).
    pub(crate) busy: crate::html::activation::Busy,
    /// The listeners in `listeners` and `window_listeners`.
    pub(crate) listener_count: usize,
    /// The window's current event (HTML `window.event`).
    pub(crate) current_event: Option<Gc<Object>>,
    /// The console buffer of the runtime (also written by the reports of
    /// exceptions in event listeners).
    pub(crate) console: Rc<RefCell<VecDeque<ConsoleMessage>>>,
    /// The console lines and reports written to the log so far.
    pub(crate) console_logged: Rc<Cell<usize>>,
    /// The page clock and the time origin of the window
    /// (`performance.now()`, `Event.timeStamp`, timers).
    pub(crate) clock: DocumentClock,
    /// The task queue, the timers and the state of the microtask
    /// checkpoint and the error reports.
    pub(crate) event_loop: EventLoop,
    /// The window's `Performance` object, made at the first access.
    pub(crate) performance: Option<Gc<Object>>,
    /// The script elements and the script lists of the window's document.
    pub(crate) scripts: crate::scripts::Scripts,
}

impl HostState for WebState {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        self.glue.trace(tracer);
        for wrapper in self.wrappers.iter().flatten() {
            tracer.object(*wrapper);
        }
        for object in self.side.values() {
            tracer.object(*object);
        }
        for object in self.attr_objects.values() {
            tracer.object(*object);
        }
        for list in &self.list_registry {
            tracer.generic(*list);
        }
        for value in &self.pinned {
            tracer.value(*value);
        }
        for list in self.listeners.values() {
            list.trace(tracer);
        }
        self.window_listeners.trace(tracer);
        self.handlers.trace(tracer);
        if let Some(event) = self.current_event {
            tracer.object(event);
        }
        if let Some(performance) = self.performance {
            tracer.object(performance);
        }
        self.event_loop.trace(tracer);
    }
}

/// The host data of the platform objects that carry no data of their own
/// (the global object, the named properties object): a word that is no
/// node index, so that [`b::node_of`] never takes them for a node (the
/// word 0 is the document node).
pub(crate) const NOT_A_NODE: HostData = HostData::Word(u64::MAX);

/// The message when a binding runs without a lent document (a bug of the
/// host: scripts run only while the engine lends it).
const NO_DOCUMENT: &str = "the document is not available";

/// The binding state of the runtime.
pub(crate) fn state(rt: &Runtime) -> VmResult<&WebState> {
    rt.host_data::<WebState>()
        .ok_or(VmError::type_error("the runtime has no binding state"))
}

/// The binding state of the runtime, for changes.
pub(crate) fn state_mut(rt: &mut Runtime) -> VmResult<&mut WebState> {
    rt.host_data_mut::<WebState>()
        .ok_or(VmError::type_error("the runtime has no binding state"))
}

/// The lent document state.
pub(crate) fn engine(rt: &Runtime) -> VmResult<&dyn EngineHost> {
    state(rt)?
        .host
        .as_deref()
        .ok_or(VmError::type_error(NO_DOCUMENT))
}

/// The lent document state, for changes.
pub(crate) fn engine_mut(rt: &mut Runtime) -> VmResult<&mut dyn EngineHost> {
    match state_mut(rt)?.host.as_deref_mut() {
        Some(host) => Ok(host),
        None => Err(VmError::type_error(NO_DOCUMENT)),
    }
}

/// The DOM of the lent document.
pub(crate) fn dom(rt: &Runtime) -> VmResult<&Document> {
    Ok(engine(rt)?.dom())
}

/// The DOM of the lent document, for changes.
pub(crate) fn dom_mut(rt: &mut Runtime) -> VmResult<&mut Document> {
    Ok(engine_mut(rt)?.dom_mut())
}

/// The interface of a node's wrapper: its most derived interface among
/// those that swb has (HTML and SVG elements: a table by tag name,
/// `html::classes` and `svg`; elements of other namespaces `Element`; every
/// document `HTMLDocument`).
fn class_of(dom: &Document, node: NodeId) -> ClassId {
    match &dom.node(node).data {
        NodeData::Document => g::HTML_DOCUMENT,
        NodeData::DocumentFragment => g::DOCUMENT_FRAGMENT,
        NodeData::Doctype { .. } => g::DOCUMENT_TYPE,
        NodeData::Element(e) if e.name.ns == ns!(html) => crate::html::class_of_html(&e.name.local),
        NodeData::Element(e) if e.name.ns == ns!(svg) => crate::svg::class_of_svg(&e.name.local),
        NodeData::Element(_) => g::ELEMENT,
        NodeData::Text(_) => g::TEXT,
        NodeData::Comment(_) => g::COMMENT,
        NodeData::ProcessingInstruction { .. } => g::PROCESSING_INSTRUCTION,
    }
}

/// The `[SameObject]` object of `node` in `slot`, created by `create` at
/// the first access and kept in the side table.
pub(crate) fn same_object(
    rt: &mut Runtime,
    node: NodeId,
    slot: Slot,
    create: impl FnOnce(&mut Runtime) -> VmResult<Gc<Object>>,
) -> VmResult<Gc<Object>> {
    if let Some(object) = state(rt)?.side.get(&(node, slot)) {
        return Ok(*object);
    }
    let object = create(rt)?;
    state_mut(rt)?.side.insert((node, slot), object);
    Ok(object)
}

/// The wrapper of `node`, made with `proto` as its prototype if the node
/// has none yet (a constructor that script called through a subclass:
/// `class A extends Image`); else the class's prototype.
pub(crate) fn wrap_node_with_proto(
    rt: &mut Runtime,
    realm: u32,
    node: NodeId,
    proto: Option<Gc<Object>>,
) -> VmResult<Gc<Object>> {
    let state = state(rt)?;
    if let Some(Some(wrapper)) = state.wrappers.get(node.index()) {
        return Ok(*wrapper);
    }
    let dom = dom(rt)?;
    if dom.get(node).is_none() {
        return Err(VmError::type_error(
            "a wrapper of a node that does not exist",
        ));
    }
    let class = class_of(dom, node);
    let wrapper = match proto {
        Some(proto) => rt.new_host_object_with_proto(Some(proto), class, b::node_data(node))?,
        None => rt.new_host_object(realm, class, b::node_data(node))?,
    };
    b::glue::install_unforgeables::<WebState>(rt, realm, class, wrapper)?;
    let state = state_mut(rt)?;
    if state.wrappers.len() <= node.index() {
        state.wrappers.resize(node.index() + 1, None);
    }
    if let Some(slot) = state.wrappers.get_mut(node.index()) {
        *slot = Some(wrapper);
    }
    Ok(wrapper)
}

impl b::Host for WebState {
    fn glue_state(&mut self) -> &mut GlueState {
        &mut self.glue
    }

    fn wrap_node(rt: &mut Runtime, realm: u32, node: NodeId) -> VmResult<Gc<Object>> {
        wrap_node_with_proto(rt, realm, node, None)
    }

    fn reflect_get(cx: &mut Cx<'_>, element: NodeId, name: &str) -> b::Result<Option<String>> {
        Ok(dom(cx.rt)?
            .element(element)
            .and_then(|e| e.attr(name))
            .map(str::to_owned))
    }

    fn reflect_set(cx: &mut Cx<'_>, element: NodeId, name: &str, value: &str) -> b::Result<()> {
        let name = swb_dom::QualName::new(None, ns!(), swb_dom::LocalName::from(name));
        crate::dom::set_attribute(cx.rt, element, &name, value)
    }

    fn reflect_remove(cx: &mut Cx<'_>, element: NodeId, name: &str) -> b::Result<()> {
        let name = swb_dom::QualName::new(None, ns!(), swb_dom::LocalName::from(name));
        crate::dom::remove_attribute(cx.rt, element, &name)?;
        Ok(())
    }

    fn reflect_url(cx: &mut Cx<'_>, _element: NodeId, value: &str) -> Option<String> {
        engine_mut(cx.rt).ok()?.parse_url(value)
    }

    fn event_handler(rt: &mut Runtime, target: b::HandlerTarget, name: &str) -> VmResult<Value> {
        crate::events::handlers::get(rt, target, name)
    }

    fn set_event_handler(
        rt: &mut Runtime,
        target: b::HandlerTarget,
        name: &str,
        value: Value,
    ) -> VmResult<()> {
        crate::events::handlers::set(rt, target, name, value)
    }
}
