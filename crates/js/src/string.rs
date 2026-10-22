//! String values, interned strings (atoms) and property keys (ADR 0026
//! section 7, memo 4.2).
//!
//! A string is flat (its code units in one of the two widths of
//! `js-text`) or a rope: two child strings and the length, which a
//! concatenation builds ([`rope`]). An operation that needs the code
//! units flattens the rope in place first ([`Heap::flatten`],
//! `Runtime::flatten`) and then reads the flat view ([`Heap::text`]).
//! `length` and the width of a rope need no flatten. Property names are
//! atoms: the atom table maps the content to the one string handle with
//! that content. Atoms are always flat. The table holds its entries
//! weakly; the collector removes the dead ones. Its hash has a random key
//! per heap, against collision floods.
//!
//! [`Heap::flatten`]: crate::heap::Heap::flatten
//! [`Heap::text`]: crate::heap::Heap::text

pub(crate) mod rope;

use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{BuildHasher, BuildHasherDefault, Hasher, RandomState};

use swb_js_text::{Str16, String16};

use crate::heap::{Arena, Gc, MarkBits};
use crate::value::Symbol;
use rope::Rope;

/// A string value: immutable code units with UTF-16 semantics.
pub struct JsString {
    repr: Repr,
    /// Whether the string is in the atom table.
    atom: bool,
    /// The keyed hash of the content that `Map` and `Set` use (0: not
    /// computed yet). It sits in the padding after `atom`, so it costs no
    /// memory.
    key_hash: Cell<u32>,
}

/// The two forms of a string.
enum Repr {
    /// The code units.
    Flat(String16),
    /// A concatenation that is not flattened yet.
    Rope(Rope),
}

impl JsString {
    pub(crate) fn new(text: String16) -> Self {
        JsString {
            repr: Repr::Flat(text),
            atom: false,
            key_hash: Cell::new(0),
        }
    }

    pub(crate) fn rope(rope: Rope) -> Self {
        JsString {
            repr: Repr::Rope(rope),
            atom: false,
            key_hash: Cell::new(0),
        }
    }

    /// The code units of a flat string; `None` for a rope (flatten it
    /// first: `Heap::flatten`).
    pub fn as_flat(&self) -> Option<Str16<'_>> {
        match &self.repr {
            Repr::Flat(text) => Some(text.as_str16()),
            Repr::Rope(_) => None,
        }
    }

    /// The cached 32-bit hash of the content, if one was stored. The
    /// content of a string never changes (a flatten keeps it), so the
    /// hash stays valid.
    #[inline]
    pub(crate) fn cached_key_hash(&self) -> Option<u32> {
        Some(self.key_hash.get()).filter(|&hash| hash != 0)
    }

    /// Stores the hash of the content (never 0).
    #[inline]
    pub(crate) fn remember_key_hash(&self, hash: u32) {
        self.key_hash.set(hash.max(1));
    }

    /// Whether the string is a rope (not flattened yet).
    pub fn is_rope(&self) -> bool {
        matches!(self.repr, Repr::Rope(_))
    }

    /// The parts of a rope.
    pub(crate) fn as_rope(&self) -> Option<&Rope> {
        match &self.repr {
            Repr::Flat(_) => None,
            Repr::Rope(rope) => Some(rope),
        }
    }

    /// The number of code units.
    pub fn len(&self) -> usize {
        match &self.repr {
            Repr::Flat(text) => text.len(),
            Repr::Rope(rope) => rope.len(),
        }
    }

    /// Whether the string has no code units.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether all code units are below 256 (the narrow width).
    pub fn is_latin1(&self) -> bool {
        match &self.repr {
            Repr::Flat(text) => text.is_latin1(),
            Repr::Rope(rope) => rope.is_latin1(),
        }
    }

    /// The depth of the rope: 0 for a flat string.
    pub(crate) fn depth(&self) -> u16 {
        match &self.repr {
            Repr::Flat(_) => 0,
            Repr::Rope(rope) => rope.depth(),
        }
    }

    /// Whether the string is interned (an atom).
    pub fn is_atom(&self) -> bool {
        self.atom
    }

    pub(crate) fn set_atom(&mut self) {
        self.atom = true;
    }

    /// Replaces a rope with its flat text (the same code units).
    fn set_flat(&mut self, text: String16) {
        self.repr = Repr::Flat(text);
    }

    /// Replaces a rope with another node of the same code units (a
    /// rebalance).
    fn set_rope(&mut self, rope: Rope) {
        self.repr = Repr::Rope(rope);
    }

    /// The bytes that the code units occupy outside the slot (0 for a
    /// rope: its node is the slot).
    pub(crate) fn heap_size(&self) -> usize {
        match &self.repr {
            Repr::Flat(text) if text.is_latin1() => text.len(),
            Repr::Flat(text) => text.len() * 2,
            Repr::Rope(_) => 0,
        }
    }
}

/// A property key (ECMA-262 §6.1.7): an array index, an interned string
/// or a symbol.
///
/// An array index is an integer from 0 to 2^32 − 2 in canonical form, so
/// the string `"5"` gives `Index(5)`, but `"05"`, `"-0"` and
/// `"4294967295"` are string keys. Two keys are the same property if and
/// only if they are equal, because string keys are atoms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyKey {
    /// An array index, below 2^32 − 1.
    Index(u32),
    /// An interned string that is not an array index.
    String(Gc<JsString>),
    /// A symbol.
    Symbol(Gc<Symbol>),
}

impl PropertyKey {
    /// Whether the key is a symbol.
    pub fn is_symbol(self) -> bool {
        matches!(self, PropertyKey::Symbol(_))
    }
}

/// The largest array index plus one: `length` of an array is at most this.
pub(crate) const MAX_ARRAY_LENGTH: u32 = u32::MAX;

/// The array index that `text` is the canonical form of (§6.1.7), if any.
pub(crate) fn array_index(text: Str16<'_>) -> Option<u32> {
    let len = text.len();
    if len == 0 || len > 10 {
        return None;
    }
    let mut value: u64 = 0;
    for (position, unit) in text.units().enumerate() {
        let digit = unit.checked_sub(u16::from(b'0')).filter(|&d| d < 10)?;
        if digit == 0 && position == 0 && len > 1 {
            return None;
        }
        value = value * 10 + u64::from(digit);
    }
    u32::try_from(value)
        .ok()
        .filter(|&index| index < MAX_ARRAY_LENGTH)
}

/// The decimal text of an integer.
pub(crate) fn decimal(value: u32) -> String16 {
    String16::from(value.to_string().as_str())
}

/// A hasher that passes a precomputed 64-bit hash through.
#[derive(Default)]
pub(crate) struct PrehashedHasher(u64);

impl Hasher for PrehashedHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(byte);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

/// A map keyed by a precomputed hash.
type HashKeyed<V> = HashMap<u64, V, BuildHasherDefault<PrehashedHasher>>;

/// The weak table of interned strings.
pub(crate) struct AtomTable {
    /// Keyed hash of the content → the atom.
    primary: HashKeyed<Gc<JsString>>,
    /// Further atoms whose content has the same 64-bit hash as the one in
    /// `primary` (practically never; needed for correctness).
    overflow: HashKeyed<Vec<Gc<JsString>>>,
    /// The random key of the content hash.
    seed: RandomState,
    len: usize,
}

impl AtomTable {
    pub(crate) fn new() -> Self {
        AtomTable {
            primary: HashMap::default(),
            overflow: HashMap::default(),
            seed: RandomState::new(),
            len: 0,
        }
    }

    /// The number of atoms.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The keyed hash of the content. Both widths of equal content give
    /// the same hash: text whose units all fit in a byte is hashed as
    /// bytes, other text as little-endian unit pairs. The length in units
    /// follows, so that wide text does not collide with the narrow text of
    /// its bytes.
    pub(crate) fn hash(&self, text: Str16<'_>) -> u64 {
        let mut hasher = self.seed.build_hasher();
        match text {
            Str16::Latin1(bytes) => hasher.write(bytes),
            Str16::Wide(units) => {
                let narrow = units.iter().all(|&unit| unit < 256);
                let mut buffer = [0u8; 256];
                let mut used = 0;
                for &unit in units {
                    if used + 2 > buffer.len() {
                        hasher.write(&buffer[..used]);
                        used = 0;
                    }
                    if narrow {
                        buffer[used] = unit as u8;
                        used += 1;
                    } else {
                        buffer[used..used + 2].copy_from_slice(&unit.to_le_bytes());
                        used += 2;
                    }
                }
                hasher.write(&buffer[..used]);
            }
        }
        hasher.write_usize(text.len());
        hasher.finish()
    }

    /// The atom with the content `text`, if there is one.
    pub(crate) fn find(
        &self,
        hash: u64,
        text: Str16<'_>,
        strings: &Arena<JsString>,
    ) -> Option<Gc<JsString>> {
        let matches = |atom: &Gc<JsString>| {
            strings
                .get(*atom)
                .is_ok_and(|string| string.as_flat() == Some(text))
        };
        if let Some(atom) = self.primary.get(&hash).copied().filter(matches) {
            return Some(atom);
        }
        self.overflow
            .get(&hash)
            .and_then(|atoms| atoms.iter().copied().find(matches))
    }

    /// Adds an atom whose content is not in the table.
    pub(crate) fn insert(&mut self, hash: u64, atom: Gc<JsString>) {
        self.len += 1;
        if let std::collections::hash_map::Entry::Vacant(entry) = self.primary.entry(hash) {
            entry.insert(atom);
        } else {
            self.overflow.entry(hash).or_default().push(atom);
        }
    }

    /// Drops the atoms whose strings are not marked (the weak sweep), and
    /// shrinks the table when most of it is empty.
    pub(crate) fn sweep(&mut self, marks: &MarkBits) {
        let live = |atom: &Gc<JsString>| marks.get(atom.index() as usize);
        self.primary.retain(|_, atom| live(atom));
        self.overflow.retain(|_, atoms| {
            atoms.retain(live);
            !atoms.is_empty()
        });
        self.len = self.primary.len() + self.overflow.values().map(Vec::len).sum::<usize>();
        shrink_if_sparse(&mut self.primary);
        shrink_if_sparse(&mut self.overflow);
    }

    /// The approximate bytes of the table.
    pub(crate) fn bytes(&self) -> usize {
        self.primary.capacity() * (size_of::<(u64, Gc<JsString>)>() + 1)
            + self.overflow.capacity() * (size_of::<(u64, Vec<Gc<JsString>>)>() + 1)
    }
}

/// Shrinks a hash map whose capacity is more than four times its length.
pub(crate) fn shrink_if_sparse<K: Eq + std::hash::Hash, V, S: BuildHasher>(
    map: &mut HashMap<K, V, S>,
) {
    if map.capacity() > 64 && map.capacity() / 4 > map.len() {
        map.shrink_to(map.len() * 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(text: &str) -> Option<u32> {
        array_index(String16::from(text).as_str16())
    }

    #[test]
    fn canonical_array_indices() {
        assert_eq!(index("0"), Some(0));
        assert_eq!(index("5"), Some(5));
        assert_eq!(index("4294967294"), Some(4_294_967_294));
        assert_eq!(index("4294967295"), None);
        assert_eq!(index("05"), None);
        assert_eq!(index("-0"), None);
        assert_eq!(index("-1"), None);
        assert_eq!(index("1.5"), None);
        assert_eq!(index(""), None);
        assert_eq!(index("+1"), None);
        assert_eq!(index(" 1"), None);
        assert_eq!(index("99999999999"), None);
        assert_eq!(array_index(Str16::Wide(&[0x31, 0x32])), Some(12));
    }

    #[test]
    fn a_string_costs_no_memory_for_its_cached_hash() {
        // The hash sits in the padding of the record (40 bytes, 48 with
        // the slot header): a field of 8 bytes would grow both.
        assert_eq!(size_of::<JsString>(), 40);
        assert_eq!(Arena::<JsString>::slot_size(), 48);
        let text = JsString::new(String16::from("key"));
        assert_eq!(text.cached_key_hash(), None);
        text.remember_key_hash(0);
        assert_eq!(text.cached_key_hash(), Some(1));
        text.remember_key_hash(77);
        assert_eq!(text.cached_key_hash(), Some(77));
    }

    #[test]
    fn hash_ignores_the_width() {
        let table = AtomTable::new();
        assert_eq!(
            table.hash(Str16::Latin1(b"ab")),
            table.hash(Str16::Wide(&[0x61, 0x62]))
        );
        assert_ne!(
            table.hash(Str16::Wide(&[0x100])),
            table.hash(Str16::Latin1(&[0x00, 0x01]))
        );
    }
}
