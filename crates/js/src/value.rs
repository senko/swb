//! Values (ADR 0026 section 7, memo 4.1) and the small heap kinds that
//! values refer to: symbols, `BigInt`s, cells.
//!
//! A value is a Rust enum of 16 bytes. Numbers have two forms: a 32-bit
//! integer for integral results and a double for all others (including
//! `-0`). The two forms are the same Number value where the
//! specification says so; the equality helpers here compare them
//! numerically.

use crate::error::Result;
use crate::heap::{Gc, Heap};
use crate::object::Object;
use crate::string::JsString;

/// A JavaScript value, or the internal `Empty` marker.
///
/// `==` (the derived `PartialEq`) compares representations: `Int(1)` is
/// not equal to `Double(1.0)`, and `Double(NaN)` is not equal to itself.
/// It is for tests and for code that wants identical representations. The
/// language's equality (`SameValue`, `SameValueZero`, `===`) is
/// [`Heap::equal`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Value {
    /// `undefined`.
    #[default]
    Undefined,
    /// `null`.
    Null,
    /// A Boolean.
    Bool(bool),
    /// A Number that is an integer in the `i32` range (never `-0`).
    Int(i32),
    /// Any other Number (also integral values from the interpreter's
    /// double paths).
    Double(f64),
    /// A String.
    String(Gc<JsString>),
    /// A Symbol.
    Symbol(Gc<Symbol>),
    /// A `BigInt`.
    BigInt(Gc<BigInt>),
    /// An Object.
    Object(Gc<Object>),
    /// Internal: an uninitialized binding or an array hole. It never
    /// reaches a script: every path out of the VM and the object model
    /// converts or rejects it.
    Empty,
    /// Internal: the cell of a captured binding, held in a register of the
    /// frame that declares it (ADR 0026 section 3). Only the interpreter
    /// reads it; it never reaches a script or the object model.
    Cell(Gc<ValueCell>),
}

impl Value {
    /// A Number, in the integer form when the value is an integer in the
    /// `i32` range and not `-0` (the form that built-in functions and the
    /// API produce).
    pub fn number(value: f64) -> Value {
        let int = value as i32;
        if f64::from(int) == value && !(int == 0 && value.is_sign_negative()) {
            Value::Int(int)
        } else {
            Value::Double(value)
        }
    }

    /// The Number value as a double, if this is a Number.
    pub fn as_number(self) -> Option<f64> {
        match self {
            Value::Int(int) => Some(f64::from(int)),
            Value::Double(double) => Some(double),
            _ => None,
        }
    }

    /// The object handle, if this is an Object.
    pub fn as_object(self) -> Option<Gc<Object>> {
        match self {
            Value::Object(object) => Some(object),
            _ => None,
        }
    }

    /// Whether this is `undefined`.
    pub fn is_undefined(self) -> bool {
        matches!(self, Value::Undefined)
    }

    /// Whether this is `undefined` or `null`.
    pub fn is_nullish(self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    /// Whether this is the internal `Empty` marker.
    pub fn is_empty(self) -> bool {
        matches!(self, Value::Empty)
    }

    /// Whether this is one of the internal variants (`Empty`, `Cell`),
    /// which never reach a script or the object model.
    pub fn is_internal(self) -> bool {
        matches!(self, Value::Empty | Value::Cell(_))
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Int(value)
    }
}

impl From<f64> for Value {
    /// The Number in its integer form where there is one (see
    /// [`Value::number`]).
    fn from(value: f64) -> Self {
        Value::number(value)
    }
}

impl From<Gc<BigInt>> for Value {
    fn from(bigint: Gc<BigInt>) -> Self {
        Value::BigInt(bigint)
    }
}

impl From<Gc<Object>> for Value {
    fn from(object: Gc<Object>) -> Self {
        Value::Object(object)
    }
}

impl From<Gc<JsString>> for Value {
    fn from(string: Gc<JsString>) -> Self {
        Value::String(string)
    }
}

impl From<Gc<Symbol>> for Value {
    fn from(symbol: Gc<Symbol>) -> Self {
        Value::Symbol(symbol)
    }
}

/// A Symbol: a unique identity with a description (§6.1.5).
pub struct Symbol {
    /// The `[[Description]]`: a string, or `None` for `undefined`.
    pub description: Option<Gc<JsString>>,
}

/// A `BigInt` (placeholder until M7 feature 10): sign and magnitude in
/// 64-bit limbs, least significant first.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BigInt {
    /// Whether the value is negative.
    pub negative: bool,
    /// The magnitude, least significant limb first, without leading zero
    /// limbs.
    pub limbs: Vec<u64>,
}

/// A cell: the storage of a binding that a closure captures (ADR 0026
/// section 3). A register holds it as [`Value::Cell`].
#[derive(Debug, Default)]
pub struct ValueCell {
    /// The current value; `Empty` while the binding is uninitialized.
    pub value: Value,
}

/// The kind of comparison in [`Heap::equal`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Equality {
    /// `SameValue` (§7.2.9): `NaN` equals `NaN`, `+0` differs from `-0`.
    SameValue,
    /// `SameValueZero` (§7.2.10): `NaN` equals `NaN`, `+0` equals `-0`.
    SameValueZero,
    /// `IsStrictlyEqual` (§7.2.14): `NaN` differs from `NaN`, `+0` equals
    /// `-0`.
    Strict,
}

fn numbers_equal(mode: Equality, x: f64, y: f64) -> bool {
    match mode {
        // Number::sameValue (§6.1.6.1.14).
        Equality::SameValue => {
            if x.is_nan() && y.is_nan() {
                return true;
            }
            x == y && x.is_sign_negative() == y.is_sign_negative()
        }
        // Number::sameValueZero (§6.1.6.1.15).
        Equality::SameValueZero => (x.is_nan() && y.is_nan()) || x == y,
        // Number::equal (§6.1.6.1.13).
        Equality::Strict => x == y,
    }
}

impl Heap {
    /// Compares two values with `SameValue`, `SameValueZero` or
    /// `IsStrictlyEqual` (§7.2.9, §7.2.10, §7.2.14). The integer and double
    /// forms of a Number compare as the same Number. `Empty` equals only
    /// `Empty`.
    pub fn equal(&self, mode: Equality, a: Value, b: Value) -> Result<bool> {
        if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
            return Ok(numbers_equal(mode, x, y));
        }
        Ok(match (a, b) {
            (Value::Undefined, Value::Undefined)
            | (Value::Null, Value::Null)
            | (Value::Empty, Value::Empty) => true,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::String(x), Value::String(y)) => {
                x == y || self.string(x)?.as_str16() == self.string(y)?.as_str16()
            }
            (Value::Symbol(x), Value::Symbol(y)) => x == y,
            (Value::Object(x), Value::Object(y)) => x == y,
            (Value::Cell(x), Value::Cell(y)) => x == y,
            (Value::BigInt(x), Value::BigInt(y)) => x == y || self.bigint(x)? == self.bigint(y)?,
            _ => false,
        })
    }

    /// `SameValue` (§7.2.9).
    pub fn same_value(&self, a: Value, b: Value) -> Result<bool> {
        self.equal(Equality::SameValue, a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_into_values() {
        assert_eq!(Value::from(3.0), Value::Int(3));
        assert_eq!(Value::from(0.5), Value::Double(0.5));
        assert!(matches!(Value::from(-0.0), Value::Double(d) if d.is_sign_negative()));
        let mut heap = Heap::new(crate::heap::HeapConfig::default());
        let big = heap.alloc_bigint(BigInt::default()).unwrap();
        assert_eq!(Value::from(big), Value::BigInt(big));
        // `==` compares representations.
        assert_ne!(Value::Int(1), Value::Double(1.0));
        assert_ne!(Value::Double(f64::NAN), Value::Double(f64::NAN));
    }

    #[test]
    fn value_is_16_bytes() {
        assert_eq!(size_of::<Value>(), 16);
    }

    #[test]
    fn number_picks_the_integer_form() {
        assert_eq!(Value::number(3.0), Value::Int(3));
        assert!(matches!(Value::number(-0.0), Value::Double(d) if d.is_sign_negative()));
        assert!(matches!(Value::number(0.5), Value::Double(_)));
        assert!(matches!(Value::number(2_147_483_648.0), Value::Double(_)));
        assert_eq!(Value::number(-2_147_483_648.0), Value::Int(i32::MIN));
        assert!(matches!(Value::number(f64::NAN), Value::Double(_)));
    }

    #[test]
    fn number_equality_follows_the_three_algorithms() {
        let heap = Heap::new(crate::heap::HeapConfig::default());
        let nan = Value::Double(f64::NAN);
        let neg_zero = Value::Double(-0.0);
        let zero = Value::Int(0);
        let cases = [
            // (a, b, SameValue, SameValueZero, Strict)
            (nan, nan, true, true, false),
            (zero, neg_zero, false, true, true),
            (Value::Double(0.0), neg_zero, false, true, true),
            (zero, Value::Double(0.0), true, true, true),
            (Value::Int(7), Value::Double(7.0), true, true, true),
            (Value::Int(7), Value::Double(7.5), false, false, false),
            (Value::Undefined, Value::Null, false, false, false),
            (Value::Bool(true), Value::Int(1), false, false, false),
            (Value::Empty, Value::Undefined, false, false, false),
        ];
        for (a, b, same, zero_eq, strict) in cases {
            assert_eq!(
                heap.equal(Equality::SameValue, a, b),
                Ok(same),
                "{a:?} {b:?}"
            );
            assert_eq!(heap.equal(Equality::SameValueZero, a, b), Ok(zero_eq));
            assert_eq!(heap.equal(Equality::Strict, a, b), Ok(strict));
        }
    }

    #[test]
    fn strings_compare_by_content() {
        let mut heap = Heap::new(crate::heap::HeapConfig::default());
        let a = heap.alloc_str("abc").unwrap();
        let b = heap
            .alloc_string(swb_js_text::String16::from_units(&[0x61, 0x62, 0x63]))
            .unwrap();
        let c = heap.alloc_str("abd").unwrap();
        assert_eq!(heap.same_value(a.into(), b.into()), Ok(true));
        assert_eq!(heap.equal(Equality::Strict, a.into(), c.into()), Ok(false));
        let s1 = heap.alloc_symbol(None).unwrap();
        let s2 = heap.alloc_symbol(None).unwrap();
        assert_eq!(heap.same_value(s1.into(), s1.into()), Ok(true));
        assert_eq!(heap.same_value(s1.into(), s2.into()), Ok(false));
    }
}
