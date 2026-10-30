//! The `Array.prototype` methods that make a new array from the receiver
//! (ECMA-262 §23.1.3, <https://tc39.es/ecma262/#sec-properties-of-the-array-prototype-object>):
//! `concat`, `slice`, `flat`, `flatMap`, `toReversed`, `toSpliced` and
//! `with` (`toSorted` is in `array_sort.rs`).
//!
//! The results are built with the dense builder (`object/element_access.rs`)
//! where `ArraySpeciesCreate` gives a plain array, or the specification
//! creates one with `ArrayCreate`; the copy from an array with dense
//! elements reads its vector directly. The methods that read every index
//! (`toReversed`, `toSpliced`, `with`) make an element for each hole, so
//! their result needs memory in proportion to the length; the heap limit
//! ends a copy of a huge sparse array (V8 loops until it runs out of
//! memory or time).
//!
//! `flat` and `flatMap` (`FlattenIntoArray`, §23.1.3.13.1) keep the open
//! levels in a vector, not on the Rust stack; like V8, which recurses,
//! `flat` ends deep nesting with "Maximum call stack size exceeded".

use crate::heap::{Gc, HandleScope};
use crate::object::{GetResult, Object};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::{MAX_SAFE_INTEGER, checked_array_length, to_integer_or_infinity};
use crate::vm::{NativeCall, NativeReturn, STACK_OVERFLOW, VmError, VmResult, WellKnown};

use super::array::{
    INITIAL_CAPACITY, callback_arg, check_safe_length, length_overflow, receiver, relative_index,
};
use super::array_access::{Flow, Item};

/// The elements of the results that `concat` makes in one step
/// (`concat_plain`); longer ones take the steps of the specification.
const CONCAT_PLAIN_MAX: usize = 4096;

/// The elements that `slice` copies through a buffer on the stack.
const SLICE_BUFFER: usize = 16;

/// The values that one round of a copy appends (a fixed buffer).
const COPY_CHUNK: usize = 256;

/// The most open levels of `flat` (V8's recursion ends between 8,000 and
/// 10,000 levels, measured in Chromium 148).
const FLAT_DEPTH_LIMIT: usize = 9_000;

/// `IsConcatSpreadable(O)` (§23.1.3.1.1).
fn is_concat_spreadable(rt: &mut Runtime, value: Value) -> VmResult<bool> {
    let Value::Object(object) = value else {
        return Ok(false);
    };
    // A plain array (`vm/plain_array.rs`) has no `@@isConcatSpreadable`
    // on its chain.
    if rt.plain_array(object) {
        return Ok(true);
    }
    let key = rt.symbol_key(WellKnown::IsConcatSpreadable);
    let spreadable = match rt.heap.get(object, key, value)? {
        // A data property or none, read by the heap's walk without a
        // handle record (`ToBoolean` cannot collect).
        GetResult::Value(found) => found,
        _ => rt.get_value(value, key)?,
    };
    if !spreadable.is_undefined() {
        return rt.to_boolean(spreadable);
    }
    rt.is_array(value)
}

/// `Array.prototype.concat(...items)` (§23.1.3.1).
pub(super) fn concat(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object_checked(call, "Array.prototype.concat")?;
    if let Some(result) = concat_plain(rt, object, call)? {
        return Ok(NativeReturn::Value(result.into()));
    }
    let result = rt.array_species_create(object, 0.0)?;
    let mut n = 0.0;
    for index in 0..=call.argc() {
        let item = if index == 0 {
            object.into()
        } else {
            rt.arg(call, index - 1)
        };
        rt.with_scope(|rt| concat_item(rt, result, item, &mut n))?;
    }
    rt.set_length(result, n)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `concat` when the receiver and every argument are plain arrays
/// (`vm/plain_array.rs`: `ArraySpeciesCreate` is `ArrayCreate` and each
/// item is spread) with dense elements and no elements on their
/// prototypes (a hole stays a hole: `HasProperty` is false for it), with
/// fewer than [`CONCAT_PLAIN_MAX`] elements in all: every step reads
/// without effects, so the result is made in one step from the vectors.
/// `None` for every other case (nothing done).
fn concat_plain(
    rt: &mut Runtime,
    object: Gc<Object>,
    call: &NativeCall,
) -> VmResult<Option<Gc<Object>>> {
    let mut values = Vec::new();
    for index in 0..=call.argc() {
        let item = if index == 0 {
            object
        } else {
            match rt.arg(call, index - 1) {
                Value::Object(item) => item,
                _ => return Ok(None),
            }
        };
        if !rt.plain_array(item) {
            return Ok(None);
        }
        let Some(state) = rt.heap.dense_array(item)? else {
            return Ok(None);
        };
        let length = state.length as usize;
        if values.len() + length >= CONCAT_PLAIN_MAX {
            return Ok(None);
        }
        let stored = rt
            .heap
            .object(item)?
            .elements
            .dense()
            .ok_or(VmError::invariant("a dense array without a dense vector"))?;
        values.extend_from_slice(stored);
        // The holes after the last stored element.
        values.resize(
            values.len() + length.saturating_sub(stored.len()),
            Value::Empty,
        );
    }
    rt.charge(values.len() + call.argc())?;
    // The values are elements of the items, which the call roots.
    Ok(Some(rt.array_from_values(&values)?))
}

/// One item of `concat` (steps 5.b to 5.c), appended at index `n`.
fn concat_item(rt: &mut Runtime, result: Gc<Object>, item: Value, n: &mut f64) -> VmResult<()> {
    rt.tick()?;
    if !is_concat_spreadable(rt, item)? {
        if *n >= MAX_SAFE_INTEGER {
            return Err(length_overflow());
        }
        rt.create_index(result, *n, item)?;
        *n += 1.0;
        return Ok(());
    }
    let Value::Object(source) = item else {
        return Err(VmError::invariant("a spreadable primitive"));
    };
    let length = rt.length_of_array_like(source)?;
    if *n + length > MAX_SAFE_INTEGER {
        return Err(length_overflow());
    }
    if let Some(state) = rt.heap.dense_array(source)?
        && *n + length < f64::from(u32::MAX)
        && rt.heap.copy_dense(result, *n as u32, source, &rt.vm)?
    {
        rt.charge(state.stored_len)?;
        *n += length;
        return Ok(());
    }
    let start = *n;
    rt.for_present(source, 0.0, length, |rt, k, value| {
        rt.create_index(result, start + k, value)?;
        Ok(Flow::Continue)
    })?;
    *n += length;
    Ok(())
}

/// Appends `Get(source, k)` for `k` in `from..to` (descending with
/// `descending`) to the array `target`, holes as `undefined`: the copy
/// loops of `toReversed`, `toSpliced` and `with`. `target` was made by the
/// builder; a form the builder refuses takes `CreateDataPropertyOrThrow`.
fn copy_range(
    rt: &mut Runtime,
    target: Gc<Object>,
    source: Gc<Object>,
    (from, to): (f64, f64),
    descending: bool,
) -> VmResult<()> {
    if from >= to {
        return Ok(());
    }
    if rt.heap.dense_array(source)?.is_some() {
        // No index can run script code: copy in chunks from the vector.
        let (from, to) = (from as usize, to as usize);
        let mut buffer = [Value::Undefined; COPY_CHUNK];
        let mut done = 0;
        while done < to - from {
            let chunk = (to - from - done).min(COPY_CHUNK);
            {
                let values = rt
                    .heap
                    .object(source)?
                    .elements
                    .dense()
                    .ok_or(VmError::invariant("a dense copy of sparse elements"))?;
                for (i, slot) in buffer.iter_mut().take(chunk).enumerate() {
                    let k = if descending {
                        to - 1 - done - i
                    } else {
                        from + done + i
                    };
                    *slot = match values.get(k) {
                        Some(value) if !value.is_empty() => *value,
                        _ => Value::Undefined,
                    };
                }
            }
            let part = buffer
                .get(..chunk)
                .ok_or(VmError::invariant("a copy chunk outside the buffer"))?;
            append_values(rt, target, part)?;
            rt.charge(chunk)?;
            done += chunk;
        }
        return Ok(());
    }
    rt.for_index(source, from, to, descending, |rt, item| {
        match item {
            Item::Value(_, value) => append_values(rt, target, &[value])?,
            Item::Gap(low, high) => {
                let mut left = (high - low) as u64;
                let undefined = [Value::Undefined; COPY_CHUNK];
                while left > 0 {
                    let chunk = left.min(COPY_CHUNK as u64) as usize;
                    let part = undefined
                        .get(..chunk)
                        .ok_or(VmError::invariant("a copy chunk outside the buffer"))?;
                    append_values(rt, target, part)?;
                    rt.charge(chunk)?;
                    left -= chunk as u64;
                }
            }
        }
        Ok(Flow::Continue)
    })
}

/// Appends values to a new array of the builder, or with
/// `CreateDataPropertyOrThrow` where the builder refuses.
fn append_values(rt: &mut Runtime, target: Gc<Object>, values: &[Value]) -> VmResult<()> {
    if rt.heap.extend_elements(target, values)? {
        return Ok(());
    }
    for &value in values {
        rt.push_value(target, value)?;
    }
    Ok(())
}

/// `ArrayCreate(length)` for the copying methods: the check of the length
/// and a new array of the builder with room for the first elements.
fn new_copy(rt: &mut Runtime, length: f64) -> VmResult<Gc<Object>> {
    checked_array_length(length)?;
    rt.new_array_with_capacity((length as usize).min(INITIAL_CAPACITY))
}

/// `Array.prototype.slice(start, end)` (§23.1.3.28).
pub(super) fn slice(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let start = relative_index(rt, rt.arg(call, 0), length, None)?;
    let end = relative_index(rt, rt.arg(call, 1), length, Some(length))?;
    let count = (end - start).max(0.0);
    if rt.default_species(object)?
        && let Some(state) = rt.heap.dense_array(object)?
        && f64::from(state.length) == length
    {
        // The new array gets the elements and the holes of the range (a
        // short range through a buffer on the stack).
        let stored = state.stored_len as f64;
        let mut small = [Value::Undefined; SLICE_BUFFER];
        let large: Vec<Value>;
        let values: &[Value] = match rt.heap.object(object)?.elements.dense() {
            Some(values) if start < stored => {
                let range = values
                    .get(start as usize..end.min(stored) as usize)
                    .unwrap_or_default();
                if let Some(buffer) = small.get_mut(..range.len()) {
                    buffer.copy_from_slice(range);
                    buffer
                } else {
                    large = range.to_vec();
                    &large
                }
            }
            _ => &[],
        };
        rt.charge(values.len())?;
        let result = rt.array_from_values(values)?;
        rt.heap.set_array_length_field(result, count as u32, true)?;
        return Ok(NativeReturn::Value(result.into()));
    }
    let result = rt.array_species_create(object, count)?;
    rt.for_present(object, start, end, |rt, k, value| {
        rt.create_index(result, k - start, value)?;
        Ok(Flow::Continue)
    })?;
    rt.set_length(result, count)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Array.prototype.toReversed()` (§23.1.3.33).
pub(super) fn to_reversed(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let result = new_copy(rt, length)?;
    copy_range(rt, result, object, (0.0, length), true)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Array.prototype.with(index, value)` (§23.1.3.39).
pub(super) fn with(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let relative = to_integer_or_infinity(rt.to_number(rt.arg(call, 0))?);
    let index = if relative >= 0.0 {
        relative
    } else {
        length + relative
    };
    if index >= length || index < 0.0 {
        return Err(VmError::range_error(format!(
            "Invalid index : {}",
            crate::vm::number::number_to_string(relative)
        )));
    }
    let value = rt.arg(call, 1);
    let result = new_copy(rt, length)?;
    copy_range(rt, result, object, (0.0, index), false)?;
    append_values(rt, result, &[value])?;
    copy_range(rt, result, object, (index + 1.0, length), false)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Array.prototype.toSpliced(start, skipCount, ...items)` (§23.1.3.34).
pub(super) fn to_spliced(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let (start, skip) = super::array_mutate::splice_range(rt, call, length)?;
    let item_count = call.argc().saturating_sub(2);
    let new_length = length + item_count as f64 - skip;
    check_safe_length(new_length)?;
    let result = new_copy(rt, new_length)?;
    copy_range(rt, result, object, (0.0, start), false)?;
    let items: Vec<Value> = (0..item_count).map(|i| rt.arg(call, i + 2)).collect();
    rt.charge(items.len())?;
    append_values(rt, result, &items)?;
    copy_range(rt, result, object, (start + skip, length), false)?;
    Ok(NativeReturn::Value(result.into()))
}

/// An open level of `FlattenIntoArray`: a source array-like and the next
/// index to read, with the handle scope that keeps the source alive.
struct Level {
    source: Gc<Object>,
    length: f64,
    next: f64,
    depth: f64,
    scope: Option<HandleScope>,
}

/// `FlattenIntoArray(target, source, sourceLen, 0, depth, mapper,
/// thisArg)` (§23.1.3.13.1) as a loop over a vector of open levels. The
/// mapper applies to the elements of the first level only.
fn flatten_into_array(
    rt: &mut Runtime,
    target: Gc<Object>,
    source: Gc<Object>,
    length: f64,
    depth: f64,
    mapper: Option<(Value, Value)>,
) -> VmResult<()> {
    let mut levels = vec![Level {
        source,
        length,
        next: 0.0,
        depth,
        scope: None,
    }];
    let mut target_index = 0.0;
    while let Some(level) = levels.last_mut() {
        rt.tick()?;
        let (source, length, next, depth, top) = (
            level.source,
            level.length,
            level.next,
            level.depth,
            levels.len() == 1,
        );
        let step = rt.with_scope(|rt| {
            let mut found = None;
            rt.for_present(source, next, length, |_, k, value| {
                found = Some((k, value));
                Ok(Flow::Break)
            })?;
            let Some((k, mut element)) = found else {
                return Ok(None);
            };
            // The loop's scope has closed: keep the element in this one.
            rt.heap.record(element);
            if let (true, Some((mapper, this_arg))) = (top, mapper) {
                element = rt.call(
                    mapper,
                    this_arg,
                    &[element, Value::number(k), source.into()],
                )?;
            }
            // `IsArray` sees through proxies (a revoked one throws).
            let nested = match element {
                Value::Object(inner) if depth > 0.0 && rt.is_array(element)? => Some(inner),
                _ => None,
            };
            if nested.is_none() {
                if target_index >= MAX_SAFE_INTEGER {
                    return Err(length_overflow());
                }
                if !rt.heap.push_element(target, element)? {
                    rt.create_index(target, target_index, element)?;
                }
                target_index += 1.0;
            }
            Ok(Some((k, nested)))
        })?;
        let Some((k, nested)) = step else {
            if let Some(level) = levels.pop()
                && let Some(scope) = level.scope
            {
                rt.heap.close_scope(scope)?;
            }
            continue;
        };
        if let Some(level) = levels.last_mut() {
            level.next = k + 1.0;
        }
        if let Some(inner) = nested {
            if levels.len() >= FLAT_DEPTH_LIMIT {
                return Err(VmError::range_error(STACK_OVERFLOW));
            }
            // Nothing collects between the step's scope and this one.
            let scope = rt.heap.open_scope();
            rt.heap.record(inner);
            let inner_length = rt.length_of_array_like(inner)?;
            levels.push(Level {
                source: inner,
                length: inner_length,
                next: 0.0,
                depth: if depth == f64::INFINITY {
                    depth
                } else {
                    depth - 1.0
                },
                scope: Some(scope),
            });
        }
    }
    Ok(())
}

/// `Array.prototype.flat(depth)` (§23.1.3.13).
pub(super) fn flat(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let depth = match rt.arg(call, 0) {
        Value::Undefined => 1.0,
        value => to_integer_or_infinity(rt.to_number(value)?).max(0.0),
    };
    let result = rt.array_species_create(object, 0.0)?;
    flatten_into_array(rt, result, object, length, depth, None)?;
    Ok(NativeReturn::Value(result.into()))
}

/// `Array.prototype.flatMap(mapper, thisArg)` (§23.1.3.14).
pub(super) fn flat_map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, None)?;
    let mapper = callback_arg(rt, call, 0)?;
    let this_arg = rt.arg(call, 1);
    let result = rt.array_species_create(object, 0.0)?;
    flatten_into_array(rt, result, object, length, 1.0, Some((mapper, this_arg)))?;
    Ok(NativeReturn::Value(result.into()))
}
