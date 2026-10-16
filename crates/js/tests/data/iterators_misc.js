// Iterator helpers (M7 6c): the steps that skip the result object must
// still honour a patched `next`, a `return` that a script adds, and a
// subclass of `Iterator`. The expected output (iterators_misc.expected) is
// that of Node.js 22.
function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
// A patched `next` of the helper prototype is called by `for`-`of`, by the
// consuming methods and by a helper over a helper.
t(function () {
  var p = Object.getPrototypeOf([].values().map(function (x) { return x; }));
  var orig = p.next, calls = 0;
  p.next = function () { calls++; return orig.call(this); };
  try {
    for (var x of [1, 2].values().map(function (x) { return x; })) { }
    var a = calls;
    [1, 2].values().map(function (x) { return x; }).map(function (x) { return x; }).toArray();
    return a + ' ' + calls;
  } finally { p.next = orig; }
});
// A patched `next` of the Array Iterator prototype is called by a helper.
t(function () {
  var p = Object.getPrototypeOf([].values());
  var orig = p.next, calls = 0;
  p.next = function () { calls++; return orig.call(this); };
  try {
    var r = [1, 2, 3].values().map(function (x) { return x * 2; }).toArray();
    return r.join() + ' ' + calls;
  } finally { p.next = orig; }
});
// An own `next` on the underlying iterator is read once, when the helper is
// made.
t(function () {
  var it = [1, 2, 3].values(), calls = 0;
  var h = it.map(function (x) { return x; });
  it.next = function () { calls++; return { done: true }; };
  return h.toArray().join() + ' ' + calls;
});
// A value that the callback returns survives a collection while the helper
// is suspended.
t(function () {
  var h = [1, 2, 3].values().map(function (x) { return { v: 'v' + x }; });
  var first = h.next().value;
  var junk = [];
  for (var i = 0; i < 2000; i++) junk.push({ i: i });
  return first.v + ' ' + h.next().value.v;
});
// Helpers on a subclass of Iterator, and Iterator.from of a subclass.
t(function () {
  class Count extends Iterator {
    constructor(n) { super(); this.n = n; this.i = 0; }
    next() { return this.i < this.n ? { value: this.i++, done: false } : { value: undefined, done: true }; }
  }
  var c = new Count(5);
  return c.map(function (x) { return x * x; }).filter(function (x) { return x % 2 === 0; }).toArray().join() + ' ' + (Iterator.from(new Count(1)) instanceof Count);
});
// The `return` of a helper before the first `next` closes the iterator
// once; a second `return` does nothing.
t(function () {
  var log = [];
  var it = { __proto__: Iterator.prototype, next: function () { log.push('next'); return { done: false, value: 1 }; }, return: function () { log.push('return'); return {}; } };
  var h = it.map(function (x) { return x; });
  h.return(); h.return(); h.next();
  return log.join();
});
// A chain of 1,000 helpers works; a chain of 3,000 ends with a catchable
// RangeError, and the runtime works afterwards.
t(function () {
  var it = [1].values();
  for (var i = 0; i < 1000; i++) it = it.map(function (x) { return x + 1; });
  return it.next().value;
});
t(function () {
  var it = [1].values();
  for (var i = 0; i < 3000; i++) it = it.filter(function (x) { return true; });
  try { it.next(); } catch (e) { return e.constructor.name + ' ' + [1, 2].values().map(function (x) { return x; }).toArray().join(); }
  return 'no error';
});
// The result of `next` is a fresh ordinary object.
t(function () {
  var h = [5].values().map(function (x) { return x; });
  var r = h.next();
  return Object.keys(r).join() + ' ' + (Object.getPrototypeOf(r) === Object.prototype) + ' ' + (r !== h.next());
});
