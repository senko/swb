// Makes the snippet cases of the Chromium probe file of `Intl` (M9 feature
// 3) from a list of snippets and, from the probe output, the script and the
// expected output of `crates/js/tests/builtins_intl.rs`:
//
//   node crates/js/tests/data/gen-intl-cases.js probe \
//     crates/js/tests/data/intl-snippets.txt tools/probes/js-intl.json
//   just probe --json tools/probes/js-intl.json > probe.json
//   node crates/js/tests/data/gen-intl-cases.js cases probe.json \
//     crates/js/tests/data/intl-snippets.txt crates/js/tests/data
//
// `cases` writes `intl_cases.js` (the snippets once, and for each zone the
// indices of its snippets) and `intl_cases_ZONE.expected` for each zone.
// A third mode, `show`, prints the lines of a probe output (for a scratch
// snippet file): `node gen-intl-cases.js show probe.json`.
//
// The snippet file has one snippet per line; a line that starts with `#`
// is a comment, and a line `## NAME` starts a group. A group runs in
// every zone of ZONES; `## NAME @ Zone/A, Zone/B` runs it in those zones
// only. A snippet line that starts with `!known ` (swb differs from
// Chromium on purpose, ADR 0027 section 10) goes to the probe file but not
// to the script of swb-js. The probe evaluates each snippet with an
// indirect eval and prints "SNIPPET => ok VALUE" or "SNIPPET => err NAME:
// MESSAGE" (`show` below); the script for swb-js does the same in the zone
// that the test sets, so the expected output is Chromium 148's. No snippet
// depends on the current time.

const fs = require('fs');
const path = require('path');

const PER_CASE = 60;

// UTC, a northern zone with summer time, the east coast of America, a
// zone with a half-hour offset and no summer time, a southern zone with a
// half-hour standard offset, and a zone with a 30-minute change.
const ZONES = [
  'UTC',
  'Europe/Berlin',
  'America/New_York',
  'Asia/Kolkata',
  'Australia/Adelaide',
  'Australia/Lord_Howe',
];

const SHOW = `const show = v => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'bigint') return String(v) + 'n';
  if (typeof v === 'function') return 'function';
  if (typeof v === 'number' && Object.is(v, -0)) return '-0';
  if (v instanceof Date) return 'Date(' + v.getTime() + ')';
  if (Array.isArray(v)) return '[' + v.map(x => typeof x === 'object' && x !== null ? (x instanceof Date ? show(x) : show(x)) : show(x)).join(',') + ']';
  if (v && typeof v === 'object') return '{' + Object.keys(v).map(k => k + ':' + show(v[k])).join(',') + '}';
  return String(v);
};`;

// The output list is `$out`: a snippet may declare `out` (in swb-js the
// snippets share the global scope with the runner).
const RUN = 'for (const s of SNIPPETS) { let r; try { r = "ok " + show((0, eval)(s)); } ' +
  'catch (e) { r = "err " + (e && e.constructor && e.constructor.name) + ": " + (e && e.message); } ' +
  '$out.push(s + " => " + r); }';

const slug = zone => zone.replace(/[^A-Za-z0-9]+/g, '_').toLowerCase();

function readGroups(file) {
  const groups = [];
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    if (line.startsWith('## ')) {
      const [name, zones] = line.slice(3).split('@').map(s => s.trim());
      groups.push({
        name,
        zones: zones ? zones.split(',').map(s => s.trim()) : ZONES,
        snippets: [],
      });
    } else if (line.trim() !== '' && !line.startsWith('#')) {
      if (groups.length === 0) throw new Error('a snippet before the first group');
      groups[groups.length - 1].snippets.push(line);
    }
  }
  return groups;
}

// The text of a snippet line and its mark ('known' or '').
function parseSnippet(line) {
  const m = /^!known (.*)$/s.exec(line);
  return m ? { mark: 'known', text: m[1] } : { mark: '', text: line };
}

function probe(snippetsFile, out) {
  const cases = [];
  for (const group of readGroups(snippetsFile)) {
    const texts = group.snippets.map(s => parseSnippet(s).text);
    for (const zone of group.zones) {
      for (let i = 0; i < texts.length; i += PER_CASE) {
        const part = texts.slice(i, i + PER_CASE);
        const js = `(() => { const $out = []; ${SHOW.replace(/\n\s*/g, ' ')} ` +
          `const SNIPPETS = ${JSON.stringify(part)}; ${RUN} return $out; })()`;
        cases.push({
          name: `${group.name}-${slug(zone)}-${i / PER_CASE}`,
          html: '<!DOCTYPE html><p>x</p>',
          time_zone: zone,
          measure: [{ js }],
        });
      }
    }
  }
  fs.writeFileSync(out, JSON.stringify({ viewport: [800, 600], cases }, null, 1) + '\n');
  console.log(`${cases.length} probe cases`);
}

function marks(snippetsFile) {
  const result = new Map();
  for (const group of readGroups(snippetsFile)) {
    for (const s of group.snippets) {
      const { mark, text } = parseSnippet(s);
      if (mark) result.set(text, mark);
    }
  }
  return result;
}

function zoneOf(name) {
  const zone = ZONES.find(z => name.includes('-' + slug(z) + '-'));
  if (!zone) throw new Error('no zone in ' + name);
  return zone;
}

function lines(c) {
  const out = [];
  for (const r of c.results) {
    if (r.kind !== 'js' || !Array.isArray(r.value)) throw new Error('bad result in ' + c.name);
    out.push(...r.value);
  }
  return out;
}

function cases(input, snippetsFile, outDir) {
  const result = JSON.parse(fs.readFileSync(input, 'utf8'));
  const skip = marks(snippetsFile);
  const byZone = {};
  for (const zone of ZONES) byZone[zone] = { snippets: [], expected: [] };
  let nKnown = 0;
  for (const c of result.cases) {
    const zone = zoneOf(c.name);
    for (const line of lines(c)) {
      const m = /^(.*?) => (ok|err) (.*)$/s.exec(line);
      if (!m) throw new Error('bad line: ' + line);
      if (skip.get(m[1]) === 'known') { nKnown++; continue; }
      byZone[zone].snippets.push(m[1]);
      byZone[zone].expected.push(line);
    }
  }
  const all = [];
  const index = new Map();
  const lists = {};
  for (const zone of ZONES) {
    lists[zone] = byZone[zone].snippets.map((text) => {
      if (!index.has(text)) { index.set(text, all.length); all.push(text); }
      return index.get(text);
    });
  }
  fs.writeFileSync(
    path.join(outDir, 'intl_cases.js'),
    '// Made by gen-intl-cases.js from tools/probes/js-intl.json.\n' +
      SHOW.replace(/^const /, 'var ') + '\n' +
      'var ALL = ' + JSON.stringify(all, null, 0).replace(/","/g, '",\n"') + ';\n' +
      'var INDEX = ' + JSON.stringify(lists, null, 0) + ';\n' +
      'var SNIPPETS = INDEX[ZONE].map(function (i) { return ALL[i]; });\n' +
      'var $out = { push: print };\n' + RUN + '\n',
  );
  let total = 0;
  for (const zone of ZONES) {
    fs.writeFileSync(path.join(outDir, `intl_cases_${slug(zone)}.expected`),
      byZone[zone].expected.join('\n') + '\n');
    total += byZone[zone].snippets.length;
  }
  console.log(`intl_cases: ${total} snippets in ${ZONES.length} zones (${nKnown} known left out)`);
}

function showLines(input) {
  const result = JSON.parse(fs.readFileSync(input, 'utf8'));
  for (const c of result.cases) {
    console.log('# ' + c.name);
    for (const line of lines(c)) console.log(line);
  }
}

const [mode, a, b, c] = process.argv.slice(2);
if (mode === 'probe') probe(a, b);
else if (mode === 'cases') cases(a, b, c);
else if (mode === 'show') showLines(a);
else throw new Error('usage: gen-intl-cases.js probe SNIPPETS OUT | cases PROBE SNIPPETS DIR | show PROBE');
