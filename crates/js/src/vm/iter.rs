//! The iterator operations for native functions (ECMA-262 §7.4):
//! `GetIterator`, `IteratorStepValue` and `IteratorClose`.
//!
//! An [`IteratorRecord`] holds an iterator and its `next` method. Both are
//! recorded in the handle scope that is open when the record is made, so
//! the caller keeps the record in a scope that outlives its loop and
//! opens a handle scope per step (`with_scope`), as the loops of
//! `Array.prototype.forEach` do. Each step calls `next` through
//! [`Runtime::call`] (re-entry, charged to the recursion budget) and ticks
//! the time countdown, so an iterator that never ends stops at the time
//! limit.
//!
//! Closing: a loop that fails after it took a value from the iterator
//! calls [`Runtime::iterator_close_after_error`] with its error, which
//! runs `return` and keeps the loop's error (§7.4.13 `IteratorClose` with
//! a throw completion). A termination or an engine error runs no script
//! code and passes through unchanged.
//!
//! Compiled code (`for`-`of`, spread, array patterns) keeps its iterator
//! records in registers and steps them with instructions
//! (`GetIterator`, `IteratorCall`, `IteratorValue`, `IteratorClose`);
//! the parts of those that run in Rust are here: V8's messages for a
//! value that is not iterable, the recognition of the built-in Array
//! Iterator (whose steps need no result object), the result check and
//! the close. A thrown value that a close must keep is in a register of
//! the compiled `finally` block, so the close itself holds none.

use crate::bytecode::{
    FunctionCode, HINT_NO_TEXT, HINT_SHIFT, ITER_ARRAY_SPREAD, ITER_CALL, ITER_RENDERED,
    ITER_SPREAD,
};
use crate::heap::Gc;
use crate::object::{IterationKind, Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, VmError, VmResult, WellKnown};

/// An Iterator Record (§7.4.1).
#[derive(Clone, Copy, Debug)]
pub(crate) struct IteratorRecord {
    /// `[[Iterator]]`: an object.
    pub(crate) iterator: Value,
    /// `[[NextMethod]]`: not checked for callability until it is called.
    pub(crate) next_method: Value,
    /// `[[Done]]`.
    pub(crate) done: bool,
}

impl Runtime {
    /// `GetIterator(obj, sync)` (§7.4.3). The record's values are recorded
    /// in the current handle scope.
    pub(crate) fn get_iterator(&mut self, value: Value) -> VmResult<IteratorRecord> {
        let key = self.symbol_key(WellKnown::Iterator);
        let method = match value {
            Value::Undefined | Value::Null => Value::Undefined,
            _ => self.get_value(value, key)?,
        };
        if !self.is_callable(method)? {
            return Err(self.not_iterable(value));
        }
        self.get_iterator_from_method(value, method)
    }

    /// `GetIteratorFromMethod` (§7.4.2).
    pub(crate) fn get_iterator_from_method(
        &mut self,
        value: Value,
        method: Value,
    ) -> VmResult<IteratorRecord> {
        let iterator = self.call(method, value, &[])?;
        if !matches!(iterator, Value::Object(_)) {
            return Err(VmError::type_error(
                "Result of the Symbol.iterator method is not an object",
            ));
        }
        let key = PropertyKey::String(self.vm.atoms.next);
        let next_method = self.get_value(iterator, key)?;
        Ok(IteratorRecord {
            iterator,
            next_method,
            done: false,
        })
    }

    /// `IteratorStep` (§7.4.9): the result object of the next step, or
    /// `None` when the iterator is done. A failure marks the record done
    /// (the iterator is not closed after its own errors). The result is
    /// recorded in the current handle scope.
    pub(crate) fn iterator_step(&mut self, record: &mut IteratorRecord) -> VmResult<Option<Value>> {
        let step = self.iterator_step_inner(record);
        if !matches!(step, Ok(Some(_))) {
            record.done = true;
        }
        step
    }

    /// `IteratorStepValue` (§7.4.10): the next value, or `None` when the
    /// iterator is done; failures as in [`Runtime::iterator_step`]. The
    /// value is recorded in the current handle scope.
    pub(crate) fn iterator_step_value(
        &mut self,
        record: &mut IteratorRecord,
    ) -> VmResult<Option<Value>> {
        if let Some(builtin) = self.builtin_step(record.iterator, record.next_method) {
            // The built-in `next` of an Array, Map or Set Iterator: the
            // step without the result object.
            self.tick()?;
            let step = crate::builtins::builtin_iterator_next_value(self, builtin);
            if !matches!(step, Ok(Some(_))) {
                record.done = true;
            }
            return step;
        }
        let Some(result) = self.iterator_step(record)? else {
            return Ok(None);
        };
        let key = PropertyKey::String(self.vm.atoms.value);
        match self.get_value(result, key) {
            Ok(value) => Ok(Some(value)),
            Err(error) => {
                record.done = true;
                Err(error)
            }
        }
    }

    /// One call of `next`: the result object, or `None` if it says done.
    fn iterator_step_inner(&mut self, record: &IteratorRecord) -> VmResult<Option<Value>> {
        self.tick()?;
        if !self.is_callable(record.next_method)? {
            return Err(self.not_callable(record.next_method));
        }
        let result = self.call(record.next_method, record.iterator, &[])?;
        if !matches!(result, Value::Object(_)) {
            return Err(self.iterator_result_error(result));
        }
        let done = self.get_value(result, PropertyKey::String(self.vm.atoms.done))?;
        Ok((!self.to_boolean(done)?).then_some(result))
    }

    /// The `TypeError` for a result of `next` or `return` that is not an
    /// object (V8's wording).
    fn iterator_result_error(&self, result: Value) -> VmError {
        VmError::type_error(format!(
            "Iterator result {} is not an object",
            self.primitive_text(result)
        ))
    }

    /// `IteratorClose` (§7.4.11) with a normal completion: calls `return`;
    /// a failure of `return` or a result that is not an object is the
    /// outcome.
    pub(crate) fn iterator_close(&mut self, record: &IteratorRecord) -> VmResult<()> {
        let inner = self.iterator_return(record)?;
        match inner {
            Some(result) if !matches!(result, Value::Object(_)) => {
                Err(self.iterator_result_error(result))
            }
            _ => Ok(()),
        }
    }

    /// `IteratorClose` (§7.4.11) with the throw completion `error`:
    /// calls `return` and returns `error`. Errors of `return` are
    /// dropped, except a termination or an engine error, which end the
    /// script.
    pub(crate) fn iterator_close_after_error(
        &mut self,
        record: &IteratorRecord,
        error: VmError,
    ) -> VmError {
        if matches!(error, VmError::Terminated(_) | VmError::Internal(_)) {
            return error;
        }
        // `return` can collect.
        self.hold_error(&error);
        match self.iterator_return(record) {
            Err(inner @ (VmError::Terminated(_) | VmError::Internal(_))) => inner,
            _ => error,
        }
    }

    /// The call of `return` in `IteratorClose`: its result, or `None`
    /// when the iterator has no `return` method.
    fn iterator_return(&mut self, record: &IteratorRecord) -> VmResult<Option<Value>> {
        let key = PropertyKey::String(self.vm.atoms.return_);
        match self.get_method(record.iterator, key)? {
            None => Ok(None),
            Some(method) => Ok(Some(self.call(method, record.iterator, &[])?)),
        }
    }

    /// `GetIterator(value, sync)` (§7.4.3) for compiled code: a value
    /// without a callable `@@iterator` fails with the message that `hint`
    /// selects ([`crate::bytecode::Insn::GetIterator`]). The record's
    /// values are recorded in the current handle scope.
    pub(crate) fn get_iterator_hinted(
        &mut self,
        value: Value,
        hint: u32,
        code: &FunctionCode,
    ) -> VmResult<IteratorRecord> {
        let key = self.symbol_key(WellKnown::Iterator);
        let method = match value {
            Value::Undefined | Value::Null => Value::Undefined,
            _ => self.get_value(value, key)?,
        };
        if !self.is_callable(method)? {
            return Err(self.iteration_error(value, hint, code));
        }
        let record = self.get_iterator_from_method(value, method)?;
        if hint >> HINT_SHIFT == ITER_ARRAY_SPREAD && !self.is_callable(record.next_method)? {
            return Err(self.iteration_error(value, hint, code));
        }
        Ok(record)
    }

    /// V8's `TypeError` for a value that is not iterable, in the form
    /// that `hint` selects.
    fn iteration_error(&self, value: Value, hint: u32, code: &FunctionCode) -> VmError {
        let index = hint & HINT_NO_TEXT;
        let text = (index != HINT_NO_TEXT)
            .then(|| code.string(index))
            .flatten()
            .map(|text| self.name_text(text));
        let message = match (hint >> HINT_SHIFT, text) {
            (ITER_SPREAD, _) if !value.is_nullish() => {
                "Spread syntax requires ...iterable[Symbol.iterator] to be a function".to_owned()
            }
            (ITER_SPREAD, text) => {
                let value_text = self.primitive_text(value);
                format!(
                    "{} is not iterable (cannot read property {value_text})",
                    text.unwrap_or_else(|| value_text.clone())
                )
            }
            (ITER_RENDERED | ITER_ARRAY_SPREAD, Some(text)) => format!("{text} is not iterable"),
            (ITER_CALL, Some(text)) => {
                format!("{text} is not a function or its return value is not iterable")
            }
            _ => return self.not_iterable(value),
        };
        VmError::type_error(message)
    }

    /// The Array Iterator of a record whose step can run without calling
    /// `next`: `iterator` is an Array Iterator and `next` is the built-in
    /// `%ArrayIteratorPrototype%.next` of some realm. The step then does
    /// what `next` does, except for the result object, which nothing can
    /// observe.
    pub(crate) fn builtin_array_step(&self, iterator: Value, next: Value) -> Option<Gc<Object>> {
        let (Value::Object(iterator), Value::Object(next)) = (iterator, next) else {
            return None;
        };
        let ObjectKind::Native(native) = &self.heap.object(next).ok()?.kind else {
            return None;
        };
        let builtin = self
            .intrinsic(native.realm, Intrinsic::ArrayIteratorNext)
            .ok()?;
        let is_array_iterator = matches!(
            self.heap.object(iterator).ok()?.kind,
            ObjectKind::ArrayIterator(_)
        );
        (builtin == next && is_array_iterator).then_some(iterator)
    }

    /// The built-in iterator of a record whose step can run without
    /// calling `next`: an Array, Map or Set Iterator or an iterator
    /// helper whose `next` is the built-in method of its kind (of some
    /// realm). The step then does
    /// what `next` does, except for the result object, which nothing can
    /// observe ([`crate::builtins::builtin_iterator_next_value`]).
    pub(crate) fn builtin_step(&self, iterator: Value, next: Value) -> Option<Gc<Object>> {
        let (Value::Object(iterator), Value::Object(next)) = (iterator, next) else {
            return None;
        };
        let ObjectKind::Native(native) = &self.heap.object(next).ok()?.kind else {
            return None;
        };
        let which = match self.heap.object(iterator).ok()?.kind {
            ObjectKind::ArrayIterator(_) => Intrinsic::ArrayIteratorNext,
            ObjectKind::MapIterator(_) => Intrinsic::MapIteratorNext,
            ObjectKind::SetIterator(_) => Intrinsic::SetIteratorNext,
            ObjectKind::IteratorHelper(_) => Intrinsic::IteratorHelperNext,
            _ => return None,
        };
        let builtin = self.intrinsic(native.realm, which).ok()?;
        (builtin == next).then_some(iterator)
    }

    /// The rest of `IteratorStepValue` (§7.4.10) after `next` returned
    /// `result`: the value, or `None` when the result says done. Getters
    /// of `done` and `value` run through re-entry. The value is recorded
    /// in the current handle scope.
    pub(crate) fn iterator_result_value(&mut self, result: Value) -> VmResult<Option<Value>> {
        if !matches!(result, Value::Object(_)) {
            return Err(self.iterator_result_error(result));
        }
        let done = self.get_value(result, PropertyKey::String(self.vm.atoms.done))?;
        if self.to_boolean(done)? {
            return Ok(None);
        }
        let value = self.get_value(result, PropertyKey::String(self.vm.atoms.value))?;
        Ok(Some(value))
    }

    /// `IteratorClose` (§7.4.11) of compiled code. After a throw
    /// completion (`throw`), the errors of `return` are dropped (a
    /// termination or an engine error is not); the thrown value itself is
    /// in a register of the caller's frame. Otherwise an error of
    /// `return`, or a result that is not an object, is the outcome.
    pub(crate) fn close_iterator(&mut self, iterator: Value, throw: bool) -> VmResult<()> {
        let record = IteratorRecord {
            iterator,
            next_method: Value::Undefined,
            done: false,
        };
        if !throw {
            return self.with_scope(|rt| rt.iterator_close(&record));
        }
        match self.scoped(|rt| rt.iterator_return(&record)) {
            Err(error @ (VmError::Terminated(_) | VmError::Internal(_))) => Err(error),
            _ => Ok(()),
        }
    }

    /// The bulk copy of [`crate::bytecode::Insn::SpreadArray`]: appends
    /// the values that the built-in Array Iterator `iterator` has left to
    /// the array `target` if it iterates the values of an array without
    /// holes, and finishes the iterator. Returns whether it did. The
    /// copy charges a time step per element.
    pub(crate) fn spread_array_iterator(
        &mut self,
        target: Gc<Object>,
        iterator: Gc<Object>,
    ) -> VmResult<bool> {
        let ObjectKind::ArrayIterator(state) = &self.heap.object(iterator)?.kind else {
            return Ok(false);
        };
        let (Some(source), IterationKind::Values) = (state.target, state.kind) else {
            return Ok(false);
        };
        // The next index is below 2^53; above the array's length the
        // array is done (the copy appends nothing).
        let start = u32::try_from(state.next_index as u64).unwrap_or(u32::MAX);
        let Some(count) = self
            .heap
            .append_array_elements(target, source, start, &self.vm)?
        else {
            return Ok(false);
        };
        if let ObjectKind::ArrayIterator(state) = &mut self.heap.object_mut(iterator)?.kind {
            state.target = None;
        }
        self.charge(count as usize)?;
        Ok(true)
    }
}
