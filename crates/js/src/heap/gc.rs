//! The collector: stop-the-world mark-and-sweep (ADR 0026 section 8,
//! memo 4.4).
//!
//! 1. Mark: the roots (the caller's [`RootSource`], handle scopes,
//!    persistent roots, the engine's atoms) go through the [`Tracer`],
//!    which sets mark bits in bit vectors beside the arenas and pushes
//!    things with children onto an explicit work list. No recursion. Large
//!    vectors (slots, dense and sparse elements, dictionaries) are marked
//!    in parts of [`CHUNK`] entries, so that the work list stays short.
//! 2. Weak tables (atoms, transitions, root shapes) drop the entries
//!    whose things are not marked; shared key lists drop the entries that
//!    no live shape uses.
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

use crate::heap::arena::MarkBits;
use crate::heap::{Arenas, Gc, GcStats, Generic, Heap, RootSource};
use crate::object::{Elements, KeyList, Object, Property, Shape, ShapeKind};
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
    Slots { object: u32, start: usize },
    Dense { object: u32, start: usize },
    Sparse { object: u32, start: u32 },
    Shape(u32),
    Dictionary { shape: u32, start: usize },
    Cell(u32),
    Generic(u32),
}

/// Marks things during a collection. Root sources and generic data report
/// their handles to it.
pub struct Tracer<'a> {
    arenas: &'a Arenas,
    marks: &'a mut Marks,
    work: Vec<Work>,
    /// Handles that did not refer to a live slot (rooting bugs).
    stale: usize,
}

impl Tracer<'_> {
    /// Marks the thing that a value refers to.
    pub fn value(&mut self, value: Value) {
        match value {
            Value::String(string) => self.string(string),
            Value::Symbol(symbol) => self.symbol(symbol),
            Value::BigInt(bigint) => self.bigint(bigint),
            Value::Object(object) => self.object(object),
            Value::Undefined
            | Value::Null
            | Value::Bool(_)
            | Value::Int(_)
            | Value::Double(_)
            | Value::Empty => {}
        }
    }

    /// Marks an object.
    pub fn object(&mut self, object: Gc<Object>) {
        if self.mark(
            object,
            |arenas| arenas.objects.contains(object),
            |marks| &mut marks.objects,
        ) {
            self.work.push(Work::Object(object.index()));
        }
    }

    /// Marks a string.
    pub fn string(&mut self, string: Gc<JsString>) {
        self.mark(
            string,
            |arenas| arenas.strings.contains(string),
            |marks| &mut marks.strings,
        );
    }

    /// Marks a symbol and its description.
    pub fn symbol(&mut self, symbol: Gc<Symbol>) {
        if self.mark(
            symbol,
            |arenas| arenas.symbols.contains(symbol),
            |marks| &mut marks.symbols,
        ) && let Ok(Symbol {
            description: Some(description),
        }) = self.arenas.symbols.get(symbol)
        {
            self.string(*description);
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
        let arenas = self.arenas;
        while let Some(work) = self.work.pop() {
            match work {
                Work::Object(index) => {
                    let Some(object) = arenas.objects.at(index) else {
                        continue;
                    };
                    self.shape(object.shape);
                    object.kind.trace(self);
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
                Work::Generic(index) => {
                    if let Some(generic) = arenas.generic.at(index) {
                        generic.0.trace(self);
                    }
                }
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
            names,
            ..
        } = self;
        marks.reset(arenas);
        let mut tracer = Tracer {
            arenas,
            marks,
            work: Vec::new(),
            stale: 0,
        };
        roots.trace_roots(&mut tracer);
        heap_roots.trace(&mut tracer);
        tracer.string(names.length);
        tracer.drain();
        // Ephemerons (WeakMap, WeakSet; M7 feature 9) go here: mark the
        // values of entries whose keys are marked, drain, and repeat until
        // nothing new is marked.
        let stale = tracer.stale;

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
        add(arenas.objects.sweep(&marks.objects, Object::heap_size));
        add(arenas.strings.sweep(&marks.strings, JsString::heap_size));
        add(arenas.symbols.sweep(&marks.symbols, |_| 0));
        add(arenas
            .bigints
            .sweep(&marks.bigints, |b| b.limbs.capacity() * 8));
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
            + self.roots.bytes();

        self.live_bytes = live;
        self.allocated = 0;
        self.stats = GcStats {
            collections: self.stats.collections + 1,
            last_pause: start.elapsed(),
            last_freed: freed,
            live_bytes: live,
            last_stale_roots: stale,
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
