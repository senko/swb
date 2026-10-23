//! Stack sizes for the page (ADR 0028 section 2).
//!
//! The JavaScript runtime of a page is not `Send` and recurses on the
//! native stack of the thread that runs the page, so that thread needs a
//! large stack:
//!
//! - The headless runner and the headless automation server run the page
//!   on a thread that swb creates with [`PAGE_STACK`] bytes.
//! - The GUI runs the page on the main thread, whose stack limit comes
//!   from the process (`ulimit -s`). [`main_stack_is_enough`] reads it.
//!   Below [`MIN_MAIN_STACK`], the GUI keeps JavaScript off.

use std::thread;

use anyhow::{Result, anyhow};

/// The stack size of the thread that runs a headless page.
pub(crate) const PAGE_STACK: usize = 16 * 1024 * 1024;

/// The least stack limit of the process with which the GUI allows
/// JavaScript.
pub(crate) const MIN_MAIN_STACK: u64 = 8 * 1024 * 1024;

/// True if a stack limit (`None` is no limit) is enough for JavaScript on
/// the main thread.
pub(crate) fn limit_is_enough(limit: Option<u64>) -> bool {
    limit.is_none_or(|bytes| bytes >= MIN_MAIN_STACK)
}

/// The stack limit of the process in bytes, or `None` if there is no limit
/// (or the platform has no such limit).
#[cfg(unix)]
pub(crate) fn main_stack_limit() -> Option<u64> {
    use rustix::process::{Resource, getrlimit};
    getrlimit(Resource::Stack).current
}

/// The stack limit of the process (not known on this platform).
#[cfg(not(unix))]
pub(crate) fn main_stack_limit() -> Option<u64> {
    None
}

/// True if the main thread's stack can run JavaScript.
pub(crate) fn main_stack_is_enough() -> bool {
    limit_is_enough(main_stack_limit())
}

/// Runs `work` on a new thread with a [`PAGE_STACK`] stack and returns its
/// result. A panic of the thread continues on the calling thread, so the
/// process ends as after a panic on the main thread (exit status 101).
pub(crate) fn on_page_thread<T: Send>(work: impl FnOnce() -> T + Send) -> Result<T> {
    thread::scope(|scope| {
        let handle = thread::Builder::new()
            .name("page".to_owned())
            .stack_size(PAGE_STACK)
            .spawn_scoped(scope, work)
            .map_err(|e| anyhow!("cannot start the page thread: {e}"))?;
        Ok(handle
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits() {
        assert!(limit_is_enough(None));
        assert!(limit_is_enough(Some(8 * 1024 * 1024)));
        assert!(limit_is_enough(Some(u64::MAX)));
        assert!(!limit_is_enough(Some(8 * 1024 * 1024 - 1)));
        assert!(!limit_is_enough(Some(0)));
    }

    #[test]
    fn page_thread_has_a_deep_stack() {
        // About 5 MiB of frames: more than the 2 MiB of a test thread.
        fn depth(n: u32) -> u64 {
            let block = [u64::from(n); 64];
            if n == 0 {
                0
            } else {
                std::hint::black_box(block)[0] + depth(n - 1)
            }
        }
        let sum = on_page_thread(|| depth(10_000)).expect("thread");
        assert!(sum > 0);
    }

    #[test]
    fn page_thread_panic_continues_on_the_caller() {
        let result = std::panic::catch_unwind(|| on_page_thread(|| -> u32 { panic!("test") }));
        let payload = result.expect_err("the panic reaches the caller");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"test"));
    }
}
