//! `PerformanceObserver` and `PerformanceObserverEntryList` (Performance
//! Timeline §5, <https://w3c.github.io/performance-timeline/#the-performanceobserver-interface>;
//! M8 feature 8a2). Only the entry types that swb makes are supported:
//! `mark`, `measure` and `navigation` (the last one only with `buffered`,
//! as it is made before a script can observe it).
//!
//! An observer that observes is in the registry of the document's
//! performance state, so it lives while it observes. A new entry is queued
//! at each observer that observes its type; the observers get their queue in
//! one task ([`crate::event_loop::Task::PerformanceObservers`], HTML "queue a
//! `PerformanceObserver` task"), in the order they started to observe, each
//! with a microtask checkpoint after its callback. Measured in Chromium:
//! `performance.mark('a'); setTimeout(f, 0)` runs the callback before `f`
//! and after the microtasks of the calling script; an exception of a
//! callback is reported and the next delivery works; `entryTypes` replaces
//! the types and `type` adds one; mixing the two forms is an
//! `InvalidModificationError`; unsupported types are ignored.
//!
//! Limit: an observer queues at most [`MAX_QUEUE`] entries that wait for its
//! delivery; later entries are not queued for it.

use swb_js::{Gc, GenericData, HostData, Object, Runtime, ScriptError, Tracer, Value};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::event_loop::{Task, queue_task};
use crate::generated as g;
use crate::generated::types::{PerformanceObserverCallback, PerformanceObserverInit};
use crate::performance::{EntryKind, SUPPORTED_TYPES, filter_sorted};
use crate::runtime::task_error;
use crate::state::{WebState, state, state_mut};

/// The most entries that wait in an observer's queue.
pub(crate) const MAX_QUEUE: usize = 131_072;

/// Which form of `observe()` an observer used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    None,
    /// `observe({ entryTypes })`.
    EntryTypes,
    /// `observe({ type })`.
    Type,
}

/// The host data of an observer.
struct ObserverData {
    callback: Gc<Object>,
    kinds: Vec<EntryKind>,
    form: Form,
    queue: Vec<Gc<Object>>,
    registered: bool,
}

impl GenericData for ObserverData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.callback);
        for &entry in &self.queue {
            tracer.object(entry);
        }
    }

    fn heap_size(&self) -> usize {
        self.queue.capacity() * size_of::<Gc<Object>>()
    }
}

/// The host data of an entry list.
struct ListData {
    entries: Vec<Gc<Object>>,
}

impl GenericData for ListData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for &entry in &self.entries {
            tracer.object(entry);
        }
    }

    fn heap_size(&self) -> usize {
        self.entries.capacity() * size_of::<Gc<Object>>()
    }
}

fn observer_of(rt: &mut Runtime, this: Platform) -> b::Result<&mut ObserverData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap_mut()
            .generic_mut::<ObserverData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// The data of an observer object that the registry holds.
fn registered_data(rt: &mut Runtime, object: Gc<Object>) -> Option<&mut ObserverData> {
    let (_, data) = rt.host_object(Value::Object(object))?;
    let HostData::Generic(handle) = data else {
        return None;
    };
    rt.heap_mut().generic_mut::<ObserverData>(handle).ok()
}

fn list_of(rt: &Runtime, this: Platform) -> b::Result<&ListData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<ListData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// Queues the new entry `entry` of `kind` at the observers that observe
/// its type, and queues the delivery task if one of them got it.
pub(crate) fn queue_for_observers(
    rt: &mut Runtime,
    entry: Gc<Object>,
    kind: EntryKind,
) -> swb_js::VmResult<()> {
    if state(rt)?.perf.observers.is_empty() {
        return Ok(());
    }
    let observers = state(rt)?.perf.observers.clone();
    let mut queued = false;
    for observer in observers {
        if let Some(data) = registered_data(rt, observer)
            && data.kinds.contains(&kind)
            && data.queue.len() < MAX_QUEUE
        {
            data.queue.push(entry);
            queued = true;
        }
    }
    if queued {
        queue_delivery(rt)?;
    }
    Ok(())
}

/// Queues the delivery task unless one waits.
fn queue_delivery(rt: &mut Runtime) -> swb_js::VmResult<()> {
    let perf = &mut state_mut(rt)?.perf;
    if perf.delivery_queued {
        return Ok(());
    }
    perf.delivery_queued = true;
    queue_task(rt, Task::PerformanceObservers)
}

/// The delivery task: each observer with entries in its queue gets them in
/// a `PerformanceObserverEntryList`.
pub(crate) fn deliver(rt: &mut Runtime) -> Result<(), ScriptError> {
    state_mut(rt).map_err(task_error)?.perf.delivery_queued = false;
    let observers = state(rt).map_err(task_error)?.perf.observers.clone();
    for observer in observers {
        let Some(data) = registered_data(rt, observer) else {
            continue;
        };
        if !data.registered || data.queue.is_empty() {
            continue;
        }
        let callback = data.callback;
        let entries = std::mem::take(&mut data.queue);
        call_callback(rt, observer, callback, entries)?;
        crate::event_loop::microtask_checkpoint(rt)?;
    }
    Ok(())
}

/// Calls the callback of an observer: `callback(list, observer, { droppedEntriesCount: 0 })`
/// with the observer as `this`. An exception is reported.
fn call_callback(
    rt: &mut Runtime,
    observer: Gc<Object>,
    callback: Gc<Object>,
    entries: Vec<Gc<Object>>,
) -> Result<(), ScriptError> {
    let made = (|| {
        let data = rt.alloc_host_data(ListData { entries })?;
        let list = rt.new_host_object(0, g::PERFORMANCE_OBSERVER_ENTRY_LIST, data)?;
        b::keep(rt, Value::Object(list));
        let options = rt.new_plain_object(0)?;
        b::keep(rt, Value::Object(options));
        b::to_js::dictionary_member(rt, options, "droppedEntriesCount", Value::number(0.0))?;
        swb_js::VmResult::Ok((list, options))
    })();
    let (list, options) = made.map_err(task_error)?;
    b::keep(rt, Value::Object(callback));
    let result = rt.call_function(
        Value::Object(callback),
        Value::Object(observer),
        &[
            Value::Object(list),
            Value::Object(observer),
            Value::Object(options),
        ],
    );
    match result {
        Ok(_) => Ok(()),
        Err(error @ (ScriptError::Terminated(_) | ScriptError::Internal(_))) => Err(error),
        Err(error) => {
            let site = crate::errors::Site::named("a PerformanceObserver callback");
            crate::errors::report(rt, &site, &error).map_err(task_error)
        }
    }
}

/// Puts the observer in the registry (once).
fn register(rt: &mut Runtime, this: Platform) -> b::Result<()> {
    let data = observer_of(rt, this)?;
    if std::mem::replace(&mut data.registered, true) {
        return Ok(());
    }
    state_mut(rt)?.perf.observers.push(this.object);
    Ok(())
}

impl g::performance_observer::PerformanceObserverImpl for WebState {
    fn supported_entry_types(_cx: &mut Cx<'_>) -> b::Result<Vec<String>> {
        Ok(SUPPORTED_TYPES.iter().map(|&t| t.to_owned()).collect())
    }

    fn constructor(
        cx: &mut Cx<'_>,
        callback: PerformanceObserverCallback,
    ) -> b::Result<Gc<Object>> {
        let data = cx.rt.alloc_host_data(ObserverData {
            callback: callback.0,
            kinds: Vec::new(),
            form: Form::None,
            queue: Vec::new(),
            registered: false,
        })?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn observe(cx: &mut Cx<'_>, this: Platform, options: PerformanceObserverInit) -> b::Result<()> {
        match (options.entry_types, options.type_) {
            (None, None) => Err(Exception::type_error(
                "An observe() call must include either entryTypes or type arguments.",
            )),
            (Some(_), Some(_)) => Err(Exception::type_error(
                "An observe() call must not include both entryTypes and type arguments.",
            )),
            (Some(types), None) => {
                if observer_of(cx.rt, this)?.form == Form::Type {
                    return Err(Exception::dom(
                        "InvalidModificationError",
                        "This PerformanceObserver has performed observe({type:...}, therefore it cannot perform observe({entryTypes:...})",
                    ));
                }
                let kinds = supported_kinds(&types);
                if kinds.is_empty() {
                    return Ok(());
                }
                let data = observer_of(cx.rt, this)?;
                data.form = Form::EntryTypes;
                data.kinds = kinds;
                register(cx.rt, this)
            }
            (None, Some(entry_type)) => {
                if observer_of(cx.rt, this)?.form == Form::EntryTypes {
                    return Err(Exception::dom(
                        "InvalidModificationError",
                        "This observer has performed observe({entryTypes:...}, therefore it cannot perform observe({type:...})",
                    ));
                }
                let Some(kind) = EntryKind::from_type(&entry_type) else {
                    log::debug!(
                        "PerformanceObserver: the entry type '{entry_type}' is not supported"
                    );
                    return Ok(());
                };
                let data = observer_of(cx.rt, this)?;
                data.form = Form::Type;
                if !data.kinds.contains(&kind) {
                    data.kinds.push(kind);
                }
                register(cx.rt, this)?;
                if options.buffered == Some(true) {
                    queue_buffered(cx, this, kind)?;
                }
                Ok(())
            }
        }
    }

    fn disconnect(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let data = observer_of(cx.rt, this)?;
        // The form of `observe()` stays (measured: after `observe({
        // entryTypes })` and `disconnect()`, `observe({ type })` throws).
        data.kinds.clear();
        data.queue.clear();
        data.registered = false;
        state_mut(cx.rt)?
            .perf
            .observers
            .retain(|&observer| observer != this.object);
        Ok(())
    }

    fn take_records(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        Ok(std::mem::take(&mut observer_of(cx.rt, this)?.queue))
    }
}

/// The supported kinds among entry type names, without repeats.
fn supported_kinds(types: &[String]) -> Vec<EntryKind> {
    let mut kinds = Vec::new();
    for name in types {
        match EntryKind::from_type(name) {
            Some(kind) if !kinds.contains(&kind) => kinds.push(kind),
            Some(_) => {}
            None => log::debug!("PerformanceObserver: the entry type '{name}' is not supported"),
        }
    }
    kinds
}

/// `buffered`: queues the entries of `kind` that exist now.
fn queue_buffered(cx: &mut Cx<'_>, this: Platform, kind: EntryKind) -> b::Result<()> {
    let all = crate::performance::buffer(cx)?;
    let found = filter_sorted(cx.rt, all.into_iter(), None, Some(kind.type_name()));
    if found.is_empty() {
        return Ok(());
    }
    let data = observer_of(cx.rt, this)?;
    for entry in found {
        if data.queue.len() < MAX_QUEUE {
            data.queue.push(entry);
        }
    }
    queue_delivery(cx.rt)?;
    Ok(())
}

impl g::performance_observer_entry_list::PerformanceObserverEntryListImpl for WebState {
    fn get_entries(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        let entries = list_of(cx.rt, this)?.entries.clone();
        Ok(filter_sorted(cx.rt, entries.into_iter(), None, None))
    }

    fn get_entries_by_type(
        cx: &mut Cx<'_>,
        this: Platform,
        type_: String,
    ) -> b::Result<Vec<Gc<Object>>> {
        let entries = list_of(cx.rt, this)?.entries.clone();
        Ok(filter_sorted(
            cx.rt,
            entries.into_iter(),
            None,
            Some(&type_),
        ))
    }

    fn get_entries_by_name(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
        type_: Option<String>,
    ) -> b::Result<Vec<Gc<Object>>> {
        let entries = list_of(cx.rt, this)?.entries.clone();
        Ok(filter_sorted(
            cx.rt,
            entries.into_iter(),
            Some(&name),
            type_.as_deref(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_types_are_left_out() {
        let types: Vec<String> = ["resource", "mark", "paint", "mark", "measure"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(
            supported_kinds(&types),
            [EntryKind::Mark, EntryKind::Measure]
        );
        assert_eq!(supported_kinds(&[]), Vec::<EntryKind>::new());
    }
}
