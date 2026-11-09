//! `MutationObserver` and `MutationRecord` (DOM §4.3,
//! <https://dom.spec.whatwg.org/#mutation-observers>; M8 feature 9a).
//!
//! The registered observers and the queued records are Rust data in the
//! [registry](crate::mutation_registry), which is the document's mutation
//! sink once a script calls `observe()` (ADR 0028 sections 3 and 13). This
//! module makes the JavaScript objects and delivers.
//!
//! **Delivery.** DOM "queue a mutation observer microtask": the registry
//! sets a request flag when it queues a record (a cell of the runtime that
//! all documents share: a registry changes only while its document is the
//! current one). The glue of every native call checks the flag after the
//! call ([`after_call`]), and so does each
//! microtask checkpoint, and then queues one microtask (a host function)
//! unless one waits. So the microtask has its place in the queue among the
//! promise jobs, as in Chromium (measured: a promise reaction queued before
//! the first change runs before the callback, one queued after it runs
//! after). The microtask runs "notify mutation observers": for each
//! observer with records, in creation order, it calls the callback with an
//! array of `MutationRecord` objects and the observer, `this` being the
//! observer; an exception is reported and the next observer runs.
//!
//! **Lifetime.** An observer lives while it has registrations: the binding
//! state holds it strongly from `observe()` until `disconnect()`, as the
//! registered observer lists of the nodes do in the specification (nodes
//! and their lists live as long as the document, ADR 0028 section 3). An
//! observer that was never observed or was disconnected is collected as
//! soon as script drops it.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use swb_dom::NodeId;
use swb_js::{
    Gc, GenericData, HostData, NativeCall, NativeReturn, Object, Runtime, ScriptError, Tracer,
    Value, VmError, VmResult,
};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{MutationCallback, MutationObserverInit};
use crate::mutation_registry::{Options, Record, Registry, Shared, Sink};
use crate::state::{WebState, dom_mut, state, state_mut};

/// The mutation observer state of the agent.
pub(crate) struct MutationState {
    registry: Shared,
    next_id: u32,
    /// The observers that have registrations, by number.
    kept: BTreeMap<u32, Gc<Object>>,
    /// True while the mutation observer microtask waits in the job queue.
    queued: bool,
    /// The host function that is the microtask.
    notify_fn: Option<Gc<Object>>,
}

impl Default for MutationState {
    fn default() -> MutationState {
        MutationState::new(Rc::new(Cell::new(false)))
    }
}

impl Drop for MutationState {
    /// The sinks of the documents outlive the runtime.
    fn drop(&mut self) {
        self.close();
    }
}

impl MutationState {
    /// The state of a document whose registry sets `request` when a record
    /// is queued ([`Runtime::host_signal_cell`]).
    pub(crate) fn new(request: Rc<Cell<bool>>) -> MutationState {
        MutationState {
            registry: Rc::new(RefCell::new(Registry::new(request))),
            next_id: 0,
            kept: BTreeMap::new(),
            queued: false,
            notify_fn: None,
        }
    }

    /// Counts the bytes of the waiting records in the budget of the page's frames.
    pub(crate) fn join_page(&self, page: &Rc<Cell<u64>>) {
        self.registry.borrow_mut().join_page(page);
    }

    /// The runtime stopped: see [`Registry::close`].
    pub(crate) fn close(&self) {
        if let Ok(mut registry) = self.registry.try_borrow_mut() {
            registry.close();
        }
    }

    /// See [`Registry::move_nodes`].
    pub(crate) fn move_nodes(&self, from: u32, to: u32, pairs: &[(NodeId, NodeId)]) -> bool {
        self.registry.borrow_mut().move_nodes(from, to, pairs)
    }

    /// Reports the handles that the state holds.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for observer in self.kept.values() {
            tracer.object(*observer);
        }
        if let Some(function) = self.notify_fn {
            tracer.object(function);
        }
    }
}

/// The host data of a `MutationObserver`.
struct ObserverData {
    id: u32,
    callback: Gc<Object>,
}

impl GenericData for ObserverData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.callback);
    }

    fn heap_size(&self) -> usize {
        0
    }
}

/// The host data of a `MutationRecord`.
struct RecordData {
    record: Record,
    added_list: Option<Gc<Object>>,
    removed_list: Option<Gc<Object>>,
}

impl GenericData for RecordData {
    /// The ids of a record mean the arena of its document, whatever happened
    /// since; a node that moved is found from there. So the word names the
    /// document node, which stays.
    fn host_word(&self) -> Option<u64> {
        Some(b::word_of(self.record.document, NodeId::DOCUMENT))
    }

    fn trace(&self, tracer: &mut Tracer<'_>) {
        for list in [self.added_list, self.removed_list].into_iter().flatten() {
            tracer.object(list);
        }
    }

    fn heap_size(&self) -> usize {
        size_of::<Record>()
            + size_of::<NodeId>() * (self.record.added.len() + self.record.removed.len())
            + self.record.old_value.as_ref().map_or(0, String::len)
    }
}

fn observer_data(rt: &Runtime, this: Platform) -> b::Result<&ObserverData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<ObserverData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

fn record_data(rt: &Runtime, this: Platform) -> b::Result<&RecordData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<RecordData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

fn record_data_mut(rt: &mut Runtime, this: Platform) -> b::Result<&mut RecordData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap_mut()
            .generic_mut::<RecordData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// Called by the glue after each native call and by the microtask
/// checkpoint: queues the mutation observer microtask if a record was
/// queued since the last time.
///
/// It also charges the work of the registry to the time limit of the task
/// (native work does not tick the countdown of the VM): a termination is
/// returned.
#[inline]
pub(crate) fn after_call(rt: &mut Runtime) -> VmResult<()> {
    if needs_attention(rt) {
        return after_request(rt);
    }
    Ok(())
}

/// True if the registry asked for attention (a record was queued or work
/// piled up): the fast path of the glue of every native call is this one
/// test, then [`after_request`].
#[inline]
pub(crate) fn needs_attention(rt: &Runtime) -> bool {
    rt.host_signal()
}

/// The slow path of [`after_call`]: the registry asked for attention.
#[cold]
#[inline(never)]
pub(crate) fn after_request(rt: &mut Runtime) -> VmResult<()> {
    let registry = state(rt)?.mutation.registry.clone();
    let (microtask, overdue) = registry.borrow_mut().take_signals();
    if microtask {
        queue_microtask(rt);
    }
    // The custom element reactions that the sink queued with no scope active
    // wait on the backup element queue, which needs its microtask; the nodes
    // that the sink visited count as work.
    let reactions = state(rt)?.custom.reactions.clone();
    let (backup, reactions_overdue) = {
        let mut reactions = reactions.borrow_mut();
        (reactions.take_microtask_wanted(), reactions.take_overdue())
    };
    if backup {
        crate::custom_elements::reactions::queue_backup_microtask(rt)?;
    }
    // The work of the pre-removing steps of the node iterators counts too.
    if crate::node_iterator::take_overdue(rt) || overdue || reactions_overdue {
        rt.check_time()?;
    }
    Ok(())
}

/// DOM "queue a mutation observer microtask".
fn queue_microtask(rt: &mut Runtime) {
    let Ok(web) = state_mut(rt) else {
        return;
    };
    if web.mutation.queued {
        return;
    }
    web.mutation.queued = true;
    let function = if let Some(function) = web.mutation.notify_fn {
        function
    } else if let Ok(function) =
        crate::state::doc_realm(rt).and_then(|realm| rt.new_host_method(realm, "", 0, notify))
    {
        if let Ok(web) = state_mut(rt) {
            web.mutation.notify_fn = Some(function);
        }
        function
    } else {
        if let Ok(web) = state_mut(rt) {
            web.mutation.queued = false;
        }
        return;
    };
    rt.queue_microtask(Value::Object(function));
}

/// The job queue of the agent was cleared (a termination): the microtask is
/// gone, and records may wait.
pub(crate) fn jobs_cleared(rt: &mut Runtime) {
    let Ok(web) = state_mut(rt) else {
        return;
    };
    web.mutation.queued = false;
    web.mutation.registry.borrow_mut().want_microtask();
}

/// The mutation observer microtask.
fn notify(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    // The function belongs to the realm of the document that asked first.
    let entered = crate::state::enter_realm(rt, call.realm())?;
    let result = deliver(rt);
    crate::state::leave(rt, entered);
    result?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// DOM "notify mutation observers".
fn deliver(rt: &mut Runtime) -> VmResult<()> {
    let registry = {
        let web = state_mut(rt)?;
        web.mutation.queued = false;
        web.mutation.registry.clone()
    };
    let ids = registry.borrow_mut().take_notify_set();
    for (position, &id) in ids.iter().enumerate() {
        let result = rt.check_time().and_then(|()| notify_one(rt, &registry, id));
        if let Err(error) = result {
            // A termination: the observers whose turn has not come keep
            // their records and are notified by the next delivery.
            registry.borrow_mut().restore_notify(&ids[position..]);
            return Err(error);
        }
    }
    after_call(rt)
}

/// The turn of one observer in "notify mutation observers".
fn notify_one(rt: &mut Runtime, registry: &Shared, id: u32) -> VmResult<()> {
    let records = registry.borrow_mut().take_for_notification(id);
    if records.is_empty() {
        return Ok(());
    }
    let Some(&observer) = state(rt)?.mutation.kept.get(&id) else {
        return Ok(());
    };
    let Some(callback) = callback_of(rt, observer) else {
        return Ok(());
    };
    let list = records_to_js(rt, observer, records)?;
    b::keep(rt, Value::Object(list));
    b::keep(rt, Value::Object(callback));
    let result = rt.call_function(
        Value::Object(callback),
        Value::Object(observer),
        &[Value::Object(list), Value::Object(observer)],
    );
    match result {
        Ok(_) => Ok(()),
        Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
        Err(error) => {
            let site = crate::errors::Site::named("a MutationObserver callback");
            crate::errors::report_callback(rt, callback, &site, &error)
        }
    }
}

/// The callback of an observer object.
fn callback_of(rt: &Runtime, observer: Gc<Object>) -> Option<Gc<Object>> {
    let (_, data) = rt.host_object(Value::Object(observer))?;
    let HostData::Generic(handle) = data else {
        return None;
    };
    rt.heap()
        .generic::<ObserverData>(handle)
        .ok()
        .map(|d| d.callback)
}

/// The array of `MutationRecord` objects for `records`.
fn records_to_js(
    rt: &mut Runtime,
    observer: Gc<Object>,
    records: Vec<Record>,
) -> VmResult<Gc<Object>> {
    // The realm of the observer (measured: also for a record of a node of
    // another document).
    let realm = match rt.host_object_realm(Value::Object(observer)) {
        Some(realm) => realm,
        None => crate::state::doc_realm(rt)?,
    };
    let mut objects = Vec::with_capacity(records.len());
    for record in records {
        let data = rt.alloc_host_data(RecordData {
            record,
            added_list: None,
            removed_list: None,
        })?;
        let object = rt.new_host_object(realm, g::MUTATION_RECORD, data)?;
        b::keep(rt, Value::Object(object));
        objects.push(object);
    }
    let value = b::to_js::sequence(rt, realm, &objects, |_, item| Ok(Value::Object(*item)))?;
    match value {
        Value::Object(array) => Ok(array),
        _ => Err(VmError::type_error("a sequence is an array")),
    }
}

/// Makes the dispatcher the mutation sink of the current document, once: for
/// a mutation observer (`observing`) or because the agent has a custom
/// element definition.
pub(crate) fn install_sink(rt: &mut Runtime, observing: bool) -> VmResult<()> {
    if observing {
        state_mut(rt)?.doc.observing = true;
    }
    if state(rt)?.doc.mutation_sink {
        return Ok(());
    }
    let sink = make_sink(state(rt)?, crate::state::document_key(rt)?);
    dom_mut(rt)?.set_mutation_sink(Some(Box::new(sink)));
    state_mut(rt)?.doc.mutation_sink = true;
    Ok(())
}

/// The dispatcher for the document `document` of the agent.
pub(crate) fn make_sink(web: &WebState, document: u32) -> Sink {
    let registry = web.mutation.registry.clone();
    let request = registry.borrow().request();
    Sink {
        registry,
        reactions: Rc::clone(&web.custom.reactions),
        document,
        request,
    }
}

/// The options of `observe()` (DOM §4.3.2 steps 1 to 6; Chromium checks
/// the old-value and filter conditions before the "at least one" one:
/// measured).
fn options_of(init: MutationObserverInit) -> b::Result<Options> {
    let attributes = init.attributes.or_else(|| {
        (init.attribute_old_value.is_some() || init.attribute_filter.is_some()).then_some(true)
    });
    let character_data = init
        .character_data
        .or_else(|| init.character_data_old_value.is_some().then_some(true));
    if init.attribute_old_value == Some(true) && attributes == Some(false) {
        return Err(Exception::type_error(
            "The options object may only set 'attributeOldValue' to true when 'attributes' is true or not present.",
        ));
    }
    if init.attribute_filter.is_some() && attributes == Some(false) {
        return Err(Exception::type_error(
            "The options object may only set 'attributeFilter' when 'attributes' is true or not present.",
        ));
    }
    if init.character_data_old_value == Some(true) && character_data == Some(false) {
        return Err(Exception::type_error(
            "The options object may only set 'characterDataOldValue' to true when 'characterData' is true or not present.",
        ));
    }
    let attributes = attributes.unwrap_or(false);
    let character_data = character_data.unwrap_or(false);
    if !init.child_list && !attributes && !character_data {
        return Err(Exception::type_error(
            "The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.",
        ));
    }
    Ok(Options {
        child_list: init.child_list,
        attributes,
        character_data,
        subtree: init.subtree,
        attribute_old_value: init.attribute_old_value.unwrap_or(false),
        character_data_old_value: init.character_data_old_value.unwrap_or(false),
        attribute_filter: init.attribute_filter,
    })
}

impl g::mutation_observer::MutationObserverImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, callback: MutationCallback) -> b::Result<Gc<Object>> {
        let mutation = &mut state_mut(cx.rt)?.mutation;
        mutation.next_id += 1;
        let id = mutation.next_id;
        let data = cx.rt.alloc_host_data(ObserverData {
            id,
            callback: callback.0,
        })?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn observe(
        cx: &mut Cx<'_>,
        this: Platform,
        target: b::NodeRef,
        options: MutationObserverInit,
    ) -> b::Result<()> {
        let id = observer_data(cx.rt, this)?.id;
        let options = options_of(options)?;
        let entered = crate::state::switch_if_other(cx.rt, target.document)?;
        let installed = install_sink(cx.rt, true);
        crate::state::leave(cx.rt, entered);
        installed?;
        let mutation = &mut state_mut(cx.rt)?.mutation;
        mutation
            .registry
            .borrow_mut()
            .observe(id, (target.document, target.node), options);
        mutation.kept.insert(id, this.object);
        Ok(())
    }

    fn disconnect(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let id = observer_data(cx.rt, this)?.id;
        let mutation = &mut state_mut(cx.rt)?.mutation;
        mutation.registry.borrow_mut().disconnect(id);
        mutation.kept.remove(&id);
        Ok(())
    }

    fn take_records(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<Gc<Object>>> {
        let id = observer_data(cx.rt, this)?.id;
        let registry = state(cx.rt)?.mutation.registry.clone();
        let records = registry.borrow_mut().take_records(id);
        let mut objects = Vec::with_capacity(records.len());
        for record in records {
            cx.rt.check_time()?;
            let data = cx.rt.alloc_host_data(RecordData {
                record,
                added_list: None,
                removed_list: None,
            })?;
            let realm = cx.realm();
            let object = cx.rt.new_host_object(realm, g::MUTATION_RECORD, data)?;
            b::keep(cx.rt, Value::Object(object));
            objects.push(object);
        }
        Ok(objects)
    }
}

impl g::mutation_record::MutationRecordImpl for WebState {
    fn type_(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(record_data(cx.rt, this)?.record.kind.name().to_owned())
    }

    fn target(cx: &mut Cx<'_>, this: Platform) -> b::Result<NodeId> {
        Ok(record_data(cx.rt, this)?.record.target)
    }

    fn added_nodes(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let data = record_data(cx.rt, this)?;
        if let Some(list) = data.added_list {
            return Ok(list);
        }
        let nodes = data.record.added.clone();
        let list = crate::dom::lists::new_static_list(cx.rt, cx.realm(), nodes)?;
        record_data_mut(cx.rt, this)?.added_list = Some(list);
        Ok(list)
    }

    fn removed_nodes(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let data = record_data(cx.rt, this)?;
        if let Some(list) = data.removed_list {
            return Ok(list);
        }
        let nodes = data.record.removed.clone();
        let list = crate::dom::lists::new_static_list(cx.rt, cx.realm(), nodes)?;
        record_data_mut(cx.rt, this)?.removed_list = Some(list);
        Ok(list)
    }

    fn previous_sibling(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        Ok(record_data(cx.rt, this)?.record.previous_sibling)
    }

    fn next_sibling(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        Ok(record_data(cx.rt, this)?.record.next_sibling)
    }

    fn attribute_name(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        Ok(record_data(cx.rt, this)?.record.attribute_name.clone())
    }

    fn attribute_namespace(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        Ok(record_data(cx.rt, this)?.record.attribute_namespace.clone())
    }

    fn old_value(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        Ok(record_data(cx.rt, this)?.record.old_value.clone())
    }
}
