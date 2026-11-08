//! The module hooks of the page (ADR 0034 sections 8 and 9): HTML
//! `HostLoadImportedModule`
//! (<https://html.spec.whatwg.org/multipage/webappapis.html#hostloadimportedmodule>)
//! with "resolve a module specifier" and the document's import map
//! (`import_map.rs`), and `import.meta` (`HostGetImportMetaProperties`:
//! `url` and `resolve`). The runtime installs them before the page compiles
//! its first script, so `import()` and `import.meta` compile.
//!
//! The base URL of a request is the URL of the module that asks; for a
//! classic script it is the script's base URL (`Referrer::Script`: the
//! script's URL, or the document's base URL when an inline script was
//! prepared); code without a script and event handlers (`Referrer::Realm`
//! and the number 0) resolve against the document's URL. A script number
//! names the realm of the document that prepared the script: the request is
//! answered in that document (its import map and module map), wherever the
//! script's function is called from.
//!
//! `import()` loads the whole graph of its root before it answers the
//! engine, as Chromium does: a failed fetch anywhere in the graph rejects
//! with `TypeError: Failed to fetch dynamically imported module: <root>`
//! (measured, case `msg-import-call-descendant`); a parse, resolution or
//! link error rejects with that error.
//!
//! The messages are Chromium 148's (measured in `tools/probes/web-module-graph.json`).

use swb_js::{
    Gc, LoadAnswer, LoadRequest, Module, ModuleHooks, NativeCall, NativeReturn, Object,
    PromiseStatus, ReactionKind, Referrer, Runtime, ThrowKind, Value, VmError, VmResult,
};
use url::Url;

use super::import_map::{ImportMap, Resolved};
use super::module_map::{self, FETCH_FAILED, GraphOwner, Lookup, Options, Waiter, map_mut};
use crate::event_loop::{Task, queue_task};
use crate::state::engine_mut;

/// The text of a resolution error that Chromium adds to a bare specifier.
const RELATIVE_REFERENCES: &str =
    "Relative references must start with either \"/\", \"./\", or \"../\".";

/// The text of a resolution error of a URL-like specifier that does not
/// parse (measured, case `msg-lone-surrogate`).
const INVALID_RELATIVE: &str = "Invalid relative url or base scheme isn't hierarchical.";

/// Installs the hooks in the runtime (before the page compiles a script).
pub(crate) fn install(rt: &mut Runtime) {
    rt.set_module_hooks(ModuleHooks {
        load_imported_module,
        import_meta_properties: Some(import_meta_properties),
    });
}

/// Why a specifier did not resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Unresolved {
    /// Not a URL and not URL-like (a bare specifier).
    Bare,
    /// URL-like (`/`, `./`, `../`), but it does not parse against the base.
    /// Also the empty specifier (measured, case `msg-resolve-empty`).
    InvalidRelative,
    /// The import map blocks it: the text of the reason (`Import Map: ...`).
    ImportMap(String),
}

/// Who resolves: the message of an error differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolver {
    /// A static import.
    Static,
    /// `import()`.
    Dynamic,
    /// `import.meta.resolve()`.
    Meta,
}

/// HTML "resolve a module specifier" with the import map `map`: the map's
/// rules, else a URL-like specifier parsed against `base`, else an absolute
/// URL. `lone_surrogate` is the byte offset of the first lone surrogate of the
/// specifier: Chromium fails to resolve a specifier with one before its
/// fragment (measured, case `msg-lone-surrogate`); the map is not asked.
pub(crate) fn resolve(
    map: &mut ImportMap,
    specifier: &str,
    lone_surrogate: Option<usize>,
    base: &Url,
) -> Result<Url, Unresolved> {
    let url_like =
        specifier.starts_with('/') || specifier.starts_with("./") || specifier.starts_with("../");
    let fragment = specifier.find('#').unwrap_or(specifier.len());
    let broken = lone_surrogate.is_some_and(|at| at < fragment);
    let failure = if url_like || specifier.is_empty() {
        Unresolved::InvalidRelative
    } else {
        Unresolved::Bare
    };
    if broken {
        return Err(failure);
    }
    match map.resolve(specifier, base) {
        Resolved::Url(url) => Ok(url),
        Resolved::None => Err(failure),
        Resolved::Blocked(blocked) => Err(Unresolved::ImportMap(blocked.text())),
    }
}

/// The message of a resolution error (Chromium 148's texts: `resolve-error`,
/// `import-call-errors`, `msg-meta-resolve`, `msg-lone-surrogate`,
/// `msg-importmap-resolve`).
pub(crate) fn resolve_message(resolver: Resolver, specifier: &str, why: &Unresolved) -> String {
    let reason = match why {
        Unresolved::Bare => RELATIVE_REFERENCES,
        Unresolved::InvalidRelative => INVALID_RELATIVE,
        Unresolved::ImportMap(text) => text,
    };
    match resolver {
        Resolver::Static => format!("Failed to resolve module specifier \"{specifier}\". {reason}"),
        Resolver::Dynamic => format!("Failed to resolve module specifier '{specifier}'"),
        Resolver::Meta => format!("Failed to resolve module specifier {specifier}: {reason}"),
    }
}

/// The error of an import attribute `type` (the engine refused the other
/// keys): swb has no JSON and no CSS modules (ADR 0034 section 4; a
/// departure from Chromium, which has them), and another value is not a
/// module type (Chromium 148's text, measured: `msg-import-attribute`,
/// `msg-import-call-options`).
fn attribute_error(attributes: &[(String, String)]) -> Option<String> {
    let (_, kind) = attributes.iter().find(|(key, _)| key == "type")?;
    Some(if kind == "json" || kind == "css" {
        format!("Unsupported module type '{kind}'")
    } else {
        format!("\"{kind}\" is not a valid module type.")
    })
}

/// A new `TypeError` (the error of a load).
fn type_error(rt: &mut Runtime, message: &str) -> VmResult<Value> {
    Ok(Value::Object(rt.new_error(ThrowKind::TypeError, message)?))
}

/// `HostLoadImportedModule`: in the document of the referrer's realm. An
/// error of the host (a termination in a parse, a document that is not
/// there) leaves the request without an answer: its load waits for ever,
/// as a load that a termination ended.
#[allow(clippy::needless_pass_by_value, reason = "the signature of the hook")]
fn load_imported_module(rt: &mut Runtime, request: LoadRequest) -> LoadAnswer {
    let realm = match request.referrer {
        Referrer::Module(module) => rt.module_realm(module),
        Referrer::Realm(realm) => Some(realm),
        Referrer::Script(number) => crate::state::state(rt)
            .ok()
            .and_then(|state| state.script_bases.get(number))
            .map(|base| base.realm),
    };
    let entered = match realm {
        Some(realm) => crate::state::enter_realm(rt, realm),
        None => Ok(crate::state::NO_SWITCH),
    };
    let answer = entered.and_then(|entered| {
        let answer = answer(rt, &request);
        crate::state::leave(rt, entered);
        answer
    });
    answer.unwrap_or_else(|error| {
        log::warn!("the module {} was not loaded: {error:?}", request.specifier);
        LoadAnswer::Later
    })
}

/// What a referrer's requests resolve against.
enum Base {
    /// The base URL and the fetch options of the requests.
    Url(String, Options),
    /// A classic script of another origin, fetched without CORS: its
    /// `import()` is rejected (HTML "muted errors"; Chromium, measured, case
    /// `import-call-base-cors-message`).
    Muted,
}

/// The base URL and the fetch options of a referrer's requests: a module's
/// URL; a classic script's base URL (its URL, or the document's base URL when
/// it was prepared); the document's URL for code without a script and for
/// event handlers (Chromium, measured: `import-call-document-base`,
/// `import-call-base-*`).
fn referrer_base(rt: &mut Runtime, referrer: Referrer) -> VmResult<Base> {
    match referrer {
        Referrer::Module(module) => {
            if let Some(number) = rt.module_host_defined(module)
                && let Some(info) = map_mut(rt)?.info(number)
            {
                let options = Options {
                    credentials: info.credentials,
                    quiet: info.quiet,
                };
                return Ok(Base::Url(info.url.clone(), options));
            }
        }
        Referrer::Script(number) => match crate::state::state(rt)?.script_bases.get(number) {
            Some(base) if base.muted => return Ok(Base::Muted),
            Some(base) => return Ok(Base::Url(base.url.clone(), Options::default())),
            None => {}
        },
        Referrer::Realm(_) => {}
    }
    // The default options: credentials `same-origin`.
    Ok(Base::Url(engine_mut(rt)?.url(), Options::default()))
}

/// The answer to a request in the current document.
fn answer(rt: &mut Runtime, request: &LoadRequest) -> VmResult<LoadAnswer> {
    if let Some(message) = attribute_error(&request.attributes) {
        return Ok(LoadAnswer::Now(Err(type_error(rt, &message)?)));
    }
    let (base, mut options) = match referrer_base(rt, request.referrer)? {
        Base::Url(base, options) => (base, options),
        Base::Muted => {
            let message = format!(
                "Failed to resolve module specifier '{}'. The base URL is about:blank because \
                 import() is called from a CORS-cross-origin script.",
                request.specifier
            );
            return Ok(LoadAnswer::Now(Err(type_error(rt, &message)?)));
        }
    };
    let resolver = if request.dynamic {
        Resolver::Dynamic
    } else {
        Resolver::Static
    };
    let resolved = Url::parse(&base)
        .map_err(|_| Unresolved::InvalidRelative)
        .and_then(|base| {
            let map = &mut map_mut(rt)
                .map_err(|_| Unresolved::InvalidRelative)?
                .import_map;
            resolve(map, &request.specifier, request.lone_surrogate, &base)
        });
    let url = match resolved {
        Ok(url) => url.to_string(),
        Err(why) => {
            let message = resolve_message(resolver, &request.specifier, &why);
            return Ok(LoadAnswer::Now(Err(type_error(rt, &message)?)));
        }
    };
    if !request.dynamic {
        return Ok(
            match module_map::request(rt, &url, Waiter::Load(request.token), options)? {
                Lookup::Now(result) => LoadAnswer::Now(result),
                Lookup::Later => LoadAnswer::Later,
            },
        );
    }
    // The fetches of `import()` do not delay the `load` event.
    options.quiet = true;
    match module_map::request(rt, &url, Waiter::Dynamic(request.token), options)? {
        Lookup::Now(_) => {
            queue_import(rt, request.token, &url, options)?;
            Ok(LoadAnswer::Later)
        }
        Lookup::Later => Ok(LoadAnswer::Later),
    }
}

/// Queues the answer of the `import()` with the token `token` for the root
/// `url`, which the map has, in a task ([`Task::ModuleImport`]). Chromium
/// settles an `import()` of a module that is in the map, in any state, in a
/// task that it queues at the call, in order with the timers and messages
/// queued between the calls (measured, cases `import-call-timing-*` and
/// `msg-import-call-meta`); HTML answers at once. A specifier that does not
/// resolve rejects at once. An `import()` that waits for a fetch gets its
/// answer when the fetch arrives, before the script elements that wait for it
/// run ([`module_map::fetched`]).
fn queue_import(rt: &mut Runtime, token: u32, url: &str, options: Options) -> VmResult<()> {
    map_mut(rt)?
        .waiting_imports
        .insert(token, (url.to_owned(), options));
    queue_task(rt, Task::ModuleImport(token))
}

/// [`Task::ModuleImport`]: the `import()` with the token `token` gets the
/// answer of the map (its root's graph loads first).
pub(crate) fn answer_import(rt: &mut Runtime, token: u32) -> VmResult<()> {
    let Some((url, options)) = map_mut(rt)?.waiting_imports.remove(&token) else {
        return Ok(());
    };
    match module_map::request(rt, &url, Waiter::Dynamic(token), options)? {
        Lookup::Now(result) => dynamic_root_ready(rt, token, &url, result),
        Lookup::Later => Ok(()),
    }
}

/// The error of an `import()` of `url` from the error of its graph's load:
/// a failed fetch becomes Chromium's `TypeError` with the root's URL.
fn dynamic_error(rt: &mut Runtime, url: &str, error: Value) -> VmResult<Value> {
    if map_mut(rt)?.is_fetch_failure(error) {
        return type_error(rt, &format!("{FETCH_FAILED}{url}"));
    }
    Ok(error)
}

/// Loads the graph of the root `root` of the `import()` with the token
/// `token`: the answer now if the load ends now, else when its promise
/// settles ([`graph_settled`]).
fn start_dynamic_graph(
    rt: &mut Runtime,
    token: u32,
    url: &str,
    root: Module,
) -> VmResult<LoadAnswer> {
    let promise = rt.load_requested_modules(root)?;
    match rt.promise_settlement(promise) {
        Some((PromiseStatus::Fulfilled, _)) => Ok(LoadAnswer::Now(Ok(root))),
        Some((PromiseStatus::Rejected, error)) => {
            Ok(LoadAnswer::Now(Err(dynamic_error(rt, url, error)?)))
        }
        _ => {
            let owner = GraphOwner::Dynamic {
                token,
                url: url.to_owned(),
            };
            react_to_graph(rt, promise, owner, root)?;
            Ok(LoadAnswer::Later)
        }
    }
}

/// The root of the `import()` with the token `token` arrived (`result`: the
/// module of the entry `url`, or its error): its graph loads, and the engine
/// gets the answer when it is complete.
pub(crate) fn dynamic_root_ready(
    rt: &mut Runtime,
    token: u32,
    url: &str,
    result: Result<Module, Value>,
) -> VmResult<()> {
    let answer = match result {
        Ok(root) => start_dynamic_graph(rt, token, url, root)?,
        Err(error) => LoadAnswer::Now(Err(dynamic_error(rt, url, error)?)),
    };
    match answer {
        LoadAnswer::Now(result) => rt.finish_loading_imported_module(token, result),
        LoadAnswer::Later => Ok(()),
    }
}

/// Attaches [`graph_settled`] to the promise of a graph load of the page.
pub(super) fn react_to_graph(
    rt: &mut Runtime,
    promise: Value,
    owner: GraphOwner,
    root: Module,
) -> VmResult<()> {
    let key = crate::state::document_key(rt)?;
    let id = map_mut(rt)?.add_graph(owner, root);
    // The document and the graph in one number (a document key is far below
    // 2^21, so the value is exact).
    let data = Value::number(f64::from(key) * 4_294_967_296.0 + f64::from(id));
    match rt.react_to_promise(promise, graph_settled, data) {
        Ok(_) => Ok(()),
        Err(error) => Err(super::modules::vm_error(&error)),
    }
}

/// The promise of a graph load of the page settled (a job): a script
/// element's graph is ready for its link, which runs in a task
/// ([`super::modules::graph_loaded`]); an `import()` gets its answer.
fn graph_settled(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    value: Value,
) -> VmResult<Value> {
    let Some(number) = data.as_number() else {
        return Ok(Value::Undefined);
    };
    // The inverse of the encoding in `react_to_graph`.
    let key = (number / 4_294_967_296.0) as u32;
    let id = (number % 4_294_967_296.0) as u32;
    let Ok(entered) = crate::state::switch_if_other(rt, key) else {
        return Ok(Value::Undefined);
    };
    let result = graph_settled_here(rt, id, kind, value);
    crate::state::leave(rt, entered);
    result.map(|()| Value::Undefined)
}

fn graph_settled_here(rt: &mut Runtime, id: u32, kind: ReactionKind, value: Value) -> VmResult<()> {
    let Some(graph) = map_mut(rt)?.take_graph(id) else {
        return Ok(());
    };
    let error = matches!(kind, ReactionKind::Reject).then_some(value);
    match graph.owner {
        GraphOwner::Element(node) => super::modules::graph_loaded(rt, node, graph.root, error),
        GraphOwner::Dynamic { token, url } => {
            let result = match error {
                None => Ok(graph.root),
                Some(error) => Err(dynamic_error(rt, &url, error)?),
            };
            rt.finish_loading_imported_module(token, result)
        }
    }
}

/// `HostGetImportMetaProperties` and `HostFinalizeImportMeta` (HTML):
/// `url`, the module's URL, then `resolve`, a function of the module
/// (measured: keys `url,resolve`, both writable, enumerable and
/// configurable; case `msg-meta-resolve`).
fn import_meta_properties(rt: &mut Runtime, module: Module, meta: Gc<Object>) -> VmResult<()> {
    let Some(realm) = rt.module_realm(module) else {
        return Ok(());
    };
    let entered = crate::state::enter_realm(rt, realm)?;
    let result = define_meta(rt, module, meta, realm);
    crate::state::leave(rt, entered);
    result
}

fn define_meta(rt: &mut Runtime, module: Module, meta: Gc<Object>, realm: u32) -> VmResult<()> {
    let Some(number) = rt.module_host_defined(module) else {
        return Ok(());
    };
    let Some(url) = map_mut(rt)?.info(number).map(|info| info.url.clone()) else {
        return Ok(());
    };
    let value = rt.string_value(&url)?;
    let key = rt.key_from_str("url")?;
    rt.create_data_property(meta, key, value)?;
    let function = rt.new_host_method(realm, "resolve", 1, meta_resolve)?;
    map_mut(rt)?.resolvers.insert(function, number);
    let key = rt.key_from_str("resolve")?;
    rt.create_data_property(meta, key, Value::Object(function))
}

/// `import.meta.resolve(specifier)` (HTML
/// <https://html.spec.whatwg.org/multipage/webappapis.html#import-meta-resolve>):
/// the URL of the specifier against the module's URL, as a string; a
/// `TypeError` if it does not resolve.
fn meta_resolve(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let entered = crate::state::enter_realm(rt, call.realm())?;
    let result = meta_resolve_here(rt, call);
    crate::state::leave(rt, entered);
    result
}

fn meta_resolve_here(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let number = map_mut(rt)?.resolvers.get(&call.callee()).copied();
    let base = number
        .and_then(|number| map_mut(rt).ok()?.info(number))
        .map(|info| info.url.clone());
    let Some(base) = base else {
        return Err(VmError::invariant("import.meta.resolve without its module"));
    };
    let argument = rt.arg(call, 0);
    // A string keeps its lone surrogates for the check of `resolve`
    // (measured: `./a\uD800.js` does not resolve); the result of a
    // conversion is lossy.
    let (specifier, lone_surrogate) = match argument {
        Value::String(string) => swb_js::decode_specifier(rt.string_units(string)?),
        other => (rt.to_rust_string(other)?, None),
    };
    let resolved = Url::parse(&base)
        .map_err(|_| Unresolved::InvalidRelative)
        .and_then(|base| {
            let map = &mut map_mut(rt)
                .map_err(|_| Unresolved::InvalidRelative)?
                .import_map;
            resolve(map, &specifier, lone_surrogate, &base)
        });
    match resolved {
        Ok(url) => Ok(NativeReturn::Value(rt.string_value(url.as_str())?)),
        Err(why) => Err(VmError::type_error(resolve_message(
            Resolver::Meta,
            &specifier,
            &why,
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("http://a.test/dir/m.js").unwrap()
    }

    #[test]
    fn url_like_and_absolute_specifiers_resolve() {
        let r =
            |s: &str| resolve(&mut ImportMap::default(), s, None, &base()).map(|u| u.to_string());
        assert_eq!(r("./x.js"), Ok("http://a.test/dir/x.js".to_owned()));
        assert_eq!(r("../y/z.js#h"), Ok("http://a.test/y/z.js#h".to_owned()));
        assert_eq!(r("/abs.js"), Ok("http://a.test/abs.js".to_owned()));
        assert_eq!(r("https://b.test/q"), Ok("https://b.test/q".to_owned()));
        assert_eq!(r("bare"), Err(Unresolved::Bare));
        assert_eq!(r("x.js"), Err(Unresolved::Bare));
        assert_eq!(r("//[x"), Err(Unresolved::InvalidRelative));
    }

    #[test]
    fn lone_surrogates_before_the_fragment_do_not_resolve() {
        // The engine gives the specifier with U+FFFD and the offset.
        let s = "./s\u{fffd}.js";
        assert_eq!(
            resolve(&mut ImportMap::default(), s, Some(3), &base()),
            Err(Unresolved::InvalidRelative)
        );
        let s = "http://a.test/u\u{fffd}.js";
        assert_eq!(
            resolve(&mut ImportMap::default(), s, Some(15), &base()),
            Err(Unresolved::Bare)
        );
        let s = "./ok.js#\u{fffd}";
        assert!(resolve(&mut ImportMap::default(), s, Some(8), &base()).is_ok());
    }

    #[test]
    fn messages_are_chromiums() {
        assert_eq!(
            resolve_message(Resolver::Static, "bare-x", &Unresolved::Bare),
            "Failed to resolve module specifier \"bare-x\". Relative references must start with \
             either \"/\", \"./\", or \"../\"."
        );
        assert_eq!(
            resolve_message(Resolver::Dynamic, "bare", &Unresolved::Bare),
            "Failed to resolve module specifier 'bare'"
        );
        assert_eq!(
            resolve_message(Resolver::Meta, "bare", &Unresolved::Bare),
            "Failed to resolve module specifier bare: Relative references must start with either \
             \"/\", \"./\", or \"../\"."
        );
        assert_eq!(
            resolve_message(Resolver::Static, "//[x", &Unresolved::InvalidRelative),
            "Failed to resolve module specifier \"//[x\". Invalid relative url or base scheme \
             isn't hierarchical."
        );
    }

    #[test]
    fn the_empty_specifier_and_blocked_specifiers() {
        let mut map = ImportMap::default();
        // Measured, case `msg-resolve-empty`.
        assert_eq!(
            resolve(&mut map, "", None, &base()),
            Err(Unresolved::InvalidRelative)
        );
        let parsed =
            super::super::import_map::parse(r#"{"imports": {"n": null, "p/": "/p/"}}"#, &base())
                .unwrap();
        map.register(parsed);
        let why = resolve(&mut map, "n", None, &base()).unwrap_err();
        assert_eq!(
            resolve_message(Resolver::Static, "n", &why),
            "Failed to resolve module specifier \"n\". Import Map: \"n\" matches with \"n\" but \
             is blocked by a null value"
        );
        // `import()` has no reason; `import.meta.resolve` has the colon form.
        assert_eq!(
            resolve_message(Resolver::Dynamic, "n", &why),
            "Failed to resolve module specifier 'n'"
        );
        assert_eq!(
            resolve_message(Resolver::Meta, "n", &why),
            "Failed to resolve module specifier n: Import Map: \"n\" matches with \"n\" but is \
             blocked by a null value"
        );
        assert_eq!(
            resolve(&mut map, "p/x.js", None, &base()).map(|u| u.to_string()),
            Ok("http://a.test/p/x.js".to_owned())
        );
    }

    #[test]
    fn import_attribute_types() {
        let attrs = |v: &str| vec![("type".to_owned(), v.to_owned())];
        assert_eq!(
            attribute_error(&attrs("javascript")).as_deref(),
            Some("\"javascript\" is not a valid module type.")
        );
        assert_eq!(
            attribute_error(&attrs("json")).as_deref(),
            Some("Unsupported module type 'json'")
        );
        assert_eq!(attribute_error(&[]), None);
    }
}
