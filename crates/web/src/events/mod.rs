//! Events (DOM §2, <https://dom.spec.whatwg.org/#events>; ADR 0028 section
//! 11): `Event` with its state and flags, its subclasses `CustomEvent`
//! (<https://dom.spec.whatwg.org/#interface-customevent>), `ErrorEvent`
//! (<https://html.spec.whatwg.org/multipage/webappapis.html#the-errorevent-interface>),
//! `PromiseRejectionEvent`
//! (<https://html.spec.whatwg.org/multipage/webappapis.html#promiserejectionevent>),
//! `PageTransitionEvent`
//! (<https://html.spec.whatwg.org/multipage/nav-history-apis.html#pagetransitionevent>),
//! `MediaQueryListEvent`
//! (<https://drafts.csswg.org/cssom-view/#mediaquerylistevent>, feature 7b2)
//! and `document.createEvent`
//! (<https://dom.spec.whatwg.org/#dom-document-createevent>).
//!
//! - `listeners`: the event listener list of a target.
//! - `target`: `EventTarget` (listeners on nodes, the window and
//!   constructed targets).
//! - `dispatch`: the dispatch algorithm (DOM §2.9) and the entry for
//!   events that the engine fires.
//!
//! An event object is a host object whose generic data is [`EventData`];
//! the subclasses keep their attributes in [`Extra`]. The path goes through
//! shadow trees (retargeting, `relatedTarget` per struct, closed trees in
//! `composedPath`; ADR 0036 section 7); the touch target list is left out
//! (swb has no touch events).

mod dispatch;
mod handler_names;
pub(crate) mod handlers;
mod listened;
mod listeners;
pub(crate) mod misc;
mod mouse;
mod target;
mod ui;
mod user;

pub use dispatch::{DispatchTarget, EventInterface, TrustedEvent};
pub(crate) use dispatch::{
    dispatch, dispatch_trusted, fire_at_object, fire_trusted, new_event_object,
};
pub(crate) use handlers::{
    Handlers, attribute_changed as handler_attribute_changed,
    before_attribute_change as handler_before_attribute_change,
};
pub(crate) use listened::Listened;
pub(crate) use listeners::Listeners;
pub(crate) use mouse::new_click_event;
pub(crate) use target::{TargetData, has_listener_for, remove_listener};
pub(crate) use user::node_has_blocking_listener;
pub use user::{UserEvent, UserKind, modifiers};

use swb_js::{
    Gc, Generic, GenericData, HostData, Object, Runtime, Tracer, Value, VmError, VmResult,
};

use crate::bindings::{self as b, Cx, Exception, NodeId, Platform};
use crate::generated as g;
use crate::generated::types::{
    CustomEventInit, ErrorEventInit, EventInit, MediaQueryListEventInit, PageTransitionEventInit,
    PromiseRejectionEventInit,
};
use crate::state::{DocKey, WebState, state};

/// `eventPhase` values (DOM §2.2).
pub(crate) const NONE: u16 = 0;
pub(crate) const CAPTURING_PHASE: u16 = 1;
pub(crate) const AT_TARGET: u16 = 2;
pub(crate) const BUBBLING_PHASE: u16 = 3;

/// The attributes of the subclasses of `Event`.
#[derive(Debug)]
pub(crate) enum Extra {
    /// `Event` itself.
    None,
    /// `CustomEvent.detail`.
    Custom { detail: Value },
    /// `ErrorEvent`.
    Error {
        message: String,
        filename: String,
        lineno: u32,
        colno: u32,
        error: Value,
    },
    /// `PromiseRejectionEvent`.
    PromiseRejection { promise: Gc<Object>, reason: Value },
    /// `PageTransitionEvent.persisted`.
    PageTransition { persisted: bool },
    /// `MediaQueryListEvent` (CSSOM View §4.2).
    MediaQuery { media: String, matches: bool },
    /// The UI events: `UIEvent` and its subclasses (`ui.rs`, `mouse.rs`).
    Ui(Box<ui::Fields>),
    /// `SubmitEvent`, `MessageEvent` and the other events of `misc.rs`.
    Other(Box<misc::Other>),
}

/// A target in an event's path: an object (the window, a constructed
/// target, the first struct's target), or a node of the document whose
/// wrapper is made when a listener or a script needs it (a path of
/// thousands of structs makes no wrappers for the nodes without listeners).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target {
    /// An object.
    Object(Gc<Object>),
    /// A node of the document with this key (the document of the dispatch,
    /// whichever document is current when the target is read).
    Node(DocKey, NodeId),
}

impl Target {
    /// The object of the target: the wrapper of a node.
    pub(crate) fn object(self, rt: &mut Runtime) -> VmResult<Gc<Object>> {
        match self {
            Target::Object(object) => Ok(object),
            Target::Node(key, node) => {
                // A getter of an event enters the event's own document, which
                // can differ from the document of the path.
                let entered = crate::state::switch_if_other(rt, key)?;
                let wrapper = crate::state::doc_realm(rt).and_then(|realm| {
                    <WebState as crate::bindings::Host>::wrap_node(rt, realm, node)
                });
                crate::state::leave(rt, entered);
                wrapper
            }
        }
    }

    /// [`Target::object`] for an optional target.
    pub(crate) fn object_of(
        rt: &mut Runtime,
        target: Option<Target>,
    ) -> VmResult<Option<Gc<Object>>> {
        target.map(|t| t.object(rt)).transpose()
    }
}

/// A struct of an event's path (DOM §2.2, "path").
#[derive(Clone, Copy, Debug)]
pub(crate) struct PathItem {
    /// The invocation target.
    pub(crate) target: Target,
    /// True if the invocation target's root is a shadow root.
    pub(crate) in_shadow_tree: bool,
    /// The shadow-adjusted target: the target (or the legacy target
    /// override) for the first struct, the invocation target for the first
    /// struct outside a shadow tree that the path leaves, `None` for the
    /// others.
    pub(crate) shadow_adjusted_target: Option<Target>,
    /// The related target of this struct (the event's `relatedTarget`
    /// retargeted against the invocation target); `None` for an event
    /// without one.
    pub(crate) related_target: Option<Gc<Object>>,
    /// True if the invocation target is a closed shadow root.
    pub(crate) root_of_closed_tree: bool,
    /// True if the invocation target is a slot in a closed shadow tree that
    /// the path reached from an assigned node.
    pub(crate) slot_in_closed_tree: bool,
}

/// The state of an event (DOM §2.2): its attributes and flags.
#[derive(Debug)]
pub(crate) struct EventData {
    pub(crate) kind: String,
    pub(crate) bubbles: bool,
    pub(crate) cancelable: bool,
    pub(crate) composed: bool,
    pub(crate) is_trusted: bool,
    pub(crate) time_stamp: f64,
    pub(crate) target: Option<Target>,
    pub(crate) current_target: Option<Target>,
    pub(crate) phase: u16,
    pub(crate) path: Vec<PathItem>,
    /// DOM "clearTargets": `target` and `relatedTarget` become null after
    /// the dispatch.
    pub(crate) clear_targets: bool,
    pub(crate) stop_propagation: bool,
    pub(crate) stop_immediate_propagation: bool,
    pub(crate) canceled: bool,
    pub(crate) in_passive_listener: bool,
    pub(crate) initialized: bool,
    pub(crate) dispatching: bool,
    pub(crate) extra: Extra,
}

impl EventData {
    /// A new event as the constructor makes it (DOM "inner event creation
    /// steps"): initialized, not trusted.
    pub(crate) fn new(kind: String, init: &EventInit, time_stamp: f64, extra: Extra) -> EventData {
        EventData {
            kind,
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            is_trusted: false,
            time_stamp,
            target: None,
            current_target: None,
            phase: NONE,
            path: Vec::new(),
            clear_targets: false,
            stop_propagation: false,
            stop_immediate_propagation: false,
            canceled: false,
            in_passive_listener: false,
            initialized: true,
            dispatching: false,
            extra,
        }
    }

    /// The `relatedTarget` that the event shows now (`FocusEvent` and
    /// `MouseEvent` and their subclasses).
    pub(crate) fn related_target(&self) -> Option<Gc<Object>> {
        match &self.extra {
            Extra::Ui(f) if f.family == ui::Family::Focus => f.related_target,
            Extra::Ui(f) if f.family.is_mouse() => f.mouse.related_target,
            _ => None,
        }
    }

    /// Sets what [`EventData::related_target`] shows (DOM "invoke" step 3
    /// and the clear-targets step); no effect on other events.
    pub(crate) fn set_related_target(&mut self, value: Option<Gc<Object>>) {
        match &mut self.extra {
            Extra::Ui(f) if f.family == ui::Family::Focus => f.related_target = value,
            Extra::Ui(f) if f.family.is_mouse() => f.mouse.related_target = value,
            _ => {}
        }
    }

    /// DOM "set the canceled flag": only for a cancelable event outside a
    /// passive listener.
    pub(crate) fn set_canceled(&mut self) {
        if self.cancelable && !self.in_passive_listener {
            self.canceled = true;
        }
    }

    /// DOM "initialize" (of `initEvent` and `initCustomEvent`).
    pub(crate) fn initialize(&mut self, kind: String, bubbles: bool, cancelable: bool) {
        self.initialized = true;
        self.stop_propagation = false;
        self.stop_immediate_propagation = false;
        self.canceled = false;
        self.is_trusted = false;
        self.target = None;
        self.kind = kind;
        self.bubbles = bubbles;
        self.cancelable = cancelable;
    }
}

impl GenericData for EventData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        let object = |target: Option<Target>| match target {
            Some(Target::Object(object)) => Some(object),
            _ => None,
        };
        for target in [self.target, self.current_target] {
            if let Some(object) = object(target) {
                tracer.object(object);
            }
        }
        for item in &self.path {
            for object in [
                object(Some(item.target)),
                object(item.shadow_adjusted_target),
                item.related_target,
            ]
            .into_iter()
            .flatten()
            {
                tracer.object(object);
            }
        }
        match &self.extra {
            Extra::None | Extra::PageTransition { .. } | Extra::MediaQuery { .. } => {}
            Extra::Ui(fields) => fields.trace(tracer),
            Extra::Other(other) => other.trace(tracer),
            Extra::Custom { detail } => tracer.value(*detail),
            Extra::Error { error, .. } => tracer.value(*error),
            Extra::PromiseRejection { promise, reason } => {
                tracer.object(*promise);
                tracer.value(*reason);
            }
        }
    }

    fn heap_size(&self) -> usize {
        let extra = match &self.extra {
            Extra::Error {
                message, filename, ..
            } => message.capacity() + filename.capacity(),
            Extra::Ui(fields) => fields.heap_size(),
            Extra::Other(other) => other.heap_size(),
            Extra::MediaQuery { media, .. } => media.capacity(),
            _ => 0,
        };
        self.kind.capacity() + self.path.capacity() * size_of::<PathItem>() + extra
    }
}

/// The generic data handle of an event object.
pub(crate) fn event_handle(data: HostData) -> VmResult<Gc<Generic>> {
    match data {
        HostData::Generic(handle) => Ok(handle),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation")),
    }
}

/// The state of an event.
pub(crate) fn data(rt: &Runtime, event: Gc<Generic>) -> VmResult<&EventData> {
    Ok(rt.heap().generic::<EventData>(event)?)
}

/// The state of an event, for changes.
pub(crate) fn data_mut(rt: &mut Runtime, event: Gc<Generic>) -> VmResult<&mut EventData> {
    Ok(rt.heap_mut().generic_mut::<EventData>(event)?)
}

/// The node `object` retargeted against the event target `against` (DOM
/// "retarget"), for the getters of event attributes that retarget
/// (`ToggleEvent.source`): against the document when `against` is not a
/// node of the current document. An object that is not a node of the
/// current document stays as it is.
/// <https://dom.spec.whatwg.org/#retarget>
pub(crate) fn retarget_object(
    rt: &mut Runtime,
    object: Gc<Object>,
    against: Option<Target>,
) -> VmResult<Gc<Object>> {
    let Ok(target::TargetKind::Node(node)) = target::kind_of(rt, object) else {
        return Ok(object);
    };
    let key = crate::state::document_key(rt)?;
    let against = match against {
        Some(Target::Node(k, n)) if k == key => n,
        Some(Target::Object(o)) => match target::kind_of(rt, o) {
            Ok(target::TargetKind::Node(n)) => n,
            _ => NodeId::DOCUMENT,
        },
        _ => NodeId::DOCUMENT,
    };
    let result = crate::state::dom(rt)?.retarget(node, against);
    if result == node {
        return Ok(object);
    }
    let realm = crate::state::doc_realm(rt)?;
    <WebState as b::Host>::wrap_node(rt, realm, result)
}

/// The state of the event `this`.
fn this_data<'a>(cx: &'a Cx<'_>, this: Platform) -> b::Result<&'a EventData> {
    Ok(data(cx.rt, event_handle(this.data)?)?)
}

/// The state of the event `this`, for changes.
fn this_data_mut<'a>(cx: &'a mut Cx<'_>, this: Platform) -> b::Result<&'a mut EventData> {
    Ok(data_mut(cx.rt, event_handle(this.data)?)?)
}

/// The current high resolution time relative to the time origin of the
/// window, coarsened to 0.1 ms as Chromium does for pages without
/// cross-origin isolation (HR Time §4,
/// <https://w3c.github.io/hr-time/#dfn-current-high-resolution-time>).
/// The time comes from the page clock (`clock.rs`), so it follows virtual
/// time.
pub(crate) fn now(rt: &Runtime) -> f64 {
    state(rt).map_or(0.0, |state| state.doc.clock.now())
}

/// A new event object of a constructor (`new Event(...)` and the
/// subclasses): the prototype of `new.target`.
fn construct(
    cx: &mut Cx<'_>,
    kind: String,
    init: &EventInit,
    extra: Extra,
) -> b::Result<Gc<Object>> {
    let event = EventData::new(kind, init, now(cx.rt), extra);
    let data = cx.rt.alloc_host_data(event)?;
    Ok(cx.new_instance::<WebState>(data)?)
}

/// The `EventInit` part of a subclass's dictionary.
fn base(bubbles: bool, cancelable: bool, composed: bool) -> EventInit {
    EventInit {
        bubbles,
        cancelable,
        composed,
    }
}

impl g::event::EventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: EventInit) -> b::Result<Gc<Object>> {
        construct(cx, kind, &init, Extra::None)
    }

    fn type_(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(this_data(cx, this)?.kind.clone())
    }

    fn target(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        let target = this_data(cx, this)?.target;
        Ok(Target::object_of(cx.rt, target)?)
    }

    fn src_element(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        let target = this_data(cx, this)?.target;
        Ok(Target::object_of(cx.rt, target)?)
    }

    fn current_target(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        let target = this_data(cx, this)?.current_target;
        Ok(Target::object_of(cx.rt, target)?)
    }

    fn composed_path(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        let event = this_data(cx, this)?;
        let path = dispatch::composed_path(&event.path, event.current_target);
        let mut objects = Vec::with_capacity(path.len());
        for target in path {
            objects.push(target.object(cx.rt)?);
        }
        Ok(objects)
    }

    fn event_phase(cx: &mut Cx<'_>, this: Platform) -> b::Result<u16> {
        Ok(this_data(cx, this)?.phase)
    }

    fn stop_propagation(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        this_data_mut(cx, this)?.stop_propagation = true;
        Ok(())
    }

    fn cancel_bubble(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(this_data(cx, this)?.stop_propagation)
    }

    fn set_cancel_bubble(cx: &mut Cx<'_>, this: Platform, value: bool) -> b::Result<()> {
        if value {
            this_data_mut(cx, this)?.stop_propagation = true;
        }
        Ok(())
    }

    fn stop_immediate_propagation(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let event = this_data_mut(cx, this)?;
        event.stop_propagation = true;
        event.stop_immediate_propagation = true;
        Ok(())
    }

    fn bubbles(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(this_data(cx, this)?.bubbles)
    }

    fn cancelable(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(this_data(cx, this)?.cancelable)
    }

    fn return_value(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(!this_data(cx, this)?.canceled)
    }

    fn set_return_value(cx: &mut Cx<'_>, this: Platform, value: bool) -> b::Result<()> {
        if !value {
            this_data_mut(cx, this)?.set_canceled();
        }
        Ok(())
    }

    fn prevent_default(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        this_data_mut(cx, this)?.set_canceled();
        Ok(())
    }

    fn default_prevented(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(this_data(cx, this)?.canceled)
    }

    fn composed(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(this_data(cx, this)?.composed)
    }

    fn is_trusted(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(this_data(cx, this)?.is_trusted)
    }

    fn time_stamp(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        Ok(this_data(cx, this)?.time_stamp)
    }

    fn init_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
    ) -> b::Result<()> {
        let event = this_data_mut(cx, this)?;
        if !event.dispatching {
            event.initialize(kind, bubbles, cancelable);
        }
        Ok(())
    }
}

impl g::custom_event::CustomEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: CustomEventInit) -> b::Result<Gc<Object>> {
        let extra = Extra::Custom {
            detail: init.detail,
        };
        let event = base(init.bubbles, init.cancelable, init.composed);
        construct(cx, kind, &event, extra)
    }

    fn detail(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        match this_data(cx, this)?.extra {
            Extra::Custom { detail } => Ok(detail),
            _ => Ok(Value::Null),
        }
    }

    fn init_custom_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
        detail: Value,
    ) -> b::Result<()> {
        let event = this_data_mut(cx, this)?;
        if !event.dispatching {
            event.initialize(kind, bubbles, cancelable);
            event.extra = Extra::Custom { detail };
        }
        Ok(())
    }
}

impl g::error_event::ErrorEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: ErrorEventInit) -> b::Result<Gc<Object>> {
        let event = base(init.bubbles, init.cancelable, init.composed);
        let extra = Extra::Error {
            message: init.message,
            filename: init.filename,
            lineno: init.lineno,
            colno: init.colno,
            error: init.error.unwrap_or(Value::Undefined),
        };
        construct(cx, kind, &event, extra)
    }

    fn message(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match &this_data(cx, this)?.extra {
            Extra::Error { message, .. } => Ok(message.clone()),
            _ => Ok(String::new()),
        }
    }

    fn filename(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match &this_data(cx, this)?.extra {
            Extra::Error { filename, .. } => Ok(filename.clone()),
            _ => Ok(String::new()),
        }
    }

    fn lineno(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        match this_data(cx, this)?.extra {
            Extra::Error { lineno, .. } => Ok(lineno),
            _ => Ok(0),
        }
    }

    fn colno(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        match this_data(cx, this)?.extra {
            Extra::Error { colno, .. } => Ok(colno),
            _ => Ok(0),
        }
    }

    fn error(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        match this_data(cx, this)?.extra {
            Extra::Error { error, .. } => Ok(error),
            _ => Ok(Value::Undefined),
        }
    }
}

impl g::promise_rejection_event::PromiseRejectionEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: PromiseRejectionEventInit,
    ) -> b::Result<Gc<Object>> {
        let event = base(init.bubbles, init.cancelable, init.composed);
        let extra = Extra::PromiseRejection {
            promise: init.promise,
            reason: init.reason.unwrap_or(Value::Undefined),
        };
        construct(cx, kind, &event, extra)
    }

    fn promise(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        match this_data(cx, this)?.extra {
            Extra::PromiseRejection { promise, .. } => Ok(promise),
            _ => Err(VmError::type_error("Illegal invocation").into()),
        }
    }

    fn reason(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        match this_data(cx, this)?.extra {
            Extra::PromiseRejection { reason, .. } => Ok(reason),
            _ => Ok(Value::Undefined),
        }
    }
}

impl g::page_transition_event::PageTransitionEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: PageTransitionEventInit,
    ) -> b::Result<Gc<Object>> {
        let event = base(init.bubbles, init.cancelable, init.composed);
        let extra = Extra::PageTransition {
            persisted: init.persisted,
        };
        construct(cx, kind, &event, extra)
    }

    fn persisted(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        match this_data(cx, this)?.extra {
            Extra::PageTransition { persisted } => Ok(persisted),
            _ => Ok(false),
        }
    }
}

impl g::media_query_list_event::MediaQueryListEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: MediaQueryListEventInit,
    ) -> b::Result<Gc<Object>> {
        let event = base(init.bubbles, init.cancelable, init.composed);
        let extra = Extra::MediaQuery {
            media: init.media,
            matches: init.matches,
        };
        construct(cx, kind, &event, extra)
    }

    fn media(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match &this_data(cx, this)?.extra {
            Extra::MediaQuery { media, .. } => Ok(media.clone()),
            _ => Ok(String::new()),
        }
    }

    fn matches(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        match this_data(cx, this)?.extra {
            Extra::MediaQuery { matches, .. } => Ok(matches),
            _ => Ok(false),
        }
    }
}

/// `document.createEvent(interface)` (DOM §4.5,
/// <https://dom.spec.whatwg.org/#dom-document-createevent>): an
/// uninitialized event of the interface that the legacy name gives. The
/// names are those of the specification's table that swb has an interface
/// for, and `TransitionEvent`, which Chromium adds; `TouchEvent` throws, as
/// in Chromium without touch input.
pub(crate) fn create_event(
    cx: &mut Cx<'_>,
    _document: NodeId,
    interface: &str,
) -> b::Result<Gc<Object>> {
    use ui::Family;
    let name = interface.to_ascii_lowercase();
    let ui = |class, family| (class, ui::blank(family));
    let (class, extra) = match name.as_str() {
        "event" | "events" | "htmlevents" | "svgevents" => (g::EVENT, Extra::None),
        "customevent" => (
            g::CUSTOM_EVENT,
            Extra::Custom {
                detail: Value::Null,
            },
        ),
        "uievent" | "uievents" => ui(g::UI_EVENT, Family::Ui),
        "mouseevent" | "mouseevents" => ui(g::MOUSE_EVENT, Family::Mouse),
        "keyboardevent" => ui(g::KEYBOARD_EVENT, Family::Keyboard),
        "focusevent" => ui(g::FOCUS_EVENT, Family::Focus),
        "textevent" => ui(g::TEXT_EVENT, Family::Text),
        "compositionevent" => ui(g::COMPOSITION_EVENT, Family::Composition),
        "dragevent" => ui(g::DRAG_EVENT, Family::Drag),
        other => match misc::blank(other) {
            Some((class, other)) => (class, Extra::Other(Box::new(other))),
            None => {
                return Err(Exception::dom(
                    "NotSupportedError",
                    format!("The provided event type ('{interface}') is invalid."),
                ));
            }
        },
    };
    let mut event = EventData::new(String::new(), &base(false, false, false), now(cx.rt), extra);
    event.initialized = false;
    Ok(new_event_object(cx.rt, cx.realm(), class, event)?)
}
