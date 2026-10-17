// Writes promises_cases.js (a script for swb-js) and
// promises_cases.expected (what it prints) from the output of the
// Chromium probe:
//
//   just probe --json tools/probes/js-promises.json > probe.json
//   node crates/js/tests/data/gen-promise-cases.js probe.json crates/js/tests/data
//
// Each line of the probe output is "SNIPPET => ok VALUE | LOG" or
// "SNIPPET => err NAME: MESSAGE | LOG", made by the runner of the probe
// file: it evaluates each snippet with an indirect eval, attaches handlers
// to a promise result, lets a fixed number of jobs run (a chain of `then`
// on a resolved promise, `ticks` long) and records the result and the
// lines that the snippet and its jobs pushed to `log`. The probe cannot
// wait for a timer (JavaScript is disabled in the probe's pages, so only
// the jobs of promises run), so the runner uses promises alone and works
// unchanged in swb-js, where the test harness runs the jobs after the
// script. The script keeps the runner of the probe file and prints the
// lines when the runner's promise is fulfilled, so the expected output is
// Chromium 148's. Snippets whose result swb gives differently on purpose
// are left out (see EXCLUDED).

const fs = require('fs');
const path = require('path');

const [input, outDir] = process.argv.slice(2);
const probe = JSON.parse(fs.readFileSync(input, 'utf8'));
const definitions = JSON.parse(
  fs.readFileSync(path.join(__dirname, '../../../../tools/probes/js-promises.json'), 'utf8'),
);

// Left out:
// - V8 names an ordinary object by the constructor that it finds on the
//   prototype chain in some messages (`Object.create(Promise.prototype)`
//   is `#<Promise>`); swb names it by its own `constructor` only
//   (`#<Object>`), as for the other built-ins (backlog).
const EXCLUDED = [(snippet) => snippet.includes('Object.create(Promise.prototype)')];

// The runner of the probe file: everything before the list of snippets.
const js = definitions.cases[0].measure[0].js;
const prelude = js.slice(0, js.indexOf(' const S = '));
const ending = ' return run(S); })()';
if (!js.endsWith(ending)) throw new Error('unexpected end of the runner');

const groups = { promises_cases: [[], []], promises_cases_heavy: [[], []] };
for (const c of probe.cases) {
  const group = c.name === 'promises-heavy' ? 'promises_cases_heavy' : 'promises_cases';
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
    '// Made by gen-promise-cases.js from tools/probes/js-promises.json.\n' +
      'void Promise.prototype.then.call(' + prelude + ' const S = ' + list + ';' + ending +
      ', function (out) { for (var line of out) print(line); });\n',
  );
  fs.writeFileSync(path.join(outDir, name + '.expected'), expected.join('\n') + '\n');
  console.log(`${name}: ${snippets.length} snippets`);
}
