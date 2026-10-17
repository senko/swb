// Async functions (M7 7c) that allocate and collect while they wait at an
// `await`: their registers, `arguments`, captured cells, `finally` state,
// iterators of `for`-`of` and the values in flight are then held only by
// the suspended frame, which only the reaction on the awaited promise (or
// the job queue) holds. The output is printed from the jobs, so it also
// shows their order. The expected output (async_gc.expected) is that of
// Node.js 22 (`just jsdiff` compares).
var gc = typeof gc === 'function' ? gc : function () {};
function junk(n) { var o = []; for (var i = 0; i < n; i++) o.push({ i: i, s: 'x' + i }); return o.length; }
function show(e) { return e && e.constructor ? e.constructor.name + ': ' + e.message : 'value ' + e; }

// Registers and a captured cell across awaits; only the queue holds the
// frame after the script.
(async function locals(a, b) {
  var o = { tag: 'local' + a };
  var count = 0;
  var bump = function () { count++; return count; };
  for (var i = 0; i < 10; i++) {
    await { then: function (f) { gc(); junk(5); f(i); } };
    bump();
    gc();
  }
  print('locals', o.tag, count, arguments.length, arguments[1], b.s);
})(1, { s: 'second' });

// A frame that waits on a promise that the script resolves later; the
// promise is held only by a closure in a global.
var resolveLater;
(async function waiting() {
  var kept = { v: 'kept value' };
  var got = await new Promise(function (r) { resolveLater = r; });
  gc();
  print('waiting', kept.v, got.tag);
})();
gc();
junk(100);
Promise.resolve().then(function () { gc(); resolveLater({ tag: 'late' }); });

// `finally` with an await in it, and a rejection caught after a
// collection.
(async function cleanup() {
  var log = [];
  try {
    try {
      await null;
      gc();
      throw new Error('inner ' + junk(3));
    } finally {
      log.push('finally');
      await { then: function (f) { gc(); f(); } };
      log.push('after await in finally');
    }
  } catch (e) {
    gc();
    log.push(show(e));
  }
  print('cleanup', log.join(' | '));
})();

// `for`-`of` over a generator with awaits in the body; the iterator is
// closed by a `break`.
(async function iterate() {
  function* numbers() {
    try {
      for (var i = 0; ; i++) yield { n: i, s: 's' + i };
    } finally {
      print('generator closed');
    }
  }
  var sum = 0;
  for (var item of numbers()) {
    await item;
    gc();
    sum += item.n;
    if (item.n === 5) break;
  }
  print('iterate', sum);
})();

// Async functions that await each other, with collections at each level.
async function level(n) {
  var here = { n: n, s: 'level ' + n };
  if (n === 0) {
    await null;
    gc();
    return here.s;
  }
  var below = await level(n - 1);
  gc();
  return below + ' < ' + here.n;
}
level(6).then(function (v) { print('levels', v); });

// Many suspended functions at once, resumed in order.
(async function many() {
  var all = [];
  for (var i = 0; i < 50; i++) {
    all.push((async function (k) { var box = { k: k }; await null; junk(2); await null; return box.k; })(i));
  }
  gc();
  var values = await Promise.all(all);
  print('many', values.length, values[0], values[49]);
})();

// A rejection with an error object held only by the reaction, thrown
// after a collection.
(async function reject() {
  await null;
  gc();
  throw new TypeError('rejected ' + junk(4));
})().catch(function (e) { gc(); print('reject', show(e)); });

// An async arrow and an async method that keep `this` and `super` across
// awaits.
var base = { hello: function () { return 'hello from base'; } };
var derived = {
  __proto__: base,
  name: 'derived',
  async greet() {
    await null;
    gc();
    var arrow = async () => { await null; gc(); return this.name + ': ' + super.hello(); };
    return await arrow();
  },
};
derived.greet().then(function (v) { print('method', v); });

// Destructuring defaults and template literals with awaits in them.
(async function shapes() {
  var { a = await { then: function (f) { gc(); f('default a'); } }, b = 2 } = {};
  var t = `${await 'x'}-${junk(3)}-${await Promise.resolve('y')}`;
  gc();
  print('shapes', a, b, t);
})();

// A function created by AsyncFunction, awaiting a thenable.
var AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
AsyncFunction('x', 'gc(); var r = await x; gc(); return r + "!";')({ then: function (f) { f('dynamic'); } })
  .then(function (v) { print('dynamic', v); });

print('sync done');
