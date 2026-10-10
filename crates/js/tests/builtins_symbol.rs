//! Cases for `Symbol`, `Boolean`, `@@toPrimitive`, `@@hasInstance`,
//! `@@toStringTag`, the array iterator, `Object.fromEntries` and
//! `Object.groupBy` (M7 feature 2b). The expected output of each case is
//! the output of Node.js 22 for the same script (`print` joins `String` of
//! its arguments). Each case also runs in the GC stress mode.
//!
//! Generated from a list of scripts and Node's output, then edited by hand;
//! cases whose output differs from Node on purpose say so.

#![allow(clippy::too_many_lines, reason = "a test is a list of script cases")]

mod common;

use common::check_cases;

#[test]
fn symbol_constructor_and_description() {
    check_cases(&[
        (
            r"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
t(function(){return new Symbol()});
t(function(){return Symbol.name + Symbol.length + typeof Symbol()});
t(function(){return Symbol('x').description + '|' + Symbol().description + '|' + Symbol('').description + '|' + Symbol(undefined).description + '|' + Symbol(null).description + '|' + Symbol(12).description});
t(function(){return Symbol(Symbol())});
t(function(){return Symbol({toString: function(){ return 'ts' }}).description});
t(function(){return String(Symbol('x')) + String(Symbol()) + Symbol('y').toString() + Symbol.prototype.toString.call(Object(Symbol('z')))});
t(function(){return Symbol('a') === Symbol('a')});
t(function(){var s = Symbol('k'); return s === s && s == s && Object(s) == s && Object(s) !== s});
t(function(){return Symbol('a') + ''});
t(function(){return `${Symbol('a')}`});
t(function(){return +Symbol('a')});
t(function(){return Symbol() * 2});
t(function(){return Symbol() + 1});
t(function(){return Symbol() < 1});
t(function(){return !!Symbol() + typeof Object(Symbol())});
t(function(){return Object.getOwnPropertyNames(Symbol).join()});
t(function(){return Object.getOwnPropertyNames(Symbol.prototype).join()});
t(function(){return Reflect.ownKeys(Symbol.prototype).map(String).join()});
t(function(){return Symbol.prototype.constructor === Symbol});
t(function(){var s = Symbol('a\ud800b'); return s.toString().length + ',' + s.description.length + ',' + (String(s) === 'Symbol(' + s.description + ')') + ',' + (s.toString() === 'Symbol(' + s.description + ')')});
",
            r"TypeError: Symbol is not a constructor
Symbol0symbol
x|undefined||undefined|null|12
TypeError: Cannot convert a Symbol value to a string
ts
Symbol(x)Symbol()Symbol(y)Symbol(z)
false
true
TypeError: Cannot convert a Symbol value to a string
TypeError: Cannot convert a Symbol value to a string
TypeError: Cannot convert a Symbol value to a number
TypeError: Cannot convert a Symbol value to a number
TypeError: Cannot convert a Symbol value to a number
TypeError: Cannot convert a Symbol value to a number
trueobject
length,name,prototype,for,keyFor,asyncIterator,hasInstance,isConcatSpreadable,iterator,match,matchAll,replace,search,species,split,toPrimitive,toStringTag,unscopables
constructor,toString,valueOf,description
constructor,toString,valueOf,description,Symbol(Symbol.toStringTag),Symbol(Symbol.toPrimitive)
true
11,3,true,true",
        ),
        (
            r"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
t(function(){return Symbol.prototype.toString.call(1)});
t(function(){return Symbol.prototype.valueOf.call({})});
t(function(){return Symbol.prototype.valueOf.call(Object(Symbol.iterator)) === Symbol.iterator});
t(function(){return Symbol.prototype.description});
t(function(){return Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get.call(1)});
var d = Object.getOwnPropertyDescriptor(Symbol.prototype, 'description');
print(typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length);
t(function(){return Symbol.prototype[Symbol.toPrimitive].call(1)});
var p = Object.getOwnPropertyDescriptor(Symbol.prototype, Symbol.toPrimitive);
print(p.writable, p.enumerable, p.configurable, p.value.name, p.value.length);
var g = Object.getOwnPropertyDescriptor(Symbol.prototype, Symbol.toStringTag);
print(g.value, g.writable, g.enumerable, g.configurable);
t(function(){var s = Symbol('v'); return Symbol.prototype[Symbol.toPrimitive].call(s) === s && Symbol.prototype[Symbol.toPrimitive].call(Object(s)) === s});
",
            r"TypeError: Symbol.prototype.toString requires that 'this' be a Symbol
TypeError: Symbol.prototype.valueOf requires that 'this' be a Symbol
true
TypeError: Symbol.prototype.description requires that 'this' be a Symbol
TypeError: Symbol.prototype.description requires that 'this' be a Symbol
function undefined false true get description 0
TypeError: Symbol.prototype [ @@toPrimitive ] requires that 'this' be a Symbol
false false true [Symbol.toPrimitive] 1
Symbol false false true
true",
        ),
    ]);
}

#[test]
fn symbol_registry_and_well_known() {
    check_cases(&[
        (
            r"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
t(function(){return Symbol.for('a') === Symbol.for('a')});
t(function(){return Symbol.for('a') === Symbol('a')});
t(function(){return Symbol.for() === Symbol.for('undefined')});
t(function(){return Symbol.for(1) === Symbol.for('1')});
t(function(){return Symbol.for(Symbol())});
t(function(){return Symbol.keyFor(Symbol.for('q')) + '|' + Symbol.keyFor(Symbol('q')) + '|' + Symbol.keyFor(Symbol.iterator)});
t(function(){return Symbol.keyFor(1)});
t(function(){return Symbol.keyFor('a')});
t(function(){return Symbol.keyFor({})});
t(function(){return Symbol.keyFor(Object(Symbol.for('w')))});
t(function(){return Symbol.for('').description === '' && Symbol.keyFor(Symbol.for('')) === ''});
t(function(){return Symbol.for('0') === Symbol.for(0) && Symbol.keyFor(Symbol.for('0')) === '0'});
t(function(){return Symbol.for.length + ',' + Symbol.keyFor.length + ',' + Symbol.for.name + ',' + Symbol.keyFor.name});
t(function(){var s = Symbol.for('registered'); var r = []; for (var i = 0; i < 200; i++) r.push(Symbol.for('x' + i)); gc(); return s === Symbol.for('registered') && r[199] === Symbol.for('x199') && Symbol.keyFor(r[3])});
",
            r"true
false
true
true
TypeError: Cannot convert a Symbol value to a string
q|undefined|undefined
TypeError: 1 is not a symbol
TypeError: a is not a symbol
TypeError: #<Object> is not a symbol
TypeError: [object Symbol] is not a symbol
true
true
1,1,for,keyFor
x3",
        ),
        (
            r"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
var names = ['asyncIterator', 'hasInstance', 'isConcatSpreadable', 'iterator', 'match', 'matchAll', 'replace', 'search', 'species', 'split', 'toPrimitive', 'toStringTag', 'unscopables'];
for (var i = 0; i < names.length; i++) {
  var d = Object.getOwnPropertyDescriptor(Symbol, names[i]);
  print(names[i], typeof d.value, d.writable, d.enumerable, d.configurable, String(d.value), d.value.description);
}
print(Symbol.iterator === Symbol.iterator, Symbol.keyFor(Symbol.iterator));
t(function(){Symbol.iterator = 1; return typeof Symbol.iterator});
t(function(){'use strict'; Symbol.iterator = 1});
t(function(){'use strict'; return delete Symbol.iterator});
",
            r"asyncIterator symbol false false false Symbol(Symbol.asyncIterator) Symbol.asyncIterator
hasInstance symbol false false false Symbol(Symbol.hasInstance) Symbol.hasInstance
isConcatSpreadable symbol false false false Symbol(Symbol.isConcatSpreadable) Symbol.isConcatSpreadable
iterator symbol false false false Symbol(Symbol.iterator) Symbol.iterator
match symbol false false false Symbol(Symbol.match) Symbol.match
matchAll symbol false false false Symbol(Symbol.matchAll) Symbol.matchAll
replace symbol false false false Symbol(Symbol.replace) Symbol.replace
search symbol false false false Symbol(Symbol.search) Symbol.search
species symbol false false false Symbol(Symbol.species) Symbol.species
split symbol false false false Symbol(Symbol.split) Symbol.split
toPrimitive symbol false false false Symbol(Symbol.toPrimitive) Symbol.toPrimitive
toStringTag symbol false false false Symbol(Symbol.toStringTag) Symbol.toStringTag
unscopables symbol false false false Symbol(Symbol.unscopables) Symbol.unscopables
true undefined
symbol
TypeError: Cannot assign to read only property 'iterator' of function 'function Symbol() { [native code] }'
TypeError: Cannot delete property 'iterator' of function Symbol() { [native code] }",
        ),
    ]);
}

#[test]
fn to_primitive_hint() {
    check_cases(&[(
        r"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
var log = [];
var o = {};
o[Symbol.toPrimitive] = function(h){ log.push(h); return 5 };
print(o + 1, `${o}`, +o, o * 2, o == 5, o < 6, String(o), Number(o), log.join());
var d = {};
d[Symbol.toPrimitive] = function(h){ return h };
print(d + '', `${d}`, +{[Symbol.toPrimitive]: function(h){ return h === 'number' ? 7 : 0 }}, d == 'default', d < 'default', String(d));
t(function(){var x = {}; x[Symbol.toPrimitive] = 1; return x + 1});
t(function(){var x = {}; x[Symbol.toPrimitive] = function(){ return {} }; return x + 1});
t(function(){var x = {}; x[Symbol.toPrimitive] = function(){}; return x + 1});
t(function(){var x = {}; x[Symbol.toPrimitive] = null; return x + 1});
t(function(){var x = {}; x[Symbol.toPrimitive] = undefined; x.valueOf = function(){ return 3 }; return x + 1});
t(function(){var x = {}; x[Symbol.toPrimitive] = function(){ return Symbol.iterator }; return typeof Object.keys({a: 1}) + (x + '')});
t(function(){var x = {}; x[Symbol.toPrimitive] = function(){ return Symbol.iterator }; var k = {}; k[x] = 1; return Object.getOwnPropertySymbols(k).length});
t(function(){var x = {}; x[Symbol.toPrimitive] = function(){ throw new RangeError('tp') }; return x + 1});
t(function(){var x = {}; x[Symbol.toPrimitive] = function(){ return this === x }; return x + 1});
t(function(){var f = function(){}; f[Symbol.toPrimitive] = function(){ return 'fn' }; return f + '!'});
t(function(){var s = Object(Symbol('w')); return typeof (s + '')});
",
        r"6 5 5 10 true true 5 5 default,string,number,number,default,number,string,number
default string 7 true false string
TypeError: number 1 is not a function
TypeError: Cannot convert object to primitive value
NaN
[object Object]1
4
TypeError: Cannot convert a Symbol value to a string
1
RangeError: tp
2
fn!
TypeError: Cannot convert a Symbol value to a string",
    )]);
}

#[test]
fn instanceof_has_instance() {
    check_cases(&[(
        r"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
t(function(){return 1 instanceof 5});
t(function(){return 1 instanceof {}});
t(function(){var o = {}; o[Symbol.hasInstance] = 5; return 1 instanceof o});
t(function(){var o = {}; o[Symbol.hasInstance] = function(v){ return v === 1 }; return (1 instanceof o) + ',' + (2 instanceof o)});
t(function(){var o = {}; o[Symbol.hasInstance] = null; return 1 instanceof o});
t(function(){var o = {}; o[Symbol.hasInstance] = function(){ return 'yes' }; return 1 instanceof o});
t(function(){var o = {}; o[Symbol.hasInstance] = function(){ return 0 }; return 1 instanceof o});
t(function(){return Function.prototype[Symbol.hasInstance].call(5, 1)});
t(function(){return Function.prototype[Symbol.hasInstance].call(Object, {})});
t(function(){return Function.prototype[Symbol.hasInstance].name + ',' + Function.prototype[Symbol.hasInstance].length});
var d = Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance);
print(d.writable, d.enumerable, d.configurable);
function F() {}
var f = new F();
print(f instanceof F, {} instanceof F, F[Symbol.hasInstance](f), F[Symbol.hasInstance](1));
var B = F.bind(null);
print(f instanceof B, new B() instanceof F, new B() instanceof B, B[Symbol.hasInstance](f));
var B3 = B.bind().bind();
print(f instanceof B3, {} instanceof B3);
Object.defineProperty(F, Symbol.hasInstance, {value: function(v){ return v === 7 }});
print(7 instanceof F, f instanceof F, f instanceof B, 7 instanceof B, 7 instanceof B3);
t(function(){function G(){}; G.prototype = 3; return ({}) instanceof G});
t(function(){function G(){}; G.prototype = 3; return 1 instanceof G});
t(function(){var o = {}; o[Symbol.hasInstance] = Function.prototype[Symbol.hasInstance]; return 1 instanceof o});
",
        r"TypeError: Right-hand side of 'instanceof' is not an object
TypeError: Right-hand side of 'instanceof' is not callable
TypeError: number 5 is not a function
true,false
TypeError: Right-hand side of 'instanceof' is not callable
true
false
false
true
[Symbol.hasInstance],1
false false false
true false true false
true true true true
true false
true false false true true
TypeError: Function has non-object prototype '3' in instanceof check
false
false",
    )]);
}

#[test]
fn object_to_string_tag() {
    check_cases(&[(
        r"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
var ts = Object.prototype.toString;
t(function(){return ts.call(Symbol()) + ts.call(Symbol.prototype) + ts.call(Math) + ts.call(Reflect)});
t(function(){return ts.call(Object(Symbol())) + ts.call(new Boolean(true)) + ts.call(Boolean.prototype) + ts.call(null) + ts.call(undefined)});
t(function(){return ts.call([].values()) + ts.call(function*(){}()) + ts.call(Object.getPrototypeOf([].values()))});
t(function(){return ts.call({[Symbol.toStringTag]: 5}) + ts.call({[Symbol.toStringTag]: 'x'}) + ts.call({[Symbol.toStringTag]: ''})});
t(function(){var o = [1]; o[Symbol.toStringTag] = 'Arr'; return ts.call(o)});
t(function(){var o = function(){}; o[Symbol.toStringTag] = 'Fn'; return ts.call(o) + ts.call(function(){}) + ts.call(Math.pow.bind(null))});
t(function(){var o = {}; Object.defineProperty(o, Symbol.toStringTag, {get: function(){ throw new RangeError('g') }}); return ts.call(o)});
t(function(){return String([].values()) + ({[Symbol.toStringTag]: 'Q'}) + ts.call(arguments) + ts.call(new Error('e')) + ts.call(new Number(1)) + ts.call(new String(''))});
t(function(){return ts.call(Object.getPrototypeOf(function*(){})) + ts.call(Object.getPrototypeOf(function*(){}.prototype))});
t(function(){var d = Object.getOwnPropertyDescriptor(Math, Symbol.toStringTag); return d.value + d.writable + d.enumerable + d.configurable});
t(function(){var d = Object.getOwnPropertyDescriptor(Reflect, Symbol.toStringTag); return d.value + d.writable + d.enumerable + d.configurable});
t(function(){return Object.keys(Reflect).length + ',' + Object.keys(Math).length});
t(function(){var long = ''; for (var i = 0; i < 100000; i++) long += 'ab'; var o = {}; o[Symbol.toStringTag] = long; return ts.call(o).length});
",
        r"[object Symbol][object Symbol][object Math][object Reflect]
[object Symbol][object Boolean][object Boolean][object Null][object Undefined]
[object Array Iterator][object Generator][object Array Iterator]
[object Object][object x][object ]
[object Arr]
[object Fn][object Function][object Function]
RangeError: g
[object Array Iterator][object Q][object Arguments][object Error][object Number][object String]
[object GeneratorFunction][object Generator]
Mathfalsefalsetrue
Reflectfalsefalsetrue
0,0
200009",
    )]);
}

#[test]
fn boolean() {
    check_cases(&[(
        r"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
t(function(){return Boolean() + ',' + Boolean(0) + ',' + Boolean('') + ',' + Boolean('a') + ',' + Boolean({}) + ',' + Boolean(Symbol()) + ',' + Boolean(NaN) + ',' + Boolean(-0)});
t(function(){return typeof new Boolean(0) + ',' + (new Boolean(0) ? 'truthy' : 'falsy') + ',' + new Boolean(0).valueOf() + ',' + new Boolean(1).toString()});
t(function(){return Boolean.length + Boolean.name + Boolean.prototype.constructor.name + typeof Boolean.prototype});
t(function(){return Boolean.prototype.valueOf() + ',' + Boolean.prototype.toString() + ',' + Object.prototype.toString.call(Boolean.prototype)});
t(function(){return Boolean.prototype.toString.call(1)});
t(function(){return Boolean.prototype.valueOf.call({})});
t(function(){return Boolean.prototype.toString.call(true) + Boolean.prototype.valueOf.call(Object(false)) + Boolean.prototype.toString.call(new Boolean(true))});
t(function(){return Object.getOwnPropertyNames(Boolean.prototype).join() + '|' + Object.getOwnPropertyNames(Boolean).join()});
t(function(){var d = Object.getOwnPropertyDescriptor(Boolean, 'prototype'); return d.writable + ',' + d.enumerable + ',' + d.configurable});
t(function(){function Sub() {} var o = Reflect.construct(Boolean, [1], Sub); return (Object.getPrototypeOf(o) === Sub.prototype) + ',' + o.valueOf() + ',' + Object.prototype.toString.call(o)});
t(function(){var b = new Boolean(true); b.x = 1; return Object.keys(b).join() + (b instanceof Boolean) + (true instanceof Boolean)});
t(function(){return new Boolean(false) + '' + (new Boolean(true) + 1) + (true + true) + (true + '')});
t(function(){return Boolean.prototype.toString.name + Boolean.prototype.toString.length + Boolean.prototype.valueOf.name});
",
        r"false,false,false,true,true,true,false,false
object,truthy,false,true
1BooleanBooleanobject
false,false,[object Boolean]
TypeError: Boolean.prototype.toString requires that 'this' be a Boolean
TypeError: Boolean.prototype.valueOf requires that 'this' be a Boolean
truefalsetrue
constructor,toString,valueOf|length,name,prototype
false,false,false
true,[object Boolean],[object Boolean]
xtruefalse
false22true
toString0valueOf",
    )]);
}
