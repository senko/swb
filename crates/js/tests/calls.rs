//! The call paths of the interpreter (ADR 0026 sections 5 and 15, M7
//! 13b): the frame setup of script calls, the binding of `this`, the
//! arguments objects (made from a template after the first one of a
//! realm), native calls and their errors. Every case runs normally and in
//! the GC stress mode. The expected outputs come from Node.js 22 (a black
//! box), with `print` for `console.log`.

mod common;

use common::check_cases;

/// The first arguments object of a realm gives the template of the later
/// ones (sloppy and strict); changes to the first object do not reach
/// them.
#[test]
fn arguments_objects_after_the_first_of_a_realm() {
    check_cases(ARGUMENTS_TEMPLATES);
}

/// Mapped arguments alias their parameters both ways (also after `delete`
/// and `defineProperty`, with repeated names and missing arguments);
/// unmapped and strict ones do not.
#[test]
fn mapped_and_unmapped_arguments() {
    check_cases(MAPPED);
}

/// Iteration, keys, identity, `callee`, many arguments, nested functions
/// and arrows.
#[test]
fn arguments_object_contents() {
    check_cases(CONTENTS);
}

/// `OrdinaryCallBindThis` (§10.2.1.2) for sloppy, strict and arrow
/// functions.
#[test]
fn this_binding() {
    check_cases(THIS);
}

/// Missing and extra arguments, `call`, `apply`, `bind`, `new.target`,
/// class constructors without `new`, deep recursion and its `RangeError`,
/// the messages of calls of values that are not functions, natives.
#[test]
fn call_paths() {
    check_cases(CALLS);
}

/// Generators and async functions that use `arguments` and suspend.
#[test]
fn generators_and_async_functions() {
    check_cases(SUSPENDING);
}

const ARGUMENTS_TEMPLATES: &[(&str, &str)] = &[(
    r"function first(a, b) { arguments.length = 7; arguments.extra = 1; delete arguments.callee; arguments[0] = 'x'; return a; }
function later(a, b) { return [arguments.length, typeof arguments.extra, typeof arguments.callee, arguments[0], a, Object.getOwnPropertyNames(arguments).join()].join(' '); }
print(first(1, 2), later(3), later(4, 5, 6));
function strictFirst() { 'use strict'; arguments.length = 0; return Object.getOwnPropertyNames(arguments).join(); }
function strictLater(a) { 'use strict'; var d = Object.getOwnPropertyDescriptor(arguments, 'callee'); return [arguments.length, typeof d.get, d.get === d.set, d.enumerable, d.configurable, Object.getOwnPropertyNames(arguments).join()].join(' '); }
print(strictFirst(1, 2), strictLater(), strictLater(1, 2, 3));",
    r"x 1 undefined function 3 3 0,length,callee 3 undefined function 4 4 0,1,2,length,callee
0,1,length,callee 0 function true false false length,callee 3 function true false false 0,1,2,length,callee",
)];

const MAPPED: &[(&str, &str)] = &[(
    r"function m(a, b) { arguments[0] = 10; b = 20; var r = [a, arguments[1], arguments.length]; arguments[2] = 30; return r.concat(arguments[2], arguments.length).join(); }
print(m(1, 2), m(1), m(1, 2, 3, 4));
function md(a) { delete arguments[0]; arguments[0] = 5; return a + ',' + arguments[0]; }
print(md(1), md(2));
function mdef(a) { Object.defineProperty(arguments, '0', { value: 7, writable: false }); a = 8; return a + ',' + arguments[0]; }
print(mdef(1), mdef(2));
function dup(a, a) { arguments[1] = 9; return a + ',' + arguments[0] + ',' + arguments[1]; }
print(dup(1, 2), dup(1));
function unm(a = 0, b) { arguments[0] = 99; return a + ',' + arguments[0] + ',' + arguments.length; }
print(unm(1, 2), unm());
function sm(a) { 'use strict'; arguments[0] = 99; a = 5; return a + ',' + arguments[0]; }
print(sm(1), sm(2));",
    r"10,20,2,30,2 10,,1,30,1 10,20,4,30,4
1,5 2,5
8,7 8,7
9,1,9 undefined,1,9
1,99,2 0,99,0
5,99 5,99",
)];

const CONTENTS: &[(&str, &str)] = &[(
    r"function it() { return [...arguments].join('-') + ' ' + Array.prototype.slice.call(arguments, 1).join('-'); }
print(it(1, 2, 3), it(), it('a'));
function keys() { var k = []; for (var p in arguments) k.push(p); return k.join() + ' ' + JSON.stringify(arguments) + ' ' + Object.keys(arguments).length; }
print(keys(1, 2), keys());
function same() { return arguments === arguments; }
function distinct() { return arguments; }
print(same(), distinct(1) !== distinct(1), Object.getPrototypeOf(distinct()) === Object.prototype, Object.prototype.toString.call(distinct()));
function callee() { return arguments.callee === callee; }
print(callee(), callee(1, 2));
function many() { return arguments.length + ':' + arguments[arguments.length - 1]; }
var big = []; for (var i = 0; i < 1000; i++) big.push(i);
print(many.apply(null, big), many.apply(null, []), many(...big.slice(0, 3)));
function nested() { var outer = arguments; return (function () { return outer.length + arguments.length; })(1, 2, 3); }
print(nested(1), nested());
function arrow() { return (() => arguments[0] + arguments.length)(); }
print(arrow(5, 6), arrow());",
    r#"1-2-3 2-3   a 
0,1 {"0":1,"1":2} 2  {} 0
true true true [object Arguments]
true true
1000:999 0:undefined 3:2
4 3
7 NaN"#,
)];

const THIS: &[(&str, &str)] = &[(
    r"function sloppy() { return typeof this + ':' + (this === globalThis); }
function strict() { 'use strict'; return typeof this + ':' + String(this); }
print(sloppy(), sloppy.call(undefined), sloppy.call(null), sloppy.call(1), sloppy.call('s'), sloppy.call({}));
print(strict(), strict.call(undefined), strict.call(null), strict.call(1), strict.call('s'));
var o = { f: sloppy, g: strict, h: () => typeof this };
print(o.f(), o.g(), o.h(), (0, o.f)(), (0, o.g)());
Number.prototype.self = function () { return typeof this; }; Boolean.prototype.s2 = function () { 'use strict'; return typeof this; };
print((5).self(), true.s2());",
    r"object:true object:true object:true object:false object:false object:false
undefined:undefined undefined:undefined object:null number:1 string:s
object:false object:[object Object] object object:true undefined:undefined
object boolean",
)];

const CALLS: &[(&str, &str)] = &[(
    r"function count(a, b, c) { return [a, b, c].join(); }
print(count(), count(1), count(1, 2, 3, 4), count.call(null, 1, 2), count.apply(null, [1, 2, 3]), count.bind(null, 1)(2));
function nt() { return new.target === nt; }
print(nt(), new nt() instanceof nt, Reflect.construct(nt, []) instanceof nt);
class K { constructor(x) { this.x = x; } }
try { K(1); } catch (e) { print(e.constructor.name, e.message); }
try { (class {})(); } catch (e) { print(e.message); }
function rec(n) { return n === 0 ? 0 : 1 + rec(n - 1); }
print(rec(5000));
try { (function deep() { deep(); })(); } catch (e) { print(e.constructor.name, e.message); }
function thrower() { throw new Error('boom'); }
function mid() { return thrower(); }
try { mid(); } catch (e) { print(e.message, e.stack.split('\n').length > 2); }
var notf = 1;
try { notf(); } catch (e) { print(e.message); }
try { new count.bind()(); print('ok'); } catch (e) { print(e.message); }
try { new Math.abs(); } catch (e) { print(e.message); }
print([1, 2, 3].map(function (x) { return x * this.k; }, { k: 2 }).join());
print(Date.now() > 0, typeof Date.now(), Math.max(1, 5, 3), 'abc'.slice(1), [1].push(2, 3));",
    r",, 1,, 1,2,3 1,2, 1,2,3 1,2,
false true true
TypeError Class constructor K cannot be invoked without 'new'
Class constructors cannot be invoked without 'new'
5000
RangeError Maximum call stack size exceeded
boom true
notf is not a function
count.bind is not a constructor
Math.abs is not a constructor
2,4,6
true number 5 bc 3",
)];

const SUSPENDING: &[(&str, &str)] = &[(
    r"function* gen(a) { var x = yield a + arguments.length; yield x * 2; return arguments[0]; }
var g = gen(3, 4);
print(JSON.stringify(g.next()), JSON.stringify(g.next(5)), JSON.stringify(g.next()), JSON.stringify(g.next()));
async function af(a) { await null; return a + arguments.length; }
af(1, 2).then(v => print('async', v));
function wrap(x) { return af(x); }
wrap(10).then(v => print('wrapped', v));
undefined;",
    r#"{"value":5,"done":false} {"value":10,"done":false} {"value":3,"done":true} {"done":true}
async 3
wrapped 11"#,
)];
