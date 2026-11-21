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

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use swb_css::{ComponentValue, Function, SimpleBlock, trim_whitespace};

use crate::properties::{CssWideKeyword, CustomDeclaration};
use crate::registered::{Registration, Registry};
use crate::values::LengthContext;

/// Custom properties (`--name: value`) of an element, after `var()`
/// substitution. Shared with the parent when unchanged.
///
/// A registered property (`@property`) that is not in the map has its
/// initial value: the map does not copy the initial values of the registry
/// into every element (ADR 0038, section "7b"). A registered property with
/// an initial value that is invalid on an element (a failed `var()` in a
/// universal syntax) is listed in `invalid` (sorted; empty for almost all
/// elements, so that the map stays as cheap to copy as a plain map).
///
/// Two maps that differ compare cheaply: the entries have a fingerprint
/// that `insert` and `invalidate` keep up to date (the wrapping sum of a
/// hash of each entry, so that it does not depend on the order of the
/// insertions). Equal maps are shared across restyles ([`CustomPool`]), so
/// that `Arc` pointer equality decides most comparisons (ADR 0007, "Sharing
/// of custom-property maps").
#[derive(Clone, Debug, Default)]
pub struct CustomProperties {
    entries: HashMap<Arc<str>, Arc<[ComponentValue]>>,
    /// The wrapping sum of [`entry_hash`] of the entries.
    fingerprint: u64,
    invalid: Vec<Arc<str>>,
    registry: Option<Arc<Registry>>,
    /// True if the map may hold a registered property that does not
    /// inherit: the children must reset it.
    resettable: bool,
}

impl PartialEq for CustomProperties {
    fn eq(&self, other: &Self) -> bool {
        let same_registry = match (&self.registry, &other.registry) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same_registry
            && self.fingerprint == other.fingerprint
            && self.entries.len() == other.entries.len()
            && self.resettable == other.resettable
            && self.invalid == other.invalid
            && (self.same_entries_in_order(other) || self.same_entries_by_lookup(other))
    }
}

// The values are `ComponentValue`s, whose only non-reflexive case is a NaN
// number, which the tokenizer never produces.
impl Eq for CustomProperties {}

/// The most component values (nested ones counted) that the hash of one
/// value reads, and the most bytes of each string. The fingerprint only has
/// to agree with equality (equal values hash alike, as they read the same
/// first values and bytes); maps whose fingerprints are equal are compared
/// entry by entry. Without the bound, every change of a map hashed its whole
/// value: a 1 MB custom property inherited by 20,000 elements with their own
/// custom properties cost 6 s (review of M11 feature 4).
const HASHED_VALUES: usize = 16;
const HASHED_BYTES: usize = 64;

/// Hashes the length and at most the first [`HASHED_BYTES`] bytes of a
/// string.
fn hash_text(s: &str, h: &mut DefaultHasher) {
    s.len().hash(h);
    s.as_bytes()
        .get(..HASHED_BYTES)
        .unwrap_or(s.as_bytes())
        .hash(h);
}

/// Hashes a list of component values consistently with their `PartialEq`
/// (the `-0` of a number hashes as `0`), at most `budget` values in all
/// (nested ones counted). The recursion is bounded by the budget and by
/// [`MAX_NESTING`].
fn hash_values(values: &[ComponentValue], h: &mut DefaultHasher, budget: &mut usize) {
    values.len().hash(h);
    for v in values {
        let Some(left) = budget.checked_sub(1) else {
            return;
        };
        *budget = left;
        std::mem::discriminant(v).hash(h);
        match v {
            ComponentValue::Ident(s)
            | ComponentValue::AtKeyword(s)
            | ComponentValue::String(s)
            | ComponentValue::Url(s) => hash_text(s, h),
            ComponentValue::Hash { value, is_id } => {
                hash_text(value, h);
                is_id.hash(h);
            }
            ComponentValue::Delim(c) => c.hash(h),
            ComponentValue::Number(n) | ComponentValue::Percentage(n) => hash_number(n, h),
            ComponentValue::Dimension { number, unit } => {
                hash_number(number, h);
                hash_text(unit, h);
            }
            ComponentValue::UnicodeRange { start, end } => {
                start.hash(h);
                end.hash(h);
            }
            ComponentValue::Function(f) => {
                hash_text(&f.name, h);
                hash_values(&f.arguments, h, budget);
            }
            ComponentValue::Block(b) => {
                b.kind.hash(h);
                hash_values(&b.contents, h, budget);
            }
            _ => {}
        }
    }
}

fn hash_number(n: &swb_css::Number, h: &mut DefaultHasher) {
    // `+ 0.0` turns -0 into 0: they are equal.
    (n.value + 0.0).to_bits().hash(h);
    (n.value_f64 + 0.0).to_bits().hash(h);
    n.int_value.hash(h);
    n.has_sign.hash(h);
}

/// The hash of one entry of a map, in bounded time.
fn entry_hash(name: &str, value: &[ComponentValue]) -> u64 {
    let mut h = DefaultHasher::new();
    hash_text(name, &mut h);
    let mut budget = HASHED_VALUES;
    hash_values(value, &mut h, &mut budget);
    h.finish()
}

/// The custom-property maps that a style pass may reuse: those of the
/// previous pass and those that this pass has made. [`Self::intern`] returns
/// an existing equal map instead of a new one, so that an element's map is
/// pointer-equal to its old map after a restyle that did not change it, and
/// so are the maps of its descendants (they inherit the same `Arc`).
#[derive(Debug, Default)]
pub(crate) struct CustomPool {
    maps: RefCell<HashMap<u64, Vec<Arc<CustomProperties>>>>,
}

/// The most maps with one fingerprint that the pool keeps: equal
/// fingerprints of different maps are not expected.
const MAX_PER_FINGERPRINT: usize = 8;

impl CustomPool {
    /// Adds a map of the previous pass.
    pub(crate) fn seed(&self, map: &Arc<CustomProperties>) {
        let mut maps = self.maps.borrow_mut();
        let bucket = maps.entry(map.fingerprint).or_default();
        if bucket.len() < MAX_PER_FINGERPRINT && !bucket.iter().any(|m| Arc::ptr_eq(m, map)) {
            bucket.push(Arc::clone(map));
        }
    }

    /// The pooled map that equals `map`, or `map` itself, now in the pool.
    pub(crate) fn intern(&self, map: CustomProperties) -> Arc<CustomProperties> {
        let mut maps = self.maps.borrow_mut();
        let bucket = maps.entry(map.fingerprint).or_default();
        if let Some(same) = bucket.iter().find(|m| ***m == map) {
            return Arc::clone(same);
        }
        let map = Arc::new(map);
        if bucket.len() < MAX_PER_FINGERPRINT {
            bucket.push(Arc::clone(&map));
        }
        map
    }
}

impl CustomProperties {
    /// An empty map; the registered properties of `registry` have their
    /// initial values in it.
    pub(crate) fn new(registry: Option<&Arc<Registry>>) -> CustomProperties {
        CustomProperties {
            entries: HashMap::new(),
            fingerprint: 0,
            invalid: Vec::new(),
            registry: registry.cloned(),
            resettable: false,
        }
    }

    /// The value of the custom property `name`; `None` if it is not set
    /// or invalid (the guaranteed-invalid value).
    pub fn get(&self, name: &str) -> Option<&Arc<[ComponentValue]>> {
        match self.entries.get(name) {
            Some(value) => Some(value),
            None => self
                .registry
                .as_ref()
                .filter(|_| !self.is_invalid(name))
                .and_then(|r| r.initial(name)),
        }
    }

    fn is_invalid(&self, name: &str) -> bool {
        !self.invalid.is_empty() && self.invalid.binary_search_by(|n| (**n).cmp(name)).is_ok()
    }

    /// The registration of the custom property `name` (`@property`), if
    /// any.
    pub(crate) fn registration(&self, name: &str) -> Option<&Registration> {
        self.registry.as_ref().and_then(|r| r.get(name))
    }

    /// The names of the custom properties that have a value, in no order.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.entries.keys().map(|k| &**k).collect();
        if let Some(registry) = &self.registry {
            names.extend(
                registry
                    .initial_names()
                    .filter(|n| !self.entries.contains_key(&***n) && !self.is_invalid(n))
                    .map(|n| &**n),
            );
        }
        names
    }

    /// The number of custom properties that have a value.
    pub fn len(&self) -> usize {
        self.names().len()
    }

    /// True if no custom property has a value.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True if the element has no custom properties at all: no entry and
    /// no registry.
    fn is_blank(&self) -> bool {
        self.entries.is_empty() && self.registry.is_none()
    }

    fn insert(&mut self, name: &Arc<str>, value: Arc<[ComponentValue]>) {
        if self
            .registry
            .as_ref()
            .and_then(|r| r.get(name))
            .is_some_and(|r| !r.inherits)
        {
            self.resettable = true;
        }
        if !self.invalid.is_empty() {
            self.invalid.retain(|n| n != name);
        }
        self.fingerprint = self.fingerprint.wrapping_add(entry_hash(name, &value));
        if let Some(old) = self.entries.insert(Arc::clone(name), value) {
            self.fingerprint = self.fingerprint.wrapping_sub(entry_hash(name, &old));
        }
    }

    /// Makes `name` invalid (the guaranteed-invalid value).
    fn invalidate(&mut self, name: &Arc<str>) {
        if let Some(old) = self.entries.remove(&**name) {
            self.fingerprint = self.fingerprint.wrapping_sub(entry_hash(name, &old));
        }
        let Some(registration) = self.registry.as_ref().and_then(|r| r.get(name)) else {
            return;
        };
        if registration.initial.is_some() {
            if let Err(at) = self.invalid.binary_search_by(|n| (**n).cmp(&**name)) {
                self.invalid.insert(at, Arc::clone(name));
            }
            // The children reset it to the initial value if it does not
            // inherit.
            self.resettable |= !registration.inherits;
        }
    }

    /// True if both maps list equal entries in the same iteration order
    /// (given equal lengths, that makes them equal). Maps that are copies
    /// of one map, or that were built by the same steps, have the same
    /// order, and this is a pass over both tables without a lookup.
    fn same_entries_in_order(&self, other: &CustomProperties) -> bool {
        self.entries
            .iter()
            .zip(&other.entries)
            .all(|((ka, a), (kb, b))| {
                (Arc::ptr_eq(ka, kb) || ka == kb) && (Arc::ptr_eq(a, b) || a == b)
            })
    }

    /// True if every entry of this map is in `other` with an equal value
    /// (given equal lengths, that makes them equal).
    fn same_entries_by_lookup(&self, other: &CustomProperties) -> bool {
        self.entries.iter().all(|(name, a)| {
            other
                .entries
                .get(name)
                .is_some_and(|b| Arc::ptr_eq(a, b) || a == b)
        })
    }

    /// True if this map, made from `parent`'s by the `declared` custom
    /// properties, equals `parent`'s: an element that redeclares what it
    /// inherits shares its parent's map. Only the declared names can differ
    /// in entries (and the reset of non-inherited properties only removes
    /// entries, which shows in the length), so this costs in proportion to
    /// `declared`.
    fn same_as_parent(
        &self,
        parent: &CustomProperties,
        declared: &[(&Arc<str>, &CustomDeclaration)],
    ) -> bool {
        self.fingerprint == parent.fingerprint
            && self.entries.len() == parent.entries.len()
            && self.resettable == parent.resettable
            && self.invalid == parent.invalid
            && declared.iter().all(|(name, _)| {
                match (self.entries.get(&***name), parent.entries.get(&***name)) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
                    (None, None) => true,
                    _ => false,
                }
            })
    }

    /// Removes the entries of registered properties that do not inherit:
    /// they have their initial values again. Costs in proportion to the
    /// entries, not to the registrations.
    fn reset_non_inherited(&mut self) {
        if let Some(registry) = &self.registry {
            let fingerprint = &mut self.fingerprint;
            self.entries.retain(|name, value| {
                let keep = registry.get(name).is_none_or(|r| r.inherits);
                if !keep {
                    *fingerprint = fingerprint.wrapping_sub(entry_hash(name, value));
                }
                keep
            });
            self.invalid
                .retain(|name| registry.get(name).is_none_or(|r| r.inherits));
        }
        self.resettable = false;
    }
}

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

/// The registered properties and the length context of the element for
/// the computed values of registered properties (ADR 0038, section "7b").
#[derive(Clone, Copy)]
pub(crate) struct Registered<'a> {
    pub(crate) registry: &'a Arc<Registry>,
    pub(crate) lengths: &'a LengthContext,
    /// The registered properties in a dependency cycle through
    /// font-relative units: they have their initial values
    /// ([`crate::font_cycle`]).
    pub(crate) font_cycle: &'a [Arc<str>],
}

/// The map of an element while its declarations are applied: the parent's
/// map until a declaration changes something, then a copy. Most elements
/// that declare custom properties re-declare what they inherit (a rule
/// like `*, ::before, ::after { --x: 0 }`), and then the copy of a large
/// map is the cost of the style pass.
struct Working<'a> {
    map: Cow<'a, CustomProperties>,
}

impl Working<'_> {
    /// Sets `name` to `value`; no copy if the map has that value already.
    fn store(&mut self, name: &Arc<str>, value: Arc<[ComponentValue]>) {
        let same = self
            .map
            .entries
            .get(&**name)
            .is_some_and(|old| Arc::ptr_eq(old, &value) || **old == *value);
        if !same {
            self.map.to_mut().insert(name, value);
        }
    }

    /// Makes `name` invalid; no copy if that changes nothing.
    fn forget(&mut self, name: &Arc<str>) {
        let changes = self.map.entries.contains_key(&**name)
            || self
                .map
                .registry
                .as_ref()
                .and_then(|r| r.get(name))
                .is_some_and(|r| r.initial.is_some());
        if changes {
            self.map.to_mut().invalidate(name);
        }
    }

    /// `store` of `value`, or `forget` for `None`.
    fn set(&mut self, name: &Arc<str>, value: Option<Arc<[ComponentValue]>>) {
        match value {
            Some(value) => self.store(name, value),
            None => self.forget(name),
        }
    }
}

/// Resolves the custom properties declared on one element.
struct Resolver<'a> {
    registered: Option<Registered<'a>>,
    working: Working<'a>,
    /// The declared values with `var()` that are not resolved yet.
    raw: HashMap<Arc<str>, Arc<[ComponentValue]>>,
    states: HashMap<Arc<str>, State>,
    stack: Vec<Arc<str>>,
    cyclic: Vec<Arc<str>>,
    budget: usize,
}

impl VarLookup for Resolver<'_> {
    fn lookup(&mut self, name: &str) -> Option<Arc<[ComponentValue]>> {
        self.resolve(name)
    }
}

impl Resolver<'_> {
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
            Some(State::Done) | None => return self.working.map.get(name).cloned(),
        }
        let (key, raw) = self
            .raw
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
        let registration = self.registered.and_then(|r| r.registry.get(&key));
        let value = match result {
            Some(value) if !self.cyclic.contains(&key) => match (registration, self.registered) {
                // A registered property: computed against its syntax; a
                // value that does not match is the initial value.
                (Some(reg), Some(r)) => reg.compute(&value, r.lengths),
                _ => Some(Arc::from(trim_whitespace(&value))),
            },
            // Invalid at computed-value time, or in a cycle.
            // (A universal syntax takes the guaranteed-invalid value.)
            _ => registration
                .filter(|r| !r.is_universal())
                .and_then(|r| r.initial.clone()),
        };
        self.working.set(&key, value.clone());
        value
    }
}

/// Computes the custom properties of an element from its parent's and the
/// winning custom declarations (`declared`, at most one per name).
///
/// Returns the parent's map when the declarations do not change it, and
/// otherwise a map equal to one of `pool` if there is one (ADR 0007,
/// "Sharing of custom-property maps").
pub(crate) fn compute_custom_properties(
    parent: Option<&Arc<CustomProperties>>,
    declared: &[(&Arc<str>, &CustomDeclaration)],
    registered: Option<Registered<'_>>,
    pool: &CustomPool,
) -> Option<Arc<CustomProperties>> {
    let registry = registered.map(|r| r.registry);
    // A registered property that does not inherit is reset to its initial
    // value on every element.
    let reset = parent.is_some_and(|p| p.resettable);
    if declared.is_empty() && !reset && (parent.is_some() || registry.is_none()) {
        return parent.cloned();
    }
    let mut working = Working {
        map: match parent {
            Some(p) => Cow::Borrowed(&**p),
            None => Cow::Owned(CustomProperties::new(registry)),
        },
    };
    if reset {
        working.map.to_mut().reset_non_inherited();
    }
    let mut raw = HashMap::new();
    let mut states = HashMap::new();
    for &(name, decl) in declared {
        let registration = registry.and_then(|reg| reg.get(name));
        let initial = || registration.and_then(|r| r.initial.clone());
        if registered.is_some_and(|r| r.font_cycle.contains(name)) {
            working.set(name, initial());
            continue;
        }
        match decl {
            CustomDeclaration::Value(value) => {
                if crate::properties::has_references(value) {
                    raw.insert(Arc::clone(name), Arc::clone(value));
                    states.insert(Arc::clone(name), State::Unresolved);
                } else if let (Some(reg), Some(r)) = (registration, registered) {
                    working.set(name, reg.compute(value, r.lengths));
                } else {
                    working.store(name, Arc::clone(value));
                }
            }
            CustomDeclaration::CssWide(CssWideKeyword::Initial) => working.set(name, initial()),
            // `unset` and `revert` are `initial` for a registered property
            // that does not inherit.
            CustomDeclaration::CssWide(
                CssWideKeyword::Unset | CssWideKeyword::Revert | CssWideKeyword::RevertLayer,
            ) if registration.is_some_and(|r| !r.inherits) => working.set(name, initial()),
            // Custom properties are inherited: `inherit` and `unset` keep
            // the parent's value. `revert` does too: no user-agent rule
            // sets custom properties. So does `revert-layer` when no
            // layer below has a value (the cascade drops the others).
            CustomDeclaration::CssWide(
                CssWideKeyword::Inherit
                | CssWideKeyword::Unset
                | CssWideKeyword::Revert
                | CssWideKeyword::RevertLayer,
            ) => working.set(name, parent.and_then(|p| p.get(name)).cloned()),
        }
    }
    if !states.is_empty() {
        let names: Vec<Arc<str>> = states.keys().cloned().collect();
        let mut resolver = Resolver {
            registered,
            working,
            raw,
            states,
            stack: Vec::new(),
            cyclic: Vec::new(),
            budget: SUBSTITUTION_BUDGET,
        };
        for name in names {
            resolver.resolve(&name);
        }
        working = resolver.working;
    }
    let map = match working.map {
        Cow::Borrowed(_) => return parent.cloned(),
        Cow::Owned(map) => map,
    };
    if map.is_blank() {
        return None;
    }
    if let Some(p) = parent
        && map.same_as_parent(p, declared)
    {
        return Some(Arc::clone(p));
    }
    Some(pool.intern(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;
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
        compute_custom_properties(parent, &refs, None, &CustomPool::default())
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

    /// A registry of `count` non-inheriting properties `--r0`, `--r1`, ...
    /// with initial values, plus `--u` (universal, with an initial value,
    /// inherits).
    fn registry_of(count: usize) -> Arc<Registry> {
        let mut css = String::new();
        for i in 0..count {
            let _ = write!(
                css,
                "@property --r{i} {{ syntax: '<length>'; inherits: false; initial-value: {i}px }}"
            );
        }
        css.push_str("@property --u { syntax: '*'; inherits: true; initial-value: init }");
        css.push_str("@property --n { syntax: '*'; inherits: false; initial-value: ni }");
        let sheet = swb_css::parse_stylesheet(&css);
        let regs: Vec<_> = sheet
            .rules
            .iter()
            .filter_map(|r| match r {
                swb_css::CssRule::Property(rule) => Registration::from_rule(rule),
                _ => None,
            })
            .collect();
        Arc::new(Registry::new(regs.iter()))
    }

    fn compute_in(
        registry: &Arc<Registry>,
        parent: Option<&Arc<CustomProperties>>,
        pairs: &[(&str, &str)],
    ) -> Option<Arc<CustomProperties>> {
        let d = decls(pairs);
        let refs: Vec<(&Arc<str>, &CustomDeclaration)> = d.iter().map(|(n, v)| (n, v)).collect();
        let lengths = LengthContext::DEFAULT;
        let registered = Registered {
            registry,
            lengths: &lengths,
            font_cycle: &[],
        };
        compute_custom_properties(parent, &refs, Some(registered), &CustomPool::default())
    }

    #[test]
    fn registered_initial_values_are_not_copied_into_the_maps() {
        let registry = registry_of(500);
        let root = compute_in(&registry, None, &[]);
        let root_map = root.as_ref().expect("map");
        assert!(root_map.entries.is_empty());
        assert_eq!(get(root.as_ref(), "--r7").as_deref(), Some("7px"));
        assert_eq!(root_map.len(), 502);
        // An element that declares one property holds one entry, and a
        // child that declares another holds two.
        let a = compute_in(&registry, root.as_ref(), &[("--r1", "9px")]);
        assert_eq!(a.as_ref().expect("map").entries.len(), 1);
        assert_eq!(get(a.as_ref(), "--r1").as_deref(), Some("9px"));
        assert_eq!(get(a.as_ref(), "--r2").as_deref(), Some("2px"));
        let b = compute_in(&registry, a.as_ref(), &[("--x", "1")]);
        // `--r1` does not inherit: it is the initial value again.
        assert_eq!(get(b.as_ref(), "--r1").as_deref(), Some("1px"));
        assert_eq!(b.as_ref().expect("map").entries.len(), 1);
        assert_eq!(get(b.as_ref(), "--x").as_deref(), Some("1"));
        // An element without declarations resets too, and then shares.
        let c = compute_in(&registry, a.as_ref(), &[]);
        assert_eq!(get(c.as_ref(), "--r1").as_deref(), Some("1px"));
        let d = compute_in(&registry, c.as_ref(), &[]);
        assert!(Arc::ptr_eq(
            c.as_ref().expect("map"),
            d.as_ref().expect("map")
        ));
    }

    #[test]
    fn an_invalid_registered_value_with_an_initial_value_stays_invalid() {
        let registry = registry_of(1);
        let root = compute_in(&registry, None, &[]);
        // `--u` has the universal syntax: a failed `var()` is the
        // guaranteed-invalid value, not the initial value.
        let broken = compute_in(&registry, root.as_ref(), &[("--u", "var(--nope)")]);
        assert_eq!(get(broken.as_ref(), "--u"), None);
        assert!(!broken.as_ref().expect("map").names().contains(&"--u"));
        let child = compute_in(&registry, broken.as_ref(), &[]);
        assert_eq!(get(child.as_ref(), "--u"), None);
        let reset = compute_in(&registry, child.as_ref(), &[("--u", "initial")]);
        assert_eq!(get(reset.as_ref(), "--u").as_deref(), Some("init"));
        // An invalid property that does not inherit is reset in the child.
        let gone = compute_in(&registry, root.as_ref(), &[("--n", "var(--nope)")]);
        assert_eq!(get(gone.as_ref(), "--n"), None);
        let after = compute_in(&registry, gone.as_ref(), &[]);
        assert_eq!(get(after.as_ref(), "--n").as_deref(), Some("ni"));
        // Invalid, and then declared again: no longer invalid.
        let again = compute_in(&registry, broken.as_ref(), &[("--u", "1")]);
        assert_eq!(get(again.as_ref(), "--u").as_deref(), Some("1"));
    }

    /// The fingerprint that a map must have: the sum over its entries.
    fn expected_fingerprint(map: &CustomProperties) -> u64 {
        map.entries
            .iter()
            .fold(0u64, |sum, (n, v)| sum.wrapping_add(entry_hash(n, v)))
    }

    fn map_of(pairs: &[(&str, &str)]) -> Arc<CustomProperties> {
        compute(None, pairs).expect("map")
    }

    #[test]
    fn equal_maps_with_different_insertion_orders_are_equal() {
        let a = map_of(&[("--a", "1px"), ("--b", "red"), ("--c", "calc(1px + 2px)")]);
        let b = map_of(&[("--c", "calc(1px + 2px)"), ("--a", "1px"), ("--b", "red")]);
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a, b);
        // A value that was replaced, and a property that was added and
        // removed again, leave no trace.
        let c = compute(
            Some(&map_of(&[("--a", "9px"), ("--x", "1")])),
            &[
                ("--a", "1px"),
                ("--x", "initial"),
                ("--b", "red"),
                ("--c", "calc(1px + 2px)"),
            ],
        )
        .expect("map");
        assert_eq!(c.fingerprint, a.fingerprint);
        assert_eq!(*c, *a);
        assert_eq!(c.fingerprint, expected_fingerprint(&c));
    }

    #[test]
    fn maps_that_differ_are_not_equal() {
        let base = map_of(&[("--a", "1px"), ("--b", "red")]);
        let value = map_of(&[("--a", "2px"), ("--b", "red")]);
        let key = map_of(&[("--a", "1px"), ("--c", "red")]);
        let longer = map_of(&[("--a", "1px"), ("--b", "red"), ("--c", "red")]);
        // The same tokens with a different structure.
        let nested = map_of(&[("--a", "1px"), ("--b", "(red)")]);
        for other in [&value, &key, &longer, &nested] {
            assert_ne!(*base, **other);
            assert_ne!(base.fingerprint, other.fingerprint);
        }
        // The fingerprint says nothing when it collides: a forced collision
        // is still unequal.
        let mut forged = (*value).clone();
        forged.fingerprint = base.fingerprint;
        assert_ne!(*base, forged);
    }

    #[test]
    fn long_values_hash_in_bounded_time_and_still_compare_exactly() {
        // Two values that differ only after the values and bytes that the
        // hash reads: the same fingerprint, yet unequal maps.
        let tokens = |last: &str| {
            let mut text = (0..HASHED_VALUES * 4)
                .map(|i| format!("a{i}"))
                .collect::<Vec<_>>()
                .join(" ");
            text.push(' ');
            text.push_str(last);
            text
        };
        let (first, second) = (tokens("x"), tokens("y"));
        let a = map_of(&[("--v", first.as_str())]);
        let b = map_of(&[("--v", second.as_str())]);
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_ne!(*a, *b);
        // Long strings: only their first bytes are hashed.
        let long = |end: char| format!("\"{}{end}\"", "s".repeat(HASHED_BYTES * 4));
        let c = map_of(&[("--s", long('x').as_str())]);
        let d = map_of(&[("--s", long('y').as_str())]);
        assert_eq!(c.fingerprint, d.fingerprint);
        assert_ne!(*c, *d);
        assert_eq!(*c, *map_of(&[("--s", long('x').as_str())]));
    }

    #[test]
    fn number_spellings_are_different_values() {
        // Tokens that differ in the integer flag, the sign or the unit are
        // different values (the serialization differs), and the equal ones
        // are equal.
        let zero = map_of(&[("--n", "0")]);
        for other in ["-0", "0.0", "+0", "0px", "0%", "1e1"] {
            let map = map_of(&[("--n", other)]);
            assert_ne!(*zero, *map, "{other}");
        }
        assert_eq!(*zero, *map_of(&[("--n", " 0 ")]));
    }

    #[test]
    fn the_invalid_list_and_the_registry_are_compared() {
        let registry = registry_of(1);
        let root = compute_in(&registry, None, &[]);
        let broken = compute_in(&registry, root.as_ref(), &[("--u", "var(--nope)")]);
        let fine = compute_in(&registry, root.as_ref(), &[]);
        let (broken, fine) = (broken.expect("map"), fine.expect("map"));
        // Same entries (none), same fingerprint, but `--u` is invalid in one.
        assert_eq!(broken.fingerprint, fine.fingerprint);
        assert_ne!(*broken, *fine);
        // The same entries with another registry (an `Arc` of its own).
        let other_registry = registry_of(1);
        let other = compute_in(&other_registry, None, &[]).expect("map");
        assert_eq!(other.fingerprint, fine.fingerprint);
        assert_ne!(*other, *fine);
        // A plain map and a map with a registry.
        assert_ne!(CustomProperties::default(), *fine);
    }

    #[test]
    fn the_fingerprint_follows_every_change() {
        let registry = registry_of(3);
        let root = compute_in(&registry, None, &[("--x", "1"), ("--r1", "4px")]);
        let child = compute_in(
            &registry,
            root.as_ref(),
            &[
                ("--x", "var(--y, 2)"),
                ("--z", "3"),
                ("--r2", "initial"),
                ("--u", "var(--nope)"),
            ],
        )
        .expect("map");
        assert_eq!(child.fingerprint, expected_fingerprint(&child));
        let grandchild = compute_in(&registry, Some(&child), &[("--z", "initial")]).expect("map");
        assert_eq!(grandchild.fingerprint, expected_fingerprint(&grandchild));
    }

    #[test]
    fn a_pool_returns_the_equal_map_it_holds() {
        let pool = CustomPool::default();
        let old = map_of(&[("--a", "1px"), ("--b", "red")]);
        pool.seed(&old);
        pool.seed(&old);
        let same = pool.intern(CustomProperties::clone(&map_of(&[
            ("--b", "red"),
            ("--a", "1px"),
        ])));
        assert!(Arc::ptr_eq(&same, &old));
        let other = pool.intern(CustomProperties::clone(&map_of(&[("--a", "2px")])));
        assert!(!Arc::ptr_eq(&other, &old));
        let again = pool.intern(CustomProperties::clone(&map_of(&[("--a", "2px")])));
        assert!(Arc::ptr_eq(&again, &other), "maps of this pass are pooled");
    }

    #[test]
    fn declarations_that_change_nothing_share_the_parent_map() {
        let parent = compute(None, &[("--a", "1px"), ("--b", "var(--a)")]);
        let p = parent.as_ref().expect("map");
        // The same values again, also through `var()`, and `inherit`.
        let same = compute(
            parent.as_ref(),
            &[("--a", "1px"), ("--b", "var(--a)"), ("--c", "initial")],
        );
        assert!(Arc::ptr_eq(same.as_ref().expect("map"), p));
        let same = compute(parent.as_ref(), &[("--a", "inherit")]);
        assert!(Arc::ptr_eq(same.as_ref().expect("map"), p));
        // A real change is a new map, and the parent keeps its value.
        let changed = compute(parent.as_ref(), &[("--a", "2px"), ("--b", "var(--a)")]);
        assert!(!Arc::ptr_eq(changed.as_ref().expect("map"), p));
        assert_eq!(get(changed.as_ref(), "--b").as_deref(), Some("2px"));
        assert_eq!(get(parent.as_ref(), "--a").as_deref(), Some("1px"));
        // An invalid `var()` removes the inherited value.
        let removed = compute(parent.as_ref(), &[("--a", "var(--nope)")]);
        assert_eq!(get(removed.as_ref(), "--a"), None);
        assert_eq!(get(parent.as_ref(), "--a").as_deref(), Some("1px"));
    }

    #[test]
    fn a_map_made_again_is_the_pooled_one() {
        let pool = CustomPool::default();
        let build = |pool: &CustomPool| {
            let d = decls(&[("--a", "1px"), ("--b", "var(--a) solid")]);
            let refs: Vec<(&Arc<str>, &CustomDeclaration)> =
                d.iter().map(|(n, v)| (n, v)).collect();
            compute_custom_properties(None, &refs, None, pool).expect("map")
        };
        let first = build(&pool);
        let second = build(&pool);
        assert!(Arc::ptr_eq(&first, &second));
    }
}
