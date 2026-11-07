//! `MessageChannel` and `MessagePort` (HTML §9.5,
//! <https://html.spec.whatwg.org/multipage/web-messaging.html#message-channels>;
//! ADR 0031 section 7).
//!
//! **Hub and runtimes.** A page has one [`PortHub`] of plain data, which its runtimes
//! share (`AgentShared`): each port has an id that is unique in the page and never
//! reused, and an `Entry` (`hub.rs`): the entangled port, the queue of waiting messages
//! (serialization records, [`Serialized`]), whether the queue is enabled, and the
//! tag of the runtime that holds its object (`None` while the port travels in a
//! message). A record holds port ids and no object, so it can go to any runtime as
//! it is, and the queue of a port stays where it is when the port moves. What belongs
//! to one runtime is in that runtime's [`Ports`]: the table of port objects (a `Gc`
//! and a realm), which is the only way from an id to an object, and the budget.
//!
//! The port object is a host object of its realm; its [`PortState`] (in the generic
//! data of the event target) holds the id and a share of the [`Ports`]. A transfer
//! detaches the sender's object ("neutered": every call does nothing) and a new
//! object adopts the id in the receiver's realm, in whatever runtime that is. A
//! closed port has no entry.
//!
//! **Across runtimes.** A message to a port of another runtime goes to the queue of
//! that port at once, because the queue is plain data; the sender's task leaves a
//! [`crate::messaging::Mail`] for the task of the port, and the page hands it to that
//! runtime between tasks. No script of another runtime runs inside a native.
//!
//! **Lifetime.** The entries are outside the JavaScript heaps, so the collectors and
//! the hub cooperate:
//!
//! - A port holds its entangled port strongly when both objects are in one heap
//!   ([`PortState::trace`]; the rule of HTML "Ports and garbage collection"). A pair
//!   that script no longer reaches is collected together, also with listeners on both
//!   ends (measured in Chromium: all four ports of two such pairs are finalized).
//! - A port object that the collector frees drops its [`PortState`]; the drop leaves
//!   the table of objects and puts the id on a list ([`PortHub::doomed`]), and the next
//!   operation of the hub removes the entry ([`PortHub::reap`]) with its queue; the
//!   partner is disentangled, wherever it is, and the ports that travel in the dropped
//!   messages are closed. When a runtime goes, its heap drops all its port objects, so
//!   the same path disentangles what they were entangled with.
//! - A port stays alive while it has a task in the event loop, and while its queue is
//!   enabled and its entangled port is not in this heap: it travels, or it is in
//!   another runtime, and could send (the spec's "must not be garbage collected while
//!   there is a message in a task queue ... and its entangled port could send"):
//!   [`Ports::trace`]. So a pair across runtimes with listeners on both ends stays until
//!   a side closes or a runtime goes, which [`MAX_PORTS`] bounds (ADR 0031 section 7).
//! - `close()` removes the entry at once. The ports of a window whose document retired
//!   close too ([`Ports::close_realm`]).
//! - A port that travels has no object, so no collector can free it.
//!   `Registry::sweep` removes the travelling entries that nothing can deliver: those
//!   that no entry with an object or a task, and no window message that waits, reaches
//!   through the queued records. A port that travels only in the queue of a travelling
//!   port (`c.port1.postMessage(1, [c.port2])`) is removed this way. A port with a
//!   listener whose partner travels is a root of the collector; the sweep removes the
//!   unreachable partner, and the next collection frees the port (at most two
//!   collections, each followed by a sweep).
//! - A runtime makes at most [`MAX_PORTS`] live ports, wherever they are now.
//!   `new MessageChannel()` past it first forces a collection (a page that makes a
//!   channel per call in a loop, as some `setImmediate` polyfills do, has garbage
//!   entries that the collector has not freed yet; the forced collection runs at most
//!   once per [`COLLECT_EVERY`] channels), then throws a `RangeError` with a warning.
//!   A port counts for the runtime that made it, so a frame that sends ports to the top
//!   level does not use up the top level's budget.
//!
//! **Tasks.** Chromium delivers a port's messages one per task, and queues the task for
//! the next message after the listener of the last one has run (measured: a timer that
//! the handler of message 1 queues runs before message 2, also when the handler posted
//! message 3 first). `Entry::pumps` counts the queued tasks of a port
//! ([`Task::PortMessage`]) in its home runtime: at most one unless the port came back
//! to a runtime that still had a task for it. The task has no timer clamping and the
//! event loop orders it with the others by queuing time, so timers, rendering and input
//! get their turns in an endless ping-pong.
//!
//! **Queue.** Setting `onmessage` or `onmessageerror` (to any value; measured) enables
//! the queue, `start()` does too; `addEventListener` does not. A received port is not
//! enabled (measured), whatever the sender's state was.
//!
//! **Budget.** The waiting messages of the ports and of `window.postMessage` count for
//! the runtime that sent them until they are delivered or dropped ([`MessageBudget`],
//! ADR 0031 section 11): a frame's flood fills the budget of the frames, and the top
//! level's messages go on.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::event_loop::{Task, page_now};
use crate::events::TargetData;
use crate::generated as g;
use crate::generated::types::StructuredSerializeOptions;
use crate::messaging::Mail;
use crate::state::{WebState, state, state_mut};
use crate::structured::{Serialized, detach_transfer, serialize_for_transfer};

mod hub;

use hub::{
    COLLECT_EVERY, IdBuild, MAX_PORTS, MAX_WAITING, Message, MessageBudget, WAITING_OVERHEAD,
};
pub(crate) use hub::{Charge, PortHub, RuntimeTag};

// ----- What belongs to a runtime -----

/// The port objects and the budget of one runtime.
#[derive(Debug)]
pub(crate) struct Ports {
    hub: Rc<PortHub>,
    tag: RuntimeTag,
    /// The objects of the ports whose home is this runtime, with their realms. The only
    /// way from an id to an object. An object that the collector frees leaves it.
    objects: RefCell<HashMap<u64, (Gc<Object>, u32), IdBuild>>,
    budget: Rc<MessageBudget>,
    /// True after the warning about the port limit, until a channel fits again.
    warned: Cell<bool>,
    /// Channels made since the collection that the limit forced.
    since_collect: Cell<usize>,
}

impl Default for Ports {
    fn default() -> Ports {
        Ports::new(Rc::default())
    }
}

impl Ports {
    /// The ports of a new runtime of the page of `hub`.
    pub(crate) fn new(hub: Rc<PortHub>) -> Ports {
        let tag = hub.next_tag.get();
        hub.next_tag.set(tag.wrapping_add(1));
        Ports {
            hub,
            tag,
            objects: RefCell::default(),
            budget: Rc::default(),
            warned: Cell::new(false),
            since_collect: Cell::new(0),
        }
    }

    /// Counts the waiting bytes in the budget of the page's frames.
    pub(crate) fn join_page(&self, page: &Rc<Cell<u64>>) {
        self.budget.bytes.borrow_mut().join(page);
    }

    /// The tag of this runtime in the hub.
    pub(crate) fn tag(&self) -> RuntimeTag {
        self.tag
    }

    /// The hub of the page.
    pub(crate) fn hub(&self) -> &Rc<PortHub> {
        &self.hub
    }

    /// Counts a message of `bytes` that this runtime is about to leave waiting; `None`
    /// if the budget is full (the caller drops the message).
    pub(crate) fn admit(&self, bytes: usize) -> Option<Charge> {
        self.admit_as(bytes, "message")
    }

    /// [`Ports::admit`] for something else that waits in the same budget (a `storage`
    /// event); `what` names it in the warning.
    pub(crate) fn admit_as(&self, bytes: usize, what: &str) -> Option<Charge> {
        let bytes = bytes.saturating_add(WAITING_OVERHEAD);
        self.budget.admit_as(bytes, what).then(|| Charge {
            budget: Rc::clone(&self.budget),
            bytes,
            settled: false,
        })
    }

    /// Reports the port objects that the collector must keep (see the module comment).
    /// Nothing is borrowed while the heap allocates, so a failed borrow cannot happen; it
    /// would only hide roots.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        let (Ok(registry), Ok(objects)) =
            (self.hub.registry.try_borrow(), self.objects.try_borrow())
        else {
            return;
        };
        for (id, (object, _)) in objects.iter() {
            let Some(entry) = registry.entries.get(id) else {
                continue;
            };
            let partner_elsewhere = entry.enabled
                && entry
                    .partner
                    .and_then(|id| registry.entries.get(&id))
                    .is_some_and(|partner| partner.home != Some(self.tag));
            if entry.pumps > 0 || partner_elsewhere {
                tracer.object(*object);
            }
        }
    }

    /// The number of live ports of the page (entries).
    pub(crate) fn live(&self) -> usize {
        self.hub.live()
    }

    /// The number of messages that this runtime sent and that wait.
    pub(crate) fn waiting(&self) -> usize {
        self.budget.waiting.get()
    }

    /// The bytes of the messages that this runtime sent and that wait.
    pub(crate) fn waiting_bytes(&self) -> u64 {
        self.budget.bytes.borrow().mine()
    }

    /// The window of `realm` lost its document: its ports close, wherever their partners
    /// are, with what waits in their queues.
    pub(crate) fn close_realm(&self, realm: u32) {
        let mut objects = self.objects.borrow_mut();
        let ids: Vec<u64> = objects
            .iter()
            .filter(|(_, (_, r))| *r == realm)
            .map(|(&id, _)| id)
            .collect();
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            objects.remove(id);
        }
        drop(objects);
        self.hub.doom(&ids);
    }

    /// The scripts of the runtime stopped: its ports close, wherever their partners are,
    /// with what waits in their queues.
    pub(crate) fn close_all(&self) {
        let ids: Vec<u64> = self
            .objects
            .borrow_mut()
            .drain()
            .map(|(id, _)| id)
            .collect();
        self.hub.doom(&ids);
        self.hub.reap();
    }
}

/// The state of a `MessagePort` object, in the data of its event target.
pub(crate) struct PortState {
    id: u64,
    ports: Rc<Ports>,
    /// True after a transfer: the object is neutered and the id belongs to
    /// the object that adopts it.
    detached: Cell<bool>,
}

impl fmt::Debug for PortState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortState")
            .field("id", &self.id)
            .field("detached", &self.detached.get())
            .finish_non_exhaustive()
    }
}

impl PortState {
    /// A port holds its entangled port if that is in this heap (module comment,
    /// "Lifetime").
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if self.detached.get() {
            return;
        }
        let (Ok(registry), Ok(objects)) = (
            self.ports.hub.registry.try_borrow(),
            self.ports.objects.try_borrow(),
        ) else {
            return;
        };
        let partner = registry
            .entries
            .get(&self.id)
            .and_then(|e| e.partner)
            .and_then(|id| objects.get(&id));
        if let Some((object, _)) = partner {
            tracer.object(*object);
        }
    }
}

impl Drop for PortState {
    fn drop(&mut self) {
        if !self.detached.get() {
            // The table is not borrowed while the collector runs.
            if let Ok(mut objects) = self.ports.objects.try_borrow_mut() {
                objects.remove(&self.id);
            }
            self.ports.hub.doom(&[self.id]);
        }
    }
}

/// What the object of a port says about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PortInfo {
    pub(crate) id: u64,
    /// True after a transfer.
    pub(crate) detached: bool,
}

/// The id of `object` if it is a `MessagePort`.
pub(crate) fn port_info(rt: &Runtime, object: Gc<Object>) -> Option<PortInfo> {
    let (_, HostData::Generic(handle)) = rt.host_object(object.into())? else {
        return None;
    };
    let data = rt.heap().generic::<TargetData>(handle).ok()?;
    let port = data.port.as_ref()?;
    Some(PortInfo {
        id: port.id,
        detached: port.detached.get(),
    })
}

fn ports_of(rt: &Runtime) -> VmResult<Rc<Ports>> {
    Ok(Rc::clone(&state(rt)?.ports))
}

fn illegal() -> Exception {
    Exception::type_error("Illegal invocation")
}

/// A new `MessagePort` object for `id` in `realm`.
fn new_port_object(
    rt: &mut Runtime,
    ports: &Rc<Ports>,
    realm: u32,
    id: u64,
) -> VmResult<Gc<Object>> {
    let mut data = TargetData::default();
    data.port = Some(Box::new(PortState {
        id,
        ports: Rc::clone(ports),
        detached: Cell::new(false),
    }));
    let data = rt.alloc_host_data(data)?;
    rt.new_host_object(realm, g::MESSAGE_PORT, data)
}

/// Queues the task of port `id` if it has a message and no task, in its home. A home
/// that is another runtime gets it from the page, between tasks.
fn schedule(rt: &mut Runtime, ports: &Ports, id: u64) -> VmResult<()> {
    let wanted = ports.hub.registry.borrow_mut().want_pump(id);
    let Some(home) = wanted else {
        return Ok(());
    };
    let now = page_now(rt);
    let several = rt.host_flag();
    let state = state_mut(rt)?;
    let seq = state.event_loop.tasks.take_seq();
    if home == ports.tag {
        let document = if several {
            document_of_port(state, ports, id)
        } else {
            state.doc.key
        };
        state
            .event_loop
            .tasks
            .push_for(document, now, seq, Task::PortMessage(id));
    } else {
        ports.hub.post_mail(Mail::port(home, id, now, seq));
    }
    Ok(())
}

/// The document of the realm of the port `id`: its tasks run there, whichever document
/// sent the message (measured: a frame that goes after its message does not take the
/// message with it, case `port-sender-removed-same`).
fn document_of_port(state: &WebState, ports: &Ports, id: u64) -> crate::state::DocKey {
    let realm = ports.objects.borrow().get(&id).map(|&(_, realm)| realm);
    realm.map_or(state.doc.key, |realm| {
        crate::state::document_of_realm(state, realm)
    })
}

/// The task of the port `id` that another runtime asked for: it runs at its place in
/// the order of the page. A port that has moved or closed since has no object here.
pub(crate) fn receive_pump(rt: &mut Runtime, id: u64, due: f64, seq: u64) -> VmResult<()> {
    let state = state_mut(rt)?;
    let ports = Rc::clone(&state.ports);
    if !ports.objects.borrow().contains_key(&id) {
        return Ok(());
    }
    let document = document_of_port(state, &ports, id);
    state
        .event_loop
        .tasks
        .push_for(document, due, seq, Task::PortMessage(id));
    Ok(())
}

// ----- MessageChannel -----

/// The data of a `MessageChannel`.
struct ChannelData {
    port1: Gc<Object>,
    port2: Gc<Object>,
}

impl GenericData for ChannelData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.port1);
        tracer.object(self.port2);
    }

    fn heap_size(&self) -> usize {
        0
    }
}

/// Makes room for one more channel: at the limit, forces a collection
/// (rarely) and counts again.
fn make_room(cx: &mut Cx<'_>, ports: &Rc<Ports>) -> b::Result<()> {
    let hub = &ports.hub;
    hub.reap();
    let since_collect = ports.since_collect.get() + 1;
    ports.since_collect.set(since_collect);
    let force = {
        let mut registry = hub.registry.borrow_mut();
        registry.sweep_if_due();
        // A full queue of waiting messages also asks for a collection and a
        // sweep (garbage may hold the budget), but does not stop the
        // channel.
        let full = ports.budget.ports.get() + 2 > MAX_PORTS
            || ports.budget.waiting.get() + 2 > MAX_WAITING;
        let force = full && since_collect >= COLLECT_EVERY;
        if force {
            ports.since_collect.set(0);
            registry.sweep();
        }
        force
    };
    if force {
        // The sweep before the collection lets a port that only a travelling
        // partner kept alive (`Ports::trace`) go in this collection.
        cx.rt.collect();
        hub.reap();
        hub.registry.borrow_mut().sweep();
    }
    let live = ports.budget.ports.get();
    if live + 2 <= MAX_PORTS {
        ports.warned.set(false);
        return Ok(());
    }
    if !ports.warned.replace(true) {
        log::warn!("MessageChannel: {live} message ports are live; no further channel is made");
    }
    Err(Exception::range_error(format!(
        "Too many message ports ({MAX_PORTS})."
    )))
}

impl g::message_channel::MessageChannelImpl for WebState {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let ports = ports_of(cx.rt)?;
        make_room(cx, &ports)?;
        let realm = cx.realm();
        let (first, second) = {
            let mut registry = ports.hub.registry.borrow_mut();
            (registry.fresh_id(), registry.fresh_id())
        };
        // The objects are in the scope of the call until the channel holds
        // them; the entries come last, with no allocation after them.
        let port1 = new_port_object(cx.rt, &ports, realm, first)?;
        let port2 = new_port_object(cx.rt, &ports, realm, second)?;
        let data = cx.rt.alloc_host_data(ChannelData { port1, port2 })?;
        let channel = cx.new_instance::<WebState>(data)?;
        ports
            .hub
            .registry
            .borrow_mut()
            .insert_pair(first, second, ports.tag, &ports.budget);
        let mut objects = ports.objects.borrow_mut();
        objects.insert(first, (port1, realm));
        objects.insert(second, (port2, realm));
        Ok(channel)
    }

    fn port1(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(channel(cx.rt, this)?.0)
    }

    fn port2(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(channel(cx.rt, this)?.1)
    }
}

/// The two ports of the channel `this`.
fn channel(rt: &Runtime, this: Platform) -> b::Result<(Gc<Object>, Gc<Object>)> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<ChannelData>(handle)
            .map(|data| (data.port1, data.port2))
            .map_err(|_| illegal()),
        HostData::Word(_) => Err(illegal()),
    }
}

// ----- MessagePort -----

impl g::message_port::MessagePortImpl for WebState {
    fn post_message_1(
        cx: &mut Cx<'_>,
        this: Platform,
        message: Value,
        transfer: Vec<Gc<Object>>,
    ) -> b::Result<()> {
        post(cx, this, message, &transfer)
    }

    fn post_message_2(
        cx: &mut Cx<'_>,
        this: Platform,
        message: Value,
        options: StructuredSerializeOptions,
    ) -> b::Result<()> {
        post(cx, this, message, &options.transfer)
    }

    fn start(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        Ok(enable(cx.rt, this.object)?)
    }

    fn close(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let Some(info) = port_info(cx.rt, this.object) else {
            return Err(illegal());
        };
        if !info.detached {
            let ports = ports_of(cx.rt)?;
            ports.hub.reap();
            let mut registry = ports.hub.registry.borrow_mut();
            if registry.entries.contains_key(&info.id) {
                registry.remove(info.id);
                // The partner may travel with nothing else to reach it.
                registry.churn += 1;
                registry.sweep_if_due();
            }
            ports.objects.borrow_mut().remove(&info.id);
        }
        Ok(())
    }
}

/// `postMessage()` of a port (HTML "message port post message steps"). A
/// closed or neutered port does nothing, not even the checks (measured; see
/// `Registry::post_does_nothing`). A port whose partner is closed may still serialize
/// and transfer, and the message is dropped. The message counts for the sender until it
/// is delivered or dropped.
fn post(cx: &mut Cx<'_>, this: Platform, message: Value, transfer: &[Gc<Object>]) -> b::Result<()> {
    let Some(info) = port_info(cx.rt, this.object) else {
        return Err(illegal());
    };
    let ports = ports_of(cx.rt)?;
    ports.hub.reap();
    if info.detached || ports.hub.registry.borrow().post_does_nothing(info.id) {
        return Ok(());
    }
    let record = serialize_for_transfer(cx.rt, message, transfer, Some(this.object))?;
    ports.hub.reap();
    let Some(charge) = ports.admit(record.message_bytes()) else {
        return Ok(());
    };
    // A failure here drops the charge: the message does not wait.
    detach_transfer(cx.rt, transfer)?;
    let target = ports
        .hub
        .registry
        .borrow_mut()
        .route(info.id, Message { record, charge });
    log::trace!("MessagePort.postMessage: port {} to {target:?}", info.id);
    if let Some(target) = target {
        schedule(cx.rt, &ports, target)?;
    }
    Ok(())
}

/// Enables the queue of the port `object` (`start()`, a set `onmessage`)
/// and queues the task for its waiting messages. A neutered or closed port
/// does nothing.
pub(crate) fn enable(rt: &mut Runtime, object: Gc<Object>) -> VmResult<()> {
    let Some(info) = port_info(rt, object) else {
        return Ok(());
    };
    if info.detached {
        return Ok(());
    }
    let ports = ports_of(rt)?;
    ports.hub.reap();
    if let Some(entry) = ports.hub.registry.borrow_mut().entries.get_mut(&info.id) {
        entry.enabled = true;
    }
    schedule(rt, &ports, info.id)
}

/// The setter of `onmessage` or `onmessageerror` on a `MessagePort`: the
/// first assignment of either enables the queue, whatever the value
/// (measured in Chromium 148: `null` and `5` enable it too).
pub(crate) fn handler_assigned(rt: &mut Runtime, target: Gc<Object>, attr: &str) -> VmResult<()> {
    if matches!(attr, "onmessage" | "onmessageerror") && port_info(rt, target).is_some() {
        enable(rt, target)?;
    }
    Ok(())
}

// ----- Transfer -----

/// HTML "transfer steps" of `MessagePort` in the sender: the objects of
/// `ports` are neutered, and their entries wait for the object that
/// adopts them. A task of the port that is still queued finds no object and
/// does nothing. A closed port has no entry and stays closed.
pub(crate) fn detach_ports(rt: &mut Runtime, list: &[Gc<Object>]) -> VmResult<()> {
    let ports = ports_of(rt)?;
    for &object in list {
        let Some(info) = port_info(rt, object) else {
            continue;
        };
        if let Some((_, HostData::Generic(handle))) = rt.host_object(object.into())
            && let Ok(data) = rt.heap().generic::<TargetData>(handle)
            && let Some(port) = &data.port
        {
            port.detached.set(true);
        }
        ports.objects.borrow_mut().remove(&info.id);
        if let Some(entry) = ports.hub.registry.borrow_mut().entries.get_mut(&info.id) {
            entry.home = None;
            entry.enabled = false;
            entry.pumps = 0;
        }
    }
    Ok(())
}

/// HTML "transfer-receiving steps": a new object in `realm` adopts the
/// port `id` (queue not enabled). If the port was closed in the meantime
/// the object is a closed port.
pub(crate) fn adopt(rt: &mut Runtime, realm: u32, id: u64) -> VmResult<Gc<Object>> {
    let ports = ports_of(rt)?;
    ports.hub.reap();
    let object = new_port_object(rt, &ports, realm, id)?;
    if let Some(entry) = ports.hub.registry.borrow_mut().entries.get_mut(&id) {
        entry.home = Some(ports.tag);
        ports.objects.borrow_mut().insert(id, (object, realm));
    }
    Ok(object)
}

/// Closes the ports `ids` of a record that is not delivered (a message that
/// was dropped, a failed clone). A port that is already adopted is left.
pub(crate) fn release_ports(rt: &Runtime, ids: &[u64]) {
    let Ok(ports) = ports_of(rt) else {
        return;
    };
    ports.hub.reap();
    let mut registry = ports.hub.registry.borrow_mut();
    for id in registry.travelling(ids) {
        registry.remove(id);
    }
}

// ----- Delivery -----

/// The task of a port ([`Task::PortMessage`]): fires `message` for the next
/// waiting message, then queues the task for the one after it.
pub(crate) fn deliver(rt: &mut Runtime, id: u64) -> VmResult<()> {
    let ports = ports_of(rt)?;
    ports.hub.reap();
    let object = ports.objects.borrow().get(&id).copied();
    let Some((object, realm)) = object else {
        return Ok(());
    };
    let next = ports.hub.registry.borrow_mut().take_message(id, ports.tag);
    let result = match next {
        Some(Message { record, mut charge }) => {
            charge.settle();
            fire(rt, object, realm, &record)
        }
        None => Ok(()),
    };
    if let Some(entry) = ports.hub.registry.borrow_mut().entries.get_mut(&id) {
        entry.pumps = entry.pumps.saturating_sub(1);
    }
    result?;
    schedule(rt, &ports, id)
}

/// Fires `message` (or `messageerror`) at the port `object`.
fn fire(rt: &mut Runtime, object: Gc<Object>, realm: u32, record: &Serialized) -> VmResult<()> {
    let received = crate::messaging::receive_message(rt, realm, record, |rt| {
        // HTML "message port post message steps": the data cannot be
        // deserialized. Ports that it adopted before that close too.
        release_ports(rt, record.ports());
        ports_of(rt)?.hub.doom(record.ports());
        Ok(())
    })?;
    crate::messaging::fire_message(rt, object, received, String::new(), None)
}
