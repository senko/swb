// Proxies whose traps allocate and collect while the engine holds the
// target, the handler, keys, descriptors and results (the expected output
// is that of Node.js 22; `collect` is the collector in swb-js tests).
var collect = typeof gc === 'function' ? gc : function () {};
function junk(n) { var out = []; for (var i = 0; i < n; i++) out.push({ i: i, s: 'v' + i }); return out; }

var handler = {
  get: function (t, k, r) { junk(20); collect(); return typeof k === 'string' ? 'got ' + k + Reflect.get(t, k, r) : undefined; },
  set: function (t, k, v) { junk(20); collect(); t[k] = v + '!'; return true; },
  has: function (t, k) { junk(10); return k in t; },
  deleteProperty: function (t, k) { collect(); return delete t[k]; },
  ownKeys: function (t) { var keys = Reflect.ownKeys(t).map(function (k) { return typeof k === 'string' ? (k + '').slice(0) : k; }); collect(); return keys; },
  getOwnPropertyDescriptor: function (t, k) { junk(5); var d = Reflect.getOwnPropertyDescriptor(t, k); collect(); return d && { value: d.value, writable: d.writable, enumerable: d.enumerable, configurable: d.configurable }; },
  defineProperty: function (t, k, d) { collect(); return Reflect.defineProperty(t, k, { value: d.value, writable: true, enumerable: true, configurable: true }); },
  getPrototypeOf: function (t) { collect(); return Reflect.getPrototypeOf(t); },
};
var p = new Proxy({ a: 1, b: 2 }, handler);
print(p.a, p.zz);
p.c = 3;
print(p.c, 'c' in p, 'q' in p, delete p.b, Object.keys(p).join());
print(JSON.stringify(p));
var out = [];
for (var k in p) out.push(k);
print(out.join());
print(JSON.stringify(Object.getOwnPropertyDescriptors(p)));
print(JSON.stringify(Object.assign({}, p)));
print(JSON.stringify({ ...p }));

var accessors = new Proxy({}, {
  getOwnPropertyDescriptor: function () {
    var g = function () { return 'g'; };
    var s = function (v) {};
    junk(50);
    collect();
    return { get: g, set: s, enumerable: true, configurable: true };
  },
});
var d = Object.getOwnPropertyDescriptor(accessors, 'x');
print(typeof d.get, typeof d.set, d.get());

var chain = { leaf: 'leaf' };
for (var i = 0; i < 50; i++) chain = new Proxy(chain, { get: function (t, k, r) { junk(3); return Reflect.get(t, k, r); } });
print(chain.leaf);

var keys = [];
for (var i = 0; i < 2000; i++) keys.push('key' + i);
var wide = new Proxy({}, {
  ownKeys: function () { var copy = keys.map(function (k) { return k + ''; }); junk(100); collect(); return copy; },
  getOwnPropertyDescriptor: function (t, k) { return { value: k, enumerable: true, configurable: true }; },
});
print(Object.keys(wide).length, Object.keys(wide)[1999]);

var revocable = Proxy.revocable({ v: 1 }, { get: function (t, k) { revocable.revoke(); collect(); return t[k]; } });
print(revocable.proxy.v);
try { revocable.proxy.v; } catch (e) { print(e.constructor.name, e.message); }

var callable = new Proxy(function (a, b) { return a + b; }, {
  apply: function (t, self, args) { junk(30); collect(); return Reflect.apply(t, self, args) * 2; },
  construct: function (t, args, nt) { collect(); return { made: args.join('+') }; },
});
print(callable(1, 2), new callable(3, 4).made, [1, 2, 3].map(callable).join());
