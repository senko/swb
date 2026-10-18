//! The methods of `%TypedArray%.prototype` that read the elements without
//! a callback (ECMA-262 §23.2.3,
//! <https://tc39.es/ecma262/#sec-properties-of-the-%typedarrayprototype%-object>):
//! `at`, `includes`, `indexOf`, `lastIndexOf`, `join` and
//! `toLocaleString`.
//!
//! The searches compare the bits of the elements in the buffer without
//! making values: the search value becomes a [`Needle`] once (the bits of
//! an integer or `BigInt` element, a Number for the float types, or
//! nothing when no element can equal it), and the scan runs over chunks of
//! [`CHARGE_CHUNK`] elements with a charge of the time countdown after each.
//! The length is read before the conversion of `fromIndex`, which can
//! shrink or detach the buffer: `indexOf` and `lastIndexOf` skip the
//! indices that are no longer valid (`HasProperty`), `includes` reads them
//! as `undefined` (`Get`), so only a search for `undefined` finds them.
//! `join` converts the separator after the length is read, too, and
//! writes the empty string for an invalid index.

use crate::object::{ElementType, RawValue, TypedArrayData, decode_bits, load, mask};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::{NativeCall, NativeReturn, VmResult};

use swb_js_text::String16;
use swb_js_text::number::number_to_string;

use super::string::{Builder, string_result};
use super::typed_array_create::CHARGE_CHUNK;
use super::typed_array_method::validate_this;

// --- at (§23.2.3.1) ---

/// `%TypedArray%.prototype.at(index)`.
pub(super) fn at(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data, length) = validate_this(rt, call, "at")?;
    let relative = to_integer_or_infinity(rt.to_number(rt.arg(call, 0))?);
    let k = if relative >= 0.0 {
        relative
    } else {
        length as f64 + relative
    };
    if k < 0.0 || k >= length as f64 {
        return Ok(NativeReturn::Value(Value::Undefined));
    }
    Ok(NativeReturn::Value(rt.typed_element(&data, k as usize)?))
}

// --- The searches ---

/// What an element must equal to match the search value.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Needle {
    /// No element can match.
    Never,
    /// The bits of an integer or `BigInt` element.
    Bits(u64),
    /// The value of a float element; with `nan`, a NaN element matches
    /// (`SameValueZero`).
    Float { value: f64, nan: bool },
}

/// The range of the integers an element of an integer type can hold.
fn int_range(ty: ElementType) -> (f64, f64) {
    match ty {
        ElementType::Int8 => (-128.0, 127.0),
        ElementType::Int16 => (-32768.0, 32767.0),
        ElementType::Int32 => (-2_147_483_648.0, 2_147_483_647.0),
        ElementType::Uint8 | ElementType::Uint8Clamped => (0.0, 255.0),
        ElementType::Uint16 => (0.0, 65535.0),
        _ => (0.0, 4_294_967_295.0),
    }
}

impl Runtime {
    /// The needle for `search` among elements of type `ty`; `same_value_zero`
    /// for `includes` (a NaN search matches NaN elements).
    fn needle(&self, ty: ElementType, search: Value, same_value_zero: bool) -> VmResult<Needle> {
        if ty.is_bigint() {
            let Value::BigInt(bigint) = search else {
                return Ok(Needle::Never);
            };
            let bigint = self.heap.bigint(bigint)?;
            let bits = if ty == ElementType::BigInt64 {
                bigint.to_i64().map(|int| int as u64)
            } else if bigint.is_negative() {
                None
            } else {
                match bigint.limbs() {
                    [] => Some(0),
                    [limb] => Some(*limb),
                    _ => None,
                }
            };
            return Ok(bits.map_or(Needle::Never, Needle::Bits));
        }
        let Some(number) = search.as_number() else {
            return Ok(Needle::Never);
        };
        if !matches!(
            ty,
            ElementType::Float16 | ElementType::Float32 | ElementType::Float64
        ) {
            let (low, high) = int_range(ty);
            if number.fract() != 0.0 || number < low || number > high {
                return Ok(Needle::Never);
            }
            return Ok(Needle::Bits(number as i64 as u64 & mask(ty.size())));
        }
        Ok(Needle::Float {
            value: number,
            nan: same_value_zero && number.is_nan(),
        })
    }
}

impl Needle {
    /// Whether the element with these bits matches.
    fn matches(self, ty: ElementType, bits: u64) -> bool {
        match self {
            Needle::Never => false,
            Needle::Bits(wanted) => bits == wanted,
            Needle::Float { value, nan } => match decode_bits(ty, bits) {
                RawValue::Number(element) => element == value || (nan && element.is_nan()),
                _ => false,
            },
        }
    }
}

/// The first (last, with `backwards`) index in `range` whose element
/// matches, looking at the window of the array now; the time is charged
/// for each chunk. The range is clamped to the current length.
fn scan(
    rt: &mut Runtime,
    data: &TypedArrayData,
    needle: Needle,
    range: (usize, usize),
    backwards: bool,
) -> VmResult<Option<usize>> {
    if needle == Needle::Never {
        return Ok(None);
    }
    let (ty, size) = (data.ty, data.ty.size());
    let (mut low, mut high) = range;
    while low < high {
        let (from, to) = if backwards {
            (high.saturating_sub(CHARGE_CHUNK).max(low), high)
        } else {
            (low, (low + CHARGE_CHUNK).min(high))
        };
        let Some(window) = rt.heap.typed_window(data)? else {
            return Ok(None);
        };
        let part = window.get(from * size..to * size).unwrap_or_default();
        if let Some(offset) = find_in(part, ty, needle, backwards) {
            return Ok(Some(from + offset));
        }
        rt.charge_bytes(part.len())?;
        if backwards {
            high = from;
        } else {
            low = to;
        }
    }
    Ok(None)
}

/// The first (last, with `backwards`) item of `items` that `test` accepts.
fn find_by<T>(items: &[T], backwards: bool, test: impl Fn(&T) -> bool) -> Option<usize> {
    if backwards {
        items.iter().rposition(test)
    } else {
        items.iter().position(test)
    }
}

/// The index of the first (last) element in `part` (the bytes of whole
/// elements of type `ty`) that matches. The integer and float types of 1,
/// 2, 4 and 8 bytes compare in typed loops that the compiler can
/// vectorize; `Float16Array` decodes each element.
fn find_in(part: &[u8], ty: ElementType, needle: Needle, backwards: bool) -> Option<usize> {
    match (needle, ty.size()) {
        (Needle::Never, _) => None,
        (Needle::Bits(wanted), 1) => find_by(part, backwards, |&byte| u64::from(byte) == wanted),
        (Needle::Bits(wanted), 2) => find_by(part.as_chunks::<2>().0, backwards, |chunk| {
            u64::from(u16::from_le_bytes(*chunk)) == wanted
        }),
        (Needle::Bits(wanted), 4) => find_by(part.as_chunks::<4>().0, backwards, |chunk| {
            u64::from(u32::from_le_bytes(*chunk)) == wanted
        }),
        (Needle::Bits(wanted), 8) => find_by(part.as_chunks::<8>().0, backwards, |chunk| {
            u64::from_le_bytes(*chunk) == wanted
        }),
        (Needle::Float { value, nan }, _) if ty == ElementType::Float32 => {
            find_by(part.as_chunks::<4>().0, backwards, |chunk| {
                let element = f64::from(f32::from_le_bytes(*chunk));
                element == value || (nan && element.is_nan())
            })
        }
        (Needle::Float { value, nan }, _) if ty == ElementType::Float64 => {
            find_by(part.as_chunks::<8>().0, backwards, |chunk| {
                let element = f64::from_le_bytes(*chunk);
                element == value || (nan && element.is_nan())
            })
        }
        _ => {
            let test = |chunk: &[u8]| needle.matches(ty, load(chunk).unwrap_or(0));
            if backwards {
                part.chunks_exact(ty.size()).rposition(test)
            } else {
                part.chunks_exact(ty.size()).position(test)
            }
        }
    }
}

/// The current length of the array for the loops after script code ran
/// (0 when it is out of bounds).
fn current_length(rt: &Runtime, data: &TypedArrayData) -> VmResult<usize> {
    Ok(rt.heap.typed_length(data)?.unwrap_or(0))
}

/// The start index of `includes` and `indexOf` from `fromIndex`
/// (`ToIntegerOrInfinity`, from the end if negative); `None` for +∞.
fn forward_start(rt: &mut Runtime, from_index: Value, length: usize) -> VmResult<Option<usize>> {
    let n = match from_index {
        Value::Int(int) => f64::from(int),
        value => to_integer_or_infinity(rt.to_number(value)?),
    };
    if n == f64::INFINITY {
        return Ok(None);
    }
    let k = if n >= 0.0 {
        n
    } else {
        (length as f64 + n).max(0.0)
    };
    Ok(Some(k as usize))
}

/// `%TypedArray%.prototype.includes(searchElement [, fromIndex])`
/// (§23.2.3.16).
pub(super) fn includes(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data, length) = validate_this(rt, call, "includes")?;
    if length == 0 {
        return Ok(NativeReturn::Value(Value::Bool(false)));
    }
    let Some(start) = forward_start(rt, rt.arg(call, 1), length)? else {
        return Ok(NativeReturn::Value(Value::Bool(false)));
    };
    let search = rt.arg(call, 0);
    let current = current_length(rt, &data)?;
    // An index that is no longer valid reads as `undefined`.
    let beyond = start < length && current < length;
    if search.is_undefined() {
        return Ok(NativeReturn::Value(Value::Bool(beyond)));
    }
    let needle = rt.needle(data.ty, search, true)?;
    let end = length.min(current);
    let found = scan(rt, &data, needle, (start.min(end), end), false)?;
    Ok(NativeReturn::Value(Value::Bool(found.is_some())))
}

/// `%TypedArray%.prototype.indexOf(searchElement [, fromIndex])`
/// (§23.2.3.17).
pub(super) fn index_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data, length) = validate_this(rt, call, "indexOf")?;
    if length == 0 {
        return Ok(NativeReturn::Value(Value::Int(-1)));
    }
    let Some(start) = forward_start(rt, rt.arg(call, 1), length)? else {
        return Ok(NativeReturn::Value(Value::Int(-1)));
    };
    let needle = rt.needle(data.ty, rt.arg(call, 0), false)?;
    let end = length.min(current_length(rt, &data)?);
    let found = scan(rt, &data, needle, (start.min(end), end), false)?;
    Ok(NativeReturn::Value(found.map_or(
        Value::Int(-1),
        super::typed_array_method::position,
    )))
}

/// `%TypedArray%.prototype.lastIndexOf(searchElement [, fromIndex])`
/// (§23.2.3.20).
pub(super) fn last_index_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data, length) = validate_this(rt, call, "lastIndexOf")?;
    let missing = NativeReturn::Value(Value::Int(-1));
    if length == 0 {
        return Ok(missing);
    }
    let n = if call.argc() > 1 {
        match rt.arg(call, 1) {
            Value::Int(int) => f64::from(int),
            value => to_integer_or_infinity(rt.to_number(value)?),
        }
    } else {
        length as f64 - 1.0
    };
    if n == f64::NEG_INFINITY {
        return Ok(missing);
    }
    let k = if n >= 0.0 {
        n.min(length as f64 - 1.0)
    } else {
        length as f64 + n
    };
    if k < 0.0 {
        return Ok(missing);
    }
    let needle = rt.needle(data.ty, rt.arg(call, 0), false)?;
    // The indices from `k` down to 0 that are still valid.
    let end = (k as usize + 1).min(current_length(rt, &data)?);
    let found = scan(rt, &data, needle, (0, end), true)?;
    Ok(NativeReturn::Value(found.map_or(
        Value::Int(-1),
        super::typed_array_method::position,
    )))
}

// --- join (§23.2.3.18) and toLocaleString (§23.2.3.31) ---

/// The elements that `join` formats between two checks of the room.
const JOIN_BLOCK: usize = 4096;

/// The decimal digits of `n` (with a minus sign) in the end of `buf`; the
/// text is the returned part.
fn decimal(buf: &mut [u8; 24], negative: bool, mut n: u64) -> &[u8] {
    let mut at = buf.len();
    loop {
        at -= 1;
        if let Some(slot) = buf.get_mut(at) {
            *slot = b'0' + (n % 10) as u8;
        }
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if negative {
        at -= 1;
        if let Some(slot) = buf.get_mut(at) {
            *slot = b'-';
        }
    }
    buf.get(at..).unwrap_or_default()
}

/// Appends the text of the element `bits` of type `ty` (`ToString` of its
/// Number or `BigInt`).
fn push_element(
    builder: &mut Builder,
    rt: &mut Runtime,
    ty: ElementType,
    bits: u64,
) -> VmResult<()> {
    let mut buf = [0u8; 24];
    let owned;
    let text: &[u8] = match decode_bits(ty, bits) {
        RawValue::Int(int) => decimal(&mut buf, int < 0, u64::from(int.unsigned_abs())),
        RawValue::BigInt64(int) => decimal(&mut buf, int < 0, int.unsigned_abs()),
        RawValue::BigUint64(int) => decimal(&mut buf, false, int),
        RawValue::Number(number) => {
            owned = number_to_string(number);
            owned.as_bytes()
        }
    };
    builder.check(rt, text.len())?;
    let out = builder.text_mut();
    for &byte in text {
        out.push(u16::from(byte));
    }
    Ok(())
}

/// `%TypedArray%.prototype.join(separator)`.
pub(super) fn join(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data, length) = validate_this(rt, call, "join")?;
    let separator = match rt.arg(call, 0) {
        Value::Undefined => String16::from(","),
        value => {
            let string = rt.to_string(value)?;
            let units = rt.heap.copy_string(string)?;
            rt.charge_units(units.len())?;
            units
        }
    };
    let current = current_length(rt, &data)?;
    let mut builder = Builder::new();
    // The shortest text: one unit per element that is still there and the
    // separators. Failing here is V8's `RangeError` or the heap limit,
    // before any work.
    let separators = length.saturating_sub(1).saturating_mul(separator.len());
    builder.check(rt, current.min(length).saturating_add(separators))?;
    let size = data.ty.size();
    let mut block: Vec<u64> = Vec::new();
    for start in (0..length).step_by(JOIN_BLOCK) {
        let end = (start + JOIN_BLOCK).min(length);
        block.clear();
        if let Some(window) = rt.heap.typed_window(&data)? {
            let valid = end.min(current).saturating_sub(start.min(current));
            block.extend(
                window
                    .chunks_exact(size)
                    .skip(start)
                    .take(valid)
                    .map(|chunk| load(chunk).unwrap_or(0)),
            );
        }
        let before = builder.len();
        for k in start..end {
            if k > 0 {
                builder.check(rt, separator.len())?;
                builder.push_text(separator.as_str16());
            }
            if let Some(&bits) = block.get(k - start) {
                push_element(&mut builder, rt, data.ty, bits)?;
            }
        }
        rt.charge_units(builder.len() - before)?;
        rt.tick()?;
    }
    let string = builder.finish(rt)?;
    string_result(string)
}

/// `%TypedArray%.prototype.toLocaleString()`: the `toLocaleString` of each
/// element, joined with "," (no ECMA-402: the calls get no arguments, as
/// in `Array.prototype.toLocaleString`).
pub(super) fn to_locale_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, data, length) = validate_this(rt, call, "toLocaleString")?;
    let key = rt.heap.key_from_str("toLocaleString")?;
    let mut builder = Builder::new();
    builder.check(rt, length.saturating_sub(1))?;
    for k in 0..length {
        rt.tick()?;
        rt.with_scope(|rt| {
            if k > 0 {
                builder.check(rt, 1)?;
                builder.push_unit(u16::from(b','));
            }
            let element = rt.typed_value(&data, k)?;
            if element.is_nullish() {
                return Ok(());
            }
            let method = rt.get_value(element, key)?;
            if !rt.is_callable(method)? {
                return Err(rt.not_callable(method));
            }
            let result = rt.call(method, element, &[])?;
            let string = rt.to_string(result)?;
            builder.push_string(rt, string)
        })?;
    }
    let string = builder.finish(rt)?;
    string_result(string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_text() {
        let mut buf = [0u8; 24];
        assert_eq!(decimal(&mut buf, false, 0), b"0");
        assert_eq!(decimal(&mut buf, true, 12345), b"-12345");
        assert_eq!(decimal(&mut buf, false, u64::MAX), b"18446744073709551615");
        assert_eq!(
            decimal(&mut buf, true, i64::MIN.unsigned_abs()),
            b"-9223372036854775808"
        );
    }

    #[test]
    fn masks_and_ranges() {
        assert_eq!(mask(1), 0xff);
        assert_eq!(mask(4), 0xffff_ffff);
        assert_eq!(mask(8), u64::MAX);
        assert_eq!(int_range(ElementType::Int8), (-128.0, 127.0));
        assert_eq!(int_range(ElementType::Uint32), (0.0, 4_294_967_295.0));
    }

    #[test]
    fn needles_compare_bits_or_values() {
        let int = Needle::Bits(0xff);
        assert!(int.matches(ElementType::Int8, 0xff));
        assert!(!int.matches(ElementType::Int8, 0x7f));
        let nan = Needle::Float {
            value: f64::NAN,
            nan: true,
        };
        assert!(nan.matches(ElementType::Float64, f64::NAN.to_bits()));
        let strict = Needle::Float {
            value: f64::NAN,
            nan: false,
        };
        assert!(!strict.matches(ElementType::Float64, f64::NAN.to_bits()));
        let zero = Needle::Float {
            value: 0.0,
            nan: false,
        };
        assert!(zero.matches(ElementType::Float64, (-0.0f64).to_bits()));
        assert!(!Needle::Never.matches(ElementType::Int8, 0));
    }
}
