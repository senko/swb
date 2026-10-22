//! Assignment and update expressions (§13.15, §13.4): the targets, the
//! compound and logical forms, and `++` and `--`.

use swb_js_syntax::{AssignOp, AssignTarget, ExprId, ExprKind, Ident, LogicalOp, UpdateOp};

use super::CResult;
use super::expr::{KeyReg, PropertyRef};
use super::function::FunctionCompiler;
use crate::bytecode::{
    INVALID_TARGET_ASSIGN, INVALID_TARGET_FOR, INVALID_TARGET_POSTFIX, INVALID_TARGET_PREFIX, Insn,
    Reg,
};

impl FunctionCompiler<'_> {
    // --- Assignment and update ---

    pub(super) fn assign(
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
    pub(super) fn update(
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
}
