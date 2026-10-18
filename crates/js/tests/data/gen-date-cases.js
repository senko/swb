// Makes the snippet cases of the Chromium probe file of `Date` (M7 feature
// 12a) from a list of snippets and, from the probe output, the script and
// the expected output of `crates/js/tests/builtins_date.rs`:
//
//   node crates/js/tests/data/gen-date-cases.js probe \
//     crates/js/tests/data/date-snippets.txt tools/probes/js-date.json
//   just probe --json tools/probes/js-date.json > probe.json
//   node crates/js/tests/data/gen-date-cases.js cases probe.json \
//     crates/js/tests/data/date-snippets.txt crates/js/tests/data
//
// `cases` writes `date_cases.js` (the snippets once, and for each zone the
// indices of its snippets) and `date_cases_ZONE.expected` for each zone.
//
// The snippet file has one snippet per line; a line that starts with `#`
// is a comment, and a line `## NAME` starts a group. A group runs in
// every zone of ZONES; `## NAME @ Zone/A, Zone/B` runs it in those zones
// only. A snippet line that starts with `!known ` (swb differs from
// Chromium on purpose, ADR 0026 section 15) or `!later ` (Chromium's
// forms of feature 12b) goes to the probe file but not to the script of
// swb-js. Each group becomes one probe case per zone (or several, of at
// most PER_CASE snippets); the probe file gives each case its `time_zone`.
// The probe evaluates each snippet with an indirect eval and prints
// "SNIPPET => ok VALUE" or "SNIPPET => err NAME: MESSAGE" (`show` below; a
// Date shows as `Date(milliseconds)`); the script for swb-js does the
// same in the zone that the test sets, so the expected output is Chromium
// 148's. No snippet depends on the current time.

const fs = require('fs');
const path = require('path');

const PER_CASE = 60;

// The zones of the cases: UTC, a northern zone with daylight saving time,
// the east coast of America, a zone with a half-hour offset and no
// daylight saving time, a southern zone with a half-hour standard offset,
// a zone whose daylight saving time is 30 minutes, and a zone that
// skipped a whole day (Apia, 2011-12-30).
const ZONES = [
  'UTC',
  'Europe/Berlin',
  'America/New_York',
  'Asia/Kolkata',
  'Australia/Adelaide',
  'Australia/Lord_Howe',
  'Pacific/Apia',
];

const SHOW = `const show = v => {
  if (typeof v === 'string') return JSON.stringify(v);
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'bigint') return String(v) + 'n';
  if (typeof v === 'function') return 'function';
  if (typeof v === 'number' && Object.is(v, -0)) return '-0';
  if (v instanceof Date) return 'Date(' + v.getTime() + ')';
  if (Array.isArray(v)) return '[' + v.map(x => typeof x === 'object' && x !== null ? (x instanceof Date ? show(x) : 'object') : show(x)).join(',') + ']';
  if (v && typeof v === 'object') return 'object';
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

// The text of a snippet line and its mark ('known', 'later' or '').
function parseSnippet(line) {
  const m = /^!(known|later) (.*)$/s.exec(line);
  return m ? { mark: m[1], text: m[2] } : { mark: '', text: line };
}

function probe(snippetsFile, out) {
  const cases = [];
  for (const group of readGroups(snippetsFile)) {
    const texts = group.snippets.map(s => parseSnippet(s).text);
    for (const zone of group.zones) {
      if (!ZONES.includes(zone)) throw new Error('unknown zone ' + zone);
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

// The marks of the snippet file by snippet text.
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

function cases(input, snippetsFile, outDir) {
  const result = JSON.parse(fs.readFileSync(input, 'utf8'));
  const skip = marks(snippetsFile);
  const byZone = {};
  for (const zone of ZONES) byZone[zone] = { snippets: [], expected: [] };
  let nLater = 0;
  let nKnown = 0;
  for (const c of result.cases) {
    const zone = ZONES.find(z => c.name.includes('-' + slug(z) + '-'));
    if (!zone) throw new Error('no zone in ' + c.name);
    for (const r of c.results) {
      if (r.kind !== 'js' || !Array.isArray(r.value)) throw new Error('bad result in ' + c.name);
      for (const line of r.value) {
        const m = /^(.*?) => (ok|err) (.*)$/s.exec(line);
        if (!m) throw new Error('bad line: ' + line);
        const mark = skip.get(m[1]);
        if (mark === 'later') { nLater++; continue; }
        if (mark === 'known') { nKnown++; continue; }
        byZone[zone].snippets.push(m[1]);
        byZone[zone].expected.push(line);
      }
    }
  }
  // One script for all zones: the snippets once, and for each zone the
  // indices of its snippets. The test defines `ZONE` and sets the zone of
  // the runtime; the expected file of each zone has its lines.
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
    path.join(outDir, 'date_cases.js'),
    '// Made by gen-date-cases.js from tools/probes/js-date.json.\n' +
      SHOW.replace(/^const /, 'var ') + '\n' +
      'var ALL = ' + JSON.stringify(all, null, 0).replace(/","/g, '",\n"') + ';\n' +
      'var INDEX = ' + JSON.stringify(lists, null, 0) + ';\n' +
      'var SNIPPETS = INDEX[ZONE].map(function (i) { return ALL[i]; });\n' +
      'var $out = { push: print };\n' + RUN + '\n',
  );
  let total = 0;
  for (const zone of ZONES) {
    fs.writeFileSync(path.join(outDir, `date_cases_${slug(zone)}.expected`),
      byZone[zone].expected.join('\n') + '\n');
    total += byZone[zone].snippets.length;
  }
  console.log(`date_cases: ${total} snippets in ${ZONES.length} zones ` +
    `(${nKnown} known, ${nLater} later left out)`);
}

const [mode, a, b, c] = process.argv.slice(2);
if (mode === 'probe') probe(a, b);
else if (mode === 'cases') cases(a, b, c);
else throw new Error('usage: gen-date-cases.js probe SNIPPETS OUT | cases PROBE SNIPPETS DIR');
