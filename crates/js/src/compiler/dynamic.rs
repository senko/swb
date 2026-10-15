//! Dynamic references (ADR 0026 "Dynamic scope"): the names in the reach
//! of a `with` statement or of the vars that sloppy direct eval code
//! declares. The scope analysis gives such a reference a count of records
//! to check before its static binding ([`swb_js_syntax::DynamicLookup`]);
//! the code first finds the record that has the name (`FindBinding`,
//! `undefined` if none) and then uses it, or falls back to the static
//! access:
//!
//! ```text
//!       <the innermost record>     -> rec
//!       FindBinding rec rec count name
//!       JumpIfNullish rec -> S
//!       GetBinding t rec name      (SetBinding, DeleteBinding, ...)
//!       Jump E
//!   S:  <the static access>
//!   E:
//! ```
//!
//! An assignment finds the record before it evaluates its right side
//! (§13.15.2: the reference is evaluated first). Records are made at the
//! entry of their scope: the record of a `with` body by the statement, the
//! record of eval vars by the scope entry of the function's var scope (or
//! of its parameter scope, for an eval in the parameters).

use swb_js_syntax::{BindingKind, ExprId, RefId, ScopeId, StmtId, Storage};

use super::CResult;
use super::function::FunctionCompiler;
use crate::bytecode::{Insn, Reg};
use crate::vm::env::ALL_RECORDS;

/// Where the innermost record of a dynamic reference comes from.
#[derive(Clone, Copy, Debug)]
pub(super) enum EnvSource {
    /// A reference (name `%env`) that resolves to the binding of the
    /// record.
    Ref(RefId),
    /// A capture of the function (eval code: the record of the caller).
    Capture(u32),
}

/// The dynamic part of a reference.
#[derive(Clone, Copy, Debug)]
pub(super) struct Dynamic {
    pub(super) env: EnvSource,
    /// The records to check ([`ALL_RECORDS`]: all of them).
    pub(super) count: u16,
}

/// The count operand of `FindBinding` for a count of the analysis. A count
/// that does not fit walks all records (more than 65,534 nested
/// environments; the parser's nesting limit is far below).
pub(super) fn count_operand(count: u32) -> u16 {
    u16::try_from(count)
        .ok()
        .filter(|&c| c < ALL_RECORDS)
        .unwrap_or(ALL_RECORDS)
}

impl FunctionCompiler<'_> {
    /// The dynamic part of a reference, if it has one.
    pub(super) fn dynamic_of(&self, id: RefId) -> Option<Dynamic> {
        if let Some(access) = self.caller_access(id) {
            return access.dynamic;
        }
        let lookup = self.scopes.dynamic_lookup(id)?;
        Some(Dynamic {
            env: EnvSource::Ref(lookup.env),
            count: count_operand(lookup.count),
        })
    }

    /// Emits the search for the record of a dynamic reference into `rec`.
    pub(super) fn find_record(&mut self, id: RefId, dynamic: Dynamic, rec: Reg) -> CResult<()> {
        match dynamic.env {
            EnvSource::Ref(env) => {
                self.load_static(env, Some(rec))?;
            }
            EnvSource::Capture(index) => {
                self.emit(Insn::LoadCapture { dst: rec, index });
            }
        }
        let name = self.name_const(self.reference(id).name);
        self.emit(Insn::FindBinding {
            dst: rec,
            env: rec,
            count: dynamic.count,
            name,
        });
        Ok(())
    }

    /// A new temporary with the record of a dynamic reference (or
    /// `undefined`); `None` for a static reference.
    pub(super) fn prepare_dynamic(&mut self, id: RefId) -> CResult<Option<Reg>> {
        let Some(dynamic) = self.dynamic_of(id) else {
            return Ok(None);
        };
        let rec = self.temp()?;
        self.find_record(id, dynamic, rec)?;
        Ok(Some(rec))
    }

    /// The value of a reference into `t`, from the record in `rec` if it
    /// has one, else from the static binding.
    pub(super) fn load_found(&mut self, id: RefId, rec: Option<Reg>, t: Reg) -> CResult<()> {
        let Some(rec) = rec else {
            self.load_static(id, Some(t))?;
            return Ok(());
        };
        let name = self.name_const(self.reference(id).name);
        let skip = self.emit(Insn::JumpIfNullish {
            src: rec,
            offset: 0,
        });
        self.emit(Insn::GetBinding {
            dst: t,
            record: rec,
            name,
        });
        let done = self.emit(Insn::Jump { offset: 0 });
        self.patch_here(skip);
        self.load_static(id, Some(t))?;
        self.patch_here(done);
        Ok(())
    }

    /// `PutValue` of `src` on a reference, through the record in `rec` if
    /// it has one, else to the static binding.
    pub(super) fn store_found(&mut self, id: RefId, src: Reg, rec: Option<Reg>) -> CResult<()> {
        let Some(rec) = rec else {
            return self.store_static(id, src);
        };
        let name = self.name_const(self.reference(id).name);
        let skip = self.emit(Insn::JumpIfNullish {
            src: rec,
            offset: 0,
        });
        self.emit(Insn::SetBinding {
            record: rec,
            src,
            name,
        });
        let done = self.emit(Insn::Jump { offset: 0 });
        self.patch_here(skip);
        self.store_static(id, src)?;
        self.patch_here(done);
        Ok(())
    }

    /// The callee of a call of a dynamic name into `base` and its `this`
    /// into `base + 1` (`WithBaseObject`, §9.1.1.2.10; `undefined` without
    /// a `with` object). Returns false for a static name (nothing emitted).
    pub(super) fn dynamic_callee(&mut self, id: RefId, base: Reg) -> CResult<bool> {
        let Some(dynamic) = self.dynamic_of(id) else {
            return Ok(false);
        };
        let this = base + 1;
        self.find_record(id, dynamic, this)?;
        let name = self.name_const(self.reference(id).name);
        let skip = self.emit(Insn::JumpIfNullish {
            src: this,
            offset: 0,
        });
        self.emit(Insn::GetBinding {
            dst: base,
            record: this,
            name,
        });
        self.emit(Insn::WithBase {
            dst: this,
            record: this,
        });
        let done = self.emit(Insn::Jump { offset: 0 });
        self.patch_here(skip);
        self.load_static(id, Some(base))?;
        self.emit(Insn::Undefined { dst: this });
        self.patch_here(done);
        Ok(true)
    }

    /// `with (object) body` (§14.11.2): `ToObject` of the object, its
    /// record (linked to the enclosing one) in the body's `%env` binding,
    /// the body.
    pub(super) fn with_statement(
        &mut self,
        object: ExprId,
        body: StmtId,
        scope: ScopeId,
    ) -> CResult<()> {
        let position = self.position;
        let Some(env) = self.scopes.dynamic_env(scope).copied() else {
            return Err(Self::internal("a with body without a record"));
        };
        let mark = self.mark();
        let value = self.temp()?;
        self.expr(object, Some(value))?;
        let outer = self.temp()?;
        self.load_outer_record(env.outer, outer)?;
        // Chromium reports a failed `ToObject` at the statement.
        self.position = position;
        self.emit(Insn::NewWithEnv {
            dst: value,
            obj: value,
            outer,
        });
        self.enter_scope(scope)?;
        self.init_record(env.binding, value)?;
        self.release(mark);
        self.stmt(body)
    }

    /// The record of eval vars of a scope with a dynamic environment of
    /// that kind, at its entry, into the environment's binding.
    pub(super) fn enter_var_record(&mut self, scope: ScopeId) -> CResult<()> {
        let Some(env) = self.scopes.dynamic_env(scope).copied() else {
            return Ok(());
        };
        if env.kind != swb_js_syntax::DynamicKind::EvalVars {
            return Ok(());
        }
        let mark = self.mark();
        let outer = self.temp()?;
        self.load_outer_record(env.outer, outer)?;
        self.emit(Insn::NewVarEnv { dst: outer, outer });
        self.init_record(env.binding, outer)?;
        self.release(mark);
        Ok(())
    }

    /// The enclosing record (or `undefined`) into `dst`.
    fn load_outer_record(&mut self, outer: Option<RefId>, dst: Reg) -> CResult<()> {
        match outer {
            Some(reference) => {
                self.load_static(reference, Some(dst))?;
            }
            None => {
                self.emit(Insn::Undefined { dst });
            }
        }
        Ok(())
    }

    /// Stores a new record into the `%env` binding of its scope (a cell
    /// that the scope entry created, or a register).
    fn init_record(&mut self, binding: swb_js_syntax::BindingId, src: Reg) -> CResult<()> {
        let b = *self.scopes.binding(binding);
        if b.kind != BindingKind::DynamicEnv {
            return Err(Self::internal("a record binding of another kind"));
        }
        match b.storage {
            Storage::Register(r) => {
                self.emit(Insn::Move { dst: r, src });
            }
            Storage::Cell(r) => {
                self.emit(Insn::StoreCell { cell: r, src });
            }
            _ => return Err(Self::internal("a record binding without a register")),
        }
        Ok(())
    }
}
