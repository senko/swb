//! The heap: arenas, allocation, byte accounting, safepoints and
//! reservations (ADR 0026 section 8).
//!
//! The heap owns one arena per kind of heap thing. A collection runs only
//! at a safepoint: [`Heap::safepoint`], [`Heap::reserve`], or an internal
//! method that grows a buffer whose size a script controls (those take a
//! [`RootSource`] parameter, so their signature shows that they can
//! collect). An allocation of a small, fixed size never collects.
//!
//! Accounting: every allocation charges its bytes (the slot and the
//! buffers it owns, approximately). `allocated` counts the bytes charged
//! since the last collection; a collection measures the live bytes
//! exactly. A collection is due when `allocated` passes the threshold
//! (live bytes times the growth factor, at least the minimum), or when the
//! heap size passes the limit. If the heap stays above the limit after a
//! collection, the script ends ([`Termination::HeapLimit`]).
//!
//! What the heap size counts: the capacity of every slot vector, free
//! slots included, plus the capacity of the buffers that live things own
//! (slots, elements, strings, shapes, key lists), the weak tables and the
//! roots. After a sweep an arena gives back its trailing free slots, so the
//! heap size follows the memory that the process holds, within the slack of
//! free slots in the middle of a vector and of buffers grown since the last
//! collection. Not counted (transient or outside the heap): the mark bits
//! and the work list of a collection, and Rust vectors that the caller owns
//! (for example the result of `own_property_keys`; reserve their size
//! first).
//!
//! Near the limit: if a collection leaves the live size above 90 % of the
//! limit, [`Heap::safepoint`] ends the script with
//! [`Termination::HeapLimit`]. Otherwise every safepoint would run a full
//! collection that frees almost nothing.

mod arena;
mod gc;
mod roots;

use std::any::Any;
use std::time::Duration;

use swb_js_text::{Str16, String16};

pub use arena::Gc;
pub(crate) use arena::{Arena, MarkBits};
pub use gc::Tracer;
/// A map for handle keys (a keyed hasher; see `swb_js_text::hash`).
pub(crate) type HandleMap<K, V> = swb_js_text::hash::KeyedMap<K, V>;
pub use roots::{HandleScope, NoRoots, Persistent, Root, RootSource};

use crate::error::{Error, Result, Termination};
use crate::object::{KeyList, Object, Shape, Transition};
use crate::string::{AtomTable, JsString, PropertyKey, array_index, decimal};
use crate::value::{BigInt, Symbol, Value, ValueCell};

/// The heap limits and collection policy.
#[derive(Clone, Copy, Debug)]
pub struct HeapConfig {
    /// The heap size above which the script ends, in bytes.
    pub limit: usize,
    /// The smallest collection threshold, in bytes.
    pub min_threshold: usize,
    /// The threshold is the live size after a collection times this.
    pub growth_factor: usize,
    /// Collect at every safepoint and reservation (finds rooting bugs).
    pub stress: bool,
}

impl Default for HeapConfig {
    fn default() -> Self {
        HeapConfig {
            limit: 1 << 30,
            min_threshold: 16 << 20,
            growth_factor: 2,
            stress: false,
        }
    }
}

/// Data of the generic heap kind, for things that the object model does
/// not know (code objects of the compiler, later environments).
pub trait GenericData: Any {
    /// Reports every heap handle that the data holds.
    fn trace(&self, tracer: &mut Tracer<'_>);
    /// The bytes of the buffers that the data owns (not its own size).
    fn heap_size(&self) -> usize;
}

/// A slot of the generic kind.
pub struct Generic(Box<dyn GenericData>);

/// Statistics of the collector.
#[derive(Clone, Copy, Debug, Default)]
pub struct GcStats {
    /// The number of collections so far.
    pub collections: u64,
    /// The pause of the last collection.
    pub last_pause: Duration,
    /// The pauses of all collections so far.
    pub total_pause: Duration,
    /// The slots that the last collection freed.
    pub last_freed: usize,
    /// The live bytes after the last collection.
    pub live_bytes: usize,
    /// Stale handles that the last collection found in roots (rooting
    /// bugs; each is ignored).
    pub last_stale_roots: usize,
    /// Stale handles in roots over all collections so far (a nonzero
    /// value is a rooting bug).
    pub total_stale_roots: usize,
}

/// The arenas, one per kind.
pub(crate) struct Arenas {
    pub(crate) objects: Arena<Object>,
    pub(crate) strings: Arena<JsString>,
    pub(crate) symbols: Arena<Symbol>,
    pub(crate) bigints: Arena<BigInt>,
    pub(crate) cells: Arena<ValueCell>,
    pub(crate) shapes: Arena<Shape>,
    pub(crate) key_lists: Arena<KeyList>,
    pub(crate) generic: Arena<Generic>,
}

/// The atoms that the engine itself uses; always alive.
pub(crate) struct Names {
    pub(crate) length: Gc<JsString>,
}

/// The JavaScript heap of one runtime.
pub struct Heap {
    pub(crate) arenas: Arenas,
    marks: gc::Marks,
    pub(crate) atoms: AtomTable,
    /// Prototype → its root shape (weak in both).
    pub(crate) root_shapes: HandleMap<Option<Gc<Object>>, Gc<Shape>>,
    /// (parent shape, key, attributes) → child shape (weak).
    pub(crate) transitions: HandleMap<Transition, Gc<Shape>>,
    roots: roots::HeapRoots,
    pub(crate) names: Names,
    config: HeapConfig,
    /// Live bytes measured by the last collection.
    live_bytes: usize,
    /// Bytes charged since the last collection.
    allocated: usize,
    /// Depth of the regions in which collections are not allowed.
    no_gc: u32,
    stats: GcStats,
}

impl Heap {
    /// An empty heap.
    pub fn new(config: HeapConfig) -> Self {
        let mut strings = Arena::new("string");
        let mut atoms = AtomTable::new();
        let mut length = JsString::new(String16::from("length"));
        length.set_atom();
        let length = strings
            .insert(length)
            .expect("an empty arena has room for the first atom");
        atoms.insert(atoms.hash(Str16::Latin1(b"length")), length);
        Heap {
            arenas: Arenas {
                objects: Arena::new("object"),
                strings,
                symbols: Arena::new("symbol"),
                bigints: Arena::new("BigInt"),
                cells: Arena::new("cell"),
                shapes: Arena::new("shape"),
                key_lists: Arena::new("key list"),
                generic: Arena::new("generic"),
            },
            marks: gc::Marks::default(),
            atoms,
            root_shapes: HandleMap::default(),
            transitions: HandleMap::default(),
            roots: roots::HeapRoots::default(),
            names: Names { length },
            config,
            live_bytes: 0,
            allocated: 0,
            no_gc: 0,
            stats: GcStats::default(),
        }
    }

    /// The configuration.
    pub fn config(&self) -> &HeapConfig {
        &self.config
    }

    /// Collector statistics.
    pub fn stats(&self) -> GcStats {
        self.stats
    }

    // --- Accounting ---------------------------------------------------

    /// The estimated heap size: the live bytes of the last collection plus
    /// the bytes charged since.
    #[inline]
    pub fn heap_size(&self) -> usize {
        self.live_bytes.saturating_add(self.allocated)
    }

    /// The bytes charged since the last collection.
    pub fn allocated_since_gc(&self) -> usize {
        self.allocated
    }

    /// The collection threshold for [`Heap::allocated_since_gc`].
    #[inline]
    pub fn threshold(&self) -> usize {
        self.live_bytes
            .saturating_mul(self.config.growth_factor)
            .max(self.config.min_threshold)
    }

    /// Adds `bytes` to the accounting. Never collects, never fails.
    pub(crate) fn charge(&mut self, bytes: usize) {
        self.allocated = self.allocated.saturating_add(bytes);
    }

    /// Whether a collection is due at the next safepoint.
    #[inline]
    pub fn gc_due(&self) -> bool {
        self.config.stress
            || self.allocated > self.threshold()
            || self.heap_size() > self.config.limit
    }

    // --- Safepoints ---------------------------------------------------

    /// A safepoint: collects if a collection is due. Returns whether it
    /// collected. Fails with [`Termination::HeapLimit`] if the heap is
    /// above 90 % of the limit after the collection (see the module
    /// documentation). In a no-GC region it does not collect and fails when
    /// the heap size is over the limit.
    pub fn safepoint(&mut self, roots: &dyn RootSource) -> Result<bool> {
        if self.no_gc > 0 {
            if self.heap_size() > self.config.limit {
                return Err(Termination::HeapLimit.into());
            }
            return Ok(false);
        }
        if !self.gc_due() {
            return Ok(false);
        }
        self.collect(roots);
        if self.heap_size() > self.config.limit - self.config.limit / 10 {
            return Err(Termination::HeapLimit.into());
        }
        Ok(true)
    }

    /// Makes room for an allocation of `bytes` whose size a script
    /// controls. Collects if the request would pass the threshold or the
    /// limit (or always, in the stress mode), then checks the limit. It
    /// does not charge the bytes; the allocation does.
    ///
    /// Call it before the operation creates handles that are not rooted.
    pub fn reserve(&mut self, bytes: usize, roots: &dyn RootSource) -> Result<()> {
        let would_pass_threshold = self.allocated.saturating_add(bytes) > self.threshold();
        let would_pass_limit = self.heap_size().saturating_add(bytes) > self.config.limit;
        if self.no_gc == 0 && (self.config.stress || would_pass_threshold || would_pass_limit) {
            self.collect(roots);
        }
        if self.heap_size().saturating_add(bytes) > self.config.limit {
            return Err(Termination::HeapLimit.into());
        }
        Ok(())
    }

    /// Fails with [`Termination::HeapLimit`] when `bytes` more would put
    /// the heap size over the limit. Does not collect (it takes `&self`);
    /// for a buffer outside the heap that a script sizes.
    pub fn check_room(&self, bytes: usize) -> Result<()> {
        if self.heap_size().saturating_add(bytes) > self.config.limit {
            return Err(Termination::HeapLimit.into());
        }
        Ok(())
    }

    /// Starts a region in which no collection runs (for example the end
    /// of a compile, ADR 0026 section 3). Safepoints and reservations in
    /// the region only check the limit. Regions nest.
    pub fn enter_no_gc(&mut self) {
        self.no_gc += 1;
    }

    /// Ends a region of [`Heap::enter_no_gc`].
    pub fn leave_no_gc(&mut self) {
        self.no_gc = self.no_gc.saturating_sub(1);
    }

    /// The depth of the no-GC regions (0: collections are allowed).
    pub fn no_gc_depth(&self) -> u32 {
        self.no_gc
    }

    /// Sets the depth of the no-GC regions back to `depth`, as an unwinder
    /// does after an early return skipped a [`Heap::leave_no_gc`]. Read the
    /// depth with [`Heap::no_gc_depth`] before the region.
    pub fn restore_no_gc_depth(&mut self, depth: u32) {
        self.no_gc = depth;
    }

    // --- Access -------------------------------------------------------

    /// The string of a handle.
    #[inline]
    pub fn string(&self, handle: Gc<JsString>) -> Result<&JsString> {
        self.arenas.strings.get(handle)
    }

    /// The symbol of a handle.
    pub fn symbol(&self, handle: Gc<Symbol>) -> Result<&Symbol> {
        self.arenas.symbols.get(handle)
    }

    /// The `BigInt` of a handle.
    pub fn bigint(&self, handle: Gc<BigInt>) -> Result<&BigInt> {
        self.arenas.bigints.get(handle)
    }

    /// The cell of a handle.
    pub fn cell(&self, handle: Gc<ValueCell>) -> Result<&ValueCell> {
        self.arenas.cells.get(handle)
    }

    /// The cell of a handle, for writing.
    pub fn cell_mut(&mut self, handle: Gc<ValueCell>) -> Result<&mut ValueCell> {
        self.arenas.cells.get_mut(handle)
    }

    /// The object of a handle.
    #[inline]
    pub fn object(&self, handle: Gc<Object>) -> Result<&Object> {
        self.arenas.objects.get(handle)
    }

    /// The generic data of a handle, if it has the type `T`.
    pub fn generic<T: GenericData>(&self, handle: Gc<Generic>) -> Result<&T> {
        let data: &dyn Any = &*self.arenas.generic.get(handle)?.0;
        data.downcast_ref()
            .ok_or(Error::invariant("generic data of another type"))
    }

    /// Fails with the stale-handle error if `handle` is not live generic
    /// data.
    pub(crate) fn generic_check(&self, handle: Gc<Generic>) -> Result<()> {
        self.arenas.generic.get(handle).map(|_| ())
    }

    /// Whether a handle refers to a live object.
    pub fn is_live_object(&self, handle: Gc<Object>) -> bool {
        self.arenas.objects.contains(handle)
    }

    /// Whether a handle refers to a live string.
    pub fn is_live_string(&self, handle: Gc<JsString>) -> bool {
        self.arenas.strings.contains(handle)
    }

    /// The number of live things of each kind: objects, strings, symbols,
    /// shapes.
    pub fn counts(&self) -> HeapCounts {
        HeapCounts {
            objects: self.arenas.objects.len(),
            strings: self.arenas.strings.len(),
            symbols: self.arenas.symbols.len(),
            shapes: self.arenas.shapes.len(),
            atoms: self.atoms.len(),
            transitions: self.transitions.len(),
            root_shapes: self.root_shapes.len(),
        }
    }

    // --- Allocation ---------------------------------------------------

    /// A new string. The caller reserved the bytes of a long string
    /// before it built the text.
    pub fn alloc_string(&mut self, text: String16) -> Result<Gc<JsString>> {
        let string = JsString::new(text);
        self.charge(Arena::<JsString>::slot_size() + string.heap_size());
        self.arenas.strings.insert(string)
    }

    /// A new string from UTF-8 text.
    pub fn alloc_str(&mut self, text: &str) -> Result<Gc<JsString>> {
        self.alloc_string(String16::from(text))
    }

    /// The atom with the content `text`, created if needed.
    pub fn intern(&mut self, text: Str16<'_>) -> Result<Gc<JsString>> {
        let hash = self.atoms.hash(text);
        if let Some(atom) = self.atoms.find(hash, text, &self.arenas.strings) {
            return Ok(atom);
        }
        let mut string = JsString::new(text.to_string16());
        string.set_atom();
        self.charge(Arena::<JsString>::slot_size() + string.heap_size() + 16);
        let atom = self.arenas.strings.insert(string)?;
        self.atoms.insert(hash, atom);
        Ok(atom)
    }

    /// The atom with the content of UTF-8 `text`.
    pub fn intern_str(&mut self, text: &str) -> Result<Gc<JsString>> {
        self.intern(String16::from(text).as_str16())
    }

    /// The atom with the content of `string`: the string itself if it
    /// becomes the atom, or the existing atom.
    pub fn intern_string(&mut self, string: Gc<JsString>) -> Result<Gc<JsString>> {
        let existing = self.string(string)?;
        if existing.is_atom() {
            return Ok(string);
        }
        let text = existing.as_str16();
        let hash = self.atoms.hash(text);
        if let Some(atom) = self.atoms.find(hash, text, &self.arenas.strings) {
            return Ok(atom);
        }
        self.arenas.strings.get_mut(string)?.set_atom();
        self.charge(16);
        self.atoms.insert(hash, string);
        Ok(string)
    }

    /// A new symbol with a description.
    pub fn alloc_symbol(&mut self, description: Option<Gc<JsString>>) -> Result<Gc<Symbol>> {
        self.charge(Arena::<Symbol>::slot_size());
        self.arenas.symbols.insert(Symbol { description })
    }

    /// A new `BigInt`.
    pub fn alloc_bigint(&mut self, value: BigInt) -> Result<Gc<BigInt>> {
        self.charge(Arena::<BigInt>::slot_size() + value.limbs.capacity() * 8);
        self.arenas.bigints.insert(value)
    }

    /// A new cell.
    pub fn alloc_cell(&mut self, value: Value) -> Result<Gc<ValueCell>> {
        self.charge(Arena::<ValueCell>::slot_size());
        self.arenas.cells.insert(ValueCell { value })
    }

    /// A new thing of the generic kind.
    pub fn alloc_generic<T: GenericData>(&mut self, data: T) -> Result<Gc<Generic>> {
        self.charge(Arena::<Generic>::slot_size() + size_of::<T>() + data.heap_size());
        self.arenas.generic.insert(Generic(Box::new(data)))
    }

    // --- Property keys ------------------------------------------------

    /// The property key of a text: an array index or an atom.
    pub fn key_from_str16(&mut self, text: Str16<'_>) -> Result<PropertyKey> {
        if let Some(index) = array_index(text) {
            return Ok(PropertyKey::Index(index));
        }
        Ok(PropertyKey::String(self.intern(text)?))
    }

    /// The property key of UTF-8 text.
    pub fn key_from_str(&mut self, text: &str) -> Result<PropertyKey> {
        self.key_from_str16(String16::from(text).as_str16())
    }

    /// The property key of a string value.
    pub fn key_from_string(&mut self, string: Gc<JsString>) -> Result<PropertyKey> {
        if let Some(index) = array_index(self.string(string)?.as_str16()) {
            return Ok(PropertyKey::Index(index));
        }
        Ok(PropertyKey::String(self.intern_string(string)?))
    }

    /// The property key of a non-negative integer: an array index below
    /// 2^32 − 1, otherwise the atom of its decimal text.
    pub fn key_from_u32(&mut self, value: u32) -> Result<PropertyKey> {
        if value < crate::string::MAX_ARRAY_LENGTH {
            return Ok(PropertyKey::Index(value));
        }
        Ok(PropertyKey::String(self.intern(decimal(value).as_str16())?))
    }

    /// The value of a key, as `[[OwnPropertyKeys]]` lists it: a string (an
    /// index becomes its decimal text) or a symbol.
    pub fn key_to_value(&mut self, key: PropertyKey) -> Result<Value> {
        Ok(match key {
            PropertyKey::Index(index) => Value::String(self.alloc_string(decimal(index))?),
            PropertyKey::String(atom) => Value::String(atom),
            PropertyKey::Symbol(symbol) => Value::Symbol(symbol),
        })
    }

    /// The atom `"length"`.
    pub fn length_atom(&self) -> Gc<JsString> {
        self.names.length
    }
}

/// The number of live things per kind, and the sizes of the weak tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapCounts {
    /// Live objects.
    pub objects: usize,
    /// Live strings (atoms included).
    pub strings: usize,
    /// Live symbols.
    pub symbols: usize,
    /// Live shapes.
    pub shapes: usize,
    /// Entries of the atom table.
    pub atoms: usize,
    /// Entries of the transition index.
    pub transitions: usize,
    /// Entries of the root shape table.
    pub root_shapes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoms_are_unique_per_content() {
        let mut heap = Heap::new(HeapConfig::default());
        let a = heap.intern_str("foo").unwrap();
        let b = heap.intern(Str16::Wide(&[0x66, 0x6F, 0x6F])).unwrap();
        assert_eq!(a, b);
        let plain = heap.alloc_str("bar").unwrap();
        let atom = heap.intern_string(plain).unwrap();
        assert_eq!(atom, plain);
        assert_eq!(heap.intern_str("bar").unwrap(), plain);
        let other = heap.alloc_str("foo").unwrap();
        assert_eq!(heap.intern_string(other).unwrap(), a);
        assert_eq!(heap.intern_str("length").unwrap(), heap.length_atom());
    }

    #[test]
    fn keys_and_their_strings() {
        let mut heap = Heap::new(HeapConfig::default());
        assert_eq!(heap.key_from_str("7").unwrap(), PropertyKey::Index(7));
        let key = heap.key_from_str("07").unwrap();
        assert!(matches!(key, PropertyKey::String(_)));
        let max = heap.key_from_u32(u32::MAX).unwrap();
        assert_eq!(max, heap.key_from_str("4294967295").unwrap());
        assert_eq!(
            heap.key_from_u32(4_294_967_294).unwrap(),
            PropertyKey::Index(4_294_967_294)
        );
        let Value::String(text) = heap.key_to_value(PropertyKey::Index(42)).unwrap() else {
            panic!("an index key gives a string");
        };
        assert!(heap.string(text).unwrap().as_str16().eq_str("42"));
        let s = heap.alloc_str("12").unwrap();
        assert_eq!(heap.key_from_string(s).unwrap(), PropertyKey::Index(12));
    }

    #[test]
    fn generic_data_downcasts() {
        struct Code(Vec<Value>);
        impl GenericData for Code {
            fn trace(&self, tracer: &mut Tracer<'_>) {
                for value in &self.0 {
                    tracer.value(*value);
                }
            }
            fn heap_size(&self) -> usize {
                self.0.capacity() * 16
            }
        }
        struct Other;
        impl GenericData for Other {
            fn trace(&self, _: &mut Tracer<'_>) {}
            fn heap_size(&self) -> usize {
                0
            }
        }
        struct CodeRoot(Gc<Generic>);
        impl RootSource for CodeRoot {
            fn trace_roots(&self, tracer: &mut Tracer<'_>) {
                tracer.generic(self.0);
            }
        }
        let mut heap = Heap::new(HeapConfig::default());
        let s = heap.alloc_str("constant").unwrap();
        let code = heap.alloc_generic(Code(vec![s.into()])).unwrap();
        assert_eq!(heap.generic::<Code>(code).unwrap().0.len(), 1);
        assert!(heap.generic::<Other>(code).is_err());
        heap.collect(&CodeRoot(code));
        assert!(heap.is_live_string(s));
        heap.collect(&NoRoots);
        assert!(heap.generic::<Code>(code).is_err());
        assert!(!heap.is_live_string(s));
    }
}
