//! The event loop of an agent (HTML §8.1.7,
//! <https://html.spec.whatwg.org/multipage/webappapis.html#event-loops>;
//! ADR 0028 section 7): the task queue, the microtask checkpoint, the
//! notification of rejected promises and the cleanup of finalization
//! registries. The timers are in `timers.rs`, the error reports in
//! `errors.rs`, and the runtime runs the tasks (`runtime.rs`). A task belongs
//! to a document: it runs with that document as the current one.
//!
//! The queue orders every task by its due time in page time and then by
//! the order of queuing. A timer is due when its timeout has passed; any
//! other task is due when it is queued. So a task that is queued at time
//! `t` runs after the timers that were due at `t` and were scheduled
//! before it, which is the order that Chromium shows (measured: the
//! `unhandledrejection` notification of a task runs after the `setTimeout(0)`
//! timers that the task scheduled, and before a `setTimeout(1)` timer).
//!
//! The microtask checkpoint runs the job queue to the end; an exception of
//! a job is reported and the next job runs. At its end the promises that
//! were rejected without a handler are noted, and one task fires
//! `unhandledrejection` for them (HTML "notify about rejected promises").
//! The checkpoint also runs after each listener of an event that the
//! engine fires while no script runs (HTML "clean up after running a
//! callback"); not for the `error` event of an exception report, where
//! Chromium runs the microtasks only after all listeners (measured).

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use swb_js::{Runtime, ScriptError, Tracer, Value, VmError, VmResult};

use crate::state::{DocKey, state, state_mut};

/// A task of the event loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Task {
    /// A timer of `setTimeout` or `setInterval`: its id and its unique
    /// handle (the sequence number of its queue entry).
    Timer { id: i32, handle: u64 },
    /// Fire `unhandledrejection` for the noted promises.
    NotifyRejections,
    /// Give the entries that were queued to the `PerformanceObserver`
    /// objects (HTML "queue a `PerformanceObserver` task").
    PerformanceObservers,
    /// Give the queued entries to the `IntersectionObserver` objects (HTML
    /// "queue an intersection observer task").
    IntersectionObservers,
    /// The `timeout` of an idle callback is over: run it if it still
    /// waits (`requestIdleCallback`).
    IdleTimeout(u32),
    /// Run the cleanup of a finalization registry.
    FinalizationCleanup,
    /// The time of an `AbortSignal.timeout()` signal is over (`abort.rs`).
    AbortTimeout(u64),
    /// The `timeout` of an `XMLHttpRequest` is over (`xhr.rs`): the request
    /// id.
    XhrTimeout(u64),
    /// A request that did not go to the network (a Blob URL, a refused
    /// request) completes (`fetch/mod.rs`): its id.
    LocalFetch(u64),
    /// A step of a `FileReader` read (`file_api/reader.rs`): `finish` false
    /// fires `loadstart`, true the rest.
    FileReader {
        id: u64,
        generation: u64,
        finish: bool,
    },
    /// Let the parser go on after a wait (`document_load.rs`).
    ParserResume,
    /// Fire the waiting `select` and `selectionchange` events of text
    /// controls.
    SelectionEvents,
    /// Fire `error` at a script element whose `src` is empty or not a URL
    /// (HTML "queue an element task").
    ScriptErrorEvent(swb_dom::NodeId),
    /// The fetch of a script element's script failed at once.
    ScriptFetchFailed(swb_dom::NodeId),
    /// The module fetch of a `link rel=modulepreload` ended: fire `load` or
    /// `error` (`error` true) at the element (`scripts/preload.rs`).
    LinkEvent {
        /// The `link` element.
        node: swb_dom::NodeId,
        /// True for `error`.
        error: bool,
    },
    /// The module script of an element is ready without a fetch: an inline
    /// module, or a URL that the module map already has (HTML "fetch an
    /// inline module script graph", "fetch a single module script").
    ModuleReady(swb_dom::NodeId),
    /// An `import()` of a module that the module map has gets its answer
    /// (the engine's token; `scripts/module_hooks.rs`).
    ModuleImport(u32),
    /// Run the script of a `javascript:` URL that a link or a form
    /// navigates to (HTML "navigate to a javascript: URL" queues a task).
    JavascriptUrl(String),
    /// Fire `hashchange` at the window after a navigation to a fragment
    /// (HTML "navigate to a fragment" queues a global task).
    HashChange {
        /// `oldURL`.
        old_url: String,
        /// `newURL`.
        new_url: String,
    },
    /// Deliver the first message that waits for the window of the task's document
    /// (the posted message task source, `messaging.rs`).
    Message,
    /// Deliver the next message of the `MessagePort` with this id (the posted
    /// message task source of a port, `message_port.rs`).
    PortMessage(u64),
    /// Fire the first `storage` event that waits for the window of the task's document
    /// (`storage_events.rs`).
    Storage,
}

/// A due time in page time (milliseconds), ordered as a number.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Due(pub(crate) f64);

impl PartialEq for Due {
    fn eq(&self, other: &Due) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Due {}

impl PartialOrd for Due {
    fn partial_cmp(&self, other: &Due) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Due {
    fn cmp(&self, other: &Due) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// The key of a task in the queue: the due time, then the sequence number.
pub(crate) type TaskKey = (Due, u64);

/// The tasks of an agent, by due time and order of queuing. Each task
/// belongs to the document that was the current one when it was queued.
#[derive(Debug, Default)]
pub(crate) struct TaskQueue {
    queue: BTreeMap<TaskKey, (DocKey, Task)>,
    /// The sequence counter, which the runtimes of a page share (ADR 0031
    /// section 6): tasks of different runtimes run in the order of queuing.
    next_seq: Rc<Cell<u64>>,
    /// The current document (the binding state keeps it up to date).
    document: DocKey,
}

impl TaskQueue {
    /// Sets the document of the tasks that are queued from now on.
    pub(crate) fn set_document(&mut self, document: DocKey) {
        self.document = document;
    }

    /// Shares the sequence counter of the page.
    pub(crate) fn set_counter(&mut self, counter: Rc<Cell<u64>>) {
        self.next_seq = counter;
    }

    /// The sequence number that the next task gets.
    pub(crate) fn next_seq(&self) -> u64 {
        self.next_seq.get()
    }

    /// Takes a sequence number from the counter of the page.
    pub(crate) fn take_seq(&mut self) -> u64 {
        let seq = self.next_seq.get();
        self.next_seq.set(seq + 1);
        seq
    }

    /// Queues `task` to run at page time `due`; returns its key.
    pub(crate) fn push(&mut self, due: f64, task: Task) -> TaskKey {
        let seq = self.take_seq();
        let key = (Due(due), seq);
        self.queue.insert(key, (self.document, task));
        key
    }

    /// Queues `task` of the document `document` at the place that the due time `due` and
    /// the number `seq` ([`TaskQueue::take_seq`]) give.
    pub(crate) fn push_for(&mut self, document: DocKey, due: f64, seq: u64, task: Task) {
        self.queue.insert((Due(due), seq), (document, task));
    }

    /// Removes the task of `key` (a cleared timer).
    pub(crate) fn remove(&mut self, key: TaskKey) {
        self.queue.remove(&key);
    }

    /// The due time of the first task.
    pub(crate) fn next_due(&self) -> Option<f64> {
        self.queue.first_key_value().map(|((due, _), _)| due.0)
    }

    /// The key of the first task: its due time and its sequence number.
    pub(crate) fn next_key(&self) -> Option<(f64, u64)> {
        self.queue
            .first_key_value()
            .map(|((due, seq), _)| (due.0, *seq))
    }

    /// Takes the first task if it is due at `now` and comes before `before`
    /// (the first task of another runtime), with its document.
    pub(crate) fn pop_due(
        &mut self,
        now: f64,
        before: Option<(f64, u64)>,
    ) -> Option<(DocKey, Task)> {
        let entry = self.queue.first_entry()?;
        let (due, seq) = *entry.key();
        if due.0 > now {
            return None;
        }
        if let Some((other_due, other_seq)) = before
            && (due, seq) > (Due(other_due), other_seq)
        {
            return None;
        }
        Some(entry.remove())
    }
}

/// The state of the event loop in the binding state.
#[derive(Debug, Default)]
pub(crate) struct EventLoop {
    pub(crate) tasks: TaskQueue,
    /// The timer nesting level of the running task: the level of a timer
    /// task while it and its microtasks run (Chromium keeps it in the
    /// microtasks; measured), 0 for other tasks.
    pub(crate) nesting: u32,
    /// HTML "performing a microtask checkpoint".
    pub(crate) performing_checkpoint: bool,
    /// True while the engine dispatches an event: a checkpoint runs after
    /// each listener.
    pub(crate) listener_checkpoints: bool,
    /// HTML "about-to-be-notified rejected promises list".
    pub(crate) about_to_notify: Vec<(Value, u32)>,
    /// True while a [`Task::NotifyRejections`] waits in the queue.
    pub(crate) notify_queued: bool,
    /// True while a [`Task::FinalizationCleanup`] waits in the queue.
    pub(crate) cleanup_queued: bool,
    /// The promises of evaluations that wait for their settlement.
    pub(crate) evaluations: Vec<(u64, Value)>,
    pub(crate) next_evaluation: u64,
}

impl EventLoop {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for (value, _) in &self.about_to_notify {
            tracer.value(*value);
        }
        for (_, value) in &self.evaluations {
            tracer.value(*value);
        }
    }
}

/// The page time now, from the document's clock.
pub(crate) fn page_now(rt: &Runtime) -> f64 {
    state(rt).map_or(0.0, |s| s.doc.clock.page_now())
}

/// Queues a task that is due now.
pub(crate) fn queue_task(rt: &mut Runtime, task: Task) -> VmResult<()> {
    let now = page_now(rt);
    state_mut(rt)?.event_loop.tasks.push(now, task);
    Ok(())
}

/// HTML "perform a microtask checkpoint": runs the jobs to the end,
/// reports their exceptions, and notes the rejected promises without a
/// handler. Does nothing while script runs (a nested checkpoint) or while
/// a checkpoint runs. A termination drops the remaining jobs and is
/// returned.
pub(crate) fn microtask_checkpoint(rt: &mut Runtime) -> Result<(), ScriptError> {
    if rt.stack_depths().0 > 0 {
        return Ok(());
    }
    let Ok(state) = state_mut(rt) else {
        return Ok(());
    };
    if state.event_loop.performing_checkpoint {
        return Ok(());
    }
    state.event_loop.performing_checkpoint = true;
    // Records that the engine queued while no script ran (the parser).
    let result = match crate::mutation_observer::after_call(rt) {
        Ok(()) => run_jobs(rt),
        Err(error) => {
            // As in `run_jobs`: the queued jobs (also the mutation observer
            // microtask that `after_call` queued before the time check)
            // would run in the next task without its time.
            rt.clear_jobs();
            crate::mutation_observer::jobs_cleared(rt);
            Err(crate::runtime::task_error(error))
        }
    };
    if let Ok(state) = state_mut(rt) {
        state.event_loop.performing_checkpoint = false;
    }
    result?;
    note_rejections(rt).map_err(crate::runtime::task_error)
}

/// Runs the jobs until the queue is empty; reports each exception.
fn run_jobs(rt: &mut Runtime) -> Result<(), ScriptError> {
    loop {
        match rt.run_jobs() {
            Ok(()) => return Ok(()),
            Err(error @ (ScriptError::Terminated(_) | ScriptError::Internal(_))) => {
                // The jobs that are left would run in the next task without
                // its time.
                rt.clear_jobs();
                crate::mutation_observer::jobs_cleared(rt);
                return Err(error);
            }
            Err(error) => {
                if let Err(error) = report_job_error(rt, &error) {
                    rt.clear_jobs();
                    crate::mutation_observer::jobs_cleared(rt);
                    return Err(crate::runtime::task_error(error));
                }
            }
        }
    }
}

/// Reports the exception of a microtask at the window of its function.
fn report_job_error(rt: &mut Runtime, error: &ScriptError) -> VmResult<()> {
    let entered = match rt.take_failed_job_realm() {
        Some(realm) => crate::state::enter_realm(rt, realm)?,
        None => crate::state::NO_SWITCH,
    };
    let result = crate::errors::report(rt, &crate::errors::Site::JOB, error);
    crate::state::leave(rt, entered);
    result
}

/// The end of a checkpoint: notes the promises that the tracker reported
/// and queues the task that notifies about them.
fn note_rejections(rt: &mut Runtime) -> VmResult<()> {
    let rejected = rt.take_unhandled_rejections_by_realm();
    if rejected.is_empty() {
        return Ok(());
    }
    let now = page_now(rt);
    let event_loop = &mut state_mut(rt)?.event_loop;
    event_loop.about_to_notify.extend(rejected);
    if !event_loop.notify_queued {
        event_loop.notify_queued = true;
        event_loop.tasks.push(now, Task::NotifyRejections);
    }
    Ok(())
}

/// The checkpoint after a listener that the engine called (HTML "clean up
/// after running a callback"): only while the engine dispatches and no
/// script runs. A termination goes up.
pub(crate) fn after_listener(rt: &mut Runtime) -> VmResult<()> {
    if !state(rt)?.event_loop.listener_checkpoints {
        return Ok(());
    }
    match microtask_checkpoint(rt) {
        Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
        _ => Ok(()),
    }
}

/// Queues a cleanup task if a finalization registry waits for one (HTML
/// `HostEnqueueFinalizationRegistryCleanupJob`: a task on the JavaScript
/// engine task source).
pub(crate) fn queue_finalization_cleanup(rt: &mut Runtime) -> VmResult<()> {
    if rt.pending_finalization_cleanups() == 0 || state(rt)?.event_loop.cleanup_queued {
        return Ok(());
    }
    state_mut(rt)?.event_loop.cleanup_queued = true;
    queue_task(rt, Task::FinalizationCleanup)
}

/// Puts the rejected promises of each window together, in the order of their
/// rejection (HTML has one list per window; measured, case
/// `errors-promises-grouped`). The windows come in the order of their first
/// rejection; their order in Chromium varies from run to run.
fn group_by_window(list: &mut [(Value, u32)]) {
    let mut rank: HashMap<u32, usize> = HashMap::new();
    for &(_, realm) in list.iter() {
        let next = rank.len();
        rank.entry(realm).or_insert(next);
    }
    list.sort_by_key(|(_, realm)| rank.get(realm).copied());
}

/// Runs the task that notifies about rejected promises: an
/// `unhandledrejection` event per promise that still has no handler, and a
/// console line if no listener canceled it.
pub(crate) fn notify_rejections(rt: &mut Runtime) -> VmResult<()> {
    let event_loop = &mut state_mut(rt)?.event_loop;
    event_loop.notify_queued = false;
    let mut list = std::mem::take(&mut event_loop.about_to_notify);
    if rt.host_flag() {
        group_by_window(&mut list);
    }
    // The list is no longer traced: keep the promises in the task's scope.
    for &(promise, _) in &list {
        crate::bindings::keep(rt, promise);
    }
    for (promise, realm) in list {
        if rt.promise_is_handled(promise) != Some(false) {
            continue;
        }
        let reason = rt
            .promise_settlement(promise)
            .map_or(Value::Undefined, |(_, reason)| reason);
        let entered = crate::state::enter_realm(rt, realm)?;
        let result = crate::errors::report_rejection(rt, promise, reason);
        crate::state::leave(rt, entered);
        result?;
    }
    Ok(())
}
