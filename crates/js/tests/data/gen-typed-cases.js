// Makes the snippet cases of the Chromium probe file of the typed arrays
// (M7 feature 11b) from a list of snippets and, from the probe output, the
// script and the expected output of `crates/js/tests/builtins_typed.rs`:
//
//   node crates/js/tests/data/gen-typed-cases.js probe \
//     crates/js/tests/data/typed-snippets.txt tools/probes/js-typed.json
//   just probe --json tools/probes/js-typed.json > probe.json
//   node crates/js/tests/data/gen-typed-cases.js cases probe.json crates/js/tests/data
//
// The format is that of gen-binary-cases.js: one snippet per line, `#`
// starts a comment, `## NAME` a group; each group becomes one probe case
// (or several, of at most PER_CASE snippets); the cases whose names start
// with `facts` are hand-written and stay. The probe evaluates each snippet
// with an indirect eval and prints "SNIPPET => ok VALUE" or "SNIPPET =>
// err NAME: MESSAGE" (`show`; a BigInt shows as `1n`). The helpers of
// PRELUDE are globals: `list(t)` the elements of a typed array (or of any
// array-like) read by index and joined with commas (`-0` and `1n` shown as
// such), `rab(n, max)` a resizable buffer, `desc(o, k)` a property
// descriptor as text, `keys(o)` the own keys as text, `det(x)` detaches
// the buffer of a view (or a buffer) with `transfer` and returns `x`, `nt(f)`
// a constructor whose `prototype` getter calls `f` (as in
// gen-binary-cases.js), `sp(f)` an object for a `constructor` property
// whose `@@species` getter returns `f()` (for `TypedArraySpeciesCreate`).
// The snippets of KNOWN, where swb differs from Chromium on purpose (ADR
// 0026 section 15), and those of LATER, which need the methods of feature
// 11c, are left out of the script. The probe file also gets the case
// `detach`, made from typed_detach.js (the callback cases of
// typed_hostile.rs); `cases` writes its result to typed_detach.expected,
// with the KNOWN_DETACH lines replaced by swb's.

const fs = require('fs');
const path = require('path');

const PER_CASE = 60;

// Snippets whose Chromium 148 result swb does not reproduce on purpose
// (docs/adr/0026 section 15, M7 11b).
const KNOWN = [
  // TestIntegrityLevel: the elements of a typed array are configurable
  // (§10.4.5 [[GetOwnProperty]]), so a non-extensible typed array with
  // elements is not sealed; V8 answers true.
  'Object.isSealed(Object.preventExtensions(new Uint8Array(1)))',
  // An Array Iterator whose step threw is finished (GeneratorStart: the
  // closure of CreateArrayIterator completed); V8 lets the iterator of a
  // typed array go on after its TypeError.
  '(() => { const t = new Uint8Array(2); const it = t.values(); it.next(); det(t); try { it.next(); } catch (e) {} return JSON.stringify(it.next()); })()',
  '(() => { const b = rab(4, 8); const t = new Uint8Array(b, 2); const it = t.values(); it.next(); b.resize(1); try { it.next(); } catch (e) {} b.resize(8); return JSON.stringify(it.next()); })()',
  // subarray of a length-tracking array with `end` undefined passes two
  // arguments to the species constructor (§23.2.3.30 step 15); V8 passes
  // a third, undefined.
  '(() => { const b = rab(8, 16); const t = new Uint8Array(b); const log = []; t.constructor = sp(() => function (...a) { log.push(a.length, a[1], a[2]); return new Uint8Array(...a); }); t.subarray(1); return log.join(); })()',
];

// Snippets that need the methods of `%TypedArray%.prototype` that come with
// feature 11c: `toString` (the function of `Array.prototype`) and `join`,
// which `toString` calls.
const LATER = [
  'Object.getPrototypeOf(Int8Array).prototype.toString === Array.prototype.toString',
  'String(new Uint8Array(0))',
  'Object.assign(new Uint8Array(3), [7, 8]).join === undefined',
  'Array.prototype.reverse.call(new Uint8Array([1, 2, 3])).join === undefined',
  'Array.prototype.toString.call(new Uint8Array([2, 1]))',
  "new Uint8Array(2) == '0,0'",
  'new Uint8Array([1]) + 1',
  '`${new Uint8Array([1, 2])}`',
  'new Uint8Array([3]) * 2',
  'Number(new Uint8Array([3]))',
];

const PRELUDE = `globalThis.list = function (t) {
  var s = [];
  for (var i = 0; i < t.length; i++) {
    var v = t[i];
    s.push(typeof v === 'bigint' ? v + 'n' : (v === 0 && 1 / v < 0) ? '-0' : String(v));
  }
  return s.join(',');
};
globalThis.rab = function (n, max) { return new ArrayBuffer(n, { maxByteLength: max }); };
globalThis.desc = function (o, k) {
  var d = Object.getOwnPropertyDescriptor(o, k);
  if (!d) return 'none';
  var head = 'value' in d ? 'v=' + (typeof d.value === 'bigint' ? d.value + 'n' : typeof d.value === 'function' ? 'function' : typeof d.value === 'object' && d.value !== null ? 'object' : String(d.value)) + ' w=' + d.writable : 'get=' + typeof d.get + ' set=' + typeof d.set;
  return head + ' e=' + d.enumerable + ' c=' + d.configurable;
};
globalThis.keys = function (o) { return Reflect.ownKeys(o).map(String).join(); };
globalThis.det = function (x) { (x instanceof ArrayBuffer ? x : x.buffer).transfer(); return x; };
globalThis.nt = function (f) {
  var c = function () {}.bind(null);
  Object.defineProperty(c, 'prototype', { get: function () { f(); return Object.prototype; } });
  return c;
};
globalThis.sp = function (f) {
  var c = {};
  Object.defineProperty(c, Symbol.species, { get: function () { return f(); } });
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

// The script of `typed_hostile.rs` whose output comes from Chromium: the
// probe case `detach` runs it (without its comments and its last line,
// which prints) and returns its lines.
const DETACH = path.join(__dirname, 'typed_detach.js');

// The lines of the detach script where swb follows the specification and
// V8 does not (KNOWN): `new T(typedArray)` reads the length of the source
// after the `prototype` getter of `new.target` (§23.2.5.1 step 6.b:
// AllocateTypedArray, then InitializeTypedArrayFromTypedArray); V8 reads
// it before, so a getter that shrinks the source makes V8 copy elements
// past its end (NaN, 0, or a TypeError for BigInt) and one that grows it
// makes V8 copy the old length.
const KNOWN_DETACH = {
  'shrink Uint8Array ctor from a typed array, prototype getter': 'ok [[0]]',
  'shrink Int16Array ctor from a typed array, prototype getter': 'ok [[]]',
  'shrink Float64Array ctor from a typed array, prototype getter': 'ok [[]]',
  'shrink BigInt64Array ctor from a typed array, prototype getter': 'ok [[]]',
  'grow Uint8Array ctor from a typed array, prototype getter': 'ok [[0,0,0,0,0,0,0,0]]',
  'grow Int16Array ctor from a typed array, prototype getter': 'ok [[0,0,0,0,0,0,0,0]]',
  'grow Float64Array ctor from a typed array, prototype getter': 'ok [[0,0,0,0,0,0,0,0]]',
  'grow BigInt64Array ctor from a typed array, prototype getter':
    'ok [[0n,0n,0n,0n,0n,0n,0n,0n]]',
};

function detachLines(lines) {
  return lines.map(line => {
    const [name] = line.split(' => ');
    return name in KNOWN_DETACH ? `${name} => ${KNOWN_DETACH[name]}` : line;
  });
}

function detachCase() {
  const lines = fs.readFileSync(DETACH, 'utf8').split('\n')
    .filter(line => !/^\s*\/\//.test(line) && line.trim() !== '');
  if (!lines[lines.length - 1].startsWith('print(')) throw new Error('typed_detach.js must end with print');
  const body = lines.slice(0, -1).join(' ');
  return { name: 'detach', html: '<!DOCTYPE html><p>x</p>', measure: [{ js: `(() => { ${body} return out; })()` }] };
}

function probe(snippetsFile, out) {
  const kept = fs.existsSync(out)
    ? JSON.parse(fs.readFileSync(out, 'utf8')).cases.filter(c => c.name.startsWith('facts'))
    : [];
  const cases = [...kept, detachCase()];
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
    if (c.name === 'detach') {
      fs.writeFileSync(
        path.join(outDir, 'typed_detach.expected'),
        detachLines(c.results[0].value).join('\n') + '\n',
      );
      continue;
    }
    for (const r of c.results) {
      if (r.kind !== 'js' || !Array.isArray(r.value)) throw new Error('bad result in ' + c.name);
      for (const line of r.value) {
        const m = /^(.*?) => (ok|err) (.*)$/s.exec(line);
        if (!m) throw new Error('bad line: ' + line);
        if (KNOWN.includes(m[1]) || LATER.includes(m[1])) continue;
        snippets.push(m[1]);
        expected.push(line);
      }
    }
  }
  fs.writeFileSync(
    path.join(outDir, 'typed_cases.js'),
    '// Made by gen-typed-cases.js from tools/probes/js-typed.json.\n' +
      PRELUDE + '\n' + SHOW.replace(/^const /, 'var ') + '\n' +
      'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
      'var $out = { push: print };\n' + RUN + '\n',
  );
  fs.writeFileSync(path.join(outDir, 'typed_cases.expected'), expected.join('\n') + '\n');
  console.log(`typed_cases: ${snippets.length} snippets`);
}

const [mode, a, b] = process.argv.slice(2);
if (mode === 'probe') probe(a, b);
else if (mode === 'cases') cases(a, b);
else throw new Error('usage: gen-typed-cases.js probe|cases IN OUT');
