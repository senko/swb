// Writes the scripts and the expected output of builtins_unicode.rs from
// the cases of tools/probes/js-unicode.json and the output of the
// Chromium probe:
//
//   just probe --json tools/probes/js-unicode.json > probe.json
//   node crates/js/tests/data/gen-unicode-cases.js probe.json crates/js/tests/data
//
// - unicode_cases.js, unicode_cases.expected: the snippets of the case
//   "snippets" with Chromium 148's results (the format of
//   gen-string-cases.js). Snippets with a locale argument are left out:
//   swb has no Intl and ignores it (ADR 0026 section 15).
// - unicode_case_all.js, unicode_case_all.expected: toLowerCase and
//   toUpperCase of every code point; one line "code point:lower:upper"
//   (hexadecimal; code points joined by dots) for each code point where
//   one differs from the code point.
// - unicode_normalize_all.js, unicode_normalize_all.digest: the same for
//   the four forms of normalize ("=" for an unchanged form). The 567 KB
//   of output are not kept; the digest is the number of lines and the
//   FNV-1a hash (64 bits) of the output with a line feed after each line.
// - unicode_collation.js: the 225 words of the case "collation-sorted"
//   in Chromium's order, and a count of the pairs that swb orders
//   differently.

const fs = require('fs');
const path = require('path');

const [input, outDir] = process.argv.slice(2);
const root = path.join(__dirname, '..', '..', '..', '..');
const probe = JSON.parse(fs.readFileSync(input, 'utf8'));
const definition = JSON.parse(fs.readFileSync(path.join(root, 'tools', 'probes', 'js-unicode.json'), 'utf8'));

const results = {};
for (const c of probe.cases) results[c.name] = c.results[0].value;
const code = {};
for (const c of definition.cases) code[c.name] = c.measure[0].js;

const prelude = fs.readFileSync(path.join(__dirname, 'strings_cases_prelude.js'), 'utf8');

// Snippets.
const EXCLUDED = ["'tr'"];
const snippets = [];
const expected = [];
for (const line of results.snippets) {
  const m = /^(.*) => (ok|err) (.*)$/s.exec(line);
  if (!m) throw new Error('bad line: ' + line);
  if (EXCLUDED.some((part) => m[1].includes(part))) continue;
  snippets.push(m[1]);
  expected.push(line);
}
fs.writeFileSync(
  path.join(outDir, 'unicode_cases.js'),
  prelude + 'var SNIPPETS = ' + JSON.stringify(snippets, null, 0).replace(/","/g, '",\n"') + ';\n' +
    'for (var s of SNIPPETS) { var r; try { r = "ok " + show((0, eval)(s)); } catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } print(s + " => " + r); }\n',
);
fs.writeFileSync(path.join(outDir, 'unicode_cases.expected'), expected.join('\n') + '\n');
console.log(`${snippets.length} snippets`);

// Every code point.
const lines = (name) => results[name].split(' ');
fs.writeFileSync(path.join(outDir, 'unicode_case_all.js'), `print(${code['case-all']}.split(' ').join('\\n'));\n`);
fs.writeFileSync(path.join(outDir, 'unicode_case_all.expected'), lines('case-all').join('\n') + '\n');
console.log(`${lines('case-all').length} code points with a case mapping`);

fs.writeFileSync(path.join(outDir, 'unicode_normalize_all.js'), `print(${code['normalize-all']}.split(' ').join('\\n'));\n`);
let hash = 0xcbf29ce484222325n;
const text = Buffer.from(lines('normalize-all').join('\n') + '\n', 'utf8');
for (const byte of text) {
  hash ^= BigInt(byte);
  hash = (hash * 0x100000001b3n) & 0xffffffffffffffffn;
}
fs.writeFileSync(
  path.join(outDir, 'unicode_normalize_all.digest'),
  `${lines('normalize-all').length} ${hash.toString(16).padStart(16, '0')}\n`,
);
console.log(`${lines('normalize-all').length} code points with a normalization`);

// Collation.
fs.writeFileSync(
  path.join(outDir, 'unicode_collation.js'),
  'var w = ' + JSON.stringify(results['collation-sorted']) + ';\n' +
    'var bad = 0, tied = 0, n = 0;\n' +
    'for (var i = 0; i < w.length; i++) for (var j = i + 1; j < w.length; j++) {\n' +
    '  n++; var c = w[i].localeCompare(w[j]); if (c > 0) bad++; else if (c === 0) tied++;\n' +
    '}\n' +
    'print(n, bad, tied);\n',
);
