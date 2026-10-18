//! The methods of `%TypedArray%.prototype` that call a function for the
//! elements (ECMA-262 §23.2.3,
//! <https://tc39.es/ecma262/#sec-properties-of-the-%typedarrayprototype%-object>):
//! `forEach`, `every`, `some`, `map`, `filter`, `reduce`, `reduceRight` and
//! the `find` family.
//!
//! Each reads the length once, before the callback check, and visits every
//! index below it: the element is `Get(O, k)`, which is `undefined` for an
//! index that a callback made invalid (detach, shrink), and the loop does
//! not stop there. `map` and `filter` make their result through
//! `TypedArraySpeciesCreate`; `filter` keeps the selected elements in a
//! compact list of element bytes (the source array already holds as many
//! bytes) until the species constructor ran. Every step ticks the time
//! countdown and runs in a handle scope of its own, so a `BigInt` element
//! and the callback's results stay alive for one step only.
//!
//! V8's messages (measured in Chromium 148): a callback that is not
//! callable is "number 5 is not a function" (the type and the value, as
//! for `Array.prototype`) except for `map`, which says "5 is not a
//! function".

use crate::heap::Gc;
use crate::object::{Object, TypedArrayData};
use crate::runtime::Runtime;
use crate::value::{Value, ValueCell};
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::typed_array_create::{CreateArgs, element_of_raw};
use super::typed_array_method::{position, validate_this};

/// The state of a method that calls a function for the elements.
struct Each {
    array: Gc<Object>,
    data: TypedArrayData,
    length: usize,
    callback: Value,
    this_arg: Value,
}

/// `ValidateTypedArray(this)`, the length and the callback (`plain`: V8's
/// shorter message for a callback that is not callable, as `map` has it).
fn begin(rt: &mut Runtime, call: &NativeCall, name: &str, plain: bool) -> VmResult<Each> {
    let (array, data, length) = validate_this(rt, call, name)?;
    let callback = rt.arg(call, 0);
    if !rt.is_callable(callback)? {
        return Err(if plain {
            VmError::type_error(format!("{} is not a function", rt.primitive_text(callback)))
        } else {
            rt.not_callable(callback)
        });
    }
    Ok(Each {
        array,
        data,
        length,
        callback,
        this_arg: rt.arg(call, 1),
    })
}

impl Each {
    /// The index of the `n`-th step.
    fn index(&self, n: usize, descending: bool) -> usize {
        if descending { self.length - 1 - n } else { n }
    }

    /// `Call(callback, thisArg, « element, k, O »)` for index `k`, in the
    /// current handle scope: the element (read before the call) and the
    /// result.
    fn call(&self, rt: &mut Runtime, k: usize) -> VmResult<(Value, Value)> {
        let element = rt.typed_value(&self.data, k)?;
        let result = rt.call(
            self.callback,
            self.this_arg,
            &[element, position(k), self.array.into()],
        )?;
        Ok((element, result))
    }

    /// Visits the indices in order (or from the end) until `step` says
    /// stop; `step(rt, k, element, result)` runs in the scope of the step.
    fn run(
        &self,
        rt: &mut Runtime,
        descending: bool,
        mut step: impl FnMut(&mut Runtime, usize, Value, Value) -> VmResult<bool>,
    ) -> VmResult<()> {
        for n in 0..self.length {
            rt.tick()?;
            let k = self.index(n, descending);
            let stop = rt.with_scope(|rt| {
                let (element, result) = self.call(rt, k)?;
                step(rt, k, element, result)
            })?;
            if stop {
                break;
            }
        }
        Ok(())
    }
}

/// `%TypedArray%.prototype.forEach(callback, thisArg)` (§23.2.3.15).
pub(super) fn for_each(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let each = begin(rt, call, "forEach", false)?;
    each.run(rt, false, |_, _, _, _| Ok(false))?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `every` (§23.2.3.7) and `some` (§23.2.3.29): the first callback result
/// whose `ToBoolean` is `stop` ends the loop.
fn every_or_some(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    stop: bool,
) -> VmResult<NativeReturn> {
    let each = begin(rt, call, name, false)?;
    let mut stopped = false;
    each.run(rt, false, |rt, _, _, result| {
        stopped = rt.to_boolean(result)? == stop;
        Ok(stopped)
    })?;
    // `every` is true unless it stopped, `some` true only if it stopped.
    Ok(NativeReturn::Value(Value::Bool(stopped == stop)))
}

/// `%TypedArray%.prototype.every(callback, thisArg)` (§23.2.3.7).
pub(super) fn every(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    every_or_some(rt, call, "every", false)
}

/// `%TypedArray%.prototype.some(callback, thisArg)` (§23.2.3.29).
pub(super) fn some(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    every_or_some(rt, call, "some", true)
}

/// What the `find` family returns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Found {
    Value,
    Index,
}

/// `FindViaPredicate` (§23.2.3.10.1) for `find`, `findIndex`, `findLast`
/// and `findLastIndex`.
fn find_via_predicate(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    descending: bool,
    found: Found,
) -> VmResult<NativeReturn> {
    let each = begin(rt, call, name, false)?;
    let mut hit = None;
    each.run(rt, descending, |rt, k, element, result| {
        if rt.to_boolean(result)? {
            hit = Some((k, element));
        }
        Ok(hit.is_some())
    })?;
    Ok(NativeReturn::Value(match (hit, found) {
        // The scope of the step is closed: root the element again.
        (Some((_, element)), Found::Value) => rt.heap.record(element),
        (Some((k, _)), Found::Index) => position(k),
        (None, Found::Value) => Value::Undefined,
        (None, Found::Index) => Value::Int(-1),
    }))
}

/// `%TypedArray%.prototype.find(predicate, thisArg)` (§23.2.3.10).
pub(super) fn find(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "find", false, Found::Value)
}

/// `%TypedArray%.prototype.findIndex(predicate, thisArg)` (§23.2.3.11).
pub(super) fn find_index(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findIndex", false, Found::Index)
}

/// `%TypedArray%.prototype.findLast(predicate, thisArg)` (§23.2.3.12).
pub(super) fn find_last(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findLast", true, Found::Value)
}

/// `%TypedArray%.prototype.findLastIndex(predicate, thisArg)`
/// (§23.2.3.13).
pub(super) fn find_last_index(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findLastIndex", true, Found::Index)
}

/// `%TypedArray%.prototype.map(callback, thisArg)` (§23.2.3.22): the
/// result of `TypedArraySpeciesCreate(O, « len »)` gets each mapped value
/// through `Set` (the conversion can run script code, and an index that is
/// no longer valid in the result is skipped).
pub(super) fn map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "%TypedArray%.prototype.map";
    let each = begin(rt, call, "map", true)?;
    let (result, target) = rt.typed_species_create(
        each.array,
        each.data.ty,
        CreateArgs::Length(each.length),
        NAME,
    )?;
    each.run(rt, false, |rt, k, _, mapped| {
        rt.typed_set_element(&target, k as f64, mapped)?;
        Ok(false)
    })?;
    Ok(NativeReturn::Value(result.into()))
}

// --- filter (§23.2.3.9) ---

/// The elements that `filter` selected: the bytes of the elements (the
/// array's own element type, little endian), plus the runs of entries
/// whose `Get` gave `undefined`, since a callback made the index invalid.
struct Kept {
    size: usize,
    bytes: Vec<u8>,
    /// `(first entry, count)` of each run of `undefined` entries (their
    /// bytes are zero).
    undefined: Vec<(usize, usize)>,
    count: usize,
}

impl Kept {
    /// Makes room for one more entry, checking the heap limit when the
    /// vectors grow.
    fn room(&mut self, rt: &mut Runtime, limit: usize) -> VmResult<()> {
        if self.bytes.len() + self.size > self.bytes.capacity() {
            let wanted = (self.bytes.capacity() * 2)
                .max(64)
                .min(limit.max(self.size));
            let extra = wanted.saturating_sub(self.bytes.len());
            if !rt.heap.reserve_buffer(extra, &rt.vm)
                || self.bytes.try_reserve_exact(extra).is_err()
            {
                return Err(VmError::range_error(super::array_buffer::ALLOCATION_FAILED));
            }
        }
        Ok(())
    }

    /// Adds the element `bits`, or an `undefined` entry for `None`.
    fn push(&mut self, rt: &mut Runtime, limit: usize, bits: Option<u64>) -> VmResult<()> {
        self.room(rt, limit)?;
        if let Some(bits) = bits {
            let pattern = bits.to_le_bytes();
            self.bytes.extend_from_slice(
                pattern
                    .get(..self.size)
                    .ok_or(VmError::invariant("an element longer than 8 bytes"))?,
            );
        } else {
            self.bytes.resize(self.bytes.len() + self.size, 0);
            match self.undefined.last_mut() {
                Some((first, count)) if *first + *count == self.count => *count += 1,
                _ => {
                    if self.undefined.len() == self.undefined.capacity()
                        && !rt
                            .heap
                            .reserve_buffer(self.undefined.capacity().max(4) * 32, &rt.vm)
                    {
                        return Err(VmError::range_error(super::array_buffer::ALLOCATION_FAILED));
                    }
                    self.undefined.push((self.count, 1));
                }
            }
        }
        self.count += 1;
        Ok(())
    }

    /// The entry `n`: its bits, or `None` for `undefined`.
    fn get(&self, n: usize, run: &mut usize) -> Option<u64> {
        while let Some(&(first, count)) = self.undefined.get(*run) {
            if n >= first + count {
                *run += 1;
            } else {
                if n >= first {
                    return None;
                }
                break;
            }
        }
        let bytes = self.bytes.get(n * self.size..(n + 1) * self.size)?;
        let mut word = [0u8; 8];
        word.get_mut(..self.size)?.copy_from_slice(bytes);
        Some(u64::from_le_bytes(word))
    }
}

/// `%TypedArray%.prototype.filter(callback, thisArg)` (§23.2.3.9).
pub(super) fn filter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "%TypedArray%.prototype.filter";
    let each = begin(rt, call, "filter", false)?;
    let ty = each.data.ty;
    let size = ty.size();
    let mut kept = Kept {
        size,
        bytes: Vec::new(),
        undefined: Vec::new(),
        count: 0,
    };
    let limit = each.length.saturating_mul(size);
    for k in 0..each.length {
        rt.tick()?;
        rt.with_scope(|rt| {
            let bits = rt.heap.typed_bits(&each.data, k)?;
            let (_, selected) = each.call(rt, k)?;
            if rt.to_boolean(selected)? {
                kept.push(rt, limit, bits)?;
            }
            Ok(())
        })?;
    }
    let (result, target) =
        rt.typed_species_create(each.array, ty, CreateArgs::Length(kept.count), NAME)?;
    let mut run = 0;
    for n in 0..kept.count {
        rt.tick()?;
        rt.with_scope(|rt| match kept.get(n, &mut run) {
            Some(bits) => {
                let raw = crate::object::decode_bits(ty, bits);
                rt.heap.typed_set(&target, n, element_of_raw(raw))?;
                Ok(())
            }
            None => rt.typed_set_element(&target, n as f64, Value::Undefined),
        })?;
    }
    Ok(NativeReturn::Value(result.into()))
}

// --- reduce (§23.2.3.25) and reduceRight (§23.2.3.26) ---

/// `reduce` and `reduceRight`.
fn reduce_either(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    descending: bool,
) -> VmResult<NativeReturn> {
    let each = begin(rt, call, name, false)?;
    let has_initial = call.argc() >= 2;
    if each.length == 0 && !has_initial {
        return Err(VmError::type_error(
            "Reduce of empty array with no initial value",
        ));
    }
    let accumulator = rt.heap.alloc_cell(rt.arg(call, 1))?;
    rt.heap.record(accumulator);
    let mut first = !has_initial;
    for n in 0..each.length {
        rt.tick()?;
        let k = each.index(n, descending);
        rt.with_scope(|rt| {
            let element = rt.typed_value(&each.data, k)?;
            if first {
                first = false;
                return set_cell(rt, accumulator, element);
            }
            let previous = rt.heap.cell(accumulator)?.value;
            let next = rt.call(
                each.callback,
                Value::Undefined,
                &[previous, element, position(k), each.array.into()],
            )?;
            set_cell(rt, accumulator, next)
        })?;
    }
    Ok(NativeReturn::Value(rt.heap.cell(accumulator)?.value))
}

fn set_cell(rt: &mut Runtime, cell: Gc<ValueCell>, value: Value) -> VmResult<()> {
    rt.heap.cell_mut(cell)?.value = value;
    Ok(())
}

/// `%TypedArray%.prototype.reduce(callback [, initialValue])`
/// (§23.2.3.25).
pub(super) fn reduce(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    reduce_either(rt, call, "reduce", false)
}

/// `%TypedArray%.prototype.reduceRight(callback [, initialValue])`
/// (§23.2.3.26).
pub(super) fn reduce_right(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    reduce_either(rt, call, "reduceRight", true)
}
