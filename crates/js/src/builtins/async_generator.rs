//! The intrinsics of async iteration: `%AsyncIteratorPrototype%`
//! (ECMA-262 §27.1.3, <https://tc39.es/ecma262/#sec-asynciteratorprototype>),
//! `%AsyncFromSyncIteratorPrototype%` (§27.1.6.2), the
//! `AsyncGeneratorFunction` constructor (§27.4.1),
//! `%AsyncGeneratorFunction.prototype%` (§27.4.3) and
//! `%AsyncGeneratorPrototype%` (§27.6.1). The async generator objects run
//! in `vm/async_generator.rs`, the Async-from-Sync Iterator methods in
//! `vm/async_iteration.rs`; the source text and compile of
//! `AsyncGeneratorFunction(...)` are those of the `Function` constructor
//! (`vm/create_function.rs`).
//!
//! `AsyncGeneratorFunction` is not a global; scripts reach it as
//! `Object.getPrototypeOf(async function* () {}).constructor`. The
//! property order is Chromium 148's (measured):
//! `%AsyncGeneratorFunction.prototype%` has `prototype`, `constructor`,
//! `@@toStringTag`; `%AsyncGeneratorPrototype%` has `constructor`, `next`,
//! `return`, `throw`, `@@toStringTag`; `%AsyncIteratorPrototype%` has
//! `@@asyncIterator` (Chromium also has `@@asyncDispose` of the explicit
//! resource management proposal, which swb does not have).

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::vm::async_generator;
use crate::vm::async_iteration::{from_sync_next, from_sync_return, from_sync_throw};
use crate::vm::{
    DynamicKind, Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown,
};

/// Installs the async iteration intrinsics of a realm;
/// `function_constructor` is `%Function%`, the prototype of
/// `AsyncGeneratorFunction` (§27.4.2).
pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    function_constructor: Gc<Object>,
) -> VmResult<()> {
    let async_iterator_proto = rt.intrinsic(realm, Intrinsic::AsyncIteratorPrototype)?;
    super::symbol_method(
        rt,
        realm,
        async_iterator_proto,
        WellKnown::AsyncIterator,
        0,
        async_iterator,
        (true, true),
    )?;
    let from_sync_proto = rt.intrinsic(realm, Intrinsic::AsyncFromSyncIteratorPrototype)?;
    let from_sync: [(&str, NativeFn); 3] = [
        ("next", from_sync_next),
        ("return", from_sync_return),
        ("throw", from_sync_throw),
    ];
    for (name, func) in from_sync {
        super::method(rt, realm, from_sync_proto, name, 1, func)?;
    }
    install_async_generator(rt, realm, function_constructor)
}

/// `AsyncGeneratorFunction`, its prototype and `%AsyncGeneratorPrototype%`.
fn install_async_generator(
    rt: &mut Runtime,
    realm: u32,
    function_constructor: Gc<Object>,
) -> VmResult<()> {
    let function_proto = rt.intrinsic(realm, Intrinsic::AsyncGeneratorFunctionPrototype)?;
    let generator_proto = rt.intrinsic(realm, Intrinsic::AsyncGeneratorPrototype)?;
    let constructor = rt.new_builtin(
        realm,
        "AsyncGeneratorFunction",
        1,
        async_generator_function,
        true,
    )?;
    rt.heap
        .set_prototype_of(constructor, Some(function_constructor))
        .map_err(VmError::from)?;
    let prototype = PropertyKey::String(rt.vm.atoms.prototype);
    let constructor_key = PropertyKey::String(rt.vm.atoms.constructor);
    // §27.4.2.1: not writable, not enumerable, not configurable.
    rt.define(
        constructor,
        prototype,
        function_proto.into(),
        false,
        false,
        false,
    )?;
    // §27.4.3.1, §27.4.3.2 and §27.6.1.1: not writable, not enumerable,
    // configurable.
    for (object, key, value) in [
        (function_proto, prototype, generator_proto),
        (function_proto, constructor_key, constructor),
    ] {
        rt.define(object, key, value.into(), false, false, true)?;
    }
    super::to_string_tag(rt, function_proto, "AsyncGeneratorFunction")?;
    rt.define(
        generator_proto,
        constructor_key,
        function_proto.into(),
        false,
        false,
        true,
    )?;
    let methods: [(&str, NativeFn); 3] = [
        ("next", async_generator::next),
        ("return", async_generator::return_),
        ("throw", async_generator::throw),
    ];
    for (name, func) in methods {
        super::method(rt, realm, generator_proto, name, 1, func)?;
    }
    super::to_string_tag(rt, generator_proto, "AsyncGenerator")
}

/// `%AsyncIteratorPrototype%[@@asyncIterator]()` (§27.1.3.1): returns
/// `this`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn async_iterator(_rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(call.this()))
}

/// `AsyncGeneratorFunction(...parameterArgs, bodyArg)` and its `new` form
/// (§27.4.1.1, `CreateDynamicFunction` with kind async-generator): the
/// prototype of the function comes from `new.target` after the parse, with
/// `%AsyncGeneratorFunction.prototype%` of the constructor's realm as the
/// fallback; the function's own `prototype` inherits from
/// `%AsyncGeneratorPrototype%` (step 34).
fn async_generator_function(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (code, compiled) = rt.dynamic_function_code(call, DynamicKind::AsyncGenerator)?;
    let proto = rt.prototype_from(call, Intrinsic::AsyncGeneratorFunctionPrototype)?;
    let function = rt.new_dynamic_function((code, &compiled), proto, call.realm())?;
    Ok(NativeReturn::Value(function.into()))
}
