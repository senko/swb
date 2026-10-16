// Collections inside the callbacks of JSON.parse and JSON.stringify: the
// values that the natives hold must stay alive. The output is the same in
// Node.js 22 (where gc() does nothing).
if (typeof gc !== 'function') { var gc = function () {}; }
var out = [];
function show(v) {
  if (v === undefined) return 'undefined';
  if (typeof v === 'string') return '"' + v + '"';
  if (typeof v !== 'object' || v === null) return String(v);
  if (Array.isArray(v)) return '[' + v.map(show).join(',') + ']';
  return '{' + Object.keys(v).map(function (k) { return k + ':' + show(v[k]); }).join(',') + '}';
}
function junk() { var a = []; for (var i = 0; i < 50; i++) a.push({ k: 'x' + i, v: [i] }); return a; }

// 1. A reviver that collects at each call.
var log = [];
var r = JSON.parse('{"a":[1,"two",{"c":"three","d":[true,null]}],"e":"f\\u00e9\\n","g":{"h":{"i":"j"}}}', function (k, v) {
  gc(); junk(); gc();
  log.push(k + '=' + (typeof v === 'object' && v !== null ? 'obj' : show(v)));
  return typeof v === 'string' ? v + '!' : v;
});
out.push(show(r), log.join(' '));

// 2. A reviver that changes the holder.
r = JSON.parse('{"a":1,"b":2,"c":[1,2,3],"d":{"x":1}}', function (k, v) {
  gc();
  if (k === 'a') { delete this.b; this.z = 'added' + k; this.c = ['fresh', { n: 1 }]; gc(); }
  if (k === 'x') { this.y = junk().length; }
  if (k === 'fresh' || v === 'fresh') { gc(); }
  return v;
});
out.push(show(r));
r = JSON.parse('[1,2,3,4]', function (k, v) {
  if (k === '0') { this.length = 2; gc(); }
  if (k === '1') { this.push('p' + junk().length); }
  gc();
  return v === 2 ? undefined : v;
});
out.push(show(r));

// 3. The source text of primitives.
var sources = [];
r = JSON.parse('{"n": 1.50, "s": "a\\tb", "t": true, "z": null, "arr": [ -0 , 1e3 ], "o": {}}', function (k, v, c) {
  gc();
  sources.push(k + ':' + (c && Object.keys(c).length ? c.source : '-'));
  return v;
});
out.push(sources.join(' '));
sources = [];
JSON.parse('[1,2,3]', function (k, v, c) {
  if (k === '0') { this[1] = 'changed'; gc(); }
  sources.push(k + ':' + c.source);
  return v;
});
out.push(sources.join(' '));

// 4. A replacer function that returns new values.
var s = JSON.stringify({ a: 1, b: [2, 3], c: { d: 'e' } }, function (k, v) {
  gc(); junk();
  if (typeof v === 'number') return 'n' + v;
  if (k === 'd') return { fresh: junk().length, text: 'x'.repeat(3) + k };
  return v;
});
out.push(s);

// 5. A property list from a collected array.
s = JSON.stringify({ a: 1, b: { a: 2, c: 3 }, c: 4 }, (function () { var l = ['c', 'a', new String('b'), 1, 'a']; gc(); return l; })());
out.push(s);
s = JSON.stringify({ a: 1, 2: { 2: 3, a: 4 } }, [2, 'a', new Number(2)], '--');
out.push(s);

// 6. toJSON methods that allocate and collect.
var calls = 0;
s = JSON.stringify([{ toJSON: function (k) { gc(); junk(); calls++; return { key: k, big: junk().length }; } },
  { toJSON: function () { gc(); return 'str' + calls; } }, 'plain']);
out.push(s);
s = JSON.stringify({ a: { toJSON: function () { gc(); return [1, 2, { toJSON: function () { gc(); return 'inner'; } }]; } } });
out.push(s);

// 7. Getters that collect and delete.
var o = {
  get a() { gc(); delete this.c; junk(); return 'A'; },
  b: 'B',
  get c() { return 'C'; },
  get d() { gc(); this.late = 1; return { e: junk().length > 0 }; },
};
out.push(JSON.stringify(o));
var arr = [1, 2, 3];
Object.defineProperty(arr, 1, { get: function () { gc(); arr.length = 1; return 'gone'; }, enumerable: true });
out.push(JSON.stringify(arr));

// 8. Wrapper objects with valueOf and toString that collect.
var num = new Number(5); num.valueOf = function () { gc(); return 7; };
var str = new String('q'); str.toString = function () { gc(); return 'w' + junk().length; };
out.push(JSON.stringify([num, str, new Boolean(false)]));
out.push(JSON.stringify({ a: 1, b: [2, 3] }, null, num));
out.push(JSON.stringify({ a: 1, b: [2, 3] }, null, str));

// 9. Raw JSON.
var raw = JSON.rawJSON('12345678901234567890');
gc();
out.push(JSON.stringify({ big: raw, list: [raw, JSON.rawJSON('"x"'), JSON.rawJSON('null')] }, function (k, v) { gc(); return v; }));

// 10. Errors after collections.
try {
  JSON.parse('{"a":[1,2]}', function (k, v) { gc(); if (k === '1') throw new RangeError('stop ' + junk().length); return v; });
} catch (e) { out.push(e.name + ': ' + e.message); }
try {
  JSON.stringify({ a: { b: 1 } }, function (k, v) { gc(); if (k === 'b') throw new TypeError('no ' + k); return v; });
} catch (e) { out.push(e.name + ': ' + e.message); }
try { JSON.parse('{"a":[1,2,{"b":x}]}'); } catch (e) { gc(); out.push(e.name + ': ' + e.message); }

// 11. Many keys and strings parsed under collection pressure.
var parts = [];
for (var i = 0; i < 300; i++) parts.push('"key' + i + '":{"s":"value' + i + '","a":[' + i + ',"' + i + '",{"deep":' + i + '}]}');
var big = JSON.parse('{' + parts.join(',') + '}');
gc();
var ok = true;
for (i = 0; i < 300; i++) {
  var e = big['key' + i];
  if (e.s !== 'value' + i || e.a[0] !== i || e.a[1] !== String(i) || e.a[2].deep !== i) ok = false;
}
out.push(ok, Object.keys(big).length);
var again = JSON.stringify(big);
out.push(again.length, JSON.stringify(JSON.parse(again)) === again);

// 12. A cycle below a deep path, and the same object twice (not a cycle).
var root = {}; var cur = root;
for (i = 0; i < 40; i++) { cur.next = { level: i }; cur = cur.next; }
var shared = { v: 1 };
out.push(JSON.stringify({ a: shared, b: [shared, shared] }));
cur.back = root.next;
try { JSON.stringify(root); } catch (e) { out.push(e.message.split('\n').length, e.message.split('\n').slice(0, 3).join('|')); }
cur.back = undefined;
cur.again = shared;
out.push(JSON.stringify(root).length);

console.log(out.join('\n'));
