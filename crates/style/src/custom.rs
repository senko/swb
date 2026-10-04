//! Custom properties and `var()` substitution.
//!
//! <https://www.w3.org/TR/css-variables-1/>
//!
//! Custom properties are inherited. An element that declares none shares
//! its parent's map (`Arc`). Declared values that contain `var()` are
//! resolved against the element's other custom properties with a
//! depth-first search; properties in a dependency cycle become invalid
//! (the guaranteed-invalid value, which removes them from the map).
//!
//! `env()` is treated as a reference to an undefined variable: its
//! fallback is used, or the value is invalid.
//!
//! Substitution is bounded, so that hostile stylesheets cannot use
//! exponential expansion or build deeply nested values: at most
//! [`SUBSTITUTION_BUDGET`] component values (counted recursively) per
//! resolution, [`MAX_DEPTH`] nested references, and [`MAX_NESTING`] levels
//! of functions and blocks in a result. Values over a limit are invalid at
//! computed-value time.

use std::collections::HashMap;
use std::sync::Arc;

use swb_css::{ComponentValue, Function, SimpleBlock, trim_whitespace};

use crate::computed::CustomProperties;
use crate::properties::{CssWideKeyword, CustomDeclaration};

/// The maximum number of component values that substitution may produce
/// for one value.
const SUBSTITUTION_BUDGET: usize = 100_000;

/// The maximum depth of nested `var()` references between custom
/// properties.
const MAX_DEPTH: usize = 128;

/// The maximum nesting depth of functions and blocks in a substituted value
/// (the CSS parser's limit). Without it, each element could wrap an
/// inherited value in more parentheses, and recursive code on the value
/// (parsing, cloning, dropping) could overflow the stack.
const MAX_NESTING: usize = 64;

/// The number of component values in `values`, counted recursively, and
/// their nesting depth. Values are at most [`MAX_NESTING`] deep (the CSS
/// parser and substitution guarantee it), so the recursion is bounded.
fn measure(values: &[ComponentValue]) -> (usize, usize) {
    let mut count = values.len();
    let mut depth = 0;
    for v in values {
        let inner = match v {
            ComponentValue::Function(f) => &f.arguments,
            ComponentValue::Block(b) => &b.contents,
            _ => continue,
        };
        let (c, d) = measure(inner);
        count = count.saturating_add(c);
        depth = depth.max(d + 1);
    }
    (count, depth)
}

/// Looks up the value of a custom property during substitution.
trait VarLookup {
    /// The value of `name`, or `None` if it is undefined or invalid.
    fn lookup(&mut self, name: &str) -> Option<Arc<[ComponentValue]>>;
}

impl VarLookup for Option<&CustomProperties> {
    fn lookup(&mut self, name: &str) -> Option<Arc<[ComponentValue]>> {
        self.and_then(|map| map.get(name).cloned())
    }
}

/// Substitutes `var()` and `env()` in `tokens` using the element's computed
/// custom properties. Returns `None` if the value is invalid at
/// computed-value time.
pub(crate) fn substitute(
    tokens: &[ComponentValue],
    custom: Option<&CustomProperties>,
) -> Option<Vec<ComponentValue>> {
    let mut lookup = custom;
    let mut out = Vec::new();
    let mut budget = SUBSTITUTION_BUDGET;
    substitute_into(&mut lookup, tokens, &mut out, &mut budget, 0)?;
    Some(out)
}

/// Substitutes references in `tokens` and appends the result to `out`,
/// which is at nesting depth `depth`.
fn substitute_into<L: VarLookup>(
    lookup: &mut L,
    tokens: &[ComponentValue],
    out: &mut Vec<ComponentValue>,
    budget: &mut usize,
    depth: usize,
) -> Option<()> {
    for token in tokens {
        *budget = budget.checked_sub(1)?;
        match token {
            ComponentValue::Function(f) if f.name.eq_ignore_ascii_case("var") => {
                let (name, fallback) = reference_arguments(&f.arguments, true)?;
                match lookup.lookup(name) {
                    Some(value) => {
                        let (count, value_depth) = measure(&value);
                        *budget = budget.checked_sub(count)?;
                        if depth + value_depth > MAX_NESTING {
                            return None;
                        }
                        out.extend(value.iter().cloned());
                    }
                    None => substitute_into(lookup, fallback?, out, budget, depth)?,
                }
            }
            ComponentValue::Function(f) if f.name.eq_ignore_ascii_case("env") => {
                let (_, fallback) = reference_arguments(&f.arguments, false)?;
                substitute_into(lookup, fallback?, out, budget, depth)?;
            }
            ComponentValue::Function(_) | ComponentValue::Block(_) if depth >= MAX_NESTING => {
                return None;
            }
            ComponentValue::Function(f) => {
                let mut arguments = Vec::with_capacity(f.arguments.len());
                substitute_into(lookup, &f.arguments, &mut arguments, budget, depth + 1)?;
                out.push(ComponentValue::Function(Function {
                    name: f.name.clone(),
                    arguments,
                }));
            }
            ComponentValue::Block(b) => {
                let mut contents = Vec::with_capacity(b.contents.len());
                substitute_into(lookup, &b.contents, &mut contents, budget, depth + 1)?;
                out.push(ComponentValue::Block(SimpleBlock {
                    kind: b.kind,
                    contents,
                }));
            }
            other => out.push(other.clone()),
        }
    }
    Some(())
}

/// Splits the arguments of `var()` (or `env()`) into the name and the
/// fallback (if a comma is present; the fallback may be empty). For
/// `var()` the name must be a custom property name.
fn reference_arguments(
    args: &[ComponentValue],
    custom: bool,
) -> Option<(&str, Option<&[ComponentValue]>)> {
    let args = trim_whitespace(args);
    let (first, rest) = args.split_first()?;
    let ComponentValue::Ident(name) = first else {
        return None;
    };
    if custom && !name.starts_with("--") {
        return None;
    }
    let comma = rest.iter().position(ComponentValue::is_comma);
    let before_comma = &rest[..comma.unwrap_or(rest.len())];
    // `env()` may have integer indices after the name.
    let only_whitespace = before_comma.iter().all(|v| {
        v.is_whitespace() || (!custom && matches!(v, ComponentValue::Number(n) if n.is_integer()))
    });
    if !only_whitespace {
        return None;
    }
    let fallback = comma.map(|i| trim_whitespace(&rest[i + 1..]));
    Some((name, fallback))
}

/// The state of a custom property during resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Unresolved,
    InProgress,
    Done,
}

/// Resolves the custom properties declared on one element.
struct Resolver {
    map: CustomProperties,
    states: HashMap<Arc<str>, State>,
    stack: Vec<Arc<str>>,
    cyclic: Vec<Arc<str>>,
    budget: usize,
}

impl VarLookup for Resolver {
    fn lookup(&mut self, name: &str) -> Option<Arc<[ComponentValue]>> {
        self.resolve(name)
    }
}

impl Resolver {
    fn resolve(&mut self, name: &str) -> Option<Arc<[ComponentValue]>> {
        match self.states.get(name) {
            Some(State::InProgress) => {
                // A cycle: every property on the stack from `name` up is in
                // it.
                if let Some(start) = self.stack.iter().position(|n| &**n == name) {
                    self.cyclic.extend(self.stack[start..].iter().cloned());
                }
                return None;
            }
            Some(State::Unresolved) => {}
            Some(State::Done) | None => return self.map.get(name).cloned(),
        }
        let (key, raw) = self
            .map
            .get_key_value(name)
            .map(|(k, v)| (Arc::clone(k), Arc::clone(v)))?;
        self.states.insert(Arc::clone(&key), State::InProgress);
        self.stack.push(Arc::clone(&key));
        let result = if self.stack.len() > MAX_DEPTH {
            None
        } else {
            let mut out = Vec::new();
            let mut budget = self.budget;
            let result = substitute_into(self, &raw, &mut out, &mut budget, 0).map(|()| out);
            self.budget = budget;
            result
        };
        self.stack.pop();
        self.states.insert(Arc::clone(&key), State::Done);
        match result {
            Some(value) if !self.cyclic.contains(&key) => {
                let value: Arc<[ComponentValue]> = Arc::from(trim_whitespace(&value));
                self.map.insert(key, Arc::clone(&value));
                Some(value)
            }
            _ => {
                self.map.remove(&key);
                None
            }
        }
    }
}

/// Computes the custom properties of an element from its parent's and the
/// winning custom declarations (`declared`, at most one per name).
///
/// Returns the parent's map unchanged when nothing is declared.
pub(crate) fn compute_custom_properties(
    parent: Option<&Arc<CustomProperties>>,
    declared: &[(&Arc<str>, &CustomDeclaration)],
) -> Option<Arc<CustomProperties>> {
    if declared.is_empty() {
        return parent.cloned();
    }
    let mut map: CustomProperties = parent.map(|p| (**p).clone()).unwrap_or_default();
    let mut states = HashMap::new();
    for &(name, decl) in declared {
        match decl {
            CustomDeclaration::Value(value) => {
                map.insert(Arc::clone(name), Arc::clone(value));
                if crate::properties::has_references(value) {
                    states.insert(Arc::clone(name), State::Unresolved);
                }
            }
            CustomDeclaration::CssWide(CssWideKeyword::Initial) => {
                map.remove(&**name);
            }
            // Custom properties are inherited: `inherit` and `unset` keep
            // the parent's value. `revert` does too: no user-agent rule
            // sets custom properties.
            CustomDeclaration::CssWide(
                CssWideKeyword::Inherit | CssWideKeyword::Unset | CssWideKeyword::Revert,
            ) => match parent.and_then(|p| p.get(&**name)) {
                Some(v) => {
                    map.insert(Arc::clone(name), Arc::clone(v));
                }
                None => {
                    map.remove(&**name);
                }
            },
        }
    }
    if !states.is_empty() {
        let names: Vec<Arc<str>> = states.keys().cloned().collect();
        let mut resolver = Resolver {
            map,
            states,
            stack: Vec::new(),
            cyclic: Vec::new(),
            budget: SUBSTITUTION_BUDGET,
        };
        for name in names {
            resolver.resolve(&name);
        }
        map = resolver.map;
    }
    if map.is_empty() {
        return None;
    }
    Some(Arc::new(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::{parse_component_values, serialize_component_values};

    fn decls(pairs: &[(&str, &str)]) -> Vec<(Arc<str>, CustomDeclaration)> {
        pairs
            .iter()
            .map(|(n, v)| {
                let values = parse_component_values(v);
                let decl = match CssWideKeyword::from_value(&values) {
                    Some(k) => CustomDeclaration::CssWide(k),
                    None => CustomDeclaration::Value(Arc::from(trim_whitespace(&values))),
                };
                (Arc::from(*n), decl)
            })
            .collect()
    }

    fn compute(
        parent: Option<&Arc<CustomProperties>>,
        pairs: &[(&str, &str)],
    ) -> Option<Arc<CustomProperties>> {
        let d = decls(pairs);
        let refs: Vec<(&Arc<str>, &CustomDeclaration)> = d.iter().map(|(n, v)| (n, v)).collect();
        compute_custom_properties(parent, &refs)
    }

    fn get(map: Option<&Arc<CustomProperties>>, name: &str) -> Option<String> {
        map?.get(name).map(|v| serialize_component_values(v))
    }

    #[test]
    fn plain_and_inherited() {
        let root = compute(None, &[("--a", "1px"), ("--b", "red")]);
        assert_eq!(get(root.as_ref(), "--a").as_deref(), Some("1px"));
        let child = compute(root.as_ref(), &[]);
        assert!(Arc::ptr_eq(
            child.as_ref().expect("map"),
            root.as_ref().expect("map")
        ));
        let child = compute(root.as_ref(), &[("--b", "blue"), ("--a", "initial")]);
        assert_eq!(get(child.as_ref(), "--b").as_deref(), Some("blue"));
        assert_eq!(get(child.as_ref(), "--a"), None);
        assert_eq!(get(root.as_ref(), "--b").as_deref(), Some("red"));
    }

    #[test]
    fn references_and_fallbacks() {
        let root = compute(None, &[("--size", "10px")]);
        let map = compute(
            root.as_ref(),
            &[
                ("--a", "var(--size)"),
                ("--b", "calc(var(--a) * 2)"),
                ("--c", "var(--missing, 3px)"),
                ("--d", "var(--missing)"),
                ("--e", "var(--missing, var(--size))"),
                ("--f", "var(--g)"),
                ("--g", "var(--size) solid"),
                ("--h", "var(--missing,)"),
                ("--i", "env(safe-area-inset-top, 4px)"),
            ],
        );
        assert_eq!(get(map.as_ref(), "--a").as_deref(), Some("10px"));
        assert_eq!(get(map.as_ref(), "--b").as_deref(), Some("calc(10px * 2)"));
        assert_eq!(get(map.as_ref(), "--c").as_deref(), Some("3px"));
        assert_eq!(get(map.as_ref(), "--d"), None);
        assert_eq!(get(map.as_ref(), "--e").as_deref(), Some("10px"));
        assert_eq!(get(map.as_ref(), "--f").as_deref(), Some("10px solid"));
        assert_eq!(get(map.as_ref(), "--h").as_deref(), Some(""));
        assert_eq!(get(map.as_ref(), "--i").as_deref(), Some("4px"));
    }

    #[test]
    fn cycles_are_invalid() {
        let root = compute(None, &[("--outside", "1")]);
        let map = compute(
            root.as_ref(),
            &[
                ("--a", "var(--b, 1)"),
                ("--b", "var(--a, 2)"),
                ("--self", "var(--self, 3)"),
                ("--user", "var(--a, fallback)"),
                ("--ok", "var(--outside)"),
            ],
        );
        assert_eq!(get(map.as_ref(), "--a"), None);
        assert_eq!(get(map.as_ref(), "--b"), None);
        assert_eq!(get(map.as_ref(), "--self"), None);
        assert_eq!(get(map.as_ref(), "--user").as_deref(), Some("fallback"));
        assert_eq!(get(map.as_ref(), "--ok").as_deref(), Some("1"));
    }

    #[test]
    fn exponential_expansion_is_bounded() {
        let mut pairs = vec![("--l0".to_owned(), "x x x x x x x x x x".to_owned())];
        for i in 1..12 {
            let prev = format!("var(--l{})", i - 1);
            pairs.push((format!("--l{i}"), vec![prev; 10].join(" ")));
        }
        let pairs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let map = compute(None, &pairs);
        assert!(get(map.as_ref(), "--l2").is_some());
        assert_eq!(get(map.as_ref(), "--l11"), None);
    }

    #[test]
    fn nested_expansion_is_bounded() {
        // Each level wraps ten copies of the previous one in a block: the
        // top-level length stays small, the tree grows exponentially.
        let mut pairs = vec![("--l0".to_owned(), "x x x x x x x x x x".to_owned())];
        for i in 1..9 {
            let prev = format!("var(--l{})", i - 1);
            pairs.push((format!("--l{i}"), format!("({})", vec![prev; 10].join(" "))));
        }
        let pairs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let map = compute(None, &pairs);
        assert!(get(map.as_ref(), "--l2").is_some());
        assert_eq!(get(map.as_ref(), "--l8"), None);
    }

    #[test]
    fn nesting_depth_is_bounded_across_elements() {
        // Each element wraps the value inherited from its parent in more
        // parentheses (alternating between two properties, so that there
        // is no cycle).
        let wrap = |name: &str| format!("{}var({name}){}", "(".repeat(30), ")".repeat(30));
        let mut map = compute(None, &[("--a", "x"), ("--b", "x")]);
        let mut levels = 0;
        loop {
            let (name, other) = if levels % 2 == 0 {
                ("--a", "--b")
            } else {
                ("--b", "--a")
            };
            map = compute(map.as_ref(), &[(name, &wrap(other))]);
            levels += 1;
            if get(map.as_ref(), name).is_none() {
                break;
            }
            assert!(levels < 10, "nesting must stop growing");
        }
        assert!(levels >= 2, "{levels}");
    }

    #[test]
    fn substitution_in_values() {
        let map = compute(None, &[("--c", "blue"), ("--w", "2px")]);
        let tokens = parse_component_values("var(--w) solid var(--c)");
        let out = substitute(&tokens, map.as_deref()).expect("valid");
        assert_eq!(serialize_component_values(&out), "2px solid blue");
        let tokens = parse_component_values("var(--nope)");
        assert_eq!(substitute(&tokens, map.as_deref()), None);
        let tokens = parse_component_values("var(nope)");
        assert_eq!(substitute(&tokens, map.as_deref()), None);
        let tokens = parse_component_values("rgb(var(--nope, 1 2 3))");
        let out = substitute(&tokens, map.as_deref()).expect("valid");
        assert_eq!(serialize_component_values(&out), "rgb(1 2 3)");
    }
}
