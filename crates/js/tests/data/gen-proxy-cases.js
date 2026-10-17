// Makes the Chromium probe file of the Proxy cases (M7 feature 9a) from a
// list of snippets and, from the probe output, the script and the expected
// output of `crates/js/tests/builtins_proxy.rs`:
//
//   node crates/js/tests/data/gen-proxy-cases.js probe \
//     crates/js/tests/data/proxy-snippets.txt tools/probes/js-proxy.json
//   just probe --json tools/probes/js-proxy.json > probe.json
//   node crates/js/tests/data/gen-proxy-cases.js cases probe.json crates/js/tests/data
//
// The snippet file has one snippet per line; a line that starts with `#`
// is a comment, and a line `## NAME` starts a group. Each group becomes
// one probe case (or several, of at most PER_CASE snippets). The probe
// defines `tracer` (PRELUDE), evaluates each snippet with an indirect eval
// and prints "SNIPPET => ok VALUE" or "SNIPPET => err NAME: MESSAGE"
// (`show` below); the script for swb-js does the same, so the expected
// output is Chromium 148's. A group whose name ends in `-heavy` goes to
// proxy_cases_heavy.* (run without the GC stress mode). Snippets that
// contain a part of EXCLUDED, and the snippets of KNOWN (where swb follows
// the specification and Chromium does not; ADR 0026 section 15), are left
// out of the script.

const fs = require('fs');
const path = require('path');

const PER_CASE = 60;

// Snippets with these parts need features that swb does not have yet
// (`Date`: feature 12; `WeakMap`: 9b).
const EXCLUDED = ['Date', 'WeakMap'];

// Snippets whose Chromium 148 result differs from the specification; swb
// follows the specification (see docs/adr/0026 section 15, M7 9a). In a
// `with` body V8 reads and writes the binding object without the second
// `HasProperty` of GetBindingValue and SetMutableBinding (§9.1.1.2.5,
// §9.1.1.2.6), and resolves the target of an assignment after its right
// side (§13.15.2 resolves it first). The last one is a message: V8 words a
// `construct` trap that is not callable "is not a function".
const KNOWN = [
  "const log = []; const p = tracer(log, 'p', { x: 1 }); with (p) { x; } log.join()",
  "const log = []; const p = tracer(log, 'p', { x: 1 }); with (p) { x = 2; } log.join() + ' ' + p.x",
  "const log = []; const p = tracer(log, 'p', { f() { return this === p; } }); let r; with (p) { r = f(); } log.join() + ' ' + r",
  "const log = []; const p = tracer(log, 'p', { x: 1 }); with (p) { typeof x; } log.join()",
  'new (new Proxy(function () {}, { construct: 5 }))()',
];

// `tracer(log, name, target)`: a proxy whose handler logs each trap that
// an operation asks for (and the key, for the traps that have one) and
// forwards it with `Reflect`.
const PRELUDE = `globalThis.tracer = (log, name, target) => new Proxy(target, new Proxy({}, {
  get(_, trap) {
    return function (...args) {
      const k = args[1];
      const keyed = trap !== 'apply' && trap !== 'construct' && (typeof k === 'string' || typeof k === 'symbol');
      log.push(name + '.' + String(trap) + (keyed ? ':' + String(k) : ''));
      return Reflect[trap](...args);
    };
  },
}));`;

const SHOW = `const show = v => {
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
};`;

// The output list is `$out`: a snippet may declare `out` (in swb-js the
// snippets share the global scope with the runner).
const RUN = 'for (const s of SNIPPETS) { let r; try { r = "ok " + show((0, eval)(s)); } ' +
  'catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } ' +
  '$out.push(s + " => " + r); }';

function readGroups(file) {
  const groups = [];
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    if (line.startsWith('## ')) {
      groups.push({ name: line.slice(3).trim(), snippets: [] });
    } else if (line.trim() !== '' && !line.startsWith('#')) {
      if (groups.length === 0) throw new Error('a snippet before the first group');
      groups[groups.length - 1].snippets.push(line);
    }
  }
  return groups;
}

function probe(snippetsFile, out) {
  const cases = [];
  for (const group of readGroups(snippetsFile)) {
    for (let i = 0; i < group.snippets.length; i += PER_CASE) {
      const part = group.snippets.slice(i, i + PER_CASE);
      const js = `(() => { const $out = []; ${PRELUDE.replace(/\n\s*/g, ' ')} ` +
        `${SHOW.replace(/\n\s*/g, ' ')} ` +
        `const SNIPPETS = ${JSON.stringify(part)}; ${RUN} return $out; })()`;
      cases.push({
        name: `${group.name}-${i / PER_CASE}`,
        html: '<!DOCTYPE html><p>x</p>',
        measure: [{ js }],
      });
    }
  }
  fs.writeFileSync(out, JSON.stringify({ viewport: [800, 600], cases }, null, 1) + '\n');
  console.log(`${cases.length} probe cases`);
}

function cases(input, outDir) {
  const result = JSON.parse(fs.readFileSync(input, 'utf8'));
  const outputs = { proxy_cases: [[], []], proxy_cases_heavy: [[], []] };
  for (const c of result.cases) {
    const heavy = /-heavy-\d+$/.test(c.name);
    const [snippets, expected] = outputs[heavy ? 'proxy_cases_heavy' : 'proxy_cases'];
    for (const r of c.results) {
      if (r.kind !== 'js' || !Array.isArray(r.value)) throw new Error('bad result in ' + c.name);
      for (const line of r.value) {
        const m = /^(.*?) => (ok|err) (.*)$/s.exec(line);
        if (!m) throw new Error('bad line: ' + line);
        if (EXCLUDED.some((part) => m[1].includes(part)) || KNOWN.includes(m[1])) continue;
        snippets.push(m[1]);
        expected.push(line);
      }
    }
  }
  for (const [name, [snippets, expected]] of Object.entries(outputs)) {
    fs.writeFileSync(
      path.join(outDir, name + '.js'),
      '// Made by gen-proxy-cases.js from tools/probes/js-proxy.json.\n' +
        PRELUDE + '\n' +
        SHOW.replace(/^const /, 'var ') + '\n' +
        'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
        'var $out = { push: print };\n' + RUN + '\n',
    );
    fs.writeFileSync(path.join(outDir, name + '.expected'), expected.join('\n') + '\n');
    console.log(`${name}: ${snippets.length} snippets`);
  }
}

const [mode, a, b] = process.argv.slice(2);
if (mode === 'probe') probe(a, b);
else if (mode === 'cases') cases(a, b);
else throw new Error('usage: gen-proxy-cases.js probe|cases IN OUT');
