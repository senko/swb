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
use crate::bytecode::{Insn, Reg};
use crate::vm::number::number_to_string;

/// How many nodes `may_write_locals` looks at before it gives up.
const SCAN_LIMIT: usize = 32;

/// The deepest callee that error messages render.
const RENDER_DEPTH: u32 = 8;

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
            ExprKind::This(reference) => self.load_ref(reference, dst),
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
            ExprKind::Template(id) => self.template(id, dst),
            ExprKind::RegExp { .. } => Err(self.unsupported("regular expression literal")),
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
                object, property, ..
            } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                let obj = self.expr(object, None)?;
                let site = self.site(self.name_text(property));
                self.position = self.name_position(e);
                self.emit(Insn::GetNamed { dst: t, obj, site });
                self.release(mark);
                Ok(t)
            }
            ExprKind::Index { object, index, .. } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                if let Some(text) = self.constant_key(index) {
                    let obj = self.expr(object, None)?;
                    let site = self.site(text);
                    self.position = self.bracket_position(e);
                    self.emit(Insn::GetNamed { dst: t, obj, site });
                } else {
                    let obj = self.expr_stable(object, &[index])?;
                    let key = self.expr(index, None)?;
                    self.position = self.bracket_position(e);
                    self.emit(Insn::GetIndex { dst: t, obj, key });
                }
                self.release(mark);
                Ok(t)
            }
            ExprKind::Call {
                callee, arguments, ..
            } => self.call(callee, arguments, dst, false),
            ExprKind::New { callee, arguments } => self.call(callee, arguments, dst, true),
            ExprKind::Spread(_)
            | ExprKind::OptionalChain(_)
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::NewTarget(_)
            | ExprKind::BigInt(_)
            | ExprKind::Class(_)
            | ExprKind::Await(_)
            | ExprKind::SuperMember { .. }
            | ExprKind::SuperIndex { .. }
            | ExprKind::SuperCall(_)
            | ExprKind::PrivateMember { .. }
            | ExprKind::PrivateIn { .. }
            | ExprKind::ImportCall { .. }
            | ExprKind::ImportMeta => {
                Err(self
                    .unsupported(super::support::unsupported_expr(kind).unwrap_or("expression")))
            }
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
                self.emit(Insn::Yield { reg: t });
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

    // --- Bindings ---

    /// Loads the binding of an identifier occurrence.
    ///
    /// A reference with a dynamic lookup (`ScopeTree::dynamic_lookup`: in a
    /// `with` body or in reach of a sloppy direct `eval`) compiles to its
    /// static fallback only. That is correct while the VM has no `eval`
    /// and the compiler rejects `with` (M7 feature 3 adds both): no
    /// dynamic environment can hold a binding then.
    pub(super) fn load_ref(&mut self, id: RefId, dst: Option<Reg>) -> CResult<Reg> {
        let reference = self.reference(id);
        let name = || reference.name;
        match reference.resolution {
            Resolution::Register(r) => {
                if reference.tdz_check {
                    let name = self.name_const(name());
                    self.emit(Insn::CheckTdz { src: r, name });
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
                let name = self.name_const(name());
                self.emit(Insn::GetGlobal { dst: t, name });
                Ok(t)
            }
            Resolution::Unresolved => Err(Self::internal("an unresolved reference")),
            // Only eval code has these, and nothing compiles eval code yet.
            Resolution::Caller => Err(self.unsupported("eval code")),
        }
    }

    fn check_tdz(&mut self, reference: &swb_js_syntax::Reference, register: Reg) {
        if reference.tdz_check {
            let name = self.name_const(reference.name);
            self.emit(Insn::CheckTdz {
                src: register,
                name,
            });
        }
    }

    /// `PutValue` on an identifier (an assignment, not a declaration):
    /// checks the dead zone, rejects constants.
    pub(super) fn store_ref(&mut self, id: RefId, src: Reg) -> CResult<()> {
        let reference = self.reference(id);
        let kind = self.binding_kind(&reference);
        let constant = kind == Some(BindingKind::Const)
            || (kind == Some(BindingKind::FunctionName) && self.strict);
        let ignored = kind == Some(BindingKind::FunctionName) && !self.strict;
        let name = self.name_const(reference.name);
        match reference.resolution {
            Resolution::Global => {
                self.emit(Insn::SetGlobal { src, name });
            }
            Resolution::Register(r) => {
                if reference.tdz_check {
                    self.emit(Insn::CheckTdz { src: r, name });
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
            Resolution::Caller => return Err(self.unsupported("eval code")),
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
        self.expr_named(value, Some(t), name)?;
        self.store_ref(id, t)?;
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
                )
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
                | ExprKind::NewTarget(_)
                | ExprKind::BigInt(_)
                | ExprKind::Class(_)
                | ExprKind::Await(_)
                | ExprKind::SuperMember { .. }
                | ExprKind::SuperIndex { .. }
                | ExprKind::SuperCall(_)
                | ExprKind::PrivateMember { .. }
                | ExprKind::PrivateIn { .. }
                | ExprKind::ImportCall { .. }
                | ExprKind::ImportMeta => return true,
                ExprKind::Identifier(_)
                | ExprKind::This(_)
                | ExprKind::Null
                | ExprKind::Boolean(_)
                | ExprKind::Number(_)
                | ExprKind::String(_)
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
                | ExprKind::Member { object: inner, .. } => work.push(inner),
                ExprKind::Yield { argument, .. } => work.extend(argument),
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
                    ExprKind::Member {
                        object, property, ..
                    } => {
                        let obj = self.expr(object, None)?;
                        let site = self.site(self.name_text(property));
                        self.emit(Insn::DeleteNamed { dst: t, obj, site });
                    }
                    ExprKind::Index { object, index, .. } => {
                        let obj = self.expr_stable(object, &[index])?;
                        let key = self.expr(index, None)?;
                        self.emit(Insn::DeleteIndex { dst: t, obj, key });
                    }
                    ExprKind::Identifier(ident) => {
                        let reference = self.reference(ident.reference);
                        if reference.resolution == Resolution::Global {
                            let name = self.name_const(ident.name);
                            self.emit(Insn::DeleteGlobal { dst: t, name });
                        } else {
                            self.emit(Insn::Bool {
                                dst: t,
                                value: false,
                            });
                        }
                    }
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
                match self.ast.expr(inner).kind {
                    ExprKind::Identifier(ident)
                        if self.reference(ident.reference).resolution == Resolution::Global =>
                    {
                        let name = self.name_const(ident.name);
                        self.emit(Insn::TypeofGlobal { dst: t, name });
                    }
                    _ => {
                        let src = self.expr(argument, None)?;
                        self.emit(Insn::Typeof { dst: t, src });
                    }
                }
            }
            UnaryOp::Minus => {
                if let ExprKind::Number(value) = self.ast.expr(self.unparen(argument)).kind {
                    self.load_number(-value, t);
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
            if self.ast.expr(item).kind == ExprKind::Hole {
                self.emit(Insn::Hole { array });
            } else {
                let inner = self.mark();
                let v = self.expr(item, None)?;
                self.emit(Insn::Push { array, src: v });
                self.release(inner);
            }
        }
        self.emit(Insn::Move { dst: t, src: array });
        self.release(mark);
        Ok(t)
    }

    /// The text of a literal property key.
    fn key_text(&self, key: AstKey) -> Option<String16> {
        match key {
            AstKey::Name(name) => Some(self.name_text(name)),
            AstKey::Number(value) => Some(String16::from(number_to_string(value).as_str())),
            AstKey::Computed(_) | AstKey::BigInt(_) => None,
        }
    }

    /// An object literal (§13.2.5.4).
    fn object(
        &mut self,
        properties: List<swb_js_syntax::Property>,
        dst: Option<Reg>,
    ) -> CResult<Reg> {
        let t = self.target(dst)?;
        let mark = self.mark();
        let object = self.temp()?;
        self.emit(Insn::NewObject { dst: object });
        for property in self.ast.properties(properties).to_vec() {
            self.position = property.span.start;
            let inner = self.mark();
            match property.kind {
                PropertyKind::Init | PropertyKind::Shorthand | PropertyKind::Method => {
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
                        let v = self.expr(property.value, None)?;
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
                    return Err(self.unsupported("getter or setter"));
                }
                PropertyKind::Spread => return Err(self.unsupported("object spread")),
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

    /// The key text of a constant index expression (`o["x"]`, `o[0]`).
    pub(super) fn constant_key(&self, index: ExprId) -> Option<String16> {
        match self.ast.expr(self.unparen(index)).kind {
            ExprKind::String(id) => Some(self.ast.string(id).clone()),
            ExprKind::Number(value) => Some(String16::from(number_to_string(value).as_str())),
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
        let AssignTarget::Simple(target) = target else {
            return Err(self.unsupported("destructuring assignment"));
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
            ExprKind::Call { .. } => {
                // Annex B web compatibility: the call runs, then the
                // assignment throws (Chromium's behaviour).
                self.effect(target)?;
                self.emit(Insn::ThrowInvalidAssign);
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
                self.load_ref(ident.reference, Some(t))?;
                let mark = self.mark();
                let b = self.expr(value, None)?;
                self.emit(Self::binary_insn(binary, t, t, b));
                self.release(mark);
                self.store_ref(ident.reference, t)?;
                Ok(t)
            }
            AssignOp::Logical(logical) => {
                let t = self.target(dst)?;
                self.load_ref(ident.reference, Some(t))?;
                let skip = self.emit(Self::logical_skip(logical, t));
                self.expr_named(value, Some(t), name)?;
                self.store_ref(ident.reference, t)?;
                self.patch_here(skip);
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

    fn emit_get(&mut self, dst: Reg, obj: Reg, key: KeyReg) {
        match key {
            KeyReg::Site(site) => self.emit(Insn::GetNamed { dst, obj, site }),
            KeyReg::Register(key) => self.emit(Insn::GetIndex { dst, obj, key }),
        };
    }

    fn emit_put(&mut self, obj: Reg, key: KeyReg, src: Reg) {
        match key {
            KeyReg::Site(site) => self.emit(Insn::SetNamed { obj, src, site }),
            KeyReg::Register(key) => self.emit(Insn::SetIndex { obj, key, src }),
        };
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
                self.load_ref(ident.reference, Some(t))?;
                if prefix || discard {
                    self.emit(step(t, t));
                    self.store_ref(ident.reference, t)?;
                } else {
                    self.emit(Insn::ToNumeric { dst: t, src: t });
                    let mark = self.mark();
                    let n = self.temp()?;
                    self.emit(step(n, t));
                    self.store_ref(ident.reference, n)?;
                    self.release(mark);
                }
                Ok(t)
            }
            ExprKind::Member { .. } | ExprKind::Index { .. } => {
                let t = self.target(dst)?;
                let mark = self.mark();
                let (obj, key) = match self.ast.expr(target).kind {
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
                };
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
            ExprKind::Call { .. } => {
                self.effect(target)?;
                self.emit(Insn::ThrowInvalidAssign);
                self.target(dst)
            }
            _ => Err(Self::internal("an update of an invalid target")),
        }
    }

    // --- Calls ---

    /// A call or `new` (§13.3.6, §13.3.5): the callee, `this` and the
    /// arguments in consecutive registers; the result in the callee's
    /// register.
    fn call(
        &mut self,
        callee: ExprId,
        arguments: List<ExprId>,
        dst: Option<Reg>,
        is_new: bool,
    ) -> CResult<Reg> {
        let args = self.ast.exprs(arguments).to_vec();
        let Ok(argc) = u16::try_from(args.len()) else {
            return Err(super::CompileError::Script {
                kind: crate::error::ThrowKind::SyntaxError,
                message: TOO_MANY_ARGUMENTS.into(),
                offset: self.position,
            });
        };
        let call_position = if is_new {
            self.position
        } else {
            self.callee_position(callee, self.position)
        };
        let base = self.temps(2 + args.len())?;
        let this = base + 1;
        let target = self.unparen(callee);
        match self.ast.expr(target).kind {
            ExprKind::Member {
                object, property, ..
            } if !is_new => {
                self.expr(object, Some(this))?;
                let site = self.site(self.name_text(property));
                self.position = self.name_position(target);
                self.emit(Insn::GetNamed {
                    dst: base,
                    obj: this,
                    site,
                });
            }
            ExprKind::Index { object, index, .. } if !is_new => {
                self.expr(object, Some(this))?;
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
            _ => {
                self.expr(callee, Some(base))?;
                if !is_new {
                    self.emit(Insn::Undefined { dst: this });
                }
            }
        }
        for (i, &argument) in args.iter().enumerate() {
            self.expr(argument, Some(base + 2 + i as Reg))?;
        }
        let text = self
            .render(callee, RENDER_DEPTH)
            .unwrap_or_else(|| "(intermediate value)".to_owned());
        let text = self.string_const(String16::from(text.as_str()));
        self.position = call_position;
        let pc = if is_new {
            self.emit(Insn::New { callee: base, argc })
        } else {
            self.emit(Insn::Call { callee: base, argc })
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

    /// The text of a callee for "... is not a function" (V8 shows the
    /// expression).
    fn render(&self, e: ExprId, depth: u32) -> Option<String> {
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
            ExprKind::Paren(inner) => self.render(inner, depth),
            ExprKind::Member {
                object, property, ..
            } => Some(format!(
                "{}.{}",
                self.render(object, depth - 1)
                    .unwrap_or_else(|| "(intermediate value)".to_owned()),
                text(property)
            )),
            ExprKind::Index { object, index, .. } => {
                let object = self
                    .render(object, depth - 1)
                    .unwrap_or_else(|| "(intermediate value)".to_owned());
                match self.ast.expr(self.unparen(index)).kind {
                    ExprKind::String(id) => Some(format!(
                        "{object}.{}",
                        self.ast.string(id).to_string_lossy()
                    )),
                    ExprKind::Number(value) => {
                        Some(format!("{object}[{}]", number_to_string(value)))
                    }
                    ExprKind::Identifier(ident) => Some(format!("{object}[{}]", text(ident.name))),
                    _ => None,
                }
            }
            ExprKind::Call { callee, .. } => Some(format!(
                "{}(...)",
                self.render(callee, depth - 1)
                    .unwrap_or_else(|| "(intermediate value)".to_owned())
            )),
            _ => None,
        }
    }
}

/// The key of a property assignment target.
enum PropertyRef {
    Named(String16),
    Computed(ExprId),
}

/// A compiled property key: a site or a register.
#[derive(Clone, Copy)]
enum KeyReg {
    Site(u32),
    Register(Reg),
}
