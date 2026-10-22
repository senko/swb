//! `WeakRef` (ECMA-262 §26.1, <https://tc39.es/ecma262/#sec-weak-ref-objects>)
//! and `FinalizationRegistry` (§26.2), with V8's messages (measured with
//! `tools/probes/js-weak.json`). `cleanupSome` is not defined (a proposal
//! that Chromium 148 does not have).
//!
//! A new `WeakRef` and each `deref` add the target to the kept set
//! (`AddToKeptObjects`, §9.12), which keeps it alive until the host calls
//! `ClearKeptObjects` at the end of the synchronous run
//! (`heap/weak_roots.rs`). The cells of a registry are in
//! `object/weak.rs`; the cleanup job runs from the host
//! (`Runtime::run_finalization_cleanup`), never during a collection.

use crate::heap::Gc;
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::WeakRefPrototype)?;
    super::constructor(rt, realm, "WeakRef", 1, weak_ref_constructor, proto)?;
    super::method(rt, realm, proto, "deref", 0, deref)?;
    super::to_string_tag(rt, proto, "WeakRef")?;
    let proto = rt.intrinsic(realm, Intrinsic::FinalizationRegistryPrototype)?;
    super::constructor(
        rt,
        realm,
        "FinalizationRegistry",
        1,
        registry_constructor,
        proto,
    )?;
    super::method(rt, realm, proto, "register", 2, register)?;
    super::method(rt, realm, proto, "unregister", 1, unregister)?;
    super::to_string_tag(rt, proto, "FinalizationRegistry")
}

/// `WeakRef ( target )` (§26.1.1.1).
fn weak_ref_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error("Constructor WeakRef requires 'new'"));
    }
    let target = rt
        .weak_key(rt.arg(call, 0))
        .ok_or_else(|| VmError::type_error("WeakRef: invalid target"))?;
    let proto = rt.prototype_from(call, Intrinsic::WeakRefPrototype)?;
    rt.heap.add_to_kept_objects(target)?;
    let weak_ref = rt.heap.new_weak_ref(Some(proto), target)?;
    Ok(NativeReturn::Value(weak_ref.into()))
}

/// `WeakRef.prototype.deref ( )` (§26.1.3.2, `WeakRefDeref`, §26.1.4.1).
fn deref(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let this = match call.this() {
        Value::Object(object) if matches!(rt.heap.object(object)?.kind, ObjectKind::WeakRef(_)) => {
            object
        }
        other => {
            return Err(rt.incompatible_receiver("WeakRef.prototype.deref", other));
        }
    };
    let Some(target) = rt.heap.weak_ref_target(this)? else {
        return Ok(NativeReturn::Value(Value::Undefined));
    };
    rt.heap.add_to_kept_objects(target)?;
    Ok(NativeReturn::Value(target.value()))
}

/// `FinalizationRegistry ( cleanupCallback )` (§26.2.1.1).
fn registry_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error(
            "Constructor FinalizationRegistry requires 'new'",
        ));
    }
    let callback = rt.arg(call, 0);
    if !rt.is_callable(callback)? {
        return Err(VmError::type_error(
            "FinalizationRegistry: cleanup must be callable",
        ));
    }
    let proto = rt.prototype_from(call, Intrinsic::FinalizationRegistryPrototype)?;
    let registry = rt.heap.new_registry(Some(proto), callback)?;
    Ok(NativeReturn::Value(registry.into()))
}

/// `RequireInternalSlot(this, [[Cells]])` with V8's message.
fn this_registry(rt: &Runtime, call: &NativeCall, method: &str) -> VmResult<Gc<Object>> {
    if let Value::Object(object) = call.this()
        && rt.heap.registry(object)?.is_some()
    {
        return Ok(object);
    }
    Err(rt.incompatible_receiver(method, call.this()))
}

/// V8's message for an unregister token that cannot be held weakly
/// ("Invalid unregisterToken ('1')").
fn invalid_token(rt: &Runtime, token: Value) -> VmError {
    VmError::type_error(format!(
        "Invalid unregisterToken ('{}')",
        rt.value_text(token)
    ))
}

/// `FinalizationRegistry.prototype.register ( target, heldValue [ ,
/// unregisterToken ] )` (§26.2.3.2).
fn register(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let registry = this_registry(rt, call, "FinalizationRegistry.prototype.register")?;
    let target = rt.arg(call, 0);
    let held = rt.arg(call, 1);
    let Some(weak_target) = rt.weak_key(target) else {
        return Err(VmError::type_error(
            "FinalizationRegistry.prototype.register: invalid target",
        ));
    };
    if rt.heap.same_value(target, held)? {
        return Err(VmError::type_error(
            "FinalizationRegistry.prototype.register: target and holdings must not be same",
        ));
    }
    let token = rt.arg(call, 2);
    let weak_token = match rt.weak_key(token) {
        Some(key) => Some(key),
        None if token.is_undefined() => None,
        None => return Err(invalid_token(rt, token)),
    };
    rt.heap
        .registry_register(registry, weak_target, held, weak_token)?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `FinalizationRegistry.prototype.unregister ( unregisterToken )`
/// (§26.2.3.3).
fn unregister(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let registry = this_registry(rt, call, "FinalizationRegistry.prototype.unregister")?;
    let token = rt.arg(call, 0);
    let Some(weak_token) = rt.weak_key(token) else {
        return Err(invalid_token(rt, token));
    };
    let removed = rt.heap.registry_unregister(registry, weak_token)?;
    Ok(NativeReturn::Value(Value::Bool(removed)))
}
