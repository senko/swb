// Makes the Chromium probe file of the weak collection cases (M7 feature
// 9b) from a list of snippets and, from the probe output, the script and
// the expected output of `crates/js/tests/builtins_weak.rs`:
//
//   node crates/js/tests/data/gen-weak-cases.js probe \
//     crates/js/tests/data/weak-snippets.txt tools/probes/js-weak.json
//   just probe --json tools/probes/js-weak.json > probe.json
//   node crates/js/tests/data/gen-weak-cases.js cases probe.json crates/js/tests/data
//
// The snippet file has one snippet per line; a line that starts with `#`
// is a comment, and a line `## NAME` starts a group. Each group becomes
// one probe case (or several, of at most PER_CASE snippets). The probe
// evaluates each snippet with an indirect eval and prints "SNIPPET => ok
// VALUE" or "SNIPPET => err NAME: MESSAGE" (`show` below); the script for
// swb-js does the same, so the expected output is Chromium 148's. A group
// whose name ends in `heavy` goes to weak_cases_heavy.* (run without the
// GC stress mode). The snippets of KNOWN, where swb follows the
// specification and Chromium does not (ADR 0026 section 15), are left out
// of the script.

const fs = require('fs');
const path = require('path');

const PER_CASE = 60;

// Snippets whose Chromium 148 result differs from the specification; swb
// follows the specification (docs/adr/0026 section 15, M7 9b).
const KNOWN = [];

const SHOW = `const show = v => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol') return String(v);
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
  const outputs = { weak_cases: [[], []], weak_cases_heavy: [[], []] };
  for (const c of result.cases) {
    const heavy = /heavy-\d+$/.test(c.name);
    const [snippets, expected] = outputs[heavy ? 'weak_cases_heavy' : 'weak_cases'];
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
  for (const [name, [snippets, expected]] of Object.entries(outputs)) {
    fs.writeFileSync(
      path.join(outDir, name + '.js'),
      '// Made by gen-weak-cases.js from tools/probes/js-weak.json.\n' +
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
else throw new Error('usage: gen-weak-cases.js probe|cases IN OUT');
