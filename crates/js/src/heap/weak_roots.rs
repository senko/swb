//! The kept set of the weak references (ECMA-262 §9.10 to §9.13; ADR
//! 0026 section 8), `[[KeptAlive]]` of the agent, which is a root, and the
//! queue of the finalization registries whose cleanup job the host has not
//! run yet, which is not.
//!
//! - `AddToKeptObjects` (§9.12) adds the target of a new `WeakRef` and of
//!   each `deref`; the set keeps them alive until `ClearKeptObjects`
//!   (§9.11), which the host calls when a synchronous run of script ends
//!   (the end of `Runtime::run_jobs`, as HTML performs it at the end of a
//!   microtask checkpoint). The specification appends to a list; a set
//!   holds each target once, so a loop of `deref` calls does not grow it.
//! - The cleanup queue stands for the jobs of
//!   `HostEnqueueFinalizationRegistryCleanupJob` (§9.5.6): a registry is
//!   queued by the collection that found one of its targets dead, and the
//!   host runs the job later (`Runtime::run_finalization_cleanup`). The
//!   queue holds its registries weakly: a registry that script can no
//!   longer reach leaves the queue at the next collection and calls no
//!   callback (`CleanupFinalizationRegistry` is optional for an
//!   implementation, §9.13; a dead registry has no observer).

use std::collections::{HashSet, VecDeque};

use swb_js_text::hash::BuildKeyedHasher;

use crate::error::Result;
use crate::heap::{Gc, Heap, Tracer};
use crate::object::{Object, WeakKey};

/// The kept set (a root) and the cleanup queue (weak).
#[derive(Default)]
pub(crate) struct WeakRoots {
    kept: HashSet<WeakKey, BuildKeyedHasher>,
    pub(crate) cleanup: VecDeque<Gc<Object>>,
}

impl WeakRoots {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for key in &self.kept {
            key.trace(tracer);
        }
    }

    /// The bytes of the set and the queue.
    pub(crate) fn bytes(&self) -> usize {
        self.kept.capacity() * (size_of::<WeakKey>() + 1)
            + self.cleanup.capacity() * size_of::<Gc<Object>>()
    }
}

impl Heap {
    /// `AddToKeptObjects` (§9.12): keeps `key` alive until
    /// [`Heap::clear_kept_objects`]. Checks the heap limit before the set
    /// grows and charges the growth.
    pub(crate) fn add_to_kept_objects(&mut self, key: WeakKey) -> Result<()> {
        let kept = &mut self.weak_roots.kept;
        if kept.contains(&key) {
            return Ok(());
        }
        let before = kept.capacity();
        if kept.len() == before {
            self.check_room(before.max(4) * (size_of::<WeakKey>() + 1))?;
        }
        let kept = &mut self.weak_roots.kept;
        kept.try_reserve(1)?;
        kept.insert(key);
        let grown = kept.capacity().saturating_sub(before);
        self.charge(grown * (size_of::<WeakKey>() + 1));
        Ok(())
    }

    /// `ClearKeptObjects` (§9.11): the targets of the `WeakRef`s made or
    /// read since the last call can die at the next collection.
    pub fn clear_kept_objects(&mut self) {
        self.weak_roots.kept = HashSet::default();
    }

    /// The number of targets in the kept set.
    pub fn kept_objects(&self) -> usize {
        self.weak_roots.kept.len()
    }
}
