//! The warning about the port limit (M9 feature 9, ADR 0031 section 11):
//! once per episode, not once per failed `new MessageChannel()`. A separate
//! test binary, because the test installs the process's logger.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};

use common::{answer, expected, load_in, on_big_stack};
use swb_web::ClockMode;

/// The warnings about the port limit.
static LIMIT_WARNINGS: AtomicUsize = AtomicUsize::new(0);

struct Counter;

impl log::Log for Counter {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.args().to_string().contains("message ports are live") {
            LIMIT_WARNINGS.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn flush(&self) {}
}

static COUNTER: Counter = Counter;

/// 5,000 failed constructions at the limit log one warning; after a channel
/// fits again, the next episode logs one more.
#[test]
fn the_port_limit_warns_once_per_episode() {
    on_big_stack(|| {
        log::set_logger(&COUNTER).unwrap();
        log::set_max_level(log::LevelFilter::Warn);
        let mut rt = load_in("<!DOCTYPE html><p>x", false, ClockMode::Virtual);
        // Keeps channels until the first failure.
        let fill = "(() => { window.keep = []; try { for (;;) keep.push(new MessageChannel()); } catch (e) { return keep.length; } })()";
        assert_eq!(answer(&mut rt, fill), expected("32768"));
        let fail = "(() => { let n = 0; for (let i = 0; i < 5000; i++) { try { new MessageChannel(); } catch (e) { n++; } } return n; })()";
        assert_eq!(answer(&mut rt, fail), expected("5000"));
        assert_eq!(LIMIT_WARNINGS.load(Ordering::SeqCst), 1);
        // Room again, then full again: a second episode.
        assert_eq!(answer(&mut rt, "keep = null; 1"), expected("1"));
        rt.collect();
        assert_eq!(
            answer(&mut rt, "new MessageChannel() instanceof MessageChannel"),
            expected("true")
        );
        assert_eq!(answer(&mut rt, fill), expected("32768"));
        assert_eq!(answer(&mut rt, fail), expected("5000"));
        assert_eq!(LIMIT_WARNINGS.load(Ordering::SeqCst), 2);
    });
}
