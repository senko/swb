// Iterator helpers (M7 6c) with allocations and collections inside the
// callbacks, inside `next` of user iterators, inside the inner iterables
// of `flatMap` and `Iterator.concat`, with helpers that only other helpers
// refer to. The expected output (iterators_gc.expected) is that of
// Node.js 22; `Iterator.concat` (Node.js 22 lacks it) has a polyfill that
// gives the same values.
var gc = typeof gc === 'function' ? gc : function () {};
function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
function junk(n) { var o = []; for (var i = 0; i < n; i++) o.push({ i: i, s: 'x' + i }); return o.length; }
if (!Iterator.concat) {
  Iterator.concat = function concat() {
    var items = Array.prototype.slice.call(arguments);
    return (function* () { for (var item of items) for (var v of item) yield v; })();
  };
}
function counter(n, extra) {
  var it = { i: 0, __proto__: Iterator.prototype };
  it.next = function () { gc(); junk(2); return this.i < n ? { value: { n: this.i++, s: 'item' + this.i }, done: false } : { done: true, value: undefined }; };
  it.return = function () { gc(); if (extra) extra.push('closed' + this.i); return {}; };
  return it;
}
// Callbacks that allocate and collect.
t(function () { var keys = []; for (var i = 0; i < 50; i++) keys.push(i); return keys.values().map(function (i) { gc(); return { i: i, s: 'v' + i }; }).filter(function (o) { gc(); return o.i % 3 === 0; }).map(function (o) { return o.s + o.i; }).toArray().join(); });
t(function () { var r = counter(30).map(function (o) { gc(); return { w: o, t: 'm' + o.n }; }).filter(function (m) { junk(3); return m.w.n % 2; }).take(8).toArray(); return r.length + ' ' + r[7].t + ' ' + r[7].w.s; });
t(function () { var r = counter(30).drop(10).map(function (o) { return o.s + '!'; }).drop(5).take(4); return r.toArray().join(); });
// The intermediate helpers are only referenced by the next helper.
t(function () { var out = []; var h = counter(20).map(function (o) { return { a: o }; }).filter(function (x) { gc(); return true; }).map(function (x) { return { b: x }; }); gc(); junk(50); gc(); for (var i = 0; i < 5; i++) out.push(h.next().value.b.a.s); return out.join(); });
// flatMap: inner iterables made in the callback, collections in `next`.
t(function () { return counter(6).flatMap(function (o) { gc(); return [o.s, o.s + 'b', { k: o.n }]; }).toArray().map(function (x) { return typeof x === 'string' ? x : 'o' + x.k; }).join(); });
t(function () { return counter(5).flatMap(function (o) { gc(); return new Set(['s' + o.n, 't' + o.n]).values(); }).toArray().join(); });
t(function () { return counter(4).flatMap(function (o) { return new Map([[o.n, 'm' + o.n], ['z', o.s]]).entries(); }).map(function (e) { gc(); return e.join(':'); }).toArray().join(); });
t(function () { var inner = function (n) { var i = 0; return { next: function () { gc(); junk(1); return i < 3 ? { value: n + '/' + i++, done: false } : { done: true }; } }; }; return counter(4).flatMap(function (o) { return inner(o.s); }).toArray().join(); });
t(function () { return counter(3).flatMap(function (o) { return counter(3).flatMap(function (p) { gc(); return [o.n * 10 + p.n]; }); }).toArray().join(); });
t(function () { var log = []; var h = counter(10, log).flatMap(function (o) { return counter(10, log); }); h.next(); h.next(); gc(); h.return(); return log.join(); });
// Closing: callbacks that throw after allocating, `return` that allocates.
t(function () { var log = []; try { counter(10, log).map(function (o) { gc(); if (o.n === 3) throw new Error('e' + o.s); return o; }).toArray(); } catch (e) { return e.message + ' ' + log.join(); } });
t(function () { var log = []; var r = counter(10, log).take(3).toArray(); return r.length + ' ' + log.join(); });
t(function () { var log = []; var h = counter(10, log).filter(function (o) { return o.n > 1; }); h.next(); gc(); h.return(); return log.join() + ' ' + JSON.stringify(h.next()); });
t(function () { var log = []; try { counter(10, log).forEach(function (o) { gc(); if (o.n === 2) throw { code: o.s }; }); } catch (e) { return e.code + ' ' + log.join(); } });
// Consuming methods.
t(function () { var r = counter(10).find(function (o) { gc(); return o.n === 4; }); junk(10); gc(); return r.s + ' ' + r.n; });
t(function () { return counter(10).some(function (o) { gc(); return o.n === 4; }) + ' ' + counter(10).every(function (o) { gc(); return o.n < 20; }); });
t(function () { var r = counter(8).reduce(function (acc, o) { gc(); junk(2); return { sum: acc.sum + o.n, last: o.s, prev: acc }; }, { sum: 0, last: '', prev: null }); return r.sum + ' ' + r.last + ' ' + r.prev.last; });
t(function () { var r = counter(8).reduce(function (acc, o) { gc(); return { v: (acc.v === undefined ? acc.n : acc.v) + o.n }; }); return r.v; });
t(function () { var a = counter(500).toArray(); return a.length + ' ' + a[499].s + ' ' + a[250].n; });
// Iterator.from and Iterator.concat.
t(function () { var w = Iterator.from({ i: 0, next: function () { gc(); return this.i < 5 ? { value: { n: this.i++ }, done: false } : { done: true }; } }); junk(20); return w.map(function (o) { gc(); return o.n * 2; }).toArray().join(); });
t(function () { return Iterator.concat([1, 2, 3], new Set([4, 5]), counter(3).map(function (o) { return o.n + 100; }), 'xyz'.split('')).map(function (v) { gc(); return v; }).toArray().join(); });
t(function () { var parts = []; for (var i = 0; i < 40; i++) parts.push(['p' + i, { q: i }]); var h = Iterator.concat.apply(null, parts); gc(); junk(30); gc(); var out = []; for (var v of h) { gc(); out.push(typeof v === 'string' ? v : 'q' + v.q); } return out.length + ' ' + out[0] + ' ' + out[79]; });
t(function () { var log = []; var h = Iterator.concat(counter(5, log), counter(5, log)); h.next(); h.next(); gc(); h.return(); return log.join() + ' ' + JSON.stringify(h.next()); });
t(function () { var h = Iterator.concat([1, 2], { [Symbol.iterator]: function () { gc(); return [3].values(); } }); gc(); return h.toArray().join(); });
// Collections inside callbacks.
t(function () { return [1, 2, 3, 4].values().map(function (x) { var m = new Map([[x, { v: x * 2 }]]); gc(); return m.get(x).v; }).toArray().join(); });
t(function () { var seen = new Set(); return counter(20).filter(function (o) { gc(); var k = o.n % 4; if (seen.has(k)) return false; seen.add(k); return true; }).map(function (o) { return o.s; }).toArray().join(); });
t(function () { var groups = new Map(); counter(12).forEach(function (o) { gc(); var k = 'g' + (o.n % 3); if (!groups.has(k)) groups.set(k, []); groups.get(k).push(o.s); }); return Array.from(groups.keys()).join() + ' ' + groups.get('g1').join(); });
t(function () { return new Map([['a', 1], ['b', 2], ['c', 3]]).entries().map(function (e) { gc(); return e[0] + e[1]; }).toArray().join() + ' ' + new Set([1, 2, 3]).values().drop(1).toArray().join(); });
t(function () { var m = new Map(); for (var i = 0; i < 20; i++) m.set('k' + i, i); var h = m.keys().filter(function (k) { gc(); if (k === 'k2') { m.delete('k3'); m.set('late', 99); } return true; }); return h.toArray().join(); });
// A helper over a generator and inside `for`-`of`.
t(function () { function* g() { for (var i = 0; i < 6; i++) { gc(); yield { i: i, s: 's' + i }; } } var out = []; for (var x of g().map(function (o) { return o.s; }).filter(function (s) { return s !== 's2'; })) { gc(); out.push(x); } return out.join(); });
t(function () { function* g() { try { for (var i = 0; ; i++) yield 'g' + i; } finally { print('fin'); } } var r = g().drop(2).take(3).toArray(); return r.join(); });
