'use strict';
// Writes parse_int_radix.tsv, the test table of js-text's parseInt in the
// radixes that are not powers of two and not 10: one case per line,
// `radix<TAB>text<TAB>bits`, where bits is the bit pattern (hex) of the
// expected double: the exact integer of the leading digits rounded to
// nearest, ties to even (radix-reference.js: Number of a BigInt). Cases:
// random digit strings of 1 to 800 digits, leading zeros, a sign, a stop
// at a character that is not a digit, the ties at 2^53, and the
// boundary to infinity (2^1024 − 2^970 and one less, 2^1024, the digit
// counts around the cut-off of js-text).
// Usage: node gen-parse-int-table.js parse_int_radix.tsv
const fs = require('fs');
const path = require('path');
const { parseIntExact } = require(path.join(__dirname, 'radix-reference.js'));
let seed = 20261015;
function rand() { // xorshift32, so that the table is reproducible
  seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0;
  return seed / 4294967296;
}
const view = new DataView(new ArrayBuffer(8));
function bits(x) { view.setFloat64(0, x); return view.getBigUint64(0).toString(16).padStart(16, '0'); }
const radixes = [];
for (let r = 3; r <= 36; r++) if ((r & (r - 1)) !== 0 && r !== 10) radixes.push(r);
const digitChars = '0123456789abcdefghijklmnopqrstuvwxyz';
function randomDigits(radix, length) {
  let s = digitChars[1 + Math.floor(rand() * (radix - 1))];
  for (let i = 1; i < length; i++) {
    const c = digitChars[Math.floor(rand() * radix)];
    s += rand() < 0.3 ? c.toUpperCase() : c;
  }
  return s;
}
const lines = [];
function add(radix, text) {
  lines.push([radix, text, bits(parseIntExact(text, radix))].join('\t'));
}
// Random digit strings, lengths from 1 to 800 (more short ones).
for (let i = 0; i < 600; i++) {
  const radix = radixes[i % radixes.length];
  add(radix, randomDigits(radix, 1 + Math.floor(800 ** rand())));
}
for (const radix of radixes) {
  const b = BigInt(radix);
  // Leading zeros, a sign, white space and a stop.
  add(radix, '0'.repeat(400) + randomDigits(radix, 30));
  add(radix, '-' + '0'.repeat(100) + '1');
  add(radix, '0'.repeat(50));
  add(radix, '  +' + randomDigits(radix, 40) + '.5');
  add(radix, randomDigits(radix, 25) + (digitChars[radix] || '_') + '1');
  add(radix, randomDigits(radix, 800));
  // Ties at 2^53: 2^53 + 1 rounds down, 2^53 + 3 up.
  for (const n of [2n ** 53n + 1n, 2n ** 53n + 3n, 2n ** 54n + 2n, 2n ** 54n + 6n]) add(radix, n.toString(radix));
  // The boundary to infinity.
  const limit = 2n ** 1024n - 2n ** 970n;
  for (const n of [limit, limit - 1n, 2n ** 1024n, 2n ** 1024n - 1n, 2n ** 1023n * 3n / 2n]) add(radix, n.toString(radix));
  // The digit counts around js-text's cut-off: n digits with
  // radix^(n−1) < 2^1024 ≤ radix^n.
  let n = 0;
  while (b ** BigInt(n) < 2n ** 1024n) n++;
  add(radix, '1' + '0'.repeat(n - 2));
  add(radix, '1' + '0'.repeat(n - 1));
  add(radix, digitChars[radix - 1].repeat(n - 1));
  add(radix, digitChars[radix - 1].repeat(n));
  add(radix, '1' + '0'.repeat(n));
  add(radix, '00' + '1' + '0'.repeat(n - 1));
}
fs.writeFileSync(process.argv[2], lines.join('\n') + '\n');
console.log(lines.length, 'cases');
