//! Roots of the collector (ADR 0026 section 8): the interface through
//! which the VM reports its roots, persistent roots, and handle scopes.
//!
//! - The VM (value stack, frames, realms, job queue) implements
//!   [`RootSource`] and passes it to every call that can collect.
//! - Persistent roots keep a value alive until the host releases the
//!   token.
//! - Handle scopes: every handle that the native API gives a native
//!   function is recorded in the current scope ([`Heap::record`]). The
//!   scope ends with the native function; scopes nest; a value can be
//!   passed out of an inner scope into the outer one.

use crate::error::{Error, Result};
use crate::heap::{Gc, Generic, Heap, Tracer};
use crate::string::PropertyKey;
use crate::value::{Value, ValueCell};

/// A root of any kind that native code can hold: a value (which covers
/// strings, symbols, `BigInt`s and objects), a cell, generic data, or a
/// property key. [`Heap::record`] and [`Heap::persist`] accept everything
/// that converts into a `Root`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Root {
    /// A value.
    Value(Value),
    /// A cell.
    Cell(Gc<ValueCell>),
    /// A thing of the generic kind.
    Generic(Gc<Generic>),
    /// A property key (an index key holds nothing).
    Key(PropertyKey),
}

impl Root {
    fn trace(self, tracer: &mut Tracer<'_>) {
        match self {
            Root::Value(value) => tracer.value(value),
            Root::Cell(cell) => tracer.cell(cell),
            Root::Generic(generic) => tracer.generic(generic),
            Root::Key(key) => tracer.key(key),
        }
    }
}

impl<T: Into<Value>> From<T> for Root {
    fn from(value: T) -> Self {
        Root::Value(value.into())
    }
}

impl From<Gc<ValueCell>> for Root {
    fn from(cell: Gc<ValueCell>) -> Self {
        Root::Cell(cell)
    }
}

impl From<Gc<Generic>> for Root {
    fn from(generic: Gc<Generic>) -> Self {
        Root::Generic(generic)
    }
}

impl From<PropertyKey> for Root {
    fn from(key: PropertyKey) -> Self {
        Root::Key(key)
    }
}

/// A set of roots outside the heap, which a collection traces.
pub trait RootSource {
    /// Reports every root handle to the tracer.
    fn trace_roots(&self, tracer: &mut Tracer<'_>);
}

/// No roots outside the heap.
pub struct NoRoots;

impl RootSource for NoRoots {
    fn trace_roots(&self, _: &mut Tracer<'_>) {}
}

impl RootSource for Vec<Value> {
    fn trace_roots(&self, tracer: &mut Tracer<'_>) {
        for value in self {
            tracer.value(*value);
        }
    }
}

/// A token for a persistent root. Release it with [`Heap::release`];
/// until then the value stays alive.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a persistent root keeps its value alive until it is released"]
pub struct Persistent {
    index: u32,
    generation: u32,
}

/// A token for an open handle scope. Close it with [`Heap::close_scope`]
/// or [`Heap::close_scope_with`], innermost first.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a handle scope must be closed"]
pub struct HandleScope {
    /// The number of open scopes including this one.
    depth: usize,
    /// A number unique to this scope, so that the token of a scope that
    /// [`Heap::close_scopes_to`] closed cannot close a later scope at the
    /// same depth.
    id: u64,
}

struct PersistentSlot {
    generation: u32,
    value: Option<Root>,
}

/// The roots that the heap itself holds.
#[derive(Default)]
pub(crate) struct HeapRoots {
    /// Values recorded in handle scopes, innermost scope last.
    scope_values: Vec<Root>,
    /// The start of each open scope in `scope_values`.
    scope_starts: Vec<usize>,
    /// The id of each open scope ([`HandleScope::id`]).
    scope_ids: Vec<u64>,
    /// The id of the next scope.
    next_scope_id: u64,
    persistent: Vec<PersistentSlot>,
    free_persistent: Vec<u32>,
}

impl HeapRoots {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for root in &self.scope_values {
            root.trace(tracer);
        }
        for slot in &self.persistent {
            if let Some(root) = slot.value {
                root.trace(tracer);
            }
        }
    }

    pub(crate) fn bytes(&self) -> usize {
        self.scope_values.capacity() * size_of::<Root>()
            + self.persistent.capacity() * size_of::<PersistentSlot>()
    }
}

impl Heap {
    /// Opens a handle scope inside the current one.
    pub fn open_scope(&mut self) -> HandleScope {
        let roots = &mut self.roots;
        roots.scope_starts.push(roots.scope_values.len());
        let id = roots.next_scope_id;
        roots.next_scope_id += 1;
        roots.scope_ids.push(id);
        HandleScope {
            depth: roots.scope_starts.len(),
            id,
        }
    }

    /// Closes the innermost handle scope; its values are no longer roots.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the token is consumed, so that it cannot close a scope twice"
    )]
    pub fn close_scope(&mut self, scope: HandleScope) -> Result<()> {
        let HandleScope { depth, id } = scope;
        let roots = &mut self.roots;
        if depth != roots.scope_starts.len() || roots.scope_ids.last() != Some(&id) {
            return Err(Error::invariant("handle scopes closed out of order"));
        }
        roots.scope_ids.pop();
        let start = roots
            .scope_starts
            .pop()
            .ok_or(Error::invariant("no handle scope is open"))?;
        roots.scope_values.truncate(start);
        Ok(())
    }

    /// Closes every handle scope above `depth` (a value of
    /// [`Heap::scope_depth`] read before the scopes were opened). An
    /// exception unwinder calls it to repair the scopes that an early
    /// return skipped; tokens of the closed scopes are dead afterwards
    /// (closing one is an internal error).
    pub fn close_scopes_to(&mut self, depth: usize) {
        let roots = &mut self.roots;
        if let Some(&start) = roots.scope_starts.get(depth) {
            roots.scope_values.truncate(start);
            roots.scope_starts.truncate(depth);
            roots.scope_ids.truncate(depth);
        }
    }

    /// Closes the innermost handle scope and records `value` in the outer
    /// scope (a value passed out of the scope). If no outer scope is open,
    /// the value is returned unrooted: the caller roots it.
    pub fn close_scope_with(&mut self, scope: HandleScope, value: Value) -> Result<Value> {
        self.close_scope(scope)?;
        if self.roots.scope_starts.is_empty() {
            return Ok(value);
        }
        Ok(self.record(value))
    }

    /// Records a handle in the current handle scope, so that it stays
    /// alive until the scope closes. Returns the handle. Accepts values,
    /// every `Gc` kind that a value can hold, cells, generic data and
    /// property keys (see [`Root`]).
    ///
    /// Without an open scope this is a bug of the caller: it fails a
    /// `debug_assert!`, and in a release build the handle stays alive as
    /// long as the heap (a leak, never a free).
    #[inline]
    pub fn record<R: Into<Root> + Copy>(&mut self, root: R) -> R {
        debug_assert!(
            !self.roots.scope_starts.is_empty(),
            "Heap::record without an open handle scope"
        );
        self.roots.scope_values.push(root.into());
        root
    }

    /// Records the keys of a list (for example the result of
    /// `own_property_keys`) in the current handle scope.
    pub fn record_keys(&mut self, keys: &[PropertyKey]) {
        debug_assert!(
            keys.is_empty() || !self.roots.scope_starts.is_empty(),
            "Heap::record_keys without an open handle scope"
        );
        self.roots
            .scope_values
            .extend(keys.iter().map(|&key| Root::Key(key)));
    }

    /// [`Heap::reserve`] with `extra` values rooted during the call: the
    /// internal methods protect their own arguments at their collection
    /// points.
    pub(crate) fn reserve_with(
        &mut self,
        bytes: usize,
        roots: &dyn RootSource,
        extra: &[Value],
    ) -> Result<()> {
        let mark = self.roots.scope_values.len();
        self.roots
            .scope_values
            .extend(extra.iter().map(|&value| Root::Value(value)));
        let result = self.reserve(bytes, roots);
        self.roots.scope_values.truncate(mark);
        result
    }

    /// The number of open handle scopes.
    pub fn scope_depth(&self) -> usize {
        self.roots.scope_starts.len()
    }

    /// The number of values recorded in all open scopes.
    pub fn scope_len(&self) -> usize {
        self.roots.scope_values.len()
    }

    /// Makes a handle (see [`Root`]) a root until the token is released.
    /// Pass `handle.into()`.
    pub fn persist(&mut self, value: Root) -> Persistent {
        let roots = &mut self.roots;
        if let Some(index) = roots.free_persistent.pop()
            && let Some(slot) = roots.persistent.get_mut(index as usize)
        {
            slot.value = Some(value);
            return Persistent {
                index,
                generation: slot.generation,
            };
        }
        let index = roots.persistent.len() as u32;
        roots.persistent.push(PersistentSlot {
            generation: 0,
            value: Some(value),
        });
        Persistent {
            index,
            generation: 0,
        }
    }

    fn persistent_slot(&mut self, root: &Persistent) -> Result<&mut PersistentSlot> {
        match self.roots.persistent.get_mut(root.index as usize) {
            Some(slot) if slot.generation == root.generation && slot.value.is_some() => Ok(slot),
            _ => Err(Error::invariant("a released persistent root")),
        }
    }

    /// The root of a persistent root.
    pub fn persistent_root(&mut self, root: &Persistent) -> Result<Root> {
        self.persistent_slot(root)?
            .value
            .ok_or(Error::invariant("a released persistent root"))
    }

    /// The value of a persistent root that holds a value.
    pub fn persistent_value(&mut self, root: &Persistent) -> Result<Value> {
        match self.persistent_root(root)? {
            Root::Value(value) => Ok(value),
            _ => Err(Error::invariant("a persistent root that holds no value")),
        }
    }

    /// Replaces the handle of a persistent root.
    pub fn set_persistent(&mut self, root: &Persistent, value: Root) -> Result<()> {
        self.persistent_slot(root)?.value = Some(value);
        Ok(())
    }

    /// Releases a persistent root.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the token is consumed, so that it cannot be released twice"
    )]
    pub fn release(&mut self, root: Persistent) -> Result<()> {
        let slot = self.persistent_slot(&root)?;
        let Persistent { index, .. } = root;
        slot.value = None;
        slot.generation = slot.generation.wrapping_add(1);
        self.roots.free_persistent.push(index);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::HeapConfig;

    #[test]
    fn scopes_keep_values_until_closed() {
        let mut heap = Heap::new(HeapConfig::default());
        let outer = heap.open_scope();
        let a = heap.alloc_str("a").unwrap();
        heap.record(a);
        let inner = heap.open_scope();
        let b = heap.alloc_str("b").unwrap();
        heap.record(b);
        let c = heap.alloc_str("c").unwrap();
        heap.record(c);
        heap.collect(&NoRoots);
        assert!(heap.is_live_string(b));
        // Pass `c` out of the inner scope.
        heap.close_scope_with(inner, c.into()).unwrap();
        heap.collect(&NoRoots);
        assert!(heap.is_live_string(a));
        assert!(!heap.is_live_string(b));
        assert!(heap.is_live_string(c));
        heap.close_scope(outer).unwrap();
        heap.collect(&NoRoots);
        assert!(!heap.is_live_string(a));
        assert!(!heap.is_live_string(c));
        assert_eq!(heap.scope_depth(), 0);
    }

    #[test]
    fn scopes_close_innermost_first() {
        let mut heap = Heap::new(HeapConfig::default());
        let outer = heap.open_scope();
        let inner = heap.open_scope();
        assert!(heap.close_scope(outer).is_err());
        heap.close_scope(inner).unwrap();
        let again = heap.open_scope();
        heap.close_scope(again).unwrap();
        assert_eq!(heap.scope_depth(), 1);
    }

    #[test]
    fn persistent_roots_until_released() {
        let mut heap = Heap::new(HeapConfig::default());
        let s = heap.alloc_str("kept").unwrap();
        let root = heap.persist(s.into());
        heap.collect(&NoRoots);
        assert!(heap.is_live_string(s));
        assert_eq!(heap.persistent_value(&root), Ok(Value::String(s)));
        let copy = Persistent {
            index: root.index,
            generation: root.generation,
        };
        heap.release(root).unwrap();
        assert!(heap.persistent_value(&copy).is_err());
        assert!(heap.release(copy).is_err());
        heap.collect(&NoRoots);
        assert!(!heap.is_live_string(s));
        // The slot is reused with a new generation.
        let other = heap.persist(Value::Null.into());
        assert_eq!(other.index, 0);
        assert_eq!(other.generation, 1);
    }

    /// Recording without a scope is a caller bug: it fails a debug
    /// assertion. In a release build the handle is kept for the life of
    /// the heap.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "without an open handle scope")]
    fn record_without_a_scope_is_a_bug() {
        let mut heap = Heap::new(HeapConfig::default());
        let s = heap.alloc_str("x").unwrap();
        heap.record(s);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn record_without_a_scope_leaks() {
        let mut heap = Heap::new(HeapConfig::default());
        let s = heap.alloc_str("x").unwrap();
        heap.record(s);
        heap.collect(&NoRoots);
        assert!(heap.is_live_string(s));
    }
}
