// Generators and `yield*` (M7 7a) with allocations and collections inside
// the `next`, `throw` and `return` of user delegates, inside the getters of
// their result objects, and while a delegate, its `next` method or a
// received value is held only by a suspended generator. The expected output
// (generators_gc.expected) is that of Node.js 22 (`just jsdiff` compares).
var gc = typeof gc === 'function' ? gc : function () {};
function t(f) { try { print(f()); } catch (e) { print(e && e.constructor ? e.constructor.name + ': ' + e.message : 'threw ' + e); } }
function junk(n) { var o = []; for (var i = 0; i < n; i++) o.push({ i: i, s: 'x' + i }); return o.length; }
// A user delegate whose methods allocate and collect; `log` records calls.
function delegate(n, log, extra) {
  var it = { i: 0 };
  it[Symbol.iterator] = function () { gc(); return this; };
  it.next = function (v) { gc(); junk(3); log.push('next:' + (v && v.tag)); return this.i < n ? { value: { n: this.i++, s: 'v' + this.i }, done: false } : { value: { end: 'e' + n }, done: true }; };
  if (!extra || extra.throw !== false) it.throw = function (e) { gc(); log.push('throw:' + (e && e.tag)); return { value: { thrown: e.tag }, done: (extra && extra.throwDone) || false }; };
  if (!extra || extra.return !== false) it.return = function (v) { gc(); junk(2); log.push('return:' + (v && v.tag)); return { value: { returned: v && v.tag }, done: true }; };
  return it;
}
// Values through yield* from a user delegate, with fresh received values.
t(function () { var log = []; function* g() { var r = yield* delegate(4, log); gc(); yield r.end; } var it = g(); var out = []; var x = it.next({ tag: 'first' }); while (!x.done) { gc(); out.push(x.value.s || x.value); x = it.next({ tag: 't' + out.length }); } return out.join() + ' ' + log.join(); });
// The delegate is only held by the suspended generator.
t(function () { var log = []; function* g() { yield* delegate(3, log); } var it = g(); it.next(); gc(); junk(100); gc(); var a = it.next({ tag: 'a' }); gc(); var b = it.next({ tag: 'b' }); return a.value.s + b.value.s + ' ' + log.join(); });
// The `next` method is only held by the generator (the delegate's own `next` was replaced).
t(function () { var log = []; var d = delegate(3, log); function* g() { yield* d; } var it = g(); it.next(); d.next = null; gc(); junk(50); var r = it.next({ tag: 'x' }); return r.value.s + ' ' + log.join(); });
// throw and return forwarded to a delegate that allocates.
t(function () { var log = []; function* g() { try { var r = yield* delegate(5, log, { throwDone: true }); yield r.thrown; } finally { gc(); log.push('fin'); } } var it = g(); it.next(); var x = it.throw({ tag: 'E' }); gc(); return x.value + ' ' + log.join(); });
t(function () { var log = []; function* g() { try { yield* delegate(5, log); } finally { gc(); junk(10); log.push('fin'); } } var it = g(); it.next(); var x = it.return({ tag: 'R' }); gc(); return JSON.stringify(x) + ' ' + log.join(); });
// A delegate without `throw`: close, then the TypeError.
t(function () { var log = []; function* g() { try { yield* delegate(5, log, { throw: false }); } catch (e) { gc(); return e.message + ' ' + log.join(); } } var it = g(); it.next(); return it.throw({ tag: 'E' }).value; });
// A delegate without `return`: the outer generator returns the received value.
t(function () { var log = []; function* g() { yield* delegate(5, log, { return: false }); } var it = g(); it.next(); var v = { tag: 'R', payload: [1, 2, 3] }; var x = it.return(v); v = null; gc(); return x.value.tag + x.value.payload.join() + ' ' + x.done + ' ' + log.join(); });
// Result objects with getters that allocate and collect.
t(function () { var n = 0; var it = { [Symbol.iterator]() { return this; }, next() { var k = n++; return { get done() { gc(); junk(5); return k >= 3; }, get value() { gc(); return { k: k }; } }; } }; function* g() { var r = yield* it; return r; } var out = []; for (var x of g()) out.push(typeof x.k); var last = g(); return out.join() + ' ' + n; });
t(function () { var it = { [Symbol.iterator]() { return this; }, next() { return { get done() { gc(); return true; }, get value() { gc(); junk(20); return { w: 'done value' }; } }; } }; function* g() { return yield* it; } var r = g().next(); gc(); return r.value.w + ' ' + r.done; });
// A chain of generators with collections at every level.
t(function () { function* leaf() { for (var i = 0; i < 5; i++) { gc(); yield { i: i, s: 'l' + i }; } return { r: 'leaf' }; } function* mid(k) { var r = yield* (k ? mid(k - 1) : leaf()); gc(); junk(3); return { r: r.r + k }; } var it = mid(10); var out = []; var x; while (!(x = it.next()).done) out.push(x.value.s); return out.join() + ' ' + x.value.r; });
t(function () { function* leaf() { try { yield 1; yield 2; } finally { gc(); } } function* mid(k) { try { yield* (k ? mid(k - 1) : leaf()); } finally { gc(); junk(2); } } var it = mid(20); it.next(); gc(); return JSON.stringify(it.return({ big: new Array(100).fill('z').join('') }).value.big.length); });
t(function () { function* leaf() { try { yield 1; } catch (e) { gc(); return { caught: e.v }; } } function* mid(k) { return yield* (k ? mid(k - 1) : leaf()); } var it = mid(30); it.next(); gc(); var e = { v: 'deep' }; var x = it.throw(e); e = null; gc(); return x.value.caught + ' ' + x.done; });
// The result object passes through unchanged.
t(function () { var keep = []; var it = { [Symbol.iterator]() { return this; }, next() { var r = { done: false, value: keep.length }; keep.push(r); gc(); return r; } }; function* g() { yield* it; } var o = g(); var a = o.next(); gc(); var b = o.next(); return (a === keep[0]) + ' ' + (b === keep[1]); });
// GeneratorFunction with collections.
t(function () { var GF = Object.getPrototypeOf(function* () {}).constructor; var f = new GF('a', 'b', 'gc(); var x = yield* a; yield x; yield* b;'); gc(); return [...f(function* () { gc(); yield 1; return 'r'; }(), [2, 3])].join(); });
// Built-in delegates whose state is only in the suspended generator.
t(function () { function* g() { yield* new Map([[1, { a: 1 }], [2, { a: 2 }]]); yield* new Set(['s1', 's2']).values(); yield* [{ q: 1 }].values().map(function (o) { gc(); return o.q; }); } var it = g(); var out = []; for (var i = 0; i < 6; i++) { gc(); junk(10); var x = it.next(); out.push(JSON.stringify(x.value)); } return out.join(' '); });
t(function () { function* g() { yield* 'a😀b'; } var it = g(); it.next(); gc(); return it.next().value.length + ' ' + it.next().value; });
// Many suspended generators with delegates.
t(function () { var gens = []; function* inner(k) { yield { k: k }; yield { k: -k }; } function* outer(k) { yield* inner(k); } for (var i = 0; i < 200; i++) { var g = outer(i); g.next(); gens.push(g); } gc(); junk(500); gc(); var sum = 0; for (var g of gens) sum += g.next().value.k; return sum; });
// Re-entrant next through a delegate.
t(function () { var o; var it = { [Symbol.iterator]() { return this; }, next() { gc(); try { o.next(); } catch (e) { return { done: true, value: e.message }; } } }; function* g() { return yield* it; } o = g(); return o.next().value; });
