//! The intrinsics of generators: the `GeneratorFunction` constructor
//! (ECMA-262 §27.3.1, <https://tc39.es/ecma262/#sec-generatorfunction-constructor>),
//! `%GeneratorFunction.prototype%` (§27.3.3) and `%GeneratorPrototype%`
//! (§27.5.1, <https://tc39.es/ecma262/#sec-properties-of-generator-prototype>).
//! The generator objects themselves run in `vm/generator.rs`; the source
//! text and compile of `GeneratorFunction(...)` are those of the
//! `Function` constructor (`vm/create_function.rs`).
//!
//! `GeneratorFunction` is not a global; scripts reach it as
//! `Object.getPrototypeOf(function* () {}).constructor`. The property
//! order is Chromium 148's (measured): `GeneratorFunction.prototype` has
//! `prototype`, `constructor`, `@@toStringTag`; `%GeneratorPrototype%`
//! has `constructor`, `next`, `return`, `throw`, `@@toStringTag`.

use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::vm::{
    DynamicKind, Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, generator_next,
    generator_return, generator_throw,
};

/// Installs the generator intrinsics of a realm; `function_constructor`
/// is `%Function%`, the prototype of `GeneratorFunction` (§27.3.2).
pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    function_constructor: Gc<Object>,
) -> VmResult<()> {
    let function_proto = rt.intrinsic(realm, Intrinsic::GeneratorFunctionPrototype)?;
    let generator_proto = rt.intrinsic(realm, Intrinsic::GeneratorPrototype)?;
    let constructor = rt.new_builtin(realm, "GeneratorFunction", 1, generator_function, true)?;
    rt.heap
        .set_prototype_of(constructor, Some(function_constructor))
        .map_err(VmError::from)?;
    let prototype = PropertyKey::String(rt.vm.atoms.prototype);
    let constructor_key = PropertyKey::String(rt.vm.atoms.constructor);
    // §27.3.2.1: not writable, not enumerable, not configurable.
    rt.define(
        constructor,
        prototype,
        function_proto.into(),
        false,
        false,
        false,
    )?;
    // §27.3.3.1, §27.3.3.2 and §27.5.1.1: not writable, not enumerable,
    // configurable.
    for (object, key, value) in [
        (function_proto, prototype, generator_proto),
        (function_proto, constructor_key, constructor),
    ] {
        rt.define(object, key, value.into(), false, false, true)?;
    }
    super::to_string_tag(rt, function_proto, "GeneratorFunction")?;
    rt.define(
        generator_proto,
        constructor_key,
        function_proto.into(),
        false,
        false,
        true,
    )?;
    let methods: [(&str, NativeFn); 3] = [
        ("next", generator_next),
        ("return", generator_return),
        ("throw", generator_throw),
    ];
    for (name, func) in methods {
        super::method(rt, realm, generator_proto, name, 1, func)?;
    }
    super::to_string_tag(rt, generator_proto, "Generator")
}

/// `GeneratorFunction(...parameterArgs, bodyArg)` and its `new` form
/// (§27.3.1.1, `CreateDynamicFunction` with kind generator): the prototype
/// of the function comes from `new.target` after the parse, with
/// `%GeneratorFunction.prototype%` of the constructor's realm as the
/// fallback; the function's own `prototype` inherits from
/// `%GeneratorPrototype%` (step 33).
fn generator_function(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (code, compiled) = rt.dynamic_function_code(call, DynamicKind::Generator)?;
    let proto = rt.prototype_from(call, Intrinsic::GeneratorFunctionPrototype)?;
    let function = rt.new_dynamic_function((code, &compiled), proto, call.realm())?;
    Ok(NativeReturn::Value(function.into()))
}
