//! The plain data of the message ports (ADR 0031 sections 7 and 11): the limits,
//! the byte budget of the waiting messages ([`MessageBudget`], [`Charge`]), the
//! entries of the ports and their queues ([`Registry`]) and the hub that the
//! runtimes of a page share ([`PortHub`]). Nothing here holds a `Runtime` or an
//! object; `super` has the port objects and the natives.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

use crate::messaging::Mail;
use crate::page_budget::ByteShare;
use crate::structured::Serialized;

/// The most live ports that one runtime has made.
pub(crate) const MAX_PORTS: usize = 65_536;

/// The most waiting messages that one runtime has sent (ports and window together).
pub(crate) const MAX_WAITING: usize = 65_536;

/// The most bytes of serialized messages that wait (256 MiB): for the top level's
/// runtime, and for the runtimes of the page's frames together.
pub(crate) const MAX_BYTES: usize = 256 << 20;

/// What the budget counts for a waiting message besides its record: the message, its
/// origin text, its task and their table entries (measured: about 430 bytes). So messages
/// without a payload also reach [`MAX_BYTES`].
pub(super) const WAITING_OVERHEAD: usize = 512;

/// At the port limit, a collection is forced after this many new channels
/// since the last forced one.
pub(super) const COLLECT_EVERY: usize = 1024;

/// The tag of a runtime in its page's hub. It means nothing outside the hub.
pub(crate) type RuntimeTag = u32;

/// A hasher for the ids of ports, which are consecutive numbers: one multiplication
/// that spreads them over the bits that the table looks at.
#[derive(Debug, Default)]
pub(crate) struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

pub(super) type IdBuild = BuildHasherDefault<IdHasher>;

// ----- The budget -----

/// What one runtime has sent or made and what is still there: the messages that wait
/// (with the `storage` events that it wrote, `storage_events.rs`) and the live ports. The
/// counts live as long as something counts in them, which can be longer than the runtime:
/// a message that waits in another runtime stays counted until it is delivered or dropped.
#[derive(Debug, Default)]
pub(crate) struct MessageBudget {
    pub(super) waiting: Cell<usize>,
    pub(super) bytes: RefCell<ByteShare>,
    /// True after the warning about a full queue, until nothing waits.
    pub(super) warned: Cell<bool>,
    /// The live ports that this runtime made.
    pub(super) ports: Cell<usize>,
}

impl MessageBudget {
    /// Counts a message that is about to wait. False (with one warning per full queue)
    /// if a bound is reached: the caller drops the message.
    #[cfg(test)]
    pub(super) fn admit(&self, bytes: usize) -> bool {
        self.admit_as(bytes, "message")
    }

    /// [`MessageBudget::admit`] for something else that waits in the same budget (`what`
    /// names it in the warning).
    pub(super) fn admit_as(&self, bytes: usize, what: &str) -> bool {
        let mut share = self.bytes.borrow_mut();
        if self.waiting.get() >= MAX_WAITING || !share.fits(bytes as u64, MAX_BYTES as u64) {
            if !self.warned.replace(true) {
                log::warn!(
                    "{} messages ({} bytes) wait; further {what}s are dropped until none waits",
                    self.waiting.get(),
                    share.mine()
                );
            }
            return false;
        }
        self.waiting.set(self.waiting.get() + 1);
        share.add(bytes as u64);
        true
    }

    /// Counts a message that no longer waits.
    pub(super) fn release(&self, bytes: usize) {
        self.waiting.set(self.waiting.get().saturating_sub(1));
        self.bytes.borrow_mut().remove(bytes as u64);
        if self.waiting.get() == 0 {
            self.warned.set(false);
        }
    }
}

/// What one message holds in the budget of its sender. Its drop gives it back, so that
/// no path can leave it counted.
#[derive(Debug)]
pub(crate) struct Charge {
    pub(super) budget: Rc<MessageBudget>,
    pub(super) bytes: usize,
    pub(super) settled: bool,
}

impl Charge {
    /// The message no longer waits.
    pub(crate) fn settle(&mut self) {
        if !self.settled {
            self.settled = true;
            self.budget.release(self.bytes);
        }
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.settle();
    }
}

// ----- The hub -----

/// A message that waits in the queue of a port.
#[derive(Debug)]
pub(super) struct Message {
    pub(super) record: Serialized,
    pub(super) charge: Charge,
}

/// A port of the hub. See the module comment.
#[derive(Debug)]
pub(super) struct Entry {
    /// The runtime that holds the port object, or `None` while the port travels in a
    /// message.
    pub(super) home: Option<RuntimeTag>,
    /// The entangled port, if it is not closed.
    pub(super) partner: Option<u64>,
    pub(super) queue: VecDeque<Message>,
    /// HTML "port message queue" enabled.
    pub(super) enabled: bool,
    /// The tasks of this port in the event loop of its home ([`Task::PortMessage`]).
    pub(super) pumps: u32,
    /// The task in which the partner closed ([`Registry::task`]).
    pub(super) partner_closed_in: u64,
    /// The budget of the runtime that made the port.
    pub(super) maker: Rc<MessageBudget>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.maker
            .ports
            .set(self.maker.ports.get().saturating_sub(1));
    }
}

/// The ports of a page.
#[derive(Debug, Default)]
pub(super) struct Registry {
    pub(super) entries: HashMap<u64, Entry, IdBuild>,
    pub(super) next_id: u64,
    /// The ports in the records of the window messages that wait (roots of
    /// [`Registry::sweep`]).
    pub(super) window_ports: HashSet<u64, IdBuild>,
    /// Entries removed (closed or collected) since the last sweep.
    pub(super) churn: usize,
    /// The number of the running task of the page.
    pub(super) task: u64,
}

impl Registry {
    pub(super) fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Removes the entry of `id` (a closed or collected port): the partner is
    /// disentangled, the waiting messages go, and the ports that travel in them are
    /// closed in turn. A worklist: chains of ports in queues can be long.
    pub(super) fn remove(&mut self, id: u64) {
        let mut work = vec![id];
        while let Some(id) = work.pop() {
            self.window_ports.remove(&id);
            let Some(mut entry) = self.entries.remove(&id) else {
                continue;
            };
            if let Some(partner) = entry.partner.and_then(|p| self.entries.get_mut(&p))
                && partner.partner == Some(id)
            {
                partner.partner = None;
                partner.partner_closed_in = self.task;
            }
            for message in std::mem::take(&mut entry.queue) {
                work.extend(self.travelling(message.record.ports()));
            }
        }
    }

    /// Removes the travelling entries that nothing can deliver any more (module
    /// comment, "Lifetime"). An entry is reachable if it has a port object or a queued
    /// task, or if it travels in a window message that waits or in a record in the
    /// queue of a reachable entry. One pass with a worklist. Call it only where no
    /// record is in flight (not between the serialization of a message and its queuing,
    /// and not inside the adoption of the ports of a record).
    pub(super) fn sweep(&mut self) {
        self.churn = 0;
        let mut reached: HashSet<u64, IdBuild> = HashSet::default();
        let mut work: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, e)| e.home.is_some() || e.pumps > 0)
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

    /// [`Registry::sweep`] if enough entries went since the last one that its cost (the
    /// number of entries) is paid for (amortized constant per removal).
    pub(super) fn sweep_if_due(&mut self) {
        if self.churn > 0 && self.churn * 4 >= self.entries.len() {
            self.sweep();
        }
    }

    /// The ids among `ports` that travel (they have an entry, no home).
    pub(super) fn travelling(&self, ports: &[u64]) -> Vec<u64> {
        ports
            .iter()
            .copied()
            .filter(|id| self.entries.get(id).is_some_and(|e| e.home.is_none()))
            .collect()
    }

    /// The home of the port `id` if it has a message to deliver and no task: the caller
    /// queues one, which this counts.
    pub(super) fn want_pump(&mut self, id: u64) -> Option<RuntimeTag> {
        let entry = self.entries.get_mut(&id)?;
        let home = entry.home?;
        if entry.enabled && !entry.queue.is_empty() && entry.pumps == 0 {
            entry.pumps = 1;
            return Some(home);
        }
        None
    }

    /// True if `postMessage()` of the port `id` does nothing, not even the checks: the
    /// port is closed, or it has started and saw in an earlier task that its partner
    /// closed (measured, cases `port-partner-closed-*`: in the same task, and for a port
    /// that has not started at any time, the call serializes and transfers).
    pub(super) fn post_does_nothing(&self, id: u64) -> bool {
        self.entries.get(&id).is_none_or(|entry| {
            entry.enabled && entry.partner.is_none() && entry.partner_closed_in < self.task
        })
    }

    /// Entangles two new ports of the runtime `home`.
    pub(super) fn insert_pair(
        &mut self,
        first: u64,
        second: u64,
        home: RuntimeTag,
        maker: &Rc<MessageBudget>,
    ) {
        maker.ports.set(maker.ports.get() + 2);
        for (id, partner) in [(first, second), (second, first)] {
            self.entries.insert(
                id,
                Entry {
                    home: Some(home),
                    partner: Some(partner),
                    queue: VecDeque::new(),
                    enabled: false,
                    pumps: 0,
                    partner_closed_in: 0,
                    maker: Rc::clone(maker),
                },
            );
        }
    }

    /// Appends the message to the queue of the entangled port of `from`; returns the id
    /// of that port. `None` if the sender has no partner: the message is dropped, its
    /// count is released and the ports in it are closed.
    pub(super) fn route(&mut self, from: u64, message: Message) -> Option<u64> {
        let partner = self.entries.get(&from).and_then(|e| e.partner);
        let target = partner.filter(|p| self.entries.contains_key(p));
        let Some(target) = target else {
            for id in self.travelling(message.record.ports()) {
                self.remove(id);
            }
            return None;
        };
        if let Some(entry) = self.entries.get_mut(&target) {
            entry.queue.push_back(message);
        }
        Some(target)
    }

    /// Takes the next message of `id` if a task of the runtime `home` can deliver it.
    pub(super) fn take_message(&mut self, id: u64, home: RuntimeTag) -> Option<Message> {
        let entry = self.entries.get_mut(&id)?;
        if !entry.enabled || entry.home != Some(home) {
            return None;
        }
        entry.queue.pop_front()
    }
}

/// What the runtimes of a page share about messages: plain data, which holds nothing
/// that belongs to one runtime.
#[derive(Debug, Default)]
pub(crate) struct PortHub {
    pub(super) registry: RefCell<Registry>,
    /// Ports to remove: their objects were freed, or a record that held them was
    /// dropped ([`PortHub::reap`]).
    pub(super) doomed: RefCell<Vec<u64>>,
    /// What the page has to hand to other runtimes.
    pub(super) mail: RefCell<VecDeque<Mail>>,
    /// True if [`PortHub::mail`] has something: the test of each task of a runtime.
    pub(super) has_mail: Cell<bool>,
    pub(super) next_tag: Cell<RuntimeTag>,
}

impl PortHub {
    /// A task of a runtime of the page starts. What the last one left to remove belongs
    /// to that task.
    #[inline]
    pub(crate) fn next_task(&self) {
        self.reap();
        self.registry.borrow_mut().task += 1;
    }

    /// The ports `ids` will be removed by the next operation of the hub.
    pub(crate) fn doom(&self, ids: &[u64]) {
        self.doomed.borrow_mut().extend_from_slice(ids);
    }

    /// Removes the entries of the ports on the list. Called at the start of each
    /// operation, with no borrow held.
    pub(crate) fn reap(&self) {
        let doomed = std::mem::take(&mut *self.doomed.borrow_mut());
        if doomed.is_empty() {
            return;
        }
        let mut registry = self.registry.borrow_mut();
        registry.churn += doomed.len();
        for id in doomed {
            registry.remove(id);
        }
    }

    /// Leaves `mail` for the page.
    pub(crate) fn post_mail(&self, mail: Mail) {
        self.mail.borrow_mut().push_back(mail);
        self.has_mail.set(true);
    }

    /// True if the page has something to hand over.
    #[inline]
    pub(crate) fn has_mail(&self) -> bool {
        self.has_mail.get()
    }

    /// What the page has to hand over, in the order of the calls.
    pub(crate) fn take_mail(&self) -> VecDeque<Mail> {
        self.has_mail.set(false);
        std::mem::take(&mut *self.mail.borrow_mut())
    }

    /// The ports `ids` travel in a window message that waits (a root of the sweep) until
    /// it is delivered or dropped.
    pub(crate) fn hold_for_window(&self, ids: &[u64]) {
        if !ids.is_empty() {
            self.registry
                .borrow_mut()
                .window_ports
                .extend(ids.iter().copied());
        }
    }

    /// The window message that carried the ports `ids` left its queue: the caller adopts
    /// or closes them.
    pub(crate) fn unhold_for_window(&self, ids: &[u64]) {
        if !ids.is_empty() {
            let mut registry = self.registry.borrow_mut();
            for id in ids {
                registry.window_ports.remove(id);
            }
        }
    }

    /// The live ports of the page and the messages in their queues, after the removals
    /// that wait (tests and measurements).
    pub(crate) fn live(&self) -> usize {
        self.reap();
        let mut registry = self.registry.borrow_mut();
        registry.sweep();
        registry.entries.len()
    }
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
        let maker: Rc<MessageBudget> = Rc::default();
        maker.ports.set(1);
        Entry {
            home: None,
            partner,
            queue: VecDeque::new(),
            enabled: false,
            pumps: 0,
            partner_closed_in: 0,
            maker,
        }
    }

    /// A message of `bytes` counted in `budget`.
    fn message(budget: &Rc<MessageBudget>, ports: &[u64], bytes: usize) -> Message {
        assert!(budget.admit(bytes));
        Message {
            record: record_with(ports),
            charge: Charge {
                budget: Rc::clone(budget),
                bytes,
                settled: false,
            },
        }
    }

    #[test]
    fn removal_disentangles_the_partner_and_frees_the_queue() {
        let mut registry = Registry::default();
        let budget: Rc<MessageBudget> = Rc::default();
        registry.entries.insert(1, travelling_entry(Some(2)));
        registry.entries.insert(2, travelling_entry(Some(1)));
        let queued = message(&budget, &[], 10);
        registry
            .entries
            .get_mut(&2)
            .unwrap()
            .queue
            .push_back(queued);
        registry.remove(2);
        assert!(!registry.entries.contains_key(&2));
        assert_eq!(registry.entries[&1].partner, None);
        assert_eq!((budget.waiting.get(), budget.bytes.borrow().mine()), (0, 0));
    }

    #[test]
    fn removal_closes_the_ports_in_the_dropped_messages() {
        let mut registry = Registry::default();
        let budget: Rc<MessageBudget> = Rc::default();
        // Port 3 travels in a message that waits in the queue of 2; its
        // partner 4 is a port with an object (here: also travelling, to
        // keep the test free of the heap), and the pair 1 and 2 is closed.
        registry.entries.insert(1, travelling_entry(Some(2)));
        registry.entries.insert(2, travelling_entry(Some(1)));
        registry.entries.insert(3, travelling_entry(Some(4)));
        registry.entries.insert(4, travelling_entry(Some(3)));
        let queued = message(&budget, &[3], 5);
        registry
            .entries
            .get_mut(&2)
            .unwrap()
            .queue
            .push_back(queued);
        registry.remove(1);
        registry.remove(2);
        assert!(!registry.entries.contains_key(&3));
        assert_eq!(registry.entries[&4].partner, None);
        assert_eq!(budget.waiting.get(), 0);
    }

    #[test]
    fn a_message_to_a_closed_port_is_dropped_with_its_ports() {
        let mut registry = Registry::default();
        let budget: Rc<MessageBudget> = Rc::default();
        registry.entries.insert(1, travelling_entry(None));
        registry.entries.insert(3, travelling_entry(None));
        assert_eq!(registry.route(1, message(&budget, &[3], 7)), None);
        assert!(!registry.entries.contains_key(&3));
        assert_eq!(budget.waiting.get(), 0);
    }

    #[test]
    fn a_message_goes_to_the_queue_of_the_partner() {
        let mut registry = Registry::default();
        let budget: Rc<MessageBudget> = Rc::default();
        registry.entries.insert(1, travelling_entry(Some(2)));
        registry.entries.insert(2, travelling_entry(Some(1)));
        assert_eq!(registry.route(1, message(&budget, &[], 1)), Some(2));
        assert_eq!(registry.entries[&2].queue.len(), 1);
        assert!(registry.entries[&1].queue.is_empty());
        // No home and not enabled: no task is wanted.
        assert_eq!(registry.want_pump(2), None);
    }

    #[test]
    fn the_budget_bounds_messages_and_bytes_and_warns_once() {
        let budget = MessageBudget::default();
        for _ in 0..MAX_WAITING {
            assert!(budget.admit(1));
        }
        assert!(!budget.admit(1));
        assert!(budget.warned.get());
        budget.release(1);
        assert!(budget.admit(1));
        let budget = MessageBudget::default();
        assert!(budget.admit(MAX_BYTES));
        assert!(!budget.admit(1));
        budget.release(MAX_BYTES);
        assert!(!budget.warned.get());
        assert!(budget.admit(MAX_BYTES));
    }

    /// A message with `ports` in the queue of `id`, counted in the budget.
    fn queue_message(registry: &mut Registry, budget: &Rc<MessageBudget>, id: u64, ports: &[u64]) {
        let queued = message(budget, ports, 3);
        registry
            .entries
            .get_mut(&id)
            .unwrap()
            .queue
            .push_back(queued);
    }

    #[test]
    fn a_sweep_frees_a_port_that_only_its_own_queue_holds() {
        let mut registry = Registry::default();
        let budget: Rc<MessageBudget> = Rc::default();
        registry.entries.insert(1, travelling_entry(None));
        queue_message(&mut registry, &budget, 1, &[1]);
        registry.sweep();
        assert!(registry.entries.is_empty());
        assert_eq!((budget.waiting.get(), budget.bytes.borrow().mine()), (0, 0));
    }

    #[test]
    fn a_sweep_frees_a_cycle_of_travelling_ports() {
        let mut registry = Registry::default();
        let budget: Rc<MessageBudget> = Rc::default();
        registry.entries.insert(1, travelling_entry(None));
        registry.entries.insert(2, travelling_entry(None));
        queue_message(&mut registry, &budget, 1, &[2]);
        queue_message(&mut registry, &budget, 2, &[1]);
        registry.sweep();
        assert!(registry.entries.is_empty());
        assert_eq!(budget.waiting.get(), 0);
    }

    #[test]
    fn a_sweep_keeps_what_a_root_reaches() {
        let mut registry = Registry::default();
        let budget: Rc<MessageBudget> = Rc::default();
        // 1 has a task (root); its queue carries 2, whose queue carries 3.
        // 4 travels in a window message. 5 is garbage.
        for id in 1..=5 {
            registry.entries.insert(id, travelling_entry(None));
        }
        registry.entries.get_mut(&1).unwrap().pumps = 1;
        queue_message(&mut registry, &budget, 1, &[2]);
        queue_message(&mut registry, &budget, 2, &[3]);
        registry.window_ports.insert(4);
        registry.sweep();
        let mut kept: Vec<u64> = registry.entries.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, [1, 2, 3, 4]);
        assert_eq!(budget.waiting.get(), 2);
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

    #[test]
    fn a_long_chain_of_ports_in_queues_goes_without_recursion() {
        let mut registry = Registry::default();
        let budget: Rc<MessageBudget> = Rc::default();
        let last = 1_000_000;
        for id in 1..=last {
            registry.entries.insert(id, travelling_entry(None));
            if id > 1 {
                let queued = message_unbounded(&budget, id);
                registry
                    .entries
                    .get_mut(&(id - 1))
                    .unwrap()
                    .queue
                    .push_back(queued);
            }
        }
        registry.remove(1);
        assert!(registry.entries.is_empty());
    }

    /// A message that carries the port `id`, outside the bound of the budget.
    fn message_unbounded(budget: &Rc<MessageBudget>, id: u64) -> Message {
        budget.waiting.set(budget.waiting.get() + 1);
        Message {
            record: record_with(&[id]),
            charge: Charge {
                budget: Rc::clone(budget),
                bytes: 0,
                settled: false,
            },
        }
    }

    #[test]
    fn a_message_that_counts_for_its_sender_goes_back_with_its_drop() {
        let sender: Rc<MessageBudget> = Rc::default();
        let other: Rc<MessageBudget> = Rc::default();
        let mut registry = Registry::default();
        registry.entries.insert(1, travelling_entry(Some(2)));
        registry.entries.insert(2, travelling_entry(Some(1)));
        registry.route(1, message(&sender, &[], 100));
        assert_eq!(sender.bytes.borrow().mine(), 100);
        assert_eq!(other.bytes.borrow().mine(), 0);
        registry.remove(2);
        assert_eq!(sender.bytes.borrow().mine(), 0);
    }

    #[test]
    fn a_port_counts_for_the_runtime_that_made_it() {
        let maker: Rc<MessageBudget> = Rc::default();
        let mut registry = Registry::default();
        registry.insert_pair(1, 2, 7, &maker);
        assert_eq!(maker.ports.get(), 2);
        registry.entries.get_mut(&1).unwrap().home = Some(8);
        registry.remove(1);
        assert_eq!(maker.ports.get(), 1);
        registry.remove(2);
        assert_eq!(maker.ports.get(), 0);
    }
}
