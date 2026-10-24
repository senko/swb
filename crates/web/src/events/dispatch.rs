//! The dispatch algorithm (DOM §2.9, <https://dom.spec.whatwg.org/#concept-event-dispatch>,
//! and §2.10, "firing events"), `composedPath` (DOM §2.2), and the entry
//! for events that the engine fires ([`dispatch_trusted`]).
//!
//! Without shadow trees, the path is the target, its ancestors and, for a
//! node of the window's document (DOM "get the parent" of a document)
//! and an event other than `load`, the window. Listeners are called with
//! [`Runtime::call_function`], so that a call from the engine gets the
//! semantics of an outermost run and a call from `dispatchEvent` charges
//! the recursion budget; an exception of a listener is reported (HTML
//! "report the exception": a console line until feature 5b fires `error`
//! at the window) and the dispatch goes on. A termination ends the
//! dispatch after its cleanup and goes up.
//!
//! Not implemented: the legacy `webkit` renames of trusted animation
//! events without listeners (DOM "invoke", step 9; swb fires no animation
//! events), activation behaviour (feature 6d), `relatedTarget` and touch
//! targets (no shadow trees, no touch events).

use std::rc::Rc;

use swb_dom::NodeId;
use swb_js::{ClassId, Gc, Generic, Object, Runtime, ScriptError, Value, VmError, VmResult};

use super::listeners::Listener;
use super::target::{TargetKind, kind_of, listeners_of};
use super::{AT_TARGET, BUBBLING_PHASE, CAPTURING_PHASE, EventData, Extra, NONE, PathItem};
use crate::bindings::{self as b, Host};
use crate::generated as g;
use crate::generated::types::EventInit;
use crate::state::{WebState, dom, state, state_mut};

/// Where the engine dispatches an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchTarget {
    /// A node of the document.
    Node(NodeId),
    /// The window.
    Window,
}

/// The interface and the attributes of an event that the engine fires.
#[derive(Clone, Debug, PartialEq)]
pub enum EventInterface {
    /// `Event`.
    Event,
    /// `PageTransitionEvent` (`pageshow`, `pagehide`).
    PageTransition {
        /// `persisted`.
        persisted: bool,
    },
}

/// An event that the engine fires (DOM "fire an event"): `isTrusted` is
/// true.
#[derive(Clone, Debug, PartialEq)]
pub struct TrustedEvent {
    /// The type (`load`, `click`, ...).
    pub kind: String,
    /// `bubbles`.
    pub bubbles: bool,
    /// `cancelable`.
    pub cancelable: bool,
    /// `composed`.
    pub composed: bool,
    /// The interface.
    pub interface: EventInterface,
    /// The legacy target override flag of DOM "dispatch": for an event at
    /// the window, the target is the document (HTML fires `load` at the
    /// window this way).
    pub legacy_target_override: bool,
}

impl TrustedEvent {
    /// A trusted `Event` of `kind` that neither bubbles nor can be
    /// canceled.
    pub fn new(kind: &str) -> TrustedEvent {
        TrustedEvent {
            kind: kind.to_owned(),
            bubbles: false,
            cancelable: false,
            composed: false,
            interface: EventInterface::Event,
            legacy_target_override: false,
        }
    }
}

/// A new event object of `class` with `data` and the `[LegacyUnforgeable]`
/// `isTrusted` (for `createEvent` and the engine's events).
pub(crate) fn new_event_object(
    rt: &mut Runtime,
    realm: u32,
    class: ClassId,
    data: EventData,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(data)?;
    let object = rt.new_host_object(realm, class, data)?;
    b::glue::install_unforgeables::<WebState>(rt, realm, class, object)?;
    Ok(object)
}

/// Fires a trusted event at `target` in realm 0 (DOM "fire an event"; the
/// caller runs it as a task). Returns false if the event was canceled.
/// Call it with an open handle scope.
pub(crate) fn dispatch_trusted(
    rt: &mut Runtime,
    target: DispatchTarget,
    event: &TrustedEvent,
) -> VmResult<bool> {
    let init = EventInit {
        bubbles: event.bubbles,
        cancelable: event.cancelable,
        composed: event.composed,
    };
    let (class, extra) = match event.interface {
        EventInterface::Event => (g::EVENT, Extra::None),
        EventInterface::PageTransition { persisted } => (
            g::PAGE_TRANSITION_EVENT,
            Extra::PageTransition { persisted },
        ),
    };
    let mut data = EventData::new(event.kind.clone(), &init, super::now(rt), extra);
    data.is_trusted = true;
    let object = new_event_object(rt, 0, class, data)?;
    let target_object = match target {
        DispatchTarget::Node(node) => WebState::wrap_node(rt, 0, node)?,
        DispatchTarget::Window => rt.global(0)?,
    };
    let target_override = if event.legacy_target_override && target == DispatchTarget::Window {
        Some(WebState::wrap_node(rt, 0, NodeId::DOCUMENT)?)
    } else {
        None
    };
    dispatch(rt, 0, target_object, object, target_override)
}

/// DOM "dispatch" of `event` to `target`, with the legacy target override
/// (the target object that listeners see instead of `target`). The caller
/// checked the dispatch and initialized flags. Returns false if the event
/// was canceled.
pub(crate) fn dispatch(
    rt: &mut Runtime,
    realm: u32,
    target: Gc<Object>,
    event: Gc<Object>,
    target_override: Option<Gc<Object>>,
) -> VmResult<bool> {
    let (_, data) = rt
        .host_object(event.into())
        .ok_or(VmError::type_error("Illegal invocation"))?;
    let handle = super::event_handle(data)?;
    super::data_mut(rt, handle)?.dispatching = true;
    let result = build_path(rt, realm, target, handle, target_override)
        .and_then(|()| run_phases(rt, handle, event));
    // Steps 7 to 10, also after a termination.
    let data = super::data_mut(rt, handle)?;
    data.phase = NONE;
    data.current_target = None;
    data.path = Vec::new();
    data.dispatching = false;
    data.stop_propagation = false;
    data.stop_immediate_propagation = false;
    let canceled = data.canceled;
    result.map(|()| !canceled)
}

/// Steps 5.3 to 5.9 of "dispatch": the event path, stored in the event.
fn build_path(
    rt: &mut Runtime,
    realm: u32,
    target: Gc<Object>,
    event: Gc<Generic>,
    target_override: Option<Gc<Object>>,
) -> VmResult<()> {
    let kind = kind_of(rt, target)?;
    let mut path = vec![PathItem {
        target,
        in_shadow_tree: false,
        shadow_adjusted_target: Some(target_override.unwrap_or(target)),
        root_of_closed_tree: false,
        slot_in_closed_tree: false,
    }];
    let mut parents = Vec::new();
    let mut window = false;
    if let TargetKind::Node(node) = kind {
        // Copied out of the DOM before the wrappers are made.
        let dom = dom(rt)?;
        parents.extend(dom.ancestors(node));
        let root = parents.last().copied().unwrap_or(node);
        // "Get the parent" of a document: the window, unless the event is
        // `load` or the document has no browsing context (only the
        // document of the window has one).
        window = root == NodeId::DOCUMENT && super::data(rt, event)?.kind != "load";
    }
    for node in parents {
        let wrapper = WebState::wrap_node(rt, realm, node)?;
        path.push(item(wrapper));
    }
    if window {
        path.push(item(rt.global(realm)?));
    }
    super::data_mut(rt, event)?.path = path;
    Ok(())
}

/// A struct of the path for a parent (no shadow-adjusted target).
fn item(target: Gc<Object>) -> PathItem {
    PathItem {
        target,
        in_shadow_tree: false,
        shadow_adjusted_target: None,
        root_of_closed_tree: false,
        slot_in_closed_tree: false,
    }
}

/// Steps 5.13 and 5.14 of "dispatch": the capture pass from the end of
/// the path, then the bubble pass from its start.
fn run_phases(rt: &mut Runtime, handle: Gc<Generic>, event: Gc<Object>) -> VmResult<()> {
    // Step 1 of "invoke" for each struct: the shadow-adjusted target of
    // the struct or of the nearest preceding one (found once, not by a
    // scan per struct). The path holds the objects for the dispatch.
    let mut last = None;
    let targets: Vec<Option<Gc<Object>>> = super::data(rt, handle)?
        .path
        .iter()
        .map(|item| {
            last = item.shadow_adjusted_target.or(last);
            last
        })
        .collect();
    let target = |index: usize| targets.get(index).copied().flatten();
    let len = targets.len();
    for index in (0..len).rev() {
        let at_target = path_item(rt, handle, index)?
            .shadow_adjusted_target
            .is_some();
        super::data_mut(rt, handle)?.phase = if at_target {
            AT_TARGET
        } else {
            CAPTURING_PHASE
        };
        invoke(rt, handle, event, index, target(index), Phase::Capturing)?;
    }
    for index in 0..len {
        let at_target = path_item(rt, handle, index)?
            .shadow_adjusted_target
            .is_some();
        let data = super::data_mut(rt, handle)?;
        if at_target {
            data.phase = AT_TARGET;
        } else {
            if !data.bubbles {
                continue;
            }
            data.phase = BUBBLING_PHASE;
        }
        invoke(rt, handle, event, index, target(index), Phase::Bubbling)?;
    }
    Ok(())
}

/// A struct of the event's path.
fn path_item(rt: &Runtime, handle: Gc<Generic>, index: usize) -> VmResult<PathItem> {
    super::data(rt, handle)?
        .path
        .get(index)
        .copied()
        .ok_or(VmError::type_error(
            "the event path changed during the dispatch",
        ))
}

/// The pass of "invoke".
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Capturing,
    Bubbling,
}

/// DOM "invoke" for the struct at `index` of the path; `target` is the
/// result of its step 1.
fn invoke(
    rt: &mut Runtime,
    handle: Gc<Generic>,
    event: Gc<Object>,
    index: usize,
    target: Option<Gc<Object>>,
    phase: Phase,
) -> VmResult<()> {
    let item = path_item(rt, handle, index)?;
    let data = super::data_mut(rt, handle)?;
    data.target = target;
    if data.stop_propagation {
        return Ok(());
    }
    data.current_target = Some(item.target);
    let kind = data.kind.clone();
    let target_kind = kind_of(rt, item.target)?;
    let listeners = listeners_of(rt, target_kind, &kind)?;
    inner_invoke(
        rt,
        handle,
        event,
        &listeners,
        phase,
        item,
        target_kind,
        &kind,
    )
}

/// DOM "inner invoke".
#[allow(
    clippy::too_many_arguments,
    reason = "the arguments of the specification's steps"
)]
fn inner_invoke(
    rt: &mut Runtime,
    handle: Gc<Generic>,
    event: Gc<Object>,
    listeners: &[Rc<Listener>],
    phase: Phase,
    item: PathItem,
    target: TargetKind,
    kind: &str,
) -> VmResult<()> {
    for listener in listeners {
        if listener.removed.get()
            || (phase == Phase::Capturing && !listener.capture)
            || (phase == Phase::Bubbling && listener.capture)
        {
            continue;
        }
        if listener.once {
            super::target::remove(rt, target, |list| list.remove_listener(kind, listener))?;
        }
        // Steps 6 to 9: the window's current event, the passive flag.
        let current_event = state(rt)?.current_event;
        if !item.in_shadow_tree {
            state_mut(rt)?.current_event = Some(event);
        }
        super::data_mut(rt, handle)?.in_passive_listener = listener.passive;
        b::keep(rt, Value::Object(listener.callback));
        let result = call_listener(rt, listener.callback, item.target, event, kind);
        super::data_mut(rt, handle)?.in_passive_listener = false;
        state_mut(rt)?.current_event = current_event;
        result?;
        if super::data(rt, handle)?.stop_immediate_propagation {
            break;
        }
    }
    Ok(())
}

/// Step 11 of "inner invoke": "call a user object's operation" (Web IDL
/// §3.11) with `handleEvent`, `this` = the current target for a function.
/// An exception is reported and the call returns `Ok`; a termination or an
/// engine error goes up.
fn call_listener(
    rt: &mut Runtime,
    callback: Gc<Object>,
    current_target: Gc<Object>,
    event: Gc<Object>,
    kind: &str,
) -> VmResult<()> {
    let args = [Value::Object(event)];
    let result = if rt.callable(callback.into())? {
        rt.call_function(callback.into(), current_target.into(), &args)
    } else {
        let key = rt.key_from_str("handleEvent")?;
        let method = match rt.get_key(callback.into(), key) {
            Ok(method) => method,
            Err(error) => return report_vm_error(rt, kind, error),
        };
        if !rt.callable(method)? {
            let error = VmError::type_error("The provided callback is not callable.");
            return report_vm_error(rt, kind, error);
        }
        rt.call_function(method, callback.into(), &args)
    };
    match result {
        Ok(_) => Ok(()),
        Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
        Err(error) => {
            crate::runtime::report(rt, &listener_name(kind), &error);
            Ok(())
        }
    }
}

/// Reports an exception that is not of a call (the lookup of
/// `handleEvent`); a termination or an engine error goes up.
fn report_vm_error(rt: &mut Runtime, kind: &str, error: VmError) -> VmResult<()> {
    let name = listener_name(kind);
    match error {
        VmError::Throw(value) => {
            b::keep(rt, value);
            crate::runtime::report_thrown(rt, &name, value);
        }
        VmError::Raise { kind, message } => {
            let error = ScriptError::Uncaught {
                name: kind.name().to_owned(),
                message: message.into_owned(),
                offset: None,
            };
            crate::runtime::report(rt, &name, &error);
        }
        other => return Err(other),
    }
    Ok(())
}

/// The name of a listener in the log.
fn listener_name(kind: &str) -> String {
    format!("a listener of '{kind}'")
}

/// `composedPath()` (DOM §2.2) of a path with the current target.
pub(crate) fn composed_path(
    path: &[PathItem],
    current_target: Option<Gc<Object>>,
) -> Vec<Gc<Object>> {
    let Some(current_target) = current_target else {
        return Vec::new();
    };
    if path.is_empty() {
        return Vec::new();
    }
    let mut current_index = 0;
    let mut current_hidden_level = 0_i32;
    for (index, item) in path.iter().enumerate().rev() {
        if item.root_of_closed_tree {
            current_hidden_level += 1;
        }
        if item.target == current_target {
            current_index = index;
            break;
        }
        if item.slot_in_closed_tree {
            current_hidden_level -= 1;
        }
    }
    let mut before = Vec::new();
    let (mut level, mut max_level) = (current_hidden_level, current_hidden_level);
    for item in path[..current_index].iter().rev() {
        if item.root_of_closed_tree {
            level += 1;
        }
        if level <= max_level {
            before.push(item.target);
        }
        if item.slot_in_closed_tree {
            level -= 1;
            max_level = max_level.min(level);
        }
    }
    before.reverse();
    let mut composed = before;
    composed.push(current_target);
    let (mut level, mut max_level) = (current_hidden_level, current_hidden_level);
    for item in path.iter().skip(current_index + 1) {
        if item.slot_in_closed_tree {
            level += 1;
        }
        if level <= max_level {
            composed.push(item.target);
        }
        if item.root_of_closed_tree {
            level -= 1;
            max_level = max_level.min(level);
        }
    }
    composed
}
