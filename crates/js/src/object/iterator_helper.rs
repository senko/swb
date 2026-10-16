//! The payloads of iterator helper objects and of the objects that
//! `Iterator.from` wraps (ECMA-262 §27.1.2.1 `%IteratorHelperPrototype%`,
//! §27.1.3.2.1.1 `%WrapForValidIteratorPrototype%`,
//! <https://tc39.es/ecma262/#sec-iteration>).
//!
//! The specification writes a helper as an abstract closure that yields;
//! swb stores the closure's variables here and runs its steps in
//! `builtins/iterator_helper.rs` (a native state machine). The states are
//! those of a generator (§27.5.3): suspended-start, suspended-yield,
//! executing and completed.

use std::mem::size_of;

use crate::heap::Tracer;
use crate::value::Value;
use crate::vm::iter::IteratorRecord;

/// The generator state of a helper (`[[GeneratorState]]`, §27.5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HelperStatus {
    /// Created; `next` has not run.
    SuspendedStart,
    /// Stopped after a yield.
    SuspendedYield,
    /// `next` or `return` is running (a re-entrant call is a `TypeError`).
    Executing,
    /// Finished; it never leaves this state.
    Completed,
}

/// What a helper does: the method that made it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HelperOp {
    /// `Iterator.prototype.map` (<https://tc39.es/ecma262/#sec-iterator.prototype.map>).
    Map,
    /// `Iterator.prototype.filter` (<https://tc39.es/ecma262/#sec-iterator.prototype.filter>).
    Filter,
    /// `Iterator.prototype.take` (<https://tc39.es/ecma262/#sec-iterator.prototype.take>).
    Take,
    /// `Iterator.prototype.drop` (<https://tc39.es/ecma262/#sec-iterator.prototype.drop>).
    Drop,
    /// `Iterator.prototype.flatMap` (<https://tc39.es/ecma262/#sec-iterator.prototype.flatMap>).
    FlatMap,
    /// `Iterator.concat` (iterator sequencing proposal).
    Concat,
}

/// An iterator helper object: the closure variables of the specification.
#[derive(Debug)]
pub struct IteratorHelper {
    pub(crate) status: HelperStatus,
    pub(crate) op: HelperOp,
    /// `[[UnderlyingIterator]]` with its `next` method (not used by
    /// `concat`).
    pub(crate) underlying: IteratorRecord,
    /// The mapper or predicate (`undefined` for `take`, `drop` and
    /// `concat`).
    pub(crate) function: Value,
    /// `counter` of `map`, `filter` and `flatMap`; `remaining` of `take`
    /// and `drop` (an integer or +∞).
    pub(crate) count: f64,
    /// `flatMap`: the inner iterator while it is being read. `concat`: the
    /// iterator of the current iterable.
    pub(crate) inner: Option<IteratorRecord>,
    /// `concat`: the open method and the iterable of each argument, in
    /// pairs.
    pub(crate) sources: Vec<Value>,
    /// `concat`: the index of the next pair of `sources` to open.
    pub(crate) next_source: usize,
}

impl IteratorHelper {
    /// A helper in the suspended-start state.
    pub(crate) fn new(
        op: HelperOp,
        underlying: IteratorRecord,
        function: Value,
        count: f64,
    ) -> Self {
        IteratorHelper {
            status: HelperStatus::SuspendedStart,
            op,
            underlying,
            function,
            count,
            inner: None,
            sources: Vec::new(),
            next_source: 0,
        }
    }

    /// Reports the handles of the payload to the tracer.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        trace_record(&self.underlying, tracer);
        tracer.value(self.function);
        if let Some(inner) = &self.inner {
            trace_record(inner, tracer);
        }
        for value in &self.sources {
            tracer.value(*value);
        }
    }

    /// The bytes that the payload owns outside the object record.
    pub(crate) fn heap_size(&self) -> usize {
        size_of::<IteratorHelper>() + self.sources.capacity() * size_of::<Value>()
    }
}

/// Reports the iterator and the `next` method of a record.
pub(crate) fn trace_record(record: &IteratorRecord, tracer: &mut Tracer<'_>) {
    tracer.value(record.iterator);
    tracer.value(record.next_method);
}

/// The `[[Iterated]]` record of an object that `Iterator.from` made
/// (§27.1.3.2.1.1).
#[derive(Debug)]
pub struct WrappedIterator {
    pub(crate) record: IteratorRecord,
}
