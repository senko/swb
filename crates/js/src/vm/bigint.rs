//! `BigInt` values in the VM: the operators of §6.1.6.2
//! (<https://tc39.es/ecma262/#sec-numeric-types-bigint>) on heap values,
//! `ToNumeric` (§7.1.3), `ToBigInt` (§7.1.13), `StringToBigInt` (§7.1.14),
//! `ToBigInt64` and `ToBigUint64` (§7.1.15, §7.1.16), `NumberToBigInt`
//! (§21.2.1.1.1), and the comparisons of a `BigInt` with a Number or a
//! String for `IsLessThan` and `IsLooselyEqual` (§7.2.13, §7.2.14).
//!
//! The arithmetic is in `js-text` ([`swb_js_text::bigint`]). Every
//! operation charges its cost ([`bigint::cost`], in work units of about
//! 0.7 ns) to the time countdown before it runs, and every result goes
//! through [`Heap::alloc_bigint`], which charges the heap limit. The
//! messages are V8's (measured in Chromium 148, `tools/probes/js-bigint.json`).
//!
//! [`Heap::alloc_bigint`]: crate::heap::Heap::alloc_bigint

use std::cmp::Ordering;

use swb_js_text::String16;
use swb_js_text::bigint::{self, BigIntError, cost};

use crate::error::Termination;
use crate::heap::Gc;
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::{BigInt, Value};
use crate::vm::convert::{Hint, NumberOp};
use crate::vm::number::number_to_string;
use crate::vm::{VmError, VmResult};

/// The work units of `bigint::cost` that cost as much as one step of the
/// time countdown (a step is about 12 ns, a unit about 0.7 ns).
const UNITS_PER_STEP: u64 = 16;

/// V8's message for an operator whose operands are a `BigInt` and a
/// Number (`ApplyStringOrNumericBinaryOperator` step 6).
pub(crate) const MIXED_TYPES: &str = "Cannot mix BigInt and other types, use explicit conversions";

/// V8's message for `ToNumber` of a `BigInt`. The same constant as the
/// array's, so that a `length` write gets its longer message.
pub(crate) const NOT_A_NUMBER: &str = crate::object::BIGINT_LENGTH;

/// V8's message for `>>>` on two `BigInt`s (`BigInt::unsignedRightShift`).
const NO_UNSIGNED_SHIFT: &str = "BigInts have no unsigned right shift, use >> instead";

/// The longest text that the message of a failed `StringToBigInt` quotes;
/// V8 keeps the first 1,000 code units of a longer one and appends `…`
/// (measured).
const QUOTE_LIMIT: usize = 1000;

/// The result of `ToNumeric` (§7.1.3).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Numeric {
    Number(f64),
    BigInt(Gc<BigInt>),
}

/// The operators on one `BigInt`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    /// `-x` (`BigInt::unaryMinus`).
    Neg,
    /// `~x` (`BigInt::bitwiseNOT`).
    BitNot,
    /// `x + 1n` (`++`).
    Inc,
    /// `x - 1n` (`--`).
    Dec,
}

/// The error of a failed operation of the library, with V8's message.
pub(crate) fn bigint_error(error: BigIntError) -> VmError {
    match error {
        BigIntError::TooLarge => VmError::range_error("Maximum BigInt size exceeded"),
        BigIntError::DivisionByZero => VmError::range_error("Division by zero"),
        BigIntError::NegativeExponent => VmError::range_error("Exponent must be positive"),
        BigIntError::NotIntegral => {
            VmError::range_error("The number cannot be converted to a BigInt")
        }
        BigIntError::OutOfMemory => VmError::Terminated(Termination::OutOfMemory),
        BigIntError::InvalidSyntax => VmError::syntax_error("Cannot convert the text to a BigInt"),
    }
}

/// The steps of the time countdown for `units` work units.
fn steps(units: u64) -> usize {
    usize::try_from(units / UNITS_PER_STEP).unwrap_or(usize::MAX)
}

/// The cost of the operator `op` on two `BigInt`s.
fn binary_cost(op: NumberOp, x: &BigInt, y: &BigInt) -> u64 {
    match op {
        NumberOp::Add | NumberOp::Sub => cost::add(x, y),
        NumberOp::Mul => cost::mul(x, y),
        NumberOp::Div | NumberOp::Rem => cost::div(x, y),
        NumberOp::Exp => cost::pow(x, y),
        NumberOp::BitAnd | NumberOp::BitOr | NumberOp::BitXor => cost::bitwise(x, y),
        NumberOp::Shl => cost::shift(x, y, false),
        NumberOp::Shr => cost::shift(x, y, true),
        NumberOp::UShr => 0,
    }
}

/// The operator `op` on two `BigInt`s (§6.1.6.2).
fn binary(op: NumberOp, x: &BigInt, y: &BigInt) -> VmResult<BigInt> {
    let result = match op {
        NumberOp::Add => bigint::add(x, y),
        NumberOp::Sub => bigint::sub(x, y),
        NumberOp::Mul => bigint::mul(x, y),
        NumberOp::Div => bigint::div(x, y),
        NumberOp::Rem => bigint::rem(x, y),
        NumberOp::Exp => bigint::pow(x, y),
        NumberOp::BitAnd => bigint::bit_and(x, y),
        NumberOp::BitOr => bigint::bit_or(x, y),
        NumberOp::BitXor => bigint::bit_xor(x, y),
        NumberOp::Shl => bigint::shift_left(x, y),
        NumberOp::Shr => bigint::shift_right(x, y),
        NumberOp::UShr => return Err(VmError::type_error(NO_UNSIGNED_SHIFT)),
    };
    result.map_err(bigint_error)
}

/// The cost of the operator `op` on one `BigInt`.
fn unary_cost(op: UnaryOp, x: &BigInt) -> u64 {
    match op {
        UnaryOp::Neg => cost::linear(x),
        UnaryOp::BitNot => cost::bitwise(x, &BigInt::ZERO),
        UnaryOp::Inc | UnaryOp::Dec => cost::add(x, &BigInt::ZERO),
    }
}

/// The operator `op` on one `BigInt`.
fn unary(op: UnaryOp, x: &BigInt) -> VmResult<BigInt> {
    let one = || BigInt::from_u64(1);
    let result = match op {
        UnaryOp::Neg => bigint::neg(x),
        UnaryOp::BitNot => bigint::not(x),
        UnaryOp::Inc => one().and_then(|one| bigint::add(x, &one)),
        UnaryOp::Dec => one().and_then(|one| bigint::sub(x, &one)),
    };
    result.map_err(bigint_error)
}

/// `x` with digit groups of three separated by commas (en-US, the
/// default locale of Chromium; `toLocaleString` without ECMA-402).
fn group_digits(text: &[u8]) -> Vec<u8> {
    let (sign, digits) = match text.split_first() {
        Some((b'-', rest)) => (Some(b'-'), rest),
        _ => (None, text),
    };
    let mut out = Vec::with_capacity(text.len() + digits.len() / 3);
    out.extend(sign);
    for (i, digit) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(b',');
        }
        out.push(*digit);
    }
    out
}

// The conversions keep the specification's names (`ToBigInt` and others).
#[allow(clippy::wrong_self_convention)]
impl Runtime {
    /// Charges `units` work units of `bigint::cost` to the time countdown.
    pub(crate) fn charge_bigint(&mut self, units: u64) -> VmResult<()> {
        self.charge(steps(units))
    }

    /// A new `BigInt` value (charged to the heap). Not recorded in a
    /// handle scope: the caller stores or records it before the next
    /// allocation that can collect.
    pub(crate) fn new_bigint(&mut self, value: BigInt) -> VmResult<Gc<BigInt>> {
        Ok(self.heap.alloc_bigint(value)?)
    }

    /// The `BigInt` of a small integer.
    pub(crate) fn bigint_from_i64(&mut self, value: i64) -> VmResult<Gc<BigInt>> {
        let value = BigInt::from_i64(value).map_err(bigint_error)?;
        self.new_bigint(value)
    }

    /// A binary operator on two `BigInt`s (§6.1.6.2): charged before the
    /// work, the result allocated.
    pub(crate) fn bigint_binary(
        &mut self,
        op: NumberOp,
        x: Gc<BigInt>,
        y: Gc<BigInt>,
    ) -> VmResult<Value> {
        let (a, b) = (self.heap.bigint(x)?, self.heap.bigint(y)?);
        self.vm.charge(steps(binary_cost(op, a, b)))?;
        let result = binary(op, a, b)?;
        Ok(self.new_bigint(result)?.into())
    }

    /// A unary operator on a `BigInt`: charged before the work, the
    /// result allocated.
    pub(crate) fn bigint_unary(&mut self, op: UnaryOp, x: Gc<BigInt>) -> VmResult<Value> {
        let a = self.heap.bigint(x)?;
        self.vm.charge(steps(unary_cost(op, a)))?;
        let result = unary(op, a)?;
        Ok(self.new_bigint(result)?.into())
    }

    /// `ToNumeric` (§7.1.3): a Number or a `BigInt`. A `BigInt` that
    /// `ToPrimitive` returned is recorded in the current handle scope.
    pub(crate) fn to_numeric(&mut self, value: Value) -> VmResult<Numeric> {
        match value {
            Value::Int(i) => return Ok(Numeric::Number(f64::from(i))),
            Value::Double(d) => return Ok(Numeric::Number(d)),
            Value::BigInt(b) => return Ok(Numeric::BigInt(b)),
            _ => {}
        }
        let primitive = self.to_primitive(value, Hint::Number)?;
        if let Value::BigInt(b) = primitive {
            self.heap.record(b);
            return Ok(Numeric::BigInt(b));
        }
        Ok(Numeric::Number(self.to_number(primitive)?))
    }

    /// A numeric unary operator for a value that is not an `i32` (the
    /// interpreter's fast path): `ToNumeric`, then the Number or `BigInt`
    /// operation.
    pub(crate) fn unary_slow(&mut self, op: UnaryOp, value: Value) -> VmResult<Value> {
        match self.to_numeric(value)? {
            Numeric::Number(x) => Ok(match op {
                UnaryOp::Neg => Value::Double(-x),
                UnaryOp::BitNot => Value::Int(!super::convert::to_int32(x)),
                UnaryOp::Inc => Value::Double(x + 1.0),
                UnaryOp::Dec => Value::Double(x - 1.0),
            }),
            Numeric::BigInt(b) => self.bigint_unary(op, b),
        }
    }

    /// `ToNumeric` as a value (the `ToNumeric` instruction: the old value
    /// of a postfix `++` or `--`).
    pub(crate) fn to_numeric_value(&mut self, value: Value) -> VmResult<Value> {
        Ok(match self.to_numeric(value)? {
            Numeric::Number(x) => Value::number(x),
            Numeric::BigInt(b) => Value::BigInt(b),
        })
    }

    /// The decimal (or other radix, 2 to 36) text of a `BigInt` as a new
    /// string, charged (`BigInt::toString`, §6.1.6.2.23). Not recorded.
    pub(crate) fn bigint_to_string(&mut self, x: Gc<BigInt>, radix: u32) -> VmResult<Gc<JsString>> {
        let digits = self.bigint_digits(x, radix)?;
        Ok(self.heap.alloc_string(String16::from_latin1(digits))?)
    }

    /// The ASCII digits of a `BigInt` in a radix from 2 to 36, charged.
    pub(crate) fn bigint_digits(&mut self, x: Gc<BigInt>, radix: u32) -> VmResult<Vec<u8>> {
        if !(2..=36).contains(&radix) {
            return Err(VmError::invariant("a BigInt radix outside 2 to 36"));
        }
        let value = self.heap.bigint(x)?;
        self.vm.charge(steps(cost::to_string(value, radix)))?;
        bigint::to_string_radix(value, radix).map_err(bigint_error)
    }

    /// The text of `BigInt.prototype.toLocaleString` without ECMA-402:
    /// the decimal digits in groups of three (en-US, as Chromium prints
    /// with its default locale). Not recorded.
    pub(crate) fn bigint_locale_string(&mut self, x: Gc<BigInt>) -> VmResult<Gc<JsString>> {
        let digits = self.bigint_digits(x, 10)?;
        self.charge_units(digits.len())?;
        Ok(self
            .heap
            .alloc_string(String16::from_latin1(group_digits(&digits)))?)
    }

    /// The decimal text of a `BigInt` for messages and `console.log`. The
    /// conversion is quadratic (35 ms at the size limit), so it is
    /// charged before the work, without a time check (the callers hold a
    /// shared borrow): the next step of the countdown runs the check.
    pub(crate) fn bigint_text(&self, x: Gc<BigInt>) -> String {
        let Ok(value) = self.heap.bigint(x) else {
            return String::new();
        };
        self.vm.charge_deferred(steps(cost::to_string(value, 10)));
        bigint::to_string_radix(value, 10)
            .map(|digits| digits.into_iter().map(char::from).collect())
            .unwrap_or_default()
    }

    /// `StringToBigInt` (§7.1.14) of a string value: `None` for a text
    /// that is not a `StringIntegerLiteral`. Charged before the scan; a
    /// rope is flattened without a collection.
    pub(crate) fn string_to_bigint(&mut self, string: Gc<JsString>) -> VmResult<Option<BigInt>> {
        self.flatten_no_gc(string)?;
        let units = self.heap.string(string)?.len();
        self.charge_bigint(cost::parse(units))?;
        match bigint::string_to_bigint(self.heap.text(string)?) {
            Ok(value) => Ok(Some(value)),
            Err(BigIntError::InvalidSyntax) => Ok(None),
            Err(error) => Err(bigint_error(error)),
        }
    }

    /// The order of a `BigInt` and the value of a string by
    /// `StringToBigInt`; `None` when the text is not a
    /// `StringIntegerLiteral` (undefined). Charged before the scan.
    pub(crate) fn compare_bigint_string(
        &mut self,
        x: Gc<BigInt>,
        string: Gc<JsString>,
    ) -> VmResult<Option<Ordering>> {
        self.flatten_no_gc(string)?;
        let units = self.heap.string(string)?.len();
        let value = self.heap.bigint(x)?;
        let units = cost::parse(units).saturating_add(cost::linear(value));
        self.vm.charge(steps(units))?;
        let text = self.heap.text(string)?;
        bigint::compare_text(self.heap.bigint(x)?, text).map_err(bigint_error)
    }

    /// The order of a `BigInt` and a Number; `None` for NaN (charged).
    pub(crate) fn compare_bigint_number(
        &mut self,
        x: Gc<BigInt>,
        y: f64,
    ) -> VmResult<Option<Ordering>> {
        let value = self.heap.bigint(x)?;
        self.vm.charge(steps(cost::linear(value)))?;
        Ok(bigint::compare_f64(value, y))
    }

    /// The order of two `BigInt`s (charged).
    pub(crate) fn compare_bigints(&mut self, x: Gc<BigInt>, y: Gc<BigInt>) -> VmResult<Ordering> {
        let (a, b) = (self.heap.bigint(x)?, self.heap.bigint(y)?);
        let longer = if a.limbs().len() >= b.limbs().len() {
            a
        } else {
            b
        };
        self.vm.charge(steps(cost::linear(longer)))?;
        Ok(a.cmp(b))
    }

    /// `Number::toBigInt` of `NumberToBigInt` (§21.2.1.1.1): a
    /// `RangeError` for a Number that is not an integer. Not recorded.
    pub(crate) fn number_to_bigint(&mut self, number: f64) -> VmResult<Gc<BigInt>> {
        let value = bigint::from_f64(number).map_err(|error| match error {
            BigIntError::NotIntegral => VmError::range_error(format!(
                "The number {} cannot be converted to a BigInt because it is not an integer",
                number_to_string(number)
            )),
            other => bigint_error(other),
        })?;
        self.charge_bigint(cost::linear(&value))?;
        self.new_bigint(value)
    }

    /// The `Number` of a `BigInt` (`Number(bigint)`, §21.1.1.1 step 2.a):
    /// the nearest double, ties to even.
    pub(crate) fn bigint_to_number(&mut self, x: Gc<BigInt>) -> VmResult<f64> {
        let value = self.heap.bigint(x)?;
        self.vm.charge(steps(cost::linear(value)))?;
        Ok(bigint::to_f64(value))
    }

    /// `ToBigInt` (§7.1.13). The result is recorded in the current handle
    /// scope.
    pub(crate) fn to_bigint(&mut self, value: Value) -> VmResult<Gc<BigInt>> {
        let primitive = self.to_primitive(value, Hint::Number)?;
        self.heap.record(primitive);
        self.primitive_to_bigint(primitive)
    }

    /// Steps 2 and 3 of `ToBigInt` for a primitive. The result is
    /// recorded in the current handle scope.
    pub(crate) fn primitive_to_bigint(&mut self, primitive: Value) -> VmResult<Gc<BigInt>> {
        let result = match primitive {
            Value::BigInt(b) => return Ok(b),
            Value::Bool(b) => self.bigint_from_i64(i64::from(b))?,
            Value::String(s) => match self.string_to_bigint(s)? {
                Some(value) => self.new_bigint(value)?,
                None => return Err(self.string_not_bigint(s)),
            },
            Value::Undefined
            | Value::Null
            | Value::Int(_)
            | Value::Double(_)
            | Value::Symbol(_) => {
                return Err(VmError::type_error(format!(
                    "Cannot convert {} to a BigInt",
                    self.value_text(primitive)
                )));
            }
            Value::Object(_) | Value::Empty | Value::Cell(_) => {
                return Err(VmError::invariant(
                    "ToBigInt of a value that is not a primitive",
                ));
            }
        };
        Ok(self.heap.record(result))
    }

    /// The `SyntaxError` of a string that is not a `StringIntegerLiteral`
    /// (V8 quotes at most 1,000 code units, then `…`).
    pub(crate) fn string_not_bigint(&self, string: Gc<JsString>) -> VmError {
        let mut text = self.heap.string_lossy(string, QUOTE_LIMIT);
        if self
            .heap
            .string(string)
            .is_ok_and(|s| s.len() > QUOTE_LIMIT)
        {
            text.push('…');
        }
        VmError::syntax_error(format!("Cannot convert {text} to a BigInt"))
    }

    /// `ToBigInt64` (§7.1.15): `ToBigInt`, then the value modulo 2^64 as
    /// a signed integer.
    pub(crate) fn to_big_int64(&mut self, value: Value) -> VmResult<i64> {
        let b = self.to_bigint(value)?;
        Ok(self.heap.bigint(b)?.to_i64_wrapping())
    }

    /// `ToBigUint64` (§7.1.16): `ToBigInt`, then the value modulo 2^64.
    pub(crate) fn to_big_uint64(&mut self, value: Value) -> VmResult<u64> {
        let b = self.to_bigint(value)?;
        Ok(self.heap.bigint(b)?.to_u64_wrapping())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeConfig;

    #[test]
    fn digits_group_by_three() {
        let group = |text: &str| String::from_utf8(group_digits(text.as_bytes())).unwrap();
        assert_eq!(group("0"), "0");
        assert_eq!(group("999"), "999");
        assert_eq!(group("1000"), "1,000");
        assert_eq!(group("-1234567"), "-1,234,567");
        assert_eq!(group("123456"), "123,456");
    }

    #[test]
    fn library_errors_have_v8_messages() {
        let message = |error| match bigint_error(error) {
            VmError::Raise { message, .. } => message.into_owned(),
            other => format!("{other:?}"),
        };
        assert_eq!(
            message(BigIntError::TooLarge),
            "Maximum BigInt size exceeded"
        );
        assert_eq!(message(BigIntError::DivisionByZero), "Division by zero");
        assert_eq!(
            message(BigIntError::NegativeExponent),
            "Exponent must be positive"
        );
        assert!(matches!(
            bigint_error(BigIntError::OutOfMemory),
            VmError::Terminated(Termination::OutOfMemory)
        ));
    }

    /// `ToBigInt64` and `ToBigUint64` wrap modulo 2^64 and convert like
    /// `ToBigInt` (feature 11 calls them for the 64-bit typed arrays).
    #[test]
    fn to_big_int64_wraps() {
        let mut rt = Runtime::new(RuntimeConfig::default()).expect("a runtime");
        let cases: [(&str, i64, u64); 6] = [
            ("1n", 1, 1),
            ("-1n", -1, u64::MAX),
            ("2n ** 64n + 5n", 5, 5),
            ("2n ** 63n", i64::MIN, 1 << 63),
            ("true", 1, 1),
            ("'0xFFFFFFFFFFFFFFFF'", -1, u64::MAX),
        ];
        for (source, signed, unsigned) in cases {
            let value = rt.eval(source).expect("the script runs");
            let result = rt.scoped(|rt| Ok((rt.to_big_int64(value)?, rt.to_big_uint64(value)?)));
            assert_eq!(result.ok(), Some((signed, unsigned)), "{source}");
        }
        for source in ["1", "undefined", "'x'", "Symbol()"] {
            let value = rt.eval(source).expect("the script runs");
            assert!(rt.scoped(|rt| rt.to_big_int64(value)).is_err(), "{source}");
        }
    }
}
