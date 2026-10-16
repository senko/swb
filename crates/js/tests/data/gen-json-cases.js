// Makes the Chromium probe file of the JSON cases and, from its output,
// the test script and the expected output (M7 feature 5c):
//
//   node crates/js/tests/data/gen-json-cases.js probe \
//     crates/js/tests/data/json_snippets.txt tools/probes/js-json.json
//   just probe --json tools/probes/js-json.json > probe.json
//   node crates/js/tests/data/gen-json-cases.js cases probe.json crates/js/tests/data
//
// `json_snippets.txt` has one snippet per line (an expression or a few
// statements; blank lines and lines that start with `#` are skipped).
// The probe evaluates each snippet with an indirect eval and prints
// "SNIPPET => ok VALUE" or "SNIPPET => err NAME: MESSAGE", with the `show`
// function of `strings_cases_prelude.js` (it does not use JSON). The
// `cases` step writes `json_cases.js`, which prints the same lines in
// swb-js, and `json_cases.expected`, which has Chromium 148's lines.

const fs = require('fs');
const path = require('path');

const PER_CASE = 40;

// Snippets with these parts need features that swb does not have yet
// (BigInt: feature 10, Date, Proxy: feature 9) or show a known
// difference: V8 names the constructor in the message of a cycle from the
// function that made the object, swb from the `constructor` of the
// prototype chain (the two snippets with `Foo.prototype.constructor = null`
// and `Bar.prototype = Object.create(Foo.prototype)`).
const EXCLUDED = [
  '1n', 'BigInt', 'Date(', 'Proxy',
  'Foo.prototype.constructor = null', 'Bar.prototype = Object.create(Foo.prototype); var a',
];

// Snippets that nest 100,000 levels (a replacer or `toJSON` that makes a new
// object at each level, which ends with a `RangeError`): they go to
// `json_cases_heavy.js`, which the test runs without the GC stress mode
// (a collection at every safepoint of 100,000 levels takes hours).
const HEAVY = ['return {x: v}', 'return {self: a}', 'repeat(1e5)', 'repeat(3e5)', 'mk(5e4)'];
const prelude = fs.readFileSync(path.join(__dirname, 'strings_cases_prelude.js'), 'utf8');

function runner(snippetsExpr) {
  return `for (var s of ${snippetsExpr}) { var r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } out.push(s + " => " + r); }`;
}

function readSnippets(file) {
  return fs
    .readFileSync(file, 'utf8')
    .split('\n')
    .filter((line) => line.trim() !== '' && !line.startsWith('#'));
}

const [mode, a, b] = process.argv.slice(2);
if (mode === 'probe') {
  const snippets = readSnippets(a);
  const cases = [];
  for (let i = 0; i < snippets.length; i += PER_CASE) {
    const part = snippets.slice(i, i + PER_CASE);
    const js = `(() => { var out = []; ${prelude.replace(/^\/\/.*$/gm, '')}\n${runner(JSON.stringify(part))} return out; })()`;
    cases.push({
      name: `json-${cases.length}`,
      html: '<!DOCTYPE html><p>x</p>',
      measure: [{ js }],
    });
  }
  fs.writeFileSync(b, JSON.stringify({ viewport: [800, 600], cases }, null, 1) + '\n');
  console.log(`${snippets.length} snippets in ${cases.length} cases`);
} else if (mode === 'cases') {
  const probe = JSON.parse(fs.readFileSync(a, 'utf8'));
  const lines = [];
  for (const c of probe.cases) {
    for (const r of c.results) {
      if (r.kind === 'js' && Array.isArray(r.value)) lines.push(...r.value);
    }
  }
  for (const [suffix, heavy] of [['', false], ['_heavy', true]]) {
    const snippets = [];
    const expected = [];
    for (const line of lines) {
      const m = /^(.*) => (ok|err) (.*)$/s.exec(line);
      if (!m) throw new Error('bad line: ' + line);
      if (EXCLUDED.some((part) => m[1].includes(part))) continue;
      if (HEAVY.some((part) => m[1].includes(part)) !== heavy) continue;
      snippets.push(m[1]);
      expected.push(line);
    }
    fs.writeFileSync(
      path.join(b, `json_cases${suffix}.js`),
      prelude + 'var out = [];\nvar SNIPPETS = ' + JSON.stringify(snippets).replace(/","/g, '",\n"') + ';\n' +
        runner('SNIPPETS').replace(/out\.push/, 'print') + '\n',
    );
    fs.writeFileSync(path.join(b, `json_cases${suffix}.expected`), expected.join('\n') + '\n');
    console.log(`${snippets.length} snippets${suffix}`);
  }
} else {
  throw new Error('usage: gen-json-cases.js probe|cases ...');
}
