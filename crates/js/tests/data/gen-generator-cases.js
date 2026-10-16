// Writes generators_cases.js (a script for swb-js) and
// generators_cases.expected (what it prints) from the output of the
// Chromium probe:
//
//   just probe --json tools/probes/js-generators.json > probe.json
//   node crates/js/tests/data/gen-generator-cases.js probe.json crates/js/tests/data
//
// Each line of the probe output is "SNIPPET => ok VALUE" or
// "SNIPPET => err NAME: MESSAGE", made by the `show` function of the
// probe file; the script evaluates each snippet with an indirect eval and
// prints the same line, so the expected output is Chromium 148's.
// Snippets whose result swb gives differently on purpose are left out
// (see EXCLUDED).

const fs = require('fs');
const path = require('path');

const [input, outDir] = process.argv.slice(2);
const probe = JSON.parse(fs.readFileSync(input, 'utf8'));

// Left out:
// - V8 words the error of a `throw` or `return` method that is not
//   callable as "yield* (intermediate value)(intermediate value) is not
//   iterable" for some call operands of `yield*` and as "yield*
//   (intermediate value) is not iterable" for others (`f()`); swb uses the
//   second form for every operand that is not a simple expression.
// - A recursion of 5,000 generators through `yield*`: Chromium 148 throws
//   `RangeError: Maximum call stack size exceeded`, swb runs it (its frame
//   limit is 10,000 frames; 10,000 levels end with the same RangeError).
const EXCLUDED = [
  (snippet, result) => result.includes('(intermediate value)(intermediate value)'),
  (snippet) => snippet.includes('g(5000)'),
];

const lines = [];
for (const c of probe.cases) {
  for (const r of c.results) {
    if (r.kind === 'js' && Array.isArray(r.value)) lines.push(...r.value);
  }
}
// The `show` function of the probe file.
const definitions = JSON.parse(
  fs.readFileSync(path.join(__dirname, '../../../../tools/probes/js-generators.json'), 'utf8'),
);
const js = definitions.cases[0].measure[0].js;
const show = js.slice(js.indexOf('const show = '), js.indexOf(' for (const s of'));

// Snippets with a loop or a recursion of thousands of steps run only
// without the GC stress mode (generators_cases_heavy.*).
const HEAVY = /i < \d{4,}|g\(\d{4,}\)/;
const groups = { generators_cases: [[], []], generators_cases_heavy: [[], []] };
for (const line of lines) {
  const m = /^(.*) => (ok|err) (.*)$/s.exec(line);
  if (!m) throw new Error('bad line: ' + line);
  if (EXCLUDED.some((excluded) => excluded(m[1], m[3]))) continue;
  const [snippets, expected] = groups[HEAVY.test(m[1]) ? 'generators_cases_heavy' : 'generators_cases'];
  snippets.push(m[1]);
  expected.push(line);
}

for (const [name, [snippets, expected]] of Object.entries(groups)) {
  fs.writeFileSync(
    path.join(outDir, name + '.js'),
    '// Made by gen-generator-cases.js from tools/probes/js-generators.json.\n' +
      show.replace(/^const /, 'var ') + '\n' +
      'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
      'for (var s of SNIPPETS) { var r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } print(s + " => " + r); }\n',
  );
  fs.writeFileSync(path.join(outDir, name + '.expected'), expected.join('\n') + '\n');
  console.log(`${name}: ${snippets.length} snippets`);
}
