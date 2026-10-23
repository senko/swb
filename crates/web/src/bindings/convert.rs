//! The conversions of JavaScript values to IDL values (Web IDL §3.2,
//! <https://webidl.spec.whatwg.org/#es-type-mapping>), with Chromium's
//! messages (measured in `tools/probes/web-bindings.json`).
//!
//! A conversion that fails because the value has the wrong type returns
//! [`Exception::NotOfType`]; the glue words it by the place of the value.
//! The errors that a conversion raises itself (`ToString` of a Symbol,
//! `[EnforceRange]`, a non-finite `double`) are `TypeError`s that get the
//! member's prefix, as in Chromium; a value that script code throws
//! (`valueOf`, a getter) passes unchanged.
//!
//! Strings: the DOM stores UTF-8, so `DOMString` and `USVString` both
//! convert unpaired surrogates to U+FFFD (ADR 0028 section 3; a deviation
//! for `DOMString`).

use std::collections::HashMap;
use std::hash::Hash;

use swb_js::{Gc, Object, Property, PropertyKey, Runtime, Value, VmError};

use super::{Exception, NodeId, Platform, Result, node_of};

/// The longest sequence that a conversion builds; a longer one is a
/// `RangeError` (a limit of swb, so that an endless iterator cannot fill
/// the memory before the time limit ends it).
pub const MAX_SEQUENCE: usize = 1 << 24;

/// The extended attribute of an integer type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntMode {
    /// No attribute: modulo arithmetic.
    Plain,
    /// `[Clamp]`.
    Clamp,
    /// `[EnforceRange]`.
    EnforceRange,
}

/// `ToNumber` with the messages of the bindings for a Symbol and a
/// `BigInt`.
pub fn number(rt: &mut Runtime, value: Value) -> Result<f64> {
    match value {
        Value::Int(i) => Ok(f64::from(i)),
        Value::Double(d) => Ok(d),
        Value::Symbol(_) => Err(Exception::type_error(
            "Cannot convert a Symbol value to a number",
        )),
        Value::BigInt(_) => Err(Exception::type_error(
            "Cannot convert a BigInt value to a number",
        )),
        _ => Ok(rt.number_of(value)?),
    }
}

/// `ConvertToInt(V, bitLength, signedness)` (Web IDL §3.2.4.9); `name` is
/// the IDL type for the messages of `[EnforceRange]`.
fn convert_to_int(
    rt: &mut Runtime,
    value: Value,
    bits: u32,
    signed: bool,
    mode: IntMode,
    name: &str,
) -> Result<f64> {
    let (lower, upper) = if bits == 64 {
        let bound = 2f64.powi(53) - 1.0;
        (if signed { -bound } else { 0.0 }, bound)
    } else if signed {
        let half = 2f64.powi(bits as i32 - 1);
        (-half, half - 1.0)
    } else {
        (0.0, 2f64.powi(bits as i32) - 1.0)
    };
    let mut x = number(rt, value)?;
    if x == 0.0 {
        x = 0.0;
    }
    match mode {
        IntMode::EnforceRange => {
            if x.is_nan() {
                return Err(Exception::type_error(format!(
                    "Value is not of type '{name}'."
                )));
            }
            if x.is_infinite() {
                return Err(Exception::type_error(format!(
                    "Value is infinite and not of type '{name}'."
                )));
            }
            let x = x.trunc();
            if x < lower || x > upper {
                return Err(Exception::type_error(format!(
                    "Value is outside the '{name}' value range."
                )));
            }
            Ok(x + 0.0)
        }
        IntMode::Clamp if !x.is_nan() => {
            let x = x.clamp(lower, upper).round_ties_even();
            Ok(x + 0.0)
        }
        _ => {
            if !x.is_finite() {
                return Ok(0.0);
            }
            let modulus = 2f64.powi(bits as i32);
            let mut x = x.trunc().rem_euclid(modulus);
            if signed && x >= modulus / 2.0 {
                x -= modulus;
            }
            Ok(x + 0.0)
        }
    }
}

/// `byte`.
pub fn byte(rt: &mut Runtime, value: Value, mode: IntMode) -> Result<i8> {
    Ok(convert_to_int(rt, value, 8, true, mode, "byte")? as i8)
}

/// `octet`.
pub fn octet(rt: &mut Runtime, value: Value, mode: IntMode) -> Result<u8> {
    Ok(convert_to_int(rt, value, 8, false, mode, "octet")? as u8)
}

/// `short`.
pub fn short(rt: &mut Runtime, value: Value, mode: IntMode) -> Result<i16> {
    Ok(convert_to_int(rt, value, 16, true, mode, "short")? as i16)
}

/// `unsigned short`.
pub fn unsigned_short(rt: &mut Runtime, value: Value, mode: IntMode) -> Result<u16> {
    Ok(convert_to_int(rt, value, 16, false, mode, "unsigned short")? as u16)
}

/// `long`.
pub fn long(rt: &mut Runtime, value: Value, mode: IntMode) -> Result<i32> {
    Ok(convert_to_int(rt, value, 32, true, mode, "long")? as i32)
}

/// `unsigned long`.
pub fn unsigned_long(rt: &mut Runtime, value: Value, mode: IntMode) -> Result<u32> {
    Ok(convert_to_int(rt, value, 32, false, mode, "unsigned long")? as u32)
}

/// `long long`. Without an extended attribute the result is modulo 2^64
/// of the exact integer part; values beyond 2^63 lose precision only as
/// far as the Number had none.
pub fn long_long(rt: &mut Runtime, value: Value, mode: IntMode) -> Result<i64> {
    if mode != IntMode::Plain {
        return Ok(convert_to_int(rt, value, 64, true, mode, "long long")? as i64);
    }
    let x = number(rt, value)?;
    Ok(modulo_64(x) as i64)
}

/// `unsigned long long`.
pub fn unsigned_long_long(rt: &mut Runtime, value: Value, mode: IntMode) -> Result<u64> {
    if mode != IntMode::Plain {
        return Ok(convert_to_int(rt, value, 64, false, mode, "unsigned long long")? as u64);
    }
    let x = number(rt, value)?;
    Ok(modulo_64(x))
}

/// `IntegerPart(x)` modulo 2^64 (Web IDL §3.2.4.9 steps 9 to 11).
fn modulo_64(x: f64) -> u64 {
    if !x.is_finite() {
        return 0;
    }
    // A Number of 2^117 or more is a multiple of 2^64 (its last bit is
    // worth at least 2^65), so it is 0 modulo 2^64; below that, the integer
    // part fits an i128 exactly.
    let x = x.trunc();
    if x.abs() >= 2f64.powi(117) {
        return 0;
    }
    (x as i128).rem_euclid(1 << 64) as u64
}

/// `float`: a `TypeError` for a non-finite value or one beyond the float
/// range.
pub fn float(rt: &mut Runtime, value: Value) -> Result<f32> {
    let x = number(rt, value)?;
    let y = x as f32;
    if !x.is_finite() || y.is_infinite() {
        return Err(Exception::type_error(
            "The provided float value is non-finite.",
        ));
    }
    Ok(y)
}

/// `unrestricted float`.
pub fn unrestricted_float(rt: &mut Runtime, value: Value) -> Result<f32> {
    Ok(number(rt, value)? as f32)
}

/// `double`: a `TypeError` for a non-finite value.
pub fn double(rt: &mut Runtime, value: Value) -> Result<f64> {
    let x = number(rt, value)?;
    if !x.is_finite() {
        return Err(Exception::type_error(
            "The provided double value is non-finite.",
        ));
    }
    Ok(x)
}

/// `unrestricted double`.
pub fn unrestricted_double(rt: &mut Runtime, value: Value) -> Result<f64> {
    number(rt, value)
}

/// `boolean` (`ToBoolean`).
pub fn boolean(rt: &mut Runtime, value: Value) -> Result<bool> {
    Ok(rt.truthy(value)?)
}

/// `DOMString` (`ToString`; unpaired surrogates become U+FFFD).
pub fn dom_string(rt: &mut Runtime, value: Value) -> Result<String> {
    if let Value::Symbol(_) = value {
        return Err(Exception::type_error(
            "Cannot convert a Symbol value to a string",
        ));
    }
    Ok(rt.to_rust_string(value)?)
}

/// `[LegacyNullToEmptyString] DOMString`: `null` is the empty string.
pub fn dom_string_null_empty(rt: &mut Runtime, value: Value) -> Result<String> {
    if value == Value::Null {
        return Ok(String::new());
    }
    dom_string(rt, value)
}

/// `USVString`.
pub fn usv_string(rt: &mut Runtime, value: Value) -> Result<String> {
    dom_string(rt, value)
}

/// `ByteString`: a `TypeError` for a code unit above U+00FF.
pub fn byte_string(rt: &mut Runtime, value: Value) -> Result<String> {
    let text = dom_string(rt, value)?;
    if text.chars().any(|c| u32::from(c) > 0xFF) {
        return Err(Exception::type_error(
            "String contains non ISO-8859-1 code point.",
        ));
    }
    Ok(text)
}

/// `object`.
pub fn object(value: Value) -> Result<Gc<Object>> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(Exception::NotOfType("object".into())),
    }
}

/// An interface type that inherits from `Node`: the node of a wrapper of
/// `class` or of a subclass.
pub fn node(
    rt: &Runtime,
    value: Value,
    class: swb_js::ClassId,
    name: &'static str,
) -> Result<NodeId> {
    if rt.implements(value, class)
        && let Some((_, data)) = rt.host_object(value)
        && let Some(node) = node_of(data)
    {
        return Ok(node);
    }
    Err(Exception::NotOfType(name.into()))
}

/// Another interface type: a platform object of `class` or of a subclass.
pub fn platform(
    rt: &Runtime,
    value: Value,
    class: swb_js::ClassId,
    name: &'static str,
) -> Result<Platform> {
    if let Value::Object(object) = value
        && rt.implements(value, class)
        && let Some((_, data)) = rt.host_object(value)
    {
        return Ok(Platform { object, data });
    }
    Err(Exception::NotOfType(name.into()))
}

/// Whether a value is a platform object (a host object).
pub fn is_platform_object(rt: &Runtime, value: Value) -> bool {
    rt.host_object(value).is_some()
}

/// A callback function type: a callable object (Chromium names the type
/// `Function`).
pub fn callback_function(rt: &Runtime, value: Value) -> Result<Gc<Object>> {
    match value {
        Value::Object(object) if rt.callable(value)? => Ok(object),
        _ => Err(Exception::NotOfType("Function".into())),
    }
}

/// A callback interface type: any object (Chromium names the type
/// `Object`).
pub fn callback_interface(value: Value) -> Result<Gc<Object>> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(Exception::NotOfType("Object".into())),
    }
}

/// A promise type: `PromiseResolve(%Promise%, V)` of `realm` (Web IDL
/// §3.2.24).
pub fn promise(rt: &mut Runtime, realm: u32, value: Value) -> Result<Gc<Object>> {
    match rt.resolved_promise(realm, value)? {
        Value::Object(object) => Ok(object),
        _ => Err(Exception::type_error("PromiseResolve gave a non-object")),
    }
}

/// An enumeration: the index of the value of `ToString(V)` in `values`,
/// or Chromium's `TypeError`.
pub fn enumeration(rt: &mut Runtime, value: Value, values: &[&str], name: &str) -> Result<usize> {
    let text = dom_string(rt, value)?;
    values.iter().position(|v| *v == text).ok_or_else(|| {
        Exception::type_error(format!(
            "The provided value '{text}' is not a valid enum value of type {name}."
        ))
    })
}

/// `GetMethod(V, @@iterator)` for an object (the test of a union or an
/// overload for a sequence type): `None` if it is undefined or null; a
/// `TypeError` if it is not callable.
pub fn iterator_method(rt: &mut Runtime, value: Value) -> Result<Option<Value>> {
    let key = rt.iterator_key();
    let method = rt.get_key(value, key)?;
    if method.is_nullish() {
        return Ok(None);
    }
    if !rt.callable(method)? {
        return Err(Exception::type_error("@@iterator must be a callable."));
    }
    Ok(Some(method))
}

/// `sequence<T>` (Web IDL §3.2.21), each element converted with `f` as it
/// is taken from the iterator.
pub fn sequence<T>(
    rt: &mut Runtime,
    value: Value,
    f: impl FnMut(&mut Runtime, Value) -> Result<T>,
) -> Result<Vec<T>> {
    if !matches!(value, Value::Object(_)) {
        return Err(Exception::type_error(
            "The provided value cannot be converted to a sequence.",
        ));
    }
    let Some(method) = iterator_method(rt, value)? else {
        return Err(Exception::type_error(
            "The object must have a callable @@iterator property.",
        ));
    };
    sequence_from_method(rt, value, method, f)
}

/// Creating a sequence from an iterable with its `@@iterator` method
/// (Web IDL §3.2.21).
pub fn sequence_from_method<T>(
    rt: &mut Runtime,
    value: Value,
    method: Value,
    mut f: impl FnMut(&mut Runtime, Value) -> Result<T>,
) -> Result<Vec<T>> {
    let mut iterator = rt.iterator_from_method(value, method)?;
    let mut items = Vec::new();
    while let Some(item) = rt.iterator_next(&mut iterator)? {
        if items.len() >= MAX_SEQUENCE {
            return Err(Exception::range_error("The sequence is too long."));
        }
        items.push(f(rt, item).map_err(Exception::nested)?);
    }
    Ok(items)
}

/// `record<K, V>` (Web IDL §3.2.23): the enumerable own properties in the
/// order of `[[OwnPropertyKeys]]`, the key converted with `fk` (from the
/// key as a String or Symbol value), the value with `fv`. Two property
/// keys can convert to one typed key (lone surrogates become U+FFFD): the
/// later value replaces the earlier one in its place.
pub fn record<K: Eq + Hash + Clone, V>(
    rt: &mut Runtime,
    value: Value,
    mut fk: impl FnMut(&mut Runtime, Value) -> Result<K>,
    mut fv: impl FnMut(&mut Runtime, Value) -> Result<V>,
) -> Result<Vec<(K, V)>> {
    let Value::Object(object) = value else {
        return Err(Exception::type_error(
            "Only objects can be converted to record<K,V> types",
        ));
    };
    let keys = rt.own_keys_of(object)?;
    let mut entries: Vec<(K, V)> = Vec::new();
    let mut positions: HashMap<K, usize> = HashMap::new();
    for key in keys {
        let enumerable = match rt.own_property_of(object, key)? {
            Some(Property::Data { enumerable, .. } | Property::Accessor { enumerable, .. }) => {
                enumerable
            }
            None => false,
        };
        if !enumerable {
            continue;
        }
        let key_value = key_value(rt, key)?;
        let typed_key = fk(rt, key_value).map_err(Exception::nested)?;
        let item = rt.get_key(value, key)?;
        let typed_value = fv(rt, item).map_err(Exception::nested)?;
        if let Some(entry) = positions.get(&typed_key).and_then(|&i| entries.get_mut(i)) {
            entry.1 = typed_value;
            continue;
        }
        if entries.len() >= MAX_SEQUENCE {
            return Err(Exception::range_error("The record is too large."));
        }
        positions.insert(typed_key.clone(), entries.len());
        entries.push((typed_key, typed_value));
    }
    Ok(entries)
}

/// A property key as a value (an index key becomes its string).
fn key_value(rt: &mut Runtime, key: PropertyKey) -> Result<Value> {
    let value = rt.heap_mut().key_to_value(key).map_err(VmError::from)?;
    if let Value::String(_) = value {
        super::keep(rt, value);
    }
    Ok(value)
}

/// The first step of a dictionary conversion (Web IDL §3.2.17): the
/// object, `None` for `undefined` and `null`, or Chromium's `TypeError`
/// for another value.
pub fn dictionary_object(value: Value, name: &str) -> Result<Option<Gc<Object>>> {
    match value {
        Value::Undefined | Value::Null => Ok(None),
        Value::Object(object) => Ok(Some(object)),
        _ => Err(Exception::type_error(format!(
            "The provided value is not of type '{name}'."
        ))),
    }
}

/// `Get(jsDict, key)` of a dictionary member, or `undefined` without an
/// object.
pub fn member(rt: &mut Runtime, dict: Option<Gc<Object>>, key: &str) -> Result<Value> {
    let Some(object) = dict else {
        return Ok(Value::Undefined);
    };
    let key = rt.key_from_str(key)?;
    Ok(rt.get_key(object.into(), key)?)
}

/// "Required member is undefined." of a required dictionary member.
pub fn required_member(member: &str, dictionary: &str) -> Exception {
    Exception::type_error("Required member is undefined.").member(member, dictionary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_js::RuntimeConfig;

    fn rt() -> Runtime {
        Runtime::new(RuntimeConfig::default()).unwrap()
    }

    fn int(rt: &mut Runtime, x: f64, bits: u32, signed: bool, mode: IntMode) -> String {
        match convert_to_int(rt, Value::number(x), bits, signed, mode, "long") {
            Ok(v) => format!("{v}"),
            Err(Exception::Type(m)) => m.into_owned(),
            Err(other) => format!("{other:?}"),
        }
    }

    #[test]
    fn integer_conversions() {
        let mut rt = rt();
        let plain: Vec<String> = [
            1.9,
            -1.9,
            2f64.powi(32) + 5.0,
            2f64.powi(31),
            -(2f64.powi(31)) - 1.0,
            f64::NAN,
            f64::INFINITY,
            -0.0,
        ]
        .into_iter()
        .map(|x| int(&mut rt, x, 32, true, IntMode::Plain))
        .collect();
        assert_eq!(
            plain,
            ["1", "-1", "5", "-2147483648", "2147483647", "0", "0", "0"]
        );
        let clamp: Vec<String> = [70000.0, -5.0, 2999.5, 3000.5, 3001.5, f64::NAN, -0.5]
            .into_iter()
            .map(|x| int(&mut rt, x, 16, false, IntMode::Clamp))
            .collect();
        assert_eq!(clamp, ["65535", "0", "3000", "3000", "3002", "0", "0"]);
        let enforce: Vec<String> = [f64::NAN, f64::INFINITY, 2f64.powi(31), 1.9]
            .into_iter()
            .map(|x| int(&mut rt, x, 32, true, IntMode::EnforceRange))
            .collect();
        assert_eq!(
            enforce,
            [
                "Value is not of type 'long'.",
                "Value is infinite and not of type 'long'.",
                "Value is outside the 'long' value range.",
                "1"
            ]
        );
        assert_eq!(modulo_64(-1.0), u64::MAX);
        assert_eq!(modulo_64(2f64.powi(64) + 4096.0), 4096);
    }

    #[test]
    fn number_of_symbols_and_bigints() {
        let mut rt = rt();
        let value = rt.eval("Symbol()").unwrap();
        let Err(Exception::Type(message)) = number(&mut rt, value) else {
            panic!()
        };
        assert_eq!(message, "Cannot convert a Symbol value to a number");
        let value = rt.eval("1n").unwrap();
        assert!(matches!(number(&mut rt, value), Err(Exception::Type(_))));
    }
}
