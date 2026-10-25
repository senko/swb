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
//! "report an exception": an `error` event at the window, `errors.rs`)
//! and the dispatch goes on. After each listener of an event that the
//! engine fires, a microtask checkpoint runs (`event_loop.rs`). A
//! termination ends the dispatch after its cleanup and goes up.
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
use super::user::UserEvent;
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
    /// `FocusEvent` (`focus`, `blur`, `focusin`, `focusout`), with the
    /// window as `view`.
    Focus {
        /// `relatedTarget`.
        related_target: Option<NodeId>,
    },
    /// `SubmitEvent` (`submit`).
    Submit {
        /// `submitter`.
        submitter: Option<NodeId>,
    },
    /// An event of the user's input: a mouse, pointer, wheel, keyboard or
    /// input event ([`UserEvent`]).
    User(Box<UserEvent>),
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
    let target_object = match target {
        DispatchTarget::Node(node) => WebState::wrap_node(rt, 0, node)?,
        DispatchTarget::Window => rt.global(0)?,
    };
    let mut init = EventInit {
        bubbles: event.bubbles,
        cancelable: event.cancelable,
        composed: event.composed,
    };
    let (class, extra) = match &event.interface {
        EventInterface::Event => (g::EVENT, Extra::None),
        EventInterface::PageTransition { persisted } => (
            g::PAGE_TRANSITION_EVENT,
            Extra::PageTransition {
                persisted: *persisted,
            },
        ),
        EventInterface::Focus { related_target } => {
            let mut fields = super::ui::Fields::new(super::ui::Family::Focus);
            fields.view = Some(rt.global(0)?);
            fields.related_target = match related_target {
                Some(node) => Some(WebState::wrap_node(rt, 0, *node)?),
                None => None,
            };
            (g::FOCUS_EVENT, Extra::Ui(Box::new(fields)))
        }
        EventInterface::Submit { submitter } => {
            let submitter = match submitter {
                Some(node) => Some(WebState::wrap_node(rt, 0, *node)?),
                None => None,
            };
            (
                g::SUBMIT_EVENT,
                Extra::Other(Box::new(super::misc::Other::Submit { submitter })),
            )
        }
        EventInterface::User(user) => {
            // Chromium makes a `wheel` event cancelable only when a
            // listener on its path can cancel it.
            if user.kind == super::user::UserKind::Wheel && init.cancelable {
                let kind = kind_of(rt, target_object)?;
                init.cancelable = super::user::has_blocking_listener(rt, kind, &event.kind)?;
            }
            let (class, fields) = super::user::fields_of(rt, user)?;
            (class, Extra::Ui(Box::new(fields)))
        }
    };
    let mut data = EventData::new(event.kind.clone(), &init, super::now(rt), extra);
    data.is_trusted = true;
    let object = new_event_object(rt, 0, class, data)?;
    let target_override = if event.legacy_target_override && target == DispatchTarget::Window {
        Some(WebState::wrap_node(rt, 0, NodeId::DOCUMENT)?)
    } else {
        None
    };
    dispatch(rt, 0, target_object, object, target_override)
}

/// [`dispatch_trusted`] in a handle scope of its own, for a native
/// function that fires an event while script runs (`focus()`,
/// `form.reset()`). Returns false if the event was canceled.
pub(crate) fn fire_trusted(
    rt: &mut Runtime,
    target: DispatchTarget,
    event: &TrustedEvent,
) -> VmResult<bool> {
    let scope = rt.heap_mut().open_scope();
    let result = dispatch_trusted(rt, target, event);
    if let Err(error) = rt.heap_mut().close_scope(scope) {
        log::warn!("the handle scope of an event was not the last one: {error:?}");
    }
    result
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
    // Steps 5.4 to 5.10 of "dispatch": the activation target and its
    // legacy-pre-activation behavior (HTML activation behaviors).
    let activation = activation_before(rt, target, handle);
    let result = activation
        .and_then(|activation| {
            build_path(rt, realm, target, handle, target_override)?;
            run_phases(rt, handle, event)?;
            Ok(activation)
        })
        .and_then(|activation| {
            // Steps 5.15 and 5.16, after the path is done.
            super::data_mut(rt, handle)?.dispatching = false;
            let (canceled, origin, offset, mods) = {
                let data = super::data(rt, handle)?;
                let (detail, offset, mods) = match &data.extra {
                    Extra::Ui(fields) => (fields.detail, fields.mouse.offset, fields.mods),
                    _ => (0, (0.0, 0.0), 0),
                };
                // The point of the click relative to the target, for an
                // image button (HTML "form submission", `x` and `y`).
                let coordinate = (
                    offset.0.floor().max(0.0) as i32,
                    offset.1.floor().max(0.0) as i32,
                );
                (data.canceled, (data.is_trusted, detail), coordinate, mods)
            };
            match activation {
                Some(activation) => crate::html::activation::after_dispatch(
                    rt, activation, canceled, origin, offset, mods,
                ),
                None => Ok(()),
            }
        });
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

/// The activation target of a `click` `MouseEvent` and its
/// legacy-pre-activation behavior (DOM §2.9, "dispatch", steps 5.4 to
/// 5.10): the target if it has an activation behavior, else, if the event
/// bubbles, the nearest ancestor that has one. `None` for other events.
fn activation_before(
    rt: &mut Runtime,
    target: Gc<Object>,
    handle: Gc<Generic>,
) -> VmResult<Option<crate::html::activation::Activation>> {
    let event = super::data(rt, handle)?;
    let is_click =
        event.kind == "click" && matches!(&event.extra, Extra::Ui(f) if f.family.is_mouse());
    if !is_click {
        return Ok(None);
    }
    let bubbles = event.bubbles;
    let TargetKind::Node(node) = kind_of(rt, target)? else {
        return Ok(None);
    };
    match crate::html::activation::find_target(rt, node, bubbles)? {
        Some(activation) => {
            let activation = crate::html::activation::pre_activation(rt, activation)?;
            Ok(Some(activation))
        }
        None => Ok(None),
    }
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
    super::handlers::sync(rt, target_kind)?;
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
        let result = match listener.handler {
            Some(name) => super::handlers::call(rt, target, name, event, handle, item.target),
            None => call_listener(rt, listener.callback, item.target, event, kind),
        };
        super::data_mut(rt, handle)?.in_passive_listener = false;
        state_mut(rt)?.current_event = current_event;
        result?;
        // HTML "clean up after running a callback" (for the engine's
        // events; a no-op while script runs).
        crate::event_loop::after_listener(rt)?;
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
            let name = listener_name(kind);
            crate::errors::report(rt, &crate::errors::Site::named(&name), &error)
        }
    }
}

/// Reports an exception that is not of a call (the lookup of
/// `handleEvent`) to the console only: Chromium fires no `error` event for
/// it (measured; HTML would). A termination or an engine error goes up.
fn report_vm_error(rt: &mut Runtime, kind: &str, error: VmError) -> VmResult<()> {
    let value = match error {
        VmError::Throw(value) => value,
        VmError::Raise { kind, message } => Value::Object(rt.new_error(kind, &message)?),
        other => return Err(other),
    };
    b::keep(rt, value);
    let text = format!("Uncaught {}", crate::runtime::thrown_value_text(rt, value));
    crate::runtime::report_line(rt, &listener_name(kind), &text);
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
