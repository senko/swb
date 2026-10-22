//! Native functions that call script functions (M7 13c, ADR 0026
//! sections 5 and 15): the array and typed-array callback methods, `sort`
//! with a comparator and `replace` with a function run as native loops
//! whose calls are frames of the interpreter loop (`vm/native_loop.rs`);
//! `for`-`of` over generators and built-in iterators takes no result
//! objects; proxy traps, iterator helpers and promise reactions, which
//! still re-enter, run inside such loops. The order of the observable
//! steps, the exceptions, the recursion limit and re-entrancy must be as
//! before.
//! Every case runs normally and in the GC stress mode. The expected
//! outputs come from Node.js 22 (a black box), with `print` for
//! `console.log`.

mod common;

use std::time::{Duration, Instant};

use common::{check_cases, on_big_stack, runtime};
use swb_js::{ScriptError, Termination};

/// An exception in the middle of a callback loop (`forEach`, `sort`,
/// `reduce`, `map`, typed `map`, `replace`) ends the loop with it, and a
/// handler inside the callback keeps the loop going; the stack trace
/// shows the callback and its caller (native frames are not shown, as
/// before 13c; V8 shows `at Array.forEach`).
#[test]
fn exceptions_in_callbacks() {
    check_cases(EXCEPTIONS);
}

/// Callbacks that change the array during the loop: the loop reads
/// the elements again at each index (`HasProperty`, `Get`), holes on the
/// prototype, array-likes, sparse arrays and getters.
#[test]
fn mutation() {
    check_cases(MUTATION);
}

/// A callback that calls the same native again, natives as callbacks
/// (`indexOf`, `Number`, bound `apply`, a bound `forEach`), and proxy
/// traps that run callbacks.
#[test]
fn reentrancy() {
    check_cases(REENTRANCY);
}

/// Natives as callbacks that call script code themselves (a bound
/// `sort` whose comparator throws, the `finally` closure of a promise,
/// a bound `call`) and their exceptions.
#[test]
fn native_callees() {
    check_cases(NATIVE_CALLEES);
}

/// Deep recursion through callbacks (`forEach`, `map`, `replace`, a
/// proxy trap, a comparator) ends with the `RangeError` of the stack
/// limit, and the engine works afterwards.
#[test]
fn recursion() {
    check_cases(RECURSION);
}

/// Generator and async functions as callbacks, and generator
/// methods (`next`) as callbacks.
#[test]
fn generators_async() {
    check_cases(GENERATORS_ASYNC);
}

/// `for`-`of` over generators with the built-in `next` and over
/// built-in iterators: patched `next` methods of the prototypes and own
/// ones are seen, `return` runs on an early exit, `yield*` passes result
/// objects of user iterators with their getters through.
#[test]
fn iteration() {
    check_cases(ITERATION);
}

/// Iterator helpers over user iterators and promise reactions.
#[test]
fn helpers_promises() {
    check_cases(HELPERS_PROMISES);
}

const EXCEPTIONS: &[(&str, &str)] = &[
    (
        r"var seen = [];
try { [1, 2, 3, 4].forEach(function (x) { seen.push(x); if (x === 3) throw new Error('stop at ' + x); }); }
catch (e) { print(e.message, seen.join()); }
try { [5, 1, 4, 2, 3].sort(function (a, b) { if (a === 4 || b === 4) throw new RangeError('cmp'); return a - b; }); }
catch (e) { print(e.name, e.message); }
var arr = [5, 1, 4]; try { arr.sort(function () { throw 1; }); } catch (e) { print('thrown', e, arr.join()); }
try { [1, 2, 3].reduce(function (a, b) { if (b === 3) throw 'r' + a; return a + b; }); } catch (e) { print(e); }
try { [1, 2].map(function () { null.x; }); } catch (e) { print(e.constructor.name); }
try { new Int8Array([1, 2]).map(function (x) { if (x === 2) throw 'typed'; return x; }); } catch (e) { print(e); }
try { 'abcabc'.replace(/b/g, function () { throw 'repl'; }); } catch (e) { print(e); }
function inner() { [1].forEach(function () { throw new Error('deep'); }); }
try { inner(); } catch (e) { print(e.stack.split('\n').length > 1, e.message); }
void 0;",
        r"stop at 3 1,2,3
RangeError cmp
thrown 1 5,1,4
r3
TypeError
typed
repl
true deep",
    ),
    (
        r"function cb(x) { if (x === 2) throw new Error('boom'); }
function outer() { [1, 2, 3].forEach(cb); }
try { outer(); } catch (e) { print(e.stack.split('\n').slice(0, 3).map(function (l) { return l.trim().split(' (')[0]; }).join(' | ')); }
void 0;",
        r"Error: boom | at cb | at outer",
    ),
    (
        r"var r = [];
[1, 2, 3].forEach(function (x) { try { if (x === 2) throw x; r.push(x); } catch (e) { r.push('c' + e); } });
print(r.join());
var s = [3, 2, 1].map(function (x) { try { return [x].map(function () { throw x; }); } catch (e) { return -e; } });
print(s.join());
void 0;",
        r"1,c2,3
-3,-2,-1",
    ),
];

const MUTATION: &[(&str, &str)] = &[
    (
        r"var a = [1, 2, 3];
a.forEach(function (x, i, arr) { if (i === 0) { arr.push(4); arr[2] = 30; } print(i, x); });
var b = [1, 2, 3, 4, 5];
print(b.filter(function (x, i, arr) { if (i === 1) arr.length = 3; return true; }).join());
var c = [1, , 3];
print(c.map(function (x, i) { if (i === 0) c[1] = 2; return x * 2; }).join());
var d = [1, 2, 3];
print(d.reduceRight(function (acc, x, i, arr) { if (i === 2) arr.length = 1; return acc + x; }, 0));
var e = [1, 2, 3];
print(e.some(function (x, i, arr) { arr.shift(); return x === 3; }), e.join());
var f = [3, 1, 2];
f.sort(function (x, y) { f.push(9); return x - y; }); print(f.length >= 3, f.slice(0, 3).join());
var g = [1, 2, 3, 4];
print(g.findLast(function (x, i, arr) { if (i === 3) arr[0] = 'z'; return typeof x === 'string'; }));
void 0;",
        r"0 1
1 2
2 30
1,2,3
2,4,6
4
true 3
true 1,2,3
z",
    ),
    (
        r"var proto = [, 'from proto'];
var h = [1, , 3]; Object.setPrototypeOf(h, proto);
h.forEach(function (x, i) { print(i, x); });
var o = { length: 3, 0: 'a', 2: 'c' };
Array.prototype.forEach.call(o, function (x, i) { print(i, x); });
var sparse = []; sparse[5] = 'five'; sparse[1000000] = 'm';
var visits = 0; sparse.forEach(function () { visits++; }); print(visits);
print([, , 7].find(function (x) { return x === undefined; }), [, , 7].findIndex(function (x) { return x === 7; }));
var getterArr = [1, 2, 3]; Object.defineProperty(getterArr, 1, { get: function () { print('get 1'); return 'two'; } });
print(getterArr.map(function (x) { return x; }).join());
void 0;",
        r"0 1
1 from proto
2 3
0 a
2 c
2
undefined 2
get 1
1,two,3",
    ),
];

const REENTRANCY: &[(&str, &str)] = &[
    (
        r"var log = [];
[1, 2].forEach(function (x) { [10, 20].forEach(function (y) { log.push(x + y); }); });
print(log.join());
print([[3, 1], [2, 0]].map(function (p) { return p.slice().sort(function (a, b) { return a - b; }).join('<'); }).join());
var depth = 0;
function rec(n) { if (n === 0) return 0; return [n].reduce(function (a, b) { return a + b + rec(n - 1); }, 0); }
print(rec(50));
print([1, 2, 3].map(Array.prototype.indexOf, [3, 2, 1]).join());
print(['1', '2', '3'].map(Number).join(), ['a', 'b'].map(String.prototype.toUpperCase.call, String.prototype.toUpperCase).join());
print([[1, 2], [3]].map(Function.prototype.apply.bind(Math.max, null)).join());
var bound = [].forEach.bind([1, 2]);
[0].forEach(function () { bound(function (x) { print('bound', x); }); });
print([1, 2].map(Array.prototype.map.bind([5, 6], function (v) { return v * 2; })).join('|'));
void 0;",
        r"11,21,12,22
1<3,0<2
1275
2,1,-1
1,2,3 A,B
2,3
bound 1
bound 2
10,12|10,12",
    ),
    (
        r"var it = [1, 2, 3];
it.forEach(function (x) { if (x === 1) it.forEach(function (y) { print('in', y); }); print('out', x); });
var calls = 0;
var p = new Proxy({}, { get: function (t, k) { calls++; return k === 'x' ? [1, 2].map(function (v) { return v * 10; }).join() : undefined; } });
print(p.x, p.y, calls);
var nested = new Proxy({}, { get: function (t, k) { return new Proxy({}, { get: function () { return k + '!'; } }).z; } });
print(nested.a, nested.b);
void 0;",
        r"in 1
in 2
in 3
out 1
out 2
out 3
10,20 undefined 2
a! b!",
    ),
];

const NATIVE_CALLEES: &[(&str, &str)] = &[(
    r"try { [1].forEach(Array.prototype.sort.bind([2, 1], function () { throw new Error('in sort'); })); }
catch (e) { print('caught', e.message); }
var order = [];
[1, 2].forEach(function (x) { order.push(x); });
Promise.resolve(1).finally(function () { throw 3; }).catch(function (e) { print('finally threw', e); });
[1, 2].forEach(Function.prototype.call.bind(function (x) { order.push('c' + x); }));
print(order.join());
try { ['{', '[1]'].forEach(JSON.parse); } catch (e) { print(e.name); }
print(['[1]', '[2]'].map(JSON.parse).map(function (a) { return a[0]; }).join());
try { [1].map(function () { return [2].map(function () { throw new TypeError('two levels'); }); }); } catch (e) { print(e.message); }
void 0;",
    r"caught in sort
1,2,c0,c1
SyntaxError
1,2
two levels
finally threw 3",
)];

const RECURSION: &[(&str, &str)] = &[(
    r"function f(n) { [1].forEach(function () { f(n + 1); }); }
try { f(0); } catch (e) { print(e.constructor.name, e.message); }
function g(n) { return [n].map(function (x) { return g(x + 1); }); }
try { g(0); } catch (e) { print(e.constructor.name, e.message); }
function h(n) { return 'x'.replace(/x/, function () { return h(n + 1); }); }
try { h(0); } catch (e) { print(e.constructor.name, e.message); }
var p = new Proxy({}, { get: function (t, k) { return p[k]; } });
try { p.x; } catch (e) { print(e.constructor.name, e.message); }
function s(n) { return [2, 1].sort(function (a, b) { s(n + 1); return a - b; }); }
try { s(0); } catch (e) { print(e.constructor.name, e.message); }
print('after', [3, 1, 2].sort(function (a, b) { return a - b; }).join());
void 0;",
    r"RangeError Maximum call stack size exceeded
RangeError Maximum call stack size exceeded
RangeError Maximum call stack size exceeded
RangeError Maximum call stack size exceeded
RangeError Maximum call stack size exceeded
after 1,2,3",
)];

const GENERATORS_ASYNC: &[(&str, &str)] = &[
    (
        r"function* gen(x) { yield x; yield x * 2; }
var made = [1, 2].map(gen);
print(made.map(function (g) { return [...g].join('/'); }).join());
async function af(x) { await null; return x * 3; }
var ps = [1, 2].map(af);
print(ps.length, ps[0] instanceof Promise);
Promise.all(ps).then(function (v) { print('async', v.join()); });
[1, 2].forEach(async function (x) { await 0; print('later', x); });
print('sync end');
var g2 = (function* () { var got = yield 1; print('got', got); yield 2; })();
print([5, 6].map(g2.next, g2).map(function (r) { return r.value + ':' + r.done; }).join());
void 0;",
        r"1/2,2/4
2 true
sync end
got 6
1:false,2:false
later 1
later 2
async 3,6",
    ),
    (
        r"var log = [];
async function* ag() { yield 1; yield 2; }
(async function () { for await (var v of ag()) log.push(v); log.push('done'); print(log.join()); })();
Promise.resolve().then(function () { return [1, 2].map(async function (x) { return x; }); }).then(function (a) { print(a.length); });
async function thrower() { await null; throw new Error('async boom'); }
thrower().catch(function (e) { print(e.message); });
Promise.reject(1).then(null, function (e) { return [e, e + 1].map(function (x) { return x * 2; }); }).then(function (v) { print(v.join()); });
void 0;",
        r"2
async boom
2,4
1,2,done",
    ),
];

const ITERATION: &[(&str, &str)] = &[
    (
        r"function* count(n) { try { for (var i = 0; i < n; i++) yield i; return 'end'; } finally { print('cleanup', n); } }
var s = 0; for (var v of count(4)) s += v; print('sum', s);
for (var v of count(5)) { if (v === 1) break; }
try { for (var v of count(3)) { if (v === 2) throw new Error('loop'); } } catch (e) { print(e.message); }
var it = count(3); it.next(); for (var v of it) print('rest', v);
var GP = Object.getPrototypeOf(Object.getPrototypeOf(count(0)));
var next = GP.next;
GP.next = function () { print('patched next'); return next.apply(this, arguments); };
for (var v of count(1)) print('patched value', v);
GP.next = next;
var own = count(2); own.next = function () { return { done: true }; };
for (var v of own) print('never');
var AIP = Object.getPrototypeOf([][Symbol.iterator]());
var anext = AIP.next;
AIP.next = function () { var r = anext.call(this); if (!r.done) r.value = r.value * 100; return r; };
for (var v of [1, 2]) print('array', v);
AIP.next = anext;
var MIP = Object.getPrototypeOf(new Map()[Symbol.iterator]());
var mnext = MIP.next;
MIP.next = function () { var r = mnext.call(this); if (!r.done) r.value = 'm' + r.value; return r; };
for (var e of new Map([[1, 2]])) print('map', e);
MIP.next = mnext;
var SIP = Object.getPrototypeOf(''[Symbol.iterator]());
var snext = SIP.next;
SIP.next = function () { var r = snext.call(this); if (!r.done) r.value += '!'; return r; };
for (var c of 'ab') print('string', c);
SIP.next = snext;
for (var c of 'x😀y') print(c.length);
for (var m of 'a1b22'.matchAll(/\d+/g)) print('match', m[0], m.index);
void 0;",
        r"cleanup 4
sum 6
cleanup 5
cleanup 3
loop
rest 1
rest 2
cleanup 3
patched next
patched value 0
patched next
cleanup 1
array 100
array 200
map m1,2
string a!
string b!
1
2
1
match 1 1
match 22 3",
    ),
    (
        r"var closed = [];
function iterable(name) { var i = 0; return { [Symbol.iterator]: function () { return this; }, next: function () { i++; return { value: i, done: i > 3 }; }, return: function () { closed.push(name); return {}; } }; }
for (var v of iterable('break')) { if (v === 2) break; }
try { for (var v of iterable('throw')) { throw 0; } } catch (e) {}
var [a, b] = iterable('destructure');
(function () { for (var v of iterable('return')) return v; })();
print(closed.join(), a, b);
function* inner() { try { yield 1; yield 2; } finally { print('inner closed'); } }
function* outer() { yield* inner(); }
for (var v of outer()) { print(v); break; }
var results = [];
var r1 = { value: 'v', done: false };
var custom = { [Symbol.iterator]: function () { var n = 0; return { next: function () { n++; return n < 3 ? { get value() { results.push('value' + n); return n; }, get done() { results.push('done' + n); return false; } } : { done: true }; } }; } };
for (var v of custom) results.push('body' + v);
print(results.join());
function* d() { yield* custom; }
results = []; for (var v of d()) results.push('d' + v); print(results.join());
void 0;",
        r"break,throw,destructure,return 1 2
1
inner closed
done1,value1,body1,done2,value2,body2
done1,done1,value1,d1,done2,done2,value2,d2",
    ),
];

const HELPERS_PROMISES: &[(&str, &str)] = &[(
    r"var i = 0;
var user = { next: function () { i++; return { value: i, done: i > 3 }; }, return: function () { print('user return'); return {}; } };
print(Iterator.from(user).map(function (x) { return x * 2; }).toArray().join());
i = 0;
try { Iterator.from(user).map(function (x) { if (x === 2) throw 'mapper'; return x; }).toArray(); } catch (e) { print('caught', e); }
print([1, 2, 3, 4].values().filter(function (x) { return x % 2; }).map(function (x) { return x * 10; }).reduce(function (a, b) { return a + b; }, 0));
print([1, 2, 3].values().flatMap(function (x) { return [x, -x]; }).take(4).toArray().join());
var log = [];
Promise.resolve(1).then(function (v) { log.push('a' + v); return v + 1; }).then(function (v) { log.push('b' + v); throw v; }).catch(function (e) { log.push('c' + e); }).finally(function () { print(log.join()); });
new Promise(function (res) { res({ then: function (r) { r('thenable'); } }); }).then(print);
void 0;",
    r"2,4,6
user return
caught mapper
40
1,-1,2,-2
thenable
a1,b2,c2",
)];

/// A time limit inside a callback loop ends the script; the loop's
/// record and its registers go, and the runtime runs the next script.
#[test]
fn time_limit_inside_callback_loops() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        for source in [
            "var a = []; for (var i = 0; i < 100; i++) a.push(i); a.forEach(function () { for (;;) {} });",
            "var big = new Array(1e7).fill(1); for (;;) big.forEach(function (x) { return x; });",
            "for (;;) [3, 1, 2].sort(function (a, b) { return a - b; });",
            "Promise.resolve().then(function f() { return Promise.resolve().then(f); }); undefined",
        ] {
            let start = Instant::now();
            rt.set_deadline(Some(start + Duration::from_millis(100)));
            let result = rt.eval(source).and_then(|_| rt.run_jobs());
            rt.set_deadline(None);
            assert_eq!(
                result,
                Err(ScriptError::Terminated(Termination::TimeLimit)),
                "{source}"
            );
            assert!(start.elapsed() < Duration::from_secs(2), "{source}");
            assert_eq!(rt.stack_depths(), (0, 0), "{source}");
            assert_eq!(rt.heap().scope_depth(), 0, "{source}");
            rt.clear_jobs();
        }
        let value = rt.eval("[1, 2, 3].map(function (x) { return x * 2; }).join()");
        assert_eq!(rt.display(value.unwrap()), "2,4,6");
    });
}
