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
//! bytes) until the species constructor ran. Each method is a native loop
//! (`vm/native_loop.rs`): every step ticks the time countdown and runs in
//! a handle scope of its own, so a `BigInt` element and the callback's
//! results stay alive for one step only, and the callback runs as a frame
//! of the interpreter loop.
//!
//! V8's messages (measured in Chromium 148): a callback that is not
//! callable is "number 5 is not a function" (the type and the value, as
//! for `Array.prototype`) except for `map`, which says "5 is not a
//! function".

use crate::heap::Gc;
use crate::object::{Object, TypedArrayData};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::native_loop::{LoopStep, NativeLoop};
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::array_iteration::{Found, Method};
use super::typed_array_create::{CreateArgs, element_of_raw};
use super::typed_array_method::{position, validate_this};

/// The receiver, its length, the callback and `thisArg` of a method.
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
}

/// The registers of a typed-array element loop: the array, the callback,
/// `thisArg`, the result, the current element, the accumulator.
const CURRENT: usize = 4;
const ACCUMULATOR: usize = 5;

/// The state of a typed-array element loop (a native loop): one call per
/// index, `Call(callback, thisArg, « element, k, O »)` (`reduce`:
/// `« accumulator, element, k, O »` with `undefined` as `this`); the
/// element is read before the call.
struct TypedLoop {
    each: Each,
    method: Method,
    descending: bool,
    /// The number of indices visited.
    n: usize,
    /// The index and the element whose call runs.
    current: Option<(usize, Value)>,
    /// `map`: the result and its data.
    result: Option<(Gc<Object>, TypedArrayData)>,
    /// `filter`: the selected elements, and the bits of the current one.
    kept: Option<Box<Kept>>,
    bits: Option<u64>,
    /// `reduce`: the accumulator once it has one.
    accumulator: Option<Value>,
}

impl TypedLoop {
    fn new(each: Each, method: Method) -> TypedLoop {
        TypedLoop {
            each,
            method,
            descending: false,
            n: 0,
            current: None,
            result: None,
            kept: None,
            bits: None,
            accumulator: None,
        }
    }

    /// Runs the state as a native loop.
    fn run(self, rt: &mut Runtime) -> VmResult<NativeReturn> {
        let registers = [
            self.each.array.into(),
            self.each.callback,
            self.each.this_arg,
            self.result
                .as_ref()
                .map_or(Value::Undefined, |(result, _)| Value::Object(*result)),
            Value::Undefined,
            self.accumulator.unwrap_or(Value::Undefined),
        ];
        rt.native_loop(self, &registers)
    }

    /// Takes the callback's result for the current element; returns the
    /// loop's result if it ends here.
    fn take(&mut self, rt: &mut Runtime, result: Value) -> VmResult<Option<Value>> {
        let Some((k, element)) = self.current.take() else {
            return Ok(None);
        };
        match self.method {
            Method::ForEach => {}
            Method::EverySome { stop } => {
                if rt.to_boolean(result)? == stop {
                    // `every` stops with false, `some` with true.
                    return Ok(Some(Value::Bool(stop)));
                }
            }
            Method::Find(found) => {
                if rt.to_boolean(result)? {
                    return Ok(Some(match found {
                        Found::Value => element,
                        Found::Index => position(k),
                    }));
                }
            }
            Method::Map => {
                if let Some((_, target)) = &self.result {
                    rt.typed_set_element(target, k as f64, result)?;
                }
            }
            Method::Filter => {
                if rt.to_boolean(result)?
                    && let Some(kept) = &mut self.kept
                {
                    let limit = self.each.length.saturating_mul(kept.size);
                    kept.push(rt, limit, self.bits)?;
                }
            }
            Method::Reduce => self.accumulator = Some(result),
        }
        Ok(None)
    }

    /// The loop's result after the last index.
    fn finish(&mut self, rt: &mut Runtime) -> VmResult<Value> {
        Ok(match self.method {
            Method::ForEach | Method::Find(Found::Value) => Value::Undefined,
            Method::EverySome { stop } => Value::Bool(!stop),
            Method::Find(Found::Index) => Value::Int(-1),
            Method::Map => self
                .result
                .as_ref()
                .map_or(Value::Undefined, |(result, _)| Value::Object(*result)),
            Method::Filter => match self.kept.take() {
                Some(kept) => filter_result(rt, &self.each, &kept)?.into(),
                None => Value::Undefined,
            },
            Method::Reduce => self.accumulator.unwrap_or(Value::Undefined),
        })
    }
}

impl NativeLoop for TypedLoop {
    fn step(&mut self, rt: &mut Runtime, regs: usize, input: Value) -> VmResult<LoopStep> {
        if let Some(result) = self.take(rt, input)? {
            return Ok(LoopStep::Done(result));
        }
        if let Some(accumulator) = self.accumulator {
            rt.set_loop_reg(regs, ACCUMULATOR, accumulator)?;
        }
        let mut first = true;
        loop {
            if self.n >= self.each.length {
                return Ok(LoopStep::Done(self.finish(rt)?));
            }
            if !first {
                // The first element of `reduce` took no call: a step of
                // its own.
                rt.tick()?;
            }
            first = false;
            let k = self.each.index(self.n, self.descending);
            self.n += 1;
            if self.method == Method::Filter {
                self.bits = rt.heap.typed_bits(&self.each.data, k)?;
            }
            let element = rt.typed_value(&self.each.data, k)?;
            let array = Value::Object(self.each.array);
            if self.method == Method::Reduce {
                let Some(accumulator) = self.accumulator else {
                    self.accumulator = Some(element);
                    rt.set_loop_reg(regs, ACCUMULATOR, element)?;
                    continue;
                };
                self.current = Some((k, element));
                let args = [accumulator, element, position(k), array];
                rt.push_call(self.each.callback, Value::Undefined, &args)?;
                return Ok(LoopStep::Call);
            }
            self.current = Some((k, element));
            // `find` returns the element after the call.
            rt.set_loop_reg(regs, CURRENT, element)?;
            let args = [element, position(k), array];
            rt.push_call(self.each.callback, self.each.this_arg, &args)?;
            return Ok(LoopStep::Call);
        }
    }
}

/// `%TypedArray%.prototype.forEach(callback, thisArg)` (§23.2.3.15).
pub(super) fn for_each(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let each = begin(rt, call, "forEach", false)?;
    TypedLoop::new(each, Method::ForEach).run(rt)
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
    let method = Method::EverySome { stop };
    TypedLoop::new(each, method).run(rt)
}

/// `%TypedArray%.prototype.every(callback, thisArg)` (§23.2.3.7).
pub(super) fn every(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    every_or_some(rt, call, "every", false)
}

/// `%TypedArray%.prototype.some(callback, thisArg)` (§23.2.3.29).
pub(super) fn some(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    every_or_some(rt, call, "some", true)
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
    let mut state = TypedLoop::new(each, Method::Find(found));
    state.descending = descending;
    state.run(rt)
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
    let created = rt.typed_species_create(
        each.array,
        each.data.ty,
        CreateArgs::Length(each.length),
        NAME,
    )?;
    let mut state = TypedLoop::new(each, Method::Map);
    state.result = Some(created);
    state.run(rt)
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
    let each = begin(rt, call, "filter", false)?;
    let size = each.data.ty.size();
    let mut state = TypedLoop::new(each, Method::Filter);
    state.kept = Some(Box::new(Kept {
        size,
        bytes: Vec::new(),
        undefined: Vec::new(),
        count: 0,
    }));
    state.run(rt)
}

/// The result of `filter` after the loop: `TypedArraySpeciesCreate` with
/// the number of selected elements, then the elements.
fn filter_result(rt: &mut Runtime, each: &Each, kept: &Kept) -> VmResult<Gc<Object>> {
    const NAME: &str = "%TypedArray%.prototype.filter";
    let ty = each.data.ty;
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
    Ok(result)
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
    let initial = rt.arg(call, 1);
    let mut state = TypedLoop::new(each, Method::Reduce);
    state.descending = descending;
    state.accumulator = has_initial.then_some(initial);
    state.run(rt)
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
