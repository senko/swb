//! The module loader of the `swb-js` shell and the test262 runner (ADR
//! 0034 section 7): a specifier is a file path relative to the directory of
//! the module that requests it; a request that leaves the root directory
//! (after `..` and links are resolved) fails with a `TypeError`. Files are
//! read and parsed when a graph first needs them, once per path; the
//! records stay in the host state (traced).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use swb_js::{
    LoadAnswer, LoadRequest, Module, PromiseStatus, Referrer, Runtime, ScriptError, ThrowKind,
    Tracer, Value,
};
use swb_js_syntax::LineIndex;
use swb_js_text::String16;

use super::Host;

/// The modules of a run.
pub(crate) struct ModuleMap {
    /// The directory that requests may not leave (canonical).
    root: PathBuf,
    /// The modules by canonical path.
    by_path: HashMap<PathBuf, Module>,
    /// The path of each module, by its host-defined number.
    paths: Vec<PathBuf>,
    /// The report of the first module that did not parse: `SyntaxError:
    /// message` and its position.
    pub(crate) parse_failure: Option<String>,
}

impl ModuleMap {
    /// A map whose requests stay in `root`.
    pub(crate) fn new(root: &Path) -> std::io::Result<ModuleMap> {
        Ok(ModuleMap {
            root: root.canonicalize()?,
            by_path: HashMap::new(),
            paths: Vec::new(),
            parse_failure: None,
        })
    }

    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for module in self.by_path.values() {
            module.trace(tracer);
        }
    }
}

/// How a module run failed.
#[derive(Debug)]
pub(crate) enum ModuleFailure {
    /// The root module did not compile.
    Parse(ScriptError),
    /// A load or the link failed: the error value (a `TypeError`, the
    /// `SyntaxError` of a module that did not parse, or a link error).
    Resolution(Value),
    /// The evaluation threw: the value.
    Runtime(Value),
    /// A termination, an engine error, or an error of a job.
    Other(ScriptError),
}

/// The modules of the runtime's host state.
fn map(rt: &mut Runtime) -> Option<&mut ModuleMap> {
    rt.host_data_mut::<Host>()?.modules.as_mut()
}

/// The module of a file: parsed on first use (a syntax error becomes a
/// `SyntaxError` value and the map's report).
fn module_of(rt: &mut Runtime, path: &Path) -> Result<Module, Value> {
    if let Some(&module) = map(rt).and_then(|map| map.by_path.get(path)) {
        return Ok(module);
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            let message = format!("Cannot read module {}: {error}", path.display());
            return Err(new_error(rt, ThrowKind::TypeError, &message));
        }
    };
    let source = String::from_utf8_lossy(&bytes).into_owned();
    let Some(number) = map(rt).map(|map| map.paths.len() as u32) else {
        return Err(new_error(rt, ThrowKind::TypeError, "no module map"));
    };
    let name = path.display().to_string();
    match rt.parse_module(0, &source, &name, (0, 0), number) {
        Ok(module) => {
            if let Some(map) = map(rt) {
                map.paths.push(path.to_owned());
                map.by_path.insert(path.to_owned(), module);
            }
            Ok(module)
        }
        Err(ScriptError::Compile {
            kind,
            message,
            offset,
        }) => {
            let text = String16::from(source.as_str());
            let location = LineIndex::new(text.as_str16()).location(offset);
            if let Some(map) = map(rt)
                && map.parse_failure.is_none()
            {
                map.parse_failure = Some(format!(
                    "{}: {message}\n    at {name}:{}:{}",
                    kind.name(),
                    location.line,
                    location.column
                ));
            }
            Err(new_error(rt, kind, &message))
        }
        Err(other) => Err(new_error(rt, ThrowKind::Error, &other.to_string())),
    }
}

fn new_error(rt: &mut Runtime, kind: ThrowKind, message: &str) -> Value {
    rt.new_error(kind, message)
        .map_or(Value::Undefined, Value::Object)
}

/// The file of a specifier: relative to the referrer's directory, inside
/// the root.
fn resolve(rt: &mut Runtime, request: &LoadRequest) -> Result<PathBuf, String> {
    let Referrer::Module(referrer) = request.referrer else {
        return Err("only modules import".to_owned());
    };
    let number = rt.module_host_defined(referrer).unwrap_or(u32::MAX);
    let map = map(rt).ok_or("no module map")?;
    let base = map
        .paths
        .get(number as usize)
        .and_then(|path| path.parent())
        .ok_or("a referrer without a path")?;
    let joined = base.join(&request.specifier);
    let path = joined
        .canonicalize()
        .map_err(|error| format!("Cannot find module '{}': {error}", request.specifier))?;
    if !path.starts_with(&map.root) {
        return Err(format!(
            "Cannot load module '{}': outside {}",
            request.specifier,
            map.root.display()
        ));
    }
    Ok(path)
}

/// `HostLoadImportedModule`: the answer comes now.
#[allow(clippy::needless_pass_by_value, reason = "the signature of the hook")]
pub(crate) fn load(rt: &mut Runtime, request: LoadRequest) -> LoadAnswer {
    // The engine has refused every key other than `type`. A value other
    // than "json" and "css" is a `TypeError` with Chromium 148's text. No
    // module type other than JavaScript exists yet (ADR 0034 section 4), so
    // "json" and "css" are unsupported.
    if let Some((_, kind)) = request.attributes.iter().find(|(key, _)| key == "type") {
        let message = if kind == "json" || kind == "css" {
            format!("Unsupported module type '{kind}'")
        } else {
            format!("\"{kind}\" is not a valid module type.")
        };
        return LoadAnswer::Now(Err(new_error(rt, ThrowKind::TypeError, &message)));
    }
    match resolve(rt, &request) {
        Ok(path) => LoadAnswer::Now(module_of(rt, &path)),
        Err(message) => LoadAnswer::Now(Err(new_error(rt, ThrowKind::TypeError, &message))),
    }
}

/// Runs the file `path` (with the text `source`) as a module: load the
/// graph, link, evaluate, then the jobs. Requests may not leave `root`.
pub(crate) fn run_module(
    rt: &mut Runtime,
    root: &Path,
    path: &Path,
    source: &str,
) -> Result<(), ModuleFailure> {
    let map =
        ModuleMap::new(root).map_err(|error| ModuleFailure::Other(io_error(&error.to_string())))?;
    let path = path
        .canonicalize()
        .map_err(|error| ModuleFailure::Other(io_error(&error.to_string())))?;
    if let Some(host) = rt.host_data_mut::<Host>() {
        host.modules = Some(map);
    }
    let name = path.display().to_string();
    let module = rt
        .parse_module(0, source, &name, (0, 0), 0)
        .map_err(ModuleFailure::Parse)?;
    if let Some(map) = super::loader::map(rt) {
        map.paths.push(path.clone());
        map.by_path.insert(path, module);
    }
    let promise = rt
        .load_requested_modules(module)
        .map_err(|error| ModuleFailure::Other(io_error(&format!("{error:?}"))))?;
    if let Some((PromiseStatus::Rejected, error)) = rt.promise_settlement(promise) {
        return Err(ModuleFailure::Resolution(error));
    }
    match rt.link(module) {
        Ok(()) => {}
        Err(ScriptError::Uncaught { .. }) => {
            return Err(ModuleFailure::Resolution(rt.last_value()));
        }
        Err(other) => return Err(ModuleFailure::Other(other)),
    }
    let promise = rt.evaluate(module).map_err(ModuleFailure::Other)?;
    if let Some((PromiseStatus::Rejected, error)) = rt.promise_settlement(promise) {
        return Err(ModuleFailure::Runtime(error));
    }
    rt.run_jobs().map_err(ModuleFailure::Other)?;
    while rt
        .run_finalization_cleanup()
        .map_err(ModuleFailure::Other)?
    {
        rt.run_jobs().map_err(ModuleFailure::Other)?;
    }
    Ok(())
}

/// A failure of the host (a file that cannot be read) as an error.
fn io_error(message: &str) -> ScriptError {
    ScriptError::Uncaught {
        name: "Error".to_owned(),
        message: message.to_owned(),
        offset: None,
    }
}

/// The report of a failed module run, as the shell prints it, and its
/// exit code (as for scripts: 1 uncaught, 2 compile error, 3 termination).
pub(crate) fn describe_failure(
    rt: &mut Runtime,
    failure: &ModuleFailure,
    name: &str,
    source: &str,
) -> (String, u8) {
    match failure {
        ModuleFailure::Parse(error) | ModuleFailure::Other(error) => (
            super::describe_error(error, name, source),
            super::exit_code(error),
        ),
        ModuleFailure::Resolution(value) => {
            if let Some(report) = map(rt).and_then(|map| map.parse_failure.clone()) {
                return (report, 2);
            }
            (value_report(rt, *value), 1)
        }
        ModuleFailure::Runtime(value) => (value_report(rt, *value), 1),
    }
}

/// `Uncaught Name: message` and the location of an error value.
fn value_report(rt: &Runtime, value: Value) -> String {
    let report = rt.error_report(value);
    let first = report.lines().next().unwrap_or("").to_owned();
    match rt.error_location(value) {
        Some(location) => format!(
            "Uncaught {first}\n    at {}:{}:{}",
            location.name, location.line, location.column
        ),
        None => format!("Uncaught {first}"),
    }
}
