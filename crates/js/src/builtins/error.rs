//! The error constructors (ECMA-262 §20.5): `Error` and the native errors
//! `TypeError`, `RangeError`, `ReferenceError`, `SyntaxError`,
//! `EvalError` and `URIError`, with `message`, the `cause` option, `name`
//! and `message` on the prototypes and `Error.prototype.toString`. The
//! `stack` property is M7.

use crate::error::ThrowKind;
use crate::object::ObjectKind;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{NativeCall, NativeFn, NativeReturn, VmError, VmResult, error_prototype};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let kinds: [(ThrowKind, NativeFn); 7] = [
        (ThrowKind::Error, error),
        (ThrowKind::TypeError, type_error),
        (ThrowKind::RangeError, range_error),
        (ThrowKind::ReferenceError, reference_error),
        (ThrowKind::SyntaxError, syntax_error),
        (ThrowKind::EvalError, eval_error),
        (ThrowKind::UriError, uri_error),
    ];
    let name_key = PropertyKey::String(rt.vm.atoms.name);
    let message_key = PropertyKey::String(rt.vm.atoms.message);
    let mut base = None;
    for (kind, func) in kinds {
        let proto = rt.intrinsic(realm, error_prototype(kind))?;
        let constructor = super::constructor(rt, realm, kind.name(), 1, func, proto)?;
        // The native error constructors inherit from `Error` (§20.5.6.2).
        if let Some(base) = base {
            rt.heap.set_prototype_of(constructor, Some(base))?;
        } else {
            base = Some(constructor);
        }
        let name = rt.heap.intern_str(kind.name())?;
        rt.define(proto, name_key, name.into(), true, false, true)?;
        let empty = rt.vm.atoms.empty;
        rt.define(proto, message_key, empty.into(), true, false, true)?;
    }
    let proto = rt.intrinsic(realm, error_prototype(ThrowKind::Error))?;
    super::method(rt, realm, proto, "toString", 0, to_string)?;
    Ok(())
}

fn error(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct(rt, call, ThrowKind::Error)
}

fn type_error(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct(rt, call, ThrowKind::TypeError)
}

fn range_error(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct(rt, call, ThrowKind::RangeError)
}

fn reference_error(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct(rt, call, ThrowKind::ReferenceError)
}

fn syntax_error(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct(rt, call, ThrowKind::SyntaxError)
}

fn eval_error(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct(rt, call, ThrowKind::EvalError)
}

fn uri_error(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    construct(rt, call, ThrowKind::UriError)
}

/// `Error(message, options)` (§20.5.1.1) and the native errors
/// (§20.5.6.1.1), with and without `new`.
fn construct(rt: &mut Runtime, call: &NativeCall, kind: ThrowKind) -> VmResult<NativeReturn> {
    let proto = rt.prototype_from(call, error_prototype(kind))?;
    let object = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::Error)?;
    rt.heap.record(object);
    let message = rt.arg(call, 0);
    if !message.is_undefined() {
        let text = rt.to_string(message)?;
        let key = PropertyKey::String(rt.vm.atoms.message);
        rt.define(object, key, text.into(), true, false, true)?;
    }
    // InstallErrorCause (§20.5.8.1).
    if let Value::Object(options) = rt.arg(call, 1) {
        let key = rt.heap.key_from_str("cause")?;
        if rt.heap.has_property(options, key)? {
            let cause = rt.get_value(options.into(), key)?;
            rt.define(object, key, cause, true, false, true)?;
        }
    }
    Ok(NativeReturn::Value(object.into()))
}

/// The atom "Error", recorded in the current scope.
fn error_name(rt: &mut Runtime) -> VmResult<crate::heap::Gc<crate::string::JsString>> {
    let name = rt.heap.intern_str("Error")?;
    Ok(rt.heap.record(name))
}

/// `Error.prototype.toString()` (§20.5.3.4).
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = call.this();
    if !matches!(this, Value::Object(_)) {
        return Err(VmError::type_error(format!(
            "Method Error.prototype.toString called on incompatible receiver {}",
            rt.primitive_text(this)
        )));
    }
    let part = |rt: &mut Runtime, key: PropertyKey, default| -> VmResult<_> {
        match rt.get_value(this, key)? {
            Value::Undefined => Ok(default),
            value => rt.to_string(value),
        }
    };
    let default_name = error_name(rt)?;
    let name = part(rt, PropertyKey::String(rt.vm.atoms.name), default_name)?;
    let message = part(
        rt,
        PropertyKey::String(rt.vm.atoms.message),
        rt.vm.atoms.empty,
    )?;
    let name_text = rt.heap.string(name)?.as_str16();
    let message_text = rt.heap.string(message)?.as_str16();
    let string = if name_text.is_empty() {
        message
    } else if message_text.is_empty() {
        name
    } else {
        let mut text = name_text.to_string16();
        text.push_str16(swb_js_text::String16::from(": ").as_str16());
        text.push_str16(message_text);
        let units = text.len();
        let string = rt.heap.alloc_string(text)?;
        rt.charge_units(units)?;
        string
    };
    Ok(NativeReturn::Value(string.into()))
}
