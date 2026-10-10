//! `ArraySpeciesCreate` (ECMA-262 §10.4.2.3), `Array[@@species]`
//! (§23.1.2.5) and `Array.prototype[@@unscopables]` (§23.1.3.41).

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::checked_array_length;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WellKnown};

/// The names on `Array.prototype[@@unscopables]` (ES2025, in Chromium's
/// order).
const UNSCOPABLES: [&str; 16] = [
    "at",
    "copyWithin",
    "entries",
    "fill",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flat",
    "flatMap",
    "includes",
    "keys",
    "toReversed",
    "toSorted",
    "toSpliced",
    "values",
];

/// Defines the `@@species` getter on `Array` and `@@unscopables` on
/// `Array.prototype`.
pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    array: Gc<Object>,
    proto: Gc<Object>,
) -> VmResult<()> {
    let getter = rt.new_builtin(realm, "get [Symbol.species]", 0, species, false)?;
    let key = rt.symbol_key(WellKnown::Species);
    let desc = PropertyDescriptor::accessor(getter.into(), Value::Undefined, false, true);
    rt.define_own_property(array, key, desc)?;
    let unscopables = rt.heap.new_object(None)?;
    rt.heap.record(unscopables);
    for name in UNSCOPABLES {
        let key = rt.heap.key_from_str(name)?;
        rt.heap
            .create_data_property(unscopables, key, Value::Bool(true), &rt.vm)?;
    }
    let key = rt.symbol_key(WellKnown::Unscopables);
    rt.define(proto, key, unscopables.into(), false, false, true)
}

/// `get Array[@@species]`: the receiver.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn species(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

impl Runtime {
    /// `ArrayCreate(length)` (§10.4.2.2) in the current realm, recorded in
    /// the current handle scope. A length above 2^32 − 1 is a
    /// `RangeError`.
    pub(crate) fn array_create(&mut self, length: f64) -> VmResult<Gc<Object>> {
        let length = checked_array_length(length)?;
        let proto = self.intrinsic(self.current_realm(), Intrinsic::ArrayPrototype)?;
        let array = self.heap.new_array(Some(proto), length)?;
        Ok(self.heap.record(array))
    }

    /// `ArraySpeciesCreate(originalArray, length)` (§10.4.2.3). The result
    /// is recorded in the current handle scope.
    pub(crate) fn array_species_create(
        &mut self,
        original: Gc<Object>,
        length: f64,
    ) -> VmResult<Gc<Object>> {
        if !matches!(self.heap.object(original)?.kind, ObjectKind::Array { .. }) {
            return self.array_create(length);
        }
        let key = PropertyKey::String(self.vm.atoms.constructor);
        let mut constructor = self.get_value(original.into(), key)?;
        if let Value::Object(candidate) = constructor
            && self.is_constructor(constructor)?
            && let Some(other) = self.function_realm(candidate)
            && other != self.current_realm()
            && candidate == self.intrinsic(other, Intrinsic::ArrayConstructor)?
        {
            // The `Array` of another realm: use the default.
            constructor = Value::Undefined;
        }
        if let Value::Object(candidate) = constructor {
            let key = self.symbol_key(WellKnown::Species);
            constructor = self.get_value(candidate.into(), key)?;
            if constructor.is_nullish() {
                constructor = Value::Undefined;
            }
        }
        if constructor.is_undefined() {
            return self.array_create(length);
        }
        if !self.is_constructor(constructor)? {
            return Err(VmError::type_error(
                "object.constructor[Symbol.species] is not a constructor",
            ));
        }
        match self.construct(constructor, &[Value::number(length)])? {
            Value::Object(result) => Ok(result),
            _ => Err(VmError::invariant("a constructor returned a non-object")),
        }
    }
}
