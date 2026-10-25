//! The scripts of the page (ADR 0028 sections 2 and 7): one script runtime
//! per document with JavaScript on, created when the document commits and
//! dropped with it (a navigation, a reload, or a heap-limit stop). The page
//! lends the document state to the runtime for each task and takes it back
//! afterwards, which processes the DOM changes of the task.
//!
//! The runtime drives the parse of its document ([`Page::start_scripts`],
//! feature 5c): the parser stops at each script, inline scripts run at
//! their place, and the parser waits for the scripts that block it and for
//! the style sheets that block scripts. The page fetches the scripts of
//! script elements and hands each result to the runtime
//! ([`Page::deliver_scripts`]), tells it about arrived style sheets, and
//! fires the `load` event when the runtime is ready for it and nothing
//! delays it ([`Page::check_load_event`]: no request, also after a layout
//! that can find background images and fonts). A document without a
//! runtime parses to the end at once. Leaving a document fires `pagehide`
//! and `unload` ([`Page::unload_document`]).
//!
//! The page also evaluates expressions for the automation API and
//! `--eval` ([`Page::evaluate`]) and fires DOM events
//! ([`Page::dispatch_event`]). [`Page::pump`] runs the due tasks (timers,
//! parser tasks, notifications of rejected promises, finalization
//! cleanups) and says when the next one is due; the embedder calls it from
//! its own loop. [`Page::run_event_loop`], [`Page::wait_until_idle`] and
//! the evaluation of a promise block and run the loop themselves. All of
//! them follow the page clock: in virtual time the clock jumps to the next
//! task when nothing runs and no request is pending.

use std::time::{Duration, Instant};

use swb_web::{
    ConsoleMessage, DispatchTarget, EVALUATION_TIMEOUT, Evaluation, EvaluationId, EvaluationStart,
    JsonForm, PageClock, ScriptRuntime, TrustedEvent,
};

use super::{HistoryHandling, Page};
use crate::document::{DocumentState, ScriptNavigation};

/// The most real time that one [`Page::pump`] spends on tasks (a frame at
/// 60 Hz), so that the embedder's loop stays responsive.
const PUMP_BUDGET: Duration = Duration::from_millis(16);

/// The error of an evaluation after the document's runtime was dropped.
const DROPPED: &str = "Error: the scripts of the document were stopped";

/// What one turn of the event loop did ([`Page::pump`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pump {
    /// True if a network completion or a task ran (the page may need a
    /// repaint).
    pub changed: bool,
    /// When the next task is due: the embedder calls [`Page::pump`] again
    /// then (or earlier, on input or a network wake-up). `None` if no task
    /// waits, or in virtual time if none is due now (virtual time moves
    /// only in the blocking loops).
    pub next: Option<Instant>,
}

impl Page {
    /// True if this page may run scripts: the thread that runs it has a
    /// stack large enough for the script runtime
    /// ([`super::PageConfig::scripts_allowed`]).
    pub fn javascript_available(&self) -> bool {
        self.scripts_allowed
    }

    /// The page clock (real time unless [`Page::set_clock`] set another).
    pub fn clock(&self) -> &PageClock {
        &self.clock
    }

    /// Sets the page clock: virtual time and a fixed start time (`swb
    /// --now`). It applies to the documents that commit after the call.
    pub fn set_clock(&mut self, clock: PageClock) {
        self.clock = clock;
    }

    /// Creates the script runtime of the current document if the document
    /// has scripting on, and starts the document: the runtime parses it
    /// with its scripts. Without a runtime the parse runs to the end at
    /// once. Called when a document commits.
    pub(super) fn start_scripts(&mut self) {
        self.script = None;
        self.scripts_dropped = false;
        if self.scripts_allowed && self.document_has_scripting() {
            self.create_runtime();
        }
        if self.script.is_some() {
            self.run_task(ScriptRuntime::start_document);
            self.check_load_event();
        }
        self.finish_parse_without_scripts();
    }

    fn create_runtime(&mut self) {
        let started = Instant::now();
        let runtime = match ScriptRuntime::new(self.clock.clone()) {
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
    }

    /// Without a script runtime (JavaScript off, or a runtime that was
    /// dropped), a parse in steps runs to the end.
    fn finish_parse_without_scripts(&mut self) {
        if self.script.is_none()
            && let Some(doc) = self.doc.as_deref_mut()
            && doc.is_parsing()
        {
            doc.run_parser();
        }
    }

    /// Hands the scripts that the document fetched to the runtime, in the
    /// order of arrival, and tells it when style sheets arrived (a script
    /// may wait for them); then fires `load` if it is due.
    pub(super) fn deliver_scripts(&mut self, sheets_changed: bool) {
        let arrived = match self.doc.as_deref_mut() {
            Some(doc) => doc.take_arrived_scripts(),
            None => return,
        };
        if self.script.is_none() {
            return;
        }
        for (node, script) in arrived {
            self.run_task(|runtime| runtime.script_fetched(node, script));
        }
        if sheets_changed
            && self
                .script
                .as_ref()
                .is_some_and(ScriptRuntime::waits_for_style_sheets)
        {
            self.run_task(ScriptRuntime::style_sheets_changed);
        }
        self.check_load_event();
    }

    /// Fires the `load` event of the document when the runtime is ready
    /// for it ("the end" waits for nothing that delays the load event) and
    /// nothing delays it: no request runs, also after a layout, which can
    /// start the loads of background images and web fonts.
    pub(super) fn check_load_event(&mut self) {
        if self.lent
            || !self
                .script
                .as_ref()
                .is_some_and(ScriptRuntime::load_event_due)
        {
            return;
        }
        if self.doc.as_ref().is_none_or(|d| d.is_loading()) {
            return;
        }
        self.update_layout();
        if self.doc.as_ref().is_none_or(|d| d.is_loading()) {
            return;
        }
        self.run_task(ScriptRuntime::fire_load_event);
    }

    /// True if the document has a script runtime whose `load` event has
    /// not fired yet.
    pub(super) fn load_event_pending(&self) -> bool {
        self.script.as_ref().is_some_and(|s| !s.document_loaded())
    }

    /// Fires `pagehide` and `unload` at the window of the current document
    /// (HTML "unload a document"); a navigation calls it before it
    /// replaces the document.
    pub(super) fn unload_document(&mut self) {
        if self.script.is_some() {
            self.run_task(ScriptRuntime::unload_document);
        }
    }

    /// Runs `task` with the document state lent to the script runtime, then
    /// gives the state back (which processes the DOM changes). A runtime
    /// that reached the heap limit is dropped afterwards (ADR 0028 section
    /// 7). `None` without a runtime or a document.
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
        if self.script.as_ref().is_some_and(ScriptRuntime::failed) {
            log::warn!(
                "the script runtime of the document reached the heap limit and is dropped; the page stays as it is"
            );
            self.script = None;
            self.scripts_dropped = true;
            self.finish_parse_without_scripts();
        }
        self.finish_stopped_scripts();
        self.act_on_script_requests();
        Some(result)
    }

    /// Does what the scripts of the task asked for: scrolls an element
    /// into view (after `focus()`), follows a link or submits a form
    /// (`a.click()`, `form.submit()`). A task that makes several
    /// navigation requests navigates to the last one.
    fn act_on_script_requests(&mut self) {
        let Some(doc) = self.doc.as_deref_mut() else {
            return;
        };
        let requests = doc.take_script_requests();
        if let Some(node) = requests.scroll_into_view {
            self.scroll_into_view(node);
        }
        match requests.navigation {
            Some(ScriptNavigation::Link(url)) => {
                self.follow_link(url);
            }
            Some(ScriptNavigation::Form(request)) => {
                log::info!("a script submits a form: {}", request.url);
                let initiator = self.document_origin();
                self.start_navigation(
                    request.url,
                    request.post,
                    HistoryHandling::Push,
                    None,
                    initiator,
                );
            }
            None => {}
        }
    }

    /// A runtime whose scripts stopped (three time limits, the budget of
    /// long tasks) runs no task, so the parse that waits for a script or a
    /// style sheet would never go on (ADR 0028 section 8). The document
    /// parses on without scripts: the runtime ends its load without
    /// events, the fetches of scripts are abandoned and the engine runs
    /// the parser to the end.
    fn finish_stopped_scripts(&mut self) {
        let Some(script) = self.script.as_mut() else {
            return;
        };
        if !script.stopped() || script.document_loaded() {
            return;
        }
        script.abort_document();
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.abandon_scripts();
            if doc.is_parsing() {
                doc.run_parser();
            }
        }
    }

    // ----- The event loop -----

    /// One turn of the event loop (ADR 0028 section 7): handles the network
    /// completions, runs the tasks that are due (for at most 16 ms of real
    /// time), and says when the next task is due. It never blocks. The GUI,
    /// the headless runner and the automation server call it from their
    /// loops.
    pub fn pump(&mut self) -> Pump {
        let mut changed = self.process_network();
        changed |= self.run_due_tasks() > 0;
        changed |= self.run_frame();
        Pump {
            changed,
            next: self.next_task_instant(),
        }
    }

    /// Runs the tasks that are due now (at most [`PUMP_BUDGET`]). Returns
    /// how many ran.
    pub(super) fn run_due_tasks(&mut self) -> usize {
        if !self.task_due() {
            return 0;
        }
        let ran = self
            .run_task(|script| script.run_due_tasks(PUMP_BUDGET))
            .unwrap_or(0);
        // A task (the parser's) can make the load event due; it fires
        // before the next task when nothing delays it.
        self.check_load_event();
        ran
    }

    /// True if a task of the document is due at the page time now.
    fn task_due(&self) -> bool {
        self.script
            .as_ref()
            .and_then(ScriptRuntime::next_task_time)
            .is_some_and(|time| time <= self.clock.now())
    }

    /// The page time of the next task of the document.
    fn next_task_time(&self) -> Option<f64> {
        let tasks = self.script.as_ref()?.next_task_time();
        match (tasks, self.frame_time()) {
            (Some(task), Some(frame)) => Some(task.min(frame)),
            (task, frame) => task.or(frame),
        }
    }

    /// When the next task of the document is due, in real time; in virtual
    /// time only a task that is due now counts.
    pub fn next_task_instant(&self) -> Option<Instant> {
        let time = self.next_task_time()?;
        if self.clock.is_virtual() && time > self.clock.now() {
            return None;
        }
        Some(self.clock.instant_of(time))
    }

    /// Runs the event loop for `duration` of page time (real time waits;
    /// virtual time jumps from task to task while no request is pending),
    /// but at most `timeout` of real time (the headless `--settle-ms`).
    pub fn run_event_loop(&mut self, duration: Duration, timeout: Duration) {
        self.run_until(Some(duration), timeout, |_| false);
    }

    /// Runs the event loop until the page is idle: nothing is loading and no
    /// task is due within `quiet` of page time. Returns false if `timeout`
    /// of real time passed first (`page.waitForIdle`).
    pub fn wait_until_idle(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.run_until(None, timeout, |page| page.is_idle(quiet))
    }

    /// True if nothing is loading and no task is due within `quiet` of page
    /// time.
    pub fn is_idle(&mut self, quiet: Duration) -> bool {
        if self.is_loading() {
            return false;
        }
        let horizon = self.clock.now() + quiet.as_secs_f64() * 1000.0;
        if self.next_task_time().is_some_and(|time| time <= horizon) {
            return false;
        }
        // Layout can discover more resources.
        self.is_fully_loaded()
    }

    /// Runs the event loop until `done` holds (true), `page_time` of page
    /// time has passed or `timeout` of real time has passed (false). It
    /// waits for the next task, the end or a network completion; in virtual
    /// time, when nothing runs and no request is pending, the clock jumps
    /// to the next task (or the end) instead.
    fn run_until(
        &mut self,
        page_time: Option<Duration>,
        timeout: Duration,
        mut done: impl FnMut(&mut Page) -> bool,
    ) -> bool {
        let deadline = Instant::now().checked_add(timeout);
        let end = page_time.map(|d| self.clock.now() + d.as_secs_f64() * 1000.0);
        loop {
            if done(self) {
                return true;
            }
            if deadline.is_some_and(|d| Instant::now() >= d)
                || end.is_some_and(|e| self.clock.now() >= e)
                || self.lent
            {
                return false;
            }
            if self.pump().changed {
                continue;
            }
            let next = self.next_task_time();
            let loading = self.is_loading();
            if self.clock.is_virtual() && !loading {
                // Nothing runs and nothing is pending: time jumps.
                let Some(target) = next.into_iter().chain(end).reduce(f64::min) else {
                    return done(self);
                };
                self.clock.advance_to(target);
                continue;
            }
            if !loading && next.is_none() && end.is_none() {
                // Nothing can happen any more.
                return done(self);
            }
            let mut until = deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(3600));
            if !self.clock.is_virtual() {
                for time in next.into_iter().chain(end) {
                    until = until.min(self.clock.instant_of(time));
                }
            }
            let wait = until.saturating_duration_since(Instant::now());
            if let Some(completion) = self.loader.recv_timeout(wait) {
                self.handle_completion(completion);
            }
        }
    }

    // ----- Evaluation -----

    /// Evaluates `source` as a classic script in the global scope of the
    /// document (the value of the last expression statement) and returns
    /// the JSON text of the value in `form`, or the error. A promise is
    /// awaited through the event loop for at most 30 s
    /// ([`Page::evaluate_with_timeout`]). `None` if the document has no
    /// script runtime (JavaScript off).
    pub fn evaluate(&mut self, source: &str, form: JsonForm) -> Option<Evaluation> {
        self.evaluate_with_timeout(source, form, EVALUATION_TIMEOUT)
    }

    /// [`Page::evaluate`], waiting at most `timeout` of real time for a
    /// promise to settle.
    pub fn evaluate_with_timeout(
        &mut self,
        source: &str,
        form: JsonForm,
        timeout: Duration,
    ) -> Option<Evaluation> {
        let id = match self.start_evaluation(source, form)? {
            EvaluationStart::Done(evaluation) => return Some(evaluation),
            EvaluationStart::Pending(id) => id,
        };
        let mut result = None;
        self.run_until(None, timeout, |page| {
            result = page.poll_evaluation(id, form);
            result.is_some()
        });
        Some(result.unwrap_or_else(|| {
            if let Some(script) = self.script.as_mut() {
                script.cancel_evaluation(id);
            }
            Evaluation::Error("Error: the promise did not settle in time".to_owned())
        }))
    }

    /// Starts an evaluation without waiting: a value, an error, or a promise
    /// that has not settled ([`Page::poll_evaluation`]). `None` without a
    /// script runtime; an error if the runtime was dropped.
    pub fn start_evaluation(&mut self, source: &str, form: JsonForm) -> Option<EvaluationStart> {
        if self.script.is_none() && self.scripts_dropped {
            return Some(EvaluationStart::Done(Evaluation::Error(DROPPED.to_owned())));
        }
        self.run_task(|script| script.start_evaluation(source, form))
    }

    /// The result of a pending evaluation once its promise settled, else
    /// `None`. A document whose runtime went away gives an error.
    pub fn poll_evaluation(&mut self, id: EvaluationId, form: JsonForm) -> Option<Evaluation> {
        if self.script.is_none() {
            return Some(Evaluation::Error(DROPPED.to_owned()));
        }
        // Lend the state only when there is a result to write (its getters
        // may read the DOM).
        if !self.script.as_ref()?.evaluation_settled(id) {
            return None;
        }
        self.run_task(|script| script.poll_evaluation(id, form))
            .flatten()
    }

    /// Fires a trusted DOM event at a node of the current document or at
    /// its window as a task (DOM "fire an event"; ADR 0028 section 11):
    /// the listeners run under the task's time limit, with a microtask
    /// checkpoint after each listener and after the task; the DOM changes
    /// are processed when the state comes back. Returns whether the
    /// default action should run (false if a listener canceled the event;
    /// true when the dispatch failed or the document's scripts are
    /// stopped), or `None` without a script runtime. Features 5c (`load`,
    /// `DOMContentLoaded`, `pageshow`) and 6d (input events) call it.
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

    /// The script runtime of the current document, to change its limits
    /// (tests and tools).
    pub fn script_runtime_mut(&mut self) -> Option<&mut ScriptRuntime> {
        self.script.as_mut()
    }
}
