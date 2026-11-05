//! The binding state of a runtime (ADR 0028 sections 2 and 4, ADR 0031
//! section 5): the runtime's host data. It has two parts.
//!
//! - [`WebState`] holds what belongs to the agent, the whole runtime: the
//!   glue, the event loop, the console, the registry of the message ports,
//!   the table of the `WindowProxy` objects.
//! - [`DocState`] holds what belongs to one document and its `Window`: the
//!   document while the engine lends it for a task, the wrapper vector (one
//!   wrapper per node, indexed by `NodeId`), the side table of the
//!   `[SameObject]` objects of nodes, the event listeners of nodes and of the
//!   window, the window's current event, the clock, the timers and the
//!   objects of the window.
//!
//! The collector traces both, so wrappers, their expandos, `WeakMap` entries
//! keyed by them and listeners live as long as the runtime.
//!
//! One document is the current one (`state.doc`), and the bindings read it
//! as they read a field. A native switches at its entry when its `this`, or
//! without a `this` its realm, belongs to another document, and back at its
//! exit ([`enter_call`], [`leave`]; the glue calls them). While a runtime has
//! one document, that is one comparison. The switch holds across re-entry:
//! each native that switched restores what it found.
//!
//! Access from the bindings follows the rule of ADR 0028 section 2: a
//! function copies what it needs out of the DOM ([`dom`]), releases the
//! borrow, and only then allocates or calls script.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use swb_dom::{Document, NodeData, NodeId, ns};
use swb_js::{
    ClassId, Gc, HostData, HostState, NativeCall, Object, Runtime, Tracer, Value, VmError, VmResult,
};

use crate::bindings::{self as b, Cx, GlueState};
use crate::clock::DocumentClock;
use crate::event_loop::EventLoop;
use crate::events::{Handlers, Listeners};
use crate::generated as g;
use crate::host::EngineHost;
use crate::runtime::ConsoleMessage;
use crate::timers::Timers;

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
    /// `ElementCSSInlineStyle.style`.
    Style,
    /// `document.forms` and the other collections of a document.
    Collection(crate::dom::DocCollection),
    /// `validity` of a form control (`ValidityState`).
    Validity,
    /// `document.styleSheets` (`StyleSheetList`).
    StyleSheets,
}

/// The parsed tokens of token lists by element and attribute name.
pub(crate) type TokenCache = HashMap<(NodeId, swb_dom::LocalName), Rc<Vec<String>>>;

/// The elements of the window's named properties by `name`: the first one
/// and how many there are (up to 2).
pub(crate) type WindowNames = HashMap<String, (NodeId, u8)>;

/// The key of a document in a runtime: its index in [`WebState::docs`]. The
/// first document has the key 0.
pub(crate) type DocKey = u32;

/// The binding state of a runtime: what belongs to the agent.
#[derive(Default)]
pub(crate) struct WebState {
    glue: GlueState,
    /// The current document.
    pub(crate) doc: Box<DocState>,
    /// The other documents of the runtime by key; the entry of the current
    /// document is empty.
    docs: Vec<Option<Box<DocState>>>,
    /// The key of the document of each realm, by realm.
    realm_docs: Vec<DocKey>,
    /// The `WindowProxy` of each frame that the runtime has seen (HTML gives
    /// a browsing context one `WindowProxy`; ADR 0031 section 5).
    window_proxies: HashMap<u32, Gc<Object>>,
    /// Values that the host holds across runs (the result of an
    /// evaluation while it is serialized, the serializer).
    pub(crate) pinned: Vec<Value>,
    /// The class of the named properties object of the global object
    /// (`WindowProperties`, registered after the generated classes).
    pub(crate) window_properties: Option<ClassId>,
    /// The parsed selector lists of `querySelector*`, `matches` and
    /// `closest`, by text (the page that calls them in a loop parses each
    /// string once).
    pub(crate) selectors: HashMap<String, Rc<swb_css::SelectorList>>,
    /// The console buffer of the runtime (also written by the reports of
    /// exceptions in event listeners).
    pub(crate) console: Rc<RefCell<VecDeque<ConsoleMessage>>>,
    /// The console lines and reports written to the log so far.
    pub(crate) console_logged: Rc<Cell<usize>>,
    /// The task queue and the state of the microtask checkpoint and the
    /// error reports. A task remembers its document.
    pub(crate) event_loop: EventLoop,
    /// The store of the runtime when the engine lends none (`storage.rs`).
    pub(crate) own_storage: Option<crate::storage::SharedStorage>,
    /// The Blob URLs of the agent (`file_api/mod.rs`; same-origin documents
    /// resolve each other's).
    pub(crate) blob_urls: crate::file_api::BlobUrls,
    /// The blob vectors that Blob URL requests and `FileReader` reads hold
    /// until their task runs (`file_api/mod.rs`).
    pub(crate) held_blobs: crate::file_api::HeldBlobs,
    /// True after the warning about a failed `FileReader` read.
    pub(crate) reader_warned: bool,
    /// The registry of the message ports and the budget of the waiting
    /// messages (`message_port.rs`).
    pub(crate) ports: Rc<crate::message_port::Shared>,
}

/// The binding state of one document and its `Window`.
#[derive(Default)]
pub(crate) struct DocState {
    /// The key of the document.
    pub(crate) key: DocKey,
    /// The realm of the document's `Window`.
    pub(crate) realm: u32,
    /// The document while the engine lends it for a task.
    pub(crate) host: Option<Box<dyn EngineHost>>,
    /// The wrapper of each node that a script has seen, by node index.
    wrappers: Vec<Option<Gc<Object>>>,
    /// The `[SameObject]` objects of nodes.
    side: HashMap<(NodeId, Slot), Gc<Object>>,
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
    /// The documents that `DOMParser` made (they have the URL of the
    /// window's document).
    pub(crate) parsed_documents: std::collections::HashSet<NodeId>,
    /// The `input` elements whose `indeterminate` is true.
    pub(crate) indeterminate: std::collections::HashSet<NodeId>,
    /// The `output` elements whose value mode flag is "value" (a script
    /// wrote `value`), with their default value override
    /// (<https://html.spec.whatwg.org/multipage/form-elements.html#the-output-element>).
    pub(crate) output_defaults: HashMap<NodeId, String>,
    /// The `select` and `selectionchange` events that wait for their task,
    /// and the owner of the document's selection (feature 9b2).
    pub(crate) selection_events: crate::html::selection_events::SelectionEvents,
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
    /// The page clock and the time origin of the window
    /// (`performance.now()`, `Event.timeStamp`, timers).
    pub(crate) clock: DocumentClock,
    /// The timers of the window (HTML "map of active timers").
    pub(crate) timers: Timers,
    /// HTML "in error reporting mode" of the window.
    pub(crate) error_reporting: bool,
    /// The window's `Performance` object, made at the first access.
    pub(crate) performance: Option<Gc<Object>>,
    /// The entry buffer, the observers and the load times (`performance/`).
    pub(crate) perf: crate::performance::PerfState,
    /// The window's map of idle callbacks (`idle_callbacks.rs`).
    pub(crate) idle: crate::idle_callbacks::IdleCallbacks,
    /// The window's `Navigator` and `Crypto` objects, made at the first
    /// access.
    pub(crate) navigator: Option<Gc<Object>>,
    pub(crate) crypto: Option<Gc<Object>>,
    /// The `navigator.clipboard` object, made at the first access
    /// (`clipboard.rs`).
    pub(crate) clipboard: Option<Gc<Object>>,
    /// The print-when-loaded flag and the flag of the printing steps
    /// (`dialogs.rs`).
    pub(crate) print_when_loaded: bool,
    pub(crate) printing: bool,
    /// The dialogs counted for the log (`dialogs.rs`).
    pub(crate) dialogs_logged: u32,
    /// The window's `Location` and `History` objects, made at the first
    /// access, and the deserialized `history.state` (`location.rs`).
    pub(crate) location: Option<Gc<Object>>,
    pub(crate) history: Option<Gc<Object>>,
    pub(crate) history_state: Option<crate::location::StateCache>,
    /// The window's `Screen` object and its `ScreenOrientation`, made at
    /// the first access.
    pub(crate) screen: Option<Gc<Object>>,
    pub(crate) screen_orientation: Option<Gc<Object>>,
    /// The window's `localStorage` and `sessionStorage` objects (in the
    /// order of `StorageKind`), made at the first access (`storage.rs`).
    pub(crate) storage_objects: [Option<Gc<Object>>; 2],
    /// The script elements and the script lists of the window's document.
    pub(crate) scripts: crate::scripts::Scripts,
    /// The declaration blocks of the `style` attributes that scripts read
    /// or changed (`element.style`).
    pub(crate) style_blocks: crate::cssom::StyleBlocks,
    /// The wrappers of the style sheets and rules that scripts have seen
    /// (`cssom_sheets.rs`).
    pub(crate) cssom: crate::cssom_sheets::CssomObjects,
    /// The window's map of animation frame callbacks.
    pub(crate) animation_frames: crate::animation_frames::AnimationFrames,
    /// The `MediaQueryList` objects with listeners by their order of
    /// creation: the handles of their data (`media_queries.rs`).
    pub(crate) media_queries: std::collections::BTreeMap<u64, Gc<swb_js::Generic>>,
    /// The `MediaQueryList` objects made so far.
    pub(crate) media_query_count: u64,
    /// The mutation observers (`mutation_observer.rs`).
    pub(crate) mutation: crate::mutation_observer::MutationState,
    /// The intersection observers (`intersection_observer.rs`).
    pub(crate) io: crate::intersection_observer::IoState,
    /// The resize observers (`resize_observer.rs`).
    pub(crate) ro: crate::resize_observer::RoState,
    /// The `AbortSignal.timeout()` signals that wait for their timer task,
    /// by id (`abort.rs`), and the last id given.
    pub(crate) signal_timeouts: HashMap<u64, Gc<Object>>,
    pub(crate) next_timeout_signal: u64,
    /// The `fetch()` calls in flight (`fetch/`).
    pub(crate) fetches: crate::fetch::Fetches,
    /// The forms whose entry list is being constructed (`file_api/form_data.rs`).
    pub(crate) constructing_forms: std::collections::HashSet<NodeId>,
    /// The `FileReader` objects that read, by id (`file_api/reader.rs`).
    pub(crate) readers: HashMap<u64, Gc<Object>>,
    pub(crate) next_reader: u64,
    /// The bytes of the beacons that have not been sent (`navigator.rs`).
    pub(crate) beacon_bytes: usize,
    /// The messages of `window.postMessage()` that wait for their task
    /// (`messaging.rs`).
    pub(crate) messages: crate::messaging::Messages,
    /// The objects of `navigator.plugins` and `navigator.mimeTypes`, made
    /// at the first access (`navigator.rs`).
    pub(crate) plugins: Option<Gc<Object>>,
    pub(crate) mime_types: Option<Gc<Object>>,
    /// The `img.decode()` promises that wait for their image
    /// (`image_events.rs`).
    pub(crate) decodes: HashMap<NodeId, Vec<Gc<Object>>>,
}

impl HostState for WebState {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        self.glue.trace(tracer);
        for value in &self.pinned {
            tracer.value(*value);
        }
        self.event_loop.trace(tracer);
        self.ports.trace(tracer);
        for proxy in self.window_proxies.values() {
            tracer.object(*proxy);
        }
        self.doc.trace(tracer);
        for doc in self.docs.iter().flatten() {
            doc.trace(tracer);
        }
    }
}

impl DocState {
    fn trace(&self, tracer: &mut Tracer<'_>) {
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
        for list in self.listeners.values() {
            list.trace(tracer);
        }
        self.window_listeners.trace(tracer);
        self.handlers.trace(tracer);
        if let Some(event) = self.current_event {
            tracer.object(event);
        }
        self.timers.trace(tracer);
        self.perf.trace(tracer);
        self.idle.trace(tracer);
        for object in [
            self.performance,
            self.navigator,
            self.crypto,
            self.clipboard,
            self.plugins,
            self.mime_types,
            self.screen,
            self.screen_orientation,
            self.location,
            self.history,
        ]
        .into_iter()
        .flatten()
        {
            tracer.object(object);
        }
        for object in self.storage_objects.iter().flatten() {
            tracer.object(*object);
        }
        if let Some(cache) = &self.history_state {
            cache.trace(tracer);
        }
        self.animation_frames.trace(tracer);
        self.mutation.trace(tracer);
        self.io.trace(tracer);
        self.ro.trace(tracer);
        for list in self.media_queries.values() {
            tracer.generic(*list);
        }
        for signal in self.signal_timeouts.values() {
            tracer.object(*signal);
        }
        self.fetches.trace(tracer);
        self.cssom.trace(tracer);
        for reader in self.readers.values() {
            tracer.object(*reader);
        }
        for promise in self.decodes.values().flatten() {
            tracer.object(*promise);
        }
    }
}

// ----- The documents of a runtime -----

/// What [`enter_call`] found: the document to go back to, or [`NO_SWITCH`].
pub(crate) type Entered = DocKey;

/// [`Entered`] when no switch happened.
pub(crate) const NO_SWITCH: Entered = DocKey::MAX;

/// Makes the document `key` the current one; returns the one it replaced.
fn switch_to(state: &mut WebState, key: DocKey) -> VmResult<DocKey> {
    let old = state.doc.key;
    if old == key {
        return Ok(old);
    }
    let Some(next) = state.docs.get_mut(key as usize).and_then(Option::take) else {
        return Err(VmError::type_error("a document key without a document"));
    };
    let previous = std::mem::replace(&mut state.doc, next);
    if let Some(slot) = state.docs.get_mut(old as usize) {
        *slot = Some(previous);
    }
    state.event_loop.tasks.set_document(key);
    Ok(old)
}

/// The document of a realm.
fn document_of_realm(state: &WebState, realm: u32) -> DocKey {
    state
        .realm_docs
        .get(realm as usize)
        .copied()
        .unwrap_or(state.doc.key)
}

/// The document of a platform object: the one in the host data word of a
/// node's wrapper (and the other objects that name a node with their word),
/// else the document of the object's realm. `None` for another value.
fn document_of_object(rt: &Runtime, state: &WebState, value: Value) -> Option<DocKey> {
    let (_, data) = rt.host_object(value)?;
    match data {
        HostData::Word(word) if HostData::Word(word) != NOT_A_NODE => Some(b::node_document(word)),
        _ => Some(document_of_realm(state, rt.host_object_realm(value)?)),
    }
}

/// The start of a native of the glue (ADR 0031 section 5): the document of
/// its `this`, or without a platform object as `this` the document of the
/// function's realm, becomes the current one. Give the result to [`leave`].
/// With one document in the runtime, this is one comparison.
#[inline]
pub(crate) fn enter_call(rt: &mut Runtime, call: &NativeCall) -> VmResult<Entered> {
    if !rt.host_flag() {
        return Ok(NO_SWITCH);
    }
    switch_for_call(rt, call)
}

/// [`enter_call`] of a runtime with several documents.
#[cold]
#[inline(never)]
fn switch_for_call(rt: &mut Runtime, call: &NativeCall) -> VmResult<Entered> {
    let state = state(rt)?;
    let key = document_of_object(rt, state, call.this())
        .unwrap_or_else(|| document_of_realm(state, call.realm()));
    switch_if_other(rt, key)
}

/// The start of a hook of the platform object `object` (a legacy platform
/// object, the named properties object): as [`enter_call`], by the object.
#[inline]
pub(crate) fn enter_object(rt: &mut Runtime, object: Gc<Object>) -> VmResult<Entered> {
    if !rt.host_flag() {
        return Ok(NO_SWITCH);
    }
    switch_for_object(rt, object)
}

/// [`enter_object`] of a runtime with several documents.
#[cold]
#[inline(never)]
fn switch_for_object(rt: &mut Runtime, object: Gc<Object>) -> VmResult<Entered> {
    let state = state(rt)?;
    let Some(key) = document_of_object(rt, state, object.into()) else {
        return Ok(NO_SWITCH);
    };
    switch_if_other(rt, key)
}

/// The start of a function that belongs to `realm` and has no platform
/// object (the mutation observer microtask): as [`enter_call`], by the realm.
pub(crate) fn enter_realm(rt: &mut Runtime, realm: u32) -> VmResult<Entered> {
    if !rt.host_flag() {
        return Ok(NO_SWITCH);
    }
    let key = document_of_realm(state(rt)?, realm);
    switch_if_other(rt, key)
}

/// Makes `key` the current document if it is not; the [`Entered`] for it.
pub(crate) fn switch_if_other(rt: &mut Runtime, key: DocKey) -> VmResult<Entered> {
    if state(rt)?.doc.key == key {
        return Ok(NO_SWITCH);
    }
    crate::mutation_observer::before_switch(rt);
    switch_to(state_mut(rt)?, key)
}

/// The end of a native or a hook: back to the document that [`enter_call`]
/// or [`enter_object`] found.
#[inline]
pub(crate) fn leave(rt: &mut Runtime, entered: Entered) {
    if entered != NO_SWITCH {
        leave_switched(rt, entered);
    }
}

/// [`leave`] after a switch.
#[cold]
#[inline(never)]
fn leave_switched(rt: &mut Runtime, entered: Entered) {
    crate::mutation_observer::before_switch(rt);
    if let Ok(state) = state_mut(rt)
        && let Err(error) = switch_to(state, entered)
    {
        log::warn!("the document of a native is gone: {error:?}");
    }
}

/// The current document's key.
pub(crate) fn document_key(rt: &Runtime) -> VmResult<DocKey> {
    Ok(state(rt)?.doc.key)
}

/// The realm of the current document.
pub(crate) fn doc_realm(rt: &Runtime) -> VmResult<u32> {
    Ok(state(rt)?.doc.realm)
}

/// The `WindowProxy` of the current document's `Window`: the global this
/// value of its realm.
pub(crate) fn window_proxy(rt: &Runtime) -> VmResult<Gc<Object>> {
    rt.global_this(doc_realm(rt)?)
}

/// The `Window` of the current document: the global object of its realm.
pub(crate) fn window_global(rt: &Runtime) -> VmResult<Gc<Object>> {
    rt.global(doc_realm(rt)?)
}

/// The realm of the document `key`.
fn realm_of_document(rt: &Runtime, key: DocKey) -> VmResult<u32> {
    let state = state(rt)?;
    if state.doc.key == key {
        return Ok(state.doc.realm);
    }
    state
        .docs
        .get(key as usize)
        .and_then(Option::as_deref)
        .map(|doc| doc.realm)
        .ok_or(VmError::type_error("a document key without a document"))
}

/// The `WindowProxy` of the document `key`.
pub(crate) fn window_proxy_of(rt: &Runtime, key: DocKey) -> VmResult<Gc<Object>> {
    rt.global_this(realm_of_document(rt, key)?)
}

/// The `Window` of the document `key`.
pub(crate) fn global_of(rt: &Runtime, key: DocKey) -> VmResult<Gc<Object>> {
    rt.global(realm_of_document(rt, key)?)
}

/// The host data of the wrapper of `node` of the current document.
pub(crate) fn node_data_here(rt: &Runtime, node: NodeId) -> VmResult<HostData> {
    Ok(b::node_data_in(document_key(rt)?, node))
}

/// Registers the realm `realm` of a new document with the frame `frame`:
/// the record of the document, the `WindowProxy` of the frame (one per
/// frame: an existing one gets the new `Window` as its target), the global
/// this value of the realm. The first document of a runtime replaces the
/// empty record. Returns the key.
pub(crate) fn add_document(
    rt: &mut Runtime,
    realm: u32,
    frame: u32,
    clock: DocumentClock,
) -> VmResult<DocKey> {
    let global = rt.global(realm)?;
    let known = state(rt)?.window_proxies.get(&frame).copied();
    let proxy = match known {
        Some(proxy) => {
            rt.set_window_proxy_target(proxy, Some(global))?;
            proxy
        }
        None => rt.new_window_proxy(frame, Some(global))?,
    };
    let signal = rt.host_signal_cell();
    let state = state_mut(rt)?;
    state.window_proxies.insert(frame, proxy);
    let first = state.realm_docs.is_empty();
    let key = if first {
        0
    } else {
        DocKey::try_from(state.docs.len()).map_err(|_| VmError::type_error("too many documents"))?
    };
    let record = Box::new(DocState {
        key,
        realm,
        clock,
        mutation: crate::mutation_observer::MutationState::new(signal),
        ..DocState::default()
    });
    if first {
        state.doc = record;
        state.docs.push(None);
    } else {
        state.docs.push(Some(record));
    }
    if state.realm_docs.len() <= realm as usize {
        state.realm_docs.resize(realm as usize + 1, 0);
    }
    if let Some(slot) = state.realm_docs.get_mut(realm as usize) {
        *slot = key;
    }
    let several = state.docs.len() > 1;
    rt.set_host_flag(several);
    rt.set_global_this(realm, proxy)?;
    Ok(key)
}

/// All documents of the runtime, the current one first.
pub(crate) fn documents_mut(state: &mut WebState) -> impl Iterator<Item = &mut DocState> {
    let others = state.docs.iter_mut().flatten();
    std::iter::once(&mut *state.doc).chain(others.map(|doc| &mut **doc))
}

/// The record of a document, for the host: its lent state goes in and out.
pub(crate) fn document_mut(state: &mut WebState, key: DocKey) -> Option<&mut DocState> {
    if state.doc.key == key {
        return Some(&mut state.doc);
    }
    state.docs.get_mut(key as usize)?.as_deref_mut()
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

/// The host data of an object that carries nothing ([`NOT_A_NODE`]), for
/// [`window_object`].
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of the data argument"
)]
pub(crate) fn no_data(_rt: &mut Runtime) -> VmResult<HostData> {
    Ok(NOT_A_NODE)
}

/// The host data of an `EventTarget` that is no node: its listeners, for
/// [`window_object`].
pub(crate) fn target_data(rt: &mut Runtime) -> VmResult<HostData> {
    rt.alloc_host_data(crate::events::TargetData::default())
}

/// The `[SameObject]` getter of a window attribute: the object in the
/// document's field that `slot` selects, made at the first access in the
/// document's realm as an instance of `class` with the host data that `data`
/// allocates.
pub(crate) fn window_object(
    cx: &mut Cx<'_>,
    slot: fn(&mut DocState) -> &mut Option<Gc<Object>>,
    class: ClassId,
    data: fn(&mut Runtime) -> VmResult<HostData>,
) -> b::Result<Gc<Object>> {
    if let Some(object) = *slot(&mut state_mut(cx.rt)?.doc) {
        return Ok(object);
    }
    let data = data(cx.rt)?;
    let realm = doc_realm(cx.rt)?;
    let object = cx.rt.new_host_object(realm, class, data)?;
    *slot(&mut state_mut(cx.rt)?.doc) = Some(object);
    Ok(object)
}

/// The lent document state.
pub(crate) fn engine(rt: &Runtime) -> VmResult<&dyn EngineHost> {
    state(rt)?
        .doc
        .host
        .as_deref()
        .ok_or(VmError::type_error(NO_DOCUMENT))
}

/// The lent document state, for changes.
pub(crate) fn engine_mut(rt: &mut Runtime) -> VmResult<&mut dyn EngineHost> {
    match state_mut(rt)?.doc.host.as_deref_mut() {
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
    if let Some(object) = state(rt)?.doc.side.get(&(node, slot)) {
        return Ok(*object);
    }
    let object = create(rt)?;
    state_mut(rt)?.doc.side.insert((node, slot), object);
    Ok(object)
}

/// The wrapper of `node` of the current document, made with `proto` as its
/// prototype if the node has none yet (a constructor that script called
/// through a subclass: `class A extends Image`); else the class's prototype
/// in the document's realm.
pub(crate) fn wrap_node_with_proto(
    rt: &mut Runtime,
    node: NodeId,
    proto: Option<Gc<Object>>,
) -> VmResult<Gc<Object>> {
    let state = state(rt)?;
    if let Some(Some(wrapper)) = state.doc.wrappers.get(node.index()) {
        return Ok(*wrapper);
    }
    let (key, realm) = (state.doc.key, state.doc.realm);
    let dom = dom(rt)?;
    if dom.get(node).is_none() {
        return Err(VmError::type_error(
            "a wrapper of a node that does not exist",
        ));
    }
    let class = class_of(dom, node);
    let data = b::node_data_in(key, node);
    let wrapper = match proto {
        Some(proto) => rt.new_host_object_with_proto(realm, Some(proto), class, data)?,
        None => rt.new_host_object(realm, class, data)?,
    };
    b::glue::install_unforgeables::<WebState>(rt, realm, class, wrapper)?;
    let doc = &mut state_mut(rt)?.doc;
    if doc.wrappers.len() <= node.index() {
        doc.wrappers.resize(node.index() + 1, None);
    }
    if let Some(slot) = doc.wrappers.get_mut(node.index()) {
        *slot = Some(wrapper);
    }
    Ok(wrapper)
}

impl b::Host for WebState {
    fn glue_state(&mut self) -> &mut GlueState {
        &mut self.glue
    }

    /// The wrapper is made in the realm of the node's document; `realm` is
    /// the realm of the function that asks.
    fn wrap_node(rt: &mut Runtime, _realm: u32, node: NodeId) -> VmResult<Gc<Object>> {
        wrap_node_with_proto(rt, node, None)
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
