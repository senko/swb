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

mod expr;
mod function;
mod support;

use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;

use swb_js_syntax::{CaptureSource, FunctionId, Script};
use swb_js_text::RecursionBudget;
use swb_js_text::{Str16, String16};

use crate::bytecode::{
    CodeKind, CodeObject, Constant, FunctionCode, GlobalDecl, Insn, ThisMode, verify,
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
    pub(crate) kind: CodeKind,
    pub(crate) this_mode: ThisMode,
    pub(crate) strict: bool,
    pub(crate) uses_arguments: bool,
    pub(crate) name: Option<String16>,
    pub(crate) lines: Vec<(u32, u32)>,
    pub(crate) call_names: Vec<(u32, u32)>,
    /// The exception handlers, sorted.
    pub(crate) handlers: Vec<crate::bytecode::Handler>,
    /// The range of the function in the source.
    pub(crate) span: (u32, u32),
}

/// State shared by the functions of one script: the names that
/// `NamedEvaluation` gives anonymous functions (§8.4.5).
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) inferred_names: HashMap<FunctionId, String16>,
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
    budget: &mut RecursionBudget,
) -> CResult<(Gc<Generic>, Rc<FunctionCode>)> {
    let initial = *budget;
    let source = Rc::new(source.to_string16());
    let result = compile_functions(script, budget)
        .and_then(|(builders, session)| materialize(heap, script, builders, &session, &source));
    *budget = initial;
    result
}

/// Compiles every function of the script.
fn compile_functions(
    script: &Script,
    budget: &mut RecursionBudget,
) -> CResult<(Vec<Option<Builder>>, Session)> {
    if let Some((offset, construct)) = support::first_unsupported(script) {
        return Err(CompileError::Script {
            kind: ThrowKind::SyntaxError,
            message: format!("{} ({construct})", swb_js_syntax::messages::NOT_SUPPORTED).into(),
            offset,
        });
    }
    let mut session = Session::default();
    let mut builders = Vec::with_capacity(script.ast.function_count());
    for id in script.ast.function_ids() {
        let builder =
            function::FunctionCompiler::new(script, id, &mut session, budget).compile()?;
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

/// Creates the code objects in the heap, innermost functions first, in a
/// no-GC region.
fn materialize(
    heap: &mut Heap,
    script: &Script,
    mut builders: Vec<Option<Builder>>,
    session: &Session,
    source: &Rc<String16>,
) -> CResult<(Gc<Generic>, Rc<FunctionCode>)> {
    let depth = depths(script);
    let mut order: Vec<FunctionId> = script.ast.function_ids().collect();
    order.sort_by_key(|f| std::cmp::Reverse(depth.get(f.index()).copied().unwrap_or(0)));
    let saved = heap.no_gc_depth();
    heap.enter_no_gc();
    let mut done: Vec<Option<(Gc<Generic>, Rc<FunctionCode>)>> = vec![None; builders.len()];
    let result = (|| {
        for id in order {
            let builder = builders
                .get_mut(id.index())
                .and_then(Option::take)
                .ok_or(Error::invariant("a function compiled twice"))?;
            let name = session.inferred_names.get(&id).cloned();
            let code = finish_code(heap, builder, name, &done, source)?;
            let code = Rc::new(code);
            let handle = heap.alloc_generic(CodeObject(Rc::clone(&code)))?;
            if let Some(slot) = done.get_mut(id.index()) {
                *slot = Some((handle, code));
            }
        }
        done.get(script.top.index())
            .cloned()
            .flatten()
            .ok_or(CompileError::Heap(Error::invariant(
                "the script has no code",
            )))
    })();
    heap.restore_no_gc_depth(saved);
    result
}

/// Turns a builder into code: atoms for the texts, code objects for the
/// nested functions, keys for the sites; then verifies it.
fn finish_code(
    heap: &mut Heap,
    builder: Builder,
    inferred_name: Option<String16>,
    done: &[Option<(Gc<Generic>, Rc<FunctionCode>)>],
    source: &Rc<String16>,
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
                    .get(child.index())
                    .cloned()
                    .flatten()
                    .ok_or(Error::invariant("a nested function without code"))?;
                Constant::Function(handle, code)
            }
        });
    }
    let mut sites = Vec::with_capacity(builder.sites.len());
    for text in &builder.sites {
        sites.push(heap.key_from_str16(text.as_str16())?);
    }
    let name_text = builder.name.or(inferred_name).unwrap_or_default();
    let name = heap.intern(name_text.as_str16())?;
    let code = FunctionCode {
        insns: builder.insns.into(),
        constants: constants.into(),
        sites: sites.into(),
        captures: builder.captures.into(),
        globals: builder.globals.into(),
        register_count: builder.register_count,
        param_count: builder.param_count,
        kind: builder.kind,
        this_mode: builder.this_mode,
        strict: builder.strict,
        uses_arguments: builder.uses_arguments,
        name,
        lines: builder.lines.into(),
        handlers: builder.handlers.into(),
        call_names: builder.call_names.into(),
        source: Rc::clone(source),
        span: builder.span,
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
