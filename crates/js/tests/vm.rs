//! The compiler and the interpreter (ADR 0026 sections 4 and 5): small
//! scripts with their expected output, each run normally and in the GC
//! stress mode (the harness also checks that no collection found a stale
//! root). The expected outputs of the tables come from Node.js 22 (a
//! black box), with the same test functions (`print`, `defineAccessor`,
//! `callTwice`, `later`, `gc`); `tests/common` describes them.

mod common;

use common::check_cases;

#[test]
fn numbers() {
    check_cases(NUMBERS);
}

#[test]
fn bitwise() {
    check_cases(BITWISE);
}

#[test]
fn comparison() {
    check_cases(COMPARISON);
}

#[test]
fn unary() {
    check_cases(UNARY);
}

#[test]
fn logical() {
    check_cases(LOGICAL);
}

#[test]
fn variables() {
    check_cases(VARIABLES);
}

#[test]
fn functions() {
    check_cases(FUNCTIONS);
}

#[test]
fn this() {
    check_cases(THIS);
}

#[test]
fn construct() {
    check_cases(CONSTRUCT);
}

#[test]
fn accessors() {
    check_cases(ACCESSORS);
}

#[test]
fn natives() {
    check_cases(NATIVES);
}

#[test]
fn generators() {
    check_cases(GENERATORS);
}

#[test]
fn globals() {
    check_cases(GLOBALS);
}

#[test]
fn strings() {
    check_cases(STRINGS);
}

#[test]
fn objects() {
    check_cases(OBJECTS);
}

#[test]
fn control() {
    check_cases(CONTROL);
}

#[test]
fn semantics() {
    check_cases(SEMANTICS);
}

/// numbers (expected output from Node.js 22).
const NUMBERS: &[(&str, &str)] = &[
    ("1 + 2", "=> 3"),
    (
        "print(7 / 2, 7 % 3, 2 ** 10, 0.1 + 0.2, 100 / 3)",
        "3.5 1 1024 0.30000000000000004 33.333333333333336",
    ),
    (
        "print(1 / 0, -1 / 0, 0 / 0, 1 / -0)",
        "Infinity -Infinity NaN -Infinity",
    ),
    (
        "print(1 / (0 * -5), 1 / (-4 % 2), 1 / (-0 + 0), 1 / -(0))",
        "-Infinity -Infinity Infinity -Infinity",
    ),
    (
        "print(2147483647 + 1, -2147483648 - 1, 65536 * 65536, -2147483648 / -1)",
        "2147483648 -2147483649 4294967296 2147483648",
    ),
    (
        "print(1e21, 1e-7, 123456789012345680000, 0.000001, 5e-324, -1e-7, 1e100, 123e-20)",
        "1e+21 1e-7 123456789012345680000 0.000001 5e-324 -1e-7 1e+100 1.23e-18",
    ),
    (
        "print(2 ** 53, 2 ** 53 + 1, -(2 ** 31), 2 ** 31, 1.5e300 * 1e10)",
        "9007199254740992 9007199254740992 -2147483648 2147483648 Infinity",
    ),
    (
        "print((-8) % 3, 8 % -3, 5.5 % 2, -5.5 % 2, 5 % 0)",
        "-2 2 1.5 -1.5 NaN",
    ),
    (
        "print((-2) ** 2, 2 ** -1, 1 ** Infinity, (-8) ** (1/3), 2 ** 0.5, NaN ** 0)",
        "4 0.5 NaN NaN 1.4142135623730951 1",
    ),
    (
        "print('5' * '2', '5' + 2, '5' - '2', null + 1, undefined + 1, true + true)",
        "10 52 3 1 NaN 2",
    ),
    (
        "print('0x10' * 1, ' 12 ' * 1, '1e3' * 1, '.5' * 1, '5.' * 1, '+5' * 1, '-0x10' * 1)",
        "16 12 1000 0.5 5 5 NaN",
    ),
    (
        "print('0b11' * 1, '0o17' * 1, 'Infinity' * 1, '-Infinity' * 1, 'infinity' * 1, '1_0' * 1, '' * 1, '12px' * 1)",
        "3 15 Infinity -Infinity NaN NaN 0 NaN",
    ),
    (
        "print(+'3', -'3', +true, +null, +undefined, +'', -'')",
        "3 -3 1 0 NaN 0 0",
    ),
    (
        "var x = 0.5; x += 0.25; x *= 4; x -= 1; x /= 4; x %= 0.3; x",
        "=> 0.2",
    ),
    (
        "print(10 - 2 - 3, 2 * 3 + 4 * 5, (1 + 2) * 3, 2 ** 3 ** 2, -(2 ** 2))",
        "5 26 9 512 -4",
    ),
];

/// bitwise (expected output from Node.js 22).
const BITWISE: &[(&str, &str)] = &[
    (
        "print(-1 >>> 0, 1 << 31, 1 << 32, -8 >> 1, -8 >>> 1, ~5, ~~3.7, ~~-3.7)",
        "4294967295 -2147483648 1 -4 2147483644 -6 3 -3",
    ),
    (
        "print(5 & 3, 5 | 3, 5 ^ 3, 2 ** 32 | 0, 4294967296.5 | 0, '12' << 1, 1.9 | 0, -1.9 | 0)",
        "1 7 6 0 0 24 1 -1",
    ),
    (
        "print(2 ** 31 >> 0, 2 ** 31 >>> 0, -(2 ** 31) >>> 0, 1 << -1, 1 >>> 33, NaN | 0, Infinity | 0)",
        "-2147483648 2147483648 2147483648 -2147483648 0 0 0",
    ),
    (
        "var a = 6; a &= 3; a |= 8; a ^= 1; a <<= 2; a >>= 1; a >>>= 1; a",
        "=> 11",
    ),
];

/// comparison (expected output from Node.js 22).
const COMPARISON: &[(&str, &str)] = &[
    (
        "print('b' > 'a', '10' < '9', 10 < '9', undefined < 1, null >= 0, NaN < 1, NaN >= NaN)",
        "true true false false true false false",
    ),
    (
        "print('a' < 'ab', 1 <= 1, '2' > 1, 'B' < 'a', 2 >= 3, 3 >= 3, -0 < 0, null < 1)",
        "true true true true false true false true",
    ),
    (
        "print('' == 0, null == undefined, null == 0, NaN == NaN, '1' == 1, true == 1, true == '1', false == '')",
        "true true false false true true true true",
    ),
    (
        "print(0 === -0, 'a' === 'a', 1 === 1.0, null === undefined, undefined === undefined, 'a' !== 'b', 1 != '1')",
        "true true true false true true false",
    ),
    (
        "var o = {}; var p = {}; print(o === o, o === p, o == o, o != p)",
        "true false true true",
    ),
    (
        "print(({valueOf: function () { return 5; }}) == 5, 5 == {valueOf: function () { return 5; }})",
        "true true",
    ),
    (
        "var l = {valueOf: function () { print('L'); return 1; }}; var r = {valueOf: function () { print('R'); return 2; }}; print(l > r, l <= r, l < r, l >= r)",
        "L
R
L
R
L
R
L
R
false true true false",
    ),
    (
        "var i = 0; var n = 3; var s = ''; while (i < n) { s += i; i++; } for (var j = 5; j >= 3; j--) { s += j; } s",
        "=> 012543",
    ),
    (
        "var c = 0; for (var k = 0.5; k <= 2.5; k += 0.5) { c++; } for (var m = 'a'; m < 'aaa'; m += 'a') { c++; } c",
        "=> 7",
    ),
];

/// unary (expected output from Node.js 22).
const UNARY: &[(&str, &str)] = &[
    (
        "print(typeof 1, typeof 'a', typeof true, typeof undefined, typeof null, typeof {}, typeof [], typeof function () {}, typeof undeclaredName)",
        "number string boolean undefined object object object function undefined",
    ),
    (
        "var t = 5; print(typeof t, typeof (t), typeof typeof t, void 0, void t, !t, !!t, !0, !'')",
        "number number string undefined undefined false true true true",
    ),
    (
        "var o = {a: 1, b: 2}; print(delete o.a, o.a, 'a' in o, delete o.zz, delete o['b'], o.b)",
        "true undefined false true true undefined",
    ),
    (
        "var g = 1; implicitGlobal = 2; print(delete g, delete implicitGlobal, typeof implicitGlobal, delete 5)",
        "false true undefined true",
    ),
    (
        "function f() { var local = 1; return delete local; } f()",
        "=> false",
    ),
    (
        "var u; var s = '5'; s++; var r = s; var v = '7'; var w = v++; print(r, typeof w, w, v, u++, u)",
        "6 number 7 8 NaN NaN",
    ),
    (
        "var n = 1; var a = n++ + ++n; var b = n-- - --n; print(a, b, n)",
        "4 2 1",
    ),
];

/// logical (expected output from Node.js 22).
const LOGICAL: &[(&str, &str)] = &[
    (
        "print(0 || 'a', 1 && 2, null ?? 3, 0 ?? 3, '' || null || 'x', 1 && 0 && 2, undefined ?? null ?? 4)",
        "a 2 3 0 x 0 4",
    ),
    (
        "var log = ''; function t(v) { log += v; return v; } t(0) && t(1); t(1) || t(2); t(null) ?? t(3); t(4) ?? t(5); log",
        "=> 01null34",
    ),
    (
        "var a = 0, b = 1, c = null; a ||= 5; b &&= 7; c ??= 9; var d = 2; d ??= 8; print(a, b, c, d)",
        "5 7 9 2",
    ),
    (
        "var o = {x: 0, y: 1}; o.x ||= 3; o.y &&= 4; o.z ??= 5; print(o.x, o.y, o.z)",
        "3 4 5",
    ),
    (
        "print(1 ? 'y' : 'n', 0 ? 'y' : 'n', (1, 2, 3), true ? false ? 1 : 2 : 3)",
        "y n 3 2",
    ),
    ("var x = 5; var y = (x = 6, x + 1); print(x, y)", "6 7"),
    ("var a = 1; print(a && a || 'z', (a || 0) && 'w')", "1 w"),
    (
        "var n = 0; if (n || !n && 1) { print('in'); } if (!(n > 0)) { print('not'); }",
        "in
not",
    ),
];

/// variables (expected output from Node.js 22).
const VARIABLES: &[(&str, &str)] = &[
    (
        "print(x); var x = 1; print(x)",
        "undefined
1",
    ),
    (
        "let a = 1; { let a = 2; print(a); } print(a)",
        "2
1",
    ),
    ("const c = 3; c", "=> 3"),
    (
        "{ print(x); let x = 1; }",
        "Uncaught ReferenceError: Cannot access 'x' before initialization",
    ),
    (
        "const c = 1; c = 2;",
        "Uncaught TypeError: Assignment to constant variable.",
    ),
    (
        "function f() { return later; } let later = 3; print(f());",
        "3",
    ),
    (
        "function f() { return early; } print(f()); let early = 3;",
        "Uncaught ReferenceError: Cannot access 'early' before initialization",
    ),
    (
        "let z; print(z); z = 4; z",
        "undefined
=> 4",
    ),
    ("var v = 1; var v; v", "=> 1"),
    ("function h() { return 'h'; } h()", "=> h"),
    ("print(typeof hoisted); function hoisted() {}", "function"),
    (
        "{ function blockFn() { return 1; } print(blockFn()); }",
        "1",
    ),
    (
        "x = 1; let x;",
        "Uncaught ReferenceError: Cannot access 'x' before initialization",
    ),
    (
        "let q = 1; { q = 2; let r = q; print(r); } q",
        "2
=> 2",
    ),
    (
        "for (let i = 0; i < 2; i++) { let i = 'inner'; print(i); }",
        "inner
inner",
    ),
    (
        "if (true) { let v = 1; print(v); } else { let v = 2; print(v); }",
        "1",
    ),
    (
        "var s = ''; for (let i = 0; i < 3; i++) { let x = i * 2; s += x; } s",
        "=> 024",
    ),
];

/// functions (expected output from Node.js 22).
const FUNCTIONS: &[(&str, &str)] = &[
    (
        "function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); } fib(20)",
        "=> 6765",
    ),
    (
        "function f(a, b, c) { return [a, b, c]; } var r = f(1); print(r[0], r[1], r[2], r.length)",
        "1 undefined undefined 3",
    ),
    ("function f(a) { return a; } f(1, 2, 3)", "=> 1"),
    (
        "function f() { return arguments.length + ':' + arguments[0] + ':' + arguments[3]; } f(1, 2, 3, 4, 5)",
        "=> 5:1:4",
    ),
    (
        "function f(a, b) { return arguments.length + ' ' + a + ' ' + arguments[1] + ' ' + arguments[2]; } f(1)",
        "=> 1 1 undefined undefined",
    ),
    (
        "function f(a, b) { var args = arguments; return function () { return args[0] + args[2]; }; } f(1, 2, 3)()",
        "=> 4",
    ),
    ("function f(a, a) { return a; } f(1, 2)", "=> 2"),
    (
        "function counter() { var n = 0; return function () { n++; return n; }; } var c = counter(); c(); c(); var d = counter(); print(c(), d())",
        "3 1",
    ),
    (
        "function a(x) { return function (y) { return function (z) { return x + y + z; }; }; } a(1)(2)(3)",
        "=> 6",
    ),
    (
        "function outer() { var v = 1; function mid() { function inner() { return v; } return inner; } var g = mid(); v = 5; return g(); } outer()",
        "=> 5",
    ),
    (
        "var fns = []; for (let i = 0; i < 3; i++) { fns[i] = function () { return i; }; } print(fns[0](), fns[1](), fns[2]())",
        "0 1 2",
    ),
    (
        "var fns = []; for (var i = 0; i < 3; i++) { fns[i] = function () { return i; }; } print(fns[0](), fns[1](), fns[2]())",
        "3 3 3",
    ),
    (
        "var fs = []; for (let i = 0; i < 3; i++) { fs[i] = () => i; i += 0; } print(fs[0](), fs[1](), fs[2]())",
        "0 1 2",
    ),
    (
        "var gs = []; for (let i = 0, g = () => i; i < 2; i++) { gs[i] = g; } print(gs[0](), gs[1]())",
        "0 0",
    ),
    (
        "var fs = []; for (let i = 0; i < 4; i++) { if (i == 1) continue; fs[fs.length] = () => i; } print(fs.length, fs[0](), fs[1](), fs[2]())",
        "3 0 2 3",
    ),
    (
        "var fs = []; for (let i = 0; i < 3; i++) { fs[i] = function () { return i++; }; } print(fs[0](), fs[0](), fs[1]())",
        "0 1 1",
    ),
    ("var r = (function (x) { return x * 2; })(21); r", "=> 42"),
    (
        "var f = function g(n) { return n ? n * g(n - 1) : 1; }; print(f(5), typeof g)",
        "120 undefined",
    ),
    (
        "var f = function g() { g = 1; return typeof g; }; f()",
        "=> function",
    ),
    (
        "var f = function g() { 'use strict'; g = 1; }; f()",
        "Uncaught TypeError: Assignment to constant variable.",
    ),
    (
        "function foo() {} var bar = function () {}; var baz = () => 1; var o = {m() {}, n: function () {}, k: () => 2}; print(foo.name, bar.name, baz.name, o.m.name, o.n.name, o.k.name, (function () {}).name === '')",
        "foo bar baz m n k true",
    ),
    (
        "function f(a, b, c) {} var g = (x) => x; print(f.length, g.length, (function () {}).length)",
        "3 1 0",
    ),
    (
        "var add = (a, b) => a + b; var twice = v => { return add(v, v); }; twice(21)",
        "=> 42",
    ),
    (
        "function f() { return; } print(f(), (() => {})())",
        "undefined undefined",
    ),
    (
        "var depth = 0; function rec(n) { if (n == 0) return 0; return 1 + rec(n - 1); } rec(5000)",
        "=> 5000",
    ),
    (
        "function make() { var count = 0; return { inc: function () { return ++count; }, get: function () { return count; } }; } var m = make(); m.inc(); m.inc(); m.get()",
        "=> 2",
    ),
    (
        "function f(x) { return function () { x = x + 1; return x; }; } var a = f(10); var b = f(20); print(a(), a(), b(), a())",
        "11 12 21 13",
    ),
    (
        "function sum() { var s = 0; var i = 0; while (i < arguments.length) { s += arguments[i]; i++; } return s; } sum(1, 2, 3, 4)",
        "=> 10",
    ),
    (
        "function f() { return arguments.callee === f; } f()",
        "=> true",
    ),
];

/// this (expected output from Node.js 22).
const THIS: &[(&str, &str)] = &[
    (
        "var o = {v: 1, m: function () { return this.v; }}; o.m()",
        "=> 1",
    ),
    (
        "var o = {v: 2, m() { return this.v; }}; var f = o.m; print(o.m(), f === o.m, o['m']())",
        "2 true 2",
    ),
    (
        "function f() { return this === globalThis; } f()",
        "=> true",
    ),
    ("function f() { 'use strict'; return this; } f()", ""),
    ("this === globalThis", "=> true"),
    (
        "var o = {v: 3, m: function () { var g = () => this.v; return g(); }}; o.m()",
        "=> 3",
    ),
    (
        "var o = {v: 4, m: function () { return () => () => this.v; }}; o.m()()()",
        "=> 4",
    ),
    ("var a = () => this; a() === globalThis", "=> true"),
    (
        "var o = {v: 5, m: function () { function inner() { return this === globalThis; } return inner(); }}; o.m()",
        "=> true",
    ),
    (
        "var o = {v: 6, m: function () { 'use strict'; function inner() { return this; } return inner(); }}; o.m()",
        "",
    ),
    ("var x = 7; function f() { return this.x; } f()", "=> 7"),
    (
        "var o = {f: function () { return this; }}; (o.f)() === o",
        "=> true",
    ),
    (
        "var o = {f: function () { return this; }}; (0, o.f)() === globalThis",
        "=> true",
    ),
    (
        "var o = {a: {b: {c: function () { return this.n; }, n: 8}}}; o.a.b.c()",
        "=> 8",
    ),
];

/// construct (expected output from Node.js 22).
const CONSTRUCT: &[(&str, &str)] = &[
    (
        "function P(x) { this.x = x; } var p = new P(3); p.x",
        "=> 3",
    ),
    (
        "function P() { this.a = 1; return {b: 2}; } var p = new P(); print(p.a, p.b)",
        "undefined 2",
    ),
    (
        "function P() { this.a = 1; return 5; } var p = new P(); print(p.a)",
        "1",
    ),
    (
        "function P() {} P.prototype.hello = function () { return 'hi ' + this.n; }; var p = new P(); p.n = 'x'; p.hello()",
        "=> hi x",
    ),
    (
        "function P() {} var p = new P(); print(p instanceof P, {} instanceof P, p.constructor === P)",
        "true false true",
    ),
    (
        "function A() {} function B() {} B.prototype = new A(); var b = new B(); print(b instanceof B, b instanceof A)",
        "true true",
    ),
    (
        "function P() { this.self = this; } var p = new P; p.self === p",
        "=> true",
    ),
    (
        "var arrow = () => 1; new arrow()",
        "Uncaught TypeError: arrow is not a constructor",
    ),
    (
        "var o = {m() {}}; new o.m()",
        "Uncaught TypeError: o.m is not a constructor",
    ),
    (
        "var x = 5; new x()",
        "Uncaught TypeError: x is not a constructor",
    ),
    (
        "function* g() {} new g()",
        "Uncaught TypeError: g is not a constructor",
    ),
    ("function P() { return null; } typeof new P()", "=> object"),
    ("function P(a, b) { this.s = a + b; } new P(1, 2).s", "=> 3"),
];

/// accessors (expected output from Node.js 22).
const ACCESSORS: &[(&str, &str)] = &[
    (
        "var o = {}; defineAccessor(o, 'x', function () { return 'got ' + this.tag; }); o.tag = 1; o.x",
        "=> got 1",
    ),
    (
        "var o = {}; var stored; defineAccessor(o, 'x', undefined, function (v) { stored = v * 2; }); o.x = 21; print(stored, o.x)",
        "42 undefined",
    ),
    (
        "var proto = {}; defineAccessor(proto, 'v', function () { return this.n; }); function C(n) { this.n = n; } C.prototype = proto; var c = new C(9); c.v",
        "=> 9",
    ),
    (
        "var proto = {}; var log = ''; defineAccessor(proto, 'v', undefined, function (x) { log += x; this.w = x; }); var o = {__proto__: proto}; o.v = 1; o.v = 2; print(log, o.w, 'v' in o)",
        "12 2 true",
    ),
    (
        "var n = 0; var o = {}; defineAccessor(o, 'c', function () { return ++n; }); var s = 0; for (var i = 0; i < 100; i++) { s += o.c; } print(s, n)",
        "5050 100",
    ),
    (
        "defineAccessor(globalThis, 'gg', function () { return 'global getter'; }); gg",
        "=> global getter",
    ),
    (
        "var box = 0; defineAccessor(globalThis, 'gs', function () { return box; }, function (v) { box = v + 1; }); gs = 10; gs",
        "=> 11",
    ),
    (
        "var o = {}; defineAccessor(o, 'r', function () { return this.r2; }); defineAccessor(o, 'r2', function () { return 'deep'; }); o.r",
        "=> deep",
    ),
    (
        "var o = {}; defineAccessor(o, 'x', function () { throw 'from getter'; }); o.x",
        "Uncaught from getter",
    ),
    (
        "var o = {}; defineAccessor(o, 'x', function () { return 1; }); o.x = 5; o.x",
        "=> 1",
    ),
    (
        "var o = {}; defineAccessor(o, 'x', function () { return 1; }); (function () { 'use strict'; o.x = 5; })()",
        "Uncaught TypeError: Cannot set property x of #<Object> which has only a getter",
    ),
    (
        "var o = {}; defineAccessor(o, 'x', function () { return 3; }); o.x += 1; o.x",
        "=> 3",
    ),
    (
        "var o = {}; defineAccessor(o, 'x', function () { return 3; }); typeof o.x + typeof o['x']",
        "=> numbernumber",
    ),
    (
        "var o = {}; var k = 'x'; var v = 0; defineAccessor(o, k, function () { return v; }, function (n) { v = n; }); o[k] = 4; o[k]++; o.x",
        "=> 5",
    ),
];

/// natives (expected output from Node.js 22).
const NATIVES: &[(&str, &str)] = &[
    ("function sq(x) { return x * x; } callTwice(sq, 3)", "=> 18"),
    ("later(function (a, b) { return a + b; }, 2, 3)", "=> 5"),
    (
        "var r = later(function () { return later(function (x) { return x; }, 'deep'); }); r",
        "=> deep",
    ),
    (
        "function f(n) { return n == 0 ? 0 : callTwice(f, n - 1) + 1; } f(3)",
        "=> 7",
    ),
    (
        "var o = {}; defineAccessor(o, 'v', function () { return callTwice(function (x) { return x; }, 4); }); o.v",
        "=> 8",
    ),
    (
        "callTwice(function (x) { return later(function (y) { return y + 1; }, x); }, 1)",
        "=> 4",
    ),
    (
        "var n = 0; function g() { n++; gc(); return n; } callTwice(g, 0); later(g); n",
        "=> 3",
    ),
];

/// generators (expected output from Node.js 22).
const GENERATORS: &[(&str, &str)] = &[
    (
        "function* g() { yield 1; yield 2; return 3; } var it = g(); var a = it.next(); var b = it.next(); var c = it.next(); var d = it.next(); print(a.value, a.done, b.value, c.value, c.done, d.value, d.done)",
        "1 false 2 3 true undefined true",
    ),
    (
        "function* g() { var x = yield 'first'; var y = yield x * 2; return x + y; } var it = g(); print(it.next('ignored').value, it.next(5).value, it.next(7).value)",
        "first 10 12",
    ),
    (
        "function* g(n) { for (let i = 0; i < n; i++) { yield i; } } var it = g(3); var s = ''; var r = it.next(); while (!r.done) { s += r.value; r = it.next(); } s",
        "=> 012",
    ),
    (
        "function* g() { var count = 0; while (true) { count++; yield count; } } var a = g(); var b = g(); a.next(); a.next(); print(a.next().value, b.next().value)",
        "3 1",
    ),
    (
        "function make() { var shared = 0; function* gen() { while (true) { shared += 10; yield shared; } } return {gen: gen, bump: function () { shared++; return shared; }}; } var m = make(); var it = m.gen(); print(it.next().value, m.bump(), it.next().value, m.bump())",
        "10 11 21 22",
    ),
    (
        "function deep(n) { return n == 0 ? 0 : 1 + deep(n - 1); } function* g() { let local = 'kept'; let f = () => local; yield f(); deep(200); local = 'changed'; yield f(); } var it = g(); var a = it.next().value; deep(500); print(a, it.next().value, deep(100))",
        "kept changed 100",
    ),
    (
        "function* g() { yield arguments.length; yield arguments[1]; } var it = g('a', 'b', 'c'); print(it.next().value, it.next().value)",
        "3 b",
    ),
    (
        "function* g() { yield 1 + (yield 2); } var it = g(); print(it.next().value, it.next(10).value, it.next().done)",
        "2 11 true",
    ),
    (
        "function* g() { it.next(); } var it = g(); it.next()",
        "Uncaught TypeError: Generator is already running",
    ),
    (
        "function* g() { yield 1; } var it = g(); print(typeof it, typeof it.next, it.next().value)",
        "object function 1",
    ),
    (
        "var o = {*m() { yield this.v; }, v: 'method'}; o.m().next().value",
        "=> method",
    ),
    (
        "function* g() { yield; } var r = g().next(); print(r.value, r.done)",
        "undefined false",
    ),
    (
        "function* g() { throw 'boom'; } var it = g(); it.next()",
        "Uncaught boom",
    ),
    (
        "function* g() { yield 1; } var it = g(); it.next(); it.next(); var r = it.next(); print(r.value, r.done)",
        "undefined true",
    ),
    (
        "function* outer() { var inner = (function* () { yield 'i1'; yield 'i2'; })(); var r = inner.next(); while (!r.done) { yield r.value; r = inner.next(); } yield 'o'; } var it = outer(); print(it.next().value, it.next().value, it.next().value, it.next().done)",
        "i1 i2 o true",
    ),
    (
        "var gens = []; for (let i = 0; i < 3; i++) { gens[i] = (function* () { yield i; yield i * 10; })(); } print(gens[2].next().value, gens[0].next().value, gens[0].next().value, gens[1].next().value)",
        "2 0 0 1",
    ),
];

/// globals (expected output from Node.js 22).
const GLOBALS: &[(&str, &str)] = &[
    ("var gv = 1; globalThis.gv", "=> 1"),
    ("let gl = 2; typeof globalThis.gl", "=> undefined"),
    ("function gf() {} typeof globalThis.gf", "=> function"),
    ("implicit = 3; globalThis.implicit", "=> 3"),
    (
        "'use strict'; undeclared = 1;",
        "Uncaught ReferenceError: undeclared is not defined",
    ),
    (
        "undefinedVariable",
        "Uncaught ReferenceError: undefinedVariable is not defined",
    ),
    ("typeof undefinedVariable", "=> undefined"),
    (
        "print(undefined, NaN, Infinity, typeof globalThis)",
        "undefined NaN Infinity object",
    ),
    ("undefined = 5; undefined", ""),
    (
        "(function () { 'use strict'; undefined = 5; })()",
        "Uncaught TypeError: Cannot assign to read only property 'undefined' of object '[object Object]'",
    ),
    ("var NaN; typeof NaN", "=> number"),
    ("let a = 1; a = 2; a", "=> 2"),
    (
        "const k = 1; (function () { k = 2; })()",
        "Uncaught TypeError: Assignment to constant variable.",
    ),
    (
        "function f() { return gx; } var gx = 'later'; f()",
        "=> later",
    ),
];

/// strings (expected output from Node.js 22).
const STRINGS: &[(&str, &str)] = &[
    ("'abc'.length + 'abc'[1] + 'abc'[5]", "=> 3bundefined"),
    (
        "var s = 'h' + 'e' + 'l' + 'l' + 'o'; print(s, s.length, s[4])",
        "hello 5 o",
    ),
    ("var n = 3; `a${n}b${n + 1}c${'x'}`", "=> a3b4cx"),
    ("`${1}${2}` + `` + `plain`", "=> 12plain"),
    (
        "print('a' + 1 + 2, 1 + 2 + 'a', 'a' + null + undefined + true)",
        "a12 3a anullundefinedtrue",
    ),
    (
        r"print('\u00e9t\u00e9'.length, '\u{1F600}'.length, 'tab\there')",
        "3 2 tab	here",
    ),
    (
        "var s = ''; for (var i = 0; i < 5; i++) { s = s + i; } s",
        "=> 01234",
    ),
    (
        "var o = {toString: function () { return 'T'; }}; `x${o}y`",
        "=> xTy",
    ),
    (
        "var o = {valueOf: function () { return 1; }, toString: function () { return 'T'; }}; print(o + '', `${o}`, o * 2)",
        "1 T 2",
    ),
    ("'abc'['length']", "=> 3"),
];

/// objects (expected output from Node.js 22).
const OBJECTS: &[(&str, &str)] = &[
    (
        "var o = {a: 1, 'b': 2, 3: 'three', [1 + 1]: 'two'}; print(o.a, o.b, o[3], o['3'], o[2])",
        "1 2 three three two",
    ),
    ("var o = {a: {b: {c: 'deep'}}}; o.a.b.c", "=> deep"),
    (
        "var a = 'x'; var b = 1; var o = {a, b}; print(o.a, o.b)",
        "x 1",
    ),
    (
        "var arr = [1, , 3]; print(arr.length, arr[1], arr[2], 1 in arr)",
        "3 undefined 3 false",
    ),
    (
        "var arr = [1, 2, 3]; arr.length = 1; print(arr.length, arr[1])",
        "1 undefined",
    ),
    (
        "var arr = []; arr[5] = 1; print(arr.length, arr[0])",
        "6 undefined",
    ),
    (
        "var o = {}; o.x = 1; o.x++; o['y'] = 10; o['y'] += 5; ++o.x; print(o.x, o.y)",
        "3 15",
    ),
    (
        "var arr = [10, 20]; var i = 1; arr[i] += 1; arr[i++] *= 2; print(arr[0], arr[1], i)",
        "10 42 2",
    ),
    (
        "var p = {greet: function () { return 'p'; }}; var o = {__proto__: p}; o.greet()",
        "=> p",
    ),
    ("var o = {__proto__: null}; typeof o.x", "=> undefined"),
    (
        "var o = {x: 1}; var k = 'x'; o[k] = 2; o[k + ''] + o.x",
        "=> 4",
    ),
    (
        "var o = {}; o[1.5] = 'a'; o[-1] = 'b'; o[1e21] = 'c'; print(o['1.5'], o['-1'], o['1e+21'])",
        "a b c",
    ),
    (
        "var u; u.x",
        "Uncaught TypeError: Cannot read properties of undefined (reading 'x')",
    ),
    (
        "var u = null; u.x = 1;",
        "Uncaught TypeError: Cannot set properties of null (setting 'x')",
    ),
    (
        "var u; u[0]",
        "Uncaught TypeError: Cannot read properties of undefined (reading '0')",
    ),
    (
        "var s = 'abc'; (function () { 'use strict'; s.x = 1; })()",
        "Uncaught TypeError: Cannot create property 'x' on string 'abc'",
    ),
    (
        "(function () { 'use strict'; (5).x = 1; })()",
        "Uncaught TypeError: Cannot create property 'x' on number '5'",
    ),
    ("var s = 'str'; s.x = 1; s.x", ""),
    (
        "'x' in 5",
        "Uncaught TypeError: Cannot use 'in' operator to search for 'x' in 5",
    ),
    (
        "var o = {}; o.f()",
        "Uncaught TypeError: o.f is not a function",
    ),
    (
        "var a = [1]; a[0]()",
        "Uncaught TypeError: a[0] is not a function",
    ),
    (
        "var o = {}; o.a.b.c()",
        "Uncaught TypeError: Cannot read properties of undefined (reading 'b')",
    ),
    (
        "var t = 1; t()()",
        "Uncaught TypeError: t is not a function",
    ),
    (
        "(function () {})()()",
        "Uncaught TypeError: (intermediate value)(...) is not a function",
    ),
    (
        "var o = {}; o['k']()",
        "Uncaught TypeError: o.k is not a function",
    ),
    ("this.x()", "Uncaught TypeError: this.x is not a function"),
    (
        "({}) instanceof 5",
        "Uncaught TypeError: Right-hand side of 'instanceof' is not an object",
    ),
    (
        "({}) instanceof {}",
        "Uncaught TypeError: Right-hand side of 'instanceof' is not callable",
    ),
    (
        "function C() {} C.prototype = 5; ({}) instanceof C",
        "Uncaught TypeError: Function has non-object prototype '5' in instanceof check",
    ),
    (
        "var o = {valueOf: function () { return 3; }}; o + 1",
        "=> 4",
    ),
    (
        "var o = {toString: function () { return 'x'; }}; o + 1",
        "=> x1",
    ),
    (
        "var a = {toString: function () { return 'a'; }}; var b = {valueOf: function () { return 1; }}; a + b",
        "=> a1",
    ),
    (
        "var l = {valueOf: function () { print('a'); return 1; }}; var r = {valueOf: function () { print('b'); return 2; }}; l + r",
        "a
b
=> 3",
    ),
    ("'5' * {valueOf: function () { return 2; }}", "=> 10"),
    (
        "var o = {valueOf: function () { return {}; }, toString: function () { return {}; }}; o + 1",
        "Uncaught TypeError: Cannot convert object to primitive value",
    ),
    (
        "var o = {valueOf: 5, toString: function () { return '7'; }}; o * 1",
        "=> 7",
    ),
];

/// control (expected output from Node.js 22).
const CONTROL: &[(&str, &str)] = &[
    (
        "var s = ''; outer: for (var i = 0; i < 3; i++) { for (var j = 0; j < 3; j++) { if (j == 1) continue outer; if (i == 2) break outer; s += i + '' + j + ' '; } } s",
        "=> 00 10 ",
    ),
    (
        "var s = ''; block: { s += 'a'; if (s) break block; s += 'b'; } s",
        "=> a",
    ),
    ("var i = 0; do { i++; } while (i < 5); i", "=> 5"),
    (
        "var i = 0; do { i++; if (i == 2) continue; if (i > 3) break; } while (true); i",
        "=> 4",
    ),
    (
        "var n = 0; while (true) { n++; if (n >= 10) break; } n",
        "=> 10",
    ),
    (
        "var s = 0; for (var i = 0; i < 10; i++) { if (i % 2) continue; s += i; } s",
        "=> 20",
    ),
    ("var s = 0; for (;;) { s++; if (s == 3) break; } s", "=> 3"),
    (
        "var r = ''; for (var i = 0; i < 3; i++) { if (i == 0) r += 'z'; else if (i == 1) r += 'o'; else r += 't'; } r",
        "=> zot",
    ),
    (
        "a: b: for (var i = 0; i < 5; i++) { if (i == 3) break a; } i",
        "=> 3",
    ),
    (
        "var x = 0; lbl: while (x < 10) { x++; if (x < 5) continue lbl; break; } x",
        "=> 5",
    ),
    ("throw 42", "Uncaught 42"),
    (
        "throw {name: 'MyError', message: 'custom'}",
        "Uncaught MyError: custom",
    ),
    (
        "function f() { throw 'deep'; } function g() { f(); } g()",
        "Uncaught deep",
    ),
];

/// semantics (expected output from Node.js 22).
const SEMANTICS: &[(&str, &str)] = &[
    (
        "function f() { var s = 1; s += (s = 5); return s; } f()",
        "=> 6",
    ),
    (
        "function f() { var a = 1; a = a + (a = 5); return a; } f()",
        "=> 6",
    ),
    (
        "function f() { var a = 2; return a * (a = 3) + a; } f()",
        "=> 9",
    ),
    (
        "function f() { var x = 1; var y = x + x++ + x; return [x, y].length + ':' + y; } f()",
        "=> 2:4",
    ),
    (
        "function f() { var i = 0; function g(a, b) { return a + '-' + b; } return g(i++, i++); } f()",
        "=> 0-1",
    ),
    (
        "function f() { var o = {}; var log = ''; function k() { log += 'k'; return 'p'; } function v() { log += 'v'; return 1; } o[k()] = v(); return log + o.p; } f()",
        "=> kv1",
    ),
    (
        "function f() { var o = {n: 1}; var t = o; o.n = (o = {n: 2}, 3); return t.n + ' ' + o.n; } f()",
        "=> 3 2",
    ),
    (
        "function f() { var a = [1, 2]; var i = 0; a[i] = (i = 1, 9); return a[0] + ' ' + a[1]; } f()",
        "=> 9 2",
    ),
    (
        "function f() { var x = 'a'; x += x += 'b'; return x; } f()",
        "=> aab",
    ),
    (
        "function f() { let c = 0; const inc = () => c++; inc(); inc(); return c; } f()",
        "=> 2",
    ),
    (
        "{ typeof tdz; let tdz; }",
        "Uncaught ReferenceError: Cannot access 'tdz' before initialization",
    ),
    (
        "var f = function f() { return typeof f; }; f()",
        "=> function",
    ),
    (
        "function outer() { var a = arguments; var arrow = () => arguments[0] + a.length; return arrow('ignored'); } outer('x', 'y')",
        "=> x2",
    ),
    (
        "function a() { var deep = 'v'; return function b() { return function c() { return function d() { return deep; }; }; }; } a()()()()",
        "=> v",
    ),
    (
        "function a() { var x = 1; function b() { function c() { x++; return x; } return c; } var f = b(); f(); return x; } a()",
        "=> 2",
    ),
    (
        "var s = ''; var i = 0; do { i++; if (i == 2) continue; s += i; } while (i < 4); s",
        "=> 134",
    ),
    (
        "var s = ''; for (const c = 'k'; s.length < 3;) { s += c; } s",
        "=> kkk",
    ),
    (
        "var fs = []; for (let i = 0; i < 5; i++) { if (i == 1) continue; if (i == 4) break; fs[fs.length] = function () { return i; }; } var r = ''; for (var j = 0; j < fs.length; j++) r += fs[j](); r",
        "=> 023",
    ),
    ("function f() { { var v = 1; } return v; } f()", "=> 1"),
    (
        "var g = 'global'; function f() { var g = 'local'; return g; } f() + g",
        "=> localglobal",
    ),
    ("var o = {a: 1, a: 2}; o.a", "=> 2"),
    (
        "var a = [1, 2, 3]; delete a[1]; print(a[1], a.length, 1 in a)",
        "undefined 3 false",
    ),
    ("var a = []; a[10] = 1; a.length", "=> 11"),
    ("var o = {}; o[1] = 'one'; o['1'] === o[1]", "=> true"),
    (
        "print('' + 1e21, '' + -0, `${-0}`, '' + 0.1, '' + 1/3)",
        "1e+21 0 0 0.1 0.3333333333333333",
    ),
    (
        "var x = 2147483647; x++; var y = -2147483648; y--; print(x, y, -(-2147483648), x | 0)",
        "2147483648 -2147483649 2147483648 -2147483648",
    ),
    (
        "print(1 < 1.5, 2.5 > 2, 'ab' === 'a' + 'b', 0.1 + 0.2 == 0.3)",
        "true true true false",
    ),
    (
        "function* g() { var self = this; yield typeof self; } var o = {m: g}; o.m().next().value",
        "=> object",
    ),
    (
        "var x = 1; { let x = 2; { let x = 3; print(x); } print(x); } x",
        "3
2
=> 1",
    ),
    (
        "function f(n) { var r = 0; while (n > 0) { let k = n; r += k; n--; } return r; } f(100)",
        "=> 5050",
    ),
    (
        "var o = {get: 1, set: 2, new: 3, typeof: 4}; o.get + o.set + o.new + o.typeof",
        "=> 10",
    ),
    (
        "var n = 0; var o = {valueOf: function () { n++; return 1; }}; o + o; o * o; o < o; n",
        "=> 6",
    ),
];

/// Array `length` conversions, messages and the tie rule of
/// `Number::toString`.
#[test]
fn array_length_messages_and_number_to_string_ties() {
    check_cases(FIXES);
}

/// `Number::toString` of about 5,000 doubles (ties, exact binary
/// fractions above 2^50, random bit patterns). The file holds Node.js 22's
/// string for each double; it is also a valid literal for it, so the
/// script parses the literal and prints it back.
#[test]
fn number_to_string_matches_node() {
    let table = include_str!("data/number_strings.txt");
    common::on_big_stack(move || {
        let mut rt = common::runtime(false);
        let mut failures = Vec::new();
        for literal in table.lines().filter(|l| !l.is_empty()) {
            let out = common::run_in(&mut rt, &format!("print({literal})"));
            if out != literal {
                failures.push(format!("{literal}: {out}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{} mismatches: {:?}",
            failures.len(),
            &failures[..failures.len().min(10)]
        );
    });
}

/// fixes (expected output from Node.js 22).
const FIXES: &[(&str, &str)] = &[
    (
        "print(1000 + 1/16384, 109.589996337890625, 4/3*1e15, 2**50 + 0.25, 2**50 + 0.75, 2**51 + 0.5)",
        "1000.0000610351562 109.58999633789062 1333333333333333.2 1125899906842624.2 1125899906842624.8 2251799813685248.5",
    ),
    (
        "var a = [1, 2, 3]; a.length = '1'; print(a.length, a[1], a[0])",
        "1 undefined 1",
    ),
    (
        "var a = [1, 2, 3]; var n = 0; a.length = {valueOf() { n++; return 2; }}; print(a.length, n)",
        "2 2",
    ),
    (
        "var a = [1, 2, 3]; a.length = {toString() { return '1'; }}; print(a.length)",
        "1",
    ),
    ("var a = [1, 2, 3]; a.length += '1'; print(a.length)", "31"),
    (
        "var a = [1, 2, 3]; a.length = true; print(a.length); a.length = null; print(a.length); a.length = ' 7 '; print(a.length)",
        "1
0
7",
    ),
    (
        "var a = [1, 2, 3]; var r = (a.length = '2'); print(r, a.length)",
        "2 2",
    ),
    (
        "var a = [1, 2, 3]; a.length = 'abc'",
        "Uncaught RangeError: Invalid array length",
    ),
    (
        "var a = [1, 2, 3]; a.length = -1",
        "Uncaught RangeError: Invalid array length",
    ),
    (
        "var a = [1, 2, 3]; a.length = 1.5",
        "Uncaught RangeError: Invalid array length",
    ),
    (
        "var a = [1, 2, 3]; a.length = 2 ** 32",
        "Uncaught RangeError: Invalid array length",
    ),
    (
        "var a = [1, 2, 3]; a.length = undefined",
        "Uncaught RangeError: Invalid array length",
    ),
    (
        "var a = [1, 2, 3]; a.length = NaN",
        "Uncaught RangeError: Invalid array length",
    ),
    (
        "var a = [1, 2, 3]; a.length = {valueOf() { throw 7; }}",
        "Uncaught 7",
    ),
    (
        "'use strict'; var a = [1, 2, 3]; a.length = '0'; print(a.length)",
        "0",
    ),
    (
        "var a = [1, 2, 3]; a['length'] = '2'; print(a.length); var k = 'length'; a[k] = '1'; print(a.length)",
        "2
1",
    ),
    (
        "undefined[print()] = 1",
        "
Uncaught TypeError: Cannot set properties of undefined (setting 'undefined')",
    ),
    (
        "null[print()] = 1",
        "
Uncaught TypeError: Cannot set properties of null (setting 'undefined')",
    ),
    (
        "var o; o[print()] = 1",
        "
Uncaught TypeError: Cannot set properties of undefined (setting 'undefined')",
    ),
    (
        "var o; o[print()]",
        "
Uncaught TypeError: Cannot read properties of undefined (reading 'undefined')",
    ),
    (
        "var o; delete o[print()]",
        "
Uncaught TypeError: Cannot convert undefined or null to object",
    ),
    (
        "'use strict'; 'abc'[0] = 'x'",
        "Uncaught TypeError: Cannot assign to read only property '0' of string 'abc'",
    ),
    (
        "'use strict'; var s = 'abc'; s[2] = 'x'",
        "Uncaught TypeError: Cannot assign to read only property '2' of string 'abc'",
    ),
    (
        "'use strict'; 'abc'.length = 5",
        "Uncaught TypeError: Cannot assign to read only property 'length' of string 'abc'",
    ),
    (
        "'use strict'; 'abc'[5] = 'x'",
        "Uncaught TypeError: Cannot create property '5' on string 'abc'",
    ),
    (
        "'use strict'; 'abc'.foo = 'x'",
        "Uncaught TypeError: Cannot create property 'foo' on string 'abc'",
    ),
    ("'abc'[0] = 'x'; 'abc'.length = 1; print('ok')", "ok"),
    (
        "'use strict'; function f() {} delete f.prototype",
        "Uncaught TypeError: Cannot delete property 'prototype' of function f() {}",
    ),
    (
        "'use strict'; var f = function () {}; delete f.prototype",
        "Uncaught TypeError: Cannot delete property 'prototype' of function () {}",
    ),
    (
        "'use strict'; var f = function g(a, b) { return a + b; }; delete f.prototype",
        "Uncaught TypeError: Cannot delete property 'prototype' of function g(a, b) { return a + b; }",
    ),
    (
        "'use strict'; var f = function*() { yield 1; }; delete f.prototype",
        "Uncaught TypeError: Cannot delete property 'prototype' of function*() { yield 1; }",
    ),
    (
        "'use strict'; var f = function () { /* long comment xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx */ return 1234567890; }; delete f.prototype",
        "Uncaught TypeError: Cannot delete property 'prototype' of function () { /* long comment xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx...<omitted>... }",
    ),
    (
        "'use strict'; var f = function () { /*abcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghij*/ }; delete f.prototype",
        "Uncaught TypeError: Cannot delete property 'prototype' of function () { /*abcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghij*/ }",
    ),
    (
        "'use strict'; function f() {} f.name = 'x'",
        "Uncaught TypeError: Cannot assign to read only property 'name' of function 'function f() {}'",
    ),
    (
        "'use strict'; function f(a) { return 1; } f.length = 3",
        "Uncaught TypeError: Cannot assign to read only property 'length' of function 'function f(a) { return 1; }'",
    ),
    (
        "'use strict'; var f = () => 1; f.name = 'x'",
        "Uncaught TypeError: Cannot assign to read only property 'name' of function '() => 1'",
    ),
    (
        "'use strict'; var o = {m(a) { return 1; }}; o.m.name = 1",
        "Uncaught TypeError: Cannot assign to read only property 'name' of function 'm(a) { return 1; }'",
    ),
    (
        "'use strict'; var o = {}; defineAccessor(o, 'x', function () { return 1; }); o.x = 2",
        "Uncaught TypeError: Cannot set property x of #<Object> which has only a getter",
    ),
    (
        "'use strict'; var f = function () {}; defineAccessor(f, 'x', function () { return 1; }); f.x = 2",
        "Uncaught TypeError: Cannot set property x of function () {} which has only a getter",
    ),
    ("new 5", "Uncaught TypeError: 5 is not a constructor"),
    ("1()", "Uncaught TypeError: 1 is not a function"),
    ("'a'()", r#"Uncaught TypeError: "a" is not a function"#),
    ("new 'a'", r#"Uncaught TypeError: "a" is not a constructor"#),
    ("null()", "Uncaught TypeError: null is not a function"),
    ("true()", "Uncaught TypeError: true is not a function"),
    ("false()", "Uncaught TypeError: false is not a function"),
    ("1.5()", "Uncaught TypeError: 1.5 is not a function"),
    ("new null", "Uncaught TypeError: null is not a constructor"),
    ("new true", "Uncaught TypeError: true is not a constructor"),
    ("0x10()", "Uncaught TypeError: 16 is not a function"),
    ("1e21()", "Uncaught TypeError: 1e+21 is not a function"),
    (
        "10000000000000000000000()",
        "Uncaught TypeError: 1e+22 is not a function",
    ),
    (
        "'a b'.x()",
        r#"Uncaught TypeError: "a b".x is not a function"#,
    ),
    ("5..x()", "Uncaught TypeError: 5.x is not a function"),
    (
        "var o = {}; o.x()",
        "Uncaught TypeError: o.x is not a function",
    ),
    ("this()", "Uncaught TypeError: this is not a function"),
    (
        "undefined()",
        "Uncaught TypeError: undefined is not a function",
    ),
    (
        "var before = 1; function undefined() {}",
        "Uncaught SyntaxError: Identifier 'undefined' has already been declared",
    ),
    (
        "function NaN() {}",
        "Uncaught SyntaxError: Identifier 'NaN' has already been declared",
    ),
    ("var q = 1; var NaN; print(q)", "1"),
    ("var Infinity; print(typeof Infinity)", "number"),
];

// --- Exceptions, `finally`, `switch`, generators with exceptions,
// built-ins and `console.log`. ---

#[test]
fn exceptions() {
    check_cases(EXCEPTIONS);
}

#[test]
fn completions() {
    check_cases(COMPLETIONS);
}

#[test]
fn switches() {
    check_cases(SWITCHES);
}

#[test]
fn generator_exceptions() {
    check_cases(GENERATOR_EXCEPTIONS);
}

#[test]
fn builtins_errors() {
    check_cases(BUILTINS_ERRORS);
}

#[test]
fn builtins_arrays() {
    check_cases(BUILTINS_ARRAYS);
}

#[test]
fn builtins_objects() {
    check_cases(BUILTINS_OBJECTS);
}

#[test]
fn builtins_primitives() {
    check_cases(BUILTINS_PRIMITIVES);
}

#[test]
fn console() {
    check_cases(CONSOLE);
}

/// `exceptions` (expected output from Node.js 22).
const EXCEPTIONS: &[(&str, &str)] = &[
    (
        "try { throw 1; } catch (e) { print('caught', e); }",
        "caught 1",
    ),
    (
        "try { throw 'x'; } catch { print('no binding'); }",
        "no binding",
    ),
    (
        "try { print('body'); } catch (e) { print('not here'); }",
        "body",
    ),
    (
        "try { undefinedName; } catch (e) { print(e.name, e.message, e instanceof ReferenceError, e instanceof Error); }",
        "ReferenceError undefinedName is not defined true true",
    ),
    (
        "try { null.x; } catch (e) { print(e.constructor === TypeError, e.message); }",
        "true Cannot read properties of null (reading 'x')",
    ),
    (
        "try { undefined(); } catch (e) { print(e.message); }",
        "undefined is not a function",
    ),
    (
        "var r = []; for (var i = 0; i < 3; i++) { try { if (i == 1) throw i; r.push('ok' + i); } catch (e) { r.push('caught' + e); } } r.join()",
        "=> ok0,caught1,ok2",
    ),
    (
        "function thrower() { throw new TypeError('from callee'); } try { thrower(); } catch (e) { print(e.name + ': ' + e.message); }",
        "TypeError: from callee",
    ),
    (
        "function a() { b(); } function b() { c(); } function c() { throw 'deep'; } try { a(); } catch (e) { print(e); } print('after')",
        "deep
after",
    ),
    (
        "var o = {}; defineAccessor(o, 'g', function () { throw 'getter'; }); try { o.g; } catch (e) { print('caught', e); }",
        "caught getter",
    ),
    (
        "var o = {}; defineAccessor(o, 's', undefined, function (v) { throw 'setter ' + v; }); try { o.s = 5; } catch (e) { print(e); }",
        "setter 5",
    ),
    (
        "try { callTwice(function () { throw 'through native'; }, 1); } catch (e) { print('caught', e); }",
        "caught through native",
    ),
    (
        "try { later(function () { throw 'deferred'; }); } catch (e) { print(e); }",
        "deferred",
    ),
    (
        "try { [1, 2, 3].forEach(function (x) { if (x == 2) throw 'at ' + x; print('saw', x); }); } catch (e) { print('caught', e); }",
        "saw 1
caught at 2",
    ),
    (
        "function f(n) { return f(n + 1) + 1; } var msgs = []; for (var k = 0; k < 3; k++) { try { f(0); } catch (e) { msgs.push(e instanceof RangeError ? e.message : 'other'); } } msgs.join(' | ')",
        "=> Maximum call stack size exceeded | Maximum call stack size exceeded | Maximum call stack size exceeded",
    ),
    (
        "function f() { f(); } try { f(); } catch (e) { print(e.name); } try { f(); } catch (e) { print('again', e.name); } 'usable'",
        "RangeError
again RangeError
=> usable",
    ),
    (
        "try { try { throw 'inner'; } catch (e) { throw e + ' rethrown'; } } catch (e) { print(e); }",
        "inner rethrown",
    ),
    (
        "try { try { throw 1; } finally { print('inner finally'); } } catch (e) { print('outer caught', e); }",
        "inner finally
outer caught 1",
    ),
    (
        "try { try { throw 1; } catch (e) { throw 2; } finally { print('finally runs'); } } catch (e) { print('got', e); }",
        "finally runs
got 2",
    ),
    (
        "try { try { throw 1; } finally { throw 2; } } catch (e) { print('finally wins', e); }",
        "finally wins 2",
    ),
    (
        "var log = []; function f() { try { log.push('try'); return 'r'; } catch (e) { log.push('catch'); } finally { log.push('finally'); } } log.push(f()); log.join()",
        "=> try,finally,r",
    ),
    (
        "var fs = []; for (var i = 0; i < 3; i++) { try { throw 'v' + i; } catch (e) { fs.push(function () { return e; }); } } fs[0]() + fs[1]() + fs[2]()",
        "=> v0v1v2",
    ),
    (
        "var fs = []; for (let i = 0; i < 3; i++) { try { throw i; } catch (e) { var j = e * 10; fs.push(() => e + j); } } print(fs[0](), fs[1](), fs[2]())",
        "20 21 22",
    ),
    ("try { throw {code: 42}; } catch (e) { e.code }", "=> 42"),
    (
        "var e = 'outer'; try { throw 'inner'; } catch (e) { var e = 'assigned'; } e",
        "=> outer",
    ),
    (
        "function f() { try { throw 1; } catch (e) { return typeof e; } } f()",
        "=> number",
    ),
    (
        "var n = 0; while (true) { try { n++; if (n > 3) break; } finally { print('f', n); } } n",
        "f 1
f 2
f 3
f 4
=> 4",
    ),
    (
        "var count = 0; for (var i = 0; i < 100000; i++) { try { throw i; } catch (e) { count += e & 1; } } count",
        "=> 50000",
    ),
    (
        "try { throw undefined; } catch (e) { print(e === undefined); }",
        "true",
    ),
    (
        "function f() { try { return g(); } catch (e) { return 'caught ' + e; } } function g() { throw 'g'; } f()",
        "=> caught g",
    ),
    (
        "var x = 1; try { x = 2; throw 0; } catch (e) { x += 10; } finally { x *= 2; } x",
        "=> 24",
    ),
    (
        "throw new RangeError('top level')",
        "Uncaught RangeError: top level",
    ),
    ("throw 'a string'", "Uncaught a string"),
    (
        "throw {message: 'custom', name: 'Mine'}",
        "Uncaught Mine: custom",
    ),
    (
        "function f() { throw new Error('inside', {cause: 'why'}); } try { f(); } catch (e) { print(e.message, e.cause); }",
        "inside why",
    ),
    (
        "try { throw 1 } catch (e) { try { throw 2 } catch (e) { print('inner', e) } print('outer', e) }",
        "inner 2
outer 1",
    ),
    (
        "try { let a = 1; throw a; } catch (e) { let a = e + 1; print(a); } finally { let a = 'f'; print(a); }",
        "2
f",
    ),
    (
        "function f() { try { return 'try'; } finally { print('cleanup'); } } print(f())",
        "cleanup
try",
    ),
    (
        "var o = { valueOf: function () { throw 'valueOf'; } }; try { o + 1; } catch (e) { print('caught', e); }",
        "caught valueOf",
    ),
    (
        "try { new (function () { throw 'ctor'; })(); } catch (e) { print(e); }",
        "ctor",
    ),
    (
        "try { 1(); } catch (e) { print(e.message); } try { new 5; } catch (e) { print(e.message); }",
        "1 is not a function
5 is not a constructor",
    ),
];

/// `completions` (expected output from Node.js 22).
const COMPLETIONS: &[(&str, &str)] = &[
    (
        "function f() { try { return 'try'; } finally { return 'finally'; } } f()",
        "=> finally",
    ),
    (
        "function f() { try { throw 'x'; } finally { return 'finally overrides throw'; } } f()",
        "=> finally overrides throw",
    ),
    (
        "function f() { try { return 'a'; } finally { try { return 'b'; } finally { print('inner'); } } } f()",
        "inner
=> b",
    ),
    (
        "function f() { for (var i = 0; i < 3; i++) { try { if (i == 1) return 'returned ' + i; } finally { print('fin', i); } } } f()",
        "fin 0
fin 1
=> returned 1",
    ),
    (
        "var out = []; outer: for (var i = 0; i < 3; i++) { for (var j = 0; j < 3; j++) { try { try { if (j == 1) continue outer; if (i == 2) break outer; out.push(i + '' + j); } finally { out.push('f1'); } } finally { out.push('f2'); } } } out.join()",
        "=> 00,f1,f2,f1,f2,10,f1,f2,f1,f2,f1,f2",
    ),
    (
        "var out = []; for (var i = 0; i < 4; i++) { try { if (i % 2) continue; out.push(i); } finally { out.push('f' + i); } } out.join()",
        "=> 0,f0,f1,2,f2,f3",
    ),
    (
        "var out = []; a: { try { out.push('in'); break a; } finally { out.push('finally'); } out.push('never'); } out.join()",
        "=> in,finally",
    ),
    (
        "var out = []; for (var i = 0; i < 3; i++) { try { break; } finally { out.push('f'); continue; } } out.join() + i",
        "=> f,f,f3",
    ),
    (
        "function f() { while (true) { try { return 'r'; } finally { break; } } return 'after loop'; } f()",
        "=> after loop",
    ),
    (
        "function f() { l: try { return 'r'; } finally { break l; } return 'label'; } f()",
        "=> label",
    ),
    (
        "var out = []; for (var i = 0; i < 2; i++) { switch (i) { case 0: try { out.push('case0'); break; } finally { out.push('fin0'); } case 1: out.push('case1'); } } out.join()",
        "=> case0,fin0,case1",
    ),
    (
        "function f(x) { switch (x) { case 1: try { return 'one'; } finally { print('cleanup one'); } default: return 'other'; } } print(f(1), f(2))",
        "cleanup one
one other",
    ),
    (
        "function f() { var log = []; for (var i = 0; i < 2; i++) { for (var j = 0; j < 2; j++) { try { if (j) break; log.push(i + ':' + j); } finally { log.push('f'); } } } return log.join(); } f()",
        "=> 0:0,f,f,1:0,f,f",
    ),
    (
        "function f() { try { try { return 1; } finally { print('a'); } } finally { print('b'); } } f()",
        "a
b
=> 1",
    ),
    (
        "function f() { try { try { throw 'x'; } finally { print('a'); } } catch (e) { return 'caught ' + e; } finally { print('b'); } } f()",
        "a
b
=> caught x",
    ),
    (
        "function f() { var i = 0; do { try { i++; if (i < 3) continue; return i; } finally { print('fin', i); } } while (true); } f()",
        "fin 1
fin 2
fin 3
=> 3",
    ),
    (
        "function f() { try { return (function () { try { throw 'inner'; } finally { print('in fn'); } })(); } catch (e) { return 'outer ' + e; } } f()",
        "in fn
=> outer inner",
    ),
    (
        "var log = []; function f() { try { log.push(1); return log.push(2); } finally { log.push(3); } } print(f(), log.join())",
        "2 1,2,3",
    ),
    (
        "var n = 0; for (;;) { try { n++; if (n == 5) break; continue; } finally { if (n == 3) n = 4; } } n",
        "=> 5",
    ),
    (
        "var x = 0; l1: for (;;) { l2: for (;;) { try { try { x++; break l1; } finally { x += 10; } } finally { x += 100; } } } x",
        "=> 111",
    ),
    (
        "function f() { try { return 'x'; } finally { } } f()",
        "=> x",
    ),
];

/// `switches` (expected output from Node.js 22).
const SWITCHES: &[(&str, &str)] = &[
    (
        "switch (2) { case 1: print('one'); case 2: print('two'); case 3: print('three'); break; case 4: print('four'); }",
        "two
three",
    ),
    (
        "switch ('x') { case 'y': print('y'); break; default: print('default'); case 'z': print('z'); }",
        "default
z",
    ),
    ("switch (5) { case 1: print(1); }", ""),
    (
        "function f(v) { switch (v) { case 1: return 'num'; case '1': return 'str'; default: return 'none'; } } print(f(1), f('1'), f(true))",
        "num str none",
    ),
    (
        "var log = []; function k(v) { log.push(v); return v; } switch (k(3)) { case k(1): case k(3): log.push('match'); break; case k(4): } log.join()",
        "=> 3,1,3,match",
    ),
    (
        "var log = []; function k(v) { log.push(v); return v; } switch (k(9)) { case k(1): break; default: log.push('d'); case k(2): log.push('two'); } log.join()",
        "=> 9,1,2,d,two",
    ),
    ("switch (0) { case 0: let a = 'block'; print(a); }", "block"),
    (
        "try { switch (1) { case 0: let a = 1; case 1: a; } } catch (e) { print(e.name, e.message); }",
        "ReferenceError Cannot access 'a' before initialization",
    ),
    (
        "switch (1) { case 1: print(inner()); function inner() { return 'hoisted'; } } 'end'",
        "hoisted
=> end",
    ),
    (
        "var out = []; for (var i = 0; i < 4; i++) { switch (i) { case 1: continue; case 2: out.push('two'); break; default: out.push(i); } out.push('end' + i); } out.join()",
        "=> 0,end0,two,end2,3,end3",
    ),
    (
        "lbl: switch (1) { case 1: for (var i = 0; i < 3; i++) { if (i == 1) break lbl; print(i); } print('not here'); } print('out')",
        "0
out",
    ),
    (
        "var fs = []; switch (1) { case 1: let v = 'cap'; fs.push(() => v); } fs[0]()",
        "=> cap",
    ),
    (
        "switch (NaN) { case NaN: print('nan'); break; default: print('no match for NaN'); }",
        "no match for NaN",
    ),
    ("var x = 0; switch (x++) { case 0: x += 10; } x", "=> 11"),
];

/// `generator_exceptions` (expected output from Node.js 22).
const GENERATOR_EXCEPTIONS: &[(&str, &str)] = &[
    (
        "function* g() { yield 1; throw new Error('gen error'); } var it = g(); it.next(); try { it.next(); } catch (e) { print('caught', e.message); } var r = it.next(); print(r.value, r.done)",
        "caught gen error
undefined true",
    ),
    (
        "function* g() { try { yield 1; yield 2; } finally { print('cleanup'); } } var it = g(); print(it.next().value); var r = it.return(42); print(r.value, r.done); print(it.next().done)",
        "1
cleanup
42 true
true",
    ),
    (
        "function* g() { try { yield 1; } catch (e) { print('caught in gen', e); yield 'recovered'; } } var it = g(); it.next(); var r = it.throw('E'); print(r.value, r.done); print(it.next().done)",
        "caught in gen E
recovered false
true",
    ),
    (
        "function* g() { yield 1; } var it = g(); try { it.throw('before start'); } catch (e) { print('thrown', e); } print(it.next().done)",
        "thrown before start
true",
    ),
    (
        "function* g() { print('never'); yield 1; } var it = g(); var r = it.return('early'); print(r.value, r.done, it.next().done)",
        "early true true",
    ),
    (
        "function* g() { yield 1; } var it = g(); it.next(); it.next(); print(it.return('x').value); try { it.throw('y'); } catch (e) { print('done gen throws', e); }",
        "x
done gen throws y",
    ),
    (
        "function* g() { try { yield 1; } finally { yield 'from finally'; print('after finally yield'); } } var it = g(); it.next(); var a = it.return('R'); print(a.value, a.done); var b = it.next(); print(b.value, b.done)",
        "from finally false
after finally yield
R true",
    ),
    (
        "function* g() { try { yield 1; } finally { return 'finally return'; } } var it = g(); it.next(); var r = it.return('R'); print(r.value, r.done)",
        "finally return true",
    ),
    (
        "function* g() { try { try { yield 1; } finally { print('inner'); } } finally { print('outer'); } } var it = g(); it.next(); print(it.return(7).value)",
        "inner
outer
7",
    ),
    (
        "function* g() { while (true) { try { yield 'tick'; } catch (e) { print('caught', e); } } } var it = g(); it.next(); it.throw(1); it.throw(2); print(it.next().value)",
        "caught 1
caught 2
tick",
    ),
    (
        "function* g() { yield 1; } var it = g(); it.next(); try { it.throw(new TypeError('t')); } catch (e) { print(e.name); }",
        "TypeError",
    ),
    (
        "function* g() { it.return(5); } var it = g(); try { it.next(); } catch (e) { print(e.name, e.message); }",
        "TypeError Generator is already running",
    ),
    (
        "function* g() { it.throw(5); } var it = g(); try { it.next(); } catch (e) { print(e.message); }",
        "Generator is already running",
    ),
    (
        "function* g() { yield 1; } var it = g(); try { it.next.call({}); } catch (e) { print(e.message); } try { it.return.call(1); } catch (e) { print(e.message); }",
        "Method [Generator].prototype.next called on incompatible receiver #<Object>
Method [Generator].prototype.return called on incompatible receiver 1",
    ),
    (
        "function* g() { var x = yield 1; print('got', x); try { yield 2; } catch (e) { print('c', e); } return 'end'; } var it = g(); it.next(); it.next('v'); var r = it.throw('t'); print(r.value, r.done)",
        "got v
c t
end true",
    ),
    (
        "function* g() { for (var i = 0; i < 3; i++) { try { yield i; } finally { print('f' + i); } } } var it = g(); it.next(); it.next(); it.return(); print(it.next().done)",
        "f0
f1
true",
    ),
    (
        "function* g() { yield 1; yield 2; } var it = g(); it.next(); print(JSON_missing_is_fine = 1); it.return('a').value",
        "1
=> a",
    ),
    (
        "function* g() { try { yield 1; } finally { throw 'from finally'; } } var it = g(); it.next(); try { it.return('x'); } catch (e) { print('caught', e); } print(it.next().done)",
        "caught from finally
true",
    ),
];

/// `builtins_errors` (expected output from Node.js 22).
const BUILTINS_ERRORS: &[(&str, &str)] = &[
    (
        "var e = new Error('m'); print(e.message, e.name, String(e), e instanceof Error, Object.keys(e).length)",
        "m Error Error: m true 0",
    ),
    (
        "var e = Error('no new'); print(e.message, e instanceof Error)",
        "no new true",
    ),
    (
        "print(TypeError.name, TypeError.length, Error.length, typeof RangeError, URIError.name, EvalError.prototype.name)",
        "TypeError 1 1 function URIError EvalError",
    ),
    (
        "print(new TypeError('t') instanceof Error, new SyntaxError('s') instanceof SyntaxError, new RangeError() instanceof TypeError)",
        "true true false",
    ),
    (
        "print(TypeError.prototype.constructor === TypeError, Error.prototype.name, Error.prototype.message === '', TypeError.prototype.message === '')",
        "true Error true true",
    ),
    (
        "Error.marker = 'inherited'; print(TypeError.marker, URIError.marker, TypeError.prototype instanceof Error, Error.prototype instanceof Error)",
        "inherited inherited true false",
    ),
    (
        "var e = new Error('m', {cause: 'c'}); print(e.cause, Object.keys(e).length, new Error('x', {}).cause, 'cause' in new Error('x', 5))",
        "c 0 undefined false",
    ),
    (
        "print(new Error().message === '', 'message' in new Error(), new Error(undefined).hasOwnProperty('message'), new Error('x').hasOwnProperty('message'))",
        "true true false true",
    ),
    (
        "print(String(new Error('')), String(new TypeError('tt')), String(new ReferenceError()))",
        "Error TypeError: tt ReferenceError",
    ),
    (
        "print(Error.prototype.toString.call({name: 'N', message: 'M'}), Error.prototype.toString.call({}), Error.prototype.toString.call({name: '', message: 'only'}))",
        "N: M Error only",
    ),
    (
        "try { Error.prototype.toString.call(1); } catch (e) { print(e.message); }",
        "Method Error.prototype.toString called on incompatible receiver 1",
    ),
    (
        "var e = new Error({toString: function () { return 'converted'; }}); e.message",
        "=> converted",
    ),
    (
        "var e = new RangeError('r'); e.name = 'Custom'; String(e)",
        "=> Custom: r",
    ),
    (
        "try { null.f(); } catch (e) { print(e.constructor.name, Object.keys(e).join(), e.toString()); }",
        "TypeError  TypeError: Cannot read properties of null (reading 'f')",
    ),
    (
        "try { (function () { 'use strict'; return arguments.callee; })(); } catch (e) { print(e.name, e.message); }",
        "TypeError 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them",
    ),
    (
        "(function () { 'use strict'; try { arguments.callee = 1; } catch (e) { print('set', e.name); } try { return typeof arguments.callee; } catch (e) { return 'get ' + e.name; } })()",
        "set TypeError
=> get TypeError",
    ),
    (
        "(function () { return typeof arguments.callee; })()",
        "=> function",
    ),
    (
        "print(typeof Error.prototype.toString, Error.prototype.toString.name, Error.prototype.toString.length)",
        "function toString 0",
    ),
    (
        "var o = {}; o.f = TypeError; var e = new o.f('via member'); e.message",
        "=> via member",
    ),
];

/// `builtins_arrays` (expected output from Node.js 22).
const BUILTINS_ARRAYS: &[(&str, &str)] = &[
    (
        "var a = [1, 2]; print(a.push(3, 4), a.length, a.join('-'))",
        "4 4 1-2-3-4",
    ),
    (
        "print([1, [2, [3, 4]], 5].join(), [null, undefined, 0].join(), [].join(), [1, 2].join(undefined))",
        "1,2,3,4,5 ,,0  1,2",
    ),
    (
        "var a = [1, 2]; a.push(a); print(a.join(), String(a))",
        "1,2, 1,2,",
    ),
    ("var a = [1, 2]; a.push([3, a]); String(a)", "=> 1,2,3,"),
    (
        "print(Array.prototype.join.call({length: 3, 0: 'a', 2: 'c'}), Array.prototype.join.call('abc', '+'))",
        "a,,c a+b+c",
    ),
    (
        "var o = {length: 2}; Array.prototype.push.call(o, 'x'); print(o.length, o[2])",
        "3 x",
    ),
    (
        "var o = {}; print(Array.prototype.push.call(o), o.length)",
        "0 0",
    ),
    (
        "try { Array.prototype.push.call({length: 2 ** 53 - 1}, 1); } catch (e) { print(e.message); }",
        "Pushing 1 elements on an array-like of length 9007199254740991 is disallowed, as the total surpasses 2**53-1",
    ),
    (
        "var a = []; a.length = 4294967295; try { a.push(1); } catch (e) { print(e.name, e.message); }",
        "RangeError Invalid array length",
    ),
    (
        "var sum = 0; [1, 2, 3].forEach(function (x, i, arr) { sum += x * i + arr.length; }); sum",
        "=> 17",
    ),
    (
        "var seen = []; [1, , 3].forEach(function (x, i) { seen.push(i + ':' + x); }); seen.join()",
        "=> 0:1,2:3",
    ),
    (
        "var o = {v: 10}; var r = []; [1, 2].forEach(function (x) { r.push(this.v + x); }, o); r.join()",
        "=> 11,12",
    ),
    (
        "var r = []; Array.prototype.forEach.call({length: 2, 0: 'a', 1: 'b'}, function (x) { r.push(x); }); r.join()",
        "=> a,b",
    ),
    (
        "var a = [1, 2, 3]; var n = 0; a.forEach(function (x) { if (x == 1) a.push(99); n++; }); print(n, a.length)",
        "3 4",
    ),
    (
        "try { [1].forEach(1); } catch (e) { print(e.message); } try { [1].map(); } catch (e) { print(e.message); } try { [1].forEach('s'); } catch (e) { print(e.message); }",
        r#"number 1 is not a function
undefined is not a function
string "s" is not a function"#,
    ),
    (
        "try { [1].forEach({}); } catch (e) { print(e.message); } try { [1].map(null); } catch (e) { print(e.message); }",
        "object is not a function
object null is not a function",
    ),
    (
        "var m = [1, 2, 3].map(function (x, i) { return x * 10 + i; }); print(m.join(), m.length, Array.isArray(m))",
        "10,21,32 3 true",
    ),
    (
        "var m = [1, , 3].map(function (x) { return x * 2; }); print(m.length, 1 in m, m.join())",
        "3 false 2,,6",
    ),
    (
        "var m = Array.prototype.map.call('ab', function (c) { return c + c; }); m.join()",
        "=> aa,bb",
    ),
    (
        "print(Array.isArray([]), Array.isArray({}), Array.isArray('a'), Array.isArray(), Array.isArray.length, Array.isArray.name)",
        "true false false false 1 isArray",
    ),
    (
        "print(Array(3).length, Array(1, 2).join(), new Array().length, Array('3').length, new Array(2.0).length, Array(0).length)",
        "3 1,2 0 1 2 0",
    ),
    (
        "try { Array(-1); } catch (e) { print(e.name, e.message); } try { new Array(1.5); } catch (e) { print(e.message); }",
        "RangeError Invalid array length
Invalid array length",
    ),
    (
        "var a = new Array(2); a[5] = 'x'; print(a.length, a.join('.'))",
        "6 .....x",
    ),
    (
        "print([1, 2, 3].toString(), String([]), [] + [], [1] + 1, typeof Array.prototype.toString)",
        "1,2,3   11 function",
    ),
    (
        "var o = {join: 1}; print(Array.prototype.toString.call(o))",
        "[object Object]",
    ),
    (
        "var o = {join: function () { return 'custom join'; }}; Array.prototype.toString.call(o)",
        "=> custom join",
    ),
    (
        "var a = [{toString: function () { return 'T'; }}, 1]; a.join('|')",
        "=> T|1",
    ),
    (
        "var a = []; for (var i = 0; i < 500; i++) a.push(i); var total = 0; a.forEach(function (x) { total += x; }); print(total, a.map(function (x) { return x * 2; })[499])",
        "124750 998",
    ),
    (
        "print(Array.prototype.push.length, Array.prototype.join.length, Array.prototype.forEach.length, Array.prototype.map.length, Array.length, Array.name)",
        "1 1 1 1 1 Array",
    ),
    (
        "print(Array.prototype.constructor === Array, [].constructor === Array, [] instanceof Array, Object.keys(Array.prototype).length)",
        "true true true 0",
    ),
    (
        "var o = {}; defineAccessor(o, 'length', function () { print('length read'); return 2; }); defineAccessor(o, 0, function () { return 'g0'; }); Array.prototype.join.call(o)",
        "length read
=> g0,",
    ),
    (
        "var r = [1, 2, 3].map(function (x) { gc(); return {v: x}; }); r[2].v",
        "=> 3",
    ),
];

/// `builtins_objects` (expected output from Node.js 22).
const BUILTINS_OBJECTS: &[(&str, &str)] = &[
    (
        "Object.keys({a: 1, b: 2, 1: 'x', 0: 'y'}).join()",
        "=> 0,1,a,b",
    ),
    (
        "print(Object.keys([1, , 3]).join(), Object.keys('ab').join(), Object.keys(new String('xyz')).join(), Object.keys(5).length)",
        "0,2 0,1 0,1,2 0",
    ),
    (
        "try { Object.keys(null); } catch (e) { print(e.message); } try { Object.keys(); } catch (e) { print(e.name); }",
        "Cannot convert undefined or null to object
TypeError",
    ),
    (
        "var o = {}; defineAccessor(o, 'acc', function () { return 1; }); o.plain = 2; Object.keys(o).join()",
        "=> acc,plain",
    ),
    (
        "function F() { this.own = 1; } F.prototype.inherited = 2; Object.keys(new F()).join()",
        "=> own",
    ),
    (
        "print(Object.keys.length, Object.keys.name, Object.length, Object.name, typeof Object)",
        "1 keys 1 Object function",
    ),
    (
        "print(Object.prototype.toString.call(null), Object.prototype.toString.call([]), Object.prototype.toString.call(function () {}), Object.prototype.toString.call(new Error('x')))",
        "[object Null] [object Array] [object Function] [object Error]",
    ),
    (
        "print(Object.prototype.toString.call(1), Object.prototype.toString.call('s'), Object.prototype.toString.call(true), Object.prototype.toString.call(undefined))",
        "[object Number] [object String] [object Boolean] [object Undefined]",
    ),
    (
        "print(String({}), String(Object.prototype), (function () { return Object.prototype.toString.call(arguments); })())",
        "[object Object] [object Object] [object Arguments]",
    ),
    (
        "print(typeof Object(1), Object(1) instanceof Number, typeof Object(), Object(null) instanceof Object, new Object('s') instanceof String)",
        "object true object true true",
    ),
    (
        "var o = {a: 1}; print(Object(o) === o, o.valueOf() === o, typeof Object.prototype.valueOf.call(2))",
        "true true object",
    ),
    (
        "print(Function.prototype.call.length, Function.prototype.apply.length, Function.prototype.toString.length, Function.prototype.call.name)",
        "1 2 0 call",
    ),
    (
        "function add(a, b) { return this.base + a + b; } print(add.call({base: 1}, 2, 3), add.apply({base: 10}, [20, 30]), add.apply({base: 0}, {length: 2, 0: 4, 1: 5}))",
        "6 60 9",
    ),
    (
        "function f() { return arguments.length; } print(f.apply(null), f.apply(null, undefined), f.call(), f.apply(null, []))",
        "0 0 0 0",
    ),
    (
        "try { (function () {}).apply(null, 1); } catch (e) { print(e.message); }",
        "CreateListFromArrayLike called on non-object",
    ),
    (
        "function f() { 'use strict'; return this; } print(f.call(5), typeof f.call(), f.apply('s'))",
        "5 undefined s",
    ),
    (
        "function f() { return typeof this; } print(f.call(5), f.call(null) === undefined)",
        "object false",
    ),
    (
        "function count(n) { return n == 0 ? 0 : 1 + count.call(null, n - 1); } count(2000)",
        "=> 2000",
    ),
    (
        "print(String(function f(a) { return a; }), String(Array.prototype.push))",
        "function f(a) { return a; } function push() { [native code] }",
    ),
    (
        "try { Function.prototype.toString.call({}); } catch (e) { print(e.message); }",
        "Function.prototype.toString requires that 'this' be a Function",
    ),
    (
        "var bound = Function.prototype.call; print(typeof bound, bound.call(function () { return 'inner'; }))",
        "function inner",
    ),
    (
        "print(typeof Function, Function.length, Function.name, Function.prototype.constructor === Function, (function () {}).constructor === Function)",
        "function 1 Function true true",
    ),
    (
        "print({a: 1}.hasOwnProperty('a'), {a: 1}.hasOwnProperty('b'), [1].hasOwnProperty(0), [1].hasOwnProperty('length'), 'ab'.hasOwnProperty(1), new String('ab').hasOwnProperty(2))",
        "true false true true true false",
    ),
];

/// `builtins_primitives` (expected output from Node.js 22).
const BUILTINS_PRIMITIVES: &[(&str, &str)] = &[
    (
        "print(String(123), String(null), String(undefined), String(true), String(-0), String(1e21), String(), String('s'))",
        "123 null undefined true 0 1e+21  s",
    ),
    (
        "print(String([1, 2]), String({}), String(function () {} ) === undefined)",
        "1,2 [object Object] false",
    ),
    (
        "var s = new String('ab'); print(typeof s, s.length, s[1], s instanceof String, String(s), s + 'c', s.toString(), s.valueOf())",
        "object 2 b true ab abc ab ab",
    ),
    (
        "print(Number(undefined), Number(null), Number(''), Number(' 12 '), Number('1e3'), Number([5]), Number(true), Number('0x10'), Number('x'))",
        "NaN 0 0 12 1000 5 1 16 NaN",
    ),
    (
        "print(Number(), typeof Number('1'), new Number(3).valueOf(), new Number(5) + 1, typeof new Number(1), new Number(2) instanceof Number)",
        "0 number 3 6 object true",
    ),
    (
        "print((255).toString(16), (-255).toString(36), (0.5).toString(2), (3.75).toString(2), (10).toString(), (1.5).toString(10), (255).toString(2))",
        "ff -73 0.1 11.11 10 1.5 11111111",
    ),
    (
        "try { (255).toString(1); } catch (e) { print(e.name, e.message); } try { (255).toString(37); } catch (e) { print(e.message); }",
        "RangeError toString() radix argument must be between 2 and 36
toString() radix argument must be between 2 and 36",
    ),
    (
        "try { Number.prototype.toString.call('x'); } catch (e) { print(e.message); } try { Number.prototype.valueOf.call({}); } catch (e) { print(e.message); }",
        "Number.prototype.toString requires that 'this' be a Number
Number.prototype.valueOf requires that 'this' be a Number",
    ),
    (
        "try { String.prototype.toString.call(1); } catch (e) { print(e.message); } try { String.prototype.valueOf.call({}); } catch (e) { print(e.message); }",
        "String.prototype.toString requires that 'this' be a String
String.prototype.valueOf requires that 'this' be a String",
    ),
    (
        "print(String.length, String.name, Number.length, Number.name, String.prototype.constructor === String, (5).constructor === Number)",
        "1 String 1 Number true true",
    ),
    (
        "print('abc'.toString(), (12).valueOf(), (1.25).toString(), typeof (7).toString)",
        "abc 12 1.25 function",
    ),
    (
        "var n = new Number(42); var s = new String('x'); print(n == 42, n === 42, s == 'x', s === 'x')",
        "true false true false",
    ),
    (
        "print(Number('Infinity'), Number('-0') === 0, 1 / Number('-0'), Number('1_000'), Number('.5'), Number('5.'))",
        "Infinity true -Infinity NaN 0.5 5",
    ),
];

/// `console` (expected output from Node.js 22).
const CONSOLE: &[(&str, &str)] = &[
    ("console.log('hello', 'world')", "hello world"),
    (
        "console.log(1, -0, 1.5, NaN, -Infinity, true, null, undefined)",
        "1 -0 1.5 NaN -Infinity true null undefined",
    ),
    (
        "console.log([1, 2, 3], [], [[]], ['a', 'b'])",
        "[ 1, 2, 3 ] [] [ [] ] [ 'a', 'b' ]",
    ),
    (
        "console.log({a: 1, b: 'x'}, {}, {nested: {deeper: {deepest: {gone: 1}}}})",
        "{ a: 1, b: 'x' } {} { nested: { deeper: { deepest: [Object] } } }",
    ),
    (
        "console.log([1, , 3], [, ,], new Array(3), [undefined, null])",
        "[ 1, <1 empty item>, 3 ] [ <2 empty items> ] [ <3 empty items> ] [ undefined, null ]",
    ),
    (
        "var o = {}; o.self = o; console.log(o)",
        "<ref *1> { self: [Circular *1] }",
    ),
    (
        "var a = [1]; a.push(a); console.log(a)",
        "<ref *1> [ 1, [Circular *1] ]",
    ),
    (
        "var o = {x: {}}; o.x.back = o; o.y = o.x; console.log(o)",
        "<ref *1> { x: { back: [Circular *1] }, y: { back: [Circular *1] } }",
    ),
    (
        "console.log(function f() {}, function () {}, () => 1, function* g() {})",
        "[Function: f] [Function (anonymous)] [Function (anonymous)] [GeneratorFunction: g]",
    ),
    (
        "function f() {} f.prop = 1; console.log(f)",
        "[Function: f] { prop: 1 }",
    ),
    (
        "console.log([function named() {}, Array.prototype.push])",
        "[ [Function: named], [Function: push] ]",
    ),
    (
        "console.log(new Number(5), new String('ab'), Object(true))",
        "[Number: 5] [String: 'ab'] [Boolean: true]",
    ),
    (
        "console.log({'a-b': 1, $x: 2, _y: 3, 0: 4, a1: 5, '1a': 6})",
        "{ '0': 4, 'a-b': 1, '$x': 2, _y: 3, a1: 5, '1a': 6 }",
    ),
    (
        r#"console.log(['a\nb', "it's", 'say "hi"', 'tab\t', 'back\\slash'])"#,
        r#"[ 'a\nb', "it's", 'say "hi"', 'tab\t', 'back\\slash' ]"#,
    ),
    (
        "console.log({s: 'single'}, ['x'], 'top level stays raw')",
        "{ s: 'single' } [ 'x' ] top level stays raw",
    ),
    (
        "function Foo() { this.a = 1; } console.log(new Foo())",
        "Foo { a: 1 }",
    ),
    (
        "console.log((function* () {})(), (function () { return arguments; })(1, 2))",
        "Object [Generator] {} [Arguments] { '0': 1, '1': 2 }",
    ),
    (
        "var o = {}; defineAccessor(o, 'g', function () { throw 'never called'; }); defineAccessor(o, 's', undefined, function () {}); console.log(o)",
        "{ g: [Getter], s: [Setter] }",
    ),
    (
        "var a = [1, 2]; a.extra = 'x'; console.log(a)",
        "[ 1, 2, extra: 'x' ]",
    ),
    (
        "console.log([[[[1]]]], {a: [{b: {c: 1}}]})",
        "[ [ [ [Array] ] ] ] { a: [ { b: [Object] } ] }",
    ),
    (
        "var big = []; for (var i = 0; i < 105; i++) big[i] = 0; console.log(big.length, [1, 2, 3, 4, 5, 6])",
        "105 [ 1, 2, 3, 4, 5, 6 ]",
    ),
    (
        "var o = {valueOf: function () { throw 'no'; }, toString: function () { throw 'no'; }}; console.log(o)",
        "{ valueOf: [Function: valueOf], toString: [Function: toString] }",
    ),
    (
        "console.log({a: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx', b: {c: 1}})",
        "{
  a: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx',
  b: { c: 1 }
}",
    ),
    (
        "console.log({o: {a: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'}, p: [1, 2]})",
        "{
  o: {
    a: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'
  },
  p: [ 1, 2 ]
}",
    ),
    (
        "console.log({a: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'}, {a: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'})",
        "{ a: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx' } {
  a: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'
}",
    ),
    (
        "console.log(console.log.name, console.log.length, typeof console)",
        "log 0 object",
    ),
    ("console.log()", ""),
    (
        "console.log(Object.keys({b: 1, a: 2}), [1, 2].map(function (x) { return {v: x}; }))",
        "[ 'b', 'a' ] [ { v: 1 }, { v: 2 } ]",
    ),
    (
        "console.log([new Number(-0), 'x'.length])",
        "[ [Number: -0], 1 ]",
    ),
];

// --- Messages of the built-ins. Expected outputs from Node.js 22. ---

#[test]
fn built_in_messages() {
    check_cases(BUILTINS_MESSAGES);
}

const BUILTINS_MESSAGES: &[(&str, &str)] = &[
    (
        "try { Array.prototype.forEach.call(undefined, function () {}); } catch (e) { print(e.name + ': ' + e.message); }",
        "TypeError: Array.prototype.forEach called on null or undefined",
    ),
    (
        "try { Array.prototype.forEach.call(null, function () {}); } catch (e) { print(e.message); }",
        "Array.prototype.forEach called on null or undefined",
    ),
    (
        "try { Array.prototype.map.call(undefined, function () {}); } catch (e) { print(e.message); }",
        "Array.prototype.map called on null or undefined",
    ),
    (
        "try { Array.prototype.join.call(undefined); } catch (e) { print(e.message); }",
        "Cannot convert undefined or null to object",
    ),
    (
        "try { Array.prototype.push.call(null, 1); } catch (e) { print(e.message); }",
        "Cannot convert undefined or null to object",
    ),
    (
        "'use strict'; try { [].join.name = 'x'; } catch (e) { print(e.message); }",
        "Cannot assign to read only property 'name' of function 'function join() { [native code] }'",
    ),
    (
        "'use strict'; try { Object.keys.length = 5; } catch (e) { print(e.message); }",
        "Cannot assign to read only property 'length' of function 'function keys() { [native code] }'",
    ),
    (
        "'use strict'; try { (function () { arguments.callee = 1; })(); } catch (e) { print(e.message); }",
        "Cannot assign to read only property 'callee' of object '#<Object>'",
    ),
];

// --- M7 feature 3a: `for`-`in`, object literals, optional chaining, tagged
// templates, `new.target`. Expected outputs from Node.js 22. ---

#[test]
fn for_in() {
    check_cases(FOR_IN);
}

#[test]
fn object_literals() {
    check_cases(OBJECT_LITERALS);
}

#[test]
fn optional_chaining() {
    check_cases(OPTIONAL_CHAINING);
}

#[test]
fn tagged_templates() {
    check_cases(TAGGED_TEMPLATES);
}

#[test]
fn new_target() {
    check_cases(NEW_TARGET);
}

const FOR_IN: &[(&str, &str)] = &[
    (
        "var r = []; for (var k in { a: 1, b: 2, 1: 'x', 0: 'y' }) r.push(k); print(r.join(), typeof k);",
        "0,1,a,b string",
    ),
    (
        "var p = { x: 1, y: 2 }; var o = Object.create(p); o.a = 1; o.y = 3; var r = []; for (var k in o) r.push(k); print(r.join());",
        "a,y,x",
    ),
    (
        "var p = { h: 1, v: 2 }; var o = Object.create(p); Object.defineProperty(o, 'h', { value: 0, enumerable: false }); var r = []; for (var k in o) r.push(k); print(r.join());",
        "v",
    ),
    (
        "var r = []; for (var k in null) r.push(k); for (var k2 in undefined) r.push(k2); print(r.length, k, k2);",
        "0 undefined undefined",
    ),
    (
        "var r = []; for (var k in 'ab') r.push(k); for (var k2 in new String('c')) r.push(k2); for (var k3 in 5) r.push(k3); print(r.join());",
        "0,1,0",
    ),
    (
        "var a = [1, , 3]; a.x = 'x'; var r = []; for (var k in a) r.push(k); print(r.join());",
        "0,2,x",
    ),
    (
        "var o = { a: 1, b: 2, c: 3 }; var r = []; for (var k in o) { if (k === 'a') { delete o.b; o.d = 4; } r.push(k); } print(r.join());",
        "a,c",
    ),
    (
        "var p = { z: 1 }; var o = Object.create(p); o.a = 1; var r = []; for (var k in o) { if (k === 'a') { delete p.z; p.w = 2; } r.push(k); } print(r.join());",
        "a",
    ),
    (
        "var o = { a: 1, b: 2 }; var r = []; for (var k in o) { if (k === 'a') Object.defineProperty(o, 'b', { enumerable: false }); r.push(k); } print(r.join());",
        "a,b",
    ),
    (
        "var p = { x: 1 }; var o = Object.create(p); o.a = 1; var r = []; for (var k in o) { Object.setPrototypeOf(o, { y: 1 }); r.push(k); } print(r.join());",
        "a",
    ),
    (
        "var o = Object.create(null); o.b = 1; o[2] = 2; o.a = 3; o[1] = 4; var s = Symbol(); o[s] = 5; var r = []; for (var k in o) r.push(k); print(r.join());",
        "1,2,b,a",
    ),
    (
        "var fns = []; for (let k in { a: 1, b: 2 }) fns.push(function () { return k; }); print(fns[0](), fns[1]());",
        "a b",
    ),
    (
        "var fns = []; for (const k in { x: 1, y: 2 }) fns.push(() => k); print(fns[0](), fns[1]());",
        "x y",
    ),
    (
        "try { for (const k in { x: 1 }) { k = 2; } } catch (e) { print(e.constructor.name, e.message); }",
        "TypeError Assignment to constant variable.",
    ),
    (
        "try { for (let z in z) {} } catch (e) { print(e.constructor.name, e.message); }",
        "ReferenceError Cannot access 'z' before initialization",
    ),
    (
        "var g; try { for (let w in (g = function () { return w; }, { a: 1 })) {} g(); } catch (e) { print(e.constructor.name, e.message); }",
        "ReferenceError Cannot access 'w' before initialization",
    ),
    (
        "var t = {}; for (t.p in { a: 1, b: 2 }) {} var u = []; var i = 0; for (u[i++] in { x: 1, y: 2 }) {} print(t.p, u.join(), i);",
        "b x,y 2",
    ),
    (
        "for (var v = 'init' in {}) {} for (var w = 'init' in { q: 1 }) {} print(v, w);",
        "init q",
    ),
    (
        "var out = []; outer: for (var a in { a1: 1, a2: 2 }) { for (var b in { b1: 1, b2: 2 }) { if (b === 'b2') continue outer; if (a === 'a2') break outer; out.push(a + b); } } print(out.join());",
        "a1b1",
    ),
    (
        "var log = []; function f() { for (var x in { a: 1, b: 2 }) { try { return x; } finally { log.push('finally ' + x); } } } print(f(), log.join());",
        "a finally a",
    ),
    (
        "function* gen() { for (var x in { a: 1, b: 2, c: 3 }) yield x; } var it = gen(); print(it.next().value, it.next().value, it.next().value, it.next().done);",
        "a b c true",
    ),
    (
        "function args() { var r = []; for (var x in arguments) r.push(x); return r.join(); } print(args('a', 'b', 'c'));",
        "0,1,2",
    ),
    (
        "function F() {} F.prototype.m = function () {}; var o = new F(); o.own = 1; var r = []; for (var k in o) r.push(k); for (var k2 in function () {}) r.push(k2); print(r.join());",
        "own,m",
    ),
    (
        "try { (function () { 'use strict'; for (undeclared in { a: 1 }) {} })(); } catch (e) { print(e.constructor.name, e.message); }",
        "ReferenceError undeclared is not defined",
    ),
    (
        "for (implicitGlobal in { zz: 1 }) {} print(implicitGlobal);",
        "zz",
    ),
    (
        "var log = []; function target() { log.push('target'); return {}; } for (target().k in { a: 1, b: 2 }) { log.push('body'); } print(log.join());",
        "target,body,target,body",
    ),
    (
        "var o = {}; for (var j = 0; j < 100; j++) o['k' + j] = j; var n = 0; for (var k in o) { delete o['k' + (99 - n)]; n++; } print(n);",
        "50",
    ),
    (
        "var p = { a: 1 }; for (var i = 0; i < 50; i++) { p = Object.create(p); p['k' + i] = i; p.a = i; } var r = []; for (var k in p) r.push(k); print(r.length, r[0], r[1], r[50]);",
        "51 k49 a k0",
    ),
    (
        "var r = []; for (var k in { get g() { return 1; }, v: 2 }) r.push(k); for (var k2 in Object.freeze({ f: 1 })) r.push(k2); print(r.join());",
        "g,v,f",
    ),
    (
        "var n = 0; for (var x in { a: 1 }) { var inner = () => x; } print(inner(), (function () { for (var y in { b: 1 }) {} return y; })());",
        "a b",
    ),
    (
        "var o = { a: 1, b: 2 }; var r = []; for (var k in o) { r.push(k); gc(); } print(r.join());",
        "a,b",
    ),
    (
        "function f() { return {}; } var r = []; try { for (f() in { a: 1 }); } catch (e) { r.push(e.name + ': ' + e.message); } try { f()++; } catch (e) { r.push(e.message); } try { --f(); } catch (e) { r.push(e.message); } try { f() = 1; } catch (e) { r.push(e.message); } print(r.join(' / '));",
        "ReferenceError: Invalid left-hand side in for-loop / Invalid left-hand side expression in postfix operation / Invalid left-hand side expression in prefix operation / Invalid left-hand side in assignment",
    ),
];

const OBJECT_LITERALS: &[(&str, &str)] = &[
    (
        "var o = { get a() { return 1; }, set a(v) { this.v = v; } }; o.a = 5; var d = Object.getOwnPropertyDescriptor(o, 'a'); print(o.a, o.v, d.enumerable, d.configurable, d.get.name, d.set.name, d.get.length, d.set.length);",
        "1 5 true true get a set a 0 1",
    ),
    (
        "var o = { a: 1, get a() { return 2; } }; var d = Object.getOwnPropertyDescriptor(o, 'a'); print(o.a, typeof d.get, d.set, 'value' in d);",
        "2 function undefined false",
    ),
    (
        "var o = { get a() { return 2; }, a: 3 }; var d = Object.getOwnPropertyDescriptor(o, 'a'); print(o.a, d.writable, 'get' in d);",
        "3 true false",
    ),
    (
        "var o = { get 1() { return 'one'; }, get 'x y'() { return 'xy'; } }; print(o[1], o['x y'], Object.getOwnPropertyDescriptor(o, '1').get.name, Object.getOwnPropertyDescriptor(o, 'x y').get.name);",
        "one xy get 1 get x y",
    ),
    (
        "var g = Object.getOwnPropertyDescriptor({ get a() {} }, 'a').get; print('prototype' in g, Object.getPrototypeOf(g) === Function.prototype); try { new g(); } catch (e) { print(e.constructor.name); }",
        "false true\nTypeError",
    ),
    (
        "var s = Symbol('desc'); var e = Symbol(); var o = { [s]() {}, [e]() {}, ['k' + 1]: function () {}, ['a' + 'r']: () => {}, get [s]() { return 1; } }; print(Object.getOwnPropertyDescriptor(o, s).get.name, o[e].name === '', o.k1.name, o.ar.name);",
        "get [desc] true k1 ar",
    ),
    (
        "var s = Symbol('x'); var o = { set [s](v) {}, [1 + 1]() {}, [s.description]: function named() {} }; print(Object.getOwnPropertyDescriptor(o, s).set.name, o[2].name, o.x.name);",
        "set [x] 2 named",
    ),
    (
        "var order = []; var o = { [(order.push('k1'), 'a')]: order.push('v1'), get [(order.push('k2'), 'b')]() {}, [(order.push('k3'), 'c')]: order.push('v3') }; print(order.join(), Object.keys(o).join());",
        "k1,v1,k2,k3,v3 a,b,c",
    ),
    (
        "var o = { ...{ a: 1, b: 2 }, c: 3, ...{ a: 4 } }; print(Object.keys(o).join(), o.a, o.b, o.c);",
        "a,b,c 4 2 3",
    ),
    (
        "var o = { ...null, ...undefined, ...5, ...true, ...'hi' }; print(Object.keys(o).join(), o[0], o[1]);",
        "0,1 h i",
    ),
    (
        "var s = Symbol('s'); var src = { [s]: 1, z: 2 }; Object.defineProperty(src, 'hid', { value: 1, enumerable: false }); var o = { ...src }; print(o[s], o.z, 'hid' in o);",
        "1 2 false",
    ),
    (
        "var log = []; var src = { get a() { log.push('a'); delete this.b; return 1; }, b: 2, c: 3 }; var o = { ...src }; print(log.join(), Object.keys(o).join(), Object.getOwnPropertyDescriptor(o, 'a').value);",
        "a a,c 1",
    ),
    (
        "var o = { ...[1, 2], length: 0 }; print(Object.keys(o).join(), Array.isArray(o));",
        "0,1,length false",
    ),
    (
        "var p = { inherited: 1 }; var src = Object.create(p); src.own = 2; var o = { ...src }; print(Object.keys(o).join());",
        "own",
    ),
    (
        "var frozen = Object.freeze({ a: 1 }); var o = { ...frozen }; o.a = 2; var d = Object.getOwnPropertyDescriptor(o, 'a'); print(o.a, d.writable, d.enumerable, d.configurable);",
        "2 true true true",
    ),
    (
        "try { ({ ...{ get x() { throw new Error('boom'); } } }); } catch (e) { print(e.message); }",
        "boom",
    ),
    (
        "var o = { __proto__: { inherited: true }, own: 1 }; print(o.inherited, Object.keys(o).join(), Object.getPrototypeOf(o) === Object.prototype);",
        "true own false",
    ),
    (
        "var o = { ['__proto__']: 1 }; print(Object.keys(o).join(), Object.getPrototypeOf(o) === Object.prototype);",
        "__proto__ true",
    ),
];

const OPTIONAL_CHAINING: &[(&str, &str)] = &[
    (
        "var a = { b: { c: 1 }, n: null }; print(a?.b?.c, a?.x?.y, a.n?.z, a?.['b'].c, a?.b.c);",
        "1 undefined undefined 1 1",
    ),
    (
        "var u; print(u?.b.c.d, u?.[print('not evaluated')], u?.(), u?.b(), null?.x, 0?.toString(), ''?.length);",
        "undefined undefined undefined undefined undefined 0 0",
    ),
    (
        "var o = { f() { return this === o; }, g: null }; print(o.f?.(), o?.f(), (o?.f)(), o.g?.(), o['f']?.());",
        "true true true undefined true",
    ),
    (
        "var a = { b: 1 }; var u = null; print(delete u?.b, delete a?.b, 'b' in a); try { delete a?.x.y; } catch (e) { print(e.message); }",
        "true true false\nCannot convert undefined or null to object",
    ),
    (
        "var a = { x: undefined }; try { a.x?.y.z; print('short'); } catch (e) { print(e.message); } try { (a?.x).y; } catch (e) { print(e.message); }",
        "short\nCannot read properties of undefined (reading 'y')",
    ),
    (
        "var a = {}; try { a?.x(); } catch (e) { print(e.message); } try { a?.['x'](); } catch (e) { print(e.message); } try { a.y?.z(); print('ok'); } catch (e) { print(e.message); }",
        "a?.x is not a function\na?.x is not a function\nok",
    ),
    (
        "var n = 0; function g() { n++; return null; } print(g()?.[n++], n);",
        "undefined 1",
    ),
    (
        "var a = { b: { c() { return this.d; }, d: 5 } }; print(a?.b.c(), a.b?.c(), a?.b?.['c']?.());",
        "5 5 5",
    ),
    (
        "var a = null; var r = a?.b ?? 'default'; print(r, typeof a?.b);",
        "default undefined",
    ),
    (
        "function f(o) { return o?.p?.q; } print(f(), f({}), f({ p: { q: 3 } }));",
        "undefined undefined 3",
    ),
];

const TAGGED_TEMPLATES: &[(&str, &str)] = &[
    (
        "function tag(s, a, b) { return [s.length, s.join('|'), s.raw.join('|'), a, b, arguments.length].join(' '); } print(tag`a${1}b${2}c`);",
        "3 a|b|c a|b|c 1 2 3",
    ),
    (
        "function id(s) { return s; } var x = []; for (var i = 0; i < 3; i++) x.push(id`same`); print(x[0] === x[1], x[1] === x[2]);",
        "true true",
    ),
    (
        "function id(s) { return s; } function mk() { return id`site`; } print(mk() === mk(), id`site` === mk());",
        "true false",
    ),
    (
        "function id(s) { return s; } var t = id`a${0}b`; print(Object.isFrozen(t), Object.isFrozen(t.raw), Array.isArray(t), Array.isArray(t.raw), t.length, t.raw.length);",
        "true true true true 2 2",
    ),
    (
        "function id(s) { return s; } var t = id`x`; var d = Object.getOwnPropertyDescriptor(t, 'raw'); var e = Object.getOwnPropertyDescriptor(t, '0'); print(d.writable, d.enumerable, d.configurable, e.writable, e.enumerable, e.configurable);",
        "false false false false true false",
    ),
    (
        "function id(s) { return s; } var t = id`\\unicode and \\u{110000} ok`; print(t[0] === undefined, t.raw[0]);",
        "true \\unicode and \\u{110000} ok",
    ),
    (
        "function id(s) { return s; } var t = id`line\\nnext`; print(t[0].length, t.raw[0].length);",
        "9 10",
    ),
    (
        "var o = { m(s) { return this === o; } }; print(o.m`x`, o['m']`y`);",
        "true true",
    ),
    (
        "var u; try { u`x`; } catch (e) { print(e.message); } var a = {}; try { a.zz`x`; } catch (e) { print(e.message); }",
        "u is not a function\na.zz is not a function",
    ),
    (
        "var order = []; function tag() { order.push('call'); } function sub(n) { order.push('sub' + n); } tag`${sub(1)}${sub(2)}`; print(order.join());",
        "sub1,sub2,call",
    ),
    (
        "function f(s) { return s[0]; } print(f`a\\`b`, f``.length, f`${1}`.length);",
        "a`b 0 0",
    ),
    (
        "var calls = 0; function t(s) { calls++; return s; } function site() { return t`x`; } var first = site(); for (var i = 0; i < 1000; i++) { if (site() !== first) throw 'differs'; } print(calls);",
        "1001",
    ),
    (
        "function tg() { return 1; } var o = { m: tg }; var r = []; try { tg`x``y`; } catch (e) { r.push(e.message); } try { new tg`x`; } catch (e) { r.push(e.message); } try { o.m`x`.p(); } catch (e) { r.push(e.message); } print(r.join(' / '));",
        "tg(...) is not a function / tg(...) is not a constructor / o.m(...).p is not a function",
    ),
];

const NEW_TARGET: &[(&str, &str)] = &[
    (
        "function F() { return new.target; } print(F() === undefined, new F() === F);",
        "true true",
    ),
    (
        "function G() { var arrow = () => new.target; return arrow(); } print(G() === undefined, new G() === G);",
        "true true",
    ),
    (
        "function H() { return (() => () => new.target)()(); } print(new H() === H, H());",
        "true undefined",
    ),
    (
        "function P() { this.nt = new.target; } var p = new P(); print(p.nt === P, Reflect.construct(P, []).nt === P);",
        "true true",
    ),
    (
        "function A() { return new.target; } function B() {} var r = Reflect.construct(A, [], B); print(r === B);",
        "true",
    ),
    (
        "function C() { return new.target; } var Bound = C.bind(null); print(new Bound() === C);",
        "true",
    ),
    (
        "function D() { if (!new.target) return 'call'; this.x = 1; } print(D(), new D().x);",
        "call 1",
    ),
    (
        "function E() { return typeof new.target; } print(E(), typeof new E());",
        "undefined object",
    ),
    (
        "function* gen() { yield new.target; } print(gen().next().value);",
        "undefined",
    ),
    (
        "var o = { m() { return new.target; } }; print(o.m());",
        "undefined",
    ),
];

// M7 feature 3b: iteration and destructuring. The iterator helper `it(n,
// opts)` logs the protocol calls (`get`, `next`, `return`) of an
// iterator of `n` values; `show` prints a label, the result or the thrown
// value, and the log.

#[test]
fn for_of() {
    check_cases(FOR_OF);
}

#[test]
fn spread() {
    check_cases(SPREAD);
}

#[test]
fn destructuring() {
    check_cases(DESTRUCTURING);
}

#[test]
fn destructuring_assignment() {
    check_cases(DESTRUCTURING_ASSIGNMENT);
}

#[test]
fn parameters() {
    check_cases(PARAMETERS);
}

#[test]
fn destructuring_messages() {
    check_cases(DESTRUCTURING_MESSAGES);
}

const FOR_OF: &[(&str, &str)] = &[
    (
        "var r = []; for (var x of [1, 2, 3]) r.push(x * 2); print(r.join(), x);",
        "2,4,6 3",
    ),
    (
        "var r = []; for (let x of [1, , 3]) r.push(x); print(r.join());",
        "1,,3",
    ),
    (
        "var fs = []; for (let x of [1, 2]) fs.push(() => x); for (const [a, b] of [[3, 4]]) fs.push(() => a + b); print(fs.map(f => f()).join());",
        "1,2,7",
    ),
    (
        "var r = []; a: for (var x of [1, 2, 3]) { for (var y of [10, 20]) { if (y === 20) continue a; if (x === 3) break a; r.push(x + y); } } print(r.join());",
        "11,12",
    ),
    (
        "var o = {}; var i = 0; for (o[i++] of ['a', 'b']); var a, b; for ([a, b] of [[1, 2]]); for ({ c: o.c } of [{ c: 3 }]); print(o[0], o[1], i, a, b, o.c);",
        "a b 2 1 2 3",
    ),
    (
        "function* g() { yield 1; yield 2; } var r = []; for (var v of g()) r.push(v); for (var w of [].values()) r.push(w); print(r.join());",
        "1,2",
    ),
    (
        "var a = [1, 2, 3]; var r = []; for (var x of a) { r.push(x); if (x === 1) a.push(4); if (x === 2) a.length = 3; } print(r.join());",
        "1,2,3",
    ),
    (
        "var r = []; (function () { for (var a of arguments) r.push(a); })(4, 5); print(r.join());",
        "4,5",
    ),
    (
        "try { for (const z of [1]) z = 2; } catch (e) { print(e.constructor.name, e.message); }",
        "TypeError Assignment to constant variable.",
    ),
    (
        "try { for (let q of [q]) ; } catch (e) { print(e.constructor.name, e.message); }",
        "ReferenceError Cannot access 'q' before initialization",
    ),
    (
        "function f() { for (var x of [5, 6]) return x; } print(f());",
        "5",
    ),
    (
        "var r = 0; for (var x of [1, 2]) { try { continue; } finally { r += x; } } print(r);",
        "3",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('break', () => { for (var x of it(3)) if (x === 2) break; return x; }); show('continue', () => { for (var x of it(2)) continue; return x; }); show('return', () => { for (var x of it(3)) return x; }); show('throw', () => { for (var x of it(3)) throw 'body'; });",
        "break 2 get,next,next,return\ncontinue 2 get,next,next,next\nreturn 1 get,next,return\nthrow threw body get,next,return",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('outer break', () => { a: for (var y of [1]) for (var x of it(3)) break a; return y; }); show('outer continue', () => { var n = 0; a: for (var y of [1, 2]) for (var x of it(3)) { n++; continue a; } return n; }); show('done', () => { for (var x of it(2)); return x; });",
        "outer break 1 get,next,return\nouter continue 2 get,next,return,get,next,return\ndone 2 get,next,next,next",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('next throws', () => { for (var x of it(2, { nextThrows: true })); }); show('next prim', () => { for (var x of it(2, { nextPrim: true })); }); show('no return', () => { for (var x of it(2, { noReturn: true })) break; return 'ok'; });",
        "next throws threw next-err get,next\nnext prim threw TypeError: Iterator result 1 is not an object get,next\nno return ok get,next",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('return throws, break', () => { for (var x of it(2, { returnThrows: true })) break; }); show('return throws, throw', () => { for (var x of it(2, { returnThrows: true })) throw 'body'; }); show('return prim, break', () => { for (var x of it(2, { returnPrim: true })) break; }); show('return prim, throw', () => { for (var x of it(2, { returnPrim: true })) throw 'body'; });",
        "return throws, break threw return-err get,next,return\nreturn throws, throw threw body get,next,return\nreturn prim, break threw TypeError: Iterator result 5 is not an object get,next,return\nreturn prim, throw threw body get,next,return",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('finally', () => { for (var x of it(3)) { try { break; } finally { log.push('fin'); } } }); show('nested', () => { for (var x of it(2)) for (var y of it(2)) return x + y; });",
        "finally undefined get,next,fin,return\nnested 2 get,next,get,next,return,return",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('gen return', () => { function* g() { for (var x of it(3)) yield x; } var i = g(); i.next(); return JSON_like(i.return(9)); }); function JSON_like(r) { return r.value + '/' + r.done; } show('gen throw', () => { function* g() { for (var x of it(3)) yield x; } var i = g(); i.next(); try { i.throw('t'); } catch (e) { log.push('caught ' + e); } });",
        "gen return 9/true get,next,return\ngen throw undefined get,next,return,caught t",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('gen finally', () => { function* g() { try { yield 1; yield 2; } finally { log.push('gfin'); } } for (var v of g()) break; return v; }); show('gen in loop', () => { function* g() { for (var x of [1, 2]) yield x * 10; } var r = []; for (var v of g()) r.push(v); return r.join(); });",
        "gen finally 1 gfin\ngen in loop 10,20 ",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('return getter throws, break', () => { var o = { [Symbol.iterator]() { return { next() { return { done: false }; }, get return() { throw 'getter'; } }; } }; for (var x of o) break; }); show('return getter throws, throw', () => { var o = { [Symbol.iterator]() { return { next() { return { done: false }; }, get return() { throw 'getter'; } }; } }; for (var x of o) throw 'body'; });",
        "return getter throws, break threw getter \nreturn getter throws, throw threw body ",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('return null', () => { var o = { [Symbol.iterator]() { return { next() { return { done: false }; }, return: null }; } }; for (var x of o) break; return 'ok'; }); show('return not callable', () => { var o = { [Symbol.iterator]() { return { next() { return { done: false }; }, return: 5 }; } }; for (var x of o) break; });",
        "return null ok \nreturn not callable threw TypeError: number 5 is not a function ",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('result getters', () => { var o = { [Symbol.iterator]() { var i = 0; return { next() { return { get done() { log.push('done'); return i++ > 0; }, get value() { log.push('value'); return 7; } }; } }; } }; for (var x of o); return x; }); show('next getter once', () => { var o = { [Symbol.iterator]() { return { get next() { log.push('get next'); return function () { return { done: true }; }; } }; } }; for (var x of o); return 'ok'; });",
        "result getters 7 done,value,done\nnext getter once ok get next",
    ),
    (
        "function f() { log.push('call'); } var log = []; try { for (f() of [1]); } catch (e) { print(e.constructor.name, e.message, log.join()); }",
        "ReferenceError Invalid left-hand side in for-loop call",
    ),
    (
        "var proto = Object.getPrototypeOf([][Symbol.iterator]()); var next = proto.next; var n = 0; proto.next = function () { n++; return next.call(this); }; var r = []; for (var x of [1, 2]) r.push(x); proto.next = next; print(r.join(), n);",
        "1,2 3",
    ),
    (
        "var orig = Array.prototype[Symbol.iterator]; var n = 0; Array.prototype[Symbol.iterator] = function () { n++; return orig.call(this); }; for (var x of [1]); Array.prototype[Symbol.iterator] = orig; print(n);",
        "1",
    ),
];

const SPREAD: &[(&str, &str)] = &[
    (
        "var a = [1, 2]; var b = [0, ...a, , ...a, 3]; print(b.length, b.join(), 3 in b, 4 in b);",
        "7 0,1,2,,1,2,3 false true",
    ),
    (
        "print([...[1, , 3]].hasOwnProperty(1), [...[]].length, [...[], ...[]].length);",
        "true 0 0",
    ),
    (
        "function f() { return arguments.length + ':' + Array.prototype.slice.call(arguments).join(); } print(f(...[1, 2], 3, ...[], ...[4]));",
        "4:1,2,3,4",
    ),
    (
        "function F(a, b) { this.s = a + b; this.t = new.target === F; } var o = new F(...[1, 2]); print(o.s, o.t);",
        "3 true",
    ),
    (
        "var o = { v: 1, m(...a) { return this.v + a.length; } }; var n = null; print(o.m(...[1, 2]), o?.m(...[1]), n?.m(...[1]), o['m'](...[3]));",
        "3 2 undefined 2",
    ),
    (
        "function* g() { yield 1; yield 2; } function sum() { var s = 0; for (var x of arguments) s += x; return s; } print([...g()].join(), sum(...g(), ...g()));",
        "1,2 6",
    ),
    (
        "var a = [1, 2, 3]; var it = a[Symbol.iterator](); it.next(); var o = { [Symbol.iterator]() { return it; } }; var r = [...o]; var n = it.next(); print(r.join(), n.done, n.value);",
        "2,3 true undefined",
    ),
    (
        "Array.prototype[1] = 'p'; var r = [...[0, , 2]]; delete Array.prototype[1]; print(r.join());",
        "0,p,2",
    ),
    (
        "var log = []; var a = [1, 2]; Object.defineProperty(a, 1, { get() { log.push('g'); return 9; } }); print([...a].join(), log.join());",
        "1,9 g",
    ),
    (
        "var a = ['x', 'y']; print([...a.keys()].join(), [...a.entries()].join());",
        "0,1 0,x,1,y",
    ),
    (
        "var proto = Object.getPrototypeOf([][Symbol.iterator]()); var next = proto.next; var n = 0; proto.next = function () { n++; return next.call(this); }; var r = [...[1, 2]]; function f() { return arguments.length; } var c = f(...[1, 2, 3]); proto.next = next; print(r.join(), c, n);",
        "1,2 3 7",
    ),
    (
        "var orig = Array.prototype[Symbol.iterator]; var n = 0; Array.prototype[Symbol.iterator] = function () { n++; return orig.call(this); }; var r = [...[1]]; Math_max = function () { return arguments.length; }; var c = Math_max(...[1, 2]); Array.prototype[Symbol.iterator] = orig; print(r.join(), c, n);",
        "1 2 2",
    ),
    (
        "var a = [1, 2]; a[Symbol.iterator] = function* () { yield 'own'; }; print([...a].join());",
        "own",
    ),
    (
        "var o = { [Symbol.iterator]() { var i = 0; return { next() { return { value: i, done: i++ > 2 }; } }; } }; print([...o].join(), [...o, ...o].length);",
        "0,1,2 6",
    ),
    (
        "try { var x = 5; x(...[1]); } catch (e) { print(e.constructor.name, e.message); }",
        "TypeError x is not a function",
    ),
    (
        "try { var y = () => 1; new y(...[1]); } catch (e) { print(e.constructor.name, e.message); }",
        "TypeError y is not a constructor",
    ),
    (
        "try { var o = {}; o.m(...[1]); } catch (e) { print(e.constructor.name, e.message); }",
        "TypeError o.m is not a function",
    ),
    (
        "function f() { return arguments.length + ':' + arguments[1]; } print(f(...[1, , 3]));",
        "3:undefined",
    ),
];

const DESTRUCTURING: &[(&str, &str)] = &[
    (
        "var [a, b = 5, ...c] = [1, undefined, 3, 4]; let [d, , e] = [6, 7, 8]; const [f = 9] = []; print(a, b, c.join(), d, e, f);",
        "1 5 3,4 6 8 9",
    ),
    (
        "var { p, q: { r = 7 } = {}, ...s } = { p: 1, t: 2, u: 3 }; print(p, r, Object.keys(s).join());",
        "1 7 t,u",
    ),
    (
        "var [a = 1, b = 2, c = 3] = [null, 0, undefined]; var { x = 1, y = 2 } = { x: null }; print(a, b, c, x, y);",
        "null 0 3 null 2",
    ),
    (
        "var n = 0; var [a = n++, b = n++] = [1]; var { [(n++, 'k')]: k = n } = {}; print(a, b, k, n);",
        "1 0 2 2",
    ),
    (
        "var [f = function () {}, g = () => 1] = []; var { h = function () {}, i: j = function () {} } = {}; print(f.name, g.name, h.name, j.name);",
        "f g h j",
    ),
    (
        "var [[a, b], { c: [d] }] = [[1, 2], { c: [3] }]; print(a, b, d);",
        "1 2 3",
    ),
    (
        "var [...[x, ...y]] = [1, 2, 3]; var [m, ...rest] = [1]; print(x, y.join(), m, rest.length);",
        "1 2,3 1 0",
    ),
    (
        "var { length, 0: first } = 'abc'; var { toString: t } = 5; var {} = 0; print(length, first, typeof t);",
        "3 a function",
    ),
    (
        "var s = Symbol('s'); var o = { a: 1 }; o[s] = 2; Object.defineProperty(o, 'h', { value: 3, enumerable: false }); var { ...r } = o; print(Object.keys(r).join(), r[s], 'h' in r);",
        "a 2 false",
    ),
    (
        "var p = { inherited: 1 }; var o = Object.create(p); o.own = 2; o[1] = 'one'; var { 1: one, ...r } = o; print(one, Object.keys(r).join(), Object.getPrototypeOf(r) === Object.prototype);",
        "one own true",
    ),
    (
        "var k = 'b'; var { a, [k]: b, ...r } = { a: 1, b: 2, c: 3, 1: 4 }; var { ...q } = 'xy'; print(a, b, Object.keys(r).join(), Object.keys(q).join());",
        "1 2 1,c 0,1",
    ),
    (
        "var log = []; var src = { get a() { log.push('a'); return 1; }, get b() { log.push('b'); return 2; } }; var { [(log.push('k1'), 'b')]: x, [(log.push('k2'), 'a')]: y } = src; print(x, y, log.join());",
        "2 1 k1,b,k2,a",
    ),
    (
        "var log = []; var { ...r } = { get g() { log.push('g'); return 1; } }; print(r.g, log.join(), Object.getOwnPropertyDescriptor(r, 'g').value);",
        "1 g 1",
    ),
    (
        "var fs = []; for (let [a, b] of [[1, 2], [3, 4]]) fs.push(() => a + b); for (const { v } of [{ v: 5 }]) fs.push(() => v); print(fs.map(f => f()).join());",
        "3,7,5",
    ),
    (
        "var r = []; for (var { length } in { abc: 1, de: 2 }) r.push(length); var a, b; for ({ length: a } in { xyz: 1 }); print(r.join(), a);",
        "3,2 3",
    ),
    (
        "try { throw { m: 'x', n: [1, 2] }; } catch ({ m, n: [, second] }) { print(m, second); }",
        "x 2",
    ),
    (
        "function* g() { var [a = yield 'y1', b = yield 'y2'] = []; return a + b; } var i = g(); print(i.next().value, i.next(10).value, i.next(20).value);",
        "y1 y2 30",
    ),
    (
        "var o = { m() { var [a = this] = []; return a === o; } }; function f() { var [a = arguments.length] = []; return a; } print(o.m(), f(1, 2));",
        "true 2",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('partial', () => { var [a] = it(3); return a; }); show('full', () => { var [a, b, c] = it(2); return [a, b, c].join(); }); show('exact', () => { var [a, b] = it(2); return a + b; }); show('rest', () => { var [a, ...r] = it(3); return r.join(); });",
        "partial 1 get,next,return\nfull 1,2, get,next,next,next\nexact 3 get,next,next,return\nrest 2,3 get,next,next,next,next",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('elision', () => { var [, , ] = it(3); return 'el'; }); show('empty', () => { var [] = it(3); return 'empty'; }); show('nested', () => { var [[a, b], c] = [it(3), 4]; return a + b + c; });",
        "elision el get,next,next,return\nempty empty get,return\nnested 7 get,next,next,return",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('default throws', () => { var [a = (() => { throw 'dflt'; })()] = it(0); }); show('default throws, not done', () => { var [a, b = (() => { throw 'dflt'; })()] = it(5); });",
        "default throws threw dflt get,next\ndefault throws, not done undefined get,next,next,return",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('next throws', () => { var [a] = it(3, { nextThrows: true }); }); show('return throws', () => { var [a] = it(3, { returnThrows: true }); }); show('return prim', () => { var [a] = it(3, { returnPrim: true }); });",
        "next throws threw next-err get,next\nreturn throws threw return-err get,next,return\nreturn prim threw TypeError: Iterator result 5 is not an object get,next,return",
    ),
    (
        "var log = []; function it(n, o) { o = o || {}; var i = 0; var r = { next() { log.push('next'); if (o.nextThrows) throw 'next-err'; if (o.nextPrim) return 1; i++; return { value: i, done: i > n }; } }; if (!o.noReturn) r.return = function () { log.push('return'); if (o.returnThrows) throw 'return-err'; if (o.returnPrim) return 5; return {}; }; return { [Symbol.iterator]() { log.push('get'); return r; } }; } function show(label, f) { log = []; try { var v = f(); print(label, v, log.join()); } catch (e) { print(label, 'threw', typeof e === 'object' && e ? e.constructor.name + ': ' + e.message : e, log.join()); } } show('gen return in pattern', () => { function* g() { var [a = yield 1, b] = it(3); } var i = g(); i.next(); i.return(5); return 'r'; });",
        "gen return in pattern r get,next,next,return",
    ),
];

const DESTRUCTURING_ASSIGNMENT: &[(&str, &str)] = &[
    (
        "var a, b; var r = ([a, b] = [1, 2]); var s = ({ a } = { a: 7 }); print(r.join(), a, b, s.a);",
        "1,2 7 2 7",
    ),
    (
        "var o = {}; [o.a, o['b'], o[1 + 1]] = [10, 20, 30]; ({ x: o.c, y: o.d = 'd' } = { x: 40 }); print(o.a, o.b, o[2], o.c, o.d);",
        "10 20 30 40 d",
    ),
    (
        "var o = {}; ({ a: o.a, ...o.rest } = { a: 1, b: 2, c: 3 }); var x, y; ({ x = 1, ...y } = { z: 2 }); print(o.a, Object.keys(o.rest).join(), x, Object.keys(y).join());",
        "1 b,c 1 z",
    ),
    (
        "var log = []; var o = { set x(v) { log.push('set ' + v); } }; var src = { get a() { log.push('get a'); return 1; }, get b() { log.push('get b'); return 2; } }; ({ a: (log.push('ref a'), o).x, b: (log.push('ref b'), o).x } = src); print(log.join());",
        "ref a,get a,set 1,ref b,get b,set 2",
    ),
    (
        "var log = []; var o = { set x(v) { log.push('set ' + v); } }; var src = { [Symbol.iterator]() { return { next() { log.push('next'); return { value: 1, done: false }; }, return() { log.push('return'); return {}; } }; } }; [(log.push('ref'), o).x, (log.push('ref2'), o).x] = src; print(log.join());",
        "ref,next,set 1,ref2,next,set 1,return",
    ),
    (
        "var log = []; var k = { toString() { log.push('key'); return 'p'; } }; var o = {}; var src = { [Symbol.iterator]() { return { next() { log.push('next'); return { value: 1, done: false }; }, return() { return {}; } }; } }; [o[k]] = src; print(o.p, log.join());",
        "1 next,key",
    ),
    (
        "var i, j, f; [i = function () {}] = []; ({ j = () => 0 } = {}); var o = {}; [o.k = function () {}] = []; print(i.name, j.name, o.k.name === '');",
        "i j true",
    ),
    (
        "var a, b, c; [a, [b, ...c]] = [1, [2, 3, 4]]; ({ x: [a] } = { x: [5] }); print(a, b, c.join());",
        "5 2 3,4",
    ),
    ("var x = 1, y = 2; [x, y] = [y, x]; print(x, y);", "2 1"),
    (
        "var log = []; var o = { set x(v) { throw 'setter'; } }; var src = { [Symbol.iterator]() { return { next() { return { value: 1, done: false }; }, return() { log.push('return'); return {}; } }; } }; try { [o.x] = src; } catch (e) { print(e, log.join()); }",
        "setter return",
    ),
    (
        "(function () { 'use strict'; try { [undeclared_q] = [1]; } catch (e) { print(e.constructor.name, e.message); } })();",
        "ReferenceError undeclared_q is not defined",
    ),
    (
        "const c = 1; try { [c] = [2]; } catch (e) { print(e.constructor.name, e.message); }",
        "TypeError Assignment to constant variable.",
    ),
];

const PARAMETERS: &[(&str, &str)] = &[
    (
        "function f(x, y = x + 1, ...z) { return [x, y, z.length].join(); } print(f(1), f(1, 2, 3, 4), f.length);",
        "1,2,0 1,2,2 1",
    ),
    (
        "print(function (a, b = 1, c) {}.length, function (...a) {}.length, function ([a], { b }) {}.length, ((a, b) => 0).length, function (a, [b] = [], c) {}.length);",
        "1 0 2 2 1",
    ),
    (
        "var fn = (a, b, ...c) => c; print(fn().length, fn(1, 2, 3).join(), ((...args) => args)(1, 2, 3).join());",
        "0 3 1,2,3",
    ),
    (
        "function f(a, ...rest) { arguments[0] = 1; arguments[1] = 2; return a + ':' + rest.join() + ':' + arguments.length; } print(f(3, 4, 5));",
        "3:4,5:3",
    ),
    (
        "function f(...[a, b]) { return a + b; } function g(...{ length }) { return length; } print(f(1, 2, 3), g(1, 2, 3));",
        "3 3",
    ),
    (
        "function f(a = b, b) {} function g(a = a) {} function h(a = (b = 1), b) {} function k(a = typeof b, b) {} for (var t of [f, g, h, k]) { try { t(); } catch (e) { print(e.constructor.name, e.message); } }",
        "ReferenceError Cannot access 'b' before initialization\nReferenceError Cannot access 'a' before initialization\nReferenceError Cannot access 'b' before initialization\nReferenceError Cannot access 'b' before initialization",
    ),
    (
        "function f(a, b = a) { return b; } function g(a, h = () => a) { a = 5; return h(); } print(f(3), g(1));",
        "3 5",
    ),
    (
        "function f(a = 1, b) { a = 9; return arguments[0] + ':' + arguments.length; } print(f(undefined, 2));",
        "undefined:2",
    ),
    (
        "function f(a = 1) { var a; return a; } function g(a, h = () => a) { var a = 2; return h() + ':' + a; } function k(a = 1) { var arguments; return typeof arguments; } print(f(), g(1), k());",
        "1 1:2 object",
    ),
    (
        "function f({ a, b = 2 }, [c]) { return a + b + c; } print(f({ a: 1 }, [3]));",
        "6",
    ),
    (
        "var f = ([a, b] = [1, 2], { c } = { c: 3 }) => a + b + c; print(f(), f([10, 20]), f.length);",
        "6 33 0",
    ),
    (
        "function* g(a = 1, ...r) { yield a; yield r.length; } var it = g(undefined, 1, 2); print(it.next().value, it.next().value);",
        "1 2",
    ),
    (
        "function f(a, b = 2) { function a() {} return typeof a + b; } print(f(1));",
        "function2",
    ),
    (
        "function f([a]) { function a() {} return typeof a; } print(f([1]));",
        "function",
    ),
    (
        "var x = 'outer'; function f(a = () => x) { var x = 'inner'; return a(); } print(f());",
        "outer",
    ),
    (
        "function f(a, b = 1) { return arguments.length; } print(f(), f(1, 2, 3));",
        "0 3",
    ),
];

const DESTRUCTURING_MESSAGES: &[(&str, &str)] = &[
    (
        "var t = (f) => { try { f(); } catch (e) { print(e.constructor.name + ': ' + e.message); } }; var o = { u: undefined, p: 5, f() {} }; var nul = null; t(() => { var { a } = undefined; }); t(() => { var { a } = nul; }); t(() => { var {} = null; }); t(() => { var { [1]: a } = null; }); t(() => { var { ...r } = undefined; }); t(() => { var { a } = o.u; }); t(() => { var { a } = o['u']; }); t(() => { var { a } = o.f(); }); t(() => { var { 'b c': a } = undefined; }); t(() => { var { 1.5: a } = undefined; });",
        "TypeError: Cannot destructure property 'a' of 'undefined' as it is undefined.\nTypeError: Cannot destructure property 'a' of 'nul' as it is null.\nTypeError: Cannot destructure 'null' as it is null.\nTypeError: Cannot destructure 'null' as it is null.\nTypeError: Cannot destructure 'undefined' as it is undefined.\nTypeError: Cannot destructure property 'a' of 'o.u' as it is undefined.\nTypeError: Cannot destructure property 'a' of 'o.u' as it is undefined.\nTypeError: Cannot destructure property 'a' of 'o.f(...)' as it is undefined.\nTypeError: Cannot destructure property 'b c' of 'undefined' as it is undefined.\nTypeError: Cannot destructure property '1.5' of 'undefined' as it is undefined.",
    ),
    (
        "var t = (f) => { try { f(); } catch (e) { print(e.constructor.name + ': ' + e.message); } }; t(() => { (function ({ a }) {})(null); }); t(() => { (function ({}) {})(); }); t(() => { (({ b }) => 1)(); }); t(() => { var [{ y }] = [null]; }); t(() => { var [{}] = [undefined]; }); t(() => { for (const { a } of [null]); }); t(() => { try { throw undefined; } catch ({ a }) {} }); t(() => { var a; ({ a } = undefined); }); t(() => { for ({ z } of [undefined]); });",
        "TypeError: Cannot destructure property 'a' of 'object null' as it is null.\nTypeError: Cannot destructure 'undefined' as it is undefined.\nTypeError: Cannot destructure property 'b' of 'undefined' as it is undefined.\nTypeError: Cannot read properties of null (reading 'y')\nTypeError: Cannot read properties of undefined\nTypeError: Cannot destructure property 'a' of '.for' as it is null.\nTypeError: Cannot destructure property 'a' of '.catch' as it is undefined.\nTypeError: Cannot destructure property 'a' of 'undefined' as it is undefined.\nTypeError: Cannot read properties of undefined (reading 'z')",
    ),
    (
        "var t = (f) => { try { f(); } catch (e) { print(e.constructor.name + ': ' + e.message); } }; var o = { p: 5, u: undefined, f() {} }; var five = 5; t(() => { var [a] = five; }); t(() => { var [a] = o.p; }); t(() => { var [a] = undefined; }); t(() => { var [a] = true; }); t(() => { var a; [a] = five; }); t(() => { var [[x]] = [1]; }); t(() => { function g() { return 1; } var [a] = g(); }); t(() => { var [x] = o.f(); }); t(() => { try { throw null; } catch ([q]) {} });",
        "TypeError: five is not iterable\nTypeError: number 5 is not iterable (cannot read property Symbol(Symbol.iterator))\nTypeError: undefined is not iterable\nTypeError: true is not iterable\nTypeError: number 5 is not iterable (cannot read property Symbol(Symbol.iterator))\nTypeError: number 1 is not iterable (cannot read property Symbol(Symbol.iterator))\nTypeError: g is not a function or its return value is not iterable\nTypeError: undefined is not iterable (cannot read property Symbol(Symbol.iterator))\nTypeError: object null is not iterable (cannot read property Symbol(Symbol.iterator))",
    ),
    (
        "var t = (f) => { try { f(); } catch (e) { print(e.constructor.name + ': ' + e.message); } }; var o = { p: 5, q: { r: 5 }, u: undefined, f() {} }; t(() => { for (var a of o.p); }); t(() => { for (var a of o.q.r); }); t(() => { for (var a of undefined); }); t(() => { function g() {} for (var a of g()); }); t(() => { for (var a of o.f()); }); t(() => [...o.p]); t(() => [...5]); t(() => [...o.u]); t(() => [...o.f()]); t(() => (function () {})(...o.p)); t(() => (function () {})(1, ...o.u)); t(() => (function () {})(...null));",
        "TypeError: o.p is not iterable\nTypeError: o.q.r is not iterable\nTypeError: undefined is not iterable\nTypeError: g is not a function or its return value is not iterable\nTypeError: o.f is not a function or its return value is not iterable\nTypeError: o.p is not iterable\nTypeError: 5 is not iterable\nTypeError: o.u is not iterable\nTypeError: o.f is not a function or its return value is not iterable\nTypeError: Spread syntax requires ...iterable[Symbol.iterator] to be a function\nTypeError: o.u is not iterable (cannot read property undefined)\nTypeError: null is not iterable (cannot read property null)",
    ),
    (
        "var t = (f) => { try { f(); } catch (e) { print(e.constructor.name + ': ' + e.message); } }; var bad = { [Symbol.iterator]() { return { next: 5 }; } }; var nonext = { [Symbol.iterator]() { return {}; } }; var prim = { [Symbol.iterator]() { return 1; } }; var badResult = { [Symbol.iterator]() { return { next() { return 1; } }; } }; t(() => [...bad]); t(() => { var [a] = bad; }); t(() => { for (var a of bad); }); t(() => [...nonext]); t(() => { for (var a of nonext); }); t(() => [...prim]); t(() => { for (var a of badResult); });",
        "TypeError: bad is not iterable\nTypeError: number 5 is not a function\nTypeError: number 5 is not a function\nTypeError: nonext is not iterable\nTypeError: undefined is not a function\nTypeError: Result of the Symbol.iterator method is not an object\nTypeError: Iterator result 1 is not an object",
    ),
];

// --- Classes and `super` (M7 feature 3c); expected outputs from Node.js 22 ---

#[test]
fn classes() {
    check_cases(CLASSES);
}

#[test]
fn class_messages() {
    check_cases(CLASS_MESSAGES);
}

#[test]
fn class_edges() {
    check_cases(CLASS_EDGES);
}

const CLASSES: &[(&str, &str)] = &[
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('order', () => { log = []; class C { [k('a')]() {} static [k('b')] = log.push('static init b'); [k('c')] = log.push('init c'); static { log.push('block'); } get [k('d')]() { return 1; } [k('e')] = log.push('init e'); } log.push('defined'); new C(); return log.join(); });",
        "order key a,key b,key c,key d,key e,static init b,block,defined,init c,init e",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('names', () => { var s = Symbol('sym'); class C { a = function () {}; [s] = () => {}; static b = class {}; ['c' + 1] = function () {}; static [Symbol()] = function () {} } var c = new C(); return [c.a.name, c[s].name, C.b.name, c.c1.name].join('|'); });",
        "names a|[sym]|b|c1",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('fields-derived', () => { log = []; class A { x = log.push('A.x ' + this.constructor.name); constructor() { log.push('A ctor'); } } class B extends A { y = log.push('B.y'); constructor() { log.push('B before'); super(); log.push('B after ' + this.y); } } new B(); return log.join(); });",
        "fields-derived B before,A.x B,A ctor,B.y,B after 4",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('tdz-heritage', () => { class C extends C {} });",
        "tdz-heritage threw ReferenceError: Cannot access 'C' before initialization",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('tdz-name', () => { var D = class E { static x = E.name; }; return D.x; });",
        "tdz-name E",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('inner-immutable', () => { class C { static m() { C = 1; } } C.m(); });",
        "inner-immutable threw TypeError: Assignment to constant variable.",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('outer-mutable', () => { class C {} C = 5; return C; });",
        "outer-mutable 5",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('tdz-outer', () => { new C(); class C {} });",
        "tdz-outer threw ReferenceError: Cannot access 'C' before initialization",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('static-block', () => { var r; class C { static x = 1; static { var y = this.x + 1; r = y; } } return r; });",
        "static-block 2",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('static-this', () => { class C { static f = this; static g = () => this; } return C.f === C && C.g() === C; });",
        "static-this true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-static', () => { class A { static m() { return 'A.m'; } } class B extends A { static m() { return super.m() + '+B'; } } return B.m(); });",
        "super-static A.m+B",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-getter', () => { class A { get g() { return 'Ag' + this.v; } } class B extends A { v = 1; get g() { return super.g + '+B'; } } return new B().g; });",
        "super-getter Ag1+B",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-set', () => { class A { set s(v) { this._s = v * 2; } } class B extends A { m() { super.s = 4; return this._s; } } return new B().m(); });",
        "super-set 8",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-set-data', () => { class A {} A.prototype.x = 1; class B extends A { m() { super.x = 7; return this.hasOwnProperty('x') + ' ' + this.x + ' ' + A.prototype.x; } } return new B().m(); });",
        "super-set-data true 7 1",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-compound', () => { class A {} A.prototype.n = 10; class B extends A { m() { super.n += 5; super.n++; ++super['n']; super.n ??= 0; super.n ||= 3; return this.n; } } return new B().m(); });",
        "super-compound 11",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-destructure', () => { class B { m() { [super.a, super['b']] = [1, 2]; ({ c: super.c } = { c: 3 }); for (super.d of [4]); for (super.e in { k: 1 }); return [this.a, this.b, this.c, this.d, this.e].join(); } } return new B().m(); });",
        "super-destructure 1,2,3,4,k",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-field', () => { class A { m() { return 'Am'; } } class B extends A { f = super.m(); static s = super.name; } return new B().f + ' ' + B.s; });",
        "super-field Am A",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-arrow', () => { class A { m() { return 'Am'; } } class B extends A { m() { return (() => super.m())(); } } return new B().m(); });",
        "super-arrow Am",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-literal', () => { var p = { get g() { return 'pg' + this.v; }, set s(x) { this.r = x; } }; var o = { __proto__: p, v: 2, get g() { return super.g; }, set s(x) { super.s = x + 1; } }; o.s = 1; return o.g + ' ' + o.r; });",
        "super-literal pg2 2",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-call-arrow', () => { class A { constructor() { this.a = 1; } } class B extends A { constructor() { var f = () => super(); f(); this.b = 2; } } var b = new B(); return b.a + b.b; });",
        "super-call-arrow 3",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-twice-arrow', () => { class A {} class B extends A { constructor() { var f = () => super(); f(); f(); } } new B(); });",
        "super-twice-arrow threw ReferenceError: Super constructor may only be called once",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('this-in-arrow-before', () => { class A {} class B extends A { constructor() { var f = () => this; f(); super(); } } new B(); });",
        "this-in-arrow-before threw ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('new.target', () => { var r = []; class A { constructor() { r.push(new.target.name); } } class B extends A {} new A(); new B(); return r.join(); });",
        "new.target A,B",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('default-args', () => { var got; class A { constructor(...a) { got = a.join(); } } class B extends A {} var saved = Array.prototype[Symbol.iterator]; Array.prototype[Symbol.iterator] = function () { throw new Error('iter'); }; try { new B(1, 2, 3); } finally { Array.prototype[Symbol.iterator] = saved; } return got; });",
        "default-args 1,2,3",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('extends-boolean', () => { class B extends Boolean {} var b = new B(false); return (b instanceof B) + ' ' + b.valueOf() + ' ' + typeof b; });",
        "extends-boolean true false object",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('extends-object', () => { class O extends Object { constructor() { super(); } } var o = new O(); return Object.getPrototypeOf(o) === O.prototype; });",
        "extends-object true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('extends-array', () => { class A2 extends Array {} var a = new A2(3); return a.length + ' ' + (a instanceof A2) + ' ' + Array.isArray(a) + ' ' + (a.slice(0, 1) instanceof A2); });",
        "extends-array 3 true true true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('extends-error', () => { class E extends TypeError { constructor(m) { super(m); this.name = 'E'; } } var e = new E('x'); return e.name + ' ' + e.message + ' ' + (e instanceof TypeError) + ' ' + String(e); });",
        "extends-error E x true E: x",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('extends-null', () => { class N extends null {} return Object.getPrototypeOf(N.prototype) === null; });",
        "extends-null true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('extends-null-new', () => { class N extends null {} new N(); });",
        "extends-null-new threw TypeError: Super constructor null of N is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('extends-null-ctor', () => { class N extends null { constructor() { return Object.create(N.prototype); } } return new N() instanceof N; });",
        "extends-null-ctor true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('gen-method', () => { class G { *g() { yield 1; yield 2; } static *s() { yield 3; } } return [...new G().g(), ...G.s()].join(); });",
        "gen-method 1,2,3",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('method-props', () => { class C { m() {} static s() {} get g() { return 1; } } var d = Object.getOwnPropertyDescriptor(C.prototype, 'm'); var p = Object.getOwnPropertyDescriptor(C, 'prototype'); return [d.enumerable, d.writable, d.configurable, p.writable, p.enumerable, p.configurable, Object.keys(C.prototype).length, 'prototype' in C.prototype.m].join(); });",
        "method-props false,true,true,false,false,false,0,false",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('keys-order', () => { class C { static b() {} static a = 1; static 1() {} static [Symbol.iterator]() {} } return Reflect.ownKeys(C).map(String).join(); });",
        "keys-order 1,length,name,prototype,b,a,Symbol(Symbol.iterator)",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('frozen-this', () => { class A { constructor() { return Object.freeze({}); } } class B extends A { x = 1; } new B(); });",
        "frozen-this threw TypeError: Cannot define property x, object is not extensible",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('loop', () => { var fs = []; for (let i = 0; i < 3; i++) { fs.push(class { static v = i; m() { return i; } }); } return fs.map(c => c.v + new c().m()).join(); });",
        "loop 0,2,4",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('ctor-return-object', () => { class A { constructor() { return { o: 1 }; } } return new A().o; });",
        "ctor-return-object 1",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('base-return-prim', () => { class A { constructor() { return 1; } } return typeof new A(); });",
        "base-return-prim object",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('derived-finally', () => { class A {} class B extends A { constructor() { try { return; } finally { super(); } } } return new B() instanceof B; });",
        "derived-finally true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('class-expr-name', () => { var C = class Named {}; return C.name + ' ' + typeof Named; });",
        "class-expr-name Named undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('static-name', () => { class C { static name = 'x'; } return C.name; });",
        "static-name x",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('accessor-pair', () => { class C { get x() { return this._x; } set x(v) { this._x = v; } } var c = new C(); c.x = 3; return c.x + ' ' + typeof Object.getOwnPropertyDescriptor(C.prototype, 'x').set; });",
        "accessor-pair 3 function",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('computed-once', () => { var n = 0; class C { [n++] = n; [n++] = n; } var c = new C(); return n + ' ' + JSON_like(c); function JSON_like(o) { return Object.keys(o).map(k => k + ':' + o[k]).join(','); } });",
        "computed-once 2 0:2,1:2",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('key-throws', () => { class C { [{ toString() { throw new Error('key'); } }]() {} } });",
        "key-throws threw Error: key",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-prop-null-base', () => { class C { static m() { return super.x; } } Object.setPrototypeOf(C, null); return C.m(); });",
        "super-prop-null-base threw TypeError: Cannot read properties of null (reading 'x')",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-index-order', () => { log = []; var o = { __proto__: { set x(v) { log.push('set ' + v); } }, m() { super[{ toString() { log.push('tostring'); return 'x'; } }] = (log.push('rhs'), 1); } }; o.m(); return log.join(); });",
        "super-index-order rhs,tostring,set 1",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('this-before-super', () => { class A {} class B extends A { constructor() { this.x = 1; super(); } } new B(); });",
        "this-before-super threw ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('super-prop-before-super', () => { class A {} class B extends A { constructor() { super.x; super(); } } new B(); });",
        "super-prop-before-super threw ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('instanceof', () => { class A {} class B extends A {} return (new B() instanceof A) + ' ' + (B.__proto__ === A); });",
        "instanceof true true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('call-apply', () => { class A {} A.call({}); });",
        "call-apply threw TypeError: Class constructor A cannot be invoked without 'new'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('bind-new', () => { class A { constructor(x) { this.x = x; } } var B = A.bind(null, 5); return new B().x; });",
        "bind-new 5",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('reflect-construct', () => { class A { constructor() { this.nt = new.target; } } function F() {} var o = Reflect.construct(A, [], F); return (o.nt === F) + ' ' + (Object.getPrototypeOf(o) === F.prototype); });",
        "reflect-construct true true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('typeof-class', () => typeof class {});",
        "typeof-class function",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } var log = []; function k(v) { log.push('key ' + String(v)); return v; } t('arguments-ctor', () => { class A { constructor() { this.n = arguments.length; } } return new A(1, 2).n; });",
        "arguments-ctor 2",
    ),
];

const CLASS_MESSAGES: &[(&str, &str)] = &[
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('call', () => A());",
        "call threw TypeError: Class constructor A cannot be invoked without 'new'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('call-anon', () => (class {})());",
        "call-anon threw TypeError: Class constructors cannot be invoked without 'new'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('call-named-expr', () => { var Anon = class {}; Anon(); });",
        "call-named-expr threw TypeError: Class constructor Anon cannot be invoked without 'new'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('extends-5', () => { class B extends 5 {} });",
        "extends-5 threw TypeError: Class extends value 5 is not a constructor or null",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('extends-null-new', () => { class B extends null {}; return new B(); });",
        "extends-null-new threw TypeError: Super constructor null of B is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('bad-proto', () => { function F() {} F.prototype = 3; class B extends F {} });",
        "bad-proto threw TypeError: Class extends value does not have valid prototype property 3",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('extends-object', () => { var o = {}; class B extends o {} });",
        "extends-object threw TypeError: Class extends value #<Object> is not a constructor or null",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('extends-arrow', () => { class B extends (() => 1) {} });",
        "extends-arrow threw TypeError: Class extends value () => 1 is not a constructor or null",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-twice', () => { class B extends A { constructor() { super(); super(); } } new B(); });",
        "super-twice threw ReferenceError: Super constructor may only be called once",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('this-first', () => { class B extends A { constructor() { this.x = 1; } } new B(); });",
        "this-first threw ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('no-super', () => { class B extends A { constructor() { } } new B(); });",
        "no-super threw ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('return-1', () => { class B extends A { constructor() { return 1; } } new B(); });",
        "return-1 threw TypeError: Derived constructors may only return object or undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('return-1-after', () => { class B extends A { constructor() { super(); return 1; } } new B(); });",
        "return-1-after threw TypeError: Derived constructors may only return object or undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('return-undefined', () => { class B extends A { constructor() { return undefined; } } new B(); });",
        "return-undefined threw ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('return-null', () => { class B extends A { constructor() { super(); return null; } } new B(); });",
        "return-null threw TypeError: Derived constructors may only return object or undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('delete-super', () => { class B extends A { m() { delete super.x; } } new B().m(); });",
        "delete-super threw ReferenceError: Unsupported reference to 'super'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('static-prototype-method', () => { class B { static ['prototype']() {} } });",
        "static-prototype-method threw TypeError: Classes may not have a static property named 'prototype'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('static-prototype-field', () => { class B { static [String('prototype')] = 1; } });",
        "static-prototype-field threw TypeError: Classes may not have a static property named 'prototype'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('static-prototype-getter', () => { class B { static get ['prototype']() {} } });",
        "static-prototype-getter threw TypeError: Classes may not have a static property named 'prototype'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('parent-null', () => { class B extends A {} Object.setPrototypeOf(B, null); new B(); });",
        "parent-null threw TypeError: Super constructor null of B is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('parent-object', () => { class B extends A {} Object.setPrototypeOf(B, {}); new B(); });",
        "parent-object threw TypeError: Super constructor #<Object> of B is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('parent-arrow', () => { class B extends A {} Object.setPrototypeOf(B, () => 1); new B(); });",
        "parent-arrow threw TypeError: Super constructor null of B is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('parent-anon-class', () => { var C = (0, class extends A {}); Object.setPrototypeOf(C, null); new C(); });",
        "parent-anon-class threw TypeError: Super constructor null of anonymous class is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('parent-in-ctor', () => { class B extends A { constructor() { Object.setPrototypeOf(B, null); super(); } } new B(); });",
        "parent-in-ctor threw TypeError: Super constructor null of B is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('tdz-extends', () => { class C extends C {} });",
        "tdz-extends threw ReferenceError: Cannot access 'C' before initialization",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('tdz-extends-expr', () => { var x = class C extends C {}; });",
        "tdz-extends-expr threw ReferenceError: Cannot access 'C' before initialization",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('assign-inner', () => { class C { m() { C = 1; } } new C().m(); });",
        "assign-inner threw TypeError: Assignment to constant variable.",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('own-keys', () => Object.getOwnPropertyNames(class {}).join() + ' ' + Object.getOwnPropertyNames(class { static m() {} }).join());",
        "own-keys length,name,prototype length,name,prototype,m",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('field-frozen', () => { var o = Object.freeze({}); class B { constructor() { return o; } } class D extends B { x = 1; } new D(); });",
        "field-frozen threw TypeError: Cannot define property x, object is not extensible",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('field-redefine', () => { class B { constructor() { return Object.defineProperty({}, 'x', { value: 1 }); } } class D extends B { x = 2; } new D(); });",
        "field-redefine threw TypeError: Cannot redefine property: x",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('field-symbol-nonext', () => { class B { constructor() { return Object.preventExtensions({}); } } class D extends B { [Symbol.iterator] = 2; } new D(); });",
        "field-symbol-nonext threw TypeError: Cannot define property Symbol(Symbol.iterator), object is not extensible",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-null-get', () => { class B { m() { return super.x; } } Object.setPrototypeOf(B.prototype, null); return new B().m(); });",
        "super-null-get threw TypeError: Cannot read properties of null (reading 'x')",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-null-set', () => { class B { m() { super.x = 1; } } Object.setPrototypeOf(B.prototype, null); return new B().m(); });",
        "super-null-set threw TypeError: Cannot set properties of null (setting 'x')",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-not-fn', () => { class B { m() { return super.m2(); } } return new B().m(); });",
        "super-not-fn threw TypeError: (intermediate value).m2 is not a function",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-not-fn-sym', () => { class B { m() { return super[Symbol.iterator](); } } return new B().m(); });",
        "super-not-fn-sym threw TypeError: (intermediate value)[Symbol.iterator] is not a function",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-set-prim', () => { class B { m() { super.x = 1; } } B.prototype.m.call(5); });",
        "super-set-prim threw TypeError: Cannot assign to read only property 'x' of number '5'",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-get-prim', () => { class B { m() { return super.x; } } return B.prototype.m.call(5); });",
        "super-get-prim undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-frozen-this', () => { class B { m() { super.x = 1; } } var o = Object.freeze(new B()); o.m(); });",
        "super-frozen-this threw TypeError: Cannot add property x, object is not extensible",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('literal-frozen', () => { var o = { m() { 'use strict'; super.x = 1; } }; Object.freeze(o); o.m(); });",
        "literal-frozen threw TypeError: Cannot add property x, object is not extensible",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('literal-sloppy-frozen', () => { var o = { m() { super.x = 1; return Object.keys(this).length; } }; Object.freeze(o); return o.m(); });",
        "literal-sloppy-frozen 1",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('delete-key-first', () => { class B extends A { constructor() { super(); delete super[(() => { throw 1; })()]; } } new B(); });",
        "delete-key-first threw Number: undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('arrow-this-before', () => { class B extends A { constructor() { (() => this)(); } } new B(); });",
        "arrow-this-before threw ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('super-prop-before', () => { class B extends A { constructor() { super.x; } } new B(); });",
        "super-prop-before threw ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('method-new', () => { class B { m() {} } return new B.prototype.m(); });",
        "method-new threw TypeError: B.prototype.m is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('getter-new', () => { class B { get g() {} } return new (Object.getOwnPropertyDescriptor(B.prototype, 'g').get)(); });",
        "getter-new threw TypeError: Object.getOwnPropertyDescriptor(...).get is not a constructor",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('toString', () => { class Q { static m() {} get g() { return 1; } *gen() {} ['x' + 1]() {} } return [Q, Q.m, Object.getOwnPropertyDescriptor(Q.prototype, 'g').get, Q.prototype.gen, Q.prototype.x1, class extends Q {}].join('\\n'); });",
        "toString class Q { static m() {} get g() { return 1; } *gen() {} ['x' + 1]() {} }\nm() {}\nget g() { return 1; }\n*gen() {}\n['x' + 1]() {}\nclass extends Q {}",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('toString-default', () => String(class   Z   extends   A   {  }));",
        "toString-default class   Z   extends   A   {  }",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('descriptors', () => { class C { m() {} get g() { return 1; } static s() {} } var d = Object.getOwnPropertyDescriptor(C.prototype, 'm'); var g = Object.getOwnPropertyDescriptor(C.prototype, 'g'); var c = Object.getOwnPropertyDescriptor(C.prototype, 'constructor'); var p = Object.getOwnPropertyDescriptor(C, 'prototype'); var n = Object.getOwnPropertyDescriptor(C, 'name'); return [d.writable, d.enumerable, d.configurable, g.enumerable, g.configurable, c.writable, c.enumerable, c.configurable, p.writable, p.enumerable, p.configurable, n.writable, n.configurable, C.length].join(); });",
        "descriptors true,false,true,false,true,true,false,true,false,false,false,false,true,0",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('ctor-length', () => { class C { constructor(a, b = 1, ...c) {} } class D extends C {} return C.length + ' ' + D.length; });",
        "ctor-length 1 0",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('name-sym-key', () => { var s = Symbol('sym'); var o = { [s]: class {} }; return o[s].name; });",
        "name-sym-key [sym]",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('name-static-method', () => { var C = class { static name() {} }; return typeof C.name; });",
        "name-static-method function",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('name-computed-static-field-sees-name', () => { var o = { ['n' + 1]: class { static x = this.name; } }; return o.n1.x; });",
        "name-computed-static-field-sees-name n1",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} t('console', () => { console.log(class Kx {}, class extends Error {}, class {}); return 'ok'; });",
        "[class Kx] [class (anonymous) extends Error] [class (anonymous)]\nconsole ok",
    ),
];

const CLASS_EDGES: &[(&str, &str)] = &[
    (
        // All parts of a class are strict, also in sloppy code (§11.2.2).
        "var r = []; function t(f) { try { f(); r.push('ok'); } catch (e) { r.push(e.name + ': ' + e.message); } } var o = Object.freeze({ a: 1 }); t(() => { class C extends (zz1 = Object) {} }); t(() => { class C { [(zz2 = 'a')]() {} } }); t(() => { class C extends (o.a = 2, Object) {} }); t(() => { class C { static [(o.a = 3, 'k')] = 1 } }); t(() => { class C { [delete o.a]() {} } }); t(() => { class C extends class extends (zz4 = Object) {} {} }); zz3 = 1; o.a = 9; r.push(typeof zz3, o.a, delete o.a); print(r.join(' | '));",
        "ReferenceError: zz1 is not defined | ReferenceError: zz2 is not defined | TypeError: Cannot assign to read only property 'a' of object '#<Object>' | TypeError: Cannot assign to read only property 'a' of object '#<Object>' | TypeError: Cannot delete property 'a' of #<Object> | ReferenceError: zz4 is not defined | number | 1 | false",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('key-tdz', () => { class C { [C] = 1; } });",
        "key-tdz threw ReferenceError: Cannot access 'C' before initialization",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('key-tdz-method', () => { class C { [C.name]() {} } });",
        "key-tdz-method threw ReferenceError: Cannot access 'C' before initialization",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('accessor-merge', () => { class C { static get x() { return 1; } static set x(v) { this.y = v; } } C.x = 5; return C.x + ' ' + C.y; });",
        "accessor-merge 1 5",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('default-name', () => { var { a = class {} } = {}; var [b = class {}] = []; return a.name + ' ' + b.name; });",
        "default-name a b",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('param-default-name', () => { function f(c = class {}) { return c.name; } return f(); });",
        "param-default-name c",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('assign-name', () => { var d; d = class {}; var e = (class {}); return d.name + ' ' + e.name; });",
        "assign-name d e",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('logical-name', () => { var g; g ??= class {}; return g.name; });",
        "logical-name g",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('closures-inner', () => { var fs = []; for (var i = 0; i < 2; i++) { class K { static f() { return K; } } fs.push(K); } return fs[0] !== fs[1] && fs[0].f() === fs[0]; });",
        "closures-inner true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('block-class', () => { { class Q {} } return typeof Q; });",
        "block-class undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('species', () => { class MA extends Array {} var m = new MA(1, 2, 3).slice(1); return (m instanceof MA) + ' ' + m.length; });",
        "species true 2",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('computed-literal-super', () => { var k = 'mm'; var o = { __proto__: { mm() { return 'p'; } }, [k]() { return super[k]() + '!'; } }; return o.mm(); });",
        "computed-literal-super p!",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('class-getter-super', () => { class A { get v() { return 1; } } class B extends A { get v() { return super.v + 1; } static get w() { return super.name; } } return new B().v + ' ' + B.w; });",
        "class-getter-super 2 A",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('arrow-in-field', () => { class A { m() { return 'A'; } } class B extends A { f = () => super.m() + this.constructor.name; } return new B().f(); });",
        "arrow-in-field AB",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('static-arrow-super', () => { class A { static s() { return 'S'; } } class B extends A { static g = () => super.s(); } return B.g(); });",
        "static-arrow-super S",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('this-in-static-block-arrow', () => { var r; class C { static { r = (() => this)(); } } return r === C; });",
        "this-in-static-block-arrow true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('static-block-var', () => { var x = 'outer'; class C { static { var x = 'inner'; } } return x; });",
        "static-block-var outer",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('field-shadow', () => { class A { constructor() { this.x = 'a'; } } class B extends A { x = 'b'; } return new B().x; });",
        "field-shadow b",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('field-setter-not-called', () => { var called = false; class A { set x(v) { called = true; } } class B extends A { x = 1; } var b = new B(); return called + ' ' + b.hasOwnProperty('x'); });",
        "field-setter-not-called false true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('super-in-arrow-default', () => { class A { m() { return 2; } } class B extends A { m(a = super.m()) { return a; } } return new B().m(); });",
        "super-in-arrow-default 2",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('super-eval-order', () => { var log = []; class A {} class B extends A { constructor() { super((log.push('arg'), 1)); log.push('after'); } } new B(); return log.join(); });",
        "super-eval-order arg,after",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('new-target-default', () => { var nt; class A { constructor() { nt = new.target; } } class B extends A {} new B(); return nt === B; });",
        "new-target-default true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('extends-expr', () => { function mk() { return class { hi() { return 'hi'; } }; } class B extends mk() {} return new B().hi(); });",
        "extends-expr hi",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('class-in-cond', () => { var C = true ? class X {} : null; return C.name; });",
        "class-in-cond X",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('proto-chain', () => { class A {} class B extends A {} return Object.getPrototypeOf(B) === A && Object.getPrototypeOf(B.prototype) === A.prototype; });",
        "proto-chain true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('ctor-prop', () => { class A {} var d = Object.getOwnPropertyDescriptor(A.prototype, 'constructor'); return d.writable + ' ' + d.enumerable + ' ' + d.configurable; });",
        "ctor-prop true false true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('sloppy-outer', () => { class A { m() { return typeof this; } } return A.prototype.m.call(5); });",
        "sloppy-outer number",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('instance-of-default', () => { class A {} class B extends A {} class C extends B {} return new C() instanceof A; });",
        "instance-of-default true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('getter-field-order', () => { var log = []; class C { static [(log.push('k1'), 'a')] = log.push('v1'); static [(log.push('k2'), 'b')] = log.push('v2'); } return log.join(); });",
        "getter-field-order k1,k2,v1,v2",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('super-call-args-spread', () => { class A { constructor(...a) { this.a = a.join(); } } class B extends A { constructor() { super(...[1, 2], 3); } } return new B().a; });",
        "super-call-args-spread 1,2,3",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('super-call-in-try', () => { class A {} class B extends A { constructor() { try { super(); } catch (e) {} } } return new B() instanceof B; });",
        "super-call-in-try true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('super-call-then-throw-caught', () => { class A { constructor() { throw new Error('x'); } } class B extends A { constructor() { try { super(); } catch (e) { return {caught: true}; } } } return new B().caught; });",
        "super-call-then-throw-caught true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } t('this-cell-ctor', () => { class A {} class B extends A { constructor() { var get = () => this; super(); this.ok = get() === this; } } return new B().ok; });",
        "this-cell-ctor true",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} class B extends A { constructor() { super(); return 5; } } t('reflect', () => Reflect.construct(B, []));",
        "reflect threw TypeError: Derived constructors may only return object or undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} class B extends A { constructor() { super(); return 5; } } t('map', () => [1].map(() => new B()));",
        "map threw TypeError: Derived constructors may only return object or undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} class B extends A { constructor() { super(); return 5; } } t('nested-finally', () => { var r = []; try { try { new B(); } finally { r.push('f'); } } catch (e) { r.push(e.constructor.name); } return r.join(); });",
        "nested-finally f,TypeError",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} class B extends A { constructor() { super(); return 5; } } t('bound', () => { var BB = B.bind(null); new BB(); });",
        "bound threw TypeError: Derived constructors may only return object or undefined",
    ),
    (
        "function t(label, f) { try { print(label, String(f())); } catch (e) { print(label, 'threw', e && e.constructor ? e.constructor.name + ': ' + e.message : e); } } class A {} class B extends A { constructor() { super(); return 5; } } t('after', () => { class C extends A { constructor() { super(); return 5; } } try { new C(); } catch (e) {} return new A() instanceof A; });",
        "after true",
    ),
];
