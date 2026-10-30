//! The event loop of a document (HTML §8.1.7,
//! <https://html.spec.whatwg.org/multipage/webappapis.html#event-loops>;
//! ADR 0028 section 7): the task queue, the microtask checkpoint, the
//! notification of rejected promises and the cleanup of finalization
//! registries. The timers are in `timers.rs`, the error reports in
//! `errors.rs`, and the runtime runs the tasks (`runtime.rs`).
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

use std::cmp::Ordering;
use std::collections::BTreeMap;

use swb_js::{Runtime, ScriptError, Tracer, Value, VmError, VmResult};

use crate::state::{state, state_mut};
use crate::timers::Timers;

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
    /// Let the parser go on after a wait (`document_load.rs`).
    ParserResume,
    /// Fire `error` at a script element whose `src` is empty or not a URL
    /// (HTML "queue an element task").
    ScriptErrorEvent(swb_dom::NodeId),
    /// The fetch of a script element's script failed at once.
    ScriptFetchFailed(swb_dom::NodeId),
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

/// The tasks of a document, by due time and order of queuing.
#[derive(Debug, Default)]
pub(crate) struct TaskQueue {
    queue: BTreeMap<TaskKey, Task>,
    next_seq: u64,
}

impl TaskQueue {
    /// The sequence number that the next task gets.
    pub(crate) fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Queues `task` to run at page time `due`; returns its key.
    pub(crate) fn push(&mut self, due: f64, task: Task) -> TaskKey {
        let key = (Due(due), self.next_seq);
        self.next_seq += 1;
        self.queue.insert(key, task);
        key
    }

    /// Removes the task of `key` (a cleared timer).
    pub(crate) fn remove(&mut self, key: TaskKey) {
        self.queue.remove(&key);
    }

    /// The due time of the first task.
    pub(crate) fn next_due(&self) -> Option<f64> {
        self.queue.first_key_value().map(|((due, _), _)| due.0)
    }

    /// Takes the first task if it is due at `now`.
    pub(crate) fn pop_due(&mut self, now: f64) -> Option<Task> {
        let entry = self.queue.first_entry()?;
        if entry.key().0.0 > now {
            return None;
        }
        Some(entry.remove())
    }

    /// The number of queued tasks.
    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }
}

/// The state of the event loop in the binding state.
#[derive(Debug, Default)]
pub(crate) struct EventLoop {
    pub(crate) tasks: TaskQueue,
    pub(crate) timers: Timers,
    /// The timer nesting level of the running task: the level of a timer
    /// task while it and its microtasks run (Chromium keeps it in the
    /// microtasks; measured), 0 for other tasks.
    pub(crate) nesting: u32,
    /// HTML "performing a microtask checkpoint".
    pub(crate) performing_checkpoint: bool,
    /// True while the engine dispatches an event: a checkpoint runs after
    /// each listener.
    pub(crate) listener_checkpoints: bool,
    /// HTML "in error reporting mode" of the window.
    pub(crate) error_reporting: bool,
    /// HTML "about-to-be-notified rejected promises list".
    pub(crate) about_to_notify: Vec<Value>,
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
        self.timers.trace(tracer);
        for value in &self.about_to_notify {
            tracer.value(*value);
        }
        for (_, value) in &self.evaluations {
            tracer.value(*value);
        }
    }
}

/// The page time now, from the document's clock.
pub(crate) fn page_now(rt: &Runtime) -> f64 {
    state(rt).map_or(0.0, |s| s.clock.page_now())
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
                if let Err(error) = crate::errors::report(rt, &crate::errors::Site::JOB, &error) {
                    rt.clear_jobs();
                    crate::mutation_observer::jobs_cleared(rt);
                    return Err(crate::runtime::task_error(error));
                }
            }
        }
    }
}

/// The end of a checkpoint: notes the promises that the tracker reported
/// and queues the task that notifies about them.
fn note_rejections(rt: &mut Runtime) -> VmResult<()> {
    let rejected = rt.take_unhandled_rejections();
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

/// Runs the task that notifies about rejected promises: an
/// `unhandledrejection` event per promise that still has no handler, and a
/// console line if no listener canceled it.
pub(crate) fn notify_rejections(rt: &mut Runtime) -> VmResult<()> {
    let event_loop = &mut state_mut(rt)?.event_loop;
    event_loop.notify_queued = false;
    let list = std::mem::take(&mut event_loop.about_to_notify);
    // The list is no longer traced: keep the promises in the task's scope.
    for &promise in &list {
        crate::bindings::keep(rt, promise);
    }
    for promise in list {
        if rt.promise_is_handled(promise) != Some(false) {
            continue;
        }
        let reason = rt
            .promise_settlement(promise)
            .map_or(Value::Undefined, |(_, reason)| reason);
        crate::errors::report_rejection(rt, promise, reason)?;
    }
    Ok(())
}
