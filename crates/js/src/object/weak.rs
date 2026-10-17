//! The data of the weak collections and references (ECMA-262 §24.3
//! `WeakMap`, §24.4 `WeakSet`, §26.1 `WeakRef`, §26.2
//! `FinalizationRegistry`, <https://tc39.es/ecma262/#sec-weakmap-objects>;
//! ADR 0026 sections 8 and 15).
//!
//! - `WeakMap` and `WeakSet` keep their entries in the hash table of `Map`
//!   and `Set` (`table.rs`), keyed by the handle of an object or of a
//!   symbol that is not registered (`CanBeHeldWeakly`, §9.13). They have
//!   no iterators and no `size`, so the table's order and cursors are not
//!   used. The collector does not trace their entries: a `WeakMap` entry is
//!   an ephemeron (`heap/gc.rs`), and the sweep removes the entries whose
//!   keys died ([`Table::remove_dead`]).
//! - A `WeakRef` holds its target without tracing it; the sweep clears a
//!   dead target. `AddToKeptObjects` (§9.12) puts the target into the
//!   heap's kept set, a root until `ClearKeptObjects` (§9.11).
//! - A `FinalizationRegistry` holds cells: a target and an unregister
//!   token, both weak, and a held value, which the registry traces. When
//!   the collector finds a cell's target dead, the cell becomes pending
//!   and the registry goes into the heap's cleanup queue (the jobs of
//!   `HostEnqueueFinalizationRegistryCleanupJob`, §9.5.6; the queue holds
//!   the registries weakly, so a dead registry calls no callback). The
//!   host runs the queue later (`Runtime::run_finalization_cleanup`):
//!   no script code runs during a collection. `unregister` finds the cells
//!   of a token through an index, so it costs the cells of that token, not
//!   all cells.
//!
//! Handles carry a generation, so a key, a target or a token that died
//! never compares equal to a later object in the same slot; the sweep
//! removes them in the same collection anyway, so the memory returns.

use std::collections::VecDeque;

use crate::error::{Error, Result, Termination};
use crate::heap::{Gc, HandleMap, Heap, Tracer};
use crate::object::{Object, ObjectKind, Table};
use crate::value::{Symbol, Value};

/// A value that can be held weakly: an object or a symbol that is not
/// registered (the caller checks the registry; `CanBeHeldWeakly`, §9.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum WeakKey {
    Object(Gc<Object>),
    Symbol(Gc<Symbol>),
}

impl WeakKey {
    /// The key of an object or symbol value (without the registry check).
    pub(crate) fn of(value: Value) -> Option<WeakKey> {
        match value {
            Value::Object(object) => Some(WeakKey::Object(object)),
            Value::Symbol(symbol) => Some(WeakKey::Symbol(symbol)),
            _ => None,
        }
    }

    /// The key as a value.
    pub(crate) fn value(self) -> Value {
        match self {
            WeakKey::Object(object) => Value::Object(object),
            WeakKey::Symbol(symbol) => Value::Symbol(symbol),
        }
    }

    /// Marks the key (for the roots that hold keys strongly).
    pub(crate) fn trace(self, tracer: &mut Tracer<'_>) {
        match self {
            WeakKey::Object(object) => tracer.object(object),
            WeakKey::Symbol(symbol) => tracer.symbol(symbol),
        }
    }
}

/// The internal slot of a `WeakRef` (`[[WeakRefTarget]]`).
#[derive(Debug)]
pub struct WeakRefData {
    /// The target; `None` after the collector found it dead.
    pub(crate) target: Option<WeakKey>,
}

/// A cell of a registry (§26.2.1: `[[WeakRefTarget]]`, `[[HeldValue]]`,
/// `[[UnregisterToken]]`).
#[derive(Debug)]
struct Cell {
    /// Incremented when the cell is freed, so that a stale reference to
    /// it (in the pending list or the token index) is recognized.
    generation: u32,
    state: CellState,
}

#[derive(Debug)]
enum CellState {
    Free,
    /// The target is alive (as far as the last collection knows).
    Live {
        target: WeakKey,
        held: Value,
        token: Option<WeakKey>,
    },
    /// The target died; the cell waits for the cleanup job.
    Pending {
        held: Value,
        token: Option<WeakKey>,
    },
}

/// A reference to a cell: its index and generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellRef {
    index: u32,
    generation: u32,
}

/// The internal slots of a `FinalizationRegistry` (§26.2.1.1).
#[derive(Debug)]
pub struct Registry {
    /// `[[CleanupCallback]]`.
    pub(crate) callback: Value,
    /// The cells; freed cells are reused.
    cells: Vec<Cell>,
    free: Vec<u32>,
    /// The cells whose targets died, in the order the collector found them.
    pending: VecDeque<CellRef>,
    /// Unregister token → its cells (some may be stale; a list is pruned
    /// when it doubles).
    tokens: HandleMap<WeakKey, Vec<CellRef>>,
    /// Whether the registry is in the heap's cleanup queue.
    pub(crate) queued: bool,
}

impl Registry {
    /// An empty registry (a placeholder while the collector sweeps one).
    pub(crate) fn empty() -> Self {
        Registry::new(Value::Undefined)
    }

    fn new(callback: Value) -> Self {
        Registry {
            callback,
            cells: Vec::new(),
            free: Vec::new(),
            pending: VecDeque::new(),
            tokens: HandleMap::default(),
            queued: false,
        }
    }

    /// The bytes that the registry owns.
    pub(crate) fn heap_size(&self) -> usize {
        size_of::<Registry>()
            + self.cells.capacity() * size_of::<Cell>()
            + self.free.capacity() * size_of::<u32>()
            + self.pending.capacity() * size_of::<CellRef>()
            + self.tokens.capacity() * (size_of::<(WeakKey, Vec<CellRef>)>() + 1)
            + self
                .tokens
                .values()
                .map(|list| list.capacity() * size_of::<CellRef>())
                .sum::<usize>()
    }

    /// Reports the callback and the held values (strong) to the tracer;
    /// targets and tokens are weak.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.value(self.callback);
        for cell in &self.cells {
            match cell.state {
                CellState::Live { held, .. } | CellState::Pending { held, .. } => {
                    tracer.value(held);
                }
                CellState::Free => {}
            }
        }
    }

    fn cell(&self, at: CellRef) -> Option<&Cell> {
        self.cells
            .get(at.index as usize)
            .filter(|cell| cell.generation == at.generation)
    }

    /// Adds a cell (`register`, §26.2.3.2 steps 6 and 7). The memory is
    /// reserved first, so a failure leaves the registry as it was.
    fn register(&mut self, target: WeakKey, held: Value, token: Option<WeakKey>) -> Result<()> {
        if self.free.is_empty() {
            u32::try_from(self.cells.len()).map_err(|_| oom())?;
            self.cells.try_reserve(1)?;
        }
        if let Some(token) = token {
            self.tokens.try_reserve(1)?;
            let cells = &self.cells;
            let list = self.tokens.entry(token).or_default();
            if list.len() >= 4 && list.len().is_power_of_two() {
                list.retain(|&at| {
                    cells
                        .get(at.index as usize)
                        .is_some_and(|cell| cell.generation == at.generation)
                });
            }
            if let Err(error) = list.try_reserve(1) {
                if list.is_empty() {
                    self.tokens.remove(&token);
                }
                return Err(error.into());
            }
        }
        let state = CellState::Live {
            target,
            held,
            token,
        };
        let at = if let Some(index) = self.free.pop() {
            let cell = self
                .cells
                .get_mut(index as usize)
                .ok_or(Error::invariant("a free registry cell outside the cells"))?;
            cell.state = state;
            CellRef {
                index,
                generation: cell.generation,
            }
        } else {
            let index = u32::try_from(self.cells.len()).map_err(|_| oom())?;
            self.cells.push(Cell {
                generation: 0,
                state,
            });
            CellRef {
                index,
                generation: 0,
            }
        };
        if let Some(list) = token.and_then(|token| self.tokens.get_mut(&token)) {
            list.push(at);
        }
        Ok(())
    }

    /// Frees a cell.
    fn free_cell(&mut self, at: CellRef) {
        if let Some(cell) = self.cells.get_mut(at.index as usize)
            && cell.generation == at.generation
            && !matches!(cell.state, CellState::Free)
        {
            cell.state = CellState::Free;
            cell.generation = cell.generation.wrapping_add(1);
            // The free list never needs more room than the cells.
            if self.free.try_reserve(1).is_ok() {
                self.free.push(at.index);
            }
        }
    }

    /// `unregister` (§26.2.3.3 steps 4 to 6): removes the cells of `token`,
    /// live or pending; returns whether there were any.
    fn unregister(&mut self, token: WeakKey) -> bool {
        let Some(list) = self.tokens.remove(&token) else {
            return false;
        };
        let mut removed = false;
        for at in list {
            if self
                .cell(at)
                .is_some_and(|cell| !matches!(cell.state, CellState::Free))
            {
                self.free_cell(at);
                removed = true;
            }
        }
        removed
    }

    /// The held value of the next pending cell, which is removed
    /// (`CleanupFinalizationRegistry`, §9.13 step 3).
    fn take_pending(&mut self) -> Option<Value> {
        while let Some(at) = self.pending.pop_front() {
            if let Some(Cell {
                state: CellState::Pending { held, .. },
                ..
            }) = self.cell(at)
            {
                let held = *held;
                self.free_cell(at);
                return Some(held);
            }
        }
        None
    }

    /// Whether cells wait for the cleanup job.
    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The sweep of a live registry: cells whose targets `live` rejects
    /// become pending, tokens that died are dropped. Returns whether a
    /// cell became pending.
    pub(crate) fn sweep(&mut self, live: impl Fn(WeakKey) -> bool) -> bool {
        let mut found = false;
        for (index, cell) in self.cells.iter_mut().enumerate() {
            let CellState::Live {
                target,
                held,
                token,
            } = cell.state
            else {
                continue;
            };
            if live(target) {
                continue;
            }
            cell.state = CellState::Pending { held, token };
            // The pending list is at most as long as the cells; a failed
            // reservation drops the cell's callback, never a panic.
            if self.pending.try_reserve(1).is_ok() {
                self.pending.push_back(CellRef {
                    index: index as u32,
                    generation: cell.generation,
                });
                found = true;
            }
        }
        let cells = &mut self.cells;
        self.tokens.retain(|&token, list| {
            if live(token) {
                return true;
            }
            for at in list.iter() {
                if let Some(cell) = cells.get_mut(at.index as usize)
                    && cell.generation == at.generation
                {
                    match &mut cell.state {
                        CellState::Live { token, .. } | CellState::Pending { token, .. } => {
                            *token = None;
                        }
                        CellState::Free => {}
                    }
                }
            }
            false
        });
        crate::string::shrink_if_sparse(&mut self.tokens);
        found
    }

    /// The number of cells that are not free (for tests).
    #[cfg(test)]
    fn cell_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|cell| !matches!(cell.state, CellState::Free))
            .count()
    }
}

fn oom() -> Error {
    Error::from(Termination::OutOfMemory)
}

impl Heap {
    // --- WeakMap and WeakSet ---

    /// A new empty `WeakMap` (`map` true) or `WeakSet` object.
    pub(crate) fn new_weak_collection(
        &mut self,
        proto: Option<Gc<Object>>,
        map: bool,
    ) -> Result<Gc<Object>> {
        let table = Box::new(Table::new());
        let kind = if map {
            ObjectKind::WeakMap(table)
        } else {
            ObjectKind::WeakSet(table)
        };
        self.new_object_with_kind(proto, kind)
    }

    /// The table of a `WeakMap` or `WeakSet` object; `None` for any other
    /// object.
    pub(crate) fn weak_table(&self, object: Gc<Object>) -> Result<Option<&Table>> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::WeakMap(table) | ObjectKind::WeakSet(table) => Some(table),
            _ => None,
        })
    }

    fn weak_table_ref(&self, object: Gc<Object>) -> Result<&Table> {
        self.weak_table(object)?.ok_or(Error::invariant(
            "a weak collection operation on another object",
        ))
    }

    fn weak_table_mut(&mut self, object: Gc<Object>) -> Result<&mut Table> {
        match &mut self.object_mut(object)?.kind {
            ObjectKind::WeakMap(table) | ObjectKind::WeakSet(table) => Ok(table),
            _ => Err(Error::invariant(
                "a weak collection operation on another object",
            )),
        }
    }

    /// The hash and the position of the entry of `key`.
    fn weak_locate(&self, object: Gc<Object>, key: WeakKey) -> Result<(u64, Option<usize>)> {
        let table = self.weak_table_ref(object)?;
        let key = key.value();
        let hash = self.key_hash(key)?;
        if table.len() == 0 {
            return Ok((hash, None));
        }
        Ok((hash, table.find(hash, |stored| stored == key)))
    }

    /// The value of the entry of `key` (a `WeakSet` stores `undefined`).
    pub(crate) fn weak_get(&self, object: Gc<Object>, key: WeakKey) -> Result<Option<Value>> {
        let Some(position) = self.weak_locate(object, key)?.1 else {
            return Ok(None);
        };
        Ok(self
            .weak_table_ref(object)?
            .entry(position)
            .map(|entry| entry.value))
    }

    /// Sets the value of the entry of `key`, or adds an entry
    /// (`WeakMap.prototype.set`, `WeakSet.prototype.add`). Growth checks
    /// the heap limit first and is charged; it never collects.
    pub(crate) fn weak_put(
        &mut self,
        object: Gc<Object>,
        key: WeakKey,
        value: Value,
    ) -> Result<()> {
        let (hash, position) = self.weak_locate(object, key)?;
        if let Some(position) = position {
            self.weak_table_mut(object)?.set_value(position, value);
            return Ok(());
        }
        let growth = self.weak_table_ref(object)?.insert_growth();
        self.check_room(growth)?;
        let table = self.weak_table_mut(object)?;
        let before = table.heap_size();
        table.insert(hash, key.value(), value)?;
        let after = table.heap_size();
        self.charge(after.saturating_sub(before));
        Ok(())
    }

    /// Deletes the entry of `key`; returns whether there was one.
    pub(crate) fn weak_remove(&mut self, object: Gc<Object>, key: WeakKey) -> Result<bool> {
        let Some(position) = self.weak_locate(object, key)?.1 else {
            return Ok(false);
        };
        self.weak_table_mut(object)?.remove(position);
        Ok(true)
    }

    // --- WeakRef and the kept set ---

    /// A new `WeakRef` object with `target` (the caller adds the target to
    /// the kept set).
    pub(crate) fn new_weak_ref(
        &mut self,
        proto: Option<Gc<Object>>,
        target: WeakKey,
    ) -> Result<Gc<Object>> {
        let data = Box::new(WeakRefData {
            target: Some(target),
        });
        self.new_object_with_kind(proto, ObjectKind::WeakRef(data))
    }

    /// The target of a `WeakRef` (`None` once it died); an error for
    /// another object.
    pub(crate) fn weak_ref_target(&self, object: Gc<Object>) -> Result<Option<WeakKey>> {
        match &self.object(object)?.kind {
            ObjectKind::WeakRef(data) => Ok(data.target),
            _ => Err(Error::invariant("a WeakRef operation on another object")),
        }
    }

    // --- FinalizationRegistry ---

    /// A new `FinalizationRegistry` object with a cleanup callback.
    pub(crate) fn new_registry(
        &mut self,
        proto: Option<Gc<Object>>,
        callback: Value,
    ) -> Result<Gc<Object>> {
        let registry = Box::new(Registry::new(callback));
        self.new_object_with_kind(proto, ObjectKind::FinalizationRegistry(registry))
    }

    fn registry_mut(&mut self, object: Gc<Object>) -> Result<&mut Registry> {
        match &mut self.object_mut(object)?.kind {
            ObjectKind::FinalizationRegistry(registry) => Ok(registry),
            _ => Err(Error::invariant(
                "a FinalizationRegistry operation on another object",
            )),
        }
    }

    /// The registry of an object, if it is a `FinalizationRegistry`.
    pub(crate) fn registry(&self, object: Gc<Object>) -> Result<Option<&Registry>> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::FinalizationRegistry(registry) => Some(registry),
            _ => None,
        })
    }

    /// Adds a cell (`register`); checks the heap limit and charges the
    /// growth.
    pub(crate) fn registry_register(
        &mut self,
        object: Gc<Object>,
        target: WeakKey,
        held: Value,
        token: Option<WeakKey>,
    ) -> Result<()> {
        // At most one cell, one token entry and a doubled list.
        self.check_room(256)?;
        let registry = self.registry_mut(object)?;
        let before = registry.heap_size();
        registry.register(target, held, token)?;
        let after = registry.heap_size();
        self.charge(after.saturating_sub(before));
        Ok(())
    }

    /// Removes the cells of `token`; returns whether there were any.
    pub(crate) fn registry_unregister(
        &mut self,
        object: Gc<Object>,
        token: WeakKey,
    ) -> Result<bool> {
        Ok(self.registry_mut(object)?.unregister(token))
    }

    /// The held value of the next pending cell of a registry, which is
    /// removed; `None` when no cell waits.
    pub(crate) fn registry_take_pending(&mut self, object: Gc<Object>) -> Result<Option<Value>> {
        Ok(self.registry_mut(object)?.take_pending())
    }

    /// Takes the next registry of the cleanup queue (it is no longer
    /// queued, so a later collection queues it again).
    pub(crate) fn take_cleanup(&mut self) -> Option<Gc<Object>> {
        while let Some(object) = self.weak_roots.cleanup.pop_front() {
            if let Ok(registry) = self.registry_mut(object) {
                registry.queued = false;
                return Some(object);
            }
        }
        None
    }

    /// Puts a registry back into the cleanup queue if cells still wait
    /// (after a callback threw).
    pub(crate) fn requeue_cleanup(&mut self, object: Gc<Object>) -> Result<()> {
        let registry = self.registry_mut(object)?;
        if registry.queued || !registry.has_pending() {
            return Ok(());
        }
        registry.queued = true;
        self.weak_roots.cleanup.try_reserve(1)?;
        self.weak_roots.cleanup.push_back(object);
        Ok(())
    }

    /// The number of registries in the cleanup queue.
    pub fn pending_cleanups(&self) -> usize {
        self.weak_roots.cleanup.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::HeapConfig;

    fn key(heap: &mut Heap) -> WeakKey {
        WeakKey::Object(heap.new_object(None).unwrap())
    }

    #[test]
    fn unregister_removes_live_and_pending_cells() {
        let mut heap = Heap::new(HeapConfig::default());
        let mut registry = Registry::new(Value::Undefined);
        let (a, b, token) = (key(&mut heap), key(&mut heap), key(&mut heap));
        registry.register(a, Value::Int(1), Some(token)).unwrap();
        registry.register(b, Value::Int(2), Some(token)).unwrap();
        // `a` dies: its cell is pending, `b`'s is live.
        assert!(registry.sweep(|k| k != a));
        assert!(registry.has_pending());
        assert!(registry.unregister(token));
        assert!(!registry.unregister(token));
        assert_eq!(registry.cell_count(), 0);
        assert_eq!(registry.take_pending(), None);
    }

    #[test]
    fn freed_cells_are_reused_without_stale_matches() {
        let mut heap = Heap::new(HeapConfig::default());
        let mut registry = Registry::new(Value::Undefined);
        let (a, b, token) = (key(&mut heap), key(&mut heap), key(&mut heap));
        registry.register(a, Value::Int(1), Some(token)).unwrap();
        assert!(registry.sweep(|k| k != a));
        // Unregistered while pending: the pending reference is stale.
        assert!(registry.unregister(token));
        registry.register(b, Value::Int(2), None).unwrap();
        assert_eq!(registry.cells.len(), 1, "the freed cell is reused");
        assert_eq!(registry.take_pending(), None, "the new cell is not pending");
        assert!(registry.sweep(|k| k != b));
        assert_eq!(registry.take_pending(), Some(Value::Int(2)));
        assert_eq!(registry.take_pending(), None);
    }

    #[test]
    fn dead_tokens_are_dropped() {
        let mut heap = Heap::new(HeapConfig::default());
        let mut registry = Registry::new(Value::Undefined);
        let (a, token) = (key(&mut heap), key(&mut heap));
        registry.register(a, Value::Int(1), Some(token)).unwrap();
        assert!(!registry.sweep(|k| k != token));
        assert!(registry.tokens.is_empty());
        assert!(!registry.unregister(token));
        assert_eq!(registry.cell_count(), 1);
    }

    #[test]
    fn token_lists_stay_short_when_cells_are_cleaned_up() {
        let mut heap = Heap::new(HeapConfig::default());
        let mut registry = Registry::new(Value::Undefined);
        let token = key(&mut heap);
        for i in 0..10_000 {
            let target = key(&mut heap);
            registry
                .register(target, Value::Int(i), Some(token))
                .unwrap();
            registry.sweep(|k| k != target);
            assert_eq!(registry.take_pending(), Some(Value::Int(i)));
        }
        let list = registry.tokens.get(&token).map_or(0, Vec::len);
        assert!(list <= 8, "the token list has {list} references");
        assert_eq!(registry.cells.len(), 1);
    }
}
