//! Script elements (HTML §4.12.1.1, "processing model",
//! <https://html.spec.whatwg.org/multipage/scripting.html#script-processing-model>;
//! ADR 0028 section 8): the state of each `script` element, "prepare the
//! script element", "mark as ready", "execute the script element", the
//! steps that run when a script element is inserted or its children change,
//! and `HTMLScriptElement`.
//!
//! The parser and the lists of the document (the pending parsing-blocking
//! script, the scripts that run after the parse, in order or as soon as
//! possible) are driven by `document_load.rs`; this module keeps them in
//! [`Scripts`]. The engine fetches external scripts
//! ([`crate::EngineHost::fetch_script`]) and hands the result back
//! ([`crate::ScriptRuntime::script_fetched`]).
//!
//! Departures (comments at the places): module scripts do not run (a
//! warning; ADR 0028 section 6, item 7), and neither do import maps,
//! speculation rules and SVG scripts; the `charset` attribute is ignored
//! (the engine decodes with the response's charset, else the document's
//! encoding); the attribute change steps (a new `src`, an `async`
//! attribute that `setAttribute` adds) come with the attribute API of
//! feature 6a.

use std::collections::{HashMap, HashSet, VecDeque};

use swb_dom::{Document, NodeData, NodeId, TextPosition, local_name};
use swb_js::{Runtime, ScriptError, VmError, VmResult};

use crate::bindings::{self as b, Cx};
use crate::event_loop::{Task, queue_task};
use crate::events::{DispatchTarget, TrustedEvent};
use crate::generated as g;
use crate::generated::types::DocumentReadyState;
use crate::state::{WebState, dom, dom_mut, engine, engine_mut, state, state_mut};

/// The JavaScript MIME type essences (MIME Sniffing §4.6,
/// <https://mimesniff.spec.whatwg.org/#javascript-mime-type>).
const JAVASCRIPT_TYPES: &[&str] = &[
    "application/ecmascript",
    "application/javascript",
    "application/x-ecmascript",
    "application/x-javascript",
    "text/ecmascript",
    "text/javascript",
    "text/javascript1.0",
    "text/javascript1.1",
    "text/javascript1.2",
    "text/javascript1.3",
    "text/javascript1.4",
    "text/javascript1.5",
    "text/jscript",
    "text/livescript",
    "text/x-ecmascript",
    "text/x-javascript",
];

/// True if `essence` is a JavaScript MIME type essence (ASCII case
/// insensitive, no parameters): the type test of script elements, and the
/// engine's `nosniff` check of fetched scripts.
pub fn is_javascript_mime_type(essence: &str) -> bool {
    JAVASCRIPT_TYPES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(essence))
}

/// A classic script that the engine fetched for a script element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchedScript {
    /// The decoded source text.
    pub source: String,
    /// The URL of the response, which names the script in error reports
    /// and stack traces.
    pub url: String,
}

/// The source of a classic script and where it comes from.
#[derive(Debug)]
pub(crate) struct ClassicScript {
    pub(crate) source: String,
    /// The name in errors and stack traces: the script's URL, or the
    /// document's for an inline script.
    pub(crate) url: String,
    /// Where the text starts in its document (line and column from 0;
    /// inline scripts of the parser).
    pub(crate) start: (u32, u32),
    /// The bytes of the text that count against
    /// [`MAX_HELD_SCRIPT_BYTES`] while the script waits: the length of a
    /// fetched script's text, 0 for an inline script.
    held: usize,
}

/// The most bytes of fetched script text that a document holds before the
/// scripts run (ADR 0028 section 16). A script whose text would pass it
/// fails as a network error: its `error` event fires.
pub(crate) const MAX_HELD_SCRIPT_BYTES: usize = 256 << 20;

/// HTML "result" of a script element.
#[derive(Debug, Default)]
enum ScriptResult {
    /// "uninitialized": the fetch goes on.
    #[default]
    Uninitialized,
    /// Null: the fetch failed.
    Failed,
    /// A classic script.
    Classic(Box<ClassicScript>),
    /// The script ran; its text is dropped.
    Done,
}

/// HTML "steps to run when the result is ready".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WhenReady {
    /// Execute, then leave the set of scripts that run as soon as
    /// possible.
    Asap,
    /// Execute the ready scripts at the front of the in-order list.
    InOrder,
    /// Set "ready to be parser-executed" (the parser runs it).
    ParserExecuted,
}

/// The state of one script element (HTML §4.12.1.1).
#[derive(Debug)]
pub(crate) struct ScriptElement {
    /// HTML "parser document" is the document (parser-inserted).
    parser_inserted: bool,
    /// HTML "force async", initially true; the parser sets it to false.
    force_async: bool,
    pub(crate) from_external_file: bool,
    pub(crate) ready_to_be_parser_executed: bool,
    already_started: bool,
    /// HTML "preparation-time document" is the document of the window.
    prepared_in_document: bool,
    result: ScriptResult,
    pub(crate) when_ready: Option<WhenReady>,
}

impl ScriptElement {
    fn new() -> ScriptElement {
        ScriptElement {
            parser_inserted: false,
            force_async: true,
            from_external_file: false,
            ready_to_be_parser_executed: false,
            already_started: false,
            prepared_in_document: false,
            result: ScriptResult::Uninitialized,
            when_ready: None,
        }
    }

    /// True once the result is not "uninitialized".
    pub(crate) fn has_result(&self) -> bool {
        !matches!(self.result, ScriptResult::Uninitialized)
    }
}

/// Where the document is in its life (HTML §13.2.7, "the end", and the
/// unload).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The runtime has not started the document (a test that lends a
    /// parsed document and runs scripts itself).
    #[default]
    NotStarted,
    /// The parser runs or waits.
    Parsing,
    /// The parse ended; the deferred scripts run, then `DOMContentLoaded`.
    Deferred,
    /// After `DOMContentLoaded`: waiting for the scripts that run as soon
    /// as possible, then for the engine to fire `load`.
    AfterContentLoaded,
    /// The `load` event fired (or the load was aborted).
    Loaded,
    /// The document was unloaded.
    Unloaded,
}

/// The script state of the window's document.
#[derive(Debug)]
pub(crate) struct Scripts {
    /// The state of each script element that the runtime met. Entries stay
    /// for the life of the document (one per script element, so bounded by
    /// the node limit).
    pub(crate) elements: HashMap<NodeId, ScriptElement>,
    /// HTML "pending parsing-blocking script".
    pub(crate) pending_parsing_blocking: Option<NodeId>,
    /// HTML "list of scripts that will execute when the document has
    /// finished parsing".
    pub(crate) deferred: VecDeque<NodeId>,
    /// HTML "list of scripts that will execute in order as soon as
    /// possible".
    pub(crate) in_order: VecDeque<NodeId>,
    /// HTML "set of scripts that will execute as soon as possible".
    pub(crate) asap: HashSet<NodeId>,
    /// The bytes of fetched script text that has not run yet (see
    /// [`MAX_HELD_SCRIPT_BYTES`]).
    pub(crate) held_bytes: usize,
    /// `document.currentScript`.
    pub(crate) current_script: Option<NodeId>,
    /// HTML "current document readiness".
    pub(crate) readiness: DocumentReadyState,
    pub(crate) phase: Phase,
    /// True while a [`Task::ParserResume`] waits in the queue.
    pub(crate) resume_queued: bool,
    /// True once the parser waited for a script or a style sheet.
    pub(crate) parser_waited: bool,
    /// True after the input ended and "the end" was put off to the next
    /// parser task (see `defer_parse_end`).
    pub(crate) parse_end_deferred: bool,
    /// HTML "page showing".
    pub(crate) page_showing: bool,
}

impl Default for Scripts {
    fn default() -> Scripts {
        Scripts {
            elements: HashMap::new(),
            pending_parsing_blocking: None,
            deferred: VecDeque::new(),
            in_order: VecDeque::new(),
            asap: HashSet::new(),
            held_bytes: 0,
            current_script: None,
            // HTML: a document's readiness is "complete" until a parser
            // sets it to "loading".
            readiness: DocumentReadyState::Complete,
            phase: Phase::NotStarted,
            resume_queued: false,
            parser_waited: false,
            parse_end_deferred: false,
            page_showing: false,
        }
    }
}

impl Scripts {
    /// True if the element has a result (not "uninitialized").
    pub(crate) fn has_result(&self, node: NodeId) -> bool {
        self.elements
            .get(&node)
            .is_some_and(ScriptElement::has_result)
    }

    /// Drops the text of the scripts that wait (the load was aborted).
    pub(crate) fn release_all(&mut self) {
        for el in self.elements.values_mut() {
            if matches!(el.result, ScriptResult::Classic(_)) {
                el.result = ScriptResult::Done;
            }
        }
        self.held_bytes = 0;
    }
}

/// The script state of the runtime.
pub(crate) fn scripts(rt: &Runtime) -> VmResult<&Scripts> {
    Ok(&state(rt)?.scripts)
}

/// The script state of the runtime, for changes.
pub(crate) fn scripts_mut(rt: &mut Runtime) -> VmResult<&mut Scripts> {
    Ok(&mut state_mut(rt)?.scripts)
}

/// What the caller of [`prepare`] does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prepared {
    /// Nothing (no script, a fetch started, a queued error event).
    Nothing,
    /// Execute the element now ("immediately execute the script element").
    ExecuteNow,
    /// The element is the pending parsing-blocking script: the parser
    /// waits.
    Blocking,
}

/// The kind of a script element by its `type` and `language` (steps 7 to
/// 11 of "prepare the script element").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Classic,
    Module,
    /// An import map, speculation rules or a type that is not a script.
    Other,
}

fn kind_of(element: &swb_dom::ElementData) -> Kind {
    let trimmed = |s: &str| s.trim_matches(|c: char| c.is_ascii_whitespace()).to_owned();
    let type_string = match (element.attr("type"), element.attr("language")) {
        (Some(""), _) | (None, Some("") | None) => "text/javascript".to_owned(),
        (Some(kind), _) => kind.to_owned(),
        (None, Some(language)) => format!("text/{language}"),
    };
    if is_javascript_mime_type(&type_string)
        || element
            .attr("type")
            .is_some_and(|t| is_javascript_mime_type(&trimmed(t)))
    {
        Kind::Classic
    } else if type_string.eq_ignore_ascii_case("module") {
        Kind::Module
    } else {
        Kind::Other
    }
}

/// The child text content of a node.
fn child_text(dom: &Document, node: NodeId) -> String {
    dom.children(node)
        .filter_map(|c| dom.node(c).as_text())
        .collect()
}

/// True if the `event` and `for` attributes, when both are present, leave
/// the script to run (step 18: only `for=window event=onload`).
fn event_for_allows(element: &swb_dom::ElementData) -> bool {
    let (Some(event), Some(target)) = (element.attr("event"), element.attr("for")) else {
        return true;
    };
    let strip = |s: &str| s.trim_matches(|c: char| c.is_ascii_whitespace()).to_owned();
    let event = strip(event);
    strip(target).eq_ignore_ascii_case("window")
        && (event.eq_ignore_ascii_case("onload") || event.eq_ignore_ascii_case("onload()"))
}

/// What step 1 to 13 of "prepare" found in the element.
struct Found {
    source: String,
    src: Option<String>,
    has_async: bool,
    has_defer: bool,
    in_document: bool,
}

/// HTML "prepare the script element"
/// (<https://html.spec.whatwg.org/multipage/scripting.html#prepare-the-script-element>).
/// `parser` is the position of the script's text when the parser calls it
/// at the end tag (the element is parser-inserted), `None` for the steps
/// after an insertion. The fetch of an external script starts here; the
/// caller executes the script on [`Prepared::ExecuteNow`].
pub(crate) fn prepare(
    rt: &mut Runtime,
    node: NodeId,
    parser: Option<TextPosition>,
) -> VmResult<Prepared> {
    let dom = dom(rt)?;
    let Some(element) = dom.element(node) else {
        return Ok(Prepared::Nothing);
    };
    let parser_flag_started = element.script_already_started;
    let kind = kind_of(element);
    let found = Found {
        source: child_text(dom, node),
        src: element.attr("src").map(str::to_owned),
        has_async: element.has_attr("async"),
        has_defer: element.has_attr("defer"),
        in_document: dom.node_document(node) == NodeId::DOCUMENT,
    };
    let connected = dom.is_connected(node);
    let nomodule = element.has_attr("nomodule");
    let event_for = event_for_allows(element);
    let scripts = scripts_mut(rt)?;
    let el = scripts.elements.entry(node).or_insert_with(|| {
        let mut el = ScriptElement::new();
        el.already_started = parser_flag_started;
        if parser.is_some() {
            // Set when the parser created the element.
            el.parser_inserted = true;
            el.force_async = false;
        }
        el
    });
    // Steps 1 to 5.
    if el.already_started {
        return Ok(Prepared::Nothing);
    }
    let parser_document = el.parser_inserted;
    el.parser_inserted = false;
    if parser_document && !found.has_async {
        el.force_async = true;
    }
    if found.src.is_none() && found.source.is_empty() {
        return Ok(Prepared::Nothing);
    }
    if !connected {
        return Ok(Prepared::Nothing);
    }
    // Steps 6 to 11.
    match kind {
        Kind::Classic => {}
        Kind::Module => {
            // Departure (ADR 0028 section 6, item 7): no module scripts.
            el.already_started = true;
            log::warn!("a module script is not run (swb runs classic scripts only)");
            return Ok(Prepared::Nothing);
        }
        Kind::Other => return Ok(Prepared::Nothing),
    }
    // Steps 12 to 16.
    if parser_document {
        el.parser_inserted = true;
        el.force_async = false;
    }
    el.already_started = true;
    el.prepared_in_document = found.in_document;
    // Scripting is disabled for an element of another document (the inert
    // documents of template contents).
    if !found.in_document || nomodule || !event_for {
        return Ok(Prepared::Nothing);
    }
    match found.src.clone() {
        Some(src) => prepare_external(rt, node, &src, &found),
        None => prepare_inline(rt, node, found.source, parser),
    }
}

/// Steps 31 and 32 of "prepare" for an element with `src`.
fn prepare_external(
    rt: &mut Runtime,
    node: NodeId,
    src: &str,
    found: &Found,
) -> VmResult<Prepared> {
    // Departure from HTML, as Chromium (measured, case
    // `ignored-attributes-and-errors`): a `src` of only whitespace is an
    // error like an empty one (HTML would fetch the document's URL).
    if src
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .is_empty()
    {
        queue_task(rt, Task::ScriptErrorEvent(node))?;
        return Ok(Prepared::Nothing);
    }
    let Some(url) = engine_mut(rt)?.parse_url(src) else {
        queue_task(rt, Task::ScriptErrorEvent(node))?;
        return Ok(Prepared::Nothing);
    };
    if !engine_mut(rt)?.fetch_script(node, &url) {
        // The fetch fails at once (a URL that the document may not load);
        // its result comes as a task, as a fetch's would.
        queue_task(rt, Task::ScriptFetchFailed(node))?;
    }
    let scripts = scripts_mut(rt)?;
    let Some(el) = scripts.elements.get_mut(&node) else {
        return Ok(Prepared::Nothing);
    };
    el.from_external_file = true;
    let (when_ready, prepared) = if found.has_async || el.force_async {
        scripts.asap.insert(node);
        (WhenReady::Asap, Prepared::Nothing)
    } else if !el.parser_inserted {
        scripts.in_order.push_back(node);
        (WhenReady::InOrder, Prepared::Nothing)
    } else if found.has_defer {
        scripts.deferred.push_back(node);
        (WhenReady::ParserExecuted, Prepared::Nothing)
    } else {
        scripts.pending_parsing_blocking = Some(node);
        (WhenReady::ParserExecuted, Prepared::Blocking)
    };
    if let Some(el) = scripts.elements.get_mut(&node) {
        el.when_ready = Some(when_ready);
    }
    Ok(prepared)
}

/// Steps 33 and 34 of "prepare" for an inline classic script: the result
/// is ready at once; a parser-inserted one waits for the style sheets that
/// block scripts.
fn prepare_inline(
    rt: &mut Runtime,
    node: NodeId,
    source: String,
    parser: Option<TextPosition>,
) -> VmResult<Prepared> {
    let url = engine(rt)?.url();
    let start = parser.map_or((0, 0), |p| (p.line, p.column));
    let parser_inserted = scripts(rt)?
        .elements
        .get(&node)
        .is_some_and(|el| el.parser_inserted);
    let blocked = parser_inserted && engine_mut(rt)?.style_sheets_block_scripts();
    let scripts = scripts_mut(rt)?;
    let Some(el) = scripts.elements.get_mut(&node) else {
        return Ok(Prepared::Nothing);
    };
    el.result = ScriptResult::Classic(Box::new(ClassicScript {
        source,
        url,
        start,
        held: 0,
    }));
    if blocked {
        el.ready_to_be_parser_executed = true;
        scripts.pending_parsing_blocking = Some(node);
        return Ok(Prepared::Blocking);
    }
    Ok(Prepared::ExecuteNow)
}

/// HTML "mark as ready" with the fetched script (`None`: the fetch
/// failed). Returns the steps to run when the result is ready, which the
/// caller runs (they execute scripts).
pub(crate) fn mark_as_ready(
    rt: &mut Runtime,
    node: NodeId,
    mut fetched: Option<FetchedScript>,
) -> VmResult<Option<WhenReady>> {
    let scripts = scripts_mut(rt)?;
    if scripts
        .elements
        .get(&node)
        .is_none_or(ScriptElement::has_result)
    {
        return Ok(None);
    }
    let held = fetched.as_ref().map_or(0, |script| script.source.len());
    if scripts.held_bytes.saturating_add(held) > MAX_HELD_SCRIPT_BYTES {
        log::warn!(
            "a script of {held} bytes fails: the document holds {} bytes of script text \
             that has not run (the limit is {MAX_HELD_SCRIPT_BYTES})",
            scripts.held_bytes
        );
        fetched = None;
    }
    let held = fetched.as_ref().map_or(0, |script| script.source.len());
    scripts.held_bytes += held;
    let Some(el) = scripts.elements.get_mut(&node) else {
        return Ok(None);
    };
    el.result = match fetched {
        Some(script) => ScriptResult::Classic(Box::new(ClassicScript {
            source: script.source,
            url: script.url,
            start: (0, 0),
            held,
        })),
        None => ScriptResult::Failed,
    };
    Ok(el.when_ready.take())
}

/// HTML "execute the script element"
/// (<https://html.spec.whatwg.org/multipage/scripting.html#execute-the-script-element>):
/// fires `error` for a failed fetch; runs the classic script with
/// `document.currentScript` set, with a microtask checkpoint after it when
/// no other script runs (HTML "clean up after running script"; Chromium's
/// microtasks see the `currentScript` of the script, measured); then fires
/// `load` for an external script. The exception of the script is
/// reported; a termination goes up.
pub(crate) fn execute(rt: &mut Runtime, node: NodeId) -> Result<(), ScriptError> {
    let in_document = dom(rt)
        .map(|d| d.node_document(node) == NodeId::DOCUMENT)
        .map_err(crate::runtime::task_error)?;
    let scripts = scripts_mut(rt).map_err(crate::runtime::task_error)?;
    let Some(el) = scripts.elements.get_mut(&node) else {
        return Ok(());
    };
    if !el.prepared_in_document || !in_document {
        // The script never runs: its text is dropped.
        if let ScriptResult::Classic(script) = std::mem::replace(&mut el.result, ScriptResult::Done)
        {
            scripts.held_bytes = scripts.held_bytes.saturating_sub(script.held);
        }
        return Ok(());
    }
    let external = el.from_external_file;
    let result = std::mem::replace(&mut el.result, ScriptResult::Done);
    if let ScriptResult::Classic(script) = &result {
        scripts.held_bytes = scripts.held_bytes.saturating_sub(script.held);
    }
    let script = match result {
        ScriptResult::Classic(script) => script,
        ScriptResult::Failed => {
            return fire_at_element(rt, node, "error").map_err(crate::runtime::task_error);
        }
        ScriptResult::Uninitialized | ScriptResult::Done => return Ok(()),
    };
    let old = scripts.current_script.replace(node);
    let run = rt
        .eval_named_at(&script.source, &script.url, script.start)
        .map(drop);
    let mut outcome = match &run {
        Err(error @ (ScriptError::Uncaught { .. } | ScriptError::Compile { .. })) => {
            let site = crate::errors::Site::script_at(&script.url, &script.source, script.start);
            crate::errors::report(rt, &site, error).map_err(crate::runtime::task_error)
        }
        Err(error) => Err(error.clone()),
        Ok(()) => Ok(()),
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

/// Fires a simple event (it neither bubbles nor can be canceled) at a
/// script element: `load` or `error`. When no script runs, a microtask
/// checkpoint follows each listener.
pub(crate) fn fire_at_element(rt: &mut Runtime, node: NodeId, kind: &str) -> VmResult<()> {
    fire(rt, DispatchTarget::Node(node), &TrustedEvent::new(kind)).map(drop)
}

/// Fires a trusted event in a handle scope; with listener checkpoints
/// when no script runs. Returns false if the event was canceled.
pub(crate) fn fire(
    rt: &mut Runtime,
    target: DispatchTarget,
    event: &TrustedEvent,
) -> VmResult<bool> {
    let outermost = rt.stack_depths().0 == 0;
    let event_loop = &mut state_mut(rt)?.event_loop;
    let checkpoints = std::mem::replace(&mut event_loop.listener_checkpoints, outermost);
    let scope = rt.heap_mut().open_scope();
    let result = crate::events::dispatch_trusted(rt, target, event);
    if let Err(error) = rt.heap_mut().close_scope(scope) {
        log::warn!("the handle scope of an event was not the last one: {error:?}");
    }
    state_mut(rt)?.event_loop.listener_checkpoints = checkpoints;
    result
}

/// The HTML script elements that an insertion of `nodes` connected, in
/// tree order (their inclusive descendants).
fn inserted_scripts(dom: &Document, nodes: &[NodeId]) -> Vec<NodeId> {
    let mut found = Vec::new();
    for &node in nodes {
        if !dom.is_connected(node) {
            continue;
        }
        let mut subtree = std::iter::once(node).chain(dom.descendants(node));
        found.extend(subtree.by_ref().filter(|&n| is_script(dom, n)));
    }
    found
}

/// True for an HTML `script` element.
fn is_script(dom: &Document, node: NodeId) -> bool {
    dom.is_html_element(node, &local_name!("script"))
}

/// The steps after `nodes` were inserted into `parent` by a script (DOM
/// "insert", the children changed steps of a `script` parent, then the
/// post-connection steps of the inserted script elements, HTML §4.12.1.1).
/// An inline script runs now, inside the call that inserted it. Feature 6a
/// calls this from every insertion.
pub(crate) fn after_insertion(rt: &mut Runtime, parent: NodeId, nodes: &[NodeId]) -> VmResult<()> {
    if is_script(dom(rt)?, parent) {
        children_changed(rt, parent)?;
    }
    let found = inserted_scripts(dom(rt)?, nodes);
    for script in found {
        // An earlier script may have removed a later one.
        if !dom(rt)?.is_connected(script) {
            continue;
        }
        post_connection(rt, script)?;
    }
    Ok(())
}

/// The children changed steps of a script element: the post-connection
/// steps if it is connected.
pub(crate) fn children_changed(rt: &mut Runtime, script: NodeId) -> VmResult<()> {
    if dom(rt)?.is_connected(script) {
        post_connection(rt, script)?;
    }
    Ok(())
}

/// The post-connection steps of a script element: prepare it unless it is
/// parser-inserted, and execute an inline one now (nested in the running
/// script; its exceptions are reported there).
fn post_connection(rt: &mut Runtime, script: NodeId) -> VmResult<()> {
    let parser_inserted = scripts(rt)?
        .elements
        .get(&script)
        .is_some_and(|el| el.parser_inserted);
    if parser_inserted || scripts(rt)?.phase == Phase::Unloaded {
        return Ok(());
    }
    if prepare(rt, script, None)? == Prepared::ExecuteNow {
        match execute(rt, script) {
            Err(ScriptError::Terminated(termination)) => {
                return Err(VmError::Terminated(termination));
            }
            Err(ScriptError::Internal(error)) => return Err(VmError::Internal(error)),
            _ => {}
        }
    }
    Ok(())
}

/// The URL of the lent document (the name of the scripts of string
/// timer handlers), or empty without one.
pub(crate) fn document_url(rt: &Runtime) -> String {
    engine(rt).map_or_else(|_| String::new(), crate::host::EngineHost::url)
}

/// `document.readyState`: the readiness of the window's document; other
/// documents (template contents) are "complete".
pub(crate) fn ready_state(rt: &Runtime, document: NodeId) -> VmResult<DocumentReadyState> {
    if document != NodeId::DOCUMENT {
        return Ok(DocumentReadyState::Complete);
    }
    Ok(scripts(rt)?.readiness)
}

impl g::html_script_element::HTMLScriptElementImpl for WebState {
    /// <https://html.spec.whatwg.org/multipage/scripting.html#dom-script-async>
    fn async_(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        let has_attribute = dom(cx.rt)?
            .element(this)
            .is_some_and(|e| e.has_attr("async"));
        let force_async = scripts(cx.rt)?
            .elements
            .get(&this)
            .is_none_or(|el| el.force_async);
        Ok(force_async || has_attribute)
    }

    fn set_async(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        scripts_mut(cx.rt)?
            .elements
            .entry(this)
            .or_insert_with(ScriptElement::new)
            .force_async = false;
        if value {
            let result = dom_mut(cx.rt)?.set_attr(this, "async", "");
            crate::dom::dom_result(cx.rt, result)
        } else {
            dom_mut(cx.rt)?.remove_attr(this, "async");
            Ok(())
        }
    }

    /// <https://html.spec.whatwg.org/multipage/scripting.html#dom-script-text>:
    /// the child text content.
    fn text(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(child_text(dom(cx.rt)?, this))
    }

    /// "String replace all" with the value, then the children changed
    /// steps (a connected script that has not started runs now).
    fn set_text(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        let node = if value.is_empty() {
            None
        } else {
            let doc = dom(cx.rt)?.node_document(this);
            let result = dom_mut(cx.rt)?.create_node_in(doc, NodeData::Text(value));
            Some(crate::dom::dom_result(cx.rt, result)?)
        };
        let result = dom_mut(cx.rt)?.replace_all(this, node);
        crate::dom::dom_result(cx.rt, result)?;
        Ok(children_changed(cx.rt, this)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(html: &str) -> Vec<Kind> {
        let dom = swb_dom::parse_html_with(html, true);
        dom.descendants(NodeId::DOCUMENT)
            .filter(|&n| is_script(&dom, n))
            .filter_map(|n| dom.element(n).map(kind_of))
            .collect()
    }

    #[test]
    fn script_types() {
        use Kind::{Classic as C, Module as M, Other as O};
        // As Chromium (tools/probes/web-scripts.json, case `types`): a type
        // with parameters is not JavaScript; `type` wins over `language`.
        let html = "<script>1</script><script type=''>2</script>\
                    <script language=''>3</script>\
                    <script type=' TEXT/JavaScript '>4</script>\
                    <script language=javascript>5</script>\
                    <script type=module>no</script><script type=text/plain>no</script>\
                    <script language=vbscript>no</script>\
                    <script type='text/javascript; charset=utf-8'>no</script>\
                    <script type=MODULE language=javascript>m</script>\
                    <script type=importmap>{}</script>";
        assert_eq!(kinds(html), [C, C, C, C, C, M, O, O, O, M, O]);
    }

    #[test]
    fn event_and_for_attributes() {
        let dom = swb_dom::parse_html_with(
            "<script for=window event=onload></script>\
             <script for=' Window ' event='onload()'></script>\
             <script for=x event=onload></script><script event=onclick></script>\
             <script for=window event=onclick></script>",
            true,
        );
        let allowed: Vec<bool> = dom
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| is_script(&dom, n))
            .filter_map(|n| dom.element(n).map(event_for_allows))
            .collect();
        assert_eq!(allowed, [true, true, false, true, false]);
    }
}
