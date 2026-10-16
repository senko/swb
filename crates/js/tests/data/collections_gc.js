// Map and Set with collections inside callbacks, adders, getters, `has`,
// `keys` and `next` of the set methods' argument (M7 6b). The expected
// output (collections_gc.expected) is that of Node.js 22, with
// getOrInsert and getOrInsertComputed added from the proposal's text where
// Node.js 22 lacks them.
var gc = typeof gc === 'function' ? gc : function () {};
function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
function junk(n) { var o = []; for (var i = 0; i < n; i++) o.push({ i: i, s: 'x' + i }); return o.length; }
if (!Map.prototype.getOrInsert) {
  Object.defineProperty(Map.prototype, 'getOrInsert', { configurable: true, writable: true, value: function (key, value) {
    if (this.has(key)) return this.get(key);
    this.set(key, value);
    return value;
  } });
  Object.defineProperty(Map.prototype, 'getOrInsertComputed', { configurable: true, writable: true, value: function (key, callback) {
    if (this.has(key)) return this.get(key);
    var value = callback(key);
    this.set(key, value);
    return value;
  } });
}
// Callbacks that allocate and collect while the table changes.
t(function () { var m = new Map(); for (var i = 0; i < 60; i++) m.set('k' + i, { v: 'v' + i }); var out = []; m.forEach(function (v, k) { gc(); junk(2); out.push(k + v.v); if (k === 'k3') { m.delete('k4'); m.set('late', { v: 'L' }); } }); return out.length + ' ' + out[4] + ' ' + out[out.length - 1]; });
t(function () { var s = new Set(); for (var i = 0; i < 60; i++) s.add('s' + i); var out = []; s.forEach(function (v) { gc(); out.push(v); if (out.length < 5) s.add('extra' + out.length); if (v === 's10') s.clear(); }); return out.join(); });
t(function () { var m = new Map(); for (var i = 0; i < 40; i++) m.set({ id: i }, 'p' + i); var n = 0; for (var e of m) { gc(); if (e[0].id % 2) m.delete(e[0]); n++; } return n + ' ' + m.size; });
t(function () { var m = new Map(); for (var i = 0; i < 100; i++) m.set(i, 'w' + i); var out = []; for (var e of m) { out.push(e[1]); if (e[0] === 2) { for (var j = 0; j < 95; j++) m.delete(j); gc(); } } return out.join(); });
// Constructors with adders and iterators that allocate.
t(function () { var log = []; var orig = Map.prototype.set; Map.prototype.set = function (k, v) { gc(); junk(2); log.push(k); return orig.call(this, k, v); }; try { var m = new Map([['a', 1], ['b', 2], ['c' + 'd', 3]]); } finally { Map.prototype.set = orig; } return log.join() + ' ' + m.size + ' ' + m.get('cd'); });
t(function () { var it = { i: 0 }; it[Symbol.iterator] = function () { return this; }; it.next = function () { gc(); junk(1); return this.i < 40 ? { value: ['key' + this.i, { n: this.i++ }], done: false } : { done: true }; }; var m = new Map(it); return m.size + ' ' + m.get('key39').n; });
t(function () { var it = { i: 0 }; it[Symbol.iterator] = function () { return this; }; it.next = function () { gc(); return this.i < 40 ? { value: 'item' + this.i++, done: false } : { done: true }; }; var s = new Set(it); return s.size + ' ' + s.has('item39'); });
// getOrInsertComputed: a callback that collects and changes the map.
t(function () { var m = new Map(); m.set('x', 1); var r = m.getOrInsertComputed('y' + 'z', function (k) { gc(); m.set('w', 2); m.delete('x'); junk(3); return { made: k }; }); return r.made + ' ' + m.size + ' ' + m.get('yz').made; });
t(function () { var m = new Map(); var r = m.getOrInsertComputed('k', function (k) { m.set(k, 'inner'); gc(); return 'outer'; }); return r + ' ' + m.get('k') + ' ' + m.size; });
// groupBy.
t(function () { var items = []; for (var i = 0; i < 50; i++) items.push({ g: 'g' + (i % 5), i: i }); var r = Map.groupBy(items, function (o) { gc(); junk(1); return o.g + '!'; }); return r.size + ' ' + r.get('g3!').length + ' ' + r.get('g3!')[2].i; });
// The set methods with an argument whose has, keys and next collect.
t(function () { var a = new Set(); for (var i = 0; i < 30; i++) a.add('a' + i); var b = { size: 40, has: function (v) { gc(); junk(1); return v.length > 2; }, keys: function () { gc(); var i = 0; return { next: function () { gc(); return i < 30 ? { value: 'b' + i++, done: false } : { done: true }; } }; } }; return [a.union(b).size, a.intersection(b).size, a.difference(b).size, a.symmetricDifference(b).size, a.isSubsetOf(b), a.isSupersetOf(b), a.isDisjointFrom(b)].join(); });
t(function () { var a = new Set(); for (var i = 0; i < 30; i++) a.add('a' + i); var b = { size: 3, has: function (v) { gc(); a.delete('a1'); a.add('new' + a.size); return v.length > 3; }, keys: function () { gc(); return ['a2', 'new', 'a3'][Symbol.iterator](); } }; return [[].concat(Array.from(a.union(b))).length, Array.from(a.intersection(b)).length, a.size].join(); });
t(function () { var a = new Set(['p' + 'q', 'r']); var b = { get size() { gc(); return 5; }, get has() { gc(); return function (v) { return true; }; }, get keys() { gc(); return function () { return ['p' + 'q', 'z' + 'z'][Symbol.iterator](); }; } }; return Array.from(a.symmetricDifference(b)).join() + ' ' + Array.from(a.union(b)).join(); });
// Iterators that outlive many compactions and clears.
t(function () { var m = new Map(); for (var i = 0; i < 64; i++) m.set(i, 'm' + i); var its = []; for (var j = 0; j < 10; j++) { var it = m.keys(); for (var k = 0; k < j * 6; k++) it.next(); its.push(it); } for (var r = 0; r < 6; r++) { for (var i = 0; i < 64; i++) m.delete(r * 1000 + i + (r ? 0 : 0)); for (var i = 0; i < 64; i++) m.set((r + 1) * 1000 + i, 'n'); gc(); } return its.map(function (it) { return it.next().value; }).join(); });
t(function () { var s = new Set([1, 2, 3]); var its = []; for (var j = 0; j < 30; j++) { var it = s.values(); it.next(); its.push(it); junk(1); if (j % 7 === 0) gc(); } s.clear(); s.add(9); gc(); return its.map(function (it) { return it.next().value; }).join(); });
t(function () { var m = new Map([[1, 2]]); var c = 0; for (var i = 0; i < 3000; i++) { var it = m.entries(); if (i % 3 === 0) it.next(); c += i % 5; if (i % 500 === 0) gc(); } m.set(3, 4); return c + ' ' + Array.from(m).join('|'); });
// Keys that are strings made by concatenation, rooted while the table flattens them.
t(function () { var m = new Map(); var k = ''; for (var i = 0; i < 40; i++) { k += 'ab'; m.set(k + '', i); gc(); } var s = new Set(); for (var i = 0; i < 40; i++) { s.add('x' + 'y' + i); gc(); } return m.size + ' ' + m.get('ab'.repeat(40)) + ' ' + s.has('xy39') + ' ' + s.size; });
t(function () { var m = new Map(); for (var i = 0; i < 30; i++) m.set('k' + i + 'x', [i, 'v' + i]); gc(); var out = []; m.forEach(function (v, k) { out.push(k.length + v[1]); }); return out.slice(0, 3).join() + ' ' + out.length; });
