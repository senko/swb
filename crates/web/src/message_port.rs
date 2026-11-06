//! `MessageChannel` and `MessagePort` (HTML §9.5,
//! <https://html.spec.whatwg.org/multipage/web-messaging.html#message-channels>;
//! ADR 0031 section 7, M9 feature 9), within one document.
//!
//! **Registry.** Each port has an id (a number that is never reused) and an
//! [`Entry`] in the [`Registry`]: its entangled port, its queue of waiting
//! messages (serialization records, [`Serialized`]), whether the queue is
//! enabled, and the port object that carries the id (`None` while the port
//! travels in a message). The port object is a host object of its realm;
//! its [`PortState`] (in the generic data of the event target) holds the id
//! and a share of the registry. A transfer detaches the sender's object
//! ("neutered": every call does nothing) and a new object adopts the id in
//! the receiver's realm; the queue stays in the entry, so it moves with the
//! port. A closed port has no entry.
//!
//! **Lifetime.** The entries are outside the JavaScript heap, so the
//! collector and the registry cooperate:
//!
//! - A port holds its entangled port strongly ([`PortState::trace`]; the
//!   rule of HTML "Ports and garbage collection"). A pair that script no
//!   longer reaches is collected together, also with listeners on both ends
//!   (measured in Chromium: all four ports of two such pairs are finalized).
//! - A port object that the collector frees drops its [`PortState`]; the
//!   drop puts the id on a list ([`Shared::dead`]) and the next operation of
//!   the registry removes the entry ([`Shared::reap`]) with its queue; the
//!   partner is disentangled and the ports that travel in the dropped
//!   messages are closed. So the memory of the registry follows the
//!   collector.
//! - A port stays alive while it has a task in the event loop, and while its
//!   queue is enabled and its entangled port travels in a message (the
//!   spec's "must not be garbage collected while there is a message in a
//!   task queue ... and its entangled port could send"): [`Shared::trace`].
//! - `close()` removes the entry at once.
//! - A port that travels has no object, so the collector cannot free it.
//!   [`Registry::sweep`] removes the travelling entries that nothing can
//!   deliver: those that no entry with an object or a task, and no record of
//!   the window queue, reaches through the queued records. A port that
//!   travels only in the queue of a travelling port (`c.port1.postMessage(1,
//!   [c.port2])`) is removed this way. A port with a listener whose partner
//!   travels is a root of the collector ([`Shared::trace`]); the sweep
//!   removes the unreachable partner, and the next collection frees the
//!   port (at most two collections, each followed by a sweep).
//! - At most [`MAX_PORTS`] ports are live (entries). `new MessageChannel()`
//!   past it first forces a collection (a page that makes a channel per
//!   call in a loop, as some `setImmediate` polyfills do, has garbage
//!   entries that the collector has not freed yet; the forced collection
//!   runs at most once per [`COLLECT_EVERY`] channels), then throws a
//!   `RangeError` with a warning.
//!
//! **Tasks.** Chromium delivers a port's messages one per task, and queues
//! the task for the next message after the listener of the last one has
//! run (measured: a timer that the handler of message 1 queues runs before
//! message 2, also when the handler posted message 3 first). [`Entry::pumps`]
//! counts the queued tasks of a port ([`Task::PortMessage`]): at most one
//! unless the port was transferred. The task has no timer clamping and the
//! event loop orders it with the others by queuing time, so timers,
//! rendering and input get their turns in an endless ping-pong.
//!
//! **Queue.** Setting `onmessage` or `onmessageerror` (to any value; measured)
//! enables the queue, `start()` does too; `addEventListener` does not. A
//! received port is not enabled (measured), whatever the sender's state
//! was.
//!
//! **Budget.** The waiting messages of the ports and of `window.postMessage`
//! together are bounded ([`Registry::admit`], ADR 0031 section 11); one
//! document has one runtime, so the count is per runtime.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::rc::Rc;

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value, VmError, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::event_loop::{Task, queue_task};
use crate::events::{Extra, TargetData, fire_at_object, misc::Other};
use crate::generated as g;
use crate::generated::types::StructuredSerializeOptions;
use crate::state::{WebState, state};
use crate::structured::{
    Serialized, deserialize_with_ports, detach_transfer, serialize_for_transfer,
};

/// The most live ports of a document.
pub(crate) const MAX_PORTS: usize = 65_536;

/// The most waiting messages (ports and window together).
pub(crate) const MAX_WAITING: usize = 65_536;

/// The most bytes of serialized messages that wait (256 MiB): in the top level's
/// runtime, and in the runtimes of the page's frames together.
pub(crate) const MAX_BYTES: usize = 256 << 20;

/// At the port limit, a collection is forced after this many new channels
/// since the last forced one.
const COLLECT_EVERY: usize = 1024;

/// A message that waits in the queue of a port.
#[derive(Debug)]
struct Message {
    record: Serialized,
    bytes: usize,
}

/// A port of the registry.
#[derive(Debug)]
struct Entry {
    /// The port object, or `None` while the port travels in a message.
    object: Option<Gc<Object>>,
    /// The realm of the object (the realm of the event).
    realm: u32,
    /// The entangled port, if it is not closed.
    partner: Option<u64>,
    queue: VecDeque<Message>,
    /// HTML "port message queue" enabled.
    enabled: bool,
    /// The tasks of this port in the event loop ([`Task::PortMessage`]).
    pumps: u32,
}

/// The ports of a document and the budget of its waiting messages.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    entries: HashMap<u64, Entry>,
    next_id: u64,
    /// The messages that wait in the queues of ports and in the queue of
    /// `window.postMessage()`.
    waiting: usize,
    waiting_bytes: crate::page_budget::ByteShare,
    /// True after the warning about a full queue, until nothing waits.
    warned: bool,
    /// True after the warning about the port limit, until a channel fits
    /// again.
    warned_ports: bool,
    /// Channels made since the collection that the limit forced.
    since_collect: usize,
    /// The ports in the records of the queue of `window.postMessage()`
    /// (roots of [`Registry::sweep`]).
    window_ports: HashSet<u64>,
    /// Entries removed (closed or collected) since the last sweep.
    churn: usize,
}

/// The registry and the ids of dead port objects, shared by the binding
/// state and the port objects.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    registry: RefCell<Registry>,
    /// Ids of port objects that the collector freed (see [`Shared::reap`]).
    dead: RefCell<Vec<u64>>,
}

impl Shared {
    /// Counts the waiting bytes in the budget of the page's frames.
    pub(crate) fn join_page(&self, page: &Rc<Cell<u64>>) {
        self.registry.borrow_mut().waiting_bytes.join(page);
    }

    /// Removes the entries of the port objects that the collector freed.
    /// Called at the start of each operation, with no borrow held.
    fn reap(&self) {
        let dead = std::mem::take(&mut *self.dead.borrow_mut());
        if dead.is_empty() {
            return;
        }
        let mut registry = self.registry.borrow_mut();
        registry.churn += dead.len();
        for id in dead {
            registry.remove(id);
        }
    }

    /// Reports the port objects that the collector must keep (see the
    /// module comment). The registry is never borrowed while the heap
    /// allocates, so a failed borrow cannot happen; it would only hide
    /// roots.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        let Ok(registry) = self.registry.try_borrow() else {
            return;
        };
        for entry in registry.entries.values() {
            let Some(object) = entry.object else {
                continue;
            };
            let travelling_partner = entry.enabled
                && entry
                    .partner
                    .and_then(|id| registry.entries.get(&id))
                    .is_some_and(|partner| partner.object.is_none());
            if entry.pumps > 0 || travelling_partner {
                tracer.object(object);
            }
        }
    }

    /// The number of live ports (entries).
    pub(crate) fn live(&self) -> usize {
        self.reap();
        let mut registry = self.registry.borrow_mut();
        registry.sweep();
        registry.entries.len()
    }

    /// The number of messages that wait (ports and window together).
    pub(crate) fn waiting(&self) -> usize {
        self.registry.borrow().waiting
    }
}

impl Registry {
    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Removes the entry of `id` (a closed or collected port): the partner
    /// is disentangled, the waiting messages go, and the ports that travel
    /// in them are closed in turn.
    fn remove(&mut self, id: u64) {
        let mut work = vec![id];
        while let Some(id) = work.pop() {
            let Some(entry) = self.entries.remove(&id) else {
                continue;
            };
            if let Some(partner) = entry.partner.and_then(|p| self.entries.get_mut(&p))
                && partner.partner == Some(id)
            {
                partner.partner = None;
            }
            for message in entry.queue {
                self.release(message.bytes);
                work.extend(self.travelling(message.record.ports()));
            }
        }
    }

    /// Removes the travelling entries that nothing can deliver any more
    /// (module comment, "Lifetime"). An entry is reachable if it has a port
    /// object or a queued task, or if it travels in a record of the window
    /// queue or in a record in the queue of a reachable entry. One pass with
    /// a worklist. Call it only where no record is in flight (not between
    /// the serialization of a message and its queuing, and not inside the
    /// adoption of the ports of a record).
    pub(crate) fn sweep(&mut self) {
        self.churn = 0;
        let mut reached: HashSet<u64> = HashSet::new();
        let mut work: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, e)| e.object.is_some() || e.pumps > 0)
            .map(|(&id, _)| id)
            .chain(self.window_ports.iter().copied())
            .collect();
        while let Some(id) = work.pop() {
            if !reached.insert(id) {
                continue;
            }
            let Some(entry) = self.entries.get(&id) else {
                continue;
            };
            for message in &entry.queue {
                work.extend(
                    message
                        .record
                        .ports()
                        .iter()
                        .copied()
                        .filter(|port| !reached.contains(port)),
                );
            }
        }
        let lost: Vec<u64> = self
            .entries
            .keys()
            .copied()
            .filter(|id| !reached.contains(id))
            .collect();
        for id in lost {
            self.remove(id);
        }
    }

    /// [`Registry::sweep`] if enough entries went since the last one that
    /// its cost (the number of entries) is paid for (amortized constant per
    /// removal).
    fn sweep_if_due(&mut self) {
        if self.churn > 0 && self.churn * 4 >= self.entries.len() {
            self.sweep();
        }
    }

    /// The ids among `ports` that travel (they have an entry, no object).
    fn travelling(&self, ports: &[u64]) -> Vec<u64> {
        ports
            .iter()
            .copied()
            .filter(|id| self.entries.get(id).is_some_and(|e| e.object.is_none()))
            .collect()
    }

    /// Counts a message that is about to wait. False (with one warning per
    /// full queue) if a bound is reached: the caller drops the message.
    pub(crate) fn admit(&mut self, bytes: usize) -> bool {
        if self.waiting >= MAX_WAITING || !self.waiting_bytes.fits(bytes as u64, MAX_BYTES as u64) {
            if !self.warned {
                self.warned = true;
                log::warn!(
                    "postMessage: {} messages ({} bytes) wait; further messages are dropped until none waits",
                    self.waiting,
                    self.waiting_bytes.mine()
                );
            }
            return false;
        }
        self.waiting += 1;
        self.waiting_bytes.add(bytes as u64);
        true
    }

    /// Counts a message that no longer waits.
    pub(crate) fn release(&mut self, bytes: usize) {
        self.waiting = self.waiting.saturating_sub(1);
        self.waiting_bytes.remove(bytes as u64);
        if self.waiting == 0 {
            self.warned = false;
        }
    }

    /// True (and counts the task) if the port `id` has a message to deliver
    /// and no task: the caller queues one.
    fn want_pump(&mut self, id: u64) -> bool {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        if entry.enabled && entry.object.is_some() && !entry.queue.is_empty() && entry.pumps == 0 {
            entry.pumps = 1;
            return true;
        }
        false
    }

    /// Entangles two new ports.
    fn insert_pair(&mut self, first: (u64, Gc<Object>), second: (u64, Gc<Object>), realm: u32) {
        for ((id, object), partner) in [(first, second.0), (second, first.0)] {
            self.entries.insert(
                id,
                Entry {
                    object: Some(object),
                    realm,
                    partner: Some(partner),
                    queue: VecDeque::new(),
                    enabled: false,
                    pumps: 0,
                },
            );
        }
    }

    /// Appends the message to the queue of the entangled port of `from`;
    /// returns the id of that port. `None` if the sender has no partner:
    /// the message is dropped, its count is released and the ports in it
    /// are closed.
    fn route(&mut self, from: u64, record: Serialized, bytes: usize) -> Option<u64> {
        let partner = self.entries.get(&from).and_then(|e| e.partner);
        let target = partner.filter(|p| self.entries.contains_key(p));
        let Some(target) = target else {
            self.release(bytes);
            for id in self.travelling(record.ports()) {
                self.remove(id);
            }
            return None;
        };
        if let Some(entry) = self.entries.get_mut(&target) {
            entry.queue.push_back(Message { record, bytes });
        }
        Some(target)
    }

    /// Takes the next message of `id` if its task can deliver it.
    fn take_message(&mut self, id: u64) -> Option<(Gc<Object>, u32, Message)> {
        let entry = self.entries.get_mut(&id)?;
        let object = entry.object.filter(|_| entry.enabled)?;
        let realm = entry.realm;
        let message = entry.queue.pop_front()?;
        self.release(message.bytes);
        Some((object, realm, message))
    }
}

/// The state of a `MessagePort` object, in the data of its event target.
pub(crate) struct PortState {
    id: u64,
    shared: Rc<Shared>,
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
    /// A port holds its entangled port (module comment, "Lifetime").
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if self.detached.get() {
            return;
        }
        let Ok(registry) = self.shared.registry.try_borrow() else {
            return;
        };
        let partner = registry
            .entries
            .get(&self.id)
            .and_then(|e| e.partner)
            .and_then(|id| registry.entries.get(&id))
            .and_then(|e| e.object);
        if let Some(object) = partner {
            tracer.object(object);
        }
    }
}

impl Drop for PortState {
    fn drop(&mut self) {
        if !self.detached.get() {
            self.shared.dead.borrow_mut().push(self.id);
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

fn shared_of(rt: &Runtime) -> VmResult<Rc<Shared>> {
    Ok(Rc::clone(&state(rt)?.ports))
}

fn illegal() -> Exception {
    Exception::type_error("Illegal invocation")
}

/// A new `MessagePort` object for `id` in `realm`.
fn new_port_object(
    rt: &mut Runtime,
    shared: &Rc<Shared>,
    realm: u32,
    id: u64,
) -> VmResult<Gc<Object>> {
    let mut data = TargetData::default();
    data.port = Some(Box::new(PortState {
        id,
        shared: Rc::clone(shared),
        detached: Cell::new(false),
    }));
    let data = rt.alloc_host_data(data)?;
    rt.new_host_object(realm, g::MESSAGE_PORT, data)
}

/// Queues the task of port `id` if it has a message and no task.
fn schedule(rt: &mut Runtime, shared: &Shared, id: u64) -> VmResult<()> {
    let wanted = shared.registry.borrow_mut().want_pump(id);
    if wanted {
        queue_task(rt, Task::PortMessage(id))?;
    }
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
fn make_room(cx: &mut Cx<'_>, shared: &Rc<Shared>) -> b::Result<()> {
    shared.reap();
    let force = {
        let mut registry = shared.registry.borrow_mut();
        registry.since_collect += 1;
        registry.sweep_if_due();
        // A full queue of waiting messages also asks for a collection and a
        // sweep (garbage may hold the budget), but does not stop the
        // channel.
        let full = registry.entries.len() + 2 > MAX_PORTS || registry.waiting + 2 > MAX_WAITING;
        let force = full && registry.since_collect >= COLLECT_EVERY;
        if force {
            registry.since_collect = 0;
            registry.sweep();
        }
        force
    };
    if force {
        // The sweep before the collection lets a port that only a travelling
        // partner kept alive (`Shared::trace`) go in this collection.
        cx.rt.collect();
        shared.reap();
        shared.registry.borrow_mut().sweep();
    }
    let (live, warned) = {
        let mut registry = shared.registry.borrow_mut();
        let warned = std::mem::replace(&mut registry.warned_ports, true);
        (registry.entries.len(), warned)
    };
    if live + 2 <= MAX_PORTS {
        shared.registry.borrow_mut().warned_ports = false;
        return Ok(());
    }
    if !warned {
        log::warn!("MessageChannel: {live} message ports are live; no further channel is made");
    }
    Err(Exception::range_error(format!(
        "Too many message ports ({MAX_PORTS})."
    )))
}

impl g::message_channel::MessageChannelImpl for WebState {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let shared = shared_of(cx.rt)?;
        make_room(cx, &shared)?;
        let realm = cx.realm();
        let (first, second) = {
            let mut registry = shared.registry.borrow_mut();
            (registry.fresh_id(), registry.fresh_id())
        };
        // The objects are in the scope of the call until the channel holds
        // them; the entries come last, with no allocation after them.
        let port1 = new_port_object(cx.rt, &shared, realm, first)?;
        let port2 = new_port_object(cx.rt, &shared, realm, second)?;
        let data = cx.rt.alloc_host_data(ChannelData { port1, port2 })?;
        let channel = cx.new_instance::<WebState>(data)?;
        shared
            .registry
            .borrow_mut()
            .insert_pair((first, port1), (second, port2), realm);
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
            let shared = shared_of(cx.rt)?;
            shared.reap();
            let mut registry = shared.registry.borrow_mut();
            if registry.entries.contains_key(&info.id) {
                registry.remove(info.id);
                // The partner may travel with nothing else to reach it.
                registry.churn += 1;
                registry.sweep_if_due();
            }
        }
        Ok(())
    }
}

/// `postMessage()` of a port (HTML "message port post message steps"). A
/// closed or neutered port does nothing, not even the checks (measured). A
/// port whose partner is closed still serializes and transfers, and the
/// message is dropped.
fn post(cx: &mut Cx<'_>, this: Platform, message: Value, transfer: &[Gc<Object>]) -> b::Result<()> {
    let Some(info) = port_info(cx.rt, this.object) else {
        return Err(illegal());
    };
    let shared = shared_of(cx.rt)?;
    shared.reap();
    if info.detached || !shared.registry.borrow().entries.contains_key(&info.id) {
        return Ok(());
    }
    let record = serialize_for_transfer(cx.rt, message, transfer, Some(this.object))?;
    shared.reap();
    let bytes = record.byte_size();
    if !shared.registry.borrow_mut().admit(bytes) {
        return Ok(());
    }
    if let Err(error) = detach_transfer(cx.rt, transfer) {
        // The message does not wait: give its count back.
        shared.registry.borrow_mut().release(bytes);
        return Err(error);
    }
    let target = shared.registry.borrow_mut().route(info.id, record, bytes);
    log::trace!("MessagePort.postMessage: port {} to {target:?}", info.id);
    if let Some(target) = target {
        schedule(cx.rt, &shared, target)?;
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
    let shared = shared_of(rt)?;
    shared.reap();
    if let Some(entry) = shared.registry.borrow_mut().entries.get_mut(&info.id) {
        entry.enabled = true;
    }
    schedule(rt, &shared, info.id)
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
/// adopts them. A closed port has no entry and stays closed.
pub(crate) fn detach_ports(rt: &mut Runtime, ports: &[Gc<Object>]) -> VmResult<()> {
    let shared = shared_of(rt)?;
    for &object in ports {
        let Some(info) = port_info(rt, object) else {
            continue;
        };
        if let Some((_, HostData::Generic(handle))) = rt.host_object(object.into())
            && let Ok(data) = rt.heap().generic::<TargetData>(handle)
            && let Some(port) = &data.port
        {
            port.detached.set(true);
        }
        if let Some(entry) = shared.registry.borrow_mut().entries.get_mut(&info.id) {
            entry.object = None;
            entry.enabled = false;
        }
    }
    Ok(())
}

/// HTML "transfer-receiving steps": a new object in `realm` adopts the
/// port `id` (queue not enabled). If the port was closed in the meantime
/// the object is a closed port.
pub(crate) fn adopt(rt: &mut Runtime, realm: u32, id: u64) -> VmResult<Gc<Object>> {
    let shared = shared_of(rt)?;
    shared.reap();
    let object = new_port_object(rt, &shared, realm, id)?;
    if let Some(entry) = shared.registry.borrow_mut().entries.get_mut(&id) {
        entry.object = Some(object);
        entry.realm = realm;
    }
    Ok(object)
}

/// Closes the ports `ids` of a record that is not delivered (a message that
/// was dropped, a failed clone). A port that is already adopted is left.
pub(crate) fn release_ports(rt: &Runtime, ids: &[u64]) {
    let Ok(shared) = shared_of(rt) else {
        return;
    };
    shared.reap();
    let mut registry = shared.registry.borrow_mut();
    for id in registry.travelling(ids) {
        registry.remove(id);
    }
}

// ----- Budget (shared with window.postMessage) -----

/// Counts a message of `bytes` that is about to wait; false if the budget
/// is full (the caller drops the message).
pub(crate) fn admit(rt: &Runtime, bytes: usize) -> VmResult<bool> {
    let shared = shared_of(rt)?;
    let admitted = shared.registry.borrow_mut().admit(bytes);
    Ok(admitted)
}

/// The ports `ids` travel in a record of the window's queue (a root of the
/// sweep) until [`unhold_for_window`].
pub(crate) fn hold_for_window(rt: &Runtime, ids: &[u64]) {
    if let Ok(shared) = shared_of(rt) {
        shared
            .registry
            .borrow_mut()
            .window_ports
            .extend(ids.iter().copied());
    }
}

/// The record of the window's queue that carried the ports `ids` left the
/// queue (the caller adopts or closes the ports).
pub(crate) fn unhold_for_window(rt: &Runtime, ids: &[u64]) {
    if let Ok(shared) = shared_of(rt) {
        let mut registry = shared.registry.borrow_mut();
        for id in ids {
            registry.window_ports.remove(id);
        }
    }
}

/// Counts a message that no longer waits.
pub(crate) fn release(rt: &Runtime, bytes: usize) {
    if let Ok(shared) = shared_of(rt) {
        shared.registry.borrow_mut().release(bytes);
    }
}

// ----- Delivery -----

/// The task of a port ([`Task::PortMessage`]): fires `message` for the next
/// waiting message, then queues the task for the one after it.
pub(crate) fn deliver(rt: &mut Runtime, id: u64) -> VmResult<()> {
    let shared = shared_of(rt)?;
    shared.reap();
    let next = shared.registry.borrow_mut().take_message(id);
    let result = match next {
        Some((object, realm, message)) => fire(rt, object, realm, &message.record),
        None => Ok(()),
    };
    if let Some(entry) = shared.registry.borrow_mut().entries.get_mut(&id) {
        entry.pumps = entry.pumps.saturating_sub(1);
    }
    result?;
    schedule(rt, &shared, id)
}

/// Fires `message` (or `messageerror`) at the port `object`.
fn fire(rt: &mut Runtime, object: Gc<Object>, realm: u32, record: &Serialized) -> VmResult<()> {
    let (kind, other) = match deserialize_with_ports(rt, realm, record) {
        Ok((data, ports)) => {
            b::keep(rt, data);
            (
                "message",
                Other::Message {
                    data,
                    origin: String::new(),
                    last_event_id: String::new(),
                    source: None,
                    ports,
                    frozen: None,
                },
            )
        }
        Err(Exception::Js(error @ (VmError::Terminated(_) | VmError::Internal(_)))) => {
            return Err(error);
        }
        Err(_) => {
            // HTML "message port post message steps": the data cannot be
            // deserialized.
            release_ports(rt, record.ports());
            (
                "messageerror",
                Other::Message {
                    data: Value::Null,
                    origin: String::new(),
                    last_event_id: String::new(),
                    source: None,
                    ports: Vec::new(),
                    frozen: None,
                },
            )
        }
    };
    fire_at_object(
        rt,
        object,
        g::MESSAGE_EVENT,
        kind,
        false,
        Extra::Other(Box::new(other)),
    )
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record that carries the ports `ids` and nothing else.
    fn record_with(ids: &[u64]) -> Serialized {
        Serialized::null().with_ports(ids.to_vec())
    }

    /// An entry without an object, as a port that travels.
    fn travelling_entry(partner: Option<u64>) -> Entry {
        Entry {
            object: None,
            realm: 0,
            partner,
            queue: VecDeque::new(),
            enabled: false,
            pumps: 0,
        }
    }

    #[test]
    fn removal_disentangles_the_partner_and_frees_the_queue() {
        let mut registry = Registry::default();
        registry.entries.insert(1, travelling_entry(Some(2)));
        registry.entries.insert(2, travelling_entry(Some(1)));
        assert!(registry.admit(10));
        registry
            .entries
            .get_mut(&2)
            .unwrap()
            .queue
            .push_back(Message {
                record: record_with(&[]),
                bytes: 10,
            });
        registry.remove(2);
        assert!(!registry.entries.contains_key(&2));
        assert_eq!(registry.entries[&1].partner, None);
        assert_eq!((registry.waiting, registry.waiting_bytes.mine()), (0, 0));
    }

    #[test]
    fn removal_closes_the_ports_in_the_dropped_messages() {
        let mut registry = Registry::default();
        // Port 3 travels in a message that waits in the queue of 2; its
        // partner 4 is a port with an object (here: also travelling, to
        // keep the test free of the heap), and the pair 1 and 2 is closed.
        registry.entries.insert(1, travelling_entry(Some(2)));
        registry.entries.insert(2, travelling_entry(Some(1)));
        registry.entries.insert(3, travelling_entry(Some(4)));
        registry.entries.insert(4, travelling_entry(Some(3)));
        assert!(registry.admit(5));
        registry
            .entries
            .get_mut(&2)
            .unwrap()
            .queue
            .push_back(Message {
                record: record_with(&[3]),
                bytes: 5,
            });
        registry.remove(1);
        registry.remove(2);
        assert!(!registry.entries.contains_key(&3));
        assert_eq!(registry.entries[&4].partner, None);
        assert_eq!(registry.waiting, 0);
    }

    #[test]
    fn a_message_to_a_closed_port_is_dropped_with_its_ports() {
        let mut registry = Registry::default();
        registry.entries.insert(1, travelling_entry(None));
        registry.entries.insert(3, travelling_entry(None));
        assert!(registry.admit(7));
        assert_eq!(registry.route(1, record_with(&[3]), 7), None);
        assert!(!registry.entries.contains_key(&3));
        assert_eq!(registry.waiting, 0);
    }

    #[test]
    fn a_message_goes_to_the_queue_of_the_partner() {
        let mut registry = Registry::default();
        registry.entries.insert(1, travelling_entry(Some(2)));
        registry.entries.insert(2, travelling_entry(Some(1)));
        assert!(registry.admit(1));
        assert_eq!(registry.route(1, record_with(&[]), 1), Some(2));
        assert_eq!(registry.entries[&2].queue.len(), 1);
        assert!(registry.entries[&1].queue.is_empty());
        // No object and not enabled: no task is wanted.
        assert!(!registry.want_pump(2));
    }

    #[test]
    fn the_budget_bounds_messages_and_bytes_and_warns_once() {
        let mut registry = Registry::default();
        for _ in 0..MAX_WAITING {
            assert!(registry.admit(1));
        }
        assert!(!registry.admit(1));
        assert!(registry.warned);
        registry.release(1);
        assert!(registry.admit(1));
        let mut registry = Registry::default();
        assert!(registry.admit(MAX_BYTES));
        assert!(!registry.admit(1));
        registry.release(MAX_BYTES);
        assert!(!registry.warned);
        assert!(registry.admit(MAX_BYTES));
    }

    /// A message with `ports` in the queue of `id`, counted in the budget.
    fn queue_message(registry: &mut Registry, id: u64, ports: &[u64]) {
        assert!(registry.admit(3));
        registry
            .entries
            .get_mut(&id)
            .unwrap()
            .queue
            .push_back(Message {
                record: record_with(ports),
                bytes: 3,
            });
    }

    #[test]
    fn a_sweep_frees_a_port_that_only_its_own_queue_holds() {
        let mut registry = Registry::default();
        registry.entries.insert(1, travelling_entry(None));
        queue_message(&mut registry, 1, &[1]);
        registry.sweep();
        assert!(registry.entries.is_empty());
        assert_eq!((registry.waiting, registry.waiting_bytes.mine()), (0, 0));
    }

    #[test]
    fn a_sweep_frees_a_cycle_of_travelling_ports() {
        let mut registry = Registry::default();
        registry.entries.insert(1, travelling_entry(None));
        registry.entries.insert(2, travelling_entry(None));
        queue_message(&mut registry, 1, &[2]);
        queue_message(&mut registry, 2, &[1]);
        registry.sweep();
        assert!(registry.entries.is_empty());
        assert_eq!(registry.waiting, 0);
    }

    #[test]
    fn a_sweep_keeps_what_a_root_reaches() {
        let mut registry = Registry::default();
        // 1 has a task (root); its queue carries 2, whose queue carries 3.
        // 4 travels in the window queue. 5 is garbage.
        for id in 1..=5 {
            registry.entries.insert(id, travelling_entry(None));
        }
        registry.entries.get_mut(&1).unwrap().pumps = 1;
        queue_message(&mut registry, 1, &[2]);
        queue_message(&mut registry, 2, &[3]);
        registry.window_ports.insert(4);
        registry.sweep();
        let mut kept: Vec<u64> = registry.entries.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, [1, 2, 3, 4]);
        assert_eq!(registry.waiting, 2);
    }

    #[test]
    fn a_sweep_is_due_when_the_churn_pays_for_it() {
        let mut registry = Registry::default();
        for id in 1..=40 {
            registry.entries.insert(id, travelling_entry(None));
        }
        registry.churn = 5;
        registry.sweep_if_due();
        assert_eq!(registry.entries.len(), 40, "5 * 4 < 40: not due");
        registry.churn = 10;
        registry.sweep_if_due();
        assert!(registry.entries.is_empty());
        assert_eq!(registry.churn, 0);
    }

    #[test]
    fn ids_are_never_reused() {
        let mut registry = Registry::default();
        let first = registry.fresh_id();
        registry.entries.insert(first, travelling_entry(None));
        registry.remove(first);
        assert_ne!(registry.fresh_id(), first);
    }
}
