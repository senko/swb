//! The check mode of the targeted restyle (ADR 0041 section 6, ADR 0042
//! section 7), for tests: when it is on, each restyle after a state change
//! or after DOM changes that took the targeted path is compared with a full
//! restyle, and a difference panics. Only in
//! builds with debug assertions; release builds have no check and no cost.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);
static CHECKS: AtomicU64 = AtomicU64::new(0);

/// Turns the check mode on or off (for the whole process).
pub fn set_restyle_check(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// The number of targeted restyles that the check mode compared.
pub fn restyle_checks() -> u64 {
    CHECKS.load(Ordering::Relaxed)
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn count() {
    CHECKS.fetch_add(1, Ordering::Relaxed);
}
