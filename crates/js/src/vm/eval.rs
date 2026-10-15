//! `eval` (ECMA-262 §19.2.1): `PerformEval` for direct calls (the
//! `CallEval` instruction, §13.3.6.1) and for the global `eval` function
//! (indirect: global code), and `EvalDeclarationInstantiation`
//! (§19.2.1.3) with Annex B.3.2.3 and B.3.4.
//!
//! A call parses the string with the context of its call site, looks up
//! the names that the eval code takes from its caller in the caller's
//! scope description (`bytecode::EvalInfo`, see `compiler/eval.rs`),
//! checks the declarations, compiles the code, creates the var bindings
//! and runs the code as a frame of the interpreter loop (no Rust
//! recursion, so `function f() { eval('f()') }` ends at the frame limit).
//! The code object is garbage once the frame and its closures are gone.
//!
//! The checks of `EvalDeclarationInstantiation` read the bindings between
//! the call and the variable environment from the description (static)
//! and the global object (dynamic); no script code runs between them and
//! the creation of the bindings, so the compile can sit in between and use
//! the decisions of Annex B.3.2.3.

use std::borrow::Cow;
use std::collections::HashSet;
use std::rc::Rc;

use swb_js_syntax::{
    BindingId, BindingKind, ErrorKind, EvalContext, ParseError, Resolution, Script, Storage,
};
use swb_js_text::{Str16, String16};

use crate::bytecode::{
    EvalInfo, EvalName, EvalSite, EvalSlot, EvalVarEnv, EvalWalk, FunctionCode, Reg, find_eval_name,
};
use crate::compiler::eval::{
    EvalCaller, EvalInput, MAX_FUNCTION_CAPTURES, MAX_HANDED_CAPTURES, is_dynamic_name,
};
use crate::compiler::{CompileError, compile_eval};
use crate::error::ThrowKind;
use crate::heap::{Gc, Generic};
use crate::object::{Object, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::call::{CallSite, Step};
use crate::vm::{Intrinsic, NativeCall, NativeReturn, ReturnTo, VmError, VmResult};

/// The bytes of the parse tree per code unit of eval code that the heap
/// must have room for before the parse. Real code takes about this much;
/// a long string literal takes almost nothing, dense code such as
/// `y;y;y;` about 30. The exact size is checked after the parse, so a
/// script that evals dense code can make the parse allocate up to a few
/// times this check beyond the heap limit, for as long as the parse runs.
const PARSE_BYTES_PER_UNIT: usize = 8;

/// The bytes of the compile for each handed capture: the entry of the
/// capture plan, the capture of the closure and the copied name of the
/// scope description.
const HANDED_CAPTURE_BYTES: usize = 96;

/// The declarations of sloppy eval code that go into its variable
/// environment, after the checks (§19.2.1.3 steps 3 to 11).
#[derive(Default)]
struct Declarations {
    /// The names of the bindings to create, in the order of the
    /// specification: Annex B.3.2.3 vars, functions, vars.
    created: Vec<Gc<JsString>>,
    /// The block functions that Annex B.3.2.3 does not hoist.
    skip: HashSet<BindingId>,
}

/// The variable environment and the walk of `EvalDeclarationInstantiation`.
struct VarEnvironment<'a> {
    env: EvalVarEnv,
    /// The caller's description and call (direct eval only).
    caller: Option<(&'a EvalInfo, &'a EvalSite)>,
}

impl VarEnvironment<'_> {
    fn walk(&self, name: Gc<JsString>) -> EvalWalk {
        match self.caller {
            Some((info, site)) => eval_walk(info, site, name),
            None => EvalWalk::default(),
        }
    }
}

impl Runtime {
    /// Whether `value` is the `%eval%` of the running code's realm
    /// (§13.3.6.1 step 6.a).
    pub(crate) fn is_eval_function(&self, value: Value) -> bool {
        let Value::Object(function) = value else {
            return false;
        };
        self.vm
            .realms
            .get(self.current_realm() as usize)
            .and_then(|realm| realm.intrinsic(Intrinsic::Eval))
            == Some(function)
    }

    /// The first element of the argument array of a spread call.
    pub(crate) fn first_element(&mut self, list: Value) -> VmResult<Value> {
        let Value::Object(array) = list else {
            return Err(VmError::invariant("spread arguments that are not an array"));
        };
        Ok(match self.heap.get(array, PropertyKey::Index(0), list)? {
            crate::object::GetResult::Value(value) => value,
            crate::object::GetResult::CallGetter { .. } => {
                return Err(VmError::invariant("a getter in spread arguments"));
            }
        })
    }

    /// The `CallEval` instruction at `pc` (§13.3.6.1 steps 6 and 7): a
    /// direct eval if the callee in register `callee` is `%eval%`, else an
    /// ordinary call of the window at `callee` (`argc` arguments, or the
    /// array of a spread call). Kept out of the interpreter loop, so that
    /// ordinary calls do not pay for it.
    #[inline(never)]
    pub(crate) fn call_eval(
        &mut self,
        code: &Rc<FunctionCode>,
        pc: usize,
        (base, callee, argc, spread, restore_top): (usize, Reg, u16, bool, usize),
    ) -> VmResult<Step> {
        let register = |r: Reg| base + usize::from(r);
        let function = self.stack_value(register(callee))?;
        if self.is_eval_function(function) {
            let argument = if spread {
                let list = self.stack_value(register(callee.saturating_add(2)))?;
                self.first_element(list)?
            } else if argc > 0 {
                self.stack_value(register(callee.saturating_add(2)))?
            } else {
                Value::Undefined
            };
            return self.direct_eval(code, pc, callee, argument);
        }
        let (window, argc) = if spread {
            let this = self.stack_value(register(callee.saturating_add(1)))?;
            let list = self.stack_value(register(callee.saturating_add(2)))?;
            self.push_spread_window(function, this, list)?
        } else {
            (register(callee) + 2, usize::from(argc))
        };
        self.invoke(CallSite {
            window,
            argc,
            ret: ReturnTo::Register(callee),
            restore_top,
            construct: false,
            new_target: None,
        })
    }

    /// A direct `eval` call at `pc` of the running frame (§19.2.1.1 with
    /// `direct` true): the result of a non-string argument, or the frame of
    /// the eval code, whose result goes into `callee`.
    pub(crate) fn direct_eval(
        &mut self,
        code: &Rc<FunctionCode>,
        pc: usize,
        callee: Reg,
        argument: Value,
    ) -> VmResult<Step> {
        let Value::String(string) = argument else {
            return Ok(Step::Value(argument));
        };
        let info = code.eval.as_deref().ok_or(VmError::invariant(
            "a direct eval without a scope description",
        ))?;
        let site = *info
            .site(pc)
            .ok_or(VmError::invariant("a direct eval call without its site"))?;
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("eval without a frame"))?;
        let (base, realm) = (frame.base as usize, frame.realm);
        let text = self.heap.string(string)?.as_str16().to_string16();
        let context = EvalContext {
            direct: true,
            strict: site.flags.strict,
            in_function: site.flags.in_function,
            in_method: site.flags.in_method,
            in_derived_constructor: site.flags.in_derived_constructor,
            in_class_field_initializer: site.flags.in_class_field_initializer,
            private_names: self.private_names_at(info, &site)?,
        };
        let script = self.parse_eval_code(text.as_str16(), &context)?;
        self.with_scope(|rt| {
            let caller = rt.eval_caller(info, &site, &script)?;
            let var_env = VarEnvironment {
                env: site.var_env,
                caller: Some((info, &site)),
            };
            let declarations = rt.eval_declarations(&script, &var_env)?;
            let input = EvalInput {
                caller: Some(caller),
                skip_block_functions: declarations.skip.clone(),
            };
            let (handle, compiled) = rt.compile_eval_code(&script, text.as_str16(), input)?;
            rt.heap.record(handle);
            let record = match site.var_env {
                EvalVarEnv::Record(slot) => Some(rt.caller_record(base, slot)?),
                _ => None,
            };
            rt.create_eval_bindings(&declarations, &var_env, record)?;
            let captures = rt.collect_captures(base, &compiled)?;
            let closure = rt.new_closure(handle, &compiled, captures, realm)?;
            let restore_top = base + code.register_count as usize;
            let window = rt.push_window(closure.into(), Value::Undefined, &[])?;
            let call = CallSite {
                window,
                argc: 0,
                ret: ReturnTo::Register(callee),
                restore_top,
                construct: false,
                new_target: None,
            };
            rt.invoke(call)
        })
    }

    /// The global `eval(x)` (§19.2.1.1 with `direct` false): global code of
    /// the function's realm, run as a deferred call.
    pub(crate) fn indirect_eval(&mut self, call: &NativeCall) -> VmResult<NativeReturn> {
        let argument = self.arg(call, 0);
        let Value::String(string) = argument else {
            return Ok(NativeReturn::Value(argument));
        };
        let text = self.heap.string(string)?.as_str16().to_string16();
        let script = self.parse_eval_code(text.as_str16(), &EvalContext::default())?;
        let var_env = VarEnvironment {
            env: if script.ast.function(script.top).strict {
                EvalVarEnv::None
            } else {
                EvalVarEnv::Global
            },
            caller: None,
        };
        let declarations = self.eval_declarations(&script, &var_env)?;
        let input = EvalInput {
            caller: None,
            skip_block_functions: declarations.skip.clone(),
        };
        let (handle, compiled) = self.compile_eval_code(&script, text.as_str16(), input)?;
        self.heap.record(handle);
        self.create_eval_bindings(&declarations, &var_env, None)?;
        let closure = self.new_closure(handle, &compiled, Box::new([]), call.realm)?;
        let global = self.global_object(call.realm)?;
        Ok(NativeReturn::Call {
            callee: closure.into(),
            this: global.into(),
            args: Vec::new(),
        })
    }

    /// Parses eval code; an early error is a `SyntaxError` (or the
    /// `RangeError` of too deep nesting) thrown at the call. The memory of
    /// the parse counts against the heap limit while the compile runs.
    fn parse_eval_code(&mut self, text: Str16<'_>, context: &EvalContext) -> VmResult<Script> {
        // The tree of a source takes some bytes per code unit: a script
        // that evals a string too large for the heap ends before the
        // parse allocates the tree, not after.
        self.heap
            .reserve(text.len().saturating_mul(PARSE_BYTES_PER_UNIT), &self.vm)?;
        let script =
            swb_js_syntax::parse_eval(text, context, &mut self.budget).map_err(parse_failure)?;
        self.charge_units(text.len())?;
        let size = script.ast.heap_size() + script.scopes.heap_size();
        self.heap.reserve(size, &self.vm)?;
        Ok(script)
    }

    /// Compiles parsed eval code; a compile failure is thrown at the call.
    fn compile_eval_code(
        &mut self,
        script: &Script,
        text: Str16<'_>,
        input: EvalInput,
    ) -> VmResult<(Gc<Generic>, Rc<FunctionCode>)> {
        if let Some(caller) = &input.caller {
            self.check_handed_captures(script, caller)?;
        }
        let origin = self.eval_origin();
        compile_eval(
            &mut self.heap,
            script,
            (text, origin),
            &mut self.budget,
            input,
        )
        .map_err(|error| match error {
            CompileError::Script { kind, message, .. } => VmError::Raise { kind, message },
            CompileError::Heap(error) => error.into(),
        })
    }

    /// Bounds the work of eval code whose functions call `eval` themselves:
    /// each of them, and every function around it, captures every cell of
    /// the `caller` (see `CallerPlan` in `compiler/eval.rs`), so the
    /// compile costs about functions x cells. Past [`MAX_HANDED_CAPTURES`]
    /// in all, or [`MAX_FUNCTION_CAPTURES`] in one function, the call
    /// throws the `RangeError` of the capture limit; otherwise the work is
    /// charged to the time budget and its memory reserved before the
    /// compile. The plan itself stops at the same limits for the cells
    /// that functions only use (depth x names, without a nested `eval`).
    fn check_handed_captures(&mut self, script: &Script, caller: &EvalCaller) -> VmResult<()> {
        if caller.entries.is_empty() {
            return Ok(());
        }
        // The distinct cells: the names, their var records, the caller's
        // record and its variable environment.
        let mut cells = HashSet::new();
        for entry in &caller.entries {
            cells.insert(entry.slot);
            cells.extend(entry.walk.var_slot);
        }
        self.charge(caller.entries.len())?;
        let per_function = cells.len().saturating_add(2);
        let mut marked = vec![false; script.ast.function_count()];
        let mut functions = 0usize;
        let mut widest = 0usize;
        for function in script.ast.function_ids() {
            if !script.scopes.function(function).has_direct_eval {
                continue;
            }
            let mut current = Some(function);
            while let Some(f) = current {
                let Some(seen) = marked.get_mut(f.index()) else {
                    break;
                };
                if *seen {
                    break;
                }
                *seen = true;
                functions += 1;
                widest = widest.max(script.scopes.function(f).captures.len());
                current = script.ast.function(f).parent;
            }
        }
        if functions == 0 {
            return Ok(());
        }
        let handed = functions.saturating_mul(per_function);
        if handed > MAX_HANDED_CAPTURES
            || widest.saturating_add(per_function) > MAX_FUNCTION_CAPTURES
        {
            return Err(VmError::range_error(
                swb_js_syntax::messages::TOO_MANY_CAPTURES,
            ));
        }
        self.charge(handed)?;
        self.heap
            .reserve(handed.saturating_mul(HANDED_CAPTURE_BYTES), &self.vm)?;
        Ok(())
    }

    /// The origin of eval code that the running frame makes, as V8 shows
    /// it in stack traces: `eval at f (file:1:5)`, with `<anonymous>` for
    /// script and eval code; a caller in eval code shows its own origin
    /// instead of a position. At most 1,024 characters, so that nested
    /// eval code cannot grow it without bound.
    fn eval_origin(&self) -> Rc<str> {
        let Some(frame) = self.vm.frames.last() else {
            return Rc::from("eval at <anonymous>");
        };
        let code = &frame.compiled;
        let caller = match code.kind {
            crate::bytecode::CodeKind::Script | crate::bytecode::CodeKind::Eval => String::new(),
            _ => self.code_name(code),
        };
        let caller = if caller.is_empty() {
            "<anonymous>"
        } else {
            &caller
        };
        let location = if let Some(origin) = &code.source.origin {
            origin.to_string()
        } else {
            let pc = (frame.pc as usize).saturating_sub(1);
            crate::vm::stack_text::frame_location(code, code.source_offset(pc))
        };
        let origin: String = format!("eval at {caller} ({location})")
            .chars()
            .take(1_024)
            .collect();
        Rc::from(origin)
    }

    /// The record of eval vars of the caller in `slot`.
    fn caller_record(&self, base: usize, slot: EvalSlot) -> VmResult<Gc<Object>> {
        let cell = match slot {
            EvalSlot::Cell(r) => match self.vm.stack.get(base + usize::from(r)) {
                Some(Value::Cell(cell)) => *cell,
                _ => return Err(VmError::invariant("a var record register without a cell")),
            },
            EvalSlot::Capture(index) => self.capture(index)?,
        };
        match self.heap.cell(cell)?.value {
            Value::Object(record) => Ok(record),
            _ => Err(VmError::invariant("a var record cell without a record")),
        }
    }

    /// The private names of the classes around a call (§19.2.1.1 step
    /// 5.c.ii: the early errors of eval code need them).
    fn private_names_at(&self, info: &EvalInfo, site: &EvalSite) -> VmResult<Vec<String16>> {
        let mut names = Vec::new();
        let mut take = |name: &EvalName| -> VmResult<()> {
            if name.kind == BindingKind::PrivateName {
                names.push(self.heap.string(name.name)?.as_str16().to_string16());
            }
            Ok(())
        };
        for_each_scope(info, site, |scope| {
            for name in &scope.names {
                take(name)?;
            }
            Ok(())
        })?;
        for name in &info.outer {
            take(name)?;
        }
        Ok(names)
    }

    /// The names that eval code takes from its caller (§19.2.1.1 steps
    /// 13 to 15): those of its references that do not resolve inside it
    /// and of its var declarations; all names of the caller if the eval
    /// code calls `eval` itself.
    fn eval_caller(
        &mut self,
        info: &EvalInfo,
        site: &EvalSite,
        script: &Script,
    ) -> VmResult<EvalCaller> {
        let mut caller = EvalCaller {
            record: innermost_record(info, site),
            var_env: site.var_env,
            flags: site.flags,
            ..EvalCaller::default()
        };
        let tree = &script.scopes;
        let calls_eval = script
            .ast
            .function_ids()
            .any(|f| tree.function(f).has_direct_eval);
        let mut atoms: Vec<Gc<JsString>> = Vec::new();
        if calls_eval {
            atoms = all_names(info, site);
        } else {
            let mut seen = HashSet::new();
            let mut texts: Vec<String16> = Vec::new();
            for id in tree.reference_ids() {
                let reference = tree.reference(id);
                if reference.dead
                    || reference.resolution != Resolution::Caller
                    || !seen.insert(reference.name)
                {
                    continue;
                }
                texts.push(script_name(script, reference.name));
            }
            let top_scope = script.ast.function(script.top).scope;
            for &binding in tree.bindings_of(top_scope) {
                let b = tree.binding(binding);
                if b.storage == Storage::Caller && seen.insert(b.name) {
                    texts.push(script_name(script, b.name));
                }
            }
            for text in texts {
                let atom = self.heap.intern(text.as_str16())?;
                self.heap.record(Value::String(atom));
                atoms.push(atom);
            }
        }
        self.charge(atoms.len())?;
        for atom in atoms {
            let text = self.heap.string(atom)?.as_str16().to_string16();
            let dynamic = is_dynamic_name(text.as_str16());
            let Some(mut entry) = describe(info, site, atom, dynamic) else {
                continue;
            };
            entry.walk = eval_walk(info, site, atom);
            let index = caller.entries.len() as u32;
            caller.entries.push(entry);
            caller.by_name.insert(text, index);
        }
        Ok(caller)
    }

    /// The checks of `EvalDeclarationInstantiation` (§19.2.1.3 steps 3 to
    /// 11, Annex B.3.2.3 and B.3.4) for sloppy eval code: a `SyntaxError`
    /// for a var that a lexical declaration between the call and the
    /// variable environment (or a global lexical declaration) has; a
    /// `TypeError` for a global function or var that cannot be declared.
    fn eval_declarations(
        &mut self,
        script: &Script,
        var_env: &VarEnvironment<'_>,
    ) -> VmResult<Declarations> {
        let mut declarations = Declarations::default();
        if var_env.env == EvalVarEnv::None {
            return Ok(declarations);
        }
        let tree = &script.scopes;
        let top_scope = script.ast.function(script.top).scope;
        let outside = |storage: Storage| matches!(storage, Storage::Caller | Storage::Global);
        let mut functions = Vec::new();
        let mut vars = Vec::new();
        for &binding in tree.bindings_of(top_scope) {
            let b = tree.binding(binding);
            if !outside(b.storage) {
                continue;
            }
            let atom = self.heap.intern(script_name(script, b.name).as_str16())?;
            self.heap.record(Value::String(atom));
            match b.kind {
                BindingKind::Function => functions.push(atom),
                BindingKind::Var => vars.push(atom),
                _ => {}
            }
        }
        self.charge(functions.len() + vars.len())?;
        let global = match var_env.env {
            EvalVarEnv::Global => Some(self.global_object(self.current_realm())?),
            _ => None,
        };
        // Step 3: the conflicts with lexical declarations.
        for &name in functions.iter().chain(&vars) {
            if (global.is_some() && self.has_global_lexical(name)) || var_env.walk(name).conflict {
                return Err(redeclared(self, name));
            }
        }
        // Steps 8 and 10: what the global object allows.
        if let Some(global) = global {
            for &name in &functions {
                if !self.can_declare_global_function(global, name)? {
                    return Err(VmError::type_error(format!(
                        "Cannot redefine property: {}",
                        self.name_text(name)
                    )));
                }
            }
            for &name in &vars {
                if !self.can_declare_global_var(global, name)? {
                    return Err(VmError::type_error(format!(
                        "Cannot define global variable '{}', the global object is not extensible",
                        self.name_text(name)
                    )));
                }
            }
        }
        // Annex B.3.2.3: the vars of block functions.
        let mut hoisted = Vec::new();
        let mut decided = HashSet::new();
        for function in script.ast.function_ids() {
            let Some(target) = tree.function(function).block_function_var else {
                continue;
            };
            let Some(binding) = tree.reference(target).binding else {
                continue;
            };
            let b = tree.binding(binding);
            if b.scope != top_scope || !outside(b.storage) || !decided.insert(binding) {
                continue;
            }
            let name = self.heap.intern(script_name(script, b.name).as_str16())?;
            self.heap.record(Value::String(name));
            let exists = var_env.walk(name).conflict;
            let definable = match global {
                Some(global) if !exists => {
                    !self.has_global_lexical(name) && self.can_declare_global_var(global, name)?
                }
                _ => true,
            };
            if exists || !definable {
                declarations.skip.insert(binding);
            } else if b.kind == BindingKind::BlockFunctionVar {
                hoisted.push(name);
            }
        }
        declarations.created = hoisted;
        declarations.created.extend(functions);
        declarations.created.extend(vars);
        Ok(declarations)
    }

    /// Creates the var bindings of eval code (§19.2.1.3 steps 17 and 18,
    /// Annex B.3.2.3): deletable global properties, or bindings of the
    /// caller's record of eval vars for names that its variable
    /// environment does not have. The function values come when the code
    /// runs.
    fn create_eval_bindings(
        &mut self,
        declarations: &Declarations,
        var_env: &VarEnvironment<'_>,
        record: Option<Gc<Object>>,
    ) -> VmResult<()> {
        match var_env.env {
            EvalVarEnv::None => Ok(()),
            EvalVarEnv::Global => {
                let global = self.global_object(self.current_realm())?;
                for &name in &declarations.created {
                    let key = PropertyKey::String(name);
                    if self.heap.get_own_property(global, key)?.is_none()
                        && self.heap.is_extensible(global)?
                    {
                        let desc = PropertyDescriptor::data(Value::Undefined, true, true, true);
                        self.heap.define_own_property(global, key, desc, &self.vm)?;
                    }
                }
                Ok(())
            }
            EvalVarEnv::Record(_) => {
                let record = record.ok_or(VmError::invariant("eval vars without a record"))?;
                for &name in &declarations.created {
                    let key = PropertyKey::String(name);
                    if var_env.walk(name).var_slot.is_none() && !self.var_env_has(record, key)? {
                        self.set_var_binding(record, key, Value::Undefined)?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Whether the global environment has a lexical declaration of `name`
    /// (§9.1.1.4.12 `HasLexicalDeclaration`).
    fn has_global_lexical(&self, name: Gc<JsString>) -> bool {
        self.vm
            .realms
            .get(self.current_realm() as usize)
            .is_some_and(|realm| realm.lexical.contains_key(&name))
    }

    /// `CanDeclareGlobalFunction` (§9.1.1.4.16).
    pub(crate) fn can_declare_global_function(
        &self,
        global: Gc<Object>,
        name: Gc<JsString>,
    ) -> VmResult<bool> {
        Ok(
            match self
                .heap
                .get_own_property(global, PropertyKey::String(name))?
            {
                None => self.heap.is_extensible(global)?,
                Some(property) if property.configurable() => true,
                Some(Property::Data {
                    writable,
                    enumerable,
                    ..
                }) => writable && enumerable,
                Some(Property::Accessor { .. }) => false,
            },
        )
    }

    /// `CanDeclareGlobalVar` (§9.1.1.4.15).
    pub(crate) fn can_declare_global_var(
        &self,
        global: Gc<Object>,
        name: Gc<JsString>,
    ) -> VmResult<bool> {
        let key = PropertyKey::String(name);
        Ok(
            self.heap.get_own_property(global, key)?.is_some()
                || self.heap.is_extensible(global)?,
        )
    }
}

/// The `SyntaxError` of a var of eval code that a lexical declaration has
/// (V8's message).
fn redeclared(rt: &Runtime, name: Gc<JsString>) -> VmError {
    VmError::Raise {
        kind: ThrowKind::SyntaxError,
        message: Cow::Owned(format!(
            "Identifier '{}' has already been declared",
            rt.name_text(name)
        )),
    }
}

/// The error that a failed parse of eval code throws.
fn parse_failure(error: ParseError) -> VmError {
    let mut message = error.message.into_owned();
    if let Some(construct) = error.unsupported {
        message.push_str(" (");
        message.push_str(construct);
        message.push(')');
    }
    VmError::Raise {
        kind: match error.kind {
            ErrorKind::Syntax => ThrowKind::SyntaxError,
            ErrorKind::Range => ThrowKind::RangeError,
        },
        message: Cow::Owned(message),
    }
}

/// The text of a name of a script.
fn script_name(script: &Script, name: swb_js_syntax::NameId) -> String16 {
    script
        .names
        .get(name)
        .map(Str16::to_string16)
        .unwrap_or_default()
}

/// Calls `f` with each scope of the calling function from the call
/// outward (at most as many as there are, so a broken parent link cannot
/// loop).
fn for_each_scope(
    info: &EvalInfo,
    site: &EvalSite,
    mut f: impl FnMut(&crate::bytecode::EvalScope) -> VmResult<()>,
) -> VmResult<()> {
    let mut current = Some(site.scope);
    let mut left = info.scopes.len();
    while let Some(index) = current {
        if left == 0 {
            break;
        }
        left -= 1;
        let Some(scope) = info.scopes.get(index as usize) else {
            break;
        };
        f(scope)?;
        current = scope.parent;
    }
    Ok(())
}

/// The first binding of `name` from the call outward, with the number of
/// dynamic records before it (for a name that records can have).
fn describe(
    info: &EvalInfo,
    site: &EvalSite,
    name: Gc<JsString>,
    dynamic: bool,
) -> Option<EvalName> {
    let mut count = 0u32;
    let mut found = None;
    let _ = for_each_scope(info, site, |scope| {
        if found.is_some() {
            return Ok(());
        }
        if let Some(entry) = find_eval_name(&scope.names, name) {
            found = Some(EvalName {
                count: if dynamic { count } else { 0 },
                ..*entry
            });
        } else if dynamic && scope.record.is_some() {
            count = count.saturating_add(1);
        }
        Ok(())
    });
    if found.is_some() {
        return found;
    }
    let entry = find_eval_name(&info.outer, name)?;
    Some(EvalName {
        count: if dynamic {
            count.saturating_add(entry.count)
        } else {
            entry.count
        },
        ..*entry
    })
}

/// The innermost dynamic record at a call.
fn innermost_record(info: &EvalInfo, site: &EvalSite) -> Option<EvalSlot> {
    let mut record = None;
    let _ = for_each_scope(info, site, |scope| {
        if record.is_none() {
            record = scope.record;
        }
        Ok(())
    });
    record.or(info.outer_record)
}

/// The walk of `EvalDeclarationInstantiation` for `name` (see
/// [`EvalWalk`]).
fn eval_walk(info: &EvalInfo, site: &EvalSite, name: Gc<JsString>) -> EvalWalk {
    let mut walk = EvalWalk::default();
    let mut inner = true;
    let mut current = Some(site.scope);
    let mut left = info.scopes.len();
    while let Some(index) = current {
        if left == 0 {
            break;
        }
        left -= 1;
        let Some(scope) = info.scopes.get(index as usize) else {
            break;
        };
        let at_var = site.var_scope == Some(index);
        if let Some(entry) = find_eval_name(&scope.names, name) {
            if at_var && !site.params {
                if entry.kind.is_lexical() {
                    walk.conflict = true;
                } else {
                    walk.var_slot = Some(entry.slot);
                }
            } else if inner && entry.kind != BindingKind::CatchParameter {
                // Annex B.3.4: a catch parameter neither blocks the var nor
                // the hoisting of a block function (§19.2.1.3 steps 3.d
                // and 13.b as B.3.4 replaces them). The replaced step
                // exempts every catch clause, also one with a pattern
                // (`catch ({ e }) { eval('var e'); }`); V8 throws a
                // `SyntaxError` there, as the static rule of B.3.4 does for
                // a `var` in the block. test262 has no case; swb follows
                // the text.
                walk.conflict = true;
            }
        }
        if at_var {
            inner = false;
        }
        current = scope.parent;
    }
    if info.eval_code
        && inner
        && let Some(entry) = find_eval_name(&info.outer, name)
    {
        walk.conflict |= entry.walk.conflict;
        walk.var_slot = walk.var_slot.or(entry.walk.var_slot);
    }
    walk
}

/// Every name that a call can see, each once (for eval code that calls
/// `eval` itself).
fn all_names(info: &EvalInfo, site: &EvalSite) -> Vec<Gc<JsString>> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    let _ = for_each_scope(info, site, |scope| {
        for entry in &scope.names {
            if seen.insert((entry.name.index(), entry.name.generation())) {
                names.push(entry.name);
            }
        }
        Ok(())
    });
    for entry in &info.outer {
        if seen.insert((entry.name.index(), entry.name.generation())) {
            names.push(entry.name);
        }
    }
    names
}

/// The global `eval(x)` (§19.2.1).
pub(crate) fn eval_function(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    rt.indirect_eval(call)
}
