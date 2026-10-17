// Makes the Chromium probe file of the `RegExp` built-in cases (M7 feature
// 8b) from a list of snippets and, from the probe output, the script and
// the expected output of `crates/js/tests/builtins_regexp.rs`:
//
//   node crates/js/tests/data/gen-regexp-builtin-cases.js probe \
//     crates/js/tests/data/regexp-builtin-snippets.txt tools/probes/js-regexp-builtin.json
//   just probe --json tools/probes/js-regexp-builtin.json > probe.json
//   node crates/js/tests/data/gen-regexp-builtin-cases.js cases probe.json crates/js/tests/data
//
// The snippet file has one snippet per line; a line that starts with `#`
// is a comment, and a line `## NAME` starts a group. Each group becomes
// one probe case (or several, of at most PER_CASE snippets). The probe
// evaluates each snippet with an indirect eval and prints
// "SNIPPET => ok VALUE" or "SNIPPET => err NAME: MESSAGE" (`show` below);
// the script for swb-js does the same, so the expected output is
// Chromium 148's. A group whose name ends in `-heavy` goes to
// regexp_builtin_cases_heavy.* (run without the GC stress mode). Snippets
// that contain a part of EXCLUDED are left out of the script.

const fs = require('fs');
const path = require('path');

const PER_CASE = 60;

// Snippets with these parts need features that swb does not have yet
// (Proxy: feature 9; BigInt: 10; `\p{...}`: 8c) or show a deliberate
// difference (docs/adr/0026: section 15, M7 8b): V8's getter `flags`
// reads `sticky` before `unicodeSets`, and a RegExp String Iterator goes
// on after its `exec` threw (the specification's generator completes).
const EXCLUDED = [
  'Proxy', '1n', '\\p{', '\\P{', "'unicodeSets', 'sticky'].forEach",
  "if (n == 2) throw new Error('x')",
];

const SHOW = `const show = v => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'function') return 'function ' + v.name;
  if (typeof v === 'number' && Object.is(v, -0)) return '-0';
  if (v instanceof RegExp) return 're(' + v.source + ',' + v.flags + ',' + show(v.lastIndex) + ')';
  if (Array.isArray(v)) {
    if (v.length > 50) return 'Array(' + v.length + ')';
    const p = [];
    for (let i = 0; i < v.length; i++) p.push(i in v ? show(v[i]) : '_');
    const extra = Object.keys(v).filter((k) => !/^\\d+$/.test(k)).map((k) => k + ':' + show(v[k]));
    return '[' + p.join(',') + ']' + (extra.length ? '{' + extra.join(',') + '}' : '');
  }
  if (v && typeof v === 'object') {
    const p = [];
    for (const k of Object.keys(v)) p.push(k + ':' + show(v[k]));
    return (Object.getPrototypeOf(v) === null ? 'N' : '') + '{' + p.join(',') + '}';
  }
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
  console.log(`${cases.length} probe cases`);
}

function cases(input, outDir) {
  const result = JSON.parse(fs.readFileSync(input, 'utf8'));
  const outputs = { regexp_builtin_cases: [[], []], regexp_builtin_cases_heavy: [[], []] };
  for (const c of result.cases) {
    const heavy = /-heavy-\d+$/.test(c.name);
    const [snippets, expected] = outputs[heavy ? 'regexp_builtin_cases_heavy' : 'regexp_builtin_cases'];
    for (const r of c.results) {
      if (r.kind !== 'js' || !Array.isArray(r.value)) throw new Error('bad result in ' + c.name);
      for (const line of r.value) {
        const m = /^(.*?) => (ok|err) (.*)$/s.exec(line);
        if (!m) throw new Error('bad line: ' + line);
        if (EXCLUDED.some((part) => m[1].includes(part))) continue;
        snippets.push(m[1]);
        expected.push(line);
      }
    }
  }
  for (const [name, [snippets, expected]] of Object.entries(outputs)) {
    fs.writeFileSync(
      path.join(outDir, name + '.js'),
      '// Made by gen-regexp-builtin-cases.js from tools/probes/js-regexp-builtin.json.\n' +
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
else throw new Error('usage: gen-regexp-builtin-cases.js probe|cases IN OUT');
