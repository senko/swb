//! The reactions of DOM changes, as the document's mutation sink calls them
//! (ADR 0035 section 7).
//!
//! The DOM reports each primitive change to the sink of its document
//! (`swb_dom::MutationSink`). The sink decides with Rust data only (no
//! runtime): [`Reactions`] has a copy of what it needs of the definitions
//! ([`DefInfo`]) and of each window's registry. It enqueues reactions; the
//! `[CEReactions]` scope of the call that caused the change invokes them, or
//! the backup element queue does when no scope is active.
//!
//! - Insertion into a connected parent: for each inclusive descendant of
//!   each added node, in tree order, `connectedCallback` if it is custom,
//!   else an upgrade reaction if its node document's registry defines its
//!   name (HTML "try to upgrade"). The removed nodes of the same change come
//!   first.
//! - Removal from a connected parent: `disconnectedCallback` for each custom
//!   inclusive descendant, in tree order.
//! - Attribute changes: `attributeChangedCallback` for a custom element when
//!   the local name is observed, also for an unchanged value.
//! - Adoption: `adoptedCallback(oldDocument, newDocument)`.
//!
//! An arena that never held an element with a custom element name
//! (`Document::has_custom_names`) is skipped. The nodes that the walks visit
//! count as work for the time limit of the task.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use swb_dom::{CustomElementState, Document, LocalName, NodeId, QualName, ns};

use super::Callback;
use super::reactions::{Args, Enqueued, Origin, Place, Reaction, Reactions};
use crate::state::DocKey;

/// What the sink needs to know of a definition (a Rust copy of the parts of
/// the runtime's `Definition`).
#[derive(Debug)]
pub(crate) struct DefInfo {
    /// Bit `i` is set if the callback `Callback::ALL[i]` exists.
    pub(crate) callbacks: u8,
    pub(crate) observed: Rc<HashSet<LocalName>>,
}

impl DefInfo {
    fn has(&self, callback: Callback) -> bool {
        let index = Callback::ALL.iter().position(|&c| c == callback);
        index.is_some_and(|i| self.callbacks & (1 << i) != 0)
    }
}

/// The work units after which the host checks the time limit of the task
/// (as `mutation_registry`).
const WORK_PER_CHECK: usize = 4096;

/// Which reaction a walk over an inserted, removed or adopted subtree
/// enqueues.
#[derive(Clone, Copy)]
enum Walk {
    /// Insertion into a connected parent: `connectedCallback`, or try to
    /// upgrade.
    Connect,
    /// Removal from a connected parent: `disconnectedCallback`.
    Disconnect,
    /// Adoption: `adoptedCallback` with these documents.
    Adopt { old: Place, new: Place },
    /// A copy or a parsed fragment: try to upgrade.
    Upgrade,
}

impl Reactions {
    /// True if the agent has a definition (the sink acts only then).
    #[inline]
    pub(crate) fn has_definitions(&self) -> bool {
        !self.defs.is_empty()
    }

    /// Records a new definition for the sink; `number` is its index plus
    /// one.
    pub(crate) fn add_definition(
        &mut self,
        document: DocKey,
        local: LocalName,
        number: u32,
        info: DefInfo,
    ) {
        debug_assert_eq!(self.defs.len() + 1, number as usize);
        self.defs.push(info);
        self.names
            .entry(document)
            .or_default()
            .insert(local, number);
    }

    /// The runtime is gone: the sinks of the documents queue nothing more.
    pub(crate) fn close(&mut self) {
        self.defs.clear();
        self.names.clear();
        self.queues.clear();
        self.queued_docs.clear();
        self.stack.clear();
        self.backup.clear();
    }

    /// The definition numbers of `local` in the window of `document`.
    pub(crate) fn defined(&self, document: DocKey, local: &LocalName) -> Option<u32> {
        self.names.get(&document)?.get(local).copied()
    }

    /// True if the work of the sink since the last call is large enough to
    /// check the time limit (and resets it).
    pub(crate) fn take_overdue(&mut self) -> bool {
        let overdue = self.work >= WORK_PER_CHECK;
        if overdue {
            self.work = 0;
        }
        overdue
    }

    /// True once if the sink put an element on the backup queue and its
    /// microtask is not queued yet.
    pub(crate) fn take_microtask_wanted(&mut self) -> bool {
        std::mem::take(&mut self.microtask_wanted)
    }

    /// Enqueues for the sink: like [`Reactions::enqueue`], and a request for
    /// the backup queue's microtask is kept for the host. Returns true if
    /// the host must look at the reactions.
    fn sink_enqueue(&mut self, place: Place, reaction: Reaction) -> bool {
        if self.enqueue(place, reaction) == Enqueued::NeedsMicrotask {
            self.microtask_wanted = true;
            return true;
        }
        false
    }

    /// The reactions of a `Change::Children` (DOM "insert" and "remove"
    /// steps): the removed nodes first, then the added ones. Returns true if
    /// the host must look at the reactions.
    pub(crate) fn on_children(
        &mut self,
        key: DocKey,
        doc: &Document,
        parent: NodeId,
        added: &[NodeId],
        removed: &[NodeId],
    ) -> bool {
        if (added.is_empty() && removed.is_empty()) || !doc.has_custom_names() {
            return false;
        }
        if !doc.is_connected(parent) {
            return false;
        }
        let mut attention = false;
        for &node in removed {
            attention |= self.walk(key, doc, node, Walk::Disconnect);
        }
        for &node in added {
            attention |= self.walk(key, doc, node, Walk::Connect);
        }
        attention || self.work >= WORK_PER_CHECK
    }

    /// The reactions of an adoption of the subtree of `node` that stayed in
    /// the arena.
    pub(crate) fn on_adopted(
        &mut self,
        key: DocKey,
        doc: &Document,
        node: NodeId,
        old_document: NodeId,
    ) -> bool {
        self.on_adopted_from(key, doc, node, (key, old_document))
    }

    /// The reactions of an adoption of the subtree of `node` that came from
    /// the document node `old` (maybe of another arena).
    pub(crate) fn on_adopted_from(
        &mut self,
        key: DocKey,
        doc: &Document,
        node: NodeId,
        old: Place,
    ) -> bool {
        if !doc.has_custom_names() {
            return false;
        }
        let new = (key, doc.node_document(node));
        self.walk(key, doc, node, Walk::Adopt { old, new }) || self.work >= WORK_PER_CHECK
    }

    /// Upgrade reactions for the elements of the subtree of `root` whose
    /// names the registry of their node document defines, in tree order (the
    /// copies of a clone, the nodes of a parsed fragment).
    pub(crate) fn on_copies(&mut self, key: DocKey, doc: &Document, root: NodeId) -> bool {
        if !doc.has_custom_names() {
            return false;
        }
        self.walk(key, doc, root, Walk::Upgrade) || self.work >= WORK_PER_CHECK
    }

    /// The reaction of a `Change::Attribute` (DOM "handle attribute
    /// changes"): `attributeChangedCallback` for a custom element when the
    /// local name is observed. The new value is read from the document.
    pub(crate) fn on_attribute(
        &mut self,
        key: DocKey,
        doc: &Document,
        element: NodeId,
        name: &QualName,
        old_value: Option<&str>,
    ) -> bool {
        let (state, number) = doc.custom_element_state(element);
        if state != CustomElementState::Custom {
            return false;
        }
        let Some(info) = (number as usize)
            .checked_sub(1)
            .and_then(|i| self.defs.get(i))
        else {
            return false;
        };
        if !info.has(Callback::AttributeChanged) || !info.observed.contains(&name.local) {
            return false;
        }
        let new = doc
            .element(element)
            .and_then(|e| {
                e.attributes()
                    .iter()
                    .find(|a| a.name.ns == name.ns && a.name.local == name.local)
            })
            .map(|a| a.value.clone());
        let args = Args::Attribute {
            local: name.local.clone(),
            old: old_value.map(str::to_owned),
            new,
            namespace: (name.ns != ns!()).then(|| name.ns.to_string()),
        };
        self.sink_enqueue(
            (key, element),
            Reaction::Callback(Callback::AttributeChanged, args, Origin::Sink),
        )
    }

    /// Walks the shadow-including inclusive descendants of `root` in
    /// shadow-including tree order and enqueues
    /// the reaction of `walk` for each custom element (or, for an insertion,
    /// each element that can be upgraded).
    fn walk(&mut self, key: DocKey, doc: &Document, root: NodeId, walk: Walk) -> bool {
        let mut attention = false;
        let mut visited = 0;
        // DOM "insert", "remove" and "adopt" walk the shadow-including
        // inclusive descendants (measured: removing a host disconnects its
        // shadow tree, case `custom-elements-in-shadow`).
        for node in doc.shadow_including_descendants(root) {
            visited += 1;
            let (state, number) = doc.custom_element_state(node);
            let reaction = match (state, walk) {
                (CustomElementState::Custom, Walk::Connect) => {
                    self.callback_of(number, Callback::Connected, Args::None)
                }
                (CustomElementState::Custom, Walk::Disconnect) => {
                    self.callback_of(number, Callback::Disconnected, Args::None)
                }
                (CustomElementState::Custom, Walk::Adopt { old, new }) => {
                    self.callback_of(number, Callback::Adopted, Args::Adopted { old, new })
                }
                (CustomElementState::Undefined, Walk::Connect | Walk::Upgrade) => {
                    self.upgrade_of(key, doc, node)
                }
                _ => None,
            };
            if let Some(reaction) = reaction {
                attention |= self.sink_enqueue((key, node), reaction);
            }
        }
        self.work += visited;
        attention
    }

    fn callback_of(&self, number: u32, callback: Callback, args: Args) -> Option<Reaction> {
        let info = self.defs.get((number as usize).checked_sub(1)?)?;
        info.has(callback)
            .then_some(Reaction::Callback(callback, args, Origin::Sink))
    }

    /// HTML "try to upgrade an element": an upgrade reaction if the registry
    /// of the element's node document defines its name.
    fn upgrade_of(&self, key: DocKey, doc: &Document, node: NodeId) -> Option<Reaction> {
        let element = doc.element(node).filter(|e| e.is_html())?;
        if doc.node_document(node) != NodeId::DOCUMENT {
            return None;
        }
        let number = self.defined(key, &element.name.local)?;
        Some(Reaction::Upgrade(number))
    }

    /// The elements of `pairs` (old id, new id) went from the document
    /// `from` to the document `to` (ADR 0031 transplant): their queued
    /// reactions follow them, in the queues, the stack and the backup queue.
    pub(crate) fn rekey(&mut self, from: DocKey, to: DocKey, pairs: &[(NodeId, NodeId)]) {
        if !self.queued_docs.contains_key(&from) {
            return;
        }
        // The scans below are work for the time limit.
        self.work += self.queues.len() + self.stack.len() + self.backup.len();
        let index: HashMap<NodeId, NodeId> = pairs.iter().copied().collect();
        let moved = |place: &mut Place| {
            if place.0 == from
                && let Some(&new) = index.get(&place.1)
            {
                *place = (to, new);
            }
        };
        let queues = std::mem::take(&mut self.queues);
        self.queued_docs.clear();
        for (mut place, pending) in queues {
            moved(&mut place);
            *self.queued_docs.entry(place.0).or_default() += 1;
            self.queues.insert(place, pending);
        }
        self.stack.iter_mut().for_each(moved);
        self.backup.iter_mut().for_each(moved);
    }
}

#[cfg(test)]
mod tests {
    use swb_dom::{Attribute, Document, ElementData, NodeData};

    use super::*;

    const KEY: DocKey = 0;

    /// A document with `<x-a>` (state `custom`, definition 1) and `<x-b>`
    /// (state `undefined`) under the root element, and a reactions state
    /// with definition 1 (all callbacks, observes `a`) and the name `x-b`
    /// defined as 2.
    fn setup() -> (Document, Reactions, NodeId, NodeId, NodeId) {
        let mut doc = Document::new();
        let element = |doc: &mut Document, name: &str| {
            let data = ElementData::new(
                QualName::new(None, ns!(html), LocalName::from(name)),
                vec![Attribute {
                    name: QualName::new(None, ns!(), LocalName::from("a")),
                    value: "1".to_owned(),
                }],
            );
            doc.create_node(NodeData::Element(data)).unwrap()
        };
        let root = element(&mut doc, "div");
        doc.append_child(NodeId::DOCUMENT, root).unwrap();
        let a = element(&mut doc, "x-a");
        let b = element(&mut doc, "x-b");
        doc.set_custom_element_state(a, CustomElementState::Custom, Some(1));
        let mut reactions = Reactions::default();
        let info = || DefInfo {
            callbacks: 0b1_1111,
            observed: Rc::new([LocalName::from("a")].into_iter().collect()),
        };
        reactions.add_definition(KEY, LocalName::from("x-a"), 1, info());
        reactions.add_definition(KEY, LocalName::from("x-b"), 2, info());
        (doc, reactions, root, a, b)
    }

    /// Insertion enqueues the connected callback of a custom element and an
    /// upgrade of an undefined one with a defined name, in tree order; the
    /// removed nodes come first; nothing happens under a detached parent.
    #[test]
    fn insertion_and_removal_enqueue_in_tree_order() {
        let (mut doc, mut r, root, a, b) = setup();
        doc.append_child(root, b).unwrap();
        doc.append_child(root, a).unwrap();
        r.push();
        r.on_children(KEY, &doc, root, &[b, a], &[]);
        assert_eq!(r.take((KEY, b)), Some(Reaction::Upgrade(2)));
        assert_eq!(
            r.take((KEY, a)),
            Some(Reaction::Callback(
                Callback::Connected,
                Args::None,
                Origin::Sink
            ))
        );
        // Removal: only the custom element.
        r.on_children(KEY, &doc, root, &[], &[a, b]);
        assert_eq!(
            r.take((KEY, a)),
            Some(Reaction::Callback(
                Callback::Disconnected,
                Args::None,
                Origin::Sink
            ))
        );
        assert_eq!(r.take((KEY, b)), None);
        // A detached parent: nothing.
        let detached = doc.create_node(NodeData::DocumentFragment).unwrap();
        r.on_children(KEY, &doc, detached, &[a], &[]);
        assert_eq!(r.take((KEY, a)), None);
    }

    /// An attribute change enqueues a callback only for a custom element
    /// and an observed name; the new value is read from the document.
    #[test]
    fn attributes_enqueue_for_observed_names_of_custom_elements() {
        let (mut doc, mut r, _root, a, b) = setup();
        r.push();
        let name = QualName::new(None, ns!(), LocalName::from("a"));
        doc.set_attr(a, "a", "2").unwrap();
        r.on_attribute(KEY, &doc, a, &name, Some("1"));
        let other = QualName::new(None, ns!(), LocalName::from("z"));
        r.on_attribute(KEY, &doc, a, &other, None);
        r.on_attribute(KEY, &doc, b, &name, Some("1"));
        let queue = r.pop();
        assert_eq!(queue, vec![(KEY, a)]);
        assert_eq!(
            r.take((KEY, a)),
            Some(Reaction::Callback(
                Callback::AttributeChanged,
                Args::Attribute {
                    local: LocalName::from("a"),
                    old: Some("1".to_owned()),
                    new: Some("2".to_owned()),
                    namespace: None,
                },
                Origin::Sink
            ))
        );
    }

    /// A transplant moves the queued reactions to the new keys.
    #[test]
    fn a_transplant_rekeys_the_queues() {
        let (_doc, mut r, _root, a, b) = setup();
        r.push();
        r.enqueue((KEY, a), Reaction::Upgrade(1));
        r.enqueue((KEY, b), Reaction::Upgrade(2));
        r.rekey(KEY, 7, &[(a, b)]);
        let queue = r.pop();
        assert_eq!(queue, vec![(7, b), (KEY, b)]);
        assert_eq!(r.take((7, b)), Some(Reaction::Upgrade(1)));
        assert_eq!(r.take((KEY, b)), Some(Reaction::Upgrade(2)));
    }

    /// A transplant moves the reactions of the backup queue (no element
    /// queue is on the stack) to the new keys.
    #[test]
    fn a_transplant_rekeys_the_backup_queue() {
        let (_doc, mut r, _root, a, b) = setup();
        assert_eq!(
            r.enqueue((KEY, a), Reaction::Upgrade(1)),
            Enqueued::NeedsMicrotask
        );
        r.rekey(KEY, 7, &[(a, b)]);
        assert_eq!(r.take_backup(), vec![(7, b)]);
        assert_eq!(r.take((7, b)), Some(Reaction::Upgrade(1)));
    }

    /// A transplant from a document with nothing queued costs nothing; one
    /// from a document with queued reactions counts its scan as work. The
    /// count per document follows `take` and `rekey`.
    #[test]
    fn a_transplant_from_a_document_with_nothing_queued_is_free() {
        let (_doc, mut r, _root, a, b) = setup();
        r.push();
        r.enqueue((KEY, a), Reaction::Upgrade(1));
        r.rekey(5, 6, &[(a, b)]);
        assert_eq!(r.work, 0);
        r.rekey(KEY, 7, &[(a, b)]);
        assert!(r.work > 0);
        assert_eq!(r.queued_docs.get(&7), Some(&1));
        assert_eq!(r.queued_docs.get(&KEY), None);
        assert!(r.take((7, b)).is_some());
        assert!(r.queued_docs.is_empty());
        // A stale entry of the old key does nothing.
        r.work = 0;
        r.rekey(7, 8, &[(b, a)]);
        assert_eq!(r.work, 0);
    }

    /// The nodes that the walks visit count as work: after `WORK_PER_CHECK`
    /// of them the host checks the time limit (once).
    #[test]
    fn walks_count_their_nodes_as_work() {
        let (mut doc, mut r, root, _a, _b) = setup();
        let element = |doc: &mut Document| {
            let data = ElementData::new(
                QualName::new(None, ns!(html), LocalName::from("span")),
                Vec::new(),
            );
            doc.create_node(NodeData::Element(data)).unwrap()
        };
        let big = element(&mut doc);
        for _ in 0..WORK_PER_CHECK {
            let child = element(&mut doc);
            doc.append_child(big, child).unwrap();
        }
        doc.append_child(root, big).unwrap();
        r.push();
        assert!(r.on_children(KEY, &doc, root, &[big], &[]));
        assert!(r.take_overdue());
        assert!(!r.take_overdue());
    }
}
