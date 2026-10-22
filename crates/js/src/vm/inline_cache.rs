//! The miss paths of the interpreter's inline caches (ADR 0026 section
//! 15, M7 13a): the access by the general lookup, and the new entry of
//! the site. The entries and their checks are in `object/cache.rs`, the
//! global entries in `global.rs`.
//!
//! A miss walks the chain once: the walk that gives the result also
//! gives the entry, so a site that sees many shapes costs about as much
//! as an access without caches.

use crate::bytecode::FunctionCode;
use crate::heap::Gc;
use crate::object::{GetResult, Object, ObjectKind, SetPlan, Shape, SiteCache};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::property::SetOutcome;
use crate::vm::{VmError, VmResult};

impl Runtime {
    /// Stores `entry` as the cache entry of `site` of `code` (nothing for
    /// `Empty`: the old entry stays, it still hits for its own shape; and
    /// nothing for an entry of an exhausted epoch).
    pub(crate) fn fill_cache(&mut self, code: &FunctionCode, site: u32, entry: SiteCache) {
        if matches!(entry, SiteCache::Empty) || entry.exhausted() {
            return;
        }
        let bytes = code.fill_cache(site, entry);
        self.heap.charge(bytes);
    }

    /// `object.key` (a string or symbol key) after a miss of the entry of
    /// `site`: the heap's `[[Get]]` and the new entry.
    #[inline(never)]
    pub(crate) fn get_named_miss(
        &mut self,
        code: &FunctionCode,
        site: u32,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<GetResult> {
        let (found, entry) = self.heap.get_for_cache(object, key)?;
        self.fill_cache(code, site, entry);
        Ok(found)
    }

    /// `object.key = value` after a miss of the entry of `site`: the plan
    /// of one walk, carried out, and the new entry; the general `PutValue`
    /// for everything that the plan does not cover.
    #[inline(never)]
    pub(crate) fn set_named_miss(
        &mut self,
        code: &FunctionCode,
        site: u32,
        (object, key, value): (Gc<Object>, PropertyKey, Value),
        strict: bool,
    ) -> VmResult<SetOutcome> {
        let epoch = self.heap.watch_epoch();
        let (plan, entry) = self.heap.set_plan(object, key)?;
        // Each arm fills the entry before any script runs (a setter, a
        // proxy trap): a change that such a script makes leaves the entry
        // with an old shape or epoch, so the entry cannot go stale.
        match plan {
            SetPlan::Own(slot) => {
                match self.heap.object_mut(object)?.slots.get_mut(slot as usize) {
                    Some(stored) => *stored = value,
                    None => return Err(VmError::invariant("a shape entry outside the slots")),
                }
                self.fill_cache(code, site, entry);
                Ok(SetOutcome::Done)
            }
            SetPlan::Setter(setter) => {
                self.fill_cache(code, site, entry);
                Ok(SetOutcome::Setter {
                    setter,
                    receiver: object.into(),
                })
            }
            SetPlan::Add => {
                let entry = self.heap.add_entry(object, key, value, epoch, &self.vm)?;
                self.fill_cache(code, site, entry);
                Ok(SetOutcome::Done)
            }
            SetPlan::General => {
                // A proxy receiver: the next write skips the plan.
                self.fill_cache(code, site, entry);
                self.put_value(object.into(), key, value, strict)
            }
        }
    }

    /// The shape of the object literal of `NewLiteral` at `site` after a
    /// miss of its entry: the keys `sites[site..site + count]` from the
    /// root shape of `proto`, and the new entry.
    #[inline(never)]
    pub(crate) fn literal_shape_miss(
        &mut self,
        code: &FunctionCode,
        site: u32,
        count: u16,
        proto: Gc<Object>,
    ) -> VmResult<Gc<Shape>> {
        let start = site as usize;
        let keys = code
            .sites
            .get(start..start + usize::from(count))
            .ok_or(VmError::invariant("literal sites outside the table"))?;
        let shape = self
            .heap
            .literal_shape(proto, keys)?
            .ok_or(VmError::invariant("a literal key that a shape cannot hold"))?;
        self.fill_cache(code, site, SiteCache::Literal { proto, shape });
        Ok(shape)
    }

    /// A new object of an object literal with the shape `shape` of `count`
    /// data properties, all `undefined` until `InitSlot` sets them.
    pub(crate) fn new_literal_object(
        &mut self,
        shape: Gc<Shape>,
        count: u16,
    ) -> VmResult<Gc<Object>> {
        let slots = vec![Value::Undefined; usize::from(count)];
        Ok(self
            .heap
            .new_object_with_shape(shape, slots, Vec::new(), ObjectKind::Ordinary)?)
    }
}
