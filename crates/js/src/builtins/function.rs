//! `Function.prototype` (ECMA-262 §20.2.3): `call`, `apply` and
//! `toString`. `call` and `apply` return a deferred call (memo 1.2), so
//! the called function runs as a script-to-script call of the
//! interpreter loop, without Rust recursion.

use swb_js_syntax::messages::NOT_SUPPORTED;

use crate::object::{ObjectKind, Property};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::checked_array_length;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, STACK_OVERFLOW, VmError, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::FunctionPrototype)?;
    super::constructor(rt, realm, "Function", 1, function_constructor, proto)?;
    let methods: [(&str, u32, NativeFn); 3] = [
        ("call", 1, call),
        ("apply", 2, apply),
        ("toString", 0, to_string),
    ];
    for (name, length, func) in methods {
        super::method(rt, realm, proto, name, length, func)?;
    }
    Ok(())
}

/// `Function(...)` (§20.2.1.1): creating functions from text is M7 (it
/// needs the compiler at run time and the host's `eval` check).
fn function_constructor(_: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    Err(VmError::Raise {
        kind: crate::error::ThrowKind::SyntaxError,
        message: format!("{NOT_SUPPORTED} (Function constructor)").into(),
    })
}

/// `Function.prototype.call(thisArg, ...args)` (§20.2.3.3). A receiver
/// that is not callable fails in the deferred call ("... is not a
/// function").
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn call(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let args = (1..call.argc()).map(|i| rt.arg(call, i)).collect();
    Ok(NativeReturn::Call {
        callee: call.this(),
        this: rt.arg(call, 0),
        args,
    })
}

/// `Function.prototype.apply(thisArg, argArray)` (§20.2.3.1) with
/// `CreateListFromArrayLike` (§7.3.19).
fn apply(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this_arg = rt.arg(call, 0);
    let args = match rt.arg(call, 1) {
        Value::Undefined | Value::Null => Vec::new(),
        Value::Object(list) => {
            let length = rt.length_of_array_like(list)?;
            // The arguments go into the value stack; a list that cannot
            // fit fails before it is read (V8 also fails with a
            // `RangeError`).
            checked_array_length(length)?;
            if length > rt.vm.stack_limit as f64 {
                return Err(VmError::range_error(STACK_OVERFLOW));
            }
            let mut args = Vec::with_capacity(length as usize);
            for index in 0..length as u32 {
                rt.tick()?;
                // Each value is recorded in the native's scope until the
                // deferred call moves the list into the value stack.
                args.push(rt.get_value(list.into(), PropertyKey::Index(index))?);
            }
            args
        }
        _ => {
            return Err(VmError::type_error(
                "CreateListFromArrayLike called on non-object",
            ));
        }
    };
    Ok(NativeReturn::Call {
        callee: call.this(),
        this: this_arg,
        args,
    })
}

/// `Function.prototype.toString()` (§20.2.3.5): the source text of a
/// function of script code; `function name() { [native code] }` for a
/// native function.
fn to_string(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(function) = call.this() else {
        return Err(not_a_function());
    };
    let text = match &rt.heap.object(function)?.kind {
        ObjectKind::Function(closure) => closure
            .compiled
            .source_text()
            .ok_or(VmError::invariant("a function without its source text"))?,
        ObjectKind::Native(_) => {
            let key = PropertyKey::String(rt.vm.atoms.name);
            let name = match rt.heap.get_own_property(function, key)? {
                Some(Property::Data {
                    value: Value::String(name),
                    ..
                }) => rt.name_text(name),
                _ => String::new(),
            };
            swb_js_text::String16::from(format!("function {name}() {{ [native code] }}").as_str())
        }
        _ => return Err(not_a_function()),
    };
    let units = text.len();
    let string = rt.heap.alloc_string(text)?;
    rt.charge_units(units)?;
    Ok(NativeReturn::Value(string.into()))
}

fn not_a_function() -> VmError {
    VmError::type_error("Function.prototype.toString requires that 'this' be a Function")
}
