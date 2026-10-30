//! The fast paths of M8 feature 11a (ADR 0026 section 16): element reads
//! (`a[i]` and `a[0]` of objects with ordinary elements, also past the end
//! and at holes, with and without elements, getters, proxies, typed arrays
//! and String objects on the prototype chain, arguments objects), `split`
//! (the short path, the cache of recent subjects), the plain-array checks
//! of `concat`, `slice`, `filter` and `map` (`Symbol.species`,
//! `constructor`, `Symbol.isConcatSpreadable`, subclasses, prototype
//! elements), `filter(Boolean)`, `join` of one element, and the shapes of
//! new ordinary functions. Every case runs normally and in the GC stress
//! mode. The expected outputs come from Node.js 22 (a black box), with
//! `print` for `console.log` (arguments joined with spaces after
//! `String`). (Generated from the scripts by running Node.js; edit the
//! cases and the expected text together.)

mod common;

use common::check_cases;

/// Element reads of arrays: in range, holes, past the end, elements and getters added to and removed from the prototypes.
#[test]
fn element_reads() {
    check_cases(&INDEX[..1]);
}

/// Element reads through other chains: a proxy, an array, a String object or a typed array as prototype, typed arrays, arguments objects, index-like string keys, a long chain, frozen and sparse arrays.
#[test]
fn element_reads_on_other_chains() {
    check_cases(&INDEX[1..]);
}

/// `split` of subjects seen again (the cache), with limits, many pieces, long subjects, both widths, empty strings and conversions.
#[test]
fn split_paths() {
    check_cases(SPLIT);
}

/// `concat`, `slice`, `filter` and `map` with the plain-array checks: subclasses, an own or inherited `constructor` (also deleted, or an accessor that counts its calls), a changed `Symbol.species` (also a counting getter), `Symbol.isConcatSpreadable` on an array or a prototype (also getters added after the cache was filled), holes with prototype elements, a reduced `length`, sealed arrays; `filter(Boolean)` (also with a replaced global `Boolean`) and `join` of one element (also of array-likes and strings).
#[test]
fn plain_array_methods() {
    check_cases(SPECIES);
}

/// New ordinary functions get their properties in the order and with the attributes of the definitions; later changes stay on their own function; array literals with holes and spreads.
#[test]
fn function_shapes_and_literals() {
    check_cases(CLOSURES);
}

const INDEX: &[(&str, &str)] = &[
    (
        r"var a = [1, , 3]; var i = 0; var r = [];
for (i = 0; i < 5; i++) r.push(String(a[i]));
print(r.join(), a[0], a[1], a[2], a[3], a[-1], a[1.5]);
var parts = 'en'.split('-'); print(parts[0], parts[1], parts[2], 'en-US'.split('-')[1]);
Array.prototype[1] = 'AP'; Object.prototype[3] = 'OP';
r = []; for (i = 0; i < 5; i++) r.push(String(a[i]));
print(r.join(), a[1], a[3], parts[1], parts[3], [][1], ({})[3], ({ length: 0 })[1]);
delete Array.prototype[1]; delete Object.prototype[3];
r = []; for (i = 0; i < 5; i++) r.push(String(a[i]));
print(r.join(), a[1], a[3], parts[1]);
Object.defineProperty(Array.prototype, 2, { get: function () { return 'G' + this.length; }, configurable: true });
print(a[2], [7][2], [7, 8, 9][2], parts[2]);
delete Array.prototype[2];
print([7][2], parts[2]);",
        r"1,undefined,3,undefined,undefined 1 undefined 3 undefined undefined undefined
en undefined undefined US
1,AP,3,OP,undefined AP OP AP OP AP OP undefined
1,undefined,3,undefined,undefined undefined undefined undefined
3 G1 9 G1
undefined undefined",
    ),
    (
        r"var p = new Proxy([], { get: function (t, k) { return typeof k === 'string' ? 'P' + k : undefined; } });
var a = [1, 2]; Object.setPrototypeOf(a, p);
var k = 3; print(a[0], a[1], a[2], a[k], a.length);
var o = Object.create([10, 20, 30]); print(o[0], o[2], o[3], o[k]);
var s = new String('xyz'); var c = Object.create(s); print(c[0], c[2], c[3], s[k - 1], 'xyz'[k - 1], 'xyz'[k]);
var t = new Float64Array([1.5, 2.5]); print(t[0], t[1], t[2], t[k]);
var tp = Object.create(new Int8Array(2)); print(tp[0], tp[1], tp[5], tp[k]);
function f(x, y) { return [arguments[0], arguments[1], arguments[2], arguments[k]].join('|'); }
function g(x, y) { 'use strict'; x = 9; return [arguments[0], arguments[1], arguments[2]].join('|'); }
function h(x) { x = 5; return arguments[0]; }
print(f(1), f(1, 2, 3, 4), g(1, 2), h(1), h());
var table = { '419': 'latam', 'US': 'us' }; var keys = '419-US-150'.split('-');
print(table[keys[0]], table[keys[1]], table[keys[2]], table['419'], table[419]);
var deep = [1]; var chain = deep; for (var n = 0; n < 12; n++) { chain = Object.create(chain); }
print(chain[0], chain[1], chain[k]);
Object.prototype[k] = 'deep'; print(chain[k], deep[k]); delete Object.prototype[k];
var frozen = Object.freeze([1, 2]); print(frozen[0], frozen[1], frozen[2]);
var sparse = []; sparse[100000] = 'far'; print(sparse[100000], sparse[5], sparse.length);",
        r"1 2 P2 P3 2
10 30 undefined undefined
x z undefined z z undefined
1.5 2.5 undefined undefined
0 0 undefined undefined
1||| 1|2|3|4 1|2| 5 undefined
latam us undefined latam latam
1 undefined undefined
deep deep
1 2 undefined
far undefined 100001",
    ),
];

const SPLIT: &[(&str, &str)] = &[(
    r"var rules = ['en-*-*', 'es-*-$americas', '*-*-*', 'zh-Hant-TW', 'a', '', '-', 'x--y-'];
var out = [];
for (var round = 0; round < 4; round++) {
  for (var i = 0; i < rules.length; i++) {
    var p = rules[i].split('-');
    out.push(p.length + ':' + p.join('|'));
    p.push('mutated'); p[0] = 'changed';
  }
}
print(out.slice(0, 8).join(' '));
print(out.slice(24).join(' '));
print('a-b-c'.split('-', 2).join('|'), 'a-b-c'.split('-', 0).length, 'a-b-c'.split('-', 2).join('|'), 'a-b-c'.split('-').join('|'));
var many = []; for (i = 0; i < 40; i++) many.push(i); var text = many.join('-');
for (round = 0; round < 3; round++) { var m = text.split('-'); print(m.length, m[0], m[39], m[17]); }
var long = 'ab-'.repeat(600); print(long.split('-').length, long.split('-')[599], long.split('-')[600] === '');
print('Ā-ā-x'.split('-').join('|'), 'aĀb'.split('Ā').join('|'), 'abc'.split('abcd').length);
print('aXXbXXc'.split('XX').join('|'), 'XX'.split('XX').length, ''.split('').length, ''.split('x').length);
var subjects = []; for (i = 0; i < 600; i++) subjects.push('k' + i + '-v' + i);
var sum = 0; for (round = 0; round < 3; round++) for (i = 0; i < subjects.length; i++) sum += subjects[i].split('-')[1].length;
print(sum);
var obj = { toString: function () { return 'p-q'; } }; print(String.prototype.split.call(obj, '-').join('|'), 'p-q'.split({ toString: function () { return '-'; } }).join('|'));",
    r"3:en|*|* 3:es|*|$americas 3:*|*|* 3:zh|Hant|TW 1:a 1: 2:| 4:x||y|
3:en|*|* 3:es|*|$americas 3:*|*|* 3:zh|Hant|TW 1:a 1: 2:| 4:x||y|
a|b 0 a|b a|b|c
40 0 39 17
40 0 39 17
40 0 39 17
601 ab true
Ā|ā|x a|b 1
a|b|c 2 0 1
6870
p|q p|q",
)];

const SPECIES: &[(&str, &str)] = &[
    (
        r"function show(a) { return (Array.isArray(a) ? 'A' : 'O') + a.length + '[' + Array.prototype.join.call(a, ',') + ']' + (a.constructor === Array ? '' : ':' + (a.constructor && a.constructor.name)); }
var a = [1, , 3];
print(show(a.concat([4, , ], 5)), show([].concat(a)), show(a.slice(0, 5)), show(a.filter(Boolean)), show(a.map(function (x) { return x * 2; })));
class Sub extends Array {}
var s = Sub.from([1, 2, 3]);
print(show(s.concat([4])), show(s.slice(1)), show(s.filter(Boolean)), show(s.map(function (x) { return x; })));
var b = [1, 2]; b.constructor = function Other(n) { this.length = 0; };
b.constructor[Symbol.species] = b.constructor;
print(show(b.concat([3])), show(b.slice(0)));
var c = [5, 6]; Object.defineProperty(c, 'constructor', { get: function () { print('get constructor'); return Array; } });
print(show(c.concat([7])), show(c.slice(1)));
var saved = Array.prototype.constructor;
Array.prototype.constructor = function Weird() {}; Array.prototype.constructor[Symbol.species] = Sub;
print(show([1].concat([2])), show([1, 2].slice(1)), show([0, 1].filter(Boolean)));
Array.prototype.constructor = saved;
print(show([1].concat([2])), show([1, 2].slice(1)));
var desc = Object.getOwnPropertyDescriptor(Array, Symbol.species);
Object.defineProperty(Array, Symbol.species, { get: function () { return Sub; }, configurable: true });
print(show([1].concat([2])), show([3, 4].slice(0)), show([0, 5].filter(Boolean)));
Object.defineProperty(Array, Symbol.species, desc);
print(show([1].concat([2])), show([3, 4].slice(0)));
var spread = [8, 9]; spread[Symbol.isConcatSpreadable] = false; print(show([1].concat(spread)).length, [1].concat(spread)[1] === spread);
Array.prototype[Symbol.isConcatSpreadable] = false; print([1].concat([2]).length, [1].concat([2])[0].length);
delete Array.prototype[Symbol.isConcatSpreadable];
Object.prototype[Symbol.isConcatSpreadable] = false; print([1].concat([2]).length); delete Object.prototype[Symbol.isConcatSpreadable];
var like = { length: 2, 0: 'x', 1: 'y' }; like[Symbol.isConcatSpreadable] = true; print(show([0].concat(like)));
Array.prototype[1] = 'H'; print(show([0, , 2].concat([, 4])), show([, 'b'].slice(0)), show([0, , 2].filter(Boolean)), [, ].join('+'), [, 'x'].join('-'));
delete Array.prototype[1];
print(show(['only'].filter(Boolean)), ['one'].join('-'), [null].join(), [undefined].join(), [].join(), [5].join(), [{ toString: function () { return 'T'; } }].join());
var Bool = Boolean; print(show([0, 'a', '', 1, null, NaN, {}].filter(Bool)), show([1, 0].filter(function (x) { return x; })));",
        r"A6[1,,3,4,,5] A3[1,,3] A3[1,,3] A2[1,3] A3[2,,6]
A4[1,2,3,4]:Sub A2[2,3]:Sub A3[1,2,3]:Sub A3[1,2,3]:Sub
O3[1,2,3]:Other O2[1,2]:Other
get constructor
get constructor
A3[5,6,7] A1[6]
A2[1,2]:Sub A1[2]:Sub A1[1]:Sub
A2[1,2] A1[2]
A2[1,2]:Sub A2[3,4]:Sub A1[5]:Sub
A2[1,2] A2[3,4]
9 true
2 1
2
A3[0,x,y]
A5[0,H,2,,4] A2[,b] A2[H,2]  -x
A1[only] one    5 T
A3[a,1,[object Object]] A1[1]",
    ),
    (
        r"var B = Boolean; Boolean = function (x) { return x == 1; };
print([0, 1, 2].filter(Boolean).join(), [0, 1, 2].filter(B).join());
Boolean = B;
print([0, 1, 2].filter(Boolean).join());
var ap = Array.prototype, ctor = ap.constructor;
delete ap.constructor;
print([1, 2].slice(1).length, [3].concat([4]).length, [1, 0].filter(Boolean).length);
var calls = 0, orig = Object.getOwnPropertyDescriptor(ap, 'constructor') || { value: ctor, writable: true, configurable: true, enumerable: false };
Object.defineProperty(ap, 'constructor', { get: function () { calls++; return Array; }, configurable: true });
[1, 2].slice(1); [1].concat([2]); [1, 0].filter(Boolean); [1].map(function (x) { return x; });
print(calls);
Object.defineProperty(ap, 'constructor', orig); calls = 0; [1, 2].slice(1);
print(calls, ap.constructor === Array);
var sd = Object.getOwnPropertyDescriptor(Array, Symbol.species), n = 0;
Object.defineProperty(Array, Symbol.species, { get: function () { n++; return Array; }, configurable: true });
[1, 2].slice(1); [1].concat([2]); [1, 0].filter(Boolean);
print(n);
Object.defineProperty(Array, Symbol.species, sd);
for (var i = 0; i < 3; i++) [1].concat([2]);
var g = 0;
Object.defineProperty(ap, Symbol.isConcatSpreadable, { get: function () { g++; return true; }, configurable: true });
print([1].concat([2], [3]).join(), g);
delete ap[Symbol.isConcatSpreadable];
for (i = 0; i < 3; i++) [1].concat([2]);
g = 0;
Object.defineProperty(Object.prototype, Symbol.isConcatSpreadable, { get: function () { g++; return true; }, configurable: true });
print([1].concat([2], [3]).join(), g);
delete Object.prototype[Symbol.isConcatSpreadable];
var r = [1, 2, 3, 4]; r.length = 2;
print(r[2], r[1]);
r.length = 4; print(r[2], r[3]);
ap[3] = 'Q'; print(r[3], r[2]); delete ap[3];
var se = Object.seal([1, , 3]);
print(se[1], se[2], se[3], Object.seal([1, 0, 2]).filter(Boolean).join());
print(ap.join.call({ length: 1, 0: 's' }), ap.join.call({ length: 1, get 0() { return 'g'; } }), ap.join.call('a'), ap.join.call('ab'), ap.join.call({ length: 1, 0: 7 }));",
        r"1 1,2
1,2
1 2 1
4
0 true
3
1,2,3 3
1,2,3 3
undefined 2
undefined undefined
Q undefined
undefined 3 undefined 1,2
s g a a,b 7",
    ),
];

const CLOSURES: &[(&str, &str)] = &[(
    r"function own(o) { return Object.getOwnPropertyNames(o).filter(function (n) { return n !== 'arguments' && n !== 'caller'; }).join(); }
function make(i) { return function named(a, b) { return i; }; }
var fs = []; for (var i = 0; i < 5; i++) fs.push(make(i));
var f = fs[3];
print(own(f), f.length, f.name, f.prototype.constructor === f, own(f.prototype));
var d = Object.getOwnPropertyDescriptor(f, 'prototype'); print(d.writable, d.enumerable, d.configurable);
d = Object.getOwnPropertyDescriptor(f, 'name'); print(d.writable, d.enumerable, d.configurable);
d = Object.getOwnPropertyDescriptor(f.prototype, 'constructor'); print(d.writable, d.enumerable, d.configurable);
print(fs[0].prototype !== fs[1].prototype, new fs[2]() instanceof fs[2], fs[4](), Object.keys(f.prototype).length);
f.extra = 1; fs[2].prototype.m = function () { return 'm'; }; print(Object.keys(f).join(), new fs[2]().m(), fs[1].prototype.m);
Object.defineProperty(fs[0], 'name', { value: 'renamed' }); print(fs[0].name, fs[1].name);
fs[1].prototype = null; print(fs[1].prototype, typeof fs[2].prototype, Object.getPrototypeOf(new fs[1]()) === Object.prototype);
var anon = [function () {}, function (x, y, z) {}]; print(anon[0].name === '', anon[1].length, own(anon[1]));
var arrow = function () { return (a) => a; }; print(own(arrow()), 'prototype' in arrow());
function* gen() {} print(own(gen), Object.getPrototypeOf(gen.prototype) === Object.getPrototypeOf(function* () {}).prototype);
var arr = [1, , 3, ...[4, 5], 6]; print(arr.length, 1 in arr, arr.join());
var lit = [1, 2, 3]; lit.push(4); print(lit.length, lit.join(), [[], [1]].length, [ , ].length);",
    r"length,name,prototype 2 named true constructor
true false false
false false true
true false true
true true 4 0
extra m undefined
renamed named
null object true
true 3 length,name,prototype
length,name false
length,name,prototype true
6 false 1,,3,4,5,6
4 1,2,3,4 2 1",
)];
