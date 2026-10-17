// Made by gen-proxy-cases.js from tools/probes/js-proxy.json.
globalThis.tracer = (log, name, target) => new Proxy(target, new Proxy({}, {
  get(_, trap) {
    return function (...args) {
      const k = args[1];
      const keyed = trap !== 'apply' && trap !== 'construct' && (typeof k === 'string' || typeof k === 'symbol');
      log.push(name + '.' + String(trap) + (keyed ? ':' + String(k) : ''));
      return Reflect[trap](...args);
    };
  },
}));
var show = v => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'function') return 'function';
  if (typeof v === 'number' && Object.is(v, -0)) return '-0';
  if (Array.isArray(v)) {
    if (v.length > 50) return 'Array(' + v.length + ')';
    const p = [];
    for (let i = 0; i < v.length; i++) p.push(typeof v[i] === 'object' && v[i] !== null ? 'object' : show(v[i]));
    return '[' + p.join(',') + ']';
  }
  if (v && typeof v === 'object') return 'object';
  return String(v);
};
var SNIPPETS = ["let p = {}; for (let i = 0; i < 1000000; i++) p = new Proxy(p, {}); try { p.x; 'no error'; } catch (e) { e.constructor.name + ': ' + e.message; }",
"let p = function () { return 1; }; for (let i = 0; i < 100000; i++) p = new Proxy(p, {}); try { String(p()); } catch (e) { e.constructor.name + ': ' + e.message; }",
"let p = {}; for (let i = 0; i < 100; i++) p = new Proxy(p, {}); p.y = 2; p.y",
"let p = {}; for (let i = 0; i < 1000; i++) p = new Proxy(p, {}); 'z' in p",
"const keys = []; for (let i = 0; i < 100000; i++) keys.push('k' + i); const t = {}; for (const k of keys) t[k] = 1; Object.preventExtensions(t); Reflect.ownKeys(new Proxy(t, { ownKeys() { return keys; } })).length",
"const keys = []; for (let i = 0; i < 100000; i++) keys.push('k' + i); Reflect.ownKeys(new Proxy({}, { ownKeys() { return keys; } })).length",
"const keys = []; for (let i = 0; i < 100000; i++) keys.push('k' + i); keys.push('k5'); try { Reflect.ownKeys(new Proxy({}, { ownKeys() { return keys; } })); } catch (e) { e.message }",
"const p = new Proxy({}, { get(t, k, r) { return r[k]; } }); try { p.x; } catch (e) { e.constructor.name + ': ' + e.message }",
"const r = Proxy.revocable({}, { get(t, k) { return r.proxy[k]; } }); try { r.proxy.x; } catch (e) { e.constructor.name + ': ' + e.message }"];
var $out = { push: print };
for (const s of SNIPPETS) { let r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } $out.push(s + " => " + r); }
