//! Cases for the typed arrays (M7 feature 11b): the measured Chromium
//! cases (`tools/probes/js-typed.json`: the constructors with all argument
//! forms, the `TypedArray` exotic object with odd keys, the conversions of
//! every element type, length tracking and out-of-bounds arrays, detaching
//! in the conversions, `set`, `subarray`, `slice` with species, the
//! iterators, `from`, `of`, the messages), the `console.log` forms of
//! Node.js, and the typed array paths of the target scripts. Each Chromium
//! case also runs in the GC stress mode. The limits are in
//! `typed_hostile.rs`.

mod common;

use common::{check_cases, run};

/// The snippets of `tools/probes/js-typed.json` with Chromium 148's
/// results (`data/typed_cases.expected`; the script and the expected output
/// are made by `data/gen-typed-cases.js` from the output of `just probe`),
/// normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    let script = include_str!("data/typed_cases.js");
    let expected = include_str!("data/typed_cases.expected");
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

/// `console.log` of typed arrays, as Node.js 22 prints them.
#[test]
fn console_log() {
    check_cases(&[(
        include_str!("data/typed_console.js"),
        include_str!("data/typed_console.expected").trim_end_matches('\n'),
    )]);
}

/// The forms that Node.js 22 cannot show or shows otherwise: `Float16Array`
/// (Node.js 22 has none), and the arrays of more than 100 elements, which
/// end with the count of the rest as in Node.js (Node.js also groups the
/// numbers in columns, which swb does not do for arrays, `console.rs`).
#[test]
fn console_log_without_node() {
    let long = |prefix: &str, element: &str, rest: &str| {
        let mut lines = vec![format!("{prefix} [")];
        lines.extend(std::iter::repeat_n(format!("  {element},"), 100));
        lines.push(format!("  {rest}"));
        lines.push("]".to_owned());
        lines.join("\n")
    };
    check_cases(&[
        (
            "console.log(new Float16Array([1.5, -0, NaN, 65504]))",
            "Float16Array(4) [ 1.5, -0, NaN, 65504 ]",
        ),
        (
            "console.log(new Uint8Array(101))",
            &long("Uint8Array(101)", "0", "... 1 more item"),
        ),
        (
            "console.log(new BigInt64Array(150))",
            &long("BigInt64Array(150)", "0n", "... 50 more items"),
        ),
    ]);
}

/// The typed array paths of the target scripts, written anew with the same
/// calls: the `Buffer` polyfill (a `Uint8Array` whose prototype is set to a
/// subclass prototype, the `TYPED_ARRAY_SUPPORT` check), the UUID makers
/// (`Uint8Array(16)` read by index and as hexadecimal), the group sizes of a
/// style sheet (`Uint32Array` that grows with `set`), the SHA-256 of an
/// obfuscated script (`Int32Array`, `Uint32Array` constants and the views of
/// a buffer with byte offset and length), `String.fromCharCode.apply` of a
/// `Uint8Array` and `ArrayBuffer.isView`. The expected output is that of
/// Node.js 22.
#[test]
fn paths_of_the_target_scripts() {
    check_cases(&[
        (
            "function B(n) { var t = new Uint8Array(n); Object.setPrototypeOf(t, B.prototype); return t; } \
             Object.setPrototypeOf(B.prototype, Uint8Array.prototype); Object.setPrototypeOf(B, Uint8Array); \
             B.prototype.hello = function () { return this.length * 2; }; \
             var b = B(3); b[1] = 258; print(b.hello(), b[1], b instanceof Uint8Array, b instanceof B); \
             var support = (function () { try { var e = new Uint8Array(1), t = { foo: function () { return 42; } }; \
             Object.setPrototypeOf(t, Uint8Array.prototype); Object.setPrototypeOf(e, t); return e.foo() === 42; } \
             catch (e) { return false; } })(); print(support);",
            "6 2 true true\ntrue",
        ),
        (
            "var E = []; for (var x = 0; x < 256; ++x) E.push((x + 256).toString(16).slice(1)); \
             var I = new Uint8Array(16); for (var i = 0; i < 16; i++) I[i] = (i * 37 + 11) & 255; \
             I[6] = 15 & I[6] | 64; I[8] = 63 & I[8] | 128; \
             print(E[I[0]] + E[I[1]] + E[I[2]] + E[I[3]] + '-' + E[I[4]] + E[I[5]] + '-' + E[I[6]] + E[I[7]] + '-' + \
             E[I[8]] + E[I[9]] + '-' + E[I[10]] + E[I[11]] + E[I[12]] + E[I[13]] + E[I[14]] + E[I[15]])",
            "0b30557a-9fc4-490e-b358-7da2c7ec1136",
        ),
        (
            "function G() { this.groupSizes = new Uint32Array(512); this.length = 512; } \
             G.prototype.insert = function (g, n) { if (g >= this.length) { var r = this.groupSizes, o = this.length; \
             for (; g >= o;) o <<= 1; this.groupSizes = new Uint32Array(o); this.groupSizes.set(r); this.length = o; } \
             this.groupSizes[g] += n; }; \
             var s = new G(); s.insert(3, 2); s.insert(700, 5); s.insert(3, 1); \
             print(s.length, s.groupSizes[3], s.groupSizes[700], s.groupSizes.length)",
            "1024 3 5 1024",
        ),
        (
            "var K = new Uint32Array([0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5]); \
             var temp = new Int32Array(0x40), buffer = new Uint8Array(0x40); \
             for (var i = 0; i < 4; i++) temp[i] = K[i] ^ (K[(i + 1) % 4] >>> 7); \
             buffer.set(new Uint8Array(new Uint32Array([temp[0] >>> 0]).buffer), 4); \
             var view = new Uint8Array(buffer.buffer, buffer.byteOffset + 2, 6); \
             print(temp[0], temp[3], K[2], Array.from(view).join(','), ArrayBuffer.isView(view), \
             typeof Uint32Array.from)",
            "1114128657 -382676998 3049323471 0,0,17,65,104,66 true function",
        ),
        (
            "var u = new Uint8Array([104, 105, 33]); var n = new Array(u.length); \
             for (var i = 0; i < u.length; i++) n[i] = String.fromCharCode(u[i]); \
             print(String.fromCharCode.apply(null, u), n.join(''), Object.prototype.toString.call(u))",
            "hi! hi! [object Uint8Array]",
        ),
    ]);
}
