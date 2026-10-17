// RegExp objects, match results and the symbol methods with collections
// inside callbacks, getters, `exec` and `valueOf` (M7 8b). The expected
// output (regexp_gc.expected) is that of Node.js 22.
var gc = typeof gc === 'function' ? gc : function () {};
function t(f) { try { print(f()); } catch (e) { print(e.constructor.name + ': ' + e.message); } }
function junk(n) { var o = []; for (var i = 0; i < n; i++) o.push({ i: i, s: 'x' + i }); return o.length; }

// Literals and constructors that are only reachable from locals.
t(function () { var out = []; for (var i = 0; i < 50; i++) { var r = /(a)(?<b>b)?/g; junk(20); gc(); out.push(r.exec('xab')[2]); } return out.join(''); });
t(function () { var r = new RegExp('x' + 'y'.repeat(3) + '(z)', 'gi'); gc(); return r.exec('--XYYYZ--').index + r.source + r.lastIndex; });

// Collections in the callback of a global replace, with captures and groups.
t(function () { return 'a1b2c3d4'.replace(/(?<l>[a-z])(\d)/g, function (m, l, d, i, s, g) { junk(50); gc(); return g.l.toUpperCase() + (d * 2) + i; }); });
t(function () { return 'aaaa'.replace(/a/g, function () { gc(); return String(junk(5)); }); });

// A script's `exec` that collects, its results kept until the end of the loop.
t(function () {
  var r = /b/g, n = 0;
  r.exec = function (s) { gc(); junk(10); n++; return n > 3 ? null : { 0: 'b' + n, index: n, length: 1, groups: { k: 'v' + n } }; };
  return r[Symbol.replace]('abcdef', '[$&|$<k>]');
});
t(function () {
  var r = /x/g, n = 0;
  r.exec = function (s) { gc(); n++; return n > 4 ? null : { 0: '', index: n, length: 1 }; };
  return r[Symbol.match]('xxxxx').length + ':' + r.lastIndex;
});

// Getters and valueOf that collect during the steps of the methods.
t(function () {
  var r = /a/g;
  r.lastIndex = { valueOf: function () { gc(); junk(30); return 1; } };
  var m = r.exec('aXaa');
  return m.index + ':' + r.lastIndex;
});
t(function () {
  var r = /(.)(.)/;
  var src = { toString: function () { gc(); return 'q(w)e'; } };
  r.compile(src, { toString: function () { gc(); return 'g'; } });
  return r.exec('xqwe')[1] + r.flags;
});
t(function () {
  var o = { get flags() { gc(); return 'gy'; }, get source() { gc(); return 'a|b'; } };
  o[Symbol.match] = true;
  var r = new RegExp(o);
  return r.toString() + RegExp.prototype.toString.call(o);
});

// A split with the species constructor and captures; collections in it.
t(function () {
  function C(p, f) { gc(); return new RegExp(p, f); }
  C[Symbol.species] = C;
  var r = /(-)/; r.constructor = C;
  return JSON.stringify('a-b-c'.split(r));
});
t(function () { var s = 'x,'.repeat(300); gc(); var parts = s.split(/(,)/); gc(); return parts.length + parts[1] + parts[599].length; });

// matchAll iterators that outlive their regular expression's other holders.
t(function () {
  var its = [];
  for (var i = 0; i < 20; i++) its.push('a1b22c333'.matchAll(new RegExp('\\d+', 'g')));
  junk(100); gc();
  var out = [];
  for (var j = 0; j < its.length; j += 5) { var all = []; for (var m of its[j]) { gc(); all.push(m[0] + '@' + m.index); } out.push(all.join(' ')); }
  return out.join(' | ');
});
t(function () { var it = 'aXbX'.matchAll(/X/g); it.next(); gc(); junk(50); gc(); var n = it.next(); return n.value.index + String(it.next().done); });

// The legacy static properties keep their subject alive.
t(function () { (function () { var s = 'left' + 'MATCH'.toLowerCase() + 'right'; /(m)a(t)ch/.exec(s); })(); junk(100); gc(); return [RegExp.$1, RegExp.$2, RegExp.lastMatch, RegExp.leftContext, RegExp.rightContext, RegExp.input].join(','); });

// Match indices and groups objects that survive collections.
t(function () {
  var m = /(?<x>a)|(?<y>b)/d.exec('zb');
  gc(); junk(50); gc();
  return JSON.stringify([m.indices, m.indices.groups, m.groups]);
});
t(function () { var ms = []; for (var i = 0; i < 30; i++) { ms.push(/(\d)(\d)?/d.exec('a' + i)); junk(10); } gc(); return ms.map(function (m) { return m.indices[1][0] + (m[2] || '-'); }).join(''); });
