//! Error reports (HTML §8.1.4.6, "report an exception",
//! <https://html.spec.whatwg.org/multipage/webappapis.html#report-an-exception>,
//! and §8.1.4.7, "notify about rejected promises").
//!
//! An uncaught exception of a script, a timer, a listener or a microtask
//! fires a cancelable `ErrorEvent` named `error` at the window; only if no
//! listener canceled it, a console line `Uncaught Name: message` follows
//! (Chromium's text, which is also the event's `message`). While the window
//! is in error reporting mode (an `error` listener threw), the report goes
//! only to the console. A promise that was rejected without a handler fires
//! a cancelable `PromiseRejectionEvent` named `unhandledrejection`, and the
//! console line `Uncaught (in promise) ...` if no listener canceled it.
//!
//! The position (`filename`, `lineno`, `colno`) is where an error object
//! was created (the first frame of its stack, as Chromium reports it), else
//! where the value was thrown, else the place of a compile error. Lines and
//! columns of an inline script of the parser count from the start of the
//! document, as in Chromium (the script's start position goes to `js`
//! with its source, feature 5c).

use swb_js::{Runtime, ScriptError, SourceLocation, Value, VmError, VmResult};

use crate::events::{EventData, Extra, dispatch, new_event_object};
use crate::generated as g;
use crate::generated::types::EventInit;
use crate::state::state_mut;

/// What ran when an error happened.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Site<'a> {
    /// The name in the log: a script URL, "a listener of 'click'".
    pub(crate) name: &'a str,
    /// The URL and the source text of a script, for the position of a
    /// compile error.
    pub(crate) script: Option<(&'a str, &'a str)>,
    /// Where the source text starts in its document (line and column from
    /// 0).
    pub(crate) start: (u32, u32),
}

impl Site<'static> {
    /// A microtask.
    pub(crate) const JOB: Site<'static> = Site {
        name: "a microtask",
        script: None,
        start: (0, 0),
    };
    /// The function of a timer.
    pub(crate) const TIMER: Site<'static> = Site {
        name: "a timer",
        script: None,
        start: (0, 0),
    };
}

impl<'a> Site<'a> {
    /// A classic script of `url` with its source text.
    pub(crate) fn script(url: &'a str, source: &'a str) -> Site<'a> {
        Site::script_at(url, source, (0, 0))
    }

    /// A classic script whose text starts at `start` (line and column from
    /// 0) in its document: an inline script of the parser.
    pub(crate) fn script_at(url: &'a str, source: &'a str, start: (u32, u32)) -> Site<'a> {
        Site {
            name: url,
            script: Some((url, source)),
            start,
        }
    }

    /// Something that has a name but no source.
    pub(crate) fn named(name: &'a str) -> Site<'a> {
        Site {
            name,
            script: None,
            start: (0, 0),
        }
    }
}

/// Reports an error of a run at `site`: an exception or a compile error is
/// reported as HTML "report an exception"; a termination or an engine error
/// is the task's to report (`runtime.rs`). A termination of an `error`
/// listener goes up.
pub(crate) fn report(rt: &mut Runtime, site: &Site<'_>, error: &ScriptError) -> VmResult<()> {
    match error {
        ScriptError::Uncaught { .. } => {
            // Both are taken before the text is made: the serializer is a run.
            let value = rt.last_value();
            let location = rt.error_location(value).or_else(|| rt.throw_location());
            crate::runtime::pin(rt, value);
            let message = format!("Uncaught {}", crate::runtime::thrown_text(rt, error));
            let result = report_exception(rt, site.name, value, &message, location);
            crate::runtime::unpin(rt);
            result
        }
        ScriptError::Compile {
            kind,
            message,
            offset,
        } => {
            let value = Value::Object(rt.new_error(*kind, message)?);
            crate::runtime::pin(rt, value);
            let message = format!("Uncaught {}: {message}", kind.name());
            let location = site.script.map(|(url, source)| {
                let (line, column) = Runtime::source_location(source, *offset);
                let (start_line, start_column) = site.start;
                SourceLocation {
                    name: url.to_owned(),
                    line: line.saturating_add(start_line),
                    column: if line == 1 {
                        column.saturating_add(start_column)
                    } else {
                        column
                    },
                }
            });
            let result = report_exception(rt, site.name, value, &message, location);
            crate::runtime::unpin(rt);
            result
        }
        ScriptError::Terminated(_) | ScriptError::Internal(_) => Ok(()),
    }
}

/// The resize observer loop error (Resize Observer §3.4 "deliver resize
/// loop error notification"): an `ErrorEvent` at the window with this
/// message, no error object, the document's URL as the file and the
/// position 0:0 (measured in Chromium 148); a console line if no listener
/// canceled it.
pub(crate) fn report_resize_loop_error(rt: &mut Runtime) -> VmResult<()> {
    let message = "ResizeObserver loop completed with undelivered notifications.";
    let location = SourceLocation {
        name: crate::scripts::document_url(rt),
        line: 0,
        column: 0,
    };
    report_exception(
        rt,
        "a resize observer",
        Value::Null,
        message,
        Some(location),
    )
}

/// Reports a thrown value that did not come from a run (`reportError`, the
/// lookup of a listener's `handleEvent`).
pub(crate) fn report_value(rt: &mut Runtime, value: Value) -> VmResult<()> {
    crate::runtime::pin(rt, value);
    let message = format!("Uncaught {}", crate::runtime::thrown_value_text(rt, value));
    let location = rt.error_location(value);
    let result = report_exception(rt, "reportError", value, &message, location);
    crate::runtime::unpin(rt);
    result
}

/// HTML "report an exception" with the extracted error information.
fn report_exception(
    rt: &mut Runtime,
    name: &str,
    value: Value,
    message: &str,
    location: Option<SourceLocation>,
) -> VmResult<()> {
    let event_loop = &mut state_mut(rt)?.event_loop;
    let mut not_handled = true;
    if !event_loop.error_reporting {
        event_loop.error_reporting = true;
        // Chromium runs the microtasks of `error` listeners after the
        // report (measured), not after each listener.
        let checkpoints = std::mem::replace(&mut event_loop.listener_checkpoints, false);
        let fired = fire_error(rt, message, location, value);
        let event_loop = &mut state_mut(rt)?.event_loop;
        event_loop.error_reporting = false;
        event_loop.listener_checkpoints = checkpoints;
        not_handled = fired?;
    }
    if not_handled {
        crate::runtime::report_line(rt, name, message);
    }
    Ok(())
}

/// Fires the `error` event at the window; false if a listener canceled it.
fn fire_error(
    rt: &mut Runtime,
    message: &str,
    location: Option<SourceLocation>,
    error: Value,
) -> VmResult<bool> {
    let location = location.unwrap_or(SourceLocation {
        name: String::new(),
        line: 0,
        column: 0,
    });
    let extra = Extra::Error {
        message: message.to_owned(),
        filename: location.name,
        lineno: location.line,
        colno: location.column,
        error,
    };
    fire_at_window(rt, "error", g::ERROR_EVENT, extra)
}

/// Fires `unhandledrejection` for a promise; a console line if no listener
/// canceled it.
pub(crate) fn report_rejection(rt: &mut Runtime, promise: Value, reason: Value) -> VmResult<()> {
    let Value::Object(object) = promise else {
        return Ok(());
    };
    let extra = Extra::PromiseRejection {
        promise: object,
        reason,
    };
    let event_loop = &mut state_mut(rt)?.event_loop;
    // A task that the engine runs while no script runs: microtasks run
    // after each listener (HTML "clean up after running a callback").
    let checkpoints = std::mem::replace(&mut event_loop.listener_checkpoints, true);
    let fired = fire_at_window(rt, "unhandledrejection", g::PROMISE_REJECTION_EVENT, extra);
    state_mut(rt)?.event_loop.listener_checkpoints = checkpoints;
    if fired? {
        let text = format!(
            "Uncaught (in promise) {}",
            crate::runtime::thrown_value_text(rt, reason)
        );
        crate::runtime::report_line(rt, "a promise", &text);
    }
    Ok(())
}

/// Fires a trusted, cancelable event that does not bubble at the window;
/// false if a listener canceled it.
fn fire_at_window(
    rt: &mut Runtime,
    kind: &str,
    class: swb_js::ClassId,
    extra: Extra,
) -> VmResult<bool> {
    let init = EventInit {
        bubbles: false,
        cancelable: true,
        composed: false,
    };
    let mut data = EventData::new(kind.to_owned(), &init, crate::events::now(rt), extra);
    data.is_trusted = true;
    let scope = rt.heap_mut().open_scope();
    let result = (|| {
        let event = new_event_object(rt, 0, class, data)?;
        let window = rt.global(0)?;
        dispatch(rt, 0, window, event, None)
    })();
    if let Err(error) = rt.heap_mut().close_scope(scope) {
        log::warn!("the handle scope of an error report was not the last one: {error:?}");
    }
    match result {
        Err(VmError::Throw(_) | VmError::Raise { .. }) => Ok(true),
        other => other,
    }
}
