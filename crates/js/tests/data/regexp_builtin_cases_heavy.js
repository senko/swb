// Made by gen-regexp-builtin-cases.js from tools/probes/js-regexp-builtin.json.
var show = v => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'function') return 'function ' + v.name;
  if (typeof v === 'number' && Object.is(v, -0)) return '-0';
  if (v instanceof RegExp) return 're(' + v.source + ',' + v.flags + ',' + show(v.lastIndex) + ')';
  if (Array.isArray(v)) {
    if (v.length > 50) return 'Array(' + v.length + ')';
    const p = [];
    for (let i = 0; i < v.length; i++) p.push(i in v ? show(v[i]) : '_');
    const extra = Object.keys(v).filter((k) => !/^\d+$/.test(k)).map((k) => k + ':' + show(v[k]));
    return '[' + p.join(',') + ']' + (extra.length ? '{' + extra.join(',') + '}' : '');
  }
  if (v && typeof v === 'object') {
    const p = [];
    for (const k of Object.keys(v)) p.push(k + ':' + show(v[k]));
    return (Object.getPrototypeOf(v) === null ? 'N' : '') + '{' + p.join(',') + '}';
  }
  return String(v);
};
var SNIPPETS = ["/^(?:a|b)*c/.exec('ab'.repeat(250000))",
"var e; try { /^(?:a|b)*c/.exec('a'.repeat(2e7)); } catch (x) { e = x; } [e instanceof RangeError, e.message]",
"var s = 'ab'.repeat(5e5); s.replace(/b/g, 'c').length",
"var s = 'a,'.repeat(5e5); s.split(/,/).length",
"var s = 'x'.repeat(1e6); s.replace(/(?:)/g, '-').length",
"var s = 'x'.repeat(1e6); s.split(/(?:)/).length",
"var s = 'x'.repeat(2e5); [...s.matchAll(/(?:)/g)].length",
"var s = 'x'.repeat(1e6); s.match(/x/g).length",
"var s = 'ab'.repeat(1e5); s.replace(/(a)(b)/g, '$2$1').slice(0, 6)",
"var s = 'ab'.repeat(1e5); s.replace(/(a)(b)/g, (m, x, y) => y + x).slice(0, 6)",
"var s = 'a'.repeat(1e5) + 'b'; /a*b/.exec(s)[0].length",
"var r = /a/g; var n = 0; while (r.test('a'.repeat(1000))) n++; n",
"var s = ''; for (var i = 0; i < 1000; i++) s += String.fromCharCode(48 + i % 10); s.match(/\\d/g).length",
"new RegExp('(' + 'a|'.repeat(1000) + 'b)').test('b')",
"new RegExp('a'.repeat(10000)).test('a'.repeat(10000))"];
var $out = { push: print };
for (const s of SNIPPETS) { let r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } $out.push(s + " => " + r); }
