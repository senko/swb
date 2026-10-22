//! `JSON` (ECMA-262 §25.5; `rawJSON` and `isRawJSON` from the source text
//! access proposal, <https://tc39.es/proposal-json-parse-with-source/>,
//! which Chromium 148 has): `parse`, `stringify`, `rawJSON`, `isRawJSON`
//! and `@@toStringTag`.
//!
//! The text parser is in [`super::json_parse`], the reviver walk in
//! [`super::json_revive`], the serializer in [`super::json_stringify`].
//! All three keep their nesting on a Rust vector and not on the Rust
//! stack.

use super::json_parse;
use super::json_revive;
use super::json_stringify;
use crate::object::ObjectKind;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

/// The message of the `SyntaxError` for a `rawJSON` text with whitespace
/// at an end, or more than one value.
const INVALID_RAW_JSON: &str = "Invalid value for JSON.rawJSON";

/// Installs `JSON` in the order of Chromium's `Reflect.ownKeys`.
pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let json = rt.heap.new_object(Some(proto))?;
    rt.heap.record(json);
    super::method(rt, realm, json, "parse", 2, parse)?;
    super::method(rt, realm, json, "stringify", 3, json_stringify::stringify)?;
    super::method(rt, realm, json, "rawJSON", 1, raw_json)?;
    super::method(rt, realm, json, "isRawJSON", 1, is_raw_json)?;
    super::to_string_tag(rt, json, "JSON")?;
    super::define_global(rt, realm, "JSON", json.into())
}

/// `JSON.parse(text, reviver)` (§25.5.1).
fn parse(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let text = rt.arg(call, 0);
    let reviver = rt.arg(call, 1);
    let text = rt.to_string(text)?;
    rt.heap.record(text);
    let callable = rt.is_callable(reviver)?;
    let parsed = json_parse::parse(rt, text, call.realm(), callable)?;
    if !callable {
        return Ok(NativeReturn::Value(parsed.value));
    }
    let proto = rt.intrinsic(call.realm(), Intrinsic::ObjectPrototype)?;
    let root = rt.heap.new_object(Some(proto))?;
    rt.heap.record(root);
    let empty = PropertyKey::String(rt.vm.atoms.empty);
    rt.create_data_property_or_throw(root, empty, parsed.value)?;
    let result = json_revive::internalize(rt, root, reviver, parsed.records, text, call.realm())?;
    Ok(NativeReturn::Value(result))
}

/// `JSON.rawJSON(text)`: a frozen object with a null prototype and the
/// one property `rawJSON`, the text of a JSON primitive.
fn raw_json(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let text = rt.arg(call, 0);
    let text = rt.to_string(text)?;
    rt.heap.record(text);
    let length = rt.string_len(text)?;
    let invalid = || VmError::syntax_error(INVALID_RAW_JSON);
    if length == 0 {
        return Err(invalid());
    }
    let is_blank = |unit: Option<u16>| matches!(unit, Some(0x09 | 0x0a | 0x0d | 0x20));
    let last = rt.unit_at(text, length - 1)?;
    if is_blank(last) {
        return Err(invalid());
    }
    // A leading blank, `{` or `[` is the parser's unexpected token.
    if !json_parse::check_primitive(rt, text)? {
        return Err(invalid());
    }
    let object = rt.heap.new_object_with_kind(None, ObjectKind::RawJson)?;
    rt.heap.record(object);
    let key = rt.heap.key_from_str("rawJSON")?;
    rt.heap.record(key);
    rt.define(object, key, Value::String(text), false, true, false)?;
    rt.heap.prevent_extensions(object)?;
    Ok(NativeReturn::Value(object.into()))
}

/// `JSON.isRawJSON(value)`: whether the value is an object made by
/// `rawJSON`.
fn is_raw_json(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let raw = match rt.arg(call, 0) {
        Value::Object(object) => matches!(rt.heap.object(object)?.kind, ObjectKind::RawJson),
        _ => false,
    };
    Ok(NativeReturn::Value(Value::Bool(raw)))
}
