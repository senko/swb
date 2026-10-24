//! The script runtime of a document is dropped on navigation, and its
//! memory comes back (exit criterion of M8 feature 4). The test reads the
//! resident set size of the process (`/proc/self/status`, Linux only), so
//! it is the only test of this binary: other tests in the same process
//! would change the number.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Size, Url};
use swb_net::Fetcher;

mod common;
use common::{TestSite, finish_loading, new_page};

/// The resident set size of the process in bytes, if the system reports
/// it.
fn resident() -> Option<usize> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kib: usize = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[test]
fn the_memory_of_a_runtime_comes_back_after_navigation() {
    if resident().is_none() {
        return;
    }
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| {
            let mut site = TestSite::default();
            for name in ["a", "b"] {
                site.pages.insert(
                    format!("https://site.test/{name}.html"),
                    format!("<!DOCTYPE html><title>{name}</title><p>{name}"),
                );
            }
            let mut page = new_page(Arc::new(site) as Arc<dyn Fetcher>, 2, Size::new(600.0, 400.0));
            page.set_javascript(true);
            page.navigate(Url::parse("https://site.test/a.html").unwrap());
            finish_loading(&mut page, Duration::from_secs(20));
            let before = resident().unwrap();
            // About 150 MB of strings, arrays and objects, held by the
            // global object and by expandos of node wrappers.
            let grow = "window.big = []; for (let i = 0; i < 40; i++) big.push('x'.repeat(1 << 20) + i); \
                        document.body.objects = Array.from({ length: 300000 }, (_, i) => ({ i, s: 'n' + i })); \
                        big.length";
            assert_eq!(
                page.evaluate(grow, JsonForm::Strict),
                Some(Evaluation::Value("40".to_owned()))
            );
            let grown = resident().unwrap();
            let growth = grown.saturating_sub(before);
            assert!(growth > 60 << 20, "the scripts grew the process by {growth} bytes only");
            assert!(page.script_heap_size().unwrap() > 60 << 20);
            page.navigate(Url::parse("https://site.test/b.html").unwrap());
            finish_loading(&mut page, Duration::from_secs(20));
            assert_eq!(page.title(), "b");
            assert!(page.script_heap_size().unwrap() < 8 << 20);
            let after = resident().unwrap();
            let kept = after.saturating_sub(before);
            eprintln!("resident: {before} before, {grown} with the scripts' data, {after} after the navigation");
            assert!(
                kept < growth / 2,
                "after the navigation the process keeps {kept} of {growth} bytes"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
