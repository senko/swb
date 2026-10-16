// Writes collections_cases.js (a script for swb-js) and collections_cases.expected
// (what it prints) from the output of the Chromium probe:
//
//   just probe --json tools/probes/js-collections.json > probe.json
//   node crates/js/tests/data/gen-collection-cases.js probe.json crates/js/tests/data
//
// Each line of the probe output is "SNIPPET => ok VALUE" or
// "SNIPPET => err NAME: MESSAGE", made by the `show` function of the
// probe file; the script evaluates each snippet with an indirect eval and
// prints the same line, so the expected output is Chromium 148's.
// Snippets that need features that swb does not have yet, or whose
// result the specification leaves to the implementation, are left out
// (see EXCLUDED).

const fs = require('fs');
const path = require('path');

const [input, outDir] = process.argv.slice(2);
const probe = JSON.parse(fs.readFileSync(input, 'utf8'));

// Snippets with these parts need Proxy or WeakMap (feature 9) or BigInt
// (10), or show a known difference (see docs/roadmap.md, M7 6b).
const EXCLUDED = ['Proxy', 'WeakMap', '1n', 's.add(x + 10); return true', 's.add(x + 100); return false'];

const lines = [];
let show = null;
for (const c of probe.cases) {
  for (const r of c.results) {
    if (r.kind === 'js' && Array.isArray(r.value)) lines.push(...r.value);
  }
}
// The `show` function of the probe file.
const definitions = JSON.parse(
  fs.readFileSync(path.join(__dirname, '../../../../tools/probes/js-collections.json'), 'utf8'),
);
const js = definitions.cases[0].measure[0].js;
show = js.slice(js.indexOf('const show = '), js.indexOf(' for (const s of'));

// Snippets with a loop of thousands of steps (deep nesting) run only
// without the GC stress mode (collections_cases_heavy.*).
const HEAVY = /i < \d{4,}/;
const groups = { collections_cases: [[], []], collections_cases_heavy: [[], []] };
for (const line of lines) {
  const m = /^(.*) => (ok|err) (.*)$/s.exec(line);
  if (!m) throw new Error('bad line: ' + line);
  if (EXCLUDED.some((part) => m[1].includes(part))) continue;
  const [snippets, expected] = groups[HEAVY.test(m[1]) ? 'collections_cases_heavy' : 'collections_cases'];
  snippets.push(m[1]);
  expected.push(line);
}

for (const [name, [snippets, expected]] of Object.entries(groups)) {
  fs.writeFileSync(
    path.join(outDir, name + '.js'),
    '// Made by gen-collection-cases.js from tools/probes/js-collections.json.\n' +
      show.replace(/^const /, 'var ') + '\n' +
      'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
      'for (var s of SNIPPETS) { var r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } print(s + " => " + r); }\n',
  );
  fs.writeFileSync(path.join(outDir, name + '.expected'), expected.join('\n') + '\n');
  console.log(`${name}: ${snippets.length} snippets`);
}
