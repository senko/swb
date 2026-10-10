//! Cases for `Object`, `Reflect`, `Function.prototype.bind` and the
//! property paths (M7 feature 2a). The expected output of each case is the
//! output of Node.js 22 for the same script (`print` joins `String` of its
//! arguments). Each case also runs in the GC stress mode.
//!
//! Generated from a list of scripts and Node's output, then edited by hand;
//! cases whose output differs from Node on purpose say so.

mod common;

use common::check_cases;

#[test]
fn bound_basic() {
    check_cases(&[(
        r"function foo(a, b, c) { return [typeof this, a, b, c, arguments.length].join(); }
var b = foo.bind(7, 1);
print(b.name, b.length, b(2, 3), b.hasOwnProperty('prototype'), typeof b);
var d = Object.getOwnPropertyDescriptor(b, 'length');
print(d.writable, d.enumerable, d.configurable, Object.getOwnPropertyNames(b).join());
var c = b.bind(8, 2).bind(9);
print(c.name, c.length, c(5), foo.bind().bind().bind().name);
print(Object.getPrototypeOf(b) === Function.prototype, Object.prototype.toString.call(b), String(b));",
        r"bound foo 2 object,1,2,3,3 false function
false false true length,name
bound bound bound foo 1 object,1,2,5,3 bound bound bound foo
true [object Function] function () { [native code] }",
    )]);
}

#[test]
fn bound_construct() {
    check_cases(&[(
        r"function F(a, b) { this.a = a; this.b = b; }
F.prototype.p = 'proto';
var B = F.bind({ x: 1 }, 'p');
var o = new B('q');
print(o.a, o.b, o.p, o instanceof F, o instanceof B, Object.getPrototypeOf(o) === F.prototype, o.x);
var B2 = B.bind(null, 'r');
var o2 = new B2();
print(o2.a, o2.b, o2 instanceof B2);
function G() { return { z: 1 }; }
print(new (G.bind())().z);
try { new ((() => 1).bind())(); } catch (e) { print(e.constructor.name); }
try { new (Math.pow.bind())(); } catch (e) { print(e.constructor.name); }",
        r"p q proto true true true undefined
p r true
1
TypeError
TypeError",
    )]);
}

#[test]
fn bound_length_name() {
    check_cases(&[(
        r"var f = function () { };
Object.defineProperty(f, 'name', { value: 7 });
Object.defineProperty(f, 'length', { value: '3' });
print(f.bind().name, f.bind().length);
Object.defineProperty(f, 'length', { value: Infinity });
print(f.bind().length, f.bind(0, 1, 2).length);
Object.defineProperty(f, 'length', { value: -Infinity });
print(f.bind().length);
Object.defineProperty(f, 'length', { value: 2.7 });
print(f.bind().length, f.bind(0, 1).length, f.bind(0, 1, 2, 3).length);
Object.defineProperty(f, 'length', { value: -5 });
print(f.bind().length);
delete f.length;
print(f.bind().length, f.bind().name);
var g = function () { };
Object.setPrototypeOf(g, null);
var bg = Function.prototype.bind.call(g);
print(bg.name, Object.getPrototypeOf(bg));
try { Function.prototype.bind.call(1); } catch (e) { print(e.message); }",
        r"bound  0
Infinity Infinity
0
2 1 0
0
0 bound 
bound g null
Bind must be called on a function",
    )]);
}

#[test]
fn bound_console() {
    check_cases(&[(
        r"function foo() { }
var b = foo.bind();
console.log(b, [b], { b: b });
b.x = 1;
console.log(b, (function () { }).bind());",
        r"[Function: bound foo] [ [Function: bound foo] ] { b: [Function: bound foo] }
[Function: bound foo] { x: 1 } [Function: bound ]",
    )]);
}

#[test]
fn bound_calls() {
    check_cases(&[(
        r"function foo(a, b) { return [typeof this, a, b].join(); }
var b = foo.bind(null);
print(b.call(1, 2), b.apply(1, [3, 4]), Function.prototype.call.bind(foo)(undefined, 9));
var pow = Math.pow.bind(null, 2);
print(pow(10), pow.name, pow.length);
var s = (function () { 'use strict'; return this; });
print(s.bind(5)(), s.bind()(), s.bind(null)(), typeof (function () { return this; }).bind(5)());",
        r"object,2, object,3,4 object,9,
1024 bound pow 1
5 undefined null object",
    )]);
}

#[test]
fn bound_chain_deep() {
    check_cases(&[(
        r"var b = function () { return 'ok'; };
for (var i = 0; i < 4000; i++) b = b.bind();
print(b());
var c = function (a) { return [a, arguments.length].join(); };
for (var i = 0; i < 3000; i++) c = c.bind(null, i);
print(c.name.length, c.length, c());",
        r"ok
18001 0 0,3000",
    )]);
}

#[test]
fn reflect_construct_newtarget() {
    check_cases(&[(
        r"function F(a) { this.a = a; }
function G() { }
var o = Reflect.construct(F, [1], G);
print(o.a, Object.getPrototypeOf(o) === G.prototype, o instanceof F);
G.prototype = null;
print(Object.getPrototypeOf(Reflect.construct(F, [], G)) === Object.prototype);
var a = Reflect.construct(Array, [1, 2], Object);
print(Array.isArray(a), Object.getPrototypeOf(a) === Object.prototype, a.length);
print(Reflect.construct(Array, [3]).length);
var oo = Reflect.construct(Object, [], Array);
print(Object.getPrototypeOf(oo) === Array.prototype, Array.isArray(oo));
var e = Reflect.construct(Error, ['m'], TypeError);
print(e instanceof TypeError, e.message, Object.getPrototypeOf(e) === TypeError.prototype);
var s = Reflect.construct(String, ['ab'], Number);
print(Object.getPrototypeOf(s) === Number.prototype, s.length, Object.keys(s).join());
var nt = function () { }.bind();
print(Object.getPrototypeOf(Reflect.construct(F, [], nt)) === Object.prototype);
function H() { return 1; }
print(typeof Reflect.construct(H, []));",
        r"1 true false
true
true true 2
3
true false
true m true
true 2 0,1
true
object",
    )]);
}

#[test]
fn reflect_construct_errors() {
    check_cases(&[(
        r"function t(f) { try { f(); print('no error'); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { Reflect.construct(1, []); });
t(function () { Reflect.construct(function () { }, [], 1); });
t(function () { Reflect.construct(function () { }, 1); });
t(function () { Reflect.construct(() => { }, []); });
t(function () { Reflect.construct(function () { }, [], () => { }); });
t(function () { Reflect.apply(1); });
t(function () { Reflect.apply(undefined); });
t(function () { Reflect.apply(function () { }, null); });
t(function () { Reflect.apply(function () { }, null, 1); });
t(function () { Reflect.apply(function () { }, null, { length: 4294967295 }); });
t(function () { Reflect.apply(function () { }, null, { length: 9007199254740992 }); });
t(function () { Reflect.apply(function () { }, null, { length: 134217728 }); });
t(function () { Reflect.apply(function () { }, null, { length: -1 }); });
t(function () { Reflect.get(1, 'x'); });
t(function () { Reflect.ownKeys(undefined); });
t(function () { Reflect.setPrototypeOf({}, 1); });",
        r"TypeError: 1 is not a constructor
TypeError: 1 is not a constructor
TypeError: CreateListFromArrayLike called on non-object
TypeError: () => { } is not a constructor
TypeError: () => { } is not a constructor
TypeError: Function.prototype.apply was called on 1, which is a number and not a function
TypeError: Function.prototype.apply was called on undefined, which is a undefined and not a function
TypeError: CreateListFromArrayLike called on non-object
TypeError: CreateListFromArrayLike called on non-object
RangeError: Invalid array length
RangeError: Invalid array length
RangeError: Invalid array length
no error
TypeError: Reflect.get called on non-object
TypeError: Reflect.ownKeys called on non-object
TypeError: Object prototype may only be an Object or null: 1",
    )]);
}

#[test]
fn reflect_get_set() {
    check_cases(&[(
        r"var o = {};
Object.defineProperty(o, 'g', { get: function () { return this; }, configurable: true });
var r = {};
print(Reflect.get(o, 'g', r) === r, Reflect.get(o, 'g') === o, Reflect.get(o, 'g', 5) === 5);
var seen;
Object.defineProperty(o, 's', { set: function (v) { seen = [this === r, v].join(); }, configurable: true });
print(Reflect.set(o, 's', 1, r), seen, Reflect.set(o, 's', 2), seen);
var p = {};
print(Reflect.set(p, 'x', 1, r), p.x, r.x, Reflect.set(p, 'y', 1, 5), Reflect.set(p, 'z', 1, undefined));
var q = {};
Object.defineProperty(q, 'x', { value: 1 });
print(Reflect.set(q, 'x', 2, r), Reflect.set(r, 'x', 2, q));
var a = [];
print(Reflect.set(a, 'length', 3), a.length, Reflect.set(a, 'length', '1'), a.length, Reflect.set(a, 5, 1), a.length);
var arr = [1, 2, 3];
var rr = {};
print(Reflect.set(arr, 0, 'x', rr), arr[0], rr[0]);
var fr = Object.freeze({ a: 1 });
print(Reflect.set(fr, 'a', 2), Reflect.set(fr, 'b', 2), Reflect.defineProperty(fr, 'a', { value: 1 }), Reflect.defineProperty(fr, 'a', { value: 2 }), Reflect.deleteProperty(fr, 'a'), Reflect.isExtensible(fr), Reflect.preventExtensions(fr));
var k = { b: 1, a: 2, 1: 3, 0: 4 };
print(Reflect.ownKeys(k).join(), Reflect.has(k, 'a'), Reflect.has(k, 'toString'), Reflect.deleteProperty(k, 'a'), Reflect.has(k, 'a'));
try { Reflect.set([], 'length', -1); } catch (e) { print(e.constructor.name, e.message); }",
        r"true true false
true true,1 true false,2
true undefined 1 false false
false false
true 3 true 1 true 6
true 1 x
false false true false false false true
0,1,b,a true true true false
RangeError Invalid array length",
    )]);
}

#[test]
fn reflect_prototype() {
    check_cases(&[(
        r"var a = {}, b = Object.create(a);
print(Reflect.setPrototypeOf(a, b), Reflect.setPrototypeOf(a, a), Reflect.setPrototypeOf(Object.prototype, {}), Reflect.setPrototypeOf(Object.prototype, null));
var o = {};
print(Reflect.getPrototypeOf(o) === Object.prototype, Reflect.setPrototypeOf(o, null), Reflect.getPrototypeOf(o), Reflect.preventExtensions(o), Reflect.setPrototypeOf(o, {}), Reflect.setPrototypeOf(o, null));",
        r"false false false true
true true null true false true",
    )]);
}

#[test]
fn proto_accessor() {
    check_cases(&[(
        r"var a = {};
var b = { __proto__: a };
print(b.__proto__ === a, Object.getPrototypeOf(b) === a, Object.keys(b).join());
var c = {};
c.__proto__ = a;
print(c.__proto__ === a, Object.keys(c).join(), c.hasOwnProperty('__proto__'));
var n = Object.create(null);
n.__proto__ = {};
print(Object.keys(n).join(), Object.getPrototypeOf(n));
var d = {};
d.__proto__ = 5;
d.__proto__ = null;
print(Object.getPrototypeOf(d), d.__proto__);
var desc = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__');
print(desc.get.name, desc.set.name, desc.get.length, desc.set.length, desc.enumerable, desc.configurable, desc.get.call(1) === Number.prototype, desc.set.call(1, {}), desc.set.call({}, 1));
function t(f) { try { f(); print('no error'); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { desc.set.call(undefined, {}); });
t(function () { desc.get.call(undefined); });
t(function () { desc.set.call(Object.preventExtensions({}), {}); });
t(function () { Object.prototype.__proto__ = {}; });
t(function () { var x = {}; x.__proto__ = x; });
t(function () { var x = {}, y = Object.create(x); x.__proto__ = y; });",
        r"true true 
true  false
__proto__ null
null undefined
get __proto__ set __proto__ 0 1 false true true undefined undefined
TypeError: set Object.prototype.__proto__ called on null or undefined
TypeError: Cannot convert undefined or null to object
TypeError: #<Object> is not extensible
TypeError: Immutable prototype object 'Object.prototype' cannot have their prototype set
TypeError: Cyclic __proto__ value
TypeError: Cyclic __proto__ value",
    )]);
}

#[test]
fn define_lookup_accessors() {
    check_cases(&[(
        r"var o = {};
o.__defineGetter__('x', function () { return 7; });
o.__defineSetter__('x', function (v) { this.y = v; });
o.x = 4;
var d = Object.getOwnPropertyDescriptor(o, 'x');
print(o.x, o.y, d.enumerable, d.configurable, typeof d.get, typeof d.set, o.__lookupGetter__('x') === d.get, o.__lookupSetter__('x') === d.set, o.__lookupGetter__('y'), o.__lookupGetter__('nope'));
var p = {};
p.__defineGetter__('z', function () { return 1; });
var q = Object.create(p);
print(q.__lookupGetter__('z') === p.__lookupGetter__('z'), q.__lookupSetter__('z'));
function t(f) { try { f(); print('no error'); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { Object.freeze({}).__defineGetter__('x', function () { }); });
t(function () { ({}).__defineGetter__('x'); });
t(function () { ({}).__defineSetter__('x', 1); });
t(function () { Object.prototype.__lookupGetter__.call(undefined, 'x'); });",
        r"7 4 true true function function true true undefined undefined
true undefined
TypeError: Cannot define property x, object is not extensible
TypeError: Object.prototype.__defineGetter__: Expecting function
TypeError: Object.prototype.__defineSetter__: Expecting function
TypeError: Cannot convert undefined or null to object",
    )]);
}

#[test]
fn object_prototype_methods() {
    check_cases(&[(
        r"var o = { a: 1 };
print(o.hasOwnProperty('a'), o.hasOwnProperty('b'), o.hasOwnProperty('toString'), o.isPrototypeOf({}), Object.prototype.isPrototypeOf(o), Object.prototype.isPrototypeOf(1), o.propertyIsEnumerable('a'), Object.prototype.propertyIsEnumerable.call([1], 'length'), [1].propertyIsEnumerable(0));
var t = { toString: function () { return 'TS'; } };
print(t.toLocaleString(), Object.prototype.toLocaleString.call(5), Object.prototype.toLocaleString.call('s'));
function f(g) { try { g(); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
f(function () { Object.prototype.toLocaleString.call({ toString: 5 }); });
f(function () { Object.prototype.toLocaleString.call(undefined); });
f(function () { Object.prototype.hasOwnProperty.call(null, 'x'); });
f(function () { Object.prototype.isPrototypeOf.call(undefined, {}); });
print(Object.prototype.isPrototypeOf.call(undefined, 1));
print(Object.prototype.toString.call(null), Object.prototype.toString.call([]), Object.prototype.toString.call(function () { }), Object.prototype.toString.call(1), Object.prototype.toString.call('x'), Object.prototype.toString.call(new Error('x')), Object.prototype.toString.call((function () { return arguments; })()));",
        r"true false false false true false true false true
TS 5 s
TypeError: number 5 is not a function
TypeError: Object.prototype.toLocaleString called on null or undefined
TypeError: Cannot convert undefined or null to object
TypeError: Cannot convert undefined or null to object
false
[object Null] [object Array] [object Function] [object Number] [object String] [object Error] [object Arguments]",
    )]);
}

#[test]
fn object_constructor_newtarget() {
    check_cases(&[(
        r"function G() { }
var o = Reflect.construct(Object, [1], G);
print(typeof o, Object.getPrototypeOf(o) === G.prototype);
print(typeof new Object(1), typeof Object('a'), typeof Object(null), typeof new Object(true));
var x = {};
print(Object(x) === x, new Object(x) === x, Object.length, Object.name);",
        r"object true
object object object object
true true 1 Object",
    )]);
}

#[test]
fn object_assign_keys() {
    check_cases(&[(
        r"function show(a) { return a.join(); }
print(show(Object.keys(Object.assign({}, { a: 1 }, null, 'xy', [7, 8], undefined, 5))));
var t = Object.assign([1, 2, 3], [9]);
print(show(t), t.length);
try { Object.assign(Object.freeze({ a: 1 }), { a: 2 }); } catch (e) { print(e.message); }
var s = Object.assign(new String('ab'), { x: 1 });
print(s.x, Object.keys(s).join());
try { Object.assign(new String('ab'), 'xyz'); } catch (e) { print(e.constructor.name, e.message); }
var o = { b: 1, a: 2, 1: 3 };
print(show(Object.keys(o)), show(Object.values(o)), show(Object.entries(o).map(function (e) { return e.join('='); })));
print(show(Object.keys('ab')), show(Object.values('ab')), show(Object.entries([4, , 5]).map(function (e) { return e.join('='); })));
var g = { a: 1, b: 2 };
Object.defineProperty(g, 'a', { get: function () { delete g.b; return 5; }, enumerable: true });
print(show(Object.entries(g).map(function (e) { return e.join('='); })));",
        r"0,1,a
9,2,3 3
Cannot assign to read only property 'a' of object '#<Object>'
1 0,1,x
TypeError Cannot assign to read only property '0' of object '[object String]'
1,b,a 3,1,2 1=3,b=1,a=2
0,1 a,b 0=4,2=5
a=5",
    )]);
}

#[test]
fn object_define() {
    check_cases(&[(
        r"function show(o) { return Object.getOwnPropertyNames(o).map(function (k) { var d = Object.getOwnPropertyDescriptor(o, k); return k + ':' + ('value' in d ? d.value : 'acc') + (d.writable ? 'w' : '-') + (d.enumerable ? 'e' : '-') + (d.configurable ? 'c' : '-'); }).join(' '); }
var o = {};
Object.defineProperty(o, 'a', { value: 1 });
print(show(o));
var p = { a: 1 };
Object.defineProperty(p, 'a', { enumerable: false });
print(show(p));
var q = {};
Object.defineProperty(q, 'a', { get: function () { return 3; }, configurable: true });
Object.defineProperty(q, 'a', { value: 5 });
print(show(q));
var r = {};
Object.defineProperty(r, 'a', { value: NaN });
Object.defineProperty(r, 'a', { value: NaN });
try { Object.defineProperty(r, 'a', { value: 0 }); } catch (e) { print(e.message); }
var z = {};
Object.defineProperty(z, 'a', { value: 0 });
try { Object.defineProperty(z, 'a', { value: -0 }); } catch (e) { print(e.message); }
print(show(Object.defineProperties({}, { a: { value: 1 }, b: { value: 2, enumerable: true } })));
var w = {};
try { Object.defineProperties(w, { a: { value: 1 }, b: 7 }); } catch (e) { print(e.message, Object.getOwnPropertyNames(w).length); }
print(show(Object.create(null, { a: { value: 1, enumerable: true }, b: { value: 2 } })));
print(show(Object.getOwnPropertyDescriptors({ a: 1, 2: 'x' })));
function t(f) { try { f(); print('no error'); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { Object.defineProperty(1, 'x', {}); });
t(function () { Object.defineProperty({}, 'x', 1); });
t(function () { Object.defineProperty({}, 'x', { get: function () { }, value: 1 }); });
t(function () { Object.defineProperty({}, 'x', { get: 1 }); });
t(function () { Object.defineProperty({}, 'x', { set: 1 }); });
t(function () { Object.defineProperty(Object.preventExtensions({}), 'x', { value: 1 }); });
t(function () { Object.create(1); });
t(function () { Object.defineProperties({}, null); });",
        r"a:1---
a:1w-c
a:5--c
Cannot redefine property: a
Cannot redefine property: a
a:1--- b:2-e-
Property description must be an object: 7 0
a:1-e- b:2---
2:[object Object]wec a:[object Object]wec
TypeError: Object.defineProperty called on non-object
TypeError: Property description must be an object: 1
TypeError: Invalid property descriptor. Cannot both specify accessors and a value or writable attribute, #<Object>
TypeError: Getter must be a function: 1
TypeError: Setter must be a function: 1
TypeError: Cannot define property x, object is not extensible
TypeError: Object prototype may only be an Object or null: 1
TypeError: Cannot convert undefined or null to object",
    )]);
}

#[test]
fn object_descriptor_order() {
    check_cases(&[(
        r"var d = Object.getOwnPropertyDescriptor({ a: 1 }, 'a');
print(Object.keys(d).join());
var acc = {};
Object.defineProperty(acc, 'x', { get: function () { }, configurable: true });
print(Object.keys(Object.getOwnPropertyDescriptor(acc, 'x')).join());
var order = [];
var desc = {};
['enumerable', 'configurable', 'value', 'writable', 'get', 'set'].forEach(function (k) {
  Object.defineProperty(desc, k, { get: function () { order.push(k); return undefined; }, enumerable: true });
});
try { Object.defineProperty({}, 'x', desc); } catch (e) { }
print(order.join());
print(Object.getOwnPropertyDescriptor('abc', 1).value, Object.getOwnPropertyDescriptor('abc', 'length').value, Object.getOwnPropertyDescriptor('abc', 3));",
        r"value,writable,enumerable,configurable
get,set,enumerable,configurable
enumerable,configurable,value,writable,get,set
b 3 undefined",
    )]);
}

#[test]
fn object_prototype_ops() {
    check_cases(&[(
        r"var a = {}, b = Object.create(a);
print(Object.getPrototypeOf(b) === a, Object.setPrototypeOf(a, null) === a, Object.getPrototypeOf(a));
print(Object.getPrototypeOf(1) === Number.prototype, Object.getPrototypeOf('a') === String.prototype, Object.setPrototypeOf(1, null));
var o = { x: 1 }, q = { y: 2 };
Object.setPrototypeOf(o, q);
print(o.y, o.x, Object.keys(o).join());
var f = Object.preventExtensions({});
print(Object.setPrototypeOf(f, Object.prototype) === f);
function t(g) { try { g(); print('no error'); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { Object.setPrototypeOf(f, {}); });
t(function () { Object.setPrototypeOf(undefined, {}); });
t(function () { Object.setPrototypeOf({}, undefined); });
t(function () { Object.setPrototypeOf(Object.prototype, {}); });
t(function () { var x = {}, y = Object.create(x); Object.setPrototypeOf(x, y); });
t(function () { Object.getPrototypeOf(null); });
print(Object.is(NaN, NaN), Object.is(0, -0), Object.is(-0, -0), Object.is('a', 'a'), Object.is({}, {}), Object.is(), Object.is(1));
print(Object.hasOwn({ a: 1 }, 'a'), Object.hasOwn({}, 'toString'), Object.hasOwn('abc', 1), Object.hasOwn('abc', 3), Object.hasOwn([1], 'length'));",
        r"true true null
true true 1
2 1 x
true
TypeError: #<Object> is not extensible
TypeError: Object.setPrototypeOf called on null or undefined
TypeError: Object prototype may only be an Object or null: undefined
TypeError: Immutable prototype object 'Object.prototype' cannot have their prototype set
TypeError: Cyclic __proto__ value
TypeError: Cannot convert undefined or null to object
true false true true false true false
true false true false true",
    )]);
}

#[test]
fn integrity_objects() {
    check_cases(&[(
        r"var o = Object.freeze({ a: 1, b: { c: 1 } });
print(Object.isFrozen(o), Object.isSealed(o), Object.isExtensible(o), Object.isFrozen(o.b));
var s = Object.seal({ a: 1 });
s.a = 2; s.b = 3; delete s.a;
print(Object.isFrozen(s), Object.isSealed(s), s.a, s.b);
var p = Object.preventExtensions({ a: 1 });
print(Object.isFrozen(p), Object.isSealed(p), Object.isExtensible(p));
print(Object.isFrozen(Object.preventExtensions({})), Object.isSealed(Object.preventExtensions({})), Object.isFrozen(1), Object.isSealed('x'), Object.isExtensible(1), Object.freeze(1), Object.seal('a'), Object.preventExtensions(null));
var fs = Object.freeze(new String('ab'));
print(Object.isFrozen(fs), Object.isFrozen(Object.freeze(function () { })));
var acc = {};
Object.defineProperty(acc, 'x', { get: function () { return 1; }, configurable: true });
Object.freeze(acc);
var d = Object.getOwnPropertyDescriptor(acc, 'x');
print(d.configurable, d.writable, Object.isFrozen(acc));",
        r"true true false false
false true 2 undefined
false false false
true true true true false 1 a null
true true
false undefined true",
    )]);
}

#[test]
fn integrity_arrays() {
    check_cases(&[(
        r"var a = Object.freeze([1, 2, 3]);
a[0] = 9; a[5] = 1;
print(a.join(), a.length, Object.isFrozen(a), JSON_free_length(a));
function JSON_free_length(x) { var d = Object.getOwnPropertyDescriptor(x, 'length'); return [d.writable, d.enumerable, d.configurable].join(); }
var sp = [];
sp[100000] = 1; sp[3] = 2;
Object.freeze(sp);
print(Object.isFrozen(sp), sp.length, Object.keys(sp).join());
var se = [1, 2, 3];
Object.seal(se);
se.length = 1;
se[0] = 7;
print(se.join(), se.length, Object.isSealed(se), Object.isFrozen(se));
var big = Object.defineProperty([], 'length', { value: 4294967295 });
Object.freeze(big);
print(big.length, Object.isFrozen(big));
var hi = [];
hi[4294967294] = 1;
Object.freeze(hi);
print(hi.length, Object.isFrozen(hi), Object.keys(hi).join());
var h2 = Object.defineProperty([], 'length', { value: 4294967295 });
h2[4294967294] = 1;
print(h2.length, Object.keys(h2).join());",
        r"1,2,3 3 true false,false,false
true 100001 3,100000
7,2,3 3 true false
4294967295 true
4294967295 true 4294967294
4294967295 4294967294",
    )]);
}

#[test]
fn array_length_define() {
    check_cases(&[(
        r"function t(f) { try { f(); print('no error'); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
var a = [1, 2, 3];
Object.defineProperty(a, 'length', { value: 1 });
print(a.join(), a.length);
a = [1, 2, 3];
Object.defineProperty(a, 'length', { value: '2' });
print(a.join());
a = [1, 2, 3];
Object.defineProperty(a, 'length', { value: { valueOf: function () { return 1; } } });
print(a.join());
a = [1, 2, 3];
Object.defineProperty(a, 1, { configurable: false });
t(function () { Object.defineProperty(a, 'length', { value: 0 }); });
print(a.length);
t(function () { Object.defineProperty(a, 'length', { value: 0, writable: false }); });
print(a.length, Object.getOwnPropertyDescriptor(a, 'length').writable);
var b = [1, 2, 3];
Reflect.defineProperty(b, 'length', { writable: false });
print(Reflect.defineProperty(b, 'length', { value: 5 }), Reflect.defineProperty(b, 'length', { value: 3 }), Reflect.defineProperty(b, 3, { value: 1 }));
t(function () { Object.defineProperty([], 'length', { value: -1 }); });
t(function () { Object.defineProperty([], 'length', { value: 1.5 }); });
t(function () { Object.defineProperty([], 'length', { value: 4294967296 }); });
t(function () { Object.defineProperty([], 'length', { enumerable: true }); });
t(function () { Object.defineProperty([1], 'length', { get: function () { } }); });
print(Object.defineProperty([], 'length', { value: 4294967295 }).length);
var c = [1, 2];
c.length = '1';
print(c.join(), c.length);
var d = [1, 2];
Object.defineProperty(d, 5, { value: 1 });
print(d.length);",
        r"1 1
1,2
1
TypeError: Cannot delete property '1' of [object Array]
2
TypeError: Cannot delete property '1' of [object Array]
2 false
false true false
RangeError: Invalid array length
RangeError: Invalid array length
RangeError: Invalid array length
TypeError: Cannot redefine property: length
TypeError: Cannot redefine property: length
4294967295
1 1
6",
    )]);
}

#[test]
fn string_objects() {
    check_cases(&[(
        r"var s = new String('abc');
print(Object.keys(s).join(), delete s[0], delete s[3], s.hasOwnProperty(2), s.hasOwnProperty(3), s.propertyIsEnumerable(1), s.propertyIsEnumerable('length'));
s[1] = 'z'; s[3] = 'd';
print(s[1], s[3], s.length, Object.getOwnPropertyNames(s).join());
print(Reflect.defineProperty(s, 0, { value: 'a' }), Reflect.defineProperty(s, 0, { value: 'b' }), Reflect.defineProperty(s, 0, { writable: true }), Reflect.defineProperty(s, 0, { enumerable: true }), Reflect.defineProperty(s, 4, { value: 'd' }), Reflect.defineProperty(s, 'length', { value: 3 }), Reflect.defineProperty(s, 'length', { value: 4 }));
var t = new String('abc');
t.x = 1; t[7] = 2; t[5] = 3;
print(Object.getOwnPropertyNames(t).join());
var u = new String('abc');
print(Reflect.ownKeys(u).join(), Reflect.has(u, 1), Reflect.has(u, 5), Reflect.get(u, 1), Reflect.set(u, 1, 'x'), Reflect.set(u, 4, 'x'), Reflect.deleteProperty(u, 1), Reflect.deleteProperty(u, 4));
var d = Object.getOwnPropertyDescriptor(Object('abc'), 0);
print(d.value, d.writable, d.enumerable, d.configurable);
var o = Object.create(new String('abc'));
print(Reflect.has(o, 1), Reflect.has(o, 3), o[1], o[3], Reflect.get(o, 2), Reflect.get(o, 2, {}));
(function () { 'use strict'; var v = new String('ab'); try { v[1] = 'x'; } catch (e) { print(e.message); } try { delete v[1]; } catch (e) { print(e.message); } try { Object.defineProperty(v, 1, { value: 'x' }); } catch (e) { print(e.message); } })();",
        r"0,1,2 false true true false true false
b d 3 0,1,2,3,length
true false false true true true false
0,1,2,5,7,length,x
0,1,2,length true false b false true false true
a false true false
true false b undefined c c
Cannot assign to read only property '1' of object '[object String]'
Cannot delete property '1' of [object String]
Cannot redefine property: 1",
    )]);
}

#[test]
fn globals() {
    check_cases(&[(
        r"print(isNaN(NaN), isNaN('x'), isNaN('1'), isNaN(undefined), isNaN(null), isNaN({}), isNaN([]), isNaN(), isFinite(1), isFinite(Infinity), isFinite('1'), isFinite(NaN), isFinite(-Infinity), isFinite(null), isFinite());
print(isNaN.name, isNaN.length, isFinite.name, isFinite.length, Math.pow(2, 10), Math.pow(2, 0.5), Math.pow(NaN, 0), Math.pow(1, Infinity), Math.pow(-8, 1 / 3), Math.pow(0, -1), Math.pow(-0, -3), Math.pow('2', '3'), Math.pow(2), Math.pow.length, Math.pow.name);
try { isNaN({ valueOf: function () { throw new RangeError('v'); } }); } catch (e) { print(e.message); }
var d = Object.getOwnPropertyDescriptor(globalThis, 'isNaN');
var m = Object.getOwnPropertyDescriptor(globalThis, 'Math');
print(d.writable, d.enumerable, d.configurable, m.writable, m.enumerable, m.configurable, Object.getPrototypeOf(Math) === Object.prototype);",
        r"true true false true false true false true true false true false false true false
isNaN 1 isFinite 1 1024 1.4142135623730951 1 NaN NaN Infinity -Infinity 8 NaN 2 pow
v
true false true true false true true",
    )]);
}

#[test]
fn own_undefined_hides_inherited_string_characters() {
    // Also the messages of two strict writes that fail on the way.
    check_cases(&[(
        r"var p = Object.create(new String('xyz'));
Object.defineProperty(p, '1', {value: undefined});
print(p[1], p[0], Reflect.get(p, 1), Object.keys(p).join());
var mid = Object.create(new String('xyz'));
Object.defineProperty(mid, '2', {value: undefined});
var q = Object.create(mid);
print(q[2], q[1], mid[2]);
var r = Object.create(Object.create(new String('xyz')));
print(r[1], r[5]);
(function () {
  'use strict';
  var a = []; a.length = 4294967291; Object.defineProperty(a, 4294967290, { value: 1 });
  try { a.length = 0; } catch (e) { print(e.message); }
  var b = [1, 2, 3]; Object.defineProperty(b, 1, { value: 1, configurable: false });
  try { b.length = 0; } catch (e) { print(e.message, b.length); }
  var c = Object.create(new String('xyz'));
  try { c[1] = 'q'; } catch (e) { print(e.message); }
  var d = Object.create(c);
  try { d[0] = 'q'; } catch (e) { print(e.message); }
})();",
        r"undefined x undefined 
undefined y undefined
y undefined
Cannot delete property '4294967290' of [object Array]
Cannot delete property '1' of [object Array] 2
Cannot assign to read only property '1' of object '[object Object]'
Cannot assign to read only property '0' of object '[object Object]'",
    )]);
}
