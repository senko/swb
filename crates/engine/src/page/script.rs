//! The scripts of the page (ADR 0028 section 2): one script runtime per
//! document with JavaScript on, created when the document commits and
//! dropped with it (a navigation, a reload). The page lends the document
//! state to the runtime for each task and takes it back afterwards, which
//! processes the DOM changes of the task.
//!
//! Feature 4c2 runs the inline classic scripts of a document at the end of
//! its parse ([`Page::start_scripts`]) and evaluates expressions for the
//! automation API and `--eval` ([`Page::evaluate`]). Feature 5a adds the
//! entry for DOM events that the engine fires ([`Page::dispatch_event`]);
//! the engine fires none yet (`load` comes with feature 5c, input events
//! with 6d). The event loop (feature 5b) and the script processing in the
//! parser (5c) come later.

use std::time::Instant;

use swb_web::{ConsoleMessage, DispatchTarget, Evaluation, JsonForm, ScriptRuntime, TrustedEvent};

use super::Page;
use crate::document::DocumentState;

impl Page {
    /// True if this page may run scripts: the thread that runs it has a
    /// stack large enough for the script runtime
    /// ([`super::PageConfig::scripts_allowed`]).
    pub fn javascript_available(&self) -> bool {
        self.scripts_allowed
    }

    /// Creates the script runtime of the current document if the document
    /// has scripting on, and runs its inline classic scripts in tree order
    /// (feature 5c moves them into the parse). Called when a document
    /// commits.
    pub(super) fn start_scripts(&mut self) {
        self.script = None;
        if !self.scripts_allowed || !self.document_has_scripting() {
            return;
        }
        let started = Instant::now();
        let runtime = match ScriptRuntime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                log::warn!("cannot create the script runtime: {error}");
                return;
            }
        };
        log::debug!(
            "script runtime created in {:?} (heap {} bytes)",
            started.elapsed(),
            runtime.heap_size()
        );
        self.script = Some(runtime);
        let url = self.doc.as_ref().map(|d| d.url().to_string());
        let url = url.unwrap_or_default();
        self.run_task(|script| script.run_inline_scripts(&url));
    }

    /// Runs `task` with the document state lent to the script runtime, then
    /// gives the state back (which processes the DOM changes). `None`
    /// without a runtime or a document.
    pub(crate) fn run_task<R>(&mut self, task: impl FnOnce(&mut ScriptRuntime) -> R) -> Option<R> {
        self.script.as_ref()?;
        let state = self.take_document_state()?;
        let script = self.script.as_mut()?;
        script.lend(state);
        let result = task(script);
        let state = script.take_back();
        if let Some(state) = state.and_then(|s| s.into_any().downcast::<DocumentState>().ok()) {
            self.return_document_state(state);
        } else {
            // The bindings never give the state away; this is a bug.
            log::error!("the script runtime lost the document state");
            self.lent = false;
        }
        Some(result)
    }

    /// Evaluates `source` as a classic script in the global scope of the
    /// document (the value of the last expression statement; a promise is
    /// awaited as far as the microtask checkpoint settles it) and returns
    /// the JSON text of the value in `form`, or the error. `None` if the
    /// document has no script runtime (JavaScript off).
    pub fn evaluate(&mut self, source: &str, form: JsonForm) -> Option<Evaluation> {
        self.run_task(|script| script.evaluate(source, form))
    }

    /// Fires a trusted DOM event at a node of the current document or at
    /// its window as a task (DOM "fire an event"; ADR 0028 section 11):
    /// the listeners run under the task's time limit, then a microtask
    /// checkpoint; the DOM changes are processed when the state comes back.
    /// Returns whether the default action should run (false if a listener
    /// canceled the event; true when the dispatch failed or the
    /// document's scripts are stopped), or `None` without a script
    /// runtime. Features 5c (`load`, `DOMContentLoaded`, `pageshow`) and 6d
    /// (input events) call it.
    pub fn dispatch_event(&mut self, target: DispatchTarget, event: &TrustedEvent) -> Option<bool> {
        let result = self.run_task(|script| script.dispatch_event(target, event))?;
        Some(result.unwrap_or_else(|error| {
            log::debug!("the event '{}' ended early: {error}", event.kind);
            true
        }))
    }

    /// The console messages of the current document (at most the last
    /// 1,000), empty without a script runtime.
    pub fn console_messages(&self) -> Vec<ConsoleMessage> {
        self.script
            .as_ref()
            .map(ScriptRuntime::console_messages)
            .unwrap_or_default()
    }

    /// True if the current document has a script runtime.
    pub fn has_script_runtime(&self) -> bool {
        self.script.is_some()
    }

    /// The heap size of the current document's script runtime in bytes
    /// (tests and measurements).
    pub fn script_heap_size(&self) -> Option<usize> {
        self.script.as_ref().map(ScriptRuntime::heap_size)
    }
}
