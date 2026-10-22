//! Eval code (ECMA-262 §19.2.1, ADR 0026 "Dynamic scope"): the scope
//! descriptions of the functions that call `eval`, and the access of eval
//! code to the bindings of its caller.
//!
//! A function with a direct `eval` keeps a scope description
//! ([`crate::bytecode::EvalInfo`]): its scopes with their bindings (all
//! cells, see the scope analysis), the names of enclosing functions that
//! it captured for eval code (`ScopeTree::eval_outer_references`), and its
//! calls of `eval` with the scope of each call and its variable
//! environment. At a call, the VM looks up the names that the eval code
//! uses ([`EvalCaller`], `vm/eval.rs`): for each, the cell (a register of
//! the caller's frame or one of its captures), the binding kind and the
//! number of dynamic records before it.
//!
//! Eval code reaches these cells as captures: its top-level function
//! captures the cells of the caller's frame that the code uses, and each
//! function inside captures them from its parent (flat closures), in
//! addition to the captures of the analysis ([`CallerPlan`]). A name that
//! the caller does not have is global, accessed by name. Dynamic
//! references check the records of the eval code first and then those of
//! the caller: the counts add up, and the outermost record of the eval
//! code links to the innermost record of the caller.
//!
//! Eval code that calls `eval` itself captures every name of its caller,
//! so that its own description can hand them on.

use std::collections::{HashMap, HashSet};

use swb_js_syntax::{
    BindingId, BindingKind, CaptureSource, FunctionId, FunctionKind, Ident, RefId, Resolution,
    ScopeId, ScopeKind, Script, Storage, names,
};
use swb_js_text::{Str16, String16};

use super::dynamic::{Dynamic, EnvSource, count_operand};
use super::function::FunctionCompiler;
use super::{CResult, Session};
use crate::bytecode::{
    EvalFlags, EvalInfo, EvalName, EvalScope, EvalSite, EvalSlot, EvalVarEnv, EvalWalk, Insn, Reg,
};
use crate::error::Error;
use crate::heap::Heap;

/// What the VM found for eval code in the scope description of its
/// caller (direct eval only).
#[derive(Debug, Default)]
pub(crate) struct EvalCaller {
    /// The names that the caller has, as the description gives them
    /// (slots relative to the caller's frame).
    pub(crate) entries: Vec<EvalName>,
    /// The entry of each name by its text.
    pub(crate) by_name: HashMap<String16, u32>,
    /// The innermost dynamic record at the call (the cell of its binding).
    pub(crate) record: Option<EvalSlot>,
    /// The variable environment of the call.
    pub(crate) var_env: EvalVarEnv,
    /// The context of the call.
    pub(crate) flags: EvalFlags,
}

/// The input of the compile of eval code.
#[derive(Debug, Default)]
pub(crate) struct EvalInput {
    /// The caller of direct eval code.
    pub(crate) caller: Option<EvalCaller>,
    /// The block function declarations that Annex B.3.2.3 does not
    /// hoist (their var binding was not created).
    pub(crate) skip_block_functions: HashSet<BindingId>,
}

/// The most captures of the caller's cells that the functions of one
/// piece of eval code may get in all: the total capture limit of a
/// script in `js-syntax` (2^20, about 60 MB of plan at most). A cell
/// that a function uses is also captured by every function around it,
/// so the plan can grow with depth x names; past this limit (or past
/// [`MAX_FUNCTION_CAPTURES`] in one function) the compile ends with the
/// parser's `RangeError`. V8 has no such limit.
pub(crate) const MAX_HANDED_CAPTURES: usize = 1 << 20;

/// The most captures of one function (the operand width).
pub(crate) const MAX_FUNCTION_CAPTURES: usize = 65_535;

/// The caller's cells that each function of eval code captures beyond
/// the captures of the analysis.
#[derive(Debug, Default)]
pub(crate) struct CallerPlan {
    pub(crate) input: EvalInput,
    /// Per function (by index): the caller's cells, in capture order.
    extra: Vec<Vec<EvalSlot>>,
    /// The capture index of a caller's cell in a function.
    index: HashMap<(u32, EvalSlot), u32>,
    /// The number of entries in `extra`.
    total: usize,
    /// Whether the plan stopped at a capture limit.
    pub(crate) overflow: bool,
}

/// Whether a name can be the binding of a dynamic record: identifiers;
/// not the implicit names and private names (see `js-syntax`
/// `scope/dynamic.rs`).
pub(crate) fn is_dynamic_name(name: Str16<'_>) -> bool {
    let text = name.to_string_lossy();
    !(text.starts_with('#')
        || matches!(
            text.as_str(),
            "this" | "new.target" | "super" | "%function" | "%env" | "%private"
        ))
}

/// The text of a name of a script.
fn name_text(script: &Script, name: swb_js_syntax::NameId) -> String16 {
    script
        .names
        .get(name)
        .map(Str16::to_string16)
        .unwrap_or_default()
}

impl CallerPlan {
    /// The plan of the captures for eval code (§19.2.1.1 steps 13 to 15:
    /// the code runs in the caller's environments).
    pub(crate) fn new(script: &Script, input: EvalInput) -> CallerPlan {
        let mut plan = CallerPlan {
            input,
            extra: vec![Vec::new(); script.ast.function_count()],
            index: HashMap::new(),
            total: 0,
            overflow: false,
        };
        let Some(caller) = &plan.input.caller else {
            return plan;
        };
        let tree = &script.scopes;
        let mut wanted: Vec<(FunctionId, EvalSlot)> = Vec::new();
        let caller_record = caller.record;
        for id in tree.reference_ids() {
            let reference = tree.reference(id);
            if reference.dead || reference.resolution != Resolution::Caller {
                continue;
            }
            let function = tree.scope(reference.scope).function;
            let text = name_text(script, reference.name);
            let entry = caller
                .by_name
                .get(&text)
                .and_then(|&i| caller.entries.get(i as usize));
            if let Some(entry) = entry {
                wanted.push((function, entry.slot));
            }
            if is_dynamic_name(text.as_str16()) {
                let inner = tree.dynamic_lookup(id).map_or(0, |l| l.count);
                let outer = entry.map_or(u32::MAX, |e| e.count);
                if inner == 0
                    && outer > 0
                    && let Some(record) = caller_record
                {
                    wanted.push((function, record));
                }
            }
        }
        // The function declarations of sloppy eval code go into the
        // caller's variable environment.
        let top = script.top;
        let top_scope = script.ast.function(top).scope;
        for &binding in tree.bindings_of(top_scope) {
            let b = tree.binding(binding);
            if b.storage != Storage::Caller {
                continue;
            }
            let text = name_text(script, b.name);
            let var_slot = caller
                .by_name
                .get(&text)
                .and_then(|&i| caller.entries.get(i as usize))
                .and_then(|e| e.walk.var_slot);
            if let (Some(slot), _) | (None, EvalVarEnv::Record(slot)) = (var_slot, caller.var_env) {
                wanted.push((top, slot));
            }
        }
        // A function that calls `eval` hands on every name of the caller.
        for function in script.ast.function_ids() {
            if !tree.function(function).has_direct_eval {
                continue;
            }
            for entry in &caller.entries {
                wanted.push((function, entry.slot));
                if let Some(slot) = entry.walk.var_slot {
                    wanted.push((function, slot));
                }
            }
            if let Some(record) = caller_record {
                wanted.push((function, record));
            }
            if let EvalVarEnv::Record(slot) = caller.var_env {
                wanted.push((function, slot));
            }
        }
        for (function, slot) in wanted {
            if plan.overflow {
                break;
            }
            plan.add_chain(script, function, slot);
        }
        plan
    }

    /// Adds the caller's cell `slot` to the captures of `function` and of
    /// every function around it up to the top of the eval code; stops the
    /// plan at the capture limits.
    fn add_chain(&mut self, script: &Script, function: FunctionId, slot: EvalSlot) {
        let mut current = Some(function);
        while let Some(f) = current {
            let key = (f.index() as u32, slot);
            if self.index.contains_key(&key) {
                return;
            }
            let base = script.scopes.function(f).captures.len();
            let Some(list) = self.extra.get_mut(f.index()) else {
                return;
            };
            if self.total >= MAX_HANDED_CAPTURES || base + list.len() >= MAX_FUNCTION_CAPTURES {
                self.overflow = true;
                return;
            }
            self.total += 1;
            self.index.insert(key, (base + list.len()) as u32);
            list.push(slot);
            current = script.ast.function(f).parent;
        }
    }

    /// The capture index of the caller's cell `slot` in `function`.
    pub(super) fn capture_index(&self, function: FunctionId, slot: EvalSlot) -> Option<u32> {
        self.index.get(&(function.index() as u32, slot)).copied()
    }

    /// The capture sources of the caller's cells of `function`: from the
    /// caller's frame for the top function, else from the parent.
    pub(super) fn sources(&self, script: &Script, function: FunctionId) -> Vec<CaptureSource> {
        let Some(list) = self.extra.get(function.index()) else {
            return Vec::new();
        };
        let parent = script.ast.function(function).parent;
        list.iter()
            .map(|&slot| match parent {
                None => match slot {
                    EvalSlot::Cell(r) => CaptureSource::ParentRegister(r),
                    EvalSlot::Capture(i) => CaptureSource::ParentCapture(i),
                },
                Some(parent) => {
                    CaptureSource::ParentCapture(self.capture_index(parent, slot).unwrap_or(0))
                }
            })
            .collect()
    }

    /// The caller's entry of a name.
    fn entry(&self, text: &String16) -> Option<&EvalName> {
        let caller = self.input.caller.as_ref()?;
        let &index = caller.by_name.get(text)?;
        caller.entries.get(index as usize)
    }
}

/// How eval code reaches a name of its caller.
#[derive(Clone, Copy, Debug)]
pub(super) struct CallerAccess {
    /// The capture with the cell; `None` for a global name.
    pub(super) capture: Option<u32>,
    /// The kind of the caller's binding.
    pub(super) kind: Option<BindingKind>,
    /// A private name: its index.
    pub(super) private: u32,
    /// The `this` of a derived constructor.
    pub(super) derived: bool,
    /// The dynamic part: the records of the eval code, then the caller's.
    pub(super) dynamic: Option<Dynamic>,
}

/// Names of a description with their texts (before their atoms exist).
type SpecNames = Vec<(String16, EvalName)>;

/// A scope of a description before its names are atoms.
#[derive(Debug)]
struct ScopeSpec {
    parent: Option<u32>,
    names: SpecNames,
    record: Option<EvalSlot>,
}

/// The scope description of a function before its names are atoms.
#[derive(Debug, Default)]
pub(crate) struct EvalSpec {
    scopes: Vec<ScopeSpec>,
    outer: SpecNames,
    outer_record: Option<EvalSlot>,
    eval_code: bool,
    sites: Vec<EvalSite>,
}

/// A name of a description before its atom exists.
fn spec_name(kind: BindingKind, slot: EvalSlot) -> EvalName {
    EvalName {
        name: crate::heap::Gc::new(0, 0),
        kind,
        slot,
        private: 0,
        count: 0,
        derived: false,
        walk: EvalWalk::default(),
    }
}

/// Makes the scope description of a function: the atoms of its names, the
/// names sorted by handle.
pub(super) fn materialize_eval(heap: &mut Heap, spec: EvalSpec) -> Result<EvalInfo, Error> {
    let mut intern = |names: SpecNames| -> Result<Box<[EvalName]>, Error> {
        let mut out = Vec::with_capacity(names.len());
        for (text, mut name) in names {
            name.name = heap.intern(text.as_str16())?;
            out.push(name);
        }
        out.sort_by_key(|n| (n.name.index(), n.name.generation()));
        out.dedup_by_key(|n| (n.name.index(), n.name.generation()));
        Ok(out.into_boxed_slice())
    };
    let mut scopes = Vec::with_capacity(spec.scopes.len());
    for scope in spec.scopes {
        scopes.push(EvalScope {
            parent: scope.parent,
            names: intern(scope.names)?,
            record: scope.record,
        });
    }
    let mut sites = spec.sites;
    sites.sort_by_key(|site| site.pc);
    Ok(EvalInfo {
        scopes: scopes.into_boxed_slice(),
        outer: intern(spec.outer)?,
        outer_record: spec.outer_record,
        eval_code: spec.eval_code,
        sites: sites.into_boxed_slice(),
    })
}

impl Session {
    /// The scopes of each function with a direct `eval`, in tree order.
    pub(super) fn eval_scopes(script: &Script) -> HashMap<FunctionId, Vec<ScopeId>> {
        let mut map: HashMap<FunctionId, Vec<ScopeId>> = HashMap::new();
        let tree = &script.scopes;
        if !script
            .ast
            .function_ids()
            .any(|f| tree.function(f).has_direct_eval)
        {
            return map;
        }
        for scope in tree.scope_ids() {
            let function = tree.scope(scope).function;
            if tree.function(function).has_direct_eval {
                map.entry(function).or_default().push(scope);
            }
        }
        map
    }
}

impl FunctionCompiler<'_> {
    // --- Eval code: the caller's names ---

    /// How a reference of eval code that resolves to its caller reaches
    /// the binding; `None` for other references.
    pub(super) fn caller_access(&self, id: RefId) -> Option<CallerAccess> {
        let reference = self.reference(id);
        if reference.resolution != Resolution::Caller {
            return None;
        }
        let plan = self.session.caller.as_ref()?;
        let caller = plan.input.caller.as_ref()?;
        let text = self.name_text(reference.name);
        let entry = plan.entry(&text);
        let capture = entry.and_then(|e| plan.capture_index(self.id, e.slot));
        let dynamic = if is_dynamic_name(text.as_str16()) {
            let inner = self.scopes.dynamic_lookup(id);
            let inner_count = inner.map_or(0, |l| l.count);
            let outer_count = match entry {
                Some(e) => e.count,
                None if caller.record.is_some() => u32::MAX,
                None => 0,
            };
            let total = inner_count.saturating_add(outer_count);
            match inner {
                Some(lookup) if inner_count > 0 => Some(Dynamic {
                    env: EnvSource::Ref(lookup.env),
                    count: count_operand(total),
                }),
                _ if outer_count > 0 => caller
                    .record
                    .and_then(|record| plan.capture_index(self.id, record))
                    .map(|index| Dynamic {
                        env: EnvSource::Capture(index),
                        count: count_operand(total),
                    }),
                _ => None,
            }
        } else {
            None
        };
        Some(CallerAccess {
            capture: if entry.is_some() { capture } else { None },
            kind: entry.map(|e| e.kind),
            private: entry.map_or(0, |e| e.private),
            derived: entry.is_some_and(|e| e.derived),
            dynamic,
        })
    }

    /// Whether a reference is a global name (by name, `GetGlobal`).
    pub(super) fn is_global_name(&self, id: RefId) -> bool {
        match self.reference(id).resolution {
            Resolution::Global => true,
            Resolution::Caller => self
                .caller_access(id)
                .is_some_and(|access| access.capture.is_none()),
            _ => false,
        }
    }

    /// The private index of a private name that eval code takes from its
    /// caller.
    pub(super) fn caller_private_index(&self, ident: Ident) -> Option<u32> {
        let access = self.caller_access(ident.reference)?;
        (access.kind == Some(BindingKind::PrivateName)).then_some(access.private)
    }

    /// Loads the static binding of a name that eval code takes from its
    /// caller.
    pub(super) fn load_caller(&mut self, id: RefId, dst: Option<Reg>) -> CResult<Reg> {
        let access = self
            .caller_access(id)
            .ok_or(Self::internal("eval code without its caller"))?;
        let t = self.target(dst)?;
        let reference = self.reference(id);
        match access.capture {
            Some(index) => {
                self.emit(Insn::LoadCapture { dst: t, index });
                if access.derived {
                    self.emit(Insn::CheckThis { src: t });
                }
                if access.kind.is_some_and(has_dead_zone) {
                    let name = self.name_const(reference.name);
                    self.emit(Insn::CheckTdz { src: t, name });
                }
            }
            // The caller has no dynamic record.
            None if reference.name == names::DYNAMIC_ENV => {
                self.emit(Insn::Undefined { dst: t });
            }
            None => {
                let site = self.site(self.name_text(reference.name));
                self.emit(Insn::GetGlobal { dst: t, site });
            }
        }
        Ok(t)
    }

    /// `PutValue` on the static binding of a name that eval code takes
    /// from its caller.
    pub(super) fn store_caller(&mut self, id: RefId, src: Reg) -> CResult<()> {
        let access = self
            .caller_access(id)
            .ok_or(Self::internal("eval code without its caller"))?;
        let reference = self.reference(id);
        let name = self.name_const(reference.name);
        let Some(index) = access.capture else {
            let site = self.site(self.name_text(reference.name));
            self.emit(Insn::SetGlobal { src, site });
            return Ok(());
        };
        let kind = access.kind;
        let constant = matches!(kind, Some(BindingKind::Const | BindingKind::ClassName))
            || (kind == Some(BindingKind::FunctionName) && self.strict);
        if kind.is_some_and(has_dead_zone) {
            let mark = self.mark();
            let t = self.temp()?;
            self.emit(Insn::LoadCapture { dst: t, index });
            self.emit(Insn::CheckTdz { src: t, name });
            self.release(mark);
        }
        if constant {
            self.emit(Insn::ThrowConstAssign { name });
        } else if kind != Some(BindingKind::FunctionName) {
            self.emit(Insn::StoreCapture { src, index });
        }
        Ok(())
    }

    /// Initializes a var-scoped binding of sloppy eval code in the
    /// variable environment (a function declaration, §19.2.1.3 step 17;
    /// the hoisted var of a block function, Annex B.3.2.3): its static
    /// binding there, a binding of the caller's record of eval vars, or
    /// a global.
    pub(super) fn init_var_env_binding(
        &mut self,
        binding: BindingId,
        src: Reg,
        function: bool,
    ) -> CResult<()> {
        let b = *self.scopes.binding(binding);
        let name = self.name_const(b.name);
        let text = self.name_text(b.name);
        let plan = self
            .session
            .caller
            .as_ref()
            .ok_or(Self::internal("eval code without its caller"))?;
        let caller = plan
            .input
            .caller
            .as_ref()
            .ok_or(Self::internal("eval code without its caller"))?;
        let var_slot = plan.entry(&text).and_then(|e| e.walk.var_slot);
        if let Some(slot) = var_slot {
            let index = plan.capture_index(self.id, slot).ok_or(Self::internal(
                "a var binding that eval code did not capture",
            ))?;
            self.emit(Insn::StoreCapture { src, index });
            return Ok(());
        }
        match caller.var_env {
            EvalVarEnv::Record(slot) => {
                let index = plan.capture_index(self.id, slot).ok_or(Self::internal(
                    "a var record that eval code did not capture",
                ))?;
                let mark = self.mark();
                let record = self.temp()?;
                self.emit(Insn::LoadCapture { dst: record, index });
                self.emit(Insn::SetVarBinding { record, src, name });
                self.release(mark);
            }
            EvalVarEnv::Global if function => {
                self.emit(Insn::InitGlobalFunction { src, name });
            }
            EvalVarEnv::Global => {
                let site = self.site(text);
                self.emit(Insn::SetGlobal { src, site });
            }
            EvalVarEnv::None => {
                return Err(Self::internal("a var of strict eval code outside it"));
            }
        }
        Ok(())
    }

    /// Whether Annex B.3.2.3 (or B.3.2.2 for indirect eval code) leaves a
    /// block function of eval code in its block (its var binding was not
    /// created).
    pub(super) fn skips_block_function(&self, binding: BindingId) -> bool {
        self.session
            .caller
            .as_ref()
            .is_some_and(|plan| plan.input.skip_block_functions.contains(&binding))
    }

    // --- Functions that call `eval`: the scope description ---

    /// Notes a direct `eval` call at `pc` with its callee.
    pub(super) fn note_eval_site(
        &mut self,
        pc: usize,
        callee: swb_js_syntax::ExprId,
    ) -> CResult<()> {
        let swb_js_syntax::ExprKind::Identifier(ident) = self.ast.expr(callee).kind else {
            return Err(Self::internal("a direct eval without its name"));
        };
        let scope = self.reference(ident.reference).scope;
        self.eval_sites.push((pc as u32, scope));
        Ok(())
    }

    /// The scope description of the function, if it calls `eval`.
    pub(super) fn eval_description(&self) -> CResult<Option<EvalSpec>> {
        if self.eval_sites.is_empty() {
            return Ok(None);
        }
        let scopes = self
            .session
            .eval_scopes
            .get(&self.id)
            .cloned()
            .unwrap_or_default();
        let local: HashMap<ScopeId, u32> = scopes
            .iter()
            .enumerate()
            .map(|(i, &s)| (s, i as u32))
            .collect();
        let mut spec = EvalSpec {
            eval_code: self.function.kind == FunctionKind::Eval,
            ..EvalSpec::default()
        };
        for &scope in &scopes {
            let parent = self
                .scopes
                .scope(scope)
                .parent
                .and_then(|p| local.get(&p).copied());
            let names = self.scope_names(scope);
            let record = self
                .scopes
                .dynamic_env(scope)
                .and_then(|env| cell_slot(self.scopes.binding(env.binding).storage));
            spec.scopes.push(ScopeSpec {
                parent,
                names,
                record,
            });
        }
        self.outer_names(&mut spec);
        for &(pc, scope) in &self.eval_sites {
            let site = self.eval_site(pc, scope, &local)?;
            spec.sites.push(site);
        }
        Ok(Some(spec))
    }

    /// The names of a scope of the function in its description.
    fn scope_names(&self, scope: ScopeId) -> SpecNames {
        let derived_this = self.this_function_kind() == Some(FunctionKind::DerivedConstructor);
        let private_env = self
            .scopes
            .private_environment(scope)
            .and_then(|b| cell_slot(self.scopes.binding(b).storage));
        let mut names = Vec::new();
        for &binding in self.scopes.bindings_of(scope) {
            let b = self.scopes.binding(binding);
            let entry = match b.storage {
                Storage::Private(index) => private_env.map(|slot| EvalName {
                    private: index,
                    ..spec_name(b.kind, slot)
                }),
                storage => cell_slot(storage).map(|slot| EvalName {
                    derived: b.kind == BindingKind::This && derived_this,
                    ..spec_name(b.kind, slot)
                }),
            };
            if let Some(entry) = entry {
                names.push((self.name_text(b.name), entry));
            }
        }
        names
    }

    /// The names of enclosing functions in the description: the eval
    /// references of the analysis that found a capture; for eval code (and
    /// functions in it), the names of the caller.
    fn outer_names(&self, spec: &mut EvalSpec) {
        let mut seen: HashSet<String16> = HashSet::new();
        for &id in self.scopes.eval_outer_references(self.id) {
            let reference = self.reference(id);
            let text = self.name_text(reference.name);
            let slot = match reference.resolution {
                Resolution::Capture(index) => EvalSlot::Capture(index),
                // The caller's names of eval code come below.
                _ => continue,
            };
            if reference.name == names::DYNAMIC_ENV {
                spec.outer_record = Some(slot);
                continue;
            }
            let Some(binding) = reference.binding else {
                continue;
            };
            let b = self.scopes.binding(binding);
            let private =
                self.scopes
                    .private_name(id)
                    .map(|p| match self.scopes.binding(p).storage {
                        Storage::Private(index) => index,
                        _ => 0,
                    });
            let kind = if private.is_some() {
                BindingKind::PrivateName
            } else {
                b.kind
            };
            let derived = b.kind == BindingKind::This
                && self.ast.function(self.scopes.scope(b.scope).function).kind
                    == FunctionKind::DerivedConstructor;
            let count = self.scopes.dynamic_lookup(id).map_or(0, |l| l.count);
            seen.insert(text.clone());
            spec.outer.push((
                text,
                EvalName {
                    private: private.unwrap_or(0),
                    count,
                    derived,
                    ..spec_name(kind, slot)
                },
            ));
        }
        let Some(plan) = self.session.caller.as_ref() else {
            return;
        };
        let Some(caller) = plan.input.caller.as_ref() else {
            return;
        };
        // In eval code: the records of the eval code between this function
        // and the top come before the caller's.
        let inner = self.records_to_top();
        let top = self.function.kind == FunctionKind::Eval;
        let capture = |slot: EvalSlot| plan.capture_index(self.id, slot).map(EvalSlot::Capture);
        let mut texts: Vec<(&String16, &u32)> = caller.by_name.iter().collect();
        texts.sort_by_key(|&(_, &i)| i);
        for (text, &index) in texts {
            // A name of the eval code outside this function hides the
            // caller's (the analysis found its binding).
            if seen.contains(text) {
                continue;
            }
            let Some(entry) = caller.entries.get(index as usize) else {
                continue;
            };
            let Some(slot) = capture(entry.slot) else {
                continue;
            };
            let dynamic = is_dynamic_name(text.as_str16());
            let mut name = EvalName {
                slot,
                count: if dynamic {
                    entry.count.saturating_add(inner)
                } else {
                    entry.count
                },
                walk: EvalWalk::default(),
                ..*entry
            };
            // Only the top function of eval code shares the caller's
            // variable environment.
            if top {
                name.walk = EvalWalk {
                    var_slot: entry.walk.var_slot.and_then(capture),
                    ..entry.walk
                };
            }
            spec.outer.push((text.clone(), name));
        }
        if spec.outer_record.is_none() {
            spec.outer_record = caller.record.and_then(capture);
        }
    }

    /// The dynamic records of eval code between this function and the top
    /// function (0 outside eval code functions).
    fn records_to_top(&self) -> u32 {
        let mut count = 0u32;
        let mut current = self.scopes.scope(self.function.scope).parent;
        while let Some(scope) = current {
            let entry = self.scopes.scope(scope);
            if entry.kind != ScopeKind::Eval && self.scopes.dynamic_env(scope).is_some() {
                count += 1;
            }
            current = entry.parent;
        }
        count
    }

    /// One call of `eval` in the description.
    fn eval_site(
        &self,
        pc: u32,
        scope: ScopeId,
        local: &HashMap<ScopeId, u32>,
    ) -> CResult<EvalSite> {
        let index = local
            .get(&scope)
            .copied()
            .ok_or(Self::internal("an eval call outside its function"))?;
        let strict = self.strict
            || self
                .strict_ranges
                .iter()
                .any(|&(start, end)| start <= pc && pc < end);
        let kind = self.function.kind;
        let (var_scope, params) = match kind {
            FunctionKind::Eval => (None, false),
            FunctionKind::Script => (local.get(&self.function.scope).copied(), false),
            _ => {
                let body = self.function.body_scope;
                if body != self.function.scope && !self.scope_inside(scope, body) {
                    (local.get(&self.function.scope).copied(), true)
                } else {
                    (local.get(&body).copied(), false)
                }
            }
        };
        let var_env = if strict {
            EvalVarEnv::None
        } else {
            match kind {
                FunctionKind::Script => EvalVarEnv::Global,
                FunctionKind::Eval => self.inherited_var_env(),
                _ => {
                    let scope = if params {
                        self.function.scope
                    } else {
                        self.function.body_scope
                    };
                    self.scopes
                        .dynamic_env(scope)
                        .and_then(|env| cell_slot(self.scopes.binding(env.binding).storage))
                        .map_or(EvalVarEnv::None, EvalVarEnv::Record)
                }
            }
        };
        let flags = self.site_flags(strict);
        Ok(EvalSite {
            pc,
            scope: index,
            var_scope,
            params,
            var_env,
            flags,
        })
    }

    /// Whether `scope` is `outer` or inside it.
    fn scope_inside(&self, scope: ScopeId, outer: ScopeId) -> bool {
        let mut current = Some(scope);
        while let Some(s) = current {
            if s == outer {
                return true;
            }
            current = self.scopes.scope(s).parent;
        }
        false
    }

    /// The variable environment of the caller of eval code, as a capture of
    /// this function.
    fn inherited_var_env(&self) -> EvalVarEnv {
        let Some(plan) = self.session.caller.as_ref() else {
            return EvalVarEnv::None;
        };
        let Some(caller) = plan.input.caller.as_ref() else {
            return EvalVarEnv::None;
        };
        match caller.var_env {
            EvalVarEnv::Record(slot) => plan
                .capture_index(self.id, slot)
                .map_or(EvalVarEnv::None, |i| {
                    EvalVarEnv::Record(EvalSlot::Capture(i))
                }),
            other => other,
        }
    }

    /// The kind of the nearest function around this one (or itself) that
    /// has its own `this`; `None` in eval code.
    fn this_function_kind(&self) -> Option<FunctionKind> {
        let mut current = Some(self.id);
        while let Some(f) = current {
            let function = self.ast.function(f);
            match function.kind {
                FunctionKind::Arrow => current = function.parent,
                FunctionKind::Eval => return None,
                kind => return Some(kind),
            }
        }
        None
    }

    /// The context of a call of `eval` (§19.2.1.1 steps 5 to 11).
    fn site_flags(&self, strict: bool) -> EvalFlags {
        let Some(kind) = self.this_function_kind() else {
            // Eval code: its caller's context.
            let inherited = self
                .session
                .caller
                .as_ref()
                .and_then(|plan| plan.input.caller.as_ref())
                .map(|caller| caller.flags)
                .unwrap_or_default();
            return EvalFlags {
                strict,
                ..inherited
            };
        };
        EvalFlags {
            strict,
            in_function: !matches!(kind, FunctionKind::Script | FunctionKind::Module),
            in_method: kind.has_home_object(),
            in_derived_constructor: kind == FunctionKind::DerivedConstructor,
            in_class_field_initializer: matches!(
                kind,
                FunctionKind::InstanceInitializer | FunctionKind::StaticInitializer
            ),
        }
    }
}

/// The cell of a binding's storage in a function that calls `eval` (all
/// its bindings are cells); `None` for other storage.
fn cell_slot(storage: Storage) -> Option<EvalSlot> {
    match storage {
        Storage::Cell(r) => Some(EvalSlot::Cell(r)),
        _ => None,
    }
}

/// Whether a load of a binding of this kind checks its dead zone.
fn has_dead_zone(kind: BindingKind) -> bool {
    kind.is_lexical() || kind == BindingKind::Parameter
}
