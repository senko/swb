//! `Atomics` (ECMA-262 §25.4, <https://tc39.es/ecma262/#sec-atomics-object>)
//! on integer typed arrays over `ArrayBuffer`s, which no other agent can
//! see: every operation is a plain read, modify and write of the element.
//! Chromium 148 has no `SharedArrayBuffer` on a normal page (cross-origin
//! isolation is off), so `wait` and `waitAsync` always end in the
//! `TypeError` for a buffer that is not shared, and `notify` wakes nobody
//! (returns 0). `pause` (ES2026) returns `undefined` at once.
//!
//! The steps are those of `AtomicReadModifyWrite` (§25.4.3.17):
//! `ValidateIntegerTypedArray`, `ValidateAtomicAccess` (`ToIndex`, a
//! `RangeError` at or past the length), the conversion of the operands
//! (`ToIntegerOrInfinity` and the modular conversion of the element type,
//! or `ToBigInt`), `RevalidateAtomicAccess` (a conversion can detach or
//! shrink the buffer) and the operation. V8's messages (measured in
//! Chromium 148): "X is not an integer typed array." (or "… an int32 or
//! `BigInt64` typed array." for `wait`, `waitAsync` and `notify`), "Invalid
//! atomic access index", and "Cannot perform Atomics.add on a detached or
//! out-of-bounds `ArrayBuffer`" (`waitAsync` says `Atomics.wait` in it).

use crate::object::{Element, ElementType, TypedArrayData, decode_bits, element_bits};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::{NativeCall, NativeFn, NativeReturn, VmError, VmResult};

use super::typed_array_create::out_of_bounds;

/// The functions of `Atomics` in Chromium 148's key order, with their
/// `length`.
const FUNCTIONS: [(&str, u32, NativeFn); 14] = [
    ("load", 2, load),
    ("store", 3, store),
    ("add", 3, |rt, call| modify(rt, call, "add", Op::Add)),
    ("sub", 3, |rt, call| modify(rt, call, "sub", Op::Sub)),
    ("and", 3, |rt, call| modify(rt, call, "and", Op::And)),
    ("or", 3, |rt, call| modify(rt, call, "or", Op::Or)),
    ("xor", 3, |rt, call| modify(rt, call, "xor", Op::Xor)),
    ("exchange", 3, |rt, call| {
        modify(rt, call, "exchange", Op::Exchange)
    }),
    ("compareExchange", 4, compare_exchange),
    ("isLockFree", 1, is_lock_free),
    ("wait", 4, |rt, call| wait(rt, call, "Atomics.wait")),
    ("waitAsync", 4, |rt, call| wait(rt, call, "Atomics.wait")),
    ("notify", 3, notify),
    ("pause", 0, pause),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, crate::vm::Intrinsic::ObjectPrototype)?;
    let atomics = rt.heap.new_object(Some(proto))?;
    rt.heap.record(atomics);
    for (name, length, func) in FUNCTIONS {
        super::method(rt, realm, atomics, name, length, func)?;
    }
    super::to_string_tag(rt, atomics, "Atomics")?;
    super::global(rt, realm, "Atomics", atomics.into())
}

// --- Validation ---

/// `ValidateIntegerTypedArray(value, waitable)` (§25.4.3.1): the slots and
/// the length. `name` is the function's name in the message of a detached
/// or out-of-bounds array. A waitable array is an `Int32Array` or a
/// `BigInt64Array`, the others any integer type except `Uint8ClampedArray`.
fn validate_integer(
    rt: &Runtime,
    value: Value,
    name: &str,
    waitable: bool,
) -> VmResult<(TypedArrayData, usize)> {
    let wrong = |what: &str| {
        VmError::type_error(format!(
            "{} is not {what} typed array.",
            rt.primitive_text(value)
        ))
    };
    let Some((_, data)) = rt.typed_slots(value)? else {
        return Err(wrong(if waitable {
            "an int32 or BigInt64"
        } else {
            "an integer"
        }));
    };
    let Some(length) = rt.heap.typed_length(&data)? else {
        return Err(out_of_bounds(name));
    };
    let fits = if waitable {
        matches!(data.ty, ElementType::Int32 | ElementType::BigInt64)
    } else {
        data.ty.is_integer() || data.ty.is_bigint()
    };
    if !fits {
        return Err(wrong(if waitable {
            "an int32 or BigInt64"
        } else {
            "an integer"
        }));
    }
    Ok((data, length))
}

/// V8's `RangeError` for an index that `ToIndex` rejects or that is not
/// below the length.
fn invalid_access() -> VmError {
    VmError::range_error("Invalid atomic access index")
}

/// `ValidateAtomicAccess(taRecord, requestIndex)` (§25.4.3.2): the element
/// index.
fn validate_access(rt: &mut Runtime, request: Value, length: usize) -> VmResult<usize> {
    let index = rt.index_or_number(request)?.map_err(|_| invalid_access())?;
    usize::try_from(index)
        .ok()
        .filter(|&index| index < length)
        .ok_or_else(invalid_access)
}

/// `RevalidateAtomicAccess(typedArray, byteIndexInBuffer)` (§25.4.3.3)
/// after the conversions of the operands, which can run script code.
fn revalidate(rt: &Runtime, data: &TypedArrayData, index: usize, name: &str) -> VmResult<()> {
    match rt.heap.typed_length(data)? {
        None => Err(out_of_bounds(name)),
        // V8 words an index that is past the shrunk array like an array
        // that is out of bounds.
        Some(length) if index >= length => Err(out_of_bounds(name)),
        Some(_) => Ok(()),
    }
}

// --- Operands ---

/// A converted operand: the bits to store and the value that `store`
/// returns.
struct Operand {
    bits: u64,
    value: Value,
}

/// The conversion of an operand for an element of type `ty`: for the
/// `BigInt` types `ToBigInt` (the value is the `BigInt`, rooted in the
/// current handle scope), else `ToIntegerOrInfinity` of `ToNumber` (the
/// value is the integer, or an infinity) with the modular conversion of
/// the element type for the bits.
fn operand(rt: &mut Runtime, ty: ElementType, value: Value) -> VmResult<Operand> {
    if ty.is_bigint() {
        let bigint = rt.to_bigint(value)?;
        rt.heap.record(bigint);
        let bits = rt.heap.bigint(bigint)?.to_u64_wrapping();
        return Ok(Operand {
            bits,
            value: Value::BigInt(bigint),
        });
    }
    let integer = match value {
        Value::Int(int) => f64::from(int),
        _ => to_integer_or_infinity(rt.to_number(value)?),
    };
    Ok(Operand {
        bits: element_bits(ty, Element::Number(integer)),
        value: Value::number(integer),
    })
}

/// The value of the element bits `bits` of type `ty`, as the operations
/// return it (a new `BigInt` for the `BigInt` types).
fn value_of_bits(rt: &mut Runtime, ty: ElementType, bits: u64) -> VmResult<Value> {
    rt.element_value(decode_bits(ty, bits))
}

/// The current bits of the element `index`, which `revalidate` checked.
fn current_bits(rt: &Runtime, data: &TypedArrayData, index: usize) -> VmResult<u64> {
    rt.heap
        .typed_bits(data, index)?
        .ok_or(VmError::invariant("an atomic access to an invalid element"))
}

// --- The operations ---

/// The read-modify-write operations.
#[derive(Clone, Copy)]
enum Op {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Exchange,
}

impl Op {
    /// The new element bits (the caller keeps the low `size` bytes).
    fn apply(self, old: u64, operand: u64) -> u64 {
        match self {
            Op::Add => old.wrapping_add(operand),
            Op::Sub => old.wrapping_sub(operand),
            Op::And => old & operand,
            Op::Or => old | operand,
            Op::Xor => old ^ operand,
            Op::Exchange => operand,
        }
    }
}

/// `Atomics.add`, `sub`, `and`, `or`, `xor` and `exchange` (§25.4.3.17):
/// the old value.
fn modify(rt: &mut Runtime, call: &NativeCall, name: &str, op: Op) -> VmResult<NativeReturn> {
    let full = format!("Atomics.{name}");
    let (data, length) = validate_integer(rt, rt.arg(call, 0), &full, false)?;
    let index = validate_access(rt, rt.arg(call, 1), length)?;
    let operand = operand(rt, data.ty, rt.arg(call, 2))?;
    revalidate(rt, &data, index, &full)?;
    let old = current_bits(rt, &data, index)?;
    rt.heap
        .typed_store_bits(&data, index, op.apply(old, operand.bits))?;
    Ok(NativeReturn::Value(value_of_bits(rt, data.ty, old)?))
}

/// `Atomics.compareExchange(typedArray, index, expectedValue,
/// replacementValue)` (§25.4.5): stores the replacement if the element
/// equals the expected value (compared as element bits), returns the old
/// value.
fn compare_exchange(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    // V8 names `store` in the messages of a detached array (measured).
    const NAME: &str = "Atomics.store";
    let (data, length) = validate_integer(rt, rt.arg(call, 0), NAME, false)?;
    let index = validate_access(rt, rt.arg(call, 1), length)?;
    let expected = operand(rt, data.ty, rt.arg(call, 2))?;
    let replacement = operand(rt, data.ty, rt.arg(call, 3))?;
    revalidate(rt, &data, index, NAME)?;
    let old = current_bits(rt, &data, index)?;
    if old == expected.bits {
        rt.heap.typed_store_bits(&data, index, replacement.bits)?;
    }
    Ok(NativeReturn::Value(value_of_bits(rt, data.ty, old)?))
}

/// `Atomics.load(typedArray, index)` (§25.4.8).
fn load(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "Atomics.load";
    let (data, length) = validate_integer(rt, rt.arg(call, 0), NAME, false)?;
    let index = validate_access(rt, rt.arg(call, 1), length)?;
    revalidate(rt, &data, index, NAME)?;
    let bits = current_bits(rt, &data, index)?;
    Ok(NativeReturn::Value(value_of_bits(rt, data.ty, bits)?))
}

/// `Atomics.store(typedArray, index, value)` (§25.4.14): returns the
/// converted value (not wrapped to the element type).
fn store(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "Atomics.store";
    let (data, length) = validate_integer(rt, rt.arg(call, 0), NAME, false)?;
    let index = validate_access(rt, rt.arg(call, 1), length)?;
    let operand = operand(rt, data.ty, rt.arg(call, 2))?;
    revalidate(rt, &data, index, NAME)?;
    rt.heap.typed_store_bits(&data, index, operand.bits)?;
    Ok(NativeReturn::Value(operand.value))
}

/// `Atomics.isLockFree(size)` (§25.4.7): true for 1, 2, 4 and 8.
fn is_lock_free(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let size = to_integer_or_infinity(rt.to_number(rt.arg(call, 0))?);
    Ok(NativeReturn::Value(Value::Bool(
        matches!(size as i64, 1 | 2 | 4 | 8) && size.is_finite(),
    )))
}

/// `Atomics.wait` (§25.4.15) and `Atomics.waitAsync` (§25.4.16) on an
/// array of a buffer that is not shared: the `TypeError` (`name` is
/// V8's name for it in the message of a detached array).
fn wait(rt: &mut Runtime, call: &NativeCall, name: &str) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    validate_integer(rt, value, name, true)?;
    Err(VmError::type_error(format!(
        "{} is not a shared typed array.",
        rt.primitive_text(value)
    )))
}

/// `Atomics.notify(typedArray, index, count)` (§25.4.17): 0 for a buffer
/// that is not shared, after the checks and the conversion of `count`.
fn notify(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, length) = validate_integer(rt, rt.arg(call, 0), "Atomics.notify", true)?;
    validate_access(rt, rt.arg(call, 1), length)?;
    let count = rt.arg(call, 2);
    if !count.is_undefined() {
        rt.to_number(count)?;
    }
    Ok(NativeReturn::Value(Value::Int(0)))
}

/// `Atomics.pause([N])` (ES2026): `N` must be undefined or an integral
/// Number; the hint has no effect here.
fn pause(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let hint = rt.arg(call, 0);
    let integral = match hint {
        Value::Undefined | Value::Int(_) => true,
        Value::Double(number) => number.is_finite() && number.fract() == 0.0,
        _ => false,
    };
    if !integral {
        return Err(VmError::type_error(
            "Atomics.pause argument must be undefined or an integer",
        ));
    }
    Ok(NativeReturn::Value(Value::Undefined))
}
