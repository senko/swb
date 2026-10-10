//! Cases for `Function.prototype.toString`, the restricted properties of
//! `Function.prototype` and the functions without own `caller` (M7 feature 2c).
//! The expected output of each case is the output of Node.js 22 for the same
//! script (`print` joins `String` of its arguments). Each case also runs in
//! the GC stress mode.
//!
//! Generated from a list of scripts and Node's output.

#![allow(clippy::too_many_lines, reason = "a test is a list of script cases")]

mod common;

use common::check_cases;

#[test]
fn function_to_string_script_functions() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
function decl ( a , b ) { return a /* c */ + b; }
var expr = function  named ( ) { };
var anon = function(){};
var arrow = ( a ) =>   a * 2;
var arrow2 = x =>
  x;
var o = {
  m ( ) { },
  [ 'comp' + 'uted' ] ( ) { },
  * gen ( ) { },
  'quoted key' ( ) { },
  42 ( ) { },
  f : function ( ) { },
  a : ( ) => 1,
  [ 1 + 1 ] ( ) { return 2; },
};
function* g ( ) { yield 1; }
var ge = function * ( ) { };
t(function () { return decl.toString(); });
t(function () { return expr.toString(); });
t(function () { return anon.toString(); });
t(function () { return arrow.toString(); });
t(function () { return arrow2.toString(); });
t(function () { return o.m.toString(); });
t(function () { return o.computed.toString(); });
t(function () { return o.gen.toString(); });
t(function () { return o['quoted key'].toString(); });
t(function () { return o[42].toString(); });
t(function () { return o.f.toString(); });
t(function () { return o.a.toString(); });
t(function () { return o[2].toString(); });
t(function () { return g.toString(); });
t(function () { return ge.toString(); });
t(function () { return String(decl) === decl.toString() && ('' + anon) === anon.toString(); });
t(function () { return Function.prototype.toString.call(decl) === decl.toString(); });
t(function () { return (function /* c1 */ f /* c2 */ ( /* c3 */ ) /* c4 */ { /* c5 */ }).toString(); });
t(function () { return (function () { return 'éé😀'; }).toString(); });
t(function () { return (function () {
  // line comment
  return 1;
}).toString(); });
t(function () { return decl.bind(null).toString(); });
t(function () { return (function () {}).bind().toString(); });
t(function () { return decl.bind(null).bind(null).toString(); });
",
        r"function decl ( a , b ) { return a /* c */ + b; }
function  named ( ) { }
function(){}
( a ) =>   a * 2
x =>
  x
m ( ) { }
[ 'comp' + 'uted' ] ( ) { }
* gen ( ) { }
'quoted key' ( ) { }
42 ( ) { }
function ( ) { }
( ) => 1
[ 1 + 1 ] ( ) { return 2; }
function* g ( ) { yield 1; }
function * ( ) { }
true
true
function /* c1 */ f /* c2 */ ( /* c3 */ ) /* c4 */ { /* c5 */ }
function () { return 'éé😀'; }
function () {
  // line comment
  return 1;
}
function () { [native code] }
function () { [native code] }
function () { [native code] }",
    )]);
}

#[test]
fn function_to_string_natives() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { return Math.pow.toString(); });
t(function () { return Function.prototype.toString(); });
t(function () { return Function.prototype.bind.toString(); });
t(function () { return Function.prototype.toString.toString(); });
t(function () { return Object.toString(); });
t(function () { return Error.toString() + '|' + Error.prototype.toString.toString() + '|' + TypeError.toString() + '|' + AggregateError.toString(); });
t(function () { return Array.prototype.indexOf.toString() + '|' + Array.prototype.slice.toString() + '|' + Array.isArray.toString(); });
t(function () { return Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get.toString(); });
t(function () { return Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set.toString(); });
t(function () { return Symbol.prototype[Symbol.toPrimitive].toString(); });
t(function () { return Array.prototype[Symbol.iterator].toString(); });
t(function () { return Function.prototype[Symbol.hasInstance].toString(); });
t(function () { return Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get.toString(); });
t(function () { return Object.getOwnPropertyDescriptor(Array, Symbol.species).get.toString(); });
t(function () { return Function.prototype.toString.call(Object.keys); });
t(function () { return Function.prototype.toString.call(Symbol); });
t(function () { return Function.prototype.toString.call(Reflect.apply); });
t(function () { return Function.prototype.toString.call(function* () {}.bind()); });
t(function () { return Function.prototype.toString.call({}); });
t(function () { return Function.prototype.toString.call(1); });
t(function () { return Function.prototype.toString.call(undefined); });
t(function () { return Function.prototype.toString.call(null); });
t(function () { return Function.prototype.toString.call('function f() {}'); });
t(function () { return Function.prototype.toString.call(Symbol()); });
t(function () { return Function.prototype.toString.call([]); });
t(function () { return Function.prototype.toString.call(new Error()); });
t(function () { return Function.prototype.toString.call(Object.create(Function.prototype)); });
t(function () { var f = function () {}; Object.defineProperty(f, 'name', { value: 'changed' }); return f.toString(); });
t(function () { var f = Math.pow.bind(); Object.defineProperty(f, 'name', { value: 'changed' }); return f.toString(); });
t(function () { var f = Math.pow; var d = Object.getOwnPropertyDescriptor(f, 'name'); Object.defineProperty(f, 'name', { value: 'renamed' }); var r = f.toString(); Object.defineProperty(f, 'name', d); return r; });
",
        r"function pow() { [native code] }
function () { [native code] }
function bind() { [native code] }
function toString() { [native code] }
function Object() { [native code] }
function Error() { [native code] }|function toString() { [native code] }|function TypeError() { [native code] }|function AggregateError() { [native code] }
function indexOf() { [native code] }|function slice() { [native code] }|function isArray() { [native code] }
function get __proto__() { [native code] }
function set __proto__() { [native code] }
function [Symbol.toPrimitive]() { [native code] }
function values() { [native code] }
function [Symbol.hasInstance]() { [native code] }
function get description() { [native code] }
function get [Symbol.species]() { [native code] }
function keys() { [native code] }
function Symbol() { [native code] }
function apply() { [native code] }
function () { [native code] }
TypeError: Function.prototype.toString requires that 'this' be a Function
TypeError: Function.prototype.toString requires that 'this' be a Function
TypeError: Function.prototype.toString requires that 'this' be a Function
TypeError: Function.prototype.toString requires that 'this' be a Function
TypeError: Function.prototype.toString requires that 'this' be a Function
TypeError: Function.prototype.toString requires that 'this' be a Function
TypeError: Function.prototype.toString requires that 'this' be a Function
TypeError: Function.prototype.toString requires that 'this' be a Function
TypeError: Function.prototype.toString requires that 'this' be a Function
function () {}
function () { [native code] }
function pow() { [native code] }",
    )]);
}

#[test]
fn function_prototype_restricted_properties() {
    check_cases(&[(
        r#"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); return [typeof d.get, d.get === d.set, d.enumerable, d.configurable, 'value' in d].join(); });
t(function () { var d = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments'); return [typeof d.get, d.get === d.set, d.enumerable, d.configurable, 'value' in d].join(); });
t(function () { var a = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); var b = Object.getOwnPropertyDescriptor(Function.prototype, 'arguments'); return a.get === b.get; });
t(function () { var d = Object.getOwnPropertyDescriptor(Function.prototype, 'caller'); return d.get.length + ',' + JSON_free(d.get.name) + ',' + Object.isExtensible(d.get) + ',' + Object.getOwnPropertyNames(d.get).join(); function JSON_free(s) { return '"' + s + '"'; } });
t(function () { return Function.prototype.caller; });
t(function () { return Function.prototype.arguments; });
t(function () { Function.prototype.caller = 1; });
t(function () { Function.prototype.arguments = 1; });
t(function () { 'use strict'; return (function () {}).caller; });
t(function () { 'use strict'; return (function () {}).arguments; });
t(function () { return Object.getOwnPropertyNames(Function.prototype).join(); });
t(function () { return Object.getOwnPropertyNames(Function.prototype).length; });
"#,
        r#"function,true,false,true,false
function,true,false,true,false
true
0,"",false,length,name
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them
length,name,arguments,caller,constructor,apply,bind,call,toString
9"#,
    )]);
}

#[test]
fn function_has_no_own_caller_or_arguments() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
// Deviation from V8, on purpose: sloppy functions have no own `caller` and
// `arguments` (the specification has none); they inherit the throwing
// accessors of Function.prototype.
t(function () { return Object.getOwnPropertyNames(function f() {}).join(); });
t(function () { return Object.getOwnPropertyNames(function f() { 'use strict'; }).join(); });
t(function () { return Object.getOwnPropertyNames(() => 1).join(); });
t(function () { return Object.getOwnPropertyNames(Math.pow).join(); });
t(function () { return Object.getOwnPropertyNames(function* () {}).join(); });
t(function () { return Object.getOwnPropertyNames(function () {}.bind()).join(); });
t(function () { var f = function () {}; return [f.hasOwnProperty('caller'), 'caller' in f].join(); });
t(function () { return (function () {}).caller; });
t(function () { return (function () { return arguments.callee.caller; })(); });
t(function () { return (function () { 'use strict'; return arguments.callee; })(); });
",
        r"length,name,prototype
length,name,prototype
length,name
length,name
length,name,prototype
length,name
false,true
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them
TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them",
    )]);
}
