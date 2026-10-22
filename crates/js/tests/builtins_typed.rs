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

/// Runs a snippet script and compares each output line with the expected
/// line (the results of Chromium 148), normally and in the GC stress mode.
fn check_snippets(script: &str, expected: &str) {
    for stress in [false, true] {
        let actual = run(script, stress);
        let failures: Vec<String> = actual
            .lines()
            .zip(expected.lines())
            .filter(|(actual, expected)| actual != expected)
            .map(|(actual, expected)| format!("expected: {expected}\nactual:   {actual}"))
            .take(20)
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

/// The snippets of `tools/probes/js-typed.json` with Chromium 148's
/// results (`data/typed_cases.expected`; the script and the expected output
/// are made by `data/gen-typed-cases.js` from the output of `just probe`),
/// normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_snippets(
        include_str!("data/typed_cases.js"),
        include_str!("data/typed_cases.expected"),
    );
}

/// The snippets of feature 11c (`data/typed-method-snippets.txt`): the
/// methods of `%TypedArray%.prototype` with every element type, the
/// receivers and callbacks, species, arguments that change the buffer,
/// the calls of the comparator, `Atomics`, and the base64 and hex methods
/// of `Uint8Array` with their options and malformed input. The groups
/// that swb reproduces differently on purpose (`m-known-*`) are not in
/// the script; `gen-typed-cases.js` says why.
#[test]
fn chromium_method_cases() {
    check_snippets(
        include_str!("data/typed_methods_cases.js"),
        include_str!("data/typed_methods_cases.expected"),
    );
}

/// The default sort of arrays above one chunk (16,384 elements: runs of
/// `sort_unstable` and the merges between them) gives what the sort with
/// a comparator gives, for every element type, with duplicates, −0, NaN
/// and the ends of the ranges; `toSorted` leaves the receiver alone, and
/// the NaNs keep their bit patterns and their order.
#[test]
fn large_default_sorts_agree_with_the_sort_of_a_comparator() {
    let script = r"
        function cmp(x, y) {
          if (x !== x) return y !== y ? 0 : 1;
          if (y !== y) return -1;
          if (x < y) return -1;
          if (x > y) return 1;
          if (x === 0 && y === 0) return (Object.is(x, -0) ? 0 : 1) - (Object.is(y, -0) ? 0 : 1);
          return 0;
        }
        function same(a, b) {
          if (a.length !== b.length) return false;
          for (var i = 0; i < a.length; i++) if (!Object.is(a[i], b[i])) return false;
          return true;
        }
        var seed = 7;
        function next() { seed = (seed * 1103515245 + 12345) % 2147483648; return seed; }
        [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array,
         Float16Array, Float32Array, Float64Array, BigInt64Array, BigUint64Array].forEach(function (T) {
          var big = T === BigInt64Array || T === BigUint64Array, a = new T(40000);
          for (var i = 0; i < a.length; i++) {
            var r = next();
            a[i] = big ? BigInt(r) * BigInt(next()) - 4611686018427387904n : (r % 7 === 0 ? r % 5 - 2 : r) % 70000 - 30000;
          }
          if (T === Float16Array || T === Float32Array || T === Float64Array) {
            for (var k = 0; k < a.length; k += 997) a[k] = NaN;
            for (var k = 5; k < a.length; k += 1999) a[k] = -0;
            for (var k = 7; k < a.length; k += 1999) a[k] = 0;
            a[1] = Infinity; a[2] = -Infinity;
          }
          var before = a.slice(), s = a.toSorted(), c = a.toSorted(cmp), b = a.slice().sort();
          print(T.name, same(s, c), same(a, before), same(b, s));
        });
        var f = new Float64Array(new Uint8Array(24).buffer);
        var u = new Uint32Array(f.buffer);
        u.set([1, 0x7ff80000, 2, 0xfff80000, 0, 0x3ff00000]);
        f.sort();
        print(Array.from(u).join());
    ";
    let out = run(script, false);
    let mut lines: Vec<String> = [
        "Int8Array",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "BigInt64Array",
        "BigUint64Array",
    ]
    .iter()
    .map(|name| format!("{name} true true true"))
    .collect();
    // 1.0 first, then the NaNs as they were: the words (hi 7ff80000, lo 1)
    // and (hi fff80000, lo 2).
    lines.push("0,1072693248,1,2146959360,2,4294443008".to_owned());
    let expected = lines.join("\n");
    assert_eq!(out, expected);
}

/// The two-byte types sort with keys below 262,144 elements and with a
/// counting table from there on. Both paths give a sorted array with the
/// same elements (a count of every 16-bit word), with the NaNs of a float
/// type last in their bit patterns and order and -0 before +0; below the
/// threshold the result also equals the sort of a comparator.
#[test]
fn two_byte_sorts_agree_on_both_sides_of_the_counting_threshold() {
    let script = r"
        function cmp(x, y) {
          if (x !== x) return y !== y ? 0 : 1;
          if (y !== y) return -1;
          if (x < y) return -1;
          if (x > y) return 1;
          if (x === 0 && y === 0) return (Object.is(x, -0) ? 0 : 1) - (Object.is(y, -0) ? 0 : 1);
          return 0;
        }
        function same(a, b) {
          if (a.length !== b.length) return false;
          for (var i = 0; i < a.length; i++) if (!Object.is(a[i], b[i])) return false;
          return true;
        }
        var seed = 11;
        function next() { seed = (seed * 1103515245 + 12345) % 2147483648; return seed; }
        function isNan(v) { return (v & 0x7c00) === 0x7c00 && (v & 0x3ff) !== 0; }
        [Int16Array, Uint16Array, Float16Array].forEach(function (T) {
          var float = T === Float16Array;
          [0, 1, 8, 4096, 262143, 262144].forEach(function (n) {
            var a = new T(n);
            for (var i = 0; i < n; i++) a[i] = next() % 70000 - 30000;
            if (float) {
              for (var k = 0; k < n; k += 7) a[k] = NaN;
              for (var k = 1; k < n; k += 11) a[k] = -0;
              for (var k = 2; k < n; k += 13) a[k] = 0;
              if (n > 4) { a[3] = Infinity; a[4] = -Infinity; }
              var w = new Uint16Array(a.buffer);
              for (var k = 0; k < n; k += 14) w[k] = 0x7e01 + (k & 255) + ((k & 1) << 15);
            }
            var before = a.slice(), s = a.toSorted();
            var wb = new Uint16Array(before.buffer), ws = new Uint16Array(s.buffer);
            // The same words; the numbers in order; the NaNs last, as they were.
            var nans = float ? wb.filter(isNan) : wb.slice(0, 0);
            var count = n - nans.length, ok = true, histogram = new Int32Array(65536);
            for (var i = 0; i < n; i++) { histogram[wb[i]]++; histogram[ws[i]]--; }
            for (var i = 0; i < 65536; i++) if (histogram[i] !== 0) ok = false;
            for (var i = 1; i < count; i++) if (cmp(s[i - 1], s[i]) > 0) ok = false;
            for (var i = 0; i < nans.length; i++) if (ws[count + i] !== nans[i]) ok = false;
            var order = n > 5000 ? true : same(s, a.toSorted(cmp));
            var inPlace = a.sort();
            print(T.name, n, ok, order, same(inPlace, s), a === inPlace);
          });
        });
    ";
    let out = run(script, false);
    let mut lines = Vec::new();
    for name in ["Int16Array", "Uint16Array", "Float16Array"] {
        for n in [0, 1, 8, 4096, 262_143, 262_144] {
            lines.push(format!("{name} {n} true true true true"));
        }
    }
    assert_eq!(out, lines.join("\n"));
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
fn paths_of_the_target_scripts_for_typed_arrays() {
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
