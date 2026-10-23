//! The slot of a host object (ADR 0026 section 12, ADR 0028 sections 4
//! and 6): the class of the object and its host data, a word or a handle
//! of generic data, in 12 bytes, so that [`ObjectKind`] stays at 16 bytes.
//!
//! A host object of a class without hooks is [`ObjectKind::Host`]: its
//! internal methods are the ordinary ones, and the inline caches treat it
//! as an ordinary object (wrappers of DOM nodes, the global object). A
//! host object of a class with hooks is [`ObjectKind::HostExotic`]: the
//! runtime dispatches its internal methods to the hooks (`host/hooks.rs`),
//! the heap's walks return `Generic` at it, as at a proxy, and the caches
//! never store it.

use crate::error::{Error, Result};
use crate::heap::{Gc, Generic, Heap};
use crate::object::{Object, ObjectKind};

/// The bit of [`HostSlot::tag`] that marks the data as generic data.
const GENERIC_BIT: u32 = 1 << 31;

/// The host data of a host object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostData {
    /// A plain number that the host chooses (a `NodeId`); nothing to
    /// trace.
    Word(u64),
    /// Generic data that the collector traces with the object
    /// ([`crate::GenericData`]) and drops with it.
    Generic(Gc<Generic>),
}

/// The class and the host data of a host object (the payload of
/// [`ObjectKind::Host`] and [`ObjectKind::HostExotic`]; the runtime's
/// `host_object` reads it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostSlot {
    /// The class number, with [`GENERIC_BIT`] for generic data.
    tag: u32,
    /// The word, or the index and the generation of the generic data.
    data: [u32; 2],
}

impl HostSlot {
    /// The slot of an object of class `class` (below 2^31) with `data`.
    pub(crate) fn new(class: u32, data: HostData) -> Result<HostSlot> {
        if class & GENERIC_BIT != 0 {
            return Err(Error::invariant("a host class number of 2^31 or more"));
        }
        Ok(match data {
            HostData::Word(word) => HostSlot {
                tag: class,
                data: [word as u32, (word >> 32) as u32],
            },
            HostData::Generic(generic) => HostSlot {
                tag: class | GENERIC_BIT,
                data: [generic.index(), generic.generation()],
            },
        })
    }

    /// The class number.
    pub(crate) fn class(self) -> u32 {
        self.tag & !GENERIC_BIT
    }

    /// The host data.
    pub(crate) fn data(self) -> HostData {
        let [low, high] = self.data;
        if self.tag & GENERIC_BIT == 0 {
            HostData::Word(u64::from(low) | (u64::from(high) << 32))
        } else {
            HostData::Generic(Gc::new(low, high))
        }
    }

    /// The generic data, if the data is generic (for the collector).
    pub(crate) fn generic(self) -> Option<Gc<Generic>> {
        match self.data() {
            HostData::Generic(generic) => Some(generic),
            HostData::Word(_) => None,
        }
    }
}

impl Heap {
    /// A new host object with the class number `class` and generic data,
    /// whose internal methods are the ordinary ones. (For tests of the
    /// heap; the runtime makes host objects of registered classes with
    /// `Runtime::new_host_object`.)
    pub fn new_host_object(
        &mut self,
        proto: Option<Gc<Object>>,
        class: u32,
        data: Gc<Generic>,
    ) -> Result<Gc<Object>> {
        let slot = HostSlot::new(class, HostData::Generic(data))?;
        self.new_host_object_with_slot(proto, slot, false)
    }

    /// A new host object with `slot`; `exotic` for a class with hooks.
    pub(crate) fn new_host_object_with_slot(
        &mut self,
        proto: Option<Gc<Object>>,
        slot: HostSlot,
        exotic: bool,
    ) -> Result<Gc<Object>> {
        if let Some(generic) = slot.generic() {
            self.generic_check(generic)?;
        }
        let kind = if exotic {
            ObjectKind::HostExotic(slot)
        } else {
            ObjectKind::Host(slot)
        };
        self.new_object_with_kind(proto, kind)
    }

    /// The host slot of an object, if it is a host object.
    #[inline]
    pub(crate) fn host_slot(&self, object: Gc<Object>) -> Result<Option<HostSlot>> {
        Ok(match self.object(object)?.kind {
            ObjectKind::Host(slot) | ObjectKind::HostExotic(slot) => Some(slot),
            _ => None,
        })
    }

    /// Whether the internal methods of `object` need the runtime because
    /// they can run script code: a proxy, or a host object whose class has
    /// hooks. Such an object takes the generic paths of the runtime.
    #[inline]
    pub(crate) fn needs_runtime(&self, object: Gc<Object>) -> Result<bool> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::Proxy(_) | ObjectKind::HostExotic(_)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_keeps_its_class_and_data() {
        let word = HostSlot::new(7, HostData::Word(u64::MAX - 5)).unwrap();
        assert_eq!(word.class(), 7);
        assert_eq!(word.data(), HostData::Word(u64::MAX - 5));
        assert_eq!(word.generic(), None);
        let handle = Gc::new(12, 34);
        let generic = HostSlot::new(9, HostData::Generic(handle)).unwrap();
        assert_eq!(generic.class(), 9);
        assert_eq!(generic.data(), HostData::Generic(handle));
        assert_eq!(generic.generic(), Some(handle));
        assert!(HostSlot::new(GENERIC_BIT, HostData::Word(0)).is_err());
        assert_eq!(size_of::<HostSlot>(), 12);
        assert_eq!(size_of::<ObjectKind>(), 16);
    }
}
