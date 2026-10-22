//! Destructuring: binding patterns (§8.6.2 `BindingInitialization`,
//! §8.6.3 `IteratorBindingInitialization`) in declarations, parameters,
//! `catch` and the heads of `for`-`in` and `for`-`of`, and assignment
//! patterns (§13.15.5 `DestructuringAssignmentEvaluation`).
//!
//! The order of §13.15.5.5 and §13.15.5.6: a target that is not a
//! pattern (`a.b`, `a[k]`) is evaluated before its value is taken (the
//! property read or the iterator step); then the default value (only for
//! `undefined`; an anonymous function gets the name of an identifier
//! target), then the store (`PutValue`, where `a[k]` converts its key).
//! Computed keys of an object pattern are evaluated once, in order, before
//! their targets.
//!
//! An object pattern first checks its value (`RequireObjectCoercible`,
//! with V8's messages, which depend on where the value comes from). An
//! array pattern gets an iterator record in registers and runs its
//! elements in an implicit `finally` block that closes the iterator on
//! every abrupt completion (a throw, or the return of a generator that
//! is suspended in a default value) unless the record is done; after
//! the last element it closes the iterator if the record is not done.
//! A failure of the iterator itself (`next`, its result) leaves the
//! record in a state that counts as done, so the iterator is not closed
//! then. Object rest and
//! array rest build their object or array while they read.

use swb_js_syntax::{
    ExprId, ExprKind, PatternId, PatternKind, PatternProperty, PropertyKey as AstKey, RefId,
    ScopeId, VariableKind,
};
use swb_js_text::String16;

use super::CResult;
use super::expr::{KeyReg, RENDER_DEPTH};
use super::function::{EXPR_WEIGHT, FinallyBody, FunctionCompiler};
use super::iterate::GENERIC_HINT;
use crate::bytecode::{
    COERCIBLE_PARAMETER, COERCIBLE_READ, COERCIBLE_RENDERED, COERCIBLE_VALUE, HINT_PATTERN,
    INVALID_TARGET_ASSIGN, ITER_RENDERED, Insn, Reg,
};

/// How the leaves of a pattern receive their values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BindMode {
    /// `let`, `const`, parameters, `catch`: the binding is initialized.
    Init,
    /// `var`: `PutValue` on the binding.
    Var,
    /// A destructuring assignment: `PutValue` on the target.
    Assign,
}

/// Where the value of a pattern comes from; V8's messages name it.
#[derive(Clone, Copy)]
pub(super) enum PatternSource {
    /// An expression: a declaration's initializer, the right side of an
    /// assignment.
    Expr(ExprId),
    /// A declaration in the head of `for`-`in` or `for`-`of`.
    ForHead,
    /// A `catch` parameter.
    Catch,
    /// A parameter.
    Parameter,
    /// An element or property of another pattern, or an assignment
    /// pattern in a loop head.
    Nested,
}

/// An assignment target that is evaluated before its value exists.
#[derive(Clone, Copy)]
enum TargetRef {
    /// An identifier (resolving it has no effect).
    Binding(RefId),
    /// A property with its object in a register.
    Property { obj: Reg, key: KeyReg },
    /// A `super` property: the super base and `this` in `base` and
    /// `base + 1`.
    Super { base: Reg, key: KeyReg },
    /// A private member with its object in a register and the position
    /// of its write (Chromium's, see `private_position`).
    Private {
        obj: Reg,
        key: KeyReg,
        position: u32,
    },
    /// A call (Annex B web compatibility): it has run; the store throws.
    Call,
}

impl FunctionCompiler<'_> {
    /// Destructures the value in `value` into the pattern.
    pub(super) fn bind_pattern(
        &mut self,
        pattern: PatternId,
        value: Reg,
        mode: BindMode,
        source: PatternSource,
    ) -> CResult<()> {
        self.enter(EXPR_WEIGHT)?;
        let node = *self.ast.pattern(pattern);
        let outer = std::mem::replace(&mut self.position, node.span.start);
        let mark = self.mark();
        // Where a value without an iterator is reported (Chromium).
        let source_position = match source {
            PatternSource::Expr(_) if mode == BindMode::Assign => {
                self.token_position(node.span.end)
            }
            PatternSource::Expr(e) => self.ast.expr(e).span.start,
            _ => node.span.start,
        };
        match node.kind {
            PatternKind::Identifier(ident) => match mode {
                BindMode::Init => {
                    let binding = self
                        .reference(ident.reference)
                        .binding
                        .ok_or(Self::internal("a binding pattern without a binding"))?;
                    self.init_binding(binding, value)?;
                }
                BindMode::Var | BindMode::Assign => self.store_ref(ident.reference, value)?,
            },
            PatternKind::Expr(target) => {
                let inner = self.unparen(target);
                if let ExprKind::PrivateMember { .. } = self.ast.expr(inner).kind {
                    self.position = self.private_position(inner);
                }
                self.assign_to_target(target, value)?;
            }
            PatternKind::Object { properties, rest } => {
                let properties = self.ast.pattern_properties(properties).to_vec();
                self.object_pattern(&properties, rest, value, mode, source)?;
            }
            PatternKind::Array(elements) => {
                let elements = self.ast.patterns(elements).to_vec();
                self.position = source_position;
                self.array_pattern(&elements, value, mode, source)?;
            }
            PatternKind::Default { .. } | PatternKind::Rest(_) | PatternKind::Hole => {
                return Err(Self::internal("a pattern element outside a pattern"));
            }
        }
        self.release(mark);
        self.position = outer;
        self.leave(EXPR_WEIGHT);
        Ok(())
    }

    /// The pattern of a `for`-`in` or `for`-`of` declaration: `let` and
    /// `const` bindings are new in each iteration (§14.7.5.7 step 6.g).
    pub(super) fn bind_head_pattern(
        &mut self,
        kind: VariableKind,
        target: PatternId,
        value: Reg,
        scope: ScopeId,
    ) -> CResult<()> {
        if kind == VariableKind::Var {
            return self.bind_pattern(target, value, BindMode::Var, PatternSource::ForHead);
        }
        self.enter_scope_bindings(scope)?;
        self.bind_pattern(target, value, BindMode::Init, PatternSource::ForHead)
    }

    /// `pattern = value` as an expression (§13.15.2): its value is the
    /// value of the right side.
    pub(super) fn destructuring_assignment(
        &mut self,
        pattern: PatternId,
        value: ExprId,
        dst: Option<Reg>,
    ) -> CResult<Reg> {
        let t = self.target(dst)?;
        let mark = self.mark();
        let v = self.temp()?;
        self.expr(value, Some(v))?;
        self.bind_pattern(pattern, v, BindMode::Assign, PatternSource::Expr(value))?;
        self.emit(Insn::Move { dst: t, src: v });
        self.release(mark);
        Ok(t)
    }

    /// The name that a default value gets (`NamedEvaluation`): the name
    /// of an identifier target.
    pub(super) fn pattern_name(&self, pattern: PatternId) -> Option<String16> {
        match self.ast.pattern(pattern).kind {
            PatternKind::Identifier(ident) => Some(self.name_text(ident.name)),
            PatternKind::Expr(target) => match self.ast.expr(self.unparen(target)).kind {
                ExprKind::Identifier(ident) => Some(self.name_text(ident.name)),
                _ => None,
            },
            _ => None,
        }
    }

    /// An object pattern (§8.6.2, §13.15.5.2).
    fn object_pattern(
        &mut self,
        properties: &[PatternProperty],
        rest: Option<PatternId>,
        value: Reg,
        mode: BindMode,
        source: PatternSource,
    ) -> CResult<()> {
        match (properties.first(), rest) {
            (None, Some(rest)) => {
                self.position = self.ast.pattern(rest).span.start;
                self.require_object_coercible(value, None, source);
            }
            (first, _) => self.require_object_coercible(value, first, source),
        }
        let excluded = match rest {
            Some(_) => {
                let t = self.temp()?;
                self.emit(Insn::NewArray { dst: t });
                Some(t)
            }
            None => None,
        };
        for property in properties {
            self.position = property.span.start;
            let mark = self.mark();
            let key = match property.key {
                AstKey::Computed(e) => {
                    let k = self.temp()?;
                    self.expr(e, Some(k))?;
                    self.emit(Insn::ToKey { dst: k, src: k });
                    if let Some(array) = excluded {
                        self.emit(Insn::Push { array, src: k });
                    }
                    KeyReg::Register(k)
                }
                key => {
                    let text = self
                        .key_text(key)
                        .ok_or(Self::internal("a pattern key without a text"))?;
                    if let Some(array) = excluded {
                        let k = self.temp()?;
                        let index = self.string_const(text.clone());
                        self.emit(Insn::Const { dst: k, index });
                        self.emit(Insn::Push { array, src: k });
                    }
                    KeyReg::Site(self.site(text))
                }
            };
            let position = property.span.start;
            self.bind_element(property.value, mode, |c, v| {
                c.position = position;
                c.emit_get(v, value, key);
                Ok(())
            })?;
            self.release(mark);
        }
        if let (Some(rest), Some(excluded)) = (rest, excluded) {
            self.position = self.ast.pattern(rest).span.start;
            self.bind_element(rest, mode, |c, v| {
                c.emit(Insn::CopyRest {
                    dst: v,
                    src: value,
                    excluded,
                });
                Ok(())
            })?;
        }
        Ok(())
    }

    /// The check of an object pattern's value, with the message that V8
    /// gives for its source.
    fn require_object_coercible(
        &mut self,
        value: Reg,
        first: Option<&PatternProperty>,
        source: PatternSource,
    ) {
        let property = first
            .and_then(|p| self.key_text(p.key))
            .map(|key| key.to_string_lossy());
        let of = |property: &Option<String>| {
            property
                .as_ref()
                .map_or(String::new(), |key| format!("property '{key}' of "))
        };
        let rendered = |property: &Option<String>, text: &str| match property {
            Some(key) => format!("property '{key}' of '{text}'"),
            None => format!("'{text}'"),
        };
        // Chromium reports the check at the target of the first property.
        if let Some(property) = first
            && !matches!(property.key, AstKey::Computed(_))
        {
            let target = match self.ast.pattern(property.value).kind {
                PatternKind::Default { target, .. } => target,
                _ => property.value,
            };
            self.position = self.ast.pattern(target).span.start;
        }
        let (form, text) = match source {
            PatternSource::Nested => (
                COERCIBLE_READ,
                property
                    .as_ref()
                    .map_or(String::new(), |key| format!(" (reading '{key}')")),
            ),
            PatternSource::Parameter => (COERCIBLE_PARAMETER, of(&property)),
            PatternSource::ForHead => (COERCIBLE_RENDERED, rendered(&property, ".for")),
            PatternSource::Catch => (COERCIBLE_RENDERED, rendered(&property, ".catch")),
            PatternSource::Expr(e) => match self.render(e, RENDER_DEPTH) {
                Some(text) => (COERCIBLE_RENDERED, rendered(&property, &text)),
                None => (COERCIBLE_VALUE, of(&property)),
            },
        };
        let message = self.string_const(String16::from(text.as_str()));
        self.emit(Insn::RequireObjectCoercible {
            src: value,
            form,
            message,
        });
    }

    /// An array pattern (§8.6.3, §13.15.5.5): the elements in an
    /// implicit `finally` block that closes the iterator.
    fn array_pattern(
        &mut self,
        elements: &[PatternId],
        value: Reg,
        mode: BindMode,
        source: PatternSource,
    ) -> CResult<()> {
        let rec = self.temps(3)?;
        let hint = match source {
            PatternSource::Expr(e) if mode != BindMode::Assign => {
                self.iteration_hint(e, ITER_RENDERED, false)
            }
            PatternSource::ForHead if mode != BindMode::Assign => {
                self.text_hint(ITER_RENDERED, ".for")
            }
            _ => GENERIC_HINT,
        };
        self.emit(Insn::GetIterator {
            dst: rec,
            src: value,
            hint: hint | HINT_PATTERN,
        });
        let start = self.push_close()?;
        for &element in elements {
            let node = *self.ast.pattern(element);
            self.position = node.span.start;
            match node.kind {
                PatternKind::Hole => {
                    let mark = self.mark();
                    let t = self.temp()?;
                    self.emit_step(t, rec);
                    self.release(mark);
                }
                PatternKind::Rest(target) => self.rest_element(target, rec, mode)?,
                _ => self.bind_element(element, mode, |c, v| {
                    c.emit_step(v, rec);
                    Ok(())
                })?,
            }
        }
        self.finally_block(start, FinallyBody::Close(rec))
    }

    /// `IteratorStepValue` into `v` (`undefined` once the record is done).
    fn emit_step(&mut self, v: Reg, rec: Reg) {
        self.emit(Insn::IteratorCall { dst: v, rec });
        self.emit(Insn::IteratorValue {
            dst: v,
            rec,
            offset: 1,
        });
    }

    /// An array rest element: the rest of the iterator in a new array.
    fn rest_element(&mut self, target: PatternId, rec: Reg, mode: BindMode) -> CResult<()> {
        self.bind_element(target, mode, |c, array| {
            c.emit(Insn::NewArray { dst: array });
            c.emit(Insn::SpreadArray { array, rec });
            let mark = c.mark();
            let v = c.temp()?;
            let top = c.here();
            c.emit(Insn::IteratorCall { dst: v, rec });
            let exit = c.emit(Insn::IteratorValue {
                dst: v,
                rec,
                offset: 0,
            });
            c.emit(Insn::Push { array, src: v });
            c.jump_back(top);
            c.patch_here(exit);
            c.release(mark);
            Ok(())
        })
    }

    /// One element or property value: the target (with its default
    /// value) receives the value that `fetch` puts into a register, in
    /// the order of the module documentation.
    fn bind_element(
        &mut self,
        element: PatternId,
        mode: BindMode,
        fetch: impl FnOnce(&mut Self, Reg) -> CResult<()>,
    ) -> CResult<()> {
        let (target, default) = match self.ast.pattern(element).kind {
            PatternKind::Default { target, value } => (target, Some(value)),
            _ => (element, None),
        };
        let mark = self.mark();
        let reference = match self.ast.pattern(target).kind {
            PatternKind::Expr(e) => Some(self.target_reference(e)?),
            _ => None,
        };
        let v = self.temp()?;
        fetch(self, v)?;
        if let Some(default) = default {
            let skip = self.emit(Insn::JumpIfNotUndefined { src: v, offset: 0 });
            match self.pattern_name(target) {
                Some(name) => self.expr_named(default, Some(v), name)?,
                None => self.expr(default, Some(v))?,
            };
            self.patch_here(skip);
        }
        match reference {
            Some(reference) => {
                self.position = self.ast.pattern(target).span.start;
                self.put_reference(reference, v)?;
            }
            None => self.bind_pattern(target, v, mode, PatternSource::Nested)?,
        }
        self.release(mark);
        Ok(())
    }

    /// Evaluates an assignment target that is not a pattern (§13.15.5.5
    /// step 1.a): the object and the key of a property access.
    fn target_reference(&mut self, target: ExprId) -> CResult<TargetRef> {
        let target = self.unparen(target);
        Ok(match self.ast.expr(target).kind {
            ExprKind::Identifier(ident) => TargetRef::Binding(ident.reference),
            ExprKind::Member {
                object, property, ..
            } => {
                let obj = self.temp()?;
                self.expr(object, Some(obj))?;
                let site = self.site(self.name_text(property));
                TargetRef::Property {
                    obj,
                    key: KeyReg::Site(site),
                }
            }
            ExprKind::Index { object, index, .. } => {
                let obj = self.temp()?;
                self.expr(object, Some(obj))?;
                let key = if let Some(text) = self.constant_key(index) {
                    KeyReg::Site(self.site(text))
                } else {
                    let k = self.temp()?;
                    self.expr(index, Some(k))?;
                    KeyReg::Register(k)
                };
                TargetRef::Property { obj, key }
            }
            ExprKind::PrivateMember { object, name, .. } => {
                let obj = self.temp()?;
                self.expr(object, Some(obj))?;
                let key = self.private_key(name)?;
                TargetRef::Private {
                    obj,
                    key,
                    position: self.private_position(target),
                }
            }
            ExprKind::SuperMember { .. } | ExprKind::SuperIndex { .. } => {
                let (base, key) = self.super_reference(target)?;
                TargetRef::Super { base, key }
            }
            ExprKind::Call { .. } => {
                self.effect(target)?;
                TargetRef::Call
            }
            _ => {
                return Err(Self::internal(
                    "a destructuring target that is not a reference",
                ));
            }
        })
    }

    /// `PutValue` of `value` on an evaluated target.
    fn put_reference(&mut self, reference: TargetRef, value: Reg) -> CResult<()> {
        match reference {
            TargetRef::Binding(id) => self.store_ref(id, value)?,
            TargetRef::Property { obj, key } => self.emit_put(obj, key, value),
            TargetRef::Super { base, key } => self.emit_put_super(base, key, value)?,
            TargetRef::Private { obj, key, position } => {
                self.position = position;
                self.emit_put(obj, key, value);
            }
            TargetRef::Call => {
                self.emit(Insn::ThrowInvalidAssign {
                    context: INVALID_TARGET_ASSIGN,
                });
            }
        }
        Ok(())
    }
}
