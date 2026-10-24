//! `Event` and `EventTarget` until feature 5a (DOM §2,
//! <https://dom.spec.whatwg.org/#events>): an event keeps what its
//! constructor got and its canceled flag; `addEventListener` and
//! `removeEventListener` accept their arguments and register nothing, and
//! `dispatchEvent` calls no listener. Feature 5a replaces this file with
//! the event core (listener tables, the dispatch algorithm).
//!
//! The stand-ins let page scripts that register listeners run to the end;
//! the listeners never run in 4c2.

use swb_js::{Gc, GenericData, HostData, Object, Tracer, VmError};

use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::generated::types::{
    AddEventListenerOptionsOrBoolean, EventInit, EventListener, EventListenerOptionsOrBoolean,
};
use crate::state::WebState;

/// The data of an `Event`.
struct EventData {
    kind: String,
    init: EventInit,
    canceled: bool,
}

impl GenericData for EventData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.kind.capacity()
    }
}

/// The data of an `EventTarget` made by its constructor.
struct TargetData;

impl GenericData for TargetData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// The data of an event object.
fn event<'a>(cx: &'a Cx<'_>, this: Platform) -> b::Result<&'a EventData> {
    let HostData::Generic(data) = this.data else {
        return Err(VmError::type_error("Illegal invocation").into());
    };
    Ok(cx
        .rt
        .heap()
        .generic::<EventData>(data)
        .map_err(VmError::from)?)
}

/// The data of an event object, for changes.
fn event_mut<'a>(cx: &'a mut Cx<'_>, this: Platform) -> b::Result<&'a mut EventData> {
    let HostData::Generic(data) = this.data else {
        return Err(VmError::type_error("Illegal invocation").into());
    };
    Ok(cx
        .rt
        .heap_mut()
        .generic_mut::<EventData>(data)
        .map_err(VmError::from)?)
}

impl g::event_target::EventTargetImpl for WebState {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(TargetData)?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn add_event_listener(
        _cx: &mut Cx<'_>,
        _this: Platform,
        kind: String,
        _callback: Option<EventListener>,
        _options: AddEventListenerOptionsOrBoolean,
    ) -> b::Result<()> {
        log::debug!("addEventListener('{kind}'): listeners come with feature 5a");
        Ok(())
    }

    fn remove_event_listener(
        _cx: &mut Cx<'_>,
        _this: Platform,
        _kind: String,
        _callback: Option<EventListener>,
        _options: EventListenerOptionsOrBoolean,
    ) -> b::Result<()> {
        Ok(())
    }

    fn dispatch_event(cx: &mut Cx<'_>, _this: Platform, event: Platform) -> b::Result<bool> {
        Ok(!self::event(cx, event)?.canceled)
    }
}

impl g::event::EventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: EventInit) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(EventData {
            kind,
            init,
            canceled: false,
        })?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn type_(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(event(cx, this)?.kind.clone())
    }

    fn target(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn src_element(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn current_target(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn composed_path(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Vec<Gc<Object>>> {
        Ok(Vec::new())
    }

    fn event_phase(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u16> {
        Ok(0)
    }

    fn stop_propagation(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }

    fn cancel_bubble(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }

    fn set_cancel_bubble(_cx: &mut Cx<'_>, _this: Platform, _value: bool) -> b::Result<()> {
        Ok(())
    }

    fn stop_immediate_propagation(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }

    fn bubbles(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(event(cx, this)?.init.bubbles)
    }

    fn cancelable(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(event(cx, this)?.init.cancelable)
    }

    fn return_value(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(!event(cx, this)?.canceled)
    }

    fn set_return_value(cx: &mut Cx<'_>, this: Platform, value: bool) -> b::Result<()> {
        if !value {
            Self::prevent_default(cx, this)?;
        }
        Ok(())
    }

    fn prevent_default(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let event = event_mut(cx, this)?;
        if event.init.cancelable {
            event.canceled = true;
        }
        Ok(())
    }

    fn default_prevented(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(event(cx, this)?.canceled)
    }

    fn composed(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(event(cx, this)?.init.composed)
    }

    fn is_trusted(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }

    fn time_stamp(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(0.0)
    }

    fn init_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
    ) -> b::Result<()> {
        let event = event_mut(cx, this)?;
        event.kind = kind;
        event.init.bubbles = bubbles;
        event.init.cancelable = cancelable;
        event.canceled = false;
        Ok(())
    }
}
