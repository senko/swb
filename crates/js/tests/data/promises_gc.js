// Promises and jobs (M7 7b) with allocations and collections inside
// executors, handlers, thenables, `finally` callbacks and the combinators,
// while the only references to promises, handlers and values are in the
// job queue or in the reactions of pending promises. The output is printed
// from the jobs, so it also shows their order. The expected output
// (promises_gc.expected) is that of Node.js 22 (`just jsdiff` compares).
var gc = typeof gc === 'function' ? gc : function () {};
// Node.js 22 has no `Promise.try`: the same steps for these cases.
if (!Promise.try) Promise.try = function (f) { var args = [].slice.call(arguments, 1); return new this(function (r) { r(f.apply(undefined, args)); }); };
function junk(n) { var o = []; for (var i = 0; i < n; i++) o.push({ i: i, s: 'x' + i }); return o.length; }
function show(e) { return e && e.constructor ? e.constructor.name + ': ' + e.message : 'value ' + e; }

// A chain whose handlers allocate and collect; only the queue holds the
// promises after the script.
(function () {
  var p = Promise.resolve({ n: 0, s: 's0' });
  for (var i = 0; i < 20; i++) {
    p = p.then(function (v) { gc(); junk(5); return { n: v.n + 1, s: 's' + (v.n + 1) }; });
  }
  p.then(function (v) { gc(); print('chain', v.n, v.s); });
})();

// Handlers held only by the reactions of a pending promise.
var resolveLater;
(function () {
  var pending = new Promise(function (r) { resolveLater = r; });
  for (var i = 0; i < 5; i++) {
    (function (k) { pending.then(function (v) { gc(); print('pending', k, v.tag); }); })(i);
  }
})();
gc();
junk(100);
Promise.resolve().then(function () { gc(); resolveLater({ tag: 'late' }); });

// Thenables that allocate in `then` and resolve later.
(function () {
  var thenable = { then: function (f, r) { gc(); junk(10); Promise.resolve().then(function () { gc(); f({ v: 'thenable' }); }); } };
  Promise.resolve(thenable).then(function (x) { gc(); print('thenable', x.v); });
  var getter = {};
  Object.defineProperty(getter, 'then', { get: function () { gc(); return function (f) { f('from getter'); }; } });
  new Promise(function (r) { gc(); r(getter); }).then(function (x) { print('getter', x); });
  var throwing = { get then() { gc(); throw new Error('then getter'); } };
  Promise.resolve(throwing).catch(function (e) { gc(); print('rejected', show(e)); });
})();

// A promise resolved with a promise resolved with ... (the thenable jobs).
(function () {
  var p = Promise.resolve('deep');
  for (var i = 0; i < 30; i++) { (function (q) { p = new Promise(function (r) { gc(); r(q); }); })(p); }
  p.then(function (v) { gc(); print('nested', v); });
})();

// finally with collections, values passed through and overridden.
Promise.resolve({ keep: 'kept' }).finally(function () { gc(); return junk(3); }).then(function (v) { print('finally value', v.keep); });
Promise.reject(new Error('boom')).finally(function () { gc(); }).catch(function (e) { gc(); print('finally reason', show(e)); });
Promise.resolve(1).finally(function () { gc(); throw new TypeError('in finally'); }).catch(function (e) { print('finally threw', show(e)); });

// The combinators with handlers and element values that allocate.
(function () {
  var items = [];
  for (var i = 0; i < 10; i++) {
    (function (k) { items.push(k % 3 === 0 ? Promise.resolve({ k: k }) : k % 3 === 1 ? { then: function (f) { gc(); f({ k: k }); } } : { k: k }); })(i);
  }
  Promise.all(items).then(function (vs) { gc(); print('all', vs.map(function (v) { return v.k; }).join()); });
  Promise.allSettled(items.concat([Promise.reject(new RangeError('r'))])).then(function (vs) { gc(); print('allSettled', vs.map(function (v) { return v.status[0]; }).join('')); });
  Promise.any([Promise.reject(1), Promise.reject(2)]).catch(function (e) { gc(); print('any', show(e), e.errors.join()); });
  Promise.race([new Promise(function () {}), Promise.resolve('race')]).then(function (v) { gc(); print('race', v); });
})();

// Subclasses whose constructor allocates and collects.
(function () {
  class P extends Promise { constructor(e) { gc(); junk(2); super(e); } }
  P.resolve(5).then(function (v) { return v * 2; }).finally(function () { gc(); }).then(function (v) { print('subclass', v); });
  P.all([1, P.resolve(2)]).then(function (v) { print('subclass all', v.join()); });
  P.try(function (a) { gc(); return a + 1; }, 41).then(function (v) { print('subclass try', v); });
  var w = P.withResolvers();
  w.promise.then(function (v) { print('withResolvers', v.s); });
  Promise.resolve().then(function () { gc(); w.resolve({ s: 'resolved' }); });
})();

// Self-resolution and handlers attached after the rejection.
(function () {
  var r;
  var p = new Promise(function (res) { r = res; });
  r(p);
  Promise.resolve().then(function () { gc(); return p.catch(function (e) { print('cycle', show(e)); }); });
})();

// A job loop that enqueues while it runs, with garbage in each step.
(function () {
  var n = 0;
  function step() { junk(4); if (++n < 200) Promise.resolve({ n: n }).then(step); else print('loop', n); }
  step();
})();
print('script end');
