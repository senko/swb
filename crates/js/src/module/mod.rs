//! Modules (ECMA-262 §16.2, <https://tc39.es/ecma262/2025/#sec-modules>;
//! ADR 0034 sections 1, 2, 5 and 6): Source Text Module Records and the
//! embedding API of module graphs.
//!
//! A record ([`ModuleRecord`]) is generic heap data: its realm, the code of
//! the module function, the requests and the import and export entries of
//! the front end (names as atoms), `[[LoadedModules]]`, the status of the
//! cyclic module algorithms, the evaluation error, the environment cells,
//! the namespace object and the suspended module function. The host holds
//! [`Module`] handles and keeps them alive (its traced host state); records
//! reach each other through `[[LoadedModules]]`.
//!
//! The parts: `load.rs` (`LoadRequestedModules` with the host hook,
//! §16.2.1.6.1.1), `link.rs` (Link, `ResolveExport`, `GetExportedNames`,
//! `InitializeEnvironment`), `evaluate.rs` (Evaluate without top-level
//! `await`) and `namespace.rs` (module namespace exotic objects, §10.4.6).
//! Every graph algorithm is iterative with explicit stacks (ADR 0034
//! section 6).
//!
//! The module function (`CodeKind::Module`): its prologue puts the
//! environment cells in place (`Insn::ModuleCell`), initializes the `var`
//! bindings and the hoisted functions and suspends (`InitialYield`);
//! `InitializeEnvironment` runs it up to there, Evaluate resumes it. The
//! closure of the module function holds the cells of the import entries
//! that the code uses (its capture list), then the environment cells.

mod evaluate;
mod link;
mod load;
mod namespace;

use std::collections::HashMap;
use std::rc::Rc;

use swb_js_syntax::{BindingKind, Script};
use swb_js_text::{Str16, String16};

pub(crate) use load::ModuleTable;
pub use load::{LoadAnswer, LoadImportedModuleHook, LoadRequest, ModuleHooks, Referrer};
pub(crate) use namespace::{NAMESPACE_HOOKS, NamespaceData};

use crate::bytecode::FunctionCode;
use crate::error::Termination;
use crate::heap::{Gc, Generic, GenericData, Tracer};
use crate::object::Object;
use crate::runtime::{Runtime, ScriptError};
use crate::string::JsString;
use crate::value::{Value, ValueCell};
use crate::vm::{VmError, VmResult};

/// A module record (a Source Text Module Record, §16.2.1.7): a handle to
/// heap data. A handle keeps nothing alive; the host traces the modules
/// that it keeps ([`Module::trace`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Module(pub(crate) Gc<Generic>);

impl Module {
    /// The heap handle (for the host's roots).
    pub fn handle(self) -> Gc<Generic> {
        self.0
    }

    /// Reports the record to a tracer (in the host's `HostState::trace`).
    pub fn trace(self, tracer: &mut Tracer<'_>) {
        tracer.generic(self.0);
    }
}

/// A module request as the host sees it (§16.2.1.3): the specifier and the
/// import attributes, sorted by key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleRequestInfo {
    /// The specifier (lossy for lone surrogates).
    pub specifier: String,
    /// The import attributes (key, value).
    pub attributes: Vec<(String, String)>,
}

/// `[[Status]]` of a cyclic module record (§16.2.1.6, Table 44).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    New,
    Unlinked,
    Linking,
    Linked,
    Evaluating,
    Evaluated,
}

/// A request of the record, with its strings as atoms.
pub(crate) struct Request {
    pub(crate) specifier: Gc<JsString>,
    pub(crate) attributes: Box<[(Gc<JsString>, Gc<JsString>)]>,
}

/// What an import entry imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportName {
    Name(Gc<JsString>),
    Namespace,
}

/// An import entry (Table 61) with the offset of its name.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImportEntry {
    pub(crate) request: u32,
    pub(crate) name: ImportName,
    pub(crate) offset: u32,
}

/// The binding of a local export: a cell of the environment, or the
/// namespace object of the module of an import entry (a re-exported
/// namespace import).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalTarget {
    Cell(u32),
    Namespace(u32),
}

/// What an indirect export entry re-exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndirectName {
    Name(Gc<JsString>),
    /// `export * as ns from "m"`.
    All,
}

/// An indirect export entry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IndirectExport {
    pub(crate) name: Gc<JsString>,
    pub(crate) request: u32,
    pub(crate) import: IndirectName,
    pub(crate) offset: u32,
}

/// An export name of the module itself: a local export (its binding) or
/// an indirect export (its index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportSlot {
    Local(LocalTarget),
    Indirect(u32),
}

/// A binding that `ResolveExport` found (`ResolvedBinding` Record): a module
/// and a cell of its environment, or its namespace object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Cell(u32),
    Namespace,
}

/// The result of `ResolveExport` (§16.2.1.7.2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolved {
    Found(Gc<Generic>, Target),
    Ambiguous,
    NotFound,
}

/// `[[EvaluationError]]`: a thrown value, or the end of a task by a
/// termination (ADR 0034 section 6: the modules never run again and report
/// nothing more).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EvaluationError {
    Thrown(Value),
    Terminated(Termination),
}

/// A Source Text Module Record (the fields of §16.2.1.4 to §16.2.1.7 that
/// swb needs; ADR 0034 section 1).
pub(crate) struct ModuleRecord {
    pub(crate) realm: u32,
    pub(crate) host_defined: u32,
    /// The code object of the module function and its code.
    pub(crate) code: Gc<Generic>,
    pub(crate) compiled: Rc<FunctionCode>,
    pub(crate) requests: Box<[Request]>,
    pub(crate) imports: Box<[ImportEntry]>,
    pub(crate) indirect_exports: Box<[IndirectExport]>,
    /// The requests of the star export entries, with their offsets.
    pub(crate) star_exports: Box<[(u32, u32)]>,
    /// The export names of the module (local and indirect entries).
    pub(crate) exports: HashMap<Gc<JsString>, ExportSlot>,
    /// The names of the local exports, in source order (for
    /// `GetExportedNames`).
    pub(crate) local_names: Box<[Gc<JsString>]>,
    /// `[[HasTLA]]`.
    pub(crate) has_tla: bool,
    /// `[[LoadedModules]]`: one slot per request.
    pub(crate) loaded: Vec<Option<Gc<Generic>>>,
    pub(crate) status: Status,
    pub(crate) dfs_index: u32,
    pub(crate) dfs_ancestor_index: u32,
    /// `[[CycleRoot]]`.
    pub(crate) cycle_root: Option<Gc<Generic>>,
    pub(crate) evaluation_error: Option<EvaluationError>,
    /// The promise of `[[TopLevelCapability]]` (a record that Evaluate was
    /// called on).
    pub(crate) top_level_promise: Option<Gc<Object>>,
    /// The environment: one cell per exported binding, made on first use.
    pub(crate) environment: Vec<Option<Gc<ValueCell>>>,
    /// The namespace object (made on first use) and the cell that holds it
    /// for importers (`import * as ns`, `export * as ns`).
    pub(crate) namespace: Option<Gc<Object>>,
    pub(crate) namespace_cell: Option<Gc<ValueCell>>,
    /// The module function, suspended after its prologue (a generator
    /// object that no script sees).
    pub(crate) body: Option<Gc<Object>>,
    /// The results of `ResolveExport` by export name (ECMA-262 requires the
    /// same answer each time).
    pub(crate) resolved: HashMap<Gc<JsString>, Resolved>,
}

impl GenericData for ModuleRecord {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.generic(self.code);
        for request in &self.requests {
            tracer.string(request.specifier);
            for &(key, value) in &request.attributes {
                tracer.string(key);
                tracer.string(value);
            }
        }
        for entry in &self.imports {
            if let ImportName::Name(name) = entry.name {
                tracer.string(name);
            }
        }
        for entry in &self.indirect_exports {
            tracer.string(entry.name);
            if let IndirectName::Name(name) = entry.import {
                tracer.string(name);
            }
        }
        for &name in self.exports.keys() {
            tracer.string(name);
        }
        for &module in self.loaded.iter().flatten() {
            tracer.generic(module);
        }
        if let Some(root) = self.cycle_root {
            tracer.generic(root);
        }
        if let Some(EvaluationError::Thrown(value)) = self.evaluation_error {
            tracer.value(value);
        }
        for object in [self.top_level_promise, self.namespace, self.body]
            .into_iter()
            .flatten()
        {
            tracer.object(object);
        }
        for &cell in self.environment.iter().flatten() {
            tracer.cell(cell);
        }
        if let Some(cell) = self.namespace_cell {
            tracer.cell(cell);
        }
        for (&name, resolved) in &self.resolved {
            tracer.string(name);
            if let Resolved::Found(module, _) = resolved {
                tracer.generic(*module);
            }
        }
    }

    fn heap_size(&self) -> usize {
        let attributes: usize = self
            .requests
            .iter()
            .map(|r| r.attributes.len() * size_of::<(Gc<JsString>, Gc<JsString>)>())
            .sum();
        self.requests.len() * size_of::<Request>()
            + attributes
            + self.imports.len() * size_of::<ImportEntry>()
            + self.indirect_exports.len() * size_of::<IndirectExport>()
            + self.star_exports.len() * 8
            + self.exports.capacity() * (size_of::<(Gc<JsString>, ExportSlot)>() + 8)
            + self.local_names.len() * size_of::<Gc<JsString>>()
            + self.loaded.capacity() * size_of::<Option<Gc<Generic>>>()
            + self.environment.capacity() * size_of::<Option<Gc<ValueCell>>>()
            + self.resolved.capacity() * (size_of::<(Gc<JsString>, Resolved)>() + 8)
    }
}

impl Runtime {
    /// The record of a module.
    pub(crate) fn record(&self, module: Gc<Generic>) -> VmResult<&ModuleRecord> {
        Ok(self.heap.generic::<ModuleRecord>(module)?)
    }

    /// The record of a module, for writing.
    pub(crate) fn record_mut(&mut self, module: Gc<Generic>) -> VmResult<&mut ModuleRecord> {
        Ok(self.heap.generic_mut::<ModuleRecord>(module)?)
    }

    /// `GetImportedModule(referrer, request)` (§16.2.1.9): the module that
    /// the request at `index` loaded.
    pub(crate) fn imported_module(
        &self,
        referrer: Gc<Generic>,
        index: u32,
    ) -> VmResult<Gc<Generic>> {
        self.record(referrer)?
            .loaded
            .get(index as usize)
            .copied()
            .flatten()
            .ok_or(VmError::invariant("a module request that was not loaded"))
    }

    /// `ParseModule` (§16.2.1.7.1) and the compile of the whole module in
    /// the realm `realm`: a new record with the status `new`. `name` and
    /// `start` are as in [`Runtime::eval_named_at`]; `host_defined` is the
    /// host's number for the module (`[[HostDefined]]`). A syntax error is
    /// [`ScriptError::Compile`], as for scripts; a module with top-level
    /// `await` fails with "not supported yet (top-level await)" (ADR 0034
    /// sub-feature 3a-2). The handle is not rooted: the host stores it in
    /// its traced state before the next run.
    pub fn parse_module(
        &mut self,
        realm: u32,
        source: &str,
        name: &str,
        start: (u32, u32),
        host_defined: u32,
    ) -> Result<Module, ScriptError> {
        let text = String16::from(source);
        let (script, code, compiled) =
            self.parse_and_compile(text.as_str16(), name, start, crate::runtime::Goal::Module)?;
        let record = self
            .module_record(&script, (code, compiled), realm, host_defined)
            .map_err(crate::runtime::finish_error)?;
        let handle = self
            .heap
            .alloc_generic(record)
            .map_err(|e| crate::runtime::finish_error(e.into()))?;
        Ok(Module(handle))
    }

    /// Builds the record of a parsed and compiled module. Nothing here
    /// collects (atoms and generic data are allocated without a
    /// reservation), so the unrooted code object stays valid.
    fn module_record(
        &mut self,
        script: &Script,
        (code, compiled): (Gc<Generic>, Rc<FunctionCode>),
        realm: u32,
        host_defined: u32,
    ) -> VmResult<ModuleRecord> {
        let Some(parsed) = &script.module else {
            return Err(VmError::invariant("a module without its records"));
        };
        let requests = self.record_requests(script, parsed)?;
        let mut imports = Vec::with_capacity(parsed.import_entries.len());
        for entry in &parsed.import_entries {
            let name = match entry.import_name {
                swb_js_syntax::ImportName::Name(name) => {
                    ImportName::Name(name_atom(self, script, name)?)
                }
                swb_js_syntax::ImportName::Namespace => ImportName::Namespace,
            };
            imports.push(ImportEntry {
                request: entry.request,
                name,
                offset: entry.offset,
            });
        }
        let environment = crate::compiler::module_environment(script);
        let mut exports = HashMap::new();
        let local_names = self.record_local_exports(script, &environment, &mut exports)?;
        let indirect_exports = self.record_indirect_exports(script, &mut exports)?;
        let star_exports: Vec<(u32, u32)> = parsed
            .star_exports
            .iter()
            .filter_map(|entry| entry.request.map(|request| (request, entry.offset)))
            .collect();
        Ok(ModuleRecord {
            realm,
            host_defined,
            code,
            compiled,
            loaded: vec![None; requests.len()],
            requests: requests.into_boxed_slice(),
            imports: imports.into_boxed_slice(),
            indirect_exports: indirect_exports.into_boxed_slice(),
            star_exports: star_exports.into_boxed_slice(),
            exports,
            local_names: local_names.into_boxed_slice(),
            has_tla: parsed.has_top_level_await,
            status: Status::New,
            dfs_index: 0,
            dfs_ancestor_index: 0,
            cycle_root: None,
            evaluation_error: None,
            top_level_promise: None,
            environment: vec![None; environment.len()],
            namespace: None,
            namespace_cell: None,
            body: None,
            resolved: HashMap::new(),
        })
    }

    /// The requests of a parsed module with their strings as atoms.
    fn record_requests(
        &mut self,
        script: &Script,
        parsed: &swb_js_syntax::ModuleRecord,
    ) -> VmResult<Vec<Request>> {
        let mut atom =
            |text: &String16| -> VmResult<Gc<JsString>> { Ok(self.heap.intern(text.as_str16())?) };
        let mut requests = Vec::with_capacity(parsed.requests.len());
        for request in &parsed.requests {
            let specifier = atom(script.ast.string(request.specifier))?;
            let mut attributes = Vec::with_capacity(request.attributes.len());
            for attribute in &request.attributes {
                let key = atom(script.ast.string(attribute.key))?;
                let value = atom(script.ast.string(attribute.value))?;
                attributes.push((key, value));
            }
            requests.push(Request {
                specifier,
                attributes: attributes.into_boxed_slice(),
            });
        }
        Ok(requests)
    }

    /// The local export entries: each export name with its binding (a
    /// cell of `environment`, or the namespace of an import entry) into
    /// `exports`; returns the names in source order.
    fn record_local_exports(
        &mut self,
        script: &Script,
        environment: &[swb_js_syntax::BindingId],
        exports: &mut HashMap<Gc<JsString>, ExportSlot>,
    ) -> VmResult<Vec<Gc<JsString>>> {
        let Some(parsed) = &script.module else {
            return Ok(Vec::new());
        };
        let scope = script.ast.function(script.top).scope;
        let by_name: HashMap<swb_js_syntax::NameId, swb_js_syntax::BindingId> = script
            .scopes
            .bindings_of(scope)
            .iter()
            .map(|&binding| (script.scopes.binding(binding).name, binding))
            .collect();
        let cell_of: HashMap<swb_js_syntax::BindingId, u32> = environment
            .iter()
            .enumerate()
            .map(|(index, &binding)| (binding, index as u32))
            .collect();
        let mut local_names = Vec::with_capacity(parsed.local_exports.len());
        for entry in &parsed.local_exports {
            let (Some(export_name), Some(local)) = (entry.export_name, entry.local_name) else {
                return Err(VmError::invariant("a local export without names"));
            };
            let binding = *by_name
                .get(&local)
                .ok_or(VmError::invariant("a local export without a binding"))?;
            let target = if script.scopes.binding(binding).kind == BindingKind::Import {
                let index = parsed
                    .import_entries
                    .iter()
                    .position(|import| import.binding == binding)
                    .ok_or(VmError::invariant("an exported import without its entry"))?;
                LocalTarget::Namespace(index as u32)
            } else {
                LocalTarget::Cell(
                    *cell_of
                        .get(&binding)
                        .ok_or(VmError::invariant("an exported binding without a cell"))?,
                )
            };
            let name = name_atom(self, script, export_name)?;
            exports.insert(name, ExportSlot::Local(target));
            local_names.push(name);
        }
        Ok(local_names)
    }

    /// The indirect export entries, with their names into `exports`.
    fn record_indirect_exports(
        &mut self,
        script: &Script,
        exports: &mut HashMap<Gc<JsString>, ExportSlot>,
    ) -> VmResult<Vec<IndirectExport>> {
        let Some(parsed) = &script.module else {
            return Ok(Vec::new());
        };
        let mut indirect_exports = Vec::with_capacity(parsed.indirect_exports.len());
        for entry in &parsed.indirect_exports {
            let (Some(export_name), Some(request)) = (entry.export_name, entry.request) else {
                return Err(VmError::invariant("an indirect export without a name"));
            };
            let import = match entry.import_name {
                Some(swb_js_syntax::ExportImportName::Name(name)) => {
                    IndirectName::Name(name_atom(self, script, name)?)
                }
                Some(swb_js_syntax::ExportImportName::All) => IndirectName::All,
                _ => return Err(VmError::invariant("an indirect export without an import")),
            };
            let name = name_atom(self, script, export_name)?;
            exports.insert(name, ExportSlot::Indirect(indirect_exports.len() as u32));
            indirect_exports.push(IndirectExport {
                name,
                request,
                import,
                offset: entry.offset,
            });
        }
        Ok(indirect_exports)
    }

    /// The requests of a module in source order (`[[RequestedModules]]`).
    pub fn module_requests(&self, module: Module) -> Vec<ModuleRequestInfo> {
        let Ok(record) = self.record(module.0) else {
            return Vec::new();
        };
        let text = |string| self.heap.string_lossy(string, usize::MAX);
        record
            .requests
            .iter()
            .map(|request| ModuleRequestInfo {
                specifier: text(request.specifier),
                attributes: request
                    .attributes
                    .iter()
                    .map(|&(key, value)| (text(key), text(value)))
                    .collect(),
            })
            .collect()
    }

    /// The host's number of a module (`[[HostDefined]]`).
    pub fn module_host_defined(&self, module: Module) -> Option<u32> {
        self.record(module.0).ok().map(|record| record.host_defined)
    }

    /// The realm of a module.
    pub fn module_realm(&self, module: Module) -> Option<u32> {
        self.record(module.0).ok().map(|record| record.realm)
    }

    /// `GetModuleNamespace(module)` (§16.2.1.12) of a linked module; the
    /// object is made on first use. The value is recorded in the current
    /// handle scope.
    pub fn module_namespace(&mut self, module: Module) -> Result<Value, ScriptError> {
        let status = self
            .record(module.0)
            .map_err(crate::runtime::finish_error)?
            .status;
        if matches!(status, Status::New | Status::Unlinked) {
            return Err(crate::runtime::finish_error(VmError::invariant(
                "the namespace of a module that is not linked",
            )));
        }
        self.namespace_object(module.0)
            .map(Value::Object)
            .map_err(crate::runtime::finish_error)
    }

    /// Runs a module that requests no other module through the module
    /// path (ADR 0034 sub-feature 3a-1): parse, link (which runs the
    /// prologue), evaluate. Errors as [`Runtime::eval_named_at`]; a module
    /// with requests is refused ("not supported yet (module syntax)").
    pub(crate) fn run_module_without_requests(
        &mut self,
        realm: u32,
        source: Str16<'_>,
        name: &str,
        start: (u32, u32),
    ) -> Result<Value, ScriptError> {
        let (script, code, compiled) =
            self.parse_and_compile(source, name, start, crate::runtime::Goal::Module)?;
        if let Some(offset) = first_request_offset(&script) {
            return Err(ScriptError::Compile {
                kind: crate::error::ThrowKind::SyntaxError,
                message: format!("{} (module syntax)", swb_js_syntax::messages::NOT_SUPPORTED),
                offset,
            });
        }
        let record = self
            .module_record(&script, (code, compiled), realm, 0)
            .map_err(crate::runtime::finish_error)?;
        let handle = self
            .heap
            .alloc_generic(record)
            .map_err(|e| crate::runtime::finish_error(e.into()))?;
        // The record is rooted by the handle scope of the run.
        let scope = self.heap.open_scope();
        self.heap.record(crate::heap::Root::Generic(handle));
        let result = self.run_lone_module(handle);
        let result = self.finish(result);
        if let Err(error) = self.heap.close_scope(scope) {
            return Err(crate::runtime::finish_error(error.into()));
        }
        result
    }

    /// Links and evaluates a module without requests; errors go to the
    /// host as the errors of a script run.
    fn run_lone_module(&mut self, module: Gc<Generic>) -> VmResult<Value> {
        self.record_mut(module)?.status = Status::Unlinked;
        self.link_module(module)?;
        self.evaluate_sync(module)?;
        Ok(Value::Undefined)
    }
}

/// The atom of a name of the script.
fn name_atom(
    rt: &mut Runtime,
    script: &Script,
    name: swb_js_syntax::NameId,
) -> VmResult<Gc<JsString>> {
    let text = script
        .names
        .get(name)
        .ok_or(VmError::invariant("a module name outside the interner"))?;
    Ok(rt.heap.intern(text)?)
}

/// The offset of the first statement of a module that requests another
/// module (an import, or an export with `from`), if any.
fn first_request_offset(script: &Script) -> Option<u32> {
    if script.module.as_ref().is_none_or(|m| m.requests.is_empty()) {
        return None;
    }
    script
        .ast
        .stmt_ids()
        .map(|id| script.ast.stmt(id))
        .filter(|stmt| {
            matches!(
                stmt.kind,
                swb_js_syntax::StmtKind::Import | swb_js_syntax::StmtKind::ExportList
            )
        })
        .map(|stmt| stmt.span.start)
        .min()
        .or(Some(0))
}

/// A loader for the unit tests of the module files: sources by name, the
/// answers now.
#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::HashMap;

    use super::{LoadAnswer, LoadRequest, Module, ModuleHooks};
    use crate::heap::{HeapConfig, Tracer};
    use crate::runtime::{Runtime, RuntimeConfig};
    use crate::value::Value;

    /// The sources and the modules made from them.
    #[derive(Default)]
    pub(crate) struct Sources {
        pub(crate) texts: HashMap<String, String>,
        pub(crate) modules: HashMap<String, Module>,
    }

    impl crate::host::HostState for Sources {
        fn trace(&self, tracer: &mut Tracer<'_>) {
            for module in self.modules.values() {
                module.trace(tracer);
            }
        }
    }

    /// The module of a name (made on first use).
    pub(crate) fn module(rt: &mut Runtime, name: &str) -> Result<Module, Value> {
        let sources = rt.host_data_mut::<Sources>().expect("the test host");
        if let Some(&module) = sources.modules.get(name) {
            return Ok(module);
        }
        let Some(text) = sources.texts.get(name).cloned() else {
            return Err(Value::Undefined);
        };
        let number = sources.modules.len() as u32;
        let module = rt
            .parse_module(0, &text, name, (0, 0), number)
            .map_err(|_| Value::Null)?;
        rt.host_data_mut::<Sources>()
            .expect("the test host")
            .modules
            .insert(name.to_owned(), module);
        Ok(module)
    }

    #[allow(clippy::needless_pass_by_value, reason = "the signature of the hook")]
    fn load(rt: &mut Runtime, request: LoadRequest) -> LoadAnswer {
        let name = request.specifier.trim_start_matches("./").to_owned();
        LoadAnswer::Now(module(rt, &name))
    }

    /// A runtime (the stress mode with `stress`) with these sources.
    pub(crate) fn runtime(sources: &[(&str, &str)], stress: bool) -> Runtime {
        let config = RuntimeConfig {
            heap: HeapConfig {
                stress,
                ..HeapConfig::default()
            },
            ..RuntimeConfig::default()
        };
        let mut rt = Runtime::new(config).expect("a runtime");
        rt.set_host_data(Sources {
            texts: sources
                .iter()
                .map(|&(name, text)| (name.to_owned(), text.to_owned()))
                .collect(),
            modules: HashMap::new(),
        });
        rt.set_module_hooks(ModuleHooks {
            load_imported_module: load,
        });
        rt
    }

    /// Loads and links the graph of `root`.
    pub(crate) fn linked(rt: &mut Runtime, root: &str) -> Module {
        let module = module(rt, root).expect("the root parses");
        rt.load_requested_modules(module).expect("the load runs");
        rt.link(module).expect("the graph links");
        module
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{linked, module, runtime};
    use super::*;

    #[test]
    fn records_keep_the_requests_and_entries() {
        let mut rt = runtime(&[], true);
        let module = rt
            .parse_module(
                0,
                "import d, { a as b } from './x.js'; import './y.js' with { type: 'css' };\n\
                 export { b as c } from './x.js'; export * from './z.js'; export let l = 1, m = 2;\n\
                 export { l as l2 }; export default 3;",
                "m.js",
                (0, 0),
                7,
            )
            .expect("parses");
        assert_eq!(rt.module_host_defined(module), Some(7));
        assert_eq!(rt.module_realm(module), Some(0));
        let requests = rt.module_requests(module);
        let specifiers: Vec<&str> = requests.iter().map(|r| r.specifier.as_str()).collect();
        assert_eq!(specifiers, ["./x.js", "./y.js", "./z.js"]);
        assert_eq!(
            requests[1].attributes,
            [("type".to_owned(), "css".to_owned())]
        );
        let record = rt.record(module.0).expect("a record");
        assert_eq!(record.status, Status::New);
        assert_eq!(record.imports.len(), 2);
        assert_eq!(record.indirect_exports.len(), 1);
        assert_eq!(record.star_exports.len(), 1);
        // `l` and `l2` share a cell; `m` and `*default*` have their own.
        assert_eq!(record.environment.len(), 3);
        let mut slot = |name: &str| {
            let atom = rt.heap.intern_str(name).expect("an atom");
            rt.record(module.0)
                .expect("a record")
                .exports
                .get(&atom)
                .copied()
        };
        assert_eq!(slot("l"), slot("l2"));
        assert_eq!(slot("c"), Some(ExportSlot::Indirect(0)));
        assert_eq!(slot("nope"), None);
    }

    #[test]
    fn a_module_without_requests_runs_through_the_records() {
        let mut rt = runtime(&[], true);
        assert!(
            rt.eval_module_at(0, "export const a = 1;", "m.js", (0, 0))
                .is_ok()
        );
        let error = rt
            .eval_module_at(0, "let x;\nimport './a.js';", "m.js", (0, 0))
            .expect_err("refused");
        assert!(error.is_unsupported());
        assert!(
            matches!(error, ScriptError::Compile { offset: 7, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn namespaces_need_a_linked_module() {
        let mut rt = runtime(&[("a.js", "export const x = 1;")], true);
        let a = module(&mut rt, "a.js").expect("parses");
        assert!(rt.module_namespace(a).is_err());
        let a = linked(&mut rt, "a.js");
        let namespace = rt.module_namespace(a).expect("a namespace");
        assert_eq!(rt.module_namespace(a).expect("the same"), namespace);
    }
}
