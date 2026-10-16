// Made by gen-array-cases.js from tools/probes/js-arrays.json.
var show = v => { if (typeof v === 'string') return JSON.stringify(v); if (typeof v === 'symbol') return String(v); if (typeof v === 'function') return 'function ' + v.name; if (typeof v === 'number' && Object.is(v, -0)) return '-0'; if (typeof v === 'bigint') return v + 'n'; if (Array.isArray(v)) { if (v.length > 50) return 'Array(' + v.length + ')'; const p = []; for (let i = 0; i < v.length; i++) p.push(i in v ? show(v[i]) : '_'); return '[' + p.join(',') + ']'; } if (v && typeof v === 'object') { const p = []; for (const k of Object.keys(v)) p.push(k + ':' + show(v[k])); return '{' + p.join(',') + '}'; } return String(v); };
var SNIPPETS = ["var a = [1]; for (var i = 0; i < 1000; i++) a = [a]; a.flat(Infinity).length",
"var a = [1]; for (var i = 0; i < 2000; i++) a = [a]; a.flat(Infinity).length",
"var a = [1]; for (var i = 0; i < 3000; i++) a = [a]; a.flat(Infinity).length",
"var a = [1]; for (var i = 0; i < 4000; i++) a = [a]; a.flat(Infinity).length",
"var a = [1]; for (var i = 0; i < 6000; i++) a = [a]; a.flat(Infinity).length",
"var a = [1]; for (var i = 0; i < 8000; i++) a = [a]; a.flat(Infinity).length",
"var a = [1]; for (var i = 0; i < 10000; i++) a = [a]; a.flat(Infinity).length",
"var a = [1]; for (var i = 0; i < 3000; i++) a = [a]; a.flat(2999).length",
"var a = [1]; for (var i = 0; i < 100000; i++) a = [a]; String(a).length"];
for (var s of SNIPPETS) { var r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } print(s + " => " + r); }
