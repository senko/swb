//! The simple dialogs (`alert()`, `confirm()`, `prompt()`; HTML §8.8.2,
//! <https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#simple-dialogs>)
//! and `print()` (HTML §8.8.3,
//! <https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#dom-print>);
//! ADR 0028 section 4, feature 11c2.
//!
//! swb has no user interface for dialogs and no printer. A dialog behaves
//! as in Chromium under automation with no dialog handler (measured with
//! the probe tool, whose Playwright page dismisses every dialog): the
//! dialog is dismissed at once, so `alert()` returns `undefined`,
//! `confirm()` returns `false` and `prompt()` returns `null`. The message
//! goes to the log at `debug!` level, cut to [`LOG_CHARS`] characters (a
//! script may call `alert` in a loop).
//!
//! `print()` runs the printing steps and prints nothing: `beforeprint` and
//! then `afterprint` fire at the window, synchronously, before `print()`
//! returns (measured in Chromium 148; both are plain, trusted `Event`s that
//! neither bubble nor can be canceled). Chromium's rules, all measured:
//!
//! - Before the document is completely loaded, `print()` only sets the
//!   print-when-loaded flag; the events fire once, after the `load` event
//!   ([`run_when_loaded`]), however many calls set the flag. (A call from
//!   a parser-inserted script gives this in Chromium; its rules for a
//!   call from a `DOMContentLoaded`, `load` or `pageshow` listener, or
//!   with an image that is still loading, do not follow the standard: it
//!   prints nothing. swb follows the standard; a call from a `pageshow`
//!   listener sets the flag, because the document is not completely
//!   loaded until the `load` task ends.)
//! - A call during the printing steps (from a listener) does nothing.
//! - An exception in a `beforeprint` or `afterprint` listener is not
//!   reported: no `error` event, and the next listener still runs. swb puts
//!   the window in error reporting mode for the steps, which sends reports
//!   to the console. That mode also covers reports from inside a listener
//!   (`reportError()`, a listener of a nested `dispatchEvent`); what
//!   Chromium does with those is not measured.
//!
//! Departure: Chromium ignores most calls after the load that have no
//! transient user activation (measured: from a timer, from an evaluation,
//! from a `load` listener; a call in a click handler, or from a timer
//! within a few seconds of a click, runs; the first call without an
//! activation in a browser session may run, later ones do not). swb has no
//! model of user activation (backlog), so it always runs the steps. The
//! media type `print` is not applied, as in Chromium's headless pages
//! (`matchMedia('print').matches` is false in `beforeprint`).
//!
//! The log shows the first [`LOG_DIALOGS`] dialogs of a window and then one
//! line that says so, so a loop of dialogs writes a bounded log.

use swb_js::Runtime;

use crate::bindings::{self as b, Cx};
use crate::events::{DispatchTarget, TrustedEvent};
use crate::state::{state, state_mut};

/// The most characters of a dialog message that the log shows.
const LOG_CHARS: usize = 200;

/// The most dialogs per window that the log shows; a loop of dialogs
/// writes one more line, and then nothing.
const LOG_DIALOGS: u32 = 100;

/// Logs a dialog at `debug!` level: the kind, the length of the message and
/// its first [`LOG_CHARS`] characters, for the first [`LOG_DIALOGS`]
/// dialogs of the window.
fn log_dialog(cx: &mut Cx<'_>, kind: &str, message: &str) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let Ok(state) = state_mut(cx.rt) else {
        return;
    };
    if state.dialogs_logged > LOG_DIALOGS {
        return;
    }
    state.dialogs_logged += 1;
    if state.dialogs_logged > LOG_DIALOGS {
        log::debug!("more than {LOG_DIALOGS} dialogs: the log shows no more");
        return;
    }
    log::debug!(
        "{kind}() dismissed: {} bytes, {:?}",
        message.len(),
        shorten(message)
    );
}

/// The first [`LOG_CHARS`] characters of `message`, with `...` if it is
/// longer. Reads at most that many characters.
fn shorten(message: &str) -> String {
    let mut shown: String = message.chars().take(LOG_CHARS).collect();
    if shown.len() < message.len() {
        shown.push_str("...");
    }
    shown
}

/// `alert(message)`: dismissed at once.
pub(crate) fn alert(cx: &mut Cx<'_>, message: &str) {
    log_dialog(cx, "alert", message);
}

/// `confirm(message)`: dismissed at once, so the answer is `false`.
pub(crate) fn confirm(cx: &mut Cx<'_>, message: &str) -> bool {
    log_dialog(cx, "confirm", message);
    false
}

/// `prompt(message, default)`: dismissed at once, so the answer is `null`.
pub(crate) fn prompt(cx: &mut Cx<'_>, message: &str, _default: &str) -> Option<String> {
    log_dialog(cx, "prompt", message);
    None
}

/// `print()`: HTML "`print()`" steps 1 to 3. A document that was unloaded
/// is not fully active: the call does nothing (step 1).
pub(crate) fn print(cx: &mut Cx<'_>) -> b::Result<()> {
    let scripts = crate::scripts::scripts(cx.rt)?;
    if scripts.phase == crate::scripts::Phase::Unloaded {
        return Ok(());
    }
    if !scripts.completely_loaded() {
        state_mut(cx.rt)?.print_when_loaded = true;
        return Ok(());
    }
    run_printing_steps(cx.rt)?;
    Ok(())
}

/// The print-when-loaded flag: after the `load` event, the printing steps
/// run once for the calls that came earlier.
pub(crate) fn run_when_loaded(rt: &mut Runtime) -> swb_js::VmResult<()> {
    if !std::mem::take(&mut state_mut(rt)?.print_when_loaded) {
        return Ok(());
    }
    run_printing_steps(rt)
}

/// The printing steps: `beforeprint`, `afterprint` (nothing is printed in
/// between). Does nothing during the steps.
fn run_printing_steps(rt: &mut Runtime) -> swb_js::VmResult<()> {
    if state(rt)?.printing {
        return Ok(());
    }
    let event_loop = &mut state_mut(rt)?.event_loop;
    let reporting = std::mem::replace(&mut event_loop.error_reporting, true);
    state_mut(rt)?.printing = true;
    let mut result = Ok(());
    for kind in ["beforeprint", "afterprint"] {
        let fired = crate::scripts::fire(rt, DispatchTarget::Window, &TrustedEvent::new(kind));
        if let Err(error) = fired {
            result = Err(error);
            break;
        }
    }
    state_mut(rt)?.printing = false;
    state_mut(rt)?.event_loop.error_reporting = reporting;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_text_is_cut_at_a_character() {
        let long = "é".repeat(LOG_CHARS * 3);
        let shown = shorten(&long);
        assert_eq!(shown.chars().count(), LOG_CHARS + 3);
        assert!(shown.ends_with("..."));
        assert_eq!(shorten("short"), "short");
    }
}
