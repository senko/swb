//! The script runtime of an agent (ADR 0028 section 2, ADR 0031 section 5):
//! a `js::Runtime` with the bindings, a realm with a `Window` for each of its
//! documents, the console buffer, and the runs of tasks (ADR 0028 section 7).
//! The methods below act on the current document, the first one unless the
//! host selects another ([`ScriptRuntime::with_document`]).
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

use swb_js::{ConsoleLevel, Runtime, RuntimeConfig, ScriptError, Value, VmError};

use crate::clock::{DocumentClock, PageClock};
use crate::errors::Site;
use crate::event_loop::Task;
use crate::events::{DispatchTarget, TrustedEvent};
use crate::generated as g;
use crate::host::{EngineHost, FrameHost};
use crate::state::{DocState, WebState, state_mut};

mod documents;
mod evaluate;
mod rendering;

pub use evaluate::EVALUATION_TIMEOUT;
use evaluate::SERIALIZER;

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
    /// The number of the frame of the document that wrote it (0 for the top
    /// level; [`crate::FrameHost`]).
    pub frame: u32,
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

/// The frame id of the `WindowProxy` of a runtime's first document.
const TOP_FRAME: u32 = 0;

/// Makes the `Window` of the realm `realm`: its interfaces, its named
/// properties object and its global object.
pub(crate) fn init_window(rt: &mut Runtime, realm: u32) -> Result<(), VmError> {
    let Some(window_properties) = crate::state::state(rt)?.window_properties else {
        return Err(VmError::type_error("the class WindowProperties is missing"));
    };
    g::create_interfaces::<WebState>(rt, realm)?;
    crate::window::install(rt, realm, window_properties)?;
    crate::cssom::install_css_namespace(rt, realm)?;
    g::init_global::<WebState>(rt, realm, crate::state::NOT_A_NODE)
}

/// The key of a document in a runtime (ADR 0031 section 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocumentKey(u32);

impl DocumentKey {
    /// The runtime's first document.
    pub const FIRST: DocumentKey = DocumentKey(0);

    /// The key of the binding state.
    pub(crate) fn of(key: u32) -> DocumentKey {
        DocumentKey(key)
    }

    /// The number of the binding state.
    pub(crate) fn raw(self) -> u32 {
        self.0
    }
}

/// What the runtimes of one page share (ADR 0031 sections 6, 7 and 11): the console
/// buffer, so that its messages keep their order across runtimes, the count of
/// its lines for the log, the sequence counter of the tasks, the byte budgets
/// of the frames' runtimes, which the heap limit does not count, and the message ports
/// with what the page has to hand from one runtime to another.
#[derive(Clone, Default)]
pub struct AgentShared {
    console: Rc<RefCell<VecDeque<ConsoleMessage>>>,
    console_logged: Rc<Cell<usize>>,
    task_counter: Rc<Cell<u64>>,
    budgets: crate::page_budget::PageBudgets,
    port_hub: Rc<crate::message_port::PortHub>,
}

impl AgentShared {
    /// The console messages of all runtimes of the page (at most the last
    /// 1,000).
    pub fn console_messages(&self) -> Vec<ConsoleMessage> {
        self.console.borrow().iter().cloned().collect()
    }

    /// True if a task left something for another runtime.
    #[inline]
    pub fn has_mail(&self) -> bool {
        self.port_hub.has_mail()
    }

    /// What the tasks left for other runtimes, in the order of the calls. The page gives
    /// each to its runtime ([`ScriptRuntime::receive_mail`]) or drops it.
    pub fn take_mail(&self) -> VecDeque<crate::messaging::Mail> {
        self.port_hub.take_mail()
    }

    /// Does what the runtimes that went and the messages that were dropped left to do:
    /// disentangles and closes their ports.
    pub fn settle_ports(&self) {
        self.port_hub.reap();
    }

    /// The bytes of the waiting messages that the frames' runtimes sent (tests and
    /// measurements).
    pub fn frames_message_bytes(&self) -> u64 {
        self.budgets.messages.get()
    }

    /// The live message ports of the page (tests and measurements).
    pub fn live_message_ports(&self) -> usize {
        self.port_hub.live()
    }
}

/// The script runtime of an agent: its documents share one heap (ADR 0031
/// section 5). A runtime starts with one document.
pub struct ScriptRuntime {
    pub(crate) rt: Runtime,
    /// What this runtime shares with the other runtimes of its page.
    shared: AgentShared,
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
        ScriptRuntime::build(config, clock, TOP_FRAME, &AgentShared::default())
    }

    /// The runtime of another origin of a page: its first document belongs to
    /// the frame `frame` (below 2^30), and it shares `shared` with the other
    /// runtimes of the page (ADR 0031 section 5). The page lends it no
    /// document: a task borrows them from the frame registry.
    pub fn for_frame(
        clock: PageClock,
        frame: u32,
        shared: &AgentShared,
    ) -> Result<ScriptRuntime, ScriptError> {
        let mut config = RuntimeConfig::default();
        config.heap.limit = HEAP_LIMIT;
        ScriptRuntime::build(config, clock, frame, shared)
    }

    fn build(
        config: RuntimeConfig,
        clock: PageClock,
        frame: u32,
        shared: &AgentShared,
    ) -> Result<ScriptRuntime, ScriptError> {
        let mut rt = Runtime::new(config)?;
        let document_clock = DocumentClock::new(clock.clone());
        let setup = |rt: &mut Runtime| -> Result<(), VmError> {
            g::define_classes::<WebState>(rt)?;
            rt.set_host_data(WebState::default());
            crate::window::define_class(rt)?;
            crate::window_proxy::install_hooks(rt);
            init_window(rt, 0)?;
            crate::state::add_document(rt, 0, frame, document_clock)?;
            Ok(())
        };
        setup(&mut rt).map_err(|e| internal(&e))?;
        // Before the first compile: the compiler accepts `import()` and
        // `import.meta` only in a runtime with module hooks (ADR 0034,
        // "3a-3 implementation notes").
        crate::scripts::install_module_hooks(&mut rt);
        let wall = clock.clone();
        rt.set_clock(Box::new(move || wall.wall_now()));
        let serializer = rt.eval_named(SERIALIZER, "swb:serializer")?;
        state_mut(&mut rt)
            .map_err(|e| internal(&e))?
            .pinned
            .push(serializer);
        let state = state_mut(&mut rt).map_err(|e| internal(&e))?;
        state.console = Rc::clone(&shared.console);
        state.console_logged = Rc::clone(&shared.console_logged);
        state
            .event_loop
            .tasks
            .set_counter(Rc::clone(&shared.task_counter));
        state.ports = Rc::new(crate::message_port::Ports::new(Rc::clone(&shared.port_hub)));
        // The top level's runtime keeps its own count of these bytes (ADR 0031
        // section 11).
        if frame != TOP_FRAME {
            state.blob_urls.join_page(&shared.budgets.blob_urls);
            state.held_blobs.join_page(&shared.budgets.held_blobs);
            state.ports.join_page(&shared.budgets.messages);
            state.mutation.join_page(&shared.budgets.mutation_records);
        }
        state.console_frame.set(frame);
        let shared = shared.clone();
        let sink = Rc::clone(&state.console);
        let logged = Rc::clone(&state.console_logged);
        let frame = Rc::clone(&state.console_frame);
        rt.set_console(Box::new(move |level, line| {
            log_console(&logged, level, line);
            push_message(&sink, frame.get(), level, line);
        }));
        Ok(ScriptRuntime {
            rt,
            shared,
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

    /// What this runtime shares with the other runtimes of its page.
    pub fn shared(&self) -> AgentShared {
        self.shared.clone()
    }

    /// The real time of the long tasks of the runtime so far.
    pub fn long_task_time(&self) -> Duration {
        self.long_task_time
    }

    /// Stops the scripts of the runtime: the long tasks of the page used its
    /// budget (ADR 0031 section 11).
    pub fn stop_scripts(&mut self) {
        if !self.stopped {
            self.stop();
        }
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

    /// Runs `f` with the document `key` as the current one: the scripts,
    /// events and tasks that it starts belong to that document. An unknown
    /// key gives `None`.
    pub fn with_document<R>(
        &mut self,
        key: DocumentKey,
        f: impl FnOnce(&mut ScriptRuntime) -> R,
    ) -> Option<R> {
        let entered = crate::state::switch_if_other(&mut self.rt, key.0).ok()?;
        crate::state::ensure_current_lent(&mut self.rt);
        let result = f(self);
        crate::state::leave(&mut self.rt, entered);
        Some(result)
    }

    /// Lends the state of the runtime's first document for a task. A runtime
    /// with several documents lends each with [`ScriptRuntime::lend_document`]
    /// (ADR 0031 section 5).
    pub fn lend(&mut self, host: Box<dyn EngineHost>) {
        self.lend_document(DocumentKey::FIRST, host);
    }

    /// Takes the lent state of the first document back.
    pub fn take_back(&mut self) -> Option<Box<dyn EngineHost>> {
        self.take_back_document(DocumentKey::FIRST)
    }

    /// Lends the state of the document `key` for a task.
    pub fn lend_document(&mut self, key: DocumentKey, host: Box<dyn EngineHost>) {
        if let Ok(state) = state_mut(&mut self.rt)
            && let Some(doc) = crate::state::document_mut(state, key.0)
        {
            doc.host = Some(host);
            doc.note_storage_key();
        }
    }

    /// Takes the lent state of the document `key` back.
    pub fn take_back_document(&mut self, key: DocumentKey) -> Option<Box<dyn EngineHost>> {
        let state = state_mut(&mut self.rt).ok()?;
        crate::state::document_mut(state, key.0)?.host.take()
    }

    /// Runs a classic script as a task: the script, then a microtask
    /// checkpoint, under the time limit. Errors (also those of promise jobs)
    /// are reported: an `error` event at the window, and a console line if
    /// no listener canceled it; the first error is returned.
    pub fn run_classic_script(&mut self, source: &str, name: &str) -> Result<(), ScriptError> {
        self.task(name, |rt| {
            let realm = crate::state::doc_realm(rt).map_err(task_error)?;
            let result = rt
                .eval_named_at(realm, source, name, (0, 0), None)
                .map(|_| ());
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

    /// The events of a navigation within the document that the page carried
    /// out (HTML "navigate to a fragment" and "update document for history
    /// step application"): `popstate` with the state of the entry that
    /// became current, now, then, if the fragment changed (`hash_change`
    /// has the old and the new URL), `hashchange` as a task.
    pub fn same_document_navigation(
        &mut self,
        state: Option<Rc<crate::Serialized>>,
        hash_change: Option<(String, String)>,
    ) -> Result<(), ScriptError> {
        self.task("a navigation within the document", |rt| {
            crate::location::navigated_within_document(rt, state, hash_change).map_err(task_error)
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

    /// The user's selection in the focused text control `node` changed (a
    /// key, a click, a drag): `select` now if `select` (the new selection
    /// is not empty), `selectionchange` in a task (feature 9b2). A task.
    pub fn user_selection(&mut self, node: NodeId, select: bool) -> Result<(), ScriptError> {
        self.task("a selection event", |rt| {
            crate::html::selection_events::user_changed(rt, node, select).map_err(task_error)
        })
    }

    /// Fires `change` at the text control `node` if its user's edit is not
    /// committed (Enter in a field, before the implicit submission). A task.
    pub fn commit_change(&mut self, node: NodeId) -> Result<(), ScriptError> {
        self.task("a change event", |rt| {
            crate::html::focus::commit_change(rt, node).map_err(task_error)
        })
    }

    /// Fires `input` and `change` at the select `node` whose selected
    /// option the user changed with a key. A task.
    pub fn select_changed(&mut self, node: NodeId) -> Result<(), ScriptError> {
        self.task("a change event", |rt| {
            crate::html::activation::select_changed(rt, node).map_err(task_error)
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
        self.run_due_tasks_before(budget, None)
    }

    /// [`ScriptRuntime::run_due_tasks`] up to the task `before` of another
    /// runtime of the page, a due time and a sequence number
    /// ([`ScriptRuntime::next_task_key`]; ADR 0031 section 6).
    pub fn run_due_tasks_before(&mut self, budget: Duration, before: Option<(f64, u64)>) -> usize {
        let started = Instant::now();
        let mut ran = 0;
        let load_due = self.load_event_due();
        if let Ok(state) = state_mut(&mut self.rt) {
            state.frame_load_due = false;
        }
        while !self.stopped {
            let now = self.clock.now();
            let task = state_mut(&mut self.rt)
                .ok()
                .and_then(|s| s.event_loop.tasks.pop_due(now, before));
            let Some((document, task)) = task else {
                break;
            };
            // A task of a document that left its frame does not run, and
            // does not count. A task of a window goes with the window.
            let document = crate::state::state(&self.rt)
                .ok()
                .and_then(|s| crate::state::task_document(s, document));
            let Some(document) = document else {
                continue;
            };
            // Errors are reported. A task runs in its document.
            self.with_document(DocumentKey(document), |runtime| runtime.run_task(task));
            self.clock.advance_to(now + VIRTUAL_TASK_TIME);
            ran += 1;
            let frame_load =
                state_mut(&mut self.rt).is_ok_and(|s| std::mem::take(&mut s.frame_load_due));
            // A message for another runtime has an earlier number than the next task of
            // this one: the page hands it over first.
            if self.shared.has_mail()
                || started.elapsed() >= budget
                || frame_load
                || (!load_due && self.load_event_due())
            {
                break;
            }
        }
        ran
    }

    /// The tag of this runtime in [`crate::Mail`].
    pub fn mail_tag(&self) -> u32 {
        crate::state::state(&self.rt).map_or(u32::MAX, |s| s.ports.tag())
    }

    /// The page hands `mail` to this runtime, between tasks: for the document `document`
    /// if it is a window message. A stopped runtime runs no task, so the mail goes: its
    /// bytes go back and the ports in it close.
    pub fn receive_mail(&mut self, document: Option<DocumentKey>, mail: crate::Mail) {
        if self.stopped {
            return;
        }
        let document = document.map(DocumentKey::raw);
        if let Err(error) = crate::messaging::receive(&mut self.rt, document, mail) {
            log::warn!("cannot queue a message of another runtime: {error:?}");
        }
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

    /// The due time and the sequence number of the next task, or `None` if no
    /// task waits or the scripts are stopped.
    pub fn next_task_key(&self) -> Option<(f64, u64)> {
        if self.stopped {
            return None;
        }
        crate::state::state(&self.rt)
            .ok()?
            .event_loop
            .tasks
            .next_key()
    }

    /// The number of active timers.
    pub fn active_timers(&self) -> usize {
        crate::state::state(&self.rt).map_or(0, |s| s.doc.timers.len())
    }

    fn run_task(&mut self, task: Task) -> Result<(), ScriptError> {
        match task {
            Task::Timer { id, handle } => {
                self.task("a timer", |rt| crate::timers::run_timer(rt, id, handle))
            }
            Task::NotifyRejections => self.task("unhandled rejections", |rt| {
                crate::event_loop::notify_rejections(rt).map_err(task_error)
            }),
            Task::PerformanceObservers => self.task("performance observers", |rt| {
                crate::performance_observer::deliver(rt)
            }),
            Task::IntersectionObservers => self.task("intersection observers", |rt| {
                crate::intersection_observer::deliver(rt)
            }),
            Task::IdleTimeout(handle) => self.task("an idle callback", |rt| {
                crate::idle_callbacks::run(rt, handle, None)
            }),
            Task::ParserResume => {
                self.resume_parser();
                Ok(())
            }
            Task::SelectionEvents => self.task("selection events", |rt| {
                set_listener_checkpoints(rt, true);
                let result = crate::html::selection_events::deliver(rt);
                set_listener_checkpoints(rt, false);
                result
            }),
            Task::ScriptErrorEvent(node) => self.task("a script element", |rt| {
                crate::scripts::fire_at_element(rt, node, "error").map_err(task_error)
            }),
            Task::ScriptFetchFailed(node) => {
                self.script_fetched(node, None);
                Ok(())
            }
            Task::ModuleReady(node) => {
                self.module_ready(node);
                Ok(())
            }
            Task::ModuleImport(token) => self.module_import(token),
            Task::JavascriptUrl(source) => self.task("a javascript: URL", |rt| {
                crate::html::activation::run_javascript_url(rt, &source).map_err(task_error)
            }),
            Task::HashChange { old_url, new_url } => self.task("the event 'hashchange'", |rt| {
                set_listener_checkpoints(rt, true);
                let result =
                    crate::location::fire_hash_change(rt, old_url, new_url).map_err(task_error);
                set_listener_checkpoints(rt, false);
                result
            }),
            Task::Message => self.task("the event 'message'", |rt| {
                set_listener_checkpoints(rt, true);
                let result = crate::messaging::deliver(rt).map_err(task_error);
                set_listener_checkpoints(rt, false);
                result
            }),
            Task::Storage => self.storage_event_task(),
            Task::PortMessage(id) => self.task("the event 'message'", |rt| {
                set_listener_checkpoints(rt, true);
                let result = crate::message_port::deliver(rt, id).map_err(task_error);
                set_listener_checkpoints(rt, false);
                result
            }),
            Task::XhrTimeout(id) => self.task("an XMLHttpRequest timeout", |rt| {
                set_listener_checkpoints(rt, true);
                let result = crate::xhr::timeout_fired(rt, id).map_err(task_error);
                set_listener_checkpoints(rt, false);
                result
            }),
            Task::LocalFetch(id) => self.task("a local fetch", |rt| {
                set_listener_checkpoints(rt, true);
                let result = crate::fetch::local_completed(rt, id).map_err(task_error);
                set_listener_checkpoints(rt, false);
                result
            }),
            Task::FileReader {
                id,
                generation,
                finish,
            } => self.task("a FileReader", |rt| {
                set_listener_checkpoints(rt, true);
                let result =
                    crate::file_api::reader_task(rt, id, generation, finish).map_err(task_error);
                set_listener_checkpoints(rt, false);
                result
            }),
            Task::AbortTimeout(id) => self.task("AbortSignal.timeout()", |rt| {
                crate::abort::timeout_fired(rt, id).map_err(task_error)
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

    /// The console messages of all runtimes of the page so far (at most the last
    /// 1,000).
    pub fn console_messages(&self) -> Vec<ConsoleMessage> {
        self.shared.console_messages()
    }

    /// The bytes that the heap uses (for tests and measurements).
    pub fn heap_size(&self) -> usize {
        self.rt.heap().heap_size()
    }

    /// The number of live message ports of this runtime: those that were
    /// made and not closed or collected (for tests and measurements).
    pub fn live_message_ports(&self) -> usize {
        crate::state::state(&self.rt).map_or(0, |s| s.ports.live())
    }

    /// The number of messages that this runtime sent and that wait in the queues of the
    /// ports and of `window.postMessage()` (for tests and measurements).
    pub fn waiting_messages(&self) -> usize {
        crate::state::state(&self.rt).map_or(0, |s| s.ports.waiting())
    }

    /// The bytes of the messages that this runtime sent and that wait (for tests and
    /// measurements).
    pub fn waiting_message_bytes(&self) -> u64 {
        crate::state::state(&self.rt).map_or(0, |s| s.ports.waiting_bytes())
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
        self.shared.port_hub.next_task();
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
        self.collect_between_tasks();
        if let Ok(state) = state_mut(&mut self.rt) {
            state.event_loop.nesting = 0;
        }
        if let Err(error) = crate::event_loop::queue_finalization_cleanup(&mut self.rt) {
            log::warn!("cannot queue a finalization cleanup: {error:?}");
        }
        self.rt.set_deadline(None);
        // Another runtime can send messages that take long to deserialize: they do not use
        // up the budget of this one.
        let messages =
            state_mut(&mut self.rt).map_or(Duration::ZERO, |state| state.message_cost.take_time());
        self.count_time(started.elapsed().saturating_sub(messages));
        result
    }

    /// The safepoint at the end of a task, when no handle scope is open:
    /// memory that a task charged without running a script (the bytes of a
    /// response, `Heap::generic_resized`) would otherwise stay until the
    /// next script reaches a safepoint, and a page whose requests have no
    /// listeners would never collect. A heap that stays over the limit
    /// stops the document's scripts, as a safepoint in a script does.
    fn collect_between_tasks(&mut self) {
        let heap = self.rt.heap();
        if heap.scope_depth() > 0 || !heap.gc_due() {
            return;
        }
        if let Err(error) = self.rt.safepoint() {
            let error = task_error(error);
            report_line(&self.rt, "the heap", &error.to_string());
            self.count_termination(&error);
        }
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

    /// Stops the document's scripts: no task, job or callback runs again. So what waits
    /// for a task goes, with the bytes that it holds in the budget of other runtimes.
    fn stop(&mut self) {
        self.stopped = true;
        self.rt.clear_jobs();
        if let Ok(state) = state_mut(&mut self.rt) {
            state.mutation.close();
            state.doc.messages = crate::messaging::Messages::default();
            for doc in state.docs.iter_mut().flatten() {
                doc.messages = crate::messaging::Messages::default();
            }
            state.ports.close_all();
        }
    }
}

/// The time of a task from which it counts against the document's budget
/// (the threshold of a long task in the Long Tasks API).
const LONG_TASK: Duration = Duration::from_millis(50);

/// The longest that the `storage` events of a document hold up the commit that replaces
/// it, apart from the listener that runs when the time ends.
const MAX_STORAGE_FLUSH: Duration = Duration::from_millis(100);

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
    push_message(
        &state.console,
        state.console_frame.get(),
        ConsoleLevel::Error,
        text,
    );
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
fn push_message(
    buffer: &RefCell<VecDeque<ConsoleMessage>>,
    frame: u32,
    level: ConsoleLevel,
    line: &str,
) {
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
        frame,
    });
}
