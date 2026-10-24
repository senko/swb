//! The timers of a window (HTML §8.7, <https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#timers>):
//! `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`, and
//! `queueMicrotask` (§8.8) and `reportError` of `WindowOrWorkerGlobalScope`.
//!
//! A timer has an id from the map of ids (from 1, shared by both kinds, one
//! more per call as in Chromium) and a unique handle, the sequence number of
//! its task in the queue; `clearTimeout` removes both, so that a cleared
//! timer costs nothing. The timeout is clamped to 4 ms when the nesting
//! level of the running task is greater than 5 (the HTML rule; Chromium
//! measured the same: the 7th timer of a chain of `setTimeout(f, 0)` is the
//! first that waits 4 ms). Chromium keeps the nesting level of a timer task
//! in its microtasks, so a timer scheduled from a promise reaction of a
//! timer callback nests too (measured); HTML would reset it.
//!
//! An interval keeps the grid of its first due time: its next run is due at
//! the first beat of the grid after the start of its task, so a slow
//! callback skips the beats it missed and does not shift the later ones.
//! HTML schedules the next run from the end of the callback; Chromium keeps
//! the grid (measured: a 20 ms interval whose first callback runs 30 ms
//! fires at 20, 50, 60, 80; one whose first callback runs 100 ms at 20,
//! 120, 140, 160).
//!
//! A string handler is compiled and run as a classic script named after
//! the document's URL when the timer fires. Its exceptions, and those of a
//! function handler, are reported (an `error` event at the window).
//!
//! Limits: at most [`MAX_TIMER_UNITS`] timers and extra arguments and
//! [`MAX_CODE_BYTES`] of string handlers per document; one more ends the
//! task (the timers live outside the JavaScript heap, ADR 0028 section
//! 16).

use std::collections::HashMap;
use std::rc::Rc;

use swb_js::{Gc, Object, Runtime, ScriptError, Tracer, Value, VmError, VmResult};

use crate::bindings::{self as b, Cx, Platform};
use crate::event_loop::{Task, TaskKey};
use crate::generated::types::{DomStringOrFunction, VoidFunction};
use crate::state::{state, state_mut};

/// The most timers and extra arguments of the active timers of a
/// document (a timer counts one, each extra argument one more).
pub(crate) const MAX_TIMER_UNITS: usize = 1 << 20;

/// The most bytes of the string handlers of the active timers of a
/// document (each timer keeps its own copy of the code).
pub(crate) const MAX_CODE_BYTES: usize = 32 << 20;

/// The nesting level above which a timeout below 4 ms becomes 4 ms.
const NESTING_THRESHOLD: u32 = 5;

/// The smallest timeout of a nested timer, in milliseconds.
const NESTED_MINIMUM_MS: f64 = 4.0;

/// The termination reason at the limit.
const LIMIT_REASON: &str = "the timer limit of the document";

/// What a timer runs.
#[derive(Clone, Debug)]
pub(crate) enum Handler {
    Function(Gc<Object>),
    Code(Rc<str>),
}

impl Handler {
    /// The bytes of a string handler (0 for a function).
    fn code_bytes(&self) -> usize {
        match self {
            Handler::Function(_) => 0,
            Handler::Code(code) => code.len(),
        }
    }
}

/// The previous run of an interval that repeats.
#[derive(Clone, Copy, Debug)]
struct Repeat {
    id: i32,
    /// The page time when the run was due.
    due: f64,
    /// The page time when its task started.
    started: f64,
}

/// An active timer.
#[derive(Debug)]
struct Timer {
    handler: Handler,
    arguments: Vec<Value>,
    /// The timeout as converted (before the clamp).
    timeout: i32,
    repeat: bool,
    /// The timer nesting level of its task.
    nesting: u32,
    /// The key of its task: the sequence number is the unique handle.
    key: TaskKey,
}

/// The map of the timers of a window.
#[derive(Debug, Default)]
pub(crate) struct Timers {
    active: HashMap<i32, Timer>,
    /// The last id given.
    last_id: i32,
    /// The timers and extra arguments of the active timers.
    units: usize,
    /// The bytes of the string handlers of the active timers.
    code_bytes: usize,
}

impl Timers {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for timer in self.active.values() {
            if let Handler::Function(f) = &timer.handler {
                tracer.object(*f);
            }
            for value in &timer.arguments {
                tracer.value(*value);
            }
        }
    }

    /// The number of active timers.
    pub(crate) fn len(&self) -> usize {
        self.active.len()
    }

    /// A new id: one more than the last, skipping ids in use.
    fn new_id(&mut self) -> i32 {
        loop {
            self.last_id = if self.last_id == i32::MAX {
                1
            } else {
                self.last_id + 1
            };
            if !self.active.contains_key(&self.last_id) {
                return self.last_id;
            }
        }
    }

    /// Removes a timer; returns the key of its task.
    fn remove(&mut self, id: i32) -> Option<TaskKey> {
        let timer = self.active.remove(&id)?;
        self.units = self.units.saturating_sub(1 + timer.arguments.len());
        self.code_bytes = self.code_bytes.saturating_sub(timer.handler.code_bytes());
        Some(timer.key)
    }
}

/// The due time of the next run of an interval of `ms` milliseconds after
/// the run `previous`: the first beat of its grid after the start of the
/// run's task (see the module comment).
fn next_beat(previous: Repeat, ms: f64) -> f64 {
    let missed = ((previous.started - previous.due) / ms).floor().max(0.0);
    previous.due + ms * (missed + 1.0)
}

/// HTML "timer initialization steps". `previous` is the run of an
/// interval that repeats.
fn initialize(
    rt: &mut Runtime,
    handler: Handler,
    timeout: i32,
    arguments: Vec<Value>,
    repeat: bool,
    previous: Option<Repeat>,
) -> VmResult<i32> {
    let now = crate::event_loop::page_now(rt);
    let state = state_mut(rt)?;
    let units = 1 + arguments.len();
    let code_bytes = handler.code_bytes();
    let timers = &state.event_loop.timers;
    if timers.units + units > MAX_TIMER_UNITS {
        log::warn!(
            "the timers of the document reached the limit of {MAX_TIMER_UNITS}; the task ends"
        );
        return Err(rt.terminate(LIMIT_REASON));
    }
    if timers.code_bytes + code_bytes > MAX_CODE_BYTES {
        log::warn!(
            "the string handlers of the timers of the document reached the limit of {MAX_CODE_BYTES} bytes; the task ends"
        );
        return Err(rt.terminate(LIMIT_REASON));
    }
    let event_loop = &mut state.event_loop;
    let nesting = event_loop.nesting;
    let mut ms = f64::from(timeout.max(0));
    if nesting > NESTING_THRESHOLD && ms < NESTED_MINIMUM_MS {
        ms = NESTED_MINIMUM_MS;
    }
    let due = match previous {
        Some(previous) if ms > 0.0 => next_beat(previous, ms),
        _ => now + ms,
    };
    let id = match previous {
        Some(previous) => previous.id,
        None => event_loop.timers.new_id(),
    };
    let handle = event_loop.tasks.next_seq();
    let key = event_loop.tasks.push(due, Task::Timer { id, handle });
    event_loop.timers.units += units;
    event_loop.timers.code_bytes += code_bytes;
    event_loop.timers.active.insert(
        id,
        Timer {
            handler,
            arguments,
            timeout,
            repeat,
            nesting: nesting.saturating_add(1),
            key,
        },
    );
    Ok(id)
}

/// `clearTimeout` and `clearInterval`.
fn clear(rt: &mut Runtime, id: i32) -> VmResult<()> {
    let event_loop = &mut state_mut(rt)?.event_loop;
    if let Some(key) = event_loop.timers.remove(id) {
        event_loop.tasks.remove(key);
    }
    Ok(())
}

/// The handler of a `TimerHandler` value.
fn handler_of(value: DomStringOrFunction) -> Handler {
    match value {
        DomStringOrFunction::Function(f) => Handler::Function(f.0),
        DomStringOrFunction::DomString(code) => Handler::Code(code.into()),
    }
}

/// Runs the task of a timer (the steps of the task in "timer
/// initialization steps"). An exception of the handler is reported; a
/// termination or an engine error is returned.
pub(crate) fn run_timer(rt: &mut Runtime, id: i32, handle: u64) -> Result<(), ScriptError> {
    let started = crate::event_loop::page_now(rt);
    let (handler, arguments, timeout, repeat, nesting, due) = {
        let state = state(rt).map_err(crate::runtime::task_error)?;
        match state.event_loop.timers.active.get(&id) {
            Some(timer) if timer.key.1 == handle => (
                timer.handler.clone(),
                timer.arguments.clone(),
                timer.timeout,
                timer.repeat,
                timer.nesting,
                timer.key.0.0,
            ),
            _ => return Ok(()),
        }
    };
    // Held in the task's scope: a callback can clear its own timer.
    if let Handler::Function(f) = &handler {
        b::keep(rt, Value::Object(*f));
    }
    for &value in &arguments {
        b::keep(rt, value);
    }
    // The task already left the queue, so the timer's key names no task.
    state_mut(rt)
        .map_err(crate::runtime::task_error)?
        .event_loop
        .nesting = nesting;
    let url = crate::scripts::document_url(rt);
    let result = match &handler {
        Handler::Function(f) => {
            let window = rt.global(0).map_err(crate::runtime::task_error)?;
            rt.call_function(Value::Object(*f), window.into(), &arguments)
                .map(|_| ())
        }
        Handler::Code(code) => rt.eval_named(code, &url).map(|_| ()),
    };
    if let Err(error) = &result {
        let site = match &handler {
            Handler::Function(_) => crate::errors::Site::TIMER,
            Handler::Code(code) => crate::errors::Site {
                name: "a timer",
                script: Some((url.as_str(), &**code)),
                start: (0, 0),
            },
        };
        match error {
            ScriptError::Terminated(_) | ScriptError::Internal(_) => {
                clear(rt, id).map_err(crate::runtime::task_error)?;
                return result;
            }
            _ => crate::errors::report(rt, &site, error).map_err(crate::runtime::task_error)?,
        }
    }
    // HTML "clean up after running script": the microtasks of the
    // callback run before the timer repeats.
    crate::event_loop::microtask_checkpoint(rt)?;
    let still_active = state(rt)
        .map_err(crate::runtime::task_error)?
        .event_loop
        .timers
        .active
        .get(&id)
        .is_some_and(|t| t.key.1 == handle);
    if !still_active {
        return Ok(());
    }
    clear(rt, id).map_err(crate::runtime::task_error)?;
    if repeat {
        let previous = Repeat { id, due, started };
        initialize(rt, handler, timeout, arguments, true, Some(previous))
            .map_err(crate::runtime::task_error)?;
    }
    Ok(())
}

/// `setTimeout`.
pub(crate) fn set_timeout(
    cx: &mut Cx<'_>,
    handler: DomStringOrFunction,
    timeout: i32,
    arguments: Vec<Value>,
) -> b::Result<i32> {
    Ok(initialize(
        cx.rt,
        handler_of(handler),
        timeout,
        arguments,
        false,
        None,
    )?)
}

/// `setInterval`.
pub(crate) fn set_interval(
    cx: &mut Cx<'_>,
    handler: DomStringOrFunction,
    timeout: i32,
    arguments: Vec<Value>,
) -> b::Result<i32> {
    Ok(initialize(
        cx.rt,
        handler_of(handler),
        timeout,
        arguments,
        true,
        None,
    )?)
}

/// `clearTimeout` and `clearInterval`.
pub(crate) fn clear_timer(cx: &mut Cx<'_>, id: i32) -> b::Result<()> {
    Ok(clear(cx.rt, id)?)
}

/// `queueMicrotask`.
pub(crate) fn queue_microtask(cx: &mut Cx<'_>, callback: VoidFunction) {
    cx.rt.queue_microtask(Value::Object(callback.0));
}

/// `reportError(e)`: HTML "report an exception" for `e`.
pub(crate) fn report_error(cx: &mut Cx<'_>, _this: Platform, e: Value) -> b::Result<()> {
    let result: Result<(), VmError> = crate::errors::report_value(cx.rt, e);
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The run times of a 20 ms interval whose first callback runs `busy`
    /// milliseconds (the others take no time).
    fn runs(busy: f64) -> Vec<f64> {
        let mut due = 20.0;
        let mut started = 20.0;
        let mut runs = vec![started];
        for run in 0..4 {
            let end = started + if run == 0 { busy } else { 0.0 };
            due = next_beat(
                Repeat {
                    id: 1,
                    due,
                    started,
                },
                20.0,
            );
            started = due.max(end);
            runs.push(started);
        }
        runs
    }

    /// Chromium's run times (measured with the probe tool).
    #[test]
    fn an_interval_keeps_its_grid_after_a_slow_callback() {
        assert_eq!(runs(0.0), [20.0, 40.0, 60.0, 80.0, 100.0]);
        assert_eq!(runs(30.0), [20.0, 50.0, 60.0, 80.0, 100.0]);
        assert_eq!(runs(45.0), [20.0, 65.0, 80.0, 100.0, 120.0]);
        assert_eq!(runs(65.0), [20.0, 85.0, 100.0, 120.0, 140.0]);
        assert_eq!(runs(100.5), [20.0, 120.5, 140.0, 160.0, 180.0]);
    }
}
