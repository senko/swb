//! What the registries of live positions in an arena (`NodeIterator`
//! positions, `iterator.rs`; live ranges, `ranges.rs`) share with their host:
//! the work counter that the host charges to the time limit of a task, and the
//! list of ids whose script objects the collector freed (ADR 0028 "Feature
//! 15a", ADR 0039 section 1).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The work units after which the host checks the time limit.
pub const WORK_PER_CHECK: usize = 4096;

/// The ids of entries whose script objects the collector freed. The host
/// pushes; an arena drops the entries of the ids it holds when it next looks,
/// and the host clears the list when it makes a new entry.
#[derive(Debug, Default)]
pub struct DeadIds {
    ids: RefCell<Vec<u32>>,
    epoch: Cell<u32>,
}

impl DeadIds {
    /// Adds an id. Does nothing while the list is borrowed.
    pub fn push(&self, id: u32) {
        if let Ok(mut ids) = self.ids.try_borrow_mut() {
            ids.push(id);
        }
    }

    /// Takes all ids; the arenas start again at the next list.
    pub fn take(&self) -> Vec<u32> {
        self.epoch.set(self.epoch.get().wrapping_add(1));
        std::mem::take(&mut *self.ids.borrow_mut())
    }

    /// The ids that a reader has not seen; `seen` is the reader's epoch and
    /// the length of the list it has read, updated here.
    pub(crate) fn fresh(&self, seen: &mut (u32, usize)) -> Vec<u32> {
        let epoch = self.epoch.get();
        let Ok(ids) = self.ids.try_borrow() else {
            return Vec::new();
        };
        if *seen == (epoch, ids.len()) {
            return Vec::new();
        }
        let from = if seen.0 == epoch { seen.1 } else { 0 };
        let fresh = ids.get(from..).unwrap_or_default().to_vec();
        *seen = (epoch, from + fresh.len());
        fresh
    }

    /// The reader's starting point for the list as it is now.
    pub(crate) fn start(&self) -> (u32, usize) {
        (self.epoch.get(), 0)
    }
}

/// The shared counters through which an arena reports its work to the host:
/// the work units since the host last looked, and a flag that asks the host
/// to look. It also holds the list of dead ids of its registry.
#[derive(Clone, Debug)]
pub struct ArenaWork {
    steps: Rc<Cell<usize>>,
    request: Rc<Cell<bool>>,
    pub(crate) dead: Rc<DeadIds>,
}

impl ArenaWork {
    /// Counters that set `request` when `steps` reaches [`WORK_PER_CHECK`].
    pub fn new(steps: Rc<Cell<usize>>, request: Rc<Cell<bool>>, dead: Rc<DeadIds>) -> ArenaWork {
        ArenaWork {
            steps,
            request,
            dead,
        }
    }

    /// Adds `units` of work.
    pub(crate) fn charge(&self, units: usize) {
        let total = self.steps.get().saturating_add(units);
        self.steps.set(total);
        if total >= WORK_PER_CHECK {
            self.request.set(true);
        }
    }

    /// True if `other` shares this one's list of dead ids.
    pub(crate) fn same_list(&self, other: &ArenaWork) -> bool {
        Rc::ptr_eq(&self.dead, &other.dead)
    }
}
