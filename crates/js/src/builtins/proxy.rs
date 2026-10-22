//! `Proxy` (ECMA-262 §28.2,
//! <https://tc39.es/ecma262/#sec-proxy-objects>): the constructor and
//! `Proxy.revocable`. The proxy objects and their internal methods are in
//! `vm/proxy.rs`.
//!
//! `Proxy` has no `prototype` property; it is a constructor only (a call
//! without `new` is V8's `TypeError`). The revoke function of
//! `Proxy.revocable` keeps the proxy in its one internal slot
//! (`[[RevocableProxy]]`), which becomes `null` when it has revoked it.

use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proxy = rt.new_builtin(realm, "Proxy", 2, proxy_constructor, true)?;
    super::method(rt, realm, proxy, "revocable", 2, revocable)?;
    super::define_global(rt, realm, "Proxy", proxy.into())
}

/// `Proxy(target, handler)` (§28.2.1.1).
fn proxy_constructor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error("Constructor Proxy requires 'new'"));
    }
    let proxy = rt.proxy_create(rt.arg(call, 0), rt.arg(call, 1))?;
    Ok(NativeReturn::Value(proxy.into()))
}

/// `Proxy.revocable(target, handler)` (§28.2.2.1): `{ proxy, revoke }`.
fn revocable(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let proxy = rt.proxy_create(rt.arg(call, 0), rt.arg(call, 1))?;
    let slots = vec![Value::Object(proxy)].into_boxed_slice();
    let revoke = rt.new_builtin_with_slots(call.realm(), 0, revoke, slots)?;
    let proto = rt.intrinsic(rt.current_realm(), crate::vm::Intrinsic::ObjectPrototype)?;
    let result: crate::heap::Gc<Object> = rt.heap.new_object(Some(proto))?;
    rt.heap.record(result);
    for (name, value) in [("proxy", Value::Object(proxy)), ("revoke", revoke.into())] {
        let key: PropertyKey = rt.heap.key_from_str(name)?;
        rt.heap.record(key);
        rt.create_data_property_or_throw(result, key, value)?;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// A revoke function (§28.2.2.1.1): revokes its proxy once.
fn revoke(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let function = call.callee();
    if let Value::Object(proxy) = rt.native_slot(function, 0)? {
        rt.set_native_slot(function, 0, Value::Null)?;
        rt.heap.revoke_proxy(proxy)?;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}
