//! Linking (§16.2.1.6.1.2 `Link`, <https://tc39.es/ecma262/2025/#sec-moduledeclarationlinking>;
//! ADR 0034 sections 1, 2 and 6): `InnerModuleLinking` (Tarjan's DFS over
//! the requests, a loop with an explicit stack), `ResolveExport`
//! (§16.2.1.7.2.2: indirect and star exports, ambiguity; iterative, its
//! `resolveSet` a hash map; memoized per module and name),
//! `GetExportedNames` (§16.2.1.7.2.1) and `InitializeEnvironment`
//! (§16.2.1.7.3.1).
//!
//! `InitializeEnvironment` checks the indirect exports, resolves each import
//! entry to the cell of the exporting binding (a cell of the exporter's
//! environment, made on first use with `Empty`, or the cell that holds a
//! namespace object), makes the module's closure with those cells and the
//! environment cells as captures, and runs the module function up to its
//! initial suspension: the prologue initializes the `var` bindings and the
//! hoisted functions.
//!
//! A link error is a `SyntaxError` with Chromium's message, whose location
//! ([`Runtime::error_location`]) is the name of the entry in the importing
//! module (measured: `x.js:1:9` for `zz` in `import {zz} from`).
//!
//! The memo of `ResolveExport`: a result is stored when no circularity cut
//! in its computation hit an entry of the `resolveSet` that existed before
//! the call; such a result does not depend on the set and equals the
//! answer of a call with an empty set.

use std::collections::{HashMap, HashSet};

use swb_js_syntax::CaptureSource;

use crate::error::ThrowKind;
use crate::heap::{Gc, Generic, Root};
use crate::object::ObjectKind;
use crate::runtime::{Runtime, ScriptError};
use crate::string::JsString;
use crate::value::{Value, ValueCell};
use crate::vm::{ErrorData, Frame, ReturnTo, STACK_OVERFLOW, StackFrame, VmError, VmResult};

use super::{ExportSlot, ImportName, IndirectName, LocalTarget, Module, Resolved, Status, Target};

/// A module whose requests the linking loop visits.
struct Visit {
    module: Gc<Generic>,
    next: u32,
}

/// Where a call of `ResolveExport` is.
#[derive(Clone, Copy)]
enum Phase {
    /// Look at the module's own export names.
    Start,
    /// The result of the indirect export's call is the result.
    Tail,
    /// The next star export to call (the result of the one before is
    /// delivered).
    Star(u32),
}

/// A call of `ResolveExport` on the explicit stack.
struct ResolveCall {
    module: Gc<Generic>,
    name: Gc<JsString>,
    /// The length of the resolve set before the call's own entry.
    set_start: usize,
    /// The lowest position in the resolve set that a circularity cut in
    /// the call (or the calls below it) hit.
    lowest_hit: usize,
    phase: Phase,
    star: Option<(Gc<Generic>, Target)>,
}

impl ResolveCall {
    fn new(module: Gc<Generic>, name: Gc<JsString>, set_start: usize) -> Self {
        ResolveCall {
            module,
            name,
            set_start,
            lowest_hit: usize::MAX,
            phase: Phase::Start,
            star: None,
        }
    }
}

/// What one step of a call does next.
enum Step {
    Call(Gc<Generic>, Gc<JsString>),
    Finish(Resolved),
}

/// The kind of a link error, for its message.
#[derive(Clone, Copy)]
enum LinkFailure {
    Missing,
    Ambiguous,
}

impl Runtime {
    /// `Link()` (§16.2.1.6.1.2) of a loaded module (the promise of
    /// [`Runtime::load_requested_modules`] is fulfilled): resolves the
    /// imports of the graph and runs the prologue of each module function.
    /// A link error is [`ScriptError::Uncaught`] with the `SyntaxError` as
    /// [`Runtime::last_value`]; its location is the imported name
    /// ([`Runtime::error_location`]). The modules of a failed link return
    /// to `unlinked`.
    pub fn link(&mut self, module: Module) -> Result<(), ScriptError> {
        self.vm.error_offset = None;
        let start = self.run_start();
        let scope = self.heap.open_scope();
        self.heap.record(Root::Generic(module.0));
        let result = self.link_module(module.0).map(|()| Value::Undefined);
        if result.is_err() {
            self.restore_run_start(&start);
        } else if let Err(error) = self.heap.close_scope(scope) {
            return Err(crate::runtime::finish_error(error.into()));
        }
        self.finish(result).map(drop)
    }

    /// Link: `InnerModuleLinking` from `module`; on an error the modules on
    /// the stack return to `unlinked`.
    pub(crate) fn link_module(&mut self, module: Gc<Generic>) -> VmResult<()> {
        let mut stack = Vec::new();
        let result = self.inner_linking(module, &mut stack);
        if result.is_err() {
            for module in stack {
                if let Ok(record) = self.record_mut(module)
                    && record.status == Status::Linking
                {
                    record.status = Status::Unlinked;
                }
            }
        }
        result
    }

    /// `InnerModuleLinking` (§16.2.1.6.1.2.1) as a loop.
    fn inner_linking(&mut self, root: Gc<Generic>, stack: &mut Vec<Gc<Generic>>) -> VmResult<()> {
        let mut index = 0u32;
        let mut visits = Vec::new();
        self.enter_linking(root, &mut index, stack, &mut visits)?;
        while let Some(top) = visits.last_mut() {
            self.tick()?;
            let (module, request) = (top.module, top.next);
            if (request as usize) < self.record(module)?.loaded.len() {
                top.next += 1;
                let required = self.imported_module(module, request)?;
                if !self.enter_linking(required, &mut index, stack, &mut visits)? {
                    self.note_linking_child(module, required)?;
                }
                continue;
            }
            visits.pop();
            self.initialize_environment(module)?;
            let record = self.record(module)?;
            if record.dfs_ancestor_index == record.dfs_index {
                while let Some(member) = stack.pop() {
                    self.record_mut(member)?.status = Status::Linked;
                    if member == module {
                        break;
                    }
                }
            }
            if let Some(parent) = visits.last() {
                let parent = parent.module;
                self.note_linking_child(parent, module)?;
            }
        }
        Ok(())
    }

    /// Steps 2 to 8 of `InnerModuleLinking`: an `unlinked` module starts
    /// (true); a module in another state is done (false).
    fn enter_linking(
        &mut self,
        module: Gc<Generic>,
        index: &mut u32,
        stack: &mut Vec<Gc<Generic>>,
        visits: &mut Vec<Visit>,
    ) -> VmResult<bool> {
        let record = self.record_mut(module)?;
        match record.status {
            Status::Unlinked => {}
            Status::New => return Err(VmError::invariant("a link of a module that is not loaded")),
            _ => return Ok(false),
        }
        record.status = Status::Linking;
        record.dfs_index = *index;
        record.dfs_ancestor_index = *index;
        *index = index.saturating_add(1);
        stack.push(module);
        visits.push(Visit { module, next: 0 });
        Ok(true)
    }

    /// Step 9.c.ii of `InnerModuleLinking`: a required module that is still
    /// `linking` lowers the module's ancestor index.
    fn note_linking_child(&mut self, module: Gc<Generic>, required: Gc<Generic>) -> VmResult<()> {
        let child = self.record(required)?;
        if child.status != Status::Linking {
            return Ok(());
        }
        let ancestor = child.dfs_ancestor_index;
        let record = self.record_mut(module)?;
        record.dfs_ancestor_index = record.dfs_ancestor_index.min(ancestor);
        Ok(())
    }

    /// `InitializeEnvironment()` (§16.2.1.7.3.1).
    fn initialize_environment(&mut self, module: Gc<Generic>) -> VmResult<()> {
        // Step 1: every indirect export resolves.
        let count = self.record(module)?.indirect_exports.len();
        for index in 0..count {
            let entry = *self
                .record(module)?
                .indirect_exports
                .get(index)
                .ok_or(VmError::invariant("an indirect export outside the list"))?;
            match self.resolve_export(module, entry.name)? {
                Resolved::Found(..) => {}
                failed => {
                    let name = match entry.import {
                        IndirectName::Name(name) => name,
                        IndirectName::All => entry.name,
                    };
                    let kind = if failed == Resolved::Ambiguous {
                        LinkFailure::Ambiguous
                    } else {
                        LinkFailure::Missing
                    };
                    return Err(self.link_error(module, entry.request, name, entry.offset, kind));
                }
            }
        }
        // Step 7: the cells of the import entries.
        let count = self.record(module)?.imports.len();
        let mut import_cells = Vec::with_capacity(count);
        for index in 0..count {
            let entry = *self
                .record(module)?
                .imports
                .get(index)
                .ok_or(VmError::invariant("an import entry outside the list"))?;
            let imported = self.imported_module(module, entry.request)?;
            let cell = match entry.name {
                ImportName::Namespace => self.binding_cell(imported, Target::Namespace)?,
                ImportName::Name(name) => match self.resolve_export(imported, name)? {
                    Resolved::Found(owner, target) => self.binding_cell(owner, target)?,
                    failed => {
                        let kind = if failed == Resolved::Ambiguous {
                            LinkFailure::Ambiguous
                        } else {
                            LinkFailure::Missing
                        };
                        return Err(self.link_error(
                            module,
                            entry.request,
                            name,
                            entry.offset,
                            kind,
                        ));
                    }
                },
            };
            import_cells.push(cell);
        }
        let captures = self.module_captures(module, &import_cells)?;
        self.start_module_body(module, captures)
    }

    /// The captures of the module function: the cells of the import
    /// entries that its code lists, then the environment cells.
    fn module_captures(
        &mut self,
        module: Gc<Generic>,
        import_cells: &[Gc<ValueCell>],
    ) -> VmResult<Box<[Gc<ValueCell>]>> {
        let record = self.record(module)?;
        let compiled = std::rc::Rc::clone(&record.compiled);
        let environment = record.environment.len();
        let mut captures = Vec::with_capacity(compiled.captures.len() + environment);
        for source in &compiled.captures {
            let CaptureSource::Import(index) = *source else {
                return Err(VmError::invariant(
                    "a module function that captures a parent",
                ));
            };
            let cell = import_cells
                .get(index as usize)
                .copied()
                .ok_or(VmError::invariant("an import capture outside the entries"))?;
            captures.push(cell);
        }
        for index in 0..environment {
            captures.push(self.binding_cell(module, Target::Cell(index as u32))?);
        }
        Ok(captures.into_boxed_slice())
    }

    /// Makes the module's closure and runs the module function up to its
    /// initial suspension; the record keeps the suspended function.
    fn start_module_body(
        &mut self,
        module: Gc<Generic>,
        captures: Box<[Gc<ValueCell>]>,
    ) -> VmResult<()> {
        let record = self.record(module)?;
        let (code, compiled, realm) = (
            record.code,
            std::rc::Rc::clone(&record.compiled),
            record.realm,
        );
        let scope = self.heap.open_scope();
        let closure = self.new_closure(code, &compiled, captures, realm)?;
        let restore_top = self.vm.stack.len();
        let registers = compiled.register_count as usize;
        if self.vm.frames.len() >= self.vm.frame_limit
            || restore_top + 2 + registers > self.vm.stack_limit
        {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let window = self.push_window(closure.into(), Value::Undefined, &[])?;
        self.vm.stack.resize(window + registers, Value::Undefined);
        self.vm.frames.push(Frame {
            function: Some(closure),
            code,
            compiled,
            pc: 0,
            base: window as u32,
            restore_top: restore_top as u32,
            this: Value::Undefined,
            new_target: Value::Undefined,
            argc: 0,
            extra_args: Vec::new(),
            ret: ReturnTo::Host,
            construct: false,
            generator: None,
            realm,
        });
        self.heap.close_scope(scope)?;
        let body = match self.run_frames()? {
            Value::Object(body)
                if matches!(self.heap.object(body)?.kind, ObjectKind::Generator(_)) =>
            {
                body
            }
            _ => return Err(VmError::invariant("a module function that did not suspend")),
        };
        self.record_mut(module)?.body = Some(body);
        Ok(())
    }

    /// The cell of a binding that `ResolveExport` found: a cell of the
    /// module's environment (made with `Empty`, the dead zone, on first
    /// use), or the cell that holds the module's namespace object.
    pub(crate) fn binding_cell(
        &mut self,
        module: Gc<Generic>,
        target: Target,
    ) -> VmResult<Gc<ValueCell>> {
        match target {
            Target::Cell(index) => {
                let existing = *self
                    .record(module)?
                    .environment
                    .get(index as usize)
                    .ok_or(VmError::invariant("a module cell outside the environment"))?;
                if let Some(cell) = existing {
                    return Ok(cell);
                }
                let cell = self.heap.alloc_cell(Value::Empty)?;
                if let Some(slot) = self.record_mut(module)?.environment.get_mut(index as usize) {
                    *slot = Some(cell);
                }
                Ok(cell)
            }
            Target::Namespace => self.namespace_cell(module),
        }
    }

    /// The `SyntaxError` of a failed import or indirect export of `module`
    /// (Chromium's messages), located at `offset` in the module.
    fn link_error(
        &mut self,
        module: Gc<Generic>,
        request: u32,
        name: Gc<JsString>,
        offset: u32,
        kind: LinkFailure,
    ) -> VmError {
        match self.make_link_error(module, request, name, offset, kind) {
            Ok(error) => VmError::Throw(error),
            Err(error) => error,
        }
    }

    fn make_link_error(
        &mut self,
        module: Gc<Generic>,
        request: u32,
        name: Gc<JsString>,
        offset: u32,
        kind: LinkFailure,
    ) -> VmResult<Value> {
        let record = self.record(module)?;
        let specifier = record
            .requests
            .get(request as usize)
            .map(|request| request.specifier)
            .ok_or(VmError::invariant("a module request outside the list"))?;
        let (code, compiled, realm) = (
            record.code,
            std::rc::Rc::clone(&record.compiled),
            record.realm,
        );
        let specifier = self.heap.string_lossy(specifier, usize::MAX);
        let name = self.heap.string_lossy(name, usize::MAX);
        let message = match kind {
            LinkFailure::Missing => format!(
                "The requested module '{specifier}' does not provide an export named '{name}'"
            ),
            LinkFailure::Ambiguous => format!(
                "The requested module '{specifier}' contains conflicting star exports for name '{name}'"
            ),
        };
        let error = self.in_realm(realm, |rt| {
            rt.error_object(ThrowKind::SyntaxError, &message)
        })?;
        let frame = StackFrame {
            function: None,
            code,
            compiled,
            offset: Some(offset),
            this: Value::Undefined,
            construct: false,
            is_async: false,
        };
        // Chromium's `stack` of a link error is its first line only. The
        // frame stays for `error_location`: the host reports the error at
        // the import.
        self.scoped(|rt| {
            rt.heap.record(error);
            let stack = rt.heap.alloc_str(&format!("SyntaxError: {message}"))?;
            rt.heap.record(stack);
            if let ObjectKind::Error(data) = &mut rt.heap.object_mut(error)?.kind {
                *data = Some(Box::new(ErrorData {
                    frames: Box::new([frame]),
                    stack: Value::String(stack),
                    building: false,
                }));
            }
            Ok(())
        })?;
        Ok(error.into())
    }

    /// Runs `f` with `realm` as the realm of the running code when no frame
    /// runs (the errors of the linker belong to the module's realm).
    pub(crate) fn in_realm<T>(
        &mut self,
        realm: u32,
        f: impl FnOnce(&mut Runtime) -> VmResult<T>,
    ) -> VmResult<T> {
        if !self.vm.frames.is_empty() {
            return f(self);
        }
        let saved = std::mem::replace(&mut self.vm.base_realm, realm);
        let result = f(self);
        self.vm.base_realm = saved;
        result
    }

    /// `module.ResolveExport(exportName)` (§16.2.1.7.2.2) with an empty
    /// resolve set, as a loop over an explicit stack of calls.
    pub(crate) fn resolve_export(
        &mut self,
        module: Gc<Generic>,
        name: Gc<JsString>,
    ) -> VmResult<Resolved> {
        if let Some(&cached) = self.record(module)?.resolved.get(&name) {
            return Ok(cached);
        }
        let mut set: HashMap<(Gc<Generic>, Gc<JsString>), usize> = HashMap::new();
        set.insert((module, name), 0);
        let mut calls = vec![ResolveCall::new(module, name, 0)];
        // The result of the last call that ended, for the call below it.
        let mut delivered: Option<Resolved> = None;
        loop {
            self.tick()?;
            let Some(call) = calls.last_mut() else {
                return Err(VmError::invariant("ResolveExport without a call"));
            };
            let step = match call.phase {
                Phase::Start => self.resolve_own(call)?,
                Phase::Tail => Step::Finish(delivered.take().unwrap_or(Resolved::NotFound)),
                Phase::Star(_) => self.resolve_star(call, delivered.take())?,
            };
            match step {
                Step::Call(callee, callee_name) => {
                    if let Some(&cached) = self.record(callee)?.resolved.get(&callee_name) {
                        delivered = Some(cached);
                    } else if let Some(&position) = set.get(&(callee, callee_name)) {
                        // A circular request (step 1): null.
                        call.lowest_hit = call.lowest_hit.min(position);
                        delivered = Some(Resolved::NotFound);
                    } else {
                        let position = set.len();
                        set.insert((callee, callee_name), position);
                        calls.push(ResolveCall::new(callee, callee_name, position));
                    }
                }
                Step::Finish(result) => {
                    let Some(done) = calls.pop() else {
                        return Err(VmError::invariant("ResolveExport without a call"));
                    };
                    if done.lowest_hit >= done.set_start {
                        self.heap
                            .generic_resized::<super::ModuleRecord, _>(done.module, |record| {
                                record.resolved.insert(done.name, result)
                            })?;
                    }
                    match calls.last_mut() {
                        Some(parent) => {
                            parent.lowest_hit = parent.lowest_hit.min(done.lowest_hit);
                            delivered = Some(result);
                        }
                        None => return Ok(result),
                    }
                }
            }
        }
    }

    /// Steps 4 to 6 of `ResolveExport`: the module's own export names (a
    /// local export, or an indirect export, whose result is that of its
    /// call); without one, the star exports follow (not for `default`).
    fn resolve_own(&self, call: &mut ResolveCall) -> VmResult<Step> {
        let record = self.record(call.module)?;
        Ok(match record.exports.get(&call.name).copied() {
            Some(ExportSlot::Local(LocalTarget::Cell(index))) => {
                Step::Finish(Resolved::Found(call.module, Target::Cell(index)))
            }
            Some(ExportSlot::Local(LocalTarget::Namespace(import))) => {
                let request = record
                    .imports
                    .get(import as usize)
                    .map(|entry| entry.request)
                    .ok_or(VmError::invariant("an export of a missing import"))?;
                let imported = self.imported_module(call.module, request)?;
                Step::Finish(Resolved::Found(imported, Target::Namespace))
            }
            Some(ExportSlot::Indirect(index)) => {
                let entry = *record
                    .indirect_exports
                    .get(index as usize)
                    .ok_or(VmError::invariant("an indirect export outside the list"))?;
                let imported = self.imported_module(call.module, entry.request)?;
                match entry.import {
                    IndirectName::All => Step::Finish(Resolved::Found(imported, Target::Namespace)),
                    IndirectName::Name(import) => {
                        call.phase = Phase::Tail;
                        Step::Call(imported, import)
                    }
                }
            }
            // A star export never provides `default` (step 6).
            None if call.name == self.vm.atoms.default => Step::Finish(Resolved::NotFound),
            None => self.resolve_star(call, None)?,
        })
    }

    /// Steps 7 and 8 of `ResolveExport`: the result of the last star export
    /// (an ambiguity ends the call; two different bindings are ambiguous),
    /// then the call of the next one, or the end.
    fn resolve_star(&self, call: &mut ResolveCall, delivered: Option<Resolved>) -> VmResult<Step> {
        match delivered {
            Some(Resolved::Ambiguous) => return Ok(Step::Finish(Resolved::Ambiguous)),
            Some(Resolved::Found(owner, target)) => match call.star {
                None => call.star = Some((owner, target)),
                Some(star) if star == (owner, target) => {}
                Some(_) => return Ok(Step::Finish(Resolved::Ambiguous)),
            },
            Some(Resolved::NotFound) | None => {}
        }
        let next = match call.phase {
            Phase::Star(next) => next,
            _ => 0,
        };
        let record = self.record(call.module)?;
        Ok(match record.star_exports.get(next as usize) {
            Some(&(request, _)) => {
                call.phase = Phase::Star(next + 1);
                Step::Call(self.imported_module(call.module, request)?, call.name)
            }
            None => Step::Finish(match call.star {
                Some((owner, target)) => Resolved::Found(owner, target),
                None => Resolved::NotFound,
            }),
        })
    }

    /// `module.GetExportedNames()` (§16.2.1.7.2.1): the module's own export
    /// names and those of the modules that its star exports reach, without
    /// `default` from those, each once (the order does not matter: the
    /// namespace sorts them).
    pub(crate) fn exported_names(&mut self, module: Gc<Generic>) -> VmResult<Vec<Gc<JsString>>> {
        let default = self.vm.atoms.default;
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        let mut visited = HashSet::new();
        let mut work = vec![(module, true)];
        while let Some((current, root)) = work.pop() {
            self.tick()?;
            if !visited.insert(current) {
                continue;
            }
            let record = self.record(current)?;
            let own = record
                .local_names
                .iter()
                .chain(record.indirect_exports.iter().map(|entry| &entry.name));
            for &name in own {
                if (root || name != default) && seen.insert(name) {
                    names.push(name);
                }
            }
            let stars: Vec<u32> = record
                .star_exports
                .iter()
                .map(|&(request, _)| request)
                .collect();
            self.charge(names.len() / 64)?;
            for request in stars.into_iter().rev() {
                work.push((self.imported_module(current, request)?, false));
            }
        }
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{linked, module, runtime};
    use super::*;

    fn atom(rt: &mut Runtime, name: &str) -> Gc<JsString> {
        rt.heap.intern_str(name).expect("an atom")
    }

    #[test]
    fn resolve_export_follows_indirect_and_star_exports() {
        let mut rt = runtime(
            &[
                (
                    "a.js",
                    "export { x as y } from './b.js'; export * from './c.js'; export * from './d.js';",
                ),
                ("b.js", "export const x = 1;"),
                (
                    "c.js",
                    "export const z = 2; export const both = 3; export default 4;",
                ),
                ("d.js", "export const both = 5;"),
            ],
            true,
        );
        let a = linked(&mut rt, "a.js");
        let b = module(&mut rt, "b.js").expect("loaded");
        let c = module(&mut rt, "c.js").expect("loaded");
        let names = [
            ("y", Resolved::Found(b.0, Target::Cell(0))),
            ("z", Resolved::Found(c.0, Target::Cell(0))),
        ];
        for (name, expected) in names {
            let name = atom(&mut rt, name);
            assert_eq!(rt.resolve_export(a.0, name).expect("resolves"), expected);
        }
        let both = atom(&mut rt, "both");
        assert_eq!(
            rt.resolve_export(a.0, both).expect("resolves"),
            Resolved::Ambiguous
        );
        // A star export never gives `default`.
        let default = atom(&mut rt, "default");
        assert_eq!(
            rt.resolve_export(a.0, default).expect("resolves"),
            Resolved::NotFound
        );
        // Every answer is in the memo.
        assert_eq!(
            rt.record(a.0).expect("a record").resolved.get(&both),
            Some(&Resolved::Ambiguous)
        );
        let mut names = rt.exported_names(a.0).expect("names");
        names.sort_by_key(|name| rt.heap.string_lossy(*name, usize::MAX));
        let texts: Vec<String> = names
            .iter()
            .map(|&n| rt.heap.string_lossy(n, usize::MAX))
            .collect();
        assert_eq!(texts, ["both", "y", "z"]);
    }

    #[test]
    fn answers_cut_by_a_cycle_outside_the_call_are_not_kept() {
        // a: export * from b; b: export * from a. Resolving `n` in `a`
        // reaches `a` again from `b`: the answer for `b` depends on the
        // set and is not kept; the answer for `a` is.
        let mut rt = runtime(
            &[
                ("a.js", "export * from './b.js'; export const n = 1;"),
                ("b.js", "export * from './a.js';"),
            ],
            false,
        );
        let a = linked(&mut rt, "a.js");
        let b = module(&mut rt, "b.js").expect("loaded");
        let n = atom(&mut rt, "n");
        let nope = atom(&mut rt, "nope");
        assert_eq!(
            rt.resolve_export(a.0, nope).expect("resolves"),
            Resolved::NotFound
        );
        assert!(
            !rt.record(b.0)
                .expect("a record")
                .resolved
                .contains_key(&nope)
        );
        // From `b` the name resolves through `a`.
        assert_eq!(
            rt.resolve_export(b.0, n).expect("resolves"),
            Resolved::Found(a.0, Target::Cell(0))
        );
    }

    #[test]
    fn a_link_error_is_located_at_the_imported_name() {
        let mut rt = runtime(
            &[
                ("x.js", "\n  import { a, zz } from './d.js';"),
                ("d.js", "export const a = 1;"),
            ],
            true,
        );
        let x = module(&mut rt, "x.js").expect("parses");
        rt.load_requested_modules(x).expect("loads");
        let error = rt.link(x).expect_err("a link error");
        assert!(matches!(error, ScriptError::Uncaught { ref name, .. } if name == "SyntaxError"));
        let location = rt.error_location(rt.last_value()).expect("a location");
        assert_eq!(
            (location.name.as_str(), location.line, location.column),
            ("x.js", 2, 15)
        );
        assert_eq!(rt.record(x.0).expect("a record").status, Status::Unlinked);
        // `d` linked before the error: it stays linked.
        let d = module(&mut rt, "d.js").expect("loaded");
        assert_eq!(rt.record(d.0).expect("a record").status, Status::Linked);
    }

    #[test]
    fn linking_runs_the_prologue_and_suspends() {
        let mut rt = runtime(
            &[(
                "m.js",
                "export var v; export function f() {} export let l = 1; globalThis.ran = true;",
            )],
            true,
        );
        let m = linked(&mut rt, "m.js");
        let record = rt.record(m.0).expect("a record");
        assert!(record.body.is_some());
        let cells: Vec<_> = record
            .environment
            .iter()
            .map(|c| c.expect("made"))
            .collect();
        let values: Vec<Value> = cells
            .iter()
            .map(|&c| rt.heap.cell(c).expect("a cell").value)
            .collect();
        assert_eq!(values[0], Value::Undefined);
        assert!(matches!(values[1], Value::Object(_)));
        assert_eq!(values[2], Value::Empty);
        // The body has not run.
        assert_eq!(
            rt.eval("typeof ran").expect("runs"),
            Value::String(rt.vm.atoms.undefined)
        );
    }
}
