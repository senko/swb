//! What the methods of `%TypedArray%.prototype` of ECMA-262 §23.2.3
//! (<https://tc39.es/ecma262/#sec-properties-of-the-%typedarrayprototype%-object>)
//! share: `ValidateTypedArray` (§23.2.4.4) with V8's message for each
//! method, element reads for the loops, and the conversion of a value to
//! an element.
//!
//! V8 words the check of the receiver in two ways, by method (measured in
//! Chromium 148): "Method %TypedArray%.prototype.at called on incompatible
//! receiver X" for `at`, `join`, `sort`, `toReversed`, `toSorted` and
//! `toLocaleString`, and "this is not a typed array." for the others. A
//! detached or out-of-bounds array is "Cannot perform
//! %TypedArray%.prototype.NAME on a detached or out-of-bounds
//! `ArrayBuffer`", except that `findLastIndex` calls itself `findIndexLast`
//! in it and `reduce` leaves out "or out-of-bounds".

use crate::heap::Gc;
use crate::object::{Element, Object, TypedArrayData};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{NativeCall, VmError, VmResult};

use super::typed_array_create::out_of_bounds;

/// Whether V8 words the receiver check of the method as "Method …
/// called on incompatible receiver".
fn incompatible_style(name: &str) -> bool {
    matches!(
        name,
        "at" | "join" | "sort" | "toReversed" | "toSorted" | "toLocaleString"
    )
}

/// The `TypeError` of a detached or out-of-bounds array for the method
/// `name`.
fn unavailable(name: &str) -> VmError {
    match name {
        "findLastIndex" => out_of_bounds("%TypedArray%.prototype.findIndexLast"),
        "reduce" => VmError::type_error(
            "Cannot perform %TypedArray%.prototype.reduce on a detached ArrayBuffer",
        ),
        _ => out_of_bounds(&format!("%TypedArray%.prototype.{name}")),
    }
}

/// `ValidateTypedArray(this)` for the method `name` of
/// `%TypedArray%.prototype`: the object, its slots and its length.
pub(super) fn validate_this(
    rt: &Runtime,
    call: &NativeCall,
    name: &str,
) -> VmResult<(Gc<Object>, TypedArrayData, usize)> {
    let Some((object, data)) = rt.typed_slots(call.this())? else {
        return Err(if incompatible_style(name) {
            rt.incompatible_receiver(&format!("%TypedArray%.prototype.{name}"), call.this())
        } else {
            VmError::type_error("this is not a typed array.")
        });
    };
    match rt.heap.typed_length(&data)? {
        Some(length) => Ok((object, data, length)),
        None => Err(unavailable(name)),
    }
}

/// The `TypeError` of `fill` and `copyWithin` for an array that is out of
/// bounds after the conversions of their arguments ran
/// (`IsTypedArrayOutOfBounds` of a new witness record). V8 words it
/// without "or out-of-bounds" (measured).
pub(super) fn unavailable_now(name: &str) -> VmError {
    VmError::type_error(format!(
        "Cannot perform %TypedArray%.prototype.{name} on a detached ArrayBuffer"
    ))
}

/// An index as a Number value (an integer when it fits).
pub(super) fn position(index: usize) -> Value {
    match i32::try_from(index) {
        Ok(int) => Value::Int(int),
        Err(_) => Value::number(index as f64),
    }
}

impl Runtime {
    /// `Get(O, ToString(k))` for an index of a typed array: the element, or
    /// `undefined` when the index is not (or no longer) valid. A `BigInt`
    /// element is recorded in the current handle scope.
    pub(super) fn typed_value(&mut self, data: &TypedArrayData, index: usize) -> VmResult<Value> {
        let value = self.typed_element(data, index)?;
        Ok(self.heap.record(value))
    }

    /// The conversion of `value` to an element of the array's type
    /// (`ToNumber`, or `ToBigInt` for the `BigInt` types), which can run
    /// script code.
    pub(super) fn element_for(
        &mut self,
        ty: crate::object::ElementType,
        value: Value,
    ) -> VmResult<Element> {
        match self.heap.plain_element(ty, value)? {
            Some(element) => Ok(element),
            None => self.element_of(ty, value),
        }
    }
}
