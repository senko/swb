// Async generators, `for await`, `yield*` and `Array.fromAsync` (M7 7d)
// that allocate and collect while they wait: the suspended frames, the
// queues of requests, the records of the loops, the Async-from-Sync
// Iterators and the state of `Array.fromAsync` are then held only by
// generator objects, reactions and the job queue. The output is printed
// from the jobs, so it also shows their order. The expected output
// (async_iteration_gc.expected) is that of Node.js 22 (`just jsdiff`
// compares).
var gc = typeof gc === 'function' ? gc : function () {};
function junk(n) { var o = []; for (var i = 0; i < n; i++) o.push({ i: i, s: 'x' + i }); return o.length; }
function show(r) { return r && typeof r === 'object' ? JSON.stringify(r) : String(r); }

// Registers, `arguments` and a captured cell of a generator across yields
// and awaits; a queue of requests that waits while the frame awaits.
async function* counter(a, b) {
  var o = { tag: 'gen' + a };
  var count = 0;
  var bump = function () { count++; return count; };
  try {
    for (var i = 0; i < 5; i++) {
      await { then: function (f) { gc(); junk(5); f(); } };
      var sent = yield o.tag + ':' + bump() + ':' + arguments.length;
      gc();
      if (sent) o.tag = sent;
    }
  } finally {
    await null;
    gc();
    print('counter finally', count, b.s);
  }
  return 'done ' + o.tag;
}
var c = counter(1, { s: 'second' });
var requests = [];
for (var k = 0; k < 8; k++) requests.push(c.next(k === 2 ? 'renamed' : undefined));
gc();
requests.forEach(function (p, i) { p.then(function (r) { gc(); print('request', i, show(r)); }); });

// `for await` over an async generator, with `break` and the awaited close.
(async function loop() {
  var seen = [];
  async function* source() {
    try { for (var i = 0; ; i++) { junk(3); yield { v: i }; } }
    finally { await { then: function (f) { gc(); f(); } }; seen.push('closed'); }
  }
  for await (var item of source()) {
    gc();
    seen.push(item.v);
    if (item.v === 3) break;
  }
  print('loop', seen.join());
})();

// `for await` over a sync iterable of promises, with a rejection (the
// iterator has no `return`: Node.js 22 does not close it, Chromium 148
// does).
(async function sync() {
  var log = [];
  var iterable = {
    [Symbol.iterator]: function () {
      var i = 0;
      return {
        next: function () { gc(); i++; return { value: i === 3 ? Promise.reject(new Error('third')) : Promise.resolve({ n: i }), done: i > 4 }; }
      };
    }
  };
  try { for await (var x of iterable) { gc(); log.push(x.n); } } catch (e) { log.push('caught ' + e.message); }
  print('sync', log.join());
})();

// `yield*` over an async and a sync delegate while the outer generator's
// requests wait.
async function* inner() { var x = yield 'i1'; junk(4); gc(); yield 'i2:' + x; return 'inner done'; }
async function* outer() {
  var r1 = yield* inner();
  var r2 = yield* [Promise.resolve('s1'), 's2'];
  yield r1 + '|' + r2;
}
var d = outer();
var dr = [];
for (var m = 0; m < 6; m++) dr.push(d.next('sent' + m));
gc();
Promise.all(dr).then(function (rs) { print('delegate', rs.map(show).join(' ')); });

// `Array.fromAsync` with a mapping function that awaits, over an async
// generator and an array-like.
async function* letters() { for (var ch of 'abc') { gc(); yield ch; } }
Array.fromAsync(letters(), async function (v, i) { await null; gc(); return v + i; })
  .then(function (a) { print('fromAsync', a.join()); });
Array.fromAsync({ length: 3, 0: Promise.resolve('x'), 1: { y: 1 }, 2: 'z' }, function (v) { junk(2); return show(v); })
  .then(function (a) { print('fromAsync like', a.join(' ')); });

// `return` and `throw` requests on suspended generators.
async function* stopper() { try { yield 1; yield 2; } finally { gc(); print('stopper finally'); } }
var s = stopper();
s.next().then(function () { return s.return(Promise.resolve('stopped')); }).then(function (r) { print('return', show(r)); });
var t = stopper();
t.next().then(function () { return t.throw(new Error('thrown')); }).then(null, function (e) { print('throw', e.message); });

print('sync done');
gc();
