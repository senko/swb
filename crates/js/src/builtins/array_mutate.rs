//! The `Array.prototype` methods that change the receiver (ECMA-262
//! §23.1.3, <https://tc39.es/ecma262/#sec-properties-of-the-array-prototype-object>):
//! `push`, `pop`, `shift`, `unshift`, `splice`, `reverse`, `fill` and
//! `copyWithin`.
//!
//! Each method takes the specification's steps through `array_access.rs`,
//! which visits only the indices where a step can change something while
//! the chain has ordinary elements (so a sparse array of length 2^32 − 1
//! costs its stored elements, not its length). An array with dense
//! elements whose steps cannot fail or run script code (see
//! [`DenseArray`]) is changed in its element vector at once; the moved
//! elements are charged to the time countdown. Mapped arguments objects
//! take the steps, whose `Set` and `DeletePropertyOrThrow` write through
//! to the parameters (§10.4.4).

use crate::heap::Gc;
use crate::object::{DenseArray, Object};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::{MAX_SAFE_INTEGER, to_integer_or_infinity};
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::array::{check_safe_length, receiver, relative_index};
use super::array_access::Next;

/// The elements that a move in a dense vector copies per time step.
const MOVES_PER_STEP: usize = 8;

/// The state of `object` if it is a dense array that a method may change
/// in place: [`DenseArray::writable`], and with `hole_free` no holes.
fn writable_dense(
    rt: &Runtime,
    object: Gc<Object>,
    hole_free: bool,
) -> VmResult<Option<DenseArray>> {
    Ok(rt
        .heap
        .dense_array(object)?
        .filter(|state| state.writable() && (!hole_free || state.hole_free())))
}

/// `Array.prototype.push(...items)` (§23.1.3.23).
pub(super) fn push(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let count = call.argc();
    if length + count as f64 > MAX_SAFE_INTEGER {
        return Err(VmError::type_error(format!(
            "Pushing {count} elements on an array-like of length {} is disallowed, as the total surpasses 2**53-1",
            crate::vm::number::number_to_string(length)
        )));
    }
    // While the dense builder appends, `length` follows each element;
    // the final `Set` of the same value to a writable `length` then
    // changes nothing and is left out.
    let mut appended = matches!(rt.heap.array_length(object), Ok((_, true)));
    for i in 0..count {
        rt.tick()?;
        let value = rt.arg(call, i);
        // `Set` is `CreateDataProperty` when no prototype has elements
        // (no setter on the way): then the dense builder appends.
        if appended
            && rt.heap.dense_array(object)?.is_some()
            && rt.heap.push_element(object, value)?
        {
            continue;
        }
        appended = false;
        rt.set_index(object, length + i as f64, value)?;
    }
    let new_length = length + count as f64;
    if !appended {
        rt.set_length(object, new_length)?;
    }
    Ok(NativeReturn::Value(Value::number(new_length)))
}

/// `Array.prototype.pop()` (§23.1.3.22).
pub(super) fn pop(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    if length == 0.0 {
        rt.set_length(object, 0.0)?;
        return Ok(NativeReturn::Value(Value::Undefined));
    }
    if let Some(state) = rt.heap.dense_array(object)?
        && state.length_writable
    {
        let element = rt.heap.pop_dense(object)?;
        return Ok(NativeReturn::Value(element.unwrap_or(Value::Undefined)));
    }
    let index = length - 1.0;
    let element = rt.get_index(object, index)?;
    rt.delete_index(object, index)?;
    rt.set_length(object, index)?;
    Ok(NativeReturn::Value(element))
}

/// `Array.prototype.shift()` (§23.1.3.27).
pub(super) fn shift(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    if length == 0.0 {
        rt.set_length(object, 0.0)?;
        return Ok(NativeReturn::Value(Value::Undefined));
    }
    if writable_dense(rt, object, false)?.is_some() {
        // The start offset of the dense vector: no element moves.
        let first = rt.heap.shift_dense(object)?;
        rt.charge(1)?;
        return Ok(NativeReturn::Value(first));
    }
    let first = rt.get_index(object, 0.0)?;
    rt.move_elements(object, 1.0, 0.0, length - 1.0, false)?;
    rt.delete_index(object, length - 1.0)?;
    rt.set_length(object, length - 1.0)?;
    Ok(NativeReturn::Value(first))
}

/// `Array.prototype.unshift(...items)` (§23.1.3.37).
pub(super) fn unshift(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let count = call.argc();
    let new_length = length + count as f64;
    if count > 0 {
        check_safe_length(new_length)?;
        if let Some(state) = writable_dense(rt, object, false)?
            && new_length <= f64::from(u32::MAX)
        {
            let items: Vec<Value> = (0..count).map(|i| rt.arg(call, i)).collect();
            let bytes = rt.heap.dense_growth(object, state.stored_len + count)?;
            rt.heap.reserve_with(bytes, &rt.vm, &[object.into()])?;
            rt.heap.change_dense(object, |values, length| {
                values.try_reserve(count)?;
                values.splice(0..0, items);
                *length += count as u32;
                Ok(())
            })?;
            rt.charge((state.stored_len + count) / MOVES_PER_STEP)?;
            return Ok(NativeReturn::Value(Value::number(new_length)));
        }
        rt.move_elements(object, 0.0, count as f64, length, true)?;
        for j in 0..count {
            rt.tick()?;
            let value = rt.arg(call, j);
            rt.set_index(object, j as f64, value)?;
        }
    }
    rt.set_length(object, new_length)?;
    Ok(NativeReturn::Value(Value::number(new_length)))
}

/// The start and the delete count of `splice` and `toSpliced`
/// (§23.1.3.31 steps 3 to 8): `(actualStart, actualDeleteCount)`.
pub(super) fn splice_range(
    rt: &mut Runtime,
    call: &NativeCall,
    length: f64,
) -> VmResult<(f64, f64)> {
    let start = relative_index(rt, rt.arg(call, 0), length, None)?;
    let delete_count = match call.argc() {
        0 => 0.0,
        1 => length - start,
        _ => {
            let count = to_integer_or_infinity(rt.to_number(rt.arg(call, 1))?);
            count.clamp(0.0, length - start)
        }
    };
    Ok((start, delete_count))
}

/// `Array.prototype.splice(start, deleteCount, ...items)` (§23.1.3.31).
pub(super) fn splice(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let (start, delete_count) = splice_range(rt, call, length)?;
    let item_count = call.argc().saturating_sub(2);
    let new_length = length + item_count as f64 - delete_count;
    check_safe_length(new_length)?;
    // The conversions of the arguments ran after `length` was read: the
    // shortcut needs the length it was computed for.
    if rt.default_species(object)?
        && let Some(state) = writable_dense(rt, object, true)?
        && f64::from(state.length) == length
        && new_length <= f64::from(u32::MAX)
    {
        let items: Vec<Value> = (0..item_count).map(|i| rt.arg(call, i + 2)).collect();
        return splice_dense(
            rt,
            object,
            state,
            (start as usize, delete_count as usize),
            items,
        );
    }
    let removed = rt.array_species_create(object, delete_count)?;
    rt.for_present(object, start, start + delete_count, |rt, from, value| {
        rt.create_index(removed, from - start, value)?;
        Ok(super::array_access::Flow::Continue)
    })?;
    rt.set_length(removed, delete_count)?;
    let items = item_count as f64;
    let tail = length - delete_count - start;
    if items < delete_count {
        rt.move_elements(object, start + delete_count, start + items, tail, false)?;
        rt.delete_range(object, length - delete_count + items, length, false)?;
    } else if items > delete_count {
        rt.move_elements(object, start + delete_count, start + items, tail, true)?;
    }
    for i in 0..item_count {
        rt.tick()?;
        let value = rt.arg(call, i + 2);
        rt.set_index(object, start + i as f64, value)?;
    }
    rt.set_length(object, new_length)?;
    Ok(NativeReturn::Value(removed.into()))
}

/// `splice` on an array without holes whose steps cannot fail: the
/// removed elements go into a new array, the items take their place.
fn splice_dense(
    rt: &mut Runtime,
    object: Gc<Object>,
    state: DenseArray,
    (start, delete_count): (usize, usize),
    items: Vec<Value>,
) -> VmResult<NativeReturn> {
    let new_len = state.stored_len - delete_count + items.len();
    let bytes = rt.heap.dense_growth(object, new_len)?;
    rt.heap.reserve_with(bytes, &rt.vm, &[object.into()])?;
    let removed: Vec<Value> = rt.heap.change_dense(object, |values, length| {
        if start + delete_count > values.len() {
            return Err(crate::error::Error::invariant(
                "a dense splice outside the vector",
            ));
        }
        values.try_reserve(items.len())?;
        let removed = values.splice(start..start + delete_count, items).collect();
        *length = new_len as u32;
        Ok(removed)
    })?;
    // The removed values are not reachable from the heap until the new
    // array holds them: no collection before (the reservation of the new
    // array roots them).
    rt.charge((state.stored_len - start + new_len - start) / MOVES_PER_STEP)?;
    let result = rt.array_from_values(&removed)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Array.prototype.reverse()` (§23.1.3.26).
pub(super) fn reverse(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    if let Some(state) = rt.heap.dense_array(object)?
        && state.stored_len == state.length as usize
        && (state.extensible || !state.holes)
    {
        // Each pair swaps its slots, holes included: with an extensible
        // array a hole and an element trade places as the steps do.
        rt.heap.change_dense(object, |values, _| {
            values.reverse();
            Ok(())
        })?;
        rt.charge(state.stored_len / MOVES_PER_STEP)?;
        return Ok(NativeReturn::Value(object.into()));
    }
    let middle = (length / 2.0).floor();
    let mut lower = 0.0;
    while lower < middle {
        rt.tick()?;
        let Some(at) = next_reverse_pair(rt, object, length, lower, middle)? else {
            break;
        };
        rt.with_scope(|rt| reverse_pair(rt, object, at, length - at - 1.0))?;
        lower = at + 1.0;
    }
    Ok(NativeReturn::Value(object.into()))
}

/// The next `lower` from `lower` on whose pair `(lower, length - lower -
/// 1)` has an element (a pair without one is left as it is).
fn next_reverse_pair(
    rt: &mut Runtime,
    object: Gc<Object>,
    length: f64,
    lower: f64,
    middle: f64,
) -> VmResult<Option<f64>> {
    let low = match rt.next_element(object, lower, middle)? {
        Next::Generic => return Ok(Some(lower)),
        Next::At(k, _) => Some(k),
        Next::End => None,
    };
    let high = match rt.prev_element(object, length - lower - 1.0, length - middle)? {
        Next::Generic => return Ok(Some(lower)),
        Next::At(k, _) => Some(length - k - 1.0),
        Next::End => None,
    };
    Ok(match (low, high) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    })
}

/// Steps 7.c to 7.l of `reverse` for one pair.
fn reverse_pair(rt: &mut Runtime, object: Gc<Object>, lower: f64, upper: f64) -> VmResult<()> {
    let lower_exists = rt.has_index(object, lower)?;
    let lower_value = if lower_exists {
        rt.get_index(object, lower)?
    } else {
        Value::Undefined
    };
    let upper_exists = rt.has_index(object, upper)?;
    let upper_value = if upper_exists {
        rt.get_index(object, upper)?
    } else {
        Value::Undefined
    };
    match (lower_exists, upper_exists) {
        (true, true) => {
            rt.set_index(object, lower, upper_value)?;
            rt.set_index(object, upper, lower_value)
        }
        (false, true) => {
            rt.set_index(object, lower, upper_value)?;
            rt.delete_index(object, upper)
        }
        (true, false) => {
            rt.delete_index(object, lower)?;
            rt.set_index(object, upper, lower_value)
        }
        (false, false) => Ok(()),
    }
}

/// `Array.prototype.fill(value, start, end)` (§23.1.3.7).
pub(super) fn fill(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let value = rt.arg(call, 0);
    let start = relative_index(rt, rt.arg(call, 1), length, Some(0.0))?;
    let end = relative_index(rt, rt.arg(call, 2), length, Some(length))?;
    if start >= end {
        return Ok(NativeReturn::Value(object.into()));
    }
    if let Some(state) = rt.heap.dense_array(object)?
        && f64::from(state.length) == length
        && start as usize <= state.stored_len
        && (state.extensible || (!state.holes && end as usize <= state.stored_len))
    {
        // No new hole: the vector stays dense.
        let (from, to) = (start as usize, end as usize);
        let bytes = rt.heap.dense_growth(object, to)?;
        rt.heap
            .reserve_with(bytes, &rt.vm, &[object.into(), value])?;
        rt.heap.change_dense(object, |values, _| {
            if to > values.len() {
                values.try_reserve(to - values.len())?;
                values.resize(to, value);
            }
            if let Some(range) = values.get_mut(from..to) {
                range.fill(value);
            }
            Ok(())
        })?;
        rt.charge((to - from) / MOVES_PER_STEP)?;
        return Ok(NativeReturn::Value(object.into()));
    }
    let mut k = start;
    while k < end {
        rt.tick()?;
        rt.with_scope(|rt| rt.set_index(object, k, value))?;
        k += 1.0;
    }
    Ok(NativeReturn::Value(object.into()))
}

/// `Array.prototype.copyWithin(target, start, end)` (§23.1.3.4).
pub(super) fn copy_within(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let to = relative_index(rt, rt.arg(call, 0), length, None)?;
    let from = relative_index(rt, rt.arg(call, 1), length, None)?;
    let end = relative_index(rt, rt.arg(call, 2), length, Some(length))?;
    let count = (end - from).min(length - to);
    if count <= 0.0 {
        return Ok(NativeReturn::Value(object.into()));
    }
    if rt
        .heap
        .dense_array(object)?
        .is_some_and(|state| state.hole_free() && f64::from(state.length) == length)
    {
        let (from, to, count) = (from as usize, to as usize, count as usize);
        rt.heap.change_dense(object, |values, _| {
            if from.max(to) + count > values.len() {
                return Err(crate::error::Error::invariant(
                    "a dense copy outside the vector",
                ));
            }
            values.copy_within(from..from + count, to);
            Ok(())
        })?;
        rt.charge(count / MOVES_PER_STEP)?;
        return Ok(NativeReturn::Value(object.into()));
    }
    let descending = from < to && to < from + count;
    rt.move_elements(object, from, to, count, descending)?;
    Ok(NativeReturn::Value(object.into()))
}
