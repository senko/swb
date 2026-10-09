//! The runtime of the `swb-js` shell: options, the host functions
//! (`print`, `$262`), and the report of the errors of a run.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};

use swb_js::{
    HeapConfig, NativeCall, NativeReturn, Runtime, RuntimeConfig, ScriptError, Termination, Value,
    VmError, VmResult,
};
use swb_js_syntax::LineIndex;
use swb_js_text::String16;

/// The stack size of the threads that run scripts (ADR 0026 section 9).
pub(crate) const STACK_SIZE: usize = 8 << 20;

/// The limits and modes of a runtime.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Options {
    /// The heap limit in bytes (the engine default if `None`).
    pub(crate) heap_limit: Option<usize>,
    /// Collect at every safepoint.
    pub(crate) stress: bool,
    /// The time limit of each run.
    pub(crate) time_limit: Option<Duration>,
}

/// Where `print` and `console.log` write.
pub(crate) type Output = Rc<RefCell<dyn Write>>;

/// The host data of a runtime: the output of `print`.
struct Host {
    out: Output,
}

/// Creates a runtime with `print`, `console.log` and `$262`.
pub(crate) fn new_runtime(options: &Options, out: Output) -> Result<Runtime, ScriptError> {
    let mut heap = HeapConfig {
        stress: options.stress,
        ..HeapConfig::default()
    };
    if let Some(limit) = options.heap_limit {
        heap.limit = limit;
    }
    let config = RuntimeConfig {
        heap,
        ..RuntimeConfig::default()
    };
    let mut rt = Runtime::new(config)?;
    let sink = Rc::clone(&out);
    // A failed write (a closed pipe, a full disk) ends the run as a
    // termination by host request (exit code 3): `print` returns it at once;
    // `console.log` cannot fail, so it asks for the termination, which takes
    // effect at the next time check.
    let handle = rt.termination_handle();
    rt.set_console(Box::new(move |line| {
        if writeln!(sink.borrow_mut(), "{line}").is_err() {
            handle.terminate();
        }
    }));
    rt.set_host_data(Host { out });
    rt.define_global_function("print", 1, print)?;
    rt.define_global_object("$262", &[("evalScript", 1, eval_script), ("gc", 0, gc)])?;
    // `$262.global`: the global object. (`createRealm`, `detachArrayBuffer`
    // and `agent` do not exist.)
    rt.eval("$262.global = this;")?;
    Ok(rt)
}

/// `print(...)`: `ToString` of each argument, joined by spaces.
fn print(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let mut parts = Vec::new();
    for i in 0..call.argc() {
        let value = rt.arg(call, i);
        parts.push(rt.to_rust_string(value)?);
    }
    if let Some(host) = rt.host_data_mut::<Host>()
        && writeln!(host.out.borrow_mut(), "{}", parts.join(" ")).is_err()
    {
        return Err(VmError::Terminated(Termination::HostRequest));
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `$262.gc()`: a full collection.
#[allow(clippy::unnecessary_wraps)] // The signature of native functions.
fn gc(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    rt.collect();
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `$262.evalScript(source)`: runs a script in the global scope. A compile
/// error becomes a thrown `SyntaxError`.
fn eval_script(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let source = rt.arg(call, 0);
    let source = rt.to_rust_string(source)?;
    match rt.eval(&source) {
        Ok(value) => Ok(NativeReturn::Value(value)),
        Err(ScriptError::Uncaught { .. }) => Err(VmError::Throw(rt.last_value())),
        Err(ScriptError::Compile { kind, message, .. }) => Err(VmError::Raise {
            kind,
            message: message.into(),
        }),
        Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
    }
}

/// Runs a script under the time limit of the options.
pub(crate) fn run(rt: &mut Runtime, options: &Options, source: &str) -> Result<Value, ScriptError> {
    rt.set_deadline(options.time_limit.map(|limit| Instant::now() + limit));
    rt.eval(source)
}

/// The text of an error as the shell prints it: `Uncaught TypeError:
/// message` (or the compile error, or the termination), and the position
/// `    at FILE:LINE:COLUMN` when the error has one.
pub(crate) fn describe_error(error: &ScriptError, name: &str, source: &str) -> String {
    let offset = match error {
        ScriptError::Compile { offset, .. } => Some(*offset),
        ScriptError::Uncaught { offset, .. } => *offset,
        _ => None,
    };
    let mut text = truncate(error.to_string());
    if let Some(offset) = offset {
        let text16 = String16::from(source);
        let location = LineIndex::new(text16.as_str16()).location(offset);
        let _ = write!(
            text,
            "\n    at {name}:{}:{}",
            location.line, location.column
        );
    }
    text
}

/// The longest error text the shell prints, in characters.
const MAX_MESSAGE_CHARS: usize = 1000;

/// Cuts a long message at [`MAX_MESSAGE_CHARS`] characters and adds `...`.
fn truncate(text: String) -> String {
    match text.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text,
    }
}

/// The exit code of an error: 1 uncaught, 2 compile error, 3 termination
/// (an internal error counts as 1).
pub(crate) fn exit_code(error: &ScriptError) -> u8 {
    match error {
        ScriptError::Compile { .. } => 2,
        ScriptError::Terminated(_) => 3,
        ScriptError::Uncaught { .. } | ScriptError::Internal(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_messages_are_truncated() {
        let text = truncate("é".repeat(5000));
        assert_eq!(text.chars().count(), MAX_MESSAGE_CHARS + 3);
        assert!(text.ends_with("..."));
        assert_eq!(truncate("short".to_owned()), "short");
        let exact = "x".repeat(MAX_MESSAGE_CHARS);
        assert_eq!(truncate(exact.clone()), exact);
    }

    #[test]
    fn thrown_primitives_have_no_empty_name() {
        let error = ScriptError::Uncaught {
            name: String::new(),
            message: "Test262: boom".into(),
            offset: None,
        };
        assert_eq!(describe_error(&error, "t.js", ""), "Uncaught Test262: boom");
    }

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn failed_output_terminates_the_run() {
        let out: Output = Rc::new(RefCell::new(Broken));
        let mut rt = new_runtime(&Options::default(), out).expect("runtime");
        let error = rt.eval("for (;;) print(1);").expect_err("ends");
        assert_eq!(error, ScriptError::Terminated(Termination::HostRequest));
        let error = rt.eval("for (;;) console.log(1);").expect_err("ends");
        assert_eq!(error, ScriptError::Terminated(Termination::HostRequest));
    }
}
