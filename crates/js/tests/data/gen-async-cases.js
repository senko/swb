// Writes async_cases.js (a script for swb-js) and async_cases.expected
// (what it prints) from the output of the Chromium probe:
//
//   just probe --json tools/probes/js-async.json > probe.json
//   node crates/js/tests/data/gen-async-cases.js probe.json crates/js/tests/data
//
// The probe file uses the runner of tools/probes/js-promises.json (see
// gen-promise-cases.js): each snippet is evaluated with an indirect eval,
// a promise result gets handlers, a fixed number of jobs runs (a chain of
// `then` on a resolved promise, `ticks` long) and each line records the
// result and the lines that the snippet and its jobs pushed to `log`. The
// script keeps the runner of the probe file and prints the lines when the
// runner's promise is fulfilled, so the expected output is Chromium 148's.
// The cases of `async-heavy` (deep recursion, long chains) go to
// async_cases_heavy.*, which run without the stress mode. Snippets whose
// result swb gives differently on purpose are left out (see EXCLUDED).

const fs = require('fs');
const path = require('path');

const [input, outDir] = process.argv.slice(2);
const probe = JSON.parse(fs.readFileSync(input, 'utf8'));
const definitions = JSON.parse(
  fs.readFileSync(path.join(__dirname, '../../../../tools/probes/js-async.json'), 'utf8'),
);

// Left out:
// - V8 follows the elements of `Promise.all` in async stack traces (a line
//   `at async Promise.all (index 0)` and the function that awaits the
//   result); swb shows no lines for built-in functions and follows only
//   `await` (backlog).
const EXCLUDED = [(snippet) => snippet.includes('await Promise.all([s_aa()])')];

// The runner of the probe file: everything before the list of snippets.
const js = definitions.cases[0].measure[0].js;
const prelude = js.slice(0, js.indexOf(' const S = '));
const ending = ' return run(S); })()';
if (!js.endsWith(ending)) throw new Error('unexpected end of the runner');

const groups = { async_cases: [[], []], async_cases_heavy: [[], []] };
for (const c of probe.cases) {
  const group = c.name === 'async-heavy' ? 'async_cases_heavy' : 'async_cases';
  for (const r of c.results) {
    if (r.kind !== 'js' || !Array.isArray(r.value)) throw new Error('no lines in case ' + c.name);
    for (const line of r.value) {
      const m = /^(.*?) => (ok|err) /s.exec(line);
      if (!m) throw new Error('bad line: ' + line);
      if (EXCLUDED.some((excluded) => excluded(m[1], line))) continue;
      groups[group][0].push(m[1]);
      groups[group][1].push(line);
    }
  }
}

for (const [name, [snippets, expected]] of Object.entries(groups)) {
  const list = JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"');
  fs.writeFileSync(
    path.join(outDir, name + '.js'),
    '// Made by gen-async-cases.js from tools/probes/js-async.json.\n' +
      'void Promise.prototype.then.call(' + prelude + ' const S = ' + list + ';' + ending +
      ', function (out) { for (var line of out) print(line); });\n',
  );
  fs.writeFileSync(path.join(outDir, name + '.expected'), expected.join('\n') + '\n');
  console.log(`${name}: ${snippets.length} snippets`);
}
