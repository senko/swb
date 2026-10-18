//! Cases for `BigInt` in the language (M7 feature 10b): the measured
//! Chromium cases (`tools/probes/js-bigint.json`: literals and literal
//! keys, every operator with every type combination, comparisons and
//! loose equality with Numbers, strings and booleans, `++` and `--`, the
//! conversions and their errors, the `BigInt` function, `asIntN`,
//! `asUintN`, the prototype methods, wrapper objects, V8's messages), the
//! `console.log` forms of Node.js, and the two string widths of the
//! string conversions. Each Chromium case also runs in the GC stress mode.
//! The limits are in `bigint_hostile.rs`.

mod common;

use common::{check_cases, run};

/// The snippets of `tools/probes/js-bigint.json` with Chromium 148's
/// results (`data/bigint_cases.expected`; the script and the expected
/// output are made by `data/gen-bigint-cases.js` from the output of
/// `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    let script = include_str!("data/bigint_cases.js");
    let expected = include_str!("data/bigint_cases.expected");
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

/// `console.log` of `BigInt` values and wrapper objects, as Node.js 22
/// prints them.
#[test]
fn console_log() {
    check_cases(&[(
        include_str!("data/bigint_console.js"),
        include_str!("data/bigint_console.expected").trim_end_matches('\n'),
    )]);
}

/// `StringToBigInt` and the comparisons with strings read both string
/// widths: one byte per unit (Latin-1) and two (a string with a unit above
/// U+00FF, here the white space U+2028 and U+3000 or a letter that makes
/// the text invalid), also as ropes.
#[test]
fn both_string_widths() {
    check_cases(&[
        (
            "var wide = '\\u3000 123 \\u2028'; var narrow = '\\xa0 123 \\t'; \
             print(BigInt(wide), BigInt(narrow), BigInt(wide) === BigInt(narrow))",
            "123 123 true",
        ),
        (
            "var r = ''; for (var i = 0; i < 40; i++) r += '9'; var w = r + '\\u2028'; \
             print(BigInt(r) === BigInt(w), BigInt(w) % 1000n)",
            "true 999",
        ),
        (
            "print(123n == '\\u3000123', 123n == ' 123', 123n < '\\u2028124', \
             123n > '122\\u3000', 1n == '1\\u0100', 1n < '2\\u0100', 1n >= 'x\\u0100')",
            "true true true true false false false",
        ),
        (
            "var hex = '0x' + 'f'.repeat(30); print(BigInt(hex + '\\u3000') === BigInt(hex), \
             BigInt(hex) == hex + '\\u2028', BigInt(hex) > '\\u3000' + hex)",
            "true true false",
        ),
        (
            "try { BigInt('12\\u0100') } catch (e) { print(e.name, e.message) } \
             try { BigInt('12\\xff') } catch (e) { print(e.name, e.message) }",
            "SyntaxError Cannot convert 12\u{100} to a BigInt\nSyntaxError Cannot convert 12\u{ff} to a BigInt",
        ),
        (
            "var s = 'x'.repeat(1001); try { BigInt(s) } catch (e) { print(e.message.length, e.message.slice(1010)) } \
             s = '\\u0100'.repeat(1001); try { BigInt(s) } catch (e) { print(e.message.length) }",
            "1028 xxxxx… to a BigInt\n1028",
        ),
    ]);
}

/// The 64-bit integer paths of the protobuf code of ars-js/s02.js, written
/// anew with the same operators: two 32-bit halves joined with `<<` and
/// `|` into a decimal text, a text split with `asIntN(64, ...)`, `&`, `>>`
/// and `Number(...) >>> 0`, and the feature check (the expected output is
/// that of Node.js 22).
#[test]
fn int64_paths_of_the_target_script() {
    let script = "function join(lo, hi) { return '' + (BigInt(hi | 0) << BigInt(32) | BigInt(lo >>> 0)); } \
        function joinUnsigned(lo, hi) { return '' + (BigInt(hi >>> 0) << BigInt(32) | BigInt(lo >>> 0)); } \
        function split(text) { var a = BigInt.asIntN(64, BigInt(text)); \
          return [Number(a & BigInt(4294967295)) >>> 0, Number(a >> BigInt(32) & BigInt(4294967295))]; } \
        var out = []; \
        var values = ['0', '1', '-1', '4294967295', '4294967296', '-4294967296', '9223372036854775807', \
          '-9223372036854775808', '18446744073709551615', '123456789012345678', '-98765432109876543']; \
        for (var i = 0; i < values.length; i++) { var p = split(values[i]); \
          out.push(values[i] + ':' + p.join('/') + ':' + join(p[0], p[1]) + ':' + joinUnsigned(p[0], p[1])); } \
        out.push(typeof BigInt === 'function' && typeof BigInt(0) === 'bigint', \
          BigInt(Number.MIN_SAFE_INTEGER) <= BigInt('-9007199254740991'), \
          String(BigInt.asIntN(64, BigInt('9223372036854775808')))); \
        print(out.join('\\n'))";
    let expected = "0:0/0:0:0\n1:1/0:1:1\n-1:4294967295/4294967295:-1:18446744073709551615\n\
        4294967295:4294967295/0:4294967295:4294967295\n4294967296:0/1:4294967296:4294967296\n\
        -4294967296:0/4294967295:-4294967296:18446744069414584320\n\
        9223372036854775807:4294967295/2147483647:9223372036854775807:9223372036854775807\n\
        -9223372036854775808:0/2147483648:-9223372036854775808:9223372036854775808\n\
        18446744073709551615:4294967295/4294967295:-1:18446744073709551615\n\
        123456789012345678:2788225870/28744523:123456789012345678:123456789012345678\n\
        -98765432109876543:3741366977/4271971676:-98765432109876543:18347978641599675073\n\
        true\ntrue\n-9223372036854775808";
    check_cases(&[(script, expected)]);
}

/// The other uses of the conversions: template literals, `String`,
/// property keys, `parseInt`, the operators in loops (the slow paths of
/// the interpreter), `switch`, and values that only a collection keeps.
#[test]
fn language_uses() {
    check_cases(&[
        (
            "var s = 0n; for (var i = 0n; i < 100n; i++) { s += i * i; } print(s)",
            "328350",
        ),
        (
            "var f = 1n; for (var i = 1n; i <= 30n; i++) f *= i; print(f, f % 1000000007n)",
            "265252859812191058636308480000000 109361473",
        ),
        (
            "var a = [3n, 1n, 2n]; a.sort((x, y) => (x < y ? -1 : x > y ? 1 : 0)); print(a.join())",
            "1,2,3",
        ),
        (
            "var m = new Map(); for (var i = 0; i < 100; i++) m.set(BigInt(i) * 2n ** 64n, i); \
             print(m.size, m.get(50n * 2n ** 64n), m.has(18446744073709551616n))",
            "100 50 true",
        ),
        (
            "function k(x) { switch (x) { case 1n: return 'one'; case 2n ** 70n: return 'big'; default: return 'other'; } } \
             print(k(1n), k(BigInt(2 ** 70)), k(1), k('1'))",
            "one big other other",
        ),
        (
            "var o = {}; o[2n ** 64n] = 'v'; print(o['18446744073709551616'], Object.keys(o).join())",
            "v 18446744073709551616",
        ),
        (
            "print(parseInt(12n), parseFloat(-3n), Number(-0n), Object.is(Number(0n), 0), 0n === -0n)",
            "12 -3 0 true true",
        ),
        (
            "var x = 5n; var y = x; x += 1n; print(x, y, x === 6n, typeof x)",
            "6 5 true bigint",
        ),
        (
            "print(1n < 2, 2 > 1n, 1n <= 1, -1n >= -1.5, 1n == 1, 1n != 1.5, 2n ** 53n + 1n > 2 ** 53)",
            "true true true true true true true",
        ),
    ]);
}
