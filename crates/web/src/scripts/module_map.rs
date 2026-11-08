//! The module map of a document (HTML "module map",
//! <https://html.spec.whatwg.org/multipage/webappapis.html#module-map>; ADR
//! 0034 sections 8 and 11): one entry per URL with its fragment, for the
//! life of the document. An entry fetches, holds fetched text that no graph
//! needed yet, a module record, a parse error, or a failure (null in HTML).
//!
//! - **Fetch sharing.** URLs that differ only in the fragment are two
//!   entries (two modules, as in Chromium) but one request while it is in
//!   flight (Chromium's memory cache answers the second; measured, case
//!   `graph-shared`).
//! - **Parse on first use.** Fetched text is parsed when a graph needs it:
//!   at once when a load waits for the entry, else at the first request
//!   (deliberate departure from HTML, which parses at the fetch).
//! - **Waiters.** A request of a graph load (a token of the engine), a
//!   script element (the root of its graph) or an `import()` (its token)
//!   waits for an entry that fetches; the arrival answers them in order.
//! - **Records.** Each module has a number (`[[HostDefined]]`): the index of
//!   its [`ModuleInfo`], with its URL (the base URL of its requests and
//!   `import.meta.url`) and the fetch options that its descendants inherit.
//! - **Budgets.** At most [`MAX_MODULE_ENTRIES`] entries (a request for a
//!   new URL past the limit fails as a fetch error, with one warning); text
//!   that waits for its parse counts against the held script text
//!   ([`super::MAX_HELD_SCRIPT_BYTES`]).
//!
//! The collector traces the records and the error values of the entries
//! through the document's state ([`ModuleMap::trace`]); they die with the
//! document's state.

use std::collections::{HashMap, HashSet};

use swb_dom::NodeId;
use swb_js::{Gc, Module, Object, Runtime, ScriptError, SourceLocation, ThrowKind, Tracer, Value};
use swb_js::{VmError, VmResult};

use super::import_map::ImportMap;
use super::{FetchedScript, MAX_HELD_SCRIPT_BYTES, WhenReady, scripts, scripts_mut};
use crate::event_loop::{Task, queue_task};
use crate::state::engine_mut;

/// The most entries of a document's module map (ADR 0034 section 11;
/// Chromium has no limit).
pub(crate) const MAX_MODULE_ENTRIES: usize = 65_536;

/// The prefix of the message of a failed fetch of `import()` (Chromium
/// 148, measured: `import-call-errors`, `msg-import-call-descendant`).
pub(crate) const FETCH_FAILED: &str = "Failed to fetch dynamically imported module: ";

/// What the page knows of a module record, by its number.
#[derive(Debug)]
pub(crate) struct ModuleInfo {
    /// The module's URL (with the fragment of its entry): the base URL of
    /// its requests and `import.meta.url`.
    pub(crate) url: String,
    /// The fetch options that its descendants inherit (HTML "descendant
    /// script fetch options"): the credentials mode `include`.
    pub(crate) credentials: bool,
    /// True if the module was fetched for `import()`: the fetches of its
    /// descendants do not delay the `load` event either.
    pub(crate) quiet: bool,
}

/// The fetch options of an entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    /// The credentials mode `include` (`crossorigin=use-credentials`).
    pub(crate) credentials: bool,
    /// The fetch does not delay the `load` event (`import()`).
    pub(crate) quiet: bool,
}

/// The state of an entry.
#[derive(Debug)]
pub(crate) enum EntryState {
    /// The fetch with this id is in flight.
    Fetching(u64),
    /// The text arrived; no graph has needed it yet.
    Fetched(Box<FetchedText>),
    /// The module record.
    Module(Module),
    /// The text did not parse: the `SyntaxError` (HTML "parse error").
    ParseError(Value),
    /// The fetch failed (null in HTML): a `TypeError` that only `import()`
    /// shows (as the error of its root, [`FETCH_FAILED`]).
    Failed(Value),
}

/// Fetched text that waits for its parse.
#[derive(Debug)]
pub(crate) struct FetchedText {
    source: String,
    /// The URL of the response.
    url: String,
    /// The bytes that count against the held script text.
    held: usize,
}

/// What waits for an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Waiter {
    /// A request of a graph load: the engine's token.
    Load(u32),
    /// A script element whose root module the entry is.
    Element(NodeId),
    /// An `import()` whose root the entry is: the engine's token.
    Dynamic(u32),
    /// A `link rel=modulepreload` element: it needs the fetch, not the parse
    /// ([`preload`]).
    Preload(NodeId),
}

/// An entry of the map.
#[derive(Debug)]
pub(crate) struct Entry {
    pub(crate) state: EntryState,
    waiters: Vec<Waiter>,
    options: Options,
}

/// Who waits for a graph load that the page started.
#[derive(Debug)]
pub(crate) enum GraphOwner {
    /// A script element.
    Element(NodeId),
    /// An `import()`: the token of its answer and the URL of its root.
    Dynamic { token: u32, url: String },
}

/// A graph load of the page: its owner and its root.
#[derive(Debug)]
pub(crate) struct Graph {
    pub(crate) owner: GraphOwner,
    pub(crate) root: Module,
}

/// What the map knows of an error value that it made.
#[derive(Debug)]
enum ErrorKind {
    /// The parse error of a module, at this place.
    Parse(SourceLocation),
    /// A failed fetch.
    Fetch,
}

/// The answer of the map to a request.
#[derive(Debug)]
pub(crate) enum Lookup {
    /// The module, or the error (a parse error or a failed fetch).
    Now(Result<Module, Value>),
    /// The entry fetches; the waiter gets the answer.
    Later,
}

/// The module map of a document.
#[derive(Debug, Default)]
pub(crate) struct ModuleMap {
    entries: HashMap<String, Entry>,
    /// The modules by number.
    infos: Vec<ModuleInfo>,
    /// The URL (without the fragment) and the entries of each fetch in
    /// flight, by fetch id.
    fetches: HashMap<u64, (String, Vec<String>)>,
    /// The fetch in flight of each URL without its fragment.
    in_flight: HashMap<String, u64>,
    /// The fetches in flight that do not delay the `load` event (`import()`
    /// started them, and no script element has joined them).
    quiet_fetches: HashSet<u64>,
    next_fetch: u64,
    /// The graph loads whose promise has a reaction, by id.
    graphs: HashMap<u32, Graph>,
    next_graph: u32,
    /// The errors that the map made: parse errors and failed fetches. Not
    /// traced: an error that the page can still see is reachable from an
    /// entry, an element or the script (handles carry a generation, so a
    /// dead key never names a later object); the keys of dead errors stay.
    errors: HashMap<Gc<Object>, ErrorKind>,
    /// The error of the requests of new URLs past [`MAX_MODULE_ENTRIES`]
    /// (one for all; a failed fetch).
    limit_error: Option<Value>,
    /// The `import()` calls of modules that the map has, which get their
    /// answer in a task ([`crate::event_loop::Task::ModuleImport`]): the
    /// URL and the options, by the engine's token.
    pub(crate) waiting_imports: HashMap<u32, (String, Options)>,
    /// The `import.meta.resolve` functions, with the number of their module.
    pub(crate) resolvers: HashMap<Gc<Object>, u32>,
    /// True after the warning about the entry limit.
    limit_warned: bool,
    /// The document's import map and its resolved module set.
    pub(crate) import_map: ImportMap,
}

/// What `import()` in a classic script resolves against.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScriptBase {
    /// The script's base URL: its URL (external) or the document's base URL
    /// when it was prepared (inline).
    pub(crate) url: String,
    /// True for a cross-origin script fetched without CORS: its `import()`
    /// is rejected (HTML "muted errors"; Chromium, measured).
    pub(crate) muted: bool,
    /// The realm of the document that prepared the script: an `import()` in a
    /// function of the script resolves against this base and loads into the
    /// module map of this realm's document, wherever the function is called
    /// (HTML: the script's base URL and its realm's module map; Chromium,
    /// measured: `import-call-base-frame-function`).
    pub(crate) realm: u32,
}

/// The classic script bases of a runtime by number (the number minus 1),
/// shared by its documents: a function of a script of one document can be
/// called from another ([`script_number`]).
#[derive(Debug, Default)]
pub(crate) struct ScriptBases {
    bases: Vec<ScriptBase>,
    numbers: HashMap<ScriptBase, u32>,
    limit_warned: bool,
}

impl ScriptBases {
    /// The base of the classic script with the number `number`.
    pub(crate) fn get(&self, number: u32) -> Option<&ScriptBase> {
        let at = number.checked_sub(1)?;
        self.bases.get(at as usize)
    }
}

/// The number of the referrer "the document's URL": code without a script,
/// and event handlers (Chromium, measured: `import-call-document-base`).
pub(crate) const DOCUMENT_REFERRER: u32 = 0;

/// The most distinct script bases a runtime numbers (ADR 0034 section 11
/// spirit: the table is bounded); later scripts count as the document.
const MAX_SCRIPT_BASES: usize = 65_536;

impl ModuleMap {
    /// Reports the records, the error values of the entries and the functions
    /// of the map to the collector.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for entry in self.entries.values() {
            match &entry.state {
                EntryState::Module(module) => module.trace(tracer),
                EntryState::ParseError(value) | EntryState::Failed(value) => tracer.value(*value),
                EntryState::Fetching(_) | EntryState::Fetched(_) => {}
            }
        }
        for graph in self.graphs.values() {
            graph.root.trace(tracer);
        }
        for function in self.resolvers.keys() {
            tracer.object(*function);
        }
        if let Some(error) = self.limit_error {
            tracer.value(error);
        }
    }

    /// The module with the number `number`.
    pub(crate) fn info(&self, number: u32) -> Option<&ModuleInfo> {
        self.infos.get(number as usize)
    }

    /// A new graph load of `owner` with the root `root`; its id.
    pub(crate) fn add_graph(&mut self, owner: GraphOwner, root: Module) -> u32 {
        let id = self.next_graph;
        self.next_graph = self.next_graph.wrapping_add(1);
        self.graphs.insert(id, Graph { owner, root });
        id
    }

    /// Takes the graph load `id` (its promise settled).
    pub(crate) fn take_graph(&mut self, id: u32) -> Option<Graph> {
        self.graphs.remove(&id)
    }

    /// True if `value` is the error of a failed fetch of this map.
    pub(crate) fn is_fetch_failure(&self, value: Value) -> bool {
        matches!(value, Value::Object(object)
            if matches!(self.errors.get(&object), Some(ErrorKind::Fetch)))
    }

    /// Where the parse error `value` of this map is.
    pub(crate) fn parse_error_location(&self, value: Value) -> Option<SourceLocation> {
        let Value::Object(object) = value else {
            return None;
        };
        match self.errors.get(&object) {
            Some(ErrorKind::Parse(location)) => Some(location.clone()),
            _ => None,
        }
    }

    /// The document left its frame (HTML: the module map goes with the
    /// document): the records, the errors and the loads in flight go; their
    /// records die with the last script object that reaches them. The
    /// numbers stay, for the `import.meta` of records that live on.
    pub(crate) fn retire(&mut self) {
        let infos = std::mem::take(&mut self.infos);
        let resolvers = std::mem::take(&mut self.resolvers);
        *self = ModuleMap {
            infos,
            resolvers,
            ..ModuleMap::default()
        };
    }

    /// Drops the entries whose fetched text waits for a parse (the load was
    /// aborted): a later request fetches the URL again.
    pub(crate) fn release_texts(&mut self) {
        self.entries
            .retain(|_, entry| !matches!(entry.state, EntryState::Fetched(_)));
    }
}

/// The module map of the current document.
fn map(rt: &Runtime) -> VmResult<&ModuleMap> {
    Ok(&scripts(rt)?.modules)
}

/// The module map of the current document, for changes.
pub(crate) fn map_mut(rt: &mut Runtime) -> VmResult<&mut ModuleMap> {
    Ok(&mut scripts_mut(rt)?.modules)
}

/// The number that stands for the classic script base `url` (`muted`: see
/// [`ScriptBase`]) of the current document in `eval_named_at`: the same base
/// in the same realm gets the same number. A runtime with more than
/// [`MAX_SCRIPT_BASES`] distinct bases gets [`DOCUMENT_REFERRER`] for the
/// later ones, with one warning.
pub(crate) fn script_number(rt: &mut Runtime, url: &str, muted: bool) -> VmResult<u32> {
    let realm = crate::state::doc_realm(rt)?;
    let table = &mut crate::state::state_mut(rt)?.script_bases;
    let base = ScriptBase {
        url: url.to_owned(),
        muted,
        realm,
    };
    if let Some(&number) = table.numbers.get(&base) {
        return Ok(number);
    }
    if table.bases.len() >= MAX_SCRIPT_BASES {
        if !std::mem::replace(&mut table.limit_warned, true) {
            log::warn!(
                "{MAX_SCRIPT_BASES} script base URLs are in use, the limit: later scripts \
                 resolve import() against the document's URL"
            );
        }
        return Ok(DOCUMENT_REFERRER);
    }
    table.bases.push(base.clone());
    let number = table.bases.len() as u32;
    table.numbers.insert(base, number);
    Ok(number)
}

/// The URL without its fragment.
fn without_fragment(url: &str) -> &str {
    url.split_once('#').map_or(url, |(base, _)| base)
}

/// Requests the module at `url` (absolute, with its fragment) for
/// `waiter`: the answer of the map now, or `Later` (the waiter waits for
/// the entry's fetch). A new URL starts a fetch with `options`; text that
/// waits is parsed now.
pub(crate) fn request(
    rt: &mut Runtime,
    url: &str,
    waiter: Waiter,
    options: Options,
) -> VmResult<Lookup> {
    /// What the entry of the URL has.
    enum Found {
        Missing,
        Fetching,
        Text,
        Result(Result<Module, Value>),
    }
    let found = match map(rt)?.entries.get(url).map(|entry| &entry.state) {
        None => Found::Missing,
        Some(EntryState::Fetching(_)) => Found::Fetching,
        Some(EntryState::Fetched(_)) => Found::Text,
        Some(EntryState::Module(module)) => Found::Result(Ok(*module)),
        Some(EntryState::ParseError(error) | EntryState::Failed(error)) => {
            Found::Result(Err(*error))
        }
    };
    match found {
        Found::Missing => start_entry(rt, url, waiter, options),
        Found::Fetching => {
            if let Some(entry) = map_mut(rt)?.entries.get_mut(url) {
                entry.waiters.push(waiter);
            }
            delay_load_if_loud(rt, url, options)?;
            Ok(Lookup::Later)
        }
        Found::Text => {
            delay_load_if_loud(rt, url, options)?;
            Ok(Lookup::Now(parse_waiting(rt, url)?))
        }
        Found::Result(result) => Ok(Lookup::Now(result)),
    }
}

/// HTML "fetch a modulepreload module script graph" for the element `node`
/// (without the descendants) and the URL `url`: the fetch of the single module
/// into the map, with quiet options (it does not delay `load`), and the
/// element's `load` or `error` event in a task after it. Text that arrives
/// stays in the entry unparsed until a graph needs it. Measured in
/// `modulepreload*`: a URL that the map has gives the event with no request.
pub(crate) fn preload(
    rt: &mut Runtime,
    url: &str,
    node: NodeId,
    credentials: bool,
) -> VmResult<()> {
    let options = Options {
        credentials,
        quiet: true,
    };
    let state = match map(rt)?.entries.get(url).map(|entry| &entry.state) {
        None => None,
        Some(EntryState::Fetching(_)) => {
            if let Some(entry) = map_mut(rt)?.entries.get_mut(url) {
                entry.waiters.push(Waiter::Preload(node));
            }
            return Ok(());
        }
        Some(state) => Some(matches!(state, EntryState::Failed(_))),
    };
    let failed = match state {
        Some(failed) => failed,
        None => match start_entry(rt, url, Waiter::Preload(node), options)? {
            Lookup::Later => return Ok(()),
            Lookup::Now(result) => result.is_err(),
        },
    };
    queue_task(
        rt,
        Task::LinkEvent {
            node,
            error: failed,
        },
    )
}

/// A requester that delays the `load` event (a script element's graph)
/// joined the entry `url`, which an `import()` started: the entry takes the
/// requester's options (its descendants are fetched as the element's), and a
/// fetch in flight now delays `load` (measured, case
/// `import-call-timing-quiet-then-async`).
fn delay_load_if_loud(rt: &mut Runtime, url: &str, options: Options) -> VmResult<()> {
    if options.quiet {
        return Ok(());
    }
    let map = map_mut(rt)?;
    let Some(entry) = map.entries.get_mut(url) else {
        return Ok(());
    };
    entry.options.quiet = false;
    if let EntryState::Fetching(id) = entry.state {
        delay_load_of_fetch(rt, id)?;
    }
    Ok(())
}

/// The fetch `id` of the module map delays the `load` event from now on.
fn delay_load_of_fetch(rt: &mut Runtime, id: u64) -> VmResult<()> {
    if map_mut(rt)?.quiet_fetches.remove(&id) {
        engine_mut(rt)?.delay_load_for_module(id);
    }
    Ok(())
}

/// Parses the fetched text of the entry `url` (its first use). A termination
/// of the parse puts the text back: a later request parses it again.
fn parse_waiting(rt: &mut Runtime, url: &str) -> VmResult<Result<Module, Value>> {
    let Some(entry) = map_mut(rt)?.entries.get_mut(url) else {
        return Err(VmError::invariant("a module entry that went away"));
    };
    let options = entry.options;
    let EntryState::Fetched(text) = std::mem::replace(&mut entry.state, EntryState::Fetching(0))
    else {
        return Err(VmError::invariant("a module entry without its text"));
    };
    let scripts = scripts_mut(rt)?;
    scripts.held_bytes = scripts.held_bytes.saturating_sub(text.held);
    let state = match parse(rt, url, (&text.source, &text.url), options) {
        Ok(state) => state,
        Err(error) => {
            // A termination: the entry has its text again.
            if let Ok(scripts) = scripts_mut(rt) {
                scripts.held_bytes = scripts.held_bytes.saturating_add(text.held);
            }
            if let Ok(map) = map_mut(rt)
                && let Some(entry) = map.entries.get_mut(url)
            {
                entry.state = EntryState::Fetched(text);
            }
            return Err(error);
        }
    };
    let result = result_of(&state);
    if let Some(entry) = map_mut(rt)?.entries.get_mut(url) {
        entry.state = state;
    }
    Ok(result)
}

/// The answer that a parsed or failed entry gives.
fn result_of(state: &EntryState) -> Result<Module, Value> {
    match state {
        EntryState::Module(module) => Ok(*module),
        EntryState::ParseError(error) | EntryState::Failed(error) => Err(*error),
        // Not reached: the callers have a parsed or failed entry.
        EntryState::Fetching(_) | EntryState::Fetched(_) => Err(Value::Undefined),
    }
}

/// A new entry for `url` and its fetch (shared with a fetch in flight of
/// the same URL without the fragment).
fn start_entry(rt: &mut Runtime, url: &str, waiter: Waiter, options: Options) -> VmResult<Lookup> {
    if map(rt)?.entries.len() >= MAX_MODULE_ENTRIES {
        let map = map_mut(rt)?;
        if !std::mem::replace(&mut map.limit_warned, true) {
            log::warn!(
                "the module map has {MAX_MODULE_ENTRIES} entries, the limit: the module {url} \
                 and later new modules fail to load"
            );
        }
        if let Some(error) = map.limit_error {
            return Ok(Lookup::Now(Err(error)));
        }
        let error = failure(rt, url)?;
        map_mut(rt)?.limit_error = Some(error);
        return Ok(Lookup::Now(Err(error)));
    }
    let base = without_fragment(url).to_owned();
    if let Some(&id) = map(rt)?.in_flight.get(&base) {
        let map = map_mut(rt)?;
        map.entries.insert(
            url.to_owned(),
            Entry {
                state: EntryState::Fetching(id),
                waiters: vec![waiter],
                options,
            },
        );
        if let Some((_, entries)) = map.fetches.get_mut(&id) {
            entries.push(url.to_owned());
        }
        if !options.quiet {
            delay_load_of_fetch(rt, id)?;
        }
        return Ok(Lookup::Later);
    }
    let map = map_mut(rt)?;
    let id = map.next_fetch;
    map.next_fetch += 1;
    let fetch = swb_web_fetch(options);
    let started = engine_mut(rt)?.fetch_module(id, &base, fetch);
    if !started {
        // The document may not load the URL: the fetch fails at once.
        let error = failure(rt, url)?;
        map_mut(rt)?.entries.insert(
            url.to_owned(),
            Entry {
                state: EntryState::Failed(error),
                waiters: Vec::new(),
                options,
            },
        );
        return Ok(Lookup::Now(Err(error)));
    }
    let map = map_mut(rt)?;
    map.entries.insert(
        url.to_owned(),
        Entry {
            state: EntryState::Fetching(id),
            waiters: vec![waiter],
            options,
        },
    );
    map.fetches.insert(id, (base.clone(), vec![url.to_owned()]));
    map.in_flight.insert(base, id);
    if options.quiet {
        map.quiet_fetches.insert(id);
    }
    Ok(Lookup::Later)
}

/// The engine's options of a module fetch.
fn swb_web_fetch(options: Options) -> crate::ModuleFetch {
    crate::ModuleFetch {
        credentials: options.credentials,
        delays_load: !options.quiet,
    }
}

/// A new error of a failed fetch of `url` (the map knows it as one).
fn failure(rt: &mut Runtime, url: &str) -> VmResult<Value> {
    let error = rt.new_error(ThrowKind::TypeError, &format!("{FETCH_FAILED}{url}"))?;
    map_mut(rt)?.errors.insert(error, ErrorKind::Fetch);
    Ok(Value::Object(error))
}

/// The module's URL: the response's URL with the fragment of the entry
/// (the request's fragment stays through redirects, Fetch "main fetch").
fn module_url(key: &str, response: &str) -> String {
    match key.split_once('#') {
        Some((_, fragment)) if !response.contains('#') => format!("{response}#{fragment}"),
        _ => response.to_owned(),
    }
}

/// Parses the text of the entry `key` in the document's realm: the
/// module, or its parse error (a `SyntaxError` that the map places at the
/// error's position).
fn parse(
    rt: &mut Runtime,
    key: &str,
    (source, response_url): (&str, &str),
    options: Options,
) -> VmResult<EntryState> {
    let url = module_url(key, response_url);
    Ok(
        match parse_text(rt, source, (&url, (0, 0)), &url, options)? {
            Ok(module) => EntryState::Module(module),
            Err(error) if map(rt)?.is_fetch_failure(error) => EntryState::Failed(error),
            Err(error) => EntryState::ParseError(error),
        },
    )
}

/// Parses the text of an inline module script named `name` (the
/// document's URL, empty for written text) that starts at `start` in its
/// document; its URL (the base of its requests, `import.meta.url`) is
/// `url`, the document's base URL. The module, or its parse error.
pub(crate) fn parse_inline(
    rt: &mut Runtime,
    source: &str,
    name: (&str, (u32, u32)),
    url: &str,
    options: Options,
) -> VmResult<Result<Module, Value>> {
    parse_text(rt, source, name, url, options)
}

/// Parses a module in the document's realm and gives it the next number:
/// the module, its parse error (a `SyntaxError` that the map places at the
/// error's position in the document), or the error of a failed fetch (an
/// engine error of the parse).
fn parse_text(
    rt: &mut Runtime,
    source: &str,
    (name, start): (&str, (u32, u32)),
    url: &str,
    options: Options,
) -> VmResult<Result<Module, Value>> {
    let realm = crate::state::doc_realm(rt)?;
    let number = map(rt)?.infos.len() as u32;
    match rt.parse_module(realm, source, name, start, number) {
        Ok(module) => {
            map_mut(rt)?.infos.push(ModuleInfo {
                url: url.to_owned(),
                credentials: options.credentials,
                quiet: options.quiet,
            });
            Ok(Ok(module))
        }
        Err(ScriptError::Compile {
            kind,
            message,
            offset,
        }) => {
            let error = rt.new_error(kind, &message)?;
            // As `errors::report` places a compile error.
            let (line, column) = Runtime::source_location(source, offset);
            let location = SourceLocation {
                name: name.to_owned(),
                line: line.saturating_add(start.0),
                column: if line == 1 {
                    column.saturating_add(start.1)
                } else {
                    column
                },
            };
            map_mut(rt)?
                .errors
                .insert(error, ErrorKind::Parse(location));
            Ok(Err(Value::Object(error)))
        }
        Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
        Err(error) => {
            log::warn!("the module {name} was not parsed: {error}");
            Ok(Err(failure(rt, url)?))
        }
    }
}

/// The result of the module fetch `id` (`None`: it failed): each entry of
/// the fetch gets its state, then its waiters get their answers in order.
/// Returns the steps of the script elements whose results are ready.
pub(crate) fn fetched(
    rt: &mut Runtime,
    id: u64,
    mut script: Option<FetchedScript>,
) -> VmResult<Vec<(NodeId, WhenReady)>> {
    let map = map_mut(rt)?;
    let Some((base, keys)) = map.fetches.remove(&id) else {
        return Ok(Vec::new());
    };
    map.in_flight.remove(&base);
    map.quiet_fetches.remove(&id);
    let held = script.as_ref().map_or(0, |s| s.source.len());
    let scripts = scripts(rt)?;
    if scripts.held_bytes.saturating_add(held) > MAX_HELD_SCRIPT_BYTES {
        log::warn!(
            "a module of {held} bytes fails: the document holds {} bytes of script text that \
             has not run (the limit is {MAX_HELD_SCRIPT_BYTES})",
            scripts.held_bytes
        );
        script = None;
    }
    let mut answers = Vec::new();
    // The error of a parse that a termination ended: the entries that are
    // left keep their text, and a later request parses it.
    let mut terminated = None;
    for key in keys {
        let Some(entry) = map_mut(rt)?.entries.get_mut(&key) else {
            continue;
        };
        if !matches!(entry.state, EntryState::Fetching(fetch) if fetch == id) {
            continue;
        }
        let waiters = std::mem::take(&mut entry.waiters);
        let options = entry.options;
        let state = match &script {
            None => EntryState::Failed(failure(rt, &key)?),
            Some(script)
                if waiters.iter().all(|w| matches!(w, Waiter::Preload(_)))
                    || terminated.is_some() =>
            {
                // No graph needs the text yet: it waits for its first use.
                EntryState::Fetched(hold_text(rt, script)?)
            }
            Some(script) => match parse(rt, &key, (&script.source, &script.url), options) {
                Ok(state) => state,
                Err(error) => {
                    // The waiters of this entry get no answer, as the loads
                    // that a termination ended.
                    terminated = Some(error);
                    EntryState::Fetched(hold_text(rt, script)?)
                }
            },
        };
        let result = result_of(&state);
        let failed = matches!(state, EntryState::Failed(_));
        if let Some(entry) = map_mut(rt)?.entries.get_mut(&key) {
            entry.state = state;
        }
        if terminated.is_none() {
            answers.extend(
                waiters
                    .into_iter()
                    .map(|waiter| (key.clone(), waiter, result, failed)),
            );
        }
    }
    if let Some(error) = terminated {
        return Err(error);
    }
    let mut ready = Vec::new();
    for (key, waiter, result, failed) in answers {
        match waiter {
            // The element's event comes in a task after the fetch.
            Waiter::Preload(node) => queue_task(
                rt,
                Task::LinkEvent {
                    node,
                    error: failed,
                },
            )?,
            Waiter::Load(token) => rt.finish_loading_imported_module(token, result)?,
            Waiter::Element(node) => ready.extend(super::modules::element_root_ready(rt, node)?),
            // The `import()` is answered before the script elements that
            // wait for the same fetch run (measured, cases
            // `import-call-timing-quiet-then-async` and `-joins-in-flight`:
            // Chromium queues its task first).
            Waiter::Dynamic(token) => {
                super::module_hooks::dynamic_root_ready(rt, token, &key, result)?;
            }
        }
    }
    Ok(ready)
}

/// The text of a fetched script, which waits for its parse.
fn hold_text(rt: &mut Runtime, script: &FetchedScript) -> VmResult<Box<FetchedText>> {
    let held = script.source.len();
    scripts_mut(rt)?.held_bytes += held;
    Ok(Box::new(FetchedText {
        source: script.source.clone(),
        url: script.url.clone(),
        held,
    }))
}

/// The document stops (HTML "abort a document"): the engine drops the
/// fetches of the page, so every entry that fetches fails, as a network
/// error (Chromium, measured with `window.stop()`: the `import()` in flight
/// rejects with the message of a failed fetch, and a later `import()` of the
/// URL gets the stored failure and makes no request). The waiters get their
/// answers; the steps of script elements are dropped with the stopped
/// document's lists.
pub(crate) fn abort_fetches(rt: &mut Runtime) -> VmResult<()> {
    let mut ids: Vec<u64> = map(rt)?.fetches.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        fetched(rt, id, None)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_urls_keep_the_fragment_of_the_entry() {
        assert_eq!(
            module_url("http://a/m.js#f", "http://a/m.js"),
            "http://a/m.js#f"
        );
        assert_eq!(
            module_url("http://a/m.js", "http://a/m.js"),
            "http://a/m.js"
        );
        // A redirect: the response's URL, with the request's fragment.
        assert_eq!(
            module_url("http://a/m.js#f", "http://b/n.js"),
            "http://b/n.js#f"
        );
        assert_eq!(
            module_url("http://a/m.js#f", "http://b/n.js#g"),
            "http://b/n.js#g"
        );
    }

    /// The map keeps its records alive (the host state traces it), and they
    /// die with it (ADR 0034 section 5): a weak reference to the namespace
    /// of a record (the record holds it) is cleared only after the map goes.
    #[test]
    fn the_map_keeps_its_records_alive_and_they_die_with_it() {
        struct Holder(ModuleMap);
        impl swb_js::HostState for Holder {
            fn trace(&self, tracer: &mut Tracer<'_>) {
                self.0.trace(tracer);
            }
        }
        let config = swb_js::RuntimeConfig {
            heap: swb_js::HeapConfig {
                stress: true,
                ..swb_js::HeapConfig::default()
            },
            ..swb_js::RuntimeConfig::default()
        };
        let mut rt = Runtime::new(config).expect("a runtime");
        rt.set_host_data(Holder(ModuleMap::default()));
        let scope = rt.heap_mut().open_scope();
        let module = rt
            .parse_module(0, "export var a = 1;", "m.js", (0, 0), 0)
            .expect("the module parses");
        let entry = Entry {
            state: EntryState::Module(module),
            waiters: Vec::new(),
            options: Options::default(),
        };
        let holder = rt.host_data_mut::<Holder>().expect("the holder");
        holder.0.entries.insert("m.js".to_owned(), entry);
        rt.load_requested_modules(module).expect("the graph loads");
        rt.link(module).expect("the module links");
        let namespace = rt.module_namespace(module).expect("the namespace");
        let global = rt.global(0).expect("the global object");
        let key = rt.key_from_str("ns").expect("a key");
        rt.create_data_property(global, key, namespace)
            .expect("a property");
        rt.heap_mut().close_scope(scope).expect("the scope");
        rt.eval("var w = new WeakRef(ns); delete globalThis.ns;")
            .expect("the script runs");
        rt.run_jobs().expect("the jobs run");
        rt.collect();
        let alive = rt.eval("w.deref() !== undefined").expect("the script runs");
        assert_eq!(alive, Value::Bool(true));
        rt.run_jobs().expect("the jobs run");
        rt.host_data_mut::<Holder>().expect("the holder").0 = ModuleMap::default();
        rt.collect();
        let dead = rt.eval("w.deref() === undefined").expect("the script runs");
        assert_eq!(dead, Value::Bool(true));
    }

    #[test]
    fn fetch_urls_have_no_fragment() {
        assert_eq!(without_fragment("http://a/m.js#f#g"), "http://a/m.js");
        assert_eq!(without_fragment("http://a/m.js"), "http://a/m.js");
    }
}
