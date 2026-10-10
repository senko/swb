//! `Object` (ECMA-262 §20.1.1 and §20.1.2): the constructor and the
//! static functions. The descriptor functions are in
//! [`super::descriptor`], the integrity levels in [`super::integrity`],
//! `fromEntries` and `groupBy` in [`super::object_iter`], `Object.prototype`
//! in [`super::object_proto`].
//!
//! Functions that loop over the keys of an object charge the time
//! countdown in proportion to the number of keys, and open a handle scope
//! per key when they call getters.

use swb_js_text::String16;

use crate::heap::Gc;
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::internal::ProtoSet;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let object = super::constructor(rt, realm, "Object", 1, object_constructor, proto)?;
    let statics: [(&str, u32, NativeFn); 23] = [
        ("assign", 2, assign),
        ("create", 2, super::descriptor::create),
        ("defineProperties", 2, super::descriptor::define_properties),
        ("defineProperty", 3, super::descriptor::define_property),
        ("entries", 1, entries),
        ("freeze", 1, super::integrity::freeze),
        ("fromEntries", 1, super::object_iter::from_entries),
        (
            "getOwnPropertyDescriptor",
            2,
            super::descriptor::get_own_property_descriptor,
        ),
        (
            "getOwnPropertyDescriptors",
            1,
            super::descriptor::get_own_property_descriptors,
        ),
        ("getOwnPropertyNames", 1, get_own_property_names),
        ("getOwnPropertySymbols", 1, get_own_property_symbols),
        ("getPrototypeOf", 1, get_prototype_of),
        ("groupBy", 2, super::object_iter::group_by),
        ("hasOwn", 2, has_own),
        ("is", 2, is),
        ("isExtensible", 1, super::integrity::is_extensible),
        ("isFrozen", 1, super::integrity::is_frozen),
        ("isSealed", 1, super::integrity::is_sealed),
        ("keys", 1, keys),
        ("preventExtensions", 1, super::integrity::prevent_extensions),
        ("seal", 1, super::integrity::seal),
        ("setPrototypeOf", 2, set_prototype_of),
        ("values", 1, values),
    ];
    for (name, length, func) in statics {
        super::method(rt, realm, object, name, length, func)?;
    }
    super::object_proto::install(rt, realm, proto)
}

/// `Object(value)` (§20.1.1.1): for a `new.target` other than `Object`
/// itself, an ordinary object with the prototype of `new.target`;
/// otherwise a new object for `undefined` and `null`, else
/// `ToObject(value)`.
fn object_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let new_target = call.new_target();
    if !new_target.is_undefined() && new_target != Value::Object(call.callee()) {
        let proto = rt.prototype_from(call, Intrinsic::ObjectPrototype)?;
        let object = rt.heap.new_object(Some(proto))?;
        return Ok(NativeReturn::Value(object.into()));
    }
    let value = rt.arg(call, 0);
    if value.is_nullish() {
        let proto = rt.intrinsic(call.realm(), Intrinsic::ObjectPrototype)?;
        let object = rt.heap.new_object(Some(proto))?;
        return Ok(NativeReturn::Value(object.into()));
    }
    Ok(NativeReturn::Value(rt.to_object(value)?.into()))
}

/// What `EnumerableOwnProperties` (§7.3.23) collects.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keys,
    Values,
    Entries,
}

/// Whether the own property `key` exists and is enumerable, without
/// allocating the character of a String object.
fn is_enumerable_own(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
    if rt.heap.string_object_unit(object, key)?.is_some() {
        return Ok(true);
    }
    Ok(rt
        .heap
        .get_own_property(object, key)?
        .is_some_and(|property| property.enumerable()))
}

/// `EnumerableOwnProperties(O, kind)` (§7.3.23) as an array.
fn enumerable_own_properties(
    rt: &mut Runtime,
    call: &NativeCall,
    kind: Kind,
) -> VmResult<NativeReturn> {
    let object = rt.this_object_arg(call, 0)?;
    let keys = rt.own_keys(object)?;
    rt.heap.record_keys(&keys);
    let result = rt.new_array()?;
    let mut count = 0u32;
    for key in keys {
        rt.tick()?;
        if key.is_symbol() {
            continue;
        }
        let added = rt.with_scope(|rt| {
            if !is_enumerable_own(rt, object, key)? {
                return Ok(false);
            }
            let item = match kind {
                Kind::Keys => rt.heap.key_to_value(key)?,
                Kind::Values => rt.get_value(object.into(), key)?,
                Kind::Entries => {
                    let value = rt.get_value(object.into(), key)?;
                    let name = rt.heap.key_to_value(key)?;
                    rt.pair_array(name, value)?.into()
                }
            };
            rt.heap
                .create_data_property(result, PropertyKey::Index(count), item, &rt.vm)?;
            Ok(true)
        })?;
        count += u32::from(added);
    }
    Ok(NativeReturn::Value(result.into()))
}

/// `Object.keys(O)` (§20.1.2.18).
fn keys(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    enumerable_own_properties(rt, call, Kind::Keys)
}

/// `Object.values(O)` (§20.1.2.23).
fn values(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    enumerable_own_properties(rt, call, Kind::Values)
}

/// `Object.entries(O)` (§20.1.2.5).
fn entries(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    enumerable_own_properties(rt, call, Kind::Entries)
}

/// `Object.assign(target, ...sources)` (§20.1.2.1).
fn assign(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let to = rt.this_object_arg(call, 0)?;
    for index in 1..call.argc() {
        let source = rt.arg(call, index);
        if source.is_nullish() {
            continue;
        }
        let from = rt.to_object(source)?;
        rt.heap.record(from);
        let keys = rt.own_keys(from)?;
        rt.heap.record_keys(&keys);
        for key in keys {
            rt.tick()?;
            rt.with_scope(|rt| {
                let Some(property) = rt.own_property(from, key)? else {
                    return Ok(());
                };
                if !property.enumerable() {
                    return Ok(());
                }
                let value = rt.get_value(from.into(), key)?;
                rt.set_or_throw(to, key, value)
            })?;
        }
    }
    Ok(NativeReturn::Value(to.into()))
}

/// `Object.getOwnPropertyNames(O)` (§20.1.2.10).
fn get_own_property_names(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    own_keys_of_type(rt, call, false)
}

/// `Object.getOwnPropertySymbols(O)` (§20.1.2.11).
fn get_own_property_symbols(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    own_keys_of_type(rt, call, true)
}

/// `GetOwnPropertyKeys(O, type)` (§20.1.2.11.1).
fn own_keys_of_type(rt: &mut Runtime, call: &NativeCall, symbols: bool) -> VmResult<NativeReturn> {
    let object = rt.this_object_arg(call, 0)?;
    let keys = rt.own_keys(object)?;
    rt.heap.record_keys(&keys);
    let result = rt.new_array()?;
    let mut count = 0u32;
    for key in keys {
        rt.tick()?;
        if key.is_symbol() != symbols {
            continue;
        }
        let item = rt.heap.key_to_value(key)?;
        rt.heap
            .create_data_property(result, PropertyKey::Index(count), item, &rt.vm)?;
        count += 1;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// `Object.getPrototypeOf(O)` (§20.1.2.12).
fn get_prototype_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object_arg(call, 0)?;
    let proto = rt.heap.get_prototype_of(object)?;
    Ok(NativeReturn::Value(proto.map_or(Value::Null, Value::from)))
}

/// `Object.setPrototypeOf(O, proto)` (§20.1.2.22).
fn set_prototype_of(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let target = rt.arg(call, 0);
    if target.is_nullish() {
        return Err(VmError::type_error(
            "Object.setPrototypeOf called on null or undefined",
        ));
    }
    let proto = match rt.arg(call, 1) {
        Value::Object(proto) => Some(proto),
        Value::Null => None,
        other => {
            return Err(VmError::type_error(format!(
                "Object prototype may only be an Object or null: {}",
                rt.primitive_text(other)
            )));
        }
    };
    let Value::Object(object) = target else {
        return Ok(NativeReturn::Value(target));
    };
    match rt.set_prototype_of(object, proto)? {
        ProtoSet::Done => Ok(NativeReturn::Value(target)),
        why => Err(rt.set_prototype_failure(object, why)),
    }
}

/// `Object.hasOwn(O, P)` (§20.1.2.13).
fn has_own(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.this_object_arg(call, 0)?;
    let key = rt.key_arg(rt.arg(call, 1))?;
    let own = rt.heap.string_object_unit(object, key)?.is_some()
        || rt.heap.get_own_property(object, key)?.is_some();
    Ok(NativeReturn::Value(Value::Bool(own)))
}

/// `Object.is(value1, value2)` (§20.1.2.14): `SameValue`.
fn is(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let same = rt.heap.same_value(rt.arg(call, 0), rt.arg(call, 1))?;
    Ok(NativeReturn::Value(Value::Bool(same)))
}

/// `Object.prototype.toString` (§20.1.3.6) and the text of other
/// functions that need it: `[object Tag]`, where the tag is the string
/// value of `@@toStringTag` if there is one, else the built-in tag of the
/// kind of object.
pub(super) fn builtin_tag_text(rt: &mut Runtime, value: Value) -> VmResult<Value> {
    let builtin = match value {
        Value::Undefined => return tag_string(rt, "Undefined"),
        Value::Null => return tag_string(rt, "Null"),
        _ => {
            let object = rt.to_object(value)?;
            rt.heap.record(object);
            let builtin = builtin_tag(&rt.heap.object(object)?.kind);
            let key = rt.symbol_key(WellKnown::ToStringTag);
            if let Value::String(tag) = rt.get_value(object.into(), key)? {
                return tag_text(rt, tag);
            }
            builtin
        }
    };
    tag_string(rt, builtin)
}

/// The built-in tag of an object kind (steps 4 to 14 of §20.1.3.6).
fn builtin_tag(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Array { .. } => "Array",
        ObjectKind::Arguments => "Arguments",
        ObjectKind::Function(_) | ObjectKind::Native(_) | ObjectKind::Bound(_) => "Function",
        ObjectKind::Error => "Error",
        ObjectKind::BooleanWrapper(_) => "Boolean",
        ObjectKind::NumberWrapper(_) => "Number",
        ObjectKind::StringWrapper(_) => "String",
        ObjectKind::Generator(_)
        | ObjectKind::ArrayIterator(_)
        | ObjectKind::Ordinary
        | ObjectKind::Host { .. }
        | ObjectKind::SymbolWrapper(_) => "Object",
    }
}

fn tag_string(rt: &mut Runtime, tag: &str) -> VmResult<Value> {
    Ok(rt.heap.alloc_str(&format!("[object {tag}]"))?.into())
}

/// `"[object " + tag + "]"` for a tag that is a script string (it can
/// be long, so the copy is charged and reserved).
fn tag_text(rt: &mut Runtime, tag: Gc<crate::string::JsString>) -> VmResult<Value> {
    let tag_units = rt.heap.string(tag)?.len();
    if tag_units >= 1 << 16 {
        rt.heap.reserve(tag_units * 4, &rt.vm)?;
    }
    let mut text = String16::from("[object ");
    text.push_str16(rt.heap.string(tag)?.as_str16());
    text.push_str16(String16::from("]").as_str16());
    rt.charge_units(tag_units)?;
    Ok(rt.heap.alloc_string(text)?.into())
}

#[cfg(test)]
mod tests {
    use crate::runtime::{Runtime, RuntimeConfig};
    use crate::string::PropertyKey;
    use crate::value::Value;
    use crate::vm::VmResult;

    /// Defines the global `o` with the property `a` and a property
    /// whose key is a symbol (scripts cannot create symbols until
    /// `Symbol` exists), and the global `sym` with that symbol.
    fn object_with_symbol_key(rt: &mut Runtime) -> VmResult<()> {
        rt.scoped(|rt| {
            let proto = rt.intrinsic(0, crate::vm::Intrinsic::ObjectPrototype)?;
            let object = rt.heap.new_object(Some(proto))?;
            rt.heap.record(object);
            let a = rt.heap.key_from_str("a")?;
            rt.heap.record(a);
            rt.define(object, a, Value::Int(1), true, true, true)?;
            let symbol = rt.heap.alloc_symbol(None)?;
            rt.heap.record(symbol);
            rt.define(
                object,
                PropertyKey::Symbol(symbol),
                Value::Int(2),
                true,
                true,
                true,
            )?;
            let global = rt.global_object(0)?;
            for (name, value) in [("o", Value::Object(object)), ("sym", Value::Symbol(symbol))] {
                let key = rt.heap.key_from_str(name)?;
                rt.define(global, key, value, true, false, true)?;
            }
            Ok(())
        })
    }

    fn eval_text(rt: &mut Runtime, source: &str) -> String {
        let value = rt.eval(source).expect("the script runs");
        rt.to_rust_string(value).expect("a text")
    }

    #[test]
    fn symbol_keys_are_listed_copied_and_not_enumerated_as_strings() {
        for stress in [false, true] {
            let mut config = RuntimeConfig::default();
            config.heap.stress = stress;
            let mut rt = Runtime::new(config).expect("a runtime");
            object_with_symbol_key(&mut rt).expect("the object");
            let cases = [
                ("Object.getOwnPropertySymbols(o).length", "1"),
                ("Object.getOwnPropertySymbols(o)[0] === sym", "true"),
                ("Object.getOwnPropertyNames(o).join()", "a"),
                ("Object.keys(o).join()", "a"),
                (
                    "Reflect.ownKeys(o).length + ',' + (Reflect.ownKeys(o)[1] === sym)",
                    "2,true",
                ),
                (
                    "var c = Object.assign({}, o); c[sym] + ',' + Object.getOwnPropertySymbols(c).length",
                    "2,1",
                ),
                ("Object.getOwnPropertyDescriptor(o, sym).value", "2"),
                (
                    "Object.getOwnPropertyDescriptors(o)[sym].enumerable",
                    "true",
                ),
                (
                    "Object.hasOwn(o, sym) + ',' + o.propertyIsEnumerable(sym) + ',' + Reflect.has(o, sym)",
                    "true,true,true",
                ),
                (
                    "Object.freeze(o); Object.isFrozen(o) + ',' + Object.getOwnPropertyDescriptor(o, sym).writable",
                    "true,false",
                ),
                (
                    "Reflect.deleteProperty(o, sym) + ',' + Object.getOwnPropertySymbols(o).length",
                    "false,1",
                ),
            ];
            for (source, expected) in cases {
                assert_eq!(
                    eval_text(&mut rt, source),
                    expected,
                    "{source} (stress {stress})"
                );
            }
        }
    }
}
