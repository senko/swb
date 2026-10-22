//! The `Array.prototype` methods that call a function for the elements
//! (ECMA-262 §23.1.3, <https://tc39.es/ecma262/#sec-properties-of-the-array-prototype-object>):
//! `forEach`, `map`, `filter`, `every`, `some`, `reduce`, `reduceRight`
//! and the `find` family.
//!
//! Each method is a native loop (`vm/native_loop.rs`): a step takes the
//! result of the last call, then finds the next element and asks for the
//! next call, so the callbacks run as frames of the interpreter loop that
//! called the method.
//!
//! All but the `find` family skip the indices that `HasProperty` does not
//! find, so on sparse elements they visit only the stored indices
//! (`array_access.rs`). An element in the receiver's dense vector is the
//! next one without a search. The `find` family calls its predicate for
//! every index, holes included (§23.1.3.12.1 `FindViaPredicate`), so its
//! cost is the length; a run of indices that no object of the chain
//! stores reads `undefined` without a lookup.

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::native_loop::{LoopStep, NativeLoop};
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::array::{callback_arg, receiver};
use super::array_access::Next;

/// What the `find` family returns (also for typed arrays).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Found {
    Value,
    Index,
}

/// The method of an element loop (also of the typed-array loops).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Method {
    ForEach,
    Map,
    Filter,
    /// `every` (`false` stops) and `some` (`true` stops).
    EverySome {
        stop: bool,
    },
    Reduce,
    Find(Found),
}

/// The registers of an element loop (`vm/native_loop.rs`): the receiver,
/// the callback, `thisArg`, the result array, the current element and the
/// accumulator. The state keeps copies of them.
const REGISTERS: usize = 6;
const CURRENT: usize = 4;
const ACCUMULATOR: usize = 5;

/// The state of an element loop.
struct ElementLoop {
    method: Method,
    object: Gc<Object>,
    callback: Value,
    this_arg: Value,
    /// The result array of `map` and `filter`.
    result: Option<Gc<Object>>,
    /// The next index to visit.
    k: f64,
    /// Ascending: the end (exclusive); descending: the lowest index.
    bound: f64,
    descending: bool,
    /// The `find` family: the indices before `gap` (after it when
    /// descending) read `undefined` without a lookup.
    gap: f64,
    /// The index and the value of the element whose call runs.
    current: Option<(f64, Value)>,
    /// The next index of the result of `filter`.
    to: f64,
    /// The accumulator of `reduce` once it has one.
    accumulator: Option<Value>,
}

impl ElementLoop {
    fn new(method: Method, object: Gc<Object>, callback: Value, length: f64) -> ElementLoop {
        ElementLoop {
            method,
            object,
            callback,
            this_arg: Value::Undefined,
            result: None,
            k: 0.0,
            bound: length,
            descending: false,
            gap: 0.0,
            current: None,
            to: 0.0,
            accumulator: None,
        }
    }

    /// Visits indices from the top down to 0.
    fn descending(mut self, length: f64) -> ElementLoop {
        self.descending = true;
        self.k = length - 1.0;
        self.bound = 0.0;
        self.gap = length;
        self
    }

    fn in_range(&self) -> bool {
        if self.descending {
            self.k >= self.bound
        } else {
            self.k < self.bound
        }
    }

    fn advance(&mut self, to: f64) {
        self.k = if self.descending { to - 1.0 } else { to + 1.0 };
    }

    /// The next index that `HasProperty` finds, with the value of `Get`
    /// (recorded in the step's scope); `None` at the end.
    fn next_present(&mut self, rt: &mut Runtime) -> VmResult<Option<(f64, Value)>> {
        let object = self.object;
        let mut first = true;
        while self.in_range() {
            if !first {
                rt.tick()?;
            }
            first = false;
            let k = self.k;
            // A value from the dense vector needs no rooting: it goes into
            // the call's window before anything allocates.
            if let Some(value) = rt.heap.dense_element(object, k) {
                self.advance(k);
                return Ok(Some((k, value)));
            }
            let next = if self.descending {
                rt.prev_element(object, k, self.bound)?
            } else {
                rt.next_element(object, k, self.bound)?
            };
            let (index, value) = match next {
                Next::End => return Ok(None),
                Next::At(index, Some(value)) => (index, rt.heap.record(value)),
                Next::At(index, None) => (index, rt.get_index(object, index)?),
                Next::Generic => {
                    if !rt.has_index(object, k)? {
                        self.advance(k);
                        continue;
                    }
                    (k, rt.get_index(object, k)?)
                }
            };
            self.advance(index);
            return Ok(Some((index, value)));
        }
        Ok(None)
    }

    /// The next index and the value of `Get` for the `find` family, holes
    /// included; `None` at the end.
    fn next_index(&mut self, rt: &mut Runtime) -> VmResult<Option<(f64, Value)>> {
        if !self.in_range() {
            return Ok(None);
        }
        let (object, k) = (self.object, self.k);
        let in_gap = if self.descending {
            k >= self.gap
        } else {
            k < self.gap
        };
        if in_gap {
            self.advance(k);
            return Ok(Some((k, Value::Undefined)));
        }
        if let Some(value) = rt.heap.dense_element(object, k) {
            self.advance(k);
            return Ok(Some((k, rt.heap.record(value))));
        }
        let next = if self.descending {
            rt.prev_element(object, k, self.bound)?
        } else {
            rt.next_element(object, k, self.bound)?
        };
        let value = match next {
            // No index of the rest has the property.
            Next::End => {
                self.gap = if self.descending {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                Value::Undefined
            }
            // The indices up to the found one read `undefined`.
            Next::At(index, _) if index != k => {
                self.gap = if self.descending { index + 1.0 } else { index };
                Value::Undefined
            }
            Next::At(_, Some(value)) => rt.heap.record(value),
            Next::Generic | Next::At(_, None) => rt.get_index(object, k)?,
        };
        self.advance(k);
        Ok(Some((k, value)))
    }

    /// The initial registers.
    fn registers(&self) -> [Value; REGISTERS] {
        [
            self.object.into(),
            self.callback,
            self.this_arg,
            self.result.map_or(Value::Undefined, Value::Object),
            Value::Undefined,
            self.accumulator.unwrap_or(Value::Undefined),
        ]
    }

    /// Runs the state as a native loop.
    fn run(self, rt: &mut Runtime) -> VmResult<NativeReturn> {
        let registers = self.registers();
        rt.native_loop(self, &registers)
    }

    /// Takes the result of the callback for the current element; returns
    /// the loop's result if it ends here.
    fn take(&mut self, rt: &mut Runtime, result: Value) -> VmResult<Option<Value>> {
        let Some((k, value)) = self.current.take() else {
            return Ok(None);
        };
        match self.method {
            Method::ForEach => {}
            Method::Map => {
                if let Some(array) = self.result {
                    rt.create_index(array, k, result)?;
                }
            }
            Method::Filter => {
                if rt.to_boolean(result)?
                    && let Some(array) = self.result
                {
                    if !rt.heap.push_element(array, value)? {
                        rt.create_index(array, self.to, value)?;
                    }
                    self.to += 1.0;
                }
            }
            Method::EverySome { stop } => {
                if rt.to_boolean(result)? == stop {
                    // `every` stops with false, `some` with true.
                    return Ok(Some(Value::Bool(stop)));
                }
            }
            Method::Reduce => self.accumulator = Some(result),
            Method::Find(found) => {
                if rt.to_boolean(result)? {
                    return Ok(Some(match found {
                        Found::Value => value,
                        Found::Index => Value::number(k),
                    }));
                }
            }
        }
        Ok(None)
    }

    /// The loop's result after the last element.
    fn finish(&self) -> VmResult<Value> {
        Ok(match self.method {
            Method::ForEach | Method::Find(Found::Value) => Value::Undefined,
            Method::Map | Method::Filter => self.result.map_or(Value::Undefined, Value::Object),
            Method::EverySome { stop } => Value::Bool(!stop),
            Method::Reduce => self.accumulator.ok_or_else(empty_reduce)?,
            Method::Find(Found::Index) => Value::Int(-1),
        })
    }
}

impl NativeLoop for ElementLoop {
    fn step(&mut self, rt: &mut Runtime, regs: usize, input: Value) -> VmResult<LoopStep> {
        if let Some(result) = self.take(rt, input)? {
            return Ok(LoopStep::Done(result));
        }
        if self.method == Method::Reduce
            && let Some(accumulator) = self.accumulator
        {
            rt.set_loop_reg(regs, ACCUMULATOR, accumulator)?;
        }
        loop {
            let next = if matches!(self.method, Method::Find(_)) {
                self.next_index(rt)?
            } else {
                self.next_present(rt)?
            };
            let Some((k, value)) = next else {
                return Ok(LoopStep::Done(self.finish()?));
            };
            let object = Value::Object(self.object);
            if self.method == Method::Reduce {
                let Some(accumulator) = self.accumulator else {
                    // The first element is the initial accumulator.
                    self.accumulator = Some(value);
                    rt.set_loop_reg(regs, ACCUMULATOR, value)?;
                    continue;
                };
                self.current = Some((k, value));
                let args = [accumulator, value, Value::number(k), object];
                rt.push_call(self.callback, Value::Undefined, &args)?;
                return Ok(LoopStep::Call);
            }
            self.current = Some((k, value));
            if matches!(self.method, Method::Filter | Method::Find(_)) {
                // The value is used after the call.
                rt.set_loop_reg(regs, CURRENT, value)?;
            }
            rt.push_call(
                self.callback,
                self.this_arg,
                &[value, Value::number(k), object],
            )?;
            return Ok(LoopStep::Call);
        }
    }
}

/// The receiver, its length, the callback and `thisArg` of a method.
fn start(rt: &mut Runtime, call: &NativeCall, name: &str, method: Method) -> VmResult<ElementLoop> {
    let (object, length) = receiver(rt, call, Some(name))?;
    let callback = callback_arg(rt, call, 0)?;
    let mut state = ElementLoop::new(method, object, callback, length);
    state.this_arg = rt.arg(call, 1);
    Ok(state)
}

/// `Array.prototype.forEach(callback, thisArg)` (§23.1.3.15).
pub(super) fn for_each(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    start(rt, call, "forEach", Method::ForEach)?.run(rt)
}

/// `Array.prototype.map(callback, thisArg)` (§23.1.3.21).
pub(super) fn map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut state = start(rt, call, "map", Method::Map)?;
    state.result = Some(rt.array_species_create(state.object, state.bound)?);
    state.run(rt)
}

/// `Array.prototype.filter(callback, thisArg)` (§23.1.3.8).
pub(super) fn filter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut state = start(rt, call, "filter", Method::Filter)?;
    state.result = Some(rt.array_species_create(state.object, 0.0)?);
    state.run(rt)
}

/// `Array.prototype.every(callback, thisArg)` (§23.1.3.6).
pub(super) fn every(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let method = Method::EverySome { stop: false };
    start(rt, call, "every", method)?.run(rt)
}

/// `Array.prototype.some(callback, thisArg)` (§23.1.3.29).
pub(super) fn some(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let method = Method::EverySome { stop: true };
    start(rt, call, "some", method)?.run(rt)
}

/// The `TypeError` of `reduce` without an initial value on no elements.
fn empty_reduce() -> VmError {
    VmError::type_error("Reduce of empty array with no initial value")
}

/// `reduce` (§23.1.3.24) and `reduceRight` (§23.1.3.25).
fn reduce_either(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    right: bool,
) -> VmResult<NativeReturn> {
    let (object, length) = receiver(rt, call, Some(name))?;
    let callback = callback_arg(rt, call, 0)?;
    if length == 0.0 && call.argc() < 2 {
        return Err(empty_reduce());
    }
    let mut state = ElementLoop::new(Method::Reduce, object, callback, length);
    if right {
        state = state.descending(length);
    }
    if call.argc() >= 2 {
        state.accumulator = Some(rt.arg(call, 1));
    }
    state.run(rt)
}

/// `Array.prototype.reduce(callback, initialValue)` (§23.1.3.24).
pub(super) fn reduce(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    reduce_either(rt, call, "reduce", false)
}

/// `Array.prototype.reduceRight(callback, initialValue)` (§23.1.3.25).
pub(super) fn reduce_right(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    reduce_either(rt, call, "reduceRight", true)
}

/// `FindViaPredicate` (§23.1.3.12.1) for `find`, `findIndex`, `findLast`
/// and `findLastIndex`.
fn find_via_predicate(
    rt: &mut Runtime,
    call: &NativeCall,
    name: &str,
    descending: bool,
    found: Found,
) -> VmResult<NativeReturn> {
    let mut state = start(rt, call, name, Method::Find(found))?;
    if descending {
        let length = state.bound;
        state = state.descending(length);
    }
    state.run(rt)
}

/// `Array.prototype.find(predicate, thisArg)` (§23.1.3.9).
pub(super) fn find(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "find", false, Found::Value)
}

/// `Array.prototype.findIndex(predicate, thisArg)` (§23.1.3.10).
pub(super) fn find_index(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findIndex", false, Found::Index)
}

/// `Array.prototype.findLast(predicate, thisArg)` (§23.1.3.11).
pub(super) fn find_last(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findLast", true, Found::Value)
}

/// `Array.prototype.findLastIndex(predicate, thisArg)` (§23.1.3.12).
pub(super) fn find_last_index(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    find_via_predicate(rt, call, "findLastIndex", true, Found::Index)
}
