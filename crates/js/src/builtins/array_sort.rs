//! `Array.prototype.sort` and `toSorted` (ECMA-262 §23.1.3.30, §23.1.3.35,
//! <https://tc39.es/ecma262/#sec-array.prototype.sort>) with
//! `SortIndexedProperties` (§23.1.3.30.1) and `CompareArrayElements`
//! (§23.1.3.30.2).
//!
//! The sort is stable: binary insertion of short runs (Knuth, TAOCP
//! volume 3, section 5.2.1), then a bottom-up merge sort of the runs
//! (Sedgewick and Wayne, "Algorithms", 4th edition, section 2.2, with the
//! test that skips the merge of two runs that are already in order). It
//! sorts a permutation of indices (`u32`), so the values stay in a heap
//! array that the handle scope roots while the comparator runs (a
//! collection can run in any comparator), and no Rust recursion is
//! needed. It makes O(n log n) comparisons and always ends with a
//! permutation, whatever the comparator answers; a comparator that throws
//! ends the sort with its error before anything is written back. With a
//! comparator the sort is a native loop (`vm/native_loop.rs`): the same
//! merge sort as a state machine ([`SortMachine`]) gives one comparison
//! per step, and the comparator runs as a frame of the interpreter loop. The
//! comparator sees a private copy of the values: changes of the array
//! during the sort show only in the steps after it (the writes of the
//! sorted values and the deletes of the holes), as the specification
//! says.
//!
//! `undefined` sorts last without a comparison (`CompareArrayElements`
//! steps 2 to 4), so the undefined values are counted and appended. The
//! default order compares `ToString` of the values: integers by their
//! decimal digits without a string, other primitives through strings made
//! once per value (nothing can observe it), objects at each comparison.
//! The comparator is called as `comparator(later, earlier)` and a result
//! below zero moves the later value first; with a consistent comparator
//! any order of the calls gives the same result, and the constant
//! comparators of the web give Chromium's results for the lengths that
//! were measured (`() => -1` reverses, `() => 1` keeps the order).

use std::cmp::Ordering;

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::Value;
use crate::vm::native_loop::{LoopStep, NativeLoop};
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::array::receiver;
use super::array_access::{Flow, Item};

/// The values that one round of a write-back copies (a fixed buffer).
const WRITE_CHUNK: usize = 256;

/// The comparator argument of `sort` and `toSorted` (step 1): `None` for
/// `undefined`, else a callable (V8's message otherwise).
pub(super) fn comparator_arg(rt: &mut Runtime, call: &NativeCall) -> VmResult<Option<Value>> {
    let comparator = rt.arg(call, 0);
    if comparator.is_undefined() {
        return Ok(None);
    }
    if rt.is_callable(comparator)? {
        return Ok(Some(comparator));
    }
    Err(VmError::type_error(format!(
        "The comparison function must be either a function or undefined: {}",
        rt.primitive_text(comparator)
    )))
}

/// The values that `SortIndexedProperties` collected, without the
/// `undefined` values, which are only counted.
#[derive(Clone, Copy)]
struct Collected {
    /// An internal array (no prototype, never reachable from script code)
    /// whose dense elements are the values.
    items: Gc<Object>,
    /// The number of values.
    count: usize,
    /// The number of `undefined` values (with `read_through`, the holes too).
    undefined: f64,
}

/// Steps 1 to 4 of `SortIndexedProperties` (§23.1.3.30.1): the values of
/// the indices below `length`; with `read_through` every index
/// (`read-through-holes`), else only those that `HasProperty` finds
/// (`skip-holes`).
fn collect(
    rt: &mut Runtime,
    object: Gc<Object>,
    length: f64,
    read_through: bool,
) -> VmResult<Collected> {
    let items = rt.heap.new_array_with_capacity(None, 0, &rt.vm)?;
    rt.heap.record(items);
    let mut collected = Collected {
        items,
        count: 0,
        undefined: 0.0,
    };
    let add = |rt: &mut Runtime, value: Value, collected: &mut Collected| -> VmResult<()> {
        if value.is_undefined() {
            collected.undefined += 1.0;
        } else if rt.heap.push_element(collected.items, value)? {
            collected.count += 1;
        } else {
            return Err(VmError::invariant("the sort buffer refused a value"));
        }
        Ok(())
    };
    if rt.heap.dense_array(object)?.is_some() {
        // No getter: copy the vector.
        let values: Vec<Value> = rt
            .heap
            .object(object)?
            .elements
            .dense()
            .map(<[Value]>::to_vec)
            .unwrap_or_default();
        rt.charge(values.len())?;
        for value in values {
            if !value.is_empty() {
                add(rt, value, &mut collected)?;
            }
        }
        if read_through {
            collected.undefined += length - collected.count as f64 - collected.undefined;
        }
        return Ok(collected);
    }
    if read_through {
        rt.for_index(object, 0.0, length, false, |rt, item| {
            match item {
                Item::Value(_, value) => add(rt, value, &mut collected)?,
                Item::Gap(from, to) => collected.undefined += to - from,
            }
            Ok(Flow::Continue)
        })?;
    } else {
        rt.for_present(object, 0.0, length, |rt, _, value| {
            add(rt, value, &mut collected)?;
            Ok(Flow::Continue)
        })?;
    }
    Ok(collected)
}

/// The value at `index` of an internal array of the sort.
pub(super) fn item(rt: &Runtime, items: Gc<Object>, index: u32) -> VmResult<Value> {
    rt.heap
        .object(items)?
        .elements
        .dense()
        .and_then(|values| values.get(index as usize))
        .copied()
        .ok_or(VmError::invariant("a sort index outside its values"))
}

/// Sorts the collected values without a comparator (step 5 of
/// `SortIndexedProperties`): the permutation of their indices in sorted
/// order.
fn sort_collected(rt: &mut Runtime, collected: &Collected) -> VmResult<Vec<u32>> {
    let count = collected.count;
    // The two permutations are outside the heap: check that they fit.
    rt.heap
        .check_room(count.saturating_mul(2 * size_of::<u32>()))?;
    let mut order: Vec<u32> = (0..count as u32).collect();
    default_sort(rt, collected.items, &mut order)?;
    Ok(order)
}

/// The default order (`CompareArrayElements` without a comparator): by
/// `ToString` of the values.
fn default_sort(rt: &mut Runtime, items: Gc<Object>, order: &mut [u32]) -> VmResult<()> {
    let values: Vec<Value> = rt
        .heap
        .object(items)?
        .elements
        .dense()
        .map(<[Value]>::to_vec)
        .unwrap_or_default();
    // Integers in the `i32` range, in either form (`ToString(-0)` is "0").
    let as_int = |value: &Value| match *value {
        Value::Int(int) => Some(int),
        Value::Double(double)
            if double.fract() == 0.0 && (-2_147_483_648.0..=2_147_483_647.0).contains(&double) =>
        {
            Some(double as i32)
        }
        _ => None,
    };
    if values.iter().all(|value| as_int(value).is_some()) {
        let ints: Vec<i32> = values
            .iter()
            .map(|value| as_int(value).unwrap_or(0))
            .collect();
        let mut steps = 0usize;
        merge_sort(order, |later, earlier| -> VmResult<bool> {
            steps += 1;
            let (a, b) = (ints.get(later as usize), ints.get(earlier as usize));
            match (a, b) {
                (Some(&a), Some(&b)) => Ok(compare_int_strings(a, b) == Ordering::Less),
                _ => Err(VmError::invariant("a sort index outside its values")),
            }
        })?;
        return rt.charge(steps);
    }
    if values.len() < 2 {
        return Ok(());
    }
    if values.iter().any(|value| matches!(value, Value::Object(_))) {
        // `ToString` of an object can run script code: convert at each
        // comparison, as `CompareArrayElements` does.
        return merge_sort(order, |later, earlier| {
            rt.with_scope(|rt| {
                let x = item(rt, items, later)?;
                let y = item(rt, items, earlier)?;
                let x = rt.to_string(x)?;
                let y = rt.to_string(y)?;
                compare_strings(rt, x, y)
            })
        });
    }
    // Primitives: their strings, once (a symbol throws, as its first
    // comparison would).
    let keys = rt
        .heap
        .new_array_with_capacity(None, values.len(), &rt.vm)?;
    rt.heap.record(keys);
    for value in values {
        rt.tick()?;
        let string = rt.to_string(value)?;
        if !rt.heap.push_element(keys, string.into())? {
            return Err(VmError::invariant("the sort keys refused a string"));
        }
    }
    merge_sort(order, |later, earlier| {
        let (Value::String(x), Value::String(y)) =
            (item(rt, keys, later)?, item(rt, keys, earlier)?)
        else {
            return Err(VmError::invariant("a sort key that is not a string"));
        };
        compare_strings(rt, x, y)
    })
}

/// Whether the string `x` sorts before `y` (code unit order), charged by
/// the length of the shorter one.
fn compare_strings(rt: &mut Runtime, x: Gc<JsString>, y: Gc<JsString>) -> VmResult<bool> {
    let shorter = rt.heap.string(x)?.len().min(rt.heap.string(y)?.len());
    let order = rt.heap.compare_strings(x, y)?;
    rt.charge(1 + shorter / 64)?;
    Ok(order == Ordering::Less)
}

/// The order of the decimal strings of two integers (`ToString` and the
/// code unit order of `IsLessThan`), without making the strings: a minus
/// sign sorts before the digits; two magnitudes compare as digit strings
/// when the shorter one is scaled to the length of the longer, and a
/// prefix sorts first.
fn compare_int_strings(a: i32, b: i32) -> Ordering {
    match (a < 0, b < 0) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => compare_digit_strings(a.unsigned_abs(), b.unsigned_abs()),
    }
}

/// The order of the decimal digit strings of two magnitudes.
fn compare_digit_strings(x: u32, y: u32) -> Ordering {
    let (dx, dy) = (digit_count(x), digit_count(y));
    let longer = dx.max(dy);
    let scaled = |value: u32, digits: u32| u64::from(value) * 10u64.pow(longer - digits);
    scaled(x, dx).cmp(&scaled(y, dy)).then(dx.cmp(&dy))
}

/// The number of decimal digits of `value` (1 for 0).
fn digit_count(value: u32) -> u32 {
    value.checked_ilog10().map_or(1, |log| log + 1)
}

/// The length of the runs that binary insertion sorts before the merges.
const INSERTION_RUN: usize = 16;

/// A stable sort of `order`: binary insertion of runs of
/// [`INSERTION_RUN`] elements (Knuth, "The Art of Computer Programming",
/// volume 3, section 5.2.1), then a bottom-up merge sort of the runs
/// (Sedgewick and Wayne, "Algorithms", 4th edition, section 2.2, with its
/// improvements: insertion sort for small runs, and no merge for two runs
/// whose boundary is in order). `before(later, earlier)` says whether the
/// element `later` must come before `earlier`, which is ahead of it in the
/// input; an error ends the sort at once. Every comparison is between
/// elements of the slice, every index is checked, and the result is a
/// permutation whatever `before` answers.
pub(super) fn merge_sort<E>(
    order: &mut [u32],
    mut before: impl FnMut(u32, u32) -> Result<bool, E>,
) -> Result<(), E> {
    let n = order.len();
    if n < 2 {
        return Ok(());
    }
    for run in order.chunks_mut(INSERTION_RUN) {
        binary_insertion(run, &mut before)?;
    }
    if n <= INSERTION_RUN {
        return Ok(());
    }
    let mut scratch = order.to_vec();
    // `in_order` tells which buffer holds the current runs.
    let mut in_order = true;
    let mut width = INSERTION_RUN;
    while width < n {
        {
            let (source, target): (&[u32], &mut [u32]) = if in_order {
                (&*order, &mut scratch)
            } else {
                (&scratch, &mut *order)
            };
            let mut low = 0;
            while low < n {
                let middle = (low + width).min(n);
                let high = (low + 2 * width).min(n);
                merge(source, target, (low, middle, high), &mut before)?;
                low = high;
            }
        }
        in_order = !in_order;
        width *= 2;
    }
    if !in_order {
        order.copy_from_slice(&scratch);
    }
    Ok(())
}

/// Binary insertion sort of a short run: each element goes after the
/// last element of the sorted prefix that it need not come before. The
/// first comparison is with the last element of the prefix, so input
/// that is already in order costs one comparison per element.
fn binary_insertion<E>(
    run: &mut [u32],
    before: &mut impl FnMut(u32, u32) -> Result<bool, E>,
) -> Result<(), E> {
    for i in 1..run.len() {
        let (Some(&x), Some(&last)) = (run.get(i), run.get(i - 1)) else {
            break;
        };
        if !before(x, last)? {
            continue;
        }
        let (mut low, mut high) = (0, i - 1);
        while low < high {
            let middle = low + (high - low) / 2;
            let Some(&y) = run.get(middle) else {
                break;
            };
            if before(x, y)? {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        run.copy_within(low..i, low + 1);
        if let Some(slot) = run.get_mut(low) {
            *slot = x;
        }
    }
    Ok(())
}

/// Merges the runs `low..middle` and `middle..high` of `source` into
/// `target`.
fn merge<E>(
    source: &[u32],
    target: &mut [u32],
    (low, middle, high): (usize, usize, usize),
    before: &mut impl FnMut(u32, u32) -> Result<bool, E>,
) -> Result<(), E> {
    let (Some(from), Some(to)) = (source.get(low..high), target.get_mut(low..high)) else {
        return Ok(());
    };
    let split = middle - low;
    if split >= from.len() {
        to.copy_from_slice(from);
        return Ok(());
    }
    let (left, right) = from.split_at(split);
    if let (Some(&last), Some(&first)) = (left.last(), right.first())
        && !before(first, last)?
    {
        to.copy_from_slice(from);
        return Ok(());
    }
    let (mut i, mut j) = (0, 0);
    for slot in to.iter_mut() {
        let take_right = match (left.get(i), right.get(j)) {
            (Some(&l), Some(&r)) => before(r, l)?,
            (None, _) => true,
            (_, None) => false,
        };
        if take_right {
            *slot = right.get(j).copied().unwrap_or_default();
            j += 1;
        } else {
            *slot = left.get(i).copied().unwrap_or_default();
            i += 1;
        }
    }
    Ok(())
}

/// Where a step of [`SortMachine`] stands.
#[derive(Clone, Copy, Debug)]
enum Phase {
    /// Binary insertion of the run that starts at `start`: element `i` is
    /// next; `search` is the range of the binary search while it runs, and
    /// `middle` the element of its pending comparison.
    Insertion {
        start: usize,
        i: usize,
        search: Option<(usize, usize, usize)>,
    },
    /// The merges of runs of `width`: the pair at `low`; `merging` is the
    /// position in its left run, its right run and the target while the
    /// merge runs (`None` before the boundary test). `in_order` tells
    /// which buffer holds the runs.
    Merge {
        width: usize,
        low: usize,
        in_order: bool,
        merging: Option<(usize, usize, usize)>,
    },
    Done,
}

/// [`merge_sort`] as a state machine for a comparator that runs as a
/// script function: [`SortMachine::advance`] gives the next comparison
/// `(later, earlier)` and takes its answer at the next call, so a native
/// loop can call the comparator between the steps. It makes the same
/// comparisons in the same order and gives the same permutation as
/// `merge_sort` with the same answers (a unit test compares them).
pub(super) struct SortMachine {
    order: Vec<u32>,
    scratch: Vec<u32>,
    phase: Phase,
}

impl SortMachine {
    /// A machine that sorts the permutation `0..count`.
    pub(super) fn new(count: usize) -> SortMachine {
        let phase = if count < 2 {
            Phase::Done
        } else {
            Phase::Insertion {
                start: 0,
                i: 1,
                search: None,
            }
        };
        SortMachine {
            order: (0..count as u32).collect(),
            scratch: Vec::new(),
            phase,
        }
    }

    /// The sorted permutation (after `advance` returned `None`).
    #[cfg(test)]
    pub(super) fn order(&self) -> &[u32] {
        &self.order
    }

    /// Applies `answer` (whether `later` must come before `earlier`) to
    /// the pending comparison, if there is one, and returns the next
    /// comparison, or `None` when the permutation is sorted.
    pub(super) fn advance(&mut self, mut answer: Option<bool>) -> Option<(u32, u32)> {
        let n = self.order.len();
        loop {
            match self.phase {
                Phase::Done => return None,
                Phase::Insertion { start, i, search } => {
                    let len = INSERTION_RUN.min(n - start);
                    if search.is_none() && answer.is_none() && i >= len {
                        self.phase = self.after_run(start + INSERTION_RUN);
                        continue;
                    }
                    let run = self.order.get_mut(start..start + len)?;
                    let x = *run.get(i)?;
                    match (search, answer.take()) {
                        // The test against the last element of the prefix.
                        (None, Some(false)) => self.phase = Phase::insertion(start, i + 1),
                        (None, Some(true)) => {
                            self.phase = Phase::Insertion {
                                start,
                                i,
                                search: Some((0, i - 1, 0)),
                            };
                        }
                        (Some((low, high, middle)), Some(before)) => {
                            let (low, high) = if before {
                                (low, middle)
                            } else {
                                (middle + 1, high)
                            };
                            self.phase = Phase::Insertion {
                                start,
                                i,
                                search: Some((low, high, middle)),
                            };
                        }
                        (None, None) => return Some((x, *run.get(i - 1)?)),
                        (Some((low, high, _)), None) => {
                            if low < high {
                                let middle = low + (high - low) / 2;
                                self.phase = Phase::Insertion {
                                    start,
                                    i,
                                    search: Some((low, high, middle)),
                                };
                                return Some((x, *run.get(middle)?));
                            }
                            run.copy_within(low..i, low + 1);
                            *run.get_mut(low)? = x;
                            self.phase = Phase::insertion(start, i + 1);
                        }
                    }
                }
                Phase::Merge {
                    width,
                    low,
                    in_order,
                    merging,
                } => {
                    if let Some(next) = self.merge_step(width, low, in_order, merging, &mut answer)
                    {
                        return Some(next);
                    }
                }
            }
        }
    }

    /// The phase after the insertion of the run before `next`.
    fn after_run(&mut self, next: usize) -> Phase {
        let n = self.order.len();
        if next < n {
            return Phase::insertion(next, 1);
        }
        if n <= INSERTION_RUN {
            return Phase::Done;
        }
        self.scratch = self.order.clone();
        Phase::Merge {
            width: INSERTION_RUN,
            low: 0,
            in_order: true,
            merging: None,
        }
    }

    /// One part of a merge (see [`merge`]): sets the next phase and returns
    /// a comparison, or `None` to go on.
    fn merge_step(
        &mut self,
        width: usize,
        low: usize,
        in_order: bool,
        merging: Option<(usize, usize, usize)>,
        answer: &mut Option<bool>,
    ) -> Option<(u32, u32)> {
        let len = self.order.len();
        if low >= len {
            // The end of a pass over the pairs.
            let (width, in_order) = (width * 2, !in_order);
            if width >= len {
                if !in_order {
                    self.order.copy_from_slice(&self.scratch);
                }
                self.phase = Phase::Done;
            } else {
                self.phase = Phase::Merge {
                    width,
                    low: 0,
                    in_order,
                    merging: None,
                };
            }
            return None;
        }
        let middle = (low + width).min(len);
        let high = (low + 2 * width).min(len);
        let next_pair = Phase::Merge {
            width,
            low: high,
            in_order,
            merging: None,
        };
        let (source, target): (&[u32], &mut [u32]) = if in_order {
            (&self.order, &mut self.scratch)
        } else {
            (&self.scratch, &mut self.order)
        };
        let (from, to) = (source.get(low..high)?, target.get_mut(low..high)?);
        let split = middle - low;
        let (left, right) = from.split_at(split.min(from.len()));
        let Some((i, j, slot)) = merging else {
            match answer.take() {
                None if split < from.len() => {
                    return Some((*right.first()?, *left.last()?));
                }
                // The boundary is in order (or there is no right run).
                None | Some(false) => {
                    to.copy_from_slice(from);
                    self.phase = next_pair;
                }
                Some(true) => {
                    self.phase = Phase::Merge {
                        width,
                        low,
                        in_order,
                        merging: Some((0, 0, 0)),
                    };
                }
            }
            return None;
        };
        let (mut i, mut j, mut slot) = (i, j, slot);
        while slot < to.len() {
            let take_right = match (left.get(i), right.get(j)) {
                (Some(&earlier), Some(&later)) => {
                    let Some(before) = answer.take() else {
                        self.phase = Phase::Merge {
                            width,
                            low,
                            in_order,
                            merging: Some((i, j, slot)),
                        };
                        return Some((later, earlier));
                    };
                    before
                }
                (None, _) => true,
                (_, None) => false,
            };
            let value = if take_right {
                j += 1;
                right.get(j - 1)
            } else {
                i += 1;
                left.get(i - 1)
            };
            *to.get_mut(slot)? = value.copied().unwrap_or_default();
            slot += 1;
        }
        self.phase = next_pair;
        None
    }
}

impl Phase {
    fn insertion(start: usize, i: usize) -> Phase {
        Phase::Insertion {
            start,
            i,
            search: None,
        }
    }
}

/// The sorted values in order: the permuted values, then the
/// `undefined`s; `emit` receives them in chunks.
fn emit_sorted(
    rt: &mut Runtime,
    collected: &Collected,
    order: &[u32],
    mut emit: impl FnMut(&mut Runtime, &[Value]) -> VmResult<()>,
) -> VmResult<()> {
    let mut buffer = [Value::Undefined; WRITE_CHUNK];
    for part in order.chunks(WRITE_CHUNK) {
        for (slot, &index) in buffer.iter_mut().zip(part) {
            *slot = item(rt, collected.items, index)?;
        }
        let values = buffer
            .get(..part.len())
            .ok_or(VmError::invariant("a sort chunk outside the buffer"))?;
        emit(rt, values)?;
    }
    let undefined = [Value::Undefined; WRITE_CHUNK];
    let mut left = collected.undefined as u64;
    while left > 0 {
        let chunk = left.min(WRITE_CHUNK as u64) as usize;
        let values = undefined
            .get(..chunk)
            .ok_or(VmError::invariant("a sort chunk outside the buffer"))?;
        emit(rt, values)?;
        left -= chunk as u64;
    }
    Ok(())
}

/// `Array.prototype.sort(comparefn)` (§23.1.3.30). With a comparator the
/// sort is a native loop ([`SortLoop`]).
pub(super) fn sort(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let comparator = comparator_arg(rt, call)?;
    let (object, length) = receiver(rt, call, None)?;
    let collected = collect(rt, object, length, false)?;
    let target = SortTarget::InPlace { object, length };
    finish_or_loop(rt, collected, comparator, target)
}

/// Where the sorted values go.
#[derive(Clone, Copy)]
enum SortTarget {
    /// Back into the receiver (`sort`).
    InPlace { object: Gc<Object>, length: f64 },
    /// Into a new array (`toSorted`).
    Copy(Gc<Object>),
}

impl SortTarget {
    fn value(self) -> Value {
        match self {
            SortTarget::InPlace { object, .. } | SortTarget::Copy(object) => object.into(),
        }
    }
}

/// Sorts without a comparator at once; with one, returns the native loop
/// that calls it.
fn finish_or_loop(
    rt: &mut Runtime,
    collected: Collected,
    comparator: Option<Value>,
    target: SortTarget,
) -> VmResult<NativeReturn> {
    let Some(comparator) = comparator else {
        let order = sort_collected(rt, &collected)?;
        write_sorted(rt, &collected, &order, target)?;
        return Ok(NativeReturn::Value(target.value()));
    };
    // The two permutations are outside the heap: check that they fit.
    rt.heap
        .check_room(collected.count.saturating_mul(2 * size_of::<u32>()))?;
    let machine = SortMachine::new(collected.count);
    let registers = [collected.items.into(), comparator, target.value()];
    let state = SortLoop {
        collected,
        comparator,
        machine,
        target,
        pending: false,
    };
    rt.native_loop(state, &registers)
}

/// Writes the sorted values to the target (steps 6 to 9 of `sort`, or the
/// copy of `toSorted`).
fn write_sorted(
    rt: &mut Runtime,
    collected: &Collected,
    order: &[u32],
    target: SortTarget,
) -> VmResult<()> {
    match target {
        SortTarget::InPlace { object, length } => {
            let item_count = collected.count as f64 + collected.undefined;
            // Steps 6 to 9: write the sorted values, delete the rest.
            let mut j = 0.0;
            emit_sorted(rt, collected, order, |rt, values| {
                rt.charge(values.len())?;
                if write_dense(rt, object, j, values)? {
                    j += values.len() as f64;
                    return Ok(());
                }
                for &value in values {
                    rt.with_scope(|rt| rt.set_index(object, j, value))?;
                    j += 1.0;
                }
                Ok(())
            })?;
            rt.delete_range(object, item_count, length, true)
        }
        SortTarget::Copy(result) => emit_sorted(rt, collected, order, |rt, values| {
            rt.charge(values.len())?;
            if rt.heap.extend_elements(result, values)? {
                return Ok(());
            }
            for &value in values {
                rt.push_value(result, value)?;
            }
            Ok(())
        }),
    }
}

/// The sort with a comparator as a native loop (`vm/native_loop.rs`): each
/// comparison of the [`SortMachine`] is a call of the comparator; after
/// the last one the values are written. The registers hold the values,
/// the comparator and the target.
struct SortLoop {
    collected: Collected,
    comparator: Value,
    machine: SortMachine,
    target: SortTarget,
    /// Whether a comparison's call runs.
    pending: bool,
}

impl NativeLoop for SortLoop {
    fn step(&mut self, rt: &mut Runtime, _regs: usize, input: Value) -> VmResult<LoopStep> {
        let answer = if self.pending {
            self.pending = false;
            // NaN is +0 (`CompareArrayElements` step 4.b).
            Some(match input {
                Value::Int(int) => int < 0,
                Value::Double(double) => double < 0.0,
                _ => rt.to_number(input)? < 0.0,
            })
        } else {
            None
        };
        let Some((later, earlier)) = self.machine.advance(answer) else {
            let order = std::mem::take(&mut self.machine.order);
            write_sorted(rt, &self.collected, &order, self.target)?;
            return Ok(LoopStep::Done(self.target.value()));
        };
        let items = self.collected.items;
        let (x, y) = (item(rt, items, later)?, item(rt, items, earlier)?);
        self.pending = true;
        rt.push_call(self.comparator, Value::Undefined, &[x, y])?;
        Ok(LoopStep::Call)
    }
}

/// `Set(object, at + i, values[i], true)` for each `i` in the dense vector
/// of an array, when that gives the result of the steps (the elements
/// exist, or the array is extensible); false if it does not apply.
fn write_dense(rt: &mut Runtime, object: Gc<Object>, at: f64, values: &[Value]) -> VmResult<bool> {
    let Some(state) = rt.heap.dense_array(object)? else {
        return Ok(false);
    };
    if at as usize + values.len() > state.stored_len || (state.holes && !state.extensible) {
        return Ok(false);
    }
    Ok(rt.heap.overwrite_dense(object, at as usize, values)?)
}

/// `Array.prototype.toSorted(comparefn)` (§23.1.3.35).
pub(super) fn to_sorted(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let comparator = comparator_arg(rt, call)?;
    let (object, length) = receiver(rt, call, None)?;
    crate::vm::convert::checked_array_length(length)?;
    let result =
        rt.new_array_with_capacity((length as usize).min(super::array::INITIAL_CAPACITY))?;
    let collected = collect(rt, object, length, true)?;
    finish_or_loop(rt, collected, comparator, SortTarget::Copy(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_sort_is_stable_and_complete() {
        let keys = [3, 1, 2, 1, 3, 0, 2, 1];
        let mut order: Vec<u32> = (0..keys.len() as u32).collect();
        let mut comparisons = 0;
        merge_sort(&mut order, |later, earlier| -> Result<bool, ()> {
            comparisons += 1;
            Ok(keys[later as usize] < keys[earlier as usize])
        })
        .unwrap();
        assert_eq!(order, [5, 1, 3, 7, 2, 6, 0, 4]);
        assert!(comparisons <= 8 * 3);
    }

    #[test]
    fn merge_sort_survives_inconsistent_answers() {
        for n in 0..70u32 {
            let mut order: Vec<u32> = (0..n).collect();
            let mut state = 12345u32;
            merge_sort(&mut order, |_, _| -> Result<bool, ()> {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                Ok(state & 0x100 != 0)
            })
            .unwrap();
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..n).collect::<Vec<_>>());
        }
    }

    #[test]
    fn merge_sort_counts_comparisons() {
        for n in [1usize, 2, 3, 100, 1000, 4097] {
            let mut order: Vec<u32> = (0..n as u32).rev().collect();
            let mut comparisons = 0usize;
            merge_sort(&mut order, |later, earlier| -> Result<bool, ()> {
                comparisons += 1;
                Ok(later < earlier)
            })
            .unwrap();
            assert!(order.windows(2).all(|w| w[0] < w[1]));
            let bound = n * n.bit_width() as usize;
            assert!(comparisons <= bound, "{n}: {comparisons} > {bound}");
        }
    }

    #[test]
    fn sorted_input_costs_one_comparison_per_element() {
        for n in [2usize, 8, 16, 17, 100, 4097] {
            let mut order: Vec<u32> = (0..n as u32).collect();
            let mut comparisons = 0usize;
            merge_sort(&mut order, |later, earlier| -> Result<bool, ()> {
                comparisons += 1;
                Ok(later < earlier)
            })
            .unwrap();
            assert!(order.windows(2).all(|w| w[0] < w[1]));
            assert_eq!(comparisons, n - 1, "{n}");
        }
    }

    /// The comparisons and the result of the machine are those of
    /// `merge_sort`, for consistent and for random answers.
    #[test]
    fn sort_machine_follows_merge_sort() {
        let mut seed = 0x9e37_79b9u32;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for n in (0..70).chain([100, 255, 256, 257, 1000, 4097]) {
            for mode in 0..3 {
                let keys: Vec<u32> = (0..n).map(|_| random() % 50).collect();
                let answers: Vec<bool> = (0..n * 20).map(|_| random() & 1 == 1).collect();
                let answer = |count: usize, later: u32, earlier: u32| match mode {
                    0 => keys[later as usize] < keys[earlier as usize],
                    1 => answers[count % answers.len().max(1)],
                    _ => later > earlier,
                };
                let mut expected_calls = Vec::new();
                let mut order: Vec<u32> = (0..n as u32).collect();
                merge_sort(&mut order, |later, earlier| -> Result<bool, ()> {
                    let before = answer(expected_calls.len(), later, earlier);
                    expected_calls.push((later, earlier));
                    Ok(before)
                })
                .unwrap();
                let mut machine = SortMachine::new(n);
                let mut calls = Vec::new();
                let mut reply = None;
                while let Some((later, earlier)) = machine.advance(reply) {
                    reply = Some(answer(calls.len(), later, earlier));
                    calls.push((later, earlier));
                }
                assert_eq!(calls, expected_calls, "n {n} mode {mode}");
                assert_eq!(machine.order(), &order[..], "n {n} mode {mode}");
            }
        }
    }

    #[test]
    fn int_strings_order() {
        let mut values = [10, 9, 1, 100, -1, -10, -2, 0, i32::MIN, i32::MAX];
        values.sort_by(|&a, &b| compare_int_strings(a, b));
        assert_eq!(values, [-1, -10, -2, i32::MIN, 0, 1, 10, 100, i32::MAX, 9]);
        // Against the strings, for pseudo-random values of all lengths.
        let mut state = 0x1234_5678u32;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as i32) >> (state % 31)
        };
        for _ in 0..20_000 {
            let (a, b) = (next(), next());
            assert_eq!(
                compare_int_strings(a, b),
                a.to_string().cmp(&b.to_string()),
                "{a} {b}"
            );
        }
    }
}
