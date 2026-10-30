//! `AbortController` and `AbortSignal` (DOM §3.1,
//! <https://dom.spec.whatwg.org/#aborting-ongoing-activities>).
//!
//! An `AbortSignal` is an event target: its generic data is the
//! [`TargetData`] of constructed event targets with a [`SignalState`]. The
//! state keeps the abort reason, the dependent signals of `AbortSignal.any()`
//! and what the signal aborts: the `fetch()` calls that were given the
//! signal (by id, `fetch.rs`) and the event listeners that were added with
//! it (the `signal` option of `addEventListener`).
//!
//! Deviations: the dependent signals of a source are a strong list (the
//! standard keeps them weak), so a source with many `any()` signals keeps
//! them alive; a source holds at most [`MAX_DEPENDENTS`], and a signal at
//! most [`MAX_ABORT_LISTENERS`] listener registrations (further ones are
//! not tracked, with a warning). `AbortSignal.timeout()` signals live until
//! their timer task runs; at most [`MAX_TIMEOUT_SIGNALS`] wait at a time.

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value, VmError, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::event_loop::Task;
use crate::events::{EventData, Extra, TargetData};
use crate::generated as g;
use crate::generated::types::EventInit;
use crate::state::{WebState, state_mut};

/// The most dependent signals (`AbortSignal.any()`) that one source signal
/// keeps.
pub(crate) const MAX_DEPENDENTS: usize = 1 << 16;

/// The most listener registrations (`addEventListener` with `signal`) that
/// one signal tracks.
pub(crate) const MAX_ABORT_LISTENERS: usize = 1 << 16;

/// The most `AbortSignal.timeout()` signals that wait for their timer.
pub(crate) const MAX_TIMEOUT_SIGNALS: usize = 1 << 18;

/// A listener that goes away when the signal aborts.
#[derive(Debug)]
struct ListenerRef {
    target: Gc<Object>,
    kind: String,
    callback: Gc<Object>,
    capture: bool,
}

/// The state of an `AbortSignal` (DOM §3.1.2).
#[derive(Debug)]
pub(crate) struct SignalState {
    aborted: bool,
    /// The abort reason; `undefined` until the signal aborts.
    reason: Value,
    /// For a signal made by `any()`: its non-dependent source signals.
    sources: Vec<Gc<Object>>,
    /// The signals made by `any()` that follow this signal.
    dependents: Vec<Gc<Object>>,
    /// The ids of the `fetch()` calls that this signal aborts.
    fetches: Vec<u64>,
    /// The listeners that this signal removes.
    listeners: Vec<ListenerRef>,
}

impl SignalState {
    fn new() -> SignalState {
        SignalState {
            aborted: false,
            reason: Value::Undefined,
            sources: Vec::new(),
            dependents: Vec::new(),
            fetches: Vec::new(),
            listeners: Vec::new(),
        }
    }

    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.value(self.reason);
        for object in self.sources.iter().chain(&self.dependents) {
            tracer.object(*object);
        }
        for listener in &self.listeners {
            tracer.object(listener.target);
            tracer.object(listener.callback);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        64 + (self.sources.len() + self.dependents.len() + self.fetches.len()) * 8
            + self
                .listeners
                .iter()
                .map(|l| 24 + l.kind.capacity() + 16)
                .sum::<usize>()
    }
}

/// The data of an `AbortController`: its signal.
struct ControllerData {
    signal: Gc<Object>,
}

impl GenericData for ControllerData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.signal);
    }

    fn heap_size(&self) -> usize {
        16
    }
}

/// The state of the signal object `signal`.
fn signal_state(rt: &mut Runtime, signal: Gc<Object>) -> VmResult<&mut SignalState> {
    let illegal = || VmError::type_error("Illegal invocation");
    let Some((_, HostData::Generic(handle))) = rt.host_object(signal.into()) else {
        return Err(illegal());
    };
    rt.heap_mut()
        .generic_mut::<TargetData>(handle)
        .map_err(|_| illegal())?
        .signal
        .as_deref_mut()
        .ok_or_else(illegal)
}

/// A new, not aborted `AbortSignal`.
fn new_signal(rt: &mut Runtime, realm: u32) -> VmResult<Gc<Object>> {
    let mut data = TargetData::default();
    data.signal = Some(Box::new(SignalState::new()));
    let data = rt.alloc_host_data(data)?;
    rt.new_host_object(realm, g::ABORT_SIGNAL, data)
}

/// `AbortSignal.any(inputs)` (DOM "create a dependent abort signal"): a new
/// signal that aborts when one of `inputs` does.
pub(crate) fn any_of(rt: &mut Runtime, realm: u32, inputs: &[Gc<Object>]) -> VmResult<Gc<Object>> {
    let result = new_signal(rt, realm)?;
    // An input that is aborted makes the result aborted with its reason.
    for input in inputs {
        if is_aborted(rt, *input)? {
            let reason = reason_of(rt, *input)?;
            let state = signal_state(rt, result)?;
            state.aborted = true;
            state.reason = reason;
            return Ok(result);
        }
    }
    // The sources are the non-dependent signals (a dependent input stands
    // for its own sources).
    let mut sources: Vec<Gc<Object>> = Vec::new();
    // `contains` on the list would make this quadratic (up to 65,536
    // sources).
    let mut seen: std::collections::HashSet<Gc<Object>> = std::collections::HashSet::new();
    for input in inputs {
        let own = signal_state(rt, *input)?.sources.clone();
        let list = if own.is_empty() { vec![*input] } else { own };
        for source in list {
            if seen.insert(source) {
                sources.push(source);
            }
        }
        if sources.len() > MAX_DEPENDENTS {
            break;
        }
    }
    for source in &sources {
        let state = signal_state(rt, *source)?;
        if state.dependents.len() >= MAX_DEPENDENTS {
            log::warn!(
                "an AbortSignal keeps at most {MAX_DEPENDENTS} signals of AbortSignal.any(); a further one does not follow it"
            );
            continue;
        }
        state.dependents.push(result);
    }
    signal_state(rt, result)?.sources = sources;
    Ok(result)
}

/// A new signal that follows `source` (DOM "follow"): the signal of a
/// `Request`. Without a source it never aborts.
pub(crate) fn follow(
    rt: &mut Runtime,
    realm: u32,
    source: Option<Gc<Object>>,
) -> VmResult<Gc<Object>> {
    match source {
        Some(source) => any_of(rt, realm, &[source]),
        None => new_signal(rt, realm),
    }
}

/// `source`, or a new signal that never aborts (the signal of a
/// `fetch()` that was given none).
pub(crate) fn source_or_new(
    rt: &mut Runtime,
    realm: u32,
    source: Option<Gc<Object>>,
) -> VmResult<Gc<Object>> {
    match source {
        Some(source) => Ok(source),
        None => new_signal(rt, realm),
    }
}

/// True if `signal` (an `AbortSignal`) is aborted.
pub(crate) fn is_aborted(rt: &mut Runtime, signal: Gc<Object>) -> VmResult<bool> {
    Ok(signal_state(rt, signal)?.aborted)
}

/// The abort reason of `signal`.
pub(crate) fn reason_of(rt: &mut Runtime, signal: Gc<Object>) -> VmResult<Value> {
    Ok(signal_state(rt, signal)?.reason)
}

/// The reason for an abort without one: `AbortError` "signal is aborted
/// without reason".
fn default_reason(rt: &mut Runtime, realm: u32) -> VmResult<Value> {
    let error = b::dom_exception::create(
        rt,
        realm,
        g::DOM_EXCEPTION,
        "AbortError",
        "signal is aborted without reason",
    )?;
    Ok(error.into())
}

/// The reason of `signal abort` with `reason`: `reason`, else the default.
fn reason_or_default(rt: &mut Runtime, realm: u32, reason: Value) -> VmResult<Value> {
    if reason.is_undefined() {
        default_reason(rt, realm)
    } else {
        Ok(reason)
    }
}

/// DOM "signal abort": aborts `signal` and the signals that follow it with
/// `reason`: sets the reasons, aborts the `fetch()` calls and removes the
/// listeners, then fires `abort` at each signal in turn.
pub(crate) fn signal_abort(
    rt: &mut Runtime,
    realm: u32,
    signal: Gc<Object>,
    reason: Value,
) -> VmResult<()> {
    if signal_state(rt, signal)?.aborted {
        return Ok(());
    }
    let reason = reason_or_default(rt, realm, reason)?;
    let mut order = vec![signal];
    let mut dependents = {
        let state = signal_state(rt, signal)?;
        state.aborted = true;
        state.reason = reason;
        // The dependents stay listed: a later `any()` flattens the sources.
        state.dependents.clone()
    };
    dependents.retain(|dependent| {
        // A dependent that is aborted already (by another source) is left.
        !matches!(signal_state(rt, *dependent), Ok(s) if s.aborted)
    });
    for dependent in &dependents {
        let state = signal_state(rt, *dependent)?;
        state.aborted = true;
        state.reason = reason;
        order.push(*dependent);
    }
    // The abort steps, then the events (DOM "signal abort").
    for aborted in &order {
        run_abort_steps(rt, realm, *aborted)?;
    }
    for aborted in order {
        fire_abort(rt, aborted)?;
    }
    Ok(())
}

/// The abort steps of one signal: the fetches and the listeners.
fn run_abort_steps(rt: &mut Runtime, realm: u32, signal: Gc<Object>) -> VmResult<()> {
    let (reason, fetches, listeners) = {
        let state = signal_state(rt, signal)?;
        (
            state.reason,
            std::mem::take(&mut state.fetches),
            std::mem::take(&mut state.listeners),
        )
    };
    for id in fetches {
        crate::fetch::abort_fetch(rt, realm, id, reason)?;
    }
    for listener in listeners {
        crate::events::remove_listener(
            rt,
            listener.target,
            &listener.kind,
            listener.callback,
            listener.capture,
        )?;
    }
    Ok(())
}

/// Fires the trusted event `abort` at `signal`.
fn fire_abort(rt: &mut Runtime, signal: Gc<Object>) -> VmResult<()> {
    let init = EventInit {
        bubbles: false,
        cancelable: false,
        composed: false,
    };
    let now = crate::events::now(rt);
    let mut data = EventData::new("abort".to_owned(), &init, now, Extra::None);
    data.is_trusted = true;
    let event = crate::events::new_event_object(rt, 0, g::EVENT, data)?;
    crate::events::dispatch(rt, 0, signal, event, None)?;
    Ok(())
}

/// Registers the `fetch()` with id `id` to be aborted by `signal`.
pub(crate) fn register_fetch(rt: &mut Runtime, signal: Gc<Object>, id: u64) -> VmResult<()> {
    signal_state(rt, signal)?.fetches.push(id);
    Ok(())
}

/// Removes the registration of a `fetch()` that ended.
pub(crate) fn unregister_fetch(rt: &mut Runtime, signal: Gc<Object>, id: u64) {
    if let Ok(state) = signal_state(rt, signal)
        && let Some(position) = state.fetches.iter().rposition(|&f| f == id)
    {
        state.fetches.swap_remove(position);
    }
}

/// The `signal` option of `addEventListener`: `signal` removes the
/// listener when it aborts.
pub(crate) fn remove_on_abort(
    rt: &mut Runtime,
    signal: Gc<Object>,
    target: Gc<Object>,
    kind: &str,
    callback: Gc<Object>,
    capture: bool,
) -> VmResult<()> {
    let state = signal_state(rt, signal)?;
    if state.listeners.len() >= MAX_ABORT_LISTENERS {
        log::warn!(
            "an AbortSignal tracks at most {MAX_ABORT_LISTENERS} listeners; a further one stays after an abort"
        );
        return Ok(());
    }
    state.listeners.push(ListenerRef {
        target,
        kind: kind.to_owned(),
        callback,
        capture,
    });
    Ok(())
}

/// A task of `AbortSignal.timeout()` came due: abort the signal with a
/// `TimeoutError`.
pub(crate) fn timeout_fired(rt: &mut Runtime, id: u64) -> VmResult<()> {
    let Some(signal) = state_mut(rt)?.signal_timeouts.remove(&id) else {
        return Ok(());
    };
    // Out of the table, the signal lives as long as the task's scope (the
    // page may hold it only through its listeners).
    rt.heap_mut().record(signal);
    let error =
        b::dom_exception::create(rt, 0, g::DOM_EXCEPTION, "TimeoutError", "signal timed out")?;
    signal_abort(rt, 0, signal, error.into())
}

impl g::abort_controller::AbortControllerImpl for WebState {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let signal = new_signal(cx.rt, realm)?;
        // The signal is only in a local until the controller holds it.
        let data = cx.rt.alloc_host_data(ControllerData { signal })?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn signal(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        controller(cx.rt, this)
    }

    fn abort(cx: &mut Cx<'_>, this: Platform, reason: Value) -> b::Result<()> {
        let signal = controller(cx.rt, this)?;
        let realm = cx.realm();
        Ok(signal_abort(cx.rt, realm, signal, reason)?)
    }
}

/// The signal of the controller `this`.
fn controller(rt: &Runtime, this: Platform) -> b::Result<Gc<Object>> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<ControllerData>(handle)
            .map(|data| data.signal)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

impl g::abort_signal::AbortSignalImpl for WebState {
    fn aborted(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(is_aborted(cx.rt, this.object)?)
    }

    fn reason(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        Ok(reason_of(cx.rt, this.object)?)
    }

    fn abort(cx: &mut Cx<'_>, reason: Value) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let signal = new_signal(cx.rt, realm)?;
        let reason = reason_or_default(cx.rt, realm, reason)?;
        let state = signal_state(cx.rt, signal)?;
        state.aborted = true;
        state.reason = reason;
        Ok(signal)
    }

    fn timeout(cx: &mut Cx<'_>, milliseconds: u64) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let signal = new_signal(cx.rt, realm)?;
        let state = state_mut(cx.rt)?;
        if state.signal_timeouts.len() >= MAX_TIMEOUT_SIGNALS {
            log::warn!(
                "a document has more than {MAX_TIMEOUT_SIGNALS} AbortSignal.timeout() signals"
            );
            return Err(cx
                .rt
                .terminate("too many AbortSignal.timeout() signals in the document")
                .into());
        }
        state.next_timeout_signal += 1;
        let id = state.next_timeout_signal;
        state.signal_timeouts.insert(id, signal);
        let due = state.clock.page_now() + milliseconds as f64;
        state.event_loop.tasks.push(due, Task::AbortTimeout(id));
        Ok(signal)
    }

    fn any(cx: &mut Cx<'_>, signals: Vec<Platform>) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let inputs: Vec<Gc<Object>> = signals.iter().map(|s| s.object).collect();
        Ok(any_of(cx.rt, realm, &inputs)?)
    }

    fn throw_if_aborted(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        if is_aborted(cx.rt, this.object)? {
            let reason = reason_of(cx.rt, this.object)?;
            return Err(Exception::Js(VmError::Throw(reason)));
        }
        Ok(())
    }
}
