//! The error constructors (ECMA-262 §20.5): `Error` and the native errors
//! `TypeError`, `RangeError`, `ReferenceError`, `SyntaxError`,
//! `EvalError` and `URIError`, `AggregateError`, with `message`, the
//! `cause` option, `name` and `message` on the prototypes and
//! `Error.prototype.toString`.
//!
//! The V8 extensions that Chromium has and that scripts test for:
//! `Error.stackTraceLimit` and `Error.captureStackTrace`. The `stack`
//! property of error objects is in `vm/stack.rs`; this module installs
//! its accessor functions as intrinsics and gives every error object
//! its stack when it is created.

use swb_js_text::String16;

use crate::error::ThrowKind;
use crate::heap::Gc;
use crate::object::ObjectKind;
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{
    Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, capture_stack_trace,
    error_prototype, stack_getter, stack_setter,
};

/// The default of `Error.stackTraceLimit`.
const DEFAULT_STACK_TRACE_LIMIT: i32 = 10;

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    // The accessor functions of `stack` exist before the first error.
    let getter = rt.new_builtin(realm, "", 0, stack_getter, false)?;
    rt.set_intrinsic(realm, Intrinsic::StackGetter, getter);
    let setter = rt.new_builtin(realm, "", 1, stack_setter, false)?;
    rt.set_intrinsic(realm, Intrinsic::StackSetter, setter);
    let kinds: [(ThrowKind, NativeFn); 7] = [
        (ThrowKind::Error, error),
        (ThrowKind::TypeError, type_error),
        (ThrowKind::RangeError, range_error),
        (ThrowKind::ReferenceError, reference_error),
        (ThrowKind::SyntaxError, syntax_error),
        (ThrowKind::EvalError, eval_error),
        (ThrowKind::UriError, uri_error),
    ];
    let mut base = None;
    for (kind, func) in kinds {
        let proto = rt.intrinsic(realm, error_prototype(kind))?;
        let constructor = super::constructor(rt, realm, kind.name(), 1, func, proto)?;
        install_prototype(rt, proto, kind.name())?;
        // The native error constructors inherit from `Error` (§20.5.6.2).
        if let Some(base) = base {
            rt.heap.set_prototype_of(constructor, Some(base))?;
        } else {
            base = Some(constructor);
            rt.set_intrinsic(realm, Intrinsic::ErrorConstructor, constructor);
            install_statics(rt, realm, constructor)?;
        }
    }
    let proto = rt.intrinsic(realm, error_prototype(ThrowKind::Error))?;
    super::method(rt, realm, proto, "toString", 0, to_string)?;
    install_aggregate_error(rt, realm, base)
}

/// `AggregateError` (§20.5.7): a constructor of two arguments that
/// inherits from `Error`.
fn install_aggregate_error(
    rt: &mut Runtime,
    realm: u32,
    base: Option<Gc<crate::object::Object>>,
) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::AggregateErrorPrototype)?;
    let constructor = super::constructor(rt, realm, "AggregateError", 2, aggregate_error, proto)?;
    install_prototype(rt, proto, "AggregateError")?;
    if let Some(base) = base {
        rt.heap.set_prototype_of(constructor, Some(base))?;
    }
    Ok(())
}

/// `name` and `message` of an error prototype (§20.5.3.2, §20.5.3.3,
/// §20.5.6.3.1, §20.5.6.3.2).
fn install_prototype(
    rt: &mut Runtime,
    proto: Gc<crate::object::Object>,
    name: &str,
) -> VmResult<()> {
    let name_key = PropertyKey::String(rt.vm.atoms.name);
    let message_key = PropertyKey::String(rt.vm.atoms.message);
    let name = rt.heap.intern_str(name)?;
    rt.define(proto, name_key, name.into(), true, false, true)?;
    let empty = rt.vm.atoms.empty;
    rt.define(proto, message_key, empty.into(), true, false, true)
}

/// `Error.captureStackTrace` and `Error.stackTraceLimit` (V8).
fn install_statics(
    rt: &mut Runtime,
    realm: u32,
    constructor: Gc<crate::object::Object>,
) -> VmResult<()> {
    super::method(
        rt,
        realm,
        constructor,
        "captureStackTrace",
        2,
        capture_stack_trace,
    )?;
    let key = PropertyKey::String(rt.vm.atoms.stack_trace_limit);
    let limit = Value::Int(DEFAULT_STACK_TRACE_LIMIT);
    rt.define(constructor, key, limit, true, true, true)
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
    let object = new_error_object(rt, call, error_prototype(kind))?;
    install_message_and_cause(rt, object, rt.arg(call, 0), rt.arg(call, 1))?;
    Ok(NativeReturn::Value(object.into()))
}

/// `OrdinaryCreateFromConstructor` for an error object, with its stack
/// (the object is recorded in the native's handle scope).
fn new_error_object(
    rt: &mut Runtime,
    call: &NativeCall,
    fallback: Intrinsic,
) -> VmResult<Gc<crate::object::Object>> {
    let proto = rt.prototype_from(call, fallback)?;
    let object = rt
        .heap
        .new_object_with_kind(Some(proto), ObjectKind::Error(None))?;
    rt.heap.record(object);
    // Chromium hides the frames above and including the innermost call of
    // `new.target` when it is a plain function other than the constructor
    // itself; if it is not on the stack, no frame is captured. Bound
    // functions and other kinds of `new.target` hide nothing.
    let skip = match call.new_target() {
        Value::Object(target)
            if target != call.callee()
                && matches!(
                    rt.heap.object(target)?.kind,
                    ObjectKind::Function(_) | ObjectKind::Native(_)
                ) =>
        {
            Some(target)
        }
        _ => None,
    };
    rt.install_stack(object, skip)?;
    Ok(object)
}

/// Steps 3 and 4 of the error constructors: the `message` property and
/// `InstallErrorCause` (§20.5.8.1).
fn install_message_and_cause(
    rt: &mut Runtime,
    object: Gc<crate::object::Object>,
    message: Value,
    options: Value,
) -> VmResult<()> {
    if !message.is_undefined() {
        let text = rt.to_string(message)?;
        let key = PropertyKey::String(rt.vm.atoms.message);
        rt.define(object, key, text.into(), true, false, true)?;
    }
    if let Value::Object(options) = options {
        let key = rt.heap.key_from_str("cause")?;
        if rt.heap.has_property(options, key)? {
            let cause = rt.get_value(options.into(), key)?;
            rt.define(object, key, cause, true, false, true)?;
        }
    }
    Ok(())
}

/// `AggregateError(errors, message, options)` (§20.5.7.1).
fn aggregate_error(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = new_error_object(rt, call, Intrinsic::AggregateErrorPrototype)?;
    install_message_and_cause(rt, object, rt.arg(call, 1), rt.arg(call, 2))?;
    // IteratorToList: the list is the array itself, built while the
    // iterator runs, so its size is not held in a Rust vector.
    let list = rt.new_array()?;
    let mut record = rt.get_iterator(rt.arg(call, 0))?;
    let mut length = 0u32;
    loop {
        let more = rt.with_scope(|rt| {
            let Some(value) = rt.iterator_step_value(&mut record)? else {
                return Ok(false);
            };
            if length == u32::MAX {
                return Err(VmError::range_error(crate::object::INVALID_ARRAY_LENGTH));
            }
            rt.heap
                .create_data_property(list, PropertyKey::Index(length), value, &rt.vm)?;
            length += 1;
            Ok(true)
        })?;
        if !more {
            break;
        }
    }
    let key = rt.heap.key_from_str("errors")?;
    rt.define(object, key, list.into(), true, false, true)?;
    Ok(NativeReturn::Value(object.into()))
}

/// The first line of the stack and the result of
/// `Error.prototype.toString` (§20.5.3.4 steps 3 to 9) for any value
/// that is an object: `name` and `message` read with `Get`, `Error` and
/// the empty string as their defaults.
pub(crate) fn error_header(rt: &mut Runtime, this: Value) -> VmResult<Gc<JsString>> {
    let part = |rt: &mut Runtime, key: PropertyKey, default| -> VmResult<_> {
        match rt.get_value(this, key)? {
            Value::Undefined => Ok(default),
            value => {
                let text = rt.to_string(value)?;
                Ok(rt.heap.record(text))
            }
        }
    };
    let default_name = rt.heap.intern_str("Error")?;
    rt.heap.record(default_name);
    let name = part(rt, PropertyKey::String(rt.vm.atoms.name), default_name)?;
    let message = part(
        rt,
        PropertyKey::String(rt.vm.atoms.message),
        rt.vm.atoms.empty,
    )?;
    let name_text = rt.heap.string(name)?.as_str16();
    let message_text = rt.heap.string(message)?.as_str16();
    if name_text.is_empty() {
        return Ok(message);
    }
    if message_text.is_empty() {
        return Ok(name);
    }
    let mut text = name_text.to_string16();
    text.push_str16(String16::from(": ").as_str16());
    text.push_str16(message_text);
    let units = text.len();
    let string = rt.heap.alloc_string(text)?;
    rt.heap.record(string);
    rt.charge_units(units)?;
    Ok(string)
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
    let string = error_header(rt, this)?;
    Ok(NativeReturn::Value(string.into()))
}
