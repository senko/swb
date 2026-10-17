//! The intrinsics of async functions: the `AsyncFunction` constructor
//! (ECMA-262 §27.7.1, <https://tc39.es/ecma262/#sec-async-function-constructor>)
//! and `%AsyncFunction.prototype%` (§27.7.3). The functions themselves run
//! in `vm/async_function.rs`; the source text and compile of
//! `AsyncFunction(...)` are those of the `Function` constructor
//! (`vm/create_function.rs`).
//!
//! `AsyncFunction` is not a global; scripts reach it as
//! `Object.getPrototypeOf(async function () {}).constructor`. The property
//! order is Chromium 148's (measured): `AsyncFunction` has `length`,
//! `name`, `prototype`; `%AsyncFunction.prototype%` has `constructor` and
//! `@@toStringTag`.

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::vm::{DynamicKind, Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

/// Installs the async function intrinsics of a realm;
/// `function_constructor` is `%Function%`, the prototype of
/// `AsyncFunction` (§27.7.2).
pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    function_constructor: Gc<Object>,
) -> VmResult<()> {
    let function_proto = rt.intrinsic(realm, Intrinsic::AsyncFunctionPrototype)?;
    let constructor = rt.new_builtin(realm, "AsyncFunction", 1, async_function, true)?;
    rt.heap
        .set_prototype_of(constructor, Some(function_constructor))
        .map_err(VmError::from)?;
    // §27.7.2.1: not writable, not enumerable, not configurable.
    let prototype = PropertyKey::String(rt.vm.atoms.prototype);
    rt.define(
        constructor,
        prototype,
        function_proto.into(),
        false,
        false,
        false,
    )?;
    // §27.7.3.1: not writable, not enumerable, configurable.
    let constructor_key = PropertyKey::String(rt.vm.atoms.constructor);
    rt.define(
        function_proto,
        constructor_key,
        constructor.into(),
        false,
        false,
        true,
    )?;
    super::to_string_tag(rt, function_proto, "AsyncFunction")
}

/// `AsyncFunction(...parameterArgs, bodyArg)` and its `new` form
/// (§27.7.1.1, `CreateDynamicFunction` with kind async): the prototype of
/// the function comes from `new.target` after the parse, with
/// `%AsyncFunction.prototype%` of the constructor's realm as the fallback;
/// the function has no `prototype` property.
fn async_function(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (code, compiled) = rt.dynamic_function_code(call, DynamicKind::Async)?;
    let proto = rt.prototype_from(call, Intrinsic::AsyncFunctionPrototype)?;
    let function = rt.new_dynamic_function((code, &compiled), proto, call.realm())?;
    Ok(NativeReturn::Value(function.into()))
}
