//! `Intl` (ECMA-402, <https://tc39.es/ecma402/>): the namespace object, the
//! locale lists and the option readers that the three constructors share.
//! The constructors are in `intl_datetime.rs`, `intl_number.rs` and
//! `intl_relative.rs`; the formats are in `crate::intl` (ADR 0027 section
//! 10).
//!
//! The messages of the errors are V8's (measured in Chromium 148 with
//! `tools/probes/js-intl.json`): "Value bogus out of range for
//! Intl.NumberFormat options property style", "maximumFractionDigits
//! value is out of range.", "Invalid language tag: `en_US`".

use crate::heap::Gc;
use crate::intl::locale;
use crate::intl::{IntlData, IntlObject};
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult};

/// Installs the global `Intl`.
pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let intl = rt.heap.new_object(Some(proto))?;
    rt.heap.record(intl);
    super::method(
        rt,
        realm,
        intl,
        "getCanonicalLocales",
        1,
        get_canonical_locales,
    )?;
    super::to_string_tag(rt, intl, "Intl")?;
    super::intl_datetime::install(rt, realm, intl)?;
    super::intl_number::install(rt, realm, intl)?;
    super::intl_relative::install(rt, realm, intl)?;
    super::define_global(rt, realm, "Intl", intl.into())
}

/// Defines a constructor `name` on `target` with a new prototype object;
/// returns the constructor and the prototype.
pub(super) fn constructor_on(
    rt: &mut Runtime,
    realm: u32,
    target: Gc<Object>,
    name: &str,
    func: NativeFn,
) -> VmResult<(Gc<Object>, Gc<Object>)> {
    let object_proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let proto = rt.heap.new_object(Some(object_proto))?;
    rt.heap.record(proto);
    let function = rt.new_builtin(realm, name, 0, func, true)?;
    let key = PropertyKey::String(rt.vm.atoms.prototype);
    rt.define(function, key, proto.into(), false, false, false)?;
    let key = PropertyKey::String(rt.vm.atoms.constructor);
    rt.define(proto, key, function.into(), true, false, true)?;
    let key = rt.heap.key_from_str(name)?;
    rt.define(target, key, function.into(), true, false, true)?;
    Ok((function, proto))
}

/// `Intl.getCanonicalLocales(locales)` (§8.3.1).
fn get_canonical_locales(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let tags = canonicalize_locale_list(rt, rt.arg(call, 0))?;
    let array = rt.new_array()?;
    for (index, tag) in tags.iter().enumerate() {
        let value = string_value(rt, tag)?;
        rt.create_data_property_or_throw(array, PropertyKey::Index(index as u32), value)?;
    }
    Ok(NativeReturn::Value(array.into()))
}

/// A new string value, recorded in the native's handle scope.
pub(super) fn string_value(rt: &mut Runtime, text: &str) -> VmResult<Value> {
    let string = rt.heap.alloc_str(text)?;
    rt.heap.record(string);
    Ok(string.into())
}

/// The text of a value converted with `ToString`.
pub(super) fn to_text(rt: &mut Runtime, value: Value) -> VmResult<String> {
    let string = rt.to_string(value)?;
    Ok(rt.flat_text(string)?.to_string_lossy())
}

/// `CanonicalizeLocaleList` (§9.2.1): the canonical tags, each once.
pub(super) fn canonicalize_locale_list(rt: &mut Runtime, locales: Value) -> VmResult<Vec<String>> {
    let mut seen: Vec<String> = Vec::new();
    let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut add = |tag: &str| -> VmResult<()> {
        let canonical = locale::canonicalize(tag).map_err(|invalid| {
            let shown = if invalid.lowercase {
                tag.to_ascii_lowercase()
            } else {
                tag.to_owned()
            };
            VmError::range_error(format!("Invalid language tag: {shown}"))
        })?;
        if known.insert(canonical.clone()) {
            seen.push(canonical);
        }
        Ok(())
    };
    match locales {
        Value::Undefined => {}
        Value::String(_) => {
            let text = to_text(rt, locales)?;
            add(&text)?;
        }
        _ => {
            let list = rt.to_object(locales)?;
            rt.heap.record(list);
            let length = rt.length_of_array_like(list)?;
            let mut index = 0.0;
            while index < length {
                rt.tick()?;
                let key = rt.index_key(index)?;
                index += 1.0;
                if !rt.has_property(list, key)? {
                    continue;
                }
                let item = rt.get_value(list.into(), key)?;
                if !matches!(item, Value::String(_) | Value::Object(_)) {
                    return Err(VmError::type_error(
                        "Language ID should be string or object.",
                    ));
                }
                let text = to_text(rt, item)?;
                add(&text)?;
            }
        }
    }
    Ok(seen)
}

/// Whether a text is a Unicode locale identifier type (`(3*8alphanum)
/// *("-" (3*8alphanum))`, the value of `calendar` and `numberingSystem`).
pub(super) fn is_type_sequence(text: &str) -> bool {
    text.split('-').all(|part| {
        (3..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

/// The options of a call: the object (none if the argument is
/// `undefined`) and the name that V8's messages use for the call.
pub(super) struct Options {
    object: Option<Gc<Object>>,
    site: &'static str,
}

impl Options {
    /// `CoerceOptionsToObject` (§9.2.12): `undefined` gives no options, an
    /// object itself, anything else `ToObject` (`null` is a `TypeError`
    /// with `null_message`).
    pub(super) fn new(
        rt: &mut Runtime,
        value: Value,
        site: &'static str,
        null_message: &str,
    ) -> VmResult<Options> {
        let object = match value {
            Value::Undefined => None,
            Value::Null => return Err(VmError::type_error(null_message.to_owned())),
            other => {
                let object = rt.to_object(other)?;
                Some(rt.heap.record(object))
            }
        };
        Ok(Options { object, site })
    }

    /// The name of the call in the messages.
    pub(super) fn site(&self) -> &'static str {
        self.site
    }

    /// `Get(options, name)`; `undefined` without options.
    pub(super) fn get(&self, rt: &mut Runtime, name: &str) -> VmResult<Value> {
        let Some(object) = self.object else {
            return Ok(Value::Undefined);
        };
        let key = rt.heap.key_from_str(name)?;
        rt.heap.record(key);
        rt.get_value(object.into(), key)
    }

    /// `GetOption(options, name, string, values, undefined)` (§9.2.13): the
    /// text of the property, a `RangeError` if it is not one of `values`
    /// (any text if `values` is empty).
    pub(super) fn string(
        &self,
        rt: &mut Runtime,
        name: &str,
        values: &[&str],
    ) -> VmResult<Option<String>> {
        let value = self.get(rt, name)?;
        if value.is_undefined() {
            return Ok(None);
        }
        let text = to_text(rt, value)?;
        if !values.is_empty() && !values.contains(&text.as_str()) {
            return Err(VmError::range_error(format!(
                "Value {text} out of range for {} options property {name}",
                self.site
            )));
        }
        Ok(Some(text))
    }

    /// The option `numberingSystem` (§11.1.2, §15.1.2, §17.1.1): a
    /// `RangeError` unless it is a Unicode locale identifier type. The
    /// numbering system itself has no effect (ADR 0027 section 10).
    pub(super) fn numbering_system(&self, rt: &mut Runtime) -> VmResult<()> {
        if let Some(system) = self.string(rt, "numberingSystem", &[])?
            && !is_type_sequence(&system)
        {
            return Err(VmError::range_error(format!(
                "Invalid numberingSystem : {system}"
            )));
        }
        Ok(())
    }

    /// `GetOption(options, name, boolean, empty, undefined)`.
    pub(super) fn boolean(&self, rt: &mut Runtime, name: &str) -> VmResult<Option<bool>> {
        let value = self.get(rt, name)?;
        if value.is_undefined() {
            return Ok(None);
        }
        Ok(Some(rt.to_boolean(value)?))
    }

    /// `GetNumberOption` (§9.2.14) without a default: `ToNumber`, then a
    /// `RangeError` outside `min..=max`, rounded down.
    pub(super) fn number(
        &self,
        rt: &mut Runtime,
        name: &str,
        min: f64,
        max: f64,
    ) -> VmResult<Option<u32>> {
        let value = self.get(rt, name)?;
        if value.is_undefined() {
            return Ok(None);
        }
        let number = rt.to_number(value)?;
        if number.is_nan() || number < min || number > max {
            return Err(VmError::range_error(format!(
                "{name} value is out of range."
            )));
        }
        Ok(Some(number.floor() as u32))
    }
}

/// A new `Intl` object with the prototype of `new.target` (§9.1.14
/// `OrdinaryCreateFromConstructor`), recorded in the handle scope.
pub(super) fn new_object(
    rt: &mut Runtime,
    call: &NativeCall,
    data: IntlData,
) -> VmResult<Gc<Object>> {
    let key = PropertyKey::String(rt.vm.atoms.prototype);
    let Value::Object(fallback) = rt.get_value(call.callee().into(), key)? else {
        return Err(VmError::invariant(
            "a constructor of Intl without a prototype",
        ));
    };
    let proto = rt.prototype_from_with(call, |_, _| Ok(fallback))?;
    let kind = ObjectKind::Intl(Box::new(IntlObject {
        data,
        bound_format: None,
    }));
    let object = rt.heap.new_object_with_kind(Some(proto), kind)?;
    Ok(rt.heap.record(object))
}

/// The format of `this` if it is an `Intl` object of the right kind,
/// cloned so that no borrow of the heap outlives the call.
pub(super) fn this_format(
    rt: &Runtime,
    value: Value,
    accept: fn(&IntlData) -> bool,
) -> Option<(Gc<Object>, IntlData)> {
    let Value::Object(object) = value else {
        return None;
    };
    match &rt.heap.object(object).ok()?.kind {
        ObjectKind::Intl(intl) if accept(&intl.data) => Some((object, intl.data.clone())),
        _ => None,
    }
}

/// A new ordinary object in the realm, recorded in the handle scope.
pub(super) fn plain_object(rt: &mut Runtime, realm: u32) -> VmResult<Gc<Object>> {
    let proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let object = rt.heap.new_object(Some(proto))?;
    Ok(rt.heap.record(object))
}

/// `CreateDataPropertyOrThrow(object, name, value)`.
pub(super) fn set(rt: &mut Runtime, object: Gc<Object>, name: &str, value: Value) -> VmResult<()> {
    let key = rt.heap.key_from_str(name)?;
    rt.heap.record(key);
    rt.create_data_property_or_throw(object, key, value)
}

/// `CreateDataPropertyOrThrow(object, name, text)`.
pub(super) fn set_text(
    rt: &mut Runtime,
    object: Gc<Object>,
    name: &str,
    text: &str,
) -> VmResult<()> {
    let value = string_value(rt, text)?;
    set(rt, object, name, value)
}

/// The array of the `{type, value}` objects of a `formatToParts` result;
/// `extra` adds a property to each (the `unit` of a relative time).
pub(super) fn parts_array(
    rt: &mut Runtime,
    realm: u32,
    parts: &[(&str, String, Option<&str>)],
) -> VmResult<Value> {
    let array = rt.new_array()?;
    for (index, (kind, text, extra)) in parts.iter().enumerate() {
        let part = plain_object(rt, realm)?;
        set_text(rt, part, "type", kind)?;
        set_text(rt, part, "value", text)?;
        if let Some(unit) = extra {
            set_text(rt, part, "unit", unit)?;
        }
        rt.create_data_property_or_throw(array, PropertyKey::Index(index as u32), part.into())?;
    }
    Ok(array.into())
}

/// `supportedLocalesOf(locales)` of the three constructors (§9.2.9 with
/// `CanonicalizeLocaleList`); the options are read for their errors only
/// in the specification and are not used here.
pub(super) fn supported_locales_of(
    rt: &mut Runtime,
    call: &NativeCall,
    site: &'static str,
) -> VmResult<NativeReturn> {
    let requested = canonicalize_locale_list(rt, rt.arg(call, 0))?;
    let message = format!("{site} called on null or undefined");
    let options = Options::new(rt, rt.arg(call, 1), site, &message)?;
    options.string(rt, "localeMatcher", &["lookup", "best fit"])?;
    let array = rt.new_array()?;
    for (index, tag) in locale::supported(&requested).iter().enumerate() {
        let value = string_value(rt, tag)?;
        rt.create_data_property_or_throw(array, PropertyKey::Index(index as u32), value)?;
    }
    Ok(NativeReturn::Value(array.into()))
}

/// The locale of a constructor call: the canonical list, negotiated.
pub(super) fn resolve_locale(rt: &mut Runtime, locales: Value) -> VmResult<locale::Resolved> {
    let requested = canonicalize_locale_list(rt, locales)?;
    Ok(locale::resolve(&requested))
}

/// The bound `format` function of an `Intl` object, if it was made.
pub(super) fn bound_format(rt: &Runtime, object: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
    match &rt.heap.object(object)?.kind {
        ObjectKind::Intl(intl) => Ok(intl.bound_format),
        _ => Ok(None),
    }
}

/// Stores the bound `format` function (`[[BoundFormat]]`).
pub(super) fn set_bound_format(
    rt: &mut Runtime,
    object: Gc<Object>,
    function: Gc<Object>,
) -> VmResult<()> {
    if let ObjectKind::Intl(intl) = &mut rt.heap.object_mut(object)?.kind {
        intl.bound_format = Some(function);
    }
    Ok(())
}
