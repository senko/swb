//! The scope analysis (ADR 0026 section 3): runs after the parse over the
//! whole function nest of a script.
//!
//! 1. Resolves each identifier occurrence by walking its scope chain
//!    (`ResolveBinding`, §9.4.2). `this`, `new.target`, `arguments`, the
//!    home object (`super`) and the active function (`%function`)
//!    resolve to implicit bindings of the nearest non-arrow function
//!    (§10.2.11), which are created on first use. Private names (`#x`)
//!    resolve like other names: their bindings are in the class body
//!    scopes.
//! 2. Marks bindings that an inner function uses as captured, and adds
//!    the binding to the captures of every function between the use and
//!    the declaration (flat closures).
//! 3. Assigns storage: script-level declarations are global and accessed
//!    by name; other bindings get a register of their function's frame,
//!    and captured ones hold a cell there. Registers of sibling blocks are
//!    reused.
//! 4. Fills in the resolution of each occurrence and the temporal dead
//!    zone checks.
//!
//! The analysis works on the tables that the parser recorded; it does
//! not walk the tree, so it uses no recursion.
//!
//! Costs: the resolution of a name is cached per (scope, name), so a
//! repeated name in a scope costs one lookup. A name that is new in its
//! scope walks the scope chain, at most the number of enclosing scopes;
//! so the worst case is (distinct (scope, name) pairs) x (depth). The
//! capture lists are bounded by [`Limits`].

use crate::ast::{Ast, BindingId, FunctionId, FunctionKind, List, PatternKind, ScopeId};
use crate::error::ParseError;
use crate::interner::{NameId, names};
use crate::messages;

use super::{
    BindingKind, Capture, CaptureSource, FunctionScope, IdMap, Resolution, ScopeKind, ScopeTree,
    Storage, key,
};

/// The resource limits of the analysis. Tests use small values.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// The most capture entries in all functions of a script. A flat
    /// closure copies a captured variable into every function between the
    /// declaration and the use, so deep nesting multiplies the entries;
    /// about 60 bytes each, 2^20 entries are about 60 MB. Real scripts
    /// have far fewer. Past it: `RangeError`.
    pub(crate) total_captures: usize,
    /// The most captures of one function: the operand width. Past it:
    /// `RangeError`.
    pub(crate) function_captures: usize,
    /// The most registers for the declared bindings (and parameters) of
    /// one function. ADR 0026 allows 65,535 registers in all; the
    /// compiler needs the rest for temporaries, so the analysis keeps a
    /// reserve of [`TEMPORARIES_RESERVE`]. The compiler checks the total
    /// register count again and fails with the same message.
    pub(crate) declared_registers: u32,
}

/// Registers that the analysis leaves to the compiler's temporaries.
pub(crate) const TEMPORARIES_RESERVE: u32 = 1024;

impl Limits {
    /// The limits of a script.
    pub(crate) const DEFAULT: Limits = Limits {
        total_captures: 1 << 20,
        function_captures: 65_535,
        declared_registers: 65_535 - TEMPORARIES_RESERVE,
    };
}

/// Runs the analysis on the tables of a parsed script.
pub(crate) fn analyze(ast: &Ast, tree: &mut ScopeTree, limits: &Limits) -> Result<(), ParseError> {
    mark_var_arguments(ast, tree);
    let mut captures: IdMap<u32> = IdMap::default();
    resolve_references(ast, tree, &mut captures, limits)?;
    mark_direct_eval(ast, tree);
    mark_mapped_arguments(ast, tree);
    group_by_scope(tree);
    assign_storage(tree, limits)?;
    finish_references(tree, &captures);
    finish_captures(ast, tree, &captures);
    mark_needs_tdz(tree);
    Ok(())
}

/// The map key of a capture: (function, binding).
fn capture_key(function: FunctionId, binding: BindingId) -> u64 {
    ((function.index() as u64) << 32) | binding.index() as u64
}

/// A `var arguments` in a non-arrow function is the binding of the
/// arguments object (§10.2.11 steps 15 to 22: the object is needed unless
/// a parameter, a function declaration or a lexical declaration has the
/// name). In a function with parameter expressions, a `var arguments` of
/// the body is a binding of its own that starts with the object, which
/// the function then creates.
fn mark_var_arguments(ast: &Ast, tree: &mut ScopeTree) {
    for id in ast.function_ids() {
        let function = ast.function(id);
        if !function.kind.has_own_this() || function.kind == FunctionKind::Script {
            continue;
        }
        if function.body_scope != function.scope {
            let in_body = tree.declared_in(function.body_scope, names::ARGUMENTS);
            if in_body.is_some_and(|b| tree.binding(b).kind == BindingKind::Var)
                && tree.declared_in(function.scope, names::ARGUMENTS).is_none()
            {
                implicit_binding(tree, id, function.scope, BindingKind::Arguments);
            }
            continue;
        }
        let Some(binding) = tree.declared_in(function.scope, names::ARGUMENTS) else {
            continue;
        };
        if tree.binding(binding).kind == BindingKind::Var {
            if let Some(b) = tree.binding_mut(binding) {
                b.kind = BindingKind::Arguments;
            }
            if let Some(f) = tree.function_mut(id) {
                f.arguments_binding = Some(binding);
            }
        }
    }
}

/// Step 1 and 2: the binding of each occurrence, and the captures.
fn resolve_references(
    ast: &Ast,
    tree: &mut ScopeTree,
    captures: &mut IdMap<u32>,
    limits: &Limits,
) -> Result<(), ParseError> {
    let mut cache: IdMap<Option<BindingId>> = IdMap::default();
    let mut total = 0;
    for index in 0..tree.references.len() {
        let Some(&reference) = tree.references.get(index) else {
            break;
        };
        if reference.dead {
            continue;
        }
        let cache_key = key(reference.scope, reference.name);
        let resolved = if let Some(&cached) = cache.get(&cache_key) {
            cached
        } else {
            let found = resolve(ast, tree, reference.scope, reference.name);
            cache.insert(cache_key, found);
            found
        };
        let Some(binding) = resolved else {
            continue;
        };
        if let Some(r) = tree.references.get_mut(index) {
            r.binding = Some(binding);
        }
        let declared_in = tree.binding(binding).scope;
        if is_global(tree, binding) {
            continue;
        }
        let user = tree.scope(reference.scope).function;
        let owner = tree.scope(declared_in).function;
        if user != owner {
            if let Some(b) = tree.binding_mut(binding) {
                b.captured = true;
            }
            let chain = Chain {
                user,
                owner,
                binding,
            };
            add_capture_chain(ast, tree, captures, chain, &mut total, limits)?;
        }
    }
    Ok(())
}

/// The binding that `name` resolves to from `scope`, or `None` for a
/// global or unresolvable name.
fn resolve(ast: &Ast, tree: &mut ScopeTree, scope: ScopeId, name: NameId) -> Option<BindingId> {
    let mut current = scope;
    loop {
        let entry = tree.scope(current);
        if entry.binding_count > 0
            && let Some(binding) = tree.declared_in(current, name)
        {
            return Some(binding);
        }
        if matches!(entry.kind, ScopeKind::Function | ScopeKind::Script) {
            let function = entry.function;
            let kind = ast.function(function).kind;
            if kind.has_own_this()
                && let Some(implicit) = implicit_kind(kind, name)
            {
                return Some(implicit_binding(tree, function, current, implicit));
            }
        }
        current = entry.parent?;
    }
}

/// The implicit binding that `name` names in a non-arrow function of
/// `kind`, if any. The parser allows `super` and `new.target` only where
/// the function has them.
fn implicit_kind(kind: FunctionKind, name: NameId) -> Option<BindingKind> {
    let script = kind == FunctionKind::Script;
    Some(match name {
        names::THIS => BindingKind::This,
        names::NEW_TARGET if !script => BindingKind::NewTarget,
        names::ARGUMENTS if !script => BindingKind::Arguments,
        names::SUPER if !script => BindingKind::HomeObject,
        names::ACTIVE_FUNCTION if !script => BindingKind::ActiveFunction,
        _ => return None,
    })
}

/// The slot of an implicit binding of `kind` in a function's record.
fn implicit_slot(f: &mut FunctionScope, kind: BindingKind) -> &mut Option<BindingId> {
    match kind {
        BindingKind::This => &mut f.this_binding,
        BindingKind::NewTarget => &mut f.new_target_binding,
        BindingKind::HomeObject => &mut f.home_object_binding,
        BindingKind::ActiveFunction => &mut f.active_function_binding,
        _ => &mut f.arguments_binding,
    }
}

/// An implicit binding of a function (`this`, `new.target`, `arguments`,
/// the home object, the active function), created on first use.
fn implicit_binding(
    tree: &mut ScopeTree,
    function: FunctionId,
    scope: ScopeId,
    kind: BindingKind,
) -> BindingId {
    let existing = tree
        .function_mut(function)
        .and_then(|f| *implicit_slot(f, kind));
    if let Some(binding) = existing {
        return binding;
    }
    let name = match kind {
        BindingKind::This => names::THIS,
        BindingKind::NewTarget => names::NEW_TARGET,
        BindingKind::HomeObject => names::SUPER,
        BindingKind::ActiveFunction => names::ACTIVE_FUNCTION,
        _ => names::ARGUMENTS,
    };
    let offset = tree.scope(scope).start;
    let binding = tree.new_binding(scope, name, kind, offset);
    if let Some(f) = tree.function_mut(function) {
        *implicit_slot(f, kind) = Some(binding);
    }
    binding
}

/// Whether a binding is a global binding: a declaration at the top level
/// of the script (not the implicit `this`).
fn is_global(tree: &ScopeTree, binding: BindingId) -> bool {
    let binding = tree.binding(binding);
    binding.kind != BindingKind::This && tree.scope(binding.scope).kind == ScopeKind::Script
}

/// Whether a binding has a temporal dead zone: `let`, `const`, and the
/// parameters of a function with parameter expressions.
fn has_tdz(binding: &super::Binding) -> bool {
    binding.kind.is_lexical()
        || (binding.kind == BindingKind::Parameter && binding.init_end > binding.offset)
}

/// A captured binding: used in `user`, declared in `owner`.
#[derive(Clone, Copy)]
struct Chain {
    user: FunctionId,
    owner: FunctionId,
    binding: BindingId,
}

/// Adds the binding to the captures of `user` and of every function
/// between `user` and `owner`. `total` counts the entries of the script.
fn add_capture_chain(
    ast: &Ast,
    tree: &mut ScopeTree,
    captures: &mut IdMap<u32>,
    chain: Chain,
    total: &mut usize,
    limits: &Limits,
) -> Result<(), ParseError> {
    let mut function = chain.user;
    while function != chain.owner {
        let key = capture_key(function, chain.binding);
        if captures.contains_key(&key) {
            // The functions above have it too.
            return Ok(());
        }
        let offset = ast.function(function).span.start;
        let Some(f) = tree.function_mut(function) else {
            return Ok(());
        };
        *total += 1;
        if *total > limits.total_captures || f.captures.len() >= limits.function_captures {
            return Err(ParseError::range(offset, messages::TOO_MANY_CAPTURES));
        }
        captures.insert(key, f.captures.len() as u32);
        f.captures.push(Capture {
            binding: chain.binding,
            source: CaptureSource::ParentCapture(0),
        });
        match ast.function(function).parent {
            Some(parent) => function = parent,
            None => return Ok(()),
        }
    }
    Ok(())
}

/// A sloppy-mode non-arrow function with simple parameters and an
/// `arguments` binding has a mapped arguments object (§10.4.4): it
/// aliases the parameters, so every parameter is a cell that the object
/// can hold. Strict functions and functions with non-simple parameters
/// get an unmapped object and keep registers.
fn mark_mapped_arguments(ast: &Ast, tree: &mut ScopeTree) {
    for id in ast.function_ids() {
        let function = ast.function(id);
        let simple = ast
            .patterns(function.params)
            .iter()
            .all(|&p| matches!(ast.pattern(p).kind, PatternKind::Identifier(_)));
        let mapped = function.kind.has_own_this()
            && !function.strict
            && simple
            && tree.function(id).arguments_binding.is_some();
        if let Some(f) = tree.function_mut(id) {
            f.mapped_arguments = mapped;
        }
    }
}

/// A function with a direct `eval` and all functions around it keep a
/// scope description; their bindings become cells.
fn mark_direct_eval(ast: &Ast, tree: &mut ScopeTree) {
    for id in ast.function_ids() {
        if !tree.function(id).has_direct_eval {
            continue;
        }
        let mut function = Some(id);
        while let Some(current) = function {
            let Some(f) = tree.function_mut(current) else {
                break;
            };
            if f.needs_scope_description {
                break;
            }
            f.needs_scope_description = true;
            function = ast.function(current).parent;
        }
    }
}

/// Fills [`super::Scope::bindings`] and [`super::Scope::functions`]: the
/// bindings and the function declarations of each scope, in order.
fn group_by_scope(tree: &mut ScopeTree) {
    let scope_count = tree.scopes.len();
    let mut counts = vec![0usize; scope_count + 1];
    for binding in &tree.bindings {
        if let Some(c) = counts.get_mut(binding.scope.index() + 1) {
            *c += 1;
        }
    }
    let starts = prefix_sums(&mut counts);
    let mut order = vec![BindingId::from_index(0); tree.bindings.len()];
    let mut fill = starts.clone();
    for (index, binding) in tree.bindings.iter().enumerate() {
        if let Some(slot) = fill.get_mut(binding.scope.index())
            && let Some(entry) = order.get_mut(*slot)
        {
            *entry = BindingId::from_index(index);
            *slot += 1;
        }
    }
    for (index, scope) in tree.scopes.iter_mut().enumerate() {
        let start = starts.get(index).copied().unwrap_or(0);
        let end = starts.get(index + 1).copied().unwrap_or(start);
        scope.bindings = List::new(start, end - start);
    }
    tree.scope_bindings = order;

    // Function declarations: a stable sort by scope keeps source order.
    let mut decls = std::mem::take(&mut tree.function_decls);
    decls.sort_by_key(|&(scope, _)| scope.index());
    let mut start = 0;
    while let Some(&(scope, _)) = decls.get(start) {
        let end = decls
            .iter()
            .skip(start)
            .position(|&(s, _)| s != scope)
            .map_or(decls.len(), |n| start + n);
        if let Some(entry) = tree.scopes.get_mut(scope.index()) {
            entry.functions = List::new(start, end - start);
        }
        start = end;
    }
    tree.scope_functions = decls.iter().map(|&(_, f)| f).collect();
    tree.function_decls = decls;
}

/// Turns counts at `1..` into start offsets; returns the starts per index.
fn prefix_sums(counts: &mut [usize]) -> Vec<usize> {
    let mut total = 0;
    for count in counts.iter_mut() {
        total += *count;
        *count = total;
    }
    counts.to_vec()
}

/// Step 3: registers and cells, by a depth-first walk over the scope
/// tree with an explicit stack. Registers of a block are free again after
/// the block, so sibling blocks share them.
fn assign_storage(tree: &mut ScopeTree, limits: &Limits) -> Result<(), ParseError> {
    let children = scope_children(tree);
    let function_count = tree.functions.len();
    let mut next: Vec<u32> = vec![0; function_count];
    for (index, f) in tree.functions.iter_mut().enumerate() {
        let count = f.argument_registers;
        if count > limits.declared_registers {
            return Err(ParseError::syntax(0, messages::TOO_MANY_VARIABLES));
        }
        if let Some(n) = next.get_mut(index) {
            *n = count;
        }
        f.register_count = count;
        for (position, &binding) in f.params.iter().enumerate() {
            if let Some(b) = tree.bindings.get_mut(binding.index()) {
                b.storage = Storage::Register(position as u16);
            }
        }
    }
    let Some(root) = (!tree.scopes.is_empty()).then(|| ScopeId::from_index(0)) else {
        return Ok(());
    };
    // (scope, next child, the function's next register before the scope)
    let mut stack: Vec<(ScopeId, usize, u32)> = Vec::new();
    let saved = enter_scope(tree, root, &mut next, limits)?;
    stack.push((root, 0, saved));
    while let Some(top) = stack.last_mut() {
        let (scope, cursor, saved) = *top;
        let child = children
            .get(scope.index())
            .and_then(|list| list.get(cursor))
            .copied();
        if let Some(child) = child {
            top.1 += 1;
            let saved = enter_scope(tree, child, &mut next, limits)?;
            stack.push((child, 0, saved));
        } else {
            let function = tree.scope(scope).function;
            if let Some(n) = next.get_mut(function.index()) {
                *n = saved;
            }
            stack.pop();
        }
    }
    Ok(())
}

/// The child scopes of each scope.
fn scope_children(tree: &ScopeTree) -> Vec<Vec<ScopeId>> {
    let mut children = vec![Vec::new(); tree.scopes.len()];
    for (index, scope) in tree.scopes.iter().enumerate() {
        if let Some(parent) = scope.parent
            && let Some(list) = children.get_mut(parent.index())
        {
            list.push(ScopeId::from_index(index));
        }
    }
    children
}

/// Assigns the storage of a scope's bindings. Returns the function's next
/// register before the scope, to restore when the scope ends.
fn enter_scope(
    tree: &mut ScopeTree,
    scope: ScopeId,
    next: &mut [u32],
    limits: &Limits,
) -> Result<u32, ParseError> {
    let entry = tree.scope(scope);
    let function = entry.function;
    let kind = entry.kind;
    let is_root = entry
        .parent
        .is_none_or(|parent| tree.scope(parent).function != function);
    let start = entry.start;
    let all_cells = tree.function(function).needs_scope_description;
    let params = tree.function(function).argument_registers;
    let mapped = tree.function(function).mapped_arguments;
    let slot = next
        .get_mut(function.index())
        .expect("one counter per function");
    let saved = *slot;
    if is_root {
        *slot = params;
    }
    let mut register = *slot;
    let mut has_cells = false;
    let mut per_iteration = false;
    let bindings: Vec<BindingId> = tree.bindings_of(scope).to_vec();
    for id in bindings {
        let binding = tree.binding(id);
        let is_param = binding.kind == BindingKind::Parameter;
        let cell = binding.captured || all_cells || (mapped && is_param);
        let storage = if binding.kind != BindingKind::This && kind == ScopeKind::Script {
            Storage::Global
        } else if binding.kind == BindingKind::Parameter && binding.storage != Storage::Unassigned {
            match binding.storage {
                Storage::Register(r) if cell => Storage::Cell(r),
                other => other,
            }
        } else {
            if register >= limits.declared_registers {
                return Err(ParseError::syntax(start, messages::TOO_MANY_VARIABLES));
            }
            let r = u16::try_from(register)
                .map_err(|_| ParseError::syntax(start, messages::TOO_MANY_VARIABLES))?;
            register += 1;
            if cell {
                Storage::Cell(r)
            } else {
                Storage::Register(r)
            }
        };
        if matches!(storage, Storage::Cell(_)) {
            has_cells = true;
            per_iteration |= (kind == ScopeKind::For && binding.kind == BindingKind::Let)
                || (kind == ScopeKind::ForInOf && binding.kind.is_lexical());
        }
        if let Some(b) = tree.binding_mut(id) {
            b.storage = storage;
        }
    }
    *slot = register;
    if let Some(f) = tree.function_mut(function) {
        f.register_count = f.register_count.max(register);
    }
    if let Some(s) = tree.scope_mut(scope) {
        s.has_cells = has_cells;
        s.per_iteration = per_iteration;
    }
    Ok(saved)
}

/// Step 4: the resolution and TDZ check of each occurrence.
fn finish_references(tree: &mut ScopeTree, captures: &IdMap<u32>) {
    for index in 0..tree.references.len() {
        let Some(&reference) = tree.references.get(index) else {
            break;
        };
        if reference.dead {
            continue;
        }
        let user = tree.scope(reference.scope).function;
        let (resolution, tdz_check) = match reference.binding {
            None => (Resolution::Global, false),
            Some(id) => {
                let binding = tree.binding(id);
                let owner = tree.scope(binding.scope).function;
                let resolution = match binding.storage {
                    Storage::Global | Storage::Unassigned => Resolution::Global,
                    _ if user != owner => captures
                        .get(&capture_key(user, id))
                        .map_or(Resolution::Global, |&i| Resolution::Capture(i)),
                    Storage::Register(r) => Resolution::Register(r),
                    Storage::Cell(r) => Resolution::Cell(r),
                };
                // A load in the same function after the declarator has run
                // needs no check (memo 2.4). The clauses of a `switch`
                // share one scope, and a clause can run without the
                // clauses before it, so their bindings are always checked.
                let initialized = user == owner
                    && reference.offset >= binding.init_end
                    && tree.scope(binding.scope).kind != ScopeKind::Switch;
                let tdz = has_tdz(binding) && !reference.declaration && !initialized;
                (resolution, tdz)
            }
        };
        if let Some(r) = tree.references.get_mut(index) {
            r.resolution = resolution;
            r.tdz_check = tdz_check;
        }
    }
}

/// Sets [`super::Binding::needs_tdz`]: lexical bindings with a checked
/// reference, bindings that closures can read, and bindings of functions
/// with a direct `eval`.
fn mark_needs_tdz(tree: &mut ScopeTree) {
    let checked: Vec<BindingId> = tree
        .references
        .iter()
        .filter(|r| r.tdz_check)
        .filter_map(|r| r.binding)
        .collect();
    for id in checked {
        if let Some(b) = tree.binding_mut(id) {
            b.needs_tdz = true;
        }
    }
    for index in 0..tree.bindings.len() {
        let id = BindingId::from_index(index);
        let binding = tree.binding(id);
        if !has_tdz(binding) || binding.storage == Storage::Global {
            continue;
        }
        let function = tree.scope(binding.scope).function;
        if (binding.captured || tree.function(function).needs_scope_description)
            && let Some(b) = tree.binding_mut(id)
        {
            b.needs_tdz = true;
        }
    }
}

/// Fills [`Capture::source`]: from the parent's register or the parent's
/// own captures.
fn finish_captures(ast: &Ast, tree: &mut ScopeTree, captures: &IdMap<u32>) {
    for function in ast.function_ids() {
        let Some(parent) = ast.function(function).parent else {
            continue;
        };
        let list = tree.function(function).captures.clone();
        let sources: Vec<CaptureSource> = list
            .iter()
            .map(|capture| {
                let binding = tree.binding(capture.binding);
                if tree.scope(binding.scope).function == parent {
                    let register = match binding.storage {
                        Storage::Cell(r) | Storage::Register(r) => r,
                        _ => 0,
                    };
                    CaptureSource::ParentRegister(register)
                } else {
                    let index = captures
                        .get(&capture_key(parent, capture.binding))
                        .copied()
                        .unwrap_or(0);
                    CaptureSource::ParentCapture(index)
                }
            })
            .collect();
        if let Some(f) = tree.function_mut(function) {
            for (capture, source) in f.captures.iter_mut().zip(sources) {
                capture.source = source;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::key;

    #[test]
    fn capture_keys_differ_by_function_and_binding() {
        let a = capture_key(FunctionId::from_index(1), BindingId::from_index(2));
        let b = capture_key(FunctionId::from_index(2), BindingId::from_index(1));
        assert_ne!(a, b);
        assert_ne!(
            key(ScopeId::from_index(1), names::THIS),
            key(ScopeId::from_index(0), names::THIS)
        );
    }
}
