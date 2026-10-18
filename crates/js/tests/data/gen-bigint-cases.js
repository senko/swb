// Makes the snippet cases of the Chromium probe file of BigInt (M7 feature
// 10b) from a list of snippets and, from the probe output, the script and
// the expected output of `crates/js/tests/builtins_bigint.rs`:
//
//   node crates/js/tests/data/gen-bigint-cases.js probe \
//     crates/js/tests/data/bigint-snippets.txt tools/probes/js-bigint.json
//   just probe --json tools/probes/js-bigint.json > probe.json
//   node crates/js/tests/data/gen-bigint-cases.js cases probe.json crates/js/tests/data
//
// The snippet file has one snippet per line; a line that starts with `#`
// is a comment, and a line `## NAME` starts a group. Each group becomes
// one probe case (or several, of at most PER_CASE snippets). The cases of
// the probe file whose names start with `limit` are hand-written (10a) and
// stay as they are. The probe evaluates each snippet with an indirect eval
// and prints "SNIPPET => ok VALUE" or "SNIPPET => err NAME: MESSAGE"
// (`show` below; a BigInt shows as `1n`); the script for swb-js does the
// same, so the expected output is Chromium 148's. The group `sizes`
// (values beyond swb's size limit of 2^18 bits, Chromium's is 2^30 − 64)
// and the snippets of KNOWN, where swb differs from Chromium on purpose
// (ADR 0026 section 15), are left out of the script.

const fs = require('fs');
const path = require('path');

const PER_CASE = 60;

// Groups whose results depend on the size limit (swb's results are in
// crates/js/tests/bigint_hostile.rs).
const SKIPPED_GROUPS = ['sizes'];

// Snippets whose Chromium 148 result swb does not reproduce on purpose
// (docs/adr/0026 section 15, M7 10b).
const KNOWN = [
  // ECMA-402 locales: swb has no Intl, `toLocaleString` ignores its
  // arguments and groups the digits as en-US.
  "(1234n).toLocaleString('de-DE')",
  // `new Date` is M7 feature 12.
  'new Date(1n)',
  // The text of a callee `o[Symbol.iterator]` (a computed key that is not
  // a literal or an identifier) in "is not a function": swb shows
  // "(intermediate value)" for every receiver, also Numbers (backlog).
  "(() => { try { (1n)[Symbol.iterator](); } catch (e) { return e.message; } })()",
  "(() => { try { (5)[Symbol.iterator](); } catch (e) { return e.message; } })()",
  "(() => { try { const o = 1n; o[Symbol.iterator](); } catch (e) { return e.message; } })()",
];

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

function probe(snippetsFile, out) {
  const kept = fs.existsSync(out)
    ? JSON.parse(fs.readFileSync(out, 'utf8')).cases.filter(c => c.name.startsWith('limit'))
    : [];
  const cases = [...kept];
  for (const group of readGroups(snippetsFile)) {
    for (let i = 0; i < group.snippets.length; i += PER_CASE) {
      const part = group.snippets.slice(i, i + PER_CASE);
      const js = `(() => { const $out = []; ${SHOW.replace(/\n\s*/g, ' ')} ` +
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
    if (c.name.startsWith('limit')) continue;
    const group = c.name.replace(/-\d+$/, '');
    if (SKIPPED_GROUPS.includes(group)) continue;
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
    path.join(outDir, 'bigint_cases.js'),
    '// Made by gen-bigint-cases.js from tools/probes/js-bigint.json.\n' +
      SHOW.replace(/^const /, 'var ') + '\n' +
      'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
      'var $out = { push: print };\n' + RUN + '\n',
  );
  fs.writeFileSync(path.join(outDir, 'bigint_cases.expected'), expected.join('\n') + '\n');
  console.log(`bigint_cases: ${snippets.length} snippets`);
}

const [mode, a, b] = process.argv.slice(2);
if (mode === 'probe') probe(a, b);
else if (mode === 'cases') cases(a, b);
else throw new Error('usage: gen-bigint-cases.js probe|cases IN OUT');
