//! The time limit of scripts (ADR 0026 section 9, memo 1.5): a countdown
//! in the VM and the time check that runs when it reaches zero.
//!
//! The interpreter decrements the countdown at backward jumps, function
//! entries (also of accessor calls and deferred calls) and generator
//! resumptions; built-in functions with long loops decrement it once per
//! step ([`Runtime::tick`]). Every loop of a script contains a backward
//! jump or a call, so no script runs without a check. At zero, the check
//! reads the termination request of the host and compares the clock with
//! the deadline; either ends the script with a termination (no handler
//! and no `finally` block runs).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::error::Termination;
use crate::runtime::Runtime;
use crate::vm::{VmError, VmResult};

/// The countdown steps between two time checks. A check costs one read
/// of an atomic flag and, with a deadline, one read of the clock (about
/// 20 ns); a step of the fastest loop costs about 12 ns. Measured in a
/// release build: the countdown itself costs about 5 % on a loop of
/// integer additions, and intervals from 1,000 to 100,000 make no
/// measurable difference to it; with 10,000, endless loops (also in
/// native loops over holes) end within 0.15 ms of the deadline.
pub(crate) const TIME_CHECK_INTERVAL: u32 = 10_000;

/// The code units that a copy costs as much as one step of the
/// countdown (about 12 ns for a step; a copy of 64 code units takes about
/// 10 to 30 ns).
pub(crate) const UNITS_PER_STEP: usize = 64;

/// A handle that asks a runtime to end its running script. It can be
/// cloned and sent to other threads. A request ends the script that runs
/// at the next time check (or the next script that runs, if none runs);
/// the check clears the request.
#[derive(Clone, Debug, Default)]
pub struct TerminationHandle(Arc<AtomicBool>);

impl TerminationHandle {
    /// Asks the runtime to end its running script.
    pub fn terminate(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether a request is pending.
    pub fn is_requested(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Withdraws a pending request.
    pub fn cancel(&self) {
        self.0.store(false, Ordering::Relaxed);
    }

    /// Takes a pending request (clears it).
    fn take(&self) -> bool {
        self.0.swap(false, Ordering::Relaxed)
    }
}

impl Runtime {
    /// One step of the countdown; at zero, the time check. Built-in
    /// functions call it once per step of a loop whose length a script
    /// controls.
    #[inline]
    pub(crate) fn tick(&mut self) -> VmResult<()> {
        self.vm.countdown = self.vm.countdown.wrapping_sub(1);
        if self.vm.countdown == 0 {
            return self.check_time();
        }
        Ok(())
    }

    /// `steps` steps of the countdown at once, for a built-in function
    /// that does work in proportion to `steps` in one call (a loop over
    /// the keys of an object). Runs the time check when the countdown is
    /// used up.
    #[inline]
    pub(crate) fn charge(&mut self, steps: usize) -> VmResult<()> {
        let steps = u32::try_from(steps).unwrap_or(u32::MAX);
        if steps >= self.vm.countdown {
            return self.check_time();
        }
        self.vm.countdown -= steps;
        Ok(())
    }

    /// The cost of copying or scanning `units` code units: one step per
    /// [`UNITS_PER_STEP`] units. Call it after the work, so that a script
    /// that copies big strings in a loop reaches the check.
    #[inline]
    pub(crate) fn charge_units(&mut self, units: usize) -> VmResult<()> {
        self.charge(units / UNITS_PER_STEP)
    }

    /// The time check: a termination if the host asked for one or the
    /// deadline has passed. Restarts the countdown.
    #[cold]
    pub(crate) fn check_time(&mut self) -> VmResult<()> {
        self.vm.countdown = TIME_CHECK_INTERVAL;
        if self.vm.termination.take() {
            return Err(VmError::Terminated(Termination::HostRequest));
        }
        if let Some(deadline) = self.vm.deadline
            && Instant::now() >= deadline
        {
            return Err(VmError::Terminated(Termination::TimeLimit));
        }
        Ok(())
    }
}
