//! Dynamic scope in the analysis (ADR 0026 "Dynamic scope"): the dynamic
//! environments of `with` bodies and of sloppy functions with a direct
//! `eval`, and the implicit bindings that eval code may use.
//!
//! - `with` (§14.11.2): the body has an object environment.
//! - Sloppy direct eval (§19.2.1.1 `PerformEval`, §19.2.1.3
//!   `EvalDeclarationInstantiation`): the vars that eval code declares go
//!   into the variable environment of the caller: the var scope of the
//!   body, or, for an eval in the parameter expressions, the environment
//!   outside the parameters (§10.2.11 step 20). Their names are not known
//!   before the eval runs, so each call has an object for them. In global
//!   code they become properties of the global object, which by-name
//!   access finds anyway; strict eval code declares nothing outside
//!   itself.
//!
//! Each dynamic environment has an implicit binding (`%env`) in its
//! scope; the resolution walk counts the environments that it passes
//! before it finds the static binding, and the references with a count
//! get a [`DynamicLookup`] whose `env` reference resolves (and captures
//! across functions) like any other name. The representation is linear:
//! one count per reference and one `env` reference per scope that has
//! such references.

use crate::ast::{Ast, FunctionId, FunctionKind, RefId, ScopeId};
use crate::interner::{NameId, names};

use super::{BindingKind, DynamicEnv, DynamicKind, DynamicLookup, IdMap, ScopeKind, ScopeTree};

/// Whether a name can be found in a dynamic environment: identifiers can,
/// the implicit names (`this`, `new.target`, `super`, the active function,
/// `%env`) cannot. Private names never reach one: they occur only in
/// class code, which is strict, and resolve inside their class.
pub(super) fn is_dynamic_name(name: NameId) -> bool {
    !matches!(
        name,
        names::THIS
            | names::NEW_TARGET
            | names::SUPER
            | names::ACTIVE_FUNCTION
            | names::DYNAMIC_ENV
    )
}

/// Creates the dynamic environments: one per `with` body, one per var
/// scope of a function with a sloppy direct eval in its body, and one per
/// function with a sloppy direct eval in its parameters. Adds the
/// references to the next outer environment.
pub(super) fn create_dynamic_envs(ast: &Ast, tree: &mut ScopeTree) {
    for index in 0..tree.scopes.len() {
        let scope = ScopeId::from_index(index);
        if tree.scope(scope).kind == ScopeKind::With {
            add_env(tree, scope, DynamicKind::With);
        }
    }
    for id in ast.function_ids() {
        let function = ast.function(id);
        if matches!(
            function.kind,
            FunctionKind::Script | FunctionKind::Eval | FunctionKind::Module
        ) {
            continue;
        }
        // An eval in the parameters means parameter expressions, so the
        // body has a scope of its own then.
        let facts = tree.function(id);
        let (body, params) = (facts.sloppy_body_eval, facts.sloppy_param_eval);
        if params {
            add_env(tree, function.scope, DynamicKind::EvalVars);
        }
        if body {
            add_env(tree, function.body_scope, DynamicKind::EvalVars);
        }
    }
    for index in 0..tree.dynamic_envs.len() {
        let Some(&env) = tree.dynamic_envs.get(index) else {
            break;
        };
        let entry = tree.scope(env.scope);
        let (start, parent) = (entry.start, entry.parent);
        let outer =
            parent.map(|parent| tree.push_reference(names::DYNAMIC_ENV, parent, start, false));
        if let Some(env) = tree.dynamic_envs.get_mut(index) {
            env.outer = outer;
        }
    }
}

fn add_env(tree: &mut ScopeTree, scope: ScopeId, kind: DynamicKind) {
    if tree.scope(scope).dynamic_env.is_some() {
        return;
    }
    let start = tree.scope(scope).start;
    let binding = tree.new_binding(scope, names::DYNAMIC_ENV, BindingKind::DynamicEnv, start);
    let index = tree.dynamic_envs.len() as u32;
    tree.dynamic_envs.push(DynamicEnv {
        kind,
        scope,
        binding,
        outer: None,
    });
    if let Some(entry) = tree.scope_mut(scope) {
        entry.dynamic_env = Some(index);
    }
}

/// Drops the outer links that do not resolve to a dynamic environment
/// (the outermost environment of a script or of eval code).
pub(super) fn finish_outer_links(tree: &mut ScopeTree) {
    for index in 0..tree.dynamic_envs.len() {
        let Some(outer) = tree.dynamic_envs.get(index).and_then(|env| env.outer) else {
            continue;
        };
        let resolved = tree
            .reference(outer)
            .binding
            .is_some_and(|b| tree.binding(b).kind == BindingKind::DynamicEnv);
        if !resolved && let Some(env) = tree.dynamic_envs.get_mut(index) {
            env.outer = None;
        }
    }
}

/// Records the dynamic lookups: for each reference with a count (from
/// the resolution walk), the `%env` reference of its scope (one per
/// scope, created here) and the count. Returns the index of the first
/// new reference, which the caller resolves next.
pub(super) fn record_lookups(tree: &mut ScopeTree, counts: &[(RefId, u32)]) -> usize {
    let first_new = tree.references.len();
    let mut per_scope: IdMap<RefId> = IdMap::default();
    let mut lookups = Vec::with_capacity(counts.len());
    for &(id, count) in counts {
        let reference = *tree.reference(id);
        let key = reference.scope.index() as u64;
        let env = if let Some(&env) = per_scope.get(&key) {
            env
        } else {
            let env =
                tree.push_reference(names::DYNAMIC_ENV, reference.scope, reference.offset, false);
            per_scope.insert(key, env);
            env
        };
        lookups.push((id, DynamicLookup { env, count }));
    }
    lookups.sort_by_key(|&(id, _)| id);
    tree.dynamic_lookups = lookups;
    first_new
}

/// A function with a direct `eval` needs the implicit bindings that eval
/// code may use (§19.2.1.1: `this`, `new.target` in functions, `super`
/// properties in methods, `super(...)` in derived constructors, and
/// `arguments` through §10.2.11), also if its own code does not use
/// them: adds a reference to each from the function's scope, which
/// creates the binding (in an arrow function: captures the enclosing
/// function's). Eval code takes them from its caller.
pub(super) fn add_eval_references(ast: &Ast, tree: &mut ScopeTree) {
    for id in ast.function_ids() {
        if !tree.function(id).has_direct_eval {
            continue;
        }
        let Some(owner) = this_function(ast, id) else {
            continue;
        };
        let kind = ast.function(owner).kind;
        let scope = ast.function(id).scope;
        let start = tree.scope(scope).start;
        let in_function = !matches!(kind, FunctionKind::Script | FunctionKind::Module);
        let field = matches!(
            kind,
            FunctionKind::InstanceInitializer | FunctionKind::StaticInitializer
        );
        let wanted = [
            (names::THIS, true),
            (names::NEW_TARGET, in_function),
            (names::ARGUMENTS, in_function && !field),
            (names::SUPER, kind.has_home_object()),
            (
                names::ACTIVE_FUNCTION,
                kind == FunctionKind::DerivedConstructor,
            ),
        ];
        for (name, needed) in wanted {
            if needed {
                tree.push_reference(name, scope, start, false);
            }
        }
    }
}

/// The nearest function around `id` (or `id` itself) that has its own
/// `this`; `None` in eval code, which takes `this` from its caller.
fn this_function(ast: &Ast, id: FunctionId) -> Option<FunctionId> {
    let mut current = id;
    loop {
        let function = ast.function(current);
        match function.kind {
            FunctionKind::Eval => return None,
            FunctionKind::Arrow => current = function.parent?,
            _ => return Some(current),
        }
    }
}
