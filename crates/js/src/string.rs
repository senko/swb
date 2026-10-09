//! String values, interned strings (atoms) and property keys (ADR 0026
//! section 7, memo 4.2).
//!
//! A string is flat: its code units in one of the two widths of
//! `js-text`. Ropes come later. Property names are atoms: the atom table
//! maps the content to the one string handle with that content. The
//! table holds its entries weakly; the collector removes the dead ones.
//! Its hash has a random key per heap, against collision floods.

use std::collections::HashMap;
use std::hash::{BuildHasher, BuildHasherDefault, Hasher, RandomState};

use swb_js_text::{Str16, String16};

use crate::heap::{Arena, Gc, MarkBits};
use crate::value::Symbol;

/// A string value: immutable code units with UTF-16 semantics.
pub struct JsString {
    text: String16,
    /// Whether the string is in the atom table.
    atom: bool,
}

impl JsString {
    pub(crate) fn new(text: String16) -> Self {
        JsString { text, atom: false }
    }

    /// The code units.
    pub fn as_str16(&self) -> Str16<'_> {
        self.text.as_str16()
    }

    /// The number of code units.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the string has no code units.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Whether the string is interned (an atom).
    pub fn is_atom(&self) -> bool {
        self.atom
    }

    pub(crate) fn set_atom(&mut self) {
        self.atom = true;
    }

    /// The bytes that the code units occupy outside the slot.
    pub(crate) fn heap_size(&self) -> usize {
        let units = self.text.len();
        if self.text.is_latin1() {
            units
        } else {
            units * 2
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
pub const MAX_ARRAY_LENGTH: u32 = u32::MAX;

/// The array index that `text` is the canonical form of (§6.1.7), if any.
pub fn array_index(text: Str16<'_>) -> Option<u32> {
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
                .is_ok_and(|string| string.as_str16() == text)
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
