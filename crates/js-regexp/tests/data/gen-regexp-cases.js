// Makes the Chromium probe file of the regular expression cases and, from
// its output, the data file of `crates/js-regexp/tests/cases.rs` (M7
// feature 8a):
//
//   node crates/js-regexp/tests/data/gen-regexp-cases.js probe \
//     crates/js-regexp/tests/data/regexp-cases.js tools/probes/js-regexp.json
//   just probe --json tools/probes/js-regexp.json > probe.json
//   node crates/js-regexp/tests/data/gen-regexp-cases.js cases probe.json \
//     crates/js-regexp/tests/data/regexp_cases.json
//
// `regexp-cases.js` has three lists (see there). The probe runs each list
// in its own case and returns the results as a JSON text. The data file
// has one entry per case with Chromium 148's result. Strings that are not
// well-formed UTF-16 (lone surrogates) are written as arrays of code units,
// all others as JSON strings.

const fs = require('fs');
const path = require('path');

const PER_CASE = 100;

const EXEC_RUNNER = `(cases) => cases.map(([p, f, s, li]) => {
  try {
    const r = new RegExp(p, f.includes('d') ? f : f + 'd');
    if (li !== undefined) r.lastIndex = li;
    const m = r.exec(s);
    if (m === null) return { match: null, lastIndex: r.lastIndex };
    return {
      index: m.index,
      captures: Array.from(m.indices, (x) => (x === undefined ? null : [x[0], x[1]])),
      groups: m.groups === undefined ? null : Object.keys(m.groups),
      lastIndex: r.lastIndex,
    };
  } catch (e) {
    return { error: e.name + ': ' + e.message };
  }
})`;

const SYNTAX_RUNNER = `(cases) => cases.map(([p, f]) => {
  try { new RegExp(p, f); return 'ok'; } catch (e) { return e.name + ': ' + e.message; }
})`;

const LITERAL_RUNNER = `(cases) => cases.map((src) => {
  try { new Function(src); return 'ok'; } catch (e) { return e.name + ': ' + e.message; }
})`;

const RUNNERS = { exec: EXEC_RUNNER, syntax: SYNTAX_RUNNER, literals: LITERAL_RUNNER };

function probe(casesFile, out) {
  const lists = require(path.resolve(casesFile));
  const cases = [];
  for (const kind of Object.keys(RUNNERS)) {
    const list = lists[kind];
    for (let i = 0; i < list.length; i += PER_CASE) {
      const part = list.slice(i, i + PER_CASE);
      const js = `JSON.stringify((${RUNNERS[kind]})(${JSON.stringify(part)}))`;
      cases.push({
        name: `${kind}-${i / PER_CASE}`,
        html: '<!DOCTYPE html><p>x</p>',
        measure: [{ js }],
      });
    }
  }
  fs.writeFileSync(out, JSON.stringify({ viewport: [800, 600], cases }, null, 1) + '\n');
}

function isWellFormed(s) {
  return typeof s !== 'string' || s.isWellFormed();
}

function encode(s) {
  if (isWellFormed(s)) return s;
  return Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));
}

// Encodes the strings of a result (messages can quote a pattern with lone
// surrogates).
function encodeDeep(v) {
  if (typeof v === 'string') return encode(v);
  if (Array.isArray(v)) return v.map(encodeDeep);
  if (v && typeof v === 'object') {
    return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, encodeDeep(x)]));
  }
  return v;
}

function cases(probeFile, out) {
  const text = fs.readFileSync(probeFile, 'utf8');
  const output = JSON.parse(text.slice(text.indexOf('{')));
  const lists = require(path.join(__dirname, 'regexp-cases.js'));
  const results = { exec: [], syntax: [], literals: [] };
  for (const c of output.cases) {
    const kind = c.name.slice(0, c.name.lastIndexOf('-'));
    const values = JSON.parse(c.results[0].value);
    results[kind].push(...values);
  }
  const data = { exec: [], syntax: [], literals: [] };
  lists.exec.forEach(([p, f, s, li], i) => {
    const entry = { pattern: encode(p), flags: f, input: encode(s), result: encodeDeep(results.exec[i]) };
    if (li !== undefined) entry.lastIndex = li;
    data.exec.push(entry);
  });
  lists.syntax.forEach(([p, f], i) => {
    data.syntax.push({ pattern: encode(p), flags: f, result: encodeDeep(results.syntax[i]) });
  });
  lists.literals.forEach((src, i) => {
    data.literals.push({ source: encode(src), result: encodeDeep(results.literals[i]) });
  });
  for (const kind of Object.keys(data)) {
    if (data[kind].some((e) => e.result === undefined)) {
      throw new Error(`the probe output has fewer ${kind} results than the case list`);
    }
  }
  const lines = [];
  for (const kind of Object.keys(data)) {
    lines.push(`"${kind}": [\n` + data[kind].map((e) => JSON.stringify(e)).join(',\n') + '\n]');
  }
  fs.writeFileSync(out, '{\n' + lines.join(',\n') + '\n}\n');
}

const [mode, a, b] = process.argv.slice(2);
if (mode === 'probe') {
  probe(a, b);
} else if (mode === 'cases') {
  cases(a, b);
} else {
  console.error('usage: gen-regexp-cases.js probe CASES OUT | cases PROBE_OUTPUT OUT');
  process.exit(64);
}
