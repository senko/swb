//! Small items of M7 feature 13d (ADR 0026 section 15): the table of
//! one-unit strings, the cache of property reads on primitive bases
//! (`String.prototype` and `Number.prototype` methods, `length` of
//! strings), the hash kept in the strings that `Map` and `Set` use as
//! keys, `toFixed` with its fast path, `shift` on dense arrays with a
//! start offset, and the values of `|` and the shifts on doubles. Every
//! case runs normally and in the GC stress mode. The expected outputs come
//! from Node.js 22 (a black box), with `print` for `console.log` and an
//! empty `gc`. (Generated from the scripts by running Node.js; edit the
//! cases and the expected text together.)

mod common;

use common::check_cases;

/// Strings of one code unit come from a table (`s[i]`, `charAt`, `at`, `split`, iteration, `fromCharCode`, `substring`, matches); they behave as other strings, and the table survives collections.
#[test]
fn one_unit_strings() {
    check_cases(UNITS);
}

/// A site that reads a property of a string or a number caches a method of the prototype object; a change of that object must be seen, and a site that sees other values must still work.
#[test]
fn primitive_property_sites() {
    check_cases(PRIMITIVE_SITES);
}

/// `Map` and `Set` keys that are strings: equal content in different strings, ropes, both widths, keys used again, deletion, collections.
#[test]
fn collection_string_keys() {
    check_cases(KEYS);
}

/// `toFixed` for halves, values near 1e21, negative zero, tiny values and many digits.
#[test]
fn to_fixed() {
    check_cases(FIXED);
}

/// `shift` with the start offset of the dense vector: queues, holes, prototype elements, frozen arrays, other operations after shifts, collections.
#[test]
fn shift_on_dense_arrays() {
    check_cases(SHIFT);
}

/// `|`, `&`, `^`, `~` and the shifts on doubles; `>>>` can exceed the int32 range.
#[test]
fn integer_results() {
    check_cases(BITWISE);
}

const UNITS: &[(&str, &str)] = &[
    (
        r"var s = 'héllo wörld';
print(s[1], s.charAt(1), s.at(1), s.at(-1), s[1] === s.charAt(1), s.charAt(1) === 'hé'[1]);
print(s.split('').join('|'), s.split('').length, s.substring(4, 5), s.slice(-1), s.substr(7, 1));
print([...'a😀b'].length, [...'a😀b'][1].length, [...'a😀b'][2], [...'x\ud800y'].map(function (c) { return c.length; }).join());
print(String.fromCharCode(65), String.fromCharCode(0xe9), String.fromCharCode(0x100), String.fromCharCode(65, 66), String.fromCharCode().length);
print('Āx'[0].charCodeAt(0), 'Āx'[1], 'Āx'[1] === 'x', 'ab'[2], 'ab'.at(2), 'ab'.charAt(2) === '');",
        r"é é é d true true
h|é|l|l|o| |w|ö|r|l|d 11 o d ö
3 2 b 1,1,1
A é Ā AB 0
256 x true undefined undefined true",
    ),
    (
        r"var o = {}; o['a'] = 1; o['abc'[0]] += 10; o['abc'[1]] = 2;
print(Object.keys(o).join(), o.a, o.b, ('abc'[0] + 'abc'[1]).length);
var m = new Map(); m.set('abc'[0], 1); m.set('a', 2); m.set('xyz'.charAt(0), 3);
print(m.size, m.get('a'), m.get('x'), new Set('abcabc').size, [...new Set('hello')].join(''));
var parts = 'a,b,c'.split(','); print(parts[0] === 'a', parts.join('') === 'abc', 'a-b-c'.split('-').map(function (p) { return p + p; }).join());",
        r"a,b 11 2 2
2 2 3 3 helo
true true aa,bb,cc",
    ),
    (
        r"var m = 'abcabc'.match(/b/g); print(m.length, m[0], m[1], m[0] === 'b');
var all = [...'a1b2'.matchAll(/\d/g)].map(function (x) { return x[0] + x.index; }); print(all.join());
print('abc'.replace(/(b)/, '[$1]'), 'abc'.replace('b', function (x) { return x + x; }), /(a)(b)?/.exec('ac')[1], /(a)(b)?/.exec('ac')[2]);
print('a b'.split(/(\s)/).join('|'), 'abc'.split(/(?:)/).join('|'), 'x'.padStart(3, 'ab'), 'x'.padEnd(4, 'ab'), 'abc'.trim().length);",
        r"2 b b true
11,23
a[b]c abbc a undefined
a| |b a|b|c abx xaba 3",
    ),
    (
        r"var keep = 'xyz'[1]; var other = String.fromCharCode(121);
gc(); gc();
print(keep, other, keep === other, 'xyz'[1] === keep, keep + keep);
var arr = []; for (var i = 0; i < 300; i++) { arr.push(String.fromCharCode(i)); }
gc(); var ok = 0; for (var i = 0; i < 300; i++) { if (arr[i].charCodeAt(0) === i && arr[i].length === 1) ok++; }
print(ok, arr[65], arr[255].charCodeAt(0), arr[256].charCodeAt(0), arr[299].length);
var s = 'The quick brown fox'; var out = ''; for (var c of s) { out = c + out; } print(out);
var wm = new WeakMap(); try { wm.set('a'[0], 1); } catch (e) { print(e.constructor.name); }",
        r"y y true true yy
300 A 255 256 1
xof nworb kciuq ehT
TypeError",
    ),
];

const PRIMITIVE_SITES: &[(&str, &str)] = &[
    (
        r"function len(x) { return x.length; }
print(len('abc'), len([1, 2]), len({ length: 7 }), len(new String('xy')), len(function (a, b) {}), len('abcd'), len(5), len('é'));
Number.prototype.length = 99; print(len(1), len('xyz')); delete Number.prototype.length;
Boolean.prototype.length = 5; print(len(true), len('q')); delete Boolean.prototype.length;
var rope = ''; for (var i = 0; i < 3000; i++) rope += 'ab'; print(len(rope));",
        r"3 2 7 2 2 4 undefined 1
99 3
5 1
6000",
    ),
    (
        r"function up(s) { return s.toUpperCase(); }
print(up('abc'), up('abc'), up('xyz'));
String.prototype.toUpperCase = function () { return 'patched ' + this; };
print(up('abc'));
delete String.prototype.toUpperCase;
try { print(up('abc')); } catch (e) { print(e.constructor.name); }
Object.prototype.toUpperCase = function () { return 'from Object ' + typeof this; };
print(up('abc'));
delete Object.prototype.toUpperCase;
Object.defineProperty(String.prototype, 'toUpperCase', { get: function () { return function () { return 'getter ' + this; }; }, configurable: true });
print(up('abc'));",
        r"ABC ABC XYZ
patched abc
TypeError
from Object object
getter abc",
    ),
    (
        r"function f(x) { return x.toString(); }
print(f('abc'), f(12), f(true), f(1.5), f('d'));
var saved = Number.prototype.toString;
Number.prototype.toString = function () { return 'N' + saved.call(this); };
print(f(12), f('abc'), f(1.5), f(true));
Number.prototype.toString = saved; print(f(12));
Object.defineProperty(Boolean.prototype, 'toString', { value: function () { return 'B'; }, writable: true, configurable: true });
print(f(true), f(false), f('s'));
function g(x) { return x.nothing; }
print(g('a'), g(1), g(true), g('a'));
String.prototype.nothing = 7; print(g('a'), g(1)); delete String.prototype.nothing;
Number.prototype.nothing = 8; print(g('a'), g(1), g(2.5)); delete Number.prototype.nothing;
print(g('a'), g(1));",
        r"abc 12 true 1.5 d
N12 abc N1.5 true
12
B B s
undefined undefined undefined undefined
7 undefined
undefined 8 8
undefined undefined",
    ),
    (
        r"function h(x) { return x.charAt(1); }
for (var i = 0; i < 3; i++) print(h('abc'));
Object.setPrototypeOf(String.prototype, { charAt: function () { return 'unreached'; }, extra: 1 });
print(h('abc'));
function e(x) { return x.extra; }
print(e('abc'), e('abc'));
Object.setPrototypeOf(String.prototype, Object.prototype);
print(e('abc'), h('xyz'));
Object.defineProperty(String.prototype, 'charAt', { value: function () { return 'frozen'; }, writable: false, configurable: true });
print(h('abc'));
function k(x) { return x[1]; } print(k('abc'), k('é'), k([5, 6]), k({ 1: 'o' }));
function nul(x) { return x.length; }
try { nul(null); } catch (e) { print(e.constructor.name); }
try { nul(undefined); } catch (e) { print(e.constructor.name); }",
        r"b
b
b
b
1 1
undefined y
frozen
b undefined 6 o
TypeError
TypeError",
    ),
];

const KEYS: &[(&str, &str)] = &[
    (
        r"var a = 'key' + 1, b = 'ke' + 'y1';
var m = new Map(); m.set(a, 'first'); print(m.get(b), m.has('key1'), m.get(a));
m.set(b, 'second'); print(m.size, m.get(a));
var long1 = 'x'.repeat(5000), long2 = ''; for (var i = 0; i < 5000; i++) long2 += 'x';
m.set(long1, 'L'); print(m.get(long2), m.size, m.has(long2 + 'y'));
var wide1 = 'ā' + 'a', wide2 = String.fromCharCode(0x101, 97); m.set(wide1, 'W'); print(m.get(wide2), m.get('a'));
var l1 = 'é', l2 = String.fromCharCode(233); m.set(l1, 'E'); print(m.get(l2), m.get('é'), m.size);
print(m.delete(a), m.has(b), m.size); m.set(a, 'again'); print(m.get(b), m.size);
var s = new Set(); for (var i = 0; i < 1000; i++) { s.add('k' + (i % 100)); } print(s.size, s.has('k99'), s.has('k100'));
var keys = []; for (var i = 0; i < 50; i++) keys.push('item' + i);
var big = new Map(); for (var round = 0; round < 3; round++) { for (var i = 0; i < keys.length; i++) { big.set(keys[i], (big.get(keys[i]) || 0) + 1); } }
print(big.size, big.get('item0'), big.get('item49'), [...big.keys()].slice(0, 3).join());",
        r"first true first
1 second
L 2 false
W undefined
E E 4
true false 3
again 4
100 true false
50 3 3 item0,item1,item2",
    ),
    (
        r"var m = new Map(); var objs = [];
for (var i = 0; i < 200; i++) { var k = 'n' + i; m.set(k, i); objs.push(k); }
gc(); gc();
var sum = 0; for (var i = 0; i < 200; i++) { sum += m.get(objs[i]) + m.get('n' + i); } print(sum, m.size);
for (var i = 0; i < 200; i += 2) { m.delete('n' + i); } gc(); print(m.size, m.has('n0'), m.has('n1'), [...m.keys()].slice(0, 3).join());
var set = new Set(['a', 'b', 'a'.concat('')]); print(set.size, [...set].join());
var weird = new Map([['', 1], ['0', 2], [0, 3], ['-0', 4], [-0, 5], ['NaN', 6], [NaN, 7]]); print(weird.size, weird.get(''), weird.get('0'), weird.get(0), weird.get('NaN'), weird.get(NaN));
var groups = {}; for (var w of 'the quick brown the lazy the end'.split(' ')) { groups[w] = (groups[w] || 0) + 1; } print(JSON.stringify(groups));
var cnt = new Map(); for (var w of 'the quick brown the lazy the end'.split(' ')) { cnt.set(w, (cnt.get(w) || 0) + 1); } print([...cnt].join(';'));",
        r#"39800 200
100 false true n1,n3,n5
2 a,b
6 1 2 5 6 7
{"the":3,"quick":1,"brown":1,"lazy":1,"end":1}
the,3;quick,1;brown,1;lazy,1;end,1"#,
    ),
];

const FIXED: &[(&str, &str)] = &[
    (
        r"var xs = [0, -0, 0.5, 1.5, 2.5, -2.5, 1.005, 1.45, 8.345, 10.235, 123.456, 0.000001, 1e-7, 5e-324, 1e20, 1e21, 123456789012345680000, 0.1 + 0.2, 99.995, 0.045, 1.255, 2 ** 53, 4503599627370497.5, 1e-10, 0.9999999999999999, -1e-10];
for (var f of [0, 1, 2, 3, 10, 20]) { print(f, xs.map(function (x) { return x.toFixed(f); }).join(' ')); }",
        r"0 0 0 1 2 3 -3 1 1 8 10 123 0 0 0 100000000000000000000 1e+21 123456789012345683968 0 100 0 1 9007199254740992 4503599627370498 0 1 -0
1 0.0 0.0 0.5 1.5 2.5 -2.5 1.0 1.4 8.3 10.2 123.5 0.0 0.0 0.0 100000000000000000000.0 1e+21 123456789012345683968.0 0.3 100.0 0.0 1.3 9007199254740992.0 4503599627370498.0 0.0 1.0 -0.0
2 0.00 0.00 0.50 1.50 2.50 -2.50 1.00 1.45 8.35 10.23 123.46 0.00 0.00 0.00 100000000000000000000.00 1e+21 123456789012345683968.00 0.30 100.00 0.04 1.25 9007199254740992.00 4503599627370498.00 0.00 1.00 -0.00
3 0.000 0.000 0.500 1.500 2.500 -2.500 1.005 1.450 8.345 10.235 123.456 0.000 0.000 0.000 100000000000000000000.000 1e+21 123456789012345683968.000 0.300 99.995 0.045 1.255 9007199254740992.000 4503599627370498.000 0.000 1.000 -0.000
10 0.0000000000 0.0000000000 0.5000000000 1.5000000000 2.5000000000 -2.5000000000 1.0050000000 1.4500000000 8.3450000000 10.2350000000 123.4560000000 0.0000010000 0.0000001000 0.0000000000 100000000000000000000.0000000000 1e+21 123456789012345683968.0000000000 0.3000000000 99.9950000000 0.0450000000 1.2550000000 9007199254740992.0000000000 4503599627370498.0000000000 0.0000000001 1.0000000000 -0.0000000001
20 0.00000000000000000000 0.00000000000000000000 0.50000000000000000000 1.50000000000000000000 2.50000000000000000000 -2.50000000000000000000 1.00499999999999989342 1.44999999999999995559 8.34500000000000063949 10.23499999999999943157 123.45600000000000306954 0.00000100000000000000 0.00000010000000000000 0.00000000000000000000 100000000000000000000.00000000000000000000 1e+21 123456789012345683968.00000000000000000000 0.30000000000000004441 99.99500000000000454747 0.04499999999999999833 1.25499999999999989342 9007199254740992.00000000000000000000 4503599627370498.00000000000000000000 0.00000000010000000000 0.99999999999999988898 -0.00000000010000000000",
    ),
    (
        r"print((25).toFixed(2), (1000000).toFixed(0), (0.000123).toFixed(8), (-1.5).toFixed(0), (-0.5).toFixed(0), (0.5).toFixed(0), (1.45).toFixed(1), (0.05).toFixed(1));
print((1e21).toFixed(2), (-1e21).toFixed(2), (1.7976931348623157e308).toFixed(2).length, (123.456).toFixed(50), (0.1).toFixed(100).length, (5e-324).toFixed(100).length);
print(NaN.toFixed(2), Infinity.toFixed(2), (-Infinity).toFixed(0), (3).toFixed(), (3.7).toFixed(undefined), (2.345).toFixed('2'), (1.5).toFixed(1.9));
try { (1).toFixed(101); } catch (e) { print(e.constructor.name, e.message); }
try { (1).toFixed(-1); } catch (e) { print(e.constructor.name); }
print(new Number(2.5).toFixed(0), Number.prototype.toFixed.call(1.005, 2));
for (var i = 0; i < 20; i++) { var v = i * 1.37 + 0.005; if (v.toFixed(2) !== (Math.round(v * 100) / 100).toFixed(2) && i < 3) print('diff', i); }
print((1234.5678).toFixed(2), (0.30000000000000004).toFixed(17), (1 / 3).toFixed(20), (2 / 3).toFixed(20), (1e-20).toFixed(20), (9.5).toFixed(0), (0.95).toFixed(1));",
        r"25.00 1000000 0.00012300 -2 -1 1 1.4 0.1
1e+21 -1e+21 23 123.45600000000000306954461848363280296325683593750000 102 102
NaN Infinity -Infinity 3 4 2.35 1.5
RangeError toFixed() digits argument must be between 0 and 100
RangeError
3 1.00
1234.57 0.30000000000000004 0.33333333333333331483 0.66666666666666662966 0.00000000000000000001 10 0.9",
    ),
];

const SHIFT: &[(&str, &str)] = &[
    (
        r"var a = []; for (var i = 0; i < 100; i++) a.push(i);
var out = []; for (var i = 0; i < 30; i++) out.push(a.shift());
print(out.join(), a.length, a[0], a[69], a[70], a.indexOf(30), a.indexOf(5), a.slice(0, 3).join());
for (var i = 0; i < 50; i++) a.shift(); print(a.length, a.join());
a.push(100, 101); print(a.length, a.join(), a.pop(), a.shift(), a.length);
while (a.length) a.shift(); print(a.length, a.shift(), a.length, JSON.stringify(a)); a.push(1); print(a[0], a.length);",
        r"0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29 70 30 99 undefined 0 -1 30,31,32
20 80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99
22 80,81,82,83,84,85,86,87,88,89,90,91,92,93,94,95,96,97,98,99,100,101 101 80 20
0 undefined 0 []
1 1",
    ),
    (
        r"var q = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25];
for (var i = 0; i < 10; i++) q.shift();
q.unshift('u1', 'u2'); print(q.join(), q.length);
q.splice(1, 2, 'a', 'b', 'c'); print(q.join(), q.length);
q.shift(); q.shift(); q.reverse(); print(q.join()); q.sort(); print(q.join());
q.shift(); print(q.concat([0]).join(), q.slice(2, 5).join(), q.map(function (x) { return x + '!'; }).slice(0, 2).join(), JSON.stringify(q.slice(0, 3)));
q.length = 3; print(q.join(), q.length); q.shift(); print(q.join(), q.length); q.length = 0; print(q.length, q.shift(), q.length);
var r = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]; for (var i = 0; i < 12; i++) r.shift();
r[10] = 'ten'; print(r.length, r.join('|'), Object.keys(r).join(), 5 in r, 20 in r, r.hasOwnProperty(10));
var fo = []; for (var k in r) fo.push(k); print(fo.join());
r.fill(0, 1, 3); r.copyWithin(0, 5, 7); print(r.join('|'));",
        r"u1,u2,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25 17
u1,a,b,c,12,13,14,15,16,17,18,19,20,21,22,23,24,25 18
25,24,23,22,21,20,19,18,17,16,15,14,13,12,c,b
12,13,14,15,16,17,18,19,20,21,22,23,24,25,b,c
13,14,15,16,17,18,19,20,21,22,23,24,25,b,c,0 15,16,17 13!,14! [13,14,15]
13,14,15 3
14,15 2
0 undefined 0
11 12|13|14|15|16|17|18|19|||ten 0,1,2,3,4,5,6,7,10 true false true
0,1,2,3,4,5,6,7,10
17|18|0|15|16|17|18|19|||ten",
    ),
    (
        r"var h = [1, , 3, , 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
print(h.shift(), h.length, 0 in h, 1 in h, h.shift(), h.length, 0 in h, h.join('|'), Object.keys(h).length);
var t = [1, 2, 3, , ,]; print(t.length, t.shift(), t.length, t.shift(), t.shift(), t.length, t.shift(), t.length, t.shift(), t.length);
Array.prototype[1] = 'proto'; var p = [10, , 30]; print(p.shift(), p.length, p[0], p[1], p.join()); delete Array.prototype[1];
var z = [, 1, 2]; print(z.shift(), z.length, z[0], 0 in z);
var obj = { length: 3, 0: 'a', 1: 'b', 2: 'c' }; print(Array.prototype.shift.call(obj), obj.length, obj[0], obj[1], 2 in obj);
var frozen = Object.freeze([1, 2, 3]); try { frozen.shift(); } catch (e) { print(e.constructor.name); } print(frozen.join());
var lenfix = [1, 2, 3]; Object.defineProperty(lenfix, 'length', { writable: false }); try { lenfix.shift(); } catch (e) { print(e.constructor.name); } print(lenfix.join(), lenfix.length);
var sealed = Object.seal([1, 2, 3]); try { print(sealed.shift()); } catch (e) { print(e.constructor.name); } print(sealed.join());
var ne = Object.preventExtensions([1, 2, 3]); print(ne.shift(), ne.join(), ne.length);",
        r"1 19 false true undefined 18 true 3||5|6|7|8|9|10|11|12|13|14|15|16|17|18|19|20 17
5 1 4 2 3 2 undefined 1 undefined 0
10 2 proto 30 proto,30
undefined 2 1 true
a 2 b c false
TypeError
1,2,3
TypeError
2,3, 3
TypeError
2,3,3
1 2,3 2",
    ),
    (
        r"var objs = []; for (var i = 0; i < 40; i++) objs.push({ id: i, tag: 'o' + i });
var taken = []; for (var i = 0; i < 25; i++) { taken.push(objs.shift()); if (i % 7 === 0) gc(); }
gc(); gc();
print(taken.length, taken[0].id, taken[24].tag, objs.length, objs[0].id, objs[14].tag);
var held = objs[3]; while (objs.length > 5) objs.shift(); gc(); print(objs.length, objs[0].id, held.id, objs.indexOf(held), objs.map(function (o) { return o.id; }).join());
var q = [{ n: 1 }]; var seen = 0; while (q.length && seen < 200) { var cur = q.shift(); seen++; if (cur.n < 60) { q.push({ n: cur.n * 2 }, { n: cur.n * 2 + 1 }); } if (seen % 20 === 0) gc(); } print(seen, q.length);
var strs = []; for (var i = 0; i < 30; i++) strs.push('s' + i); for (var i = 0; i < 20; i++) strs.shift(); gc(); print(strs.join());
var shifted = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32];
for (var i = 0; i < 20; i++) shifted.shift(); shifted.push(33, 34); gc(); print(shifted.join(), shifted.length);",
        r"25 0 o24 15 25 o39
5 35 28 -1 35,36,37,38,39
119 0
s20,s21,s22,s23,s24,s25,s26,s27,s28,s29
21,22,23,24,25,26,27,28,29,30,31,32,33,34 14",
    ),
    (
        r"var n = 100000, a = []; for (var i = 0; i < n; i++) a.push(i);
var sum = 0; while (a.length) sum += a.shift(); print(sum, a.length);
var b = []; var total = 0; for (var i = 0; i < 20000; i++) { b.push(i, i + 1); total += b.shift(); if (i % 3 === 0) total += b.shift(); } print(total, b.length, b[0]);
var c = []; for (var i = 0; i < 1000; i++) { c.push(i); } for (var i = 0; i < 990; i++) { c.shift(); } print(c.join(), c.length);",
        r"4999950000 0
177782222 13333 13334
990,991,992,993,994,995,996,997,998,999 10",
    ),
    (
        r"var a = []; for (var i = 0; i < 64; i++) a.push(i);
for (var i = 0; i < 40; i++) a.shift();
Object.defineProperty(a, 5, { get: function () { return 'getter'; }, configurable: true, enumerable: true });
print(a.length, a[5]);
try { a.shift(); } catch (e) { print(e.constructor.name); }
print(a.length, a[4], a[5], a[0]);
var s = []; for (var i = 0; i < 64; i++) s.push(i); for (var i = 0; i < 30; i++) s.shift();
s[100] = 'far'; print(s.length, s.shift(), s[99], s.length); s.length = 4; print(s.join());
var u = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18]; for (var i = 0; i < 17; i++) u.shift(); u.unshift(0); print(u.join(), u.length);
u.shift(); u.shift(); print(u.length, u.shift(), u.length);
var args = (function () { return arguments; })(1, 2, 3); print(Array.prototype.shift.call(args), args.length, args[0]);",
        r"24 getter
TypeError
24 getter getter 41
101 30 far 100
31,32,33,34
0,18 2
0 undefined 0
1 2 2",
    ),
];

const BITWISE: &[(&str, &str)] = &[(
    r"var x = 1, a = [];
for (var i = 0; i < 6; i++) { x = (x * 1103515245 + 12345) % 2147483648; a.push(x | 0); }
print(a.join(), a.map(function (v) { return v & 255; }).join(), a.every(function (v) { return Number.isInteger(v); }));
print(Object.is((-0.5) | 0, 0), Object.is((-0) | 0, 0), NaN | 0, Infinity | 0, 1e20 | 0, 2 ** 32 | 0, (2 ** 31) | 0, -(2 ** 31) - 1 | 0, 4294967296.5 | 0, 1.9 | 0, -1.9 | 0);
print(1.5 << 1, 2 ** 31 << 0, 1e10 << 1, (-1.5) >> 1, 2 ** 32 + 5 >> 1, 2 ** 31 + 0.5 >>> 0, -1.5 >>> 0, -1 >>> 0, 1e10 >>> 3, 5.5 & 3.5, 5.5 ^ 1.2, ~5.5, ~-1.5, ~~7.9);
var d = [0.5, 1.5, -2.5, 3000000000.7, -3000000000.7, 2147483647.9, 2147483648.1];
print(d.map(function (v) { return v | 0; }).join(), d.map(function (v) { return v >>> 0; }).join(), d.map(function (v) { return v >> 1; }).join(), d.map(function (v) { return v << 2; }).join());
var arr = [1.5, 2.5]; arr[0] = arr[0] | 0; arr[1] = arr[1] | 0; print(arr.join(), arr[0] + arr[1], arr.indexOf(1), arr.indexOf(2), arr.includes(2));
var m = new Map(); m.set(5.5 | 0, 'five'); print(m.get(5), m.get(5.0), new Set([1.5 | 0, 1, 1.0]).size, [10, 20, 30][1.9 | 0], 'abc'[2.2 | 0]);
var idx = (7.8 | 0); var o = {}; o[idx] = 'x'; print(Object.keys(o).join(), o[7], Math.max(2.5 | 0, 1), (2.5 | 0) === 2, (2.5 | 0) / 2, 7.5 % 2 | 0, (1e300 * 1e10) | 0);",
    r"1103527590,377401600,333417792,314102912,611429056,1995203584 166,0,64,128,192,0 true
true true 0 0 1661992960 0 -2147483648 2147483647 0 1 -1
2 -2147483648 -1474836480 -1 2 2147483648 4294967295 4294967295 176258176 1 4 -6 0 7
0,1,-2,-1294967296,1294967296,2147483647,-2147483648 0,1,4294967294,3000000000,1294967296,2147483647,2147483648 0,0,-1,-647483648,647483648,1073741823,-1073741824 0,4,-8,-884901888,884901888,-4,0
1,2 3 0 1 true
five five 1 20 c
7 x 2 true 1 1 0",
)];
