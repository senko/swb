//! The event listener list of an event target (DOM §2.7,
//! <https://dom.spec.whatwg.org/#concept-event-listener>).
//!
//! The list keeps the listeners of each event type in the order they were
//! added; a dispatch only asks for the listeners of one type, so the list
//! is kept per type. An index by callback and capture finds a duplicate or
//! a listener to remove in O(1) in a list of 8 or more, so that pages with
//! very many listeners on one target stay linear; shorter lists are
//! searched. Removal sets the listener's `removed` flag
//! (a dispatch holds clones of the list and skips removed listeners) and
//! leaves a hole that a later compaction fills.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use swb_js::{Gc, Object, Tracer};

/// An event listener (DOM §2.7): the callback and its flags. The type is
/// the key of its list.
#[derive(Debug)]
pub(crate) struct Listener {
    /// The callback object (a function or an object with `handleEvent`).
    pub(crate) callback: Gc<Object>,
    pub(crate) capture: bool,
    pub(crate) passive: bool,
    pub(crate) once: bool,
    /// Set when the listener is removed (a dispatch that holds a clone of
    /// the list skips it).
    pub(crate) removed: Cell<bool>,
    /// The event handler that this listener stands for (HTML "event
    /// handler"): the callback is then the marker object and the dispatch
    /// calls the handler's current value (`handlers.rs`).
    pub(crate) handler: Option<&'static super::handlers::Name>,
}

/// The number of slots from which a type's list has an index; a shorter
/// list is searched. Most targets have one or two listeners of a type,
/// and an index would be most of their memory.
const INDEXED: usize = 8;

/// The listeners of one event type in the order they were added, with
/// holes where listeners were removed.
#[derive(Debug, Default)]
struct TypeListeners {
    slots: Vec<Option<Rc<Listener>>>,
    /// The listeners in `slots` (the slots that are not holes).
    live: usize,
    /// The slot of each listener by its callback and capture flag, when
    /// the list has at least `INDEXED` slots; empty otherwise.
    index: HashMap<(Gc<Object>, bool), usize>,
}

impl TypeListeners {
    /// The slot of the listener with `key` (callback and capture).
    fn find(&self, key: (Gc<Object>, bool)) -> Option<usize> {
        if self.slots.len() >= INDEXED {
            return self.index.get(&key).copied();
        }
        self.slots.iter().position(|slot| {
            slot.as_ref()
                .is_some_and(|l| (l.callback, l.capture) == key)
        })
    }

    /// Appends a listener (not a duplicate).
    fn push(&mut self, listener: Listener) {
        let key = (listener.callback, listener.capture);
        self.slots.push(Some(Rc::new(listener)));
        self.live += 1;
        if self.slots.len() == INDEXED {
            self.reindex();
        } else if self.slots.len() > INDEXED {
            self.index.insert(key, self.slots.len() - 1);
        }
    }

    /// Takes the listener out of `slot`, leaving a hole.
    fn take(&mut self, slot: usize) -> Option<Rc<Listener>> {
        let listener = self.slots.get_mut(slot).and_then(Option::take)?;
        self.live -= 1;
        self.index.remove(&(listener.callback, listener.capture));
        Some(listener)
    }

    /// Builds the index from the slots, or drops it for a short list.
    fn reindex(&mut self) {
        if self.slots.len() < INDEXED {
            self.index = HashMap::new();
            return;
        }
        self.index.clear();
        for (slot, listener) in self.slots.iter().enumerate() {
            if let Some(listener) = listener {
                self.index
                    .insert((listener.callback, listener.capture), slot);
            }
        }
    }

    /// Fills the holes when they are more than the listeners.
    fn compact(&mut self) {
        let holes = self.slots.len() - self.live;
        if holes <= 16 || holes <= self.live {
            return;
        }
        self.slots.retain(Option::is_some);
        self.reindex();
    }
}

/// The event listener list of one event target.
#[derive(Debug, Default)]
pub(crate) struct Listeners {
    by_type: HashMap<Rc<str>, TypeListeners>,
}

impl Listeners {
    /// Appends `listener` for `kind` unless the list has a listener with
    /// the same type, callback and capture flag (DOM "add an event
    /// listener", step 5). Returns true if it was appended.
    pub(crate) fn add(&mut self, kind: &str, listener: Listener) -> bool {
        let list = match self.by_type.get_mut(kind) {
            Some(list) => list,
            None => self.by_type.entry(Rc::from(kind)).or_default(),
        };
        if list.find((listener.callback, listener.capture)).is_some() {
            return false;
        }
        list.push(listener);
        true
    }

    /// Removes the listener of `kind` with `callback` and `capture` (DOM
    /// "remove an event listener"): sets its `removed` flag. Returns true
    /// if there was one.
    pub(crate) fn remove(&mut self, kind: &str, callback: Gc<Object>, capture: bool) -> bool {
        let Some(list) = self.by_type.get_mut(kind) else {
            return false;
        };
        let Some(slot) = list.find((callback, capture)) else {
            return false;
        };
        if let Some(listener) = list.take(slot) {
            listener.removed.set(true);
        }
        if list.live == 0 {
            self.by_type.remove(kind);
        } else {
            list.compact();
        }
        true
    }

    /// Removes `listener` itself (a `once` listener before its call), if it
    /// is still in the list. Returns true if it was.
    pub(crate) fn remove_listener(&mut self, kind: &str, listener: &Rc<Listener>) -> bool {
        let current = self.by_type.get(kind).and_then(|list| {
            let slot = list.find((listener.callback, listener.capture))?;
            list.slots.get(slot)?.as_ref()
        });
        if current.is_some_and(|current| Rc::ptr_eq(current, listener)) {
            return self.remove(kind, listener.callback, listener.capture);
        }
        false
    }

    /// A clone of the listeners of `kind` in order (DOM "invoke", step 6:
    /// listeners added afterwards do not run; removed ones are skipped by
    /// their flag).
    pub(crate) fn of_type(&self, kind: &str) -> Vec<Rc<Listener>> {
        self.by_type
            .get(kind)
            .map(|list| list.slots.iter().flatten().cloned().collect())
            .unwrap_or_default()
    }

    /// The number of listeners.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.by_type.values().map(|list| list.live).sum()
    }

    /// True if the list has no listener.
    pub(crate) fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }

    /// Reports the callbacks to the collector.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for list in self.by_type.values() {
            for listener in list.slots.iter().flatten() {
                tracer.object(listener.callback);
            }
        }
    }

    /// The bytes that the list owns (approximate).
    pub(crate) fn heap_size(&self) -> usize {
        self.by_type
            .iter()
            .map(|(kind, list)| {
                kind.len()
                    + list.slots.capacity() * (size_of::<Option<Rc<Listener>>>() + 48)
                    + list.index.capacity() * 24
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(callback: Gc<Object>, capture: bool) -> Listener {
        Listener {
            callback,
            capture,
            passive: false,
            once: false,
            removed: Cell::new(false),
            handler: None,
        }
    }

    /// Distinct object handles without a heap (they are never followed).
    fn objects(n: usize) -> Vec<Gc<Object>> {
        let mut rt = swb_js::Runtime::new(swb_js::RuntimeConfig::default()).unwrap();
        let scope = rt.heap_mut().open_scope();
        let objects = (0..n).map(|_| rt.new_plain_object(0).unwrap()).collect();
        rt.heap_mut().close_scope(scope).unwrap();
        objects
    }

    #[test]
    fn duplicates_removal_and_order() {
        let o = objects(3);
        let mut list = Listeners::default();
        assert!(list.add("x", listener(o[0], false)));
        assert!(!list.add("x", listener(o[0], false)));
        assert!(list.add("x", listener(o[0], true)));
        assert!(list.add("y", listener(o[0], false)));
        assert!(list.add("x", listener(o[1], false)));
        assert_eq!(list.len(), 4);
        let clone = list.of_type("x");
        assert_eq!(clone.len(), 3);
        assert!(list.remove("x", o[0], true));
        assert!(!list.remove("x", o[0], true));
        assert!(clone[1].removed.get());
        assert!(!clone[0].removed.get());
        let order: Vec<_> = list.of_type("x").iter().map(|l| l.callback).collect();
        assert_eq!(order, [o[0], o[1]]);
        // Removed and added again: a new listener at the end.
        assert!(list.remove("x", o[0], false));
        assert!(list.add("x", listener(o[0], false)));
        assert!(!list.remove_listener("x", &clone[0]));
        let order: Vec<_> = list.of_type("x").iter().map(|l| l.callback).collect();
        assert_eq!(order, [o[1], o[0]]);
    }

    /// Short lists have no index; the list keeps working across the
    /// switch to the index and back after a compaction.
    #[test]
    fn short_lists_are_searched() {
        let o = objects(24);
        let mut list = Listeners::default();
        let index_capacity = |list: &Listeners| list.by_type.get("x").map(|l| l.index.capacity());
        for &object in &o[..INDEXED - 1] {
            assert!(list.add("x", listener(object, false)));
        }
        assert_eq!(index_capacity(&list), Some(0));
        assert!(!list.add("x", listener(o[0], false)));
        for &object in &o[INDEXED - 1..] {
            assert!(list.add("x", listener(object, false)));
        }
        assert!(index_capacity(&list).is_some_and(|c| c > 0));
        assert!(!list.add("x", listener(o[INDEXED], false)));
        // 17 holes and 7 listeners: the compaction leaves 7 slots and
        // drops the index.
        for &object in &o[..17] {
            assert!(list.remove("x", object, false));
        }
        assert_eq!(list.by_type.get("x").map(|l| l.slots.len()), Some(7));
        assert_eq!(index_capacity(&list), Some(0));
        assert!(!list.add("x", listener(o[23], false)));
        assert!(list.remove("x", o[18], false));
        let rest: Vec<_> = list.of_type("x").iter().map(|l| l.callback).collect();
        assert_eq!(rest, [o[17], o[19], o[20], o[21], o[22], o[23]]);
    }

    #[test]
    fn many_removals_compact_the_list() {
        let o = objects(200);
        let mut list = Listeners::default();
        for &object in &o {
            list.add("x", listener(object, false));
        }
        for &object in &o[..150] {
            assert!(list.remove("x", object, false));
        }
        let rest: Vec<_> = list.of_type("x").iter().map(|l| l.callback).collect();
        assert_eq!(rest, o[150..]);
        let slots = list.by_type.get("x").map_or(0, |l| l.slots.len());
        assert!(slots < 120, "{slots}");
        for &object in &o[150..] {
            assert!(list.remove("x", object, false));
        }
        assert_eq!(list.len(), 0);
        assert!(list.by_type.is_empty());
    }
}
