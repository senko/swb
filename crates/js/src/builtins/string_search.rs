//! Searching in strings (ECMA-262 §22.1.3): `indexOf`, `lastIndexOf`,
//! `includes`, `startsWith` and `endsWith`, `StringIndexOf` (§6.1.4.1 in
//! the 2025 text) for the other modules, and `IsRegExp` (§7.2.8).
//!
//! The search works on the flat code units of both strings (a rope is
//! flattened first). A needle of fewer than [`KMP_FROM`] units uses a
//! scan for its first unit and a comparison of the rest, whose cost is at
//! most [`KMP_FROM`] comparisons per position. A longer needle uses the
//! prefix-function search of Knuth, Morris and Pratt (1977), linear in
//! the sum of the lengths, so that a hostile pair such as `'a' * n` and
//! `'a' * (n / 2) + 'b'` costs no more than a plain scan. `lastIndexOf`
//! runs the same search over the mirrored strings. Both widths of
//! both strings are searched without conversion; a wide needle with a
//! unit above 255 is never in a narrow string. The work is charged to
//! the time countdown in proportion to the units scanned and compared
//! ([`Meter`]), also inside one long search.

use swb_js_text::Str16;

use crate::error::Termination;
use crate::heap::{Gc, Heap};
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::Value;
use crate::vm::{NativeCall, NativeReturn, Vm, VmError, VmResult, WellKnown};

use super::string::clamp_index;

/// The needle length from which the search uses the prefix function.
const KMP_FROM: usize = 16;

/// The work points that make one step of the time countdown.
const POINTS_PER_STEP: usize = 64;

/// The work points after which [`Meter`] charges the time.
const FLUSH_POINTS: usize = 4096;

/// The work points of a comparison of two units (a scan step costs 1).
const COMPARE_POINTS: usize = 4;

/// The units that a scan covers between two charges of the time.
const SCAN_WINDOW: usize = 4096;

/// Charges the time for the work of a search: the VM's countdown and the
/// heap (for the table of the prefix function).
pub(super) struct Meter<'a> {
    vm: &'a mut Vm,
    heap: &'a Heap,
    points: usize,
}

impl<'a> Meter<'a> {
    /// A meter with no work yet.
    pub(super) fn new(vm: &'a mut Vm, heap: &'a Heap) -> Self {
        Meter {
            vm,
            heap,
            points: 0,
        }
    }

    /// Adds work; charges the time once enough has accumulated.
    #[inline]
    fn add(&mut self, points: usize) -> VmResult<()> {
        self.points += points;
        if self.points >= FLUSH_POINTS {
            return self.flush();
        }
        Ok(())
    }

    /// Charges the accumulated work.
    pub(super) fn flush(&mut self) -> VmResult<()> {
        let steps = self.points / POINTS_PER_STEP;
        self.points %= POINTS_PER_STEP;
        Ok(self.vm.charge(steps)?)
    }
}

/// A code unit of either width.
trait Unit: Copy {
    fn unit(self) -> u16;
}

impl Unit for u8 {
    #[inline]
    fn unit(self) -> u16 {
        u16::from(self)
    }
}

impl Unit for u16 {
    #[inline]
    fn unit(self) -> u16 {
        self
    }
}

/// `StringIndexOf(string, searchValue, fromIndex)`: the first position at
/// or after `from` where `needle` occurs. An empty needle is found at
/// `from` if `from` is not past the end.
pub(super) fn find(
    hay: Str16<'_>,
    needle: Str16<'_>,
    from: usize,
    meter: &mut Meter<'_>,
) -> VmResult<Option<usize>> {
    let (hay_len, needle_len) = (hay.len(), needle.len());
    if needle_len == 0 {
        return Ok((from <= hay_len).then_some(from));
    }
    if from > hay_len || needle_len > hay_len - from {
        return Ok(None);
    }
    search_str::<false>(hay, needle, from, meter)
}

/// The last position at or before `from` where `needle` occurs (the loop
/// of `lastIndexOf`, §22.1.3.12).
pub(super) fn find_last(
    hay: Str16<'_>,
    needle: Str16<'_>,
    from: usize,
    meter: &mut Meter<'_>,
) -> VmResult<Option<usize>> {
    let (hay_len, needle_len) = (hay.len(), needle.len());
    if needle_len > hay_len {
        return Ok(None);
    }
    let start = from.min(hay_len - needle_len);
    // In the mirrored strings the match at `s` starts at `hay_len - needle_len - s`.
    let mirrored = hay_len - needle_len - start;
    if needle_len == 0 {
        return Ok(Some(start));
    }
    let found = search_str::<true>(hay, needle, mirrored, meter)?;
    Ok(found.map(|position| hay_len - needle_len - position))
}

/// Dispatches on the widths. `REV` searches the mirrored strings.
fn search_str<const REV: bool>(
    hay: Str16<'_>,
    needle: Str16<'_>,
    from: usize,
    meter: &mut Meter<'_>,
) -> VmResult<Option<usize>> {
    match (hay, needle) {
        (Str16::Latin1(h), Str16::Latin1(n)) => search_slices::<u8, u8, REV>(h, n, from, meter),
        (Str16::Latin1(h), Str16::Wide(n)) => {
            if n.iter().any(|&unit| unit > 255) {
                return Ok(None);
            }
            search_slices::<u8, u16, REV>(h, n, from, meter)
        }
        (Str16::Wide(h), Str16::Latin1(n)) => search_slices::<u16, u8, REV>(h, n, from, meter),
        (Str16::Wide(h), Str16::Wide(n)) => search_slices::<u16, u16, REV>(h, n, from, meter),
    }
}

/// [`search`] over two slices, mirrored when `REV`.
fn search_slices<A: Unit, B: Unit, const REV: bool>(
    hay: &[A],
    needle: &[B],
    from: usize,
    meter: &mut Meter<'_>,
) -> VmResult<Option<usize>> {
    let (hay_len, needle_len) = (hay.len(), needle.len());
    let hay_at = |i: usize| -> u16 {
        if REV {
            hay[hay_len - 1 - i].unit()
        } else {
            hay[i].unit()
        }
    };
    let pat_at = |j: usize| -> u16 {
        if REV {
            needle[needle_len - 1 - j].unit()
        } else {
            needle[j].unit()
        }
    };
    search(hay_len, needle_len, from, hay_at, pat_at, meter)
}

/// The first position `>= from` where the needle (`needle_len >= 1`
/// units, read by `n`) occurs in the haystack (read by `h`); the caller
/// checked that `from + needle_len <= hay_len`.
fn search(
    hay_len: usize,
    needle_len: usize,
    from: usize,
    hay_at: impl Fn(usize) -> u16,
    pat_at: impl Fn(usize) -> u16,
    meter: &mut Meter<'_>,
) -> VmResult<Option<usize>> {
    let result = if needle_len < KMP_FROM {
        search_plain(hay_len, needle_len, from, &hay_at, &pat_at, meter)
    } else {
        search_prefix_function(hay_len, needle_len, from, &hay_at, &pat_at, meter)
    };
    meter.flush()?;
    result
}

/// The scan for the first unit, then the comparison of the rest.
fn search_plain(
    hay_len: usize,
    needle_len: usize,
    from: usize,
    hay_at: &impl Fn(usize) -> u16,
    pat_at: &impl Fn(usize) -> u16,
    meter: &mut Meter<'_>,
) -> VmResult<Option<usize>> {
    let first = pat_at(0);
    let last_start = hay_len - needle_len;
    let mut i = from;
    while i <= last_start {
        let window_end = last_start.min(i + SCAN_WINDOW - 1);
        let mut j = i;
        while j <= window_end && hay_at(j) != first {
            j += 1;
        }
        meter.add(j - i)?;
        if j > window_end {
            i = window_end + 1;
            continue;
        }
        let mut k = 1;
        while k < needle_len && hay_at(j + k) == pat_at(k) {
            k += 1;
        }
        meter.add(k * COMPARE_POINTS)?;
        if k == needle_len {
            return Ok(Some(j));
        }
        i = j + 1;
    }
    Ok(None)
}

/// The search of Knuth, Morris and Pratt: the table of the longest proper
/// border of each prefix of the needle, then one pass over the haystack.
fn search_prefix_function(
    hay_len: usize,
    needle_len: usize,
    from: usize,
    hay_at: &impl Fn(usize) -> u16,
    pat_at: &impl Fn(usize) -> u16,
    meter: &mut Meter<'_>,
) -> VmResult<Option<usize>> {
    meter.heap.check_room(needle_len.saturating_mul(4))?;
    let mut border: Vec<u32> = Vec::new();
    border
        .try_reserve_exact(needle_len)
        .map_err(|_| VmError::from(Termination::OutOfMemory))?;
    border.resize(needle_len, 0);
    let mut k = 0usize;
    for i in 1..needle_len {
        let unit = pat_at(i);
        while k > 0 && unit != pat_at(k) {
            k = border[k - 1] as usize;
        }
        if unit == pat_at(k) {
            k += 1;
        }
        border[i] = k as u32;
        meter.add(2 * COMPARE_POINTS)?;
    }
    let mut k = 0usize;
    for i in from..hay_len {
        let unit = hay_at(i);
        while k > 0 && unit != pat_at(k) {
            k = border[k - 1] as usize;
        }
        if unit == pat_at(k) {
            k += 1;
            if k == needle_len {
                return Ok(Some(i + 1 - needle_len));
            }
        }
        meter.add(2 * COMPARE_POINTS)?;
    }
    Ok(None)
}

impl Runtime {
    /// `StringIndexOf` of two strings (flattened first: a safepoint, so
    /// the caller roots both).
    pub(super) fn string_index_of(
        &mut self,
        hay: Gc<JsString>,
        needle: Gc<JsString>,
        from: usize,
    ) -> VmResult<Option<usize>> {
        self.flatten(hay)?;
        self.flatten(needle)?;
        let hay = self.heap.text(hay)?;
        let needle = self.heap.text(needle)?;
        let mut meter = Meter::new(&mut self.vm, &self.heap);
        find(hay, needle, from, &mut meter)
    }

    /// `IsRegExp(argument)` (§7.2.8): `@@match`, else whether the object
    /// has `[[RegExpMatcher]]`.
    pub(super) fn is_regexp(&mut self, argument: Value) -> VmResult<bool> {
        if !matches!(argument, Value::Object(_)) {
            return Ok(false);
        }
        let key = self.symbol_key(WellKnown::Match);
        let matcher = self.get_value(argument, key)?;
        if !matcher.is_undefined() {
            return self.to_boolean(matcher);
        }
        Ok(self.regexp_data(argument)?.is_some())
    }

    /// `ToString` of the search string of `includes`, `startsWith` and
    /// `endsWith`, after the `IsRegExp` check (V8's message).
    fn search_string_arg(&mut self, call: &NativeCall, method: &str) -> VmResult<Gc<JsString>> {
        let value = self.arg(call, 0);
        if self.is_regexp(value)? {
            return Err(VmError::type_error(format!(
                "First argument to String.prototype.{method} must not be a regular expression"
            )));
        }
        self.to_string(value)
    }

    /// Whether `needle` occurs in `hay` at `start` (both in range).
    fn region_matches(
        &mut self,
        hay: Gc<JsString>,
        needle: Gc<JsString>,
        start: usize,
    ) -> VmResult<bool> {
        self.flatten(hay)?;
        self.flatten(needle)?;
        let needle_text = self.heap.text(needle)?;
        let count = needle_text.len();
        let hay_text = self.heap.text(hay)?;
        let equal = hay_text
            .slice(start, start + count)
            .is_some_and(|region| region == needle_text);
        self.charge_units(count)?;
        Ok(equal)
    }
}

/// A position as a number result.
fn position_result(position: Option<usize>) -> NativeReturn {
    NativeReturn::Value(position.map_or(Value::Int(-1), |p| Value::number(p as f64)))
}

/// `String.prototype.indexOf(searchString, position)` (§22.1.3.9).
pub(super) fn index_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.indexOf")?;
    let search = rt.string_arg(call, 0)?;
    let position = rt.integer_arg(call, 1)?;
    let length = rt.string_len(string)?;
    let start = clamp_index(position, length);
    Ok(position_result(rt.string_index_of(string, search, start)?))
}

/// `String.prototype.lastIndexOf(searchString, position)` (§22.1.3.12).
pub(super) fn last_index_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.lastIndexOf")?;
    let search = rt.string_arg(call, 0)?;
    let value = rt.arg(call, 1);
    let number = rt.to_number(value)?;
    let position = if number.is_nan() {
        f64::INFINITY
    } else {
        crate::vm::convert::to_integer_or_infinity(number)
    };
    let length = rt.string_len(string)?;
    let start = clamp_index(position, length);
    rt.flatten(string)?;
    rt.flatten(search)?;
    let hay = rt.heap.text(string)?;
    let needle = rt.heap.text(search)?;
    let mut meter = Meter::new(&mut rt.vm, &rt.heap);
    Ok(position_result(find_last(hay, needle, start, &mut meter)?))
}

/// `String.prototype.includes(searchString, position)` (§22.1.3.8).
pub(super) fn includes(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.includes")?;
    let search = rt.search_string_arg(call, "includes")?;
    let position = rt.integer_arg(call, 1)?;
    let length = rt.string_len(string)?;
    let start = clamp_index(position, length);
    let found = rt.string_index_of(string, search, start)?;
    Ok(NativeReturn::Value(Value::Bool(found.is_some())))
}

/// `String.prototype.startsWith(searchString, position)` (§22.1.3.23).
pub(super) fn starts_with(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.startsWith")?;
    let search = rt.search_string_arg(call, "startsWith")?;
    let position = rt.integer_arg(call, 1)?;
    let length = rt.string_len(string)?;
    let start = clamp_index(position, length);
    let count = rt.string_len(search)?;
    let result = if count == 0 {
        true
    } else if start + count > length {
        false
    } else {
        rt.region_matches(string, search, start)?
    };
    Ok(NativeReturn::Value(Value::Bool(result)))
}

/// `String.prototype.endsWith(searchString, endPosition)` (§22.1.3.7).
pub(super) fn ends_with(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.endsWith")?;
    let search = rt.search_string_arg(call, "endsWith")?;
    let length = rt.string_len(string)?;
    let end = match rt.arg(call, 1) {
        Value::Undefined => length,
        _ => clamp_index(rt.integer_arg(call, 1)?, length),
    };
    let count = rt.string_len(search)?;
    let result = if count == 0 {
        true
    } else if count > end {
        false
    } else {
        rt.region_matches(string, search, end - count)?
    };
    Ok(NativeReturn::Value(Value::Bool(result)))
}
