//! Module scripts (ADR 0028 section 6, item 7, and section 8): the module map
//! of a document ([`ModuleEntry`]), the fetch of a module that needs no other
//! module, the elements that wait for it, and the refusal of a module that
//! needs more. The processing model of script elements is in `super`.

use swb_dom::NodeId;
use swb_js::{Runtime, ScriptError, VmResult};

use super::{
    ClassicScript, Found, MAX_REFUSED_WARNINGS, ParserScript, Prepared, ScriptFetchMode,
    ScriptResult, Scripts, WhenReady, place_script, scripts_mut,
};
use crate::event_loop::{Task, queue_task};
use crate::state::{engine, engine_mut};

/// The state of a module in the module map (HTML "module map": one entry
/// per URL of a document, `(url, "javascript")`).
#[derive(Debug)]
pub(super) enum ModuleState {
    /// The fetch is in flight; the elements in [`ModuleEntry::waiters`]
    /// wait for it, besides the one that started it.
    Fetching,
    /// The fetch succeeded; no element has run the module yet.
    Ready(Box<ClassicScript>),
    /// An element ran the module (or tried: an error of its run is
    /// reported once). Later elements with the URL do not run it again.
    Evaluated,
    /// The fetch failed (a network error, an error status, a CORS failure,
    /// a type that is not JavaScript): null in the module map.
    Failed,
}

/// An entry of the module map.
#[derive(Debug)]
pub(crate) struct ModuleEntry {
    pub(super) state: ModuleState,
    /// The elements that found the entry while it was fetching, in order.
    waiters: Vec<NodeId>,
}

/// HTML "fetch a module script graph" for a module with no requests of
/// its own (<https://html.spec.whatwg.org/multipage/webappapis.html#fetch-a-module-script-tree>):
/// the module map decides. A URL that the map has is not fetched again;
/// an entry that is fetching gets the element as a waiter, one that has its
/// result makes the element ready in a task (the result of a fetch comes
/// in a task too). Otherwise the engine fetches the URL.
pub(super) fn start_module_fetch(
    rt: &mut Runtime,
    node: NodeId,
    url: &str,
    mode: ScriptFetchMode,
) -> VmResult<()> {
    let scripts = scripts_mut(rt)?;
    if let Some(el) = scripts.elements.get_mut(&node) {
        el.module_key = Some(url.to_owned());
    }
    let fetching = scripts.modules.get_mut(url).map(|entry| {
        let fetching = matches!(entry.state, ModuleState::Fetching);
        if fetching {
            entry.waiters.push(node);
        }
        fetching
    });
    match fetching {
        Some(true) => return Ok(()),
        Some(false) => return queue_task(rt, Task::ModuleReady(node)),
        None => {}
    }
    scripts.modules.insert(
        url.to_owned(),
        ModuleEntry {
            state: ModuleState::Fetching,
            waiters: Vec::new(),
        },
    );
    if !engine_mut(rt)?.fetch_script(node, url, mode) {
        // The fetch fails at once (a URL that the document may not load).
        if let Some(entry) = scripts_mut(rt)?.modules.get_mut(url) {
            entry.state = ModuleState::Failed;
        }
        queue_task(rt, Task::ModuleReady(node))?;
    }
    Ok(())
}

/// Steps 33 and 34 of "prepare" for an inline module script (HTML "fetch
/// an inline module script graph"): a module that needs no other module is
/// ready in a task, as Chromium runs it (measured, cases `order` and
/// `inserted`: an inline module of the parser runs after the parse ends,
/// an inserted one in a task after the script that inserted it).
pub(super) fn prepare_inline_module(
    rt: &mut Runtime,
    node: NodeId,
    source: String,
    parser: Option<ParserScript>,
    found: &Found,
) -> VmResult<Prepared> {
    let (url, start) = match parser {
        Some(ParserScript::Document(p)) => (engine(rt)?.url(), (p.line, p.column)),
        Some(ParserScript::Written) => (String::new(), (0, 0)),
        None => (engine(rt)?.url(), (0, 0)),
    };
    let scripts = scripts_mut(rt)?;
    if !scripts.elements.contains_key(&node) {
        return Ok(Prepared::Nothing);
    }
    let in_parse_step = scripts.in_parse_step;
    let text = Box::new(ClassicScript {
        source,
        url,
        start,
        written: parser == Some(ParserScript::Written),
        held: 0,
    });
    let (when_ready, prepared) = place_script(scripts, node, found, true);
    let Some(el) = scripts.elements.get_mut(&node) else {
        return Ok(prepared);
    };
    if when_ready == WhenReady::ParserExecuted {
        // A module of the parser that waits for the end of the parse is
        // ready at once.
        el.result = ScriptResult::InlineModule(text);
        el.ready_to_be_parser_executed = true;
        return Ok(prepared);
    }
    el.when_ready = Some(when_ready);
    el.pending_inline_module = Some(text);
    if in_parse_step {
        // The parser queues the task when its step ends, behind its own
        // (`ScriptRuntime::parse`).
        scripts.module_ready_queue.push(node);
        Ok(prepared)
    } else {
        queue_task(rt, Task::ModuleReady(node))?;
        Ok(prepared)
    }
}

/// The result of the fetch of the module map entry `key` (`None`: the
/// fetch failed; the entry is null from now on). `node` started the
/// fetch. Returns the steps of every element that waited, in order.
pub(super) fn finish_module_fetch(
    scripts: &mut Scripts,
    node: NodeId,
    key: &str,
    script: Option<Box<ClassicScript>>,
) -> Vec<(NodeId, WhenReady)> {
    let fetching = scripts
        .modules
        .get(key)
        .is_some_and(|entry| matches!(entry.state, ModuleState::Fetching));
    let Some(entry) = scripts.modules.get_mut(key).filter(|_| fetching) else {
        // A stale result (the entry is gone or has its result).
        if let Some(script) = &script {
            scripts.held_bytes = scripts.held_bytes.saturating_sub(script.held);
        }
        return Vec::new();
    };
    entry.state = match script {
        Some(script) => ModuleState::Ready(script),
        None => ModuleState::Failed,
    };
    let mut nodes = std::mem::take(&mut entry.waiters);
    nodes.insert(0, node);
    resolve_module_elements(scripts, key, &nodes)
}

/// Sets the result of the elements `nodes` from the module map entry `key`
/// (which is no longer fetching) and returns their steps.
fn resolve_module_elements(
    scripts: &mut Scripts,
    key: &str,
    nodes: &[NodeId],
) -> Vec<(NodeId, WhenReady)> {
    let failed = scripts
        .modules
        .get(key)
        .is_none_or(|entry| matches!(entry.state, ModuleState::Failed));
    let mut ready = Vec::new();
    for node in nodes {
        let Some(el) = scripts.elements.get_mut(node) else {
            continue;
        };
        if el.has_result() {
            continue;
        }
        el.result = if failed {
            ScriptResult::Failed
        } else {
            ScriptResult::ExternalModule(key.to_owned())
        };
        if let Some(when) = el.when_ready.take() {
            ready.push((*node, when));
        }
    }
    ready
}

/// The module script of `node` is ready without a fetch ([`Task::ModuleReady`]):
/// an inline module, or an external one whose URL the module map has
/// already. Returns the steps to run.
pub(crate) fn mark_module_ready(
    rt: &mut Runtime,
    node: NodeId,
) -> VmResult<Vec<(NodeId, WhenReady)>> {
    let scripts = scripts_mut(rt)?;
    let Some(el) = scripts.elements.get_mut(&node) else {
        return Ok(Vec::new());
    };
    if el.has_result() {
        return Ok(Vec::new());
    }
    if let Some(key) = el.module_key.clone() {
        return Ok(resolve_module_elements(scripts, &key, &[node]));
    }
    let Some(text) = el.pending_inline_module.take() else {
        return Ok(Vec::new());
    };
    el.result = ScriptResult::InlineModule(text);
    Ok(el
        .when_ready
        .take()
        .map(|when| (node, when))
        .into_iter()
        .collect())
}

/// The module script `script` needs another module: the entry `key` of the
/// module map (if any) is refused for the other elements too, and a warning
/// says so. A page can have thousands of them, so only the first ones warn.
pub(super) fn refuse_module_run(
    rt: &mut Runtime,
    script: &ClassicScript,
    key: Option<&str>,
    error: &ScriptError,
) {
    let name = if script.url.is_empty() {
        "(written)"
    } else {
        &script.url
    };
    let message = format!(
        "the module script {name} is not run: {error} (swb runs modules that need no other \
         module)"
    );
    let Ok(scripts) = scripts_mut(rt) else {
        return;
    };
    if let Some(key) = key {
        refuse_module(scripts, key);
    }
    scripts.refused_modules += 1;
    if scripts.refused_modules <= MAX_REFUSED_WARNINGS {
        log::warn!("{message}");
    } else {
        log::debug!("{message}");
    }
}

/// The text of the module map entry `key` for its first run. The entry is
/// then evaluated and its text no longer counts as held. `Err(false)`: the
/// module ran already; `Err(true)`: the entry failed or the module was
/// refused.
pub(super) fn take_module(scripts: &mut Scripts, key: &str) -> Result<Box<ClassicScript>, bool> {
    let entry = scripts.modules.get_mut(key).ok_or(true)?;
    match std::mem::replace(&mut entry.state, ModuleState::Evaluated) {
        ModuleState::Ready(script) => {
            scripts.held_bytes = scripts.held_bytes.saturating_sub(script.held);
            Ok(script)
        }
        other => {
            let refused = matches!(other, ModuleState::Failed);
            entry.state = other;
            Err(refused)
        }
    }
}

/// Marks the module map entry `key` as refused: elements that run later
/// with the same URL fire `error` too.
fn refuse_module(scripts: &mut Scripts, key: &str) {
    if let Some(entry) = scripts.modules.get_mut(key) {
        entry.state = ModuleState::Failed;
    }
}
