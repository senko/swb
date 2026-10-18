//! Bulk access to the elements of a typed array for the methods of
//! `%TypedArray%.prototype` that run no script code between the reads and
//! writes (ECMA-262 §23.2.3, `fill`, `reverse`, `indexOf`, `join`, `sort`
//! and others): the bytes of the array as one window of the buffer, and
//! loads and stores of whole elements as bits.
//!
//! A window is the bytes `byte_offset .. byte_offset + length × size` of
//! the buffer for the length that `TypedArrayLength` gives now, or none
//! when the array is out of bounds or detached. Callers take a window for
//! one chunk of work, so that the time countdown is charged between
//! chunks and no borrow of the heap lives across a call into the runtime.

use crate::error::{Error, Result};
use crate::heap::Heap;
use crate::object::typed_array::{TypedArrayData, load, store};

/// The bits mask of an element of `size` bytes.
pub(crate) fn mask(size: usize) -> u64 {
    if size >= 8 {
        u64::MAX
    } else {
        (1u64 << (size * 8)) - 1
    }
}

impl Heap {
    /// The bytes of the elements of a typed array and its length, or
    /// `None` when the array is out of bounds or its buffer is detached.
    pub(crate) fn typed_window(&self, data: &TypedArrayData) -> Result<Option<&[u8]>> {
        let Some(bytes) = self.typed_bytes(data)? else {
            return Ok(None);
        };
        let Some(length) = data.length_in(bytes.len()) else {
            return Ok(None);
        };
        let start = data.view.byte_offset;
        let end = length
            .checked_mul(data.ty.size())
            .and_then(|count| start.checked_add(count))
            .ok_or(Error::invariant("a typed array window past usize"))?;
        Ok(bytes.get(start..end))
    }

    /// [`Heap::typed_window`] for writing.
    pub(crate) fn typed_window_mut(&mut self, data: &TypedArrayData) -> Result<Option<&mut [u8]>> {
        let start = data.view.byte_offset;
        let size = data.ty.size();
        let Some(bytes) = self.array_buffer_bytes_mut(data.view.buffer)? else {
            return Ok(None);
        };
        let Some(length) = data.length_in(bytes.len()) else {
            return Ok(None);
        };
        let end = length
            .checked_mul(size)
            .and_then(|count| start.checked_add(count))
            .ok_or(Error::invariant("a typed array window past usize"))?;
        Ok(bytes.get_mut(start..end))
    }

    /// The bits of element `index` (the low `size` bytes of a little-endian
    /// integer), or `None` when the index is not valid.
    pub(crate) fn typed_bits(&self, data: &TypedArrayData, index: usize) -> Result<Option<u64>> {
        let Some(window) = self.typed_window(data)? else {
            return Ok(None);
        };
        let size = data.ty.size();
        let Some(bytes) = index
            .checked_mul(size)
            .and_then(|from| window.get(from..from.saturating_add(size)))
        else {
            return Ok(None);
        };
        Ok(Some(
            load(bytes).ok_or(Error::invariant("an element of another size"))?,
        ))
    }

    /// Stores `bits` as element `index` (the low `size` bytes). Returns
    /// whether it wrote: false when the index is not valid.
    pub(crate) fn typed_store_bits(
        &mut self,
        data: &TypedArrayData,
        index: usize,
        bits: u64,
    ) -> Result<bool> {
        let size = data.ty.size();
        let Some(window) = self.typed_window_mut(data)? else {
            return Ok(false);
        };
        let Some(bytes) = index
            .checked_mul(size)
            .and_then(|from| window.get_mut(from..from.saturating_add(size)))
        else {
            return Ok(false);
        };
        if !store(bytes, bits) {
            return Err(Error::invariant("an element of another size"));
        }
        Ok(true)
    }

    /// Writes `bits` to the elements `from..to` (both clamped to the
    /// current length). Returns the number of elements written.
    pub(crate) fn typed_fill(
        &mut self,
        data: &TypedArrayData,
        (from, to): (usize, usize),
        bits: u64,
    ) -> Result<usize> {
        let size = data.ty.size();
        let Some(window) = self.typed_window_mut(data)? else {
            return Ok(0);
        };
        let to = to.min(window.len() / size);
        if from >= to {
            return Ok(0);
        }
        let Some(part) = window.get_mut(from * size..to * size) else {
            return Ok(0);
        };
        let pattern = bits.to_le_bytes();
        match pattern.get(..size) {
            Some([byte]) => part.fill(*byte),
            Some(element) => {
                for chunk in part.chunks_exact_mut(size) {
                    chunk.copy_from_slice(element);
                }
            }
            None => return Err(Error::invariant("an element longer than 8 bytes")),
        }
        Ok(to - from)
    }

    /// Writes `bits` as the elements from `start` on, as many as fit in
    /// the current length. Returns the number written.
    pub(crate) fn typed_write_bits(
        &mut self,
        data: &TypedArrayData,
        start: usize,
        bits: &[u64],
    ) -> Result<usize> {
        let size = data.ty.size();
        let Some(window) = self.typed_window_mut(data)? else {
            return Ok(0);
        };
        let Some(part) = window.get_mut(start.saturating_mul(size)..) else {
            return Ok(0);
        };
        let mut written = 0;
        for (chunk, &word) in part.chunks_exact_mut(size).zip(bits) {
            if !store(chunk, word) {
                return Err(Error::invariant("an element of another size"));
            }
            written += 1;
        }
        Ok(written)
    }

    /// Copies `bytes` to the elements of a `Uint8Array` from element `at`
    /// on, as many as fit in the current length. Returns the number
    /// copied.
    pub(crate) fn typed_put_bytes(
        &mut self,
        data: &TypedArrayData,
        at: usize,
        bytes: &[u8],
    ) -> Result<usize> {
        let Some(window) = self.typed_window_mut(data)? else {
            return Ok(0);
        };
        let Some(part) = window.get_mut(at..) else {
            return Ok(0);
        };
        let count = part.len().min(bytes.len());
        if let (Some(target), Some(source)) = (part.get_mut(..count), bytes.get(..count)) {
            target.copy_from_slice(source);
        }
        Ok(count)
    }

    /// Reverses the elements of the first `length` elements of the array
    /// by swapping the pairs `first .. first + count` with their mirror
    /// images. The pairs must be in bounds (`first + count` is at most
    /// `length / 2`); the window may be shorter than `length` (a
    /// callback shrank the buffer), then the pairs outside it stay.
    pub(crate) fn typed_reverse_pairs(
        &mut self,
        data: &TypedArrayData,
        length: usize,
        (first, count): (usize, usize),
    ) -> Result<()> {
        let size = data.ty.size();
        let Some(window) = self.typed_window_mut(data)? else {
            return Ok(());
        };
        for i in first..first.saturating_add(count) {
            let Some(j) = length.checked_sub(i + 1).filter(|&j| j > i) else {
                break;
            };
            if j * size > window.len() {
                break;
            }
            let (low, high) = window.split_at_mut(j * size);
            let (Some(a), Some(b)) = (low.get_mut(i * size..(i + 1) * size), high.get_mut(..size))
            else {
                break;
            };
            a.swap_with_slice(b);
        }
        Ok(())
    }
}
