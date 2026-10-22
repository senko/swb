//! Element access for the built-in functions (ADR 0026 section 15, M7
//! feature 6a): the checks that decide when a native may take a shortcut
//! over the steps of the specification, the searches for the next stored
//! element, and the dense builder (arrays made by natives without the
//! general property definition).
//!
//! Ordinary elements: an object whose index properties are exactly its
//! stored elements, with the ordinary `[[GetOwnProperty]]`,
//! `[[HasProperty]]`, `[[Get]]`, `[[Set]]` and `[[Delete]]` for index keys
//! (§10.1), none of which runs script code except a getter or setter of
//! an accessor element. All object kinds have them except String objects
//! (the characters are index properties that are not stored) and host
//! objects (their class can answer the internal methods) and proxies (their
//! traps answer them) and typed arrays (their elements are in the buffer).
//! Mapped arguments objects have them: the
//! element of a mapped index holds the parameter's cell, which
//! `[[GetOwnProperty]]` reads.
//!
//! When an object and every prototype on its chain have ordinary
//! elements, `HasProperty(O, k)` is true exactly for the indices that some
//! object of the chain stores, so a loop that tests `HasProperty` may skip
//! the other indices: nothing observes the skipped tests. Index keys at or
//! above 2^32 − 1 are named properties (`PropertyKey::String`); the
//! searches look at them only when the range reaches that far.

use swb_js_text::Str16;

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, RootSource};
use crate::object::{Elements, Object, ObjectKind};
use crate::string::{MAX_ARRAY_LENGTH, PropertyKey};
use crate::value::Value;

/// The first integer index that is not an array index (2^32 − 1).
pub(crate) const FIRST_NAMED_INDEX: f64 = 4_294_967_295.0;

/// The longest prototype chain (with the object) for which
/// [`Heap::dense_array`] allows the dense shortcut.
pub(crate) const DENSE_CHAIN_LIMIT: usize = 64;

/// What a walk of the prototype chain found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChainInfo {
    /// The object and all its prototypes have ordinary elements.
    pub(crate) ordinary: bool,
    /// No prototype stores an element (and the chain is short enough for
    /// the dense shortcut, see [`DENSE_CHAIN_LIMIT`]).
    pub(crate) protos_empty: bool,
    /// The number of objects the walk visited (for the time charge).
    pub(crate) visited: usize,
}

/// The result of [`Heap::search_chain`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ChainSearch {
    /// The index found, if any.
    pub(crate) found: Option<f64>,
    /// Whether every object of the chain has ordinary elements (else the
    /// search stopped and `found` is `None`).
    pub(crate) ordinary: bool,
    /// The objects and slots that the search looked at.
    pub(crate) work: usize,
}

/// The state of an array whose elements a native may read and write in
/// its dense vector directly: an `Array` with dense elements whose
/// prototypes store no elements and have ordinary elements, so a hole
/// reads as `undefined` and `HasProperty` is false for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DenseArray {
    /// `length`.
    pub(crate) length: u32,
    /// Whether `length` is writable.
    pub(crate) length_writable: bool,
    /// Whether the array is extensible.
    pub(crate) extensible: bool,
    /// The length of the dense vector (at most `length`).
    pub(crate) stored_len: usize,
    /// Whether the vector has holes below `stored_len`.
    pub(crate) holes: bool,
}

impl DenseArray {
    /// Whether every index below `length` holds an element.
    pub(crate) fn hole_free(&self) -> bool {
        !self.holes && self.stored_len == self.length as usize
    }

    /// Whether the steps of a method that writes elements (with `Set`,
    /// `DeletePropertyOrThrow` and a final `Set` of `length`) cannot fail
    /// and run no script code: every new element can be created and
    /// `length` can be written.
    pub(crate) fn writable(&self) -> bool {
        self.length_writable && self.extensible
    }
}

/// Whether the index properties of an object of this kind are exactly its
/// stored elements (see the module documentation).
fn ordinary_elements(kind: &ObjectKind) -> bool {
    !matches!(
        kind,
        ObjectKind::StringWrapper(_)
            | ObjectKind::Host { .. }
            | ObjectKind::Proxy(_)
            | ObjectKind::TypedArray(_)
    )
}

/// The integer that `text` is the canonical decimal form of, if it is at
/// least 2^32 − 1 and below 2^53 − 1 (an integer index that is not an
/// array index, §6.1.7, and below the largest length).
fn named_integer_index(text: Str16<'_>) -> Option<f64> {
    let len = text.len();
    if !(10..=16).contains(&len) {
        return None;
    }
    let mut value: u64 = 0;
    for (position, unit) in text.units().enumerate() {
        let digit = unit.checked_sub(u16::from(b'0')).filter(|&d| d < 10)?;
        if digit == 0 && position == 0 {
            return None;
        }
        value = value * 10 + u64::from(digit);
    }
    let value = value as f64;
    (FIRST_NAMED_INDEX..crate::vm::convert::MAX_SAFE_INTEGER)
        .contains(&value)
        .then_some(value)
}

impl Heap {
    /// The value of the element `k` of `object` if the object stores it in
    /// its dense vector and has ordinary elements: then `HasProperty(object,
    /// k)` is true and `Get(object, k)` is the value, without effects and
    /// without a look at the prototype chain. `None` otherwise (a hole, a
    /// mapped argument, sparse or exotic elements, an index beyond the
    /// vector); the caller takes the general steps then.
    #[inline]
    pub(crate) fn dense_element(&self, object: Gc<Object>, k: f64) -> Option<Value> {
        let record = self.object(object).ok()?;
        let Elements::Dense { values, .. } = &record.elements else {
            return None;
        };
        if !(0.0..values.len() as f64).contains(&k) || !ordinary_elements(&record.kind) {
            return None;
        }
        let value = *values.get(k as usize)?;
        (!value.is_internal()).then_some(value)
    }

    /// Walks the prototype chain of `object` (see [`ChainInfo`]). The walk
    /// stops at the first object without ordinary elements.
    pub(crate) fn element_chain(&self, object: Gc<Object>) -> Result<ChainInfo> {
        let mut info = ChainInfo {
            ordinary: true,
            protos_empty: true,
            visited: 1,
        };
        let record = self.object(object)?;
        if !ordinary_elements(&record.kind) {
            info.ordinary = false;
            return Ok(info);
        }
        let mut current = self.arenas.shapes.get(record.shape)?.proto;
        while let Some(proto) = current {
            info.visited += 1;
            if info.visited > DENSE_CHAIN_LIMIT {
                // A long chain takes the charged searches instead: the
                // check itself is not charged.
                info.protos_empty = false;
                return Ok(info);
            }
            let record = self.object(proto)?;
            if !ordinary_elements(&record.kind) {
                info.ordinary = false;
                info.protos_empty = false;
                return Ok(info);
            }
            if record.elements.stored() > 0 {
                info.protos_empty = false;
            }
            current = self.arenas.shapes.get(record.shape)?.proto;
        }
        Ok(info)
    }

    /// Whether the index properties of `object` are its stored elements
    /// (see the module documentation).
    pub(crate) fn has_ordinary_elements(&self, object: Gc<Object>) -> Result<bool> {
        Ok(ordinary_elements(&self.object(object)?.kind))
    }

    /// The state of `object` if it is a [`DenseArray`]; `None` for every
    /// other object and form.
    pub(crate) fn dense_array(&self, object: Gc<Object>) -> Result<Option<DenseArray>> {
        let record = self.object(object)?;
        let ObjectKind::Array {
            length,
            length_writable,
        } = record.kind
        else {
            return Ok(None);
        };
        let Elements::Dense { values, count } = &record.elements else {
            return Ok(None);
        };
        let state = DenseArray {
            length,
            length_writable,
            extensible: record.extensible,
            stored_len: values.len(),
            holes: *count as usize != values.len(),
        };
        let chain = self.element_chain(object)?;
        Ok((chain.ordinary && chain.protos_empty).then_some(state))
    }

    /// The smallest integer index `k` in `from..end` (integers below 2^53)
    /// that `object` itself stores, with the number of slots that the
    /// search looked at. Named integer keys (2^32 − 1 and above) count
    /// only when `end` reaches them.
    pub(crate) fn next_own_index(
        &self,
        object: Gc<Object>,
        from: f64,
        end: f64,
    ) -> Result<(Option<f64>, usize)> {
        let record = self.object(object)?;
        let mut work = 0;
        if from < FIRST_NAMED_INDEX {
            let (found, scanned) = record
                .elements
                .next_index(from as u32, end.min(FIRST_NAMED_INDEX) as u32);
            work += scanned;
            if let Some(index) = found {
                return Ok((Some(f64::from(index)), work));
            }
        }
        if end > FIRST_NAMED_INDEX {
            let (found, scanned) = self.named_index(record, |k| k >= from && k < end, false)?;
            return Ok((found, work + scanned));
        }
        Ok((None, work))
    }

    /// The largest integer index `k` in `low..=from` that `object` itself
    /// stores, with the number of slots that the search looked at.
    pub(crate) fn prev_own_index(
        &self,
        object: Gc<Object>,
        from: f64,
        low: f64,
    ) -> Result<(Option<f64>, usize)> {
        let record = self.object(object)?;
        let mut work = 0;
        if from >= FIRST_NAMED_INDEX {
            let (found, scanned) = self.named_index(record, |k| k >= low && k <= from, true)?;
            work += scanned;
            if found.is_some() {
                return Ok((found, work));
            }
        }
        if low < FIRST_NAMED_INDEX {
            let top = from.min(FIRST_NAMED_INDEX - 1.0) as u32;
            let (found, scanned) = record.elements.prev_index(top, low as u32);
            return Ok((found.map(f64::from), work + scanned));
        }
        Ok((None, work))
    }

    /// The nearest integer index that `[[HasProperty]]` finds on `object`
    /// or its prototypes: with `ascending` the smallest in `from..bound`,
    /// else the largest in `bound..=from`. Stops with `ordinary: false` at
    /// the first object without ordinary elements.
    pub(crate) fn search_chain(
        &self,
        object: Gc<Object>,
        from: f64,
        bound: f64,
        ascending: bool,
    ) -> Result<ChainSearch> {
        let mut search = ChainSearch {
            found: None,
            ordinary: true,
            work: 0,
        };
        let mut current = Some(object);
        while let Some(holder) = current {
            let record = self.object(holder)?;
            search.work += 1;
            if !ordinary_elements(&record.kind) {
                search.ordinary = false;
                search.found = None;
                return Ok(search);
            }
            if record.elements.stored() > 0 || !record.slots.is_empty() {
                let (found, work) = if ascending {
                    self.next_own_index(holder, from, search.found.unwrap_or(bound))?
                } else {
                    let low = search.found.map_or(bound, |best| best + 1.0);
                    self.prev_own_index(holder, from, low)?
                };
                search.work += work;
                if found.is_some() {
                    search.found = found;
                }
            }
            current = self.arenas.shapes.get(record.shape)?.proto;
        }
        Ok(search)
    }

    /// The smallest (or with `largest` the largest) named integer key of
    /// `record` for which `accept` holds, with the number of keys looked
    /// at.
    fn named_index(
        &self,
        record: &Object,
        accept: impl Fn(f64) -> bool,
        largest: bool,
    ) -> Result<(Option<f64>, usize)> {
        let shape = self.arenas.shapes.get(record.shape)?;
        let entries = shape.entries(&self.arenas.key_lists)?;
        let mut best: Option<f64> = None;
        for entry in entries {
            let PropertyKey::String(name) = entry.key else {
                continue;
            };
            if entry.flags.deleted() {
                continue;
            }
            let Some(text) = self.string(name)?.as_flat() else {
                continue;
            };
            if let Some(k) = named_integer_index(text).filter(|&k| accept(k)) {
                best = Some(match best {
                    Some(b) if largest => b.max(k),
                    Some(b) => b.min(k),
                    None => k,
                });
            }
        }
        Ok((best, entries.len()))
    }

    // --- Changes of a dense vector in place ---

    /// Runs `change` on the dense vector of the array `array` and its
    /// `length`, for a native that has checked with [`Heap::dense_array`]
    /// that the change gives the result of the specification's steps.
    /// Afterwards the count is recomputed, trailing holes are dropped and
    /// the growth is charged. The vector must not grow past `length`, and
    /// `change` must not store a cell. Growth is reserved before (by the
    /// caller, [`Heap::reserve_with`]).
    pub(crate) fn change_dense<T>(
        &mut self,
        array: Gc<Object>,
        change: impl FnOnce(&mut Vec<Value>, &mut u32) -> Result<T>,
    ) -> Result<T> {
        let before = self.object_bytes(array)?;
        let record = self.object_mut(array)?;
        let ObjectKind::Array { length, .. } = &mut record.kind else {
            return Err(Error::invariant(
                "a dense change of an object that is not an array",
            ));
        };
        let Elements::Dense { values, count } = &mut record.elements else {
            return Err(Error::invariant("a dense change of sparse elements"));
        };
        let result = change(values, length)?;
        while values.last().is_some_and(|value| value.is_empty()) {
            values.pop();
        }
        if values.len() > *length as usize || values.iter().any(|v| matches!(v, Value::Cell(_))) {
            return Err(Error::invariant("a dense change broke the elements"));
        }
        *count = values.iter().filter(|value| !value.is_empty()).count() as u32;
        self.charge_growth(array, before)?;
        Ok(result)
    }

    /// Stores `values` at `start..` in the dense vector of `array`, inside
    /// the vector (holes become elements). The caller has checked that this
    /// is the result of the steps ([`Heap::dense_array`]). Returns false
    /// (and changes nothing) if the range is not inside the vector.
    pub(crate) fn overwrite_dense(
        &mut self,
        array: Gc<Object>,
        start: usize,
        values: &[Value],
    ) -> Result<bool> {
        if values.iter().any(|value| value.is_internal()) {
            return Err(Error::invariant("an internal value stored as an element"));
        }
        let Elements::Dense {
            values: vector,
            count,
        } = &mut self.object_mut(array)?.elements
        else {
            return Ok(false);
        };
        let Some(range) = vector.get_mut(start..start + values.len()) else {
            return Ok(false);
        };
        let holes = range.iter().filter(|value| value.is_empty()).count();
        range.copy_from_slice(values);
        *count += holes as u32;
        Ok(true)
    }

    /// The bytes that growing the dense vector of `array` to `len`
    /// elements costs (0 if the capacity is enough).
    pub(crate) fn dense_growth(&self, array: Gc<Object>, len: usize) -> Result<usize> {
        Ok(match &self.object(array)?.elements {
            Elements::Dense { values, .. } if len > values.capacity() => {
                (len - values.capacity()) * size_of::<Value>()
            }
            _ => 0,
        })
    }

    /// The steps of `pop` after `length` (`Get`, `DeletePropertyOrThrow`
    /// and `Set` of `length`) for an `Array` with dense elements and a
    /// writable `length` above 0, whose prototypes the caller checked
    /// ([`Heap::dense_array`]): the last element (`None` for a hole),
    /// removed. Returns `Err` for another form (an internal error: the
    /// caller checks first).
    pub(crate) fn pop_dense(&mut self, array: Gc<Object>) -> Result<Option<Value>> {
        let record = self.object_mut(array)?;
        let ObjectKind::Array {
            length,
            length_writable: true,
        } = &mut record.kind
        else {
            return Err(Error::invariant("a dense pop of another object"));
        };
        let Elements::Dense { values, count } = &mut record.elements else {
            return Err(Error::invariant("a dense pop of sparse elements"));
        };
        if *length == 0 {
            return Err(Error::invariant("a dense pop of an empty array"));
        }
        *length -= 1;
        if values.len() <= *length as usize {
            // The last index is a hole.
            return Ok(None);
        }
        let last = values.pop().filter(|value| !value.is_empty());
        if last.is_some() {
            *count = count.saturating_sub(1);
        }
        while values.last().is_some_and(|value| value.is_empty()) {
            values.pop();
        }
        Ok(last)
    }

    /// `CreateDataProperty(target, at + k, source[k])` for each element `k`
    /// of the dense array `source` (the spread step of `concat`), when
    /// `target` is an extensible `Array` with dense elements and a
    /// writable `length` that stores nothing at or above `at`, and the
    /// copy keeps the vector dense. Returns false (and changes nothing)
    /// otherwise. Can collect (the growth is reserved first).
    pub(crate) fn copy_dense(
        &mut self,
        target: Gc<Object>,
        at: u32,
        source: Gc<Object>,
        roots: &dyn RootSource,
    ) -> Result<bool> {
        let (source_len, source_count) = match &self.object(source)?.elements {
            Elements::Dense { values, count } => (values.len(), *count as usize),
            Elements::Sparse(_) => return Ok(false),
        };
        let record = self.object(target)?;
        let ObjectKind::Array {
            length_writable: true,
            ..
        } = record.kind
        else {
            return Ok(false);
        };
        let Elements::Dense { values, count } = &record.elements else {
            return Ok(false);
        };
        let start = at as usize;
        let new_len = start + source_len;
        if !record.extensible
            || values.len() > start
            || new_len >= MAX_ARRAY_LENGTH as usize
            || !Elements::dense_fits(new_len, *count as usize + source_count)
        {
            return Ok(false);
        }
        let bytes = new_len.saturating_sub(values.capacity()) * size_of::<Value>();
        self.reserve_with(bytes, roots, &[target.into(), source.into()])?;
        let before = self.object_bytes(target)?;
        let copy: Vec<Value> = match &self.object(source)?.elements {
            Elements::Dense { values, .. } => values.clone(),
            Elements::Sparse(_) => {
                return Err(Error::invariant("the source of a copy became sparse"));
            }
        };
        let record = self.object_mut(target)?;
        let Elements::Dense { values, count } = &mut record.elements else {
            return Err(Error::invariant("the target of a copy became sparse"));
        };
        values.try_reserve_exact(new_len - values.len())?;
        values.resize(start, Value::Empty);
        values.extend_from_slice(&copy);
        *count += source_count as u32;
        while values.last().is_some_and(|value| value.is_empty()) {
            values.pop();
        }
        let stored = values.len() as u32;
        if let ObjectKind::Array { length, .. } = &mut record.kind
            && stored > *length
        {
            *length = stored;
        }
        self.charge_growth(target, before)?;
        Ok(true)
    }

    // --- The dense builder ---

    /// `ArrayCreate(0)` (§10.4.2.2) with room for `capacity` elements in
    /// its dense vector: the bytes are reserved once (a safepoint; the
    /// prototype must be alive, as the intrinsics of a realm are).
    pub(crate) fn new_array_with_capacity(
        &mut self,
        proto: Option<Gc<Object>>,
        capacity: usize,
        roots: &dyn RootSource,
    ) -> Result<Gc<Object>> {
        let capacity = capacity.min(MAX_ARRAY_LENGTH as usize);
        let bytes = capacity.saturating_mul(size_of::<Value>());
        self.reserve(bytes, roots)?;
        let array = self.new_array(proto, 0)?;
        let before = self.object_bytes(array)?;
        if let Elements::Dense { values, .. } = &mut self.object_mut(array)?.elements {
            values.try_reserve_exact(capacity)?;
        }
        self.charge_growth(array, before)?;
        Ok(array)
    }

    /// A new array whose elements are `values` (`CreateArrayFromList`,
    /// §7.3.17), made in one step: one reservation, one copy. The values
    /// stay alive during the reservation (the prototype must be alive). An
    /// `Empty` value is a hole.
    pub(crate) fn new_array_from(
        &mut self,
        proto: Option<Gc<Object>>,
        values: &[Value],
        roots: &dyn RootSource,
    ) -> Result<Gc<Object>> {
        if values.len() >= MAX_ARRAY_LENGTH as usize {
            return Err(Error::range_error(super::INVALID_ARRAY_LENGTH));
        }
        let bytes = size_of_val(values);
        self.reserve_with(bytes, roots, values)?;
        let array = self.new_array(proto, 0)?;
        if !self.extend_elements(array, values)? {
            return Err(Error::invariant("a new array is not dense"));
        }
        Ok(array)
    }

    /// Appends `value` to `array` as `CreateDataProperty(array, length,
    /// value)` (§7.3.5) when `array` is an extensible `Array` with a
    /// writable `length` whose elements are dense without holes. Returns
    /// false (and changes nothing) for every other form. Never collects:
    /// a vector that is full grows by doubling after a check of the heap
    /// limit.
    pub(crate) fn push_element(&mut self, array: Gc<Object>, value: Value) -> Result<bool> {
        self.extend_elements(array, std::slice::from_ref(&value))
    }

    /// Appends `values` (an `Empty` value is a hole) at the end of the
    /// array `array` (see [`Heap::push_element`]). Never collects; growth
    /// checks the heap limit and is charged.
    pub(crate) fn extend_elements(&mut self, array: Gc<Object>, values: &[Value]) -> Result<bool> {
        if values.iter().any(|value| matches!(value, Value::Cell(_))) {
            return Err(Error::invariant("a cell stored as an element"));
        }
        let record = self.object(array)?;
        let ObjectKind::Array {
            length,
            length_writable: true,
        } = record.kind
        else {
            return Ok(false);
        };
        let Elements::Dense {
            values: vector,
            count,
        } = &record.elements
        else {
            return Ok(false);
        };
        if !record.extensible || vector.len() != length as usize || *count != length {
            return Ok(false);
        }
        let Some(new_length) = length
            .checked_add(values.len() as u32)
            .filter(|&n| n < MAX_ARRAY_LENGTH && values.len() < MAX_ARRAY_LENGTH as usize)
        else {
            return Ok(false);
        };
        let spare = vector.capacity() - vector.len();
        if values.len() > spare {
            let grown = (vector.len() + values.len()).max(vector.capacity() * 2);
            self.check_room((grown - vector.capacity()) * size_of::<Value>())?;
        }
        let before = self.object_bytes(array)?;
        let record = self.object_mut(array)?;
        let Elements::Dense {
            values: vector,
            count,
        } = &mut record.elements
        else {
            return Err(Error::invariant("dense elements changed form"));
        };
        if values.len() > spare {
            let grown = (vector.len() + values.len()).max(vector.capacity() * 2);
            vector.try_reserve_exact(grown - vector.len())?;
        }
        vector.extend_from_slice(values);
        let holes = values.iter().filter(|value| value.is_empty()).count();
        *count += (values.len() - holes) as u32;
        if let ObjectKind::Array { length, .. } = &mut record.kind {
            *length = new_length;
        }
        if holes > 0 {
            // A trailing hole is not stored.
            while vector.last().is_some_and(|value| value.is_empty()) {
                vector.pop();
            }
        }
        self.charge_growth(array, before)?;
        Ok(true)
    }

    /// `CreateDataProperty` (§7.3.5) for an index of an ordinary object or
    /// an array with dense elements when no buffer must grow: an existing
    /// element gets the value (it is a default data property), a hole or
    /// the next index of an extensible object gets an element. Returns
    /// false (and changes nothing) where the general definition is needed.
    pub(crate) fn store_dense(
        &mut self,
        object: Gc<Object>,
        index: u32,
        value: Value,
    ) -> Result<bool> {
        if value.is_internal() {
            return Ok(false);
        }
        let record = self.object_mut(object)?;
        let array = match record.kind {
            ObjectKind::Array {
                length,
                length_writable,
            } => Some((length, length_writable)),
            ObjectKind::Ordinary => None,
            _ => return Ok(false),
        };
        let extensible = record.extensible;
        let Elements::Dense { values, count } = &mut record.elements else {
            return Ok(false);
        };
        let position = index as usize;
        match values.get_mut(position) {
            Some(slot) if !slot.is_empty() => *slot = value,
            Some(slot) => {
                if !extensible {
                    return Ok(false);
                }
                *slot = value;
                *count += 1;
            }
            None => {
                if !extensible || position != values.len() || values.len() == values.capacity() {
                    return Ok(false);
                }
                if let Some((length, false)) = array
                    && index >= length
                {
                    return Ok(false);
                }
                values.push(value);
                *count += 1;
                if let (Some((length, _)), ObjectKind::Array { length: field, .. }) =
                    (array, &mut record.kind)
                    && index >= length
                {
                    *field = index + 1;
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{HeapConfig, NoRoots};

    #[test]
    fn named_integer_indices() {
        let text = |s: &str| swb_js_text::String16::from(s);
        assert_eq!(
            named_integer_index(text("4294967295").as_str16()),
            Some(4_294_967_295.0)
        );
        assert_eq!(named_integer_index(text("4294967294").as_str16()), None);
        assert_eq!(
            named_integer_index(text("9007199254740991").as_str16()),
            None
        );
        assert_eq!(
            named_integer_index(text("9007199254740990").as_str16()),
            Some(9_007_199_254_740_990.0)
        );
        assert_eq!(named_integer_index(text("04294967295").as_str16()), None);
        assert_eq!(
            named_integer_index(text("42949672950").as_str16()),
            Some(42_949_672_950.0)
        );
    }

    #[test]
    fn builder_makes_dense_arrays() {
        let mut heap = Heap::new(HeapConfig::default());
        let array = heap.new_array_with_capacity(None, 4, &NoRoots).unwrap();
        for i in 0..10 {
            assert!(heap.push_element(array, Value::Int(i)).unwrap());
        }
        assert_eq!(heap.array_length(array).unwrap(), (10, true));
        let state = heap.dense_array(array).unwrap().unwrap();
        assert!(state.hole_free());
        assert!(
            heap.extend_elements(array, &[Value::Int(1), Value::Empty, Value::Empty])
                .unwrap()
        );
        assert_eq!(heap.array_length(array).unwrap(), (13, true));
        let state = heap.dense_array(array).unwrap().unwrap();
        assert_eq!(state.stored_len, 11);
        // Not hole free any more: the builder refuses.
        assert!(!heap.push_element(array, Value::Int(1)).unwrap());
        let from = heap
            .new_array_from(None, &[Value::Int(1), Value::Null], &NoRoots)
            .unwrap();
        assert_eq!(heap.array_length(from).unwrap(), (2, true));
        let (next, _) = heap.next_own_index(array, 3.0, 13.0).unwrap();
        assert_eq!(next, Some(3.0));
        let (next, _) = heap.next_own_index(array, 11.0, 13.0).unwrap();
        assert_eq!(next, None);
        let (prev, _) = heap.prev_own_index(array, 12.0, 0.0).unwrap();
        assert_eq!(prev, Some(10.0));
    }
}
