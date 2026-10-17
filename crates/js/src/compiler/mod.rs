//! The bytecode compiler (ADR 0026 sections 3 and 4): from the AST and the
//! results of the scope analysis of `js-syntax` to code objects.
//!
//! Each function compiles on its own ([`function::FunctionCompiler`]) into
//! a [`Builder`]: instructions, a constant pool with texts and numbers,
//! property sites, the line table. Nothing touches the heap while the
//! functions compile. Then [`materialize`] creates the heap things, the
//! innermost functions first so that a parent's constant pool can refer
//! to its children's code objects, in a no-GC region (ADR 0026 section 3:
//! a collection cannot run during a compile). The verifier checks each
//! function before its code object exists.
//!
//! Register allocation: the parameters and local bindings have the
//! registers of the scope analysis; temporaries are allocated above them
//! in stack order while the compiler walks the tree. The total is checked
//! against the operand width (65,535).
//!
//! Recursion: the compiler recurses over the tree and charges the shared
//! recursion budget per level; left-associative chains of binary and
//! logical operators are compiled in a loop.

mod class;
mod debug_names;
mod dynamic;
pub(crate) mod eval;
mod expr;
mod function;
mod iterate;
mod params;
mod pattern;
mod positions;
mod support;

use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;

use swb_js_syntax::{BindingId, CaptureSource, ClassId, FunctionId, Script};
use swb_js_text::RecursionBudget;
use swb_js_text::{Str16, String16};

use crate::bytecode::{
    CodeKind, CodeObject, Constant, FunctionCode, GlobalDecl, Insn, PrivateKind, PrivateNameSpec,
    PrivateNames, TemplatePart, ThisMode, verify,
};
use crate::error::{Error, ThrowKind};
use crate::heap::{Gc, Generic, Heap};
use crate::value::Value;

/// A compile failure.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CompileError {
    /// An error that the script sees: `SyntaxError` or `RangeError`.
    Script {
        kind: ThrowKind,
        message: Cow<'static, str>,
        offset: u32,
    },
    /// A failure of the heap (limit, out of memory) or an engine bug.
    Heap(Error),
}

impl From<Error> for CompileError {
    fn from(error: Error) -> Self {
        CompileError::Heap(error)
    }
}

pub(crate) type CResult<T> = Result<T, CompileError>;

/// A constant before the heap things exist.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ConstSpec {
    String(String16),
    Number(f64),
    Function(FunctionId),
    /// The code of the default constructor of a class (made by the
    /// compiler, [`Session::default_constructors`]).
    DefaultConstructor(ClassId),
    /// The (cooked, raw) strings of a tagged template's parts.
    Template(Vec<(Option<String16>, String16)>),
    /// The private names of a class body (see
    /// [`crate::bytecode::PrivateNames`]): (name, kind, static).
    PrivateNames {
        class_name: String16,
        names: Vec<(String16, PrivateKind, bool)>,
    },
    /// A regular expression literal: the pattern text and the flags.
    RegExp(String16, swb_js_regexp::Flags),
}

/// The compiled form of one function before materialization.
#[derive(Debug)]
pub(crate) struct Builder {
    pub(crate) insns: Vec<Insn>,
    pub(crate) constants: Vec<ConstSpec>,
    pub(crate) sites: Vec<String16>,
    pub(crate) captures: Vec<CaptureSource>,
    pub(crate) globals: Vec<GlobalDecl>,
    pub(crate) register_count: u32,
    pub(crate) param_count: u32,
    pub(crate) length: u32,
    pub(crate) kind: CodeKind,
    pub(crate) this_mode: ThisMode,
    pub(crate) strict: bool,
    /// See [`crate::bytecode::FunctionCode::strict_ranges`].
    pub(crate) strict_ranges: Vec<(u32, u32)>,
    pub(crate) keeps_extra_args: bool,
    pub(crate) name: Option<String16>,
    pub(crate) lines: Vec<(u32, u32)>,
    pub(crate) call_names: Vec<(u32, u32)>,
    /// The exception handlers, sorted.
    pub(crate) handlers: Vec<crate::bytecode::Handler>,
    /// The range of the function in the source.
    pub(crate) span: (u32, u32),
    /// The scope description of a function with a direct `eval`.
    pub(crate) eval: Option<eval::EvalSpec>,
}

/// State shared by the functions of one script: the names that
/// `NamedEvaluation` gives anonymous functions (§8.4.5).
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) inferred_names: HashMap<FunctionId, String16>,
    /// The names that Chromium shows in stack traces for anonymous
    /// functions assigned to members (`a.b = function () {}` gives
    /// `a.b`); they do not change the `name` property.
    pub(crate) debug_names: HashMap<FunctionId, String16>,
    /// The names that `NamedEvaluation` gives anonymous classes.
    pub(crate) class_names: HashMap<ClassId, String16>,
    /// The code of the default constructors (classes without a
    /// `constructor` method).
    pub(crate) default_constructors: HashMap<ClassId, Builder>,
    /// The class of each constructor and field initializer function.
    pub(crate) function_classes: HashMap<FunctionId, ClassId>,
    /// The kind of each private name binding (for Chromium's positions).
    pub(crate) private_kinds: HashMap<BindingId, PrivateKind>,
    /// The private name bindings that an expression uses (`o.#x`,
    /// `#x in o`), for V8's name of an anonymous class.
    pub(crate) used_private_names: std::collections::HashSet<BindingId>,
    /// The scopes of each function with a direct `eval`.
    pub(crate) eval_scopes: HashMap<FunctionId, Vec<swb_js_syntax::ScopeId>>,
    /// Eval code: the captures of the caller's cells.
    pub(crate) caller: Option<eval::CallerPlan>,
}

impl Session {
    /// The session of a script: the classes of its class functions.
    fn new(script: &Script, input: Option<eval::EvalInput>) -> Self {
        let mut session = Session {
            eval_scopes: Session::eval_scopes(script),
            caller: input.map(|input| eval::CallerPlan::new(script, input)),
            ..Session::default()
        };
        for id in script.ast.class_ids() {
            let class = script.ast.class(id);
            for function in [class.constructor, class.instance_init, class.static_init]
                .into_iter()
                .flatten()
            {
                session.function_classes.insert(function, id);
            }
            for element in script.ast.class_elements(class.elements) {
                let swb_js_syntax::ClassKey::Private(ident) = element.key else {
                    continue;
                };
                if let Some(binding) = script.scopes.private_name(ident.reference) {
                    let kind = match element.kind {
                        swb_js_syntax::ClassElementKind::Field => PrivateKind::Field,
                        swb_js_syntax::ClassElementKind::Method => PrivateKind::Method,
                        _ => PrivateKind::Accessor,
                    };
                    session.private_kinds.insert(binding, kind);
                }
            }
        }
        for &(reference, binding) in script.scopes.private_references() {
            if !script.scopes.reference(reference).declaration {
                session.used_private_names.insert(binding);
            }
        }
        session
    }
}

/// Statistics of compiled code (for measurements).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompileStats {
    /// Functions, including the script.
    pub functions: usize,
    /// Instructions of all functions.
    pub instructions: usize,
    /// The size of one instruction in bytes.
    pub instruction_size: usize,
    /// Constant pool entries.
    pub constants: usize,
    /// Property sites.
    pub sites: usize,
    /// Line table entries.
    pub lines: usize,
    /// The bytes of all code objects (records, instructions and tables).
    pub bytes: usize,
}

impl CompileStats {
    /// The statistics of a script's code and all nested functions.
    pub(crate) fn of(code: &Rc<FunctionCode>) -> CompileStats {
        let mut stats = CompileStats {
            instruction_size: size_of::<Insn>(),
            ..CompileStats::default()
        };
        let mut work = vec![Rc::clone(code)];
        while let Some(code) = work.pop() {
            stats.functions += 1;
            stats.instructions += code.insns.len();
            stats.constants += code.constants.len();
            stats.sites += code.sites.len();
            stats.lines += code.lines.len();
            stats.bytes += size_of::<FunctionCode>() + code.heap_size();
            for constant in &code.constants {
                if let Constant::Function(_, child) = constant {
                    work.push(Rc::clone(child));
                }
            }
        }
        stats
    }
}

/// Compiles a parsed script. Returns the script's code object (not
/// rooted: the caller runs it or roots it before the next safepoint) and
/// its code. The recursion budget is restored when the compile ends.
pub(crate) fn compile_script(
    heap: &mut Heap,
    script: &Script,
    source: Str16<'_>,
    name: &str,
    budget: &mut RecursionBudget,
) -> CResult<(Gc<Generic>, Rc<FunctionCode>)> {
    let shared = Rc::new(crate::bytecode::Source::new(
        source.to_string16(),
        Rc::from(name),
    ));
    compile_code(heap, script, (source, shared), budget, None)
}

/// Compiles eval code (§19.2.1.1 `PerformEval`): its top-level function
/// is eval code (`CodeKind::Eval`); direct eval code reaches the bindings
/// of its caller through `input.caller` (`compiler/eval.rs`). `origin`
/// names the call in stack traces. Like [`compile_script`], the code
/// object is not rooted.
pub(crate) fn compile_eval(
    heap: &mut Heap,
    script: &Script,
    (source, origin): (Str16<'_>, Rc<str>),
    budget: &mut RecursionBudget,
    input: eval::EvalInput,
) -> CResult<(Gc<Generic>, Rc<FunctionCode>)> {
    let shared = Rc::new(crate::bytecode::Source::eval_code(
        source.to_string16(),
        origin,
    ));
    compile_code(heap, script, (source, shared), budget, Some(input))
}

/// Compiles the source of a `Function` constructor call (parsed by
/// `swb_js_syntax::parse_dynamic_function`) and returns the code of the
/// function that it makes (`Script::dynamic_function`; not rooted). The
/// script around the function is global code without declarations, so the
/// function captures nothing; its code object is garbage after the call.
pub(crate) fn compile_dynamic_function(
    heap: &mut Heap,
    script: &Script,
    (source, origin): (Str16<'_>, Rc<str>),
    budget: &mut RecursionBudget,
) -> CResult<(Gc<Generic>, Rc<FunctionCode>)> {
    let function = script
        .dynamic_function
        .ok_or(CompileError::Heap(Error::invariant(
            "a Function constructor source without its function",
        )))?;
    let shared = Rc::new(crate::bytecode::Source::dynamic_function(
        source.to_string16(),
        origin,
    ));
    let input = eval::EvalInput::default();
    let initial = *budget;
    let result =
        compile_functions(script, source, budget, Some(input)).and_then(|(builders, session)| {
            materialize(heap, script, builders, session, &shared, function)
        });
    *budget = initial;
    result
}

fn compile_code(
    heap: &mut Heap,
    script: &Script,
    (text, source): (Str16<'_>, Rc<crate::bytecode::Source>),
    budget: &mut RecursionBudget,
    input: Option<eval::EvalInput>,
) -> CResult<(Gc<Generic>, Rc<FunctionCode>)> {
    let initial = *budget;
    let result = compile_functions(script, text, budget, input).and_then(|(builders, session)| {
        materialize(heap, script, builders, session, &source, script.top)
    });
    *budget = initial;
    result
}

/// Compiles every function of the script.
fn compile_functions<'a>(
    script: &'a Script,
    text: Str16<'a>,
    budget: &mut RecursionBudget,
    input: Option<eval::EvalInput>,
) -> CResult<(Vec<Option<Builder>>, Session)> {
    if let Some((offset, construct)) = support::first_unsupported(script) {
        return Err(CompileError::Script {
            kind: ThrowKind::SyntaxError,
            message: format!("{} ({construct})", swb_js_syntax::messages::NOT_SUPPORTED).into(),
            offset,
        });
    }
    let mut session = Session::new(script, input);
    if session.caller.as_ref().is_some_and(|plan| plan.overflow) {
        return Err(CompileError::Script {
            kind: ThrowKind::RangeError,
            message: swb_js_syntax::messages::TOO_MANY_CAPTURES.into(),
            offset: 0,
        });
    }
    let mut builders = Vec::with_capacity(script.ast.function_count());
    for id in script.ast.function_ids() {
        let builder =
            function::FunctionCompiler::new(script, id, &mut session, budget, text).compile()?;
        builders.push(Some(builder));
    }
    Ok((builders, session))
}

/// The nesting depth of each function (the script is 0).
fn depths(script: &Script) -> Vec<u32> {
    let count = script.ast.function_count();
    let mut depth: Vec<Option<u32>> = vec![None; count];
    for id in script.ast.function_ids() {
        // Walk up to a function with a known depth, then fill in the
        // chain on the way down.
        let mut chain = Vec::new();
        let mut current = Some(id);
        let mut known = 0;
        while let Some(f) = current {
            if let Some(Some(d)) = depth.get(f.index()) {
                known = *d + 1;
                break;
            }
            chain.push(f);
            current = script.ast.function(f).parent;
        }
        if current.is_none() {
            known = 0;
        }
        for f in chain.into_iter().rev() {
            if let Some(slot) = depth.get_mut(f.index()) {
                *slot = Some(known);
            }
            known += 1;
        }
    }
    depth.into_iter().map(Option::unwrap_or_default).collect()
}

/// The code objects made so far: by function, and the default
/// constructors by class.
struct Done {
    functions: Vec<Option<(Gc<Generic>, Rc<FunctionCode>)>>,
    defaults: HashMap<ClassId, (Gc<Generic>, Rc<FunctionCode>)>,
}

/// Creates the code objects in the heap, innermost functions first, in a
/// no-GC region. Returns the code of `result` (the script, or the function
/// of a `Function` constructor source).
fn materialize(
    heap: &mut Heap,
    script: &Script,
    mut builders: Vec<Option<Builder>>,
    mut session: Session,
    source: &Rc<crate::bytecode::Source>,
    result: FunctionId,
) -> CResult<(Gc<Generic>, Rc<FunctionCode>)> {
    let depth = depths(script);
    let defaults = std::mem::take(&mut session.default_constructors);
    let mut order: Vec<FunctionId> = script.ast.function_ids().collect();
    order.sort_by_key(|f| std::cmp::Reverse(depth.get(f.index()).copied().unwrap_or(0)));
    let saved = heap.no_gc_depth();
    heap.enter_no_gc();
    let mut done = Done {
        functions: vec![None; builders.len()],
        defaults: HashMap::with_capacity(defaults.len()),
    };
    let result = (|| {
        // The default constructors have no nested functions.
        for (class, builder) in defaults {
            let code = Rc::new(finish_code(heap, builder, (None, None), &done, source)?);
            let handle = heap.alloc_generic(CodeObject::new(Rc::clone(&code)))?;
            done.defaults.insert(class, (handle, code));
        }
        for id in order {
            let builder = builders
                .get_mut(id.index())
                .and_then(Option::take)
                .ok_or(Error::invariant("a function compiled twice"))?;
            let name = session.inferred_names.get(&id).cloned();
            let debug_name = session.debug_names.get(&id).cloned();
            let code = finish_code(heap, builder, (name, debug_name), &done, source)?;
            let code = Rc::new(code);
            let handle = heap.alloc_generic(CodeObject::new(Rc::clone(&code)))?;
            if let Some(slot) = done.functions.get_mut(id.index()) {
                *slot = Some((handle, code));
            }
        }
        done.functions
            .get(result.index())
            .cloned()
            .flatten()
            .ok_or(CompileError::Heap(Error::invariant(
                "the script has no code",
            )))
    })();
    heap.restore_no_gc_depth(saved);
    result
}

/// The constant of a regular expression literal: the pattern as an atom;
/// the program is compiled at the first evaluation.
fn regexp_constant(
    heap: &mut Heap,
    pattern: &String16,
    flags: swb_js_regexp::Flags,
) -> CResult<Constant> {
    Ok(Constant::RegExp(Box::new(crate::bytecode::RegExpLiteral {
        source: heap.intern(pattern.as_str16())?,
        flags,
        program: std::cell::RefCell::new(None),
    })))
}

/// Turns a builder into code: atoms for the texts, code objects for the
/// nested functions, keys for the sites; then verifies it.
fn finish_code(
    heap: &mut Heap,
    builder: Builder,
    (inferred_name, debug_name): (Option<String16>, Option<String16>),
    done: &Done,
    source: &Rc<crate::bytecode::Source>,
) -> CResult<FunctionCode> {
    let mut constants = Vec::with_capacity(builder.constants.len());
    for constant in &builder.constants {
        constants.push(match constant {
            ConstSpec::String(text) => {
                Constant::Value(Value::String(heap.intern(text.as_str16())?))
            }
            ConstSpec::Number(x) => Constant::Value(Value::number(*x)),
            ConstSpec::Function(child) => {
                let (handle, code) = done
                    .functions
                    .get(child.index())
                    .cloned()
                    .flatten()
                    .ok_or(Error::invariant("a nested function without code"))?;
                Constant::Function(handle, code)
            }
            ConstSpec::DefaultConstructor(class) => {
                let (handle, code) = done
                    .defaults
                    .get(class)
                    .cloned()
                    .ok_or(Error::invariant("a default constructor without code"))?;
                Constant::Function(handle, code)
            }
            ConstSpec::Template(parts) => {
                let mut atoms = Vec::with_capacity(parts.len());
                for (cooked, raw) in parts {
                    let cooked = match cooked {
                        Some(text) => Some(heap.intern(text.as_str16())?),
                        None => None,
                    };
                    let raw = heap.intern(raw.as_str16())?;
                    atoms.push(TemplatePart { cooked, raw });
                }
                Constant::Template(atoms.into())
            }
            ConstSpec::PrivateNames { class_name, names } => {
                let mut specs = Vec::with_capacity(names.len());
                for (name, kind, is_static) in names {
                    specs.push(PrivateNameSpec {
                        description: heap.intern(name.as_str16())?,
                        kind: *kind,
                        is_static: *is_static,
                    });
                }
                Constant::PrivateNames(Box::new(PrivateNames {
                    class_name: heap.intern(class_name.as_str16())?,
                    names: specs.into(),
                }))
            }
            ConstSpec::RegExp(pattern, flags) => regexp_constant(heap, pattern, *flags)?,
        });
    }
    let mut sites = Vec::with_capacity(builder.sites.len());
    for text in &builder.sites {
        sites.push(heap.key_from_str16(text.as_str16())?);
    }
    let name_text = builder.name.or(inferred_name).unwrap_or_default();
    let name = heap.intern(name_text.as_str16())?;
    let debug_name = heap.intern(debug_name.unwrap_or_default().as_str16())?;
    let code = FunctionCode {
        insns: builder.insns.into(),
        constants: constants.into(),
        sites: sites.into(),
        captures: builder.captures.into(),
        globals: builder.globals.into(),
        register_count: builder.register_count,
        param_count: builder.param_count,
        length: builder.length,
        kind: builder.kind,
        this_mode: builder.this_mode,
        strict: builder.strict,
        strict_ranges: builder.strict_ranges.into(),
        keeps_extra_args: builder.keeps_extra_args,
        name,
        debug_name,
        lines: builder.lines.into(),
        handlers: builder.handlers.into(),
        call_names: builder.call_names.into(),
        source: Rc::clone(source),
        span: builder.span,
        eval: match builder.eval {
            Some(spec) => Some(Box::new(eval::materialize_eval(heap, spec)?)),
            None => None,
        },
    };
    if let Err(failure) = verify(&code) {
        log::warn!(
            "JavaScript compiler bug: the verifier rejected instruction {} ({}): {:?}",
            failure.pc,
            failure.message,
            code.insns.get(failure.pc)
        );
        return Err(Error::invariant("the bytecode verifier rejected the code").into());
    }
    Ok(code)
}
