//! The scripts of the page (ADR 0028 sections 2 and 7): one script runtime
//! per top-level document with JavaScript on, created when the document
//! commits and dropped with it (a navigation, a reload, or a heap-limit stop).
//! The page lends the document state to the runtime for each task and takes it
//! back afterwards, which processes the DOM changes of the task. The frame
//! documents of the page's origin are documents of the same runtime; those of
//! another origin have a runtime of their own (ADR 0031 section 5,
//! `page/agents.rs`, [`Page::run_task_in`]).
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

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use swb_text::FontContext;
use swb_web::{
    ConsoleMessage, DispatchTarget, EVALUATION_TIMEOUT, Evaluation, EvaluationId, EvaluationStart,
    JsonForm, PageClock, ScriptRuntime, TrustedEvent,
};

use super::Page;
use crate::document::{DocumentState, MAX_PENDING_TRAVERSALS, ScriptNavigation, ScriptRequests};
use crate::frames::registry::RetiredElsewhere;
use crate::frames::{AgentId, FrameId, FrameScript, FrameTarget};

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
        self.agents.reset();
        self.scripts_dropped = false;
        if self.scripts_allowed && self.document_has_scripting() {
            self.create_runtime();
        }
        if self.script.is_some() {
            self.agents.enabled = true;
            self.frames.top_mut().script = Some(FrameScript::new(
                self.agents.top,
                swb_web::DocumentKey::FIRST,
            ));
            self.run_task(ScriptRuntime::start_document);
            self.check_load_event();
        }
        self.finish_parse_without_scripts();
    }

    fn create_runtime(&mut self) {
        let started = Instant::now();
        let mut runtime = match ScriptRuntime::new(self.clock.clone()) {
            Ok(runtime) => runtime,
            Err(error) => {
                log::warn!("cannot create the script runtime: {error}");
                return;
            }
        };
        let registry: Rc<RefCell<dyn swb_web::FrameHost>> = self.registry.clone();
        runtime.set_frame_host(registry, Rc::clone(&self.frames.count_cell));
        log::debug!(
            "script runtime created in {:?} (heap {} bytes)",
            started.elapsed(),
            runtime.heap_size()
        );
        self.agents.shared = Some(runtime.shared());
        self.agents.top = self.agents.new_id();
        self.script = Some(runtime);
    }

    /// Without a script runtime (JavaScript off, or a runtime that was
    /// dropped), a parse in steps runs to the end.
    pub(super) fn finish_parse_without_scripts(&mut self) {
        if self.script.is_none()
            && let Some(doc) = self.doc.as_deref_mut()
            && doc.is_parsing()
        {
            doc.run_parser();
            self.update_frames(FrameId::TOP);
        }
    }

    /// Hands the scripts that the document fetched to the runtime, in the
    /// order of arrival, and tells it when style sheets arrived (a script
    /// may wait for them); then fires `load` if it is due.
    pub(super) fn deliver_scripts(&mut self, sheets_changed: bool) {
        self.deliver_scripts_in(FrameId::TOP, sheets_changed);
    }

    /// [`Page::deliver_scripts`] for the document of `frame`.
    pub(super) fn deliver_scripts_in(&mut self, frame: FrameId, sheets_changed: bool) {
        let arrived = match self.doc_of_mut(frame) {
            Some(doc) => doc.take_arrived_scripts(),
            None => return,
        };
        if self.frame_runtime(frame).is_none() {
            return;
        }
        for (node, script) in arrived {
            self.run_task_in(frame, |runtime| runtime.script_fetched(node, script));
        }
        if sheets_changed
            && self
                .frame_runtime(frame)
                .is_some_and(|(runtime, key)| runtime.waits_for_style_sheets_in(key))
        {
            self.run_task_in(frame, ScriptRuntime::style_sheets_changed);
        }
        self.check_load_event();
    }

    /// Hands the results of the `fetch()` calls that arrived to the
    /// runtime, in order of arrival: each settles its promise in a task.
    pub(super) fn deliver_api_fetches(&mut self) {
        self.deliver_api_fetches_in(FrameId::TOP);
    }

    /// [`Page::deliver_api_fetches`] for the document of `frame`.
    pub(super) fn deliver_api_fetches_in(&mut self, frame: FrameId) {
        let arrived = match self.doc_of_mut(frame) {
            Some(doc) => doc.take_arrived_api_fetches(),
            None => return,
        };
        if self.frame_runtime(frame).is_none() {
            return;
        }
        for (id, result, kind, reservation) in arrived {
            self.run_task_in(frame, |runtime| {
                runtime.api_fetch_completed(id, result, kind);
            });
            // The body is in the script heap now (or was dropped): its bytes
            // leave the document's budget.
            drop(reservation);
        }
    }

    /// True if a `fetch()` call of a script waits for its response, or
    /// `load` and `error` events of images and style sheets wait for
    /// delivery. The loops that wait for the page's activity wait for it
    /// too; the `load` event does not.
    pub(super) fn has_pending_api_fetches(&self) -> bool {
        let pending = |d: &DocumentState| d.has_pending_api_fetches() || d.has_element_events();
        if self.doc.as_deref().is_some_and(pending) {
            return true;
        }
        self.frames_may_have_realms()
            && self
                .frames
                .tree_order()
                .into_iter()
                .skip(1)
                .filter_map(|id| self.frames.get(id)?.doc.as_deref())
                .any(pending)
    }

    /// True if a frame document can have a realm: a page whose frames no
    /// script touched does not walk them for what only a realm gives.
    pub(super) fn frames_may_have_realms(&self) -> bool {
        self.frames.count() > 0
            && (!self.agents.others.is_empty()
                || self
                    .script
                    .as_ref()
                    .is_some_and(ScriptRuntime::has_several_documents))
    }

    /// Hands the `load` and `error` events of images and style sheets that
    /// the document queued to the runtime (feature 10b), one slice at a
    /// time. Returns true if it delivered any.
    pub(super) fn deliver_element_events(&mut self) -> bool {
        self.deliver_element_events_in(FrameId::TOP)
    }

    /// The element events of the frame documents that wait for delivery.
    /// Returns true if it delivered any.
    fn deliver_frame_element_events(&mut self) -> bool {
        if self.lent || !self.frames_may_have_realms() {
            return false;
        }
        let mut delivered = false;
        for id in self.frames.tree_order().into_iter().skip(1) {
            if self
                .doc_of(id)
                .is_some_and(DocumentState::has_element_events)
            {
                delivered |= self.deliver_element_events_in(id);
            }
        }
        delivered
    }

    /// [`Page::deliver_element_events`] for the document of `frame`.
    pub(super) fn deliver_element_events_in(&mut self, frame: FrameId) -> bool {
        let events = match self.doc_of_mut(frame) {
            Some(doc) => doc.take_element_events(),
            None => return false,
        };
        // Without a realm nothing listens; the events go.
        if self.frame_runtime(frame).is_none() || events.is_empty() {
            return false;
        }
        self.run_task_in(frame, |runtime| runtime.element_events(events));
        self.check_load_event();
        true
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
        // The frames delay the load event (ADR 0031 section 2): each
        // fires its container's `load` first.
        self.check_frame_loads();
        if self.lent || self.frames_delay_load() {
            return;
        }
        if !self
            .script
            .as_ref()
            .is_some_and(ScriptRuntime::load_event_due)
        {
            // A listener of a container's `load` navigated or stopped.
            return;
        }
        self.run_task(ScriptRuntime::fire_load_event);
        if self
            .script
            .as_ref()
            .is_some_and(ScriptRuntime::document_loaded)
        {
            self.frames.top_mut().load_fired = true;
        }
        if let Some(runtime) = &self.script {
            log::debug!(
                "load event fired (script heap {} bytes)",
                runtime.heap_size()
            );
        }
    }

    /// True if the document has a script runtime whose `load` event has
    /// not fired yet.
    pub(super) fn load_event_pending(&self) -> bool {
        self.script.as_ref().is_some_and(|s| !s.document_loaded())
    }

    /// The load of the frame documents with a realm stops
    /// ([`Page::stop`]).
    pub(super) fn abort_frame_documents(&mut self) {
        if self.frames.count() == 0 {
            return;
        }
        for id in self.frames.tree_order().into_iter().skip(1) {
            if self.frames.get(id).is_some_and(|f| f.script.is_some()) {
                self.run_task_in(id, ScriptRuntime::abort_document);
            }
        }
    }

    /// Fires `pagehide` and `unload` at the windows of the frame documents
    /// with a realm, children first (HTML "unload a document and its
    /// descendants").
    pub(super) fn unload_frames(&mut self) {
        if self.frames.count() == 0 {
            return;
        }
        for id in self.frames.descendants_post_order(FrameId::TOP) {
            if self.frames.get(id).is_some_and(|f| f.script.is_some()) {
                self.run_task_in(id, ScriptRuntime::unload_document);
            }
        }
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
        self.run_task_in(FrameId::TOP, task)
    }

    /// [`Page::run_task`] with the document of `frame` as the current one, in the
    /// runtime of its origin. A native that reaches another document of the
    /// runtime borrows it from the registry; the page takes back what the task
    /// borrowed. So a task costs what it touches, not the number of documents
    /// with a realm. `None` if the document has no realm.
    pub(crate) fn run_task_in<R>(
        &mut self,
        frame: FrameId,
        task: impl FnOnce(&mut ScriptRuntime) -> R,
    ) -> Option<R> {
        if !self.agents.enabled {
            return None;
        }
        if self.active_frame != FrameId::TOP {
            // Inside `Page::in_frame`: `FrameId::TOP` means the active document,
            // as everything else there does.
            return self.outside_exchange(|page, active| {
                let frame = if frame == FrameId::TOP { active } else { frame };
                page.run_task_in(frame, task)
            });
        }
        let script = self.frames.get(frame)?.script?;
        let agent = script.agent();
        if frame == FrameId::TOP {
            return self.run_agent_task(agent, task);
        }
        let key = script.key_in(agent)?;
        self.run_agent_task(agent, |runtime| runtime.with_document(key, task))
            .flatten()
    }

    /// Runs `task` in the runtime of `agent`, with the frame tree in the registry
    /// (ADR 0031 section 5), then does what the task left for the page. The
    /// top-level runtime gets the top-level document lent. In a task of another
    /// runtime the top-level document is in its frame record, where layout across
    /// documents finds it and the members of the registry that answer for the
    /// agent do not: the same place as every other document of another runtime, so
    /// that no path needs a case for it. A runtime that reached the heap limit is
    /// dropped afterwards, alone. `None` without the runtime or a document.
    pub(super) fn run_agent_task<R, F: FnOnce(&mut ScriptRuntime) -> R>(
        &mut self,
        agent: AgentId,
        task: F,
    ) -> Option<R> {
        if self.active_frame != FrameId::TOP {
            return self.outside_exchange(|page, _| page.run_agent_task(agent, task));
        }
        self.runtime_of(agent)?;
        let top = agent == self.agents.top;
        self.set_limits_for_task(agent);
        let state = self.take_document_state()?;
        if top {
            if let Some(script) = self.script.as_mut() {
                script.lend(state);
            }
        } else {
            self.frames.top_mut().doc = Some(state);
        }
        self.enter_registry(agent);
        let script = self
            .runtime_of_mut(agent)
            .expect("the runtime was checked above");
        if !top {
            script.borrow_current_document();
        }
        let result = task(script);
        let state = if top { script.take_back() } else { None };
        let borrowed = script.take_back_documents();
        let after_task = self.leave_registry();
        let returned = self.file_borrowed_documents(borrowed);
        if let Some(after_task) = &after_task {
            self.made_in_task.extend(after_task.created.iter().copied());
        }
        let state = if top {
            state.and_then(|s| s.into_any().downcast::<DocumentState>().ok())
        } else {
            self.frames.top_mut().doc.take()
        };
        if let Some(state) = state {
            self.return_document_state(state);
        } else {
            // The bindings never give the state away; this is a bug. The
            // page's font context went with the state, so the page takes
            // the system fonts (also if it had others, such as the test
            // fonts) instead of the empty context, which has no glyphs.
            log::error!("the script runtime lost the document state");
            self.lent = false;
            self.fonts = FontContext::system();
        }
        for &id in &returned {
            self.update_frames(id);
            self.update_frame_charge(id);
        }
        if let Some(after_task) = after_task {
            self.after_frame_changes(after_task, agent);
        }
        if self.runtime_of(agent).is_some_and(ScriptRuntime::failed) {
            self.drop_failed_agent(agent);
        }
        self.finish_stopped_scripts(agent);
        self.check_page_long_tasks();
        self.act_on_script_requests();
        for id in returned {
            self.act_on_frame_requests(id);
        }
        Some(result)
    }

    /// Moves the frame tree into the registry, for the natives of a task of the
    /// runtime of `agent`.
    fn enter_registry(&mut self, agent: AgentId) {
        let settings = self.frame_settings();
        let frames = &self.frames;
        let others = &self.agents.others;
        let top = self.agents.top;
        let mut registry = self.registry.borrow_mut();
        registry.settings = settings;
        registry.enter(agent, || {
            if agent == top {
                frames.top().origin.clone()
            } else {
                super::agents::origin_of_agent(others, agent)
            }
        });
        std::mem::swap(&mut self.frames, &mut registry.frames);
    }

    /// Moves the frame tree back; what the task left for the page, if anything.
    fn leave_registry(&mut self) -> Option<crate::frames::registry::AfterTask> {
        let mut registry = self.registry.borrow_mut();
        std::mem::swap(&mut self.frames, &mut registry.frames);
        (!registry.after_task.is_empty()).then(|| std::mem::take(&mut registry.after_task))
    }

    /// Calls `f` with the runtime of `agent` and the frame tree in the registry,
    /// without a task and without a lent document: an operation of the runtime
    /// that reads or changes the tree.
    pub(super) fn with_registry<R>(
        &mut self,
        agent: AgentId,
        f: impl FnOnce(&mut ScriptRuntime) -> R,
    ) -> Option<R> {
        self.runtime_of(agent)?;
        self.enter_registry(agent);
        let result = self.runtime_of_mut(agent).map(f);
        if let Some(after_task) = self.leave_registry() {
            self.after_frame_changes(after_task, agent);
        }
        result
    }

    /// Puts the documents that a task borrowed back into their frames.
    /// Returns the frames.
    fn file_borrowed_documents(
        &mut self,
        borrowed: Vec<Box<dyn swb_web::EngineHost>>,
    ) -> Vec<FrameId> {
        let mut returned = Vec::with_capacity(borrowed.len());
        for host in borrowed {
            let Ok(mut doc) = host.into_any().downcast::<DocumentState>() else {
                continue;
            };
            doc.sync();
            let id = doc.frame_id;
            if let Some(frame) = self.frames.get_mut(id) {
                frame.doc = Some(doc);
                returned.push(id);
            }
        }
        returned
    }

    /// What a task of the runtime of `agent` left for the page: the input state of
    /// the frames that it destroyed, and the attributes of those that it made. What
    /// must happen in another runtime runs here: a task never runs a script of
    /// another runtime inside a native.
    fn after_frame_changes(
        &mut self,
        after_task: crate::frames::registry::AfterTask,
        agent: AgentId,
    ) {
        for (id, parent) in after_task.destroyed {
            self.frame_destroyed = true;
            self.forget_frame_input(id, parent);
            if !self.agents.others.is_empty() {
                self.forget_frame_everywhere(id);
            }
        }
        // A parent before its children (measured, case `agents-removal`).
        for retired in after_task.retired_elsewhere.into_iter().rev() {
            self.retire_elsewhere(retired);
        }
        for id in after_task.created {
            if self.frames.get(id).is_some() {
                self.process_new_frame(id);
            }
        }
        self.made_in_task.clear();
        for (id, navigation) in after_task.navigations {
            log::info!("a script navigates {id}: {}", navigation.url);
            if id == FrameId::TOP {
                self.location_navigate(navigation.url, navigation.replace);
            } else {
                self.navigate_frame_by_location(id, navigation.url, navigation.initiator);
            }
        }
        if let Some(focused) = after_task.focused {
            self.input.focus_frame = (focused != FrameId::TOP).then_some(focused);
            self.input.window_blurred = focused != FrameId::TOP;
            self.sync_window_focus();
            self.finish_focus_across_agents(after_task.focus_moves, agent);
        }
    }

    /// A task destroyed a frame whose document is in another runtime: its
    /// `pagehide` and `unload` run now, in its own runtime, and then it retires. For
    /// the task, the frame was gone at once.
    fn retire_elsewhere(&mut self, retired: RetiredElsewhere) {
        let RetiredElsewhere { script, doc, .. } = retired;
        let agent = script.agent();
        let Some(key) = script.key_in(agent) else {
            return;
        };
        self.agents.dirty = true;
        let Some(runtime) = self.runtime_of_mut(agent) else {
            // An `unload` before this one ended that runtime at the heap limit:
            // nothing can reach the document.
            if let Some(mut doc) = doc {
                self.loader.cancel_client(doc.client);
                doc.release_own_fonts(&mut self.fonts);
            }
            return;
        };
        if let Some(doc) = doc {
            runtime.lend_document(key, doc);
        }
        self.run_agent_task(agent, |runtime| {
            runtime.with_document(key, ScriptRuntime::unload_document);
            runtime.retire_document(key, None);
        });
    }

    /// Does what the scripts of a frame document asked for in the task (see
    /// [`Page::act_on_script_requests`]). A frame has no session history of its
    /// own (ADR 0031 section 2): a change of its URL is in the document, and
    /// the page only scrolls for a fragment.
    fn act_on_frame_requests(&mut self, id: FrameId) {
        let Some(doc) = self.doc_of_mut(id) else {
            return;
        };
        let ScriptRequests {
            navigation,
            scroll_into_view,
            state_changes,
            traversals,
        } = doc.take_script_requests();
        if let Some(node) = scroll_into_view {
            self.in_frame(id, |p| p.scroll_into_view(node));
        }
        for change in state_changes {
            if change.fragment_from.is_some()
                && let Some(fragment) = change.url.as_ref().and_then(|u| u.fragment())
            {
                let fragment = fragment.to_owned();
                self.in_frame(id, |p| {
                    let target = p.set_target(Some(&fragment));
                    p.scroll_to_fragment(&fragment);
                    p.focus_fragment_target(target);
                });
            }
        }
        match navigation {
            Some(ScriptNavigation::Link(url, target)) => {
                self.navigate_as_asked(super::frame_navigate::FrameNavigation {
                    source: id,
                    url,
                    post: None,
                    target,
                    by_script: true,
                });
            }
            Some(ScriptNavigation::Form(request)) => {
                if self.sandbox_of(id).forms_allowed() {
                    log::info!("a script of {id} submits a form: {}", request.url);
                    self.navigate_as_asked(super::frame_navigate::FrameNavigation {
                        source: id,
                        url: request.url,
                        post: request.post,
                        target: request.target,
                        by_script: true,
                    });
                } else {
                    log::info!("{id}: the sandbox has no `allow-forms`: not submitting");
                }
            }
            Some(ScriptNavigation::Location { url, .. }) => {
                let initiator = self.doc_of(id).map(DocumentState::origin);
                self.navigate_frame_by_location(id, url, initiator);
            }
            Some(ScriptNavigation::Reload) => self.reload_frame(id),
            None => {}
        }
        for delta in traversals {
            match delta {
                0 => self.reload_frame(id),
                delta if self.traversals.len() < MAX_PENDING_TRAVERSALS => {
                    self.traversals.push_back(delta as isize);
                }
                _ => {}
            }
        }
    }

    /// `location.reload()` of a frame document: the same document again.
    fn reload_frame(&mut self, id: FrameId) {
        let Some(doc) = self.doc_of(id) else {
            return;
        };
        let target = match doc.url().as_str() {
            "about:blank" => FrameTarget::Blank,
            "about:srcdoc" => FrameTarget::Srcdoc,
            _ => FrameTarget::Url(doc.url().clone()),
        };
        self.navigate_frame(id, target);
    }

    /// Does what the scripts of the task asked for: the scroll into view of
    /// a `focus()` that no geometry query of the task applied, the `scroll`
    /// events of the scrolls of the task (in the next frame), a link to
    /// follow or a form to submit (`a.click()`, `form.submit()`). A task
    /// that makes several navigation requests navigates to the last one.
    fn act_on_script_requests(&mut self) {
        let Some(doc) = self.doc.as_deref_mut() else {
            return;
        };
        let requests = doc.take_script_requests();
        let generation = doc.dom.generation();
        if std::mem::replace(&mut self.input.frames_seen_generation, generation) != generation {
            self.note_frame_viewports_may_change();
        }
        self.apply_state_changes(requests.state_changes);
        if let Some(node) = requests.scroll_into_view {
            self.scroll_into_view(node);
        }
        if self.doc.as_deref().is_some_and(|d| !d.scrolled.is_empty()) {
            self.update_hover(false);
            self.note_scroll(None);
        }
        match requests.navigation {
            Some(ScriptNavigation::Link(url, target)) => {
                self.navigate_from_input(url, None, target);
            }
            Some(ScriptNavigation::Form(request)) => {
                log::info!("a script submits a form: {}", request.url);
                self.navigate_from_input(request.url, request.post, request.target);
            }
            Some(ScriptNavigation::Location { url, replace }) => {
                self.location_navigate(url, replace);
            }
            Some(ScriptNavigation::Reload) => self.reload(),
            None => {}
        }
        for delta in requests.traversals {
            match delta {
                0 => self.reload(),
                delta if self.traversals.len() < MAX_PENDING_TRAVERSALS => {
                    self.traversals.push_back(delta as isize);
                }
                _ => {}
            }
        }
    }

    /// A runtime whose scripts stopped (three time limits, the budget of
    /// long tasks) runs no task, so the parse that waits for a script or a
    /// style sheet would never go on (ADR 0028 section 8). The document
    /// parses on without scripts: the runtime ends its load without
    /// events, the fetches of scripts are abandoned and the engine runs
    /// the parser to the end. The same for each frame document of the runtime of
    /// `agent` with a realm.
    pub(super) fn finish_stopped_scripts(&mut self, agent: AgentId) {
        let top = agent == self.agents.top;
        let Some(script) = self.runtime_of_mut(agent) else {
            return;
        };
        if !script.stopped() {
            return;
        }
        if top && !script.document_loaded() {
            script.abort_document();
            if let Some(doc) = self.doc.as_deref_mut() {
                doc.abandon_scripts();
                if doc.is_parsing() {
                    doc.run_parser();
                }
            }
        }
        if self.frames.count() == 0 {
            return;
        }
        for id in self.frames.tree_order().into_iter().skip(1) {
            let Some(key) = self
                .frames
                .get(id)
                .and_then(|f| f.script)
                .and_then(|s| s.key_in(agent))
            else {
                continue;
            };
            let Some(script) = self.runtime_of_mut(agent) else {
                return;
            };
            if script.document_loaded_in(key) {
                continue;
            }
            script.with_document(key, ScriptRuntime::abort_document);
            if let Some(doc) = self.doc_of_mut(id) {
                doc.abandon_scripts();
                doc.finish_parse_without_scripts();
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
        self.sweep_agents();
        let mut changed = self.process_network();
        let tasks = self.run_due_tasks();
        if changed || tasks > 0 {
            self.input.idle_streak = 0;
        }
        changed |= tasks > 0;
        changed |= self.deliver_element_events();
        changed |= self.deliver_frame_element_events();
        changed |= self.run_frame_refreshes();
        changed |= self.check_frame_loads();
        // The removal of a frame may leave nothing that delays the load.
        let removed = std::mem::take(&mut self.frame_destroyed);
        if changed || removed {
            // A frame's load may be the last that the load event waits for.
            self.check_load_event();
        }
        changed |= self.run_traversal();
        changed |= self.run_frame();
        changed |= self.release_lazy_without_frames();
        // Step 4 of the event loop: idle callbacks, if nothing else ran.
        if !changed {
            changed |= self.run_idle();
        }
        Pump {
            changed,
            next: self.next_task_instant(),
        }
    }

    /// Runs the tasks that are due now (at most [`PUMP_BUDGET`]). Returns
    /// how many ran.
    pub(super) fn run_due_tasks(&mut self) -> usize {
        if !self.agents.others.is_empty() {
            return self.run_due_tasks_across_agents();
        }
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

    /// [`Page::run_due_tasks`] with several runtimes (ADR 0031 section 6): the
    /// task with the earliest due time, then the lowest sequence number, whatever
    /// its runtime. A runtime runs its tasks up to the first task of another one.
    fn run_due_tasks_across_agents(&mut self) -> usize {
        let started = Instant::now();
        let mut ran = 0;
        while let Some((agent, before)) = self.first_due_agent(self.clock.now()) {
            let budget = PUMP_BUDGET.saturating_sub(started.elapsed());
            let done = self
                .run_agent_task(agent, |runtime| {
                    runtime.run_due_tasks_before(budget, before)
                })
                .unwrap_or(0);
            ran += done;
            // A task can make a load event due; it fires before the next task
            // when nothing delays it.
            self.check_frame_loads();
            self.check_load_event();
            if done == 0 || started.elapsed() >= PUMP_BUDGET {
                break;
            }
        }
        ran
    }

    /// Runs the first traversal of the session history that scripts asked
    /// for (a task of its own, after the due tasks). True if it ran.
    fn run_traversal(&mut self) -> bool {
        if self.lent {
            return false;
        }
        match self.traversals.pop_front() {
            Some(delta) => {
                self.traverse(delta);
                true
            }
            None => false,
        }
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
        if !self.traversals.is_empty() || !self.frame_navigations.is_empty() {
            return Some(self.clock.now());
        }
        // The declarative refreshes of frame documents (also with
        // JavaScript off).
        let refresh = self.next_frame_refresh();
        if !self.agents.enabled {
            return refresh;
        }
        [
            self.frame_time(),
            self.idle_time(),
            refresh,
            self.next_task_of_all(),
        ]
        .into_iter()
        .flatten()
        .reduce(f64::min)
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
    /// time. A frame that only runs animation frame callbacks does not
    /// count: a page with a `requestAnimationFrame` loop can be idle.
    pub fn is_idle(&mut self, quiet: Duration) -> bool {
        if self.is_loading() || !self.traversals.is_empty() || self.frames_busy() {
            return false;
        }
        let horizon = self.clock.now() + quiet.as_secs_f64() * 1000.0;
        if self
            .next_task_of_all()
            .into_iter()
            .chain(self.frame_work_due())
            .chain(self.next_frame_refresh())
            .any(|time| time <= horizon)
            || self.idle_work_before(horizon)
        {
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
            let loading = self.is_loading() || self.has_pending_api_fetches() || self.frames_busy();
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

    /// [`Page::evaluate_with_timeout`] in the document of frame `id`
    /// (`page.evaluate` with a `frameId`, `--eval-frame`), in the runtime of its
    /// origin, which is made now if it has none. `None` if the frame is gone or
    /// its document cannot have a realm.
    pub fn evaluate_in_frame(
        &mut self,
        id: FrameId,
        source: &str,
        form: JsonForm,
        timeout: Duration,
    ) -> Option<Evaluation> {
        if id == FrameId::TOP {
            return self.evaluate_with_timeout(source, form, timeout);
        }
        self.frames.get(id)?;
        self.ensure_frame_realm(id);
        let agent = self.frames.get(id)?.script?.agent();
        let start = self.run_task_in(id, |script| script.start_evaluation(source, form))?;
        let id = match start {
            EvaluationStart::Done(evaluation) => return Some(evaluation),
            EvaluationStart::Pending(id) => id,
        };
        let mut result = None;
        self.run_until(None, timeout, |page| {
            result = page.poll_evaluation_in(agent, id, form);
            result.is_some()
        });
        Some(result.unwrap_or_else(|| {
            if let Some(script) = self.runtime_of_mut(agent) {
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
        let agent = self
            .frames
            .get(self.active_frame)
            .and_then(|f| f.script)
            .map_or(self.agents.top, FrameScript::agent);
        self.poll_evaluation_in(agent, id, form)
    }

    /// [`Page::poll_evaluation`] of an evaluation in the runtime of `agent`.
    fn poll_evaluation_in(
        &mut self,
        agent: AgentId,
        id: EvaluationId,
        form: JsonForm,
    ) -> Option<Evaluation> {
        let Some(script) = self.runtime_of(agent) else {
            return Some(Evaluation::Error(DROPPED.to_owned()));
        };
        // Lend the state only when there is a result to write (its getters
        // may read the DOM).
        if !script.evaluation_settled(id) {
            return None;
        }
        self.run_agent_task(agent, |script| script.poll_evaluation(id, form))
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

    /// The console messages of all documents of the page in the order in which
    /// they were written (at most the last 1,000; each has the frame of its
    /// document), empty without a script runtime.
    pub fn console_messages(&self) -> Vec<ConsoleMessage> {
        self.agents
            .shared
            .as_ref()
            .map(swb_web::AgentShared::console_messages)
            .unwrap_or_default()
    }

    /// True if the top-level document has a script runtime. A frame of another
    /// origin can have one without it (after a heap-limit stop of the top level's).
    pub fn has_script_runtime(&self) -> bool {
        self.script.is_some()
    }

    /// The heap size of the top-level document's script runtime in bytes (tests
    /// and measurements); [`Page::script_heap_total`] is the sum of all runtimes.
    pub fn script_heap_size(&self) -> Option<usize> {
        self.script.as_ref().map(ScriptRuntime::heap_size)
    }

    /// The script runtime of the top-level document, to change its limits (tests
    /// and tools). The limits of the other runtimes of the page come from
    /// [`Page::set_agent_limits`], which also rules this one on a page with several.
    pub fn script_runtime_mut(&mut self) -> Option<&mut ScriptRuntime> {
        self.script.as_mut()
    }
}
