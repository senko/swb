//! The compile-time name table: identifier names as `u32` ids.
//!
//! The lexer interns every identifier and private name; the parser and the
//! scope analysis compare names by id. The table lives as long as the
//! compile of one script; the compiler maps names to runtime property keys
//! when it creates the code objects (ADR 0026 section 3; memo 2.1 in
//! `docs/js-study/02-front-end.md`).
//!
//! The well-known names in [`names`] (the reserved words and the
//! contextual keywords) have fixed ids in every table.

use std::collections::HashMap;

use swb_js_text::{Str16, String16};

/// The id of a name in an [`Interner`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameId(u32);

impl NameId {
    /// The index of the name in its table.
    pub const fn index(self) -> u32 {
        self.0
    }
}

macro_rules! well_known_names {
    ($($name:ident = $text:literal,)*) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u32)]
        enum Index {
            $($name,)*
        }

        $(
            #[doc = concat!("`", $text, "`")]
            pub const $name: NameId = NameId(Index::$name as u32);
        )*

        /// The texts of the well-known names, in the order of their ids.
        pub(crate) const TEXTS: &[&str] = &[$($text,)*];
    };
}

/// The names that have the same id in every [`Interner`]. The reserved
/// words come first, in the order of [`crate::TokenKind`]'s keywords.
pub mod names {
    use super::NameId;

    well_known_names! {
        // ReservedWord (§12.7.2).
        AWAIT = "await",
        BREAK = "break",
        CASE = "case",
        CATCH = "catch",
        CLASS = "class",
        CONST = "const",
        CONTINUE = "continue",
        DEBUGGER = "debugger",
        DEFAULT = "default",
        DELETE = "delete",
        DO = "do",
        ELSE = "else",
        ENUM = "enum",
        EXPORT = "export",
        EXTENDS = "extends",
        FALSE = "false",
        FINALLY = "finally",
        FOR = "for",
        FUNCTION = "function",
        IF = "if",
        IMPORT = "import",
        IN = "in",
        INSTANCEOF = "instanceof",
        NEW = "new",
        NULL = "null",
        RETURN = "return",
        SUPER = "super",
        SWITCH = "switch",
        THIS = "this",
        THROW = "throw",
        TRUE = "true",
        TRY = "try",
        TYPEOF = "typeof",
        VAR = "var",
        VOID = "void",
        WHILE = "while",
        WITH = "with",
        YIELD = "yield",
        // Reserved in strict mode code (§13.1.1).
        LET = "let",
        STATIC = "static",
        IMPLEMENTS = "implements",
        INTERFACE = "interface",
        PACKAGE = "package",
        PRIVATE = "private",
        PROTECTED = "protected",
        PUBLIC = "public",
        // Contextual keywords and names with special rules.
        ARGUMENTS = "arguments",
        AS = "as",
        ASYNC = "async",
        CONSTRUCTOR = "constructor",
        EVAL = "eval",
        FROM = "from",
        GET = "get",
        META = "meta",
        OF = "of",
        PROTOTYPE = "prototype",
        SET = "set",
        TARGET = "target",
        PROTO = "__proto__",
        // The names of the implicit `new.target` binding and of the
        // active function binding of a derived constructor (not
        // identifiers, so no script can declare them). The home object
        // binding has the name `super`.
        NEW_TARGET = "new.target",
        ACTIVE_FUNCTION = "%function",
        // The implicit binding of a dynamic environment (`with` body,
        // eval vars), and the local name of an anonymous default export
        // (§16.2.3: "*default*"). Neither is an identifier.
        DYNAMIC_ENV = "%env",
        DEFAULT_EXPORT = "*default*",
        // The implicit binding of a class body that holds the class's
        // private environment (its Private Names); not an identifier.
        PRIVATE_ENV = "%private",
    }

    /// The number of reserved words; their ids are `0..RESERVED_WORDS`.
    pub(crate) const RESERVED_WORDS: u32 = YIELD.0 + 1;
}

/// A table of names. Ids are dense, starting at 0.
///
/// The well-known names live in one table per process ([`WellKnown`]),
/// so a new table (one per script, and one per call of `eval`) costs no
/// hashing; the table of a script holds the other names, with ids after
/// the well-known ones.
#[derive(Debug)]
pub struct Interner {
    /// The names after the well-known ones.
    names: Vec<String16>,
    /// Name units (always the wide form, so that both widths of a source
    /// find the same entry) to id. The standard hasher has a random seed,
    /// so a script cannot choose colliding names.
    ids: HashMap<Box<[u16]>, NameId>,
    /// A buffer for the wide form of narrow names.
    scratch: Vec<u16>,
    /// A direct-mapped cache in front of `ids`: the id of a recent name
    /// per [`cache_slot`], or `NO_ID`. A hit is checked against the name,
    /// so a collision only costs a lookup in `ids`. Most identifiers of a
    /// script are repeated short names; the cache saves their hashing. It
    /// starts small and grows with the number of names.
    cache: Vec<u32>,
}

/// The well-known names with their ids (built once per process).
#[derive(Debug)]
struct WellKnown {
    names: Vec<String16>,
    ids: HashMap<Box<[u16]>, NameId>,
    /// Per length below [`FILTER_LENGTHS`]: the ASCII code units that
    /// start a well-known name of that length (a name outside the filter
    /// needs no lookup in `ids`; one-letter names, common in minified
    /// code, never do).
    first_units: [u128; FILTER_LENGTHS],
}

/// The lengths that [`WellKnown::first_units`] covers (all well-known
/// names are shorter).
const FILTER_LENGTHS: usize = 16;

impl WellKnown {
    /// Whether `key` can be a well-known name.
    fn may_have(&self, key: &[u16]) -> bool {
        match (self.first_units.get(key.len()), key.first()) {
            (Some(&mask), Some(&first)) if first < 128 => mask & (1u128 << first) != 0,
            _ => false,
        }
    }
}

/// The table of the well-known names.
fn well_known() -> &'static WellKnown {
    static TABLE: std::sync::OnceLock<WellKnown> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let names: Vec<String16> = names::TEXTS.iter().map(|&t| String16::from(t)).collect();
        let mut first_units = [0u128; FILTER_LENGTHS];
        let mut ids = HashMap::new();
        for (i, name) in names.iter().enumerate() {
            let wide: Box<[u16]> = name.as_str16().units().collect();
            if let (Some(mask), Some(&first)) = (first_units.get_mut(wide.len()), wide.first())
                && first < 128
            {
                *mask |= 1u128 << first;
            }
            ids.insert(wide, NameId(i as u32));
        }
        WellKnown {
            names,
            ids,
            first_units,
        }
    })
}

/// An empty slot of the cache.
const NO_ID: u32 = u32::MAX;

/// The most cache slots (a power of two).
const CACHE_SLOTS: usize = 4096;

/// The first number of cache slots (a power of two).
const FIRST_CACHE_SLOTS: usize = 64;

/// The cache slot of a name: a multiplicative hash of its code units with
/// the golden ratio as the factor, whose top bits select the slot
/// (Fibonacci hashing; Knuth, TAOCP vol. 3, §6.4). `slots` is a power of
/// two.
fn cache_slot(name: Str16<'_>, slots: usize) -> usize {
    fn hash(units: impl Iterator<Item = u64>) -> u64 {
        let mut hash: u64 = 0;
        for unit in units {
            hash = (hash ^ unit).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        hash
    }
    let hash = match name {
        Str16::Latin1(units) => hash(units.iter().map(|&u| u64::from(u))),
        Str16::Wide(units) => hash(units.iter().map(|&u| u64::from(u))),
    };
    (hash >> 52) as usize & (slots.max(1) - 1)
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

impl Interner {
    /// A table with the well-known names of [`names`].
    pub fn new() -> Self {
        Self::for_source(0)
    }

    /// A table for a source of `len` code units: the cache starts at the
    /// size that the source probably needs (a short eval string needs few
    /// slots).
    pub(crate) fn for_source(len: usize) -> Self {
        let slots = (len / 4)
            .next_power_of_two()
            .clamp(FIRST_CACHE_SLOTS, CACHE_SLOTS);
        Interner {
            names: Vec::new(),
            ids: HashMap::new(),
            scratch: Vec::new(),
            cache: vec![NO_ID; slots],
        }
    }

    /// The id of a name; adds the name if it is new.
    pub fn intern(&mut self, name: Str16<'_>) -> NameId {
        let slot = cache_slot(name, self.cache.len());
        if let Some(&id) = self.cache.get(slot)
            && id != NO_ID
            && self.get(NameId(id)).is_some_and(|known| known == name)
        {
            return NameId(id);
        }
        let id = self.intern_hashed(name);
        // A larger cache once the table has many names (the cache only
        // saves work, so it starts over empty).
        let wanted = (self.names.len() * 2).next_power_of_two().min(CACHE_SLOTS);
        if wanted > self.cache.len() {
            self.cache = vec![NO_ID; wanted];
        }
        let slot = cache_slot(name, self.cache.len());
        if let Some(entry) = self.cache.get_mut(slot) {
            *entry = id.0;
        }
        id
    }

    /// The id of a name from the hash tables; adds the name if it is new.
    fn intern_hashed(&mut self, name: Str16<'_>) -> NameId {
        let key: &[u16] = match name {
            Str16::Wide(units) => units,
            Str16::Latin1(units) => {
                self.scratch.clear();
                self.scratch.extend(units.iter().map(|&u| u16::from(u)));
                &self.scratch
            }
        };
        let base = well_known();
        if base.may_have(key)
            && let Some(&id) = base.ids.get(key)
        {
            return id;
        }
        if let Some(&id) = self.ids.get(key) {
            return id;
        }
        let id = NameId((base.names.len() + self.names.len()) as u32);
        self.ids.insert(key.into(), id);
        self.names.push(name.to_string16());
        id
    }

    /// The id of a name given as UTF-8.
    pub fn intern_str(&mut self, name: &str) -> NameId {
        let name = String16::from(name);
        self.intern(name.as_str16())
    }

    /// The text of a name, or `None` for an id of another table.
    pub fn get(&self, id: NameId) -> Option<Str16<'_>> {
        let base = well_known();
        let index = id.0 as usize;
        match index.checked_sub(base.names.len()) {
            None => base.names.get(index).map(String16::as_str16),
            Some(own) => self.names.get(own).map(String16::as_str16),
        }
    }

    /// The number of names.
    pub fn len(&self) -> usize {
        well_known().names.len() + self.names.len()
    }

    /// Whether the table has no names (never true: it has the well-known
    /// names).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_names_have_fixed_ids() {
        let mut interner = Interner::new();
        assert_eq!(interner.intern_str("await"), names::AWAIT);
        assert_eq!(interner.intern_str("yield"), names::YIELD);
        assert_eq!(interner.intern_str("let"), names::LET);
        assert_eq!(names::RESERVED_WORDS, 38);
        assert_eq!(interner.len(), names::TEXTS.len());
        for (index, text) in names::TEXTS.iter().enumerate() {
            let id = NameId(index as u32);
            assert!(interner.get(id).is_some_and(|name| name.eq_str(text)));
        }
    }

    #[test]
    fn both_widths_give_the_same_id() {
        let mut interner = Interner::new();
        let a = interner.intern(Str16::Latin1(b"caf\xE9"));
        let b = interner.intern(Str16::Wide(&[0x63, 0x61, 0x66, 0xE9]));
        assert_eq!(a, b);
        let c = interner.intern(Str16::Wide(&[0x3B1]));
        assert_ne!(a, c);
        assert_eq!(interner.intern_str("\u{3b1}"), c);
        assert!(interner.get(c).is_some_and(|name| name.eq_str("\u{3b1}")));
        assert!(interner.get(NameId(u32::MAX)).is_none());
    }

    #[test]
    fn cache_collisions_keep_names_apart() {
        // More names than cache slots: some share a slot.
        let mut interner = Interner::new();
        let texts: Vec<String> = (0..3 * CACHE_SLOTS).map(|i| format!("n{i}")).collect();
        let ids: Vec<NameId> = texts.iter().map(|t| interner.intern_str(t)).collect();
        for (text, &id) in texts.iter().zip(&ids).rev() {
            assert_eq!(interner.intern_str(text), id);
            assert!(interner.get(id).is_some_and(|name| name.eq_str(text)));
        }
        assert_eq!(interner.len(), names::TEXTS.len() + texts.len());
    }
}
