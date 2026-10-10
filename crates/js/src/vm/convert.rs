//! Type conversions and the operators of the subset (ECMA-262 §7.1,
//! §7.2, §13.5 to §13.15, §6.1.6.1).
//!
//! The interpreter handles numbers in its fast paths; the general paths
//! here run when an operand needs a conversion. Conversions that can run
//! script code (`ToPrimitive` calls `valueOf` and `toString` through
//! re-entry) record their intermediate results in the current handle
//! scope, which the caller opened.

use swb_js_text::{Str16, String16};

use crate::heap::Gc;
use crate::object::{INVALID_ARRAY_LENGTH, Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::{Equality, Value};
use crate::vm::function::{is_callable_kind, is_constructor_kind};
use crate::vm::number::{number_to_string, string_to_number};
use crate::vm::{Intrinsic, VmError, VmResult, WellKnown};

/// The longest string (V8's limit: 2^29 − 24 code units).
pub(crate) const MAX_STRING_LENGTH: usize = (1 << 29) - 24;

/// Concatenations of at least this many code units reserve their memory
/// first (a safepoint).
const RESERVE_FROM: usize = 1024;

/// The preferred type of `ToPrimitive`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hint {
    Default,
    Number,
    String,
}

/// The numeric binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumberOp {
    Sub,
    Mul,
    Div,
    Rem,
    Exp,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
}

/// The largest integer that `ToLength` gives (2^53 − 1).
pub(crate) const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `ToIntegerOrInfinity` (§7.1.5) of a Number.
pub(crate) fn to_integer_or_infinity(x: f64) -> f64 {
    if x.is_nan() {
        return 0.0;
    }
    // `trunc` keeps infinities; `+ 0.0` turns -0 into +0.
    x.trunc() + 0.0
}

/// `ToLength` (§7.1.20) of a Number.
pub(crate) fn to_length(x: f64) -> f64 {
    to_integer_or_infinity(x).clamp(0.0, MAX_SAFE_INTEGER)
}

/// The length of a new array for a Number: an integer from 0 to 2^32 − 1,
/// else a `RangeError` (`ArrayCreate`, §10.4.2.2, and `Array(len)`).
pub(crate) fn checked_array_length(length: f64) -> VmResult<u32> {
    if (0.0..=f64::from(u32::MAX)).contains(&length) && length.fract() == 0.0 {
        Ok(length as u32)
    } else {
        Err(VmError::range_error(INVALID_ARRAY_LENGTH))
    }
}

/// `ToInt32` (§7.1.6).
pub(crate) fn to_int32(x: f64) -> i32 {
    to_uint32(x) as i32
}

/// `ToUint32` (§7.1.7).
pub(crate) fn to_uint32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    let truncated = x.trunc();
    if truncated.abs() < 2_147_483_648.0 {
        return truncated as i32 as u32;
    }
    truncated.rem_euclid(4_294_967_296.0) as u32
}

/// `Number::exponentiate` (§6.1.6.1.3): differs from `powf` for a base of
/// ±1 with an infinite exponent and for a NaN exponent.
pub(crate) fn exponentiate(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() {
        return f64::NAN;
    }
    if exponent == 0.0 {
        return 1.0;
    }
    if base.abs() == 1.0 && exponent.is_infinite() {
        return f64::NAN;
    }
    base.powf(exponent)
}

/// `x * y` on integers: an integer unless it overflows or is `-0`.
pub(crate) fn int_mul(x: i32, y: i32) -> Value {
    match x.checked_mul(y) {
        Some(0) if x < 0 || y < 0 => Value::Double(-0.0),
        Some(product) => Value::Int(product),
        None => Value::Double(f64::from(x) * f64::from(y)),
    }
}

/// `x / y` on integers: an integer when exact and not `-0`.
pub(crate) fn int_div(x: i32, y: i32) -> Value {
    if y != 0 && !(x == i32::MIN && y == -1) && x % y == 0 && !(x == 0 && y < 0) {
        Value::Int(x / y)
    } else {
        Value::Double(f64::from(x) / f64::from(y))
    }
}

/// `x % y` on integers (the sign of the dividend; `-0` for a negative
/// dividend with a zero result).
pub(crate) fn int_rem(x: i32, y: i32) -> Value {
    if y == 0 || (x == i32::MIN && y == -1) {
        return Value::Double(f64::from(x) % f64::from(y));
    }
    match x % y {
        0 if x < 0 => Value::Double(-0.0),
        r => Value::Int(r),
    }
}

/// A numeric operator on two Numbers.
fn number_op(op: NumberOp, x: f64, y: f64) -> Value {
    match op {
        NumberOp::Sub => Value::Double(x - y),
        NumberOp::Mul => Value::Double(x * y),
        NumberOp::Div => Value::Double(x / y),
        NumberOp::Rem => Value::Double(x % y),
        NumberOp::Exp => Value::Double(exponentiate(x, y)),
        NumberOp::BitAnd => Value::Int(to_int32(x) & to_int32(y)),
        NumberOp::BitOr => Value::Int(to_int32(x) | to_int32(y)),
        NumberOp::BitXor => Value::Int(to_int32(x) ^ to_int32(y)),
        NumberOp::Shl => Value::Int(to_int32(x).wrapping_shl(to_uint32(y) & 31)),
        NumberOp::Shr => Value::Int(to_int32(x) >> (to_uint32(y) & 31)),
        NumberOp::UShr => Value::number(f64::from(to_uint32(x) >> (to_uint32(y) & 31))),
    }
}

// The conversions keep the specification's names (`ToString` and others);
// they need the runtime mutably because they can run script code.
#[allow(clippy::wrong_self_convention)]
impl Runtime {
    /// `ToPrimitive` (§7.1.1): `@@toPrimitive` if the object has one,
    /// else `OrdinaryToPrimitive` (§7.1.1.1).
    pub(crate) fn to_primitive(&mut self, value: Value, hint: Hint) -> VmResult<Value> {
        let Value::Object(object) = value else {
            return Ok(value);
        };
        let key = self.symbol_key(WellKnown::ToPrimitive);
        if let Some(method) = self.get_method(value, key)? {
            let atoms = &self.vm.atoms;
            let hint = match hint {
                Hint::Default => atoms.default,
                Hint::Number => atoms.number,
                Hint::String => atoms.string,
            };
            let result = self.call(method, value, &[hint.into()])?;
            if matches!(result, Value::Object(_)) {
                return Err(VmError::type_error(
                    "Cannot convert object to primitive value",
                ));
            }
            return Ok(result);
        }
        let atoms = &self.vm.atoms;
        let order = if hint == Hint::String {
            [atoms.to_string, atoms.value_of]
        } else {
            [atoms.value_of, atoms.to_string]
        };
        for name in order {
            let method = self.get_value(value, PropertyKey::String(name))?;
            if self.is_callable(method)? {
                let result = self.call(method, object.into(), &[])?;
                if !matches!(result, Value::Object(_)) {
                    return Ok(result);
                }
            }
        }
        Err(VmError::type_error(
            "Cannot convert object to primitive value",
        ))
    }

    /// `GetMethod` (§7.3.11): `None` for `undefined` and `null`, a
    /// `TypeError` for a value that is not callable. The method is
    /// recorded in the current handle scope.
    pub(crate) fn get_method(&mut self, value: Value, key: PropertyKey) -> VmResult<Option<Value>> {
        let method = self.get_value(value, key)?;
        if method.is_nullish() {
            return Ok(None);
        }
        if !self.is_callable(method)? {
            return Err(self.not_callable(method));
        }
        Ok(Some(method))
    }

    /// `IsCallable` (§7.2.3).
    pub(crate) fn is_callable(&self, value: Value) -> VmResult<bool> {
        match value {
            Value::Object(object) => Ok(is_callable_kind(&self.heap.object(object)?.kind)),
            _ => Ok(false),
        }
    }

    /// `IsConstructor` (§7.2.4).
    pub(crate) fn is_constructor(&self, value: Value) -> VmResult<bool> {
        match value {
            Value::Object(object) => Ok(is_constructor_kind(&self.heap.object(object)?.kind)),
            _ => Ok(false),
        }
    }

    /// `ToNumber` (§7.1.4); also `ToNumeric` (§7.1.3) while `BigInt` is not
    /// supported.
    pub(crate) fn to_number(&mut self, value: Value) -> VmResult<f64> {
        let primitive = self.to_primitive(value, Hint::Number)?;
        match primitive {
            Value::Undefined => Ok(f64::NAN),
            Value::Null => Ok(0.0),
            Value::Bool(b) => Ok(f64::from(u8::from(b))),
            Value::Int(i) => Ok(f64::from(i)),
            Value::Double(d) => Ok(d),
            Value::String(s) => Ok(string_to_number(self.heap.string(s)?.as_str16())),
            Value::Symbol(_) => Err(VmError::type_error(
                "Cannot convert a Symbol value to a number",
            )),
            Value::BigInt(_) => Err(VmError::type_error(
                "Cannot mix BigInt and other types, use explicit conversions",
            )),
            Value::Object(_) | Value::Empty | Value::Cell(_) => {
                Err(VmError::invariant("ToPrimitive returned an object"))
            }
        }
    }

    /// `ToString` (§7.1.17). The result is recorded in the current handle
    /// scope.
    pub(crate) fn to_string(&mut self, value: Value) -> VmResult<Gc<JsString>> {
        let primitive = self.to_primitive(value, Hint::String)?;
        let atoms = &self.vm.atoms;
        let string = match primitive {
            Value::String(s) => return Ok(s),
            Value::Undefined => atoms.undefined,
            Value::Null => atoms.null,
            Value::Bool(true) => atoms.true_,
            Value::Bool(false) => atoms.false_,
            Value::Int(i) => self.heap.alloc_str(&i.to_string())?,
            Value::Double(d) => self.heap.alloc_str(&number_to_string(d))?,
            Value::Symbol(_) => {
                return Err(VmError::type_error(
                    "Cannot convert a Symbol value to a string",
                ));
            }
            Value::BigInt(_) => {
                return Err(VmError::type_error("BigInt is not supported yet"));
            }
            Value::Object(_) | Value::Empty | Value::Cell(_) => {
                return Err(VmError::invariant("ToPrimitive returned an object"));
            }
        };
        self.heap.record(string);
        Ok(string)
    }

    /// `ToPropertyKey` (§7.1.19).
    pub(crate) fn to_property_key(&mut self, value: Value) -> VmResult<PropertyKey> {
        match value {
            Value::Int(i) if i >= 0 => return Ok(PropertyKey::Index(i as u32)),
            Value::Symbol(symbol) => return Ok(PropertyKey::Symbol(symbol)),
            Value::String(string) => return Ok(self.heap.key_from_string(string)?),
            _ => {}
        }
        let primitive = self.to_primitive(value, Hint::String)?;
        if let Value::Symbol(symbol) = primitive {
            return Ok(PropertyKey::Symbol(symbol));
        }
        let string = self.to_string(primitive)?;
        Ok(self.heap.key_from_string(string)?)
    }

    /// The key of a value that `ToKey` produced (no script code runs).
    pub(crate) fn key_from_converted(&mut self, value: Value) -> VmResult<PropertyKey> {
        match value {
            Value::Int(i) if i >= 0 => Ok(PropertyKey::Index(i as u32)),
            Value::Double(d) if (0.0..4_294_967_295.0).contains(&d) && d.fract() == 0.0 => {
                Ok(PropertyKey::Index(d as u32))
            }
            Value::String(string) => Ok(self.heap.key_from_string(string)?),
            Value::Symbol(symbol) => Ok(PropertyKey::Symbol(symbol)),
            _ => Err(VmError::invariant("a key register without a converted key")),
        }
    }

    /// `ToBoolean` (§7.1.2).
    pub(crate) fn to_boolean(&self, value: Value) -> VmResult<bool> {
        Ok(match value {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => b,
            Value::Int(i) => i != 0,
            Value::Double(d) => !(d == 0.0 || d.is_nan()),
            Value::String(s) => !self.heap.string(s)?.is_empty(),
            Value::Symbol(_) | Value::Object(_) => true,
            Value::BigInt(b) => !self.heap.bigint(b)?.limbs.is_empty(),
            Value::Empty | Value::Cell(_) => {
                return Err(VmError::invariant("ToBoolean of an internal value"));
            }
        })
    }

    /// `ToObject` (§7.1.18): a wrapper object for a primitive. The result
    /// is not recorded in a handle scope: the caller roots it before the
    /// next allocation (`coerce_this` stores it in `Frame::this`).
    pub(crate) fn to_object(&mut self, value: Value) -> VmResult<Gc<Object>> {
        let realm = self.current_realm();
        let (kind, proto) = match value {
            Value::Object(object) => return Ok(object),
            Value::Undefined | Value::Null => {
                return Err(VmError::type_error(
                    "Cannot convert undefined or null to object",
                ));
            }
            Value::String(s) => (ObjectKind::StringWrapper(s), Intrinsic::StringPrototype),
            Value::Int(i) => (
                ObjectKind::NumberWrapper(f64::from(i)),
                Intrinsic::NumberPrototype,
            ),
            Value::Double(d) => (ObjectKind::NumberWrapper(d), Intrinsic::NumberPrototype),
            Value::Bool(b) => (ObjectKind::BooleanWrapper(b), Intrinsic::BooleanPrototype),
            Value::Symbol(s) => (ObjectKind::SymbolWrapper(s), Intrinsic::SymbolPrototype),
            Value::BigInt(_) | Value::Empty | Value::Cell(_) => {
                return Err(VmError::type_error("Cannot convert value to object"));
            }
        };
        let proto = self.intrinsic(realm, proto)?;
        let object = self.heap.new_object_with_kind(Some(proto), kind)?;
        if let Value::String(s) = value {
            let length = self.heap.string(s)?.len();
            let key = PropertyKey::String(self.heap.length_atom());
            self.define(
                object,
                key,
                Value::number(length as f64),
                false,
                false,
                false,
            )?;
        }
        Ok(object)
    }

    /// `typeof` (§13.5.3).
    pub(crate) fn typeof_value(&self, value: Value) -> VmResult<Gc<JsString>> {
        let atoms = &self.vm.atoms;
        Ok(match value {
            Value::Undefined => atoms.undefined,
            Value::Null => atoms.object,
            Value::Bool(_) => atoms.boolean,
            Value::Int(_) | Value::Double(_) => atoms.number,
            Value::String(_) => atoms.string,
            Value::Symbol(_) => atoms.symbol,
            Value::BigInt(_) => atoms.bigint,
            Value::Object(object) => {
                if is_callable_kind(&self.heap.object(object)?.kind) {
                    atoms.function
                } else {
                    atoms.object
                }
            }
            Value::Empty | Value::Cell(_) => {
                return Err(VmError::invariant("typeof of an internal value"));
            }
        })
    }

    /// `IsStrictlyEqual` (§7.2.15).
    pub(crate) fn strictly_equal(&self, a: Value, b: Value) -> VmResult<bool> {
        Ok(self.heap.equal(Equality::Strict, a, b)?)
    }

    /// `IsLooselyEqual` (§7.2.14).
    pub(crate) fn loosely_equal(&mut self, a: Value, b: Value) -> VmResult<bool> {
        let mut a = a;
        let mut b = b;
        // Each round removes one conversion, so the loop ends.
        loop {
            match (a, b) {
                (Value::Undefined | Value::Null, Value::Undefined | Value::Null) => {
                    return Ok(true);
                }
                (Value::Undefined | Value::Null, _) | (_, Value::Undefined | Value::Null) => {
                    return Ok(false);
                }
                (x, y) if same_type(x, y) => return self.strictly_equal(x, y),
                (Value::Int(_) | Value::Double(_), Value::String(_)) => {
                    b = Value::Double(self.to_number(b)?);
                }
                (Value::String(_), Value::Int(_) | Value::Double(_)) => {
                    a = Value::Double(self.to_number(a)?);
                }
                (Value::Bool(x), _) => a = Value::Int(i32::from(x)),
                (_, Value::Bool(y)) => b = Value::Int(i32::from(y)),
                (
                    Value::String(_) | Value::Int(_) | Value::Double(_) | Value::Symbol(_),
                    Value::Object(_),
                ) => {
                    b = self.to_primitive(b, Hint::Default)?;
                    self.heap.record(b);
                }
                (
                    Value::Object(_),
                    Value::String(_) | Value::Int(_) | Value::Double(_) | Value::Symbol(_),
                ) => {
                    a = self.to_primitive(a, Hint::Default)?;
                    self.heap.record(a);
                }
                _ => return Ok(false),
            }
        }
    }

    /// The relational operators through `IsLessThan` (§7.2.13). With
    /// `left_first` false, `y` is converted before `x` (for `>` and `<=`,
    /// whose operands are swapped). With `or_equal`, the result is true
    /// when `IsLessThan` is false (not undefined): `<=` and `>=`.
    pub(crate) fn compare_values(
        &mut self,
        x: Value,
        y: Value,
        swapped: bool,
        or_equal: bool,
    ) -> VmResult<bool> {
        let (px, py) = if swapped {
            let py = self.to_primitive(y, Hint::Number)?;
            self.heap.record(py);
            let px = self.to_primitive(x, Hint::Number)?;
            (px, py)
        } else {
            let px = self.to_primitive(x, Hint::Number)?;
            self.heap.record(px);
            let py = self.to_primitive(y, Hint::Number)?;
            (px, py)
        };
        let less = if let (Value::String(a), Value::String(b)) = (px, py) {
            let a = self.heap.string(a)?.as_str16();
            let b = self.heap.string(b)?.as_str16();
            Some(a.units().lt(b.units()))
        } else {
            let (nx, ny) = if swapped {
                let ny = self.to_number(py)?;
                (self.to_number(px)?, ny)
            } else {
                let nx = self.to_number(px)?;
                (nx, self.to_number(py)?)
            };
            if nx.is_nan() || ny.is_nan() {
                None
            } else {
                Some(nx < ny)
            }
        };
        Ok(if or_equal {
            less == Some(false)
        } else {
            less == Some(true)
        })
    }

    /// The `+` operator for operands that are not both Numbers (§13.15.3
    /// `ApplyStringOrNumericBinaryOperator`).
    pub(crate) fn add_slow(&mut self, a: Value, b: Value) -> VmResult<Value> {
        let pa = self.to_primitive(a, Hint::Default)?;
        self.heap.record(pa);
        let pb = self.to_primitive(b, Hint::Default)?;
        self.heap.record(pb);
        if matches!(pa, Value::String(_)) || matches!(pb, Value::String(_)) {
            let sa = self.to_string(pa)?;
            let sb = self.to_string(pb)?;
            return Ok(Value::String(self.concat(sa, sb)?));
        }
        let x = self.to_number(pa)?;
        let y = self.to_number(pb)?;
        Ok(Value::Double(x + y))
    }

    /// A numeric binary operator for operands that are not both Numbers.
    pub(crate) fn arith_slow(&mut self, op: NumberOp, a: Value, b: Value) -> VmResult<Value> {
        let x = self.to_number(a)?;
        let y = self.to_number(b)?;
        Ok(number_op(op, x, y))
    }

    /// The concatenation of two strings (flat; ropes come later). A long
    /// result reserves its memory first: a safepoint, so both operands
    /// must be rooted by the caller.
    pub(crate) fn concat(&mut self, a: Gc<JsString>, b: Gc<JsString>) -> VmResult<Gc<JsString>> {
        let len_a = self.heap.string(a)?.len();
        let len_b = self.heap.string(b)?.len();
        if len_a == 0 {
            return Ok(b);
        }
        if len_b == 0 {
            return Ok(a);
        }
        let total = len_a + len_b;
        if total > MAX_STRING_LENGTH {
            return Err(VmError::range_error("Invalid string length"));
        }
        if total >= RESERVE_FROM {
            self.heap.reserve(total * 2, &self.vm)?;
        }
        let mut text = String16::new();
        push_units(&mut text, self.heap.string(a)?.as_str16());
        push_units(&mut text, self.heap.string(b)?.as_str16());
        let result = self.heap.alloc_string(text)?;
        // Charged after the copy; the result is not rooted, so a check
        // that ends the script is the only thing that can follow.
        self.charge_units(total)?;
        Ok(result)
    }

    /// `x instanceof target` (§13.10.2 `InstanceofOperator`): the
    /// `@@hasInstance` method of the target, else `OrdinaryHasInstance`.
    pub(crate) fn instance_of(&mut self, value: Value, target: Value) -> VmResult<bool> {
        self.has_instance_loop(value, target, false)
    }

    /// `OrdinaryHasInstance` (§7.3.21).
    pub(crate) fn ordinary_has_instance(
        &mut self,
        constructor: Value,
        value: Value,
    ) -> VmResult<bool> {
        self.has_instance_loop(value, constructor, true)
    }

    /// `InstanceofOperator` and `OrdinaryHasInstance` as one loop: a bound
    /// function asks its target, which is the operator again (step 3 of
    /// §7.3.21), so a chain of bound functions needs no Rust recursion.
    /// `ordinary` skips the `@@hasInstance` lookup of the first target.
    fn has_instance_loop(
        &mut self,
        value: Value,
        mut target: Value,
        mut ordinary: bool,
    ) -> VmResult<bool> {
        loop {
            if !ordinary {
                if !matches!(target, Value::Object(_)) {
                    return Err(VmError::type_error(
                        "Right-hand side of 'instanceof' is not an object",
                    ));
                }
                let key = self.symbol_key(WellKnown::HasInstance);
                match self.get_method(target, key)? {
                    // Function.prototype[@@hasInstance] is
                    // OrdinaryHasInstance: no call needed.
                    Some(handler) if !self.is_default_has_instance(handler) => {
                        let result = self.call(handler, target, &[value])?;
                        return self.to_boolean(result);
                    }
                    None if !self.is_callable(target)? => {
                        return Err(VmError::type_error(
                            "Right-hand side of 'instanceof' is not callable",
                        ));
                    }
                    Some(_) | None => {}
                }
            }
            // OrdinaryHasInstance.
            let Value::Object(constructor) = target else {
                return Ok(false);
            };
            if !self.is_callable(target)? {
                return Ok(false);
            }
            if let ObjectKind::Bound(bound) = &self.heap.object(constructor)?.kind {
                target = bound.target.into();
                self.tick()?;
                ordinary = false;
                continue;
            }
            return self.ordinary_has_instance_tail(constructor, value);
        }
    }

    /// Steps 4 to 8 of `OrdinaryHasInstance`: the prototype chain walk.
    fn ordinary_has_instance_tail(
        &mut self,
        constructor: Gc<Object>,
        value: Value,
    ) -> VmResult<bool> {
        let Value::Object(object) = value else {
            return Ok(false);
        };
        let key = PropertyKey::String(self.vm.atoms.prototype);
        let proto = self.get_value(constructor.into(), key)?;
        let Value::Object(proto) = proto else {
            return Err(VmError::type_error(format!(
                "Function has non-object prototype '{}' in instanceof check",
                self.primitive_text(proto)
            )));
        };
        let mut current = self.heap.get_prototype_of(object)?;
        while let Some(p) = current {
            if p == proto {
                return Ok(true);
            }
            self.tick()?;
            current = self.heap.get_prototype_of(p)?;
        }
        Ok(false)
    }

    /// Whether `handler` is `Function.prototype[@@hasInstance]` of the
    /// current realm.
    fn is_default_has_instance(&self, handler: Value) -> bool {
        matches!(handler, Value::Object(h) if self
            .intrinsic(self.current_realm(), Intrinsic::FunctionHasInstance)
            .is_ok_and(|default| default == h))
    }
}

/// A property key as a register value (`ToKey`): an integer for an array
/// index below 2^31, otherwise the string or symbol.
pub(crate) fn key_as_value(key: PropertyKey) -> Value {
    match key {
        PropertyKey::Index(index) => match i32::try_from(index) {
            Ok(index) => Value::Int(index),
            Err(_) => Value::Double(f64::from(index)),
        },
        PropertyKey::String(string) => Value::String(string),
        PropertyKey::Symbol(symbol) => Value::Symbol(symbol),
    }
}

/// Whether two values have the same language type (for `IsLooselyEqual`).
fn same_type(a: Value, b: Value) -> bool {
    matches!(
        (a, b),
        (
            Value::Int(_) | Value::Double(_),
            Value::Int(_) | Value::Double(_)
        ) | (Value::String(_), Value::String(_))
            | (Value::Bool(_), Value::Bool(_))
            | (Value::Symbol(_), Value::Symbol(_))
            | (Value::BigInt(_), Value::BigInt(_))
            | (Value::Object(_), Value::Object(_))
    )
}

fn push_units(text: &mut String16, units: Str16<'_>) {
    text.push_str16(units);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int32_conversions_wrap() {
        assert_eq!(to_int32(4_294_967_295.0), -1);
        assert_eq!(to_int32(2_147_483_648.0), i32::MIN);
        assert_eq!(to_int32(-1.5), -1);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(f64::INFINITY), 0);
        assert_eq!(to_uint32(-1.0), u32::MAX);
        assert_eq!(to_uint32(1e20), 1_661_992_960);
    }

    #[test]
    fn length_conversions() {
        assert_eq!(to_length(-5.0), 0.0);
        assert_eq!(to_length(f64::NAN), 0.0);
        assert_eq!(to_length(f64::INFINITY), MAX_SAFE_INTEGER);
        assert_eq!(to_length(3.9), 3.0);
        assert!(to_integer_or_infinity(-0.5).is_sign_positive());
        assert_eq!(to_integer_or_infinity(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }

    #[test]
    fn array_lengths_are_uint32_integers() {
        assert_eq!(checked_array_length(0.0).ok(), Some(0));
        assert_eq!(checked_array_length(-0.0).ok(), Some(0));
        assert_eq!(checked_array_length(4_294_967_295.0).ok(), Some(u32::MAX));
        for bad in [-1.0, 1.5, 4_294_967_296.0, f64::NAN, f64::INFINITY] {
            assert!(checked_array_length(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn integer_paths_keep_minus_zero() {
        assert!(matches!(int_mul(-3, 0), Value::Double(d) if d.is_sign_negative()));
        assert_eq!(int_mul(6, 7), Value::Int(42));
        assert!(matches!(int_div(0, -5), Value::Double(d) if d.is_sign_negative()));
        assert_eq!(int_div(10, 5), Value::Int(2));
        assert_eq!(int_div(7, 2), Value::Double(3.5));
        assert!(matches!(int_rem(-4, 2), Value::Double(d) if d.is_sign_negative()));
        assert_eq!(int_rem(-8, 3), Value::Int(-2));
        assert!(matches!(int_rem(5, 0), Value::Double(d) if d.is_nan()));
        assert!(exponentiate(1.0, f64::INFINITY).is_nan());
        assert_eq!(exponentiate(f64::NAN, 0.0), 1.0);
    }
}
