// Makes the snippet cases of the Chromium probe file of the typed arrays
// (M7 features 11b and 11c) from lists of snippets and, from the probe
// output, the scripts and the expected outputs of
// `crates/js/tests/builtins_typed.rs`:
//
//   node crates/js/tests/data/gen-typed-cases.js probe \
//     crates/js/tests/data/typed-snippets.txt \
//     crates/js/tests/data/typed-method-snippets.txt tools/probes/js-typed.json
//   just probe --json tools/probes/js-typed.json > probe.json
//   node crates/js/tests/data/gen-typed-cases.js cases probe.json crates/js/tests/data
//
// The groups of typed-method-snippets.txt (feature 11c: the methods of
// `%TypedArray%.prototype`, `Atomics`, the base64 and hex methods) have
// names that start with `m-`; their cases make typed_methods_cases.js and
// typed_methods_cases.expected, the others typed_cases.js and
// typed_cases.expected.
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
// whose `@@species` getter returns `f()` (for `TypedArraySpeciesCreate`),
// and for feature 11c `ach(T, action, f)` (an array of `T` over a
// resizable buffer that `action` detaches, shrinks, grows or empties, with
// `f(t, o, run, v)`: `o(x)` an object whose conversion does the action and
// gives `x`), `fin` (the result and the array as text), `arv` (the same
// for `Atomics`), `ao` and `ace` (an `Atomics` operation on a new array), `sb` and `sh`
// (`setFromBase64` and `setFromHex` into a new `Uint8Array`: the result or
// the error and the bytes), `sq` and `sc` (the calls of a comparator).
// The snippets of KNOWN, where swb differs from Chromium on purpose (ADR
// 0026 section 15), are left out of the script, and so are the groups
// whose names start with `m-known-` (KNOWN_GROUPS says why). The probe file also gets
// the cases `detach`, made from typed_detach.js (the callback cases of
// typed_hostile.rs), and `detach-methods`, made from
// typed_methods_detach.js; `cases` writes their results to
// typed_detach.expected and typed_methods_detach.expected, with the
// KNOWN_DETACH lines replaced by swb's.

const fs = require('fs');
const path = require('path');

const PER_CASE = 250;

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
globalThis.acts = function (b, S) {
  return { detach: function () { b.transfer(); }, shrink: function () { b.resize(2 * S); }, grow: function () { b.resize(16 * S); }, shrink0: function () { b.resize(0); } };
};
globalThis.ach = function (T, act, f) {
  var S = T.BYTES_PER_ELEMENT, b = rab(8 * S, 16 * S), t = new T(b), big = T === BigInt64Array;
  for (var i = 0; i < t.length; i++) t[i] = big ? BigInt(i + 1) : i + 1;
  var run = acts(b, S)[act];
  var o = function (v) { return { valueOf: function () { run(); return v; }, toString: function () { run(); return '-'; } }; };
  return f(t, o, run, function (n) { return big ? BigInt(n) : n; });
};
globalThis.fin = function (r, t) {
  return [r !== null && typeof r === 'object' ? list(r) : String(r), t.length, t.byteLength, list(t)].join('|');
};
globalThis.arv = function (T, act, f) {
  var S = T.BYTES_PER_ELEMENT, b = rab(8 * S, 16 * S), t = new T(b), run = acts(b, S)[act];
  var o = function (v) { return { valueOf: function () { run(); return v; } }; };
  return f(t, o, T === BigInt64Array ? 1n : 1) + '|' + list(t);
};
globalThis.ao = function (T, op, v, init) {
  var big = T === BigInt64Array || T === BigUint64Array, t = new T(4);
  t[1] = init === undefined ? (big ? 250n : 250) : init;
  var r = Atomics[op](t, 1, v);
  return (op === 'store' ? typeof r + ':' + (Object.is(r, -0) ? '-0' : String(r)) : String(r)) + '|' + list(t);
};
globalThis.ace = function (T, e, r, init) {
  var t = new T(4); t[0] = init; var x = Atomics.compareExchange(t, 0, e, r); return x + '|' + list(t);
};
globalThis.sb = function (n, s, opts) {
  var u = new Uint8Array(n);
  try { var r = u.setFromBase64(s, opts); return r.read + ',' + r.written + '|' + list(u); } catch (e) { return e.name + ': ' + e.message + '|' + list(u); }
};
globalThis.sh = function (n, s) {
  var u = new Uint8Array(n);
  try { var r = u.setFromHex(s); return r.read + ',' + r.written + '|' + list(u); } catch (e) { return e.name + ': ' + e.message + '|' + list(u); }
};
globalThis.sq = function (n) { var a = []; for (var i = 0; i < n; i++) a.push((i * 7 + 3) % 11); return a; };
globalThis.sc = function (T, data, cmp, method, big) {
  var log = [], t = new T(big ? data.map(BigInt) : data);
  var r = t[method || 'sort'](function (a, b) { log.push(a + ':' + b); return cmp(a, b); });
  return r.join() + '|' + log.join(' ');
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

// The groups of typed-method-snippets.txt that swb does not reproduce on
// purpose (ADR 0026 section 15, M7 11c). They stay in the probe file, so
// that the Chromium results can be read, and out of the test.
const KNOWN_GROUPS = {
  // V8's text of an object in a message (NoSideEffectsToString) is
  // "[object Object]" for an object with its own `toString` and for the
  // instance of a function without a name, and "[object Object]" for a
  // length-tracking typed array in the message of Atomics.wait; swb
  // prints "#<Object>" and "[object Int32Array]".
  'm-known-text': 'object text',
  // V8 calls the `toLocaleString` of an element with (locales, options),
  // as ECMA-402 says; without Intl swb passes no arguments
  // (ECMA-262 section 23.2.3.31, as Array.prototype.toLocaleString).
  'm-known-locale': 'toLocaleString arguments',
  // TypedArraySpeciesCreate with a constructor of the other content type:
  // swb throws the TypeError of the specification's content type check at
  // once ("Cannot mix BigInt and other types, use explicit conversions"),
  // V8 the TypeError of the first conversion, or none when no conversion
  // fails (map and filter do not check the content type).
  'm-known-species': 'species content type',
  // Atomics.isLockFree(1.5): ToIntegerOrInfinity makes it 1 (true, as the
  // specification says); V8 compares the Number itself (false).
  'm-known-lockfree': 'isLockFree truncation',
  // setFromBase64 into a target that is too small: FromBase64 returns as
  // soon as the output is full (the proposal text, and swb), V8 goes on
  // reading the input and throws a SyntaxError for an error after that
  // point.
  'm-known-set-base64': 'setFromBase64 error after a full target',
};

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
const DETACH_METHODS = path.join(__dirname, 'typed_methods_detach.js');

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

// The lines of the detach script of the methods (typed_methods_detach.js)
// where swb follows the specification and V8 does not (KNOWN): `map` whose
// species constructor returns the receiver, which a callback then detaches
// or shrinks: `Set` on an index that is no longer valid does nothing
// (TypedArraySetElement), V8 throws a TypeError.
for (const action of ['detach', 'shrink']) {
  const results = {
    Uint8Array: action === 'detach' ? '0' : '1',
    Int16Array: '0',
    Float64Array: '0',
    BigInt64Array: '0',
  };
  for (const [type, length] of Object.entries(results)) {
    const big = type === 'BigInt64Array';
    KNOWN_DETACH[`${action} ${type} map result is the receiver`] =
      `ok [[${big ? '1n,2n,undefined,undefined' : '1,2,undefined,undefined'}],true,${length}]`;
  }
}

function detachLines(lines) {
  return lines.map(line => {
    const [name] = line.split(' => ');
    return name in KNOWN_DETACH ? `${name} => ${KNOWN_DETACH[name]}` : line;
  });
}

function detachCase(file, name) {
  const lines = fs.readFileSync(file, 'utf8').split('\n')
    .filter(line => !/^\s*\/\//.test(line) && line.trim() !== '');
  if (!lines[lines.length - 1].startsWith('print(')) throw new Error(file + ' must end with print');
  const body = lines.slice(0, -1).join(' ');
  return { name, html: '<!DOCTYPE html><p>x</p>', measure: [{ js: `(() => { ${body} return out; })()` }] };
}

function probe(snippetsFiles, out) {
  const kept = fs.existsSync(out)
    ? JSON.parse(fs.readFileSync(out, 'utf8')).cases.filter(c => c.name.startsWith('facts'))
    : [];
  const cases = [...kept, detachCase(DETACH, 'detach'), detachCase(DETACH_METHODS, 'detach-methods')];
  const groups = snippetsFiles.flatMap(readGroups);
  for (const group of groups) {
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

function writeCases(outDir, name, snippets, expected) {
  fs.writeFileSync(
    path.join(outDir, `${name}.js`),
    '// Made by gen-typed-cases.js from tools/probes/js-typed.json.\n' +
      PRELUDE + '\n' + SHOW.replace(/^const /, 'var ') + '\n' +
      'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
      'var $out = { push: print };\n' + RUN + '\n',
  );
  fs.writeFileSync(path.join(outDir, `${name}.expected`), expected.join('\n') + '\n');
  console.log(`${name}: ${snippets.length} snippets`);
}

function cases(input, outDir) {
  const result = JSON.parse(fs.readFileSync(input, 'utf8'));
  const sets = { typed_cases: [[], []], typed_methods_cases: [[], []] };
  for (const c of result.cases) {
    if (c.name.startsWith('facts')) continue;
    if (c.name === 'detach' || c.name === 'detach-methods') {
      fs.writeFileSync(
        path.join(outDir, c.name === 'detach' ? 'typed_detach.expected' : 'typed_methods_detach.expected'),
        detachLines(c.results[0].value).join('\n') + '\n',
      );
      continue;
    }
    if (Object.keys(KNOWN_GROUPS).some(group => c.name.startsWith(group + '-'))) continue;
    const [snippets, expected] = sets[c.name.startsWith('m-') ? 'typed_methods_cases' : 'typed_cases'];
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
  for (const [name, [snippets, expected]] of Object.entries(sets)) writeCases(outDir, name, snippets, expected);
}

const args = process.argv.slice(2);
const mode = args[0];
if (mode === 'probe') probe(args.slice(1, -1), args[args.length - 1]);
else if (mode === 'cases') cases(args[1], args[2]);
else throw new Error('usage: gen-typed-cases.js probe SNIPPETS... OUT | cases PROBE_OUTPUT DIR');
