//! Error message texts that quote script strings, and `console.log` of
//! the prototype objects and tagged values (M7 feature 2b review fixes).
//! The expected output is the output of Node.js 22, except where a
//! comment says otherwise. Each case also runs in the GC stress mode.

mod common;

use common::check_cases;

#[test]
fn long_strings_in_messages() {
    // Node.js keeps the first 100 units of a string in a "... is not a
    // function" message and appends `<...>`. It does not cut the property
    // names in the other messages; swb bounds them at 1024 units (the last
    // line prints `true`, Node prints `false`).
    check_cases(&[(
        r"function rep(t, n) { var r = ''; for (var i = 0; i < n; i++) r += t; return r; }
function msg(f) { try { f(); } catch (e) { return e.message; } }
var s = rep('abcdefghij', 30) + 'XYZ';
var m = msg(function () { [1].map(s); });
print(m.length, m);
m = msg(function () { Object.fromEntries({ [Symbol.iterator]() { return { next: s }; } }); });
print(m.length, m);
var huge = 'a';
for (var i = 0; i < 22; i++) huge += huge;
print(huge.length, msg(function () { [1].map(huge); }).length);
print(msg(function () { [1].map(rep('b', 100)); }).length, msg(function () { [1].map(rep('b', 101)); }).length);
var big = rep('k', 5000);
print(msg(function () { 'use strict'; big.foo = 1; }).length < 1100);",
        r#"132 string "abcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghij<...>" is not a function
132 string "abcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghij<...>" is not a function
4194304 132
127 132
true"#,
    )]);
}

#[test]
fn symbol_primitive_write_message() {
    check_cases(&[(
        r"function msg(f) { try { f(); } catch (e) { return e.message; } }
print(msg(function () { 'use strict'; Symbol('d').x = 1; }));
print(msg(function () { 'use strict'; Symbol().x = 1; }));",
        "Cannot create property 'x' on symbol 'Symbol(d)'\nCannot create property 'x' on symbol 'Symbol()'",
    )]);
}

#[test]
fn console_prototypes_and_tags() {
    check_cases(&[(
        r"function f() {}
Object.defineProperty(f, Symbol.toStringTag, { value: 'Q' });
var a = [1];
Object.defineProperty(a, Symbol.toStringTag, { value: 'Q' });
function A() {}
A.prototype = Object.create(Array.prototype);
A.prototype.constructor = A;
console.log(Boolean.prototype, Number.prototype, String.prototype, Symbol.prototype);
console.log(f, a, Object.setPrototypeOf([1], null), Object.setPrototypeOf([1, 2], A.prototype));
console.log(Object.create(Boolean.prototype), new Boolean(true));",
        "{} {} {} Object [Symbol] {}\n[Function: f] [Q] Array(1) [Q] [ 1 ] [Array(1): null prototype] [ 1 ] A(2) [ 1, 2 ]\nBoolean {} [Boolean: true]",
    )]);
}
