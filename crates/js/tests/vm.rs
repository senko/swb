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

/// Review fixes of session 4 (array `length` conversions, messages, the
/// tie rule of `Number::toString`).
#[test]
fn review_fixes() {
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
