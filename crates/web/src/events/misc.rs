//! The other event interfaces that `createEvent` names or that forms
//! fire: `SubmitEvent` (HTML §4.10.21.3), `HashChangeEvent`,
//! `MessageEvent`, `StorageEvent`, `BeforeUnloadEvent`,
//! `TransitionEvent`, `DeviceMotionEvent` and `DeviceOrientationEvent`.
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#submitevent>
//!
//! swb fires none of them except `submit`; scripts can construct them and
//! dispatch them. The state of each is a variant of [`Other`].

use swb_js::{Gc, Object, Tracer, Value, VmError};

use super::{Extra, base, construct, this_data, this_data_mut};
use crate::bindings::{self as b, Cx, NodeId, Platform};
use crate::generated as g;
use crate::generated::types::{
    DeviceMotionEventInit, DeviceOrientationEventInit, HashChangeEventInit, MessageEventInit,
    PopStateEventInit, StorageEventInit, SubmitEventInit, TransitionEventInit,
};
use crate::state::WebState;

/// The state of an event of one of the interfaces of this module.
#[derive(Debug)]
pub(crate) enum Other {
    Submit {
        submitter: Option<Gc<Object>>,
    },
    HashChange {
        old_url: String,
        new_url: String,
    },
    PopState {
        state: Value,
        has_ua_visual_transition: bool,
    },
    Message {
        data: Value,
        origin: String,
        last_event_id: String,
        source: Option<Gc<Object>>,
        ports: Vec<Gc<Object>>,
    },
    Storage {
        key: Option<String>,
        old_value: Option<String>,
        new_value: Option<String>,
        url: String,
        storage_area: Option<Gc<Object>>,
    },
    BeforeUnload {
        return_value: String,
    },
    Transition {
        property_name: String,
        elapsed_time: f64,
        pseudo_element: String,
    },
    DeviceMotion {
        interval: f64,
    },
    DeviceOrientation {
        alpha: Option<f64>,
        beta: Option<f64>,
        gamma: Option<f64>,
        absolute: bool,
    },
}

impl Other {
    /// The objects that the collector must see.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        match self {
            Other::Submit {
                submitter: Some(object),
            }
            | Other::Storage {
                storage_area: Some(object),
                ..
            } => tracer.object(*object),
            Other::PopState { state, .. } => tracer.value(*state),
            Other::Message {
                data,
                source,
                ports,
                ..
            } => {
                tracer.value(*data);
                for object in source.iter().chain(ports) {
                    tracer.object(*object);
                }
            }
            _ => {}
        }
    }

    /// The bytes of the strings.
    pub(crate) fn heap_size(&self) -> usize {
        match self {
            Other::HashChange { old_url, new_url } => old_url.capacity() + new_url.capacity(),
            Other::Message {
                origin,
                last_event_id,
                ports,
                ..
            } => origin.capacity() + last_event_id.capacity() + ports.capacity() * 8,
            Other::Storage { url, .. } => url.capacity(),
            Other::BeforeUnload { return_value } => return_value.capacity(),
            Other::Transition {
                property_name,
                pseudo_element,
                ..
            } => property_name.capacity() + pseudo_element.capacity(),
            _ => 0,
        }
    }
}

/// The state of the event `this`, or `Illegal invocation`.
fn other<'a>(cx: &'a Cx<'_>, this: Platform) -> b::Result<&'a Other> {
    match &this_data(cx, this)?.extra {
        Extra::Other(other) => Ok(other),
        _ => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// The state of the event `this`, for changes.
fn other_mut<'a>(cx: &'a mut Cx<'_>, this: Platform) -> b::Result<&'a mut Other> {
    match &mut this_data_mut(cx, this)?.extra {
        Extra::Other(other) => Ok(other),
        _ => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// A new event of a constructor.
fn make(
    cx: &mut Cx<'_>,
    kind: String,
    flags: (bool, bool, bool),
    other: Other,
) -> b::Result<Gc<Object>> {
    construct(
        cx,
        kind,
        &base(flags.0, flags.1, flags.2),
        Extra::Other(Box::new(other)),
    )
}

/// Applies DOM "initialize" for an `init*` method; false while the event
/// is dispatched.
fn init(
    cx: &mut Cx<'_>,
    this: Platform,
    kind: String,
    bubbles: bool,
    cancelable: bool,
) -> b::Result<bool> {
    super::ui::init_event(cx, this, kind, bubbles, cancelable)
}

impl g::submit_event::SubmitEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: SubmitEventInit) -> b::Result<Gc<Object>> {
        let submitter = match init.submitter {
            Some(node) => Some(<WebState as b::Host>::wrap_node(cx.rt, cx.realm(), node)?),
            None => None,
        };
        let flags = (init.bubbles, init.cancelable, init.composed);
        make(cx, kind, flags, Other::Submit { submitter })
    }

    fn submitter(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        match other(cx, this)? {
            Other::Submit {
                submitter: Some(object),
            } => Ok(b::node_of(
                cx.rt
                    .host_object((*object).into())
                    .map(|(_, data)| data)
                    .ok_or(VmError::type_error("Illegal invocation"))?,
            )),
            _ => Ok(None),
        }
    }
}

impl g::hash_change_event::HashChangeEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: HashChangeEventInit,
    ) -> b::Result<Gc<Object>> {
        let flags = (init.bubbles, init.cancelable, init.composed);
        let other = Other::HashChange {
            old_url: init.old_url,
            new_url: init.new_url,
        };
        make(cx, kind, flags, other)
    }

    fn old_url(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match other(cx, this)? {
            Other::HashChange { old_url, .. } => Ok(old_url.clone()),
            _ => Ok(String::new()),
        }
    }

    fn new_url(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match other(cx, this)? {
            Other::HashChange { new_url, .. } => Ok(new_url.clone()),
            _ => Ok(String::new()),
        }
    }
}

impl g::pop_state_event::PopStateEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: PopStateEventInit,
    ) -> b::Result<Gc<Object>> {
        let flags = (init.bubbles, init.cancelable, init.composed);
        let other = Other::PopState {
            state: init.state,
            has_ua_visual_transition: init.has_ua_visual_transition,
        };
        make(cx, kind, flags, other)
    }

    fn state(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        match other(cx, this)? {
            Other::PopState { state, .. } => Ok(*state),
            _ => Ok(Value::Null),
        }
    }

    fn has_ua_visual_transition(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        match other(cx, this)? {
            Other::PopState {
                has_ua_visual_transition,
                ..
            } => Ok(*has_ua_visual_transition),
            _ => Ok(false),
        }
    }
}

impl g::message_event::MessageEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: MessageEventInit) -> b::Result<Gc<Object>> {
        let flags = (init.bubbles, init.cancelable, init.composed);
        let other = Other::Message {
            data: init.data,
            origin: init.origin,
            last_event_id: init.last_event_id,
            source: init.source.map(|p| p.object),
            ports: init.ports,
        };
        make(cx, kind, flags, other)
    }

    fn data(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        match other(cx, this)? {
            Other::Message { data, .. } => Ok(*data),
            _ => Ok(Value::Null),
        }
    }

    fn origin(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match other(cx, this)? {
            Other::Message { origin, .. } => Ok(origin.clone()),
            _ => Ok(String::new()),
        }
    }

    fn last_event_id(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match other(cx, this)? {
            Other::Message { last_event_id, .. } => Ok(last_event_id.clone()),
            _ => Ok(String::new()),
        }
    }

    fn source(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        match other(cx, this)? {
            Other::Message { source, .. } => Ok(*source),
            _ => Ok(None),
        }
    }

    fn ports(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        match other(cx, this)? {
            Other::Message { ports, .. } => Ok(ports.clone()),
            _ => Ok(Vec::new()),
        }
    }

    fn init_message_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
        data: Value,
        origin: String,
        last_event_id: String,
        source: Option<Platform>,
        ports: Vec<Gc<Object>>,
    ) -> b::Result<()> {
        if init(cx, this, kind, bubbles, cancelable)? {
            *other_mut(cx, this)? = Other::Message {
                data,
                origin,
                last_event_id,
                source: source.map(|p| p.object),
                ports,
            };
        }
        Ok(())
    }
}

impl g::storage_event::StorageEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: StorageEventInit) -> b::Result<Gc<Object>> {
        let flags = (init.bubbles, init.cancelable, init.composed);
        let other = Other::Storage {
            key: init.key,
            old_value: init.old_value,
            new_value: init.new_value,
            url: init.url,
            storage_area: init.storage_area,
        };
        make(cx, kind, flags, other)
    }

    fn key(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        match other(cx, this)? {
            Other::Storage { key, .. } => Ok(key.clone()),
            _ => Ok(None),
        }
    }

    fn old_value(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        match other(cx, this)? {
            Other::Storage { old_value, .. } => Ok(old_value.clone()),
            _ => Ok(None),
        }
    }

    fn new_value(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        match other(cx, this)? {
            Other::Storage { new_value, .. } => Ok(new_value.clone()),
            _ => Ok(None),
        }
    }

    fn url(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match other(cx, this)? {
            Other::Storage { url, .. } => Ok(url.clone()),
            _ => Ok(String::new()),
        }
    }

    fn storage_area(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        match other(cx, this)? {
            Other::Storage { storage_area, .. } => Ok(*storage_area),
            _ => Ok(None),
        }
    }

    fn init_storage_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
        key: Option<String>,
        old_value: Option<String>,
        new_value: Option<String>,
        url: String,
        storage_area: Option<Gc<Object>>,
    ) -> b::Result<()> {
        if init(cx, this, kind, bubbles, cancelable)? {
            *other_mut(cx, this)? = Other::Storage {
                key,
                old_value,
                new_value,
                url,
                storage_area,
            };
        }
        Ok(())
    }
}

impl g::before_unload_event::BeforeUnloadEventImpl for WebState {
    fn return_value(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match other(cx, this)? {
            Other::BeforeUnload { return_value } => Ok(return_value.clone()),
            _ => Ok(String::new()),
        }
    }

    fn set_return_value(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        if let Other::BeforeUnload { return_value } = other_mut(cx, this)? {
            *return_value = value;
        }
        Ok(())
    }
}

impl g::transition_event::TransitionEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: TransitionEventInit,
    ) -> b::Result<Gc<Object>> {
        let flags = (init.bubbles, init.cancelable, init.composed);
        let other = Other::Transition {
            property_name: init.property_name,
            elapsed_time: init.elapsed_time,
            pseudo_element: init.pseudo_element,
        };
        make(cx, kind, flags, other)
    }

    fn property_name(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match other(cx, this)? {
            Other::Transition { property_name, .. } => Ok(property_name.clone()),
            _ => Ok(String::new()),
        }
    }

    fn elapsed_time(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        match other(cx, this)? {
            Other::Transition { elapsed_time, .. } => Ok(*elapsed_time),
            _ => Ok(0.0),
        }
    }

    fn pseudo_element(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        match other(cx, this)? {
            Other::Transition { pseudo_element, .. } => Ok(pseudo_element.clone()),
            _ => Ok(String::new()),
        }
    }
}

impl g::device_motion_event::DeviceMotionEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: DeviceMotionEventInit,
    ) -> b::Result<Gc<Object>> {
        let flags = (init.bubbles, init.cancelable, init.composed);
        make(
            cx,
            kind,
            flags,
            Other::DeviceMotion {
                interval: init.interval,
            },
        )
    }

    fn acceleration(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn acceleration_including_gravity(
        _cx: &mut Cx<'_>,
        _this: Platform,
    ) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn rotation_rate(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn interval(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        match other(cx, this)? {
            Other::DeviceMotion { interval } => Ok(*interval),
            _ => Ok(0.0),
        }
    }
}

impl g::device_orientation_event::DeviceOrientationEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: DeviceOrientationEventInit,
    ) -> b::Result<Gc<Object>> {
        let flags = (init.bubbles, init.cancelable, init.composed);
        let other = Other::DeviceOrientation {
            alpha: init.alpha,
            beta: init.beta,
            gamma: init.gamma,
            absolute: init.absolute,
        };
        make(cx, kind, flags, other)
    }

    fn alpha(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<f64>> {
        match other(cx, this)? {
            Other::DeviceOrientation { alpha, .. } => Ok(*alpha),
            _ => Ok(None),
        }
    }

    fn beta(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<f64>> {
        match other(cx, this)? {
            Other::DeviceOrientation { beta, .. } => Ok(*beta),
            _ => Ok(None),
        }
    }

    fn gamma(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<f64>> {
        match other(cx, this)? {
            Other::DeviceOrientation { gamma, .. } => Ok(*gamma),
            _ => Ok(None),
        }
    }

    fn absolute(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        match other(cx, this)? {
            Other::DeviceOrientation { absolute, .. } => Ok(*absolute),
            _ => Ok(false),
        }
    }
}

/// The blank state of a `createEvent` interface of this module.
pub(crate) fn blank(name: &str) -> Option<(swb_js::ClassId, Other)> {
    Some(match name {
        "hashchangeevent" => (
            g::HASH_CHANGE_EVENT,
            Other::HashChange {
                old_url: String::new(),
                new_url: String::new(),
            },
        ),
        "messageevent" => (
            g::MESSAGE_EVENT,
            Other::Message {
                data: Value::Null,
                origin: String::new(),
                last_event_id: String::new(),
                source: None,
                ports: Vec::new(),
            },
        ),
        "storageevent" => (
            g::STORAGE_EVENT,
            Other::Storage {
                key: None,
                old_value: None,
                new_value: None,
                url: String::new(),
                storage_area: None,
            },
        ),
        "beforeunloadevent" => (
            g::BEFORE_UNLOAD_EVENT,
            Other::BeforeUnload {
                return_value: String::new(),
            },
        ),
        "transitionevent" => (
            g::TRANSITION_EVENT,
            Other::Transition {
                property_name: String::new(),
                elapsed_time: 0.0,
                pseudo_element: String::new(),
            },
        ),
        "devicemotionevent" => (
            g::DEVICE_MOTION_EVENT,
            Other::DeviceMotion { interval: 0.0 },
        ),
        "deviceorientationevent" => (
            g::DEVICE_ORIENTATION_EVENT,
            Other::DeviceOrientation {
                alpha: None,
                beta: None,
                gamma: None,
                absolute: false,
            },
        ),
        _ => return None,
    })
}
