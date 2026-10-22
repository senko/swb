//! Expressions: values into registers, effects, conditions.
//!
//! `expr(e, dst)` writes the value of `e` into `dst`, or returns a
//! register that holds it: a new temporary, or the register of a local
//! binding without a copy. Such a register is only used where no later
//! operand can assign the local (`expr_stable`). A local's register is a
//! direct destination only for expressions that write their destination
//! once, after reading all operands (`writes_target_last`).

use swb_js_syntax::messages::TOO_MANY_ARGUMENTS;
use swb_js_syntax::{
    AssignOp, AssignTarget, BinaryOp, BindingKind, ExprId, ExprKind, Ident, List, LogicalOp,
    PropertyKey as AstKey, PropertyKind, RefId, Resolution, UnaryOp, UpdateOp,
};
use swb_js_text::String16;

use super::CResult;
use super::function::{EXPR_WEIGHT, FunctionCompiler};
use crate::bytecode::{
    CodeKind, INVALID_TARGET_ASSIGN, INVALID_TARGET_FOR, INVALID_TARGET_POSTFIX,
    INVALID_TARGET_PREFIX, ITER_ARRAY_SPREAD, ITER_SPREAD, Insn, NAME_PREFIX_GET, NAME_PREFIX_NONE,
    NAME_PREFIX_SET, Reg,
};
use crate::vm::number::number_to_string;

/// How many nodes `may_write_locals` looks at before it gives up.
const SCAN_LIMIT: usize = 32;

/// The deepest callee that error messages render.
pub(super) const RENDER_DEPTH: u32 = 8;

impl FunctionCompiler<'_> {
    /// The expression without parentheses.
    pub(super) fn unparen(&self, mut e: ExprId) -> ExprId {
        while let ExprKind::Paren(inner) = self.ast.expr(e).kind {
            e = inner;
        }
        e
    }

    /// Compiles `e`; the value is in `dst` (if given) or in the returned
    /// register.
    pub(super) fn expr(&mut self, e: ExprId, dst: Option<Reg>) -> CResult<Reg> {
        self.enter(EXPR_WEIGHT)?;
        let node = *self.ast.expr(e);
        // The instructions of this node map to its start; the parent's
        // position comes back when the node is done.
        let outer = std::mem::replace(&mut self.position, node.span.start);
        let result = self.expr_kind(e, node.kind, dst)?;
        self.position = outer;
        self.leave(EXPR_WEIGHT);
        Ok(result)
    }

    #[allow(clippy::too_many_lines)]
    fn expr_kind(&mut self, e: ExprId, kind: ExprKind, dst: Option<Reg>) -> CResult<Reg> {
        match kind {
            ExprKind::Identifier(ident) => self.load_ref(ident.reference, dst),
            ExprKind::This(reference) | ExprKind::NewTarget(reference) => {
                self.load_ref(reference, dst)
            }
            ExprKind::Null => {
                let t = self.target(dst)?;
                self.emit(Insn::Null { dst: t });
                Ok(t)
            }
            ExprKind::Boolean(value) => {
                let t = self.target(dst)?;
                self.emit(Insn::Bool { dst: t, value });
                Ok(t)
            }
            ExprKind::Number(value) => {
                let t = self.target(dst)?;
                self.load_number(value, t);
                Ok(t)
            }
            ExprKind::String(id) => {
                let t = self.target(dst)?;
                let index = self.string_const(self.ast.string(id).clone());
                self.emit(Insn::Const { dst: t, index });
                Ok(t)
            }
            ExprKind::BigInt(digits) => {
                let t = self.target(dst)?;
                self.load_bigint(digits, false, t);
                Ok(t)
            }
            ExprKind::Template(id) => self.template(id, dst),
            ExprKind::RegExp { pattern, flags } => {
                let t = self.target(dst)?;
                let text = self.ast.string(pattern).clone();
                let index = self.push_constant(super::ConstSpec::RegExp(text, flags));
                self.emit(Insn::NewRegExp { dst: t, index });
                Ok(t)
            }
            ExprKind::Array(elements) => self.array(elements, dst),
            ExprKind::Hole => Err(Self::internal("a hole outside an array literal")),
            ExprKind::Object(properties) => self.object(properties, dst),
            ExprKind::Function(function) => {
                let t = self.target(dst)?;
                let index = self.function_const(function);
                self.emit(Insn::Closure { dst: t, index });
                Ok(t)
            }
            ExprKind::Paren(inner) => self.expr(inner, dst),
            ExprKind::Unary { op, argument } => self.unary(op, argument, dst),
            ExprKind::Update { op, prefix, target } => self.update(op, prefix, target, dst, false),
            ExprKind::Binary { .. } => self.binary(e, dst),
            ExprKind::Logical { op, .. } => self.logical(e, op, dst),
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let t = self.target(dst)?;
                let exits = self.branch_false(test)?;
                self.expr(consequent, Some(t))?;
                let skip = self.emit(Insn::Jump { offset: 0 });
                for exit in exits {
                    self.patch_here(exit);
                }
                self.expr(alternate, Some(t))?;
                self.patch_here(skip);
                Ok(t)
            }
            ExprKind::Assign { op, target, value } => self.assign(op, target, value, dst),
            ExprKind::Sequence(list) => {
                let items = self.ast.exprs(list);
                let Some((&last, rest)) = items.split_last() else {
                    return Err(Self::internal("an empty sequence"));
                };
                for &item in rest {
                    self.effect(item)?;
                }
                self.expr(last, dst)
            }
            ExprKind::Member {
                object,
                property,
                optional,
            } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                let obj = self.expr(object, None)?;
                if optional {
                    self.chain_jump(obj)?;
                }
                let site = self.site(self.name_text(property));
                self.position = self.read_position(e);
                self.emit(Insn::GetNamed { dst: t, obj, site });
                self.release(mark);
                Ok(t)
            }
            ExprKind::Index {
                object,
                index,
                optional,
            } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                if let Some(text) = self.constant_key(index) {
                    let obj = self.expr(object, None)?;
                    if optional {
                        self.chain_jump(obj)?;
                    }
                    let site = self.site(text);
                    self.position = self.bracket_position(e);
                    self.emit(Insn::GetNamed { dst: t, obj, site });
                } else {
                    let obj = self.expr_stable(object, &[index])?;
                    if optional {
                        self.chain_jump(obj)?;
                    }
                    let key = self.expr(index, None)?;
                    self.position = self.bracket_position(e);
                    self.emit(Insn::GetIndex { dst: t, obj, key });
                }
                self.release(mark);
                Ok(t)
            }
            ExprKind::PrivateMember {
                object,
                name,
                optional,
            } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                let obj = self.expr(object, None)?;
                if optional {
                    self.chain_jump(obj)?;
                }
                let key = self.private_key(name)?;
                self.position = self.private_read_position(e);
                self.emit_get(t, obj, key);
                self.release(mark);
                Ok(t)
            }
            ExprKind::PrivateIn { name, object } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                let obj = self.expr(object, None)?;
                let KeyReg::Private { env, index } = self.private_key(name)? else {
                    return Err(Self::internal("a private key that is not private"));
                };
                // Chromium's position: the `in`.
                let name_end =
                    self.reference(name.reference).offset + self.name_text(name.name).len() as u32;
                self.position = self.token_position(name_end);
                self.emit(Insn::HasPrivate {
                    dst: t,
                    obj,
                    env,
                    index,
                });
                self.release(mark);
                Ok(t)
            }
            ExprKind::Call {
                callee,
                arguments,
                optional,
            } => self.call(
                callee,
                CallArgs::List(arguments),
                dst,
                CallKind::Call { optional },
            ),
            ExprKind::New { callee, arguments } => {
                self.call(callee, CallArgs::List(arguments), dst, CallKind::New)
            }
            ExprKind::TaggedTemplate { tag, template } => self.call(
                tag,
                CallArgs::Template(template),
                dst,
                CallKind::Call { optional: false },
            ),
            ExprKind::OptionalChain(inner) => self.optional_chain(inner, dst),
            ExprKind::Spread(_) => Err(Self::internal("a spread outside a list")),
            ExprKind::Class(class) => self.class_definition(class, dst, None),
            ExprKind::SuperMember { .. } | ExprKind::SuperIndex { .. } => self.super_load(e, dst),
            ExprKind::SuperCall(call) => self.super_call(call, dst),
            ExprKind::ImportCall { .. } | ExprKind::ImportMeta => {
                Err(self
                    .unsupported(super::support::unsupported_expr(kind).unwrap_or("expression")))
            }
            ExprKind::Await(argument) => self.await_expr(argument, dst),
            ExprKind::Yield {
                argument: Some(argument),
                delegate: true,
            } => self.yield_star(argument, self.ast.expr(e).span.start, dst),
            ExprKind::Yield { argument, .. } => {
                // `t` receives the sent value, `t + 1` the resume mode.
                let t = self.temps(2)?;
                match argument {
                    Some(argument) => {
                        self.expr(argument, Some(t))?;
                    }
                    None => {
                        self.emit(Insn::Undefined { dst: t });
                    }
                }
                if self.kind == CodeKind::AsyncGenerator {
                    // §27.6.3.8 via `Yield`: `AsyncGeneratorYield(?
                    // Await(value))`; the value of a return request is
                    // awaited before the resumption.
                    self.emit_await(t);
                    self.emit(Insn::AsyncYield { reg: t });
                } else {
                    self.emit(Insn::Yield { reg: t });
                }
                // `next` skips the return path; `throw` throws here, inside
                // the handler ranges of the `yield`.
                let resume = self.emit(Insn::Resume { reg: t, offset: 0 });
                self.emit_return(t);
                self.patch_here(resume);
                self.release(u32::from(t) + 1);
                match dst {
                    Some(dst) => {
                        self.emit(Insn::Move { dst, src: t });
                        Ok(dst)
                    }
                    None => Ok(t),
                }
            }
        }
    }

    /// `await argument` (§15.8.5, §27.7.5.3 Await): the value goes into
    /// `t`; the resumption writes the result into `t` and the mode into
    /// `t + 1`, and `Resume` throws a rejection at the `await` (inside its
    /// handler ranges) or continues. The instructions map to the `await`
    /// keyword, where Chromium shows a waiting function in a stack.
    fn await_expr(&mut self, argument: ExprId, dst: Option<Reg>) -> CResult<Reg> {
        let t = self.temps(2)?;
        self.expr(argument, Some(t))?;
        self.emit_await(t);
        self.release(u32::from(t) + 1);
        match dst {
            Some(dst) => {
                self.emit(Insn::Move { dst, src: t });
                Ok(dst)
            }
            None => Ok(t),
        }
    }

    /// `Await` of the value in `t` (a temporary pair): afterwards `t` holds
    /// the value of the fulfilled promise; a rejection throws here.
    pub(super) fn emit_await(&mut self, t: Reg) {
        self.emit(Insn::Await { reg: t });
        self.emit(Insn::Resume { reg: t, offset: 1 });
    }

    /// Compiles `e` for its effects.
    pub(super) fn effect(&mut self, e: ExprId) -> CResult<()> {
        let mark = self.mark();
        match self.ast.expr(e).kind {
            ExprKind::Paren(inner) => self.effect(inner)?,
            ExprKind::Update { op, prefix, target } => {
                self.enter(EXPR_WEIGHT)?;
                self.position = self.ast.expr(e).span.start;
                self.update(op, prefix, target, None, true)?;
                self.leave(EXPR_WEIGHT);
            }
            ExprKind::Sequence(list) => {
                for &item in self.ast.exprs(list) {
                    self.effect(item)?;
                }
            }
            _ => {
                self.expr(e, None)?;
            }
        }
        self.release(mark);
        Ok(())
    }

    /// Compiles `e` with `NamedEvaluation` (§8.4.5): an anonymous function
    /// definition gets `name`.
    pub(super) fn expr_named(
        &mut self,
        e: ExprId,
        dst: Option<Reg>,
        name: String16,
    ) -> CResult<Reg> {
        let inner = self.unparen(e);
        if let ExprKind::Function(function) = self.ast.expr(inner).kind
            && self.ast.function(function).name.is_none()
        {
            self.session.inferred_names.insert(function, name);
        } else if let ExprKind::Class(class) = self.ast.expr(inner).kind
            && self.ast.class(class).name.is_none()
        {
            self.session.class_names.insert(class, name);
        } else {
            self.note_array_functions(inner, &name);
        }
        self.expr(e, dst)
    }

    /// Loads a number into `dst`: an integer instruction where possible.
    fn load_number(&mut self, value: f64, dst: Reg) {
        let int = value as i32;
        if f64::from(int) == value && !(int == 0 && value.is_sign_negative()) {
            self.emit(Insn::Int { dst, value: int });
        } else {
            let index = self.number_const(value);
            self.emit(Insn::Const { dst, index });
        }
    }

    /// Loads the value of a `BigInt` literal (negated with `negative`)
    /// into `dst`: a constant of the code object.
    fn load_bigint(&mut self, digits: swb_js_syntax::StringId, negative: bool, dst: Reg) {
        let text = self.ast.string(digits).clone();
        let index = self.push_constant(super::ConstSpec::BigInt(text, negative));
        self.emit(Insn::Const { dst, index });
    }

    // --- Bindings ---

    /// Loads the binding of an identifier occurrence: through the dynamic
    /// environments in its reach first (`compiler/dynamic.rs`), then from
    /// its static binding.
    pub(super) fn load_ref(&mut self, id: RefId, dst: Option<Reg>) -> CResult<Reg> {
        let Some(dynamic) = self.dynamic_of(id) else {
            return self.load_static(id, dst);
        };
        let t = self.target(dst)?;
        let mark = self.mark();
        let rec = self.temp()?;
        self.find_record(id, dynamic, rec)?;
        self.load_found(id, Some(rec), t)?;
        self.release(mark);
        Ok(t)
    }

    /// Loads the static binding of an identifier occurrence.
    pub(super) fn load_static(&mut self, id: RefId, dst: Option<Reg>) -> CResult<Reg> {
        let reference = self.reference(id);
        let name = || reference.name;
        match reference.resolution {
            Resolution::Register(r) => {
                if reference.tdz_check {
                    self.check_register_tdz(&reference, r)?;
                }
                if self.is_derived_this(&reference) {
                    self.emit(Insn::CheckThis { src: r });
                }
                match dst {
                    Some(dst) if dst != r => {
                        self.emit(Insn::Move { dst, src: r });
                        Ok(dst)
                    }
                    _ => Ok(r),
                }
            }
            Resolution::Cell(cell) => {
                let t = self.target(dst)?;
                self.emit(Insn::LoadCell { dst: t, cell });
                self.check_tdz(&reference, t);
                Ok(t)
            }
            Resolution::Capture(index) => {
                let t = self.target(dst)?;
                self.emit(Insn::LoadCapture { dst: t, index });
                self.check_tdz(&reference, t);
                Ok(t)
            }
            Resolution::Global => {
                let t = self.target(dst)?;
                let site = self.site(self.name_text(name()));
                self.emit(Insn::GetGlobal { dst: t, site });
                Ok(t)
            }
            Resolution::Unresolved => Err(Self::internal("an unresolved reference")),
            Resolution::Caller => self.load_caller(id, dst),
        }
    }

    fn check_tdz(&mut self, reference: &swb_js_syntax::Reference, register: Reg) {
        if self.is_derived_this(reference) {
            self.emit(Insn::CheckThis { src: register });
        }
        if reference.tdz_check {
            let name = self.name_const(reference.name);
            self.emit(Insn::CheckTdz {
                src: register,
                name,
            });
        }
    }

    /// `PutValue` on an identifier (an assignment, not a declaration),
    /// resolved now: through the dynamic environments in its reach, then
    /// to its static binding.
    pub(super) fn store_ref(&mut self, id: RefId, src: Reg) -> CResult<()> {
        let mark = self.mark();
        let rec = self.prepare_dynamic(id)?;
        self.store_found(id, src, rec)?;
        self.release(mark);
        Ok(())
    }

    /// `PutValue` on the static binding of an identifier: checks the dead
    /// zone, rejects constants.
    pub(super) fn store_static(&mut self, id: RefId, src: Reg) -> CResult<()> {
        let reference = self.reference(id);
        let kind = self.binding_kind(&reference);
        let constant = matches!(kind, Some(BindingKind::Const | BindingKind::ClassName))
            || (kind == Some(BindingKind::FunctionName) && self.strict);
        let ignored = kind == Some(BindingKind::FunctionName) && !self.strict;
        let name = self.name_const(reference.name);
        match reference.resolution {
            Resolution::Global => {
                let site = self.site(self.name_text(reference.name));
                self.emit(Insn::SetGlobal { src, site });
            }
            Resolution::Register(r) => {
                if reference.tdz_check {
                    self.check_register_tdz(&reference, r)?;
                }
                if constant {
                    self.emit(Insn::ThrowConstAssign { name });
                } else if !ignored && r != src {
                    self.emit(Insn::Move { dst: r, src });
                }
            }
            Resolution::Cell(_) | Resolution::Capture(_) => {
                if reference.tdz_check || constant {
                    let mark = self.mark();
                    let t = self.temp()?;
                    match reference.resolution {
                        Resolution::Cell(cell) => self.emit(Insn::LoadCell { dst: t, cell }),
                        Resolution::Capture(index) => {
                            self.emit(Insn::LoadCapture { dst: t, index })
                        }
                        _ => return Err(Self::internal("a store to an unresolved name")),
                    };
                    if reference.tdz_check {
                        self.emit(Insn::CheckTdz { src: t, name });
                    }
                    self.release(mark);
                }
                if constant {
                    self.emit(Insn::ThrowConstAssign { name });
                } else if !ignored {
                    match reference.resolution {
                        Resolution::Cell(cell) => self.emit(Insn::StoreCell { cell, src }),
                        Resolution::Capture(index) => self.emit(Insn::StoreCapture { src, index }),
                        _ => return Err(Self::internal("a store to an unresolved name")),
                    };
                }
            }
            Resolution::Unresolved => return Err(Self::internal("an unresolved reference")),
            Resolution::Caller => return self.store_caller(id, src),
        }
        Ok(())
    }

    /// The register of a local binding that a plain assignment can write
    /// directly.
    fn direct_register(&self, id: RefId) -> Option<Reg> {
        let reference = self.reference(id);
        let Resolution::Register(r) = reference.resolution else {
            return None;
        };
        if self.dynamic_of(id).is_some() {
            return None;
        }
        let plain = matches!(
            self.binding_kind(&reference),
            Some(
                BindingKind::Var
                    | BindingKind::Let
                    | BindingKind::Parameter
                    | BindingKind::Function
                    | BindingKind::Arguments
            )
        );
        (plain && !reference.tdz_check).then_some(r)
    }

    /// `name = value` (also `var name = value`): `NamedEvaluation` and the
    /// store.
    pub(super) fn assign_identifier_value(
        &mut self,
        id: RefId,
        value: ExprId,
        dst: Option<Reg>,
        name: String16,
    ) -> CResult<Reg> {
        if let Some(r) = self.direct_register(id)
            && self.writes_target_last(value)
        {
            self.expr_named(value, Some(r), name)?;
            return match dst {
                Some(dst) if dst != r => {
                    self.emit(Insn::Move { dst, src: r });
                    Ok(dst)
                }
                _ => Ok(r),
            };
        }
        let t = self.target(dst)?;
        // The reference is resolved before the value (§13.15.2 step 1.a).
        let mark = self.mark();
        let rec = self.prepare_dynamic(id)?;
        self.expr_named(value, Some(t), name)?;
        self.store_found(id, t, rec)?;
        self.release(mark);
        Ok(t)
    }

    /// Whether `e` writes its destination register once, after it read
    /// all its operands (so a local's register can be the destination).
    pub(super) fn writes_target_last(&self, e: ExprId) -> bool {
        match self.ast.expr(e).kind {
            ExprKind::Paren(inner) => self.writes_target_last(inner),
            ExprKind::Identifier(_)
            | ExprKind::This(_)
            | ExprKind::Null
            | ExprKind::Boolean(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Function(_)
            | ExprKind::Member { .. }
            | ExprKind::Index { .. }
            | ExprKind::PrivateMember { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. } => true,
            ExprKind::Unary { op, .. } => op != UnaryOp::Delete,
            _ => false,
        }
    }

    /// Compiles `e` into a register that keeps its value while `later`
    /// are evaluated: a local's own register only if none of them can
    /// assign a local.
    pub(super) fn expr_stable(&mut self, e: ExprId, later: &[ExprId]) -> CResult<Reg> {
        let inner = self.unparen(e);
        let local = match self.ast.expr(inner).kind {
            ExprKind::Identifier(Ident { reference, .. }) | ExprKind::This(reference) => {
                matches!(
                    self.reference(reference).resolution,
                    Resolution::Register(_)
                ) && self.dynamic_of(reference).is_none()
            }
            _ => false,
        };
        if local && later.iter().all(|&l| !self.may_write_locals(l)) {
            return self.expr(e, None);
        }
        let t = self.temp()?;
        self.expr(e, Some(t))
    }

    /// Whether evaluating `e` can assign a local register of this
    /// function (an assignment or update in `e`, not in nested functions).
    /// Looks at a bounded number of nodes; a larger tree counts as yes.
    fn may_write_locals(&self, e: ExprId) -> bool {
        let mut work = vec![e];
        let mut seen = 0;
        while let Some(e) = work.pop() {
            seen += 1;
            if seen > SCAN_LIMIT {
                return true;
            }
            match self.ast.expr(e).kind {
                ExprKind::Assign { .. }
                | ExprKind::Update { .. }
                | ExprKind::Spread(_)
                | ExprKind::OptionalChain(_)
                | ExprKind::TaggedTemplate { .. }
                | ExprKind::Class(_)
                | ExprKind::SuperMember { .. }
                | ExprKind::SuperIndex { .. }
                | ExprKind::SuperCall(_)
                | ExprKind::ImportCall { .. }
                | ExprKind::ImportMeta => return true,
                ExprKind::Identifier(_)
                | ExprKind::This(_)
                | ExprKind::NewTarget(_)
                | ExprKind::Null
                | ExprKind::Boolean(_)
                | ExprKind::Number(_)
                | ExprKind::String(_)
                | ExprKind::BigInt(_)
                | ExprKind::RegExp { .. }
                | ExprKind::Hole
                | ExprKind::Function(_) => {}
                ExprKind::Template(id) => {
                    work.extend(self.ast.exprs(self.ast.template(id).expressions));
                }
                ExprKind::Array(list) | ExprKind::Sequence(list) => {
                    work.extend(self.ast.exprs(list));
                }
                ExprKind::Object(properties) => {
                    for property in self.ast.properties(properties) {
                        work.push(property.value);
                        if let AstKey::Computed(key) = property.key {
                            work.push(key);
                        }
                    }
                }
                ExprKind::Paren(inner)
                | ExprKind::Unary {
                    argument: inner, ..
                }
                | ExprKind::Member { object: inner, .. }
                | ExprKind::PrivateMember { object: inner, .. }
                | ExprKind::PrivateIn { object: inner, .. } => work.push(inner),
                ExprKind::Yield { argument, .. } => work.extend(argument),
                ExprKind::Await(argument) => work.push(argument),
                ExprKind::Binary { left, right, .. }
                | ExprKind::Logical { left, right, .. }
                | ExprKind::Index {
                    object: left,
                    index: right,
                    ..
                } => {
                    work.push(left);
                    work.push(right);
                }
                ExprKind::Conditional {
                    test,
                    consequent,
                    alternate,
                } => work.extend([test, consequent, alternate]),
                ExprKind::Call {
                    callee, arguments, ..
                }
                | ExprKind::New { callee, arguments } => {
                    work.push(callee);
                    work.extend(self.ast.exprs(arguments));
                }
            }
        }
        false
    }

    // --- Operators ---

    fn unary(&mut self, op: UnaryOp, argument: ExprId, dst: Option<Reg>) -> CResult<Reg> {
        let t = self.target(dst)?;
        let mark = self.mark();
        match op {
            UnaryOp::Delete => {
                let inner = self.unparen(argument);
                match self.ast.expr(inner).kind {
                    ExprKind::Member { .. } | ExprKind::Index { .. } => {
                        self.delete_property_of(inner, t, false)?;
                    }
                    ExprKind::SuperMember { .. } | ExprKind::SuperIndex { .. } => {
                        self.super_delete(inner)?;
                        self.emit(Insn::Bool {
                            dst: t,
                            value: true,
                        });
                    }
                    ExprKind::OptionalChain(last)
                        if matches!(
                            self.ast.expr(last).kind,
                            ExprKind::Member { .. } | ExprKind::Index { .. }
                        ) =>
                    {
                        self.delete_property_of(last, t, true)?;
                    }
                    ExprKind::Identifier(ident) => self.delete_identifier(ident, t)?,
                    _ => {
                        self.effect(argument)?;
                        self.emit(Insn::Bool {
                            dst: t,
                            value: true,
                        });
                    }
                }
            }
            UnaryOp::Void => {
                self.effect(argument)?;
                self.emit(Insn::Undefined { dst: t });
            }
            UnaryOp::Typeof => {
                let inner = self.unparen(argument);
                if let ExprKind::Identifier(ident) = self.ast.expr(inner).kind {
                    self.typeof_identifier(ident, t)?;
                } else {
                    let src = self.expr(argument, None)?;
                    self.emit(Insn::Typeof { dst: t, src });
                }
            }
            UnaryOp::Minus => {
                let literal = self.ast.expr(self.unparen(argument)).kind;
                if let ExprKind::Number(value) = literal {
                    self.load_number(-value, t);
                } else if let ExprKind::BigInt(digits) = literal {
                    self.load_bigint(digits, true, t);
                } else {
                    let src = self.expr(argument, None)?;
                    self.emit(Insn::Neg { dst: t, src });
                }
            }
            UnaryOp::Plus => {
                let src = self.expr(argument, None)?;
                self.emit(Insn::ToNumber { dst: t, src });
            }
            UnaryOp::BitNot => {
                let src = self.expr(argument, None)?;
                self.emit(Insn::BitNot { dst: t, src });
            }
            UnaryOp::Not => {
                let src = self.expr(argument, None)?;
                self.emit(Insn::Not { dst: t, src });
            }
        }
        self.release(mark);
        Ok(t)
    }

    /// `typeof name` into `t`: a global name that does not exist gives
    /// `"undefined"` (§13.5.3.1 step 2); a dynamic name is looked up in
    /// its records first.
    fn typeof_identifier(&mut self, ident: Ident, t: Reg) -> CResult<()> {
        let mark = self.mark();
        let rec = self.prepare_dynamic(ident.reference)?;
        let mut done = None;
        if let Some(rec) = rec {
            let name = self.name_const(ident.name);
            let skip = self.emit(Insn::JumpIfNullish {
                src: rec,
                offset: 0,
            });
            self.emit(Insn::GetBinding {
                dst: t,
                record: rec,
                name,
            });
            self.emit(Insn::Typeof { dst: t, src: t });
            done = Some(self.emit(Insn::Jump { offset: 0 }));
            self.patch_here(skip);
        }
        if self.is_global_name(ident.reference) {
            let name = self.name_const(ident.name);
            self.emit(Insn::TypeofGlobal { dst: t, name });
        } else {
            let src = self.load_static(ident.reference, None)?;
            self.emit(Insn::Typeof { dst: t, src });
        }
        if let Some(done) = done {
            self.patch_here(done);
        }
        self.release(mark);
        Ok(())
    }

    /// `delete name` into `t` (§13.5.1.2 step 4, sloppy code only): a
    /// binding of a dynamic environment first, a global property, or
    /// `false` for a declared binding.
    fn delete_identifier(&mut self, ident: Ident, t: Reg) -> CResult<()> {
        let mark = self.mark();
        let rec = self.prepare_dynamic(ident.reference)?;
        let mut done = None;
        if let Some(rec) = rec {
            let name = self.name_const(ident.name);
            let skip = self.emit(Insn::JumpIfNullish {
                src: rec,
                offset: 0,
            });
            self.emit(Insn::DeleteBinding {
                dst: t,
                record: rec,
                name,
            });
            done = Some(self.emit(Insn::Jump { offset: 0 }));
            self.patch_here(skip);
        }
        if self.is_global_name(ident.reference) {
            let name = self.name_const(ident.name);
            self.emit(Insn::DeleteGlobal { dst: t, name });
        } else {
            self.emit(Insn::Bool {
                dst: t,
                value: false,
            });
        }
        if let Some(done) = done {
            self.patch_here(done);
        }
        self.release(mark);
        Ok(())
    }

    /// The instruction of a binary operator.
    fn binary_insn(op: BinaryOp, dst: Reg, a: Reg, b: Reg) -> Insn {
        match op {
            BinaryOp::Exponent => Insn::Exp { dst, a, b },
            BinaryOp::Multiply => Insn::Mul { dst, a, b },
            BinaryOp::Divide => Insn::Div { dst, a, b },
            BinaryOp::Remainder => Insn::Rem { dst, a, b },
            BinaryOp::Add => Insn::Add { dst, a, b },
            BinaryOp::Subtract => Insn::Sub { dst, a, b },
            BinaryOp::ShiftLeft => Insn::Shl { dst, a, b },
            BinaryOp::ShiftRight => Insn::Shr { dst, a, b },
            BinaryOp::ShiftRightUnsigned => Insn::UShr { dst, a, b },
            BinaryOp::Less => Insn::Lt { dst, a, b },
            BinaryOp::Greater => Insn::Gt { dst, a, b },
            BinaryOp::LessEqual => Insn::Le { dst, a, b },
            BinaryOp::GreaterEqual => Insn::Ge { dst, a, b },
            BinaryOp::Instanceof => Insn::InstanceOf { dst, a, b },
            BinaryOp::In => Insn::In {
                dst,
                key: a,
                obj: b,
            },
            BinaryOp::Equal => Insn::Eq { dst, a, b },
            BinaryOp::NotEqual => Insn::Ne { dst, a, b },
            BinaryOp::StrictEqual => Insn::StrictEq { dst, a, b },
            BinaryOp::StrictNotEqual => Insn::StrictNe { dst, a, b },
            BinaryOp::BitAnd => Insn::BitAnd { dst, a, b },
            BinaryOp::BitXor => Insn::BitXor { dst, a, b },
            BinaryOp::BitOr => Insn::BitOr { dst, a, b },
        }
    }

    /// A binary operator; a left-associative chain (`a + b + c`) is
    /// compiled in a loop, without recursion on its left operands.
    fn binary(&mut self, e: ExprId, dst: Option<Reg>) -> CResult<Reg> {
        let mut links = Vec::new();
        let mut leftmost = e;
        while let ExprKind::Binary { op, left, right } = self.ast.expr(leftmost).kind {
            // Chromium reports an operator at the operator token.
            let position = self.operator_position(left, self.ast.expr(leftmost).span.start);
            links.push((op, right, position));
            leftmost = self.unparen(left);
        }
        links.reverse();
        let t = self.target(dst)?;
        let mark = self.mark();
        if let [(op, right, position)] = links.as_slice() {
            let a = self.expr_stable(leftmost, &[*right])?;
            let b = self.expr(*right, None)?;
            self.position = *position;
            self.emit(Self::binary_insn(*op, t, a, b));
        } else {
            let acc = self.temp()?;
            self.expr(leftmost, Some(acc))?;
            let count = links.len();
            for (i, &(op, right, position)) in links.iter().enumerate() {
                let inner = self.mark();
                let b = self.expr(right, None)?;
                let out = if i + 1 == count { t } else { acc };
                self.position = position;
                self.emit(Self::binary_insn(op, out, acc, b));
                self.release(inner);
            }
        }
        self.release(mark);
        Ok(t)
    }

    /// The operands of a left-associative chain of one logical operator,
    /// in evaluation order (`a && b && c` gives `[a, b, c]`).
    fn logical_operands(&self, e: ExprId, op: LogicalOp) -> Vec<ExprId> {
        let mut operands = Vec::new();
        let mut leftmost = e;
        while let ExprKind::Logical {
            op: link,
            left,
            right,
        } = self.ast.expr(leftmost).kind
        {
            if link != op {
                break;
            }
            operands.push(right);
            leftmost = self.unparen(left);
        }
        operands.push(leftmost);
        operands.reverse();
        operands
    }

    /// `&&`, `||`, `??`; a chain of the same operator in a loop.
    fn logical(&mut self, e: ExprId, op: LogicalOp, dst: Option<Reg>) -> CResult<Reg> {
        let operands = self.logical_operands(e, op);
        let t = self.target(dst)?;
        let mut exits = Vec::new();
        let count = operands.len();
        for (i, &operand) in operands.iter().enumerate() {
            self.expr(operand, Some(t))?;
            if i + 1 < count {
                let jump = match op {
                    LogicalOp::And => Insn::JumpIfFalse { cond: t, offset: 0 },
                    LogicalOp::Or => Insn::JumpIfTrue { cond: t, offset: 0 },
                    LogicalOp::Coalesce => Insn::JumpIfNotNullish { src: t, offset: 0 },
                };
                exits.push(self.emit(jump));
            }
        }
        for exit in exits {
            self.patch_here(exit);
        }
        Ok(t)
    }

    /// Compiles a condition; returns the jumps to patch to the false
    /// target. Relational operators use the fused compare-and-branch
    /// instructions.
    pub(super) fn branch_false(&mut self, test: ExprId) -> CResult<Vec<usize>> {
        self.enter(EXPR_WEIGHT)?;
        let test = self.unparen(test);
        self.position = self.ast.expr(test).span.start;
        let mark = self.mark();
        let exits = match self.ast.expr(test).kind {
            ExprKind::Binary {
                op:
                    op @ (BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual),
                left,
                right,
            } => {
                let a = self.expr_stable(left, &[right])?;
                let b = self.expr(right, None)?;
                let insn = match op {
                    BinaryOp::Less => Insn::JumpIfNotLt { a, b, offset: 0 },
                    BinaryOp::LessEqual => Insn::JumpIfNotLe { a, b, offset: 0 },
                    BinaryOp::Greater => Insn::JumpIfNotGt { a, b, offset: 0 },
                    _ => Insn::JumpIfNotGe { a, b, offset: 0 },
                };
                vec![self.emit(insn)]
            }
            ExprKind::Logical {
                op: LogicalOp::And, ..
            } => {
                let mut exits = Vec::new();
                for operand in self.logical_operands(test, LogicalOp::And) {
                    exits.extend(self.branch_false(operand)?);
                }
                exits
            }
            ExprKind::Unary {
                op: UnaryOp::Not,
                argument,
            } => {
                let v = self.expr(argument, None)?;
                vec![self.emit(Insn::JumpIfTrue { cond: v, offset: 0 })]
            }
            ExprKind::Boolean(true) => Vec::new(),
            _ => {
                let v = self.expr(test, None)?;
                vec![self.emit(Insn::JumpIfFalse { cond: v, offset: 0 })]
            }
        };
        self.release(mark);
        self.leave(EXPR_WEIGHT);
        Ok(exits)
    }

    // --- Literals ---

    /// A template literal without a tag (§13.2.8.6): the cooked strings
    /// and `ToString` of the substitutions, concatenated.
    fn template(&mut self, id: swb_js_syntax::TemplateId, dst: Option<Reg>) -> CResult<Reg> {
        let template = *self.ast.template(id);
        let quasis = self.ast.template_elements(template.quasis).to_vec();
        let expressions = self.ast.exprs(template.expressions).to_vec();
        let t = self.target(dst)?;
        let cooked = |c: &Self, i: usize| -> CResult<String16> {
            let element = quasis
                .get(i)
                .ok_or(Self::internal("a template without its strings"))?;
            let cooked = element.cooked.ok_or(Self::internal(
                "an untagged template with an invalid escape",
            ))?;
            Ok(c.ast.string(cooked).clone())
        };
        let mark = self.mark();
        // The first part goes into a temporary: `t` may be read by the
        // substitutions until the end.
        let acc = self.temp()?;
        let head = cooked(self, 0)?;
        let index = self.string_const(head);
        self.emit(Insn::Const { dst: acc, index });
        for (i, &expression) in expressions.iter().enumerate() {
            let inner = self.mark();
            let v = self.temp()?;
            self.expr(expression, Some(v))?;
            self.emit(Insn::ToString { dst: v, src: v });
            self.emit(Insn::Add {
                dst: acc,
                a: acc,
                b: v,
            });
            let part = cooked(self, i + 1)?;
            if !part.is_empty() {
                let index = self.string_const(part);
                self.emit(Insn::Const { dst: v, index });
                self.emit(Insn::Add {
                    dst: acc,
                    a: acc,
                    b: v,
                });
            }
            self.release(inner);
        }
        self.emit(Insn::Move { dst: t, src: acc });
        self.release(mark);
        Ok(t)
    }

    /// An array literal (§13.2.4.1).
    fn array(&mut self, elements: List<ExprId>, dst: Option<Reg>) -> CResult<Reg> {
        let t = self.target(dst)?;
        let mark = self.mark();
        let array = self.temp()?;
        let items = self.ast.exprs(elements).to_vec();
        self.emit(Insn::NewArray { dst: array });
        for item in items {
            self.array_element(array, item, ITER_ARRAY_SPREAD, None)?;
        }
        self.emit(Insn::Move { dst: t, src: array });
        self.release(mark);
        Ok(t)
    }

    /// The text of a literal property key.
    pub(super) fn key_text(&self, key: AstKey) -> Option<String16> {
        match key {
            AstKey::Name(name) => Some(self.name_text(name)),
            AstKey::Number(value) => Some(String16::from(number_to_string(value).as_str())),
            AstKey::BigInt(digits) => super::bigint::key_text(self.ast.string(digits)),
            AstKey::Computed(_) => None,
        }
    }

    /// An object literal (§13.2.5.4).
    fn object(
        &mut self,
        properties: List<swb_js_syntax::Property>,
        dst: Option<Reg>,
    ) -> CResult<Reg> {
        let list = self.ast.properties(properties).to_vec();
        if let Some((keys, slots)) = self.literal_layout(&list) {
            return self.shaped_object(&list, &keys, &slots, dst);
        }
        let t = self.target(dst)?;
        let mark = self.mark();
        let object = self.temp()?;
        let start = self.position;
        self.emit(Insn::NewObject { dst: object });
        for (i, property) in self
            .ast
            .properties(properties)
            .to_vec()
            .into_iter()
            .enumerate()
        {
            self.position = property.span.start;
            let inner = self.mark();
            match property.kind {
                PropertyKind::Method => self.literal_method(object, property)?,
                PropertyKind::Init | PropertyKind::Shorthand => {
                    if let Some(text) = self.key_text(property.key) {
                        let v = self.expr_named(property.value, None, text.clone())?;
                        let site = self.site(text);
                        self.emit(Insn::DefineNamed {
                            obj: object,
                            src: v,
                            site,
                        });
                    } else if let AstKey::Computed(key) = property.key {
                        let k = self.temp()?;
                        self.expr(key, Some(k))?;
                        self.emit(Insn::ToKey { dst: k, src: k });
                        let v = self.expr_named_by_key(property.value, None, k)?;
                        self.emit(Insn::DefineIndex {
                            obj: object,
                            key: k,
                            src: v,
                        });
                    }
                }
                PropertyKind::Proto => {
                    let v = self.expr(property.value, None)?;
                    self.emit(Insn::SetProto {
                        obj: object,
                        src: v,
                    });
                }
                PropertyKind::Getter | PropertyKind::Setter => {
                    self.accessor_property(object, property)?;
                }
                PropertyKind::Spread => {
                    let v = self.expr(property.value, None)?;
                    // Chromium reports a getter that the copy calls at
                    // the start of the literal for a first spread, else at
                    // the spread's operand.
                    self.position = if i == 0 {
                        start
                    } else {
                        self.ast.expr(property.value).span.start
                    };
                    self.emit(Insn::CopyDataProperties {
                        obj: object,
                        src: v,
                    });
                }
            }
            self.release(inner);
        }
        self.emit(Insn::Move {
            dst: t,
            src: object,
        });
        self.release(mark);
        Ok(t)
    }

    /// The layout of an object literal whose properties are all data
    /// properties (`key: value`, shorthands, methods) with constant keys
    /// that are not array indices: the keys in order, each once, and the
    /// slot of each property (a repeated key writes the slot of its first
    /// occurrence, as `CreateDataPropertyOrThrow` replaces the value in
    /// place). `None` for any other literal, and for one with more keys
    /// than a shared shape holds.
    fn literal_layout(
        &self,
        list: &[swb_js_syntax::Property],
    ) -> Option<(Vec<String16>, Vec<u32>)> {
        let mut keys: Vec<String16> = Vec::new();
        let mut slots = Vec::with_capacity(list.len());
        for property in list {
            if !matches!(
                property.kind,
                PropertyKind::Init | PropertyKind::Shorthand | PropertyKind::Method
            ) {
                return None;
            }
            let text = self.key_text(property.key)?;
            if crate::string::array_index(text.as_str16()).is_some() {
                return None;
            }
            // A linear search: at most `LITERAL_KEY_LIMIT` keys.
            let slot = if let Some(slot) = keys.iter().position(|key| *key == text) {
                slot as u32
            } else {
                if keys.len() == crate::object::LITERAL_KEY_LIMIT {
                    return None;
                }
                keys.push(text);
                keys.len() as u32 - 1
            };
            slots.push(slot);
        }
        Some((keys, slots))
    }

    /// An object literal of [`FunctionCompiler::literal_layout`]: the
    /// object in its final shape, then the values in their slots.
    fn shaped_object(
        &mut self,
        list: &[swb_js_syntax::Property],
        keys: &[String16],
        slots: &[u32],
        dst: Option<Reg>,
    ) -> CResult<Reg> {
        let t = self.target(dst)?;
        let mark = self.mark();
        let object = self.temp()?;
        // Consecutive sites for the keys; a literal without keys has one
        // site for its cache entry.
        let site = match keys.split_first() {
            Some((first, rest)) => {
                let site = self.site(first.clone());
                for (offset, key) in rest.iter().enumerate() {
                    if self.site(key.clone()) != site + 1 + offset as u32 {
                        return Err(Self::internal("literal sites that are not consecutive"));
                    }
                }
                site
            }
            None => self.site(String16::default()),
        };
        let count =
            u16::try_from(keys.len()).map_err(|_| Self::internal("too many literal keys"))?;
        self.emit(Insn::NewLiteral {
            dst: object,
            count,
            site,
        });
        for (property, &slot) in list.iter().zip(slots) {
            self.position = property.span.start;
            let inner = self.mark();
            let text = keys
                .get(slot as usize)
                .cloned()
                .ok_or(Self::internal("a literal slot without a key"))?;
            let v = if property.kind == PropertyKind::Method {
                let ExprKind::Function(function) = self.ast.expr(property.value).kind else {
                    return Err(Self::internal("a method that is not a function"));
                };
                self.session.inferred_names.insert(function, text);
                self.position = self.ast.expr(property.value).span.start;
                let v = self.method_closure(function, object, None)?;
                self.position = property.span.start;
                v
            } else {
                self.expr_named(property.value, None, text)?
            };
            self.emit(Insn::InitSlot {
                obj: object,
                src: v,
                slot,
            });
            self.release(inner);
        }
        self.emit(Insn::Move {
            dst: t,
            src: object,
        });
        self.release(mark);
        Ok(t)
    }

    /// A method of an object literal (§15.4.4 `DefineMethod`): its home
    /// object is the literal.
    fn literal_method(&mut self, object: Reg, property: swb_js_syntax::Property) -> CResult<()> {
        let ExprKind::Function(function) = self.ast.expr(property.value).kind else {
            return Err(Self::internal("a method that is not a function"));
        };
        if let Some(text) = self.key_text(property.key) {
            self.session.inferred_names.insert(function, text.clone());
            self.position = self.ast.expr(property.value).span.start;
            let v = self.method_closure(function, object, None)?;
            let site = self.site(text);
            self.position = property.span.start;
            self.emit(Insn::DefineNamed {
                obj: object,
                src: v,
                site,
            });
        } else if let AstKey::Computed(key) = property.key {
            let k = self.temp()?;
            self.expr(key, Some(k))?;
            self.emit(Insn::ToKey { dst: k, src: k });
            self.position = self.ast.expr(property.value).span.start;
            let v = self.method_closure(function, object, None)?;
            self.emit(Insn::SetFunctionName {
                func: v,
                key: k,
                prefix: NAME_PREFIX_NONE,
            });
            self.position = property.span.start;
            self.emit(Insn::DefineIndex {
                obj: object,
                key: k,
                src: v,
            });
        }
        Ok(())
    }

    /// A getter or setter of an object literal (§15.4.5
    /// `MethodDefinitionEvaluation`): the key, the function with the name
    /// `get key` or `set key` (§10.2.9 `SetFunctionName`; at run time for
    /// a computed key) and the accessor property.
    fn accessor_property(&mut self, object: Reg, property: swb_js_syntax::Property) -> CResult<()> {
        let getter = property.kind == PropertyKind::Getter;
        let k = self.temp()?;
        let computed = match property.key {
            AstKey::Computed(key) => {
                self.expr(key, Some(k))?;
                self.emit(Insn::ToKey { dst: k, src: k });
                true
            }
            key => {
                let text = self
                    .key_text(key)
                    .ok_or(Self::internal("an accessor key without a text"))?;
                let prefix = if getter { "get " } else { "set " };
                let mut name = String16::from(prefix);
                name.push_str16(text.as_str16());
                if let ExprKind::Function(function) = self.ast.expr(property.value).kind {
                    self.session.inferred_names.insert(function, name);
                }
                let index = self.string_const(text);
                self.emit(Insn::Const { dst: k, index });
                false
            }
        };
        let ExprKind::Function(function) = self.ast.expr(property.value).kind else {
            return Err(Self::internal("an accessor that is not a function"));
        };
        self.position = self.ast.expr(property.value).span.start;
        let v = self.method_closure(function, object, None)?;
        self.position = property.span.start;
        if computed {
            let prefix = if getter {
                NAME_PREFIX_GET
            } else {
                NAME_PREFIX_SET
            };
            self.emit(Insn::SetFunctionName {
                func: v,
                key: k,
                prefix,
            });
        }
        let insn = if getter {
            Insn::DefineGetter {
                obj: object,
                key: k,
                src: v,
            }
        } else {
            Insn::DefineSetter {
                obj: object,
                key: k,
                src: v,
            }
        };
        self.emit(insn);
        Ok(())
    }

    /// `IsAnonymousFunctionDefinition` (§8.4.3) for the function kinds that
    /// compile: a function expression without a name or an arrow function.
    pub(super) fn is_anonymous_function(&self, e: ExprId) -> bool {
        matches!(
            self.ast.expr(self.unparen(e)).kind,
            ExprKind::Function(function) if self.ast.function(function).name.is_none()
        )
    }

    /// The key text of a constant index expression (`o["x"]`, `o[0]`).
    pub(super) fn constant_key(&self, index: ExprId) -> Option<String16> {
        match self.ast.expr(self.unparen(index)).kind {
            ExprKind::String(id) => Some(self.ast.string(id).clone()),
            ExprKind::Number(value) => Some(String16::from(number_to_string(value).as_str())),
            ExprKind::BigInt(digits) => super::bigint::key_text(self.ast.string(digits)),
            _ => None,
        }
    }

    // --- Assignment and update ---

    fn assign(
        &mut self,
        op: AssignOp,
        target: AssignTarget,
        value: ExprId,
        dst: Option<Reg>,
    ) -> CResult<Reg> {
        let target = match target {
            AssignTarget::Simple(target) => target,
            AssignTarget::Pattern(pattern) => {
                return self.destructuring_assignment(pattern, value, dst);
            }
        };
        let raw_target = target;
        let target = self.unparen(target);
        // Chromium reports a failed read of a compound assignment at the
        // start of the target and a failed write at the operator.
        let places = (
            self.ast.expr(target).span.start,
            self.operator_position(raw_target, self.position),
        );
        if matches!(op, AssignOp::Assign)
            && matches!(
                self.ast.expr(target).kind,
                ExprKind::Member { .. } | ExprKind::Index { .. }
            )
        {
            self.note_member_function(target, value);
        }
        if let ExprKind::PrivateMember { object, name, .. } = self.ast.expr(target).kind {
            // Chromium's positions: see `private_position`; a failed write
            // to a field by `=` is at the operator.
            let at = self.private_position(target);
            let field = at != self.name_position(target);
            let places = match op {
                AssignOp::Assign if field => (at, places.1),
                _ => (at, at),
            };
            return self.assign_property(
                op,
                object,
                PropertyRef::Private(name),
                value,
                dst,
                places,
            );
        }
        match self.ast.expr(target).kind {
            ExprKind::Identifier(ident) => {
                self.position = places.1;
                self.assign_identifier(op, ident, value, dst)
            }
            ExprKind::Member {
                object, property, ..
            } => {
                let text = self.name_text(property);
                self.assign_property(op, object, PropertyRef::Named(text), value, dst, places)
            }
            ExprKind::Index { object, index, .. } => match self.constant_key(index) {
                Some(text) => {
                    self.assign_property(op, object, PropertyRef::Named(text), value, dst, places)
                }
                None => self.assign_property(
                    op,
                    object,
                    PropertyRef::Computed(index),
                    value,
                    dst,
                    places,
                ),
            },
            ExprKind::SuperMember { .. } | ExprKind::SuperIndex { .. } => {
                self.assign_super(op, target, value, dst, places)
            }
            ExprKind::Call { .. } => {
                // Annex B web compatibility: the call runs, then the
                // assignment throws (Chromium's behaviour).
                self.effect(target)?;
                self.emit(Insn::ThrowInvalidAssign {
                    context: INVALID_TARGET_ASSIGN,
                });
                self.target(dst)
            }
            _ => Err(Self::internal("an assignment to an invalid target")),
        }
    }

    fn assign_identifier(
        &mut self,
        op: AssignOp,
        ident: Ident,
        value: ExprId,
        dst: Option<Reg>,
    ) -> CResult<Reg> {
        let name = self.name_text(ident.name);
        match op {
            AssignOp::Assign => self.assign_identifier_value(ident.reference, value, dst, name),
            AssignOp::Compound(binary) => {
                // A local whose value the right side cannot change is
                // updated in place.
                if let Some(r) = self.direct_register(ident.reference)
                    && !self.may_write_locals(value)
                {
                    let mark = self.mark();
                    let b = self.expr(value, None)?;
                    self.emit(Self::binary_insn(binary, r, r, b));
                    self.release(mark);
                    return match dst {
                        Some(dst) if dst != r => {
                            self.emit(Insn::Move { dst, src: r });
                            Ok(dst)
                        }
                        _ => Ok(r),
                    };
                }
                let t = self.target(dst)?;
                let outer = self.mark();
                let rec = self.prepare_dynamic(ident.reference)?;
                self.load_found(ident.reference, rec, t)?;
                let mark = self.mark();
                let b = self.expr(value, None)?;
                self.emit(Self::binary_insn(binary, t, t, b));
                self.release(mark);
                self.store_found(ident.reference, t, rec)?;
                self.release(outer);
                Ok(t)
            }
            AssignOp::Logical(logical) => {
                let t = self.target(dst)?;
                let outer = self.mark();
                let rec = self.prepare_dynamic(ident.reference)?;
                self.load_found(ident.reference, rec, t)?;
                let skip = self.emit(Self::logical_skip(logical, t));
                self.expr_named(value, Some(t), name)?;
                self.store_found(ident.reference, t, rec)?;
                self.patch_here(skip);
                self.release(outer);
                Ok(t)
            }
        }
    }

    /// The jump that skips the right operand of `&&=`, `||=`, `??=`.
    fn logical_skip(op: LogicalOp, register: Reg) -> Insn {
        match op {
            LogicalOp::And => Insn::JumpIfFalse {
                cond: register,
                offset: 0,
            },
            LogicalOp::Or => Insn::JumpIfTrue {
                cond: register,
                offset: 0,
            },
            LogicalOp::Coalesce => Insn::JumpIfNotNullish {
                src: register,
                offset: 0,
            },
        }
    }

    fn assign_property(
        &mut self,
        op: AssignOp,
        object: ExprId,
        key: PropertyRef,
        value: ExprId,
        dst: Option<Reg>,
        (read_position, write_position): (u32, u32),
    ) -> CResult<Reg> {
        let t = self.target(dst)?;
        let mark = self.mark();
        let (obj, key) = match key {
            PropertyRef::Named(text) => {
                let obj = self.expr_stable(object, &[value])?;
                (obj, KeyReg::Site(self.site(text)))
            }
            PropertyRef::Private(name) => {
                let obj = self.expr_stable(object, &[value])?;
                (obj, self.private_key(name)?)
            }
            PropertyRef::Computed(index) => {
                let obj = self.expr_stable(object, &[index, value])?;
                let k = if matches!(op, AssignOp::Assign) {
                    self.expr_stable(index, &[value])?
                } else {
                    // GetValue and PutValue convert the key once.
                    let k = self.temp()?;
                    self.expr(index, Some(k))?;
                    self.emit(Insn::ToKey { dst: k, src: k });
                    k
                };
                (obj, KeyReg::Register(k))
            }
        };
        match op {
            AssignOp::Assign => {
                self.expr(value, Some(t))?;
                self.position = write_position;
                self.emit_put(obj, key, t);
            }
            AssignOp::Compound(binary) => {
                self.position = read_position;
                self.emit_get(t, obj, key);
                let inner = self.mark();
                let b = self.expr(value, None)?;
                self.position = write_position;
                self.emit(Self::binary_insn(binary, t, t, b));
                self.release(inner);
                self.emit_put(obj, key, t);
            }
            AssignOp::Logical(logical) => {
                self.position = read_position;
                self.emit_get(t, obj, key);
                let skip = self.emit(Self::logical_skip(logical, t));
                self.expr(value, Some(t))?;
                self.position = write_position;
                self.emit_put(obj, key, t);
                self.patch_here(skip);
            }
        }
        self.release(mark);
        Ok(t)
    }

    /// An assignment to a `super` property (§13.15.2): the reference, the
    /// value, `PutValue`; a compound assignment reads first with the key
    /// converted once.
    fn assign_super(
        &mut self,
        op: AssignOp,
        target: ExprId,
        value: ExprId,
        dst: Option<Reg>,
        (read_position, write_position): (u32, u32),
    ) -> CResult<Reg> {
        let t = self.target(dst)?;
        let mark = self.mark();
        let (base, key) = self.super_reference(target)?;
        match op {
            AssignOp::Assign => {
                self.expr(value, Some(t))?;
                self.position = write_position;
                self.emit_put_super(base, key, t)?;
            }
            AssignOp::Compound(binary) => {
                self.key_once(key);
                self.position = read_position;
                self.emit_get_super(t, base, key)?;
                let inner = self.mark();
                let b = self.expr(value, None)?;
                self.position = write_position;
                self.emit(Self::binary_insn(binary, t, t, b));
                self.release(inner);
                self.emit_put_super(base, key, t)?;
            }
            AssignOp::Logical(logical) => {
                self.key_once(key);
                self.position = read_position;
                self.emit_get_super(t, base, key)?;
                let skip = self.emit(Self::logical_skip(logical, t));
                self.expr(value, Some(t))?;
                self.position = write_position;
                self.emit_put_super(base, key, t)?;
                self.patch_here(skip);
            }
        }
        self.release(mark);
        Ok(t)
    }

    /// `PutValue` of the value in `value` to a simple assignment target
    /// that is evaluated now (a `for`-`in` target, §14.7.5.7 step 6.c).
    pub(super) fn assign_to_target(&mut self, target: ExprId, value: Reg) -> CResult<()> {
        let target = self.unparen(target);
        let mark = self.mark();
        match self.ast.expr(target).kind {
            ExprKind::Identifier(ident) => self.store_ref(ident.reference, value)?,
            ExprKind::Member {
                object, property, ..
            } => {
                let obj = self.expr(object, None)?;
                let site = self.site(self.name_text(property));
                self.emit(Insn::SetNamed {
                    obj,
                    src: value,
                    site,
                });
            }
            ExprKind::Index { object, index, .. } => {
                if let Some(text) = self.constant_key(index) {
                    let obj = self.expr(object, None)?;
                    let site = self.site(text);
                    self.emit(Insn::SetNamed {
                        obj,
                        src: value,
                        site,
                    });
                } else {
                    let obj = self.expr_stable(object, &[index])?;
                    let key = self.expr(index, None)?;
                    self.emit(Insn::SetIndex {
                        obj,
                        key,
                        src: value,
                    });
                }
            }
            ExprKind::SuperMember { .. } | ExprKind::SuperIndex { .. } => {
                let (base, key) = self.super_reference(target)?;
                self.emit_put_super(base, key, value)?;
            }
            ExprKind::PrivateMember { object, name, .. } => {
                let obj = self.expr(object, None)?;
                let key = self.private_key(name)?;
                self.emit_put(obj, key, value);
            }
            ExprKind::Call { .. } => {
                // Annex B web compatibility, as for `f() = v`.
                self.effect(target)?;
                self.emit(Insn::ThrowInvalidAssign {
                    context: INVALID_TARGET_FOR,
                });
            }
            _ => return Err(Self::internal("an assignment to an invalid target")),
        }
        self.release(mark);
        Ok(())
    }

    pub(super) fn emit_get(&mut self, dst: Reg, obj: Reg, key: KeyReg) {
        match key {
            KeyReg::Site(site) => self.emit(Insn::GetNamed { dst, obj, site }),
            KeyReg::Register(key) => self.emit(Insn::GetIndex { dst, obj, key }),
            KeyReg::Private { env, index } => self.emit(Insn::GetPrivate {
                dst,
                obj,
                env,
                index,
            }),
        };
    }

    pub(super) fn emit_put(&mut self, obj: Reg, key: KeyReg, src: Reg) {
        match key {
            KeyReg::Site(site) => {
                // The write has a cache of its own.
                let site = self.site_like(site);
                self.emit(Insn::SetNamed { obj, src, site })
            }
            KeyReg::Register(key) => self.emit(Insn::SetIndex { obj, key, src }),
            KeyReg::Private { env, index } => self.emit(Insn::SetPrivate {
                obj,
                src,
                env,
                index,
            }),
        };
    }

    /// The key of a private name occurrence: the private environment of its
    /// class (the register of a local binding, or a temporary) and the
    /// name's index in it.
    pub(super) fn private_key(&mut self, name: Ident) -> CResult<KeyReg> {
        let index = self.private_index(name)?;
        let env = self.load_ref(name.reference, None)?;
        Ok(KeyReg::Private { env, index })
    }

    /// `++` and `--` (§13.4). With `discard`, the result is not needed.
    fn update(
        &mut self,
        op: UpdateOp,
        prefix: bool,
        target: ExprId,
        dst: Option<Reg>,
        discard: bool,
    ) -> CResult<Reg> {
        let step = |dst: Reg, src: Reg| match op {
            UpdateOp::Increment => Insn::Inc { dst, src },
            UpdateOp::Decrement => Insn::Dec { dst, src },
        };
        let target = self.unparen(target);
        match self.ast.expr(target).kind {
            ExprKind::Identifier(ident) => {
                if let Some(r) = self.direct_register(ident.reference)
                    && (discard || (prefix && dst.is_none()))
                {
                    self.emit(step(r, r));
                    return Ok(r);
                }
                let t = self.target(dst)?;
                let outer = self.mark();
                let rec = self.prepare_dynamic(ident.reference)?;
                self.load_found(ident.reference, rec, t)?;
                if prefix || discard {
                    self.emit(step(t, t));
                    self.store_found(ident.reference, t, rec)?;
                } else {
                    self.emit(Insn::ToNumeric { dst: t, src: t });
                    let mark = self.mark();
                    let n = self.temp()?;
                    self.emit(step(n, t));
                    self.store_found(ident.reference, n, rec)?;
                    self.release(mark);
                }
                self.release(outer);
                Ok(t)
            }
            ExprKind::Member { .. } | ExprKind::Index { .. } | ExprKind::PrivateMember { .. } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                let (obj, key) = self.update_reference(target)?;
                self.emit_get(t, obj, key);
                if prefix || discard {
                    self.emit(step(t, t));
                    self.emit_put(obj, key, t);
                } else {
                    self.emit(Insn::ToNumeric { dst: t, src: t });
                    let n = self.temp()?;
                    self.emit(step(n, t));
                    self.emit_put(obj, key, n);
                }
                self.release(mark);
                Ok(t)
            }
            ExprKind::SuperMember { .. } | ExprKind::SuperIndex { .. } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                let (base, key) = self.super_reference(target)?;
                self.key_once(key);
                self.emit_get_super(t, base, key)?;
                if prefix || discard {
                    self.emit(step(t, t));
                    self.emit_put_super(base, key, t)?;
                } else {
                    self.emit(Insn::ToNumeric { dst: t, src: t });
                    let n = self.temp()?;
                    self.emit(step(n, t));
                    self.emit_put_super(base, key, n)?;
                }
                self.release(mark);
                Ok(t)
            }
            ExprKind::Call { .. } => {
                self.effect(target)?;
                let context = if prefix {
                    INVALID_TARGET_PREFIX
                } else {
                    INVALID_TARGET_POSTFIX
                };
                self.emit(Insn::ThrowInvalidAssign { context });
                self.target(dst)
            }
            _ => Err(Self::internal("an update of an invalid target")),
        }
    }

    /// The object and the key of a property that `++` or `--` updates
    /// (the key converted once).
    fn update_reference(&mut self, target: ExprId) -> CResult<(Reg, KeyReg)> {
        Ok(match self.ast.expr(target).kind {
            ExprKind::PrivateMember { object, name, .. } => {
                let obj = self.expr(object, None)?;
                let key = self.private_key(name)?;
                self.position = self.private_position(target);
                (obj, key)
            }
            ExprKind::Member {
                object, property, ..
            } => {
                let obj = self.expr(object, None)?;
                (obj, KeyReg::Site(self.site(self.name_text(property))))
            }
            ExprKind::Index { object, index, .. } => {
                if let Some(text) = self.constant_key(index) {
                    let obj = self.expr(object, None)?;
                    (obj, KeyReg::Site(self.site(text)))
                } else {
                    let obj = self.expr_stable(object, &[index])?;
                    let k = self.temp()?;
                    self.expr(index, Some(k))?;
                    self.emit(Insn::ToKey { dst: k, src: k });
                    (obj, KeyReg::Register(k))
                }
            }
            _ => return Err(Self::internal("an update target")),
        })
    }

    // --- Calls ---

    /// A call, `new` or a tagged template (§13.3.6, §13.3.5, §13.3.11):
    /// the callee, `this` and the arguments in consecutive registers; the
    /// result in the callee's register. A member callee gives `this`.
    fn call(
        &mut self,
        callee: ExprId,
        args: CallArgs,
        dst: Option<Reg>,
        kind: CallKind,
    ) -> CResult<Reg> {
        let is_new = kind == CallKind::New;
        let (exprs, template) = match args {
            CallArgs::List(list) => (self.ast.exprs(list).to_vec(), None),
            CallArgs::Template(id) => {
                let template = *self.ast.template(id);
                (
                    self.ast.exprs(template.expressions).to_vec(),
                    Some(template),
                )
            }
        };
        let count = exprs.len() + usize::from(template.is_some());
        let Ok(argc) = u16::try_from(count) else {
            return Err(super::CompileError::Script {
                kind: crate::error::ThrowKind::SyntaxError,
                message: TOO_MANY_ARGUMENTS.into(),
                offset: self.position,
            });
        };
        let call_position = if is_new {
            self.position
        } else if template.is_some() {
            self.template_position(callee, self.position)
        } else {
            self.callee_position(callee, self.position)
        };
        let spread = exprs
            .iter()
            .any(|&e| matches!(self.ast.expr(e).kind, ExprKind::Spread(_)));
        // With spread, the arguments go into an array in `base + 2`.
        let base = self.temps(2 + if spread { 1 } else { count })?;
        let target = self.unparen(callee);
        self.callee_and_this(callee, target, base, is_new)?;
        if kind == (CallKind::Call { optional: true }) {
            self.chain_jump(base)?;
        }
        let mut next = base + 2;
        if let Some(template) = template {
            let index = self.template_const(template);
            self.emit(Insn::GetTemplate { dst: next, index });
            next += 1;
        }
        if spread {
            self.emit(Insn::NewArray { dst: next });
            for &argument in &exprs {
                self.array_element(next, argument, ITER_SPREAD, Some(call_position))?;
            }
        } else {
            for &argument in &exprs {
                self.expr(argument, Some(next))?;
                next += 1;
            }
        }
        let text = self
            .render(callee, RENDER_DEPTH)
            .unwrap_or_else(|| "(intermediate value)".to_owned());
        let text = self.string_const(String16::from(text.as_str()));
        self.position = call_position;
        let pc = if kind == (CallKind::Call { optional: false }) && template.is_none() {
            self.emit_call(target, base, argc, spread)?
        } else {
            match (is_new, spread) {
                (true, false) => self.emit(Insn::New { callee: base, argc }),
                (false, false) => self.emit(Insn::Call { callee: base, argc }),
                (true, true) => self.emit(Insn::NewSpread { callee: base }),
                (false, true) => self.emit(Insn::CallSpread { callee: base }),
            }
        };
        self.call_names.push((pc as u32, text));
        match dst {
            Some(dst) if dst != base => {
                self.emit(Insn::Move { dst, src: base });
                self.release(u32::from(base));
                Ok(dst)
            }
            _ => {
                self.release(u32::from(base) + 1);
                Ok(base)
            }
        }
    }

    /// The callee of a call into `base` and its `this` into `base + 1`
    /// (§13.3.6.2 `EvaluateCall`): the object of a member access, the
    /// object of a `with` statement that has the name, else `undefined`
    /// (nothing for `new`).
    fn callee_and_this(
        &mut self,
        callee: ExprId,
        target: ExprId,
        base: Reg,
        is_new: bool,
    ) -> CResult<()> {
        if !is_new && self.method_callee(target, base)? {
            return Ok(());
        }
        let this = base + 1;
        match self.ast.expr(target).kind {
            ExprKind::Identifier(ident)
                if !is_new && self.dynamic_callee(ident.reference, base)? => {}
            ExprKind::OptionalChain(inner)
                if !is_new
                    && matches!(
                        self.ast.expr(inner).kind,
                        ExprKind::Member { .. }
                            | ExprKind::Index { .. }
                            | ExprKind::PrivateMember { .. }
                    ) =>
            {
                // `(a?.b)()`: the chain keeps its reference, so `this` is
                // `a` (§13.3.9.1); a short circuit calls `undefined`.
                self.chain_exits.push(Vec::new());
                let result = self.method_callee(inner, base);
                let exits = self.chain_exits.pop().unwrap_or_default();
                result?;
                self.chain_end(exits, base);
            }
            _ => {
                self.expr(callee, Some(base))?;
                if !is_new {
                    self.emit(Insn::Undefined { dst: this });
                }
            }
        }
        Ok(())
    }

    /// The call instruction of a plain call `f(...)` with its window at
    /// `base`: `eval(...)` (also `(eval)(...)`) is a direct eval if the
    /// callee is `%eval%` at run time (§13.3.6.1; the parser counted it
    /// too).
    fn emit_call(&mut self, target: ExprId, base: Reg, argc: u16, spread: bool) -> CResult<usize> {
        let direct_eval = matches!(
            self.ast.expr(target).kind,
            ExprKind::Identifier(ident) if ident.name == swb_js_syntax::names::EVAL
        );
        let pc = match (direct_eval, spread) {
            (false, false) => return Ok(self.emit(Insn::Call { callee: base, argc })),
            (false, true) => return Ok(self.emit(Insn::CallSpread { callee: base })),
            (true, false) => self.emit(Insn::CallEval { callee: base, argc }),
            (true, true) => self.emit(Insn::CallEvalSpread { callee: base }),
        };
        self.note_eval_site(pc, target)?;
        Ok(pc)
    }

    /// The callee `target` of a call if it is a member access: the
    /// object into `base + 1` (`this`) and the function into `base`.
    /// Returns false (and emits nothing) for any other callee.
    fn method_callee(&mut self, target: ExprId, base: Reg) -> CResult<bool> {
        let this = base + 1;
        match self.ast.expr(target).kind {
            ExprKind::Member {
                object,
                property,
                optional,
            } => {
                self.expr(object, Some(this))?;
                if optional {
                    self.chain_jump(this)?;
                }
                let site = self.site(self.name_text(property));
                self.position = self.read_position(target);
                self.emit(Insn::GetNamed {
                    dst: base,
                    obj: this,
                    site,
                });
            }
            ExprKind::Index {
                object,
                index,
                optional,
            } => {
                self.expr(object, Some(this))?;
                if optional {
                    self.chain_jump(this)?;
                }
                if let Some(text) = self.constant_key(index) {
                    let site = self.site(text);
                    self.position = self.bracket_position(target);
                    self.emit(Insn::GetNamed {
                        dst: base,
                        obj: this,
                        site,
                    });
                } else {
                    let mark = self.mark();
                    let key = self.expr(index, None)?;
                    self.position = self.bracket_position(target);
                    self.emit(Insn::GetIndex {
                        dst: base,
                        obj: this,
                        key,
                    });
                    self.release(mark);
                }
            }
            ExprKind::PrivateMember {
                object,
                name,
                optional,
            } => {
                self.expr(object, Some(this))?;
                if optional {
                    self.chain_jump(this)?;
                }
                let mark = self.mark();
                let key = self.private_key(name)?;
                self.position = self.private_read_position(target);
                self.emit_get(base, this, key);
                self.release(mark);
            }
            ExprKind::SuperMember { .. } | ExprKind::SuperIndex { .. } => {
                self.super_callee(target, base)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The position of the read of `a.b`: Chromium reports a read that
    /// is a link of an optional chain at its `.`, any other at the name.
    pub(super) fn read_position(&self, member: ExprId) -> u32 {
        if self.chain_links.contains(&member) {
            self.dot_position(member)
        } else {
            self.name_position(member)
        }
    }

    /// The constant of a tagged template's strings: one per site, as
    /// each site has its own template object (§13.2.8.4).
    fn template_const(&mut self, template: swb_js_syntax::Template) -> u32 {
        let parts = self
            .ast
            .template_elements(template.quasis)
            .iter()
            .map(|element| {
                let cooked = element.cooked.map(|id| self.ast.string(id).clone());
                (cooked, self.ast.string(element.raw).clone())
            })
            .collect();
        self.push_constant(super::ConstSpec::Template(parts))
    }

    // --- Optional chains ---

    /// An optional chain (§13.3.9): the links with `?.` jump to the end
    /// of the chain when their object (or callee) is `undefined` or
    /// `null`; the chain's value is then `undefined`.
    fn optional_chain(&mut self, inner: ExprId, dst: Option<Reg>) -> CResult<Reg> {
        let t = self.target(dst)?;
        let links = self.chain_links.len();
        let mut link = inner;
        loop {
            self.chain_links.push(link);
            link = match self.ast.expr(link).kind {
                ExprKind::Member { object, .. }
                | ExprKind::Index { object, .. }
                | ExprKind::PrivateMember { object, .. } => object,
                ExprKind::Call { callee, .. } => callee,
                _ => break,
            };
        }
        self.chain_exits.push(Vec::new());
        let result = self.expr(inner, Some(t));
        let exits = self.chain_exits.pop().unwrap_or_default();
        self.chain_links.truncate(links);
        result?;
        self.chain_end(exits, t);
        Ok(t)
    }

    /// The short-circuit test of a `?.` link on the value in `register`.
    fn chain_jump(&mut self, register: Reg) -> CResult<()> {
        let at = self.emit(Insn::JumpIfNullish {
            src: register,
            offset: 0,
        });
        self.chain_exits
            .last_mut()
            .ok_or(Self::internal("an optional link outside a chain"))?
            .push(at);
        Ok(())
    }

    /// The end of a chain: the short circuits put `undefined` into
    /// `result`.
    fn chain_end(&mut self, exits: Vec<usize>, result: Reg) {
        self.short_circuit(exits, Insn::Undefined { dst: result });
    }

    /// Patches `exits` to an instruction `insn` that the normal path
    /// skips.
    fn short_circuit(&mut self, exits: Vec<usize>, insn: Insn) {
        if exits.is_empty() {
            return;
        }
        let skip = self.emit(Insn::Jump { offset: 0 });
        for exit in exits {
            self.patch_here(exit);
        }
        self.emit(insn);
        self.patch_here(skip);
    }

    /// `delete` of a property access into `t`. The access may be the
    /// last link of an optional chain (`delete a?.b`, §13.5.1.2): the
    /// result is `true` when the chain short-circuits.
    fn delete_property_of(&mut self, target: ExprId, t: Reg, chain: bool) -> CResult<()> {
        if chain {
            self.chain_exits.push(Vec::new());
            let result = self.delete_property_of(target, t, false);
            let exits = self.chain_exits.pop().unwrap_or_default();
            result?;
            self.short_circuit(
                exits,
                Insn::Bool {
                    dst: t,
                    value: true,
                },
            );
            return Ok(());
        }
        match self.ast.expr(target).kind {
            ExprKind::Member {
                object,
                property,
                optional,
            } => {
                let obj = self.expr(object, None)?;
                if optional {
                    self.chain_jump(obj)?;
                }
                let site = self.site(self.name_text(property));
                self.emit(Insn::DeleteNamed { dst: t, obj, site });
            }
            ExprKind::Index {
                object,
                index,
                optional,
            } => {
                let obj = self.expr_stable(object, &[index])?;
                if optional {
                    self.chain_jump(obj)?;
                }
                let key = self.expr(index, None)?;
                self.emit(Insn::DeleteIndex { dst: t, obj, key });
            }
            _ => return Err(Self::internal("a delete of a non-property")),
        }
        Ok(())
    }

    /// The text of a callee for "... is not a function" (V8 shows the
    /// expression).
    pub(super) fn render(&self, e: ExprId, depth: u32) -> Option<String> {
        if depth == 0 {
            return None;
        }
        let text = |name| self.name_text(name).to_string_lossy();
        match self.ast.expr(e).kind {
            ExprKind::Identifier(ident) => Some(text(ident.name)),
            ExprKind::This(_) => Some("this".to_owned()),
            // V8 shows a literal as its value (a string in double quotes,
            // without escapes).
            ExprKind::Null => Some("null".to_owned()),
            ExprKind::Boolean(value) => Some(value.to_string()),
            ExprKind::Number(value) => Some(number_to_string(value)),
            ExprKind::String(id) => Some(format!("\"{}\"", self.ast.string(id).to_string_lossy())),
            ExprKind::Paren(inner) | ExprKind::OptionalChain(inner) => self.render(inner, depth),
            ExprKind::Member {
                object,
                property,
                optional,
            } => Some(format!(
                "{}{}{}",
                self.render(object, depth - 1)
                    .unwrap_or_else(|| "(intermediate value)".to_owned()),
                if optional { "?." } else { "." },
                text(property)
            )),
            ExprKind::Index {
                object,
                index,
                optional,
            } => {
                let object = self
                    .render(object, depth - 1)
                    .unwrap_or_else(|| "(intermediate value)".to_owned());
                let (dot, link) = if optional { ("?.", "?.") } else { (".", "") };
                match self.ast.expr(self.unparen(index)).kind {
                    ExprKind::String(id) => Some(format!(
                        "{object}{dot}{}",
                        self.ast.string(id).to_string_lossy()
                    )),
                    ExprKind::Number(value) => {
                        Some(format!("{object}{link}[{}]", number_to_string(value)))
                    }
                    ExprKind::Identifier(ident) => {
                        Some(format!("{object}{link}[{}]", text(ident.name)))
                    }
                    _ => None,
                }
            }
            // V8 shows a private member as an index (`o[#x]`, `o?.[#x]`).
            ExprKind::PrivateMember {
                object,
                name,
                optional,
            } => Some(format!(
                "{}{}[{}]",
                self.render(object, depth - 1)
                    .unwrap_or_else(|| "(intermediate value)".to_owned()),
                if optional { "?." } else { "" },
                text(name.name)
            )),
            ExprKind::SuperMember { property, .. } => {
                Some(format!("(intermediate value).{}", text(property)))
            }
            ExprKind::SuperIndex { index, .. } => match self.ast.expr(self.unparen(index)).kind {
                ExprKind::String(id) => Some(format!(
                    "(intermediate value).{}",
                    self.ast.string(id).to_string_lossy()
                )),
                ExprKind::Number(value) => {
                    Some(format!("(intermediate value)[{}]", number_to_string(value)))
                }
                _ => Some(format!(
                    "(intermediate value)[{}]",
                    self.render(index, depth - 1)?
                )),
            },
            // V8 shows a tagged template as a call of its tag.
            ExprKind::Call { callee, .. } | ExprKind::TaggedTemplate { tag: callee, .. } => {
                Some(format!(
                    "{}(...)",
                    self.render(callee, depth - 1)
                        .unwrap_or_else(|| "(intermediate value)".to_owned())
                ))
            }
            _ => None,
        }
    }
}

/// The arguments of a call: a list, or a tagged template (the template
/// object and the substitutions).
#[derive(Clone, Copy)]
enum CallArgs {
    List(List<ExprId>),
    Template(swb_js_syntax::TemplateId),
}

/// What a call instruction does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallKind {
    /// `[[Call]]`; `optional` for `f?.()`.
    Call { optional: bool },
    /// `new`.
    New,
}

/// The key of a property assignment target.
enum PropertyRef {
    Named(String16),
    Computed(ExprId),
    Private(Ident),
}

/// A compiled property key: a site, a register, or a private name (its
/// class's private environment in `env` and its index there).
#[derive(Clone, Copy)]
pub(super) enum KeyReg {
    Site(u32),
    Register(Reg),
    Private { env: Reg, index: u32 },
}
