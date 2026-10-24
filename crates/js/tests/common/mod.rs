//! The harness of the VM tests: runs a script with a `print` function
//! that collects lines (`console.log` writes to the same lines), on a
//! thread with an 8 MiB stack (ADR 0026 section 9), with and without the
//! GC stress mode.

// Every integration test file compiles this module separately and uses a
// part of it, so the helpers that a file does not use are dead code there.
#![allow(dead_code, clippy::unwrap_used, clippy::unnecessary_wraps)]

pub(crate) mod host;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use swb_js::{
    HeapConfig, NativeCall, NativeReturn, Runtime, RuntimeConfig, ScriptError, Termination, Value,
    VmError, VmResult,
};

/// The output lines of `print` and `console.log`.
#[derive(Clone, Default)]
pub(crate) struct Output(pub(crate) Rc<RefCell<Vec<String>>>);

impl swb_js::HostState for Output {}

impl Output {
    /// Takes the lines written so far.
    pub(crate) fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

/// `print(...)`: `ToString` of each argument, joined by spaces.
pub(crate) fn print(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut parts = Vec::new();
    for i in 0..call.argc() {
        let value = rt.arg(call, i);
        parts.push(rt.to_rust_string(value)?);
    }
    if let Some(output) = rt.host_data_mut::<Output>() {
        output.0.borrow_mut().push(parts.join(" "));
    }
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

/// A runtime with a given configuration and no test functions.
pub(crate) fn runtime_with(config: RuntimeConfig) -> Runtime {
    Runtime::new(config).unwrap()
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
    runtime_with_tests(config)
}

/// A runtime with a given configuration and the test functions.
pub(crate) fn runtime_with_tests(config: RuntimeConfig) -> Runtime {
    let mut rt = runtime_with(config);
    let output = Output::default();
    let sink = output.clone();
    rt.set_console(Box::new(move |_level, line| {
        sink.0.borrow_mut().push(line.to_owned());
    }));
    rt.set_host_data(output);
    rt.define_global_function("print", 1, print).unwrap();
    // `defineAccessor(object, name, getter, setter)`: an accessor property,
    // defined in script.
    rt.eval(
        "Object.defineProperty(globalThis, 'defineAccessor', { value: function (o, k, g, s) { \
         Object.defineProperty(o, k, { get: g, set: s, enumerable: true, configurable: true }); } })",
    )
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
    run_in_named(rt, source, "")
}

/// Like [`run_in`], with the script name that stack traces show.
pub(crate) fn run_in_named(rt: &mut Runtime, source: &str, name: &str) -> String {
    // The promise jobs run after the script, as in the shell.
    let result = rt
        .eval_named(source, name)
        .and_then(|value| rt.run_jobs().map(|()| value));
    let mut lines = rt.host_data_mut::<Output>().unwrap().take();
    match result {
        Ok(value) => {
            if !value.is_undefined() {
                lines.push(format!("=> {}", rt.display(value)));
            }
        }
        Err(error) => lines.push(error.to_string()),
    }
    lines.join("\n")
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

/// Like [`run`], in the time zone `zone` (an IANA name or a POSIX rule)
/// with the fixed clock `now` (milliseconds since the epoch).
pub(crate) fn run_in_zone(source: &str, stress: bool, zone: &str, now: f64) -> String {
    let (source, zone) = (source.to_owned(), zone.to_owned());
    on_big_stack(move || {
        let mut rt = runtime(stress);
        rt.set_time_zone(swb_js::TimeZone::from_spec(&zone).expect("a time zone of the tests"));
        rt.set_clock(Box::new(move || now));
        let out = run_in(&mut rt, &source);
        let stats = rt.heap().stats();
        assert_eq!(stats.total_stale_roots, 0, "stale roots in {source}");
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

/// Runs a script under the name `t.js` (as the stack tests of Node.js
/// do) with and without the stress mode.
pub(crate) fn run_script(source: &str, stress: bool) -> String {
    let source = source.to_owned();
    on_big_stack(move || {
        let mut rt = runtime(stress);
        let out = run_in_named(&mut rt, &source, "t.js");
        let stats = rt.heap().stats();
        assert_eq!(stats.total_stale_roots, 0, "stale roots");
        out
    })
}

/// Like [`check_cases`], for scripts that run under the name `t.js`.
pub(crate) fn check_script_cases(cases: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for &(source, expected) in cases {
        for stress in [false, true] {
            let actual = run_script(source, stress);
            if actual.trim_end() != expected.trim_end() {
                failures.push(format!(
                    "(stress: {stress})\n--- expected:\n{expected}\n--- actual:\n{actual}\n"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// --- Helpers of the hostile and Chromium-case tests ---

/// The state that every run must leave behind: no frames, an empty value
/// stack, no open handle scope, no no-GC region and no stale root.
pub(crate) fn assert_clean(rt: &Runtime) {
    assert_eq!(rt.stack_depths(), (0, 0));
    assert_eq!(rt.heap().scope_depth(), 0);
    assert_eq!(rt.heap().no_gc_depth(), 0);
    assert_eq!(rt.heap().stats().total_stale_roots, 0);
}

/// Runs `source` with a deadline `deadline` after now; returns the result
/// and the time it took. Each test file has its own deadline.
pub(crate) fn eval_within(
    rt: &mut Runtime,
    source: &str,
    deadline: Duration,
) -> (Result<Value, ScriptError>, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + deadline));
    let result = rt.eval(source);
    rt.set_deadline(None);
    (result, start.elapsed())
}

/// Runs `source` and its jobs with a deadline; the result of the drain
/// (or of the script) and the time it took.
pub(crate) fn run_with_deadline(
    rt: &mut Runtime,
    source: &str,
    deadline: Duration,
) -> (String, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + deadline));
    let out = run_in(rt, source);
    rt.set_deadline(None);
    (out, start.elapsed())
}

/// A runtime with a heap limit of `mib` MiB and the test functions.
pub(crate) fn runtime_with_heap_limit(mib: usize, stress: bool) -> Runtime {
    runtime_with_tests(RuntimeConfig {
        heap: HeapConfig {
            limit: mib << 20,
            stress,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    })
}

/// A termination at the time or the heap limit.
pub(crate) fn is_limit(result: &Result<Value, ScriptError>) -> bool {
    matches!(
        result,
        Err(ScriptError::Terminated(
            Termination::TimeLimit | Termination::HeapLimit
        ))
    )
}

/// Runs the script of a file of `data/` in each of the stress `modes` and
/// compares its output line by line with the output that Chromium gave.
pub(crate) fn check_chromium(script: &str, expected: &str, modes: &[bool]) {
    for &stress in modes {
        let actual = run(script, stress);
        let failures: Vec<String> = actual
            .lines()
            .zip(expected.lines())
            .filter(|(actual, expected)| actual != expected)
            .map(|(actual, expected)| format!("expected: {expected}\nactual:   {actual}"))
            .collect();
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "number of lines (stress: {stress})\n{actual}"
        );
        assert!(
            failures.is_empty(),
            "stress: {stress}\n{}",
            failures.join("\n")
        );
    }
}
