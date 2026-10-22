//! The dense vector of elements with a start offset (ADR 0026 sections 6
//! and 15, M7 13d).
//!
//! `Array.prototype.shift` on a dense array removes the first element. A
//! plain vector moves all the others, so a loop `while (a.length)
//! a.shift()` costs the square of the length. [`DenseVec`] keeps a start
//! offset instead: [`DenseVec::pop_front`] advances it, and the vector
//! reads as the slice after it (`Deref`), so every reader and writer of
//! the elements sees the elements only. The slots before the offset are
//! dead: the collector does not read them. Any operation that needs the
//! elements to start at the beginning of the buffer (`splice`) first
//! moves them there, once ([`DenseVec::normalize`]).
//! The space is reclaimed when the offset is at least half of the buffer
//! (and at least [`COMPACT_FROM`] slots): the elements move down, which
//! costs at most as many moves as the shifts that led to it, so a series
//! of shifts costs O(1) amortized for each. When the last element goes,
//! the offset resets to 0.

use std::collections::TryReserveError;
use std::ops::{Deref, DerefMut, RangeBounds};

use crate::value::Value;

/// The least offset at which the elements move down to reclaim the space.
const COMPACT_FROM: usize = 16;

/// A vector of values that can drop its first element in O(1).
#[derive(Debug, Default)]
pub(crate) struct DenseVec {
    /// The slots; the elements are `buf[head..]`.
    buf: Vec<Value>,
    /// The number of dead slots at the start.
    head: u32,
    /// The number of elements that are not holes (kept by the callers).
    /// With `head` it fills the 8 bytes after the buffer, so the record
    /// of an object keeps its size.
    pub(crate) count: u32,
}

impl DenseVec {
    /// An empty vector.
    pub(crate) const fn new() -> Self {
        DenseVec {
            buf: Vec::new(),
            head: 0,
            count: 0,
        }
    }

    /// A vector of `values` with `count` elements that are not holes.
    pub(crate) fn from_parts(values: Vec<Value>, count: u32) -> Self {
        DenseVec {
            buf: values,
            head: 0,
            count,
        }
    }

    /// The number of elements.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.buf.len() - self.start()
    }

    /// The element at `index`.
    #[inline]
    pub(crate) fn element(&self, index: usize) -> Option<&Value> {
        self.buf.get(index.checked_add(self.start())?)
    }

    /// The element at `index`, for writing.
    #[inline]
    pub(crate) fn element_mut(&mut self, index: usize) -> Option<&mut Value> {
        let at = index.checked_add(self.start())?;
        self.buf.get_mut(at)
    }

    /// The elements that fit without a new allocation (counting from the
    /// first element).
    pub(crate) fn capacity(&self) -> usize {
        self.buf.capacity() - self.start()
    }

    /// The bytes of the buffer, the dead slots at the start included.
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.buf.capacity() * size_of::<Value>()
    }

    /// The number of dead slots at the start (for tests).
    #[cfg(test)]
    pub(crate) fn offset(&self) -> usize {
        self.start()
    }

    /// The index of the first element in the buffer.
    #[inline]
    fn start(&self) -> usize {
        self.head as usize
    }

    /// Appends a value.
    pub(crate) fn push(&mut self, value: Value) {
        self.buf.push(value);
    }

    /// Removes and returns the last value.
    pub(crate) fn pop(&mut self) -> Option<Value> {
        if self.buf.len() == self.start() {
            return None;
        }
        let last = self.buf.pop();
        if self.buf.len() == self.start() {
            self.reset();
        }
        last
    }

    /// Removes and returns the first value without moving the others.
    pub(crate) fn pop_front(&mut self) -> Option<Value> {
        let first = *self.buf.get(self.start())?;
        self.head += 1;
        if self.start() == self.buf.len() {
            self.reset();
        } else if self.start() >= COMPACT_FROM && self.start() >= self.buf.len() - self.start() {
            self.normalize();
        }
        Some(first)
    }

    /// Shrinks the buffer to hold at least `min` values (the elements
    /// move to the start of the buffer).
    pub(crate) fn shrink_to(&mut self, min: usize) {
        self.normalize();
        self.buf.shrink_to(min);
    }

    /// Keeps the first `len` values.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.buf.truncate(self.start().saturating_add(len));
        if self.buf.len() == self.start() {
            self.reset();
        }
    }

    /// Sets the length to `len`, filling new slots with `value`.
    pub(crate) fn resize(&mut self, len: usize, value: Value) {
        self.buf.resize(self.start().saturating_add(len), value);
    }

    /// Appends the values of a slice.
    pub(crate) fn extend_from_slice(&mut self, values: &[Value]) {
        self.buf.extend_from_slice(values);
    }

    /// Makes room for `additional` more values.
    pub(crate) fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.buf.try_reserve(additional)
    }

    /// Makes room for exactly `additional` more values.
    pub(crate) fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.buf.try_reserve_exact(additional)
    }

    /// Replaces a range, as `Vec::splice`; the elements move to the start
    /// of the buffer first.
    pub(crate) fn splice<R, I>(&mut self, range: R, with: I) -> std::vec::Splice<'_, I::IntoIter>
    where
        R: RangeBounds<usize>,
        I: IntoIterator<Item = Value>,
    {
        self.normalize();
        self.buf.splice(range, with)
    }

    /// Moves the elements to the start of the buffer.
    fn normalize(&mut self) {
        if self.head > 0 {
            self.buf.drain(..self.start());
            self.head = 0;
        }
    }

    /// Empties the vector and resets the offset.
    fn reset(&mut self) {
        self.buf.clear();
        self.head = 0;
    }
}

impl Deref for DenseVec {
    type Target = [Value];

    #[inline]
    fn deref(&self) -> &[Value] {
        self.buf.get(self.head as usize..).unwrap_or_default()
    }
}

impl DerefMut for DenseVec {
    #[inline]
    fn deref_mut(&mut self) -> &mut [Value] {
        self.buf.get_mut(self.head as usize..).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(range: std::ops::Range<i32>) -> DenseVec {
        let values: Vec<Value> = range.map(Value::Int).collect();
        let count = values.len() as u32;
        DenseVec::from_parts(values, count)
    }

    #[test]
    fn pop_front_advances_the_offset_and_reads_the_rest() {
        let mut v = ints(0..8);
        assert_eq!(v.pop_front(), Some(Value::Int(0)));
        assert_eq!(v.pop_front(), Some(Value::Int(1)));
        assert_eq!(v.len(), 6);
        assert_eq!(v[0], Value::Int(2));
        assert_eq!(v.last(), Some(&Value::Int(7)));
        assert_eq!(v.offset(), 2);
        v.push(Value::Int(8));
        assert_eq!(v.len(), 7);
        assert_eq!(v.pop(), Some(Value::Int(8)));
    }

    #[test]
    fn the_offset_resets_when_the_vector_empties() {
        let mut v = ints(0..3);
        for expected in 0..3 {
            assert_eq!(v.pop_front(), Some(Value::Int(expected)));
        }
        assert_eq!(v.offset(), 0);
        assert!(v.is_empty());
        assert_eq!(v.pop_front(), None);
        assert_eq!(v.pop(), None);
    }

    #[test]
    fn the_space_is_reclaimed_when_the_offset_is_half() {
        let mut v = ints(0..100);
        for _ in 0..49 {
            v.pop_front();
        }
        assert_eq!(v.offset(), 49);
        v.pop_front();
        // 50 dead slots and 50 elements: the elements move down.
        assert_eq!(v.offset(), 0);
        assert_eq!(v.len(), 50);
        assert_eq!(v[0], Value::Int(50));
    }

    #[test]
    fn operations_that_need_the_start_normalize_first() {
        let mut v = ints(0..40);
        for _ in 0..5 {
            v.pop_front();
        }
        let none: Vec<Value> = v.splice(0..0, [Value::Int(-1)]).collect();
        assert_eq!(none, Vec::<Value>::new());
        assert_eq!(v.offset(), 0);
        assert_eq!(v[0], Value::Int(-1));
        assert_eq!(v[1], Value::Int(5));
        assert_eq!(v.pop_front(), Some(Value::Int(-1)));
        let removed: Vec<Value> = v.splice(0..2, [Value::Int(9)]).collect();
        assert_eq!(removed, vec![Value::Int(5), Value::Int(6)]);
        assert_eq!(v[0], Value::Int(9));
        assert_eq!(v[1], Value::Int(7));
    }

    #[test]
    fn truncate_and_resize_count_from_the_first_element() {
        let mut v = ints(0..10);
        v.pop_front();
        v.pop_front();
        v.truncate(3);
        assert_eq!(&*v, &[Value::Int(2), Value::Int(3), Value::Int(4)]);
        v.resize(5, Value::Empty);
        assert_eq!(v.len(), 5);
        assert!(v[4].is_empty());
        v.truncate(0);
        assert!(v.is_empty());
        assert_eq!(v.offset(), 0);
    }
}
