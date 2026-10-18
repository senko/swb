// Detaching, shrinking and collecting inside every conversion callback of
// the DataView methods and constructor and of the ArrayBuffer methods
// (M7 feature 11a, binary_hostile.rs). The expected output is that of
// Node.js 22 with `gc` as a function that does nothing, except for the
// three Float16 lines that do not throw: Node.js 22 has no getFloat16 (a
// TypeError there), Chromium 148 has it.
var out = [];
function attempt(label, f) {
  try {
    var r = f();
    out.push(label + ': ok ' + String(r));
  } catch (e) {
    out.push(label + ': ' + e.constructor.name);
  }
}
var names = ['Int8', 'Uint8', 'Int16', 'Uint16', 'Int32', 'Uint32', 'Float16', 'Float32', 'Float64', 'BigInt64', 'BigUint64'];
names.forEach(function (n) {
  var one = n.indexOf('Big') === 0 ? 1n : 1;
  var d = new DataView(new ArrayBuffer(16));
  attempt('get' + n + ' index detach', function () {
    return d['get' + n]({ valueOf: function () { d.buffer.transfer(); return 0; } });
  });
  d = new DataView(new ArrayBuffer(16));
  attempt('get' + n + ' index gc', function () {
    return d['get' + n]({ valueOf: function () { gc(); return 0; } }, true);
  });
  d = new DataView(new ArrayBuffer(16));
  attempt('set' + n + ' index detach', function () {
    return d['set' + n]({ valueOf: function () { d.buffer.transfer(); return 0; } }, one);
  });
  d = new DataView(new ArrayBuffer(16));
  attempt('set' + n + ' value detach', function () {
    return d['set' + n](0, { valueOf: function () { d.buffer.transfer(); return one; } });
  });
  var r = new ArrayBuffer(16, { maxByteLength: 32 });
  d = new DataView(r, 8);
  attempt('set' + n + ' value shrink', function () {
    return d['set' + n](0, { valueOf: function () { r.resize(4); return one; } });
  });
  r = new ArrayBuffer(16, { maxByteLength: 32 });
  d = new DataView(r, 8, 8);
  attempt('get' + n + ' index shrink', function () {
    return d['get' + n]({ valueOf: function () { r.resize(15); return 0; } });
  });
  r = new ArrayBuffer(8, { maxByteLength: 32 });
  d = new DataView(r);
  attempt('set' + n + ' value grow', function () {
    d['set' + n](24, { valueOf: function () { r.resize(32); gc(); return one; } });
    return d['get' + n](24);
  });
  d = new DataView(new ArrayBuffer(16));
  attempt('set' + n + ' value gc', function () {
    d['set' + n](8, { valueOf: function () { gc(); return one; } }, true);
    return d['get' + n](8, true);
  });
});
var b = new ArrayBuffer(8);
attempt('ctor offset detach', function () {
  return new DataView(b, { valueOf: function () { b.transfer(); return 0; } });
});
b = new ArrayBuffer(8);
attempt('ctor length detach', function () {
  return new DataView(b, 0, { valueOf: function () { b.transfer(); return 4; } });
});
var rb = new ArrayBuffer(8, { maxByteLength: 16 });
attempt('ctor length shrink', function () {
  return new DataView(rb, 2, { valueOf: function () { rb.resize(4); return 4; } });
});
b = new ArrayBuffer(8);
function target(f) {
  var c = function () {}.bind(null);
  Object.defineProperty(c, 'prototype', { get: function () { f(); gc(); return DataView.prototype; } });
  return c;
}
attempt('ctor prototype detach', function () {
  return Reflect.construct(DataView, [b, 2], target(function () { b.transfer(); }));
});
rb = new ArrayBuffer(8, { maxByteLength: 16 });
attempt('ctor prototype shrink', function () {
  return Reflect.construct(DataView, [rb, 2], target(function () { rb.resize(1); }));
});
rb = new ArrayBuffer(8, { maxByteLength: 16 });
attempt('ctor prototype grow', function () {
  return Reflect.construct(DataView, [rb, 2], target(function () { rb.resize(16); })).byteLength;
});
b = new ArrayBuffer(8);
attempt('slice start detach', function () {
  return b.slice({ valueOf: function () { b.transfer(); return 0; } });
});
b = new ArrayBuffer(8);
attempt('slice species detach', function () {
  b.constructor = { [Symbol.species]: function (n) { b.transfer(); gc(); return new ArrayBuffer(n); } };
  return b.slice(1);
});
b = new ArrayBuffer(8);
attempt('slice species gc', function () {
  b.constructor = { [Symbol.species]: function (n) { gc(); return new ArrayBuffer(n); } };
  return b.slice(1).byteLength;
});
b = new ArrayBuffer(8);
attempt('transfer length detach', function () {
  return b.transfer({ valueOf: function () { b.transfer(); return 4; } });
});
b = new ArrayBuffer(8);
attempt('transfer length gc', function () {
  return b.transfer({ valueOf: function () { gc(); return 4; } }).byteLength + ':' + b.detached;
});
rb = new ArrayBuffer(8, { maxByteLength: 16 });
attempt('resize length detach', function () {
  return rb.resize({ valueOf: function () { rb.transfer(); return 4; } });
});
attempt('ctor length gc', function () {
  return new ArrayBuffer({ valueOf: function () { gc(); return 4; } }, { get maxByteLength() { gc(); return 8; } }).maxByteLength;
});
print(out.join('\n'));
