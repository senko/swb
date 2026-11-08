//! The module loader of the `swb-js` shell and the test262 runner (ADR
//! 0034 section 7): a specifier is a file path relative to the directory of
//! the script or module that requests it (static imports and `import()`);
//! a request that leaves the root directory of that script or module
//! (after `..` and links are resolved) fails with a `TypeError`. A module
//! has the root of the module or script that loaded it first; code without
//! a script (`-e`, `$262.evalScript`) resolves from the directory of the
//! first file of the run (the working directory before one). Files are
//! read and parsed when a graph first needs them, once per path; the
//! records stay in the host state (traced). `import.meta.url` is the
//! file URL of the module.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use swb_js::{
    LoadAnswer, LoadRequest, Module, PromiseStatus, Referrer, Runtime, ScriptError, ThrowKind,
    Tracer, Value, VmResult,
};
use swb_js_syntax::LineIndex;
use swb_js_text::String16;

use super::Host;

/// A script or module file of a run: its path and the directory that its
/// requests may not leave (both canonical).
struct File {
    path: PathBuf,
    root: PathBuf,
}

/// The scripts and modules of a run.
#[derive(Default)]
pub(crate) struct ModuleMap {
    /// The modules by canonical path.
    by_path: HashMap<PathBuf, Module>,
    /// The file of each script and module, by its host-defined number.
    files: Vec<File>,
    /// The base directory and the root of code without a script: those of
    /// the first file.
    default: Option<(PathBuf, PathBuf)>,
    /// The report of the first module that did not parse: `SyntaxError:
    /// message` and its position.
    pub(crate) parse_failure: Option<String>,
}

impl ModuleMap {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for module in self.by_path.values() {
            module.trace(tracer);
        }
    }

    /// Registers a file (canonical paths); returns its host-defined number.
    fn add(&mut self, path: PathBuf, root: PathBuf) -> u32 {
        if self.default.is_none() {
            let base = path.parent().unwrap_or(&root).to_owned();
            self.default = Some((base, root.clone()));
        }
        self.files.push(File { path, root });
        (self.files.len() - 1) as u32
    }

    /// The base directory and the root of a referrer.
    fn base_of(&self, number: Option<u32>) -> Result<(PathBuf, PathBuf), String> {
        let Some(number) = number else {
            if let Some(default) = &self.default {
                return Ok(default.clone());
            }
            let cwd = std::env::current_dir()
                .and_then(|dir| dir.canonicalize())
                .map_err(|error| format!("no working directory: {error}"))?;
            return Ok((cwd.clone(), cwd));
        };
        let file = self
            .files
            .get(number as usize)
            .ok_or("a referrer without a file")?;
        let base = file.path.parent().ok_or("a referrer without a directory")?;
        Ok((base.to_owned(), file.root.clone()))
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
    /// No error, but the promise of the evaluation is still pending after
    /// the jobs (a top-level `await` that never ends). The shell ends such
    /// a run as a success; the test262 runner judges it.
    Pending,
}

/// The modules of the runtime's host state.
fn map(rt: &mut Runtime) -> Option<&mut ModuleMap> {
    Some(&mut rt.host_data_mut::<Host>()?.modules)
}

/// The host-defined number of a script file whose requests stay in
/// `root` (`None` if a path does not exist).
pub(crate) fn register_script(rt: &mut Runtime, path: &Path, root: &Path) -> Option<u32> {
    let path = path.canonicalize().ok()?;
    let root = root.canonicalize().ok()?;
    Some(map(rt)?.add(path, root))
}

/// The module of a file: parsed on first use (a syntax error becomes a
/// `SyntaxError` value and the map's report). A new module gets `root`.
fn module_of(rt: &mut Runtime, path: &Path, root: &Path) -> Result<Module, Value> {
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
    let Some(number) = map(rt).map(|map| map.files.len() as u32) else {
        return Err(new_error(rt, ThrowKind::TypeError, "no module map"));
    };
    let name = path.display().to_string();
    match rt.parse_module(0, &source, &name, (0, 0), number) {
        Ok(module) => {
            if let Some(map) = map(rt) {
                map.add(path.to_owned(), root.to_owned());
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

/// The file of a specifier and the root of its module: relative to the
/// referrer's directory, inside the referrer's root.
fn resolve(rt: &mut Runtime, request: &LoadRequest) -> Result<(PathBuf, PathBuf), String> {
    let number = match request.referrer {
        Referrer::Module(module) => Some(rt.module_host_defined(module).unwrap_or(u32::MAX)),
        Referrer::Script(number) => Some(number),
        Referrer::Realm(_) => None,
    };
    let map = map(rt).ok_or("no module map")?;
    let (base, root) = map.base_of(number)?;
    // The root is checked on the lexical path first, so the answer for a
    // path outside the root does not depend on the file system; then on
    // the canonical path (links).
    let joined = normalize(&base.join(&request.specifier));
    let outside = || {
        format!(
            "Cannot load module '{}': outside {}",
            request.specifier,
            root.display()
        )
    };
    if !joined.starts_with(&root) {
        return Err(outside());
    }
    let path = joined
        .canonicalize()
        .map_err(|error| format!("Cannot find module '{}': {error}", request.specifier))?;
    if !path.starts_with(&root) {
        return Err(outside());
    }
    Ok((path, root))
}

/// The path without `.` components and with each `..` applied to the
/// component before it (no file system access).
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
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
        Ok((path, root)) => LoadAnswer::Now(module_of(rt, &path, &root)),
        Err(message) => LoadAnswer::Now(Err(new_error(rt, ThrowKind::TypeError, &message))),
    }
}

/// `HostGetImportMetaProperties`: `url`, the file URL of the module.
pub(crate) fn import_meta(
    rt: &mut Runtime,
    module: Module,
    meta: swb_js::Gc<swb_js::Object>,
) -> VmResult<()> {
    let number = rt.module_host_defined(module);
    let path = map(rt)
        .and_then(|map| map.files.get(number? as usize))
        .map(|file| file.path.clone());
    let Some(path) = path else {
        return Ok(());
    };
    let url = rt.string_value(&file_url(&path))?;
    let key = rt.key_from_str("url")?;
    rt.create_data_property(meta, key, url)
}

/// The `file:` URL of an absolute path: the bytes outside the unreserved
/// characters and `/` percent-encoded.
fn file_url(path: &Path) -> String {
    use std::fmt::Write as _;
    let mut url = String::from("file://");
    for &byte in path.to_string_lossy().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            url.push(char::from(byte));
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

/// Runs the file `path` (with the text `source`) as a module: load the
/// graph, link, evaluate, then the jobs. Requests may not leave `root`.
pub(crate) fn run_module(
    rt: &mut Runtime,
    root: &Path,
    path: &Path,
    source: &str,
) -> Result<(), ModuleFailure> {
    let io = |error: std::io::Error| ModuleFailure::Other(io_error(&error.to_string()));
    let path = path.canonicalize().map_err(io)?;
    let root = root.canonicalize().map_err(io)?;
    let name = path.display().to_string();
    let number = map(rt).map_or(0, |map| map.files.len() as u32);
    let module = rt
        .parse_module(0, source, &name, (0, 0), number)
        .map_err(ModuleFailure::Parse)?;
    if let Some(map) = map(rt) {
        map.add(path.clone(), root);
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
    // The module's record keeps the promise alive.
    let promise = rt.evaluate(module).map_err(ModuleFailure::Other)?;
    if let Some((PromiseStatus::Rejected, error)) = rt.promise_settlement(promise) {
        return Err(ModuleFailure::Runtime(error));
    }
    // With top-level `await` the promise settles in a job: its rejection
    // is reported as an uncaught error after the jobs. A promise that never
    // settles reports nothing.
    rt.run_jobs().map_err(ModuleFailure::Other)?;
    if let Some((PromiseStatus::Rejected, error)) = rt.promise_settlement(promise) {
        return Err(ModuleFailure::Runtime(error));
    }
    while rt
        .run_finalization_cleanup()
        .map_err(ModuleFailure::Other)?
    {
        rt.run_jobs().map_err(ModuleFailure::Other)?;
    }
    if let Some((PromiseStatus::Pending, _)) = rt.promise_settlement(promise) {
        return Err(ModuleFailure::Pending);
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
        ModuleFailure::Pending => (String::new(), 0),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_applies_dots_without_the_file_system() {
        assert_eq!(
            normalize(Path::new("/a/b/./../c/x.js")),
            PathBuf::from("/a/c/x.js")
        );
        assert_eq!(normalize(Path::new("/../..")), PathBuf::from("/"));
    }

    #[test]
    fn file_urls_encode_other_bytes() {
        assert_eq!(
            file_url(Path::new("/a b/ü-x_1.js")),
            "file:///a%20b/%C3%BC-x_1.js"
        );
    }
}
