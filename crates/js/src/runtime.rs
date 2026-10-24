//! The embedding API (ADR 0026 section 12): a runtime with its realms,
//! script evaluation, native functions, and the reports of errors.
//!
//! A [`Runtime`] owns the heap, the VM state (value stack, frames,
//! realms) and the recursion budget. It is not `Send`. Every thread that
//! runs it needs a stack of at least 8 MiB (ADR 0026 section 9).
//!
//! Values that the API returns are handles: a returned result stays alive
//! until the next call that runs script code; a native function's values
//! stay alive until it returns (they are recorded in its handle scope).
//!
//! Limits (ADR 0026 section 9): the host sets a deadline per task
//! ([`Runtime::set_deadline`]) and can ask from any thread to end the
//! running script ([`Runtime::termination_handle`]); both end the script
//! with [`ScriptError::Terminated`]. After a termination the runtime is
//! usable: the frames, the value stack, the handle scopes and the no-GC
//! regions of the ended run are gone, a generator that was running is
//! closed, and suspended generators can resume in a later run.
//!
//! Output: `console.log` writes its lines to the console sink of the
//! host ([`Runtime::set_console`]); without one, the lines are dropped.
//!
//! Jobs (ADR 0026 section 12): promise jobs wait in the runtime's queue
//! until the host runs them with [`Runtime::run_jobs`] (the HTML microtask
//! checkpoint; the shell after each script). After a drain, the host takes
//! the promises that were rejected without a handler
//! ([`Runtime::take_unhandled_rejections`]). The end of a drain is also
//! where `ClearKeptObjects` runs (§9.11; HTML performs it at the end of a
//! microtask checkpoint). The cleanup jobs of finalization registries
//! (`HostEnqueueFinalizationRegistryCleanupJob`, §9.5.6) wait in the heap
//! until the host runs each as a task of its own
//! ([`Runtime::run_finalization_cleanup`]).

use std::borrow::Cow;
use std::fmt;
use std::rc::Rc;

use swb_js_syntax::ParseError;
use swb_js_text::{RecursionBudget, Str16, String16};

use crate::bytecode::FunctionCode;
use crate::compiler::{CompileError, CompileStats, compile_script};
use crate::error::{Error, InternalError, Termination, ThrowKind};
use crate::heap::{Gc, GcStats, Generic, Heap, HeapConfig};
use crate::object::{GetResult, Object};
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{
    Atoms, DEFAULT_FRAME_LIMIT, DEFAULT_STACK_LIMIT, Frame, NativeFn, REENTRY_WEIGHT, ReturnTo,
    STACK_OVERFLOW, Symbols, TIME_CHECK_INTERVAL, TerminationHandle, Vm, VmError, VmResult,
};

/// The largest value stack: frame records keep stack positions in 32 bits.
pub const MAX_STACK_LIMIT: usize = 1 << 30;

/// The limits and options of a runtime.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeConfig {
    /// The heap limits and the collection policy (and the stress mode).
    pub heap: HeapConfig,
    /// The most frame records (script call depth).
    pub frame_limit: usize,
    /// The most values in the value stack (clamped to `MAX_STACK_LIMIT`,
    /// so that register positions fit the 32-bit fields of a frame).
    pub stack_limit: usize,
    /// The shared recursion budget (ADR 0026 section 9).
    pub recursion_budget: RecursionBudget,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        RuntimeConfig {
            heap: HeapConfig::default(),
            frame_limit: DEFAULT_FRAME_LIMIT,
            stack_limit: DEFAULT_STACK_LIMIT,
            recursion_budget: RecursionBudget::DEFAULT,
        }
    }
}

/// Why a script did not complete normally.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptError {
    /// The source did not compile (an early error, a resource limit of
    /// the front end or the compiler, or a construct outside the subset).
    Compile {
        /// `SyntaxError` or `RangeError`.
        kind: ThrowKind,
        /// The message.
        message: String,
        /// The code-unit offset of the error.
        offset: u32,
    },
    /// An exception that no handler caught. The thrown value stays alive
    /// until the next run ([`Runtime::last_value`]).
    Uncaught {
        /// The `name` of an error object (empty for other values).
        name: String,
        /// The `message` of an error object, or the text of another value.
        message: String,
        /// The source offset (code units) of the instruction that threw,
        /// in the innermost script frame.
        offset: Option<u32>,
    },
    /// The script was ended: the heap limit, the time limit or a request
    /// of the host. No handler and no `finally` block ran.
    Terminated(Termination),
    /// An engine bug ended the script (logged).
    Internal(InternalError),
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptError::Compile { kind, message, .. } => write!(f, "{}: {message}", kind.name()),
            ScriptError::Uncaught { name, message, .. } if name.is_empty() => {
                write!(f, "Uncaught {message}")
            }
            ScriptError::Uncaught { name, message, .. } => {
                write!(f, "Uncaught {name}: {message}")
            }
            ScriptError::Terminated(termination) => write!(f, "terminated: {termination}"),
            ScriptError::Internal(error) => write!(f, "internal error: {error}"),
        }
    }
}

impl std::error::Error for ScriptError {}

impl From<ParseError> for ScriptError {
    fn from(error: ParseError) -> Self {
        ScriptError::Compile {
            kind: error.kind.into(),
            message: error.message.into_owned(),
            offset: error.offset,
        }
    }
}

/// The receiver of console lines: the level of the method (`log`,
/// `info`, ...) and the line.
pub type ConsoleSink = Box<dyn FnMut(ConsoleLevel, &str)>;

/// The method of `console` that wrote a line (WHATWG Console Standard,
/// the log levels of the "Logger" calls).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleLevel {
    /// `console.log`.
    Log,
    /// `console.info`.
    Info,
    /// `console.warn`.
    Warn,
    /// `console.error`.
    Error,
    /// `console.debug`.
    Debug,
}

impl ConsoleLevel {
    /// The name of the method.
    pub fn name(self) -> &'static str {
        match self {
            ConsoleLevel::Log => "log",
            ConsoleLevel::Info => "info",
            ConsoleLevel::Warn => "warn",
            ConsoleLevel::Error => "error",
            ConsoleLevel::Debug => "debug",
        }
    }
}

/// A JavaScript runtime: one heap, one value stack, realms.
pub struct Runtime {
    pub(crate) heap: Heap,
    pub(crate) vm: Vm,
    pub(crate) budget: RecursionBudget,
    console: Option<ConsoleSink>,
}

/// The lengths of the VM's stacks and regions before a host run, to
/// restore after a run that ended abruptly.
struct RunStart {
    frames: usize,
    stack: usize,
    scopes: usize,
    no_gc: u32,
    joins: usize,
}

impl Runtime {
    /// A runtime with one realm (index 0).
    pub fn new(config: RuntimeConfig) -> Result<Runtime, ScriptError> {
        let mut heap = Heap::new(config.heap);
        let atoms = make_atoms(&mut heap).map_err(|e| finish_error(VmError::from(e)))?;
        let symbols = Symbols::new(&mut heap).map_err(|e| finish_error(VmError::from(e)))?;
        let vm = Vm {
            stack: Vec::new(),
            frames: Vec::new(),
            loops: Vec::new(),
            realms: Vec::new(),
            atoms,
            symbols,
            frame_limit: config.frame_limit,
            stack_limit: config.stack_limit.min(MAX_STACK_LIMIT),
            last_value: Value::Undefined,
            error_offset: None,
            countdown: std::cell::Cell::new(TIME_CHECK_INTERVAL),
            deadline: None,
            termination: TerminationHandle::default(),
            join_stack: Vec::new(),
            random: crate::vm::random::RandomSource::new(),
            time: crate::date::TimeHost::default(),
            split_cache: None,
            regexp_matcher: None,
            array_pattern: None,
            jobs: crate::vm::jobs::JobQueue::default(),
            host: crate::host::Host::default(),
            terminating: None,
        };
        let mut runtime = Runtime {
            heap,
            vm,
            budget: config.recursion_budget,
            console: None,
        };
        runtime.create_realm().map_err(finish_error)?;
        Ok(runtime)
    }

    /// The heap (statistics, the stress mode, direct access in tests).
    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// Runs a full collection with the VM's roots.
    pub fn collect(&mut self) -> GcStats {
        self.heap.collect(&self.vm)
    }

    /// The last result or uncaught exception value of a run.
    pub fn last_value(&self) -> Value {
        self.vm.last_value
    }

    /// The remaining recursion budget (in approximate stack bytes).
    pub fn recursion_budget(&self) -> RecursionBudget {
        self.budget
    }

    /// Sets the heap limit of the runtime in bytes (ADR 0028 section 16:
    /// the JavaScript heap of a document). The host can change it between
    /// tasks; a heap above the new limit ends the next script that reaches
    /// a safepoint with [`Termination::HeapLimit`].
    pub fn set_heap_limit(&mut self, limit: usize) {
        self.heap.set_limit(limit);
    }

    /// Sets the receiver of the lines of `console.log`, `info`, `warn`,
    /// `error` and `debug` (one call per line, without the line end).
    pub fn set_console(&mut self, sink: ConsoleSink) {
        self.console = Some(sink);
    }

    /// Sets the local time zone of `Date`. Without a call, the zone is the
    /// system zone (ADR 0027).
    pub fn set_time_zone(&mut self, zone: crate::date::TimeZone) {
        self.vm.time.set_zone(zone);
    }

    /// Sets the clock of `Date.now()` and `new Date()`: milliseconds since
    /// the epoch. Without a call, the clock is the system clock.
    pub fn set_clock(&mut self, clock: crate::date::Clock) {
        self.vm.time.set_clock(clock);
    }

    /// Writes a line to the console sink.
    pub(crate) fn console_line(&mut self, level: ConsoleLevel, line: &str) {
        if let Some(sink) = self.console.as_mut() {
            sink(level, line);
        }
    }

    /// Sets the deadline of the following runs: a script that is still
    /// running at the deadline ends with [`Termination::TimeLimit`]
    /// (checked every few thousand steps, see `TIME_CHECK_INTERVAL`).
    /// `None` removes it.
    pub fn set_deadline(&mut self, deadline: Option<std::time::Instant>) {
        self.vm.deadline = deadline;
    }

    /// The deadline of the runs.
    pub fn deadline(&self) -> Option<std::time::Instant> {
        self.vm.deadline
    }

    /// A handle that another thread can use to end the running script
    /// ([`Termination::HostRequest`]).
    pub fn termination_handle(&self) -> TerminationHandle {
        self.vm.termination.clone()
    }

    /// The number of frame records and of values in the value stack (for
    /// diagnostics and tests: both are 0 between host runs).
    pub fn stack_depths(&self) -> (usize, usize) {
        (self.vm.frames.len(), self.vm.stack.len())
    }

    /// The lengths of the VM's stacks before a host run. The outermost
    /// run (no frames) starts a new task: a termination that a native
    /// requested in the last one is over.
    fn run_start(&mut self) -> RunStart {
        if self.vm.frames.is_empty() {
            self.vm.terminating = None;
        }
        RunStart {
            frames: self.vm.frames.len(),
            stack: self.vm.stack.len(),
            scopes: self.heap.scope_depth(),
            no_gc: self.heap.no_gc_depth(),
            joins: self.vm.join_stack.len(),
        }
    }

    /// Restores the state before a host run after an abrupt end. The
    /// interpreter already removed its frames; this is the safety net for
    /// paths that left early (generators of removed frames complete).
    fn restore_run_start(&mut self, start: &RunStart) {
        self.unwind(start.frames);
        self.vm.stack.truncate(start.stack);
        self.heap.close_scopes_to(start.scopes);
        self.heap.restore_no_gc_depth(start.no_gc);
        self.vm.join_stack.truncate(start.joins);
    }

    /// Compiles and runs a classic script in realm 0. Returns its
    /// completion value (§16.1.6, with the `UpdateEmpty` rules of the
    /// statements).
    ///
    /// Like [`Runtime::call`], a nested use (from a native function)
    /// charges the shared recursion budget; too deep a nesting is a
    /// `RangeError`.
    pub fn eval(&mut self, source: &str) -> Result<Value, ScriptError> {
        self.eval_named(source, "")
    }

    /// Like [`Runtime::eval`], with the name of the script (a file name
    /// or URL) that stack traces show. An empty name shows as
    /// `<anonymous>`.
    pub fn eval_named(&mut self, source: &str, name: &str) -> Result<Value, ScriptError> {
        if self.budget.enter(REENTRY_WEIGHT).is_err() {
            return self.finish(Err(VmError::range_error(STACK_OVERFLOW)));
        }
        let text = String16::from(source);
        let result = self.eval_text(text.as_str16(), name);
        self.budget.leave(REENTRY_WEIGHT);
        result
    }

    fn eval_text(&mut self, source: Str16<'_>, name: &str) -> Result<Value, ScriptError> {
        self.vm.error_offset = None;
        let start = self.run_start();
        let (code, compiled) = self.compile(source, name)?;
        let result = self.run_script(code, compiled, 0);
        if result.is_err() {
            self.restore_run_start(&start);
        }
        self.finish(result)
    }

    /// Parses and compiles a script; the code object is not rooted, so the
    /// caller runs it before anything can collect.
    fn compile(
        &mut self,
        source: Str16<'_>,
        name: &str,
    ) -> Result<(Gc<Generic>, Rc<FunctionCode>), ScriptError> {
        let script = swb_js_syntax::parse_script(source, &mut self.budget)?;
        // The compile data counts against the heap limit while it exists
        // (ADR 0026 section 3).
        let size = script.ast.heap_size() + script.scopes.heap_size();
        self.heap
            .reserve(size, &self.vm)
            .map_err(|e| finish_error(e.into()))?;
        compile_script(&mut self.heap, &script, source, name, &mut self.budget)
            .map_err(compile_error)
    }

    /// Compiles a script and returns the statistics of its code (for
    /// measurements); nothing runs.
    pub fn compile_stats(&mut self, source: &str) -> Result<CompileStats, ScriptError> {
        let text = String16::from(source);
        let (_, compiled) = self.compile(text.as_str16(), "")?;
        Ok(CompileStats::of(&compiled))
    }

    /// Compiles a script and returns a listing of the bytecode of all its
    /// functions (for tests and debugging); nothing runs.
    pub fn disassemble(&mut self, source: &str) -> Result<String, ScriptError> {
        let text = String16::from(source);
        let (_, compiled) = self.compile(text.as_str16(), "")?;
        Ok(crate::bytecode::disassemble(&compiled, &self.heap))
    }

    /// Runs the code of a script in a realm: a frame whose result goes to
    /// the host.
    fn run_script(
        &mut self,
        code: Gc<Generic>,
        compiled: Rc<FunctionCode>,
        realm: u32,
    ) -> VmResult<Value> {
        let global = self.global_object(realm)?;
        let restore_top = self.vm.stack.len();
        let registers = compiled.register_count as usize;
        if self.vm.frames.len() >= self.vm.frame_limit
            || restore_top + 2 + registers > self.vm.stack_limit
        {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let window = self.push_window(Value::Undefined, global.into(), &[])?;
        self.vm.stack.resize(window + registers, Value::Undefined);
        self.vm.frames.push(Frame {
            function: None,
            code,
            compiled,
            pc: 0,
            base: window as u32,
            restore_top: restore_top as u32,
            this: global.into(),
            new_target: Value::Undefined,
            argc: 0,
            extra_args: Vec::new(),
            ret: ReturnTo::Host,
            construct: false,
            generator: None,
            realm,
        });
        self.run_frames()
    }

    /// Calls a function from the host (a re-entry, see [`Runtime::call`]).
    pub fn call_function(
        &mut self,
        callee: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, ScriptError> {
        self.vm.error_offset = None;
        let start = self.run_start();
        let scope = self.heap.open_scope();
        let result = self.call(callee, this, args);
        if result.is_err() {
            self.restore_run_start(&start);
        } else if let Err(error) = self.heap.close_scope(scope) {
            return Err(finish_error(error.into()));
        }
        self.finish(result)
    }

    /// Turns the result of a run into the API's result and keeps the value
    /// alive until the next run.
    fn finish(&mut self, result: VmResult<Value>) -> Result<Value, ScriptError> {
        let mut dropped = None;
        if self.vm.frames.is_empty() {
            dropped = self.vm.terminating.take();
        }
        match result {
            Ok(value) => {
                self.vm.last_value = value;
                // A native requested a termination and script code dropped
                // the error: the task still ends as terminated.
                match dropped {
                    Some(reason) => Err(ScriptError::Terminated(reason)),
                    None => Ok(value),
                }
            }
            Err(VmError::Throw(value)) => {
                self.vm.last_value = value;
                let (name, message) = self.describe_exception(value);
                Err(ScriptError::Uncaught {
                    name,
                    message,
                    offset: self.vm.error_offset,
                })
            }
            Err(VmError::Raise { kind, message }) => {
                // The host can inspect the error object as the last value.
                self.vm.last_value = match self.error_object(kind, &message) {
                    Ok(object) => object.into(),
                    Err(error) => return Err(finish_error(error)),
                };
                Err(ScriptError::Uncaught {
                    name: kind.name().to_owned(),
                    message: message.into_owned(),
                    offset: self.vm.error_offset,
                })
            }
            Err(error) => {
                self.vm.last_value = Value::Undefined;
                Err(finish_error(error))
            }
        }
    }

    /// The `name` and `message` of a thrown value, read without running
    /// script code (getters are not called).
    fn describe_exception(&self, value: Value) -> (String, String) {
        let Value::Object(object) = value else {
            return (String::new(), self.value_text(value));
        };
        let read = |key| match self.heap.get(object, PropertyKey::String(key), value) {
            Ok(GetResult::Value(Value::String(text))) => Some(self.name_text(text)),
            _ => None,
        };
        match read(self.vm.atoms.message) {
            Some(message) => {
                let name = read(self.vm.atoms.name).unwrap_or_else(|| "Error".to_owned());
                (name, message)
            }
            None => (String::new(), self.describe_value(value)),
        }
    }

    /// Installs a native function as a property of the global object of
    /// realm 0.
    pub fn define_global_function(
        &mut self,
        name: &str,
        length: u32,
        func: NativeFn,
    ) -> Result<(), ScriptError> {
        self.scoped(|rt| {
            let function = rt.new_native_function(0, name, length, func)?;
            let global = rt.global_object(0)?;
            rt.define_native(global, name, function.into(), false)
        })
        .map_err(finish_error)
    }

    /// Creates a plain object with native methods and installs it as a
    /// property of the global object of realm 0 (for host objects such as
    /// the `$262` of test262).
    pub fn define_global_object(
        &mut self,
        name: &str,
        functions: &[(&str, u32, NativeFn)],
    ) -> Result<(), ScriptError> {
        self.scoped(|rt| {
            let proto = rt.intrinsic(0, crate::vm::Intrinsic::ObjectPrototype)?;
            let object = rt.heap.new_object(Some(proto))?;
            rt.heap.record(object);
            for &(fname, length, func) in functions {
                let function = rt.new_native_function(0, fname, length, func)?;
                rt.define_native(object, fname, function.into(), true)?;
            }
            let global = rt.global_object(0)?;
            rt.define_native(global, name, object.into(), false)
        })
        .map_err(finish_error)
    }

    /// Defines a data property of a host object: writable and
    /// configurable, and enumerable if `enumerable`.
    fn define_native(
        &mut self,
        target: Gc<Object>,
        name: &str,
        value: Value,
        enumerable: bool,
    ) -> VmResult<()> {
        let key = self.heap.key_from_str(name)?;
        self.define(target, key, value, true, enumerable, true)
    }

    /// The `name` of the `constructor` of a value (both read as data
    /// properties along the prototype chain; no script code runs), or an
    /// empty string. test262 compares the constructors of thrown values.
    pub fn constructor_name(&self, value: Value) -> String {
        match value {
            Value::Object(object) => self.chain_constructor_name(object).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Records the thrown value of `error` in the open handle scope, so it
    /// survives a collection (no-op for other errors).
    ///
    /// A thrown value is not rooted while it propagates as a Rust `Err`.
    /// Every Rust site that holds a thrown value across a safepoint (a
    /// call into script code, an allocation that can collect) must call
    /// this first: `IteratorClose` after an error, and later for-of,
    /// destructuring, the Map and Set constructors and promise jobs.
    pub(crate) fn hold_error(&mut self, error: &VmError) {
        if let VmError::Throw(value) = error {
            self.heap.record(*value);
        }
    }

    /// `ToString` of a value as Rust text (lossy for unpaired surrogates).
    /// Can run script code (`toString`, `valueOf`).
    pub fn to_rust_string(&mut self, value: Value) -> VmResult<String> {
        self.scoped(|rt| {
            let string = rt.to_string(value)?;
            Ok(rt.name_text(string))
        })
    }

    /// The text of a value without running script code (for tests and
    /// diagnostics): primitives as `ToString` gives them, objects as
    /// `#<Object>`.
    pub fn display(&self, value: Value) -> String {
        self.value_text(value)
    }

    /// The text of a thrown value or a rejection reason without running
    /// script code: `Name: message` for an object with a `message` data
    /// property (the `name` found as a data property, else `Error`), the
    /// text of other values as [`Runtime::display`] gives it, then a line
    /// `    at ...` for each frame that an error object captured (the
    /// lines of its `stack`).
    pub fn error_report(&self, value: Value) -> String {
        let (name, message) = self.describe_exception(value);
        let mut text = if name.is_empty() {
            message
        } else if message.is_empty() {
            name
        } else {
            format!("{name}: {message}")
        };
        if let Value::Object(object) = value
            && let Ok(crate::object::ObjectKind::Error(Some(data))) =
                self.heap.object(object).map(|o| &o.kind)
        {
            // After the first read of `stack`, the frames are gone and
            // its text (header included) is what Chromium's console shows.
            if let Value::String(stack) = data.stack {
                return self.heap.string_lossy(stack, usize::MAX);
            }
            for frame in &data.frames {
                text.push_str("\n    at ");
                text.push_str(&self.frame_line(frame));
            }
        }
        text
    }

    // --- Jobs (ADR 0026 section 12) ---

    /// Runs the jobs of the queue in order until it is empty (the
    /// microtask checkpoint of HTML calls it; the `swb-js` shell after each
    /// script). Jobs that the running jobs enqueue run too. Does nothing
    /// while script code is running (a native function that calls it).
    ///
    /// Each job is a step of the time countdown and runs under the
    /// deadline. An error ends the run: an exception that no promise took
    /// (a capability function of a subclass that throws) is
    /// [`ScriptError::Uncaught`], and the remaining jobs stay in the queue
    /// for the next call; a termination ends it as
    /// [`ScriptError::Terminated`] (never a rejection): the job that was
    /// running is gone, the others stay in the queue until the host runs
    /// them or drops them ([`Runtime::clear_jobs`]).
    pub fn run_jobs(&mut self) -> Result<(), ScriptError> {
        if !self.vm.frames.is_empty() {
            return Ok(());
        }
        while self.vm.jobs.len() > 0 {
            if let Err(error) = self.tick() {
                return self.finish(Err(error)).map(|_| ());
            }
            self.vm.error_offset = None;
            let start = self.run_start();
            let scope = self.heap.open_scope();
            let Some((job, realm)) = self.next_job() else {
                break;
            };
            match self.run_job(job, realm) {
                Ok(()) if self.vm.terminating.is_some() => {
                    // The job dropped a native's termination.
                    self.restore_run_start(&start);
                    return self.finish(Ok(Value::Undefined)).map(|_| ());
                }
                Ok(()) => self
                    .heap
                    .close_scope(scope)
                    .map_err(|e| finish_error(e.into()))?,
                Err(error) => {
                    self.restore_run_start(&start);
                    return self.finish(Err(error)).map(|_| ());
                }
            }
            // A safepoint between two jobs: a loop of jobs that run no
            // script code still collects.
            self.heap
                .safepoint(&self.vm)
                .map_err(|e| finish_error(e.into()))?;
        }
        // The synchronous run of script ends here: `ClearKeptObjects`
        // (§9.11), as HTML performs it at the end of a microtask checkpoint.
        self.heap.clear_kept_objects();
        Ok(())
    }

    /// `ClearKeptObjects` (§9.11): the targets of the `WeakRef`s made or
    /// read since the last call can die at the next collection.
    /// [`Runtime::run_jobs`] calls it when the queue is empty; a host that
    /// ends a synchronous run of script in another way calls it then, also
    /// after a script or a job that threw.
    pub fn clear_kept_objects(&mut self) {
        self.heap.clear_kept_objects();
    }

    /// The number of finalization registries whose cleanup job waits
    /// (`HostEnqueueFinalizationRegistryCleanupJob`, §9.5.6). The host
    /// runs each with [`Runtime::run_finalization_cleanup`] as a task (HTML
    /// queues a global task on the JavaScript engine task source), and then
    /// runs the promise jobs.
    pub fn pending_finalization_cleanups(&self) -> usize {
        self.heap.pending_cleanups()
    }

    /// Runs the next cleanup job: `CleanupFinalizationRegistry` (§9.13)
    /// calls the registry's callback with the held value of each cell whose
    /// target died, in the order the collector found them. Returns whether
    /// a job ran. An exception in a callback ends the job and is returned
    /// as [`ScriptError::Uncaught`] (the host reports it); the cells that
    /// still wait get a new job. Does nothing while script code is running.
    pub fn run_finalization_cleanup(&mut self) -> Result<bool, ScriptError> {
        if !self.vm.frames.is_empty() {
            return Ok(false);
        }
        let Some(registry) = self.heap.take_cleanup() else {
            return Ok(false);
        };
        self.vm.error_offset = None;
        let start = self.run_start();
        let scope = self.heap.open_scope();
        self.heap.record(registry);
        match self.cleanup_registry(registry) {
            Ok(()) if self.vm.terminating.is_some() => {
                self.restore_run_start(&start);
                self.finish(Ok(Value::Undefined)).map(|_| true)
            }
            Ok(()) => {
                self.heap
                    .close_scope(scope)
                    .map_err(|e| finish_error(e.into()))?;
                Ok(true)
            }
            Err(error) => {
                if let Err(internal) = self.heap.requeue_cleanup(registry) {
                    log::warn!("a finalization registry was not queued again: {internal}");
                }
                self.restore_run_start(&start);
                self.finish(Err(error)).map(|_| true)
            }
        }
    }

    /// `CleanupFinalizationRegistry` (§9.13): one call per pending cell,
    /// each in a handle scope of its own and a step of the time countdown.
    fn cleanup_registry(&mut self, registry: Gc<Object>) -> VmResult<()> {
        let callback = self
            .heap
            .registry(registry)?
            .map_or(Value::Undefined, |registry| registry.callback);
        loop {
            self.tick()?;
            let more = self.with_scope(|rt| {
                let Some(held) = rt.heap.registry_take_pending(registry)? else {
                    return Ok(false);
                };
                rt.heap.record(held);
                rt.call(callback, Value::Undefined, &[held])?;
                Ok(true)
            })?;
            if !more {
                return Ok(());
            }
        }
    }

    /// The number of jobs in the queue.
    pub fn pending_jobs(&self) -> usize {
        self.vm.jobs.len()
    }

    /// Drops the jobs of the queue (for a host that stops running the
    /// scripts of a document, for example after a termination).
    pub fn clear_jobs(&mut self) {
        self.drop_jobs();
    }

    /// The promises that were rejected while no handler was attached and
    /// still have none (`HostPromiseRejectionTracker`, §27.2.1.9), in the
    /// order of their rejection; the tracker's list starts again empty.
    /// Call it after [`Runtime::run_jobs`]. The promises stay alive until
    /// the next call.
    pub fn take_unhandled_rejections(&mut self) -> Vec<Value> {
        self.take_rejections()
            .into_iter()
            .map(Value::Object)
            .collect()
    }

    /// The state and the result of a promise (`undefined` while pending);
    /// `None` for a value that is not a promise.
    pub fn promise_settlement(&self, value: Value) -> Option<(crate::vm::PromiseStatus, Value)> {
        let Value::Object(object) = value else {
            return None;
        };
        match &self.heap.object(object).ok()?.kind {
            crate::object::ObjectKind::Promise(state) => Some((state.status, state.result)),
            _ => None,
        }
    }

    /// Attaches a host reaction to a promise (the "react to a
    /// promise" of Web IDL): when it settles, a job calls `func` with `data`, the
    /// kind of the settlement and the value. `data` stays alive with the
    /// promise. Returns false (and attaches nothing) for a value that is
    /// not a promise. An exception from `func` reaches the host from
    /// [`Runtime::run_jobs`].
    pub fn react_to_promise(
        &mut self,
        promise: Value,
        func: crate::vm::ContinuationFn,
        data: Value,
    ) -> Result<bool, ScriptError> {
        let promise = match self.as_promise(promise) {
            Ok(Some(promise)) => promise,
            Ok(None) => return Ok(false),
            Err(error) => return Err(finish_error(error)),
        };
        self.then_internal(promise, func, data)
            .map_err(finish_error)?;
        Ok(true)
    }
}

/// Creates the atoms of the VM.
fn make_atoms(heap: &mut Heap) -> Result<Atoms, Error> {
    let mut atom = |text: &str| heap.intern_str(text);
    Ok(Atoms {
        prototype: atom("prototype")?,
        constructor: atom("constructor")?,
        name: atom("name")?,
        message: atom("message")?,
        stack: atom("stack")?,
        stack_trace_limit: atom("stackTraceLimit")?,
        value: atom("value")?,
        done: atom("done")?,
        callee: atom("callee")?,
        value_of: atom("valueOf")?,
        to_string: atom("toString")?,
        empty: atom("")?,
        undefined: atom("undefined")?,
        null: atom("null")?,
        object: atom("object")?,
        boolean: atom("boolean")?,
        number: atom("number")?,
        string: atom("string")?,
        symbol: atom("symbol")?,
        bigint: atom("bigint")?,
        function: atom("function")?,
        true_: atom("true")?,
        false_: atom("false")?,
        default: atom("default")?,
        next: atom("next")?,
        return_: atom("return")?,
        throw_: atom("throw")?,
        raw: atom("raw")?,
        then: atom("then")?,
        last_index: atom("lastIndex")?,
        index: atom("index")?,
        input: atom("input")?,
        groups: atom("groups")?,
        flags: atom("flags")?,
        source: atom("source")?,
        exec: atom("exec")?,
        traps: {
            let mut traps = Vec::with_capacity(crate::vm::proxy::TRAP_COUNT);
            for trap in crate::vm::proxy::Trap::ALL {
                traps.push(atom(trap.name())?);
            }
            traps
                .try_into()
                .map_err(|_| Error::invariant("the trap names are not complete"))?
        },
    })
}

/// The API error of an abrupt completion other than a thrown value.
pub(crate) fn finish_error(error: VmError) -> ScriptError {
    match error {
        VmError::Throw(_) => ScriptError::Uncaught {
            name: String::new(),
            message: "(exception)".to_owned(),
            offset: None,
        },
        VmError::Raise { kind, message } => ScriptError::Uncaught {
            name: kind.name().to_owned(),
            message: message.into_owned(),
            offset: None,
        },
        VmError::Terminated(termination) => ScriptError::Terminated(termination),
        VmError::Internal(internal) => {
            log::warn!("JavaScript engine internal error: {internal}");
            ScriptError::Internal(internal)
        }
    }
}

/// The API error of a failed compile.
pub(crate) fn compile_error(error: CompileError) -> ScriptError {
    match error {
        CompileError::Script {
            kind,
            message,
            offset,
        } => ScriptError::Compile {
            kind,
            message: match message {
                Cow::Borrowed(text) => text.to_owned(),
                Cow::Owned(text) => text,
            },
            offset,
        },
        CompileError::Heap(error) => finish_error(error.into()),
    }
}
