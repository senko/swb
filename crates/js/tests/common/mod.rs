//! The harness of the VM tests: runs a script with a `print` function
//! that collects lines, on a thread with an 8 MiB stack (ADR 0026
//! section 9), with and without the GC stress mode.

#![allow(dead_code, clippy::unwrap_used, clippy::unnecessary_wraps)]

use swb_js::{
    HeapConfig, NativeCall, NativeReturn, Runtime, RuntimeConfig, ScriptError, Value, VmError,
    VmResult,
};

/// The output lines of `print`.
#[derive(Default)]
pub(crate) struct Output(pub(crate) Vec<String>);

/// `print(...)`: `ToString` of each argument, joined by spaces.
pub(crate) fn print(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut parts = Vec::new();
    for i in 0..call.argc() {
        let value = rt.arg(call, i);
        parts.push(rt.to_rust_string(value)?);
    }
    if let Some(output) = rt.host_data_mut::<Output>() {
        output.0.push(parts.join(" "));
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `defineAccessor(object, name, getter, setter)`: an accessor property
/// (until `Object.defineProperty` exists).
pub(crate) fn define_accessor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let object = rt.arg(call, 0);
    let name = rt.arg(call, 1);
    let name = rt.to_rust_string(name)?;
    let getter = rt.arg(call, 2);
    let setter = rt.arg(call, 3);
    rt.define_accessor(object, &name, getter, setter)?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `callTwice(f, x)`: calls `f(x)` twice through re-entry and returns the
/// sum of the results (a native that calls back).
pub(crate) fn call_twice(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let f = rt.arg(call, 0);
    let x = rt.arg(call, 1);
    let a = rt.call(f, Value::Undefined, &[x])?;
    let b = rt.call(f, Value::Undefined, &[x])?;
    let (Some(a), Some(b)) = (a.as_number(), b.as_number()) else {
        return Err(VmError::type_error("callTwice: not numbers"));
    };
    Ok(NativeReturn::Value(Value::from(a + b)))
}

/// `later(f, a, b)`: a deferred call of `f(a, b)` (the protocol that
/// `Function.prototype.call` uses).
pub(crate) fn later(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let f = rt.arg(call, 0);
    let args = (1..call.argc()).map(|i| rt.arg(call, i)).collect();
    Ok(NativeReturn::Call {
        callee: f,
        this: Value::Undefined,
        args,
    })
}

/// `gc()`: a full collection now.
pub(crate) fn gc(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    rt.collect();
    Ok(NativeReturn::Value(Value::Undefined))
}

/// A runtime with the test functions.
pub(crate) fn runtime(stress: bool) -> Runtime {
    let config = RuntimeConfig {
        heap: HeapConfig {
            stress,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    };
    let mut rt = Runtime::new(config).unwrap();
    rt.set_host_data(Output::default());
    rt.define_global_function("print", 1, print).unwrap();
    rt.define_global_function("defineAccessor", 4, define_accessor)
        .unwrap();
    rt.define_global_function("callTwice", 2, call_twice)
        .unwrap();
    rt.define_global_function("later", 1, later).unwrap();
    rt.define_global_function("gc", 0, gc).unwrap();
    rt
}

/// The printed lines and the result (the completion value as text, or
/// the error) of a script.
pub(crate) fn run_in(rt: &mut Runtime, source: &str) -> String {
    let result = rt.eval(source);
    let mut lines = std::mem::take(&mut rt.host_data_mut::<Output>().unwrap().0);
    match result {
        Ok(value) => {
            if !value.is_undefined() {
                lines.push(format!("=> {}", rt.display(value)));
            }
        }
        Err(error) => lines.push(error_text(&error)),
    }
    lines.join("\n")
}

/// The text of an error, without the offset.
pub(crate) fn error_text(error: &ScriptError) -> String {
    match error {
        ScriptError::Uncaught { name, message, .. } if name.is_empty() => {
            format!("Uncaught {message}")
        }
        ScriptError::Uncaught { name, message, .. } => format!("Uncaught {name}: {message}"),
        other => other.to_string(),
    }
}

/// Runs `f` on a thread with an 8 MiB stack.
pub(crate) fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

/// Runs a script in a new runtime and checks that no collection found a
/// stale root.
pub(crate) fn run(source: &str, stress: bool) -> String {
    let source = source.to_owned();
    on_big_stack(move || {
        let mut rt = runtime(stress);
        let out = run_in(&mut rt, &source);
        let stats = rt.heap().stats();
        assert_eq!(stats.total_stale_roots, 0, "stale roots in {source}");
        if stress {
            assert!(stats.collections > 0, "no collection in stress mode");
        }
        out
    })
}

/// Runs each case (source, expected output) with and without the stress
/// mode; reports all mismatches at once.
pub(crate) fn check_cases(cases: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for &(source, expected) in cases {
        for stress in [false, true] {
            let actual = run(source, stress);
            if actual != expected {
                failures.push(format!(
                    "--- {source}\n(stress: {stress})\nexpected:\n{expected}\nactual:\n{actual}\n"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
