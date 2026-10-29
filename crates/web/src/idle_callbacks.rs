//! `requestIdleCallback`, `cancelIdleCallback` and `IdleDeadline`
//! (Cooperative Scheduling of Background Tasks,
//! <https://w3c.github.io/requestidlecallback/>; ADR 0028 section 7, step 4
//! of the event loop; M8 feature 8a2).
//!
//! The callbacks wait in the window's map by handle. The engine starts an
//! idle period when no task is due and no frame is due
//! ([`crate::ScriptRuntime::run_idle_callbacks`]): the callbacks that wait
//! then run, each as a task, with an `IdleDeadline` whose deadline is the
//! end of the period (at most 50 ms away, and not past the next task or
//! frame). A callback that the period's callbacks request waits for the next
//! period. A request with `timeout` also queues a task: if the callback did
//! not run when the task is due, the task runs it with `didTimeout` true and
//! `timeRemaining()` 0. Measured in Chromium 148
//! (`tools/probes/web-idle.json`): handles count from 1; the callbacks of a
//! period run in the order of their handles after the timers and frames
//! that are due; an exception of a callback is reported and the next one
//! runs; a loop of `requestIdleCallback(f)` in `f` runs 20 times a second
//! with idle time; `timeout` becomes an unsigned long, so `-1` is 4294967295
//! ms and `"x"` is 0.
//!
//! In virtual time, `timeRemaining()` charges 0.1 ms of page time at each
//! call (and never passes the deadline), so a loop that waits for it to
//! reach 0 ends after at most 500 calls, and the run is the same each time.

use std::collections::BTreeMap;

use swb_js::{Gc, GenericData, HostData, Object, Runtime, ScriptError, Tracer, Value};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::event_loop::{Task, TaskKey};
use crate::generated as g;
use crate::generated::types::IdleRequestOptions;
use crate::runtime::task_error;
use crate::state::{WebState, state, state_mut};

/// The most callbacks that wait at once in a document; one more ends the
/// task.
const MAX_CALLBACKS: usize = 1 << 20;

/// The length of an idle period when nothing else is due (50 ms).
pub const IDLE_PERIOD_MS: f64 = 50.0;

/// The page time that `timeRemaining()` charges in virtual time.
const VIRTUAL_STEP_MS: f64 = 0.1;

/// A callback that waits.
#[derive(Debug)]
struct Waiting {
    callback: Gc<Object>,
    /// The task that runs it when its `timeout` is over.
    timeout_task: Option<TaskKey>,
}

/// The window's map of idle callbacks.
#[derive(Debug, Default)]
pub(crate) struct IdleCallbacks {
    last: u32,
    waiting: BTreeMap<u32, Waiting>,
}

impl IdleCallbacks {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for entry in self.waiting.values() {
            tracer.object(entry.callback);
        }
    }

    /// True if a callback waits.
    pub(crate) fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }
}

/// The host data of an `IdleDeadline`.
struct DeadlineData {
    /// The end of the period, in page time.
    deadline: f64,
    did_timeout: bool,
}

impl GenericData for DeadlineData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

fn deadline_data(rt: &Runtime, this: Platform) -> b::Result<&DeadlineData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<DeadlineData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// `requestIdleCallback(callback, options)`: the handle.
pub(crate) fn request(
    cx: &mut Cx<'_>,
    callback: Gc<Object>,
    options: &IdleRequestOptions,
) -> b::Result<u32> {
    if state(cx.rt)?.idle.waiting.len() >= MAX_CALLBACKS {
        log::warn!("a document has more than {MAX_CALLBACKS} idle callbacks");
        return Err(cx.rt.terminate("too many idle callbacks").into());
    }
    let state = state_mut(cx.rt)?;
    // Handles start at 1; 0 is never a handle, also after a wrap.
    state.idle.last = state.idle.last.wrapping_add(1).max(1);
    let handle = state.idle.last;
    let timeout_task = options.timeout.filter(|&t| t > 0).map(|timeout| {
        let due = state.clock.page_now() + f64::from(timeout);
        state.event_loop.tasks.push(due, Task::IdleTimeout(handle))
    });
    state.idle.waiting.insert(
        handle,
        Waiting {
            callback,
            timeout_task,
        },
    );
    Ok(handle)
}

/// `cancelIdleCallback(handle)`.
pub(crate) fn cancel(rt: &mut Runtime, handle: u32) -> b::Result<()> {
    let state = state_mut(rt)?;
    if let Some(entry) = state.idle.waiting.remove(&handle)
        && let Some(key) = entry.timeout_task
    {
        state.event_loop.tasks.remove(key);
    }
    Ok(())
}

/// The handles of the callbacks that wait now (the callbacks of an idle
/// period that starts now).
pub(crate) fn handles(rt: &Runtime) -> Vec<u32> {
    state(rt).map_or_else(|_| Vec::new(), |s| s.idle.waiting.keys().copied().collect())
}

/// True if the callback of `handle` still waits (an earlier callback of the
/// period can cancel it).
pub(crate) fn waits(rt: &Runtime, handle: u32) -> bool {
    state(rt).is_ok_and(|s| s.idle.waiting.contains_key(&handle))
}

/// Runs the callback of `handle` if it still waits. `deadline` is the end
/// of the idle period, or `None` for a callback whose `timeout` is over
/// (`didTimeout`, no time left). A part of a task; the caller counts
/// errors.
pub(crate) fn run(rt: &mut Runtime, handle: u32, deadline: Option<f64>) -> Result<(), ScriptError> {
    let state = state_mut(rt).map_err(task_error)?;
    let Some(entry) = state.idle.waiting.remove(&handle) else {
        return Ok(());
    };
    if let Some(key) = entry.timeout_task {
        state.event_loop.tasks.remove(key);
    }
    let now = state.clock.page_now();
    let data = DeadlineData {
        deadline: deadline.unwrap_or(now),
        did_timeout: deadline.is_none(),
    };
    let object = (|| {
        let data = rt.alloc_host_data(data)?;
        rt.new_host_object(0, g::IDLE_DEADLINE, data)
    })()
    .map_err(task_error)?;
    b::keep(rt, Value::Object(entry.callback));
    b::keep(rt, Value::Object(object));
    let result = rt
        .call_function(
            Value::Object(entry.callback),
            Value::Undefined,
            &[Value::Object(object)],
        )
        .map(|_| ());
    match result {
        Ok(()) | Err(ScriptError::Terminated(_) | ScriptError::Internal(_)) => result,
        Err(error) => {
            let site = crate::errors::Site::named("an idle callback");
            crate::errors::report(rt, &site, &error).map_err(task_error)
        }
    }
}

impl g::idle_deadline::IdleDeadlineImpl for WebState {
    fn did_timeout(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(deadline_data(cx.rt, this)?.did_timeout)
    }

    fn time_remaining(cx: &mut Cx<'_>, this: Platform) -> b::Result<f64> {
        let deadline = deadline_data(cx.rt, this)?.deadline;
        let clock = &state(cx.rt)?.clock;
        if clock.page.is_virtual() {
            // Time passes while script runs, but never past the deadline.
            let now = clock.page_now();
            clock.page.advance_to((now + VIRTUAL_STEP_MS).min(deadline));
        }
        let left = (deadline - clock.page_now()).max(0.0);
        Ok((left * 10.0).floor() / 10.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_map_is_empty() {
        assert!(IdleCallbacks::default().is_empty());
    }
}
