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

use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{VmError, VmResult, WellKnown};

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
    #[allow(dead_code, reason = "for-of, spread and destructuring (feature 3)")]
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
    #[allow(dead_code, reason = "for-of, spread and destructuring (feature 3)")]
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
}
