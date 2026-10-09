//! Elements: the properties whose keys are array indices (ADR 0026
//! section 6, memo 3.2).
//!
//! - Dense: a vector of values; a hole holds `Empty`. Every element in
//!   it is a writable, enumerable, configurable data property.
//! - Sparse: an ordered map from index to property. An object moves to
//!   it when the vector would be too sparse (the density rule), or when
//!   an element gets other attributes or becomes an accessor. It returns
//!   to the (empty) dense form only when the map becomes empty. A sparse
//!   array with elements stays sparse, so a reverse fill of a large array
//!   ends sparse.
//!
//! Memory grows with the number of stored elements, never with `length`
//! or an index from the script: the dense vector is at most
//! `max(DENSE_MIN_LEN, DENSE_FILL × stored)` long.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::object::property::Property;
use crate::string::PropertyKey;
use crate::value::Value;

/// A dense vector of up to this length is always allowed.
const DENSE_MIN_LEN: usize = 64;

/// Above [`DENSE_MIN_LEN`], at least one slot in this many must hold an
/// element.
const DENSE_FILL: usize = 4;

/// The approximate bytes of one entry of the sparse map (key, property,
/// and the B-tree's share of node overhead).
pub(crate) const SPARSE_ENTRY_BYTES: usize = 64;

/// The elements of an object.
pub(crate) enum Elements {
    Dense {
        values: Vec<Value>,
        /// The number of values that are not holes.
        count: u32,
    },
    Sparse(BTreeMap<u32, Property>),
}

impl Default for Elements {
    fn default() -> Self {
        Elements::Dense {
            values: Vec::new(),
            count: 0,
        }
    }
}

impl Elements {
    /// The element at `index`.
    pub(crate) fn get(&self, index: u32) -> Option<Property> {
        match self {
            Elements::Dense { values, .. } => match values.get(index as usize) {
                Some(Value::Empty) | None => None,
                Some(&value) => Some(Property::data(value)),
            },
            Elements::Sparse(map) => map.get(&index).copied(),
        }
    }

    /// Writes the value of an existing writable data element in the dense
    /// vector. Returns false if the fast path does not apply.
    pub(crate) fn write_dense(&mut self, index: u32, value: Value) -> bool {
        if let Elements::Dense { values, .. } = self
            && let Some(slot) = values.get_mut(index as usize)
            && !slot.is_empty()
            && !value.is_empty()
        {
            *slot = value;
            return true;
        }
        false
    }

    /// The number of stored elements.
    pub(crate) fn stored(&self) -> usize {
        match self {
            Elements::Dense { count, .. } => *count as usize,
            Elements::Sparse(map) => map.len(),
        }
    }

    /// The bytes that a reservation should cover before
    /// [`Elements::insert`] at `index`: the growth of the dense vector or
    /// of a conversion to the sparse map.
    pub(crate) fn growth_estimate(&self, index: u32, property: &Property) -> usize {
        match self {
            Elements::Dense { values, count } => {
                let new_len = (index as usize + 1).max(values.len());
                if !property.is_default_data() || !dense_allowed(new_len, *count) {
                    (*count as usize + 1) * SPARSE_ENTRY_BYTES
                } else if new_len > values.capacity() {
                    new_len.max(values.capacity() * 2) * size_of::<Value>()
                } else {
                    0
                }
            }
            Elements::Sparse(_) => SPARSE_ENTRY_BYTES,
        }
    }

    /// Adds an element at an index that has none.
    pub(crate) fn insert(&mut self, index: u32, property: Property) -> Result<()> {
        if let Property::Data { value, .. } = property
            && value.is_empty()
        {
            return Err(Error::invariant("an Empty value stored as an element"));
        }
        if let Elements::Dense { values, count } = self
            && property.is_default_data()
        {
            let position = index as usize;
            if let Some(slot) = values.get_mut(position) {
                if !slot.is_empty() {
                    return Err(Error::invariant("an element inserted twice"));
                }
                *slot = property.slot_values().0;
                *count += 1;
                return Ok(());
            }
            if dense_allowed(position + 1, *count) {
                values.try_reserve(position + 1 - values.len())?;
                values.resize(position, Value::Empty);
                values.push(property.slot_values().0);
                *count += 1;
                return Ok(());
            }
        }
        self.make_sparse()?.insert(index, property);
        Ok(())
    }

    /// Replaces an existing element.
    pub(crate) fn replace(&mut self, index: u32, property: Property) -> Result<()> {
        if let Property::Data { value, .. } = property
            && value.is_empty()
        {
            return Err(Error::invariant("an Empty value stored as an element"));
        }
        if property.is_default_data() && self.write_dense(index, property.slot_values().0) {
            return Ok(());
        }
        self.make_sparse()?.insert(index, property);
        Ok(())
    }

    /// Removes the element at `index`, if any.
    pub(crate) fn remove(&mut self, index: u32) {
        match self {
            Elements::Dense { values, count } => {
                if let Some(slot) = values.get_mut(index as usize)
                    && !slot.is_empty()
                {
                    *slot = Value::Empty;
                    *count = count.saturating_sub(1);
                    while values.last().is_some_and(|value| value.is_empty()) {
                        values.pop();
                    }
                }
            }
            Elements::Sparse(map) => {
                map.remove(&index);
                self.reset_if_empty();
            }
        }
    }

    /// An empty sparse map becomes an empty dense vector again. A sparse
    /// array that still holds elements stays sparse, even when they would
    /// be dense now (for example after a reverse fill of a large array).
    fn reset_if_empty(&mut self) {
        if matches!(self, Elements::Sparse(map) if map.is_empty()) {
            *self = Elements::default();
        }
    }

    /// Step 16 of `ArraySetLength` (§10.4.2.4): deletes the elements at
    /// `new_len` and above in descending order, and stops at the first one
    /// that is not configurable. Returns that element's index. The cost
    /// is bounded by the number of stored elements.
    pub(crate) fn remove_from(&mut self, new_len: u32) -> Option<u32> {
        match self {
            Elements::Dense { values, count } => {
                let start = (new_len as usize).min(values.len());
                let removed = values.get(start..).map_or(0, |tail| {
                    tail.iter().filter(|value| !value.is_empty()).count()
                });
                *count = count.saturating_sub(removed as u32);
                values.truncate(start);
                if values.capacity() > DENSE_MIN_LEN && values.capacity() > values.len() * 4 {
                    values.shrink_to(values.len() * 2);
                }
                None
            }
            Elements::Sparse(map) => {
                let mut doomed = Vec::new();
                let mut blocking = None;
                for (&index, property) in map.range(new_len..).rev() {
                    if !property.configurable() {
                        blocking = Some(index);
                        break;
                    }
                    doomed.push(index);
                }
                for index in doomed {
                    map.remove(&index);
                }
                self.reset_if_empty();
                blocking
            }
        }
    }

    /// Appends the keys in ascending order.
    pub(crate) fn keys(&self, out: &mut Vec<PropertyKey>) {
        match self {
            Elements::Dense { values, count } => {
                out.reserve(*count as usize);
                out.extend(
                    values
                        .iter()
                        .enumerate()
                        .filter(|(_, value)| !value.is_empty())
                        .map(|(index, _)| PropertyKey::Index(index as u32)),
                );
            }
            Elements::Sparse(map) => {
                out.reserve(map.len());
                out.extend(map.keys().map(|&index| PropertyKey::Index(index)));
            }
        }
    }

    /// The bytes that the elements own.
    pub(crate) fn heap_size(&self) -> usize {
        match self {
            Elements::Dense { values, .. } => values.capacity() * size_of::<Value>(),
            Elements::Sparse(map) => map.len() * SPARSE_ENTRY_BYTES,
        }
    }

    /// Converts to the sparse form (no change if already sparse).
    fn make_sparse(&mut self) -> Result<&mut BTreeMap<u32, Property>> {
        if let Elements::Dense { values, .. } = self {
            let map = values
                .iter()
                .enumerate()
                .filter(|(_, value)| !value.is_empty())
                .map(|(index, &value)| (index as u32, Property::data(value)))
                .collect();
            *self = Elements::Sparse(map);
        }
        match self {
            Elements::Sparse(map) => Ok(map),
            Elements::Dense { .. } => Err(Error::invariant("elements did not become sparse")),
        }
    }
}

/// The density rule: whether a dense vector of `new_len` may hold
/// `count + 1` elements.
fn dense_allowed(new_len: usize, count: u32) -> bool {
    new_len <= DENSE_MIN_LEN || new_len <= (count as usize + 1) * DENSE_FILL
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_dense(elements: &Elements) -> bool {
        matches!(elements, Elements::Dense { .. })
    }

    #[test]
    fn appends_and_small_holes_stay_dense() {
        let mut elements = Elements::default();
        for index in 0..1000 {
            elements
                .insert(index, Property::data(Value::Int(index as i32)))
                .unwrap();
        }
        assert!(is_dense(&elements));
        elements.insert(1500, Property::data(Value::Null)).unwrap();
        assert!(is_dense(&elements));
        assert_eq!(elements.get(1200), None);
        assert_eq!(elements.stored(), 1001);
    }

    #[test]
    fn far_index_goes_sparse() {
        let mut elements = Elements::default();
        elements.insert(0, Property::data(Value::Int(0))).unwrap();
        elements
            .insert(4_294_967_294, Property::data(Value::Int(1)))
            .unwrap();
        assert!(!is_dense(&elements));
        assert!(elements.heap_size() < 1000);
        let mut keys = Vec::new();
        elements.keys(&mut keys);
        assert_eq!(
            keys,
            [PropertyKey::Index(0), PropertyKey::Index(4_294_967_294)]
        );
    }

    #[test]
    fn attributes_force_sparse() {
        let mut elements = Elements::default();
        elements.insert(0, Property::data(Value::Int(0))).unwrap();
        let read_only = Property::Data {
            value: Value::Int(1),
            writable: false,
            enumerable: true,
            configurable: true,
        };
        elements.replace(0, read_only).unwrap();
        assert!(!is_dense(&elements));
        assert_eq!(elements.get(0), Some(read_only));
    }

    #[test]
    fn remove_from_stops_at_non_configurable() {
        let mut elements = Elements::default();
        for index in [1, 5, 10, 20] {
            elements
                .insert(index, Property::data(Value::Int(1)))
                .unwrap();
        }
        let fixed = Property::Data {
            value: Value::Int(2),
            writable: true,
            enumerable: true,
            configurable: false,
        };
        elements.insert(7, fixed).unwrap();
        assert_eq!(elements.remove_from(3), Some(7));
        let mut keys = Vec::new();
        elements.keys(&mut keys);
        assert_eq!(
            keys,
            [
                PropertyKey::Index(1),
                PropertyKey::Index(5),
                PropertyKey::Index(7)
            ]
        );
    }

    #[test]
    fn dense_removal_trims_trailing_holes() {
        let mut elements = Elements::default();
        for index in 0..4 {
            elements
                .insert(index, Property::data(Value::Int(1)))
                .unwrap();
        }
        elements.remove(2);
        elements.remove(3);
        let Elements::Dense { values, count } = &elements else {
            panic!("still dense");
        };
        assert_eq!((values.len(), *count), (2, 2));
        assert!(elements.insert(0, Property::data(Value::Empty)).is_err());
    }

    #[test]
    fn empty_sparse_map_returns_to_dense() {
        let mut elements = Elements::default();
        elements
            .insert(1_000_000, Property::data(Value::Int(1)))
            .unwrap();
        assert!(!is_dense(&elements));
        elements.remove(1_000_000);
        assert!(is_dense(&elements));
        elements
            .insert(1_000_000, Property::data(Value::Int(1)))
            .unwrap();
        assert_eq!(elements.remove_from(0), None);
        assert!(is_dense(&elements));
        // A map with elements stays sparse.
        elements
            .insert(1_000_000, Property::data(Value::Int(1)))
            .unwrap();
        elements.insert(0, Property::data(Value::Int(1))).unwrap();
        elements.remove(1_000_000);
        assert!(!is_dense(&elements));
    }
}
