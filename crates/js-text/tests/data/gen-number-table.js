'use strict';
// Writes number_formats.tsv, the test table of js-text's number formats:
// one case per line, `op<TAB>arg<TAB>bits<TAB>expected`, where bits is the
// double's bit pattern in hex. Ops: fixed, exponential (arg "u" for
// undefined) and precision from Node.js 22; radix from the BigInt
// reference in radix-reference.js (swb's definition, which differs from
// Node.js in the last digits of some values); locale from Chromium 148,
// read from the output of
// `just probe --json --case locale tools/probes/js-numbers.json`.
// Usage: node gen-number-table.js PROBE_OUTPUT.json number_formats.tsv
const fs = require('fs');
const path = require('path');
const { radixString } = require(path.join(__dirname, 'radix-reference.js'));
let seed = 20261015;
function rand() { // xorshift32, so that the table is reproducible
  seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0;
  return seed / 4294967296;
}
const buf = new DataView(new ArrayBuffer(8));
function bits(x) { buf.setFloat64(0, x); return buf.getBigUint64(0).toString(16).padStart(16, '0'); }
function randomBits() {
  buf.setUint32(0, Math.floor(rand() * 2 ** 32)); buf.setUint32(4, Math.floor(rand() * 2 ** 32));
  return buf.getFloat64(0);
}
function finite(x) { return Number.isFinite(x) ? x : 1.5; }
function sample(kind) {
  const r = rand();
  if (kind === 'small') { // around the fixed range
    if (r < 0.3) return finite((rand() - 0.5) * 10 ** Math.floor(rand() * 42 - 21));
    if (r < 0.5) return Math.floor(rand() * 2 ** 20) / 2 ** Math.floor(rand() * 20); // exact binary fractions, ties
    if (r < 0.6) return (Math.floor(rand() * 100000) + 0.5) / 10 ** Math.floor(rand() * 6); // decimal ties
    if (r < 0.7) return 10 ** Math.floor(rand() * 44 - 22);
    return finite(randomBits());
  }
  if (r < 0.5) return finite(randomBits());
  if (r < 0.6) return 2 ** Math.floor(rand() * 2098 - 1074);
  if (r < 0.7) return f64sub();
  return finite((rand() - 0.5) * 2 ** Math.floor(rand() * 140 - 60));
}
function f64sub() { buf.setUint32(0, Math.floor(rand() * 2 ** 20)); buf.setUint32(4, Math.floor(rand() * 2 ** 32)); return buf.getFloat64(0); }
const special = [0, -0, 5e-324, -5e-324, Number.MAX_VALUE, -Number.MAX_VALUE, 2.2250738585072014e-308, 1e21, 999999999999999900000, 1e-7, 0.5, 1.5, 2.5, -2.5, 1.005, 1.45, 0.000001, 123.456, 1 / 3, Math.PI, 2 ** 53, 2 ** 53 - 1, 9.5, 0.05, 1e-10, 1e300, 25, 125];
const lines = [];
function add(op, arg, x, out) { lines.push([op, arg, bits(x), out].join('\t')); }
// toFixed: digit counts 0 to 100.
for (const x of special) for (const f of [0, 1, 2, 5, 20, 100]) add('fixed', f, x, x.toFixed(f));
for (let i = 0; i < 700; i++) { const x = sample('small'); const f = i < 101 ? i : Math.floor(rand() * 101); add('fixed', f, x, x.toFixed(f)); }
// toExponential.
for (const x of special) for (const f of ['u', 0, 1, 3, 16, 100]) add('exponential', f, x, f === 'u' ? x.toExponential() : x.toExponential(f));
for (let i = 0; i < 700; i++) { const x = sample(i % 2 ? 'small' : 'any'); const f = i % 7 === 0 ? 'u' : (i < 101 ? i : Math.floor(rand() * 101)); add('exponential', f, x, f === 'u' ? x.toExponential() : x.toExponential(f)); }
// toPrecision.
for (const x of special) for (const p of [1, 2, 3, 7, 21, 22, 100]) add('precision', p, x, x.toPrecision(p));
for (let i = 0; i < 700; i++) { const x = sample(i % 2 ? 'small' : 'any'); const p = i < 100 ? i + 1 : 1 + Math.floor(rand() * 100); add('precision', p, x, x.toPrecision(p)); }
// toString(radix): all radixes but 10.
for (const x of special) for (const r of [2, 3, 7, 16, 36]) add('radix', r, x, radixString(x, r));
for (let i = 0; i < 1100; i++) {
  let r = 2 + Math.floor(rand() * 35); if (r === 10) r = 3;
  const x = i < 400 ? sample('any') : finite((rand() - 0.5) * 2 ** Math.floor(rand() * 120 - 50));
  add('radix', r, x, radixString(x, r));
}
for (let r = 2; r <= 36; r++) { if (r === 10) continue; for (const x of [0.1, 1 / 3, Number.MIN_VALUE, Number.MAX_VALUE, 2 ** 60 + 2 ** 10, 1e21 + 12345e5]) add('radix', r, x, radixString(x, r)); }
// toLocaleString, from Chromium.
// The probe writes NaN and Infinity as bare words (Python's json), so the
// output is read as a JavaScript expression.
const probe = (0, eval)('(' + fs.readFileSync(process.argv[2], 'utf8') + ')');
const locale = probe.cases.find((c) => c.name === 'locale');
for (const [s, negz, out] of locale.results[0].value) add('locale', '-', negz ? -0 : Number(s), out);
fs.writeFileSync(process.argv[3], lines.join('\n') + '\n');
console.log(lines.length, 'cases');
