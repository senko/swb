//! The parse and the life of the window's document (ADR 0028 section 8):
//! the parser with its pauses for scripts (HTML §13.2.6.4.8, the `script`
//! end tag in the "text" insertion mode,
//! <https://html.spec.whatwg.org/multipage/parsing.html#parsing-main-incdata>),
//! "the end" of the parse (HTML §13.2.7,
//! <https://html.spec.whatwg.org/multipage/parsing.html#the-end>): the
//! readiness, the deferred scripts, `DOMContentLoaded`, the wait for the
//! scripts that run as soon as possible, `load` and `pageshow`; and the
//! unload (`pagehide`, `unload`).
//!
//! The engine lends the document and its parser ([`crate::EngineHost::parse_step`]).
//! [`ScriptRuntime::start_document`] parses until the parser has to wait
//! for a script or a style sheet, or until the end; inline scripts run
//! between the steps, each as a task. When a wait ends (the engine hands a
//! fetched script back, or a style sheet arrives), a parser task
//! ([`Task::ParserResume`]) runs the waiting script, and the next one
//! continues the parse. So a timer that the waiting script set for 0 ms
//! runs before the rest of the parse, as in Chromium (measured,
//! `tools/probes/web-scripts.json`, case `blocking-timers`).
//!
//! Departures from HTML, as Chromium (measured): `DOMContentLoaded` and
//! `load` fire at once when their conditions hold, not in queued tasks, so
//! a timer of 0 ms that an inline script set runs after `load` when
//! nothing delays it (case `timer-vs-dcl`, a document from a file). But
//! "the end" starts in a parser task, after the tasks that are due, when
//! the document came over HTTP(S) (its end of input is a task of its own;
//! cases `timer-vs-dcl-loading`, `-image`, `end-http-*`) or when the
//! parser ever waited for a script or a style sheet (cases `end-file-*`):
//! a timer of 0 ms then runs before the readiness becomes "interactive".
//! Images and scripts that still load do not defer it for a file. The
//! engine decides when nothing delays the load event
//! ([`ScriptRuntime::load_event_due`], [`ScriptRuntime::fire_load_event`]),
//! because it knows the requests and lays the page out to find its
//! background images and fonts.
//!
//! With the document's scripts stopped (time or heap limits), the parse and
//! the life of the document go on without scripts and events.

use swb_dom::{NodeId, ParseStep};

use crate::event_loop::{Task, queue_task};
use crate::events::{DispatchTarget, EventInterface, TrustedEvent};
use crate::generated::types::DocumentReadyState;
use crate::performance::Milestone;
use crate::runtime::{ScriptRuntime, task_error};
use crate::scripts::{FetchedScript, Phase, Prepared, WhenReady, scripts, scripts_mut};
use crate::state::engine_mut;

impl ScriptRuntime {
    /// Starts the lent document: its readiness becomes "loading" and the
    /// parse runs until it waits for a script or a style sheet, or to its
    /// end (then the rest of "the end" follows). The engine calls it once,
    /// when the document commits; a document without a parser (the engine
    /// made it complete) ends its parse at once.
    pub fn start_document(&mut self) {
        let Ok(scripts) = scripts_mut(&mut self.rt) else {
            return;
        };
        if scripts.phase != Phase::NotStarted {
            return;
        }
        scripts.phase = Phase::Parsing;
        scripts.readiness = DocumentReadyState::Loading;
        crate::performance::record(&mut self.rt, Milestone::DomLoading);
        crate::performance::record_url(&mut self.rt);
        self.parse();
    }

    /// Hands the result of the fetch of a script element's script to the
    /// runtime (`None`: a network error, an error status or a blocked MIME
    /// type): HTML "mark as ready", then the steps that wait for it. An
    /// async script runs now, in its own task; the scripts of the in-order
    /// list run when the ones before them have run; a parser script lets
    /// the parser go on in a task.
    pub fn script_fetched(&mut self, node: NodeId, script: Option<FetchedScript>) {
        let ready = match crate::scripts::mark_as_ready(&mut self.rt, node, script) {
            Ok(ready) => ready,
            Err(error) => {
                log::warn!("a fetched script was lost: {error:?}");
                return;
            }
        };
        match ready {
            Some(WhenReady::Asap) => {
                self.execute_script(node);
                if let Ok(scripts) = scripts_mut(&mut self.rt) {
                    scripts.asap.remove(&node);
                }
            }
            Some(WhenReady::InOrder) => self.run_in_order_scripts(),
            Some(WhenReady::ParserExecuted) => {
                if let Ok(scripts) = scripts_mut(&mut self.rt)
                    && let Some(el) = scripts.elements.get_mut(&node)
                {
                    el.ready_to_be_parser_executed = true;
                }
                if self.is_pending_parsing_blocking(node) {
                    self.queue_resume();
                }
            }
            None => {}
        }
        self.advance_document();
    }

    /// Hands the result of a `fetch()` call (feature 10a, the id of
    /// [`crate::EngineHost::start_api_fetch`]) to the runtime: the promise
    /// of the call settles in this task. `kind` is the type of the
    /// response. A call that was aborted is ignored.
    pub fn api_fetch_completed(
        &mut self,
        id: u64,
        result: Result<swb_net::Response, swb_net::NetError>,
        kind: swb_net::ResponseKind,
    ) {
        let _ = self.task("a fetch", |rt| {
            crate::fetch::complete(rt, id, result, kind).map_err(task_error)
        });
    }

    /// The engine says that a style sheet arrived or failed: a script that
    /// waits for the style sheets that block scripts may run now.
    pub fn style_sheets_changed(&mut self) {
        let Ok(scripts) = scripts(&self.rt) else {
            return;
        };
        match scripts.phase {
            Phase::Parsing if scripts.pending_parsing_blocking.is_some() => self.queue_resume(),
            Phase::Deferred => self.advance_document(),
            _ => {}
        }
    }

    /// True if a script of the document waits for style sheets that block
    /// scripts (then the engine calls [`ScriptRuntime::style_sheets_changed`]
    /// after each style sheet).
    pub fn waits_for_style_sheets(&self) -> bool {
        scripts(&self.rt).is_ok_and(|s| match s.phase {
            Phase::Parsing => s.pending_parsing_blocking.is_some(),
            Phase::Deferred => true,
            _ => false,
        })
    }

    /// True when "the end" reached the wait for the load event: the parse,
    /// the deferred scripts and `DOMContentLoaded` are done, and no script
    /// waits to run as soon as possible. The engine then fires `load`
    /// ([`ScriptRuntime::fire_load_event`]) as soon as nothing delays it.
    pub fn load_event_due(&self) -> bool {
        scripts(&self.rt).is_ok_and(|s| {
            s.phase == Phase::AfterContentLoaded && s.asap.is_empty() && s.in_order.is_empty()
        })
    }

    /// The last step of "the end": the readiness becomes "complete"
    /// (`readystatechange`), then `load` and `pageshow` fire at the window.
    /// Nothing happens unless [`ScriptRuntime::load_event_due`].
    pub fn fire_load_event(&mut self) {
        if !self.load_event_due() {
            return;
        }
        if let Ok(scripts) = scripts_mut(&mut self.rt) {
            scripts.phase = Phase::Loaded;
            scripts.in_load_event = true;
        }
        // The task's error is reported and counted by the task.
        let _ = self.task("the load event", |rt| {
            crate::performance::record(rt, Milestone::LoadStart);
            set_readiness(rt, DocumentReadyState::Complete)?;
            let load = TrustedEvent {
                legacy_target_override: true,
                ..TrustedEvent::new("load")
            };
            let fired = crate::scripts::fire(rt, DispatchTarget::Window, &load);
            crate::performance::record(rt, Milestone::LoadEnd);
            fired.map_err(task_error)?;
            scripts_mut(rt).map_err(task_error)?.page_showing = true;
            let pageshow = page_transition("pageshow");
            crate::scripts::fire(rt, DispatchTarget::Window, &pageshow).map_err(task_error)?;
            Ok(())
        });
        if let Ok(scripts) = scripts_mut(&mut self.rt) {
            scripts.in_load_event = false;
        }
    }

    /// True once the `load` event of the document fired (or its load was
    /// aborted), and for a document that the runtime did not start.
    pub fn document_loaded(&self) -> bool {
        scripts(&self.rt)
            .is_ok_and(|s| matches!(s.phase, Phase::NotStarted | Phase::Loaded | Phase::Unloaded))
    }

    /// HTML "unload a document" when the page leaves it: `pagehide` (if
    /// the page was shown) and `unload` at the window. No script runs after
    /// it.
    pub fn unload_document(&mut self) {
        let Ok(scripts) = scripts_mut(&mut self.rt) else {
            return;
        };
        if scripts.phase == Phase::Unloaded {
            return;
        }
        scripts.phase = Phase::Unloaded;
        let showing = std::mem::replace(&mut scripts.page_showing, false);
        let _ = self.task("the unload", |rt| {
            if showing {
                let pagehide = page_transition("pagehide");
                crate::scripts::fire(rt, DispatchTarget::Window, &pagehide).map_err(task_error)?;
            }
            let unload = TrustedEvent {
                legacy_target_override: true,
                ..TrustedEvent::new("unload")
            };
            crate::scripts::fire(rt, DispatchTarget::Window, &unload).map_err(task_error)?;
            Ok(())
        });
    }

    /// The load stopped (the user stopped it): the parse and the waiting
    /// scripts are dropped, the readiness is "complete" and the document
    /// counts as loaded. Departure: HTML "abort a parser" fires the
    /// readiness changes; swb fires no events.
    pub fn abort_document(&mut self) {
        // The engine drops the requests of the page; the `XMLHttpRequest`
        // objects that wait for them get no event (they stay as they are).
        crate::xhr::drop_requests(&mut self.rt);
        let Ok(scripts) = scripts_mut(&mut self.rt) else {
            return;
        };
        if matches!(
            scripts.phase,
            Phase::NotStarted | Phase::Loaded | Phase::Unloaded
        ) {
            return;
        }
        scripts.phase = Phase::Loaded;
        scripts.readiness = DocumentReadyState::Complete;
        scripts.pending_parsing_blocking = None;
        scripts.deferred.clear();
        scripts.in_order.clear();
        scripts.asap.clear();
        scripts.release_all();
    }

    /// [`Task::ParserResume`]: runs the pending parsing-blocking script if
    /// it is ready and no style sheet blocks scripts; without one, parses
    /// on. After an external script the parse goes on in the next parser
    /// task, after an inline one (it waited for style sheets) at once, as
    /// in Chromium (measured: cases `order`, `sheet-blocks-inline-in-body`).
    pub(crate) fn resume_parser(&mut self) {
        let Ok(scripts) = scripts_mut(&mut self.rt) else {
            return;
        };
        scripts.resume_queued = false;
        if scripts.phase != Phase::Parsing {
            return;
        }
        let Some(blocking) = scripts.pending_parsing_blocking else {
            self.parse();
            return;
        };
        let (ready, external) = scripts
            .elements
            .get(&blocking)
            .map_or((false, false), |el| {
                (el.ready_to_be_parser_executed, el.from_external_file)
            });
        if !ready || self.style_sheets_block() {
            return;
        }
        if let Ok(scripts) = scripts_mut(&mut self.rt) {
            scripts.pending_parsing_blocking = None;
        }
        self.execute_script(blocking);
        if external {
            self.queue_resume();
        } else {
            self.parse();
        }
    }

    /// Parses until the parser waits for a script or the input ends. Each
    /// inline script runs as a task between two steps.
    fn parse(&mut self) {
        loop {
            let step = match engine_mut(&mut self.rt) {
                Ok(host) => host.parse_step(),
                Err(_) => return,
            };
            let Some((ParseStep::Script(node), position)) = step else {
                if self.defer_parse_end() {
                    self.queue_resume();
                } else {
                    self.parse_finished();
                }
                return;
            };
            if self.stopped() {
                continue;
            }
            match crate::scripts::prepare(&mut self.rt, node, Some(position)) {
                Ok(Prepared::ExecuteNow) => self.execute_script(node),
                Ok(Prepared::Blocking) => {
                    if let Ok(scripts) = scripts_mut(&mut self.rt) {
                        scripts.parser_waited = true;
                    }
                    return;
                }
                Ok(Prepared::Nothing) => {}
                Err(error) => log::warn!("a script element was not prepared: {error:?}"),
            }
        }
    }

    /// True the first time the input ends and "the end" must wait for a
    /// parser task (as in Chromium, measured): when the document came over
    /// HTTP(S), whose end of input is a task of its own, or when the parser
    /// waited for a script or a style sheet. A document from a file ends in
    /// the task of its last parse, also while images and scripts load.
    fn defer_parse_end(&mut self) -> bool {
        let url = crate::scripts::document_url(&self.rt);
        let network = url.starts_with("http:") || url.starts_with("https:");
        let Ok(scripts) = scripts_mut(&mut self.rt) else {
            return false;
        };
        if scripts.parse_end_deferred || !(network || scripts.parser_waited) {
            return false;
        }
        scripts.parse_end_deferred = true;
        true
    }

    /// "The end", steps 1 to 4: the readiness becomes "interactive"; then
    /// the deferred scripts.
    fn parse_finished(&mut self) {
        if let Ok(scripts) = scripts_mut(&mut self.rt) {
            scripts.phase = Phase::Deferred;
            scripts.pending_parsing_blocking = None;
        }
        crate::performance::record(&mut self.rt, Milestone::DomInteractive);
        let _ = self.task("readystatechange", |rt| {
            set_readiness(rt, DocumentReadyState::Interactive)
        });
        self.advance_document();
    }

    /// "The end", steps 5 to 7: runs the deferred scripts that are ready
    /// (in order, each after the style sheets that block scripts), then
    /// fires `DOMContentLoaded`. Then the load event waits until the
    /// scripts that run as soon as possible have run.
    fn advance_document(&mut self) {
        loop {
            let Ok(scripts) = scripts(&self.rt) else {
                return;
            };
            if scripts.phase != Phase::Deferred {
                return;
            }
            let Some(&first) = scripts.deferred.front() else {
                break;
            };
            let ready = scripts
                .elements
                .get(&first)
                .is_some_and(|el| el.ready_to_be_parser_executed);
            if !ready || self.style_sheets_block() {
                return;
            }
            if let Ok(scripts) = scripts_mut(&mut self.rt) {
                scripts.deferred.pop_front();
            }
            self.execute_script(first);
        }
        if let Ok(scripts) = scripts_mut(&mut self.rt) {
            scripts.phase = Phase::AfterContentLoaded;
        }
        let _ = self.task("DOMContentLoaded", |rt| {
            crate::performance::record(rt, Milestone::ContentLoadedStart);
            let event = TrustedEvent {
                bubbles: true,
                ..TrustedEvent::new("DOMContentLoaded")
            };
            let fired = crate::scripts::fire(rt, DispatchTarget::Node(NodeId::DOCUMENT), &event);
            crate::performance::record(rt, Milestone::ContentLoadedEnd);
            fired.map_err(task_error)?;
            Ok(())
        });
    }

    /// Runs the ready scripts at the front of the in-order list.
    fn run_in_order_scripts(&mut self) {
        loop {
            let Ok(scripts) = scripts_mut(&mut self.rt) else {
                return;
            };
            let Some(&first) = scripts.in_order.front() else {
                return;
            };
            if !scripts.has_result(first) {
                return;
            }
            scripts.in_order.pop_front();
            self.execute_script(first);
        }
    }

    /// Executes a script element as a task; its errors are reported.
    fn execute_script(&mut self, node: NodeId) {
        let _ = self.task(&self.script_name(node), |rt| {
            // HTML, the parser's rule for the end tag of a `script`:
            // "perform a microtask checkpoint" before the script runs. The
            // mutation observers see the nodes that the parser inserted
            // since the last script (measured, feature 9a).
            crate::event_loop::microtask_checkpoint(rt)?;
            crate::scripts::execute(rt, node)
        });
    }

    /// The name of a script element's task in the log: its `src`, or the
    /// document's URL for an inline script.
    fn script_name(&self, node: NodeId) -> String {
        let Ok(dom) = crate::state::dom(&self.rt) else {
            return String::new();
        };
        match dom.element(node).and_then(|e| e.attr("src")) {
            Some(src) => src.to_owned(),
            None => crate::scripts::document_url(&self.rt),
        }
    }

    fn is_pending_parsing_blocking(&self, node: NodeId) -> bool {
        scripts(&self.rt).is_ok_and(|s| s.pending_parsing_blocking == Some(node))
    }

    /// True if the document has a style sheet that is blocking scripts.
    fn style_sheets_block(&mut self) -> bool {
        engine_mut(&mut self.rt).is_ok_and(crate::host::EngineHost::style_sheets_block_scripts)
    }

    /// Queues a parser task unless one waits.
    fn queue_resume(&mut self) {
        let Ok(scripts) = scripts_mut(&mut self.rt) else {
            return;
        };
        if scripts.resume_queued {
            return;
        }
        scripts.resume_queued = true;
        if let Err(error) = queue_task(&mut self.rt, Task::ParserResume) {
            log::warn!("the parser task was not queued: {error:?}");
        }
    }
}

/// HTML "update the current document readiness": sets it and fires
/// `readystatechange` at the document.
fn set_readiness(
    rt: &mut swb_js::Runtime,
    readiness: DocumentReadyState,
) -> Result<(), swb_js::ScriptError> {
    let scripts = scripts_mut(rt).map_err(task_error)?;
    if scripts.readiness == readiness {
        return Ok(());
    }
    scripts.readiness = readiness;
    let event = TrustedEvent::new("readystatechange");
    crate::scripts::fire(rt, DispatchTarget::Node(NodeId::DOCUMENT), &event).map_err(task_error)?;
    Ok(())
}

/// HTML "fire a page transition event" with `persisted` false: it bubbles
/// and can be canceled, with the legacy target override.
fn page_transition(kind: &str) -> TrustedEvent {
    TrustedEvent {
        kind: kind.to_owned(),
        bubbles: true,
        cancelable: true,
        composed: false,
        interface: EventInterface::PageTransition { persisted: false },
        legacy_target_override: true,
    }
}
