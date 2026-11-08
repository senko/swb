//! Module scripts (HTML "fetch a module script graph", "fetch an inline
//! module script graph", "fetch the descendants of and link a module
//! script", "run a module script"; ADR 0034 sections 8 and 10, ADR 0028
//! section 8): the graph of a script element, its readiness, its run and
//! the reports of its errors. The processing model of script elements is
//! in `super`, the module map in `module_map.rs`, the hooks in
//! `module_hooks.rs`.
//!
//! An external module script waits for the entry of its URL; an inline one
//! is parsed when it is prepared (its requests resolve then). Then the
//! graph loads (`Runtime::load_requested_modules`). When the load ends
//! (at once if the map has every module, else in the reaction of its
//! promise, which queues [`Task::ModuleReady`]), the root is linked in a
//! task and the element's result is set: the linked root, an error to
//! rethrow (a parse, resolution or link error), or null (a failed fetch).
//! The element is then ready as M9's single fetch was, with the lists of
//! ADR 0028 section 8.
//!
//! The run evaluates the root; an error (also a rejection of top-level
//! `await`, when it happens) goes to `window.onerror` at the error's place,
//! a parse error at the bad module's position, a value without a place at
//! the document's URL 0:0. Then `load` fires at an external element; a null
//! result fires `error` and reports nothing (the table of ADR 0034 section
//! 8).

use swb_dom::NodeId;
use swb_js::{
    Module, PromiseStatus, ReactionKind, Runtime, ScriptError, SourceLocation, Value, VmError,
    VmResult,
};

use super::module_map::{self, GraphOwner, Lookup, Options, Waiter};
use super::{
    Found, ParserScript, Prepared, ScriptResult, WhenReady, fire_at_element, place_script, scripts,
    scripts_mut,
};
use crate::event_loop::{Task, queue_task};
use crate::state::engine_mut;

/// The `VmError` of an error of the engine's module API (a termination or
/// an engine error; the API reports script errors as values).
pub(super) fn vm_error(error: &ScriptError) -> VmError {
    match error {
        ScriptError::Terminated(termination) => VmError::Terminated(*termination),
        ScriptError::Internal(error) => VmError::Internal(*error),
        ScriptError::Uncaught { .. } | ScriptError::Compile { .. } => {
            VmError::invariant("a script error of the module API")
        }
    }
}

/// HTML "fetch a module script graph" for the element `node` with the URL
/// `url`: the element waits for the entry of the URL, or is ready in a
/// task when the map has it (the result of a fetch comes in a task too).
pub(super) fn start_module_fetch(
    rt: &mut Runtime,
    node: NodeId,
    url: &str,
    credentials: bool,
) -> VmResult<()> {
    if let Some(el) = scripts_mut(rt)?.elements.get_mut(&node) {
        el.module_key = Some(url.to_owned());
        el.credentials = credentials;
    }
    let options = Options {
        credentials,
        quiet: false,
    };
    match module_map::request(rt, url, Waiter::Element(node), options)? {
        Lookup::Later => Ok(()),
        Lookup::Now(_) => queue_task(rt, Task::ModuleReady(node)),
    }
}

/// Steps 33 and 34 of "prepare" for an inline module script (HTML "fetch
/// an inline module script graph"): the text is parsed and its graph
/// starts now. A result that is ready now makes a parser-inserted module
/// without `async` ready at once, and other modules ready in a task, as
/// Chromium runs them (measured, cases `order` and `inserted` of
/// `web-module-scripts.json`); a graph that loads later is ready in a task
/// after its load.
pub(super) fn prepare_inline_module(
    rt: &mut Runtime,
    node: NodeId,
    source: &str,
    (parser, credentials): (Option<ParserScript>, bool),
    found: &Found,
) -> VmResult<Prepared> {
    let (name, start) = match parser {
        Some(ParserScript::Document(p)) => (engine_mut(rt)?.url(), (p.line, p.column)),
        Some(ParserScript::Written) => (String::new(), (0, 0)),
        None => (engine_mut(rt)?.url(), (0, 0)),
    };
    let base = engine_mut(rt)?.base_url();
    let scripts = scripts_mut(rt)?;
    if !scripts.elements.contains_key(&node) {
        return Ok(Prepared::Nothing);
    }
    let in_parse_step = scripts.in_parse_step;
    let (when_ready, prepared) = place_script(scripts, node, found, true);
    if let Some(el) = scripts.elements.get_mut(&node) {
        el.when_ready = Some(when_ready);
        el.credentials = credentials;
        el.inline_name = Some(name.clone());
    }
    let options = Options {
        credentials,
        quiet: false,
    };
    let scope = rt.heap_mut().open_scope();
    let parsed = module_map::parse_inline(rt, source, (&name, start), &base, options);
    let done = match parsed {
        Ok(Ok(root)) => start_graph(rt, node, root),
        Ok(Err(error)) => set_result(rt, node, ScriptResult::ModuleError(error)).map(|()| true),
        Err(error) => Err(error),
    };
    if let Err(error) = rt.heap_mut().close_scope(scope) {
        log::warn!("the handle scope of a module was not the last one: {error:?}");
    }
    if !done? {
        return Ok(prepared);
    }
    let scripts = scripts_mut(rt)?;
    if when_ready == WhenReady::ParserExecuted {
        // A module of the parser that waits for the end of the parse is
        // ready at once.
        if let Some(el) = scripts.elements.get_mut(&node) {
            el.when_ready = None;
            el.ready_to_be_parser_executed = true;
        }
    } else if in_parse_step {
        // The parser queues the task when its step ends, behind its own
        // (`ScriptRuntime::parse`).
        scripts.module_ready_queue.push(node);
    } else {
        queue_task(rt, Task::ModuleReady(node))?;
    }
    Ok(prepared)
}

/// Loads the graph of the root `root` of the element `node`. True if the
/// result of the element is set now (the map had every module); else the
/// reaction of the load's promise goes on ([`graph_loaded`]).
fn start_graph(rt: &mut Runtime, node: NodeId, root: Module) -> VmResult<bool> {
    let promise = rt.load_requested_modules(root)?;
    match rt.promise_settlement(promise) {
        Some((PromiseStatus::Fulfilled, _)) => finish_graph(rt, node, root, None).map(|()| true),
        Some((PromiseStatus::Rejected, error)) => {
            finish_graph(rt, node, root, Some(error)).map(|()| true)
        }
        _ => {
            super::module_hooks::react_to_graph(rt, promise, GraphOwner::Element(node), root)?;
            if let Some(el) = scripts_mut(rt)?.elements.get_mut(&node) {
                el.graph_pending = true;
            }
            Ok(false)
        }
    }
}

/// The load of the graph of the element `node` ended (a job): the link
/// runs in a task ([`Task::ModuleReady`]), which also makes the element
/// ready.
pub(super) fn graph_loaded(
    rt: &mut Runtime,
    node: NodeId,
    root: Module,
    error: Option<Value>,
) -> VmResult<()> {
    if let Some(el) = scripts_mut(rt)?.elements.get_mut(&node) {
        el.graph_pending = false;
        el.loaded_graph = Some((root, error));
    }
    queue_task(rt, Task::ModuleReady(node))
}

/// HTML "fetch the descendants of and link a module script", the end: the
/// root is linked, and the element's result is the root, the error to
/// rethrow (a parse, resolution or link error), or null (a failed fetch).
fn finish_graph(
    rt: &mut Runtime,
    node: NodeId,
    root: Module,
    error: Option<Value>,
) -> VmResult<()> {
    let result = match error {
        None => match rt.link(root) {
            Ok(()) => ScriptResult::Module(root),
            Err(ScriptError::Uncaught { .. }) => ScriptResult::ModuleError(rt.last_value()),
            Err(error) => return Err(vm_error(&error)),
        },
        Some(error) if module_map::map_mut(rt)?.is_fetch_failure(error) => ScriptResult::Failed,
        Some(error) => ScriptResult::ModuleError(error),
    };
    set_result(rt, node, result)
}

/// Sets the result of the element `node` unless it has one.
fn set_result(rt: &mut Runtime, node: NodeId, result: ScriptResult) -> VmResult<()> {
    if let Some(el) = scripts_mut(rt)?.elements.get_mut(&node)
        && !el.has_result()
    {
        el.result = result;
    }
    Ok(())
}

/// The steps of the element `node` when it has its result.
fn take_steps(rt: &mut Runtime, node: NodeId) -> VmResult<Vec<(NodeId, WhenReady)>> {
    let Some(el) = scripts_mut(rt)?.elements.get_mut(&node) else {
        return Ok(Vec::new());
    };
    if !el.has_result() {
        return Ok(Vec::new());
    }
    Ok(el
        .when_ready
        .take()
        .map(|when| (node, when))
        .into_iter()
        .collect())
}

/// The entry of the root of the external module script `node` has its
/// result (from the map's fetch, or a [`Task::ModuleReady`]): its graph
/// loads. Returns the steps of the element if it is ready now.
pub(crate) fn element_root_ready(
    rt: &mut Runtime,
    node: NodeId,
) -> VmResult<Vec<(NodeId, WhenReady)>> {
    let Some((Some(key), credentials)) = scripts(rt)?
        .elements
        .get(&node)
        .filter(|el| !el.graph_pending && !el.has_result())
        .map(|el| (el.module_key.clone(), el.credentials))
    else {
        return Ok(Vec::new());
    };
    let options = Options {
        credentials,
        quiet: false,
    };
    let done = match module_map::request(rt, &key, Waiter::Element(node), options)? {
        Lookup::Later => false,
        Lookup::Now(Ok(root)) => start_graph(rt, node, root)?,
        Lookup::Now(Err(error)) => {
            let result = if module_map::map_mut(rt)?.is_fetch_failure(error) {
                ScriptResult::Failed
            } else {
                ScriptResult::ModuleError(error)
            };
            set_result(rt, node, result)?;
            true
        }
    };
    if done {
        take_steps(rt, node)
    } else {
        Ok(Vec::new())
    }
}

/// [`Task::ModuleReady`] (in a task): the element links its loaded graph,
/// or starts the graph of a root that the map has, or has its result
/// already (an inline module). Returns its steps if it is ready.
pub(crate) fn mark_module_ready(
    rt: &mut Runtime,
    node: NodeId,
) -> VmResult<Vec<(NodeId, WhenReady)>> {
    let Some(el) = scripts_mut(rt)?.elements.get_mut(&node) else {
        return Ok(Vec::new());
    };
    if el.has_result() {
        return take_steps(rt, node);
    }
    if let Some((root, error)) = el.loaded_graph.take() {
        finish_graph(rt, node, root, error)?;
        return take_steps(rt, node);
    }
    if el.module_key.is_some() {
        return element_root_ready(rt, node);
    }
    Ok(Vec::new())
}

/// HTML "run a module script" for the element `node` (its result is a
/// module or an error to rethrow): `document.currentScript` is null; an
/// error is reported; `load` fires at an external element. A termination
/// goes up.
pub(super) fn execute_module(
    rt: &mut Runtime,
    node: NodeId,
    result: &ScriptResult,
    external: bool,
) -> Result<(), ScriptError> {
    let name = module_name(rt, node);
    let old = scripts_mut(rt)
        .map_err(crate::runtime::task_error)?
        .current_script
        .take();
    let mut outcome = match *result {
        ScriptResult::Module(root) => evaluate(rt, root, &name),
        ScriptResult::ModuleError(error) => {
            report_error(rt, &name, error).map_err(crate::runtime::task_error)
        }
        _ => Ok(()),
    };
    if outcome.is_ok() && rt.stack_depths().0 == 0 {
        outcome = crate::event_loop::microtask_checkpoint(rt);
    }
    if let Ok(scripts) = scripts_mut(rt) {
        scripts.current_script = old;
    }
    outcome?;
    if external {
        fire_at_element(rt, node, "load").map_err(crate::runtime::task_error)?;
    }
    Ok(())
}

/// The name of a module script in the log: its URL, or the document's.
fn module_name(rt: &Runtime, node: NodeId) -> String {
    scripts(rt)
        .ok()
        .and_then(|s| s.elements.get(&node))
        .and_then(|el| el.module_key.clone().or_else(|| el.inline_name.clone()))
        .unwrap_or_else(|| super::document_url(rt))
}

/// Evaluates the root `root` (its graph is linked): an error of the
/// evaluation is reported now, a rejection of its top-level `await` when it
/// happens ([`evaluation_settled`]).
fn evaluate(rt: &mut Runtime, root: Module, name: &str) -> Result<(), ScriptError> {
    let promise = match rt.evaluate(root) {
        Ok(promise) => promise,
        Err(error @ ScriptError::Uncaught { .. }) => {
            return crate::errors::report(rt, &crate::errors::Site::named(name), &error)
                .map_err(crate::runtime::task_error);
        }
        Err(error) => return Err(error),
    };
    match rt.promise_settlement(promise) {
        Some((PromiseStatus::Rejected, error)) => {
            report_error(rt, name, error).map_err(crate::runtime::task_error)
        }
        Some((PromiseStatus::Pending, _)) => {
            let key = crate::state::document_key(rt).map_err(crate::runtime::task_error)?;
            rt.react_to_promise(promise, evaluation_settled, Value::number(f64::from(key)))
                .map(drop)
        }
        _ => Ok(()),
    }
}

/// The evaluation promise of a module script settled (a job): a rejection
/// (of top-level `await`) is reported at the window of its document, as
/// Chromium does (measured, case `tla-reject`; no `unhandledrejection`).
fn evaluation_settled(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    value: Value,
) -> VmResult<Value> {
    let Some(key) = data.as_number() else {
        return Ok(Value::Undefined);
    };
    if !matches!(kind, ReactionKind::Reject) {
        return Ok(Value::Undefined);
    }
    let Ok(entered) = crate::state::switch_if_other(rt, key as u32) else {
        return Ok(Value::Undefined);
    };
    let result = report_error(rt, "a module script", value);
    crate::state::leave(rt, entered);
    result.map(|()| Value::Undefined)
}

/// HTML "report an exception" for an error of a module graph: at the
/// error's place, at the position of a parse error of the map, or at the
/// document's URL 0:0 (Chromium, measured: `resolve-error`,
/// `descendant-parse-error`).
pub(super) fn report_error(rt: &mut Runtime, name: &str, error: Value) -> VmResult<()> {
    let location = rt
        .error_location(error)
        .or_else(|| module_map::map_mut(rt).ok()?.parse_error_location(error))
        .unwrap_or_else(|| SourceLocation {
            name: super::document_url(rt),
            line: 0,
            column: 0,
        });
    crate::errors::report_value_at(rt, name, error, location)
}
