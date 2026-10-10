//! Cases for `Error`, the native errors, `cause`, `Error.prototype.toString`
//! and `AggregateError` (M7 feature 2c).
//! The expected output of each case is the output of Node.js 22 for the same
//! script (`print` joins `String` of its arguments). Each case also runs in
//! the GC stress mode.
//!
//! Generated from a list of scripts and Node's output.

#![allow(clippy::too_many_lines, reason = "a test is a list of script cases")]

mod common;

use common::check_cases;

#[test]
fn error_constructors() {
    check_cases(&[(
        r#"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { return Error.length + ',' + Error.name + ',' + TypeError.length + ',' + TypeError.name + ',' + AggregateError.length + ',' + AggregateError.name; });
t(function () { var e = Error('a'); return (e instanceof Error) + ',' + e.message + ',' + Object.prototype.toString.call(e); });
t(function () { return Object.getOwnPropertyNames(new Error()).join() + '|' + Object.getOwnPropertyNames(new Error(undefined)).join() + '|' + Object.getOwnPropertyNames(new Error('')).join(); });
t(function () { var d = Object.getOwnPropertyDescriptor(new Error('m'), 'message'); return [d.value, d.writable, d.enumerable, d.configurable].join(); });
t(function () { return new Error({ toString: function () { return 'obj'; } }).message + new Error(5).message + new Error(null).message + new Error(true).message; });
t(function () { return new Error(Symbol()).message; });
t(function () { return Error.prototype.constructor === Error && Error.prototype.name + '|' + Error.prototype.message + '|' + Object.getOwnPropertyNames(Error.prototype).join(); });
t(function () { return Object.getPrototypeOf(Error) === Function.prototype && Object.getPrototypeOf(Error.prototype) === Object.prototype; });
t(function () { return [TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError].map(function (C) { return (Object.getPrototypeOf(C) === Error) + ',' + (Object.getPrototypeOf(C.prototype) === Error.prototype) + ',' + C.prototype.name + ',' + JSON_free(C.prototype.message) + ',' + (C.prototype.constructor === C) + ',' + Object.getOwnPropertyNames(C.prototype).join('/') + ',' + Object.getOwnPropertyNames(C).join('/'); }).join('\n'); function JSON_free(s) { return '"' + s + '"'; } });
t(function () { return [TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError].map(function (C) { var e = C('m'); var n = new C('m'); return e.name + ':' + e.message + ':' + (e instanceof C) + (e instanceof Error) + (n instanceof C) + ':' + String(n) + ':' + Object.getOwnPropertyNames(n).join('/'); }).join('\n'); });
t(function () { return Object.getPrototypeOf(AggregateError) === Error && Object.getPrototypeOf(AggregateError.prototype) === Error.prototype; });
t(function () { return AggregateError.prototype.name + '|' + AggregateError.prototype.message + '|' + Object.getOwnPropertyNames(AggregateError.prototype).join() + '|' + (AggregateError.prototype.constructor === AggregateError); });
t(function () { var d = Object.getOwnPropertyDescriptor(Error, 'prototype'); return [d.writable, d.enumerable, d.configurable].join(); });
t(function () { var d = Object.getOwnPropertyDescriptor(TypeError, 'prototype'); return [d.writable, d.enumerable, d.configurable].join(); });
t(function () { var d = Object.getOwnPropertyDescriptor(this, 'TypeError'); return [d.writable, d.enumerable, d.configurable].join(); });
t(function () { return typeof Error.prototype.toString + Error.prototype.toString.length + Error.prototype.toString.name; });
"#,
        r#"1,Error,1,TypeError,2,AggregateError
true,a,[object Error]
stack|stack|stack,message
m,true,false,true
obj5nulltrue
TypeError: Cannot convert a Symbol value to a string
Error||constructor,name,message,toString
true
true,true,TypeError,"",true,constructor/name/message,length/name/prototype
true,true,RangeError,"",true,constructor/name/message,length/name/prototype
true,true,ReferenceError,"",true,constructor/name/message,length/name/prototype
true,true,SyntaxError,"",true,constructor/name/message,length/name/prototype
true,true,EvalError,"",true,constructor/name/message,length/name/prototype
true,true,URIError,"",true,constructor/name/message,length/name/prototype
TypeError:m:truetruetrue:TypeError: m:stack/message
RangeError:m:truetruetrue:RangeError: m:stack/message
ReferenceError:m:truetruetrue:ReferenceError: m:stack/message
SyntaxError:m:truetruetrue:SyntaxError: m:stack/message
EvalError:m:truetruetrue:EvalError: m:stack/message
URIError:m:truetruetrue:URIError: m:stack/message
true
AggregateError||constructor,name,message|true
false,false,false
false,false,false
true,false,true
function0toString"#,
    )]);
}

#[test]
fn error_new_target() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
function Foo() {}
t(function () { var e = Reflect.construct(Error, ['m'], Foo); return (Object.getPrototypeOf(e) === Foo.prototype) + ',' + e.message + ',' + Object.getOwnPropertyNames(e).join(); });
t(function () { var F = function () {}; F.prototype = null; var e = Reflect.construct(TypeError, [], F); return Object.getPrototypeOf(e) === TypeError.prototype; });
t(function () { var F = function () {}; F.prototype = 5; var e = Reflect.construct(RangeError, [], F); return Object.getPrototypeOf(e) === RangeError.prototype; });
t(function () { var e = Reflect.construct(AggregateError, [[]], Foo); return Object.getPrototypeOf(e) === Foo.prototype; });
t(function () { var F = function () {}; F.prototype = Array.prototype; var e = Reflect.construct(Error, [], F); return Object.prototype.toString.call(e) + (Array.isArray(e)); });
t(function () { var log = []; var F = function () {}; Object.defineProperty(F, 'prototype', { get: function () { log.push('proto'); return Foo.prototype; } }); var m = { toString: function () { log.push('msg'); return 'x'; } }; Reflect.construct(Error, [m], F); return log.join(); });
t(function () { function F() {} var bound = Error.bind(null, 'bm'); var e = new bound(); return e.message + (e instanceof Error); });
t(function () { var e = Error.call({}, 'callm'); return e.message + (Object.getPrototypeOf(e) === Error.prototype); });
",
        r"true,m,stack,message
true
true
true
[object Error]false
TypeError: Cannot redefine property: prototype
bmtrue
callmtrue",
    )]);
}

#[test]
fn error_cause() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { var e = new Error('m', { cause: 'c' }); var d = Object.getOwnPropertyDescriptor(e, 'cause'); return [e.cause, d.writable, d.enumerable, d.configurable, Object.getOwnPropertyNames(e).join()].join('|'); });
t(function () { var e = new Error('m', { cause: undefined }); return ('cause' in e) + ',' + e.hasOwnProperty('cause'); });
t(function () { var e = new Error('m', {}); return 'cause' in e; });
t(function () { var e = new Error('m', 'cause'); return 'cause' in e; });
t(function () { var e = new Error('m', 5); return 'cause' in e; });
t(function () { var e = new Error('m', null); return 'cause' in e; });
t(function () { var proto = { cause: 'inherited' }; var e = new Error('m', Object.create(proto)); return e.hasOwnProperty('cause') + ',' + e.cause; });
t(function () { var log = []; var opts = {}; Object.defineProperty(opts, 'cause', { get: function () { log.push('get'); return 'g'; }, enumerable: true }); var e = new TypeError('m', opts); return log.join() + ',' + e.cause; });
t(function () { var log = []; var opts = {}; Object.defineProperty(opts, 'cause', { get: function () { log.push('get'); throw new RangeError('thrown'); } }); return new Error('m', opts); });
t(function () { var log = []; var m = { toString: function () { log.push('message'); return 'm'; } }; var opts = {}; Object.defineProperty(opts, 'cause', { get: function () { log.push('cause'); return 1; } }); new Error(m, opts); return log.join(); });
t(function () { var e = new Error(undefined, { cause: 1 }); return Object.getOwnPropertyNames(e).join(); });
t(function () { var e = Error('m', { cause: 'c' }); return e.cause; });
t(function () { return [RangeError, ReferenceError, SyntaxError, EvalError, URIError].map(function (C) { return new C('m', { cause: C.name }).cause; }).join(); });
t(function () { var e = new AggregateError([], 'm', { cause: 'ac' }); return e.cause + ',' + Object.getOwnPropertyNames(e).join(); });
t(function () { var e = new Error('m', { cause: 1 }); e.cause = 2; delete e.cause; return 'cause' in e; });
",
        r"c|true|false|true|stack,message,cause
true,true
false
false
false
false
true,inherited
get,g
RangeError: thrown
message,cause
stack,cause
c
RangeError,ReferenceError,SyntaxError,EvalError,URIError
ac,stack,message,cause,errors
false",
    )]);
}

#[test]
fn error_to_string() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
var ts = Error.prototype.toString;
t(function () { return ts.call({}); });
t(function () { return ts.call({ name: 'N' }); });
t(function () { return ts.call({ message: 'M' }); });
t(function () { return ts.call({ name: 'N', message: 'M' }); });
t(function () { return ts.call({ name: '', message: 'M' }); });
t(function () { return ts.call({ name: 'N', message: '' }); });
t(function () { return ts.call({ name: undefined, message: undefined }); });
t(function () { return ts.call({ name: null, message: null }); });
t(function () { return ts.call({ name: 5, message: true }); });
t(function () { return ts.call({ name: { toString: function () { return 'objname'; } }, message: [1, 2] }); });
t(function () { return ts.call(function f() {}); });
t(function () { return ts.call([]); });
t(function () { return ts.call(1); });
t(function () { return ts.call('str'); });
t(function () { return ts.call(undefined); });
t(function () { return ts.call(null); });
t(function () { return ts.call(Symbol()); });
t(function () { return ts.call({ name: Symbol() }); });
t(function () { return ts.call({ message: Symbol('s') }); });
t(function () { var log = []; var o = {}; Object.defineProperty(o, 'name', { get: function () { log.push('name'); return 'n'; } }); Object.defineProperty(o, 'message', { get: function () { log.push('message'); return 'm'; } }); return ts.call(o) + ',' + log.join(); });
t(function () { return String(new TypeError('tt')) + '|' + new Error('e') + '|' + (new RangeError() + '') + '|' + Object.create(TypeError.prototype); });
t(function () { var e = new Error('a'); e.name = 'Custom'; return String(e) + '|' + e; });
t(function () { var e = new Error('a'); Error.prototype.name = 'Changed'; var r = String(e); Error.prototype.name = 'Error'; return r; });
t(function () { return ts.call({ name: 'x\u0000y', message: 'l1\nl2' }).length; });
",
        r"Error
N
Error: M
N: M
M
N
Error
null: null
5: true
objname: 1,2
f
Error
TypeError: Method Error.prototype.toString called on incompatible receiver 1
TypeError: Method Error.prototype.toString called on incompatible receiver str
TypeError: Method Error.prototype.toString called on incompatible receiver undefined
TypeError: Method Error.prototype.toString called on incompatible receiver null
TypeError: Method Error.prototype.toString called on incompatible receiver Symbol()
TypeError: Cannot convert a Symbol value to a string
TypeError: Cannot convert a Symbol value to a string
n: m,name,message
TypeError: tt|Error: e|RangeError|TypeError
Custom: a|Custom: a
Changed: a
10",
    )]);
}

#[test]
fn aggregate_error() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { var e = new AggregateError([1, 2, 3], 'agg'); var d = Object.getOwnPropertyDescriptor(e, 'errors'); return [e.errors.join(), e.message, d.writable, d.enumerable, d.configurable, Array.isArray(e.errors), Object.getOwnPropertyNames(e).join()].join('|'); });
t(function () { var src = [1, 2]; var e = new AggregateError(src); return (e.errors !== src) + ',' + e.errors.length + ',' + e.hasOwnProperty('message'); });
t(function () { var e = AggregateError([], 'noNew'); return (e instanceof AggregateError) + ',' + e.message + ',' + String(e); });
t(function () { function* g() { yield 'a'; yield 'b'; } return new AggregateError(g()).errors.join(); });
t(function () { var it = { next: function () { return { done: true }; } }; var o = {}; o[Symbol.iterator] = function () { return it; }; return new AggregateError(o).errors.length; });
t(function () { var n = 0; var o = {}; o[Symbol.iterator] = function () { return { next: function () { n++; return n > 3 ? { done: true } : { value: n, done: false }; } }; }; return new AggregateError(o).errors.join(); });
t(function () { return new AggregateError(); });
t(function () { return new AggregateError(undefined, 'm'); });
t(function () { return new AggregateError(null); });
t(function () { return new AggregateError(5); });
t(function () { return new AggregateError({}); });
t(function () { return new AggregateError({ length: 1, 0: 'x' }); });
t(function () { var o = {}; o[Symbol.iterator] = 5; return new AggregateError(o); });
t(function () { var o = {}; o[Symbol.iterator] = function () { return 1; }; return new AggregateError(o); });
t(function () { var o = {}; o[Symbol.iterator] = function () { return {}; }; return new AggregateError(o); });
t(function () { var o = {}; o[Symbol.iterator] = function () { return { next: function () { return 1; } }; }; return new AggregateError(o); });
t(function () { var o = {}; o[Symbol.iterator] = function () { return { next: function () { throw new RangeError('nextthrow'); } }; }; return new AggregateError(o); });
t(function () { var o = {}; var closed = 0; o[Symbol.iterator] = function () { return { next: function () { throw new RangeError('nt'); }, return: function () { closed++; return {}; } }; }; try { new AggregateError(o); } catch (e) { return e.message + ',' + closed; } });
t(function () { var log = []; var o = {}; o[Symbol.iterator] = function () { log.push('iter'); return { next: function () { log.push('next'); return { done: true }; } }; }; var m = { toString: function () { log.push('msg'); return 'm'; } }; var c = {}; Object.defineProperty(c, 'cause', { get: function () { log.push('cause'); return 0; } }); new AggregateError(o, m, c); return log.join(); });
t(function () { var log = []; var o = {}; o[Symbol.iterator] = function () { log.push('iter'); return { next: function () { log.push('next'); return { done: true }; } }; }; var m = { toString: function () { log.push('msg'); throw new TypeError('msgthrow'); } }; try { new AggregateError(o, m); } catch (e) { return e.message + ',' + log.join(); } });
t(function () { var e = new AggregateError([new Error('i1'), new TypeError('i2')], 'two'); return e.errors.map(function (x) { return x.name + ':' + x.message; }).join() + '|' + String(e); });
t(function () { var F = function () {}; var e = Reflect.construct(AggregateError, [[1]], F); return (Object.getPrototypeOf(e) === F.prototype) + ',' + e.errors.length; });
t(function () { return new AggregateError([1, , 3]).errors.length + ',' + (1 in new AggregateError([1, , 3]).errors) + ',' + new AggregateError([1, , 3]).errors[1]; });
",
        r"1,2,3|agg|true|false|true|true|stack,message,errors
true,2,false
true,noNew,AggregateError: noNew
a,b
0
1,2,3
TypeError: undefined is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: undefined is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: object null is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: number 5 is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))
TypeError: Result of the Symbol.iterator method is not an object
TypeError: undefined is not a function
TypeError: Iterator result 1 is not an object
RangeError: nextthrow
nt,0
msg,cause,iter,next
msgthrow,msg
Error:i1,TypeError:i2|AggregateError: two
true,1
3,true,undefined",
    )]);
}

#[test]
fn error_aggregate_order_of_keys() {
    check_cases(&[(
        r"function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
t(function () { return Object.getOwnPropertyNames(new AggregateError([], 'm', { cause: 1 })).join(); });
t(function () { return Object.getOwnPropertyNames(new AggregateError([])).join(); });
t(function () { return Object.getOwnPropertyNames(new TypeError('m', { cause: 1 })).join(); });
",
        r"stack,message,cause,errors
stack,errors
stack,message,cause",
    )]);
}
