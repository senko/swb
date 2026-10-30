//! The registry of mutation observers (DOM §4.3,
//! <https://dom.spec.whatwg.org/#mutation-observers>; ADR 0028 sections 3
//! and 13): the registered observers of the nodes, the transient registered
//! observers of removed nodes, and the records that wait for their delivery.
//!
//! It holds only Rust data (node ids, names, old values), so the document
//! can keep it as its mutation sink ([`Sink`]) while the document is lent
//! to a script runtime. The JavaScript objects (`MutationObserver`,
//! `MutationRecord`) are made at delivery (`mutation_observer.rs`).
//!
//! The sink is installed at the first `observe()`: until then the document
//! reports nothing and a parse costs nothing extra.
//!
//! Limits (ADR 0028 section 16): [`MAX_RECORDS`] records and
//! [`MAX_RECORD_BYTES`] bytes of record data wait at most; later records are
//! dropped with one warning, so a script that changes the tree in a loop
//! without yielding cannot fill the memory. At most [`MAX_TRANSIENTS`]
//! transient registered observers exist; later ones are not made (with one
//! warning). The work of the sink (nodes visited, registrations made) is
//! counted ([`Registry::take_overdue`]): the host charges it to the time
//! limit of the task, because native work does not tick the countdown of
//! the VM.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use swb_dom::{Change, Document, MutationSink, NodeId, QualName};

/// The most records that wait for delivery, over all observers. The
/// delivery of a full queue makes the JavaScript objects of every record
/// and must fit in the 256 MiB heap of the runtime with room for the
/// callback. Measured (a callback that reads `type`, `target`,
/// `addedNodes` and the first added node of every record): 150,000 records
/// are delivered and 200,000 reach the heap limit, so a record costs about
/// 1.4 KB with the objects of its node lists. The bound is
/// [`MAX_RECORD_BYTES`] over [`RECORD_COST`].
pub(crate) const MAX_RECORDS: usize = 131_072;

/// The most bytes that the waiting records hold (old values, names, node
/// lists, and a fixed cost per record), over all observers.
pub(crate) const MAX_RECORD_BYTES: usize = 128 << 20;

/// The fixed cost of a record in [`MAX_RECORD_BYTES`]: the Rust record and
/// the `MutationRecord` object with its data that the delivery makes, with
/// room for the two node lists that a callback may read.
const RECORD_COST: usize = 1024;

/// The most transient registered observers that exist at once.
pub(crate) const MAX_TRANSIENTS: usize = 500_000;

/// The work units (nodes visited, registrations made) after which the host
/// checks the time limit of the task.
const WORK_PER_CHECK: usize = 4096;

/// The kind of a record (`MutationRecord.type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    ChildList,
    Attributes,
    CharacterData,
}

impl Kind {
    /// The value of `MutationRecord.type`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::ChildList => "childList",
            Kind::Attributes => "attributes",
            Kind::CharacterData => "characterData",
        }
    }
}

/// The options of a registration (`MutationObserverInit` after the
/// defaults of `observe()`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) child_list: bool,
    pub(crate) attributes: bool,
    pub(crate) character_data: bool,
    pub(crate) subtree: bool,
    pub(crate) attribute_old_value: bool,
    pub(crate) character_data_old_value: bool,
    pub(crate) attribute_filter: Option<Vec<String>>,
}

/// A mutation record, as Rust data.
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub(crate) kind: Kind,
    pub(crate) target: NodeId,
    pub(crate) added: Vec<NodeId>,
    pub(crate) removed: Vec<NodeId>,
    pub(crate) previous_sibling: Option<NodeId>,
    pub(crate) next_sibling: Option<NodeId>,
    pub(crate) attribute_name: Option<String>,
    pub(crate) attribute_namespace: Option<String>,
    pub(crate) old_value: Option<String>,
}

impl Record {
    /// What the record costs in [`MAX_RECORD_BYTES`].
    fn cost(&self) -> usize {
        RECORD_COST
            + size_of::<NodeId>() * (self.added.len() + self.removed.len())
            + self.attribute_name.as_ref().map_or(0, String::len)
            + self.attribute_namespace.as_ref().map_or(0, String::len)
            + self.old_value.as_ref().map_or(0, String::len)
    }
}

/// A registered observer in the list of a node: a regular one, made by
/// `observe()`, or a transient one (`source` is the node of the regular
/// registration it came from).
#[derive(Clone, Debug)]
struct Registration {
    observer: u32,
    options: Rc<Options>,
    source: Option<NodeId>,
}

/// What the registry knows about one observer.
#[derive(Debug, Default)]
struct Entry {
    /// The queue of records.
    records: Vec<Record>,
    /// The nodes with a regular registration of the observer (the
    /// observer's "node list").
    nodes: Vec<NodeId>,
    /// The nodes with a transient registered observer of it.
    transient_nodes: HashSet<NodeId>,
}

/// The registered observers of all nodes and the queues of all observers.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    nodes: HashMap<NodeId, Vec<Registration>>,
    /// The observers that have registrations, by number (creation order).
    entries: BTreeMap<u32, Entry>,
    /// The observers with records or transient registrations: the ones
    /// that "notify mutation observers" visits, in creation order.
    notify: BTreeSet<u32>,
    waiting: usize,
    waiting_bytes: usize,
    /// The transient registered observers that exist.
    transients: usize,
    warned: bool,
    warned_transients: bool,
    /// Work units since the host last charged them.
    work: usize,
    /// A record was queued: the host queues the mutation observer
    /// microtask (DOM "queue a mutation observer microtask").
    microtask_wanted: bool,
    /// Set when the host must look at the registry after the current call:
    /// a record was queued or the work is overdue.
    request: Rc<Cell<bool>>,
}

/// The registry as the document's sink and the binding state share it.
pub(crate) type Shared = Rc<RefCell<Registry>>;

/// The mutation sink of the document: reports to the registry.
pub(crate) struct Sink(pub(crate) Shared);

impl MutationSink for Sink {
    fn change(&mut self, doc: &Document, change: &Change<'_>) {
        self.0.borrow_mut().change(doc, change);
    }
}

impl Registry {
    /// A registry that sets `request` when a record is queued.
    pub(crate) fn new(request: Rc<Cell<bool>>) -> Registry {
        Registry {
            request,
            ..Registry::default()
        }
    }

    /// The `observe()` algorithm of DOM §4.3.2, after the checks of its
    /// arguments: registers observer `observer` at `node`, or replaces the
    /// options of its registration there.
    pub(crate) fn observe(&mut self, observer: u32, node: NodeId, options: Options) {
        let options = Rc::new(options);
        self.entries.entry(observer).or_default();
        let existing = self.nodes.get_mut(&node).and_then(|list| {
            list.iter_mut()
                .find(|r| r.observer == observer && r.source.is_none())
        });
        if let Some(existing) = existing {
            existing.options = options;
            self.remove_transients(observer, node);
            return;
        }
        self.nodes.entry(node).or_default().push(Registration {
            observer,
            options,
            source: None,
        });
        if let Some(entry) = self.entries.get_mut(&observer) {
            entry.nodes.push(node);
        }
    }

    /// Removes the transient registered observers of `observer` whose
    /// source is the registration at `source`.
    fn remove_transients(&mut self, observer: u32, source: NodeId) {
        let Some(entry) = self.entries.get_mut(&observer) else {
            return;
        };
        let transient_nodes = std::mem::take(&mut entry.transient_nodes);
        let mut kept = HashSet::new();
        for transient in transient_nodes {
            let Some(list) = self.nodes.get_mut(&transient) else {
                continue;
            };
            let before = list.len();
            list.retain(|r| !(r.observer == observer && r.source == Some(source)));
            self.transients -= before - list.len();
            self.work += 1;
            if list.iter().any(|r| r.observer == observer) {
                kept.insert(transient);
            }
            if list.is_empty() {
                self.nodes.remove(&transient);
            }
        }
        self.check_work();
        if let Some(entry) = self.entries.get_mut(&observer) {
            entry.transient_nodes = kept;
        }
    }

    /// `disconnect()`: removes all registrations of the observer and
    /// empties its queue.
    pub(crate) fn disconnect(&mut self, observer: u32) {
        let Some(entry) = self.entries.remove(&observer) else {
            return;
        };
        for node in entry.nodes.iter().chain(&entry.transient_nodes) {
            if let Some(list) = self.nodes.get_mut(node) {
                let transient =
                    |list: &[Registration]| list.iter().filter(|r| r.source.is_some()).count();
                let before = transient(list);
                list.retain(|r| r.observer != observer);
                self.transients -= before - transient(list);
                if list.is_empty() {
                    self.nodes.remove(node);
                }
            }
        }
        for record in &entry.records {
            self.waiting -= 1;
            self.waiting_bytes -= record.cost();
        }
        self.notify.remove(&observer);
    }

    /// True if the observer has registrations.
    #[cfg(test)]
    pub(crate) fn is_registered(&self, observer: u32) -> bool {
        self.entries
            .get(&observer)
            .is_some_and(|entry| !entry.nodes.is_empty())
    }

    /// `takeRecords()`: empties the queue and returns its records.
    pub(crate) fn take_records(&mut self, observer: u32) -> Vec<Record> {
        let Some(entry) = self.entries.get_mut(&observer) else {
            return Vec::new();
        };
        let records = std::mem::take(&mut entry.records);
        for record in &records {
            self.waiting -= 1;
            self.waiting_bytes -= record.cost();
        }
        if entry.transient_nodes.is_empty() {
            self.notify.remove(&observer);
        }
        records
    }

    /// The start of "notify mutation observers": the observers with records
    /// or transient registrations, in creation order. A record that a
    /// callback queues for an observer of the set is delivered in that
    /// observer's turn (measured: `o2` gets the record that the callback of
    /// `o1` caused, together with its own).
    pub(crate) fn take_notify_set(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.notify).into_iter().collect()
    }

    /// The turn of one observer of the notify set: empties its queue (the
    /// callbacks of the observers before it may have added records), removes
    /// its transient registered observers, and returns the records.
    pub(crate) fn take_for_notification(&mut self, observer: u32) -> Vec<Record> {
        let Some(entry) = self.entries.get_mut(&observer) else {
            return Vec::new();
        };
        let records = std::mem::take(&mut entry.records);
        let transient_nodes = std::mem::take(&mut entry.transient_nodes);
        for record in &records {
            self.waiting -= 1;
            self.waiting_bytes -= record.cost();
        }
        for node in transient_nodes {
            if let Some(list) = self.nodes.get_mut(&node) {
                let before = list.len();
                list.retain(|r| !(r.observer == observer && r.source.is_some()));
                self.transients -= before - list.len();
                if list.is_empty() {
                    self.nodes.remove(&node);
                }
            }
        }
        self.work += 1;
        records
    }

    /// Puts observers back into the notify set: a termination ended the
    /// delivery before their turn. The records stay queued and the next
    /// delivery takes them.
    pub(crate) fn restore_notify(&mut self, observers: &[u32]) {
        for observer in observers {
            if self.entries.contains_key(observer) {
                self.notify.insert(*observer);
            }
        }
    }

    /// Asks for the mutation observer microtask (if records wait).
    pub(crate) fn want_microtask(&mut self) {
        if !self.notify.is_empty() {
            self.microtask_wanted = true;
            self.request.set(true);
        }
    }

    /// What the host must do after the current call: whether to queue the
    /// mutation observer microtask, and whether the work of the registry
    /// since the last charge is large enough to check the time limit.
    pub(crate) fn take_signals(&mut self) -> (bool, bool) {
        self.request.set(false);
        let microtask = std::mem::take(&mut self.microtask_wanted);
        let overdue = self.work >= WORK_PER_CHECK;
        if overdue {
            self.work = 0;
        }
        (microtask, overdue)
    }

    /// Counts work; asks the host to check the time limit when it is large.
    fn check_work(&mut self) {
        if self.work >= WORK_PER_CHECK {
            self.request.set(true);
        }
    }

    /// True if a record or a transient registration waits for the
    /// notification.
    #[cfg(test)]
    pub(crate) fn has_notifications(&self) -> bool {
        !self.notify.is_empty()
    }

    /// The number of records that wait.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.waiting
    }

    /// One change of the document (DOM "queue a mutation record" and, for
    /// removed nodes, the transient registered observers of "remove").
    fn change(&mut self, doc: &Document, change: &Change<'_>) {
        if self.nodes.is_empty() {
            return;
        }
        match *change {
            Change::Children {
                parent,
                added,
                removed,
                prev_sibling,
                next_sibling,
            } => {
                if !removed.is_empty() {
                    self.add_transients(doc, parent, removed);
                }
                self.queue(doc, Kind::ChildList, parent, None, |record| {
                    record.added = added.to_vec();
                    record.removed = removed.to_vec();
                    record.previous_sibling = prev_sibling;
                    record.next_sibling = next_sibling;
                });
            }
            Change::Attribute {
                element,
                name,
                old_value,
            } => self.queue(doc, Kind::Attributes, element, Some(name), |record| {
                record.attribute_name = Some(name.local.to_string());
                record.attribute_namespace = namespace_of(name);
                record.old_value = old_value.map(str::to_owned);
            }),
            Change::Text { node, old_value } => {
                self.queue(doc, Kind::CharacterData, node, None, |record| {
                    record.old_value = old_value.map(str::to_owned);
                });
            }
        }
    }

    /// "Queue a mutation record" of `kind` for `target`: finds the
    /// interested observers on the inclusive ancestors and queues a record
    /// (made by `fill`) at each.
    fn queue(
        &mut self,
        doc: &Document,
        kind: Kind,
        target: NodeId,
        attribute: Option<&QualName>,
        fill: impl Fn(&mut Record),
    ) {
        // (observer, whether its record gets the old value)
        let mut interested: Vec<(u32, bool)> = Vec::new();
        let mut node = Some(target);
        let mut visited = 0;
        while let Some(current) = node {
            visited += 1;
            if let Some(list) = self.nodes.get(&current) {
                visited += list.len();
                for registration in list {
                    if wants(&registration.options, kind, current == target, attribute) {
                        let old = match kind {
                            Kind::Attributes => registration.options.attribute_old_value,
                            Kind::CharacterData => registration.options.character_data_old_value,
                            Kind::ChildList => false,
                        };
                        interested.push((registration.observer, old));
                    }
                }
            }
            node = doc.parent(current);
        }
        self.work += visited;
        self.check_work();
        if interested.is_empty() {
            return;
        }
        // One record for an observer that registered more than once on the
        // chain; it gets the old value if any of its registrations asks.
        interested.sort_unstable_by_key(|&(observer, _)| observer);
        interested.dedup_by(|next, kept| {
            let same = next.0 == kept.0;
            if same {
                kept.1 |= next.1;
            }
            same
        });
        let mut proto = Record {
            kind,
            target,
            added: Vec::new(),
            removed: Vec::new(),
            previous_sibling: None,
            next_sibling: None,
            attribute_name: None,
            attribute_namespace: None,
            old_value: None,
        };
        fill(&mut proto);
        let full_cost = proto.cost();
        let old_bytes = proto.old_value.as_ref().map_or(0, String::len);
        for (observer, old) in interested {
            // Check the limits before the copy: with a full queue, a change
            // seen by many observers must not copy a long old value for each.
            let cost = if old {
                full_cost
            } else {
                full_cost - old_bytes
            };
            if !self.has_room(cost) {
                continue;
            }
            let mut record = proto.clone();
            if !old {
                record.old_value = None;
            }
            self.push(observer, record);
        }
    }

    /// True if a record of `cost` fits in the limits; else warns once.
    fn has_room(&mut self, cost: usize) -> bool {
        if self.waiting < MAX_RECORDS && self.waiting_bytes + cost <= MAX_RECORD_BYTES {
            return true;
        }
        if !self.warned {
            self.warned = true;
            log::warn!(
                "{} mutation records wait for delivery; further records are dropped",
                self.waiting
            );
        }
        false
    }

    /// Appends a record to the queue of an observer, if the limits allow.
    fn push(&mut self, observer: u32, record: Record) {
        let cost = record.cost();
        if !self.has_room(cost) {
            return;
        }
        let Some(entry) = self.entries.get_mut(&observer) else {
            return;
        };
        entry.records.push(record);
        self.waiting += 1;
        self.waiting_bytes += cost;
        self.notify.insert(observer);
        self.microtask_wanted = true;
        self.request.set(true);
    }

    /// The transient registered observers of "remove": each removed node
    /// gets one for each registered observer with `subtree` on the
    /// inclusive ancestors of `parent`.
    fn add_transients(&mut self, doc: &Document, parent: NodeId, removed: &[NodeId]) {
        if self.transients >= MAX_TRANSIENTS {
            return;
        }
        let mut sources: Vec<Registration> = Vec::new();
        let mut node = Some(parent);
        let mut visited = 0;
        while let Some(current) = node {
            visited += 1;
            if let Some(list) = self.nodes.get(&current) {
                visited += list.len();
                for registration in list.iter().filter(|r| r.options.subtree) {
                    sources.push(Registration {
                        observer: registration.observer,
                        options: registration.options.clone(),
                        source: Some(registration.source.unwrap_or(current)),
                    });
                }
            }
            node = doc.parent(current);
        }
        self.work += visited;
        for source in sources {
            self.work += removed.len();
            self.check_work();
            for &gone in removed {
                if self.transients >= MAX_TRANSIENTS {
                    if !self.warned_transients {
                        self.warned_transients = true;
                        log::warn!(
                            "{} transient registered observers exist; further ones are not made",
                            self.transients
                        );
                    }
                    return;
                }
                let list = self.nodes.entry(gone).or_default();
                let duplicate = list.iter().any(|r| {
                    r.observer == source.observer
                        && r.source == source.source
                        && r.options == source.options
                });
                if duplicate {
                    continue;
                }
                list.push(source.clone());
                self.transients += 1;
                if let Some(entry) = self.entries.get_mut(&source.observer) {
                    entry.transient_nodes.insert(gone);
                    self.notify.insert(source.observer);
                }
            }
        }
    }
}

/// True if an observer with `options` is interested in a record of `kind`
/// whose target is the registration's node (`at_target`) or a descendant.
fn wants(options: &Options, kind: Kind, at_target: bool, attribute: Option<&QualName>) -> bool {
    if !at_target && !options.subtree {
        return false;
    }
    match kind {
        Kind::ChildList => options.child_list,
        Kind::CharacterData => options.character_data,
        Kind::Attributes => {
            if !options.attributes {
                return false;
            }
            match (&options.attribute_filter, attribute) {
                (Some(filter), Some(name)) => {
                    namespace_of(name).is_none() && filter.iter().any(|f| **f == *name.local)
                }
                _ => true,
            }
        }
    }
}

/// `MutationRecord.attributeNamespace`: the namespace URL, or none.
fn namespace_of(name: &QualName) -> Option<String> {
    let ns: &str = &name.ns;
    (!ns.is_empty()).then(|| ns.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(child_list: bool, subtree: bool) -> Options {
        Options {
            child_list,
            subtree,
            ..Options::default()
        }
    }

    #[test]
    fn a_registration_is_replaced_by_a_second_observe() {
        let mut registry = Registry::new(Rc::new(Cell::new(false)));
        let node = NodeId::DOCUMENT;
        registry.observe(1, node, options(true, false));
        registry.observe(1, node, options(true, true));
        assert_eq!(registry.nodes[&node].len(), 1);
        assert!(registry.nodes[&node][0].options.subtree);
        registry.disconnect(1);
        assert!(registry.nodes.is_empty());
        assert!(!registry.is_registered(1));
    }

    #[test]
    fn waiting_records_are_bounded() {
        let request = Rc::new(Cell::new(false));
        let mut registry = Registry::new(request.clone());
        let doc = Document::new();
        registry.observe(
            1,
            NodeId::DOCUMENT,
            Options {
                character_data: true,
                character_data_old_value: true,
                ..Options::default()
            },
        );
        let change = Change::Text {
            node: NodeId::DOCUMENT,
            old_value: None,
        };
        for _ in 0..MAX_RECORDS + 10 {
            registry.change(&doc, &change);
        }
        assert_eq!(registry.waiting(), MAX_RECORDS);
        assert!(request.get());
        assert_eq!(registry.take_records(1).len(), MAX_RECORDS);
        assert_eq!(registry.waiting(), 0);
        // The byte budget: records with a long old value.
        let long = "y".repeat(1 << 20);
        let change = Change::Text {
            node: NodeId::DOCUMENT,
            old_value: Some(&long),
        };
        for _ in 0..200 {
            registry.change(&doc, &change);
        }
        assert!(registry.waiting() <= MAX_RECORD_BYTES / (1 << 20));
        assert!(registry.waiting() >= 100);
    }

    #[test]
    fn transient_registrations_are_bounded_and_released() {
        let mut registry = Registry::new(Rc::new(Cell::new(false)));
        let doc = Document::new();
        registry.observe(1, NodeId::DOCUMENT, options(true, true));
        let gone: Vec<NodeId> = (1..=MAX_TRANSIENTS + 10)
            .filter_map(NodeId::from_index)
            .collect();
        registry.change(
            &doc,
            &Change::Children {
                parent: NodeId::DOCUMENT,
                added: &[],
                removed: &gone,
                prev_sibling: None,
                next_sibling: None,
            },
        );
        assert_eq!(registry.transients, MAX_TRANSIENTS);
        // Work was counted: the host is asked to check the time limit once.
        assert!(registry.take_signals().1);
        assert!(!registry.take_signals().1);
        let records = registry.take_for_notification(1);
        assert_eq!(records.len(), 1);
        assert_eq!(registry.transients, 0);
        registry.disconnect(1);
        assert!(registry.nodes.is_empty());
    }

    #[test]
    fn restore_notify_keeps_the_observers_that_did_not_get_their_turn() {
        let mut registry = Registry::new(Rc::new(Cell::new(false)));
        let doc = Document::new();
        for observer in [1, 2] {
            registry.observe(observer, NodeId::DOCUMENT, options(true, false));
        }
        registry.change(
            &doc,
            &Change::Children {
                parent: NodeId::DOCUMENT,
                added: &[],
                removed: &[],
                prev_sibling: None,
                next_sibling: None,
            },
        );
        let set = registry.take_notify_set();
        assert_eq!(set, vec![1, 2]);
        assert_eq!(registry.take_for_notification(1).len(), 1);
        registry.restore_notify(&set[1..]);
        assert_eq!(registry.take_notify_set(), vec![2]);
        assert_eq!(registry.take_for_notification(2).len(), 1);
    }

    #[test]
    fn disconnect_drops_the_queue() {
        let mut registry = Registry::new(Rc::new(Cell::new(false)));
        let doc = Document::new();
        registry.observe(
            1,
            NodeId::DOCUMENT,
            Options {
                character_data: true,
                ..Options::default()
            },
        );
        registry.change(
            &doc,
            &Change::Text {
                node: NodeId::DOCUMENT,
                old_value: None,
            },
        );
        assert_eq!(registry.waiting(), 1);
        assert!(registry.has_notifications());
        registry.disconnect(1);
        assert_eq!(registry.waiting(), 0);
        assert_eq!(registry.waiting_bytes, 0);
        assert_eq!(registry.take_notify_set(), Vec::<u32>::new());
    }

    #[test]
    fn wants_follows_the_options() {
        let o = options(true, false);
        assert!(wants(&o, Kind::ChildList, true, None));
        assert!(!wants(&o, Kind::ChildList, false, None));
        assert!(!wants(&o, Kind::Attributes, true, None));
        let filtered = Options {
            attributes: true,
            attribute_filter: Some(vec!["id".to_owned()]),
            ..Options::default()
        };
        let id = QualName::new(None, swb_dom::ns!(), swb_dom::LocalName::from("id"));
        let title = QualName::new(None, swb_dom::ns!(), swb_dom::LocalName::from("title"));
        let xlink_id = QualName::new(None, swb_dom::ns!(xlink), swb_dom::LocalName::from("id"));
        assert!(wants(&filtered, Kind::Attributes, true, Some(&id)));
        assert!(!wants(&filtered, Kind::Attributes, true, Some(&title)));
        assert!(!wants(&filtered, Kind::Attributes, true, Some(&xlink_id)));
    }
}
