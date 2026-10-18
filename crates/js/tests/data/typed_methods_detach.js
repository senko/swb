// Detaching, shrinking, growing and collecting inside every callback of
// the methods of %TypedArray%.prototype, `Atomics` and the base64 methods
// of Uint8Array (M7 feature 11c): the callbacks of the iteration methods,
// the comparators of `sort` and `toSorted`, the conversions of the
// arguments (`fromIndex`, `start`, `end`, the value of `fill` and `with`),
// the species constructors, the `toLocaleString` of the elements, the
// operands of `Atomics` and the option getters of `setFromBase64`. Each
// case prints its result or its error. The expected output
// (typed_methods_detach.expected) is that of Chromium 148:
// gen-typed-cases.js makes the probe case `detach-methods` of
// tools/probes/js-typed.json from this file and the expected output from
// the probe's result. `gc` is a no-op there and a collection in swb.
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
  // The error's name only (the messages are checked in builtins_typed.rs).
  try { r = 'ok ' + show(f()); } catch (e) { r = 'err ' + e.name; }
  out.push(name + ' => ' + r);
}
function rab(n, max) { return new ArrayBuffer(n, { maxByteLength: max }); }
var actions = {
  detach: function (b) { b.transfer(); },
  shrink: function (b) { if (b.resizable) b.resize(1); },
  grow: function (b) { if (b.resizable) b.resize(b.maxByteLength); },
  collect: function () { gc(); },
};
var kinds = [Uint8Array, Int16Array, Float64Array, BigInt64Array];
function val(T, n) { return T === BigInt64Array ? BigInt(n) : n; }
function make(T, b) {
  var t = new T(b);
  for (var i = 0; i < t.length; i++) t[i] = val(T, i + 1);
  return t;
}
Object.keys(actions).forEach(function (name) {
  var act = actions[name];
  kinds.forEach(function (T) {
    var tag = name + ' ' + T.name + ' ';
    var size = T.BYTES_PER_ELEMENT;
    function fresh() { var b = rab(4 * size, 8 * size); return [b, make(T, b)]; }
    // The callback methods: the third step acts.
    ['forEach', 'every', 'some', 'find', 'findIndex', 'findLast', 'findLastIndex', 'map', 'filter'].forEach(function (m) {
      run(tag + m + ' callback', function () {
        var [b, t] = fresh(), seen = [];
        var r = t[m](function (v, i) { seen.push(show(v)); if (i === 1 || i === 2) { act(b); gc(); } return m === 'every' ? true : m === 'filter' ? i % 2 === 0 : m === 'map' ? val(T, 9) : false; });
        return [seen, r, t.length];
      });
    });
    ['reduce', 'reduceRight'].forEach(function (m) {
      run(tag + m + ' callback', function () {
        var [b, t] = fresh(), seen = [];
        var r = t[m](function (a, v, i) { seen.push(show(v)); if (seen.length === 2) { act(b); gc(); } return v; }, val(T, 0));
        return [seen, r, t.length];
      });
    });
    ['sort', 'toSorted'].forEach(function (m) {
      run(tag + m + ' comparator', function () {
        var [b, t] = fresh(), n = 0;
        var r = t[m](function (x, y) { if (++n === 2) { act(b); gc(); } return x < y ? 1 : -1; });
        return [r, t.length];
      });
      run(tag + m + ' comparator that throws after acting', function () {
        var [b, t] = fresh();
        try { t[m](function () { act(b); gc(); throw new RangeError('stop'); }); } catch (e) { return [e.name, t.length]; }
      });
    });
    run(tag + 'map species', function () {
      var [b, t] = fresh();
      t.constructor = { [Symbol.species]: function (n) { act(b); gc(); return new T(n); } };
      return [t.map(function (v) { return v; }), t.length];
    });
    run(tag + 'filter species', function () {
      var [b, t] = fresh();
      t.constructor = { [Symbol.species]: function (n) { act(b); gc(); return new T(n); } };
      return [t.filter(function () { return true; }), t.length];
    });
    run(tag + 'map result is the receiver', function () {
      var [b, t] = fresh(), seen = [];
      t.constructor = { [Symbol.species]: function () { return t; } };
      var r = t.map(function (v, i) { seen.push(show(v)); if (i === 1) act(b); return val(T, 7); });
      return [seen, r === t, t.length];
    });
    // The arguments.
    function acting(b, v) { return { valueOf: function () { act(b); gc(); return v; } }; }
    run(tag + 'at index', function () { var [b, t] = fresh(); return [t.at(acting(b, 3)), t.length]; });
    run(tag + 'fill value', function () { var [b, t] = fresh(); return [t.fill(acting(b, val(T, 5)) ), t.length]; });
    run(tag + 'fill start', function () { var [b, t] = fresh(); return [t.fill(val(T, 5), acting(b, 1)), t.length]; });
    run(tag + 'fill end', function () { var [b, t] = fresh(); return [t.fill(val(T, 5), 0, acting(b, 4)), t.length]; });
    run(tag + 'copyWithin target', function () { var [b, t] = fresh(); return [t.copyWithin(acting(b, 0), 2), t.length]; });
    run(tag + 'copyWithin start', function () { var [b, t] = fresh(); return [t.copyWithin(2, acting(b, 0)), t.length]; });
    run(tag + 'copyWithin end', function () { var [b, t] = fresh(); return [t.copyWithin(0, 2, acting(b, 4)), t.length]; });
    run(tag + 'includes from', function () { var [b, t] = fresh(); return [t.includes(val(T, 4), acting(b, 1)), t.length]; });
    run(tag + 'includes undefined from', function () { var [b, t] = fresh(); return [t.includes(undefined, acting(b, 1)), t.length]; });
    run(tag + 'indexOf from', function () { var [b, t] = fresh(); return [t.indexOf(val(T, 4), acting(b, 1)), t.length]; });
    run(tag + 'lastIndexOf from', function () { var [b, t] = fresh(); return [t.lastIndexOf(val(T, 2), acting(b, 3)), t.length]; });
    run(tag + 'join separator', function () { var [b, t] = fresh(); return [t.join(acting(b, '-')), t.length]; });
    run(tag + 'with index', function () { var [b, t] = fresh(); return [t.with(acting(b, 1), val(T, 9)), t.length]; });
    run(tag + 'with index out of range after', function () { var [b, t] = fresh(); return [t.with(acting(b, 3), val(T, 9)), t.length]; });
    run(tag + 'with value', function () { var [b, t] = fresh(); return [t.with(1, acting(b, val(T, 9))), t.length]; });
    run(tag + 'with value, last index', function () { var [b, t] = fresh(); return [t.with(3, acting(b, val(T, 9))), t.length]; });
    // The elements' own toLocaleString.
    run(tag + 'toLocaleString', function () {
      var [b, t] = fresh(), n = 0;
      var oldN = Number.prototype.toLocaleString, oldB = BigInt.prototype.toLocaleString;
      Number.prototype.toLocaleString = BigInt.prototype.toLocaleString = function () { if (++n === 2) { act(b); gc(); } return 'x'; };
      try { return [t.toLocaleString(), t.length]; } finally { Number.prototype.toLocaleString = oldN; BigInt.prototype.toLocaleString = oldB; }
    });
    // Atomics.
    var one = val(T, 1);
    if (T !== Float64Array) {
      run(tag + 'Atomics.add operand', function () { var [b, t] = fresh(); return [Atomics.add(t, 2, acting(b, one)), t.length]; });
      run(tag + 'Atomics.add index', function () { var [b, t] = fresh(); return [Atomics.add(t, acting(b, 2), one), t.length]; });
      run(tag + 'Atomics.store operand', function () { var [b, t] = fresh(); return [Atomics.store(t, 3, acting(b, one)), t.length]; });
      run(tag + 'Atomics.compareExchange operands', function () { var [b, t] = fresh(); return [Atomics.compareExchange(t, 3, acting(b, val(T, 4)), acting(b, one)), t.length]; });
      run(tag + 'Atomics.load index', function () { var [b, t] = fresh(); return [Atomics.load(t, acting(b, 2)), t.length]; });
    }
    if (T === Int16Array) {
      run(tag + 'Atomics.notify count', function () { var [b, t] = fresh(); return [Atomics.notify(new Int32Array(b), 0, acting(b, 1))]; });
    }
  });
});
// The base64 and hex methods.
Object.keys(actions).forEach(function (name) {
  var act = actions[name];
  var tag = name + ' Uint8Array ';
  function acting(b, v) { return { valueOf: function () { act(b); gc(); return v; } }; }
  function fresh() { var b = rab(4, 8); var u = new Uint8Array(b); u.set([1, 2, 3, 4]); return [b, u]; }
  run(tag + 'toBase64 option', function () {
    var [b, u] = fresh();
    return [u.toBase64({ get omitPadding() { act(b); gc(); return false; } })];
  });
  run(tag + 'toBase64 alphabet', function () {
    var [b, u] = fresh();
    return [u.toBase64({ get alphabet() { act(b); gc(); return 'base64url'; } })];
  });
  run(tag + 'setFromBase64 option', function () {
    var [b, u] = fresh();
    var r = u.setFromBase64('AQIDBAUG', { get alphabet() { act(b); gc(); return 'base64'; } });
    return [r.read, r.written, u.length];
  });
  run(tag + 'setFromBase64 last chunk option', function () {
    var [b, u] = fresh();
    var r = u.setFromBase64('AQIDBAUG', { get lastChunkHandling() { act(b); gc(); return 'loose'; } });
    return [r.read, r.written, u.length];
  });
  run(tag + 'setFromHex string', function () {
    var [b, u] = fresh();
    var r = u.setFromHex({ toString: function () { act(b); return '0102'; } });
    return [r];
  });
  run(tag + 'toHex', function () {
    var [b, u] = fresh();
    act(b);
    return [u.toHex()];
  });
});
print(out.join("\n"));
