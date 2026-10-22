//! The collector: stop-the-world mark-and-sweep (ADR 0026 section 8,
//! memo 4.4).
//!
//! 1. Mark: the roots (the caller's [`RootSource`], handle scopes,
//!    persistent roots, the engine's atoms) go through the [`Tracer`],
//!    which sets mark bits in bit vectors beside the arenas and pushes
//!    things with children (rope nodes included) onto an explicit work
//!    list. No recursion. Large
//!    vectors (slots, dense and sparse elements, dictionaries) are marked
//!    in parts of [`CHUNK`] entries, so that the work list stays short.
//!    Ephemerons (`WeakMap` entries, Hayes 1997) are marked in a second
//!    phase (see [`Ephemerons`]): a value is marked only when its key is.
//! 2. The weak objects that are alive (`WeakMap`, `WeakSet`, `WeakRef`,
//!    `FinalizationRegistry`) drop or clear what died, before the sweep
//!    frees the slots ([`sweep_weak_objects`]). Weak tables (atoms,
//!    transitions, root shapes) drop the entries whose things are not
//!    marked; shared key lists drop the entries that no live shape uses.
//! 3. Sweep: every unmarked slot is freed (its data dropped, its
//!    generation incremented); trailing free slots go back to the
//!    allocator; the live bytes are measured: the capacity of every slot
//!    vector (free slots included) plus the buffers the live things own.
//!
//! Transient memory: the mark bits and the work list exist only during a
//! collection and are outside the heap limit. The work list is bounded by
//! the number of live things (each is pushed at most once, and a large
//! vector is one item per [`CHUNK`] entries), so it is at most a few
//! bytes per live thing.

use std::ops::Bound;
use std::time::Instant;

use std::collections::VecDeque;

use crate::heap::arena::MarkBits;
use crate::heap::{Arenas, Gc, GcStats, Generic, HandleMap, Heap, RootSource};
use crate::object::{
    Elements, KeyList, Object, ObjectKind, Property, Registry, Shape, ShapeKind, WeakKey,
};
use crate::string::{JsString, PropertyKey, shrink_if_sparse};
use crate::value::{BigInt, Symbol, Value, ValueCell};

/// The number of entries that one work item marks.
const CHUNK: usize = 1024;

/// The mark bits of all arenas.
#[derive(Default)]
pub(crate) struct Marks {
    objects: MarkBits,
    strings: MarkBits,
    symbols: MarkBits,
    bigints: MarkBits,
    cells: MarkBits,
    shapes: MarkBits,
    key_lists: MarkBits,
    generic: MarkBits,
}

impl Marks {
    fn reset(&mut self, arenas: &Arenas) {
        self.objects.reset(arenas.objects.capacity());
        self.strings.reset(arenas.strings.capacity());
        self.symbols.reset(arenas.symbols.capacity());
        self.bigints.reset(arenas.bigints.capacity());
        self.cells.reset(arenas.cells.capacity());
        self.shapes.reset(arenas.shapes.capacity());
        self.key_lists.reset(arenas.key_lists.capacity());
        self.generic.reset(arenas.generic.capacity());
    }

    /// Frees all bit vectors. They are transient memory: they exist during
    /// a collection only and are not part of the heap size.
    fn release(&mut self) {
        self.objects.release();
        self.strings.release();
        self.symbols.release();
        self.bigints.release();
        self.cells.release();
        self.shapes.release();
        self.key_lists.release();
        self.generic.release();
    }
}

/// A unit of marking work: a marked thing whose children are not yet
/// marked, or a part of a large vector.
enum Work {
    Object(u32),
    Slots {
        object: u32,
        start: usize,
    },
    Dense {
        object: u32,
        start: usize,
    },
    Sparse {
        object: u32,
        start: u32,
    },
    Shape(u32),
    Dictionary {
        shape: u32,
        start: usize,
    },
    Cell(u32),
    Generic(u32),
    /// A rope node: its two children.
    Rope(u32),
    /// A part of the entries of a `WeakMap` in the ephemeron phase.
    Ephemerons {
        object: u32,
        start: usize,
    },
    /// Values whose key was just marked: a list of waiting nodes.
    Wake(u32),
}

/// The end of a list of waiting ephemeron values.
const NO_NODE: u32 = u32::MAX;

/// The weak objects found while marking, and the state of the ephemeron
/// phase.
///
/// The ephemeron algorithm (Hayes, "Ephemerons: a new finalization
/// mechanism", OOPSLA 1997, with a key-indexed list of waiting values in
/// place of repeated scans): the first phase marks from the roots and only
/// records the live `WeakMap`s. The second phase visits each entry of each
/// live table once (in parts of [`CHUNK`] entries): an entry whose key is
/// marked marks its value; the value of any other entry waits in a list
/// under its key. When the drain of the second phase takes a marked object
/// from the work list, or the marker marks a symbol, and the key has
/// waiting values (a bit per slot says so without a lookup), its list goes
/// on the work list. Tables found in the second phase are visited when
/// they are marked. The first phase runs without any of these checks. So every entry is visited once and waits at most once, and
/// every waiting value is marked at most once: the work is linear in the
/// number of entries and of marked things, also for chains of ephemerons
/// in any order and across any number of tables. The values that still
/// wait at the end belong to dead keys and are not marked.
#[derive(Default)]
struct Ephemerons {
    /// Whether the second phase runs.
    active: bool,
    /// The live `WeakMap` and `WeakSet` objects (slot indices).
    maps: Vec<u32>,
    sets: Vec<u32>,
    /// The live `WeakRef` and `FinalizationRegistry` objects.
    holders: Vec<u32>,
    /// Key → the first node of its list of waiting values.
    waiting: HandleMap<WeakKey, u32>,
    /// The nodes: a value and the next node of its list.
    nodes: Vec<(Value, u32)>,
    /// One bit per object or symbol slot: the key has waiting values.
    waiting_objects: MarkBits,
    waiting_symbols: MarkBits,
    bits_ready: bool,
}

/// The weak objects that the marking found alive.
struct WeakFound {
    maps: Vec<u32>,
    sets: Vec<u32>,
    holders: Vec<u32>,
}

/// Marks things during a collection. Root sources and generic data report
/// their handles to it.
pub struct Tracer<'a> {
    arenas: &'a Arenas,
    marks: &'a mut Marks,
    work: Vec<Work>,
    /// Handles that did not refer to a live slot (rooting bugs).
    stale: usize,
    weak: Ephemerons,
}

impl Tracer<'_> {
    /// Marks the thing that a value refers to.
    pub fn value(&mut self, value: Value) {
        match value {
            Value::String(string) => self.string(string),
            Value::Symbol(symbol) => self.symbol(symbol),
            Value::BigInt(bigint) => self.bigint(bigint),
            Value::Object(object) => self.object(object),
            Value::Cell(cell) => self.cell(cell),
            Value::Undefined
            | Value::Null
            | Value::Bool(_)
            | Value::Int(_)
            | Value::Double(_)
            | Value::Empty => {}
        }
    }

    /// Marks an object.
    #[inline]
    pub fn object(&mut self, object: Gc<Object>) {
        if self.mark(
            object,
            |arenas| arenas.objects.contains(object),
            |marks| &mut marks.objects,
        ) {
            self.work.push(Work::Object(object.index()));
        }
    }

    /// Marks a string (and through the work list the children of a rope).
    pub fn string(&mut self, string: Gc<JsString>) {
        if self.mark(
            string,
            |arenas| arenas.strings.contains(string),
            |marks| &mut marks.strings,
        ) && self.arenas.strings.get(string).is_ok_and(JsString::is_rope)
        {
            self.work.push(Work::Rope(string.index()));
        }
    }

    /// Marks a symbol and its description.
    pub fn symbol(&mut self, symbol: Gc<Symbol>) {
        if !self.mark(
            symbol,
            |arenas| arenas.symbols.contains(symbol),
            |marks| &mut marks.symbols,
        ) {
            return;
        }
        if self.weak.active {
            self.wake(WeakKey::Symbol(symbol));
        }
        if let Ok(Symbol {
            description: Some(description),
            ..
        }) = self.arenas.symbols.get(symbol)
        {
            self.string(*description);
        }
    }

    // --- Weak objects and ephemerons ---

    /// Records a live `WeakMap` (its entries are ephemerons).
    pub(crate) fn weak_map(&mut self, index: u32) {
        self.weak.maps.push(index);
        if self.weak.active {
            self.work.push(Work::Ephemerons {
                object: index,
                start: 0,
            });
        }
    }

    /// Records a live `WeakSet` (its keys are not traced).
    pub(crate) fn weak_set(&mut self, index: u32) {
        self.weak.sets.push(index);
    }

    /// Records a live `WeakRef` or `FinalizationRegistry` (its targets and
    /// tokens are not traced).
    pub(crate) fn weak_holder(&mut self, index: u32) {
        self.weak.holders.push(index);
    }

    /// Starts the ephemeron phase: the tables found so far are visited.
    fn start_ephemerons(&mut self) {
        self.weak.active = true;
        for &object in &self.weak.maps {
            self.work.push(Work::Ephemerons { object, start: 0 });
        }
    }

    /// Visits up to [`CHUNK`] entries of a `WeakMap` from `start`.
    fn ephemerons(&mut self, object: u32, start: usize) {
        let arenas = self.arenas;
        let Some(ObjectKind::WeakMap(table)) = arenas.objects.at(object).map(|o| &o.kind) else {
            return;
        };
        let end = start + CHUNK;
        if end < table.entry_len() {
            self.work.push(Work::Ephemerons { object, start: end });
        }
        for entry in table.entries_in(start, end) {
            if !is_heap_value(entry.value) {
                continue;
            }
            let Some(key) = WeakKey::of(entry.key) else {
                continue;
            };
            if weak_live(arenas, self.marks, key) {
                self.value(entry.value);
            } else {
                self.wait(key, entry.value);
            }
        }
    }

    /// Puts `value` on the list of `key`, which is not marked yet.
    fn wait(&mut self, key: WeakKey, value: Value) {
        let weak = &mut self.weak;
        if !weak.bits_ready {
            weak.waiting_objects.reset(self.arenas.objects.capacity());
            weak.waiting_symbols.reset(self.arenas.symbols.capacity());
            weak.bits_ready = true;
        }
        match key {
            WeakKey::Object(object) => weak.waiting_objects.set(object.index() as usize),
            WeakKey::Symbol(symbol) => weak.waiting_symbols.set(symbol.index() as usize),
        };
        let Ok(node) = u32::try_from(weak.nodes.len()) else {
            // More than 2^32 waiting values cannot fit the heap limit; mark
            // the value (it lives one collection longer).
            self.value(value);
            return;
        };
        let head = weak.waiting.entry(key).or_insert(NO_NODE);
        weak.nodes.push((value, *head));
        *head = node;
    }

    /// A key was marked: its waiting values go on the work list (for an
    /// object when the second phase takes it from the work list, for a
    /// symbol when it is marked).
    #[cold]
    #[inline(never)]
    fn wake(&mut self, key: WeakKey) {
        let weak = &mut self.weak;
        let waits = match key {
            WeakKey::Object(object) => weak.waiting_objects.get(object.index() as usize),
            WeakKey::Symbol(symbol) => weak.waiting_symbols.get(symbol.index() as usize),
        };
        if !waits {
            return;
        }
        if let Some(node) = weak.waiting.remove(&key) {
            self.work.push(Work::Wake(node));
        }
    }

    /// Marks up to [`CHUNK`] values of a list of waiting values.
    fn wake_values(&mut self, mut node: u32) {
        for _ in 0..CHUNK {
            let Some(&(value, next)) = self.weak.nodes.get(node as usize) else {
                return;
            };
            self.value(value);
            node = next;
        }
        if node != NO_NODE {
            self.work.push(Work::Wake(node));
        }
    }

    /// Marks a `BigInt`.
    pub fn bigint(&mut self, bigint: Gc<BigInt>) {
        self.mark(
            bigint,
            |arenas| arenas.bigints.contains(bigint),
            |marks| &mut marks.bigints,
        );
    }

    /// Marks a cell.
    pub fn cell(&mut self, cell: Gc<ValueCell>) {
        if self.mark(
            cell,
            |arenas| arenas.cells.contains(cell),
            |marks| &mut marks.cells,
        ) {
            self.work.push(Work::Cell(cell.index()));
        }
    }

    /// Marks a thing of the generic kind.
    pub fn generic(&mut self, generic: Gc<Generic>) {
        if self.mark(
            generic,
            |arenas| arenas.generic.contains(generic),
            |marks| &mut marks.generic,
        ) {
            self.work.push(Work::Generic(generic.index()));
        }
    }

    /// Marks the string or symbol of a property key.
    pub fn key(&mut self, key: PropertyKey) {
        match key {
            PropertyKey::Index(_) => {}
            PropertyKey::String(string) => self.string(string),
            PropertyKey::Symbol(symbol) => self.symbol(symbol),
        }
    }

    pub(crate) fn shape(&mut self, shape: Gc<Shape>) {
        if self.mark(
            shape,
            |arenas| arenas.shapes.contains(shape),
            |marks| &mut marks.shapes,
        ) {
            self.work.push(Work::Shape(shape.index()));
        }
    }

    fn key_list(&mut self, list: Gc<KeyList>) {
        self.mark(
            list,
            |arenas| arenas.key_lists.contains(list),
            |marks| &mut marks.key_lists,
        );
    }

    /// Sets the mark bit of a live handle; returns whether it was clear.
    #[inline]
    fn mark<T>(
        &mut self,
        handle: Gc<T>,
        live: impl Fn(&Arenas) -> bool,
        bits: impl Fn(&mut Marks) -> &mut MarkBits,
    ) -> bool {
        if !live(self.arenas) {
            self.stale += 1;
            return false;
        }
        bits(self.marks).set(handle.index() as usize)
    }

    fn property(&mut self, property: &Property) {
        match *property {
            Property::Data { value, .. } => self.value(value),
            Property::Accessor { get, set, .. } => {
                self.value(get);
                self.value(set);
            }
        }
    }

    /// Marks until the work list is empty.
    fn drain(&mut self) {
        self.drain_phase::<false>();
    }

    /// Marks until the work list is empty; with `EPHEMERONS`, an object
    /// taken from the work list wakes the values that wait for it (only
    /// the second phase pays for the check).
    fn drain_phase<const EPHEMERONS: bool>(&mut self) {
        let arenas = self.arenas;
        while let Some(work) = self.work.pop() {
            match work {
                Work::Object(index) => {
                    let Some(object) = arenas.objects.at(index) else {
                        continue;
                    };
                    if EPHEMERONS
                        && self.weak.waiting_objects.get(index as usize)
                        && let Some(handle) = arenas.objects.handle_at(index)
                    {
                        self.wake(WeakKey::Object(handle));
                    }
                    self.shape(object.shape);
                    // Most objects have no payload to trace.
                    if !matches!(object.kind, ObjectKind::Ordinary | ObjectKind::Array { .. }) {
                        object.kind.trace(index, self);
                    }
                    self.slots(object, index, 0);
                    match &object.elements {
                        Elements::Dense { .. } => self.dense(object, index, 0),
                        Elements::Sparse(_) => self.sparse(object, index, 0),
                    }
                }
                Work::Slots { object, start } => {
                    if let Some(record) = arenas.objects.at(object) {
                        self.slots(record, object, start);
                    }
                }
                Work::Dense { object, start } => {
                    if let Some(record) = arenas.objects.at(object) {
                        self.dense(record, object, start);
                    }
                }
                Work::Sparse { object, start } => {
                    if let Some(record) = arenas.objects.at(object) {
                        self.sparse(record, object, start);
                    }
                }
                Work::Shape(index) => {
                    if let Some(shape) = arenas.shapes.at(index) {
                        self.shape_children(shape, index);
                    }
                }
                Work::Dictionary { shape, start } => {
                    if let Some(record) = arenas.shapes.at(shape) {
                        self.dictionary(record, shape, start);
                    }
                }
                Work::Cell(index) => {
                    if let Some(cell) = arenas.cells.at(index) {
                        self.value(cell.value);
                    }
                }
                Work::Rope(index) => {
                    if let Some(rope) = arenas.strings.at(index).and_then(JsString::as_rope) {
                        self.string(rope.left);
                        self.string(rope.right);
                    }
                }
                Work::Generic(index) => {
                    if let Some(generic) = arenas.generic.at(index) {
                        generic.0.trace(self);
                    }
                }
                Work::Ephemerons { object, start } => self.ephemerons(object, start),
                Work::Wake(node) => self.wake_values(node),
            }
        }
    }

    fn slots(&mut self, object: &Object, index: u32, start: usize) {
        let end = (start + CHUNK).min(object.slots.len());
        if end < object.slots.len() {
            self.work.push(Work::Slots {
                object: index,
                start: end,
            });
        }
        for value in object.slots.get(start..end).unwrap_or_default() {
            self.value(*value);
        }
    }

    fn dense(&mut self, object: &Object, index: u32, start: usize) {
        let Elements::Dense { values, .. } = &object.elements else {
            return;
        };
        let end = (start + CHUNK).min(values.len());
        if end < values.len() {
            self.work.push(Work::Dense {
                object: index,
                start: end,
            });
        }
        for value in values.get(start..end).unwrap_or_default() {
            self.value(*value);
        }
    }

    fn sparse(&mut self, object: &Object, index: u32, start: u32) {
        let Elements::Sparse(map) = &object.elements else {
            return;
        };
        let mut last = None;
        for (count, (&key, property)) in map
            .range((Bound::Included(start), Bound::Unbounded))
            .enumerate()
        {
            if count == CHUNK {
                last = Some(key);
                break;
            }
            self.property(property);
        }
        if let Some(next) = last {
            self.work.push(Work::Sparse {
                object: index,
                start: next,
            });
        }
    }

    fn shape_children(&mut self, shape: &Shape, index: u32) {
        if let Some(proto) = shape.proto {
            self.object(proto);
        }
        match &shape.kind {
            ShapeKind::Shared(shared) => {
                if let Some(parent) = shared.parent {
                    self.shape(parent);
                }
                self.key_list(shared.keys);
                // A shared shape has at most DICTIONARY_LIMIT keys.
                if let Ok(entries) = shape.entries(&self.arenas.key_lists) {
                    for entry in entries {
                        self.key(entry.key);
                    }
                }
            }
            ShapeKind::Dictionary(_) => self.dictionary(shape, index, 0),
        }
    }

    fn dictionary(&mut self, shape: &Shape, index: u32, start: usize) {
        let ShapeKind::Dictionary(dictionary) = &shape.kind else {
            return;
        };
        let end = (start + CHUNK).min(dictionary.entries.len());
        if end < dictionary.entries.len() {
            self.work.push(Work::Dictionary {
                shape: index,
                start: end,
            });
        }
        for entry in dictionary.entries.get(start..end).unwrap_or_default() {
            if !entry.flags.deleted() {
                self.key(entry.key);
            }
        }
    }
}

impl Heap {
    /// Runs a full collection now. `roots` are the roots outside the heap
    /// (the VM's stacks, frames and realms). Returns the statistics.
    pub fn collect(&mut self, roots: &dyn RootSource) -> GcStats {
        let start = Instant::now();
        let Heap {
            arenas,
            marks,
            roots: heap_roots,
            weak_roots,
            names,
            ..
        } = self;
        marks.reset(arenas);
        let mut tracer = Tracer {
            arenas,
            marks,
            work: Vec::new(),
            stale: 0,
            weak: Ephemerons::default(),
        };
        roots.trace_roots(&mut tracer);
        heap_roots.trace(&mut tracer);
        weak_roots.trace(&mut tracer);
        tracer.string(names.length);
        tracer.drain();
        if !tracer.weak.maps.is_empty() {
            tracer.start_ephemerons();
            tracer.drain_phase::<true>();
        }
        let stale = tracer.stale;
        let found = WeakFound {
            maps: tracer.weak.maps,
            sets: tracer.weak.sets,
            holders: tracer.weak.holders,
        };

        // The cleanup queue holds its registries weakly.
        let (arenas, marks) = (&self.arenas, &self.marks);
        self.weak_roots
            .cleanup
            .retain(|&registry| weak_live(arenas, marks, WeakKey::Object(registry)));
        sweep_weak_objects(
            &mut self.arenas,
            &self.marks,
            &found,
            &mut self.weak_roots.cleanup,
        );
        self.sweep_weak_tables();
        trim_key_lists(&mut self.arenas, &self.marks);

        let marks = &self.marks;
        let arenas = &mut self.arenas;
        let mut freed = 0;
        let mut live = 0;
        let mut add = |(f, l): (usize, usize)| {
            freed += f;
            live += l;
        };
        add(arenas
            .objects
            .sweep(&marks.objects, |o| o.heap_size() + o.kind.heap_size()));
        add(arenas.strings.sweep(&marks.strings, JsString::heap_size));
        add(arenas.symbols.sweep(&marks.symbols, |_| 0));
        add(arenas.bigints.sweep(&marks.bigints, BigInt::heap_bytes));
        add(arenas.cells.sweep(&marks.cells, |_| 0));
        add(arenas.shapes.sweep(&marks.shapes, Shape::heap_size));
        add(arenas.key_lists.sweep(&marks.key_lists, KeyList::heap_size));
        add(arenas
            .generic
            .sweep(&marks.generic, |g| size_of_val(&*g.0) + g.0.heap_size()));
        // The slot vectors count with their whole capacity, free slots
        // included (the sweep gave back the trailing free slots).
        live += arenas.objects.slot_bytes()
            + arenas.strings.slot_bytes()
            + arenas.symbols.slot_bytes()
            + arenas.bigints.slot_bytes()
            + arenas.cells.slot_bytes()
            + arenas.shapes.slot_bytes()
            + arenas.key_lists.slot_bytes()
            + arenas.generic.slot_bytes();
        self.marks.release();
        live += self.atoms.bytes()
            + self.transitions.capacity() * size_of::<(crate::object::Transition, Gc<Shape>)>()
            + self.root_shapes.capacity() * size_of::<(Option<Gc<Object>>, Gc<Shape>)>()
            + self.roots.bytes()
            + self.weak_roots.bytes()
            + roots.extra_bytes();

        self.live_bytes = live;
        self.allocated = 0;
        self.update_trigger();
        let pause = start.elapsed();
        self.stats = GcStats {
            collections: self.stats.collections + 1,
            last_pause: pause,
            total_pause: self.stats.total_pause + pause,
            last_freed: freed,
            live_bytes: live,
            last_stale_roots: stale,
            total_stale_roots: self.stats.total_stale_roots + stale,
        };
        self.stats
    }

    /// Drops the entries of the weak tables whose things are not marked.
    fn sweep_weak_tables(&mut self) {
        let marks = &self.marks;
        self.atoms.sweep(&marks.strings);
        let shape_live = |shape: Gc<Shape>| marks.shapes.get(shape.index() as usize);
        let object_live = |object: Gc<Object>| marks.objects.get(object.index() as usize);
        let key_live = |key: PropertyKey| match key {
            PropertyKey::Index(_) => true,
            PropertyKey::String(string) => marks.strings.get(string.index() as usize),
            PropertyKey::Symbol(symbol) => marks.symbols.get(symbol.index() as usize),
        };
        self.root_shapes
            .retain(|proto, shape| proto.is_none_or(object_live) && shape_live(*shape));
        self.transitions.retain(|transition, child| {
            shape_live(transition.parent) && shape_live(*child) && key_live(transition.key)
        });
        shrink_if_sparse(&mut self.root_shapes);
        shrink_if_sparse(&mut self.transitions);
    }
}

/// Whether a value refers to a heap thing (a value that needs marking).
fn is_heap_value(value: Value) -> bool {
    matches!(
        value,
        Value::String(_) | Value::Symbol(_) | Value::BigInt(_) | Value::Object(_) | Value::Cell(_)
    )
}

/// Whether a weak key is marked (and its handle live).
fn weak_live(arenas: &Arenas, marks: &Marks, key: WeakKey) -> bool {
    match key {
        WeakKey::Object(object) => {
            marks.objects.get(object.index() as usize) && arenas.objects.contains(object)
        }
        WeakKey::Symbol(symbol) => {
            marks.symbols.get(symbol.index() as usize) && arenas.symbols.contains(symbol)
        }
    }
}

/// The sweep of the live weak objects, after the marking and before the
/// arenas free the slots: a `WeakMap` or `WeakSet` drops the entries whose
/// keys died, a `WeakRef` forgets a dead target, and a
/// `FinalizationRegistry` moves the cells with dead targets to its pending
/// list and goes into the cleanup queue. So no entry, target or token
/// outlives its slot, and the memory of dead entries returns in this
/// collection. Each object is taken out of its slot while it is swept, so
/// that the liveness checks can read the arenas.
fn sweep_weak_objects(
    arenas: &mut Arenas,
    marks: &Marks,
    found: &WeakFound,
    cleanup: &mut VecDeque<Gc<Object>>,
) {
    for &index in found.maps.iter().chain(&found.sets) {
        let Some(record) = arenas.objects.at_mut(index) else {
            continue;
        };
        let (ObjectKind::WeakMap(table) | ObjectKind::WeakSet(table)) = &mut record.kind else {
            continue;
        };
        let mut table = std::mem::take(&mut **table);
        table.remove_dead(|key| WeakKey::of(key).is_some_and(|key| !weak_live(arenas, marks, key)));
        if let Some(record) = arenas.objects.at_mut(index)
            && let ObjectKind::WeakMap(slot) | ObjectKind::WeakSet(slot) = &mut record.kind
        {
            **slot = table;
        }
    }
    for &index in &found.holders {
        let Some(record) = arenas.objects.at(index) else {
            continue;
        };
        match &record.kind {
            ObjectKind::WeakRef(data) => {
                let alive = data.target.is_some_and(|key| weak_live(arenas, marks, key));
                if !alive
                    && let Some(record) = arenas.objects.at_mut(index)
                    && let ObjectKind::WeakRef(data) = &mut record.kind
                {
                    data.target = None;
                }
            }
            ObjectKind::FinalizationRegistry(_) => {
                sweep_registry(arenas, marks, index, cleanup);
            }
            _ => {}
        }
    }
}

/// The sweep of a live `FinalizationRegistry` (see [`sweep_weak_objects`]).
fn sweep_registry(
    arenas: &mut Arenas,
    marks: &Marks,
    index: u32,
    cleanup: &mut VecDeque<Gc<Object>>,
) {
    let Some(handle) = arenas.objects.handle_at(index) else {
        return;
    };
    let Some(record) = arenas.objects.at_mut(index) else {
        return;
    };
    let ObjectKind::FinalizationRegistry(registry) = &mut record.kind else {
        return;
    };
    let mut registry = std::mem::replace(registry, Box::new(Registry::empty()));
    let found = registry.sweep(|key| weak_live(arenas, marks, key));
    // A failed reservation only delays this cleanup job (a later
    // collection that finds a dead target queues it).
    if found && !registry.queued && cleanup.try_reserve(1).is_ok() {
        registry.queued = true;
        cleanup.push_back(handle);
    }
    if let Some(record) = arenas.objects.at_mut(index)
        && let ObjectKind::FinalizationRegistry(slot) = &mut record.kind
    {
        *slot = registry;
    }
}

/// Drops the entries of shared key lists beyond the longest prefix that a
/// live shape uses, so that a list never holds keys of dead shapes (and
/// can be extended again instead of copied).
fn trim_key_lists(arenas: &mut Arenas, marks: &Marks) {
    let live_shared = |arenas: &Arenas| {
        arenas
            .shapes
            .iter()
            .filter(|(index, _)| marks.shapes.get(*index as usize))
            .filter_map(|(_, shape)| match &shape.kind {
                ShapeKind::Shared(shared) => Some((shared.keys, shared.len)),
                ShapeKind::Dictionary(_) => None,
            })
            .collect::<Vec<_>>()
    };
    let lists = live_shared(arenas);
    for &(list, _) in &lists {
        if let Ok(list) = arenas.key_lists.get_mut(list) {
            list.live_len = 0;
        }
    }
    for &(list, len) in &lists {
        if let Ok(list) = arenas.key_lists.get_mut(list) {
            list.live_len = list.live_len.max(len);
        }
    }
    for &(list, _) in &lists {
        if let Ok(list) = arenas.key_lists.get_mut(list) {
            list.entries.truncate(list.live_len as usize);
        }
    }
}
