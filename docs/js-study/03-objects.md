# Study memo 3: objects

Status: reviewed by the orchestrator on 2026-10-09 (ADR 0025).

This memo answers the questions of group 3 in
[README.md](README.md): property storage, arrays, caches for property
access, exotic objects, and built-in objects. It compares five small
engines (QuickJS, QuickJS-ng, Duktape, MuJS, MicroQuickJS) and the
literature. It describes ideas, not code. It does not decide the design.

Terms used in this memo:

- **Shape** (also "hidden class" or "map"): a shared description of the
  keys and attributes of a group of objects. Each object stores only the
  values. The Self papers call it a "map".
- **Transition**: the link from one shape to the shape that results when
  one more property is added.
- **Dictionary mode**: an object keeps its own private key table instead
  of a shared shape.
- **Elements**: properties whose key is an array index. **Named
  properties**: all other properties.
- **Inline cache (IC)**: a small cache that belongs to one property
  access instruction.
- **Invariant flag**: a global flag that says "a condition is still
  true" (for example "no prototype of an array has an indexed
  property"). Code clears the flag once when the condition breaks, and
  never sets it again.

---

## 3.1 Property storage

### Problem

An object maps property keys (strings, symbols, array indices) to a
value or an accessor pair, plus three attribute bits. Most objects are
small, and many objects have the same keys in the same order. The engine
must find a key fast, add keys fast, delete keys, and list keys in the
order that the specification requires. Memory per object matters,
because pages create millions of small objects.

### What the specification fixes

- The property model and the attributes: ECMA-262 §6.1.7 (The Object
  Type) and §6.1.7.2 (internal methods and slots).
- Key order: §10.1.11.1 OrdinaryOwnPropertyKeys. First all keys that are
  array indices, in ascending numeric order. Then all string keys in
  creation order. Then all symbol keys in creation order. "Array index"
  is defined in §6.1.7 (an integer from 0 to 2^32 − 2).
- `for`-`in` order: §14.7.5.9 EnumerateObjectProperties. The order is
  implementation-defined in general, but for ordinary objects without
  Proxy objects in the chain, and without mutation during the loop, it
  must match the order of §10.1.11.1 for each object of the prototype
  chain.
- The specification does not say how to store properties. Shapes,
  dictionaries and tombstones are invisible to scripts, if the key order
  is right.

### Options in the engines

**Shared shapes with a per-shape hash table (QuickJS, QuickJS-ng).**
The two QuickJS variants have the same design here. A shape holds the
prototype, an ordered list of (key, attribute bits) entries, and a small
hash table that maps a key to its position in the list. The object holds
a pointer to its shape and a separate array of value slots, in the same
order as the shape entries. A lookup hashes the key (keys are interned
32-bit integers, see memo 4), walks a short collision chain inside the
shape, and uses the position to read the value slot.

Shapes are shared through one global hash table per runtime. The table
key is the prototype plus the full sequence of (key, attributes)
entries. The hash of a shape is computed incrementally: the hash of the
parent shape, combined with the new key and its attributes. When a
script adds a property, the engine computes the hash of "current shape
plus this property" and looks for an existing shape with exactly that
content. If it finds one, the object switches to it. This has the same
effect as a transition tree, but there are no explicit parent-to-child
links: the global table is the transition index.

Details that matter:

- The prototype is part of the shape. Two objects with the same keys
  but different prototypes have different shapes. `new F()` starts from
  the shared empty shape of `F.prototype`.
- QuickJS frees a shape when its reference count drops to zero. If an
  object owns the only reference to a hashed shape, adding a property
  changes that shape in place (and moves it to its new slot in the global
  table) instead of creating a new one. This keeps the number of live
  shapes low when one object grows through many properties.
- Each shape stores the complete entry list, not only the last key. A
  chain of n shared shapes that are all alive costs O(n^2) memory. In
  practice the in-place growth above avoids most of these chains.
- Initial capacity is small (2 value slots, a hash table of 4 buckets).
  The value array grows by a factor of 1.5.

**Dictionary mode in QuickJS.** There is no separate dictionary data
structure. Any change other than "append a property with given
attributes" makes the object take a private, unshared copy of its shape
that is no longer in the global table. Such changes are: delete,
change of attributes, change of the prototype, and the first access to
a lazily created built-in property (see 3.5). From then on, the object
changes its private shape in place. I did not find a rule that moves an
object to dictionary mode because it has many properties.

**Deletion in QuickJS.** Delete marks the entry as free (a tombstone)
and unlinks it from the per-shape hash chain. The entry stays in the
list, so the creation order of the other keys stays correct. When at
least 8 entries are deleted and they are at least half of all entries,
the engine compacts the list and the value array. Deleting the last
element of a dense array is a special case (see 3.2).

**One ordered key table per object (Duktape).** Duktape has no shapes.
Each object owns its property table: parallel arrays of keys, values and
attribute bytes, in insertion order. New keys are always appended.
Delete clears the key and leaves a gap. The gap is removed only when
the table is reallocated, so the insertion order stays intact. An
optional open-addressing hash index maps keys to positions. Duktape
builds it only for objects with at least 8 properties (a build option);
smaller objects use a linear scan over the key array. The design note
argues that a linear scan over adjacent keys is often as fast as a hash
probe for small objects, and saves memory. The hash index is redundant:
the engine can drop and rebuild it at any time. Deleted hash slots are
marked and cleaned only on reallocation; the note shows that the table
cannot fill with deleted markers, because a reallocation always happens
first.

In the 3.0 development sources, Duktape keeps keys that are array
indices in a second, separate table (integer keys, own hash index),
apart from string and symbol keys. Arrays have a third store, a dense
item vector (see 3.2). The design notes in `doc/` still describe the
older layout with one combined allocation. Both layouts keep named
properties in insertion order.

**A balanced tree per object (MuJS).** MuJS stores each object's
properties in an AA tree (a balanced binary search tree) ordered by the
key string. Keys are not interned; lookups compare strings. Enumeration
walks the tree, so keys come out in sorted string order, not in creation
order. This was legal in ES5, but it does not satisfy §10.1.11.1. MuJS
is the simplest design and shows the cost of not following the modern
order.

**A hash table per object, partly in read-only memory (MicroQuickJS).**
Each object has its own hash table of (key, value, attributes) entries
with chaining. Keys are values: an interned string or a small integer.
The hash is derived from the key's address. Because the garbage collector
compacts the heap and moves interned strings, the engine rehashes every
object's table after each collection. Tables of built-in objects can
live in read-only memory and are copied to RAM on the first write (see
3.5).

### Literature

- Chambers, Ungar and Lee (Self, 1989) introduced maps: objects that are
  clones of one prototype share one map that holds the slot names and
  layout, and each object holds only its slot values. Adding a slot gives
  the object a new map. The paper reports large memory savings compared
  with a per-object dictionary.
- The V8 article "Fast properties in V8" describes the same idea for
  JavaScript: hidden classes in a transition tree, a fixed number of
  in-object slots plus an out-of-object backing store, and a fallback to
  dictionary mode for objects with many additions and deletions. It also keeps elements apart from named
  properties.
- Bynens and Meurer ("Shapes and inline caches") describe transition
  chains in which each shape records only the one key it adds and a link
  to its parent. A lookup then walks the chain, so engines add a
  key-to-entry table on demand for long chains.

### Where the engines agree and differ

- All engines that follow ES2015 or later keep string keys in creation
  order and use tombstones (gaps) for deletion, with compaction later.
- All of them intern string keys to integers or unique pointers, except
  MuJS.
- QuickJS shares key tables between objects; Duktape, MuJS and
  MicroQuickJS do not. Duktape and MicroQuickJS chose per-object tables
  to save code size and to stay simple.
- QuickJS puts the prototype into the shape. V8 (per the articles) does
  the same for the purpose of caching.
- Integer keys: QuickJS stores them in the same shape as other keys
  (when the object is not a dense array) and sorts them when it lists
  keys. Duktape 3 stores them in a separate table and also sorts on
  listing (with an insertion sort, which is quadratic). MuJS sorts all
  keys as strings, which gives the wrong order ("10" before "9").

### Trade-offs

- Shapes save memory when many objects share a layout, and they make
  caches possible (3.3): one shape comparison proves that a key is at a
  given slot. They cost a global table, more complex add and delete
  paths, and a policy for when to stop sharing.
- A per-object ordered table is simple and has no global state. Every
  lookup is a real search. Without shapes, there is nothing cheap that
  an inline cache can compare.
- Full key list per shape (QuickJS) gives O(1) lookup inside the shape.
  A parent-linked chain (V8 articles) gives O(1) memory per transition
  but needs a separate lookup table.
- Dictionary-mode triggers: QuickJS goes private on the first delete or
  attribute change and never back. A page that uses an object as a hash
  map with thousands of string keys creates one shape per key count in a
  transition design, unless a rule based on size or on the rate of
  changes sends it to dictionary mode. I did not find such a size rule in
  QuickJS.

### For swb

- In safe Rust, a shape is naturally an entry in a shape table addressed
  by a shape index, and an object holds a shape index plus a growable
  vector of value slots. Fixed in-object slots (V8) need variable-size
  objects, which are awkward without `unsafe`; a small inline array with
  spill to a vector is the safe approximation.
- A transition index is a hash map from (parent shape, key, attributes)
  to child shape. This is simpler to write in Rust than QuickJS's
  content hash with full comparison, and gives the same result.
- With a tracing GC there are no reference counts, so the QuickJS trick
  "change the shape in place if only one object uses it" is not
  available directly. Options: a transition tree that always creates a
  new shape, plus a dictionary-mode limit; or a per-shape "owned by one
  object" flag set when the object goes private.
- Shapes reference keys and the prototype, so the GC must trace them.
  The transition index must not keep dead shapes alive; it must be
  treated as weak and swept, or it grows without bound on hostile pages.
- If shape indices are reused after collection, every cache that holds a
  shape index (3.3) must be cleared at GC time, or indices need a
  generation counter.
- A dictionary-mode object fits a plain ordered map: a vector of entries
  in insertion order with tombstones, plus a hash index from key to
  position (Duktape's layout). Compaction renumbers positions, so
  `for`-`in` iterators and caches must not keep positions across it.
- Keep elements apart from named properties (Duktape 3, V8). Then
  §10.1.11.1 needs no sort of string keys, and the integer keys come
  from a dense vector (already in order) or a sparse ordered map.
- Attacker-chosen keys: per-object hash tables keyed by interned
  integers are safe from hash flooding only if the interning table itself
  uses a keyed hash (memo 4). Any sort at key-listing time must be
  O(n log n), not insertion sort.

---

## 3.2 Arrays

### Problem

Arrays are the most frequent indexed objects. Most are dense and grow at
the end. The specification allows holes, sparse indices up to
2^32 − 2, non-default attributes on single elements, accessors on
elements, and a `length` that can be set to any value. The engine must
keep the common case fast and compact and still handle the rare cases
correctly and without huge allocations.

### What the specification fixes

- Array exotic objects: ECMA-262 §10.4.2. Their only exotic internal
  method is [[DefineOwnProperty]]: defining an index at or above `length`
  increases `length`; defining `length` runs ArraySetLength (§10.4.2.4).
- ArraySetLength deletes elements from the end down to the new length.
  It stops at the first element that is not configurable and sets
  `length` to one above it. If `length` is not writable, no element at or
  above it can be added.
- A hole is a missing property. A read of a hole runs the ordinary
  [[Get]], which walks the prototype chain. So `Array.prototype[3] = 'x'`
  makes `[ , , , ][3]` return `'x'`.
- Typed arrays: TypedArray exotic objects, §10.4.5. Numeric keys never
  reach the prototype chain and never create properties. Out-of-range
  reads give `undefined`; out-of-range writes are ignored. ArrayBuffer:
  §25.1, including detached buffers and (since ES2024) resizable buffers
  with length-tracking views. TypedArray objects: §23.2.

### Options in the engines

**Dense vector without holes, all-or-nothing (QuickJS, QuickJS-ng).** A
new array has a dense vector of values and a count. `length` is stored
separately as the first property of the array shape, with a special
attribute bit so that the generic write path handles it. `length` may be
larger than the count (holes at the end are allowed; holes inside are
not). Rules:

- A write at index = count appends (vector grows by 1.5×).
- A write inside the vector replaces the value.
- A write beyond the count, a hole created by delete (except of the last
  element), an element with non-default attributes or an accessor, or an
  index above 2^31 − 1: the engine converts the whole array to ordinary
  properties in the shape, once, and never back.
- Deleting the last element only shrinks the count.
- Setting `length` smaller frees the tail of the vector.

The append fast path must also prove that no prototype has an indexed
setter or a read-only indexed property. QuickJS keeps an invariant flag
on `Array.prototype`. The flag is cleared when an integer key is added
to `Array.prototype` or `Object.prototype`. QuickJS-ng keeps the flag per
realm, also clears it when `Array.prototype` gets a new prototype, and
marks every object that serves as a prototype so that the check is
cheap. With the flag set, the append needs no prototype walk.

The same dense element path serves `arguments` objects and typed
arrays: one header bit says "elements are in a vector", and the element
access code switches on the object class to choose the element type.

For slow arrays, ArraySetLength chooses the cheaper of two methods: if
the length difference is smaller than the number of properties, it
deletes index by index; otherwise it scans all properties twice (first
to find the highest non-configurable index, then to delete). MuJS uses
the same idea. This bounds the work by the number of existing
properties, not by the length value, so `a.length = 4294967295; a.length
= 0` is cheap.

**Dense vector with hole markers and a density rule (Duktape).**
Duktape arrays (and `arguments` objects) have an item vector. An unused
slot holds a special "unused" marker that never escapes to scripts.
`length` is a separate integer field with a separate "not writable" bit,
not a stored property. The vector is comprehensive: while it exists,
every index key of the object is in it. It holds only elements with
default attributes. Rules:

- Writing beyond the vector grows it, unless the array would become too
  sparse. A quick test skips the density check when the new size is at
  most 12.5% above the old size. The full check counts used slots and
  drops the vector when fewer than 25% are used and the size is at
  least 257 (both are build options; by default, arrays of up to 256
  items always keep the vector).
- A non-default attribute or an accessor on an element drops the
  vector.
- Dropping the vector moves all items to the index-keyed table. The vector is
  never rebuilt.
- The fast path in the interpreter requires that the vector covers
  `length`.
- The design note says that array writes had no fast path, because a
  write to a missing element must check the prototype chain for setters
  and read-only properties. It proposes a per-object "has no indexed
  properties" flag for this check. The 3.0 sources have a separate code
  path for index keys; I did not check how they handle this case.

**Flat array without holes (MuJS).** A "simple" array has a flat vector
and only allows writes at indices up to the current flat length. Any
other index write, or a define with attributes, "unflattens" the array
into tree properties, once. Deleting the last element is allowed without
unflattening. MuJS caps arrays at 2^26 elements (a build constant).

**No holes at all (MicroQuickJS).** MicroQuickJS rejects writes beyond
the end with a TypeError and rejects array literals with holes. `length`
is an accessor on the prototype. This breaks the specification; it is
listed only as the extreme point of the design space.

**Typed arrays.** QuickJS: a typed array object points into the byte
data of its ArrayBuffer and caches its element count. The ArrayBuffer
keeps a list of all views. On detach or resize, the buffer walks the
list and updates each view's cached count (0 when detached). Element
access then needs one bounds check. The setter converts the value first
(which can run user code that detaches the buffer) and checks bounds
after the conversion. Duktape: a buffer object holds a reference to the
underlying byte store, an offset, a length and an element type. Every
access checks the requested range against the current size of the byte
store, right before the access, because user code can resize it. The
design note lists detachment as future work; I did not check whether
the 3.0 sources implement it.

### Literature

- "The Implementation of Lua 5.0" (Ierusalimschy, de Figueiredo,
  Celes): tables have an array part and a hash part. On rehash (only
  when the hash part is full), Lua sets the array part to the largest n
  such that at least half of the slots 1..n are in use. The array part
  takes about half the memory of the same entries in the hash part,
  because keys are implicit.
- "Fast properties in V8": elements are packed (no holes) or holey, and
  fast (a vector) or dictionary (a hash map). Very sparse arrays and
  elements with non-default attributes go to dictionary elements. V8 also
  specializes elements by content (small integers, doubles, any value);
  see the "elements kinds" articles. Holey arrays must check the
  prototype chain on a hole read; packed arrays do not.

### Where the engines agree and differ

- All engines keep a dense vector as the normal case and fall back to
  general properties, and none of them returns from the fallback.
- They differ on holes: QuickJS and MuJS have no internal holes (a hole
  forces the fallback); Duktape and V8 allow holes with a marker and use
  a density rule.
- They differ on `length`: a property with a special attribute bit
  (QuickJS), or a plain field (Duktape, MuJS).
- They agree that the prototype chain must be checked before an append
  or a hole read, and that a global invariant flag (QuickJS, QuickJS-ng)
  or a per-object flag (Duktape note) avoids the walk.
- Typed arrays: "push" (the buffer updates its views, QuickJS) versus
  "pull" (each view checks the buffer on each access, Duktape).

### Trade-offs

- No internal holes keeps the fast path simple: a slot in range always
  holds a value, and a read never walks the prototype chain. The cost is
  a permanent fallback for patterns like `a = []; a[5] = x;` or
  `new Array(n)` filled from the end. In QuickJS, `new Array(n)` gives a
  dense vector with count 0 and `length` n (trailing holes only), so a
  fill from index 0 upwards stays on the fast path.
- Hole markers keep such arrays fast but every read of a missing slot
  must still apply [[Get]] semantics, which an invariant flag makes
  cheap.
- A density rule (Lua, Duktape) bounds memory for `a[1e9] = 1` but
  needs a count of used slots, which costs a scan or a counter.
- Push-style typed array updates need a back-link list from buffer to
  views; pull-style checks cost one indirection per access.

### For swb

- A dense vector of values plus a separate `u32` length is the natural
  safe-Rust layout. A hole marker can be an internal variant of the
  value type that the engine never exposes to scripts; every exit from
  the element store must convert it.
- The fallback store for sparse elements can be an ordered map keyed by
  `u32` (for example a B-tree map). It returns keys in ascending order,
  which §10.1.11.1 needs, with no sort, and it makes ArraySetLength a
  range removal. Non-default attributes per element then live in the
  same map entry.
- Bound every allocation by the number of elements actually stored,
  never by `length` or by an index value from the script. Apply the
  "cheaper of two methods" rule for shrinking `length`. The heap limit
  (ADR 0025) must cover vector growth.
- An invariant flag per realm ("no indexed properties on
  `Array.prototype` or `Object.prototype`, and their prototypes are the
  standard ones") is cheap in Rust: one boolean in the realm, cleared in
  the define and set-prototype paths.
- Typed arrays: an ArrayBuffer object owns a byte vector; a view holds a
  handle to the buffer, a byte offset, and a fixed length or a
  "tracks the buffer length" mode. The pull model (check against the
  buffer's current length on each access) needs no back-links and
  cannot read freed memory in safe Rust anyway; the worst case is a
  wrong result, not a crash. Detach replaces the byte vector with an
  empty one and sets a flag. Element conversion must happen before the
  bounds check, as in §10.4.5.
- Element values in typed arrays use the platform byte order in the
  specification (the agent's [[LittleEndian]] value); fixed little-endian
  conversion in Rust is valid and portable.

---

## 3.3 Caches for property access

### Problem

`o.x` and `o.f()` run in loops millions of times. A generic lookup
hashes the key, probes a table, checks attributes, and may walk the
prototype chain. The question is how much of this work an interpreter
can skip, how it knows that skipped work is still valid, and how it
handles lookups that find the property on a prototype.

### What the specification fixes

Nothing. Caches are invisible to scripts. The specification only fixes
the result of [[Get]] and [[Set]] (§10.1.8, §10.1.9 for ordinary
objects), including setters, read-only inherited properties that block
a write, and receivers different from the holder.

### Options in the engines

**No caches; a fast inline lookup (QuickJS, QuickJS-ng).** Neither
QuickJS variant that I read has inline caches. The interpreter handles
the field-read instruction itself: it probes the per-shape hash table of
the object, and if the key is missing and the object is not exotic, it
follows the prototype pointer stored in the shape and probes again. It
leaves this loop for the generic path when it meets an accessor, a
special property kind, or an exotic object. The field-write instruction
has a fast path only for an existing own writable data property; adding
a property or any inherited case goes to the generic path. The cost per
access is one hash probe per object in the chain.

**No caches (Duktape, MuJS, MicroQuickJS).** These engines always run a
lookup: Duktape a linear scan or hash probe, MuJS a string-compared tree
search, MicroQuickJS a hash probe. Duktape's design note discusses fast
paths for array index access but no per-instruction caches.

**Invariant flags as a narrow cache.** The QuickJS array-prototype flag
(3.2) is the only cache-like mechanism in the engines I read. It removes
a prototype walk for one specific question and is invalidated by
clearing one flag.

### Options in the literature

- **Monomorphic inline cache** (Deutsch and Schiffman, Smalltalk-80,
  1984; described in the Self papers and in the Bynens and Meurer
  article). Each access instruction remembers the last shape it saw and
  the slot where the key was. On the next run, if the object has the same
  shape, the instruction reads the slot directly. On a miss, it runs the
  full lookup and refills the cache.
- **Polymorphic inline cache** (Hölzle, Chambers, Ungar, ECOOP 1991).
  The cache holds several (shape, result) pairs, so sites that see a few
  different shapes still hit. Above a small limit the site is
  "megamorphic" and uses the generic path or a shared global cache. The
  paper reports a median speedup of about 11% for Self from PICs alone;
  most of their value in Self came from the type data they feed to the
  optimizing compiler.
- **Inline caches in an interpreter without a JIT** (Brunthaler,
  "Inline caching meets quickening", ECOOP 2010). The interpreter
  rewrites a generic instruction into a specialized one after the first
  execution, and stores cached data next to it. Reported speedups reach
  1.71× on Python. JavaScriptCore's interpreter (per the WebKit article
  "Speculation in JavaScriptCore") keeps per-instruction cache data in a
  side table instead of rewriting the code; its fast path compares the
  structure (shape) and loads the cached slot.
- **Global lookup cache**: one table keyed by (shape, key) for all
  sites. It needs no per-site storage and catches megamorphic sites.
- **Prototype-chain loads** (Bynens and Meurer, "optimizing
  prototypes"). A cached load from a prototype naively needs a check on
  the receiver plus checks on each object up to the holder. If the
  prototype link is part of the shape, one shape check per object
  suffices. V8 goes further: each prototype has a unique shape and a
  "validity cell"; a change to a prototype, or to any object above it,
  invalidates the cells of that prototype and of all prototypes below
  it. A cached prototype load then checks only the receiver's shape and
  one cell. The cost: a change to `Object.prototype` invalidates many
  cells.
- **Invalidation by structure**: a cache keyed by shape needs no
  explicit invalidation for own properties. Any layout change gives the
  object a new shape, so stale entries just miss. Dictionary-mode
  objects either get a unique shape that changes on each structural
  change, or the cache ignores them (the JavaScriptCore article says
  dictionary objects are hard to cache).

### Where the engines agree and differ

The five engines agree: none has per-instruction caches. QuickJS is the
only one with shared shapes, which is the precondition for them. All
inline-cache material comes from the literature, which agrees on the
core mechanism (compare shape, use cached slot) and differs on the
storage (rewritten code, side table, global table) and on prototype
validation (per-object shape checks, or validity cells).

### Trade-offs

- An IC hit replaces a hash probe with one integer comparison and one
  indexed load. QuickJS shows that an inline hash probe without ICs is a
  working baseline; I have no measurements of how much ICs add on top
  in an interpreter, apart from the published numbers above.
- Per-site caches cost memory per access instruction and a miss path.
  A global cache costs one hash per access but no per-site memory.
- Writes that add a property can also be cached: (old shape, new shape,
  slot). This needs a check that no prototype has a setter or a
  read-only property with that key, which again needs prototype
  validation.
- Validity cells make prototype loads cheap but make prototype changes
  expensive; scripts that patch built-in prototypes at startup
  (polyfills) pay once, which is acceptable.

### For swb

- In Rust, rewriting bytecode during execution conflicts with shared
  borrows of the code. A side table per function works well: the
  compiler gives each property access instruction a cache slot number as
  an operand, and the function owns a vector of cache entries. If the
  entries are small `Copy` values, `Cell` gives interior mutability
  without `RefCell` and without `unsafe`.
- A cache entry for an own data property is (shape index, slot index).
  For a prototype hit it also needs the holder handle and a way to prove
  that the chain is unchanged: either the shape of each object on the
  path, or a validity counter per prototype. Putting the prototype in the
  shape (QuickJS) makes the receiver's shape check also prove the first
  prototype link.
- Shape indices in caches are not GC references. If the shape table
  reuses indices of dead shapes, a stale entry can hit on a different
  shape with a wrong slot. That is a correctness bug, not a memory-safety
  bug in safe Rust, but it is still unacceptable. Options: never reuse
  shape indices within a function's lifetime, add a generation to the
  index, or clear all caches at each collection.
- Exotic objects (3.4) and dictionary-mode objects must never match a
  cache entry; giving them shape indices that no cache ever stores is the
  simplest rule.
- A sensible order of work: shapes first (3.1) with an inline lookup
  like QuickJS, then monomorphic caches measured on the target pages,
  then polymorphic entries only if measurements show many sites with two
  to four shapes.

---

## 3.4 Exotic objects

### Problem

Some objects change one or more internal methods: arrays, strings,
`arguments`, bound functions, typed arrays, module namespaces, Proxy
objects. In a browser, the DOM adds more (named and indexed properties
on collections, `WindowProxy`, `document.all`). The generic property
code must handle all of them, but ordinary objects must not pay for
them on every access.

### What the specification fixes

ECMA-262 specifies every exotic behaviour as a set of internal methods:

- The essential internal methods and their invariants: §6.1.7.2 and
  §6.1.7.3.
- Bound function exotic objects: §10.4.1. Only [[Call]] and
  [[Construct]] differ; property access is ordinary.
- Array exotic objects: §10.4.2 (see 3.2).
- String exotic objects: §10.4.3. Index keys below the string length and
  `length` are read-only, enumerable (indices) own properties computed
  from the wrapped string.
- Arguments exotic objects: §10.4.4, built by CreateMappedArgumentsObject
  (§10.4.4.7). Mapped indices alias the function's parameters until a
  delete or a define breaks the mapping for that index.
- TypedArray exotic objects: §10.4.5.
- Module namespace exotic objects: §10.4.6. Keys are the export names,
  sorted; values are live bindings; the object is not extensible.
- Immutable prototype exotic objects: §10.4.7 (`Object.prototype`).
- Proxy objects: §10.5. Every internal method calls a trap if the
  handler has one, then checks the invariants of §6.1.7.3 against the
  target.
- Annex B.3.6 adds the [[IsHTMLDDA]] slot for `document.all`.

The engine does not need to describe these again; it needs a way to
dispatch to them.

### Options in the engines

**One header bit and a per-class hook table (QuickJS, QuickJS-ng).**
Each object has an "exotic" bit and a second bit for "elements live in a
dense vector". The fast paths (field access in the interpreter, own-key
lookup) test the exotic bit once per object; ordinary objects never
enter exotic code. For exotic objects, a per-class table holds optional
hooks: get own property, define own property, delete, list own keys,
and optionally has, get, set, get and set prototype, is extensible,
prevent extensions. The generic algorithms call a hook when it exists and
otherwise run the ordinary algorithm. The host API uses the same table,
so embedders define exotic host classes the same way.

- Proxy implements all hooks. Each trap lookup first checks the host
  stack depth, because a Proxy whose target is a Proxy (and so on)
  recurses in C.
- Arrays, typed arrays and `arguments` use the dense-element bit and
  class-specific branches instead of hooks.
- String objects implement a hook for the virtual index properties.
  Property reads on primitive strings handle indices and `length`
  directly, without creating a wrapper object.
- Mapped `arguments`: the elements are references to the same variable
  cells that hold the parameters (the cells that closures also use), so
  a write through either name changes both. Converting the object to
  ordinary properties keeps those references. Unmapped (strict)
  `arguments` is a plain dense vector.
- Module namespaces implement hooks, and their properties are references
  to the module's binding cells. Exports that can depend on circular
  imports are resolved lazily, through the same placeholder mechanism as
  the built-ins (3.5).
- Bound functions are a separate class that holds the target, the bound
  `this` and the bound arguments; their properties are ordinary. I did
  not check whether QuickJS collapses chains of bound functions.
- There is a dedicated header bit for [[IsHTMLDDA]].

**Type tag and a jump table per key kind (Duktape 3).** Each object has
a type tag. The property code first splits by key kind (string or symbol
key versus array index key), then for each object on the prototype
chain calls a handler chosen by the type tag from a table. Plain objects
use the ordinary handler. Special cases:

- Proxy: Duktape keeps two versions of the [[Get]] prototype walk. The
  normal one assumes that no user code runs during the walk. When it
  meets a Proxy, it switches to a second version that keeps the current
  object reachable on the value stack, because a trap lookup can run a
  getter that changes the prototype chain or makes objects unreachable.
  The source notes that always using the safe version cost about 10% of
  property read speed.
- Bound functions store the final non-bound target. Binding a bound
  function merges the argument lists, so a call never walks a chain of
  bound functions. Bound argument count has a fixed limit to avoid
  overflow.
- Mapped `arguments` keeps an internal map from index to parameter name
  plus a reference to the function's variable environment; accesses look
  up the variable by name. The compiler avoids creating `arguments` at
  all when the function cannot reference it.
- String objects and plain strings have virtual `length` and index
  properties, as in QuickJS.

**Switch on the class in the generic code (MuJS).** The generic get and
set functions test the object class first: arrays (`length`, the flat
vector), strings (index, `length`), regular expressions (flag and
`lastIndex` properties computed from internal fields), and host objects
with user callbacks for has, put and delete. MuJS implements ES5 and
has no Proxy.

**Fewer exotic kinds (MicroQuickJS).** No Proxy, no wrappers for
primitives, `length` of arrays and functions as accessors on the
prototype. The engine trades compliance for size.

### Where the engines agree and differ

- All engines keep a single, cheap test on the hot path (a bit or a type
  tag) and put exotic behaviour behind it.
- QuickJS uses an open table of hooks (also open to embedders); Duktape
  and MuJS use closed switches over built-in types.
- All engines compute String object indices and `length` instead of
  storing them, and all read characters of primitive strings without
  allocating a wrapper.
- `arguments` mapping: shared variable cells (QuickJS) versus lookup by
  name through the environment (Duktape).
- Proxy re-entrancy: QuickJS bounds recursion depth at each trap;
  Duktape also protects the objects it is walking from being collected.

### Trade-offs

- A hook table is extensible and is the natural interface for host
  objects (DOM). A closed switch is easier to read and lets the compiler
  check that all cases exist, but host objects then need one "host"
  case with its own dispatch.
- Shared variable cells make mapped `arguments` fast and simple, but
  every parameter of a sloppy function that uses `arguments` must live
  in a cell (memo 2 covers which variables live in cells).
- Proxy invariant checks are mandatory and expensive (for example
  `ownKeys` must compare the trap result with the target's keys); they
  only cost when a Proxy is used.

### For swb

- An enum of object kinds stored in each heap object, with kind-specific
  data in the variants, is the natural Rust form. The fast path matches
  only on "ordinary" and "array"; everything else goes to a slow path
  with one `match` per internal method. A trait object per class is an
  alternative for host classes, but the borrow checker makes it hard to
  call back into the engine from a method that borrows the heap; plain
  functions that take the engine context and an object handle avoid this.
- Any internal method that can run script code (Proxy traps, getters,
  setters, host hooks) can trigger a collection. With a tracing GC, every
  object handle that the Rust code holds across such a call must be a
  root (on the VM stack or in a root set), as Duktape does in its safe
  walk. A handle that is only in a Rust local variable is not traced; it
  can then point to a freed slot or, if slots are reused, to a different
  object. Memo 4 covers rooting; this section shows where it matters
  most.
- Proxy chains and nested exotic hooks recurse in Rust. They need the
  same depth limit as script calls (memo 1).
- The browser needs more exotic objects than ECMA-262 lists: WebIDL
  legacy platform objects with indexed and named properties, the
  `WindowProxy` and the `Window` object, `[LegacyUnforgeable]`
  properties, `document.all`. The hook interface must cover all
  internal methods, including own-key listing with the right order, so
  that layer 2 can use it without changes to the core.
- Bound functions can store the final target and the merged argument
  list (Duktape); this is not observable by scripts.

---

## 3.5 Built-in objects and realms

### Problem

A realm needs dozens of constructors and prototypes, hundreds of
built-in functions (the QuickJS tables have about 400 native function
entries), and a global object. A browser creates a realm for each
document and each iframe, and the DOM bindings add many interfaces per
realm. Creating all of this eagerly costs time
and memory for every page, even though a page uses a small part of it.

### What the specification fixes

- Realms and their creation: §9.3, InitializeHostDefinedRealm (§9.3.1)
  and CreateIntrinsics (§9.3.2). The well-known intrinsics:
  §6.1.7.4.
- Built-in function objects: §10.3, CreateBuiltinFunction (§10.3.4).
  Each built-in function has `length` and `name` properties.
- Default attributes of built-in properties: clause 18 (ECMAScript
  Standard Built-in Objects). Function properties are writable,
  configurable and not enumerable; `length` and `name` are configurable
  only.
- User functions get a `prototype` object through MakeConstructor
  (§10.2.5). The specification creates it eagerly; creating it later is
  allowed only if no script can see the difference.

### Options in the engines

**Static descriptor tables, eager objects, lazy members (QuickJS,
QuickJS-ng).** Each built-in object is described by a static array of
entries compiled into the binary. An entry has a name, a kind (native
function with its `length`, getter and setter pair, constant number or
string, nested object, alias of another property) and attribute bits.
A small integer stored with each native function lets one native routine
serve several built-ins (for example the typed array variants). All
built-in property names are predefined in a fixed atom table, so setup
does no string interning.

At realm creation, QuickJS creates every constructor and prototype
eagerly and stores the prototypes in an array indexed by class number
(this array is the realm's intrinsics table). It does not create the
function objects for the methods. Instead it stores a placeholder
property that points to the table entry. The first access to such a
property creates the function object and replaces the placeholder.
`Math`, `Reflect` and similar namespace objects are placeholders as
well. Every slow-path operation that can observe a property (get,
get own property descriptor, define, delete, key listing with
enumerability, the global variable access path) must resolve
placeholders; I counted about ten such places. User functions get their
`prototype` property as a placeholder too, so most functions never
allocate a prototype object.

QuickJS-ng adds built-ins that are written in JavaScript, compiled to
bytecode at build time, and instantiated lazily through the same
placeholder mechanism.

One runtime holds the shared data (atom table, shape table, class
table); each realm is a context with its own intrinsics and global
object.

**Generated init data, eager, optional read-only objects (Duktape).**
A metadata file describes all built-ins. A build tool converts it into a
compact bit-packed stream. At heap creation, Duktape decodes the stream
and creates all built-in objects and functions eagerly, and keeps them in
an array indexed by built-in number. Two size options exist:

- Read-only built-ins: the build tool emits all built-in objects as
  constant data. They are non-extensible, and their properties are
  non-configurable. Their data properties stay "writable" in attributes
  so that an object that inherits from them can still create an
  overriding own property; an actual write throws. The global object is
  either a RAM copy of the read-only one or a RAM object that inherits
  from it (less compliant). The design note reports that startup RAM
  drops to about 2–3 kB this way. The GC stops at read-only objects,
  because they cannot reference heap objects.
- Lightweight functions: a native function represented as a tagged value
  with no heap object, with virtual `name` and `length`. Such functions
  cannot have own properties or a `prototype`; the note says this saves
  about 14 kB and is not fully compliant.

**Read-only tables with copy-on-write (MicroQuickJS).** A build tool
compiles declarative tables of built-ins into a read-only image: hashed
property tables, a sorted table of unique strings, and function
descriptors. At context creation, each built-in object is a small RAM
object whose property table pointer points into the read-only image. The
first write to such an object copies its table to RAM. Special entries
for `prototype` and `constructor` resolve to the RAM objects of the
realm during the copy. Most built-in functions are single tagged values
(an index into a native function table), not objects. Constructors and
their prototypes are created on demand, parent class first. The README
says that context creation is very fast and needs almost no RAM.

**Simple eager setup (MuJS).** MuJS creates all built-ins with API calls
at state creation; I did not study it in detail.

### Literature

- The V8 article "Custom startup snapshots" describes a different
  approach: build the initial heap once, serialize it, and deserialize it
  at context creation. It reports context creation going from about
  40 ms to under 2 ms on desktop and from about 270 ms to 10 ms on a
  mobile phone.

### Where the engines agree and differ

- All engines describe built-ins as data (static tables or generated
  streams), not as hand-written setup code per property, and all keep a
  realm's intrinsics in an array indexed by a fixed number.
- All predefine built-in property names as interned atoms or strings.
- They differ on laziness: QuickJS creates objects eagerly but function
  properties lazily; Duktape is fully eager (with an optional read-only
  image); MicroQuickJS shares read-only tables and copies on write.
- Duktape's lightweight functions and MicroQuickJS's function values
  give up compliance to save memory. QuickJS's placeholders keep full
  compliance.

### Trade-offs

- Lazy placeholders save most of the setup cost with full compliance,
  but every observation path must resolve them. A missed path is a
  compliance bug that test262 will find only if it tests that path.
- Copy-on-write shared tables make realm creation nearly free and share
  memory between realms, but every write path must check "is this table
  shared" first, and built-in objects with references to realm objects
  (`constructor`, `prototype`) need indirect entries.
- Eager creation is the simplest and has no hidden states, but costs
  time and memory for each realm.
- Snapshots move the cost to build time, but need a serializer for the
  whole heap and must patch everything that is per-realm or per-process
  (random seeds, host pointers).

### For swb

- Static Rust tables (`const` or `static` arrays of entries) can describe
  each built-in object: name atom, kind, arity, native function pointer,
  attributes. Predefined atoms can be a Rust enum or constants whose
  values are fixed at compile time, so setup does no interning. A build
  script or a macro can generate both from one list.
- The realm can be a struct (or array indexed by an enum) of handles to
  the intrinsics, which matches §6.1.7.4 directly.
- In an index-handle heap, a "template" approach is possible: build one
  realm's built-ins once, then create each new realm by copying that
  block of objects into the heap and adding an offset to all handles
  inside it. This is a snapshot without a serializer. It needs all
  per-realm references inside the block to be handles into the block.
- Placeholder properties (QuickJS) fit Rust well as one more variant of
  the property-value enum; a `match` on that enum in every observation
  path makes it hard to forget the placeholder case, because the
  compiler requires all variants to be handled.
- For a browser, the largest gain is probably at the interface level:
  create a DOM interface object and its prototype only when a script
  first reads the global property (or when the engine creates the first
  instance). This belongs to layer 2, but the core must support lazy
  global properties for it.
- Lazy creation of `prototype` for user functions saves one object per
  closure; most closures are never constructors.
- I did not measure realm creation time in any engine; the memo has no
  numbers for QuickJS or Duktape startup on a desktop machine.

---

## Summary table

| Topic | QuickJS / -ng | Duktape | MuJS | MicroQuickJS |
|---|---|---|---|---|
| Named properties | shared shapes, hash-consed, prototype in shape | per-object ordered table, hash index from 8 keys | per-object AA tree, sorted by name | per-object hash table, ROM tables copy-on-write |
| Dictionary mode | private shape after delete or attribute change | always per object | always per object | always per object |
| Key order | creation order, indices sorted on listing | creation order, indices in separate table, sorted on listing | sorted by string (not ES2015 order) | creation order |
| Arrays | dense, no inner holes, one-way fallback | dense with hole markers, density rule, one-way fallback | flat, no holes, one-way fallback | dense only, holes rejected |
| Property caches | none; inline per-shape hash probe; one invariant flag | none | none | none |
| Exotic dispatch | exotic bit, per-class hook table | type tag, jump table per key kind | class switch | few exotic kinds |
| Built-ins | static tables, eager objects, lazy function members | generated bitstream, eager; optional ROM | eager API calls | ROM image, copy-on-write |

## Open questions for the design session

1. Shapes or per-object tables? Shapes are the precondition for inline
   caches; per-object tables are simpler. The answer depends on how much
   the target pages gain from caches, which nobody has measured for swb.
2. Holes in the dense element vector, or QuickJS-style "no inner holes"?
3. Push or pull for typed array bounds after detach and resize?
4. Lazy built-in members, a template copy per realm, or both?
5. How caches stay valid across GC when shape indices are reused.

---

## Sources

### Engines (read-only copies in the analyst scratchpad; not built or run)

**QuickJS** (Fabrice Bellard), commit 535a7c2, 2026-09-29, VERSION
2026-06-04. Files and parts read:

- `doc/quickjs.texi`: chapter "Internals" (objects, atoms, JS classes).
- `quickjs.h`: the exotic-methods interface for classes.
- `quickjs.c`: object, property and shape definitions; shape creation,
  global shape table, cloning, resizing, compaction; property add and
  delete; own-property lookup; dense array conversion, append, length
  setting, allocation; own-key listing; generic property get; the field
  read and write instruction handlers in the interpreter; array element
  set fast path; property creation for arrays and typed arrays; for-in
  iterator construction; lazy property resolution and the built-in
  function-list installation; context creation and base intrinsics
  setup; ArrayBuffer and typed array structures and detach; Proxy trap
  lookup and the Proxy hook table; mapped arguments creation (part).

**QuickJS-ng**, commit c359cac, 2026-10-09. Files read: `quickjs.c`:
shape definition; field read and write instruction handlers; array
element set fast path and property creation for arrays; set-prototype
path; the per-realm array-prototype invariant flag and where it is
cleared; lazy property kinds, including bytecode-backed built-ins.

**Duktape**, commit 3afa016, 2026-09-05 (master, 3.0 development).
Files read:

- Design notes: `doc/hobject-design.rst` (complete),
  `doc/objects-in-code-section.rst` (complete),
  `doc/lightweight-functions.rst` (first part), `doc/buffers.rst`
  (Duktape buffer support, implementation notes, validity checks,
  future work on detachment), `doc/arguments-object.rst` (overview),
  `doc/hobject-alg-exoticbehaviors.rst` (section list only),
  `doc/low-memory.rst` (searched for startup figures only).
- Sources: `src-input/duk_hobject.h` (header comment, object flags,
  object struct), `src-input/duk_harray.h`, `src-input/duk_hobject_array.c`,
  `src-input/duk_hobject_resize.c` (array density checks),
  `src-input/duk_prop_get.c` (header comment, prototype walk, type
  dispatch), `src-input/duk_prop_ownpropkeys.c` (header comment, index
  key sort), `src-input/duk_prop_enum.c` (searched only),
  `src-input/duk_hthread_builtins.c` (structure and comments),
  `src-input/duk_hboundfunc.h`, `src-input/duk_bi_function.c` (bind).
- Config option descriptions: `config/config-options/` entries for
  array density limit, minimum size for dropping the array vector, fast resize limit, hash
  part, hash property limit, ROM objects, ROM global clone and inherit,
  and lightweight built-in functions.

**MuJS** (Artifex), commit aab59f2, 2026-10-06 (codeberg.org/ccxvii/mujs).
Files read: `jsi.h` (object and property structures, limits),
`jsproperty.c` (complete), `jsrun.c` (property get and set with class
special cases, array flattening and unflattening).

**MicroQuickJS** (Bellard, Gordon), commit 6d4d7eb, 2026-09-26, MIT
license (checked by the orchestrator). Files read: `README.md`
(complete); `mquickjs.c`: property table allocation, rehash, compaction,
copy-on-write of read-only tables, property creation, key hashing,
built-in class initialization, context creation (start), post-GC
rehash; `mqjs_stdlib.c` (start, declaration format);
`mquickjs_build.c` (header only).

### Specification

- ECMA-262, current draft, https://tc39.es/ecma262/ — §6.1.7, §6.1.7.2,
  §6.1.7.3, §6.1.7.4, §9.3, §9.3.1, §9.3.2, §10.1, §10.1.11.1, §10.2.5,
  §10.3, §10.3.4, §10.4.1 to §10.4.7, §10.4.2.4, §10.4.4.7, §10.5,
  §14.7.5.9, clause 18, §23.2, §25.1, Annex B.3.6.

### Literature

- C. Chambers, D. Ungar, E. Lee. "An Efficient Implementation of SELF, a
  Dynamically-Typed Object-Oriented Language Based on Prototypes."
  OOPSLA 1989. https://bibliography.selflanguage.org/implementation.html
- U. Hölzle, C. Chambers, D. Ungar. "Optimizing Dynamically-Typed
  Object-Oriented Languages With Polymorphic Inline Caches." ECOOP 1991.
  https://bibliography.selflanguage.org/pics.html
- L. P. Deutsch, A. M. Schiffman. "Efficient Implementation of the
  Smalltalk-80 System." POPL 1984 (cited for the original inline cache;
  I did not read it).
- S. Brunthaler. "Inline Caching Meets Quickening." ECOOP 2010.
  https://www.unibw.de/ucsrl/pubs/ecoop10.pdf (abstract and summary
  only).
- R. Ierusalimschy, L. H. de Figueiredo, W. Celes. "The Implementation of
  Lua 5.0." Journal of Universal Computer Science 11(7), 2005.
  https://www.lua.org/doc/jucs05.pdf (section on tables).
- C. Bruni. "Fast properties in V8." 2017. https://v8.dev/blog/fast-properties
- M. Bynens, B. Meurer. "JavaScript engine fundamentals: Shapes and
  Inline Caches." 2018. https://mathiasbynens.be/notes/shapes-ics
- M. Bynens, B. Meurer. "JavaScript engine fundamentals: optimizing
  prototypes." 2018. https://mathiasbynens.be/notes/prototypes
- F. Pizlo. "Speculation in JavaScriptCore." WebKit blog, 2020.
  https://webkit.org/blog/10308/speculation-in-javascriptcore/ (first
  half: inline caches and structures).
- Y. Yang. "Custom startup snapshots." V8 blog, 2015.
  https://v8.dev/blog/custom-startup-snapshots
