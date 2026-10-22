//! Calls, `new`, tagged templates and optional chains (§13.3).

use swb_js_syntax::{ExprId, ExprKind, List};
use swb_js_text::String16;

use super::CResult;
use super::expr::RENDER_DEPTH;
use super::function::FunctionCompiler;
use crate::bytecode::{ITER_SPREAD, Insn, Reg};
use crate::vm::number::number_to_string;

impl FunctionCompiler<'_> {
    // --- Calls ---

    /// A call, `new` or a tagged template (§13.3.6, §13.3.5, §13.3.11):
    /// the callee, `this` and the arguments in consecutive registers; the
    /// result in the callee's register. A member callee gives `this`.
    pub(super) fn call(
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
        let argc = self.argument_count(count)?;
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
    pub(super) fn optional_chain(&mut self, inner: ExprId, dst: Option<Reg>) -> CResult<Reg> {
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
    pub(super) fn chain_jump(&mut self, register: Reg) -> CResult<()> {
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
    pub(super) fn delete_property_of(
        &mut self,
        target: ExprId,
        t: Reg,
        chain: bool,
    ) -> CResult<()> {
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
pub(super) enum CallArgs {
    List(List<ExprId>),
    Template(swb_js_syntax::TemplateId),
}

/// What a call instruction does.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CallKind {
    /// `[[Call]]`; `optional` for `f?.()`.
    Call { optional: bool },
    /// `new`.
    New,
}
