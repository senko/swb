//! The listing and the verifier of bytecode: [`disassemble`] prints a
//! function's code, [`verify`] checks the invariants that the interpreter
//! relies on (ADR 0026 section 4).

use std::rc::Rc;

use super::{
    COERCIBLE_READ, CodeKind, Constant, DELEGATE_INTERMEDIATE, EvalInfo, EvalName, EvalSlot,
    EvalVarEnv, FunctionCode, HINT_NO_TEXT, HINT_PATTERN, HINT_SHIFT, Handler, INVALID_TARGET_FOR,
    ITER_YIELD_STAR, Insn, MAX_REGISTERS, NAME_PREFIX_SET, Reg,
};
use crate::string::PropertyKey;

/// A listing of a function's code and of its nested functions, in the
/// order of their constants: one instruction per line with its position.
pub(crate) fn disassemble(code: &Rc<FunctionCode>, heap: &crate::heap::Heap) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let mut work = vec![Rc::clone(code)];
    while let Some(code) = work.pop() {
        let name = heap.string_lossy(code.name, usize::MAX);
        let _ = writeln!(
            out,
            "function {name:?}: {:?}, {} registers, {} parameters",
            code.kind, code.register_count, code.param_count
        );
        for (pc, insn) in code.insns.iter().enumerate() {
            let _ = writeln!(out, "  {pc:4} {insn:?}");
        }
        let mut children: Vec<Rc<FunctionCode>> = code
            .constants
            .iter()
            .filter_map(|c| match c {
                Constant::Function(_, child) => Some(Rc::clone(child)),
                Constant::Value(_)
                | Constant::Template(_)
                | Constant::PrivateNames(_)
                | Constant::RegExp(_) => None,
            })
            .collect();
        children.reverse();
        work.extend(children);
    }
    out
}

/// A failed check of the verifier: an engine bug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerifyError {
    /// The instruction.
    pub(crate) pc: usize,
    /// What is wrong.
    pub(crate) message: &'static str,
}

/// Checks a function's code after code generation (ADR 0026 section 4):
/// register operands below the register count, call windows inside the
/// frame, jump targets inside the code, constant, site and capture
/// indices inside their tables with the right kinds, a last instruction
/// that does not fall off the end, and a consistent handler table.
pub(crate) fn verify(code: &FunctionCode) -> Result<(), VerifyError> {
    let len = code.insns.len();
    let registers = code.register_count;
    if registers > MAX_REGISTERS || code.param_count > registers {
        return Err(VerifyError {
            pc: 0,
            message: "register count",
        });
    }
    if !code.insns.last().is_some_and(Insn::is_terminator) {
        return Err(VerifyError {
            pc: len,
            message: "the code can run past its end",
        });
    }
    let mut previous_end = 0;
    for &(start, end) in &code.strict_ranges {
        if start < previous_end || end < start || end as usize > len {
            return Err(VerifyError {
                pc: start as usize,
                message: "strict ranges",
            });
        }
        previous_end = end;
    }
    for (pc, insn) in code.insns.iter().enumerate() {
        let fail = |message| Err(VerifyError { pc, message });
        let mut ok = true;
        for_each_register(insn, |r| ok &= u32::from(r) < registers);
        if !ok {
            return fail("register operand out of range");
        }
        if let Err(message) = check_tables(code, insn) {
            return fail(message);
        }
        if let Err(message) = check_code_kind(code, insn).and_then(|()| check_layout(code, insn)) {
            return fail(message);
        }
        if let Some(offset) = insn.jump_offset() {
            let target = pc as i64 + i64::from(offset);
            if target < 0 || target >= len as i64 {
                return fail("jump target outside the code");
            }
        }
    }
    for global in &code.globals {
        if code.string(global.name).is_none() {
            return Err(VerifyError {
                pc: 0,
                message: "global declaration name",
            });
        }
    }
    if let Some(info) = &code.eval {
        verify_eval_info(code, info)?;
    }
    verify_handlers(code)
}

/// The instructions of generators and async code appear only in such
/// code.
fn check_code_kind(code: &FunctionCode, insn: &Insn) -> Result<(), &'static str> {
    match *insn {
        Insn::InitialYield
            if !matches!(code.kind, CodeKind::Generator | CodeKind::AsyncGenerator) =>
        {
            Err("yield outside a generator")
        }
        Insn::Yield { .. }
        | Insn::DelegateCall { .. }
        | Insn::DelegateResult { .. }
        | Insn::YieldDelegate { .. }
            if code.kind != CodeKind::Generator =>
        {
            Err("yield outside a generator")
        }
        Insn::AsyncYield { .. }
        | Insn::AsyncYieldDelegate { .. }
        | Insn::AsyncDelegateCall { .. }
        | Insn::AsyncDelegateResult { .. }
            if code.kind != CodeKind::AsyncGenerator =>
        {
            Err("yield outside an async generator")
        }
        Insn::AsyncStart if code.kind != CodeKind::Async => Err("await outside an async function"),
        Insn::Await { .. }
        | Insn::AsyncReject { .. }
        | Insn::GetAsyncIterator { .. }
        | Insn::AsyncIteratorCall { .. }
        | Insn::AsyncClose { .. }
        | Insn::AsyncCloseEnd { .. }
            if !code.kind.is_async() =>
        {
            Err("await outside an async function")
        }
        Insn::Resume { .. }
            if !matches!(
                code.kind,
                CodeKind::Generator | CodeKind::Async | CodeKind::AsyncGenerator
            ) =>
        {
            Err("a resumption outside a generator or async function")
        }
        _ => Ok(()),
    }
}

/// The registers that an instruction uses beyond its operands: call
/// windows, register pairs, iterator records, the resume mode.
fn check_layout(code: &FunctionCode, insn: &Insn) -> Result<(), &'static str> {
    let registers = code.register_count;
    let window = |callee: Reg, args: u32| u32::from(callee) + 2 + args <= registers;
    let ok = match *insn {
        Insn::Call { callee, argc }
        | Insn::New { callee, argc }
        | Insn::CallEval { callee, argc } => window(callee, u32::from(argc)),
        Insn::CallSpread { callee }
        | Insn::NewSpread { callee }
        | Insn::CallEvalSpread { callee } => window(callee, 1),
        // The active function is below the window.
        Insn::SuperCall { callee, argc } => callee != 0 && window(callee, u32::from(argc)),
        Insn::SuperCallSpread { callee } => callee != 0 && window(callee, 1),
        // Pairs of registers: the parents and the class, the super base
        // and `this`.
        Insn::ClassHeritage { dst: first, .. }
        | Insn::NewClass { dst: first, .. }
        | Insn::GetSuper { base: first, .. }
        | Insn::GetSuperNamed { base: first, .. }
        | Insn::SetSuper { base: first, .. }
        | Insn::SetSuperNamed { base: first, .. }
            if u32::from(first) + 2 > registers =>
        {
            return Err("register pair outside the frame");
        }
        // An iterator record takes three registers.
        Insn::GetIterator { dst: rec, .. }
        | Insn::GetAsyncIterator { dst: rec, .. }
        | Insn::IteratorCall { rec, .. }
        | Insn::AsyncIteratorCall { rec, .. }
        | Insn::AsyncClose { rec, .. }
        | Insn::SpreadArray { rec, .. }
        | Insn::IteratorValue { rec, .. }
        | Insn::IteratorClose { rec, .. }
            if u32::from(rec) + 3 > registers =>
        {
            return Err("iterator record outside the frame");
        }
        // The resume mode goes into the register after `reg`.
        Insn::Yield { reg }
        | Insn::Resume { reg, .. }
        | Insn::YieldDelegate { reg }
        | Insn::Await { reg }
        | Insn::AsyncYield { reg }
        | Insn::AsyncYieldDelegate { reg }
        | Insn::AsyncCloseEnd { reg, .. }
        | Insn::AsyncClose { dst: reg, .. }
            if u32::from(reg) + 1 >= registers =>
        {
            return Err("resume mode register out of range");
        }
        // An async `yield*` record takes six registers.
        Insn::AsyncDelegateCall { rec, form }
            if u32::from(rec) + 6 > registers || form > DELEGATE_INTERMEDIATE =>
        {
            return Err("yield* record outside the frame");
        }
        Insn::AsyncDelegateResult { rec, .. } if u32::from(rec) + 6 > registers => {
            return Err("yield* record outside the frame");
        }
        // A `yield*` record takes four registers.
        Insn::DelegateCall { rec, form, .. }
            if u32::from(rec) + 4 > registers || form > DELEGATE_INTERMEDIATE =>
        {
            return Err("yield* record outside the frame");
        }
        Insn::DelegateResult { rec, .. } if u32::from(rec) + 4 > registers => {
            return Err("yield* record outside the frame");
        }
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err("call window outside the frame")
    }
}

/// The scope description of a function with a direct `eval`: slots inside
/// the frame and the captures, scope links inside the table, calls at
/// `CallEval` instructions.
fn verify_eval_info(code: &FunctionCode, info: &EvalInfo) -> Result<(), VerifyError> {
    let slot_ok = |slot: EvalSlot| match slot {
        EvalSlot::Cell(r) => u32::from(r) < code.register_count,
        EvalSlot::Capture(i) => (i as usize) < code.captures.len(),
    };
    let name_ok = |name: &EvalName| slot_ok(name.slot) && name.walk.var_slot.is_none_or(slot_ok);
    let scope_ok = |index: Option<u32>| index.is_none_or(|i| (i as usize) < info.scopes.len());
    let scopes_ok = info.scopes.iter().all(|scope| {
        scope_ok(scope.parent)
            && scope.names.iter().all(name_ok)
            && scope.record.is_none_or(slot_ok)
    });
    let outer_ok = info.outer.iter().all(name_ok) && info.outer_record.is_none_or(slot_ok);
    let sites_ok = info.sites.iter().all(|site| {
        scope_ok(Some(site.scope))
            && scope_ok(site.var_scope)
            && matches!(
                code.insns.get(site.pc as usize),
                Some(Insn::CallEval { .. } | Insn::CallEvalSpread { .. })
            )
            && match site.var_env {
                EvalVarEnv::Record(slot) => slot_ok(slot),
                EvalVarEnv::None | EvalVarEnv::Global => true,
            }
    });
    if scopes_ok && outer_ok && sites_ok {
        Ok(())
    } else {
        Err(VerifyError {
            pc: 0,
            message: "eval scope description",
        })
    }
}

/// The constant, name, site and capture indices of an instruction: inside
/// their tables, with the right kinds; and the prefix of
/// `SetFunctionName`.
fn check_tables(code: &FunctionCode, insn: &Insn) -> Result<(), &'static str> {
    let constant = |index: u32| code.constants.get(index as usize);
    let (ok, message) = match *insn {
        Insn::Const { index, .. } => (
            matches!(constant(index), Some(Constant::Value(_))),
            "constant index",
        ),
        Insn::Closure { index, .. }
        | Insn::MethodClosure { index, .. }
        | Insn::NewClass { index, .. } => (
            matches!(constant(index), Some(Constant::Function(..))),
            "function constant index",
        ),
        Insn::GetTemplate { index, .. } => (
            matches!(constant(index), Some(Constant::Template(_))),
            "template constant index",
        ),
        Insn::NewRegExp { index, .. } => (
            matches!(constant(index), Some(Constant::RegExp(_))),
            "regular expression constant index",
        ),
        Insn::NewPrivateEnv { index, .. } => (
            matches!(constant(index), Some(Constant::PrivateNames(_))),
            "private names constant index",
        ),
        Insn::SetFunctionName { prefix, .. } => (prefix <= NAME_PREFIX_SET, "function name prefix"),
        Insn::ThrowInvalidAssign { context } => {
            (context <= INVALID_TARGET_FOR, "invalid target context")
        }
        Insn::GetIterator { hint, .. } => {
            let text = hint & HINT_NO_TEXT;
            (
                (hint & !HINT_PATTERN) >> HINT_SHIFT <= ITER_YIELD_STAR
                    && (text == HINT_NO_TEXT || code.string(text).is_some()),
                "iterator hint",
            )
        }
        Insn::RequireObjectCoercible { form, message, .. } => (
            form <= COERCIBLE_READ && code.string(message).is_some(),
            "destructuring message",
        ),
        Insn::CheckTdz { name, .. }
        | Insn::TypeofGlobal { name, .. }
        | Insn::InitGlobalLexical { name, .. }
        | Insn::InitGlobalFunction { name, .. }
        | Insn::BlockFunctionHoisted { name, .. }
        | Insn::DeleteGlobal { name, .. }
        | Insn::FindBinding { name, .. }
        | Insn::GetBinding { name, .. }
        | Insn::SetBinding { name, .. }
        | Insn::DeleteBinding { name, .. }
        | Insn::SetVarBinding { name, .. }
        | Insn::ThrowConstAssign { name } => (code.string(name).is_some(), "name constant index"),
        Insn::GetGlobal { site, .. } | Insn::SetGlobal { site, .. } => (
            matches!(code.sites.get(site as usize), Some(PropertyKey::String(_))),
            "global site index",
        ),
        Insn::NewLiteral { site, count, .. } => (
            (site as usize) < code.sites.len()
                && site as usize + usize::from(count) <= code.sites.len(),
            "literal site range",
        ),
        Insn::GetNamed { site, .. }
        | Insn::SetNamed { site, .. }
        | Insn::DeleteNamed { site, .. }
        | Insn::DefineNamed { site, .. }
        | Insn::DefineFieldNamed { site, .. }
        | Insn::GetSuperNamed { site, .. }
        | Insn::SetSuperNamed { site, .. } => ((site as usize) < code.sites.len(), "site index"),
        Insn::LoadCapture { index, .. } | Insn::StoreCapture { index, .. } => {
            ((index as usize) < code.captures.len(), "capture index")
        }
        _ => (true, ""),
    };
    if ok { Ok(()) } else { Err(message) }
}

/// The handler table: ranges inside the code, targets inside the code,
/// registers inside the frame, sorted by start, and two ranges either
/// disjoint or nested.
fn verify_handlers(code: &FunctionCode) -> Result<(), VerifyError> {
    let len = code.insns.len() as u32;
    let mut open: Vec<&Handler> = Vec::new();
    let mut previous_start = 0;
    for handler in &code.handlers {
        let fail = |message| {
            Err(VerifyError {
                pc: handler.start as usize,
                message,
            })
        };
        if handler.start >= handler.end || handler.end > len || handler.target >= len {
            return fail("handler range or target outside the code");
        }
        if u32::from(handler.register) >= code.register_count {
            return fail("handler register out of range");
        }
        if handler.start < previous_start {
            return fail("handler table not sorted");
        }
        previous_start = handler.start;
        while open.last().is_some_and(|outer| outer.end <= handler.start) {
            open.pop();
        }
        if open.last().is_some_and(|outer| handler.end > outer.end) {
            return fail("handler ranges overlap without nesting");
        }
        open.push(handler);
    }
    Ok(())
}

/// Calls `f` with every register operand of an instruction.
#[allow(clippy::too_many_lines, reason = "one arm per operand layout")]
fn for_each_register(insn: &Insn, mut f: impl FnMut(Reg)) {
    use Insn as I;
    match *insn {
        I::Undefined { dst }
        | I::Null { dst }
        | I::Bool { dst, .. }
        | I::Int { dst, .. }
        | I::Const { dst, .. }
        | I::Empty { dst }
        | I::NewCell { dst, .. }
        | I::LoadCapture { dst, .. }
        | I::This { dst }
        | I::Callee { dst }
        | I::Arguments { dst, .. }
        | I::NewTarget { dst }
        | I::GetTemplate { dst, .. }
        | I::NewRegExp { dst, .. }
        | I::GetGlobal { dst, .. }
        | I::TypeofGlobal { dst, .. }
        | I::BlockFunctionHoisted { dst, .. }
        | I::DeleteGlobal { dst, .. }
        | I::NewObject { dst }
        | I::NewLiteral { dst, .. }
        | I::NewArray { dst, .. }
        | I::RestArguments { dst, .. }
        | I::Closure { dst, .. }
        | I::HomeObject { dst }
        | I::NewClass { dst, .. }
        | I::FieldKey { dst, .. }
        | I::CheckThis { src: dst }
        | I::CheckSuperOnce { src: dst }
        | I::SuperCall { callee: dst, .. }
        | I::SuperCallSpread { callee: dst }
        | I::NewPrivateEnv { dst, .. }
        | I::LoadPrivateEnv { dst } => f(dst),
        I::PrivateMethod { env, src, .. }
        | I::PrivateGetter { env, src, .. }
        | I::PrivateSetter { env, src, .. }
        | I::PrivateClass { env, func: src }
        | I::SetPrivateEnv { func: src, env } => {
            f(env);
            f(src);
        }
        I::GetPrivate {
            dst: a,
            obj: b,
            env,
            ..
        }
        | I::SetPrivate {
            obj: a,
            src: b,
            env,
            ..
        }
        | I::HasPrivate {
            dst: a,
            obj: b,
            env,
            ..
        }
        | I::DefinePrivateField {
            obj: a,
            src: b,
            env,
            ..
        } => {
            f(a);
            f(b);
            f(env);
        }
        I::MethodClosure { dst, home, .. } => {
            f(dst);
            f(home);
        }
        I::MakeCell { reg }
        | I::CopyCell { reg }
        | I::Yield { reg }
        | I::Await { reg }
        | I::Resume { reg, .. }
        | I::YieldDelegate { reg }
        | I::DelegateCall { rec: reg, .. }
        | I::DelegateResult { rec: reg, .. }
        | I::AsyncYield { reg }
        | I::AsyncYieldDelegate { reg }
        | I::AsyncDelegateCall { rec: reg, .. }
        | I::AsyncDelegateResult { rec: reg, .. } => {
            f(reg);
        }
        I::AsyncClose { rec, kind, dst, .. } => {
            f(rec);
            f(kind);
            f(dst);
        }
        I::CheckTdz { src, .. }
        | I::StoreCapture { src, .. }
        | I::SetGlobal { src, .. }
        | I::InitGlobalLexical { src, .. }
        | I::InitGlobalFunction { src, .. }
        | I::Return { src }
        | I::AsyncReject { src }
        | I::Throw { src }
        | I::JumpIfNotNullish { src, .. }
        | I::JumpIfNullish { src, .. }
        | I::JumpIfNotUndefined { src, .. }
        | I::RequireObjectCoercible { src, .. } => f(src),
        I::JumpIfTrue { cond, .. } | I::JumpIfFalse { cond, .. } => f(cond),
        I::Hole { array } => f(array),
        I::Call { callee, .. }
        | I::New { callee, .. }
        | I::CallSpread { callee }
        | I::NewSpread { callee }
        | I::CallEval { callee, .. }
        | I::CallEvalSpread { callee } => f(callee),
        I::Move { dst, src }
        | I::ClassHeritage { dst, src }
        | I::SetFieldKeys {
            func: dst,
            keys: src,
        }
        | I::SetClassFields { func: dst, src }
        | I::InitFields {
            this: dst,
            func: src,
        }
        | I::SuperConstructor { dst, func: src }
        | I::DerivedReturn { dst, this: src }
        | I::SuperBase { dst, home: src }
        | I::GetSuperNamed { dst, base: src, .. }
        | I::SetSuperNamed { base: dst, src, .. }
        | I::DefineFieldNamed { obj: dst, src, .. }
        | I::LoadCell { dst, cell: src }
        | I::ToKey { dst, src }
        | I::Neg { dst, src }
        | I::ToNumber { dst, src }
        | I::ToNumeric { dst, src }
        | I::BitNot { dst, src }
        | I::Not { dst, src }
        | I::Typeof { dst, src }
        | I::Inc { dst, src }
        | I::Dec { dst, src }
        | I::ToString { dst, src }
        | I::StoreCell { cell: dst, src }
        | I::EndFinally {
            kind: dst,
            value: src,
            ..
        }
        | I::Push { array: dst, src }
        | I::SetProto { obj: dst, src }
        | I::CopyDataProperties { obj: dst, src }
        | I::SetFunctionName {
            func: dst,
            key: src,
            ..
        }
        | I::ForInStart { dst, src }
        | I::ForInNext { dst, iter: src, .. }
        | I::GetIterator { dst, src, .. }
        | I::GetAsyncIterator { dst, src, .. }
        | I::AsyncIteratorCall { dst, rec: src }
        | I::AsyncCloseEnd {
            reg: dst,
            kind: src,
        }
        | I::IteratorCall { dst, rec: src }
        | I::IteratorValue { dst, rec: src, .. }
        | I::IteratorClose {
            rec: dst,
            kind: src,
        }
        | I::SpreadArray {
            array: dst,
            rec: src,
        }
        | I::GetNamed { dst, obj: src, .. }
        | I::SetNamed { obj: dst, src, .. }
        | I::DeleteNamed { dst, obj: src, .. }
        | I::DefineNamed { obj: dst, src, .. }
        | I::InitSlot { obj: dst, src, .. }
        | I::JumpIfNotLt { a: dst, b: src, .. }
        | I::JumpIfNotLe { a: dst, b: src, .. }
        | I::JumpIfNotGt { a: dst, b: src, .. }
        | I::JumpIfNotGe { a: dst, b: src, .. }
        | I::NewVarEnv { dst, outer: src }
        | I::FindBinding { dst, env: src, .. }
        | I::GetBinding {
            dst, record: src, ..
        }
        | I::SetBinding {
            record: dst, src, ..
        }
        | I::DeleteBinding {
            dst, record: src, ..
        }
        | I::WithBase { dst, record: src }
        | I::SetVarBinding {
            record: dst, src, ..
        } => {
            f(dst);
            f(src);
        }
        I::NewWithEnv { dst, obj, outer } => {
            f(dst);
            f(obj);
            f(outer);
        }
        I::GetIndex { dst, obj, key }
        | I::DeleteIndex { dst, obj, key }
        | I::In { dst, key, obj } => {
            f(dst);
            f(obj);
            f(key);
        }
        I::SetIndex { obj, key, src }
        | I::GetSuper {
            dst: obj,
            base: key,
            key: src,
        }
        | I::SetSuper {
            base: obj,
            key,
            src,
        }
        | I::DefineMethod { obj, key, src }
        | I::DefineClassGetter { obj, key, src }
        | I::DefineClassSetter { obj, key, src }
        | I::DefineField { obj, key, src }
        | I::CopyRest {
            dst: obj,
            src: key,
            excluded: src,
        }
        | I::DefineIndex { obj, key, src }
        | I::DefineGetter { obj, key, src }
        | I::DefineSetter { obj, key, src } => {
            f(obj);
            f(key);
            f(src);
        }
        I::Add { dst, a, b }
        | I::Sub { dst, a, b }
        | I::Mul { dst, a, b }
        | I::Div { dst, a, b }
        | I::Rem { dst, a, b }
        | I::Exp { dst, a, b }
        | I::BitAnd { dst, a, b }
        | I::BitOr { dst, a, b }
        | I::BitXor { dst, a, b }
        | I::Shl { dst, a, b }
        | I::Shr { dst, a, b }
        | I::UShr { dst, a, b }
        | I::Eq { dst, a, b }
        | I::Ne { dst, a, b }
        | I::StrictEq { dst, a, b }
        | I::StrictNe { dst, a, b }
        | I::Lt { dst, a, b }
        | I::Le { dst, a, b }
        | I::Gt { dst, a, b }
        | I::Ge { dst, a, b }
        | I::InstanceOf { dst, a, b } => {
            f(dst);
            f(a);
            f(b);
        }
        I::DeclareGlobals
        | I::Jump { .. }
        | I::ThrowConstAssign { .. }
        | I::ThrowInvalidAssign { .. }
        | I::ThrowSuperDelete
        | I::InitialYield
        | I::AsyncStart
        | I::Nop => {}
    }
}
