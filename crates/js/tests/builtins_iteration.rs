//! Cases for the iterator operations, the array iterator,
//! `Object.fromEntries` and `Object.groupBy` (M7 feature 2b). The
//! expected output of each case is the output of Node.js 22 for the same
//! script (`print` joins `String` of its arguments). Each case also runs
//! in the GC stress mode.
//!
//! Generated from a list of scripts and Node's output, then edited by hand;
//! cases whose output differs from Node on purpose say so. Deviation: V8
//! calls `return` of the iterator of `Object.fromEntries` also when
//! `next()` throws or gives a bad result; the specification and test262
//! do not, and swb follows them (the `not closed` lines).

#![allow(clippy::too_many_lines, reason = "a test is a list of script cases")]

mod common;

use common::check_cases;

#[test]
fn array_iterator_basics() {
    check_cases(&[
        (
            r#"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
function J(v){ if (v===undefined) return undefined; if (v===null) return 'null'; if (typeof v==='string') return '"'+v+'"'; if (typeof v==='symbol') return undefined; if (typeof v!=='object') return String(v); var parts=[]; if (Array.isArray(v)){ for(var i=0;i<v.length;i++){var s=J(v[i]);parts.push(s===undefined?'null':s);} return '['+parts.join(',')+']'; } var ks=Object.keys(v); for(var i=0;i<ks.length;i++){var s=J(v[ks[i]]);if(s!==undefined)parts.push('"'+ks[i]+'":'+s);} return '{'+parts.join(',')+'}'; }
function iterOf(next, ret){ var it = {}; it[Symbol.iterator] = function(){ var i = {next: next}; if (ret) i['return'] = ret; return i }; return it }
t(function(){var it=[1,2,3].values();var r=[];var x;while(!(x=it.next()).done)r.push(x.value);return r.join()+J(it.next())});
t(function(){var it=['a','b'].entries();return J([it.next(),it.next(),it.next(),it.next()])});
t(function(){var it=['a','b'].keys();return J([it.next(),it.next(),it.next()])});
t(function(){var a=[1];var it=a.values();it.next();a.push(2);return J(it.next())});
t(function(){var a=[1];var it=a.values();it.next();it.next();a.push(2);return J(it.next())});
t(function(){var o={length:2,0:'x',1:'y'};var it=Array.prototype.values.call(o);return J([it.next(),it.next(),it.next()])});
t(function(){var it=Array.prototype.keys.call('ab');return J([it.next(),it.next(),it.next()])});
t(function(){var it=Array.prototype.entries.call({length: 1});return J(it.next())});
t(function(){var a=[1,2,3];var it=a.values();a.length=0;return J([it.next(),a.push(1),it.next()])});
t(function(){var a=[1,2,3];var it=a.values();it.next();a.length=1;return J(it.next())});
t(function(){var a=[1,,3];var it=a.values();return J([it.next(),it.next(),it.next()])});
t(function(){var a=[];a[2]=1;var it=a.entries();return J([it.next(),it.next(),it.next(),it.next()])});
t(function(){return Object.keys([].values().next()).join()});
t(function(){var r=[].values().next();var d=Object.getOwnPropertyDescriptor(r,'done');return d.writable+','+d.enumerable+','+d.configurable+','+Object.getPrototypeOf(r)===Object.prototype});
t(function(){return Array.prototype.values.call(5).next().done});
t(function(){return J(Array.prototype.values.call(true).next())});
"#,
            r#"1,2,3{"done":true}
[{"value":[0,"a"],"done":false},{"value":[1,"b"],"done":false},{"done":true},{"done":true}]
[{"value":0,"done":false},{"value":1,"done":false},{"done":true}]
{"value":2,"done":false}
{"done":true}
[{"value":"x","done":false},{"value":"y","done":false},{"done":true}]
[{"value":0,"done":false},{"value":1,"done":false},{"done":true}]
{"value":[0,null],"done":false}
[{"done":true},1,{"done":true}]
{"done":true}
[{"value":1,"done":false},{"done":false},{"value":3,"done":false}]
[{"value":[0,null],"done":false},{"value":[1,null],"done":false},{"value":[2,1],"done":false},{"done":true}]
value,done
false
true
{"done":true}"#,
        ),
        (
            r#"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
function J(v){ if (v===undefined) return undefined; if (v===null) return 'null'; if (typeof v==='string') return '"'+v+'"'; if (typeof v==='symbol') return undefined; if (typeof v!=='object') return String(v); var parts=[]; if (Array.isArray(v)){ for(var i=0;i<v.length;i++){var s=J(v[i]);parts.push(s===undefined?'null':s);} return '['+parts.join(',')+']'; } var ks=Object.keys(v); for(var i=0;i<ks.length;i++){var s=J(v[ks[i]]);if(s!==undefined)parts.push('"'+ks[i]+'":'+s);} return '{'+parts.join(',')+'}'; }
function iterOf(next, ret){ var it = {}; it[Symbol.iterator] = function(){ var i = {next: next}; if (ret) i['return'] = ret; return i }; return it }
t(function(){return [].values.call(null)});
t(function(){return [].keys.call(undefined)});
t(function(){return [].entries.call(null)});
var AIP = Object.getPrototypeOf([].values());
t(function(){return AIP.next.call({})});
t(function(){return AIP.next.call(1)});
t(function(){return AIP.next.call(undefined)});
t(function(){return AIP.next.call([].values)});
t(function(){return Object.getPrototypeOf([].keys()) === AIP && Object.getPrototypeOf([].entries()) === AIP});
t(function(){return Object.getOwnPropertyNames(AIP).join() + '|' + Reflect.ownKeys(AIP).length});
t(function(){var d = Object.getOwnPropertyDescriptor(AIP, Symbol.toStringTag); return d.value + d.writable + d.enumerable + d.configurable});
t(function(){var d = Object.getOwnPropertyDescriptor(AIP, 'next'); return d.writable + ',' + d.enumerable + ',' + d.configurable + ',' + AIP.next.name + ',' + AIP.next.length});
t(function(){var IP = Object.getPrototypeOf(AIP); return IP[Symbol.iterator].name + ',' + IP[Symbol.iterator].length + ',' + Object.prototype.hasOwnProperty.call(IP, Symbol.iterator)});
t(function(){var IP = Object.getPrototypeOf(AIP); var d = Object.getOwnPropertyDescriptor(IP, Symbol.iterator); return d.writable + ',' + d.enumerable + ',' + d.configurable});
t(function(){var i = [].values(); return i[Symbol.iterator]() === i});
t(function(){return Array.prototype[Symbol.iterator] === Array.prototype.values});
t(function(){return [].values.name + ',' + [][Symbol.iterator].name + ',' + [].keys.name + ',' + [].entries.name});
t(function(){return Array.prototype.values.length + ',' + Array.prototype.keys.length + ',' + Array.prototype.entries.length});
t(function(){var d = Object.getOwnPropertyDescriptor(Array.prototype, Symbol.iterator); return d.writable + ',' + d.enumerable + ',' + d.configurable});
t(function(){return Object.prototype.toString.call([].values()) + String([].keys())});
t(function(){var G = Object.getPrototypeOf(Object.getPrototypeOf(function*(){}.prototype)); return G === Object.getPrototypeOf(AIP)});
t(function(){var g = (function*(){ yield 1 })(); return g[Symbol.iterator]() === g});
"#,
            r"TypeError: Cannot convert undefined or null to object
TypeError: Cannot convert undefined or null to object
TypeError: Cannot convert undefined or null to object
TypeError: Method Array Iterator.prototype.next called on incompatible receiver #<Object>
TypeError: Method Array Iterator.prototype.next called on incompatible receiver 1
TypeError: Method Array Iterator.prototype.next called on incompatible receiver undefined
TypeError: Method Array Iterator.prototype.next called on incompatible receiver function values() { [native code] }
true
next|2
Array Iteratorfalsefalsetrue
true,false,true,next,0
[Symbol.iterator],0,true
true,false,true
true
true
values,values,keys,entries
0,0,0
true,false,true
[object Array Iterator][object Array Iterator]
true
true",
        ),
        (
            r#"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
function J(v){ if (v===undefined) return undefined; if (v===null) return 'null'; if (typeof v==='string') return '"'+v+'"'; if (typeof v==='symbol') return undefined; if (typeof v!=='object') return String(v); var parts=[]; if (Array.isArray(v)){ for(var i=0;i<v.length;i++){var s=J(v[i]);parts.push(s===undefined?'null':s);} return '['+parts.join(',')+']'; } var ks=Object.keys(v); for(var i=0;i<ks.length;i++){var s=J(v[ks[i]]);if(s!==undefined)parts.push('"'+ks[i]+'":'+s);} return '{'+parts.join(',')+'}'; }
function iterOf(next, ret){ var it = {}; it[Symbol.iterator] = function(){ var i = {next: next}; if (ret) i['return'] = ret; return i }; return it }
t(function(){var o={};Object.defineProperty(o,'length',{get:function(){print('len');return 1}});var it=Array.prototype.values.call(o);it.next();it.next();return 1});
t(function(){var o={length:3};Object.defineProperty(o,'1',{get:function(){throw new RangeError('el')}});var it=Array.prototype.values.call(o);var r=[J(it.next())];try{it.next()}catch(e){r.push(e.message)}return r.join('|')});
t(function(){var o={};Object.defineProperty(o,'length',{get:function(){throw new RangeError('ln')}});var it=Array.prototype.keys.call(o);var r=[];try{it.next()}catch(e){r.push(e.message)}return r.join('|')});
t(function(){var o={length:{valueOf:function(){return 2}}};return J(Array.prototype.keys.call(o).next())});
t(function(){var o={length:-5};return J(Array.prototype.keys.call(o).next())});
t(function(){var o={length:Infinity};var it=Array.prototype.keys.call(o);it.next();return J(it.next())});
t(function(){var o={length:9007199254740993};var it=Array.prototype.keys.call(o);return J(it.next())});
t(function(){var a=[1,2,3];var it=a.entries();var e=it.next().value;e[1]=9;return a.join()+J(it.next())});
t(function(){var a=[1,2];var seen=[];var it=a.values();var x;while(!(x=it.next()).done){seen.push(x.value);if(a.length<5)a.push(0)}return seen.join()});
"#,
            r#"len
len
1
{"done":false}|el
ln
{"value":0,"done":false}
{"done":true}
{"value":1,"done":false}
{"value":0,"done":false}
1,2,3{"value":[1,2],"done":false}
1,2,0,0,0"#,
        ),
    ]);
}

#[test]
fn from_entries() {
    check_cases(&[
        (
            r#"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
function J(v){ if (v===undefined) return undefined; if (v===null) return 'null'; if (typeof v==='string') return '"'+v+'"'; if (typeof v==='symbol') return undefined; if (typeof v!=='object') return String(v); var parts=[]; if (Array.isArray(v)){ for(var i=0;i<v.length;i++){var s=J(v[i]);parts.push(s===undefined?'null':s);} return '['+parts.join(',')+']'; } var ks=Object.keys(v); for(var i=0;i<ks.length;i++){var s=J(v[ks[i]]);if(s!==undefined)parts.push('"'+ks[i]+'":'+s);} return '{'+parts.join(',')+'}'; }
function iterOf(next, ret){ var it = {}; it[Symbol.iterator] = function(){ var i = {next: next}; if (ret) i['return'] = ret; return i }; return it }
t(function(){return J(Object.fromEntries([['a',1],['b',2],['a',3],[1,2],[Symbol.iterator,0]]))});
t(function(){var o = Object.fromEntries([['a', 1]]); return Object.getPrototypeOf(o) === Object.prototype && J(Object.getOwnPropertyDescriptor(o, 'a'))});
t(function(){return J(Object.fromEntries([{0:'k',1:'v'}, ['n'], [], [2,3,4]]))});
t(function(){var m = {}; m[Symbol.iterator] = Array.prototype.values; m.length = 1; m[0] = ['z', 26]; return J(Object.fromEntries(m))});
t(function(){return J(Object.fromEntries([[{toString:function(){return 'ts'}}, 1]]))});
t(function(){return J(Object.fromEntries([['__proto__', 1]])) + Object.getPrototypeOf(Object.fromEntries([['__proto__', 1]])) === Object.prototype});
t(function(){return Object.fromEntries.length + Object.fromEntries.name + Object.getOwnPropertyDescriptor(Object, 'fromEntries').enumerable});
t(function(){return Object.fromEntries()});
t(function(){return Object.fromEntries(undefined)});
t(function(){return Object.fromEntries(null)});
t(function(){return Object.fromEntries(1)});
t(function(){return Object.fromEntries(true)});
t(function(){return Object.fromEntries(Symbol())});
t(function(){return Object.fromEntries({})});
t(function(){return Object.fromEntries(function(){})});
t(function(){return Object.fromEntries(-0)});
t(function(){return Object.fromEntries([1])});
t(function(){return Object.fromEntries([null])});
t(function(){return Object.fromEntries([undefined])});
t(function(){return Object.fromEntries([true])});
"#,
            r#"{"1":2,"a":3,"b":2}
{"value":1,"writable":true,"enumerable":true,"configurable":true}
{"2":3,"k":"v"}
{"z":26}
{"ts":1}
false
1fromEntriesfalse
TypeError: undefined is not iterable
TypeError: undefined is not iterable
TypeError: undefined is not iterable
TypeError: number 1 is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: boolean true is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: symbol is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: function is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: number 0 is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: Iterator value 1 is not an entry object
TypeError: Iterator value null is not an entry object
TypeError: Iterator value undefined is not an entry object
TypeError: Iterator value true is not an entry object"#,
        ),
        (
            r#"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
function J(v){ if (v===undefined) return undefined; if (v===null) return 'null'; if (typeof v==='string') return '"'+v+'"'; if (typeof v==='symbol') return undefined; if (typeof v!=='object') return String(v); var parts=[]; if (Array.isArray(v)){ for(var i=0;i<v.length;i++){var s=J(v[i]);parts.push(s===undefined?'null':s);} return '['+parts.join(',')+']'; } var ks=Object.keys(v); for(var i=0;i<ks.length;i++){var s=J(v[ks[i]]);if(s!==undefined)parts.push('"'+ks[i]+'":'+s);} return '{'+parts.join(',')+'}'; }
function iterOf(next, ret){ var it = {}; it[Symbol.iterator] = function(){ var i = {next: next}; if (ret) i['return'] = ret; return i }; return it }
function mk(next, ret){ return iterOf(next, ret) }
t(function(){var o = {}; o[Symbol.iterator] = 1; return Object.fromEntries(o)});
t(function(){var o = {}; o[Symbol.iterator] = null; return Object.fromEntries(o)});
t(function(){var o = {}; o[Symbol.iterator] = function(){ return 1 }; return Object.fromEntries(o)});
t(function(){var o = {}; o[Symbol.iterator] = function(){ return {} }; return Object.fromEntries(o)});
t(function(){var o = {}; o[Symbol.iterator] = function(){ return {next: 1} }; return Object.fromEntries(o)});
t(function(){return Object.fromEntries(mk(function(){ return 1 }))});
t(function(){return Object.fromEntries(mk(function(){ return undefined }))});
t(function(){return J(Object.fromEntries(mk(function(){ return {done: true} })))});
t(function(){var i = 0; return J(Object.fromEntries(mk(function(){ return i++ < 2 ? {value: ['k' + i, i], done: false} : {done: true, value: ['no', 0]} })))});
t(function(){var n = 0; return Object.fromEntries(mk(function(){ return {done: false, value: 1} }, function(){ print('closed', ++n); return {} }))});
t(function(){return Object.fromEntries(mk(function(){ return {done: false, value: 1} }, function(){ print('closed'); return 5 }))});
t(function(){return Object.fromEntries(mk(function(){ return {done: false, value: 1} }, function(){ throw new RangeError('r') }))});
t(function(){return Object.fromEntries(mk(function(){ return {done: false, value: 1} }, 5))});
t(function(){return Object.fromEntries(mk(function(){ return {done: false, value: [{toString: function(){ throw new RangeError('ts') }}, 1]} }, function(){ print('closed3'); return 5 }))});
t(function(){return Object.fromEntries(mk(function(){ throw new SyntaxError('nx') }, function(){ print('not closed'); return {} }))});
t(function(){var o = {done: false}; Object.defineProperty(o, 'value', {get: function(){ throw new RangeError('val') }}); return Object.fromEntries(mk(function(){ return o }, function(){ print('not closed 2'); return {} }))});
t(function(){var o = {value: 1}; Object.defineProperty(o, 'done', {get: function(){ throw new RangeError('dn') }}); return Object.fromEntries(mk(function(){ return o }, function(){ print('not closed 3'); return {} }))});
t(function(){var item = []; Object.defineProperty(item, '0', {get: function(){ throw new RangeError('k0') }}); return Object.fromEntries(mk(function(){ return {done: false, value: item} }, function(){ print('closed4'); return {} }))});
t(function(){var item = ['a']; Object.defineProperty(item, '1', {get: function(){ throw new RangeError('k1') }}); return Object.fromEntries(mk(function(){ return {done: false, value: item} }, function(){ print('closed5'); return {} }))});
t(function(){var calls = []; var it = mk(function(){ calls.push('next'); return calls.length > 2 ? {done: true} : {done: false, value: ['a', 1]} }); return J(Object.fromEntries(it)) + calls.join()});
t(function(){var nextReads = 0; var o = {}; o[Symbol.iterator] = function(){ var i = {}; Object.defineProperty(i, 'next', {get: function(){ nextReads++; return function(){ return {done: true} } }}); return i }; Object.fromEntries(o); return nextReads});
"#,
            r#"TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: Result of the Symbol.iterator method is not an object
TypeError: undefined is not a function
TypeError: number 1 is not a function
TypeError: Iterator result 1 is not an object
TypeError: Iterator result undefined is not an object
{}
{"k1":1,"k2":2}
closed 1
TypeError: Iterator value 1 is not an entry object
closed
TypeError: Iterator value 1 is not an entry object
TypeError: Iterator value 1 is not an entry object
TypeError: Iterator value 1 is not an entry object
closed3
RangeError: ts
SyntaxError: nx
RangeError: val
RangeError: dn
closed4
RangeError: k0
closed5
RangeError: k1
{"a":1}next,next,next
1"#,
        ),
    ]);
}

#[test]
fn group_by() {
    check_cases(&[
        (
            r#"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
function J(v){ if (v===undefined) return undefined; if (v===null) return 'null'; if (typeof v==='string') return '"'+v+'"'; if (typeof v==='symbol') return undefined; if (typeof v!=='object') return String(v); var parts=[]; if (Array.isArray(v)){ for(var i=0;i<v.length;i++){var s=J(v[i]);parts.push(s===undefined?'null':s);} return '['+parts.join(',')+']'; } var ks=Object.keys(v); for(var i=0;i<ks.length;i++){var s=J(v[ks[i]]);if(s!==undefined)parts.push('"'+ks[i]+'":'+s);} return '{'+parts.join(',')+'}'; }
function iterOf(next, ret){ var it = {}; it[Symbol.iterator] = function(){ var i = {next: next}; if (ret) i['return'] = ret; return i }; return it }
t(function(){return J(Object.groupBy([1,2,3,4,5],function(x,i){return x%2?'odd':'even'}))});
t(function(){return J(Object.groupBy([1,2,3],function(x,i){return i}))});
t(function(){return Object.keys(Object.groupBy([1,2,3],function(x,i){return [x,i]})).join('|')});
t(function(){return Object.getPrototypeOf(Object.groupBy([],function(){return 1}))});
t(function(){var g = Object.groupBy([1,2],function(x){return x}); return Array.isArray(g[1]) + ',' + Object.getPrototypeOf(g[1]) === Array.prototype});
t(function(){var g = Object.groupBy([1,2,3],function(x){return Symbol.iterator}); return g[Symbol.iterator].join()});
t(function(){var g = Object.groupBy([1,2,3,4],function(x){return x%2 ? '__proto__' : 'b'}); return Object.keys(g).join() + g['__proto__'].join()});
t(function(){var args; Object.groupBy([5],function(){ args = arguments; return 'k' }); return args.length + ',' + args[0] + ',' + args[1]});
t(function(){var thisv = 'unset'; Object.groupBy([5],function(){ 'use strict'; thisv = this; return 'k' }); return thisv});
t(function(){return J(Object.groupBy(Object.fromEntries([['a',1]]).__proto__ === undefined ? [] : [10, 20, 30], function(x){ return x > 15 }))});
t(function(){return Object.groupBy.length + Object.groupBy.name});
t(function(){var d = Object.getOwnPropertyDescriptor(Object, 'groupBy'); return d.writable + ',' + d.enumerable + ',' + d.configurable});
t(function(){var order = []; Object.groupBy({length: 3, 0: 'a', 1: 'b', 2: 'c', [Symbol.iterator]: Array.prototype.values}, function(x, i){ order.push(x + i); return x }); return order.join()});
t(function(){return J(Object.groupBy({length: 2, 0: 'x', 1: 'x', [Symbol.iterator]: Array.prototype.values}, function(x){ return x }))});
"#,
            r#"{"odd":[1,3,5],"even":[2,4]}
{"0":[1],"1":[2],"2":[3]}
1,0|2,1|3,2
null
false
1,2,3
__proto__,b1,3
2,5,0
undefined
{"false":[10],"true":[20,30]}
2groupBy
true,false,true
a0,b1,c2
{"x":["x","x"]}"#,
        ),
        (
            r#"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
function J(v){ if (v===undefined) return undefined; if (v===null) return 'null'; if (typeof v==='string') return '"'+v+'"'; if (typeof v==='symbol') return undefined; if (typeof v!=='object') return String(v); var parts=[]; if (Array.isArray(v)){ for(var i=0;i<v.length;i++){var s=J(v[i]);parts.push(s===undefined?'null':s);} return '['+parts.join(',')+']'; } var ks=Object.keys(v); for(var i=0;i<ks.length;i++){var s=J(v[ks[i]]);if(s!==undefined)parts.push('"'+ks[i]+'":'+s);} return '{'+parts.join(',')+'}'; }
function iterOf(next, ret){ var it = {}; it[Symbol.iterator] = function(){ var i = {next: next}; if (ret) i['return'] = ret; return i }; return it }
function mk(next, ret){ return iterOf(next, ret) }
t(function(){return Object.groupBy()});
t(function(){return Object.groupBy([1])});
t(function(){return Object.groupBy([1], 5)});
t(function(){return Object.groupBy([1], 'abc')});
t(function(){return Object.groupBy([1], {})});
t(function(){return Object.groupBy([1], Symbol())});
t(function(){return Object.groupBy(5, function(){ return 1 })});
t(function(){return Object.groupBy({}, function(){ return 1 })});
t(function(){return Object.groupBy(null, function(){ return 1 })});
t(function(){return Object.groupBy(undefined, 5)});
t(function(){return Object.groupBy(null, 5)});
t(function(){return Object.groupBy([1], function(){ throw new RangeError('cb') })});
t(function(){return Object.groupBy(mk(function(){ return {done: false, value: 1} }, function(){ print('closedg'); return {} }), function(){ throw new RangeError('cb') })});
t(function(){return Object.groupBy(mk(function(){ return {done: false, value: 1} }, function(){ print('closedg2'); return {} }), function(){ return {toString: function(){ throw new RangeError('key') }} })});
t(function(){return Object.groupBy(mk(function(){ throw new RangeError('nx') }, function(){ print('not closed'); return {} }), function(){ return 1 })});
t(function(){var n = 0; return J(Object.groupBy(mk(function(){ return n++ < 3 ? {done: false, value: n} : {done: true} }, function(){ print('not closed 2'); return {} }), function(x){ return x % 2 }))});
"#,
            r#"TypeError: Object.groupBy called on null or undefined
TypeError: undefined is not a function
TypeError: 5 is not a function
TypeError: abc is not a function
TypeError: #<Object> is not a function
TypeError: Symbol() is not a function
TypeError: number 5 is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: Object.groupBy called on null or undefined
TypeError: Object.groupBy called on null or undefined
TypeError: Object.groupBy called on null or undefined
RangeError: cb
closedg
RangeError: cb
closedg2
RangeError: key
RangeError: nx
{"0":[2],"1":[1,3]}"#,
        ),
    ]);
}

#[test]
fn iterator_protocol_through_natives() {
    check_cases(&[(
        r#"function t(f){try{print(f())}catch(e){print(e.constructor.name+': '+e.message)}}
function J(v){ if (v===undefined) return undefined; if (v===null) return 'null'; if (typeof v==='string') return '"'+v+'"'; if (typeof v==='symbol') return undefined; if (typeof v!=='object') return String(v); var parts=[]; if (Array.isArray(v)){ for(var i=0;i<v.length;i++){var s=J(v[i]);parts.push(s===undefined?'null':s);} return '['+parts.join(',')+']'; } var ks=Object.keys(v); for(var i=0;i<ks.length;i++){var s=J(v[ks[i]]);if(s!==undefined)parts.push('"'+ks[i]+'":'+s);} return '{'+parts.join(',')+'}'; }
function iterOf(next, ret){ var it = {}; it[Symbol.iterator] = function(){ var i = {next: next}; if (ret) i['return'] = ret; return i }; return it }
t(function(){var o = {}; o[Symbol.iterator] = function(){ return 1 }; return Object.groupBy(o, function(){ return 1 })});
t(function(){var o = {}; o[Symbol.iterator] = function(){ return {next: function(){ return 'x' }} }; return Object.groupBy(o, function(){ return 1 })});
t(function(){var o = {}; o[Symbol.iterator] = function(){ return {next: function(){ return {done: 1, value: 2} }} }; return J(Object.groupBy(o, function(){ return 1 }))});
t(function(){var n = 0; var o = {}; o[Symbol.iterator] = function(){ return {next: function(){ return {done: n++ > 1, value: n} }} }; return J(Object.groupBy(o, function(v){ return 'k' }))});
t(function(){var o = {}; o[Symbol.iterator] = function(){ return {next: function(){ return {done: false, value: 1} }, 'return': function(){ throw new TypeError('ret') }} }; return Object.groupBy(o, function(){ throw new RangeError('first') })});
t(function(){var r = []; var o = {}; o[Symbol.iterator] = function(){ r.push('get'); return {next: function(){ r.push('next'); return {done: r.length > 3, value: 1} }} }; Object.fromEntries(o).x; return r.join()});
t(function(){var o = {}; o[Symbol.iterator] = function(){ return this }; o.next = function(){ return {done: true} }; return J(Object.fromEntries(o))});
t(function(){return J(Object.fromEntries([['a', 1]].values()))});
t(function(){return J(Object.fromEntries([['a', 1], ['b', 2]].entries()))});
t(function(){return J(Object.fromEntries([['a', 1]].keys()))});
"#,
        r#"TypeError: Result of the Symbol.iterator method is not an object
TypeError: Iterator result x is not an object
{}
{"k":[1,2]}
RangeError: first
TypeError: Iterator value 1 is not an entry object
{}
{"a":1}
{"0":["a",1],"1":["b",2]}
TypeError: Iterator value 0 is not an entry object"#,
    )]);
}
