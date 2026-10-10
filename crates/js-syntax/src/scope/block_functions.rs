//! Annex B.3.2 and B.3.3: the var bindings of function declarations in
//! blocks of sloppy mode code
//! (<https://tc39.es/ecma262/2025/#sec-block-level-function-declarations-web-legacy-compatibility-semantics>).
//!
//! The parser declares every block-level function declaration as a
//! lexical binding of its block (an `if` clause gets a synthetic block,
//! B.3.3). When the parse has ended and all declarations are known, this
//! pass decides for each plain function declaration of sloppy mode code
//! (not a generator or async function) whether it also gets a var binding
//! in the enclosing var scope (B.3.2.1 for functions, B.3.2.2 for global
//! code, B.3.2.3 for eval code): only if replacing it by `var F` would not
//! be an early error (no scope between the block and the var scope, and
//! not the var scope itself, declares `F` lexically; a catch parameter
//! that is a single identifier does not count, B.3.4) and `F` is not a
//! parameter name. It records on the declaration
//! ([`super::FunctionScope::block_function_var`]) a reference in the var
//! scope that resolves to the var binding.
//!
//! Deviations, both as in V8 (measured with Node 22): a function
//! declaration directly in a labelled statement in a block is hoisted too,
//! and so are duplicate declarations in one block (B.3.2.4), whose
//! literal replacement by `var F` would conflict with the other one. A
//! block function named `arguments` in a function that has an arguments
//! object is not hoisted, as test262 expects
//! (`annexB/language/function-code/block-decl-func-skip-arguments.js`;
//! V8 assigns it to the `arguments` binding); in an arrow function it
//! gets a var binding of its own, as in V8.
//!
//! Cost: one walk from the block to its var scope per declaration, so
//! declarations times nesting depth, which the recursion budget bounds.

use crate::ast::{Ast, FunctionId, FunctionKind, ScopeId};
use crate::interner::{NameId, names};

use super::{BindingKind, ScopeTree};

/// Gives each qualifying block-level function declaration its var
/// binding and the reference to it.
pub(super) fn hoist_block_functions(ast: &Ast, tree: &mut ScopeTree) {
    for index in 0..tree.function_decls.len() {
        let Some(&(block, function)) = tree.function_decls.get(index) else {
            break;
        };
        if tree.is_var_scope(block) {
            continue;
        }
        let Some(ident) = ast.function(function).name else {
            continue;
        };
        let Some(binding) = tree.declared_in(block, ident.name) else {
            continue;
        };
        if tree.special_block_functions.contains(&binding) {
            continue;
        }
        let var_scope = tree.scope(block).hoist_to;
        if !replaceable_by_var(tree, block, var_scope, ident.name) {
            continue;
        }
        let offset = tree.reference(ident.reference).offset;
        if !ensure_var_binding(ast, tree, var_scope, ident.name, offset) {
            continue;
        }
        let reference = tree.push_reference(ident.name, var_scope, offset, false);
        if let Some(record) = tree.function_mut(function) {
            record.block_function_var = Some(reference);
        }
    }
}

/// Whether `var name` in `block` would be valid: no scope from the
/// block's parent up to (not including) `var_scope` declares the name,
/// except as a single-identifier catch parameter (Annex B.3.4).
fn replaceable_by_var(tree: &ScopeTree, block: ScopeId, var_scope: ScopeId, name: NameId) -> bool {
    let mut current = tree.scope(block).parent;
    while let Some(scope) = current {
        if scope == var_scope {
            return true;
        }
        if let Some(binding) = tree.declared_in(scope, name) {
            let simple_catch = tree.binding(binding).kind == BindingKind::CatchParameter
                && !tree.pattern_catch_scopes.contains(&scope);
            if !simple_catch {
                return false;
            }
        }
        current = tree.scope(scope).parent;
    }
    false
}

/// Finds or creates the var binding of `name` in `var_scope`. Returns
/// false if the declaration is not hoisted: the var scope declares the
/// name lexically or as a parameter, or the name is `arguments` in a
/// function with an arguments object.
fn ensure_var_binding(
    ast: &Ast,
    tree: &mut ScopeTree,
    var_scope: ScopeId,
    name: NameId,
    offset: u32,
) -> bool {
    let function = tree.scope(var_scope).function;
    if tree.is_parameter_of_body(var_scope, name)
        || (name == names::ARGUMENTS && has_arguments_object(ast, function))
    {
        return false;
    }
    if let Some(existing) = tree.declared_in(var_scope, name) {
        let kind = tree.binding(existing).kind;
        return !kind.is_lexical() && kind != BindingKind::Parameter;
    }
    tree.new_binding(var_scope, name, BindingKind::BlockFunctionVar, offset);
    true
}

/// Whether a function has an arguments object that a block function named
/// `arguments` must not replace (§10.2.11 `argumentsObjectNeeded`; the
/// other conditions mean a declaration of the name, which
/// [`ensure_var_binding`] checks).
fn has_arguments_object(ast: &Ast, function: FunctionId) -> bool {
    !matches!(
        ast.function(function).kind,
        FunctionKind::Script
            | FunctionKind::Arrow
            | FunctionKind::Eval
            | FunctionKind::Module
            | FunctionKind::InstanceInitializer
            | FunctionKind::StaticInitializer
    )
}
