//! The element loops of the `Array.prototype` methods (ECMA-262 §23.1.3,
//! <https://tc39.es/ecma262/#sec-properties-of-the-array-prototype-object>):
//! the steps `HasProperty`, `Get`, `Set` and `DeletePropertyOrThrow` on an
//! index, and the loops that visit only the indices where they matter
//! (ADR 0026 section 15).
//!
//! The rule for sparse loops: when the receiver and its prototypes have
//! ordinary elements (`object/element_access.rs`), `HasProperty(O, k)`
//! has no effect and is true only where an object of the chain stores
//! `k`, so a loop that does nothing for an index without the property
//! goes from one stored index to the next. The search runs again after
//! each visited index, because a getter or a callback can add elements,
//! remove them or change the prototype chain; an object without ordinary
//! elements on the chain makes the loop take the specification's steps
//! for the next index. A delete of an index that the receiver does not
//! own has no effect, so loops that delete visit only the own indices.
//!
//! Time: each visited index is a step of the countdown, and the searches
//! charge one step per 16 slots or keys they look at.

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::VmResult;

/// The slots or keys that a search looks at per time step.
const SLOTS_PER_STEP: usize = 16;

/// The next index of a loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Next {
    /// The index, and its value if the receiver owns it as a data
    /// property (the value of `[[Get]]`).
    At(f64, Option<Value>),
    /// No index in the range has the property.
    End,
    /// The chain has an object without ordinary elements: take the
    /// specification's steps for the next index.
    Generic,
}

/// What [`Runtime::for_index`] gives its body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Item {
    /// The index and the value of `Get`.
    Value(f64, Value),
    /// The indices `from..to`, each of which reads `undefined` without
    /// side effects.
    Gap(f64, f64),
}

/// What the body of a loop wants next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flow {
    Continue,
    Break,
}

impl Runtime {
    /// The smallest index in `from..end` that `HasProperty(object, k)`
    /// finds (see the module documentation).
    pub(super) fn next_element(
        &mut self,
        object: Gc<Object>,
        from: f64,
        end: f64,
    ) -> VmResult<Next> {
        if from >= end {
            return Ok(Next::End);
        }
        let search = self.heap.search_chain(object, from, end, true)?;
        self.element_result(object, search)
    }

    /// The largest index in `low..=from` that `HasProperty(object, k)`
    /// finds.
    pub(super) fn prev_element(
        &mut self,
        object: Gc<Object>,
        from: f64,
        low: f64,
    ) -> VmResult<Next> {
        if from < low {
            return Ok(Next::End);
        }
        let search = self.heap.search_chain(object, from, low, false)?;
        self.element_result(object, search)
    }

    fn element_result(
        &mut self,
        object: Gc<Object>,
        search: crate::object::ChainSearch,
    ) -> VmResult<Next> {
        self.charge(search.work / SLOTS_PER_STEP)?;
        if !search.ordinary {
            return Ok(Next::Generic);
        }
        let Some(k) = search.found else {
            return Ok(Next::End);
        };
        Ok(Next::At(k, self.own_data_value(object, k)?))
    }

    /// The value of the own data property `k` of `object`, if it has one.
    fn own_data_value(&mut self, object: Gc<Object>, k: f64) -> VmResult<Option<Value>> {
        if k >= crate::object::FIRST_NAMED_INDEX {
            return Ok(None);
        }
        Ok(
            match self
                .heap
                .get_own_property(object, PropertyKey::Index(k as u32))?
            {
                Some(crate::object::Property::Data { value, .. }) => Some(value),
                _ => None,
            },
        )
    }

    /// The smallest index in `from..end` that `object` owns, or
    /// [`Next::Generic`] if `object` does not have ordinary elements.
    pub(super) fn next_own(&mut self, object: Gc<Object>, from: f64, end: f64) -> VmResult<Next> {
        if from >= end {
            return Ok(Next::End);
        }
        if !self.heap.has_ordinary_elements(object)? {
            return Ok(Next::Generic);
        }
        let (found, work) = self.heap.next_own_index(object, from, end)?;
        self.charge(work / SLOTS_PER_STEP)?;
        Ok(found.map_or(Next::End, |k| Next::At(k, None)))
    }

    /// The largest index in `low..=from` that `object` owns, or
    /// [`Next::Generic`].
    pub(super) fn prev_own(&mut self, object: Gc<Object>, from: f64, low: f64) -> VmResult<Next> {
        if from < low {
            return Ok(Next::End);
        }
        if !self.heap.has_ordinary_elements(object)? {
            return Ok(Next::Generic);
        }
        let (found, work) = self.heap.prev_own_index(object, from, low)?;
        self.charge(work / SLOTS_PER_STEP)?;
        Ok(found.map_or(Next::End, |k| Next::At(k, None)))
    }

    // --- The steps on one index ---

    /// `HasProperty(object, ToString(k))`.
    pub(super) fn has_index(&mut self, object: Gc<Object>, k: f64) -> VmResult<bool> {
        let key = self.index_key(k)?;
        self.has_property(object, key)
    }

    /// `Get(object, ToString(k))`, recorded in the current handle scope.
    pub(super) fn get_index(&mut self, object: Gc<Object>, k: f64) -> VmResult<Value> {
        let key = self.index_key(k)?;
        self.get_value(object.into(), key)
    }

    /// `Set(object, ToString(k), value, true)`.
    pub(super) fn set_index(&mut self, object: Gc<Object>, k: f64, value: Value) -> VmResult<()> {
        let key = self.index_key(k)?;
        self.set_or_throw(object, key, value)
    }

    /// `DeletePropertyOrThrow(object, ToString(k))` (§7.3.10).
    pub(super) fn delete_index(&mut self, object: Gc<Object>, k: f64) -> VmResult<()> {
        let key = self.index_key(k)?;
        self.delete_or_throw(object, key)
    }

    /// `DeletePropertyOrThrow(object, key)` (§7.3.10, V8's message).
    pub(super) fn delete_or_throw(&mut self, object: Gc<Object>, key: PropertyKey) -> VmResult<()> {
        self.delete_generic(object, key, true)?;
        Ok(())
    }

    /// `CreateDataPropertyOrThrow(object, ToString(k), value)`.
    pub(super) fn create_index(
        &mut self,
        object: Gc<Object>,
        k: f64,
        value: Value,
    ) -> VmResult<()> {
        let key = self.index_key(k)?;
        self.create_data_property_or_throw(object, key, value)
    }

    /// `Set(object, "length", length, true)`.
    pub(super) fn set_length(&mut self, object: Gc<Object>, length: f64) -> VmResult<()> {
        let key = PropertyKey::String(self.heap.length_atom());
        self.set_or_throw(object, key, Value::number(length))
    }

    // --- Loops ---

    /// Calls `body` with each index `k` of `start..end`, in ascending
    /// order, for which `HasProperty(object, k)` is true, and with the
    /// value of `Get(object, k)`; each call in a handle scope of its own.
    /// Stops when `body` returns [`Flow::Break`]. The value is recorded in
    /// the step's scope.
    pub(super) fn for_present(
        &mut self,
        object: Gc<Object>,
        start: f64,
        end: f64,
        mut body: impl FnMut(&mut Runtime, f64, Value) -> VmResult<Flow>,
    ) -> VmResult<()> {
        let mut k = start;
        while k < end {
            self.tick()?;
            let step = self.with_scope(|rt| {
                // An element in the receiver's dense vector is the next one
                // (no search of the chain).
                if let Some(value) = rt.heap.dense_element(object, k) {
                    let value = rt.heap.record(value);
                    return Ok(Some((k, body(rt, k, value)?)));
                }
                let (index, value) = match rt.next_element(object, k, end)? {
                    Next::End => return Ok(None),
                    Next::At(index, Some(value)) => (index, rt.heap.record(value)),
                    Next::At(index, None) => (index, rt.get_index(object, index)?),
                    Next::Generic => {
                        if !rt.has_index(object, k)? {
                            return Ok(Some((k, Flow::Continue)));
                        }
                        (k, rt.get_index(object, k)?)
                    }
                };
                Ok(Some((index, body(rt, index, value)?)))
            })?;
            match step {
                None | Some((_, Flow::Break)) => break,
                Some((index, Flow::Continue)) => k = index + 1.0,
            }
        }
        Ok(())
    }

    /// Like [`Runtime::for_present`], in descending order over the indices
    /// of `low..=top`.
    pub(super) fn for_present_rev(
        &mut self,
        object: Gc<Object>,
        top: f64,
        low: f64,
        mut body: impl FnMut(&mut Runtime, f64, Value) -> VmResult<Flow>,
    ) -> VmResult<()> {
        let mut k = top;
        while k >= low {
            self.tick()?;
            let step = self.with_scope(|rt| {
                if let Some(value) = rt.heap.dense_element(object, k) {
                    let value = rt.heap.record(value);
                    return Ok(Some((k, body(rt, k, value)?)));
                }
                let (index, value) = match rt.prev_element(object, k, low)? {
                    Next::End => return Ok(None),
                    Next::At(index, Some(value)) => (index, rt.heap.record(value)),
                    Next::At(index, None) => (index, rt.get_index(object, index)?),
                    Next::Generic => {
                        if !rt.has_index(object, k)? {
                            return Ok(Some((k, Flow::Continue)));
                        }
                        (k, rt.get_index(object, k)?)
                    }
                };
                Ok(Some((index, body(rt, index, value)?)))
            })?;
            match step {
                None | Some((_, Flow::Break)) => break,
                Some((index, Flow::Continue)) => k = index - 1.0,
            }
        }
        Ok(())
    }

    /// Calls `body` for each index of `start..end` in ascending order (in
    /// descending order with `descending`) with the value of `Get(object,
    /// k)`, for the methods that read every index, holes included
    /// (`includes`, `find`, the copying methods). While the chain has
    /// ordinary elements, a run of indices that no object of the chain
    /// stores reads `undefined` without side effects; such a run comes as
    /// one [`Item::Gap`] (`body` decides what each of its indices costs).
    /// Each call runs in a handle scope of its own.
    pub(super) fn for_index(
        &mut self,
        object: Gc<Object>,
        start: f64,
        end: f64,
        descending: bool,
        mut body: impl FnMut(&mut Runtime, Item) -> VmResult<Flow>,
    ) -> VmResult<()> {
        // The unvisited indices are `low..high`.
        let (mut low, mut high) = (start, end);
        while low < high {
            self.tick()?;
            let step = self.with_scope(|rt| {
                let next = if descending {
                    rt.prev_element(object, high - 1.0, low)?
                } else {
                    rt.next_element(object, low, high)?
                };
                let (item, visited) = match next {
                    Next::Generic => {
                        let k = if descending { high - 1.0 } else { low };
                        (Item::Value(k, rt.get_index(object, k)?), (k, k))
                    }
                    Next::End => (Item::Gap(low, high), (low, high - 1.0)),
                    Next::At(k, _) if descending && k < high - 1.0 => {
                        (Item::Gap(k + 1.0, high), (k + 1.0, high - 1.0))
                    }
                    Next::At(k, _) if !descending && k > low => (Item::Gap(low, k), (low, k - 1.0)),
                    Next::At(k, Some(value)) => (Item::Value(k, rt.heap.record(value)), (k, k)),
                    Next::At(k, None) => (Item::Value(k, rt.get_index(object, k)?), (k, k)),
                };
                Ok((body(rt, item)?, visited))
            })?;
            match step {
                (Flow::Break, _) => break,
                (Flow::Continue, (first, last)) => {
                    if descending {
                        high = first;
                    } else {
                        low = last + 1.0;
                    }
                }
            }
        }
        Ok(())
    }

    /// `DeletePropertyOrThrow(object, k)` for each `k` of `low..end` in
    /// descending order (with `ascending`, in ascending order), visiting
    /// only the indices that `object` owns while it has ordinary elements.
    pub(super) fn delete_range(
        &mut self,
        object: Gc<Object>,
        low: f64,
        end: f64,
        ascending: bool,
    ) -> VmResult<()> {
        if ascending {
            let mut k = low;
            while k < end {
                self.tick()?;
                let at = match self.next_own(object, k, end)? {
                    Next::End => break,
                    Next::At(index, _) => index,
                    Next::Generic => k,
                };
                self.with_scope(|rt| rt.delete_index(object, at))?;
                k = at + 1.0;
            }
        } else {
            let mut k = end - 1.0;
            while k >= low {
                self.tick()?;
                let at = match self.prev_own(object, k, low)? {
                    Next::End => break,
                    Next::At(index, _) => index,
                    Next::Generic => k,
                };
                self.with_scope(|rt| rt.delete_index(object, at))?;
                k = at - 1.0;
            }
        }
        Ok(())
    }

    /// The loop that moves `count` elements of `object` from `from + i` to
    /// `to + i` (shift, unshift, splice, copyWithin): for each `i`, in
    /// ascending order or descending with `descending`,
    /// `HasProperty(from + i)`, then `Set(to + i, Get(from + i))` or
    /// `DeletePropertyOrThrow(to + i)`. Only the `i` where the source has
    /// the property or the receiver owns the target can change anything,
    /// so with ordinary elements the loop visits only those.
    pub(super) fn move_elements(
        &mut self,
        object: Gc<Object>,
        from: f64,
        to: f64,
        count: f64,
        descending: bool,
    ) -> VmResult<()> {
        if count <= 0.0 {
            return Ok(());
        }
        let mut i = if descending { count - 1.0 } else { 0.0 };
        loop {
            if (descending && i < 0.0) || (!descending && i >= count) {
                return Ok(());
            }
            self.tick()?;
            let Some(at) = self.next_move(object, from, to, count, i, descending)? else {
                return Ok(());
            };
            self.with_scope(|rt| {
                if rt.has_index(object, from + at)? {
                    let value = rt.get_index(object, from + at)?;
                    rt.set_index(object, to + at, value)
                } else {
                    rt.delete_index(object, to + at)
                }
            })?;
            i = if descending { at - 1.0 } else { at + 1.0 };
        }
    }

    /// The next `i` of [`Runtime::move_elements`] from `i` on that can
    /// change anything, or `None`.
    fn next_move(
        &mut self,
        object: Gc<Object>,
        from: f64,
        to: f64,
        count: f64,
        i: f64,
        descending: bool,
    ) -> VmResult<Option<f64>> {
        let (source, target) = if descending {
            (
                self.prev_element(object, from + i, from)?,
                self.prev_own(object, to + i, to)?,
            )
        } else {
            (
                self.next_element(object, from + i, from + count)?,
                self.next_own(object, to + i, to + count)?,
            )
        };
        let source = match source {
            Next::Generic => return Ok(Some(i)),
            Next::At(k, _) => Some(k - from),
            Next::End => None,
        };
        let target = match target {
            Next::Generic => return Ok(Some(i)),
            Next::At(k, _) => Some(k - to),
            Next::End => None,
        };
        Ok(match (source, target) {
            (Some(a), Some(b)) if descending => Some(a.max(b)),
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        })
    }
}
