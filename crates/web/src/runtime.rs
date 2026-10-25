//! The script runtime of a document (ADR 0028 section 2): a `js::Runtime`
//! with the bindings, whose global object is the `Window`, the console
//! buffer, and the runs of tasks (ADR 0028 section 7).
//!
//! The engine creates one when it commits a document with JavaScript on
//! and drops it with the document. For each task it lends the document
//! state ([`ScriptRuntime::lend`]), runs scripts or due tasks of the event
//! loop ([`ScriptRuntime::run_due_tasks`]), and takes the state back
//! ([`ScriptRuntime::take_back`]). A task has a time limit
//! ([`TASK_TIME_LIMIT`]) and ends with a microtask checkpoint. The
//! document runs no more scripts after three tasks that reach the time
//! limit, one that reaches the heap limit (then the engine drops the
//! runtime), or when its long tasks used the budget of the document
//! ([`SCRIPT_BUDGET`]; ADR 0028 sections 7 and 16).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use swb_dom::NodeId;

use swb_js::{
    ConsoleLevel, PromiseStatus, ReactionKind, Runtime, RuntimeConfig, ScriptError, Value, VmError,
    VmResult,
};

use crate::clock::{DocumentClock, PageClock};
use crate::errors::Site;
use crate::event_loop::Task;
use crate::events::{DispatchTarget, TrustedEvent};
use crate::generated as g;
use crate::host::EngineHost;
use crate::state::{WebState, state_mut};

/// The time limit of a task (ADR 0028 section 16).
pub const TASK_TIME_LIMIT: Duration = Duration::from_secs(5);

/// The heap limit of a document's runtime (ADR 0028 section 16).
pub const HEAP_LIMIT: usize = 256 << 20;

/// The real time that the long tasks (of 50 ms or more) of a document may
/// take in total; then the document runs no more scripts (ADR 0028
/// section 16).
pub const SCRIPT_BUDGET: Duration = Duration::from_secs(30);

/// The number of tasks that may reach the time limit before the document
/// runs no more scripts (ADR 0028 section 7).
const MAX_TIMEOUTS: u32 = 3;

/// The page time that each task takes in virtual time, in milliseconds.
/// Without it, a chain of tasks that are due at once (an
/// `unhandledrejection` listener that rejects a new promise) keeps virtual
/// time still, and a wait for page time never ends.
const VIRTUAL_TASK_TIME: f64 = 0.01;

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
    pub(crate) rt: Runtime,
    console: Rc<RefCell<VecDeque<ConsoleMessage>>>,
    /// The page clock (shared with the binding state).
    clock: PageClock,
    /// The time limit of a task.
    time_limit: Duration,
    /// The tasks that reached the time limit.
    timeouts: u32,
    /// True when the document runs no more scripts.
    stopped: bool,
    /// True after a task reached the heap limit: the runtime cannot
    /// recover, and the engine drops it.
    failed: bool,
    /// The real time of the document's long tasks so far.
    long_task_time: Duration,
    /// The most real time that long tasks may take per document.
    script_budget: Duration,
}

/// The result of [`ScriptRuntime::start_evaluation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvaluationStart {
    /// The value or the error.
    Done(Evaluation),
    /// A promise that has not settled yet: ask again with
    /// [`ScriptRuntime::poll_evaluation`] after the event loop ran.
    Pending(EvaluationId),
}

/// An evaluation that waits for its promise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvaluationId(u64);

/// The termination of a task that does not start because the document's
/// scripts were stopped.
const STOPPED: ScriptError = ScriptError::Terminated(swb_js::Termination::Native(
    "the scripts of the document were stopped",
));

impl ScriptRuntime {
    /// A runtime with the bindings and the limits of ADR 0028 section 16,
    /// on the page clock `clock`.
    pub fn new(clock: PageClock) -> Result<ScriptRuntime, ScriptError> {
        let mut config = RuntimeConfig::default();
        config.heap.limit = HEAP_LIMIT;
        ScriptRuntime::with_config(config, clock)
    }

    /// A runtime with the bindings and another configuration (tests use
    /// the GC stress mode).
    pub fn with_config(
        config: RuntimeConfig,
        clock: PageClock,
    ) -> Result<ScriptRuntime, ScriptError> {
        let mut rt = Runtime::new(config)?;
        let document_clock = DocumentClock::new(clock.clone());
        let setup = |rt: &mut Runtime| -> Result<(), VmError> {
            g::define_classes::<WebState>(rt)?;
            let mut state = WebState::default();
            state.clock = document_clock;
            rt.set_host_data(state);
            let window_properties = crate::window::define_class(rt)?;
            g::create_interfaces::<WebState>(rt, 0)?;
            crate::window::install(rt, 0, window_properties)?;
            g::init_global::<WebState>(rt, 0, crate::state::NOT_A_NODE)
        };
        setup(&mut rt).map_err(|e| internal(&e))?;
        let wall = clock.clone();
        rt.set_clock(Box::new(move || wall.wall_now()));
        let serializer = rt.eval_named(SERIALIZER, "swb:serializer")?;
        state_mut(&mut rt)
            .map_err(|e| internal(&e))?
            .pinned
            .push(serializer);
        let state = state_mut(&mut rt).map_err(|e| internal(&e))?;
        let console = Rc::clone(&state.console);
        let logged = Rc::clone(&state.console_logged);
        let sink = Rc::clone(&console);
        rt.set_console(Box::new(move |level, line| {
            log_console(&logged, level, line);
            push_message(&sink, level, line);
        }));
        Ok(ScriptRuntime {
            rt,
            console,
            clock,
            time_limit: TASK_TIME_LIMIT,
            timeouts: 0,
            stopped: false,
            failed: false,
            long_task_time: Duration::ZERO,
            script_budget: SCRIPT_BUDGET,
        })
    }

    /// Sets the time limit of a task (tests use a short one).
    pub fn set_time_limit(&mut self, limit: Duration) {
        self.time_limit = limit;
    }

    /// Sets the heap limit in bytes (tests use a small one).
    pub fn set_heap_limit(&mut self, limit: usize) {
        self.rt.set_heap_limit(limit);
    }

    /// Sets the budget of the document's long tasks (tests use a short
    /// one).
    pub fn set_script_budget(&mut self, budget: Duration) {
        self.script_budget = budget;
    }

    /// True if the document runs no more scripts: three tasks reached the
    /// time limit, one reached the heap limit, or the long tasks used the
    /// document's budget.
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// True after a task reached the heap limit: the runtime cannot
    /// recover, and the engine drops it (ADR 0028 section 7). The DOM
    /// stays as it is.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// The page clock.
    pub fn clock(&self) -> &PageClock {
        &self.clock
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
    /// checkpoint, under the time limit. Errors (also those of promise jobs)
    /// are reported: an `error` event at the window, and a console line if
    /// no listener canceled it; the first error is returned.
    pub fn run_classic_script(&mut self, source: &str, name: &str) -> Result<(), ScriptError> {
        self.task(name, |rt| {
            let result = rt.eval_named(source, name).map(|_| ());
            if let Err(error) = &result {
                report_error(rt, &Site::script(name, source), error)?;
            }
            result
        })
    }

    /// Fires a trusted event at a node of the lent document or at the window
    /// as a task (DOM "fire an event"; ADR 0028 section 11): the dispatch
    /// under the time limit, a microtask checkpoint after each listener
    /// (HTML "clean up after running a callback") and after the task.
    /// Exceptions of listeners are reported and the dispatch goes on.
    /// Returns false if a listener canceled the event; an error if the
    /// document's scripts are stopped or the task ended (a termination,
    /// which counts like one of a script).
    pub fn dispatch_event(
        &mut self,
        target: DispatchTarget,
        event: &TrustedEvent,
    ) -> Result<bool, ScriptError> {
        let name = format!("the event '{}'", event.kind);
        self.task(&name, |rt| {
            set_listener_checkpoints(rt, true);
            let result = crate::events::dispatch_trusted(rt, target, event).map_err(task_error);
            set_listener_checkpoints(rt, false);
            if let Err(error @ ScriptError::Uncaught { .. }) = &result {
                // An exception of the bindings outside any listener.
                report_line(rt, &name, &error.to_string());
            }
            result
        })
    }

    /// The user's focus change (a click, Tab): `new` gets the focus, or
    /// with `None` the focus goes away, with the events of the focusing
    /// steps (`change` of an edited control, `blur`, `focusout`, `focus`,
    /// `focusin`; feature 6d2). A task.
    pub fn user_focus(&mut self, new: Option<NodeId>, visible: bool) -> Result<(), ScriptError> {
        self.task("a focus change", |rt| {
            crate::html::focus::user_focus(rt, new, visible).map_err(task_error)
        })
    }

    /// `blur` (`gained` false) or `focus` at the window: the user's focus
    /// left the page after its last element, or came back (feature 6d2).
    pub fn window_focus(&mut self, gained: bool) -> Result<(), ScriptError> {
        self.task("a focus change", |rt| {
            crate::html::focus::window_focus(rt, gained).map_err(task_error)
        })
    }

    /// Fires `change` at the text control `node` if its user's edit is not
    /// committed (Enter in a field, before the implicit submission). A task.
    pub fn commit_change(&mut self, node: NodeId) -> Result<(), ScriptError> {
        self.task("a change event", |rt| {
            crate::html::focus::commit_change(rt, node).map_err(task_error)
        })
    }

    /// The synthetic `click` of a key at `node` (trusted, as Chromium
    /// makes it): Enter on a button or link, Space on release, Enter in a
    /// text field for the default button. The activation behavior follows
    /// the dispatch. A task.
    pub fn simulated_click(&mut self, node: NodeId) -> Result<(), ScriptError> {
        self.task("a click", |rt| {
            crate::html::activation::simulated_click(rt, node).map_err(task_error)
        })
    }

    /// The implicit submission of `form` (Enter in a field of a form
    /// without a submit button): HTML "submit" with the validation and the
    /// `submit` event. A task.
    pub fn implicit_submit(&mut self, form: NodeId) -> Result<(), ScriptError> {
        self.task("a form submission", |rt| {
            crate::html::activation::implicit_submit(rt, form).map_err(task_error)
        })
    }

    /// Runs the tasks that are due at the page time now, in their order,
    /// until none is due or `budget` of real time has passed (checked
    /// between tasks). In virtual time each task moves the clock by
    /// [`VIRTUAL_TASK_TIME`]. Returns the number of tasks that ran. A
    /// stopped document runs none. It also returns after a task that made
    /// the load event due ([`ScriptRuntime::load_event_due`]), so that the
    /// engine fires `load` before the next task when nothing delays it.
    pub fn run_due_tasks(&mut self, budget: Duration) -> usize {
        let started = Instant::now();
        let mut ran = 0;
        let load_due = self.load_event_due();
        while !self.stopped {
            let now = self.clock.now();
            let task = state_mut(&mut self.rt)
                .ok()
                .and_then(|s| s.event_loop.tasks.pop_due(now));
            let Some(task) = task else {
                break;
            };
            // Errors are reported.
            let _ = self.run_task(task);
            self.clock.advance_to(now + VIRTUAL_TASK_TIME);
            ran += 1;
            if started.elapsed() >= budget || (!load_due && self.load_event_due()) {
                break;
            }
        }
        ran
    }

    /// The page time (milliseconds) of the next task, or `None` if no task
    /// waits or the document's scripts are stopped.
    pub fn next_task_time(&self) -> Option<f64> {
        if self.stopped {
            return None;
        }
        crate::state::state(&self.rt)
            .ok()?
            .event_loop
            .tasks
            .next_due()
    }

    /// The number of tasks that wait (timers included).
    pub fn pending_tasks(&self) -> usize {
        crate::state::state(&self.rt).map_or(0, |s| s.event_loop.tasks.len())
    }

    /// The number of active timers.
    pub fn active_timers(&self) -> usize {
        crate::state::state(&self.rt).map_or(0, |s| s.event_loop.timers.len())
    }

    fn run_task(&mut self, task: Task) -> Result<(), ScriptError> {
        match task {
            Task::Timer { id, handle } => {
                self.task("a timer", |rt| crate::timers::run_timer(rt, id, handle))
            }
            Task::NotifyRejections => self.task("unhandled rejections", |rt| {
                crate::event_loop::notify_rejections(rt).map_err(task_error)
            }),
            Task::ParserResume => {
                self.resume_parser();
                Ok(())
            }
            Task::ScriptErrorEvent(node) => self.task("a script element", |rt| {
                crate::scripts::fire_at_element(rt, node, "error").map_err(task_error)
            }),
            Task::ScriptFetchFailed(node) => {
                self.script_fetched(node, None);
                Ok(())
            }
            Task::JavascriptUrl(source) => self.task("a javascript: URL", |rt| {
                crate::html::activation::run_javascript_url(rt, &source).map_err(task_error)
            }),
            Task::FinalizationCleanup => self.task("a finalization registry", |rt| {
                if let Ok(state) = state_mut(rt) {
                    state.event_loop.cleanup_queued = false;
                }
                match rt.run_finalization_cleanup() {
                    Ok(_) => Ok(()),
                    Err(error) => {
                        report_error(rt, &Site::named("a finalization registry"), &error)?;
                        Err(error)
                    }
                }
            }),
        }
    }

    /// Evaluates `source` as a classic script in the global scope (the
    /// value of the last expression statement) as a task, and waits for a
    /// promise through this runtime's event loop (at most
    /// [`EVALUATION_TIMEOUT`] of real time; virtual time jumps to the next
    /// task). Returns the JSON text of the value in `form`, or the error.
    /// The engine, which also waits for the network, uses
    /// [`ScriptRuntime::start_evaluation`] instead.
    pub fn evaluate(&mut self, source: &str, form: JsonForm) -> Evaluation {
        let id = match self.start_evaluation(source, form) {
            EvaluationStart::Done(evaluation) => return evaluation,
            EvaluationStart::Pending(id) => id,
        };
        let deadline = Instant::now() + EVALUATION_TIMEOUT;
        loop {
            if let Some(evaluation) = self.poll_evaluation(id, form) {
                return evaluation;
            }
            if self.run_due_tasks(Duration::from_millis(16)) > 0 {
                continue;
            }
            let now = Instant::now();
            match self.next_task_time() {
                Some(time) if now < deadline => {
                    if self.clock.is_virtual() {
                        self.clock.advance_to(time);
                    } else {
                        let until = self.clock.instant_of(time).min(deadline);
                        std::thread::sleep(until.saturating_duration_since(now));
                    }
                }
                _ => break,
            }
        }
        self.cancel_evaluation(id);
        Evaluation::Error(NOT_SETTLED.to_owned())
    }

    /// Evaluates `source` as a classic script in the global scope as a
    /// task. A value that is not a promise, a settled promise (after the
    /// microtask checkpoint) and an error are done; a pending promise waits
    /// for [`ScriptRuntime::poll_evaluation`]. The evaluation handles a
    /// rejection of its promise, so it is not reported as unhandled.
    pub fn start_evaluation(&mut self, source: &str, form: JsonForm) -> EvaluationStart {
        let mut error_text = None;
        let started = self.task("evaluate", |rt| {
            let value = match rt.eval_named(source, "") {
                Ok(value) => value,
                Err(error) => {
                    error_text = Some(thrown_text(rt, &error));
                    return Err(error);
                }
            };
            if rt.promise_settlement(value).is_none() {
                pin(rt, value);
                let text = serialize(rt, value, form, false);
                unpin(rt);
                return Ok(EvaluationStart::Done(match text {
                    Ok(text) => Evaluation::Value(text),
                    Err(error) => Evaluation::Error(error_text_of(&error)),
                }));
            }
            // A reaction marks the promise as handled.
            rt.react_to_promise(value, ignore_settlement, Value::Undefined)?;
            let event_loop = &mut state_mut(rt).map_err(task_error)?.event_loop;
            let id = event_loop.next_evaluation;
            event_loop.next_evaluation += 1;
            event_loop.evaluations.push((id, value));
            Ok(EvaluationStart::Pending(EvaluationId(id)))
        });
        match started {
            Ok(EvaluationStart::Pending(id)) => match self.poll_evaluation(id, form) {
                Some(evaluation) => EvaluationStart::Done(evaluation),
                None => EvaluationStart::Pending(id),
            },
            Ok(done) => done,
            Err(error) => EvaluationStart::Done(Evaluation::Error(
                error_text.unwrap_or_else(|| error_text_of(&error)),
            )),
        }
    }

    /// The result of a pending evaluation once its promise settled (the
    /// JSON text of the value, or the error of a rejection), else `None`.
    /// A stopped document gives an error.
    pub fn poll_evaluation(&mut self, id: EvaluationId, form: JsonForm) -> Option<Evaluation> {
        let promise = crate::state::state(&self.rt)
            .ok()?
            .event_loop
            .evaluations
            .iter()
            .find(|(i, _)| *i == id.0)
            .map(|&(_, value)| value);
        let Some(promise) = promise else {
            return Some(Evaluation::Error(NOT_SETTLED.to_owned()));
        };
        let settlement = self.rt.promise_settlement(promise);
        let (value, is_error) = match settlement {
            Some((PromiseStatus::Fulfilled, result)) => (result, false),
            Some((PromiseStatus::Rejected, reason)) => (reason, true),
            _ if self.stopped => {
                self.cancel_evaluation(id);
                return Some(Evaluation::Error(error_text_of(&STOPPED)));
            }
            _ => return None,
        };
        let text = self.task("evaluate", |rt| {
            pin(rt, value);
            let text = serialize(rt, value, form, is_error);
            unpin(rt);
            text
        });
        self.cancel_evaluation(id);
        Some(match text {
            Ok(text) if is_error => Evaluation::Error(text),
            Ok(text) => Evaluation::Value(text),
            Err(error) => Evaluation::Error(error_text_of(&error)),
        })
    }

    /// True if the promise of a pending evaluation settled (or the
    /// evaluation is unknown, or the document's scripts stopped), so that
    /// [`ScriptRuntime::poll_evaluation`] gives a result.
    pub fn evaluation_settled(&self, id: EvaluationId) -> bool {
        let Ok(state) = crate::state::state(&self.rt) else {
            return true;
        };
        let promise = state
            .event_loop
            .evaluations
            .iter()
            .find(|(i, _)| *i == id.0);
        match promise {
            Some(&(_, promise)) => {
                self.stopped
                    || !matches!(
                        self.rt.promise_settlement(promise),
                        Some((PromiseStatus::Pending, _))
                    )
            }
            None => true,
        }
    }

    /// Forgets a pending evaluation (its time ran out).
    pub fn cancel_evaluation(&mut self, id: EvaluationId) {
        if let Ok(state) = state_mut(&mut self.rt) {
            state.event_loop.evaluations.retain(|(i, _)| *i != id.0);
        }
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
        // A collection can make finalization cleanups due.
        let _ = crate::event_loop::queue_finalization_cleanup(&mut self.rt);
    }

    /// Runs `body` as a task (HTML "run a task"): the deadline (the time
    /// limit, or less when the document's budget is almost used), a handle
    /// scope, the body, the microtask checkpoint, the cleanup tasks of
    /// finalization registries, and the stop rules. `name` names the task
    /// in the log. The body reports its own errors; a termination counts.
    pub(crate) fn task<R>(
        &mut self,
        name: &str,
        body: impl FnOnce(&mut Runtime) -> Result<R, ScriptError>,
    ) -> Result<R, ScriptError> {
        if self.stopped {
            return Err(STOPPED);
        }
        let started = Instant::now();
        let remaining = self.script_budget.saturating_sub(self.long_task_time) + LONG_TASK;
        self.rt
            .set_deadline(started.checked_add(self.time_limit.min(remaining)));
        let scope = self.rt.heap_mut().open_scope();
        let result = body(&mut self.rt);
        if let Err(error @ (ScriptError::Terminated(_) | ScriptError::Internal(_))) = &result {
            report_line(&self.rt, name, &error.to_string());
            self.count_termination(error);
        }
        if let Err(error) = crate::event_loop::microtask_checkpoint(&mut self.rt) {
            report_line(&self.rt, name, &error.to_string());
            self.count_termination(&error);
        }
        if let Err(error) = self.rt.heap_mut().close_scope(scope) {
            log::warn!("the handle scope of a task was not the last one: {error:?}");
        }
        if let Ok(state) = state_mut(&mut self.rt) {
            state.event_loop.nesting = 0;
        }
        if let Err(error) = crate::event_loop::queue_finalization_cleanup(&mut self.rt) {
            log::warn!("cannot queue a finalization cleanup: {error:?}");
        }
        self.rt.set_deadline(None);
        self.count_time(started.elapsed());
        result
    }

    /// Adds the time of a long task to the document's total; stops the
    /// document's scripts when the total reaches the budget.
    fn count_time(&mut self, elapsed: Duration) {
        if elapsed < LONG_TASK {
            return;
        }
        self.long_task_time += elapsed;
        if self.long_task_time >= self.script_budget && !self.stopped {
            log::warn!(
                "the scripts of the document stop: its long tasks ran for {:.1} s (the budget is {} s)",
                self.long_task_time.as_secs_f64(),
                self.script_budget.as_secs_f64()
            );
            self.stop();
        }
    }

    /// Counts a task that reached a limit; stops the document's scripts
    /// after the third time limit or the first heap limit.
    fn count_termination(&mut self, error: &ScriptError) {
        use swb_js::Termination;
        match error {
            ScriptError::Terminated(Termination::TimeLimit) => self.timeouts += 1,
            ScriptError::Terminated(Termination::HeapLimit | Termination::OutOfMemory) => {
                self.timeouts = MAX_TIMEOUTS;
                self.failed = true;
            }
            _ => return,
        }
        if self.timeouts >= MAX_TIMEOUTS && !self.stopped {
            log::warn!("the scripts of the document stop after a task reached a limit ({error})");
            self.stop();
        }
    }

    /// Stops the document's scripts: no task, job or callback runs again.
    fn stop(&mut self) {
        self.stopped = true;
        self.rt.clear_jobs();
    }
}

/// The time of a task from which it counts against the document's budget
/// (the threshold of a long task in the Long Tasks API).
const LONG_TASK: Duration = Duration::from_millis(50);

/// The longest that [`ScriptRuntime::evaluate`] waits for a promise.
pub const EVALUATION_TIMEOUT: Duration = Duration::from_secs(30);

/// The error of an evaluation whose promise did not settle in time.
const NOT_SETTLED: &str = "Error: the promise did not settle in time";

/// The reaction that marks an evaluated promise as handled.
#[allow(clippy::unnecessary_wraps, reason = "the type of a host reaction")]
fn ignore_settlement(
    _rt: &mut Runtime,
    _data: Value,
    _kind: ReactionKind,
    _value: Value,
) -> VmResult<Value> {
    Ok(Value::Undefined)
}

/// Turns the per-listener checkpoints of an engine dispatch on or off.
fn set_listener_checkpoints(rt: &mut Runtime, on: bool) {
    if let Ok(state) = state_mut(rt) {
        state.event_loop.listener_checkpoints = on;
    }
}

/// Reports an error of a run (see [`crate::errors::report`]); a
/// termination of the report goes up as the task's error.
fn report_error(rt: &mut Runtime, site: &Site<'_>, error: &ScriptError) -> Result<(), ScriptError> {
    crate::errors::report(rt, site, error).map_err(task_error)
}

/// Writes an error line to the log and the console buffer.
pub(crate) fn report_line(rt: &Runtime, name: &str, text: &str) {
    let Some(state) = rt.host_data::<WebState>() else {
        log::warn!(target: "console", "{name}: {text}");
        return;
    };
    log_line(
        &state.console_logged,
        log::Level::Warn,
        &format!("{name}: {text}"),
    );
    push_message(&state.console, ConsoleLevel::Error, text);
}

/// The text of a script error, `Name: message`. The engine reads the
/// `name` and `message` of a thrown object without calling getters; when it
/// found no `message` (a `DOMException`, whose `name` and `message` are
/// accessors), the serializer reads them, under the time limit of the
/// task. Call it before the next run replaces the last value.
pub(crate) fn thrown_text(rt: &mut Runtime, error: &ScriptError) -> String {
    if let ScriptError::Uncaught { name, .. } = error
        && name.is_empty()
        && let value @ Value::Object(_) = rt.last_value()
    {
        return thrown_value_text(rt, value);
    }
    error_text_of(error)
}

/// The text of a thrown value: `Name: message` of an object with a
/// `message` (read by the serializer), else the value as a string.
pub(crate) fn thrown_value_text(rt: &mut Runtime, value: Value) -> String {
    if !matches!(value, Value::Object(_)) {
        return rt.display(value);
    }
    pin(rt, value);
    let text = serialize(rt, value, JsonForm::Strict, true);
    unpin(rt);
    text.unwrap_or_else(|_| rt.display(value))
}

/// Calls the serializer (the first pinned value).
fn serialize(
    rt: &mut Runtime,
    value: Value,
    form: JsonForm,
    is_error: bool,
) -> Result<String, ScriptError> {
    let serializer = rt
        .host_data::<WebState>()
        .and_then(|s| s.pinned.first().copied())
        .unwrap_or(Value::Undefined);
    let strict = Value::Bool(form == JsonForm::Strict);
    let text = rt.call_function(
        serializer,
        Value::Undefined,
        &[value, strict, Value::Bool(is_error)],
    )?;
    rt.to_rust_string(text).map_err(|e| internal(&e))
}

/// Holds a value across runs (until [`unpin`]).
pub(crate) fn pin(rt: &mut Runtime, value: Value) {
    if let Ok(state) = state_mut(rt) {
        state.pinned.push(value);
    }
}

/// Drops the last pinned value (never the serializer).
pub(crate) fn unpin(rt: &mut Runtime) {
    if let Ok(state) = state_mut(rt)
        && state.pinned.len() > 1
    {
        state.pinned.pop();
    }
}

/// The API error of a task that a host call ended (the engine's event
/// dispatch): a termination, an engine error, or an exception of the
/// bindings outside any listener.
pub(crate) fn task_error(error: VmError) -> ScriptError {
    match error {
        VmError::Terminated(termination) => ScriptError::Terminated(termination),
        VmError::Internal(error) => ScriptError::Internal(error),
        VmError::Raise { kind, message } => ScriptError::Uncaught {
            name: kind.name().to_owned(),
            message: message.into_owned(),
            offset: None,
        },
        VmError::Throw(_) => ScriptError::Uncaught {
            name: String::new(),
            message: "(exception)".to_owned(),
            offset: None,
        },
    }
}

/// The text of a script error for an evaluation (`Name: message`).
fn error_text_of(error: &ScriptError) -> String {
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
fn log_console(logged: &Cell<usize>, level: ConsoleLevel, line: &str) {
    let level = match level {
        ConsoleLevel::Debug => log::Level::Debug,
        ConsoleLevel::Log | ConsoleLevel::Info => log::Level::Info,
        ConsoleLevel::Warn => log::Level::Warn,
        ConsoleLevel::Error => log::Level::Error,
    };
    log_line(logged, level, line);
}

/// The most console lines and error reports of a document that the log
/// gets at their own level; the next ones go to the debug level, so that a
/// loop of `console.error` does not flood standard error.
const MAX_LOGGED_LINES: usize = 1000;

/// Writes a line to the log at `level`, or at the debug level after the
/// first [`MAX_LOGGED_LINES`] lines of the document (counted in `logged`).
fn log_line(logged: &Cell<usize>, level: log::Level, line: &str) {
    let count = logged.get();
    logged.set(count.saturating_add(1));
    if count < MAX_LOGGED_LINES {
        log::log!(target: "console", level, "{line}");
        return;
    }
    if count == MAX_LOGGED_LINES {
        log::warn!(
            target: "console",
            "the document wrote {MAX_LOGGED_LINES} console lines; the next ones go to the debug log"
        );
    }
    log::debug!(target: "console", "{line}");
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
