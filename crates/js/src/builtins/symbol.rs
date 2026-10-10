//! `Symbol` (ECMA-262 §20.4): the constructor, `Symbol.for`,
//! `Symbol.keyFor`, the well-known symbol properties and
//! `Symbol.prototype` (`toString`, `valueOf`, the `description` getter,
//! `@@toPrimitive` and `@@toStringTag`).
//!
//! The well-known symbols and the registry belong to the runtime (see
//! [`crate::vm::Symbols`]), so all realms share them. The conversions of
//! a symbol to a string or a number throw (in `vm/convert.rs`); `String`
//! gives the descriptive string (in `primitive.rs`).

use swb_js_text::String16;

use crate::heap::Gc;
use crate::object::{ObjectKind, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::{Symbol, Value};
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WELL_KNOWN, WellKnown};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::SymbolPrototype)?;
    let symbol = super::constructor(rt, realm, "Symbol", 0, symbol, proto)?;
    super::method(rt, realm, symbol, "for", 1, symbol_for)?;
    super::method(rt, realm, symbol, "keyFor", 1, key_for)?;
    for (which, name) in WELL_KNOWN {
        let key = rt.heap.key_from_str(name)?;
        let value = rt.well_known(which);
        rt.define(symbol, key, value.into(), false, false, false)?;
    }
    install_prototype(rt, realm, proto)
}

fn install_prototype(
    rt: &mut Runtime,
    realm: u32,
    proto: Gc<crate::object::Object>,
) -> VmResult<()> {
    super::method(rt, realm, proto, "toString", 0, to_string)?;
    super::method(rt, realm, proto, "valueOf", 0, value_of)?;
    let getter = rt.new_builtin(realm, "get description", 0, description, false)?;
    let key = rt.heap.key_from_str("description")?;
    let desc = PropertyDescriptor::accessor(getter.into(), Value::Undefined, false, true);
    rt.define_own_property(proto, key, desc)?;
    super::to_string_tag(rt, proto, "Symbol")?;
    super::symbol_method(
        rt,
        realm,
        proto,
        WellKnown::ToPrimitive,
        1,
        to_primitive,
        (false, true),
    )?;
    Ok(())
}

/// `Symbol([description])` (§20.4.1.1): a new symbol; `new Symbol` throws.
fn symbol(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if !call.new_target().is_undefined() {
        return Err(VmError::type_error("Symbol is not a constructor"));
    }
    let description = match rt.arg(call, 0) {
        Value::Undefined => None,
        value => Some(rt.to_string(value)?),
    };
    let symbol = rt.heap.alloc_symbol(description)?;
    Ok(NativeReturn::Value(symbol.into()))
}

/// `Symbol.for(key)` (§20.4.2.2).
fn symbol_for(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let key = rt.to_string(rt.arg(call, 0))?;
    // Hashing the key is work in proportion to its length.
    let units = rt.heap.string(key)?.len();
    rt.charge_units(units)?;
    let symbol = rt.symbol_for(key)?;
    Ok(NativeReturn::Value(symbol.into()))
}

/// `Symbol.keyFor(sym)` (§20.4.2.6).
fn key_for(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Symbol(symbol) = rt.arg(call, 0) else {
        let text = rt.describe_value(rt.arg(call, 0));
        return Err(VmError::type_error(format!("{text} is not a symbol")));
    };
    let key = rt.symbol_key_for(symbol);
    Ok(NativeReturn::Value(
        key.map_or(Value::Undefined, Value::from),
    ))
}

/// `thisSymbolValue` (§20.4.3.4.1); `method` is V8's name of the method
/// in the `TypeError`.
fn this_symbol(rt: &Runtime, call: &NativeCall, method: &str) -> VmResult<Gc<Symbol>> {
    let wrong = || VmError::type_error(format!("{method} requires that 'this' be a Symbol"));
    match call.this() {
        Value::Symbol(symbol) => Ok(symbol),
        Value::Object(object) => match rt.heap.object(object)?.kind {
            ObjectKind::SymbolWrapper(symbol) => Ok(symbol),
            _ => Err(wrong()),
        },
        _ => Err(wrong()),
    }
}

/// `SymbolDescriptiveString` (§20.4.3.3.1): `"Symbol(" + description +
/// ")"`. The symbol must be rooted by the caller.
pub(super) fn descriptive_string(rt: &mut Runtime, symbol: Gc<Symbol>) -> VmResult<Gc<JsString>> {
    let description = rt.heap.symbol(symbol)?.description;
    let units = match description {
        Some(description) => rt.heap.string(description)?.len(),
        None => 0,
    };
    if units >= 1 << 16 {
        rt.heap.reserve(units * 4, &rt.vm)?;
    }
    let mut text = String16::from("Symbol(");
    if let Some(description) = description {
        text.push_str16(rt.heap.string(description)?.as_str16());
    }
    text.push_str16(String16::from(")").as_str16());
    rt.charge_units(units)?;
    Ok(rt.heap.alloc_string(text)?)
}

/// `Symbol.prototype.toString()` (§20.4.3.3).
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let symbol = this_symbol(rt, call, "Symbol.prototype.toString")?;
    Ok(NativeReturn::Value(descriptive_string(rt, symbol)?.into()))
}

/// `Symbol.prototype.valueOf()` (§20.4.3.4).
fn value_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let symbol = this_symbol(rt, call, "Symbol.prototype.valueOf")?;
    Ok(NativeReturn::Value(symbol.into()))
}

/// `get Symbol.prototype.description` (§20.4.3.2).
fn description(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let symbol = this_symbol(rt, call, "Symbol.prototype.description")?;
    let description = rt.heap.symbol(symbol)?.description;
    Ok(NativeReturn::Value(
        description.map_or(Value::Undefined, Value::from),
    ))
}

/// `Symbol.prototype[@@toPrimitive](hint)` (§20.4.3.5).
fn to_primitive(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let symbol = this_symbol(rt, call, "Symbol.prototype [ @@toPrimitive ]")?;
    Ok(NativeReturn::Value(symbol.into()))
}
