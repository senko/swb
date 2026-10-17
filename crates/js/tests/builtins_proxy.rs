//! Cases for `Proxy` (M7 feature 9a): the measured Chromium cases
//! (`tools/probes/js-proxy.json`: the shape of `Proxy`, the messages of
//! revoked proxies and of every invariant, the order of trap calls of the
//! operations and built-ins, `for`-`in`, `JSON`, `with`, the uses of the
//! target scripts), proxies whose traps allocate and collect, and the
//! `console.log` of proxies. Each case also runs in the GC stress mode.

mod common;

use common::{check_cases, run};

/// The snippets of `tools/probes/js-proxy.json` with Chromium 148's
/// results (`data/proxy_cases.expected`; the script and the expected
/// output are made by `data/gen-proxy-cases.js` from the output of
/// `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/proxy_cases.js"),
        include_str!("data/proxy_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with chains of 100,000 to 1,000,000 proxies and
/// lists of 100,000 keys, without the stress mode.
#[test]
fn chromium_cases_heavy() {
    common::on_big_stack(|| {
        check_chromium(
            include_str!("data/proxy_cases_heavy.js"),
            include_str!("data/proxy_cases_heavy.expected"),
            &[false],
        );
    });
}

fn check_chromium(script: &str, expected: &str, modes: &[bool]) {
    for &stress in modes {
        let actual = run(script, stress);
        let failures: Vec<String> = actual
            .lines()
            .zip(expected.lines())
            .filter(|(actual, expected)| actual != expected)
            .map(|(actual, expected)| format!("expected: {expected}\nactual:   {actual}"))
            .collect();
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "number of lines (stress: {stress})"
        );
        assert!(
            failures.is_empty(),
            "stress: {stress}\n{}",
            failures.join("\n")
        );
    }
}

/// Traps that allocate and collect while the engine holds targets,
/// handlers, keys, descriptors and results (the expected output is that
/// of Node.js 22).
#[test]
fn traps_that_collect() {
    check_cases(&[(
        include_str!("data/proxy_gc.js"),
        include_str!("data/proxy_gc.expected").trim_end_matches('\n'),
    )]);
}

/// `console.log` shows the target of a proxy and runs no trap, as
/// Node.js 22 does.
#[test]
fn console_log_shows_the_target() {
    check_cases(&[(
        include_str!("data/proxy_console.js"),
        include_str!("data/proxy_console.expected").trim_end_matches('\n'),
    )]);
}
