//! Ropes (M7 feature 5a, ADR 0026 section 7): concatenations in loops,
//! the paths that read the code units of a rope (indices, keys, numbers,
//! eval, String objects, comparisons), each run normally and in the GC
//! stress mode. The expected outputs come from Node.js 22 (a black box),
//! with `print` as in `tests/common`.

mod common;

use common::{check_cases, on_big_stack, run_in, runtime};
use swb_js::{Gc, JsString, Runtime, Value};

#[test]
fn ropes() {
    check_cases(ROPES);
}

/// The string value of a global variable.
fn global_string(rt: &mut Runtime, name: &str) -> Gc<JsString> {
    match rt.eval(name) {
        Ok(Value::String(string)) => string,
        other => panic!("{name} is not a string: {other:?}"),
    }
}

#[test]
fn length_reads_no_flatten_and_an_index_flattens_once() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let build =
            "var s = ''; for (var i = 0; i < 100000; i++) s += 'abc'; var t = s + s; undefined;";
        assert_eq!(run_in(&mut rt, build), "");
        assert_eq!(
            run_in(&mut rt, "print(s.length, t.length)"),
            "300000 600000"
        );
        let s = global_string(&mut rt, "s");
        assert!(rt.heap().string(s).unwrap().is_rope(), "length flattened");
        assert!(rt.heap().string(s).unwrap().is_latin1());
        assert_eq!(run_in(&mut rt, "print(s[0], s[299999])"), "a c");
        assert!(!rt.heap().string(s).unwrap().is_rope(), "an index flattens");
        // `t` holds `s` twice and is still a rope; its own index read
        // flattens it, and its children (the flat `s`) stay alive.
        let t = global_string(&mut rt, "t");
        assert!(rt.heap().string(t).unwrap().is_rope());
        assert_eq!(
            run_in(&mut rt, "print(t[300000], t === s + s, s.length)"),
            "a true 300000"
        );
        assert!(!rt.heap().string(t).unwrap().is_rope());
        assert_eq!(rt.heap().stats().total_stale_roots, 0);
    });
}

#[test]
fn a_rope_key_is_flat_and_interned() {
    on_big_stack(|| {
        let mut rt = runtime(true);
        let source = "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var key = r + 'z'; \
                      var o = {}; o[key] = 1; o[r + 'z'] += 1; print(o[key]);";
        assert_eq!(run_in(&mut rt, source), "2");
        let key = global_string(&mut rt, "key");
        let record = rt.heap().string(key).unwrap();
        assert!(!record.is_rope() && record.is_atom());
        assert_eq!(rt.heap().stats().total_stale_roots, 0);
    });
}

const ROPES: &[(&str, &str)] = &[
    (
        "var s = ''; for (var i = 0; i < 1000; i++) s += 'ab'; print(s.length, s[0], s[1999], s[2000], s[-1])",
        "2000 a b undefined undefined",
    ),
    (
        "var s = ''; for (var i = 0; i < 2200; i++) s = i % 2 ? 'x' + s : s + 'y'; var t = ''; for (var i = 0; i < 1100; i++) t += 'x'; for (var i = 0; i < 1100; i++) t += 'y'; print(s === t, s == t, s < t, s > t, s <= t, s.length, s[0], s[2199])",
        "true true false false true 2200 x y",
    ),
    (
        "var a = ''; for (var i = 0; i < 1100; i++) a += 'a'; var b = a + '\\u03b1'; var c = '\\u03b2' + a; print(b.length, b[300] === '\\u03b1', (b + c)[302] === 'a', (b + c)[301] === '\\u03b2', b === a + '\\u03b1', b < c)",
        "1101 false true false true true",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var q = r + 'a'; var w = r + 'b'; print(q < w, w < q, q < r, r < q, q + 'z' < w, q >= q, q === r + 'a')",
        "true false false true true true true",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var o = {}; o[r + 1] = 'one'; print(o[r + '1'], (r + 1) in o, Object.keys(o)[0].length, o[r + 2])",
        "one true 1101 undefined",
    ),
    (
        "var n = ''; for (var i = 0; i < 1100; i++) n += ' '; n += '42'; for (var i = 0; i < 1100; i++) n += ' '; print(Number(n), +n, n * 2, parseInt(n), parseFloat(n), n == 42, n < 43, n - 1)",
        "42 42 84 42 42 true true 41",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var w = new String(r + 'x'); print(w.length, w[0], w[300], typeof w, Object.keys(w).length, Object(r).length, w == r + 'x')",
        "1101 k k object 1101 1100 true",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var t = `${r}-${r}`; print(t.length, t[300], [r, r].join('-').length, [r + r, 'z'].join(r).length)",
        "2201 k 2201 3301",
    ),
    (
        "var code = 'var zz = 1'; for (var i = 0; i < 1100; i++) code += ' + 1'; eval(code); print(zz, (0, eval)(code + ';zz'))",
        "1101 1101",
    ),
    (
        "var body = 'return 0'; for (var i = 0; i < 1100; i++) body += ' + 1'; print(Function(body)(), Function('a', body + ' + a')(5))",
        "1100 1105",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; try { throw new Error(r + r); } catch (e) { print(e.message.length, String(e).length, typeof e.stack) }",
        "2200 2207 string",
    ),
    (
        "var s = 'a'; var n = 0; try { for (;;) { s += s; n++; } } catch (e) { print(n, s.length, e.name, e.message) }",
        "28 268435456 RangeError Invalid string length",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var kx = r + 'x'; var kx2 = ''; for (var i = 0; i < 1100; i++) kx2 += 'k'; kx2 += 'x'; switch (kx) { case kx2: print('match'); break; default: print('no'); }",
        "match",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; print(String(Symbol(r)).length, Symbol(r + 'd').description === undefined)",
        "1108 false",
    ),
    (
        "var s = ''; for (var i = 0; i < 5000; i++) { s += 'abc'; if (i % 1000 === 0 && typeof gc === 'function') gc(); } print(s.length, s[14999], s[3000]); if (typeof gc === 'function') gc(); print(s + s === s + s, (s + s).length)",
        "15000 c a\ntrue 30000",
    ),
    (
        "var c = ''; for (var i = 0; i < 1030; i++) c += 'c'; console.log(c + 'd'); console.log([c + 'd'].length, { k: c + 'e' }.k.length)",
        "ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccd\n1 1031",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var q = r + 'a'; print([q].indexOf(r + 'a'), [r, q].indexOf(r + 'a'), Object.is(q, r + 'a'), [r + 'b'].indexOf(q))",
        "0 1 true -1",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var f = { [r + 'f']: function () {} }[r + 'f']; print(f.name.length, f.name[300])",
        "1101 k",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var o = {}; for (var i = 0; i < 2000; i++) { o[r + (i % 10)] = i; } print(Object.keys(o).length, o[r + 9])",
        "10 1999",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var q = r + 'a'; print(!!q, typeof q, q ? 1 : 2, !(r + ''), q.length === 301)",
        "true string 1 false false",
    ),
    (
        "var p = ''; for (var i = 0; i < 1100; i++) p += String.fromCharCode === undefined ? 'x' : 'y'; var x = p + p; var hits = 0; for (var i = 0; i < 2200; i++) if (x[i] === p[i % 1100]) hits++; print(hits)",
        "2200",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var big = r + r + r; for (var k in new String(big)) { if (k === '3299') print('last', big[3299]); } print(big.length)",
        "last k\n3300",
    ),
    (
        "var r = ''; for (var i = 0; i < 1100; i++) r += 'k'; var s = r; for (var i = 0; i < 1500; i++) s = s + r; print(s.length, s[1651099], s === s + '', s.length === r.length * 1501)",
        "1651100 k true true",
    ),
    (
        "var piece = ''; for (var i = 0; i < 1100; i++) piece += 'p'; var s = ''; for (var i = 0; i < 2000; i++) s = (i % 2 ? piece + s : s + piece) + i % 10; print(s.length, s[300], s[s.length - 1])",
        "2202000 p 9",
    ),
];
