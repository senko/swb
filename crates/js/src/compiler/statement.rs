//! Statements: blocks, declarations, control flow, loops, `switch`,
//! `try` with `finally`, and the completion value of scripts.

use swb_js_syntax::{
    AssignTarget, ExprId, ForHead, NameId, Resolution, ScopeId, StmtId, StmtKind, Storage,
    VariableKind,
};

use super::CResult;
use super::function::{
    Breakable, BreakableKind, Finally, FinallyBody, FunctionCompiler, STMT_WEIGHT,
};
use super::pattern::{BindMode, PatternSource};
use crate::bytecode::{
    COMPLETION_JUMP, COMPLETION_NORMAL, COMPLETION_RETURN, COMPLETION_THROW, CodeKind, Handler,
    Insn, Reg,
};

impl FunctionCompiler<'_> {
    // --- Statements ---

    pub(super) fn stmt(&mut self, id: StmtId) -> CResult<()> {
        self.enter(STMT_WEIGHT)?;
        let statement = *self.ast.stmt(id);
        let outer = std::mem::replace(&mut self.position, statement.span.start);
        let mark = self.mark();
        if resets_completion(statement.kind) {
            self.reset_completion();
        }
        self.stmt_kind(statement.kind)?;
        self.release(mark);
        self.position = outer;
        self.leave(STMT_WEIGHT);
        Ok(())
    }

    /// The code of one statement.
    fn stmt_kind(&mut self, kind: StmtKind) -> CResult<()> {
        match kind {
            StmtKind::Empty => {}
            StmtKind::Function(function) => self.block_function_var(function)?,
            StmtKind::ForIn {
                left,
                right,
                body,
                scope,
            } => self.for_in(left, right, body, scope)?,
            StmtKind::ForOf {
                left,
                right,
                body,
                scope,
                is_await,
            } => self.for_of(left, right, body, scope, is_await)?,
            StmtKind::Class(class) => self.class_declaration(class)?,
            StmtKind::With {
                object,
                body,
                scope,
            } => self.with_statement(object, body, scope)?,
            kind @ (StmtKind::Import
            | StmtKind::ExportDeclaration(_)
            | StmtKind::ExportDefault { .. }
            | StmtKind::ExportList) => {
                let construct = super::support::unsupported_stmt(kind).unwrap_or("statement");
                return Err(self.unsupported(construct));
            }
            StmtKind::Debugger => {
                self.emit(Insn::Nop);
            }
            StmtKind::Expr(e) => match self.completion {
                Some(completion) => {
                    self.expr(e, Some(completion))?;
                }
                None => self.effect(e)?,
            },
            StmtKind::Block { body, scope } => {
                self.enter_scope(scope)?;
                for &s in self.ast.stmts(body) {
                    self.stmt(s)?;
                }
            }
            StmtKind::Variables { kind, declarators } => {
                for declarator in self.ast.declarators(declarators) {
                    // The temporaries of one declarator are free after it.
                    let mark = self.mark();
                    self.declarator(kind, declarator.target, declarator.init)?;
                    self.release(mark);
                }
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => self.if_statement(test, consequent, alternate)?,
            StmtKind::While { test, body } => self.while_statement(test, body)?,
            StmtKind::DoWhile { body, test } => self.do_while(body, test)?,
            StmtKind::For {
                init,
                test,
                update,
                body,
                scope,
            } => self.for_statement(init, test, update, body, scope)?,
            StmtKind::Labeled { label, body } => self.labeled(label, body)?,
            StmtKind::Break { label } => {
                let target = self.find_breakable(label, false)?;
                self.jump_to(target, false);
            }
            StmtKind::Continue { label } => {
                let target = self.find_breakable(label, true)?;
                self.jump_to(target, true);
            }
            StmtKind::Return(argument) => self.return_statement(argument)?,
            StmtKind::Throw(e) => {
                let value = self.expr(e, None)?;
                self.emit(Insn::Throw { src: value });
            }
            StmtKind::Switch {
                discriminant,
                cases,
                scope,
            } => self.switch_statement(discriminant, cases, scope)?,
            StmtKind::Try {
                block,
                handler,
                finalizer,
            } => self.try_statement(block, handler, finalizer)?,
        }
        Ok(())
    }

    /// `return` (§14.10.1): an async generator awaits the value first.
    fn return_statement(&mut self, argument: Option<ExprId>) -> CResult<()> {
        let value = match argument {
            Some(e) if self.kind == CodeKind::AsyncGenerator => {
                let t = self.temps(2)?;
                self.expr(e, Some(t))?;
                self.emit_await(t);
                t
            }
            Some(e) => self.expr(e, None)?,
            None => self.undefined_temp()?,
        };
        self.emit_return(value);
        Ok(())
    }

    /// Sets the completion value of script and eval code to `undefined`
    /// (§8.4 `UpdateEmpty`).
    pub(super) fn reset_completion(&mut self) {
        if let Some(completion) = self.completion {
            self.emit(Insn::Undefined { dst: completion });
        }
    }

    /// A class declaration (§15.7.15 `BindingClassDeclarationEvaluation`):
    /// the class, then its binding in the enclosing scope.
    fn class_declaration(&mut self, id: swb_js_syntax::ClassId) -> CResult<()> {
        let declaration = self
            .ast
            .class(id)
            .declaration
            .ok_or(Self::internal("a class declaration without a binding"))?;
        let binding = self
            .reference(declaration)
            .binding
            .ok_or(Self::internal("a class declaration without a binding"))?;
        let t = self.class_definition(id, None, None)?;
        self.init_binding(binding, t)
    }

    /// `if` (§14.6).
    fn if_statement(
        &mut self,
        test: ExprId,
        consequent: StmtId,
        alternate: Option<StmtId>,
    ) -> CResult<()> {
        let exits = self.branch_false(test)?;
        self.stmt(consequent)?;
        if let Some(alternate) = alternate {
            let skip = self.emit(Insn::Jump { offset: 0 });
            for exit in exits {
                self.patch_here(exit);
            }
            self.stmt(alternate)?;
            self.patch_here(skip);
        } else {
            for exit in exits {
                self.patch_here(exit);
            }
        }
        Ok(())
    }

    /// `while` (§14.7.3).
    fn while_statement(&mut self, test: ExprId, body: StmtId) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let top = self.here();
        let exits = self.branch_false(test)?;
        self.loop_body(labels, body, Some(top), exits, None)
    }

    /// `do ... while` (§14.7.2).
    fn do_while(&mut self, body: StmtId, test: ExprId) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let top = self.here();
        self.push_breakable(labels, BreakableKind::Loop);
        self.stmt(body)?;
        let breakable = self.breakables.pop().ok_or(Self::internal("loop stack"))?;
        for at in breakable.continues {
            self.patch_here(at);
        }
        let mark = self.mark();
        let v = self.expr(test, None)?;
        let at = self.here();
        self.emit(Insn::JumpIfTrue {
            cond: v,
            offset: (top as i64 - at as i64) as i32,
        });
        self.release(mark);
        for at in breakable.breaks {
            self.patch_here(at);
        }
        Ok(())
    }

    /// A labelled statement (§14.13): a loop takes the label for
    /// `continue`; any other statement is a target of `break` only.
    fn labeled(&mut self, label: NameId, body: StmtId) -> CResult<()> {
        self.pending_labels.push(label);
        let body_kind = self.ast.stmt(body).kind;
        if matches!(
            body_kind,
            StmtKind::While { .. }
                | StmtKind::DoWhile { .. }
                | StmtKind::For { .. }
                | StmtKind::ForIn { .. }
                | StmtKind::ForOf { .. }
                | StmtKind::Labeled { .. }
        ) {
            return self.stmt(body);
        }
        let labels = std::mem::take(&mut self.pending_labels);
        self.push_breakable(labels, BreakableKind::Label);
        self.stmt(body)?;
        let breakable = self.breakables.pop().ok_or(Self::internal("label stack"))?;
        for at in breakable.breaks {
            self.patch_here(at);
        }
        Ok(())
    }

    /// One declarator of `var`, `let` or `const`.
    fn declarator(
        &mut self,
        kind: VariableKind,
        target: swb_js_syntax::PatternId,
        init: Option<ExprId>,
    ) -> CResult<()> {
        let swb_js_syntax::PatternKind::Identifier(ident) = self.ast.pattern(target).kind else {
            let init = init.ok_or(Self::internal("a pattern declaration without a value"))?;
            let mark = self.mark();
            let value = self.temp()?;
            self.expr(init, Some(value))?;
            let mode = if kind == VariableKind::Var {
                BindMode::Var
            } else {
                BindMode::Init
            };
            self.bind_pattern(target, value, mode, PatternSource::Expr(init))?;
            self.release(mark);
            return Ok(());
        };
        let reference = self.reference(ident.reference);
        let name = self.name_text(ident.name);
        match (kind, init) {
            (VariableKind::Var, None) => Ok(()),
            (VariableKind::Var, Some(value)) => {
                self.assign_identifier_value(ident.reference, value, None, name)?;
                Ok(())
            }
            (VariableKind::Let | VariableKind::Const, init) => {
                // A register that is safe to fill directly.
                if let Resolution::Register(r) = reference.resolution
                    && init.is_none_or(|value| self.writes_target_last(value))
                {
                    match init {
                        Some(value) => {
                            self.expr_named(value, Some(r), name)?;
                        }
                        None => {
                            self.emit(Insn::Undefined { dst: r });
                        }
                    }
                    return Ok(());
                }
                let mark = self.mark();
                let t = self.temp()?;
                match init {
                    Some(value) => {
                        self.expr_named(value, Some(t), name)?;
                    }
                    None => {
                        self.emit(Insn::Undefined { dst: t });
                    }
                }
                let binding = reference
                    .binding
                    .ok_or(Self::internal("a declaration without a binding"))?;
                self.init_binding(binding, t)?;
                self.release(mark);
                Ok(())
            }
        }
    }

    /// A `for` statement (§14.7.4).
    fn for_statement(
        &mut self,
        init: Option<StmtId>,
        test: Option<ExprId>,
        update: Option<ExprId>,
        body: StmtId,
        scope: ScopeId,
    ) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        self.enter_scope(scope)?;
        if let Some(init) = init {
            match self.ast.stmt(init).kind {
                // The value of an initializer expression is not the
                // statement's completion value.
                StmtKind::Expr(e) => {
                    self.position = self.ast.stmt(init).span.start;
                    self.effect(e)?;
                }
                _ => self.stmt(init)?,
            }
        }
        let per_iteration = self.scopes.scope(scope).per_iteration;
        if per_iteration {
            self.copy_cells(scope);
        }
        let top = self.here();
        let exits = match test {
            Some(test) => self.branch_false(test)?,
            None => Vec::new(),
        };
        self.loop_body(
            labels,
            body,
            None,
            exits,
            Some((scope, per_iteration, update, top)),
        )
    }

    /// `for (left in right) body` (§14.7.5.6 `ForIn/OfHeadEvaluation`,
    /// §14.7.5.7 `ForIn/OfBodyEvaluation`, Annex B.3.5 for the initializer
    /// of `var`). The keys come from an iterator that the VM makes
    /// (`ForInStart`); `ForInNext` puts each key into a register or ends
    /// the loop. A `let` or `const` binding that is a cell gets a new cell
    /// in each iteration.
    fn for_in(
        &mut self,
        left: ForHead,
        right: ExprId,
        body: StmtId,
        scope: ScopeId,
    ) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let declaration = match left {
            ForHead::Declaration(stmt) => {
                let StmtKind::Variables { kind, declarators } = self.ast.stmt(stmt).kind else {
                    return Err(Self::internal("a for-in declaration without variables"));
                };
                let [declarator] = self.ast.declarators(declarators) else {
                    return Err(Self::internal("a for-in declaration with several bindings"));
                };
                let declarator = *declarator;
                if kind == VariableKind::Var && declarator.init.is_some() {
                    self.declarator(kind, declarator.target, declarator.init)?;
                }
                Some((kind, declarator.target))
            }
            ForHead::Target(_) => None,
        };
        // The value of a loop that does not run its body is `undefined`
        // (§14.7.5.7 step 2; a nullish object breaks with an empty value,
        // which §14.7.1.1 turns into `undefined`).
        if let Some(completion) = self.completion {
            self.emit(Insn::Undefined { dst: completion });
        }
        self.enter_scope(scope)?;
        let iterator = self.temp()?;
        let key = self.temp()?;
        self.expr(right, Some(iterator))?;
        self.position = self.ast.expr(right).span.start;
        self.emit(Insn::ForInStart {
            dst: iterator,
            src: iterator,
        });
        let top = self.here();
        let exit = self.emit(Insn::ForInNext {
            dst: key,
            iter: iterator,
            offset: 0,
        });
        match (declaration, left) {
            (Some((kind, target)), _) => self.for_in_binding(kind, target, key, scope)?,
            (None, ForHead::Target(AssignTarget::Simple(target))) => {
                // Chromium reports a write to a private member at its name.
                let inner = self.unparen(target);
                if let swb_js_syntax::ExprKind::PrivateMember { .. } = self.ast.expr(inner).kind {
                    self.position = self.name_position(inner);
                }
                self.assign_to_target(target, key)?;
            }
            (None, ForHead::Target(AssignTarget::Pattern(pattern))) => {
                self.bind_pattern(pattern, key, BindMode::Assign, PatternSource::Nested)?;
            }
            (None, ForHead::Declaration(_)) => {
                return Err(Self::internal("a for-in declaration without a binding"));
            }
        }
        self.loop_body(labels, body, Some(top), vec![exit], None)?;
        // After a `break`, the register still holds the iterator and its
        // key list; release them now, not when the register is reused.
        self.emit(Insn::Undefined { dst: iterator });
        Ok(())
    }

    /// The binding of a `for`-`in` declaration gets the key in `key`.
    fn for_in_binding(
        &mut self,
        kind: VariableKind,
        target: swb_js_syntax::PatternId,
        key: Reg,
        scope: ScopeId,
    ) -> CResult<()> {
        let swb_js_syntax::PatternKind::Identifier(ident) = self.ast.pattern(target).kind else {
            return self.bind_head_pattern(kind, target, key, scope);
        };
        if kind == VariableKind::Var {
            return self.store_ref(ident.reference, key);
        }
        let binding = self
            .reference(ident.reference)
            .binding
            .ok_or(Self::internal("a declaration without a binding"))?;
        match self.scopes.binding(binding).storage {
            // A new binding per iteration (§14.7.5.7 step 6.g): closures
            // of earlier iterations keep their cells.
            Storage::Cell(r) => {
                self.emit(Insn::Move { dst: r, src: key });
                self.emit(Insn::MakeCell { reg: r });
                Ok(())
            }
            _ => self.init_binding(binding, key),
        }
    }

    /// The body of a loop, its continue target and its exit. For `while`,
    /// `continue_to` is the test; for `for`, `next` holds the update.
    fn loop_body(
        &mut self,
        labels: Vec<NameId>,
        body: StmtId,
        continue_to: Option<usize>,
        exits: Vec<usize>,
        next: Option<(ScopeId, bool, Option<ExprId>, usize)>,
    ) -> CResult<()> {
        self.push_breakable(labels, BreakableKind::Loop);
        self.stmt(body)?;
        let breakable = self.breakables.pop().ok_or(Self::internal("loop stack"))?;
        match (continue_to, next) {
            (Some(top), _) => {
                for at in breakable.continues {
                    self.patch(at, top);
                }
                self.jump_back(top);
            }
            (None, Some((scope, per_iteration, update, top))) => {
                for at in breakable.continues {
                    self.patch_here(at);
                }
                if per_iteration {
                    self.copy_cells(scope);
                }
                if let Some(update) = update {
                    self.effect(update)?;
                }
                self.jump_back(top);
            }
            (None, None) => return Err(Self::internal("a loop without a continue target")),
        }
        for at in exits.into_iter().chain(breakable.breaks) {
            self.patch_here(at);
        }
        Ok(())
    }

    /// Starts a statement that `break` or `continue` can target.
    pub(super) fn push_breakable(&mut self, labels: Vec<NameId>, kind: BreakableKind) {
        self.breakables.push(Breakable {
            labels,
            kind,
            finally_depth: self.finallys.len(),
            breaks: Vec::new(),
            continues: Vec::new(),
        });
    }

    /// The breakable statement that `break label` or `continue label`
    /// targets.
    fn find_breakable(&self, label: Option<NameId>, is_continue: bool) -> CResult<usize> {
        self.breakables
            .iter()
            .rposition(|b| {
                let is_loop = b.kind == BreakableKind::Loop;
                match label {
                    Some(label) => b.labels.contains(&label) && (is_loop || !is_continue),
                    None if is_continue => is_loop,
                    None => b.kind != BreakableKind::Label,
                }
            })
            .ok_or(Self::internal("a break or continue without a target"))
    }

    /// A `break` (or `continue`) to the breakable statement `target`: a
    /// jump, or through the innermost `finally` block that it leaves.
    fn jump_to(&mut self, target: usize, is_continue: bool) {
        let depth = self.breakables.get(target).map_or(0, |b| b.finally_depth);
        if self.finallys.len() > depth
            && let Some(finally) = self.finallys.last_mut()
        {
            let known = finally
                .targets
                .iter()
                .position(|&t| t == (target, is_continue));
            let k = known.unwrap_or_else(|| {
                finally.targets.push((target, is_continue));
                finally.targets.len() - 1
            });
            let kind = finally.kind;
            self.emit(Insn::Int {
                dst: kind,
                value: COMPLETION_JUMP + k as i32,
            });
            let at = self.emit(Insn::Jump { offset: 0 });
            if let Some(finally) = self.finallys.last_mut() {
                finally.entries.push(at);
            }
            return;
        }
        let at = self.emit(Insn::Jump { offset: 0 });
        if let Some(b) = self.breakables.get_mut(target) {
            if is_continue {
                b.continues.push(at);
            } else {
                b.breaks.push(at);
            }
        }
    }

    /// Returns the value of `value`: a `Return`, or through the innermost
    /// `finally` block.
    pub(super) fn emit_return(&mut self, value: Reg) {
        let Some(finally) = self.finallys.last() else {
            match self.derived_result {
                Some(registers) => self.derived_return(value, registers),
                None => {
                    self.emit(Insn::Return { src: value });
                }
            }
            return;
        };
        let (kind, register) = (finally.kind, finally.value);
        if value != register {
            self.emit(Insn::Move {
                dst: register,
                src: value,
            });
        }
        self.emit(Insn::Int {
            dst: kind,
            value: COMPLETION_RETURN,
        });
        let at = self.emit(Insn::Jump { offset: 0 });
        if let Some(finally) = self.finallys.last_mut() {
            finally.entries.push(at);
        }
    }

    /// `switch` (§14.12.4 `CaseBlockEvaluation`): the tests in source
    /// order, then the jump to `default` (the order of the specification:
    /// the clauses before `default`, then the ones after it, then
    /// `default`), then the bodies, which fall through.
    fn switch_statement(
        &mut self,
        discriminant: ExprId,
        cases: swb_js_syntax::List<swb_js_syntax::SwitchCase>,
        scope: ScopeId,
    ) -> CResult<()> {
        let labels = std::mem::take(&mut self.pending_labels);
        let value = self.temp()?;
        self.expr(discriminant, Some(value))?;
        self.enter_scope(scope)?;
        let cases = self.ast.cases(cases).to_vec();
        let mut tests = Vec::new();
        for (i, case) in cases.iter().enumerate() {
            if let Some(test) = case.test {
                self.position = case.span.start;
                let mark = self.mark();
                let v = self.expr(test, None)?;
                let t = self.temp()?;
                self.emit(Insn::StrictEq {
                    dst: t,
                    a: value,
                    b: v,
                });
                tests.push((i, self.emit(Insn::JumpIfTrue { cond: t, offset: 0 })));
                self.release(mark);
            }
        }
        let otherwise = self.emit(Insn::Jump { offset: 0 });
        self.push_breakable(labels, BreakableKind::Switch);
        let mut starts = Vec::with_capacity(cases.len());
        for case in &cases {
            starts.push(self.here());
            for &statement in self.ast.stmts(case.body) {
                self.stmt(statement)?;
            }
        }
        let breakable = self
            .breakables
            .pop()
            .ok_or(Self::internal("switch stack"))?;
        for (i, at) in tests {
            let start = starts.get(i).copied().unwrap_or_else(|| self.here());
            self.patch(at, start);
        }
        let default = cases
            .iter()
            .position(|c| c.test.is_none())
            .and_then(|i| starts.get(i).copied())
            .unwrap_or_else(|| self.here());
        self.patch(otherwise, default);
        for at in breakable.breaks {
            self.patch_here(at);
        }
        Ok(())
    }

    /// `try` (§14.15.3); see the module documentation for the layout.
    fn try_statement(
        &mut self,
        block: StmtId,
        handler: Option<swb_js_syntax::CatchClause>,
        finalizer: Option<StmtId>,
    ) -> CResult<()> {
        if finalizer.is_some() {
            let kind = self.temps(2)?;
            self.finallys.push(Finally {
                kind,
                value: kind + 1,
                entries: Vec::new(),
                targets: Vec::new(),
            });
        }
        let start = self.here();
        self.stmt(block)?;
        if let Some(clause) = handler {
            let end = self.here();
            let skip = self.emit(Insn::Jump { offset: 0 });
            let mark = self.mark();
            let exception = self.temp()?;
            let target = self.here();
            if end > start {
                self.handlers.push(Handler {
                    start: start as u32,
                    end: end as u32,
                    target: target as u32,
                    register: exception,
                });
            }
            self.catch_clause(clause, exception)?;
            self.release(mark);
            self.patch_here(skip);
        }
        if let Some(finalizer) = finalizer {
            self.finally_block(start, FinallyBody::Block(finalizer))?;
        }
        Ok(())
    }

    /// The `catch` clause: the scope of the parameter and the block, the
    /// parameter's binding, the statements.
    fn catch_clause(&mut self, clause: swb_js_syntax::CatchClause, exception: Reg) -> CResult<()> {
        let StmtKind::Block { body, scope } = self.ast.stmt(clause.body).kind else {
            return Err(Self::internal("a catch clause without a block"));
        };
        self.enter(STMT_WEIGHT)?;
        self.position = self.ast.stmt(clause.body).span.start;
        // The value of the statement is the catch block's (§14.15.3).
        self.reset_completion();
        self.enter_scope(scope)?;
        if let Some(param) = clause.param {
            self.bind_pattern(param, exception, BindMode::Init, PatternSource::Catch)?;
        }
        for &statement in self.ast.stmts(body) {
            self.stmt(statement)?;
        }
        self.leave(STMT_WEIGHT);
        Ok(())
    }

    /// Starts the implicit `finally` block that closes the iterator record
    /// at `rec` (see [`FunctionCompiler::finally_block`]); the protected
    /// code starts at the next instruction.
    pub(super) fn push_close(&mut self) -> CResult<usize> {
        let kind = self.temps(2)?;
        self.finallys.push(Finally {
            kind,
            value: kind + 1,
            entries: Vec::new(),
            targets: Vec::new(),
        });
        Ok(self.here())
    }

    /// The `finally` block of the innermost entry of `finallys`, which
    /// protects the code from `start`: the normal completion, the block,
    /// its routes and its handler stub. The block starts at the position
    /// of the next instruction (which sets the normal completion).
    pub(super) fn finally_block(&mut self, start: usize, body: FinallyBody) -> CResult<()> {
        let finally = self.finallys.pop().ok_or(Self::internal("finally stack"))?;
        let (kind, value) = (finally.kind, finally.value);
        let end = self.here();
        self.emit(Insn::Int {
            dst: kind,
            value: COMPLETION_NORMAL,
        });
        let entry = self.here();
        for at in finally.entries {
            self.patch(at, entry);
        }
        match body {
            // A `finally` block that completes normally keeps the value of
            // the `try` statement (§14.15.3): the completion value of
            // script and eval code is saved around it.
            FinallyBody::Block(finalizer) => match self.completion {
                Some(completion) => {
                    let saved = self.temp()?;
                    self.emit(Insn::Move {
                        dst: saved,
                        src: completion,
                    });
                    self.stmt(finalizer)?;
                    self.emit(Insn::Move {
                        dst: completion,
                        src: saved,
                    });
                }
                None => self.stmt(finalizer)?,
            },
            FinallyBody::Close(rec) => {
                self.emit(Insn::IteratorClose { rec, kind });
            }
            FinallyBody::AsyncClose(rec) => {
                let t = self.temps(2)?;
                let close = self.emit(Insn::AsyncClose {
                    rec,
                    kind,
                    dst: t,
                    offset: 0,
                });
                self.emit(Insn::Await { reg: t });
                self.emit(Insn::AsyncCloseEnd { reg: t, kind });
                self.patch_here(close);
            }
        }
        let end_finally = self.emit(Insn::EndFinally {
            kind,
            value,
            offset: 0,
        });
        // The table: the return, then each jump target.
        let table: Vec<usize> = (0..=finally.targets.len())
            .map(|_| self.emit(Insn::Jump { offset: 0 }))
            .collect();
        let mut routes = table.into_iter();
        if let Some(at) = routes.next() {
            self.patch_here(at);
            self.emit_return(value);
        }
        for (&(target, is_continue), at) in finally.targets.iter().zip(routes) {
            self.patch_here(at);
            self.jump_to(target, is_continue);
        }
        // The handler of the `try` and `catch` code.
        let stub = self.here();
        self.emit(Insn::Int {
            dst: kind,
            value: COMPLETION_THROW,
        });
        self.jump_back(entry);
        if end > start {
            self.handlers.push(Handler {
                start: start as u32,
                end: end as u32,
                target: stub as u32,
                register: value,
            });
        }
        self.patch_here(end_finally);
        Ok(())
    }
}

/// Whether a statement's value is `UpdateEmpty(C, undefined)` (§14.6 to
/// §14.15): `undefined` unless a statement inside it produces a value.
/// (`for`-`in` and `for`-`of` set it themselves.)
fn resets_completion(kind: StmtKind) -> bool {
    matches!(
        kind,
        StmtKind::If { .. }
            | StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Try { .. }
            | StmtKind::With { .. }
    )
}
