//! `Promise` (ECMA-262 §27.2, <https://tc39.es/ecma262/#sec-promise-objects>):
//! the constructor, `resolve`, `reject`, `withResolvers`, `try`,
//! `@@species`, and `Promise.prototype` with `then`, `catch`, `finally`
//! and `@@toStringTag`. The combinators are in `promise_combinators.rs`,
//! the promise objects and the abstract operations in `vm/promise.rs`, the
//! jobs in `vm/jobs.rs`.
//!
//! Key order and messages are Chromium 148's (measured with
//! `tools/probes/js-promises.json`): `Promise` has `length`, `name`,
//! `prototype`, `all`, `allSettled`, `any`, `race`, `resolve`, `reject`,
//! `withResolvers`, `try`, `@@species`; `Promise.prototype` has
//! `constructor`, `then`, `catch`, `finally`, `@@toStringTag`.
//!
//! `catch`, `finally` and the closures of `finally` end in `Invoke(p,
//! "then", ...)`; they return it as a deferred call, so a chain of them
//! costs no Rust recursion.

use crate::heap::Gc;
use crate::object::{Object, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown};

/// The static functions in Chromium's order, with their `length`.
const STATICS: [(&str, u32, NativeFn); 8] = [
    ("all", 1, super::promise_combinators::all),
    ("allSettled", 1, super::promise_combinators::all_settled),
    ("any", 1, super::promise_combinators::any),
    ("race", 1, super::promise_combinators::race),
    ("resolve", 1, resolve),
    ("reject", 1, reject),
    ("withResolvers", 0, with_resolvers),
    ("try", 1, try_),
];

/// The methods of `Promise.prototype` in Chromium's order.
const METHODS: [(&str, u32, NativeFn); 3] = [
    ("then", 2, then),
    ("catch", 1, catch),
    ("finally", 1, finally),
];

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::PromisePrototype)?;
    let promise = super::constructor(rt, realm, "Promise", 1, promise_constructor, proto)?;
    rt.set_intrinsic(realm, Intrinsic::PromiseConstructor, promise);
    for (name, length, func) in STATICS {
        super::method(rt, realm, promise, name, length, func)?;
    }
    let getter = rt.new_builtin(realm, "get [Symbol.species]", 0, species, false)?;
    rt.set_intrinsic(realm, Intrinsic::PromiseSpeciesGetter, getter);
    let key = rt.symbol_key(WellKnown::Species);
    let desc = PropertyDescriptor::accessor(getter.into(), Value::Undefined, false, true);
    rt.define_own_property(promise, key, desc)?;
    for (name, length, func) in METHODS {
        super::method(rt, realm, proto, name, length, func)?;
    }
    super::to_string_tag(rt, proto, "Promise")
}

/// `Promise(executor)` (§27.2.3.1).
fn promise_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error(
            "Promise constructor cannot be invoked without 'new'",
        ));
    }
    let executor = rt.arg(call, 0);
    if !rt.is_callable(executor)? {
        return Err(VmError::type_error(format!(
            "Promise resolver {} is not a function",
            rt.primitive_text(executor)
        )));
    }
    let proto = rt.prototype_from(call, Intrinsic::PromisePrototype)?;
    let promise = rt.new_promise_with(proto)?;
    let (resolve, reject) = rt.create_resolving_functions(promise, call.realm())?;
    let result = rt.call(executor, Value::Undefined, &[resolve.into(), reject.into()]);
    if let Err(reason) = rt.completion(result)? {
        rt.reject_with_function(reject, reason)?;
    }
    Ok(NativeReturn::Value(promise.into()))
}

/// `get Promise[@@species]` (§27.2.4.9): the receiver.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn species(_: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

/// The receiver of a static function as an object (V8's message for
/// others: "Promise.all called on non-object").
pub(super) fn this_constructor(call: &NativeCall, name: &str) -> VmResult<Gc<Object>> {
    match call.this() {
        Value::Object(object) => Ok(object),
        _ => Err(VmError::type_error(format!("{name} called on non-object"))),
    }
}

/// `Promise.resolve(x)` (§27.2.4.7).
fn resolve(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let constructor = this_constructor(call, "PromiseResolve")?;
    let value = rt.arg(call, 0);
    let promise = rt.promise_resolve(constructor, value, call.realm())?;
    Ok(NativeReturn::Value(promise))
}

/// `Promise.reject(r)` (§27.2.4.6).
fn reject(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let constructor = this_constructor(call, "PromiseReject")?;
    let capability = rt.new_promise_capability(constructor.into(), call.realm(), false)?;
    let reason = rt.arg(call, 0);
    rt.capability_reject(&capability, reason)?;
    Ok(NativeReturn::Value(capability.promise()))
}

/// `Promise.withResolvers()` (§27.2.4.8).
fn with_resolvers(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let constructor = this_constructor(call, "Promise.withResolvers")?;
    let capability = rt.new_promise_capability(constructor.into(), call.realm(), true)?;
    let [promise, resolve, reject] = capability.values();
    let proto = rt.intrinsic(call.realm(), Intrinsic::ObjectPrototype)?;
    let object = rt.heap.new_object(Some(proto))?;
    rt.heap.record(object);
    for (name, value) in [
        ("promise", promise),
        ("resolve", resolve),
        ("reject", reject),
    ] {
        let key = rt.heap.key_from_str(name)?;
        rt.create_data_property_or_throw(object, key, value)?;
    }
    Ok(NativeReturn::Value(object.into()))
}

/// `Promise.try(callbackfn, ...args)` (§27.2.4.8 of ES2025+).
fn try_(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let constructor = this_constructor(call, "Promise.try")?;
    let capability = rt.new_promise_capability(constructor.into(), call.realm(), false)?;
    let callback = rt.arg(call, 0);
    let result = if rt.is_callable(callback)? {
        let args: Vec<Value> = (1..call.argc()).map(|i| rt.arg(call, i)).collect();
        rt.call(callback, Value::Undefined, &args)
    } else {
        Err(rt.not_callable(callback))
    };
    match rt.completion(result)? {
        Ok(value) => rt.capability_resolve(&capability, value)?,
        Err(reason) => rt.capability_reject(&capability, reason)?,
    }
    Ok(NativeReturn::Value(capability.promise()))
}

/// `Promise.prototype.then(onFulfilled, onRejected)` (§27.2.5.4).
fn then(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Some(promise) = rt.as_promise(call.this())? else {
        return Err(VmError::type_error(format!(
            "Method Promise.prototype.then called on incompatible receiver {}",
            rt.primitive_text(call.this())
        )));
    };
    let realm = call.realm();
    let constructor = rt.promise_species_constructor(promise, realm)?;
    let capability = rt.new_promise_capability(constructor, realm, false)?;
    let on_fulfilled = rt.handler_of(rt.arg(call, 0))?;
    let on_rejected = rt.handler_of(rt.arg(call, 1))?;
    let result = capability.promise();
    rt.perform_promise_then(promise, on_fulfilled, on_rejected, Some(capability))?;
    Ok(NativeReturn::Value(result))
}

/// `Invoke(target, "then", args)` (§7.3.20) as a deferred call; a `then`
/// that is not callable is V8's "number 1 is not a function".
pub(super) fn invoke_then(
    rt: &mut Runtime,
    target: Value,
    args: Vec<Value>,
) -> VmResult<NativeReturn> {
    let key = PropertyKey::String(rt.vm.atoms.then);
    let then = rt.get_value(target, key)?;
    if !rt.is_callable(then)? {
        return Err(rt.not_callable(then));
    }
    Ok(NativeReturn::Call {
        callee: then,
        this: target,
        args,
    })
}

/// `Promise.prototype.catch(onRejected)` (§27.2.5.1).
fn catch(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let on_rejected = rt.arg(call, 0);
    invoke_then(rt, call.this(), vec![Value::Undefined, on_rejected])
}

/// `Promise.prototype.finally(onFinally)` (§27.2.5.3).
fn finally(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(promise) = call.this() else {
        return Err(VmError::type_error(
            "Promise.prototype.finally called on non-object",
        ));
    };
    let realm = call.realm();
    let constructor = rt.promise_species_constructor(promise, realm)?;
    let on_finally = rt.arg(call, 0);
    let (then_finally, catch_finally) = if rt.is_callable(on_finally)? {
        let slots = || Box::new([on_finally, constructor]) as Box<[Value]>;
        let then_finally = rt.new_builtin_with_slots(realm, 1, then_finally, slots())?;
        let catch_finally = rt.new_builtin_with_slots(realm, 1, catch_finally, slots())?;
        (then_finally.into(), catch_finally.into())
    } else {
        (on_finally, on_finally)
    };
    invoke_then(rt, promise.into(), vec![then_finally, catch_finally])
}

/// The common steps of `thenFinally` and `catchFinally` (§27.2.5.3 steps
/// 6.a and 6.c): call `onFinally`, `PromiseResolve(C, result)`, then
/// `Invoke(p, "then", « f »)` with the function that `make` creates.
fn finally_step(rt: &mut Runtime, call: &NativeCall, settle: NativeFn) -> VmResult<NativeReturn> {
    let function = call.callee();
    let on_finally = rt.native_slot(function, 0)?;
    let Value::Object(constructor) = rt.native_slot(function, 1)? else {
        return Err(VmError::invariant(
            "a finally closure without its constructor",
        ));
    };
    let result = rt.call(on_finally, Value::Undefined, &[])?;
    let promise = rt.promise_resolve(constructor, result, call.realm())?;
    let slots = Box::new([rt.arg(call, 0)]) as Box<[Value]>;
    let settle = rt.new_builtin_with_slots(call.realm(), 0, settle, slots)?;
    invoke_then(rt, promise, vec![settle.into()])
}

/// `thenFinally(value)`.
fn then_finally(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    finally_step(rt, call, value_thunk)
}

/// `catchFinally(reason)`.
fn catch_finally(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    finally_step(rt, call, thrower)
}

/// `valueThunk`: returns the value of `thenFinally`.
fn value_thunk(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(rt.native_slot(call.callee(), 0)?))
}

/// `thrower`: throws the reason of `catchFinally`.
fn thrower(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Err(VmError::Throw(rt.native_slot(call.callee(), 0)?))
}
