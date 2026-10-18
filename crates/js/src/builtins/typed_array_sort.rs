//! `%TypedArray%.prototype.sort` and `toSorted` (ECMA-262 §23.2.3.29,
//! §23.2.3.34, <https://tc39.es/ecma262/#sec-%typedarray%.prototype.sort>)
//! with `SortIndexedProperties` (§23.1.3.30.1) and the numeric order of
//! `CompareTypedArrayElements` (§23.2.4.7): −0 sorts before +0, NaN last.
//!
//! With a comparator the elements are read first (the comparator can
//! change the array, and the sort works on those values), kept in an
//! internal heap array that the handle scope roots, and sorted with a
//! top-down merge sort over a permutation (Sedgewick and Wayne,
//! "Algorithms", 4th edition, section 2.2): split in the middle (the left
//! half is the shorter one), sort both halves, merge taking the left
//! element unless the comparator, called as `comparator(left, right)`,
//! answers above zero. That is the order of the calls and the result for
//! inconsistent comparators of Chromium 148 (measured; the merge sort of
//! `Array.prototype.sort` calls `comparator(later, earlier)` and differs).
//! The sorted values are written back with `Set`, which skips an index
//! that the comparator made invalid. Without a comparator, no script code
//! runs and the elements sort as keys: an integer is biased so that the
//! unsigned order is the signed order, a float becomes a key whose
//! unsigned order is the numeric order. The NaNs have the greatest key
//! and are written back after the numbers as they were, in their order
//! with their bit patterns (Chromium keeps them). One-byte types, and
//! two-byte types of 262,144 elements or more, are counted into a table
//! (no memory per element; a float type counts its NaNs first and keeps
//! those bits only). Other types sort keys (32 or 64 bits, one slot per
//! element; the NaN bits fill the vector from the back, so they cost no
//! more memory) with runs of `sort_unstable` and a bottom-up merge that
//! charges the time countdown as it goes, so that a sort of a large array
//! ends near the deadline. The memory (keys and the merge buffer) is
//! reserved first: the heap limit is a `RangeError`.

use crate::object::{ElementType, TypedArrayData, load, mask};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::array_buffer::ALLOCATION_FAILED;
use super::array_sort::{comparator_arg, item};
use super::typed_array_create::CHARGE_CHUNK;
use super::typed_array_method::validate_this;

// --- The keys of the numeric order ---

/// A sort key of 32 or 64 bits.
trait Key: Copy + Ord + Default {
    fn from_key(key: u64) -> Self;
    fn to_key(self) -> u64;
}

impl Key for u32 {
    fn from_key(key: u64) -> Self {
        key as u32
    }
    fn to_key(self) -> u64 {
        u64::from(self)
    }
}

impl Key for u64 {
    fn from_key(key: u64) -> Self {
        key
    }
    fn to_key(self) -> u64 {
        self
    }
}

/// The sign bit of an element of `size` bytes.
fn sign_bit(size: usize) -> u64 {
    1u64 << (size * 8 - 1)
}

/// Whether the float with these bits is a NaN.
fn is_nan_bits(ty: ElementType, bits: u64) -> bool {
    match ty {
        ElementType::Float16 => bits & 0x7c00 == 0x7c00 && bits & 0x03ff != 0,
        ElementType::Float32 => f32::from_bits(bits as u32).is_nan(),
        _ => f64::from_bits(bits).is_nan(),
    }
}

fn is_float(ty: ElementType) -> bool {
    matches!(
        ty,
        ElementType::Float16 | ElementType::Float32 | ElementType::Float64
    )
}

/// The key of an element: its unsigned order is the sort order.
fn key_of(ty: ElementType, bits: u64) -> u64 {
    let size = ty.size();
    if is_float(ty) {
        if is_nan_bits(ty, bits) {
            return mask(size);
        }
        return if bits & sign_bit(size) != 0 {
            !bits & mask(size)
        } else {
            bits | sign_bit(size)
        };
    }
    match ty {
        ElementType::Int8 | ElementType::Int16 | ElementType::Int32 | ElementType::BigInt64 => {
            bits ^ sign_bit(size)
        }
        _ => bits,
    }
}

/// The bits of the element with this key (the inverse of [`key_of`] for
/// a number; the NaN key is not used).
fn bits_of_key(ty: ElementType, key: u64) -> u64 {
    let size = ty.size();
    if is_float(ty) {
        return if key & sign_bit(size) != 0 {
            key & !sign_bit(size)
        } else {
            !key & mask(size)
        };
    }
    match ty {
        ElementType::Int8 | ElementType::Int16 | ElementType::Int32 | ElementType::BigInt64 => {
            key ^ sign_bit(size)
        }
        _ => key,
    }
}

// --- The sort without a comparator ---

/// Reserves `bytes` of memory outside the heap for the sort, or the
/// `RangeError` of V8 for an allocation that fails.
fn reserve(rt: &mut Runtime, bytes: usize) -> VmResult<()> {
    if rt.heap.reserve_buffer(bytes, &rt.vm) {
        Ok(())
    } else {
        Err(VmError::range_error(ALLOCATION_FAILED))
    }
}

/// Arrays of 2-byte elements below this length sort as keys: the counting
/// table has 65,536 counters, and clearing and scanning them costs more
/// than sorting the keys of fewer than about 260,000 elements (measured,
/// docs/performance.md).
const COUNT_SORT_MIN: usize = 262_144;

/// Sorts the first `length` elements of `source` into the start of
/// `target` (the same array for `sort`, a new one for `toSorted`).
fn sort_numeric(
    rt: &mut Runtime,
    source: &TypedArrayData,
    target: &TypedArrayData,
    length: usize,
) -> VmResult<()> {
    match source.ty.size() {
        1 => count_sort(rt, source, target, length, 256),
        2 if length >= COUNT_SORT_MIN => count_sort(rt, source, target, length, 65536),
        2 | 4 => merge_keys::<u32>(rt, source, target, length),
        _ => merge_keys::<u64>(rt, source, target, length),
    }
}

/// Counting sort for the types of one and two bytes: a table of
/// `buckets` counts, indexed by the key.
fn count_sort(
    rt: &mut Runtime,
    source: &TypedArrayData,
    target: &TypedArrayData,
    length: usize,
    buckets: usize,
) -> VmResult<()> {
    let (ty, size) = (source.ty, source.ty.size());
    // The NaNs are kept as they were, so a float type counts them first
    // and reserves memory for those only (the bits fit in 16 bits).
    let nan_count = if is_float(ty) {
        count_nans(rt, source, length)?
    } else {
        0
    };
    reserve(
        rt,
        (buckets * size_of::<u64>()).saturating_add(nan_count * size_of::<u16>()),
    )?;
    let mut counts = vec![0u64; buckets];
    let mut nans: Vec<u16> = Vec::new();
    if nans.try_reserve_exact(nan_count).is_err() {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    let mut from = 0;
    while from < length {
        let to = (from + CHARGE_CHUNK).min(length);
        let Some(window) = rt.heap.typed_window(source)? else {
            break;
        };
        for chunk in window
            .get(from * size..to * size)
            .unwrap_or_default()
            .chunks_exact(size)
        {
            let bits = load(chunk).unwrap_or(0);
            if is_float(ty) && is_nan_bits(ty, bits) {
                if nans.len() < nan_count {
                    nans.push(bits as u16);
                }
                continue;
            }
            if let Some(count) = counts.get_mut(key_of(ty, bits) as usize) {
                *count += 1;
            }
        }
        rt.charge_bytes((to - from) * size)?;
        from = to;
    }
    let mut at = 0usize;
    for (key, &count) in counts.iter().enumerate() {
        let bits = bits_of_key(ty, key as u64);
        let mut left = usize::try_from(count).unwrap_or(usize::MAX);
        while left > 0 {
            let n = left.min(CHARGE_CHUNK);
            rt.heap.typed_fill(target, (at, at + n), bits)?;
            rt.charge_bytes(n * size)?;
            at += n;
            left -= n;
        }
    }
    write_nans(rt, target, at, nans.iter().map(|&bits| u64::from(bits)))
}

/// The number of NaNs among the first `length` elements of a float array
/// (a pass over the bytes, charged to the time countdown).
fn count_nans(rt: &mut Runtime, source: &TypedArrayData, length: usize) -> VmResult<usize> {
    let (ty, size) = (source.ty, source.ty.size());
    let mut total = 0;
    let mut from = 0;
    while from < length {
        let to = (from + CHARGE_CHUNK).min(length);
        let Some(window) = rt.heap.typed_window(source)? else {
            break;
        };
        let part = window.get(from * size..to * size).unwrap_or_default();
        total += part
            .chunks_exact(size)
            .filter(|chunk| is_nan_bits(ty, load(chunk).unwrap_or(0)))
            .count();
        rt.charge_bytes(part.len())?;
        from = to;
    }
    Ok(total)
}

/// Writes the NaNs after the sorted numbers, as they were.
fn write_nans(
    rt: &mut Runtime,
    target: &TypedArrayData,
    at: usize,
    nans: impl Iterator<Item = u64>,
) -> VmResult<()> {
    let mut at = at;
    let mut nans = nans.peekable();
    let mut block: Vec<u64> = Vec::new();
    while nans.peek().is_some() {
        block.clear();
        block.extend(nans.by_ref().take(CHARGE_CHUNK));
        rt.heap.typed_write_bits(target, at, &block)?;
        rt.charge_bytes(block.len() * target.ty.size())?;
        at += block.len();
    }
    Ok(())
}

/// Sort by keys of 32 or 64 bits (the width of the element, or 32 bits
/// for a two-byte element): the keys, sorted, written back. The vector has
/// one slot per element: the keys of the numbers fill it from the front,
/// and the bits of the NaNs (which fit a slot) fill it from the back, so
/// the NaNs cost no memory of their own; only the front is sorted.
fn merge_keys<T: Key>(
    rt: &mut Runtime,
    source: &TypedArrayData,
    target: &TypedArrayData,
    length: usize,
) -> VmResult<()> {
    let width = size_of::<T>();
    let (ty, size) = (source.ty, source.ty.size());
    reserve(rt, length.saturating_mul(2 * width))?;
    let mut keys: Vec<T> = Vec::new();
    if keys.try_reserve_exact(length).is_err() {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    keys.resize(length, T::default());
    let (mut numbers, mut nans) = (0, 0);
    let mut from = 0;
    while from < length {
        let to = (from + CHARGE_CHUNK).min(length);
        let Some(window) = rt.heap.typed_window(source)? else {
            break;
        };
        let part = window.get(from * size..to * size).unwrap_or_default();
        for chunk in part.chunks_exact(size) {
            let bits = load(chunk).unwrap_or(0);
            // The front and the back never meet: one slot per element.
            let (slot, key) = if is_float(ty) && is_nan_bits(ty, bits) {
                nans += 1;
                (length - nans, bits)
            } else {
                numbers += 1;
                (numbers - 1, key_of(ty, bits))
            };
            if let Some(place) = keys.get_mut(slot) {
                *place = T::from_key(key);
            }
        }
        rt.charge_bytes(part.len())?;
        from = to;
    }
    let (front, rest) = keys.split_at_mut(numbers.min(length));
    let back = rest
        .get(rest.len().saturating_sub(nans)..)
        .unwrap_or_default();
    sort_keys(rt, front)?;
    let mut bits: Vec<u64> = Vec::with_capacity(CHARGE_CHUNK);
    let mut at = 0;
    for block in front.chunks(CHARGE_CHUNK) {
        bits.clear();
        bits.extend(block.iter().map(|key| bits_of_key(ty, key.to_key())));
        rt.heap.typed_write_bits(target, at, &bits)?;
        rt.charge_bytes(block.len() * size)?;
        at += block.len();
    }
    // The first NaN is in the last slot.
    write_nans(rt, target, at, back.iter().rev().map(|key| key.to_key()))
}

/// Sorts the keys: runs of [`CHARGE_CHUNK`] with `sort_unstable`, then a
/// bottom-up merge (Sedgewick and Wayne, "Algorithms", 4th edition,
/// section 2.2). Equal keys are equal values, so stability needs nothing.
/// The time is charged after each run and every chunk of a merge.
fn sort_keys<T: Key>(rt: &mut Runtime, keys: &mut [T]) -> VmResult<()> {
    let n = keys.len();
    for run in keys.chunks_mut(CHARGE_CHUNK) {
        run.sort_unstable();
        rt.charge(run.len())?;
    }
    if n <= CHARGE_CHUNK {
        return Ok(());
    }
    let mut scratch: Vec<T> = Vec::new();
    if scratch.try_reserve_exact(n).is_err() {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    scratch.resize(n, T::default());
    let mut in_keys = true;
    let mut width = CHARGE_CHUNK;
    while width < n {
        {
            let (source, target): (&[T], &mut [T]) = if in_keys {
                (&*keys, scratch.as_mut_slice())
            } else {
                (scratch.as_slice(), &mut *keys)
            };
            let mut low = 0;
            while low < n {
                let middle = (low + width).min(n);
                let high = (low + 2 * width).min(n);
                if let (Some(left), Some(right), Some(out)) = (
                    source.get(low..middle),
                    source.get(middle..high),
                    target.get_mut(low..high),
                ) {
                    merge(rt, left, right, out)?;
                }
                low = high;
            }
        }
        in_keys = !in_keys;
        width *= 2;
    }
    if !in_keys {
        keys.copy_from_slice(&scratch);
    }
    Ok(())
}

/// Merges two sorted runs into `out`, charging the time as it goes.
fn merge<T: Key>(rt: &mut Runtime, left: &[T], right: &[T], out: &mut [T]) -> VmResult<()> {
    let (mut i, mut j) = (0, 0);
    for (n, slot) in out.iter_mut().enumerate() {
        if n % CHARGE_CHUNK == CHARGE_CHUNK - 1 {
            rt.charge(CHARGE_CHUNK / 2)?;
        }
        match (left.get(i), right.get(j)) {
            (Some(&l), Some(&r)) if r < l => {
                *slot = r;
                j += 1;
            }
            (Some(&l), _) => {
                *slot = l;
                i += 1;
            }
            (None, Some(&r)) => {
                *slot = r;
                j += 1;
            }
            (None, None) => break,
        }
    }
    Ok(())
}

// --- The sort with a comparator ---

/// Sorts the first `length` elements of `source` with `comparator` and
/// writes them to the start of `target`.
fn sort_with(
    rt: &mut Runtime,
    source: &TypedArrayData,
    target: &TypedArrayData,
    length: usize,
    comparator: Value,
) -> VmResult<()> {
    let count = u32::try_from(length).map_err(|_| VmError::range_error(ALLOCATION_FAILED))?;
    // The values (16 bytes each) and the two permutations.
    reserve(rt, length.saturating_mul(16 + 8))?;
    let items = rt.heap.new_array_with_capacity(None, length, &rt.vm)?;
    rt.heap.record(items);
    for k in 0..length {
        rt.tick()?;
        let value = rt.typed_element(source, k)?;
        rt.heap.record(value);
        if !rt.heap.push_element(items, value)? {
            return Err(VmError::invariant("the sort buffer refused a value"));
        }
    }
    let mut order: Vec<u32> = (0..count).collect();
    let mut scratch = order.clone();
    let mut compare = |left: u32, right: u32| -> VmResult<bool> {
        rt.with_scope(|rt| {
            let x = item(rt, items, left)?;
            let y = item(rt, items, right)?;
            let result = rt.call(comparator, Value::Undefined, &[x, y])?;
            // NaN is +0 (`CompareTypedArrayElements` step 4.b).
            Ok(rt.to_number(result)? > 0.0)
        })
    };
    merge_sort(&mut order, &mut scratch, &mut compare)?;
    for (j, &index) in order.iter().enumerate() {
        rt.tick()?;
        let value = item(rt, items, index)?;
        rt.typed_set_element(target, j as f64, value)?;
    }
    Ok(())
}

/// The top-down merge sort of `order` (using `scratch` of the same
/// length): `right_first(left, right)` says whether the right element
/// goes before the left one. Every index is checked; a comparator that
/// throws ends the sort at once.
fn merge_sort(
    order: &mut [u32],
    scratch: &mut [u32],
    right_first: &mut impl FnMut(u32, u32) -> VmResult<bool>,
) -> VmResult<()> {
    let count = order.len();
    if count < 2 {
        return Ok(());
    }
    let middle = count / 2;
    {
        let (left, right) = order.split_at_mut(middle);
        let (left_scratch, right_scratch) = scratch.split_at_mut(middle);
        merge_sort(left, left_scratch, right_first)?;
        merge_sort(right, right_scratch, right_first)?;
    }
    scratch.copy_from_slice(order);
    let (left, right) = scratch.split_at(middle);
    let (mut from_left, mut from_right) = (0, 0);
    for slot in order.iter_mut() {
        let take_right = match (left.get(from_left), right.get(from_right)) {
            (Some(&first), Some(&second)) => right_first(first, second)?,
            (Some(_), None) => false,
            _ => true,
        };
        let taken = if take_right {
            from_right += 1;
            right.get(from_right - 1)
        } else {
            from_left += 1;
            left.get(from_left - 1)
        };
        *slot = taken.copied().unwrap_or_default();
    }
    Ok(())
}

/// `%TypedArray%.prototype.sort(comparator)` (§23.2.3.29).
pub(super) fn sort(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let comparator = comparator_arg(rt, call)?;
    let (array, data, length) = validate_this(rt, call, "sort")?;
    sorted_into(rt, &data, &data, length, comparator)?;
    Ok(NativeReturn::Value(array.into()))
}

/// `%TypedArray%.prototype.toSorted(comparator)` (§23.2.3.34).
pub(super) fn to_sorted(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let comparator = comparator_arg(rt, call)?;
    let (_, data, length) = validate_this(rt, call, "toSorted")?;
    let (result, target) = rt.typed_create_same_type(&data, length)?;
    sorted_into(rt, &data, &target, length, comparator)?;
    Ok(NativeReturn::Value(result.into()))
}

/// The sort of both methods.
fn sorted_into(
    rt: &mut Runtime,
    source: &TypedArrayData,
    target: &TypedArrayData,
    length: usize,
    comparator: Option<Value>,
) -> VmResult<()> {
    if length < 2 {
        if length == 1 && source.view.buffer != target.view.buffer {
            super::typed_array_change::copy_prefix(rt, target, source, 1)?;
        }
        return Ok(());
    }
    match comparator {
        Some(comparator) => sort_with(rt, source, target, length, comparator),
        None => sort_numeric(rt, source, target, length),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPES: [ElementType; 10] = [
        ElementType::Int8,
        ElementType::Uint8,
        ElementType::Int16,
        ElementType::Uint16,
        ElementType::Int32,
        ElementType::Uint32,
        ElementType::BigInt64,
        ElementType::BigUint64,
        ElementType::Float32,
        ElementType::Float64,
    ];

    /// The keys of integers are ordered like the integers.
    #[test]
    fn integer_keys_follow_the_numeric_order() {
        let signed = |ty: ElementType, values: &[i64]| {
            let size = ty.size();
            let mut keys: Vec<u64> = values
                .iter()
                .map(|&value| key_of(ty, value as u64 & mask(size)))
                .collect();
            let sorted = keys.clone();
            keys.sort_unstable();
            assert_eq!(keys, sorted, "{ty:?}");
        };
        signed(ElementType::Int8, &[-128, -1, 0, 1, 127]);
        signed(ElementType::Int16, &[-32768, -1, 0, 1, 32767]);
        signed(
            ElementType::Int32,
            &[i64::from(i32::MIN), -1, 0, 1, i64::from(i32::MAX)],
        );
        signed(ElementType::BigInt64, &[i64::MIN, -1, 0, 1, i64::MAX]);
        signed(ElementType::Uint32, &[0, 1, 2_147_483_648, 4_294_967_295]);
    }

    #[test]
    fn keys_round_trip() {
        for ty in TYPES {
            let size = ty.size();
            for bits in [
                0u64,
                1,
                0x7f,
                0x80,
                0xff,
                0x1234,
                0x8000,
                0xffff,
                0x8000_0000,
                0xffff_ffff,
                1 << 63,
                u64::MAX,
            ] {
                let bits = bits & mask(size);
                if is_float(ty) && is_nan_bits(ty, bits) {
                    continue;
                }
                assert_eq!(bits_of_key(ty, key_of(ty, bits)), bits, "{ty:?} {bits:#x}");
            }
        }
    }

    /// Floats: -0 sorts before +0, the infinities at the ends.
    #[test]
    fn float_keys_follow_the_numeric_order() {
        let ordered = [
            f64::NEG_INFINITY,
            -1.5,
            -f64::MIN_POSITIVE,
            -0.0,
            0.0,
            f64::MIN_POSITIVE,
            1.5,
            f64::INFINITY,
        ];
        let keys: Vec<u64> = ordered
            .iter()
            .map(|value| key_of(ElementType::Float64, value.to_bits()))
            .collect();
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]), "{keys:?}");
        let singles = [
            f32::NEG_INFINITY,
            -1.5,
            -1e-30,
            -0.0,
            0.0,
            1e-30,
            1.5,
            f32::INFINITY,
        ];
        let keys: Vec<u64> = singles
            .iter()
            .map(|value| key_of(ElementType::Float32, u64::from(value.to_bits())))
            .collect();
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]), "{keys:?}");
        assert!(is_nan_bits(ElementType::Float16, 0x7e00));
        assert!(is_nan_bits(ElementType::Float16, 0xfe01));
        assert!(!is_nan_bits(ElementType::Float16, 0x7c00));
    }

    /// The merge sort splits in the middle, takes the left element unless
    /// the answer is above zero, and calls `right_first(left, right)`: the
    /// order of the calls that Chromium 148 makes (`[3, 1, 2]` with
    /// `a - b`: (1, 2), (3, 1), (3, 2)).
    #[test]
    fn merge_sort_calls_like_v8() {
        let values = [3i32, 1, 2];
        let mut order = [0u32, 1, 2];
        let mut scratch = order;
        let mut calls = Vec::new();
        merge_sort(&mut order, &mut scratch, &mut |left,
                                                   right|
         -> VmResult<bool> {
            let (a, b) = (values[left as usize], values[right as usize]);
            calls.push((a, b));
            Ok(a - b > 0)
        })
        .expect("sorts");
        assert_eq!(calls, [(1, 2), (3, 1), (3, 2)]);
        assert_eq!(order, [1, 2, 0]);
    }

    /// Whatever the answers, the result is a permutation; with a constant
    /// answer the sort keeps the order (never right first) or reverses it.
    #[test]
    fn merge_sort_always_makes_a_permutation() {
        for n in 0..40u32 {
            for answer in [false, true] {
                let mut order: Vec<u32> = (0..n).collect();
                let mut scratch = order.clone();
                merge_sort(&mut order, &mut scratch, &mut |_, _| -> VmResult<bool> {
                    Ok(answer)
                })
                .expect("sorts");
                let expected: Vec<u32> = if answer {
                    (0..n).rev().collect()
                } else {
                    (0..n).collect()
                };
                assert_eq!(order, expected, "{n} {answer}");
            }
            let mut order: Vec<u32> = (0..n).collect();
            let mut scratch = order.clone();
            let mut flip = false;
            merge_sort(&mut order, &mut scratch, &mut |_, _| -> VmResult<bool> {
                flip = !flip;
                Ok(flip)
            })
            .expect("sorts");
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..n).collect::<Vec<_>>());
        }
    }
}
