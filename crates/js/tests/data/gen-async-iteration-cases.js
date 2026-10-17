// Writes async_iteration_cases.js (a script for swb-js) and
// async_iteration_cases.expected (what it prints) from the output of the
// Chromium probe:
//
//   just probe --json tools/probes/js-async-iteration.json > probe.json
//   node crates/js/tests/data/gen-async-iteration-cases.js probe.json crates/js/tests/data
//
// The probe file uses the runner of tools/probes/js-async.json (see
// gen-async-cases.js): each snippet is evaluated with an indirect eval, a
// promise result gets handlers, a fixed number of jobs runs (a chain of
// `then` on a resolved promise, `ticks` long) and each line records the
// result and the lines that the snippet and its jobs pushed to `log`. The
// script keeps the runner of the probe file and prints the lines when the
// runner's promise is fulfilled, so the expected output is Chromium 148's.
// The cases of `async-iteration-heavy` (long loops, deep `yield*`
// chains, long queues) go to async_iteration_cases_heavy.*, which run
// without the stress mode. Snippets whose result swb gives differently on
// purpose are left out (see EXCLUDED).

const fs = require('fs');
const path = require('path');

const [input, outDir] = process.argv.slice(2);
const probe = JSON.parse(fs.readFileSync(input, 'utf8'));
const definitions = JSON.parse(
  fs.readFileSync(path.join(__dirname, '../../../../tools/probes/js-async-iteration.json'), 'utf8'),
);

// Left out:
// - V8 resumes a `yield*` delegate with `next(reason)` when the value of
//   a return request is a promise that rejects; the specification
//   (AsyncGeneratorUnwrapYieldResumption) makes the rejection a throw
//   completion, which calls the delegate's `throw` (swb).
// - V8's `Array.fromAsync` calls `return` of an Async-from-Sync Iterator a
//   second time after the wrapper closed the sync iterator for a rejected
//   value; the specification closes once (swb).
// - V8's `Array.fromAsync` also closes the iterator when `next` throws,
//   when its promise rejects and when the `value` getter throws; the
//   specification does not (swb).
// - V8 shows the frame of the built-in `next` (`at g.next`) in stack
//   traces; swb shows no lines for built-in functions.
// - V8 renders any operand of `for await` in its message (`{} is not
//   async iterable`, `3` for `1 + 2`); swb renders identifiers, literals,
//   member chains and calls, as for `for`-`of`.
// - Chromium's stack ends a chain of 5,000 delegating async generators
//   with a `RangeError`; swb's frame limit is 10,000 frames.
const EXCLUDED = [
  (snippet) => snippet.includes('.return(Promise.reject(new Error(\'rj\'))), \'r1\')') && snippet.includes('yield* LA('),
  (snippet) => snippet.startsWith('Array.fromAsync(LI([1, Promise.reject('),
  (snippet) => snippet.startsWith('Array.fromAsync({ [Symbol.asyncIterator]() { return { next() {') && snippet.includes("log.push('closed')"),
  (snippet) => snippet.includes('async function* s_g2()') || snippet.includes('async function* s_g6()'),
  (snippet) => snippet.includes('for await (var x of 1 + 2)') || snippet.includes('for await (var x of {})'),
  (snippet) => snippet.includes('hy5(5000)'),
];

// The runner of the probe file: everything before the list of snippets.
const js = definitions.cases[0].measure[0].js;
const prelude = js.slice(0, js.indexOf(' const S = '));
const ending = ' return run(S); })()';
if (!js.endsWith(ending)) throw new Error('unexpected end of the runner');

const groups = { async_iteration_cases: [[], []], async_iteration_cases_heavy: [[], []] };
for (const c of probe.cases) {
  const group = c.name === 'async-iteration-heavy' ? 'async_iteration_cases_heavy' : 'async_iteration_cases';
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
    '// Made by gen-async-iteration-cases.js from tools/probes/js-async-iteration.json.\n' +
      'void Promise.prototype.then.call(' + prelude + ' const S = ' + list + ';' + ending +
      ', function (out) { for (var line of out) print(line); });\n',
  );
  fs.writeFileSync(path.join(outDir, name + '.expected'), expected.join('\n') + '\n');
  console.log(`${name}: ${snippets.length} snippets`);
}
