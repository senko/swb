// Random operations on a Map and a Set with live iterators, native loops and
// the set methods (M7 6b). The script prints the number of trace lines and a
// checksum of the trace; the expected values are those of Node.js 22. The
// test sets ROUNDS (60 normally, 6 in the GC stress mode).
if (typeof ROUNDS === 'undefined') var ROUNDS = 60;
var seed = 12345;
function rnd(n) { seed = (seed * 1103515245 + 12345) % 2147483648; return Math.floor(seed / 65536) % n; }
var out = [];
for (var round = 0; round < ROUNDS; round++) {
  var m = new Map(), s = new Set();
  var its = [];
  var range = 5 + rnd(120);
  var steps = 200 + rnd(400);
  for (var step = 0; step < steps; step++) {
    var op = rnd(14);
    var k = rnd(range);
    if (op < 4) { m.set(k, step); s.add(k); }
    else if (op < 7) { m.delete(k); s.delete(k); }
    else if (op === 7 && rnd(20) === 0) { m.clear(); s.clear(); }
    else if (op === 8) { its.push(rnd(2) ? m.entries() : s.values()); }
    else if (op === 9 && its.length) { var it = its[rnd(its.length)]; for (var j = rnd(5); j >= 0; j--) { var r = it.next(); out.push(r.done ? 'D' : JSON.stringify(r.value)); } }
    else if (op === 10 && its.length) { its.splice(rnd(its.length), 1); }
    else if (op === 11) {
      var n = 0;
      m.forEach(function (v, key) { n++; if (n < 30) { if (key % 3 === 0) m.delete((key + 1) % range); if (key % 5 === 0) m.set(key + 1000 + n, 0); if (key % 7 === 0 && n === 3) m.clear(); } });
      out.push('fe' + n);
    }
    else if (op === 12) {
      var c = 0;
      for (var v of s) { c++; if (c < 40) { if (v % 2) s.delete((v + 3) % range); else s.add(v + 500 + c); } else break; }
      out.push('fo' + c);
    }
    else if (op === 13) {
      var o = new Set([rnd(range), rnd(range), rnd(range)]);
      out.push([...s.union(o)].length, [...s.intersection(o)].join('.'), [...s.difference(o)].length, s.isSubsetOf(o), s.isDisjointFrom(o));
    }
  }
  out.push('M' + [...m].join('|'));
  out.push('S' + [...s].join(','));
  for (var it2 of its) { var x = []; for (var q = 0; q < 3; q++) { var rr = it2.next(); x.push(rr.done ? 'D' : JSON.stringify(rr.value)); } out.push(x.join(' ')); }
}
var text = out.join('\n'), h = 7;
for (var i = 0; i < text.length; i++) h = (Math.imul(h, 31) + text.charCodeAt(i)) | 0;
print(out.length, text.length, h);
