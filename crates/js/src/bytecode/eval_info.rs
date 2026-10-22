//! The scope description of a function with a direct `eval`
//! ([`EvalInfo`]): where the bindings that eval code can use live, seen
//! from the frame of the code that calls `eval` (`compiler/eval.rs`).

use super::Reg;
use crate::heap::{Gc, Tracer};
use crate::string::JsString;

/// Where a binding that eval code can use lives, seen from the frame of
/// the code that calls `eval` (`compiler/eval.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum EvalSlot {
    /// A cell whose handle is in a register of the frame.
    Cell(Reg),
    /// A cell that the running function captured.
    Capture(u32),
}

/// A name that eval code can use (the scope description of ADR 0026
/// "Dynamic scope"): a binding of a scope of the calling function, or of
/// an enclosing function (then captured).
#[derive(Clone, Copy, Debug)]
pub(crate) struct EvalName {
    /// The name (an atom).
    pub(crate) name: Gc<JsString>,
    /// The kind of the binding.
    pub(crate) kind: swb_js_syntax::BindingKind,
    /// Its cell.
    pub(crate) slot: EvalSlot,
    /// A private name: its index in the private environment in `slot`.
    pub(crate) private: u32,
    /// The dynamic records between the call and the binding (for the
    /// names of enclosing functions: between the function and the
    /// binding).
    pub(crate) count: u32,
    /// The `this` binding of a derived constructor (loads check it).
    pub(crate) derived: bool,
    /// For the names that eval code captured from its caller (the
    /// description of eval code that calls `eval`): the results of the walk
    /// of `EvalDeclarationInstantiation` at the first call (see
    /// [`EvalWalk`]).
    pub(crate) walk: EvalWalk,
}

/// What the walk of `EvalDeclarationInstantiation` (§19.2.1.3 step 3,
/// Annex B.3.2.3 and B.3.4) finds for a name between a call of `eval`
/// and its variable environment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EvalWalk {
    /// A var of the name is a `SyntaxError`, and a block function of the
    /// name is not hoisted (B.3.2.3 `bindingExists`): a binding of the name
    /// between the call and the variable environment (a lexical
    /// declaration; not a catch parameter, B.3.4).
    pub(crate) conflict: bool,
    /// The static binding of the name in the variable environment.
    pub(crate) var_slot: Option<EvalSlot>,
}

/// A scope of a function that calls `eval`, in its scope description.
#[derive(Debug)]
pub(crate) struct EvalScope {
    /// The enclosing scope of the same function.
    pub(crate) parent: Option<u32>,
    /// Its names, sorted by the handle of the name.
    pub(crate) names: Box<[EvalName]>,
    /// The cell of its dynamic record (`%env`), if it has one.
    pub(crate) record: Option<EvalSlot>,
}

/// The variable environment of a call of `eval` (§19.2.1.1 step 16):
/// where the vars of sloppy eval code go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EvalVarEnv {
    /// Strict code: eval code declares nothing outside itself.
    #[default]
    None,
    /// The global object.
    Global,
    /// A record of eval vars (its cell).
    Record(EvalSlot),
}

/// The context of a call of `eval` that the parser needs (§19.2.1.1 steps
/// 5 to 11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EvalFlags {
    pub(crate) strict: bool,
    pub(crate) in_function: bool,
    pub(crate) in_method: bool,
    pub(crate) in_derived_constructor: bool,
    pub(crate) in_class_field_initializer: bool,
}

/// A direct `eval` call of a function.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EvalSite {
    /// The `CallEval` instruction.
    pub(crate) pc: u32,
    /// The scope of the call.
    pub(crate) scope: u32,
    /// The scope that holds the static bindings of the variable
    /// environment (its lexical declarations are between, §10.2.11 step
    /// 30); `None` in eval code (the variable environment is the caller's).
    pub(crate) var_scope: Option<u32>,
    /// The call is in the parameters: the variable environment is a record
    /// outside `var_scope`, whose bindings are all between (§10.2.11 step
    /// 20).
    pub(crate) params: bool,
    pub(crate) var_env: EvalVarEnv,
    pub(crate) flags: EvalFlags,
}

/// The scope description of a function that calls `eval` (ADR 0026
/// "Dynamic scope"): its scopes, the names of enclosing functions that it
/// captured for eval code, and its calls.
#[derive(Debug)]
pub(crate) struct EvalInfo {
    pub(crate) scopes: Box<[EvalScope]>,
    /// Sorted by the handle of the name.
    pub(crate) outer: Box<[EvalName]>,
    /// The cell of the innermost dynamic record outside the function.
    pub(crate) outer_record: Option<EvalSlot>,
    /// Eval code that calls `eval`: its names come from its caller, so the
    /// walks of `EvalDeclarationInstantiation` go on there (the results
    /// are in [`EvalName::walk`] of `outer`).
    pub(crate) eval_code: bool,
    /// Sorted by `pc`.
    pub(crate) sites: Box<[EvalSite]>,
}

impl EvalInfo {
    /// The call at `pc`.
    pub(crate) fn site(&self, pc: usize) -> Option<&EvalSite> {
        let index = self
            .sites
            .binary_search_by_key(&pc, |site| site.pc as usize)
            .ok()?;
        self.sites.get(index)
    }

    pub(super) fn trace(&self, tracer: &mut Tracer<'_>) {
        for scope in &self.scopes {
            for name in &scope.names {
                tracer.string(name.name);
            }
        }
        for name in &self.outer {
            tracer.string(name.name);
        }
    }

    pub(super) fn heap_size(&self) -> usize {
        size_of::<EvalInfo>()
            + self.scopes.len() * size_of::<EvalScope>()
            + self
                .scopes
                .iter()
                .map(|s| s.names.len() * size_of::<EvalName>())
                .sum::<usize>()
            + self.outer.len() * size_of::<EvalName>()
            + self.sites.len() * size_of::<EvalSite>()
    }
}

/// The names of a scope or of the outer list, sorted by handle: the one
/// with `name`.
pub(crate) fn find_eval_name(names: &[EvalName], name: Gc<JsString>) -> Option<&EvalName> {
    let key = (name.index(), name.generation());
    let index = names
        .binary_search_by_key(&key, |n| (n.name.index(), n.name.generation()))
        .ok()?;
    names.get(index)
}
