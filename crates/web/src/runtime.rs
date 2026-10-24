//! The script runtime of a document (ADR 0028 section 2): a `js::Runtime`
//! with the bindings, whose global object is the `Window`, and the
//! console buffer.
//!
//! The engine creates one when it commits a document with JavaScript on
//! and drops it with the document. For each task it lends the document
//! state ([`ScriptRuntime::lend`]), runs scripts, and takes the state back
//! ([`ScriptRuntime::take_back`]). A task has a time limit
//! ([`TASK_TIME_LIMIT`]) and ends with a microtask checkpoint. After three
//! tasks that reach the time limit, or one that reaches the heap limit,
//! the document runs no more scripts (ADR 0028 sections 7 and 16).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use swb_js::{ConsoleLevel, PromiseStatus, Runtime, RuntimeConfig, ScriptError, Value, VmError};

use crate::generated as g;
use crate::host::EngineHost;
use crate::state::{WebState, state_mut};

/// The time limit of a task (ADR 0028 section 16).
pub const TASK_TIME_LIMIT: Duration = Duration::from_secs(5);

/// The heap limit of a document's runtime (ADR 0028 section 16).
pub const HEAP_LIMIT: usize = 256 << 20;

/// The number of tasks that may reach the time limit before the document
/// runs no more scripts (ADR 0028 section 7).
const MAX_TIMEOUTS: u32 = 3;

/// The most console messages that the buffer keeps (the oldest go first).
const MAX_CONSOLE_MESSAGES: usize = 1000;

/// The longest text of a console message in the buffer, in bytes (longer
/// texts are cut at a character boundary).
const MAX_CONSOLE_TEXT: usize = 16 << 10;

/// A line that a script wrote to the console, or an error report of the
/// runtime (uncaught exceptions are `Error` lines).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleMessage {
    /// The method that wrote it.
    pub level: ConsoleLevel,
    /// The text.
    pub text: String,
}

/// The result of [`ScriptRuntime::evaluate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evaluation {
    /// The JSON text of the value.
    Value(String),
    /// The error: `Name: message` of a thrown error or a rejected promise.
    Error(String),
}

/// How [`ScriptRuntime::evaluate`] writes numbers that JSON lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonForm {
    /// `NaN`, `Infinity`, `-Infinity` and `-0.0` as these tokens (the
    /// `--eval` contract of docs/testing.md).
    Probe,
    /// Valid JSON: those numbers become `null` and `0`, as in
    /// `JSON.stringify`, and lone surrogates in strings become U+FFFD.
    Strict,
}

/// The function that writes the JSON text of an evaluated value, or the
/// text of an error. It is compiled when the runtime is created, before any
/// page script, and keeps the built-ins it uses, so that a page that
/// replaces `JSON.stringify` or `Array.isArray` does not change it.
const SERIALIZER: &str = r"(() => {
  const isArray = Array.isArray, keys = Object.keys, stringify = JSON.stringify, S = String;
  const apply = Reflect.apply, wellFormed = String.prototype.toWellFormed;
  // Strict JSON parsers refuse the escape of a lone surrogate.
  function string(s, strict) { return stringify(strict ? apply(wellFormed, s, []) : s); }
  function number(n, strict) {
    if (n !== n) return strict ? 'null' : 'NaN';
    if (n === Infinity) return strict ? 'null' : 'Infinity';
    if (n === -Infinity) return strict ? 'null' : '-Infinity';
    if (n === 0 && 1 / n < 0) return strict ? '0' : '-0.0';
    return S(n);
  }
  function value(v, strict, seen) {
    switch (typeof v) {
      case 'boolean': return v ? 'true' : 'false';
      case 'number': return number(v, strict);
      case 'bigint': return S(v);
      case 'string': return string(v, strict);
      case 'object': break;
      default: return 'null';
    }
    if (v === null || seen.length > 100) return 'null';
    for (let i = 0; i < seen.length; i++) if (seen[i] === v) return 'null';
    seen[seen.length] = v;
    let out = '';
    if (isArray(v)) {
      for (let i = 0; i < v.length; i++) out += (i ? ',' : '') + value(v[i], strict, seen);
      out = '[' + out + ']';
    } else {
      const ks = keys(v);
      for (let i = 0; i < ks.length; i++) out += (i ? ',' : '') + string(ks[i], strict) + ':' + value(v[ks[i]], strict, seen);
      out = '{' + out + '}';
    }
    seen.length = seen.length - 1;
    return out;
  }
  function error(e) {
    try {
      if (e !== null && typeof e === 'object' && 'message' in e) return S(e.name) + ': ' + S(e.message);
      return S(e);
    } catch (x) {
      return 'a thrown value that has no text';
    }
  }
  return (v, strict, isError) => isError ? error(v) : value(v, strict, []);
})()";

/// The script runtime of one document.
pub struct ScriptRuntime {
    rt: Runtime,
    console: Rc<RefCell<VecDeque<ConsoleMessage>>>,
    /// The time limit of a task.
    time_limit: Duration,
    /// The tasks that reached the time limit.
    timeouts: u32,
    /// True when the document runs no more scripts.
    stopped: bool,
}

/// The termination of a task that does not start because the document's
/// scripts were stopped.
const STOPPED: ScriptError = ScriptError::Terminated(swb_js::Termination::Native(
    "the scripts of the document were stopped",
));

impl ScriptRuntime {
    /// A runtime with the bindings and the limits of ADR 0028 section 16.
    pub fn new() -> Result<ScriptRuntime, ScriptError> {
        let mut config = RuntimeConfig::default();
        config.heap.limit = HEAP_LIMIT;
        ScriptRuntime::with_config(config)
    }

    /// A runtime with the bindings and another configuration (tests use
    /// the GC stress mode).
    pub fn with_config(config: RuntimeConfig) -> Result<ScriptRuntime, ScriptError> {
        let mut rt = Runtime::new(config)?;
        let setup = |rt: &mut Runtime| -> Result<(), VmError> {
            g::define_classes::<WebState>(rt)?;
            rt.set_host_data(WebState::default());
            let window_properties = crate::window::define_class(rt)?;
            g::create_interfaces::<WebState>(rt, 0)?;
            crate::window::install(rt, 0, window_properties)?;
            g::init_global::<WebState>(rt, 0, crate::state::NOT_A_NODE)
        };
        setup(&mut rt).map_err(|e| internal(&e))?;
        let serializer = rt.eval_named(SERIALIZER, "swb:serializer")?;
        state_mut(&mut rt)
            .map_err(|e| internal(&e))?
            .pinned
            .push(serializer);
        let console = Rc::new(RefCell::new(VecDeque::new()));
        let sink = Rc::clone(&console);
        rt.set_console(Box::new(move |level, line| {
            log_console(level, line);
            push_message(&sink, level, line);
        }));
        Ok(ScriptRuntime {
            rt,
            console,
            time_limit: TASK_TIME_LIMIT,
            timeouts: 0,
            stopped: false,
        })
    }

    /// Sets the time limit of a task (tests use a short one).
    pub fn set_time_limit(&mut self, limit: Duration) {
        self.time_limit = limit;
    }

    /// True if the document runs no more scripts: three tasks reached the
    /// time limit, or one reached the heap limit.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// Lends the document state to the runtime for a task.
    pub fn lend(&mut self, host: Box<dyn EngineHost>) {
        if let Ok(state) = state_mut(&mut self.rt) {
            state.host = Some(host);
        }
    }

    /// Takes the lent document state back.
    pub fn take_back(&mut self) -> Option<Box<dyn EngineHost>> {
        state_mut(&mut self.rt).ok()?.host.take()
    }

    /// Runs a classic script as a task: the script, then a microtask
    /// checkpoint, under the time limit. Errors (also uncaught ones of
    /// promise jobs and unhandled rejections) go to the console and the
    /// log, as Chromium's console shows them; the first error is returned.
    pub fn run_classic_script(&mut self, source: &str, name: &str) -> Result<(), ScriptError> {
        if self.stopped {
            return Err(STOPPED);
        }
        self.start_task();
        let result = self.rt.eval_named(source, name).map(|_| ());
        if let Err(error) = &result {
            self.report(name, error);
            self.count_termination(error);
        }
        self.checkpoint(name, None);
        self.rt.set_deadline(None);
        result
    }

    /// Runs the inline classic scripts of the lent document in tree order,
    /// each as a task, until the document's scripts are stopped (feature
    /// 4c2: at the end of the parse; feature 5c runs them during the
    /// parse). `name` (the document URL) names them in error reports.
    /// Returns the number of scripts that ran.
    pub fn run_inline_scripts(&mut self, name: &str) -> usize {
        let scripts = match crate::state::dom(&self.rt) {
            Ok(dom) => crate::scripts::inline_classic_scripts(dom),
            Err(_) => return 0,
        };
        let mut ran = 0;
        for script in &scripts {
            if self.stopped {
                break;
            }
            // Errors are reported; the next script runs.
            let _ = self.run_classic_script(script, name);
            ran += 1;
        }
        ran
    }

    /// Evaluates `source` as a classic script in the global scope (the
    /// value of the last expression statement) as a task; a promise is
    /// awaited as far as the microtask checkpoint settles it. Returns the
    /// JSON text of the value in `form`, or the error.
    pub fn evaluate(&mut self, source: &str, form: JsonForm) -> Evaluation {
        if self.stopped {
            return Evaluation::Error(error_text(&STOPPED));
        }
        self.start_task();
        let evaluation = match self.rt.eval_named(source, "") {
            Ok(value) => {
                // Held across the runs below (each replaces the last
                // value). The evaluation handles a rejection of the value,
                // so the checkpoint does not report it.
                self.pin(value);
                let evaluation = self.settle(value, form);
                self.checkpoint("evaluate", Some(value));
                self.unpin();
                evaluation
            }
            Err(error) => {
                let text = self.thrown_text(&error);
                self.count_termination(&error);
                self.checkpoint("evaluate", None);
                Evaluation::Error(text)
            }
        };
        self.rt.set_deadline(None);
        evaluation
    }

    /// The value of a promise after a microtask checkpoint, then its JSON
    /// text.
    fn settle(&mut self, value: Value, form: JsonForm) -> Evaluation {
        if let Err(error) = self.rt.run_jobs() {
            let text = self.thrown_text(&error);
            self.count_termination(&error);
            return Evaluation::Error(text);
        }
        let (value, is_error) = match self.rt.promise_settlement(value) {
            None => (value, false),
            Some((PromiseStatus::Fulfilled, result)) => (result, false),
            Some((PromiseStatus::Rejected, reason)) => (reason, true),
            Some((PromiseStatus::Pending, _)) => {
                return Evaluation::Error(
                    "Error: the promise did not settle (swb has no event loop yet)".to_owned(),
                );
            }
        };
        self.pin(value);
        let text = self.serialize(value, form, is_error);
        self.unpin();
        match text {
            Ok(text) if is_error => Evaluation::Error(text),
            Ok(text) => Evaluation::Value(text),
            Err(error) => Evaluation::Error(error_text(&error)),
        }
    }

    /// Calls the serializer.
    fn serialize(
        &mut self,
        value: Value,
        form: JsonForm,
        is_error: bool,
    ) -> Result<String, ScriptError> {
        let serializer = self
            .rt
            .host_data::<WebState>()
            .and_then(|s| s.pinned.first().copied())
            .unwrap_or(Value::Undefined);
        let strict = Value::Bool(form == JsonForm::Strict);
        let text = self.rt.call_function(
            serializer,
            Value::Undefined,
            &[value, strict, Value::Bool(is_error)],
        )?;
        self.rt.to_rust_string(text).map_err(|e| internal(&e))
    }

    /// The console messages of the document so far (at most the last
    /// 1,000).
    pub fn console_messages(&self) -> Vec<ConsoleMessage> {
        self.console.borrow().iter().cloned().collect()
    }

    /// The bytes that the heap uses (for tests and measurements).
    pub fn heap_size(&self) -> usize {
        self.rt.heap().heap_size()
    }

    /// Runs a full collection (for tests).
    pub fn collect(&mut self) {
        self.rt.collect();
    }

    fn start_task(&mut self) {
        self.rt
            .set_deadline(Instant::now().checked_add(self.time_limit));
    }

    /// Counts a task that reached a limit; stops the document's scripts
    /// after the third time limit or the first heap limit.
    fn count_termination(&mut self, error: &ScriptError) {
        use swb_js::Termination;
        match error {
            ScriptError::Terminated(Termination::TimeLimit) => self.timeouts += 1,
            ScriptError::Terminated(Termination::HeapLimit | Termination::OutOfMemory) => {
                self.timeouts = MAX_TIMEOUTS;
            }
            _ => return,
        }
        if self.timeouts >= MAX_TIMEOUTS && !self.stopped {
            self.stopped = true;
            log::warn!("the scripts of the document stop after a task reached a limit ({error})");
            self.rt.clear_jobs();
        }
    }

    /// The microtask checkpoint at the end of a task (HTML "perform a
    /// microtask checkpoint"), with the reports of unhandled rejections
    /// other than of `handled`.
    fn checkpoint(&mut self, name: &str, handled: Option<Value>) {
        if let Err(error) = self.rt.run_jobs() {
            self.report(name, &error);
            self.count_termination(&error);
            // A termination leaves jobs that would run in the next task
            // without its time; drop them.
            if matches!(error, ScriptError::Terminated(_)) {
                self.rt.clear_jobs();
            }
        }
        // The promises; their results are the reasons.
        let reasons: Vec<Value> = self
            .rt
            .take_unhandled_rejections()
            .into_iter()
            .filter(|&p| Some(p) != handled)
            .map(|p| {
                self.rt
                    .promise_settlement(p)
                    .map_or(p, |(_, reason)| reason)
            })
            .collect();
        for &reason in &reasons {
            self.pin(reason);
        }
        for &reason in &reasons {
            let text = match self.serialize(reason, JsonForm::Strict, true) {
                Ok(text) => text,
                Err(_) => self.rt.display(reason),
            };
            let text = format!("Uncaught (in promise) {text}");
            log::warn!(target: "console", "{name}: {text}");
            push_message(&self.console, ConsoleLevel::Error, &text);
        }
        for _ in &reasons {
            self.unpin();
        }
    }

    /// Reports an error of a script to the log and the console buffer.
    fn report(&mut self, name: &str, error: &ScriptError) {
        let text = match error {
            ScriptError::Uncaught { .. } => format!("Uncaught {}", self.thrown_text(error)),
            other => other.to_string(),
        };
        log::warn!(target: "console", "{name}: {text}");
        push_message(&self.console, ConsoleLevel::Error, &text);
    }

    /// The text of a script error, `Name: message`. The engine reads the
    /// `name` and `message` of a thrown object without calling getters;
    /// when it found no `message` (a `DOMException`, whose `name` and
    /// `message` are accessors), the serializer reads them, under the
    /// time limit of the task. Call it before the next run replaces the
    /// last value.
    fn thrown_text(&mut self, error: &ScriptError) -> String {
        if let ScriptError::Uncaught { name, .. } = error
            && name.is_empty()
            && let value @ Value::Object(_) = self.rt.last_value()
        {
            self.pin(value);
            let text = self.serialize(value, JsonForm::Strict, true);
            self.unpin();
            if let Ok(text) = text {
                return text;
            }
        }
        error_text(error)
    }

    fn pin(&mut self, value: Value) {
        if let Ok(state) = state_mut(&mut self.rt) {
            state.pinned.push(value);
        }
    }

    fn unpin(&mut self) {
        if let Ok(state) = state_mut(&mut self.rt)
            && state.pinned.len() > 1
        {
            state.pinned.pop();
        }
    }
}

/// The text of a script error for an evaluation (`Name: message`).
fn error_text(error: &ScriptError) -> String {
    match error {
        ScriptError::Uncaught { name, message, .. } if name.is_empty() => message.clone(),
        ScriptError::Uncaught { name, message, .. } => format!("{name}: {message}"),
        other => other.to_string(),
    }
}

/// An error of the host's own calls into the runtime (the setup of the
/// bindings, the text of a value): an engine bug or a resource limit,
/// logged with its details.
fn internal(error: &VmError) -> ScriptError {
    log::warn!("the script runtime failed: {error:?}");
    ScriptError::Internal(swb_js::InternalError::Invariant(
        "a call of the host into the runtime failed",
    ))
}

/// Writes a console line to the log (target `console`).
fn log_console(level: ConsoleLevel, line: &str) {
    match level {
        ConsoleLevel::Debug => log::debug!(target: "console", "{line}"),
        ConsoleLevel::Log | ConsoleLevel::Info => log::info!(target: "console", "{line}"),
        ConsoleLevel::Warn => log::warn!(target: "console", "{line}"),
        ConsoleLevel::Error => log::error!(target: "console", "{line}"),
    }
}

/// Adds a message to the buffer, within its limits.
fn push_message(buffer: &RefCell<VecDeque<ConsoleMessage>>, level: ConsoleLevel, line: &str) {
    let mut text = line;
    if text.len() > MAX_CONSOLE_TEXT {
        let mut end = MAX_CONSOLE_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text = &text[..end];
    }
    let mut buffer = buffer.borrow_mut();
    if buffer.len() >= MAX_CONSOLE_MESSAGES {
        buffer.pop_front();
    }
    buffer.push_back(ConsoleMessage {
        level,
        text: text.to_owned(),
    });
}
