//! `EventTarget` (DOM §2.7, <https://dom.spec.whatwg.org/#interface-eventtarget>):
//! `addEventListener`, `removeEventListener`, `dispatchEvent` and the
//! constructor.
//!
//! Where the listener list of a target lives depends on its kind: a node's
//! list is in the binding state by `NodeId` (a node with listeners has a
//! wrapper, and both live as long as the document); the window's list is
//! in the binding state too; a target made by `new EventTarget()` (or a
//! subclass) keeps its list in its generic data ([`TargetData`]) and dies
//! with the object. The kinds are told apart by class, not by host data.

use std::cell::Cell;

use swb_dom::local_name;
use swb_js::{Gc, Generic, GenericData, Object, Runtime, Tracer, VmError, VmResult};

use super::listeners::{Listener, Listeners};
use crate::bindings::{self as b, Cx, NodeId, Platform};
use crate::generated as g;
use crate::generated::types::{
    AddEventListenerOptionsOrBoolean, EventListener, EventListenerOptionsOrBoolean,
};
use crate::state::{WebState, dom, state, state_mut};

/// The most listeners on the nodes and the window of a document; one more
/// ends the task (the lists live outside the JavaScript heap, so the heap
/// limit does not bound them; a listener of its own type costs about
/// 400 bytes). The lists of constructed targets count against the heap.
pub(super) const MAX_DOCUMENT_LISTENERS: usize = 1 << 18;

/// The data of an `EventTarget` made by its constructor: its listeners.
#[derive(Debug, Default)]
pub(crate) struct TargetData {
    listeners: Listeners,
}

impl GenericData for TargetData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        self.listeners.trace(tracer);
    }

    fn heap_size(&self) -> usize {
        self.listeners.heap_size()
    }
}

/// The kind of an event target, which says where its listeners are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TargetKind {
    Node(NodeId),
    Window,
    /// A constructed `EventTarget`: the handle of its [`TargetData`].
    Constructed(Gc<Generic>),
}

/// The kind of the event target `object`.
pub(crate) fn kind_of(rt: &Runtime, object: Gc<Object>) -> VmResult<TargetKind> {
    let value = object.into();
    let illegal = || VmError::type_error("Illegal invocation");
    let (_, data) = rt.host_object(value).ok_or_else(illegal)?;
    if rt.implements(value, g::NODE) {
        return b::node_of(data).map(TargetKind::Node).ok_or_else(illegal);
    }
    if rt.implements(value, g::WINDOW) {
        return Ok(TargetKind::Window);
    }
    match data {
        swb_js::HostData::Generic(handle) if rt.heap().generic::<TargetData>(handle).is_ok() => {
            Ok(TargetKind::Constructed(handle))
        }
        _ => Err(illegal()),
    }
}

/// The listeners of `kind` of a target, cloned for a dispatch.
pub(crate) fn listeners_of(
    rt: &Runtime,
    target: TargetKind,
    kind: &str,
) -> VmResult<Vec<std::rc::Rc<Listener>>> {
    Ok(match target {
        TargetKind::Node(node) => state(rt)?
            .listeners
            .get(&node)
            .map(|l| l.of_type(kind))
            .unwrap_or_default(),
        TargetKind::Window => state(rt)?.window_listeners.of_type(kind),
        TargetKind::Constructed(handle) => rt
            .heap()
            .generic::<TargetData>(handle)?
            .listeners
            .of_type(kind),
    })
}

/// Runs `f` on the listener list of a target (a node's list is created on
/// the first use).
pub(crate) fn with_listeners<R>(
    rt: &mut Runtime,
    target: TargetKind,
    f: impl FnOnce(&mut Listeners) -> R,
) -> VmResult<R> {
    Ok(match target {
        TargetKind::Node(node) => f(state_mut(rt)?.listeners.entry(node).or_default()),
        TargetKind::Window => f(&mut state_mut(rt)?.window_listeners),
        TargetKind::Constructed(handle) => {
            f(&mut rt.heap_mut().generic_mut::<TargetData>(handle)?.listeners)
        }
    })
}

/// DOM "default passive value": true for the touch and wheel events at
/// the window, a document, its document element and its body.
pub(super) fn default_passive(rt: &Runtime, target: TargetKind, kind: &str) -> VmResult<bool> {
    if !matches!(kind, "touchstart" | "touchmove" | "wheel" | "mousewheel") {
        return Ok(false);
    }
    Ok(match target {
        TargetKind::Window => true,
        TargetKind::Constructed(_) => false,
        TargetKind::Node(node) => {
            let dom = dom(rt)?;
            let document = dom.node_document(node);
            let element = dom.element_children(document).next();
            let body = element
                .filter(|&e| dom.is_html_element(e, &local_name!("html")))
                .and_then(|html| {
                    dom.element_children(html).find(|&c| {
                        dom.is_html_element(c, &local_name!("body"))
                            || dom.is_html_element(c, &local_name!("frameset"))
                    })
                });
            node == document || Some(node) == element || Some(node) == body
        }
    })
}

/// The listeners on the nodes and the window of the document.
fn document_listeners(rt: &Runtime) -> VmResult<usize> {
    let state = state(rt)?;
    Ok(state.listener_count)
}

impl g::event_target::EventTargetImpl for WebState {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(TargetData::default())?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    /// DOM "add an event listener" with the flattened options.
    fn add_event_listener(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        callback: Option<EventListener>,
        options: AddEventListenerOptionsOrBoolean,
    ) -> b::Result<()> {
        let target = kind_of(cx.rt, this.object)?;
        let Some(EventListener(callback)) = callback else {
            return Ok(());
        };
        super::handlers::sync(cx.rt, target)?;
        let (capture, passive, once) = match options {
            AddEventListenerOptionsOrBoolean::Boolean(capture) => (capture, None, false),
            AddEventListenerOptionsOrBoolean::AddEventListenerOptions(o) => {
                (o.capture, o.passive, o.once)
            }
        };
        let passive = match passive {
            Some(passive) => passive,
            None => default_passive(cx.rt, target, &kind)?,
        };
        let in_document = !matches!(target, TargetKind::Constructed(_));
        if in_document && document_listeners(cx.rt)? >= MAX_DOCUMENT_LISTENERS {
            log::warn!("a document has more than {MAX_DOCUMENT_LISTENERS} event listeners");
            return Err(cx
                .rt
                .terminate("too many event listeners in the document")
                .into());
        }
        let listener = Listener {
            callback,
            capture,
            passive,
            once,
            removed: Cell::new(false),
            handler: None,
        };
        let added = with_listeners(cx.rt, target, |list| list.add(&kind, listener))?;
        if added && in_document {
            state_mut(cx.rt)?.listener_count += 1;
        }
        Ok(())
    }

    /// DOM `removeEventListener`.
    fn remove_event_listener(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        callback: Option<EventListener>,
        options: EventListenerOptionsOrBoolean,
    ) -> b::Result<()> {
        let target = kind_of(cx.rt, this.object)?;
        let Some(EventListener(callback)) = callback else {
            return Ok(());
        };
        super::handlers::sync(cx.rt, target)?;
        let capture = match options {
            EventListenerOptionsOrBoolean::Boolean(capture) => capture,
            EventListenerOptionsOrBoolean::EventListenerOptions(o) => o.capture,
        };
        remove(cx.rt, target, |list| list.remove(&kind, callback, capture))?;
        Ok(())
    }

    /// DOM `dispatchEvent`.
    fn dispatch_event(cx: &mut Cx<'_>, this: Platform, event: Platform) -> b::Result<bool> {
        let handle = super::event_handle(event.data)?;
        let data = super::data_mut(cx.rt, handle)?;
        if data.dispatching {
            return Err(b::Exception::dom(
                "InvalidStateError",
                "The event is already being dispatched.",
            ));
        }
        if !data.initialized {
            return Err(b::Exception::dom(
                "InvalidStateError",
                "The event provided is uninitialized.",
            ));
        }
        data.is_trusted = false;
        let realm = cx.realm();
        Ok(super::handlers::with_context(
            cx.rt,
            "Failed to execute 'dispatchEvent' on 'EventTarget'",
            |rt| super::dispatch::dispatch(rt, realm, this.object, event.object, None),
        )?)
    }
}

/// Removes a listener from a target with `f` (which returns true if it
/// removed one), and counts it for the document's limit.
pub(crate) fn remove(
    rt: &mut Runtime,
    target: TargetKind,
    f: impl FnOnce(&mut Listeners) -> bool,
) -> VmResult<bool> {
    let removed = with_listeners(rt, target, f)?;
    if let TargetKind::Node(node) = target {
        let state = state_mut(rt)?;
        if state.listeners.get(&node).is_some_and(Listeners::is_empty) {
            state.listeners.remove(&node);
        }
    }
    if removed && !matches!(target, TargetKind::Constructed(_)) {
        let state = state_mut(rt)?;
        state.listener_count = state.listener_count.saturating_sub(1);
    }
    Ok(removed)
}
