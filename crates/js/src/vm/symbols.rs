//! Symbols that belong to the runtime, not to a realm: the well-known
//! symbols (ECMA-262 §6.1.5.1, "shared by all realms") and the
//! `GlobalSymbolRegistry` (§20.4.2.2, "Symbol.for").
//!
//! The runtime keeps both as roots. The registry is a map from the atom of
//! the key to the symbol, so equal strings meet one entry; registered
//! symbols stay alive for the life of the runtime (the specification
//! never removes an entry). Each registration costs a symbol slot, an atom
//! and an entry, all counted against the heap limit.

use crate::error::Error;
use crate::heap::{Gc, HandleMap, Heap, Tracer};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Symbol;
use crate::vm::VmResult;

/// The well-known symbols of ECMA-262 2025 (table 1 of §6.1.5.1), in the
/// order of the properties of `Symbol`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WellKnown {
    AsyncIterator,
    HasInstance,
    IsConcatSpreadable,
    Iterator,
    Match,
    MatchAll,
    Replace,
    Search,
    Species,
    Split,
    ToPrimitive,
    ToStringTag,
    Unscopables,
}

/// The well-known symbols with their property names on `Symbol`.
pub(crate) const WELL_KNOWN: [(WellKnown, &str); 13] = [
    (WellKnown::AsyncIterator, "asyncIterator"),
    (WellKnown::HasInstance, "hasInstance"),
    (WellKnown::IsConcatSpreadable, "isConcatSpreadable"),
    (WellKnown::Iterator, "iterator"),
    (WellKnown::Match, "match"),
    (WellKnown::MatchAll, "matchAll"),
    (WellKnown::Replace, "replace"),
    (WellKnown::Search, "search"),
    (WellKnown::Species, "species"),
    (WellKnown::Split, "split"),
    (WellKnown::ToPrimitive, "toPrimitive"),
    (WellKnown::ToStringTag, "toStringTag"),
    (WellKnown::Unscopables, "unscopables"),
];

/// The bytes charged for a registry entry (key, symbol and map slot).
const REGISTRY_ENTRY_SIZE: usize = 48;

/// The symbols of a runtime.
pub(crate) struct Symbols {
    well_known: [Gc<Symbol>; 13],
    /// The `GlobalSymbolRegistry`: atom of the key to symbol. A handle-keyed
    /// map: the keys are atoms, whose handles a script cannot choose.
    registry: HandleMap<Gc<JsString>, Gc<Symbol>>,
}

impl Symbols {
    /// Creates the well-known symbols (the runtime is not a root yet, so
    /// nothing collects during the creation).
    pub(crate) fn new(heap: &mut Heap) -> Result<Symbols, Error> {
        let mut made = Vec::with_capacity(WELL_KNOWN.len());
        for (_, name) in WELL_KNOWN {
            let description = heap.intern_str(&format!("Symbol.{name}"))?;
            made.push(heap.alloc_symbol(Some(description))?);
        }
        let well_known = made
            .try_into()
            .map_err(|_| Error::Internal(crate::error::InternalError::Invariant("symbol count")))?;
        Ok(Symbols {
            well_known,
            registry: HandleMap::default(),
        })
    }

    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for symbol in &self.well_known {
            tracer.symbol(*symbol);
        }
        for (key, symbol) in &self.registry {
            tracer.string(*key);
            tracer.symbol(*symbol);
        }
    }
}

impl Runtime {
    /// A well-known symbol.
    pub(crate) fn well_known(&self, which: WellKnown) -> Gc<Symbol> {
        self.vm.symbols.well_known[which as usize]
    }

    /// The property key of a well-known symbol.
    pub(crate) fn symbol_key(&self, which: WellKnown) -> PropertyKey {
        PropertyKey::Symbol(self.well_known(which))
    }

    /// `Symbol.for` (§20.4.2.2): the registered symbol for `key`, created
    /// if needed. The caller roots `key`.
    pub(crate) fn symbol_for(&mut self, key: Gc<JsString>) -> VmResult<Gc<Symbol>> {
        let atom = self.heap.intern_string(key)?;
        if let Some(symbol) = self.vm.symbols.registry.get(&atom) {
            return Ok(*symbol);
        }
        let symbol = self.heap.alloc_symbol(Some(atom))?;
        self.heap.charge(REGISTRY_ENTRY_SIZE);
        self.vm.symbols.registry.insert(atom, symbol);
        Ok(symbol)
    }

    /// `Symbol.keyFor` (§20.4.2.6): the key of a registered symbol.
    pub(crate) fn symbol_key_for(&self, symbol: Gc<Symbol>) -> Option<Gc<JsString>> {
        let description = self.heap.symbol(symbol).ok()?.description?;
        (self.vm.symbols.registry.get(&description) == Some(&symbol)).then_some(description)
    }
}
