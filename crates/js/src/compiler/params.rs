//! Parameter lists that are not simple (patterns, default values, a rest
//! parameter): `IteratorBindingInitialization` of the arguments
//! (§10.2.11 steps 24 to 26, §8.6.3) and the var scope of a body that
//! parameter expressions cannot see (step 28).
//!
//! The arguments arrive in registers `0..n`, one per element of the list.
//! A parameter that is a whole element (`a`, `a = 1`, `...a`) and not a
//! cell keeps its argument's register (the scope analysis assigns it), so
//! an element of that kind needs no register of its own: `a = 1` tests
//! and fills its register in place. Its temporal dead zone needs no
//! marker: every reference with a TDZ check runs before the element and
//! throws without a test ([`FunctionCompiler::check_register_tdz`]). The
//! other bindings of the list are ordinary bindings of the function's
//! scope; they get the uninitialized marker (or a new cell) before the
//! first element runs and their values when their element runs.
//!
//! Order (§10.2.11): the arguments object, the bindings of the list in
//! source order, then the var scope of the body (when the list has
//! parameter expressions, a `var` with the name of a parameter starts
//! with its value), then the function declarations.

use std::collections::HashMap;

use swb_js_syntax::{BindingId, BindingKind, PatternKind, Reference, ScopeId, Storage};

use super::CResult;
use super::function::FunctionCompiler;
use super::pattern::{BindMode, PatternSource};
use crate::bytecode::{Insn, Reg};

impl FunctionCompiler<'_> {
    /// Whether the parameter list is simple (only identifiers): each
    /// parameter is the register of its argument.
    pub(super) fn has_simple_parameters(&self) -> bool {
        self.fscope.params.len() == self.ast.patterns(self.function.params).len()
    }

    /// Whether the last element is a rest parameter.
    pub(super) fn has_rest_parameter(&self) -> bool {
        self.ast
            .patterns(self.function.params)
            .last()
            .is_some_and(|&p| matches!(self.ast.pattern(p).kind, PatternKind::Rest(_)))
    }

    /// The `length` of the function (§15.1.5 `ExpectedArgumentCount`):
    /// the elements before the first default value or rest parameter.
    pub(super) fn expected_argument_count(&self) -> u32 {
        self.ast
            .patterns(self.function.params)
            .iter()
            .take_while(|&&p| {
                !matches!(
                    self.ast.pattern(p).kind,
                    PatternKind::Default { .. } | PatternKind::Rest(_)
                )
            })
            .count() as u32
    }

    /// Whether a reference names a parameter that shares its argument's
    /// register (see the module documentation).
    pub(super) fn is_shared_parameter(&self, reference: &Reference) -> bool {
        reference
            .binding
            .is_some_and(|binding| self.shares_argument_register(binding))
    }

    fn shares_argument_register(&self, binding: BindingId) -> bool {
        let b = self.scopes.binding(binding);
        b.kind == BindingKind::Parameter
            && !self.has_simple_parameters()
            && matches!(b.storage, Storage::Register(r) if u32::from(r) < self.fscope.argument_registers)
    }

    /// The TDZ check of a load or store of the register `r` of a
    /// reference with `tdz_check`: `CheckTdz`, or for a parameter that
    /// shares its argument's register, a check that always fails.
    pub(super) fn check_register_tdz(&mut self, reference: &Reference, r: Reg) -> CResult<()> {
        let name = self.name_const(reference.name);
        if self.is_shared_parameter(reference) {
            let mark = self.mark();
            let t = self.temp()?;
            self.emit(Insn::Empty { dst: t });
            self.emit(Insn::CheckTdz { src: t, name });
            self.release(mark);
        } else {
            self.emit(Insn::CheckTdz { src: r, name });
        }
        Ok(())
    }

    /// The scope entry and the parameter bindings of a function whose
    /// parameter list is not simple.
    pub(super) fn parameter_list(&mut self, scope: ScopeId) -> CResult<()> {
        self.enter_scope_bindings(scope)?;
        self.parameter_markers(scope);
        let elements = self.ast.patterns(self.function.params).to_vec();
        for (index, element) in elements.into_iter().enumerate() {
            let argument = Reg::try_from(index)
                .map_err(|_| Self::internal("a parameter outside the registers"))?;
            self.parameter_element(element, argument, index as u32)?;
        }
        let body = self.function.body_scope;
        if body == scope {
            self.declare_functions(scope)
        } else {
            self.enter_scope_bindings(body)?;
            self.copy_parameter_vars(scope, body)?;
            self.declare_functions(body)
        }
    }

    /// The uninitialized markers and cells of the bindings of the list
    /// that do not share an argument's register.
    fn parameter_markers(&mut self, scope: ScopeId) {
        for &binding in self.scopes.bindings_of(scope) {
            let b = *self.scopes.binding(binding);
            if b.kind != BindingKind::Parameter || self.shares_argument_register(binding) {
                continue;
            }
            match b.storage {
                Storage::Cell(r) => {
                    self.emit(Insn::NewCell {
                        dst: r,
                        empty: b.needs_tdz,
                    });
                }
                Storage::Register(r) if b.needs_tdz => {
                    self.emit(Insn::Empty { dst: r });
                }
                _ => {}
            }
        }
    }

    /// One element of the list, whose argument is in `argument`.
    fn parameter_element(
        &mut self,
        element: swb_js_syntax::PatternId,
        argument: Reg,
        index: u32,
    ) -> CResult<()> {
        let node = *self.ast.pattern(element);
        self.position = node.span.start;
        let mark = self.mark();
        match node.kind {
            PatternKind::Identifier(ident) => {
                let binding = self
                    .reference(ident.reference)
                    .binding
                    .ok_or(Self::internal("a parameter without a binding"))?;
                if !self.shares_argument_register(binding) {
                    self.init_binding(binding, argument)?;
                }
            }
            PatternKind::Default { target, value } => {
                let shared = self.shared_identifier(target);
                let register = if shared.is_some() {
                    argument
                } else {
                    let t = self.temp()?;
                    self.emit(Insn::Move {
                        dst: t,
                        src: argument,
                    });
                    t
                };
                let skip = self.emit(Insn::JumpIfNotUndefined {
                    src: register,
                    offset: 0,
                });
                match self.pattern_name(target) {
                    Some(name) => self.expr_named(value, Some(register), name)?,
                    None => self.expr(value, Some(register))?,
                };
                self.patch_here(skip);
                if shared.is_none() {
                    self.bind_pattern(target, register, BindMode::Init, PatternSource::Parameter)?;
                }
            }
            PatternKind::Rest(target) => {
                if self.shared_identifier(target).is_some() {
                    self.emit(Insn::RestArguments {
                        dst: argument,
                        start: index,
                    });
                } else {
                    let t = self.temp()?;
                    self.emit(Insn::RestArguments {
                        dst: t,
                        start: index,
                    });
                    self.bind_pattern(target, t, BindMode::Init, PatternSource::Parameter)?;
                }
            }
            PatternKind::Array(_) | PatternKind::Object { .. } => {
                self.bind_pattern(element, argument, BindMode::Init, PatternSource::Parameter)?;
            }
            PatternKind::Expr(_) | PatternKind::Hole => {
                return Err(Self::internal("a parameter that is not a binding"));
            }
        }
        self.release(mark);
        Ok(())
    }

    /// The binding of a pattern that is an identifier sharing its
    /// argument's register.
    fn shared_identifier(&self, pattern: swb_js_syntax::PatternId) -> Option<BindingId> {
        let PatternKind::Identifier(ident) = self.ast.pattern(pattern).kind else {
            return None;
        };
        self.reference(ident.reference)
            .binding
            .filter(|&binding| self.shares_argument_register(binding))
    }

    /// §10.2.11 step 28.f: a `var` of the body with the name of a binding
    /// of the parameter scope (a parameter or `arguments`) starts with
    /// its value.
    fn copy_parameter_vars(&mut self, scope: ScopeId, body: ScopeId) -> CResult<()> {
        let outer: HashMap<_, _> = self
            .scopes
            .bindings_of(scope)
            .iter()
            .map(|&binding| (self.scopes.binding(binding).name, binding))
            .collect();
        for &binding in self.scopes.bindings_of(body) {
            let b = *self.scopes.binding(binding);
            if b.kind != BindingKind::Var {
                continue;
            }
            let Some(&source) = outer.get(&b.name) else {
                continue;
            };
            let mark = self.mark();
            let t = self.temp()?;
            match self.scopes.binding(source).storage {
                Storage::Register(r) => {
                    self.emit(Insn::Move { dst: t, src: r });
                }
                Storage::Cell(r) => {
                    self.emit(Insn::LoadCell { dst: t, cell: r });
                }
                _ => return Err(Self::internal("a parameter without a register")),
            }
            self.init_binding(binding, t)?;
            self.release(mark);
        }
        Ok(())
    }
}
