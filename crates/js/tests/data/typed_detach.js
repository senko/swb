// Detaching, shrinking, growing and collecting inside every callback of
// the typed arrays (M7 feature 11b): the conversions of an index or a
// value, the getters of an array-like source, the iterator of a source,
// the species constructor, the mapping function of `from`, the
// `prototype` getter of `new.target`. Each case prints its result or its
// error. The expected output (typed_detach.expected) is that of Chromium
// 148: gen-typed-cases.js makes the probe case `detach` of
// tools/probes/js-typed.json from this file and the expected output from
// the probe's result. `gc` is a no-op there and a collection in swb.
// Node.js 22 (V8 12) differs in places (it throws where a conversion
// detached the array in `from` and `of`, and does not write an element
// whose index a conversion made valid).
var gc = typeof globalThis.gc === 'function' ? globalThis.gc : function () {};
var print = typeof globalThis.print === 'function' ? globalThis.print : console.log;
var out = [];
function show(v) {
  if (typeof v === 'bigint') return v + 'n';
  if (v && typeof v === 'object' && typeof v.length === 'number') {
    var s = [];
    for (var i = 0; i < v.length; i++) s.push(show(v[i]));
    return '[' + s.join(',') + ']';
  }
  return String(v);
}
function run(name, f) {
  var r;
  // The error's name only: Node.js 22 (V8 12) words some of the
  // messages differently from Chromium 148, whose wording
  // builtins_typed.rs checks.
  try { r = 'ok ' + show(f()); } catch (e) { r = 'err ' + e.name; }
  out.push(name + ' => ' + r);
}
function rab(n, max) { return new ArrayBuffer(n, { maxByteLength: max }); }
function det(x) { (x instanceof ArrayBuffer ? x : x.buffer).transfer(); }
// What a callback does to a buffer `b`.
var actions = {
  detach: function (b) { b.transfer(); },
  shrink: function (b) { if (b.resizable) b.resize(1); },
  grow: function (b) { if (b.resizable) b.resize(b.maxByteLength); },
  collect: function () { gc(); },
};
var kinds = [Uint8Array, Int16Array, Float64Array, BigInt64Array];
function val(T, n) { return T === BigInt64Array ? BigInt(n) : n; }
function one(T) { return val(T, 1); }
Object.keys(actions).forEach(function (name) {
  var act = actions[name];
  kinds.forEach(function (T) {
    var tag = name + ' ' + T.name + ' ';
    var size = T.BYTES_PER_ELEMENT;
    run(tag + 'element write', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      t[3] = { valueOf: function () { act(b); gc(); return val(T, 7); }, toString: null };
      return [t.length, t[3]];
    });
    run(tag + 'element write past the end', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      t[6] = { valueOf: function () { act(b); return val(T, 7); } };
      return [t.length, t[6]];
    });
    run(tag + 'defineProperty', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      Object.defineProperty(t, '2', { value: { valueOf: function () { act(b); return val(T, 5); } } });
      return [t.length, t[2]];
    });
    run(tag + 'Reflect.set', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      return [Reflect.set(t, 1, { valueOf: function () { act(b); return val(T, 5); } }), t.length, t[1]];
    });
    run(tag + 'ctor offset', function () {
      var b = rab(4 * size, 8 * size);
      var t = new T(b, { valueOf: function () { act(b); return size; } });
      return [t.length, t.byteOffset];
    });
    run(tag + 'ctor length', function () {
      var b = rab(4 * size, 8 * size);
      var t = new T(b, 0, { valueOf: function () { act(b); return 2; } });
      return [t.length];
    });
    run(tag + 'ctor array-like getter', function () {
      var b = rab(4 * size, 8 * size), src = new T(b);
      var o = { length: 3, get 0() { act(b); gc(); return one(T); }, 1: one(T), 2: one(T) };
      return [new T(o), src.length];
    });
    run(tag + 'ctor iterator', function () {
      var b = rab(4 * size, 8 * size), src = new T(b);
      var it = { [Symbol.iterator]: function () { var i = 0; return { next: function () { if (i === 1) { act(b); gc(); } return { done: i++ > 2, value: one(T) }; } }; } };
      return [new T(it), src.length];
    });
    run(tag + 'ctor from a typed array, prototype getter', function () {
      var b = rab(4 * size, 8 * size), src = new T(b);
      var nt = function () {}.bind(null);
      Object.defineProperty(nt, 'prototype', { get: function () { act(b); gc(); return T.prototype; } });
      return [Reflect.construct(T, [src], nt)];
    });
    run(tag + 'set offset', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      t.set([one(T)], { valueOf: function () { act(b); return 1; } });
      return [t.length, t[1]];
    });
    run(tag + 'set array-like getter', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      t.set({ length: 3, get 0() { act(b); gc(); return one(T); }, 1: one(T), 2: one(T) });
      return [t.length];
    });
    run(tag + 'set from a typed array, offset', function () {
      var b = rab(4 * size, 8 * size), src = new T(b), t = new T(4);
      t.set(src, { valueOf: function () { act(b); return 0; } });
      return [t.length, src.length];
    });
    run(tag + 'subarray begin', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      var s = t.subarray({ valueOf: function () { act(b); return 1; } });
      return [s.length, s.byteOffset];
    });
    run(tag + 'subarray species', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      t.constructor = { [Symbol.species]: function (buf, off, len) { act(b); gc(); return new T(buf, off, len); } };
      var s = t.subarray(1, 3);
      return [s.length];
    });
    run(tag + 'slice end', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      t[0] = one(T);
      return [t.slice(0, { valueOf: function () { act(b); return 4; } })];
    });
    run(tag + 'slice species', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      t[0] = one(T);
      t.constructor = { [Symbol.species]: function (n) { act(b); gc(); return new T(n); } };
      return [t.slice()];
    });
    run(tag + 'slice species of another type', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      t[0] = one(T);
      var Other = T === BigInt64Array ? BigUint64Array : Int8Array;
      t.constructor = { [Symbol.species]: function (n) { act(b); return new Other(n); } };
      return [t.slice()];
    });
    run(tag + 'from mapping', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      var C = function () { return t; };
      T.from.call(C, [one(T), one(T), one(T)], function (x, i) { if (i === 1) { act(b); gc(); } return x; });
      return [t.length];
    });
    run(tag + 'from array-like getter', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      var C = function () { return t; };
      T.from.call(C, { length: 2, get 0() { act(b); return one(T); }, 1: one(T) });
      return [t.length];
    });
    run(tag + 'of item', function () {
      var b = rab(4 * size, 8 * size), t = new T(b);
      var C = function () { return t; };
      T.of.call(C, one(T), { valueOf: function () { act(b); return one(T); } });
      return [t.length];
    });
    run(tag + 'iteration', function () {
      var b = rab(4 * size, 8 * size), t = new T(b), seen = [];
      for (var v of t) { seen.push(v); if (seen.length === 2) { act(b); gc(); } }
      return [seen.length];
    });
  });
});
print(out.join("\n"));
