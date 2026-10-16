// Made by gen-iterator-cases.js from tools/probes/js-iterators.json.
var show = v => { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return String(v); if (typeof v === 'function') return 'function ' + v.name; if (typeof v === 'number' && Object.is(v, -0)) return '-0'; if (typeof v === 'bigint') return v + 'n'; if (Array.isArray(v)) { if (v.length > 50) return 'Array(' + v.length + ')'; const p = []; for (let i = 0; i < v.length; i++) p.push(i in v ? show(v[i]) : '_'); return '[' + p.join(',') + ']'; } if (v instanceof Map) return 'Map{' + [...v].map(([k, x]) => show(k) + '=>' + show(x)).join(',') + '}'; if (v instanceof Set) return 'Set{' + [...v].map(show).join(',') + '}'; if (v && typeof v === 'object') { const p = []; for (const k of Object.keys(v)) p.push(k + ':' + show(v[k])); return '{' + p.join(',') + '}'; } return String(v); };
var SNIPPETS = ["(() => { let it = [1].values(); for (let i = 0; i < 1000; i++) it = it.map(x => x); return it.next() })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = it.map(x => x); return it.next() })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = it.map(x => x); return it.return() })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = it.map(x => x); return it.toArray() })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = it.map(x => x); return 'created' })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = it.take(1); return it.next() })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = it.drop(0); return it.next() })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = it.filter(x => true); return it.next() })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = it.flatMap(x => [x]); return it.next() })()",
"(() => { let it = [1].values(); for (let i = 0; i < 100000; i++) it = Iterator.from(it); return it.next() })()",
"(() => { let it = {next() { return {done: false, value: 1} }}; for (let i = 0; i < 100000; i++) it = Iterator.from({next() { return it.next() }}); return it.next() })()",
"(() => { let it = [1]; for (let i = 0; i < 100000; i++) it = [it]; return it.values().flatMap(x => x).next() })()",
"(() => { let it = [1]; for (let i = 0; i < 100000; i++) it = [it]; let h = it.values(); const f = x => Array.isArray(x) ? x.values().flatMap(f) : [x]; return h.flatMap(f).next() })()",
"(() => { let it = [1]; for (let i = 0; i < 1000; i++) it = [it]; let h = it.values(); const f = x => Array.isArray(x) ? x.values().flatMap(f) : [x]; return h.flatMap(f).next() })()",
"(() => { const a = []; for (let i = 0; i < 100000; i++) a.push([i]); return Iterator.concat(...a).toArray().length })()"];
for (var s of SNIPPETS) { var r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } print(s + " => " + r); }
