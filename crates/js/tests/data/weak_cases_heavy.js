// Made by gen-weak-cases.js from tools/probes/js-weak.json.
var show = v => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'function') return 'function';
  if (typeof v === 'number' && Object.is(v, -0)) return '-0';
  if (Array.isArray(v)) return '[' + v.map(x => typeof x === 'object' && x !== null ? 'object' : show(x)).join(',') + ']';
  if (v && typeof v === 'object') return 'object';
  return String(v);
};
var SNIPPETS = ["const m = new WeakMap(); const keys = []; for (let i = 0; i < 200000; i++) { const k = {}; keys.push(k); m.set(k, i); } let s = 0; for (let i = 0; i < keys.length; i += 1000) s += m.get(keys[i]); s",
"const m = new WeakMap(); for (let i = 0; i < 300000; i++) m.set({}, i); 'done'",
"const s = new WeakSet(); for (let i = 0; i < 300000; i++) s.add({}); 'done'",
"const fr = new FinalizationRegistry(() => {}); for (let i = 0; i < 100000; i++) fr.register({}, i); 'done'",
"const fr = new FinalizationRegistry(() => {}); const ts = []; for (let i = 0; i < 50000; i++) { const t = {}; ts.push(t); fr.register({}, i, t); } let n = 0; for (const t of ts) n += fr.unregister(t); n",
"const rs = []; for (let i = 0; i < 100000; i++) rs.push(new WeakRef({ i })); rs.filter(r => r.deref() !== undefined).length",
"const m = new WeakMap(); let k = {}; const first = k; for (let i = 0; i < 100000; i++) { const next = {}; m.set(k, next); k = next; } let n = 0; for (let c = first; m.has(c); c = m.get(c)) n++; n"];
var $out = { push: print };
for (const s of SNIPPETS) { let r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } $out.push(s + " => " + r); }
