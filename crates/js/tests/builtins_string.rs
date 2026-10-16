//! `String` statics and prototype methods, the string iterator, the URI
//! functions and Annex B (M7 feature 5b). Each case runs normally and in
//! the GC stress mode.
//!
//! - `chromium_cases`: the 318 snippets of `tools/probes/js-strings.json`
//!   with Chromium 148's results (`data/strings_cases.expected`; the
//!   script and the expected output are made by `data/gen-string-cases.js`
//!   from the output of `just probe`).
//! - The other cases are scripts whose expected output is the output of
//!   Node.js 22 (the order of the conversions, the dispatch through
//!   `@@replace` and `@@split`, the iterator, long strings).

#![allow(clippy::too_many_lines, reason = "a test is a list of script cases")]

mod common;

use common::{check_cases, on_big_stack, run, run_in, runtime};

#[test]
fn chromium_cases() {
    let script = include_str!("data/strings_cases.js");
    let expected = include_str!("data/strings_cases.expected");
    for stress in [false, true] {
        let actual = run(script, stress);
        let mut failures = Vec::new();
        for (actual, expected) in actual.lines().zip(expected.lines()) {
            if actual != expected {
                failures.push(format!("expected: {expected}\nactual:   {actual}"));
            }
        }
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

#[test]
fn conversion_order_and_dispatch() {
    check_cases(&[(
        include_str!("data/strings_order.js"),
        include_str!("data/strings_order.expected").trim_end_matches('\n'),
    )]);
}

#[test]
fn random_search_and_slice_agree_with_node() {
    check_cases(&[(
        include_str!("data/strings_random.js"),
        include_str!("data/strings_random.expected").trim_end_matches('\n'),
    )]);
}

#[test]
fn iterator_and_spread() {
    check_cases(&[
        (
            "var a = []; for (var c of 'a\\uD83D\\uDE00\\uD800b') a.push(c.length); print(a.join())",
            "1,2,1,1",
        ),
        (
            "var [x, y, ...z] = 'h\\u00e9llo'; print(x, y, z.join('-'), [...'ab'].length)",
            "h é l-l-o 2",
        ),
        (
            "var it = 'ab'[Symbol.iterator](); var r = it.next(); print(r.value, r.done, it.next().value, it.next().done, it.next().done)",
            "a false b true true",
        ),
        (
            "var p = Object.getPrototypeOf('x'[Symbol.iterator]()); print(Object.prototype.toString.call(p), p[Symbol.toStringTag], Object.getPrototypeOf(p) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())))",
            "[object String Iterator] String Iterator true",
        ),
        (
            "print(Math.sumPrecise(''), 1 / Math.sumPrecise(''))",
            "0 -Infinity",
        ),
        (
            "try { Math.sumPrecise('ab') } catch (e) { print(e.constructor.name + ': ' + e.message) }",
            "TypeError: Type of 'Iterator value' must be 'number', found 'string'",
        ),
    ]);
}

#[test]
fn long_strings_survive_collections() {
    check_cases(&[
        (
            "var s = 'ab'.repeat(5000); var t = s.slice(3, 9000).split('b').join('B'); \
             print(s.length, t.length, s.indexOf('ba', 9990), s.lastIndexOf('ab'), \
             s.replaceAll('ab', 'x').length, s.padStart(10001, '-').length, \
             encodeURIComponent(s.slice(0, 4) + '\\u00e9').length, escape(s).length)",
            "10000 8997 9991 9998 5000 10001 10 10000",
        ),
        (
            "var r = 'xyz'.repeat(1000); var n = 0; for (var i = 0; i < 50; i++) \
             { n += r.replace('y', '$&$&').length + r.split('z').length; } print(n)",
            "200100",
        ),
    ]);
}

#[test]
fn repeat_and_pad_build_ropes() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        assert_eq!(
            run_in(
                &mut rt,
                "var s = 'ab'.repeat(2 ** 28 - 12); print(s.length); var p = 'x'.padEnd(2 ** 29 - 24, 'yz'); print(p.length)"
            ),
            "536870888\n536870888"
        );
        assert_eq!(
            run_in(
                &mut rt,
                "try { 'ab'.repeat(2 ** 28) } catch (e) { print(e.constructor.name + ': ' + e.message) }"
            ),
            "RangeError: Invalid string length"
        );
        assert_eq!(rt.heap().stats().total_stale_roots, 0);
    });
}
