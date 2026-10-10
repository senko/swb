//! `Function.prototype` (ECMA-262 §20.2.3): `call`, `apply`, `bind`,
//! `toString` and `@@hasInstance`. `call` and `apply` return a deferred call (memo 1.2), so
//! the called function runs as a script-to-script call of the
//! interpreter loop, without Rust recursion. `bind` creates a bound
//! function exotic object (§10.4.1); calls of bound functions are in
//! `vm/call.rs`.

use swb_js_syntax::messages::NOT_SUPPORTED;

use crate::object::{ObjectKind, Property};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::{
    BoundFunction, Intrinsic, NativeCall, NativeFn, NativeReturn, VmError, VmResult, WellKnown,
};

/// The longest name, in code units, that `bind` gives a bound function.
const BOUND_NAME_LIMIT: usize = 1 << 15;

pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::FunctionPrototype)?;
    super::constructor(rt, realm, "Function", 1, function_constructor, proto)?;
    let methods: [(&str, u32, NativeFn); 4] = [
        ("call", 1, call),
        ("apply", 2, apply),
        ("bind", 1, bind),
        ("toString", 0, to_string),
    ];
    for (name, length, func) in methods {
        super::method(rt, realm, proto, name, length, func)?;
    }
    let has_instance = super::symbol_method(
        rt,
        realm,
        proto,
        WellKnown::HasInstance,
        1,
        has_instance,
        (false, false),
    )?;
    rt.set_intrinsic(realm, Intrinsic::FunctionHasInstance, has_instance);
    Ok(())
}

/// `Function.prototype[@@hasInstance](V)` (§20.2.3.6):
/// `OrdinaryHasInstance(this, V)`.
fn has_instance(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let result = rt.ordinary_has_instance(call.this(), rt.arg(call, 0))?;
    Ok(NativeReturn::Value(Value::Bool(result)))
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
        list => rt.list_from_array_like(list)?,
    };
    Ok(NativeReturn::Call {
        callee: call.this(),
        this: this_arg,
        args,
    })
}

/// `Function.prototype.bind(thisArg, ...args)` (§20.2.3.2): a bound
/// function exotic object (`BoundFunctionCreate`, §10.4.1.3) with the
/// `length` and `name` of the specification.
fn bind(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(target) = call.this() else {
        return Err(VmError::type_error("Bind must be called on a function"));
    };
    if !rt.is_callable(call.this())? {
        return Err(VmError::type_error("Bind must be called on a function"));
    }
    let bound_args: Box<[Value]> = (1..call.argc()).map(|i| rt.arg(call, i)).collect();
    let arg_count = bound_args.len();
    let constructor = rt.is_constructor(call.this())?;
    let proto = rt.heap.get_prototype_of(target)?;
    let kind = ObjectKind::Bound(Box::new(BoundFunction {
        target,
        this: rt.arg(call, 0),
        args: bound_args,
        constructor,
    }));
    let bound = rt.heap.new_object_with_kind(proto, kind)?;
    rt.heap.record(bound);
    let length = bound_length(rt, target, arg_count)?;
    let length_key = PropertyKey::String(rt.heap.length_atom());
    rt.define(bound, length_key, length, false, false, true)?;
    let name = bound_name(rt, target)?;
    let name_key = PropertyKey::String(rt.vm.atoms.name);
    rt.define(bound, name_key, name.into(), false, false, true)?;
    Ok(NativeReturn::Value(bound.into()))
}

/// The `length` of a bound function (§20.2.3.2 steps 5 to 7).
fn bound_length(
    rt: &mut Runtime,
    target: crate::heap::Gc<crate::object::Object>,
    arg_count: usize,
) -> VmResult<Value> {
    let key = PropertyKey::String(rt.heap.length_atom());
    if rt.own_property(target, key)?.is_none() {
        return Ok(Value::Int(0));
    }
    let Some(length) = rt.get_value(target.into(), key)?.as_number() else {
        return Ok(Value::Int(0));
    };
    let result = if length == f64::INFINITY {
        length
    } else if length == f64::NEG_INFINITY {
        0.0
    } else {
        (to_integer_or_infinity(length) - arg_count as f64).max(0.0)
    };
    Ok(Value::number(result))
}

/// The `name` of a bound function: `"bound "` and the name of the target
/// (`SetFunctionName`, §10.2.9, with the prefix "bound").
fn bound_name(
    rt: &mut Runtime,
    target: crate::heap::Gc<crate::object::Object>,
) -> VmResult<crate::heap::Gc<crate::string::JsString>> {
    let key = PropertyKey::String(rt.vm.atoms.name);
    let name = match rt.get_value(target.into(), key)? {
        Value::String(name) => name,
        _ => rt.vm.atoms.empty,
    };
    // Every bound function stores its own name, so a chain of depth n
    // holds O(n^2) code units (V8 shares them in ropes; the strings here
    // are flat, ADR 0026 section 7). A limit keeps a hostile chain from
    // filling the heap: the 5,000th `bind` of a `bind` result fails.
    if rt.heap.string(name)?.len() + "bound ".len() > BOUND_NAME_LIMIT {
        return Err(VmError::range_error("Invalid string length"));
    }
    let prefix = rt.heap.alloc_str("bound ")?;
    rt.heap.record(prefix);
    rt.concat(prefix, name)
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
        ObjectKind::Bound(_) => swb_js_text::String16::from("function () { [native code] }"),
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

#[cfg(test)]
mod tests {
    use crate::object::ObjectKind;
    use crate::runtime::{Runtime, RuntimeConfig};
    use crate::value::Value;
    use crate::vm::{BoundFunction, Intrinsic, VmError, VmResult};

    /// Wraps `F` in `depth` bound functions without `bind` (whose names
    /// grow with the depth), each with `args` bound arguments, and stores
    /// the chain in the global `name`.
    fn make_chain(rt: &mut Runtime, name: &str, depth: usize, args: &[Value]) -> VmResult<()> {
        rt.scoped(|rt| {
            let global = rt.global_object(0)?;
            let key = rt.heap.key_from_str("F")?;
            let Value::Object(mut current) = rt.get_value(global.into(), key)? else {
                return Err(VmError::invariant("F is not an object"));
            };
            let proto = rt.intrinsic(0, Intrinsic::FunctionPrototype)?;
            for _ in 0..depth {
                let kind = ObjectKind::Bound(Box::new(BoundFunction {
                    target: current,
                    this: Value::Undefined,
                    args: args.into(),
                    constructor: true,
                }));
                current = rt.heap.new_object_with_kind(Some(proto), kind)?;
                rt.heap.record(current);
            }
            let key = rt.heap.key_from_str(name)?;
            rt.define(global, key, current.into(), true, false, true)
        })
    }

    fn eval_text(rt: &mut Runtime, source: &str) -> String {
        let value = rt.eval(source).expect("the script runs");
        rt.to_rust_string(value).expect("a text")
    }

    /// Chains of 100,000 bound functions call, construct and answer
    /// `instanceof` without Rust recursion (on a thread with the 8 MiB
    /// stack that the engine requires), also in the GC stress mode.
    #[test]
    fn deep_chains_of_bound_functions_need_no_rust_recursion() {
        for stress in [false, true] {
            let handle = std::thread::Builder::new()
                .stack_size(8 << 20)
                .spawn(move || {
                    let mut config = RuntimeConfig::default();
                    config.heap.stress = stress;
                    let mut rt = Runtime::new(config).expect("a runtime");
                    rt.eval("function F() { this.n = arguments.length; }")
                        .expect("F");
                    let depth = 100_000;
                    make_chain(&mut rt, "plain", depth, &[]).expect("chain");
                    make_chain(&mut rt, "with_args", depth, &[Value::Int(7)]).expect("chain");
                    assert_eq!(eval_text(&mut rt, "plain(1, 2); 'called'"), "called");
                    assert_eq!(eval_text(&mut rt, "new plain(1, 2).n"), "2");
                    assert_eq!(
                        eval_text(
                            &mut rt,
                            "(new plain() instanceof F) + ',' + (new plain() instanceof plain)"
                        ),
                        "true,true"
                    );
                    // 100,000 bound arguments and 3 more.
                    assert_eq!(eval_text(&mut rt, "new with_args(1, 2, 3).n"), "100003");
                    assert_eq!(eval_text(&mut rt, "with_args.call(null); 'ok'"), "ok");
                    assert_eq!(rt.stack_depths(), (0, 0));
                })
                .expect("a thread");
            handle.join().expect("no panic");
        }
    }
}
