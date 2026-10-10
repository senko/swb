//! Stack traces of error objects in the format of Chromium (M7 feature 2c).
//! Each case is a script that prints stacks; the expected output is what
//! Node.js 22 prints for the same script run under the file name `t.js`
//! (`vm.runInThisContext` with that `filename`; the lines of Node's own
//! frames are removed). Each case also runs in the GC stress mode.
//!
//! Generated from the scripts and Node's output; cases whose output
//! differs from Node on purpose (built-in functions are not frames, the
//! names of functions with computed keys) are not in the lists.

#![allow(clippy::too_many_lines, reason = "a test is a list of script cases")]

mod common;

use common::check_script_cases;

/// Stack frames of calls, constructors, methods and natives' errors.
#[test]
fn stack_frames() {
    check_script_cases(&[(
        r"var e = new Error('boom');
var d = Object.getOwnPropertyDescriptor(e, 'stack'); print(Object.keys(d), d.enumerable, d.configurable);
print(Object.getOwnPropertyNames(e));
print(e.stack);
function f() { return new TypeError('bad'); }
function g() { return f(); }
print(g().stack);
var o = { m() { return new Error('m'); } };
print(o.m().stack);
var h = function () { return new Error('anon'); };
print(h().stack);
print((() => new Error('arrow'))().stack);
function C() { this.e = new Error('ctor'); }
print(new C().e.stack);
print(Object.getOwnPropertyDescriptor(Error.prototype, 'stack'));
var d = Object.getOwnPropertyDescriptor(Error, 'stackTraceLimit'); print(d.value, d.writable, d.enumerable, d.configurable);
print(typeof Error.captureStackTrace, Error.captureStackTrace.length);
var e2 = new Error('x'); e2.name = 'Foo'; e2.message = 'changed'; print(e2.stack);
try { null.x } catch (e) { print(e.stack) }
try { undefinedVar } catch (e) { print(e.stack) }
function rec(n) { return n ? rec(n - 1) : new Error('deep'); }
print(rec(20).stack);
print(Error('nonew').stack);
print(Error.prototype.hasOwnProperty('stack'));
",
        r"get,set,enumerable,configurable false true
stack,message
Error: boom
    at t.js:1:9
TypeError: bad
    at f (t.js:5:23)
    at g (t.js:6:23)
    at t.js:7:7
Error: m
    at Object.m (t.js:8:24)
    at t.js:9:9
Error: anon
    at h (t.js:10:30)
    at t.js:11:7
Error: arrow
    at t.js:12:14
    at t.js:12:33
Error: ctor
    at new C (t.js:13:25)
    at t.js:14:7
undefined
10 true true true
function 2
Foo: changed
    at t.js:18:10
TypeError: Cannot read properties of null (reading 'x')
    at t.js:19:12
ReferenceError: undefinedVar is not defined
    at t.js:20:7
Error: deep
    at rec (t.js:21:43)
    at rec (t.js:21:30)
    at rec (t.js:21:30)
    at rec (t.js:21:30)
    at rec (t.js:21:30)
    at rec (t.js:21:30)
    at rec (t.js:21:30)
    at rec (t.js:21:30)
    at rec (t.js:21:30)
    at rec (t.js:21:30)
Error: nonew
    at t.js:23:7
false
",
    )]);
}

/// The name of a function and the type of its receiver in a frame line.
#[test]
fn function_and_type_names() {
    check_script_cases(&[(
        r#"function S() { return new Error('e').stack; }
function T(label, v) { print(label + ': ' + v); }
var o = { m() { return S(); }, 'x y'() { return S(); }, f: function () { return S(); }, a: () => S(), g: function named() { return S(); } };
T('method', o.m());
T('quoted', o['x y']());
T('prop fn', o.f());
T('arrow prop', o.a());
T('named fn expr', o.g());
o.p = function () { return S(); }; T('assigned member', o.p());
o.q = function qq() { return S(); }; T('assigned named', o.q());
function Foo() {} Foo.prototype.bar = function () { return S(); };
T('proto method', new Foo().bar());
Foo.prototype.baz = function baz() { return S(); }; T('proto named', new Foo().baz());
T('fn receiver', (function () { var f = function () {}; f.m = function () { return S(); }; return f.m(); })());
T('array recv', (function () { var a = []; a.m = function () { return S(); }; return a.m(); })());
String.prototype.sx = function () { return S(); };
T('string recv', 'x'.sx());
T('call', (function () { function f() { return S(); } return f.call({}); })());
T('call strict', (function () { function f() { 'use strict'; return S(); } return f.call({}); })());
T('call undefined', (function () { function f() { return S(); } return f.call(undefined); })());
T('bind', (function () { function f() { return S(); } return f.bind({})(); })());
T('apply', (function () { function f() { return S(); } return f.apply({}, []); })());
T('func new', (function () { function F() { this.s = S(); } return new F().s; })());
T('func new anon', (function () { var F = function () { this.s = S(); }; return new F().s; })());
T('Reflect.construct', (function () { function F() { this.s = S(); } return Reflect.construct(F, []).s; })());
T('name prop', (function () { function f() { return S(); } Object.defineProperty(f, 'name', { value: 'zzz' }); return f(); })());
T('name prop nonstring', (function () { function f() { return S(); } Object.defineProperty(f, 'name', { value: 5 }); return f(); })());
T('iife anon', (function () { return S(); })());
T('iife anon call', (function () { return (function () { return S(); }).call({}); })());
T('constructor prop changed', (function () { function F() {} F.prototype.m = function () { return S(); }; var i = new F(); i.constructor = function Other() {}; return i.m(); })());
T('Object.create(proto)', (function () { var base = { m() { return S(); } }; return Object.create(base).m(); })());
T('window method', (function () { globalThis.gm = function () { return S(); }; return globalThis.gm(); })());
T('this.x = fn', (function () { function F() { this.x = function () { return S(); }; } return new F().x(); })());
T('a.b.c = fn', (function () { var a = { b: {} }; a.b.c = function () { return S(); }; return a.b.c(); })());
T('a.prototype.c = fn', (function () { function A() {} A.prototype.c = function () { return S(); }; return new A().c(); })());
T('a.b = fn arrow', (function () { var a = {}; a.b = () => S(); return a.b(); })());
T('a["b"] = fn', (function () { var a = {}; a['b'] = function () { return S(); }; return a.b(); })());
T('a[k] = fn', (function () { var a = {}, k = 'kk'; a[k] = function () { return S(); }; return a.kk(); })());
T('a.b = function named', (function () { var a = {}; a.b = function nn() { return S(); }; return a.b(); })());
T('var x = fn', (function () { var x; x = function () { return S(); }; return x(); })());
T('let y = ()=>', (function () { let y = () => S(); return y(); })());
T('number recv', (function () { Number.prototype.nm = function () { return S(); }; return (5).nm(); })());
T('bool strict', (function () { Boolean.prototype.bm = function () { 'use strict'; return S(); }; return true.bm(); })());
T('Object.assign method', (function () { var q = Object.assign({}, { mm() { return S(); } }); return q.mm(); })());
T('fn in array', (function () { var arr = [function () { return S(); }]; return arr[0](); })());
T('fn in array named', (function () { var arr = [function nm() { return S(); }]; return arr[0](); })());
T('paren', (function () { var q = { m() { return S(); } }; return (q.m)(); })());
T('comma', (function () { var q = { m() { return S(); } }; return (0, q.m)(); })());
T('fn recv named prop', (function () { function Q() {} Object.defineProperty(Q, 'mm', { value: function mm() { return S(); } }); return Q.mm(); })());
T('shared fn', (function () { var f = function () { return S(); }; var q = { a: f, b: f }; return q.a(); })());
T('shared fn named', (function () { function f() { return S(); } var q = { a: f }; return q.a(); })());
T('method alias', (function () { var q = { m() { return S(); } }; q.n = q.m; return q.n(); })());
T('inherited alias', (function () { var base = { m() { return S(); } }; var d = Object.create(base); d.n = base.m; return d.n(); })());
T('nested', (function outer() { return (function inner() { return S(); })(); })());
T('deep stack', (function r(n) { return n ? r(n - 1) : S(); })(12));
"#,
        r#"method: Error: e
    at S (t.js:1:23)
    at Object.m (t.js:3:24)
    at t.js:4:15
quoted: Error: e
    at S (t.js:1:23)
    at x y (t.js:3:49)
    at t.js:5:21
prop fn: Error: e
    at S (t.js:1:23)
    at Object.f (t.js:3:81)
    at t.js:6:16
arrow prop: Error: e
    at S (t.js:1:23)
    at Object.a (t.js:3:98)
    at t.js:7:19
named fn expr: Error: e
    at S (t.js:1:23)
    at Object.named [as g] (t.js:3:132)
    at t.js:8:22
assigned member: Error: e
    at S (t.js:1:23)
    at o.p (t.js:9:28)
    at t.js:9:59
assigned named: Error: e
    at S (t.js:1:23)
    at Object.qq [as q] (t.js:10:30)
    at t.js:10:60
proto method: Error: e
    at S (t.js:1:23)
    at Foo.bar (t.js:11:60)
    at t.js:12:29
proto named: Error: e
    at S (t.js:1:23)
    at Foo.baz (t.js:13:45)
    at t.js:13:80
fn receiver: Error: e
    at S (t.js:1:23)
    at f.m (t.js:14:84)
    at t.js:14:101
    at t.js:14:108
array recv: Error: e
    at S (t.js:1:23)
    at a.m (t.js:15:71)
    at t.js:15:88
    at t.js:15:95
string recv: Error: e
    at S (t.js:1:23)
    at String.sx (t.js:16:44)
    at t.js:17:22
call: Error: e
    at S (t.js:1:23)
    at Object.f (t.js:18:48)
    at t.js:18:64
    at t.js:18:76
call strict: Error: e
    at S (t.js:1:23)
    at Object.f (t.js:19:69)
    at t.js:19:85
    at t.js:19:97
call undefined: Error: e
    at S (t.js:1:23)
    at f (t.js:20:58)
    at t.js:20:74
    at t.js:20:93
bind: Error: e
    at S (t.js:1:23)
    at Object.f (t.js:21:48)
    at t.js:21:72
    at t.js:21:78
apply: Error: e
    at S (t.js:1:23)
    at Object.f (t.js:22:49)
    at t.js:22:65
    at t.js:22:82
func new: Error: e
    at S (t.js:1:23)
    at new F (t.js:23:54)
    at t.js:23:68
    at t.js:23:81
func new anon: Error: e
    at S (t.js:1:23)
    at new F (t.js:24:66)
    at t.js:24:81
    at t.js:24:94
Reflect.construct: Error: e
    at S (t.js:1:23)
    at new F (t.js:25:63)
    at t.js:25:85
    at t.js:25:107
name prop: Error: e
    at S (t.js:1:23)
    at zzz (t.js:26:53)
    at t.js:26:119
    at t.js:26:126
name prop nonstring: Error: e
    at S (t.js:1:23)
    at f (t.js:27:63)
    at t.js:27:125
    at t.js:27:132
iife anon: Error: e
    at S (t.js:1:23)
    at t.js:28:38
    at t.js:28:45
iife anon call: Error: e
    at S (t.js:1:23)
    at Object.<anonymous> (t.js:29:65)
    at t.js:29:73
    at t.js:29:85
constructor prop changed: Error: e
    at S (t.js:1:23)
    at F.m (t.js:30:99)
    at t.js:30:170
    at t.js:30:177
Object.create(proto): Error: e
    at S (t.js:1:23)
    at Object.m (t.js:31:68)
    at t.js:31:105
    at t.js:31:112
window method: Error: e
    at S (t.js:1:23)
    at globalThis.gm (t.js:32:72)
    at t.js:32:98
    at t.js:32:106
this.x = fn: Error: e
    at S (t.js:1:23)
    at F.x (t.js:33:78)
    at t.js:33:103
    at t.js:33:110
a.b.c = fn: Error: e
    at S (t.js:1:23)
    at a.b.c (t.js:34:80)
    at t.js:34:99
    at t.js:34:106
a.prototype.c = fn: Error: e
    at S (t.js:1:23)
    at A.c (t.js:35:93)
    at t.js:35:116
    at t.js:35:123
a.b = fn arrow: Error: e
    at S (t.js:1:23)
    at a.b (t.js:36:60)
    at t.js:36:74
    at t.js:36:81
a["b"] = fn: Error: e
    at S (t.js:1:23)
    at a.b (t.js:37:75)
    at t.js:37:92
    at t.js:37:99
a[k] = fn: Error: e
    at S (t.js:1:23)
    at a.<computed> [as kk] (t.js:38:81)
    at t.js:38:98
    at t.js:38:106
a.b = function named: Error: e
    at S (t.js:1:23)
    at Object.nn [as b] (t.js:39:83)
    at t.js:39:100
    at t.js:39:107
var x = fn: Error: e
    at S (t.js:1:23)
    at x (t.js:40:64)
    at t.js:40:79
    at t.js:40:86
let y = ()=>: Error: e
    at S (t.js:1:23)
    at y (t.js:41:48)
    at t.js:41:60
    at t.js:41:67
number recv: Error: e
    at S (t.js:1:23)
    at Number.nm (t.js:42:76)
    at t.js:42:95
    at t.js:42:103
bool strict: Error: e
    at S (t.js:1:23)
    at Boolean.bm (t.js:43:91)
    at t.js:43:111
    at t.js:43:119
Object.assign method: Error: e
    at S (t.js:1:23)
    at Object.mm (t.js:44:84)
    at t.js:44:104
    at t.js:44:112
fn in array: Error: e
    at S (t.js:1:23)
    at Array.arr (t.js:45:65)
    at t.js:45:87
    at t.js:45:93
fn in array named: Error: e
    at S (t.js:1:23)
    at Array.nm (t.js:46:73)
    at t.js:46:95
    at t.js:46:101
paren: Error: e
    at S (t.js:1:23)
    at Object.m (t.js:47:50)
    at t.js:47:72
    at t.js:47:78
comma: Error: e
    at S (t.js:1:23)
    at m (t.js:48:50)
    at t.js:48:75
    at t.js:48:81
fn recv named prop: Error: e
    at S (t.js:1:23)
    at Function.mm (t.js:49:119)
    at t.js:49:139
    at t.js:49:147
shared fn: Error: e
    at S (t.js:1:23)
    at Object.f (t.js:50:60)
    at t.js:50:101
    at t.js:50:108
shared fn named: Error: e
    at S (t.js:1:23)
    at Object.f [as a] (t.js:51:59)
    at t.js:51:93
    at t.js:51:100
method alias: Error: e
    at S (t.js:1:23)
    at Object.m (t.js:52:57)
    at t.js:52:87
    at t.js:52:94
inherited alias: Error: e
    at S (t.js:1:23)
    at Object.m (t.js:53:63)
    at t.js:53:125
    at t.js:53:132
nested: Error: e
    at S (t.js:1:23)
    at inner (t.js:54:67)
    at outer (t.js:54:74)
    at t.js:54:80
deep stack: Error: e
    at S (t.js:1:23)
    at r (t.js:55:56)
    at r (t.js:55:45)
    at r (t.js:55:45)
    at r (t.js:55:45)
    at r (t.js:55:45)
    at r (t.js:55:45)
    at r (t.js:55:45)
    at r (t.js:55:45)
    at r (t.js:55:45)
"#,
    )]);
}

/// The line and column of each frame for the expressions that fail or call.
#[test]
fn source_positions() {
    check_script_cases(&[(
        r"var o = { m() { return 1; }, n: null, a: [1], f() { return { g() { return 1; } }; } };
var u; var nul = null; var nf = 5;
var C = function () { throw new Error('c'); };
try { u.x; } catch (e) { print('u.x;'); print(e.stack); }
try { nul.x; } catch (e) { print('nul.x;'); print(e.stack); }
try { nul.x.y; } catch (e) { print('nul.x.y;'); print(e.stack); }
try { o.n.x; } catch (e) { print('o.n.x;'); print(e.stack); }
try { o.n[0]; } catch (e) { print('o.n[0];'); print(e.stack); }
try { o.n['a']; } catch (e) { print('o.n[\'a\'];'); print(e.stack); }
try { o.a[0].x.y; } catch (e) { print('o.a[0].x.y;'); print(e.stack); }
try { u(); } catch (e) { print('u();'); print(e.stack); }
try { o.zz(); } catch (e) { print('o.zz();'); print(e.stack); }
try { o.n.zz(); } catch (e) { print('o.n.zz();'); print(e.stack); }
try { o.m().zz(); } catch (e) { print('o.m().zz();'); print(e.stack); }
try { o['zz'](); } catch (e) { print('o[\'zz\']();'); print(e.stack); }
try { var k='zz'; o[k](); } catch (e) { print('var k=\'zz\'; o[k]();'); print(e.stack); }
try { nf(); } catch (e) { print('nf();'); print(e.stack); }
try { (function(){ throw new Error('x'); })(); } catch (e) { print('(function(){ throw new Error(\'x\'); })();'); print(e.stack); }
try { new nf(); } catch (e) { print('new nf();'); print(e.stack); }
try { new o.zz(); } catch (e) { print('new o.zz();'); print(e.stack); }
try { new C(); } catch (e) { print('new C();'); print(e.stack); }
try { o.f().g().zz(); } catch (e) { print('o.f().g().zz();'); print(e.stack); }
try { nul.foo = 1; } catch (e) { print('nul.foo = 1;'); print(e.stack); }
try { u.x = 1; } catch (e) { print('u.x = 1;'); print(e.stack); }
try { u[0] = 1; } catch (e) { print('u[0] = 1;'); print(e.stack); }
try { zzz; } catch (e) { print('zzz;'); print(e.stack); }
try { 1 + zzz; } catch (e) { print('1 + zzz;'); print(e.stack); }
try { 1 + (u.x); } catch (e) { print('1 + (u.x);'); print(e.stack); }
try { var a = 1, b = u.x; } catch (e) { print('var a = 1, b = u.x;'); print(e.stack); }
try { throw new Error('t'); } catch (e) { print('throw new Error(\'t\');'); print(e.stack); }
try { throw (new Error('t')); } catch (e) { print('throw (new Error(\'t\'));'); print(e.stack); }
try { var e = new Error('t'); throw e; } catch (e) { print('var e = new Error(\'t\'); throw e;'); print(e.stack); }
try { null instanceof 5; } catch (e) { print('null instanceof 5;'); print(e.stack); }
try { 'x' in 5; } catch (e) { print('\'x\' in 5;'); print(e.stack); }
try { Symbol() + ''; } catch (e) { print('Symbol() + \'\';'); print(e.stack); }
try { const c = 1; c = 2; } catch (e) { print('const c = 1; c = 2;'); print(e.stack); }
try { new Array(-1); } catch (e) { print('new Array(-1);'); print(e.stack); }
try { o.a.length = -1; } catch (e) { print('o.a.length = -1;'); print(e.stack); }
try { (function(){ 'use strict'; undefinedVar2 = 1; })(); } catch (e) { print('(function(){ \'use strict\'; undefinedVar2 = 1; })();'); print(e.stack); }
try { (function(){ 'use strict'; Object.freeze(o).q = 1; })(); } catch (e) { print('(function(){ \'use strict\'; Object.freeze(o).q = 1; })();'); print(e.stack); }
try { o
  .zz
  (); } catch (e) { print('o\n  .zz\n  ();'); print(e.stack); }
try { var f = () => u.x; f(); } catch (e) { print('var f = () => u.x; f();'); print(e.stack); }
try { u.x++; } catch (e) { print('u.x++;'); print(e.stack); }
try { u.x += 1; } catch (e) { print('u.x += 1;'); print(e.stack); }
try { -u.x; } catch (e) { print('-u.x;'); print(e.stack); }
try { u.x.y.z; } catch (e) { print('u.x.y.z;'); print(e.stack); }
try { (u.x)(); } catch (e) { print('(u.x)();'); print(e.stack); }
try { Reflect.construct(1); } catch (e) { print('Reflect.construct(1);'); print(e.stack); }
try { ({}).x.y; } catch (e) { print('({}).x.y;'); print(e.stack); }
try { [].x.y; } catch (e) { print('[].x.y;'); print(e.stack); }
try { 'str'.x.y; } catch (e) { print('\'str\'.x.y;'); print(e.stack); }
try { (1).x.y; } catch (e) { print('(1).x.y;'); print(e.stack); }
try { o.n.x.y; } catch (e) { print('o.n.x.y;'); print(e.stack); }
try { let z = z + 1; } catch (e) { print('let z = z + 1;'); print(e.stack); }
try { typeof zq; zq; } catch (e) { print('typeof zq; zq;'); print(e.stack); }
try { o.m(u.x); } catch (e) { print('o.m(u.x);'); print(e.stack); }
try { o.m(nul.x, 1); } catch (e) { print('o.m(nul.x, 1);'); print(e.stack); }
try { u.x(1); } catch (e) { print('u.x(1);'); print(e.stack); }
try { o.a[5].x; } catch (e) { print('o.a[5].x;'); print(e.stack); }
try { x
=
5; } catch (e) { print('x\n=\n5;'); print(e.stack); }
try { o.f().g()(); } catch (e) { print('o.f().g()();'); print(e.stack); }
try { o.f()['g'](); } catch (e) { print('o.f()[\'g\']();'); print(e.stack); }
try { (o.m)(); } catch (e) { print('(o.m)();'); print(e.stack); }
try { (0, o.m)(); } catch (e) { print('(0, o.m)();'); print(e.stack); }
try { o.m
(); } catch (e) { print('o.m\n();'); print(e.stack); }
try { o.zz /* c */ (); } catch (e) { print('o.zz /* c */ ();'); print(e.stack); }
try { var q = { valueOf() { return u.x; } }; q + 1; } catch (e) { print('var q = { valueOf() { return u.x; } }; q + 1;'); print(e.stack); }
try { var q = { toString() { return u.x; } }; q + ''; } catch (e) { print('var q = { toString() { return u.x; } }; q + \'\';'); print(e.stack); }
try { function F() { u.x; } new F(); } catch (e) { print('function F() { u.x; } new F();'); print(e.stack); }
try { function F() { u.x; } new F; } catch (e) { print('function F() { u.x; } new F;'); print(e.stack); }
try { function F() { u.x; } new F
  (); } catch (e) { print('function F() { u.x; } new F\n  ();'); print(e.stack); }
try { function F() { return new Error('f'); } throw new F(); } catch (e) { print('function F() { return new Error(\'f\'); } throw new F();'); print(e.stack); }
try { var h = { m: function () { u.x; } }; h.m(); } catch (e) { print('var h = { m: function () { u.x; } }; h.m();'); print(e.stack); }
try { var h = { m: function () { u.x; } }; h['m'](); } catch (e) { print('var h = { m: function () { u.x; } }; h[\'m\']();'); print(e.stack); }
try { u.x
  .y; } catch (e) { print('u.x\n  .y;'); print(e.stack); }
try { o.a
  [5].y; } catch (e) { print('o.a\n  [5].y;'); print(e.stack); }
",
        r"u.x;
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:4:9
nul.x;
TypeError: Cannot read properties of null (reading 'x')
    at t.js:5:11
nul.x.y;
TypeError: Cannot read properties of null (reading 'x')
    at t.js:6:11
o.n.x;
TypeError: Cannot read properties of null (reading 'x')
    at t.js:7:11
o.n[0];
TypeError: Cannot read properties of null (reading '0')
    at t.js:8:10
o.n['a'];
TypeError: Cannot read properties of null (reading 'a')
    at t.js:9:10
o.a[0].x.y;
TypeError: Cannot read properties of undefined (reading 'y')
    at t.js:10:16
u();
TypeError: u is not a function
    at t.js:11:7
o.zz();
TypeError: o.zz is not a function
    at t.js:12:9
o.n.zz();
TypeError: Cannot read properties of null (reading 'zz')
    at t.js:13:11
o.m().zz();
TypeError: o.m(...).zz is not a function
    at t.js:14:13
o['zz']();
TypeError: o.zz is not a function
    at t.js:15:14
var k='zz'; o[k]();
TypeError: o[k] is not a function
    at t.js:16:23
nf();
TypeError: nf is not a function
    at t.js:17:7
(function(){ throw new Error('x'); })();
Error: x
    at t.js:18:26
    at t.js:18:44
new nf();
TypeError: nf is not a constructor
    at t.js:19:7
new o.zz();
TypeError: o.zz is not a constructor
    at t.js:20:7
new C();
Error: c
    at new C (t.js:3:29)
    at t.js:21:7
o.f().g().zz();
TypeError: o.f(...).g(...).zz is not a function
    at t.js:22:17
nul.foo = 1;
TypeError: Cannot set properties of null (setting 'foo')
    at t.js:23:15
u.x = 1;
TypeError: Cannot set properties of undefined (setting 'x')
    at t.js:24:11
u[0] = 1;
TypeError: Cannot set properties of undefined (setting '0')
    at t.js:25:12
zzz;
ReferenceError: zzz is not defined
    at t.js:26:7
1 + zzz;
ReferenceError: zzz is not defined
    at t.js:27:11
1 + (u.x);
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:28:14
var a = 1, b = u.x;
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:29:24
throw new Error('t');
Error: t
    at t.js:30:13
throw (new Error('t'));
Error: t
    at t.js:31:14
var e = new Error('t'); throw e;
Error: t
    at t.js:32:15
null instanceof 5;
TypeError: Right-hand side of 'instanceof' is not an object
    at t.js:33:12
'x' in 5;
TypeError: Cannot use 'in' operator to search for 'x' in 5
    at t.js:34:11
Symbol() + '';
TypeError: Cannot convert a Symbol value to a string
    at t.js:35:16
const c = 1; c = 2;
TypeError: Assignment to constant variable.
    at t.js:36:22
new Array(-1);
RangeError: Invalid array length
    at t.js:37:7
o.a.length = -1;
RangeError: Invalid array length
    at t.js:38:18
(function(){ 'use strict'; undefinedVar2 = 1; })();
ReferenceError: undefinedVar2 is not defined
    at t.js:39:48
    at t.js:39:55
(function(){ 'use strict'; Object.freeze(o).q = 1; })();
TypeError: Cannot add property q, object is not extensible
    at t.js:40:53
    at t.js:40:60
o
  .zz
  ();
TypeError: o.zz is not a function
    at t.js:42:4
var f = () => u.x; f();
TypeError: Cannot read properties of undefined (reading 'x')
    at f (t.js:44:23)
    at t.js:44:26
u.x++;
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:45:7
u.x += 1;
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:46:7
-u.x;
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:47:10
u.x.y.z;
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:48:9
(u.x)();
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:49:10
Reflect.construct(1);
TypeError: 1 is not a constructor
    at t.js:50:15
({}).x.y;
TypeError: Cannot read properties of undefined (reading 'y')
    at t.js:51:14
[].x.y;
TypeError: Cannot read properties of undefined (reading 'y')
    at t.js:52:12
'str'.x.y;
TypeError: Cannot read properties of undefined (reading 'y')
    at t.js:53:15
(1).x.y;
TypeError: Cannot read properties of undefined (reading 'y')
    at t.js:54:13
o.n.x.y;
TypeError: Cannot read properties of null (reading 'x')
    at t.js:55:11
let z = z + 1;
ReferenceError: Cannot access 'z' before initialization
    at t.js:56:15
typeof zq; zq;
ReferenceError: zq is not defined
    at t.js:57:18
o.m(u.x);
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:58:13
o.m(nul.x, 1);
TypeError: Cannot read properties of null (reading 'x')
    at t.js:59:15
u.x(1);
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:60:9
o.a[5].x;
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:61:14
o.f().g()();
TypeError: o.f(...).g(...) is not a function
    at t.js:65:16
o.zz /* c */ ();
TypeError: o.zz is not a function
    at t.js:71:9
var q = { valueOf() { return u.x; } }; q + 1;
TypeError: Cannot read properties of undefined (reading 'x')
    at Object.valueOf (t.js:72:38)
    at t.js:72:48
var q = { toString() { return u.x; } }; q + '';
TypeError: Cannot read properties of undefined (reading 'x')
    at Object.toString (t.js:73:39)
    at t.js:73:49
function F() { u.x; } new F();
TypeError: Cannot read properties of undefined (reading 'x')
    at new F (t.js:74:24)
    at t.js:74:29
function F() { u.x; } new F;
TypeError: Cannot read properties of undefined (reading 'x')
    at new F (t.js:75:24)
    at t.js:75:29
function F() { u.x; } new F
  ();
TypeError: Cannot read properties of undefined (reading 'x')
    at new F (t.js:76:24)
    at t.js:76:29
function F() { return new Error('f'); } throw new F();
Error: f
    at new F (t.js:78:29)
    at t.js:78:53
var h = { m: function () { u.x; } }; h.m();
TypeError: Cannot read properties of undefined (reading 'x')
    at Object.m (t.js:79:36)
    at t.js:79:46
var h = { m: function () { u.x; } }; h['m']();
TypeError: Cannot read properties of undefined (reading 'x')
    at Object.m (t.js:80:36)
    at t.js:80:50
u.x
  .y;
TypeError: Cannot read properties of undefined (reading 'x')
    at t.js:81:9
o.a
  [5].y;
TypeError: Cannot read properties of undefined (reading 'y')
    at t.js:84:7
",
    )]);
}

/// The `stack` property, `Error.stackTraceLimit`, `Error.captureStackTrace`.
#[test]
fn stack_property_and_api() {
    check_script_cases(&[(
        r"var e = new Error('boom');
var d = Object.getOwnPropertyDescriptor(e, 'stack');
print(Object.keys(d), typeof d.get, typeof d.set, d.value === undefined, 'writable' in d);
e.stack = 'custom'; print(e.stack);
print(Object.keys(Object.getOwnPropertyDescriptor(e, 'stack')));
var e3 = new Error('a', { cause: 'c' });
print(Object.getOwnPropertyNames(e3));
var e4 = new Error();
print(Object.getOwnPropertyNames(e4));
print(Object.getOwnPropertyNames(Object.create(Error.prototype)));
var o = {}; Error.captureStackTrace(o);
print(Object.getOwnPropertyNames(o));
print(o.stack);
function Foo() { Error.captureStackTrace(this, Foo); }
var foo = new Foo(); print(foo.stack);
var o2 = { name: 'N', message: 'M' }; Error.captureStackTrace(o2); print(o2.stack);
var o3 = { toString() { return 'TS'; } }; Error.captureStackTrace(o3); print(o3.stack);
function a() { var q = {}; Error.captureStackTrace(q, a); return q.stack; }
function b() { return a(); }
print(b());
function a2() { var q = {}; Error.captureStackTrace(q, b2); return q.stack; }
function b2() { return a2(); }
function c2() { return b2(); }
print(c2());
print(Error.captureStackTrace(o), 'ret');
try { Error.captureStackTrace(1); } catch (x) { print(x.message); }
try { Error.captureStackTrace(); } catch (x) { print(x.message); }
Error.stackTraceLimit = 2;
function r(n) { return n ? r(n - 1) : new Error('lim'); }
print(r(5).stack);
Error.stackTraceLimit = 0;
print(r(5).stack);
Error.stackTraceLimit = 'x';
var ex = r(5); print(ex.stack, Object.getOwnPropertyNames(ex));
Error.stackTraceLimit = -1;
print(r(5).stack);
Error.stackTraceLimit = 1.9;
print(r(5).stack);
delete Error.stackTraceLimit;
print(r(5).stack);
Error.stackTraceLimit = 10;
var e5 = new TypeError('t'); print(Object.getOwnPropertyNames(e5));
print(Object.getOwnPropertyNames(TypeError));
print(Object.getOwnPropertyNames(Error.prototype));
print(Object.getOwnPropertyNames(TypeError.prototype));
print(Object.getOwnPropertyNames(AggregateError.prototype));
var ae = new AggregateError([1, 2], 'agg', { cause: 5 });
print(Object.getOwnPropertyNames(ae), ae.stack);
var d = Object.getOwnPropertyDescriptor(ae, 'errors'); print(d.value, d.writable, d.enumerable, d.configurable);
var e = new Error('m1');
e.name = 'N1'; e.message = 'm2';
print(e.stack);
e.name = 'N2';
print(e.stack);
var e = new Error('m1');
print(e.stack);
e.name = 'N2';
print(e.stack);
var d = Object.getOwnPropertyDescriptor(new Error('q'), 'stack');
var d2 = Object.getOwnPropertyDescriptor(new Error('q'), 'stack');
print(d.get === d2.get, d.set === d2.set, d.get.name === '', d.get.length, d.set.name === '', d.set.length);
print(d.get.toString());
var other = {};
print(d.get.call(other));
d.set.call(other, 5);
print(Object.getOwnPropertyNames(other), other.stack);
print(d.get.call(new Error('zz')));
var e = new Error('lazy');
Object.defineProperty(e, 'message', { get() { print('getter run'); return 'G'; } });
print('before');
print(e.stack);
print(e.stack);
var e = new Error('ts');
e.toString = function () { return 'OVR'; };
print(e.stack);
var e = Object.freeze(new Error('fr'));
print(e.stack);
e.stack = 'zzz';
print(e.stack);
var e = new Error('del');
print(delete e.stack, e.stack);
var e = new Error({ toString() { return 'obj'; } });
print(e.stack);
var e = new Error('x'); e.name = undefined; print(e.stack);
var e = new Error('x'); e.name = ''; print(e.stack);
var e = new Error(''); e.name = 'A'; print(e.stack);
var e = new Error('x'); e.name = 5; print(e.stack);
var e = new Error('x'); Object.defineProperty(e, 'name', { get() { throw new Error('nm'); } });
try { print(e.stack); } catch (x) { print('threw', x.message); }
var f = Object.freeze({});
try { Error.captureStackTrace(f); print('ok frozen', Object.getOwnPropertyNames(f)); } catch (x) { print(x.name, x.message); }
var g = Object.preventExtensions({});
try { Error.captureStackTrace(g); print('ok nonext'); } catch (x) { print(x.name, x.message); }
var fn = function () {}; Error.captureStackTrace(fn); print(typeof fn.stack);
var e = new Error('hh'); Error.captureStackTrace(e); e.name = 'Z'; print(e.stack);
print(Error.prototype.stack, 'stack' in Error.prototype);
var e = new Error('d'); Object.defineProperty(e, 'stack', { value: 'v', writable: false }); print(e.stack);
var e = new Error('k'); print(Object.keys(e));
var e = new Error('c'); var e2 = Object.create(e); print(typeof e2.stack, Object.getOwnPropertyNames(e2));
print(String(new Error('s')));
print(Object.prototype.toString.call(new Error('x')));
var e = new Error('m'); var d = Object.getOwnPropertyDescriptor(e, 'message'); print(d.value, d.writable, d.enumerable, d.configurable);
var e = new Error('x', { cause: undefined }); print(Object.getOwnPropertyNames(e), 'cause' in e);
var opt = {}; Object.defineProperty(opt, 'cause', { get: function () { print('get cause'); return 1; } }); var e = new Error('x', opt); print(Object.getOwnPropertyNames(e));
var e = new Error('x', 5); print(Object.getOwnPropertyNames(e));
print(Error.length, TypeError.length, AggregateError.length, Error.name, AggregateError.name);
print(Object.getPrototypeOf(TypeError) === Error, Object.getPrototypeOf(AggregateError) === Error);
print(Object.getPrototypeOf(AggregateError.prototype) === Error.prototype);
print(AggregateError.prototype.name, AggregateError.prototype.message === '', AggregateError.prototype.hasOwnProperty('errors'));
print(typeof Error.prototype.toString, Error.prototype.toString.call({ name: 'N', message: 'M' }));
try { Error.prototype.toString.call(1); } catch (x) { print(x.message); }
try { Error.prototype.toString.call(undefined); } catch (x) { print(x.message); }
try { new AggregateError(); } catch (x) { print(x.name, x.message); }
try { new AggregateError(5); } catch (x) { print(x.name, x.message); }
try { AggregateError(); } catch (x) { print(x.name, x.message); }
print(AggregateError([1], 'm').message);
print(Object.getOwnPropertyNames(AggregateError([1], 'm')));
",
        r"get,set,enumerable,configurable function function true false
custom
get,set,enumerable,configurable
stack,message,cause
stack

stack
Error
    at t.js:11:19
Error
    at t.js:15:11
N: M
    at t.js:16:45
Error
    at t.js:17:49
Error
    at b (t.js:19:23)
    at t.js:20:7
Error
    at c2 (t.js:23:24)
    at t.js:24:7
undefined ret
invalid_argument
invalid_argument
Error: lim
    at r (t.js:29:39)
    at r (t.js:29:28)
Error: lim
undefined stack,message
Error: lim
Error: lim
    at r (t.js:29:39)
undefined
stack,message
length,name,prototype
constructor,name,message,toString
constructor,name,message
constructor,name,message
stack,message,cause,errors AggregateError: agg
    at t.js:47:10
1,2 true false true
N1: m2
    at t.js:50:9
N1: m2
    at t.js:50:9
Error: m1
    at t.js:55:9
Error: m1
    at t.js:55:9
true true true 0 true 1
function () { [native code] }
undefined
 undefined
Error: zz
    at t.js:67:18
before
getter run
Error: G
    at t.js:68:9
Error: G
    at t.js:68:9
Error: ts
    at t.js:73:9
Error: fr
    at t.js:76:23
zzz
true undefined
Error: obj
    at t.js:82:9
Error: x
    at t.js:84:9
x
    at t.js:85:9
A
    at t.js:86:9
5: x
    at t.js:87:9
threw nm
TypeError Cannot define property stack, object is not extensible
TypeError Cannot define property stack, object is not extensible
string
Z: hh
    at t.js:95:32
undefined false
v

string 
Error: s
[object Error]
m true false true
stack,message,cause true
get cause
stack,message,cause
stack,message
1 1 2 Error AggregateError
true true
true
AggregateError true false
function N: M
Method Error.prototype.toString called on incompatible receiver 1
Method Error.prototype.toString called on incompatible receiver undefined
TypeError undefined is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError number 5 is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError undefined is not iterable (cannot read property Symbol(Symbol.iterator))
m
stack,message,errors
",
    )]);
}

/// `new.target` hides the frames above and including its innermost call;
/// a `new.target` that is not on the stack gives no frames; a bound
/// function hides nothing.
#[test]
fn new_target_hides_frames() {
    check_script_cases(&[(
        r"function F() { return Reflect.construct(Error, ['m'], F); }
function g() { return new F(); }
print(g().stack);
print(Reflect.construct(Error, ['m'], Object).stack);
print(Reflect.construct(TypeError, ['m'], Error).stack);
print(Reflect.construct(Error, ['m'], TypeError).stack);
var kinds = [TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError];
function each(C) {
  function H() { return Reflect.construct(C, ['m'], H); }
  function h() { return new H(); }
  print(h().stack);
  print(Reflect.construct(C, ['m'], Object).stack);
}
for (var i = 0; i < kinds.length; i++) each(kinds[i]);
function A() { return Reflect.construct(AggregateError, [[], 'm'], A); }
function a() { return new A(); }
print(a().stack);
print(Reflect.construct(AggregateError, [[], 'm'], Object).stack);
function B() { return Reflect.construct(Error, ['m'], B.bind(null)); }
function b() { return new B(); }
print(b().stack);
function R(n) { if (n > 0) return R(n - 1); return Reflect.construct(Error, ['m'], R); }
function r() { return R(2); }
print(r().stack);
function R2(n) { if (n > 0) return new R2(n - 1); return Reflect.construct(Error, ['m'], R2); }
print(R2(2).stack);
print(Error.call(null, 'x').stack);
",
        r"Error: m
    at g (t.js:2:23)
    at t.js:3:7
Error: m
Error: m
TypeError: m
Error: m
    at h (t.js:10:25)
    at each (t.js:11:9)
    at t.js:14:40
Error: m
Error: m
    at h (t.js:10:25)
    at each (t.js:11:9)
    at t.js:14:40
Error: m
Error: m
    at h (t.js:10:25)
    at each (t.js:11:9)
    at t.js:14:40
Error: m
Error: m
    at h (t.js:10:25)
    at each (t.js:11:9)
    at t.js:14:40
Error: m
Error: m
    at h (t.js:10:25)
    at each (t.js:11:9)
    at t.js:14:40
Error: m
Error: m
    at h (t.js:10:25)
    at each (t.js:11:9)
    at t.js:14:40
Error: m
Error: m
    at a (t.js:16:23)
    at t.js:17:7
Error: m
Error: m
    at new B (t.js:19:31)
    at b (t.js:20:23)
    at t.js:21:7
Error: m
    at R (t.js:22:35)
    at R (t.js:22:35)
    at r (t.js:23:23)
    at t.js:24:7
Error: m
    at new R2 (t.js:25:36)
    at R2 (t.js:25:36)
    at t.js:26:7
Error: x
    at t.js:27:13
",
    )]);
}

/// The type name of a receiver is the name that its constructor was
/// created with, not its `name` property.
#[test]
fn receiver_type_uses_internal_name() {
    check_script_cases(&[(
        r"function F() {}
Object.defineProperty(F, 'name', { get() { return 'ZZ'; } });
var q = { f() { return new Error('x'); }, constructor: F };
print(q.f().stack);
var r = Object.create(q);
print(r.f().stack);
function G() {}
Object.defineProperty(G, 'name', { value: 'Renamed' });
var q2 = { f() { return new Error('x'); } };
q2.constructor = G;
var r2 = Object.create(q2);
print(r2.f().stack);
var Anon = function () {};
Object.defineProperty(Anon, 'name', { get() { return 'ZZ'; } });
var q3 = { f() { return new Error('x'); } };
Object.getPrototypeOf(q3).constructor;
var r3 = Object.create({ f() { return new Error('x'); }, constructor: Anon });
print(r3.f().stack);
",
        r"Error: x
    at Object.f (t.js:3:24)
    at t.js:4:9
Error: x
    at F.f (t.js:3:24)
    at t.js:6:9
Error: x
    at G.f (t.js:9:25)
    at t.js:12:10
Error: x
    at Anon.f (t.js:17:39)
    at t.js:18:10
",
    )]);
}
