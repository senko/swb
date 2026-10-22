//! Cases for `ArrayBuffer` and `DataView` (M7 feature 11a): the measured
//! Chromium cases (`tools/probes/js-binary.json`: the constructors and
//! their conversions with side effects, resizable buffers and views that
//! track their length, detach through `transfer` and inside conversions,
//! `slice` and `transfer` with species, every getter and setter in both
//! byte orders with edge values, NaN bits, V8's messages), the
//! `console.log` forms of Node.js, and the `DataView` paths of the target
//! scripts. Each Chromium case also runs in the GC stress mode. The limits
//! are in `binary_hostile.rs`.

mod common;

use common::{check_cases, run};

/// The snippets of `tools/probes/js-binary.json` with Chromium 148's
/// results (`data/binary_cases.expected`; the script and the expected
/// output are made by `data/gen-binary-cases.js` from the output of
/// `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    let script = include_str!("data/binary_cases.js");
    let expected = include_str!("data/binary_cases.expected");
    for stress in [false, true] {
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

/// `console.log` of buffers and views, as Node.js 22 prints them.
#[test]
fn console_log() {
    check_cases(&[(
        include_str!("data/binary_console.js"),
        include_str!("data/binary_console.expected").trim_end_matches('\n'),
    )]);
}

/// The forms that Node.js 22 cannot print (its inspector fails on a view
/// whose getters throw): swb names the state of the view.
#[test]
fn console_log_of_views_without_bytes() {
    check_cases(&[
        (
            "var d = new DataView(new ArrayBuffer(4)); d.buffer.transfer(); console.log(d)",
            "DataView {\n  (detached),\n  buffer: ArrayBuffer { (detached), byteLength: 0 }\n}",
        ),
        (
            "var b = new ArrayBuffer(4, { maxByteLength: 8 }); var d = new DataView(b, 2, 2); \
             b.resize(1); console.log(d)",
            "DataView {\n  (out of bounds),\n  buffer: ArrayBuffer { [Uint8Contents]: <00>, byteLength: 1 }\n}",
        ),
    ]);
}

/// The `DataView` paths of the target scripts (ars-js/s01.js reads
/// `getInt32` and `getInt16` and writes `setUint8`, `setInt16`, `setInt32`
/// and `setUint32` over a growing buffer; the BBC scripts test
/// `DataView.prototype.isPrototypeOf` and the `[object DataView]` tag),
/// written anew with the same calls. The expected output is that of
/// Node.js 22.
#[test]
fn paths_of_the_target_scripts_for_buffers() {
    check_cases(&[
        (
            "function hash(view) { var h = 0x811c9dc5; for (var i = 0; i + 4 <= view.byteLength; i += 4) \
               { h = Math.imul(h ^ view.getInt32(i, true), 16777619) | 0; } return h >>> 0; } \
             var b = new ArrayBuffer(64); var d = new DataView(b); \
             for (var i = 0; i < 64; i++) d.setUint8(i, i * 37 & 255); \
             print(hash(d), hash(new DataView(b, 4, 16)), d.getInt16(10), d.getInt16(10, true))",
            "935824581 2550787301 29335 -26766",
        ),
        (
            "var cap = 4; var buf = new ArrayBuffer(cap); var view = new DataView(buf); var len = 0; \
             function grow(n) { if (len + n <= cap) return; while (cap < len + n) cap *= 2; \
               var next = new ArrayBuffer(cap); new DataView(next).setUint32(0, 0); \
               for (var i = 0; i < len; i++) new DataView(next).setUint8(i, view.getUint8(i)); \
               buf = next; view = new DataView(buf); } \
             function put32(v) { grow(4); view.setInt32(len, v, true); len += 4; } \
             function put16(v) { grow(2); view.setInt16(len, v, true); len += 2; } \
             for (var k = 0; k < 20; k++) { put32(k * 1000003); put16(-k); } \
             var out = []; for (var p = 0; p < len; p += 6) out.push(view.getInt32(p, true) + view.getInt16(p + 4, true)); \
             print(cap, len, out.slice(-3).join())",
            "128 120 17000034,18000036,19000038",
        ),
        (
            "var d = new DataView(new ArrayBuffer(2)); \
             print(DataView.prototype.isPrototypeOf(d), Object.prototype.toString.call(d), \
               Object.prototype.toString.call(d.buffer), typeof DataView === 'function', \
               d.byteLength != 2 || d.byteOffset != 0)",
            "true [object DataView] [object ArrayBuffer] true false",
        ),
    ]);
}

/// Buffers and views keep each other alive across collections, and a view
/// sees the bytes of its buffer after the collector ran.
#[test]
fn views_survive_collections() {
    check_cases(&[(
        "function make() { var d = new DataView(new ArrayBuffer(16), 4); d.setFloat64(0, Math.PI); return d; } \
         var views = []; for (var i = 0; i < 200; i++) views.push(make()); gc(); \
         var ok = views.every(function (d) { return d.getFloat64(0) === Math.PI && d.buffer.byteLength === 16; }); \
         var b = new ArrayBuffer(8, { maxByteLength: 64 }); var t = new DataView(b); gc(); b.resize(64); gc(); \
         t.setUint8(63, 7); print(ok, t.byteLength, t.getUint8(63))",
        "true 64 7",
    )]);
}
