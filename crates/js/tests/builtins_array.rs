//! Cases for `Array[@@species]`, `ArraySpeciesCreate`, `Array.prototype[@@unscopables]`,
//! `Array.prototype.indexOf`, `slice` and `map` (M7 feature 2c).
//! The expected output of each case is the output of Node.js 22 for the same
//! script (`print` joins `String` of its arguments). Each case also runs in
//! the GC stress mode.
//!
//! Generated from a list of scripts and Node's output.

#![allow(clippy::too_many_lines, reason = "a test is a list of script cases")]

mod common;

use common::check_cases;

#[test]
fn array_species_getter_and_unscopables() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { var d = Object.getOwnPropertyDescriptor(Array, Symbol.species); return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length].join(); });
t(function () { return Array[Symbol.species] === Array; });
t(function () { var d = Object.getOwnPropertyDescriptor(Array, Symbol.species); var o = {}; return d.get.call(o) === o && d.get.call(5) === 5 && d.get.call(undefined) === undefined; });
t(function () { function C() {} return Object.getOwnPropertyDescriptor(Array, Symbol.species).get.call(C) === C; });
t(function () { function C() {} Object.setPrototypeOf(C, Array); return C[Symbol.species] === C; });
t(function () { var d = Object.getOwnPropertyDescriptor(Array.prototype, Symbol.unscopables); return [d.writable, d.enumerable, d.configurable, Object.getPrototypeOf(d.value)].join(); });
t(function () { var u = Array.prototype[Symbol.unscopables]; return Object.keys(u).join(); });
t(function () { var u = Array.prototype[Symbol.unscopables]; var ok = true; Object.keys(u).forEach(function (k) { var d = Object.getOwnPropertyDescriptor(u, k); ok = ok && d.value === true && d.writable && d.enumerable && d.configurable; }); return ok; });
",
        r"function,,false,true,get [Symbol.species],0
true
true
true
true
false,false,true,
at,copyWithin,entries,fill,find,findIndex,findLast,findLastIndex,flat,flatMap,includes,keys,toReversed,toSorted,toSpliced,values
true",
    )]);
}

#[test]
fn array_species_create() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { var a = [1, 2, 3]; var r = a.slice(); return (r !== a) + ',' + Array.isArray(r) + ',' + r.join(); });
t(function () { var a = [1, 2, 3]; a.constructor = undefined; var r = a.slice(1); return Array.isArray(r) + ',' + r.join(); });
t(function () { var a = [1, 2, 3]; a.constructor = null; return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = 5; return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; return Array.isArray(a.slice()) + ',' + a.slice().length; });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = null; return Array.isArray(a.slice()); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = undefined; return Array.isArray(a.slice()); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = 1; return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = {}; return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = function () {}; return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = () => 1; return a.slice(); });
t(function () { var args; var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { args = arguments.length + ':' + n; return {}; }; var r = a.slice(1); return args + ',' + Object.keys(r).join() + ',' + r.length; });
t(function () { var a = [1, 2, 3, 4]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { this.made = n; }; var r = a.slice(1, 3); return r.made + '|' + Object.keys(r).join() + '|' + r[0] + r[1] + '|' + r.length; });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { return [9, 9, 9, 9, 9]; }; var r = a.slice(); return r.length + ',' + r.join(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { return Object.freeze([]); }; return a.slice(); });
t(function () { var a = [1, 2, 3]; var log = []; Object.defineProperty(a, 'constructor', { get: function () { log.push('constructor'); return undefined; } }); a.slice(); a.map(function (x) { return x; }); return log.join(); });
t(function () { var a = [1, 2, 3]; Object.defineProperty(a, 'constructor', { get: function () { throw new RangeError('ctor getter'); } }); return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; Object.defineProperty(a.constructor, Symbol.species, { get: function () { throw new RangeError('species getter'); } }); return a.map(function (x) { return x; }); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { this.length = 0; }; var r = a.map(function (x) { return x * 2; }); return r.length + ',' + r[0] + r[1] + r[2]; });
t(function () { function Sub() { var a = Array.apply(null, arguments); Object.setPrototypeOf(a, Sub.prototype); return a; } Sub.prototype = Object.create(Array.prototype); Sub.prototype.constructor = Sub; var s = new Sub(); s.push(1, 2, 3); var r = s.slice(1); return (r instanceof Sub) + ',' + r.join() + ',' + Array.isArray(r); });
t(function () { function Sub() { var a = Array.apply(null, arguments); Object.setPrototypeOf(a, Sub.prototype); return a; } Sub.prototype = Object.create(Array.prototype); Sub.prototype.constructor = Sub; Sub[Symbol.species] = Array; var s = new Sub(); s.push(1, 2, 3); var r = s.map(function (x) { return x; }); return (r instanceof Sub) + ',' + r.join(); });
t(function () { var o = { length: 2, 0: 'a', 1: 'b', constructor: Array }; var r = Array.prototype.slice.call(o); return Array.isArray(r) + ',' + r.join(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = Array; var r = a.map(function (x) { return x + 1; }); return r.join(); });
t(function () { var a = [1, 2, 3]; a.constructor = Object; return Array.isArray(a.slice()) + ',' + a.slice().length; });
t(function () { var a = [1, 2, 3]; var log = []; a.constructor = {}; a.constructor[Symbol.species] = function (n) { log.push(n); return []; }; a.slice(); a.slice(1); a.slice(-1); a.slice(0, 0); a.slice(5); a.map(function (x) { return x; }); return log.join(); });
",
        r"true,true,1,2,3
true,2,3
TypeError: object.constructor[Symbol.species] is not a constructor
TypeError: object.constructor[Symbol.species] is not a constructor
true,3
true
true
TypeError: object.constructor[Symbol.species] is not a constructor
TypeError: object.constructor[Symbol.species] is not a constructor
[object Object]
TypeError: object.constructor[Symbol.species] is not a constructor
1:2,0,1,length,2
2|0,1,made,length|23|2
3,1,2,3
TypeError: Cannot define property 0, object is not extensible
constructor,constructor
RangeError: ctor getter
RangeError: species getter
0,246
false,2,3,true
false,1,2,3
true,a,b
2,3,4
true,3
3,2,1,0,0,3",
    )]);
}

#[test]
fn array_index_of() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
var ind = Array.prototype.indexOf;
t(function () { return Array.prototype.indexOf.length + ',' + Array.prototype.indexOf.name; });
t(function () { return [1, 2, 3, 2].indexOf(2) + ',' + [1, 2, 3, 2].indexOf(2, 2) + ',' + [1, 2, 3].indexOf(4) + ',' + [].indexOf(1); });
t(function () { return [1, 2, 3, 2].indexOf(2, -1) + ',' + [1, 2, 3, 2].indexOf(2, -2) + ',' + [1, 2, 3, 2].indexOf(1, -10) + ',' + [1, 2, 3, 2].indexOf(2, 10) + ',' + [1, 2, 3].indexOf(3, -1); });
t(function () { return [1, 2, 3].indexOf(1, Infinity) + ',' + [1, 2, 3].indexOf(1, -Infinity) + ',' + [1, 2, 3].indexOf(1, NaN) + ',' + [1, 2, 3].indexOf(2, '1') + ',' + [1, 2, 3].indexOf(2, 1.9) + ',' + [1, 2, 3].indexOf(2, null) + ',' + [1, 2, 3].indexOf(2, {}); });
t(function () { return [NaN].indexOf(NaN) + ',' + [0].indexOf(-0) + ',' + [-0].indexOf(0) + ',' + ['1'].indexOf(1) + ',' + [1].indexOf('1') + ',' + [null].indexOf(undefined) + ',' + [undefined].indexOf(undefined) + ',' + [, 1].indexOf(undefined); });
t(function () { var o = {}; var s = Symbol(); return [o].indexOf(o) + ',' + [{}].indexOf({}) + ',' + [s].indexOf(s) + ',' + [true].indexOf(true) + ',' + ['a'].indexOf('a'); });
t(function () { return [, 1, , 3].indexOf(undefined) + ',' + [, 1, , 3].indexOf(3); });
t(function () { var a = [1, 2, 3]; a.length = 1; return a.indexOf(2) + ',' + a.indexOf(1); });
t(function () { return ind.call({ length: 3, 0: 'a', 1: 'b', 2: 'b' }, 'b') + ',' + ind.call({ length: 2, 0: 'a', 1: 'b', 2: 'b' }, 'b', 2) + ',' + ind.call({}, 1) + ',' + ind.call({ length: -5, 0: 1 }, 1); });
t(function () { return ind.call('abcb', 'b') + ',' + ind.call('abcb', 'b', 2) + ',' + ind.call(new String('xyz'), 'z'); });
t(function () { return ind.call(undefined, 1); });
t(function () { return ind.call(null, 1); });
t(function () { return ind.call(5, 1) + ',' + ind.call(true, 1); });
t(function () { var log = []; var o = {}; Object.defineProperty(o, 'length', { get: function () { log.push('length'); return 2; } }); Object.defineProperty(o, '0', { get: function () { log.push('get0'); return 'a'; } }); Object.defineProperty(o, '1', { get: function () { log.push('get1'); return 'b'; } }); var from = { valueOf: function () { log.push('from'); return 0; } }; var r = ind.call(o, 'b', from); return r + ',' + log.join(); });
t(function () { var log = []; var o = { length: 0 }; var from = { valueOf: function () { log.push('from'); return 0; } }; ind.call(o, 1, from); return log.length; });
t(function () { return ind.call({ length: Symbol() }, 1); });
t(function () { return ind.call({ length: 1, 0: 1 }, 1, Symbol()); });
t(function () { return ind.call({ length: { valueOf: function () { throw new RangeError('lenvalueOf'); } } }, 1); });
t(function () { var a = [0, 1, 2, 3]; var n = 0; return a.indexOf(3, { valueOf: function () { a.length = 2; return 0; } }); });
t(function () { var a = [1, 2, 3]; var proto = { 5: 'p' }; var o = Object.create(proto); o.length = 6; return ind.call(o, 'p') + ',' + ind.call(o, 'q'); });
t(function () { var big = 9007199254740991; var o = { length: big }; o[big - 1] = 'end'; o[big - 3] = 'mid'; return ind.call(o, 'end', big - 3) + ',' + ind.call(o, 'mid', big - 4) + ',' + ind.call(o, 'end', -2) + ',' + ind.call(o, 'zzz', big - 2) + ',' + ind.call(o, 'zzz', -1); });
t(function () { var o = { length: 4294967296 + 3 }; o[4294967296 + 1] = 'x'; return ind.call(o, 'x', 4294967296); });
t(function () { var o = { length: 1e300, 0: 'a' }; return ind.call(o, 'a', 0); });
t(function () { var o = { length: 3, 0: 1, 1: 2, 2: 3 }; return ind.call(o, 3, -0) + ',' + ind.call(o, 1, +0) + ',' + ind.call(o, 3, -3) + ',' + ind.call(o, 3, -4); });
",
        r"1,indexOf
1,3,-1,-1
3,3,0,-1,2
-1,0,0,1,1,1,1
-1,0,0,-1,-1,-1,0,-1
0,-1,0,0,0
-1,3
-1,0
1,-1,-1,-1
1,3,2
TypeError: Array.prototype.indexOf called on null or undefined
TypeError: Array.prototype.indexOf called on null or undefined
-1,-1
1,length,from,get0,get1
0
TypeError: Cannot convert a Symbol value to a number
TypeError: Cannot convert a Symbol value to a number
RangeError: lenvalueOf
-1
5,-1
9007199254740990,9007199254740988,9007199254740990,-1,-1
4294967297
0
2,0,2,2",
    )]);
}

#[test]
fn array_slice() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
var sl = Array.prototype.slice;
t(function () { return Array.prototype.slice.length + ',' + Array.prototype.slice.name; });
t(function () { var a = [0, 1, 2, 3, 4]; return [a.slice(), a.slice(1), a.slice(1, 3), a.slice(-2), a.slice(-3, -1), a.slice(2, -1), a.slice(3, 1), a.slice(10), a.slice(0, 10), a.slice(-10, 2)].map(function (x) { return '[' + x.join() + ']'; }).join(''); });
t(function () { var a = [0, 1, 2, 3, 4]; return [a.slice(Infinity), a.slice(-Infinity), a.slice(0, Infinity), a.slice(0, -Infinity), a.slice(NaN), a.slice(0, NaN), a.slice('1', '3'), a.slice(1.9, 3.9), a.slice(null, undefined), a.slice(undefined, 2), a.slice({}, 2), a.slice(true, 3)].map(function (x) { return '[' + x.join() + ']'; }).join(''); });
t(function () { var a = [0, 1, 2]; var r = a.slice(); r[0] = 9; return a.join() + '|' + r.join(); });
t(function () { var a = [1, , 3]; var r = a.slice(); return r.length + ',' + (1 in r) + ',' + r.hasOwnProperty(1); });
t(function () { var a = [1, , 3, , ]; var r = a.slice(1); return r.length + ',' + (0 in r) + ',' + (1 in r) + ',' + (2 in r); });
t(function () { var a = []; a[5] = 'x'; var r = a.slice(2); return r.length + ',' + Object.keys(r).join(); });
t(function () { return sl.call({ length: 3, 0: 'a', 1: 'b', 2: 'c' }, 1).join() + '|' + sl.call({ length: 2, 0: 'a', 1: 'b' }).length + '|' + sl.call({}).length + '|' + Array.isArray(sl.call({})); });
t(function () { return sl.call('abc', 1).join() + '|' + sl.call(new String('hello'), -3, -1).join(''); });
t(function () { return sl.call(5).length + ',' + sl.call(true).length; });
t(function () { return sl.call(undefined); });
t(function () { return sl.call(null); });
t(function () { var log = []; var o = {}; Object.defineProperty(o, 'length', { get: function () { log.push('length'); return 3; } }); [0, 1, 2].forEach(function (i) { Object.defineProperty(o, i, { get: function () { log.push('get' + i); return i; } }); }); var s = { valueOf: function () { log.push('start'); return 1; } }; var e = { valueOf: function () { log.push('end'); return 3; } }; var r = sl.call(o, s, e); return r.join() + '|' + log.join(); });
t(function () { return sl.call({ length: Symbol() }); });
t(function () { return [1].slice(Symbol()); });
t(function () { return [1].slice(0, Symbol()); });
t(function () { var proto = { 1: 'p' }; var o = Object.create(proto); o.length = 3; o[0] = 'a'; var r = sl.call(o); return r.length + ',' + r[1] + ',' + r.hasOwnProperty(1); });
t(function () { var a = [1, 2, 3, 4]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { var o = {}; Object.defineProperty(o, 'length', { value: 0, writable: false }); return o; }; return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { var o = []; Object.defineProperty(o, 0, { value: 'fixed', writable: false, configurable: false }); return o; }; return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { return Object.preventExtensions([]); }; return a.slice(); });
t(function () { var a = [1, 2, 3]; a.constructor = {}; a.constructor[Symbol.species] = function (n) { return Object.preventExtensions([]); }; return a.slice(0, 0).length; });
t(function () { var o = { length: 4294967296 }; return sl.call(o); });
t(function () { var o = { length: 4294967295 }; return sl.call(o, 4294967290).length; });
t(function () { var o = { length: 4294967296 + 5 }; o[4294967296 + 2] = 'x'; return sl.call(o, 4294967296 + 1, 4294967296 + 4).length; });
t(function () { var o = { length: 9007199254740991 }; return sl.call(o, 9007199254740989).length; });
t(function () { var a = [0, 1, 2, 3]; var n = 0; return a.slice({ valueOf: function () { a.length = 1; return 0; } }).length + ',' + a.length; });
t(function () { var a = [1, 2, 3]; var r = a.slice(); Object.defineProperty(r, 'length', { writable: false }); return r.length + ',' + Object.isFrozen(r); });
t(function () { return Array.prototype.slice.call([1, 2, 3], 1).constructor === Array; });
t(function () { var args = (function () { return arguments; })(1, 2, 3); var r = sl.call(args, 1); return Array.isArray(r) + ',' + r.join(); });
",
        r"2,slice
[0,1,2,3,4][1,2,3,4][1,2][3,4][2,3][2,3][][][0,1,2,3,4][0,1]
[][0,1,2,3,4][0,1,2,3,4][][0,1,2,3,4][][1,2][1,2][0,1,2,3,4][0,1][0,1][1,2]
0,1,2|9,1,2
3,false,false
3,false,true,false
4,3
b,c|2|0|true
b,c|ll
0,0
TypeError: Cannot convert undefined or null to object
TypeError: Cannot convert undefined or null to object
1,2|length,start,end,get1,get2
TypeError: Cannot convert a Symbol value to a number
TypeError: Cannot convert a Symbol value to a number
TypeError: Cannot convert a Symbol value to a number
3,p,true
TypeError: Cannot assign to read only property 'length' of object '#<Object>'
TypeError: Cannot redefine property: 0
TypeError: Cannot define property 0, object is not extensible
0
RangeError: Invalid array length
5
3
2
4,1
3,false
true
true,2,3",
    )]);
}

/// `map` and `slice` report a refused `CreateDataPropertyOrThrow` alike.
#[test]
fn species_result_refuses_property() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
function sp(make) { var a = [1, 2, 3]; a.constructor = { [Symbol.species]: make }; return a; }
t(function () { return sp(function () { return Object.freeze({}); }).map(function (x) { return x; }); });
t(function () { return sp(function () { return Object.freeze({}); }).slice(); });
t(function () { return sp(function () { var o = {}; Object.defineProperty(o, '0', { value: 1 }); return o; }).map(function (x) { return x; }); });
t(function () { return sp(function () { var o = {}; Object.defineProperty(o, '0', { value: 1 }); return o; }).slice(); });
",
        r"TypeError: Cannot define property 0, object is not extensible
TypeError: Cannot define property 0, object is not extensible
TypeError: Cannot redefine property: 0
TypeError: Cannot redefine property: 0",
    )]);
}
