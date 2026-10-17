// Writes strings_cases.js (a script for swb-js) and strings_cases.expected
// (what it prints) from the output of the Chromium probe:
//
//   just probe --json tools/probes/js-strings.json > probe.json
//   node crates/js/tests/data/gen-string-cases.js probe.json crates/js/tests/data
//
// Each line of the probe output is "SNIPPET => ok VALUE" or
// "SNIPPET => err NAME: MESSAGE", made by the `show` function of the
// probe file; the script evaluates each snippet with an indirect eval and
// prints the same line, so the expected output is Chromium 148's.
// Snippets that need features that swb does not have yet are left out
// (see EXCLUDED).

const fs = require('fs');
const path = require('path');

const [input, outDir] = process.argv.slice(2);
const probe = JSON.parse(fs.readFileSync(input, 'utf8'));

// Snippets with these parts need: BigInt (feature 10), JSON (5c), the
// methods of case mapping and normalize (5d: the list of keys of
// String.prototype), or a message for an Array Iterator receiver that names
// its tag (backlog).
const EXCLUDED = [
  '1n', 'JSON', 'getOwnPropertyNames', '.filter(', 'next.call([][Symbol.iterator]())',
];

const lines = [];
for (const c of probe.cases) {
  for (const r of c.results) {
    if (r.kind === 'js' && Array.isArray(r.value)) lines.push(...r.value);
  }
}

const snippets = [];
const expected = [];
for (const line of lines) {
  const m = /^(.*) => (ok|err) (.*)$/s.exec(line);
  if (!m) throw new Error('bad line: ' + line);
  if (EXCLUDED.some((part) => m[1].includes(part))) continue;
  snippets.push(m[1]);
  expected.push(line);
}

const prelude = fs.readFileSync(path.join(__dirname, 'strings_cases_prelude.js'), 'utf8');
fs.writeFileSync(
  path.join(outDir, 'strings_cases.js'),
  prelude + 'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
    'for (var s of SNIPPETS) { var r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } print(s + " => " + r); }\n',
);
fs.writeFileSync(path.join(outDir, 'strings_cases.expected'), expected.join('\n') + '\n');
console.log(`${snippets.length} snippets`);
