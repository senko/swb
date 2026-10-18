// Makes the snippet cases of the Chromium probe file of ArrayBuffer and
// DataView (M7 feature 11a) from a list of snippets and, from the probe
// output, the script and the expected output of
// `crates/js/tests/builtins_binary.rs`:
//
//   node crates/js/tests/data/gen-binary-cases.js probe \
//     crates/js/tests/data/binary-snippets.txt tools/probes/js-binary.json
//   just probe --json tools/probes/js-binary.json > probe.json
//   node crates/js/tests/data/gen-binary-cases.js cases probe.json crates/js/tests/data
//
// The snippet file has one snippet per line; a line that starts with `#`
// is a comment, and a line `## NAME` starts a group. Each group becomes
// one probe case (or several, of at most PER_CASE snippets). The cases of
// the probe file whose names start with `facts` are hand-written and stay
// as they are. The probe evaluates each snippet with an indirect eval and
// prints "SNIPPET => ok VALUE" or "SNIPPET => err NAME: MESSAGE" (`show`
// below; a BigInt shows as `1n`); the script for swb-js does the same, so
// the expected output is Chromium 148's. The helpers of PRELUDE are
// globals, so the snippets can use them: `hex(x)` gives the bytes of a
// buffer or of the buffer of a view in hexadecimal (read with a DataView,
// because the typed arrays come with feature 11b), `filled(bytes, max)` a
// new buffer with these bytes (resizable up to `max` when it is given),
// `nt(f)` a constructor whose `prototype` getter calls `f` (for the steps
// after `OrdinaryCreateFromConstructor`). The snippets of KNOWN, where swb
// differs from Chromium on purpose (ADR 0026 section 15), are left out of
// the script.

const fs = require('fs');
const path = require('path');

const PER_CASE = 60;

// Snippets whose Chromium 148 result swb does not reproduce on purpose
// (docs/adr/0026 section 15, M7 11a).
const KNOWN = [
  // ToIndex of the length rejects 2^53 and Infinity at once (§25.1.4.1 step
  // 2); V8 reads maxByteLength and the prototype first and rejects the
  // length later.
  'new ArrayBuffer(2 ** 53, { maxByteLength: 1 })',
  "Reflect.construct(ArrayBuffer, [2 ** 53], nt(() => { throw new Error('proto'); }))",
  'new ArrayBuffer(Infinity, { maxByteLength: 1 })',
  "new ArrayBuffer(2 ** 53, { get maxByteLength() { throw new Error('mbl'); } })",
  "new ArrayBuffer(Infinity, { get maxByteLength() { throw new Error('mbl'); } })",
  // resize converts the length before it checks for a detached buffer
  // (§25.1.6.6 steps 4 and 5); V8 checks the buffer first.
  '(() => { const b = new ArrayBuffer(4, { maxByteLength: 8 }); b.transfer(); return b.resize(-1); })()',
  // The NaN bits that the operators make (V8 folds 0 / 0 to the positive
  // quiet NaN and makes a NaN of its own for undefined; swb has the NaN of
  // the hardware's division, 0xFFF8...). The specification lets the
  // implementation choose them (§25.1.3.17).
  '(() => { const d = new DataView(new ArrayBuffer(4)); d.setFloat32(0); return hex(d); })()',
  '(() => { const d = new DataView(new ArrayBuffer(8)); d.setFloat64(0, 0 / 0); return hex(d); })()',
];

const PRELUDE = `globalThis.hex = function (x) {
  var b = x instanceof DataView ? x.buffer : x;
  if (b.detached) return 'detached';
  var d = new DataView(b), s = '';
  for (var i = 0; i < d.byteLength; i++) { var v = d.getUint8(i); s += (v < 16 ? '0' : '') + v.toString(16); }
  return s;
};
globalThis.filled = function (bytes, max) {
  var b = max === undefined ? new ArrayBuffer(bytes.length) : new ArrayBuffer(bytes.length, { maxByteLength: max });
  var d = new DataView(b);
  for (var i = 0; i < bytes.length; i++) d.setUint8(i, bytes[i]);
  return b;
};
globalThis.nt = function (f) {
  var c = function () {}.bind(null);
  Object.defineProperty(c, 'prototype', { get: function () { f(); return Object.prototype; } });
  return c;
};`;

const SHOW = `const show = v => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'bigint') return String(v) + 'n';
  if (typeof v === 'function') return 'function';
  if (typeof v === 'number' && Object.is(v, -0)) return '-0';
  if (Array.isArray(v)) return '[' + v.map(x => typeof x === 'object' && x !== null ? 'object' : show(x)).join(',') + ']';
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

function oneLine(text) {
  return text.replace(/\n\s*/g, ' ');
}

function probe(snippetsFile, out) {
  const kept = fs.existsSync(out)
    ? JSON.parse(fs.readFileSync(out, 'utf8')).cases.filter(c => c.name.startsWith('facts'))
    : [];
  const cases = [...kept];
  for (const group of readGroups(snippetsFile)) {
    for (let i = 0; i < group.snippets.length; i += PER_CASE) {
      const part = group.snippets.slice(i, i + PER_CASE);
      const js = `(() => { ${oneLine(PRELUDE)} const $out = []; ${oneLine(SHOW)} ` +
        `const SNIPPETS = ${JSON.stringify(part)}; ${RUN} return $out; })()`;
      cases.push({
        name: `${group.name}-${i / PER_CASE}`,
        html: '<!DOCTYPE html><p>x</p>',
        measure: [{ js }],
      });
    }
  }
  fs.writeFileSync(out, JSON.stringify({ viewport: [800, 600], cases }, null, 1) + '\n');
  console.log(`${cases.length} probe cases (${kept.length} kept)`);
}

function cases(input, outDir) {
  const result = JSON.parse(fs.readFileSync(input, 'utf8'));
  const snippets = [];
  const expected = [];
  for (const c of result.cases) {
    if (c.name.startsWith('facts')) continue;
    for (const r of c.results) {
      if (r.kind !== 'js' || !Array.isArray(r.value)) throw new Error('bad result in ' + c.name);
      for (const line of r.value) {
        const m = /^(.*?) => (ok|err) (.*)$/s.exec(line);
        if (!m) throw new Error('bad line: ' + line);
        if (KNOWN.includes(m[1])) continue;
        snippets.push(m[1]);
        expected.push(line);
      }
    }
  }
  fs.writeFileSync(
    path.join(outDir, 'binary_cases.js'),
    '// Made by gen-binary-cases.js from tools/probes/js-binary.json.\n' +
      PRELUDE + '\n' + SHOW.replace(/^const /, 'var ') + '\n' +
      'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
      'var $out = { push: print };\n' + RUN + '\n',
  );
  fs.writeFileSync(path.join(outDir, 'binary_cases.expected'), expected.join('\n') + '\n');
  console.log(`binary_cases: ${snippets.length} snippets`);
}

const [mode, a, b] = process.argv.slice(2);
if (mode === 'probe') probe(a, b);
else if (mode === 'cases') cases(a, b);
else throw new Error('usage: gen-binary-cases.js probe|cases IN OUT');
